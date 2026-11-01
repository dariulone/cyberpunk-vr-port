-- Original device screens: real rendered finger -> native mesh UV -> live ink
-- button bounds -> that button's own device action. Never emits UI_confirm/A.
local Contact=require('modules/touch_contact')
local Sources=require('modules/world_touch_sources')
local Computer=require('modules/computer_touch')
local M={active=false,error=nil,clicks=0,haptics=0,hands={}}
local elapsed=0
local playerID,components,hover=nil,{},{}
local hands={{},{}}
local function visible(w)
    for _=1,32 do
        if not w then return true end
        if not w:IsVisible() or w:GetOpacity()<=.01 then return false end
        w=w:GetParentWidget()
    end
    return false
end
local function hoverOff(hand)
    local old=hover[hand];hover[hand]=nil
    if old then pcall(function() old.control:TriggerOnHoverOutAnimations() end) end
end
local function clear()
    for h=1,2 do hoverOff(h);Contact.reset(hands[h]);hands[h].drag=nil;hands[h].pending=nil;hands[h].glide=nil end
    components={};M.active=false;M.hands={}
end
local function allowed(overlayOpen)
    if overlayOpen then return nil end
    local p=Game.GetPlayer();if not p then return nil end
    local defs=GetAllBlackboardDefs()
    local board=Game.GetBlackboardSystem():Get(defs.UI_System)
    if not board or board:GetBool(defs.UI_System.IsInMenu) then return nil end
    local menu=GetMod('CyberpunkVRPort_Menu')
    if menu and menu.debugState and menu.debugState().open then return nil end
    return p
end
local function collectButtons(root,gameController)
    local result,scrolls={},{};local visited=0
    local computer=Computer.context(gameController,root)
    local wake=Computer.sleepTarget(computer)
    if wake then return {wake},scrolls end
    local function walk(w,keypad,depth,path)
        visited=visited+1
        if not w or visited>1200 or depth>32 or not w:IsVisible() or w:GetOpacity()<=.01 then return end
        local controller=w:GetController()
        -- Mail lists, mail text and similar lists: the native wheel/stick scroller.
        -- Collected parent first, so the innermost area comes last.
        if controller and controller:IsA('inkScrollController') and controller.direction==inkEScrollDirection.Vertical then
            local area=inkWidgetRef.Get(controller.ScrollArea)
            if area then scrolls[#scrolls+1]={control=controller,area=area} end
        end
        if Computer.collect(computer,w,controller,path,result) then return end
        if controller and controller:IsA('KeypadDeviceController') then keypad=controller end
        local name=NameToString(w:GetName())
        local digit=keypad and (name=='Cancel' or name=='AuthorizeUser' or name:match('^%d$'))
        if digit and w:IsInteractive() then
            result[#result+1]={widget=w,keypad=keypad,control=controller,key=path..'/'..name}
            return
        elseif controller and controller:IsA('DeviceActionWidgetControllerBase') then
            local target=controller.targetWidget
            if target and target:IsInteractive() then
                result[#result+1]={widget=target,control=controller,key=path..'/'..name}
            end
            return
        end
        if w:IsA('inkCompoundWidget') then
            for i=0,w:GetNumChildren()-1 do walk(w:GetWidgetByIndex(i),keypad,depth+1,path..'/'..i) end
        end
    end
    walk(root,nil,0,'')
    Computer.addWindowClose(computer,result)
    return result,scrolls
end
local function usable(b)
    if not visible(b.widget) then return false end
    if b.computer then return Computer.usable(b) end
    if not b.widget:IsInteractive() then return false end
    if b.keypad then return b.keypad:CanHandleClickAction() end
    return b.control:CanExecuteAction() and #b.control:GetActions()>0
end
local function rect(w)
    local parent=w:GetParentWidget()
    if not parent or not parent:IsA('inkCompoundWidget') then return nil end
    local size=parent:GetChildSize(w) -- actual stretch/auto-layout size; GetSize() is often zero
    local point=WidgetUtils.LocalToGlobal(w)
    local sx,sy=1,1
    local a=w
    for _=1,32 do
        if not a then break end
        -- Fail closed on unsupported 2D rotations/shears, never press an AABB's
        -- transparent corner. Ordinary device widgets use axis-aligned layout.
        local sh=a:GetShear()
        if math.abs(a:GetRotation())>.01 or math.abs(sh.X)+math.abs(sh.Y)>.001 then return nil end
        local scale=a:GetScale();sx=sx*scale.X;sy=sy*scale.Y
        a=a:GetParentWidget()
    end
    if sx<=0 or sy<=0 or size.X<=0 or size.Y<=0 then return nil end
    return point.X,point.Y,size.X*sx,size.Y*sy
end
local function insideMasks(widget,x,y)
    local parent=widget:GetParentWidget()
    for _=1,32 do
        if not parent then return true end
        if parent:IsA('inkScrollAreaWidget') and parent.useInternalMask then
            local bx,by,bw,bh=rect(parent)
            if not bx or x<bx or x>=bx+bw or y<by or y>=by+bh then return false end
        end
        parent=parent:GetParentWidget()
    end
    return false
end
local function hit(buttons,x,y)
    for i=#buttons,1,-1 do
        local b=buttons[i]
        if usable(b) then
            local bx,by,bw,bh=rect(b.widget)
            if bx and x>=bx and x<bx+bw and y>=by and y<by+bh and insideMasks(b.widget,x,y) then return b end
        end
    end
end
-- The innermost visible scroll area under a native ink point whose content
-- overflows it; scale = native ink pixels per scroll-content unit.
local function scrollAt(scrolls,x,y)
    for i=#scrolls,1,-1 do
        local s=scrolls[i]
        local control=s.control
        local range=control.contentSize.Y-control.viewportSize.Y
        if range>1 and visible(s.area) then
            local bx,by,bw,bh=rect(s.area)
            if bx and x>=bx and x<bx+bw and y>=by and y<by+bh and insideMasks(s.area,x,y) then
                return {control=control,scale=bh/control.viewportSize.Y,range=range}
            end
        end
    end
end
-- Finger travel along the glass before a touch inside a scroll area becomes a
-- drag instead of a tap, and the fling's speed decay per second.
local DRAG_SLOP,GLIDE_DECAY,GLIDE_STOP=.006,5,.03
local function scrollTo(control,pos)
    pos=math.max(0,math.min(1,pos))
    control:SetScrollPosition(pos)
    return pos
end
local function glide(state,dt)
    local g=state.glide
    if not g then return end
    local ok,old=pcall(function() return g.control:GetScrollPosition() end)
    if not ok then state.glide=nil;return end
    local pos=scrollTo(g.control,old+g.v*dt)
    g.v=g.v*math.exp(-GLIDE_DECAY*dt)
    if math.abs(g.v)<GLIDE_STOP or pos<=0 or pos>=1 then state.glide=nil end
end
local function discover()
    components={}
    for _,source in ipairs(Sources.resolve(VRViewWorldPos())) do
        local component=source.component
        local root=component:GetWidget()
        local controller=component:GetGameController()
        if root and controller and controller:IsA('DeviceInkGameControllerBase') and visible(root) and not controller:IsInteractivityBlocked() then
            local buttons,scrolls=collectButtons(root,controller)
            if #buttons>0 or #scrolls>0 then components[#components+1]={component=component,root=root,controller=controller,buttons=buttons,
                scrolls=scrolls,key=source.key,owner=source.owner,contact=source.dropPoint and Contact.dropPoint or nil} end
        end
    end
end
local function activate(panel,b,hand)
    if panel.controller:IsInteractivityBlocked() or not usable(b) then return false end
    local animate=b.control and b.control:IsA('DeviceButtonLogicControllerBase')
    if animate then b.control:TriggerOnPressAnimations() end
    if b.computer then
        if not Computer.activate(b) then return false end
    elseif b.keypad then
        -- Same per-key handler as OnMouseButtonReleased; retains digit limits,
        -- authorization, password checks, sounds and native door action callbacks.
        b.keypad:HandleButtonClicked(b.widget)
    else
        b.widget:CallCustomCallback('OnExecuteButtonAction')
    end
    -- Browser navigation destroys the clicked page synchronously. Do not query
    -- its expired weak controller after dispatch or lose the click's haptic.
    if animate then pcall(function() b.control:TriggerOnReleaseAnimations() end) end
    M.clicks=M.clicks+1
    M.lastClick={hand=hand,key=b.key,owner=panel.owner}
    -- hand 1/2 = VRWorldTouchPointer side 0/1 = left/right = haptic mask 1/2.
    if type(VRHapticEvent)=='function' then VRHapticEvent(20,hand==2 and 2 or 1);M.haptics=M.haptics+1 end
    elapsed=.1 -- reread changed/removed buttons before another contact
    return true
end
local function tick(dt,overlayOpen)
    local p=allowed(overlayOpen)
    if not p or type(VRWorldTouchPointer)~='function' then clear();return end
    local id=tostring(p:GetEntityID().hash)
    if id~=playerID then clear();playerID=id end
    elapsed=elapsed+dt
    if elapsed>=.1 then elapsed=0;discover() end
    local chosen={}
    local view=VRViewWorldPos()
    for _,panel in ipairs(components) do
        local c=panel.component
        if c:IsEnabled() and visible(panel.root) and not panel.controller:IsInteractivityBlocked() then
            local matrix=c:GetLocalToWorld();local centre=matrix.W
            local dx,dy,dz=centre.x-view.x,centre.y-view.y,centre.z-view.z
            if dx*dx+dy*dy+dz*dz<2.25 then
                SetVRDiagCapture(1)
                -- Native UI geometry is X/Z; default front is -Y. The authored
                -- face vector determines front/back for screens with a custom face.
                local face=c.sceneWidgetProperties.faceVector
                local normal
                if c.sceneWidgetProperties.useCustomFaceVector then
                    normal=Vector4.new(matrix.X.x*face.x+matrix.Y.x*face.y+matrix.Z.x*face.z,
                        matrix.X.y*face.x+matrix.Y.y*face.y+matrix.Z.y*face.z,
                        matrix.X.z*face.x+matrix.Y.z*face.y+matrix.Z.z*face.z,0)
                else normal=Vector4.new(-matrix.Y.x,-matrix.Y.y,-matrix.Y.z,0) end
                local n=math.sqrt(normal.x^2+normal.y^2+normal.z^2)
                if n>.001 then
                    normal=Vector4.new(normal.x/n,normal.y/n,normal.z/n,0)
                    local samples=VRWorldTouchPointer(c,centre,normal)
                    for h=1,2 do
                        local sample=samples[h*2-1]
                        if sample and sample.w>.5 and (not chosen[h] or math.abs(sample.z)<math.abs(chosen[h].sample.z)) then
                            chosen[h]={sample=sample,point=samples[h*2],panel=panel,button=hit(panel.buttons,sample.x,sample.y)}
                        end
                    end
                end
            end
        end
    end
    M.active=#components>0;M.hands={}
    for h=1,2 do
        local c=chosen[h];local b=c and c.button
        local control=b and b.control
        local hoverKey=b and c.panel.key..b.key
        if hoverKey~=(hover[h] and hover[h].key) then
            hoverOff(h)
            if control and control:IsA('DeviceButtonLogicControllerBase') then
                control:TriggerOnHoverOverAnimations();hover[h]={control=control,key=hoverKey}
                if control:IsA('ImageActionButtonLogicController') and c.panel.controller:IsA('VendingMachineInkGameController') then
                    -- Native vending hover updates the price for this specific product.
                    c.panel.controller:UpdatePrice(control:GetPrice())
                end
            end
        end
        local state=hands[h]
        local panel=c and c.panel.key
        if panel~=state.panel then Contact.reset(state);state.panel=panel;state.drag=nil;state.pending=nil end
        local sample=c and c.sample
        glide(state,dt)
        -- Lifting the finger ends its drag; a touch that never dragged is the tap.
        local profile=c and c.panel.contact or Contact
        if state.drag and (not sample or sample.z>(profile.press+profile.release)/2) then
            local drag,tap=state.drag,state.pending;state.drag=nil;state.pending=nil
            if drag.moved and math.abs(drag.v)>GLIDE_STOP then state.glide={control=drag.scroll.control,v=drag.v} end
            if tap and sample then activate(tap.panel,tap.button,h) end
        end
        local was=state.down
        local press=Contact.step(state,sample,b,dt,c and c.panel.contact)
        if c and state.down and not was then
            -- Inside an overflowing list a tap waits for the lift: the same touch may become a drag.
            local scroll=scrollAt(c.panel.scrolls,sample.x,sample.y)
            if scroll then
                state.glide=nil
                state.drag={scroll=scroll,at=c.point,y0=sample.y,v=0,moved=false}
                if press then state.pending={panel=c.panel,button=b};press=false end
            end
        end
        local drag=state.drag
        if drag and c then
            local control=drag.scroll.control
            if not drag.moved then
                local a,p=drag.at,c.point
                if (p.x-a.x)^2+(p.y-a.y)^2+(p.z-a.z)^2>DRAG_SLOP^2 then
                    drag.moved=true;state.pending=nil
                    drag.y0=sample.y;drag.pos0=control:GetScrollPosition();drag.last=drag.pos0
                    if type(VRHapticEvent)=='function' then VRHapticEvent(21,h==2 and 2 or 1) end
                end
            end
            if drag.moved then
                local pos=scrollTo(control,drag.pos0+(drag.y0-sample.y)/(drag.scroll.scale*drag.scroll.range))
                drag.v=drag.v*.6+(pos-drag.last)/math.max(dt,.001)*.4;drag.last=pos
            end
        end
        if c then M.hands[h]={x=c.sample.x,y=c.sample.y,d=c.sample.z,button=b and b.key or nil,armed=state.armed,down=state.down,
            drag=drag and (drag.moved and 'drag' or 'touch') or nil,glide=state.glide and state.glide.v or nil} end
        if press then
            activate(c.panel,b,h)
            -- Two fingers landing together on the same button are one click.
            local other=chosen[3-h]
            if other and other.button and other.panel.key==c.panel.key and other.button.key==b.key then
                hands[3-h].armed=false;hands[3-h].down=true
            end
        end
    end
end
function M.tick(dt,overlayOpen)
    local ok,err=pcall(tick,math.max(tonumber(dt) or 0,0),overlayOpen)
    M.error=not ok and tostring(err) or nil
    if not ok then M.lastError=tostring(err);M.errors=(M.errors or 0)+1;clear() end
end
function M.shutdown() clear();playerID=nil;elapsed=0 end
return M
