"""Ordinary-melee regressions: wrist adjustments, short strokes and reversals."""
import math
import argparse
import json
from pathlib import Path
from lupa import LuaRuntime

ROOT=Path(__file__).resolve().parents[2]

def replay(hz,case,source=None):
    lua=LuaRuntime(unpack_returned_tuples=True)
    lua.globals().package.path=str(ROOT/'mods/cet/CyberpunkVRPort_Weapon/?.lua').replace('\\','/')+';'+lua.globals().package.path
    lua.execute('''
      stamp=0;seq=0;x=0;theta=0;starts=0;sounds=0;active=0;firstSound=-1;lastStart=-1
      function v(x,y,z,w) return {x=x,y=y,z=z,w=w or 1} end
      pulses=0;function SetVRHapticPulse(...) pulses=pulses+1 end
      Vector4={new=v}
      Game={VRWeaponInLeftHand=function() return false end,GetVRSharedSlot=function() return 0 end,
        GetSystemRequestsHandler=function() return {IsGamePaused=function() return false end} end,
        VRMeleeInputFrame=function() return {v(stamp,seq,0),v(0,0,1),v(x,0,1),v(0,0,0,1),v(0,0,math.sin(theta/2),math.cos(theta/2))} end}
      player={VRCyberwareBeginSwing=function() starts=starts+1;lastStart=stamp;return true end,
        VRCyberwareUpdateSwing=function() end,
        VRMeleeSweepHit=function(_,w,p,t,oldP,oldT,r,passive) if not passive then active=active+1 end end,
        VRMeleeBladeHit=function(_,w,p,f,r,strong,passive) if not passive then active=active+1 end end,
        VRMeleeWhoosh=function() sounds=sounds+1;if firstSound<0 then firstSound=stamp end end}
      weapon={VRMeleeReach=function() return 1.25 end,VRBladeKind=function() return -1 end};origin=v(0,0,0)
    ''')
    m=lua.execute(source or (ROOT/'mods/cet/CyberpunkVRPort_Weapon/cyberware.lua').read_text())
    g=lua.globals();elapsed=0
    def tick(dx=0,theta=None):
        nonlocal elapsed
        elapsed+=1/hz;g.stamp=round(elapsed*1000);g.seq+=1;g.x+=dx
        if theta is not None:g.theta=theta
        axis = g.v(0,0,1,0) if case=='roll' else g.v(math.sin(g.theta),math.cos(g.theta),0,0)
        m.ordinary(1/hz,g.player,g.weapon,g.v(g.x,0,1),axis,g.origin)
    for _ in range(hz):tick()
    began=g.stamp
    if case in ('arc','roll','wrist'):
        steps=max(2,round(.05*hz))
        angle=.04 if case=='wrist' else .7
        for i in range(steps):tick(theta=(i+1)/steps*angle)
    elif case=='jitter':
        for i in range(hz):tick(.04 if i%2==0 else -.04)
    elif case=='short':
        for _ in range(max(1,round(.033*hz))):tick(6/hz)
    elif case=='below_sound':
        for _ in range(round(.18*hz)):tick(3.2/hz)
    elif case=='reversal':
        for _ in range(round(.15*hz)):tick(4/hz)
        for _ in range(round(.15*hz)):tick(-4/hz)
    else:raise ValueError(case)
    for _ in range(hz):tick()
    assert g.pulses==0,(hz,case,"unexpected air haptic",g.pulses)
    assert g.starts==0, "air motion selected a game attack"
    return dict(starts=m.state.fullSwingStarts or 0,sounds=g.sounds,active=g.active,latency=g.firstSound-began)

def check_contact_stroke():
    """Exercise the shipping Lua gesture gate. Only the game's collision query is doubled."""
    lua=LuaRuntime()
    lua.globals().package.path=(ROOT/'mods/cet/CyberpunkVRPort_Weapon/?.lua').as_posix()+';'+lua.globals().package.path
    lua.execute('''
        local melee=require('ordinary_melee');local epoch=0;local clock=0
        local function v(x,y,z,w) return {x=x,y=y,z=z,w=w or 1} end
        local control,grip,base=v(0,0,0),v(0,0,0),v(0,0,0);local axis=v(0,0,1,0)
        Vector4={new=v};Game={VRWeaponInLeftHand=function()return false end,GetVRSharedSlot=function()return 0 end,
            GetSystemRequestsHandler=function()return {IsGamePaused=function()return false end} end,
            VRMeleeInputFrame=function()return {v(clock,epoch,0),v(0,0,0),control,v(0,0,0),v(0,0,0)}end}
        local diag={ticks=0};local near=false;local held=false;local queryCalls=0;local attacks=0
        local owner={VRCyberwareBeginSwing=function()error('short stroke became a full swing')end,
            VRMeleeWhoosh=function()error('short stroke became an air whoosh')end,
            VRMeleeSweepHit=function(_,w,a,b,c,d,r,passive)
                if passive then return 0 end
                assert(b.z>a.z and d.z>c.z);queryCalls=queryCalls+1
                if near and not held then held=true;attacks=attacks+1;return 1 end;return 0 end}
        local weapon={VRMeleeReach=function()return .75 end,VRBladeKind=function()return 2 end,GetWorldForward=function()return v(0,1,0,0) end}
        local function tick(cx,cy,gx,gy,by)
            epoch=epoch+1;clock=clock+20;control=v(cx,cy,0);grip=v(gx,gy,0);base=v(0,by,0)
            melee.update(.02,owner,weapon,grip,axis,base,diag)
        end
        local function rest() melee.reset();held=false;for i=1,55 do tick(0,0,0,0,0) end end
        local function draw(c,g,o) for i=1,6 do local d=i*.022;tick(0,c*d,0,g*d,o*d) end end
        rest();draw(1,1,0);assert(queryCalls>0 and attacks==0,'air must be probed, not hit')
        near=true;rest();local first=queryCalls;draw(0,1,0);rest();draw(1,0,0);rest();draw(1,1,1)
        rest();for i=1,6 do tick(i*.022,0,i*.022,0,0) end
        assert(queryCalls==first and attacks==0,'wrist tracking, weapon displacement and allowed direction all required')
        rest();draw(1,1,0);assert(attacks==1 and diag.strokeDispatches==1)
        for i=7,25 do tick(0,i*.022,0,i*.022,0) end;assert(attacks==1,'unbroken travel must be spent')
        for i=1,15 do tick(0,.55,0,.55,0) end
        held=false -- the game reports separation
        for i=1,6 do tick(0,.55+i*.022,0,.55+i*.022,0) end
        assert(attacks==2 and diag.strokeDispatches==2 and (diag.fullSwingStarts or 0)==0)
        rest();local before=attacks
        for i=1,12 do tick(0,-i*.006,0,-i*.006,0) end
        assert(attacks==before+1,'slow reverse draw never reached the contact sweep')
        rest();before=attacks
        for i=1,12 do
            epoch=epoch+1;clock=clock+26;control=v(0,0,i*.006);grip=v(0,0,i*.006)
            melee.update(.026,owner,weapon,grip,axis,base,diag)
        end
        assert(attacks==before+1,'slow axial movement never reached the contact sweep')
        rest();before=attacks
        local angle=0
        local input=Game.VRMeleeInputFrame
        Game.VRMeleeInputFrame=function()local f=input();f[5]=v(math.sin(angle*.5),0,0,math.cos(angle*.5));return f end
        for i=1,12 do
            epoch=epoch+1;clock=clock+17;angle=i*.015
            melee.update(.017,owner,weapon,grip,v(0,-math.sin(angle),math.cos(angle),0),base,diag)
        end
        assert(attacks==before+1,'rotation-only sweep of the visible prop was lost')
    ''')
    print('PASS short contact: air, model-only, tracking-only, player translation, wrong direction, single dispatch until rest, rearm')

def check_delayed_virtual_sweep():
    lua=LuaRuntime()
    lua.globals().package.path=(ROOT/'mods/cet/CyberpunkVRPort_Weapon/?.lua').as_posix()+';'+lua.globals().package.path
    lua.execute('''
      local m=require('ordinary_melee')
      local function v(x,y,z,w) return {x=x,y=y,z=z,w=w or 1} end
      Vector4={new=v}
      local epoch,tracked,queries,hits=0,-.1,0,0
      local axis=v(0,0,1,0)
      Game={VRWeaponInLeftHand=function()return false end,GetVRSharedSlot=function()return 0 end,
        GetSystemRequestsHandler=function()return {IsGamePaused=function()return false end}end,
        VRMeleeInputFrame=function()return {v(epoch*20,epoch,0),axis,v(tracked,0,0),axis,v(0,0,0,1)}end}
      local w={VRMeleeReach=function()return .75 end,VRBladeKind=function()return 2 end,GetWorldForward=function()return v(0,1,0) end}
      local sounds=0
      local p={VRMeleeWhoosh=function() sounds=sounds+1 end, VRMeleeSweepHit=function(_,w,a,b,oldA,oldB,r,passive,motion,begin,strong,speed)
        if passive then return 0 end
        queries=queries+1
        if oldA.x<0 and a.x>0 then
            assert(oldB.x==oldA.x and b.x==a.x and begin and math.abs(speed-5)<.001)
            hits=hits+1;return 1
        end
        return 0
      end}
      local d={ticks=0}
      local function tick(x) m.update(.02,p,w,v(x,0,0),axis,v(0,0,0),d) end
      for i=1,55 do epoch=i;tick(-.1) end
      tracked=.1;epoch=epoch+1;tick(-.1)
      assert((d.fullSwingStarts or 0)==0 and sounds==0 and queries==0,'raw wrist crossed, virtual blade stopped')
      tick(.1);tick(.1)
      assert(d.fullSwingStarts==1 and sounds==1 and hits==1 and queries==1 and d.strokeDispatches==1,
             'missed or repeated the solved crossing within one XR epoch')
      tick(.1);tick(-.1);assert(queries==1,'stale XR stroke stayed live')
    ''')

def check_equipment_restore():
    lua=LuaRuntime()
    lua.globals().package.path=(ROOT/'mods/cet/CyberpunkVRPort_Weapon/?.lua').as_posix()+';'+lua.globals().package.path
    lua.execute('''
        local combat=require('cyberware')
        local ready=false; local reads=0
        local player={VRCyberwareKind=function() reads=reads+1;return 0 end}
        local sword={GetEntityID=function()return {hash=9} end,GetItemID=function()return 9 end,
            VRPhysicalWeaponRecordReady=function()return ready end,VRBladeKind=function()return 2 end}
        local other={GetEntityID=function()return {hash=10} end}
        WeaponObject={IsMelee=function()return false end}
        combat.kind(sword,player);assert(combat.inputKind()==0 and reads==0)
        ready=true;combat.kind(sword,player);assert(combat.inputKind()==5 and reads==1)
        combat.peekKind(other,player);assert(combat.inputKind()==5)
        combat.kind(sword,player);assert(combat.inputKind()==5 and reads==2)
    ''')
    print('PASS late equipment record and independent offhand classification')

def check_recordings(directory):
    """Replay recorded endpoints and XR translation. Observe calls, never return a hit."""
    for name in ('swing','reverse','arc','thrust'):
        recording=json.loads((directory/(name+'.json')).read_text(encoding='utf-8'))
        frames=recording['contact']
        assert frames[0]['shapeDepth']==0 and any(f['shapeDepth']>0 for f in frames), name
        lua=LuaRuntime()
        lua.globals().package.path=(ROOT/'mods/cet/CyberpunkVRPort_Weapon/?.lua').as_posix()+';'+lua.globals().package.path
        tick=lua.execute('''
          local melee=require('ordinary_melee')
          local function v(x,y,z,w) return {x=x,y=y,z=z,w=w or 1} end
          local diag={ticks=0};local sample;local eligible=false
          Vector4={new=v}
          Game={VRWeaponInLeftHand=function()return false end,GetVRSharedSlot=function()return 0 end,
            GetSystemRequestsHandler=function()return {IsGamePaused=function()return false end}end,
            VRMeleeInputFrame=function()return sample end}
          local owner={VRMeleeWhoosh=function()end,VRMeleeSweepHit=function(_,w,a,b,oldA,oldB,r,passive)
            eligible=not passive;return 0 end}
          return function(dt,stamp,epoch,px,py,pz,tx,ty,tz,hx,hy,hz,qx,qy,qz,qw,ox,oy,oz,ex,ey,ez)
            sample={v(stamp,epoch,0),v(0,0,0),v(hx,hy,hz),v(0,0,0),v(qx,qy,qz,qw)}
            local x,y,z=tx-px,ty-py,tz-pz
            local reach=math.sqrt(x*x+y*y+z*z)
            local w={VRMeleeReach=function()return reach end,VRBladeKind=function()return 2 end,
              GetWorldForward=function()return ex and v(ex,ey,ez,0) end}
            eligible=false
            melee.update(dt,owner,w,v(px,py,pz),v(x/reach,y/reach,z/reach,0),v(ox,oy,oz),diag)
            return eligible
          end
        ''')
        # Older reports stored the input rotation in the simulator stream.
        # Reuse it only when the observed controller position matches a sent pose.
        def key(point): return tuple(round(x,5) for x in point)
        rotations={key(p[1:4]):p[4:8] for p in recording['path']}
        def frame(dt,stamp,epoch,f):
            q=f.get('handRotation') or rotations.get(key(f['hand']))
            assert q, (name,'no recorded controller orientation',f['hand'])
            return tick(dt,stamp,epoch,*f['base'],*f['tip'],*f['hand'],*q,
                        *f.get('origin',(0,0,0)),*f.get('edgeWorld',()))
        for i in range(55):
            frame(.02,i*20,i,frames[0])
        first_overlap=next(i for i,f in enumerate(frames) if f['shapeDepth']>0)
        last_time=0
        eligible=[]
        for i,f in enumerate(frames):
            sent=frame(f['at']-last_time,1100+round(f['at']*1000),f['epoch'],f)
            if sent and f['shapeDepth']>0: eligible.append(i)
            last_time=f['at']
        assert first_overlap in eligible, (name,first_overlap,eligible)
        print(f'{name}: active virtual sweep at first recorded occupied sample {first_overlap} '
              f'({frames[first_overlap]["at"]:.3f}s); engine delivery untested')

if __name__=='__main__':
    parser=argparse.ArgumentParser()
    parser.add_argument('--recordings',type=Path,help='read clear-start controller fixture JSON from this directory')
    options=parser.parse_args()
    if options.recordings: check_recordings(options.recordings)
    check_equipment_restore()
    check_contact_stroke()
    check_delayed_virtual_sweep()
    for hz in (20,30,45,60,90,120,144):
        for case in ('wrist','roll','arc','jitter','short','below_sound','reversal'):
            r=replay(hz,case)
            expected={'wrist':(0,0),'roll':(0,0),'arc':(1,1),'jitter':(0,0),'short':(1,1),'below_sound':(1,0),'reversal':(2,2)}[case]
            assert (r['starts'],r['sounds'])==expected,(hz,case,r)
            if case=='short':assert 0<r['latency']<=max(40,1000/hz),(hz,case,r)
            print(hz,case,r)
