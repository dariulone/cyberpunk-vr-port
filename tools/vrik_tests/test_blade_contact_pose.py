"""Exercise the production visual contact latch; units are virtual scene units."""
from pathlib import Path
import argparse
import json
from lupa.luajit21 import LuaRuntime
root = Path(__file__).resolve().parents[2]
l = LuaRuntime()
l.globals().pose = l.execute((root/'mods/cet/CyberpunkVRPort_Weapon/blade_contact_pose.lua').read_text())
l.execute('''
function v(x,y,z,w) return {x=x,y=y,z=z,w=w or 0} end
Vector4={new=v}; WorldPosition={ToVector4=function(p)return p end}; WorldTransform={TransformPoint=function(t,p)return v(t.x+p.x,t.y+p.y,t.z+p.z,1) end}
local paused=false;local left=false
Game={VRWeaponInLeftHand=function()return left end,GetSystemRequestsHandler=function()return {IsGamePaused=function()return paused end} end}
local anchor=v(0,0,0,1);local raw=v(0,0,0,1);local last,idx,req,ok=raw,1,0,true
VRHandRawWorld=function()return raw end;VRWorldDirToModel=function(d)return v(d.x,d.y,d.z,1) end
VRBladePoseOffset=function(h,d)idx=h;last=d;return d.w==1 end
local rail,railKey,slide=false,0,0
VRBladeClearJoint=function()rail=false end
VRBladeJointPoint=function(_,key)return rail and key==railKey and v(anchor.x+slide,anchor.y,anchor.z+.12,1) or v(0,0,0,0)end
VRBladeApplySlideLoad=function(_,key,load)assert(load==2);return rail and key==railKey end
VRBladeTrackingBasis=function()return v(1,0,0,1)end
local age,n,follow=0,0,false
local npc={IsAttached=function()return true end,IsDead=function()return false end,IsActive=function()return true end,GetWorldTransform=function()return anchor end}
local w={vrBladeReceipt={jointSerial=1},GetEntityID=function()return {hash=11}end,VRBladeConfirmedSerial=function()return n end,VRBladeConfirmedAge=function()return age end,
 VRBladeConfirmedTarget=function()return npc end,VRBladeAnchorWorld=function()return anchor end,VRBladeConfirmedLocalPoint=function()return v(0,0,0,1)end,GetWorldForward=function()return v(0,1,0)end,
 VRBladeLoadRagdoll=function(_,n,d)assert(n>0 and d.w==0);return true end,
 VRBladeReleaseContact=function(_,serial)assert(serial>0)end,
 VRBladeMeasure=function(_,serial,a,b)
    return math.abs(a.x-anchor.x)<.16 and math.abs(a.y-anchor.y)<.16 and a.z<anchor.z+.10 and b.z>anchor.z-.01
        and v(.08,1,0,2) or v(0,0,0,0) end,
 VRBladeUpdateJoint=function()return false end,
 VRBladeConfirmedFollowThrough=function()return follow end, VRMeleeReach=function()return .75 end}
w.VRBladeLastFollowGate=function()return 'Dispatched' end
w.vrBladeFollowCounts=v(2,2,1,0);w.vrBladeFollowWindow=v(0,0,0,0);w.vrBladeFollowGeometry=v(0,1,1,1)
local p={VRBladeAllowed=function()return ok end,VRBladeTryFollowThrough=function(_,wep,serial,a,b,motion)
 assert(a.y==raw.y and b.z>a.z and motion.y>.045)
 req=req+1;wep.vrBladeFollowAttempt=req;wep.vrBladeFollowSerial=serial;return true end}
local function tracking(world) return v(world.x,world.z,-world.y,world.w) end
local axis=v(0,0,1);local function tick()pose.update(.016,p,w,raw,axis,tracking(raw))end
for _,hand in ipairs({false,true}) do
 left=hand;pose.reset();local base=req;raw=v(0,0,0,1);anchor=v(0,0,0,1);age=.016;tick();n=n+1;tick();assert(pose.state.phase=='bound');
 age=.16;local replies=pose.state.bodyResponses or 0;anchor.y=-.060;tick();tick()
 assert(req==base and last.y<-.025 and pose.state.bodyResponses==replies+1,'a moving NPC gets one rate-limited reaction, not damage')
 raw.y=0;anchor.y=0;tick();raw.y=.060;anchor.y=.060;tick();assert(req==base and pose.state.offset<.001,'whole scene motion is not a gesture')
 anchor.z=.23;age=.19;tick();assert(pose.state.phase=='bound' and req==base,'sliding along the overlapping segment is not separation')
 anchor.z=0;tick()
 raw.x=.075;tick();assert(req==base and last.x<-.04,'off-axis motion is resisted without another attack')
 raw.x=0;raw.y=.122;tick()
 assert(pose.state.followAttempted and pose.state.followGate=='Dispatched' and pose.state.followSerial==n
     and pose.state.followAttempt==req and pose.state.followCounts.occupied==2 and pose.state.followCounts.retained==1)
 tick();assert(not pose.state.followAttempted and req==base+1 and last.y<-.025 and idx==(left and 0 or 1),'wrong edge/hand or duplicate request')
 raw.z=-.07;tick();assert(pose.state.offset>0,'partial withdrawal with the segment still overlapping must remain bound')
 raw.z=-.8;tick();assert(last.w==0 and pose.state.offset==0,'withdrawal past the tip releases')
 raw.z=0;n=n+1;age=.01;tick();ok=false;tick();assert(last.w==0);ok=true
 n=n+1;age=.8;tick();assert(pose.state.phase=='released','stale hit acquired')
 -- The controller may publish one or two ticks before the corresponding world wrist.
 pose.reset();raw=v(0,0,0,1);anchor=v(0,0,0,1);local tracked=v(0,0,0,1)
 local function staggered() pose.update(.016,p,w,raw,axis,tracking(tracked)) end
 age=.01;staggered();n=n+1;w.vrBladeReceipt.jointSerial=n;staggered();local prior=req
 railKey=n
 age=.12;tracked.y=.067;staggered();assert(req==prior,'unmatched input cannot request damage')
 age=.14;raw.y=.067;staggered();assert(req==prior+1,'lagging world wrist lost the gesture')
 rail=true;age=.18;staggered();follow=true;n=n+1;age=.01;staggered();follow=false
 assert(pose.state.phase=='bound' and rail and pose.state.joint,'receipt rollover released the original solver joint')
 assert(not pose.state.followGate and not pose.state.followSerial and not pose.state.followCounts,'old gate reused on a new receipt')
 age=.95;staggered();staggered();assert(req==prior+1,'old travel repeated damage')
 -- The connected NPC follows the wrist; the measured wrist/target delta is zero.
 anchor.y=.063;tracked.y=.13;raw.y=.13;staggered();assert(req==prior+2,'solver-carried target masked the new XR travel')
 slide=.14;age=1.1;staggered();slide=0
 assert(pose.state.joint and pose.state.offset==0,'soft wrist force acted on top of a solver-owned constraint')
 raw.x=.7;tracked.x=.7;staggered();assert(not rail and pose.state.phase=='released','large tracking separation kept the solver joint alive')
 rail=false;pose.reset()
end
''')
print('PASS: both hands; zero relative travel, lateral rejection, one edge request, release, restriction and stale contact')

def replay_thrust(recording=None, slip=0):
    """Drive the production seat with measured shaft depths, including an unavailable D6."""
    lua=LuaRuntime()
    lua.globals().pose=lua.execute((root/'mods/cet/CyberpunkVRPort_Weapon/blade_contact_pose.lua').read_text())
    tick=lua.execute('''
      local function v(x,y,z,w) return {x=x,y=y,z=z,w=w or 0} end
      Vector4={new=v}
      Game={VRWeaponInLeftHand=function()return false end,GetSystemRequestsHandler=function()
        return {IsGamePaused=function()return false end}end}
      local raw,track,base,axis,edge,anchor,point
      local n,age,depth,bodyDepth,canBind,rail,loadMode
      local releases,attempts,locked=0,0,0
      local basis=v(0,1,0,1);local cutEdge;local travel,pressure=0,false
      local edgeRequests,cutRequests=0,0
      VRBladeClearJoint=function()rail=false end
      VRBladeJointPoint=function()return rail and point or v(0,0,0) end
      VRBladeTrackingBasis=function()return basis end
      VRHandRawWorld=function()return raw end
      VRWorldDirToModel=function(d)return v(d.x,d.y,d.z,1) end
      VRBladePoseOffset=function(h,d)return d.w==1 end
      VRBladeHoldDepth=function()loadMode='held';locked=locked+1;return rail end
      VRBladeApplySlideLoad=function()loadMode='sliding';return rail end
      VRBladeInsertSlide=VRBladeApplySlideLoad
      VRBladeDriveEdge=function(_,key,shaft,advance)
        assert(rail and shaft.z==.75 and advance>=0 and advance<=.35)
        cutEdge=edge;edgeRequests=edgeRequests+1;loadMode='edge';return true
      end
      VRBladeEdgeSample=function()
        if not cutEdge or travel==0 then return {} end
        return {v(point.x,point.y,point.z,41),v(point.x+travel*cutEdge.x,point.y+travel*cutEdge.y,point.z+travel*cutEdge.z,1),
                v(cutEdge.x,cutEdge.y,cutEdge.z,5)}
      end
      local npc={IsAttached=function()return true end}
      local w={vrBladeReceipt={puncture=true},GetEntityID=function()return {hash=1}end,
        VRBladeConfirmedFollowThrough=function()return false end,VRBladeConfirmedAge=function()return age end,
        VRBladeConfirmedSerial=function()return n end,VRBladeConfirmedTarget=function()return npc end,
        VRBladeAnchorWorld=function()return anchor end,VRMeleeReach=function()return .75 end,
        GetWorldForward=function()return edge end,VRBladeLocalEnd=function()return v(0,0,.75)end,
        VRBladeMeasure=function()return v(depth,0,0,2)end,
        VRBladeSeatInterval=function()return v(.5,bodyDepth,0,bodyDepth>0 and 1 or 0)end,
        VRBladeFollowSlot=function()return v(0,0,0,0)end,
        VRBladeLodgeContact=function(_,s)assert(s==n)end,VRBladeReleaseContact=function()releases=releases+1 end,
        VRBladeLoadRagdoll=function()return false end,
        VRBladeUpdateJoint=function(_,h,s)assert(s==n and (depth>0 or bodyDepth>0));attempts=attempts+1;rail=canBind;return rail end}
      local p={VRBladeAllowed=function()return true end,VRBladeTryFollowThrough=function(_,w,s,a,b,m,oldA,oldB)
          assert(pressure and travel>0 and loadMode=='edge' and oldA.w==1 and oldB.w==1)
          cutRequests=cutRequests+1;return false
      end}
      return function(dt,serial,time,exact,any,available,px,py,pz,tx,ty,tz,hx,hy,hz,c,s,ex,ey,ez,jx,jy,jz,verified,observedRail)
        n,age,depth,bodyDepth,canBind=serial,time,exact,any,available
        w.vrBladeReceipt.jointSerial=serial
        base=v(px,py,pz,1);axis=v((tx-px)/.75,(ty-py)/.75,(tz-pz)/.75)
        edge=math.abs(axis.x)<.8 and v(1,0,0) or v(0,1,0)
        raw=base;track=v(hx,hy,hz,1);anchor=v(px+.5*(tx-px),py+.5*(ty-py),pz+.5*(tz-pz),1)
        point=bodyDepth>0 and anchor or v(tx+axis.x,ty+axis.y,tz+axis.z,1)
        if c then basis=v(c,s,0,1) end
        if observedRail~=nil then rail=observedRail end
        if ex then
          pressure=true;rail=available;travel=verified;edge=v(ex,ey,ez);point=v(jx,jy,jz,1)
          w.vrBladeReceipt.seatEdge=edge
        end
        loadMode='none'
        pose.update(dt,p,w,base,axis,track)
        return pose.state.phase,releases,attempts,locked,loadMode,edgeRequests,cutRequests,pose.state.withdrawal,pose.state.holdRequested
      end
    ''')
    if recording is None:
        def step(serial,age,named,any_depth,track,available=False,dt=.02):
            return tick(dt,serial,age,named,any_depth,available,0,0,0,.75,0,0,0,0,track)
        step(0,0,0,0,0)
        assert step(1,0,0,0,0)[0]=='awaiting overlap'
        assert step(1,.04,0,.2,.02)[0]=='bound'
        for i in range(10): result=step(1,.06+i*.02,0,.2,.08,True)
        assert result[3]>0 and result[4]=='held'
        assert step(1,.28,0,.2,.065,True)[4]=='sliding'
        for i in range(15): result=step(1,.30+i*.02,0,.2,.065,True)
        assert result[4]=='held'
        assert step(1,.75,0,.2,.065,True,dt=.15)[0]=='bound'
        # The target moves its collider off the shaft. The wrist stays still.
        assert step(1,.77,0,0,.065)[1]==0
        assert step(1,.79,0,0,.061)[1]==0
        result=step(1,.81,0,0,-.01)
        assert result[:2]==('released',1),result
        assert step(2,0,0,.2,.03)[0]=='bound'
        # Deliberate pull, long pause, tiny inward jitter and conscious reinsertion.
        step(2,.06,0,.2,.03,True,dt=.06)
        step(2,.1,0,.2,-.04,True)
        result=step(2,.31,0,.1,-.04,True,dt=.21)
        assert result[4]=='sliding' and result[7]>.06 and not result[8],result
        result=step(2,.34,0,.1,-.035,True)
        assert result[4]=='sliding' and not result[8],result
        step(2,.37,0,.1,-.004,True)
        result=step(2,.47,0,.1,-.004,True,dt=.10)
        assert result[4]=='held' and result[7]==0 and result[8],result
        # The XR hand has already reversed 3 cm when the solved tip first clears.
        # A return while still inside was covered above. Here the old seat must end.
        step(2,.49,0,.08,-.084,True)
        result=step(2,.51,0,0,-.054,True)
        assert result[:2]==('released',2),('clear on reversal',result)
        assert step(2,.53,0,.08,-.02,True)[:2]==('released',2)
        assert step(3,0,.08,.08,0,True)[0]=='bound'
        assert step(3,.02,0,0,-.08,True)[:2]==('released',3)
        # A shallow puncture can have a confirmed receipt before CET sees any
        # occupied pose. Withdrawal still owns and releases that waiting receipt.
        assert step(4,0,0,0,0)[0]=='awaiting overlap'
        result=step(4,.02,0,0,-.04)
        assert result[:2]==('released',4),('waiting receipt still blocks entry',result)
        assert step(4,.04,0,0,-.04)[:2]==('released',4)
        assert step(5,0,0,0,0)[0]=='awaiting overlap'
        result=step(5,.02,0,.08,-.04,True)
        assert result[:2]==('bound',4) and result[7]>=.039,result
        assert step(5,.04,0,0,-.05,True)[:2]==('released',5)
        print('PASS: occupied reversal retains the seat; clear reversal and unseated withdrawal release once')
        return
    data=json.loads(recording.read_text(encoding='utf-8'))
    cal=data['calibration'];u,_,v=cal['blade'];x,y,_=cal['direction']
    c=(u*x-v*y)/(u*u+v*v);s=(v*x+u*y)/(u*u+v*v)
    if 'press' in data:
        # Recorded XR and rendered geometry select the mode. The old build had
        # no free edge; exercise absent and independently supplied solver proof.
        ex,ey,ez=cal['edge'];edge=(c*ex+s*ez,s*ex-c*ez,ey)
        def press_tick(dt,r,n):
            return tick(dt,n,.12+r['at'],r['receiptDepth'],r['shapeDepth'],bool(r.get('joint')),
                        *r['base'],*r['tip'],*r['hand'],c,s,*edge,*r.get('joint',r['base']),slip)
        # Acquire using the original stab geometry before the NPC can turn
        # the visible prop. Retain that input frame throughout the pressure.
        start=next(r for r in data['contact'] if r.get('same') and r['depth']>0)
        start=dict(start,at=0)
        press_tick(.02,start,0);press_tick(.02,start,start['serial'])
        for r in data['contact']:
            if r['epoch']>start['epoch']: press_tick(.025,r,r['serial'])
        previous=0
        for r in data['press']:
            if r.get('serial')!=start['serial']:
                # The stock error already allowed an ordinary hit after the
                # spurious withdrawal. Subsequent samples are a new receipt.
                break
            result=press_tick(r['at']-previous,r,r['serial']);previous=r['at']
            assert result[1]==0,(recording,'lateral input released seat',r['at'])
        is_edge=data['case']=='edge-press'
        assert (result[5]>0)==is_edge,(recording,slip,result)
        occupied_press=any(r['shapeDepth']>0 and r.get('gesture',0)>.045 for r in data['press'])
        if not occupied_press:
            assert result[6]==0,(recording,'solver travel with no occupied press',result)
        elif data['checks']['lodged_hold'] or slip<=0:
            assert (result[6]>0)==(is_edge and slip>0),(recording,slip,result)
        print(f'{recording.stem}: edge renewals={result[5]}, dispatch attempts={result[6]}, unit solver slip={slip}m')
        return
    start=data['before']
    tick(.02,start['serial'],0,0,0,False,*start['base'],*start['tip'],*start['hand'],c,s)
    if 'withdrawExtension' in data and not data['checks']['released']:
        # Replay the captured controller, occupancy, and observed joint lifetime.
        # The recorded extension ends inside, so clearance is an explicit extra test input.
        serial=start['serial'];elapsed=0
        for phase in ('contact','withdraw','withdrawExtension'):
            previous=0
            for frame in data[phase]:
                dt=frame['at']-previous;previous=frame['at']
                elapsed=elapsed+dt if serial==frame['serial'] else 0
                serial=frame['serial'];rail=bool(frame.get('joint'))
                result=tick(dt,serial,elapsed,frame['simDepth'],frame['shapeDepth'],rail,
                            *frame['base'],*frame['tip'],*frame['hand'],c,s,*([None]*7),rail)
                if phase=='withdraw' and previous>=.4 or phase=='withdrawExtension':
                    assert result[7]>.50 and not result[8] and result[1]==0,(phase,previous,result)
                    if rail: assert result[4]=='sliding',result
                if phase=='withdraw': bind_attempts=result[2]
                if phase=='withdrawExtension': assert result[2]==bind_attempts,'rebound during withdrawal'
        assert frame['shapeDepth']>0 and result[7]>.8
        result=tick(.02,serial,elapsed+.02,0,0,False,*frame['base'],*frame['tip'],*frame['hand'],c,s,*([None]*7),False)
        assert result[:2]==('released',1),result
        print(f'{recording.parent.name}: pull retained through residual overlap and D6 loss; released on supplied clearance')
        return
    elapsed=0
    serial=start['serial']
    for phase in ('contact','withdraw','reentry'):
        previous=0
        for frame in data[phase]:
            dt=frame['at']-previous;previous=frame['at']
            elapsed=elapsed+dt if serial==frame['serial'] else 0
            serial=frame['serial']
            # No solver double can prove a new native bind. Exercise acquisition
            # and withdrawal when that actor is absent on every frame.
            result=tick(dt,serial,elapsed,frame['simDepth'],frame['shapeDepth'],False,
                        *frame['base'],*frame['tip'],*frame['hand'],c,s)
            if phase=='contact' and frame['same']:
                expected='bound' if frame['simDepth']>0 or frame['shapeDepth']>0 else 'recovering contact'
                assert result[0]==expected and result[1]==0,(recording,frame['at'],result)
        if phase=='withdraw': assert result[:2]==('released',1),(recording,result)
    if data['reentry'][-1]['serial']>start['serial']+1:
        assert result[0]=='bound' and result[2]>1,(recording,result)
    else:
        assert result[:2]==('released',1),(recording,result)
    print(f'{recording.parent.name}: tracked withdrawal released exactly once')

if __name__=='__main__':
    parser=argparse.ArgumentParser()
    parser.add_argument('--thrust',type=Path,nargs='*',default=[])
    parser.add_argument('--press',type=Path,nargs='*',default=[])
    args=parser.parse_args()
    replay_thrust()
    for path in args.thrust: replay_thrust(path)
    for path in args.press:
        replay_thrust(path,0)
        replay_thrust(path,-.223)
        replay_thrust(path,.02)
