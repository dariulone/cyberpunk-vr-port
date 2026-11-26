-- Contact offsets the virtual wrist servo target, never the raw XR sample.
local M={state={phase='idle',requests=0,binds=0,lastPeak=0}}
local function reason(text) M.state.exitReason=text end
local serial, weaponId, side, seat, sent, waiting
local function v(x,y,z,w) return Vector4.new(x,y,z,w or 0) end
local function sub(a,b) return v(a.x-b.x,a.y-b.y,a.z-b.z) end
local function add(a,b) return v(a.x+b.x,a.y+b.y,a.z+b.z,1) end
local function mul(a,k) return v(a.x*k,a.y*k,a.z*k) end
local function dot(a,b) return a.x*b.x+a.y*b.y+a.z*b.z end
local function len(a) return math.sqrt(dot(a,a)) end
local function worldMotion(delta,basis)
    return v(basis.x*delta.x+basis.y*delta.z,basis.y*delta.x-basis.x*delta.z,delta.y)
end
local function finite(a) return a and a.x==a.x and a.y==a.y and a.z==a.z and math.abs(a.x)+math.abs(a.y)+math.abs(a.z)<1e7 end
local function clearFollowAttempt()
    M.state.followAttempted=false;M.state.followGate=nil;M.state.followSerial=nil;M.state.followAttempt=nil
    M.state.followGeometry=nil;M.state.followWindow=nil;M.state.followCounts=nil
end
local function clear(phase)
    if seat then
        seat.weapon:VRBladeReleaseContact(seat.serial)
        VRBladeClearJoint(side)
    elseif waiting then
        waiting.weapon:VRBladeReleaseContact(waiting.serial)
    end
    if sent then VRBladePoseOffset(side,v(0,0,0));sent=false end
    seat=nil;waiting=nil;M.state.phase=phase;M.state.offset=0;M.state.depth=0;M.state.resistance=0;M.state.joint=false
    M.state.receiptDepth=0;M.state.withdrawal=0;M.state.holdRequested=false
    M.state.edgeRequested=false;M.state.edgeStroke=0;M.state.edgeTravel=0;M.state.edgeCommand=0;M.state.edgeRailStage=0
    M.state.slotFollowing=false;M.state.slotApplied=false;M.state.slotGap=nil;M.state.slotDepth=0;M.state.slotShape=nil;M.state.slotFraction=nil
    clearFollowAttempt()
end
function M.reset()
    clear('idle');serial=nil;weaponId=nil;side=nil
end
function M.update(dt,player,w,position,axis,tracked)
    M.state.followAttempted=false
    if not w.VRBladeConfirmedSerial or not VRBladePoseOffset then return end
    local id=tostring(w:GetEntityID().hash)
    local hand=Game.VRWeaponInLeftHand() and 0 or 1
    local n=w:VRBladeConfirmedSerial()
    if id~=weaponId or side~=hand then M.reset();weaponId=id;side=hand;serial=n;return end
    local fresh=n~=serial;serial=n
    if fresh then waiting=nil;clearFollowAttempt() end
    if fresh and w.vrBladeReceipt.released then reason('separation');clear('released');return end
    if not fresh and not seat and not waiting then return end
    if not finite(tracked) or tracked.w<=0 or (seat and tracked.w~=seat.trackAt.w)
        or not dt or dt<=0 or not finite(axis) or not finite(position) or not player:VRBladeAllowed(w)
        or Game.GetSystemRequestsHandler():IsGamePaused() then clear('unavailable');return end
    if dt>.12 then
        if not seat or fresh or not (w.vrBladeReceipt.puncture or w.vrBladeReceipt.lodged) then clear('unavailable');return end
        -- A late CET frame can expire the solver lease. Keep the confirmed
        -- entry and remeasure the target, discarding stale gesture history.
        seat.samples={};seat.edgeUntil=nil;seat.edgeTrack=nil;seat.edgeFrom=nil
    end
    local age=w:VRBladeConfirmedAge()
    if age<0 then reason('invalid age');clear('released');return end
    if (fresh or waiting) and age>.20 then reason('stale receipt');clear('released');return end
    local continuation=fresh and w:VRBladeConfirmedFollowThrough()
    if continuation and (not seat or not seat.dispatched or seat.jointSerial~=w.vrBladeReceipt.jointSerial) then
        reason('orphaned continuation');clear('released');return
    end
    local target=w:VRBladeConfirmedTarget()
    if not target or not target:IsAttached() then clear('target gone');return end
    local puncture=w.vrBladeReceipt.puncture or w.vrBladeReceipt.lodged
    local a=w:VRBladeAnchorWorld(puncture)
    local raw=VRHandRawWorld(hand)
    if not finite(a) or a.w<.5 or not finite(raw) or raw.w<.5 then clear('tracking lost');return end
    local start=position
    local reach=w:VRMeleeReach()
    local tip=add(start,mul(axis,reach))
    local measure=w:VRBladeMeasure(n,start,tip,v(0,0,0))
    local namedDepth=finite(measure) and measure.x or 0
    local depth=namedDepth
    local edge=w:GetWorldForward()
    if not finite(edge) then clear('no edge');return end
    if puncture and depth<=0 then
        local interval=w:VRBladeSeatInterval(n,start,tip,edge)
        if finite(interval) and interval.w>.5 then depth=interval.y end
    end
    local basis=VRBladeTrackingBasis()
    if not finite(basis) or basis.w<.5 then clear('tracking basis lost');return end
    local entryAxis=w.vrBladeReceipt.seatAxis
    if not finite(entryAxis) or len(entryAxis)<.9 then entryAxis=axis end
    local entryEdge=w.vrBladeReceipt.seatEdge
    if not finite(entryEdge) or len(entryEdge)<.9 then entryEdge=edge end
    if fresh or waiting then
        if not continuation then
            if fresh then clear('idle') end
            if waiting and (waiting.track.w~=tracked.w or
                depth<=0 and dot(worldMotion(sub(tracked,waiting.track),basis),entryAxis)<-.025) then
                reason('retracted');clear('released');return
            end
            if depth<=0 then
                if puncture then
                    waiting=waiting or {weapon=w,serial=n,track=v(tracked.x,tracked.y,tracked.z,tracked.w)}
                    M.state.phase='awaiting overlap';return
                end
                reason('outside contact');M.state.phase='outside contact';return
            end
            seat={weapon=w,trackAt=v(tracked.x,tracked.y,tracked.z,tracked.w),jointSerial=w.vrBladeReceipt.jointSerial,
                rawFromAnchor=sub(raw,a),lastTrack=waiting and waiting.track or tracked,axial=0,lastSlide=0,
                inputAxis=v(entryAxis.x,entryAxis.y,entryAxis.z),inputEdge=v(entryEdge.x,entryEdge.y,entryEdge.z),
                inputBasis=v(basis.x,basis.y,basis.z,1),
                backout=0,travelTime=0,slideUntil=.09}
            waiting=nil
            M.state.binds=M.state.binds+1;M.state.lastPeak=0
        end
        seat.serial=n;seat.samples={};seat.dispatched=false;seat.measureAt=nil
        seat.trailAt=nil;seat.loadAt=nil;seat.retryAt=nil
        reason(nil);M.state.phase='bound'
    end
    local d=sub(sub(raw,a),seat.rawFromAnchor)
    if puncture then w:VRBladeLodgeContact(n) end
    local trackedTravel=len(sub(tracked,seat.trackAt))
    local railPoint=VRBladeJointPoint(hand,seat.jointSerial)
    local rail=finite(railPoint) and railPoint.w>.5
    M.state.slotFollowing=false;M.state.slotApplied=false;M.state.slotGap=nil;M.state.slotDepth=0;M.state.slotShape=nil;M.state.slotFraction=nil
    if puncture then
        local goal=w:VRBladeFollowSlot(hand,n,start,tip,edge,tracked)
        if goal and goal.w<0 then reason('target moved away');clear('released');return end
        local pin=w.vrBladeReceipt.attachment
        if pin then
            M.state.slotDepth=pin.depth;M.state.slotShape=NameToString(pin.shape);M.state.slotFraction=pin.fraction
            if finite(goal) and goal.w>1.5 then
                local station=add(start,mul(axis,reach*pin.fraction))
                M.state.slotGap=len(sub(goal,station));M.state.slotFollowing=true;M.state.slotApplied=goal.w>2.5
            end
        end
    end
    local anchor=rail and railPoint or a
    local along=dot(sub(anchor,start),axis)
    local gap=len(sub(anchor,add(start,mul(axis,math.max(0,math.min(reach,along))))))
    local onRail=rail and along>=0 and along<=reach and gap<=.10
    seat.travelTime=seat.travelTime+dt
    if puncture then
        -- Accumulate small XR steps and reset the withdrawal distance on
        -- renewed insertion. Animation and solver movement never add to it.
        local delta=worldMotion(sub(tracked,seat.lastTrack),seat.inputBasis)
        local step=dot(delta,seat.inputAxis)
        seat.lastTrack=v(tracked.x,tracked.y,tracked.z)
        if math.abs(step)<.7*len(delta) then step=0 end
        seat.axial=seat.axial+step
        seat.backout=math.max(0,seat.backout-step)
        -- A deliberate pull keeps the shaft free while the solved prop catches
        -- up to the controller. Only signed reinsertion cancels this intent.
        if seat.pullMinimum then
            seat.pullMinimum=math.min(seat.pullMinimum,seat.axial)
            -- XR can reverse on the very frame the solved shaft first clears.
            -- Only cancel the pull if the shaft is still physically occupied.
            if depth>0 and seat.axial-seat.pullMinimum>=.025 then
                seat.pullMinimum=nil;seat.backout=0
            end
        elseif seat.backout>=.025 then
            seat.pullMinimum=seat.axial
        end
        if math.abs(seat.axial-seat.lastSlide)>.004 then
            seat.lastSlide=seat.axial
            seat.slideUntil=seat.travelTime+(step<0 and .09 or .025)
        end
        if not seat.pullMinimum and depth>0 and seat.travelTime>=seat.slideUntil then seat.backout=0 end
    end
    if depth<=0 and (not onRail or M.state.slotFollowing) and (not puncture or seat.pullMinimum) then
        local actual=worldMotion(sub(tracked,seat.trackAt),basis)
        reason((puncture or along>=reach and dot(actual,axis)<0) and 'retracted' or 'separation')
        M.state.exitTravel=len(d);M.state.exitGap=gap;M.state.exitWithdrawal=seat.backout
        clear('released');return
    end
    -- A visual sample uses the solved prop, not the raw wrist. Require a fresh XR
    -- displacement as well, so an idle controller cannot paint a passing target.
    if namedDepth>0 and trackedTravel>.015 and w.VRBladeTraceSurface and age>=(seat.trailAt or 0) then
        local last=seat.trailTrack or seat.trackAt
        if len(sub(tracked,last))>.006 then
            seat.trailAt=age+.028; seat.trailTrack=v(tracked.x,tracked.y,tracked.z,tracked.w)
            w:VRBladeTraceSurface(n,start,add(start,mul(axis,reach)))
        end
    end
    edge=sub(edge,mul(axis,dot(edge,axis)))
    local edgeLength=len(edge)
    if edgeLength<.01 then clear('no edge');return end
    edge=mul(edge,1/edgeLength)
    seat.depth=depth
    seat.resistance=namedDepth>0 and measure.w or ((depth>0 or puncture and rail) and 1 or 0)
    local holdDepth=puncture and not seat.pullMinimum and seat.travelTime>=seat.slideUntil
    if puncture and rail and not seat.pullMinimum then
        seat.edgeTrack=seat.edgeTrack or v(tracked.x,tracked.y,tracked.z)
        local delta=worldMotion(sub(tracked,seat.edgeTrack),seat.inputBasis)
        local excursion=len(delta)
        if excursion>.002 then
            if dot(delta,seat.inputEdge)>=.96*excursion then
                if not seat.edgeUntil or seat.travelTime>seat.edgeUntil then seat.edgeFrom=seat.edgeTrack end
                seat.edgeUntil=seat.travelTime+.065
            else seat.edgeUntil=nil;seat.edgeFrom=nil end
            seat.edgeTrack=v(tracked.x,tracked.y,tracked.z)
        end
    else seat.edgeTrack=nil;seat.edgeUntil=nil;seat.edgeFrom=nil end
    local cutting=seat.edgeUntil and seat.travelTime<=seat.edgeUntil
    M.state.receiptDepth=namedDepth;M.state.withdrawal=seat.backout;M.state.holdRequested=holdDepth and not cutting
    M.state.edgeRequested=cutting or false;M.state.edgeStroke=0;M.state.edgeTravel=0;M.state.edgeCommand=0;M.state.edgeRailStage=0
    seat.joint=false
    if rail and (onRail or depth>0 or holdDepth or puncture and seat.backout<.004) then
        if cutting then
            local advance=dot(worldMotion(sub(tracked,seat.edgeFrom),seat.inputBasis),seat.inputEdge)
            M.state.edgeCommand=math.max(0,math.min(.35,advance))
            seat.joint=VRBladeDriveEdge(hand,seat.jointSerial,w:VRBladeLocalEnd(),M.state.edgeCommand,seat.resistance)
            local path=VRBladeEdgeSample(hand,seat.jointSerial)
            if path and #path==3 then
                M.state.edgeStroke=path[1].w
                M.state.edgeRailStage=path[3].w
                if path[1].w>0 then M.state.edgeTravel=dot(sub(path[2],path[1]),path[3]) end
            end
        elseif holdDepth then seat.joint=VRBladeHoldDepth(hand,seat.jointSerial,seat.resistance)
        elseif puncture and not seat.pullMinimum and seat.backout<.004 then seat.joint=VRBladeInsertSlide(hand,seat.jointSerial,seat.resistance)
        else seat.joint=VRBladeApplySlideLoad(hand,seat.jointSerial,seat.resistance) end
    end
    if depth>0 and not seat.pullMinimum and not seat.joint and seat.travelTime>=(seat.measureAt or 0) then
        seat.measureAt=seat.travelTime+.025
        seat.joint=w:VRBladeUpdateJoint(hand,n,start,tip,edge)
    end
    M.state.joint=seat.joint
    M.state.depth=depth;M.state.resistance=seat.resistance
    if puncture and depth<=0 then
        M.state.phase='recovering contact'
    elseif M.state.phase=='recovering contact' then M.state.phase='bound' end
    local resistance=seat.resistance or 0
    local yield=resistance>0 and 1/(1+resistance) or 0
    local load=sub(d,mul(edge,math.max(0,dot(d,edge))*yield))
    -- Keep the shaft free to back out when no dynamic target body is available.
    if not puncture or seat.pullMinimum or seat.backout>.004 then
        load=sub(load,mul(axis,math.min(0,dot(load,axis))))
    end
    local loadLength=len(load)
    local stiffness=resistance>0 and resistance/(resistance+.35) or .72
    local amount=math.max(0,math.min(.080,(loadLength-.006)*stiffness))*math.min(1,age/.075)
    local offset=loadLength>.0001 and mul(load,-amount/loadLength) or v(0,0,0)
    if seat.joint then offset=v(0,0,0);amount=0 end
    local model=VRWorldDirToModel(offset)
    if not finite(model) or model.w<.5 then clear('invalid transform');return end
    sent=VRBladePoseOffset(hand,v(model.x,model.y,model.z,1))==true
    M.state.offset=sent and amount or 0;M.state.lastPeak=math.max(M.state.lastPeak,M.state.offset)
    -- Only an already simulated ragdoll accepts this feedback; never switch an
    -- animated NPC to ragdoll merely to make the constraint visible.
    if namedDepth>0 and not seat.joint and sent and amount>.006 and age>=(seat.loadAt or .05) then
        seat.loadAt=age+.05
        if w:VRBladeLoadRagdoll(n,offset) then M.state.bodyResponses=(M.state.bodyResponses or 0)+1 end
    end
    -- Input uses displacement relative to the target, not the stopped hand,
    -- camera motion or the NPC walking past. +Y is the item's authored edge.
    -- XR, animation and CET publish independently. Compare both streams across
    -- the same short window; requiring motion in the same tick drops valid input.
    local samples=seat.samples
    local relative=sub(raw,a);local at=(seat.clock or 0)+dt;seat.clock=at
    local score=0;local motion;local before
    if finite(edge) then
        for i=#samples,1,-1 do
            local old=samples[i]
            if at-old.at>.15 then table.remove(samples,i)
            else
                local rx,ry,rz=relative.x-old.rel.x,relative.y-old.rel.y,relative.z-old.rel.z
                local hand=worldMotion(sub(tracked,old.track),puncture and seat.inputBasis or basis)
                local hx,hy,hz=hand.x,hand.y,hand.z
                local projected=rx*edge.x+ry*edge.y+rz*edge.z
                local handEdge=dot(hand,puncture and seat.inputEdge or edge)
                -- The solver can carry the target along with the wrist. On that
                -- same live joint, signed XR travel measures the player's input.
                local coupled=old.joint and seat.joint
                if handEdge>0 and handEdge*handEdge>=(hx*hx+hy*hy+hz*hz)*.9216
                    and (coupled or projected>0 and projected*projected>=(rx*rx+ry*ry+rz*rz)*.9216) then
                    local candidate=coupled and handEdge or math.min(handEdge,projected)
                    if candidate>score then score=candidate;motion=hand;before=old end
                end
            end
        end
    end
    if #samples>=24 then table.remove(samples,1) end
    samples[#samples+1]={at=at,rel=relative,track=v(tracked.x,tracked.y,tracked.z,tracked.w),joint=seat.joint,
        start=v(start.x,start.y,start.z,1),tip=v(tip.x,tip.y,tip.z,1)}
    M.state.lastGesture=score
    local canCut=puncture and depth>0 and cutting and seat.joint and M.state.edgeTravel>=.008
        or not puncture and namedDepth>0
    if age>=.09 and canCut and resistance>0 and score>.045 and age>=(seat.retryAt or 0) then
        M.state.attempts=(M.state.attempts or 0)+1;seat.retryAt=age+.095
        local dispatched=player:VRBladeTryFollowThrough(w,n,start,tip,motion,before.start,before.tip)
        M.state.followAttempted=true
        if w.VRBladeLastFollowGate then
            M.state.followGate=w:VRBladeLastFollowGate()
            M.state.followSerial=w.vrBladeFollowSerial
            M.state.followAttempt=w.vrBladeFollowAttempt
            local counts=w.vrBladeFollowCounts
            M.state.followCounts={occupied=counts.x,resolved=counts.y,retained=counts.z,missingSlots=counts.w}
            local g=w.vrBladeFollowGeometry
            M.state.followGeometry={shaftGap=g.x,solverCos=g.y,controllerCos=g.z,shapePairs=g.w}
            local window=w.vrBladeFollowWindow
            M.state.followWindow={solver=window.x,visiblePin=window.y,visibleShape=window.z,clipped=window.w}
        end
        if dispatched then
            seat.dispatched=true;seat.samples={}
            M.state.requests=M.state.requests+1;M.state.phase='requested'
        end
    end
end
return M
