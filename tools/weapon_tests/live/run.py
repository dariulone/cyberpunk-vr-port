"""Attach-only weapon audit. Requires the root's exclusive runtime lease.

Never launches, kills, restarts, or requests a save. Fixture mutations require
a causally verified root save guard and the engine's saving-blocked check. A failure
leaves the restoration snapshot available until restoration is verified.
All controller poses below are fixed literals; no pose/trajectory generation.
"""
import argparse
import ctypes
from ctypes import wintypes
import importlib.util
import json
import re
import struct
import subprocess
import sys
import threading
import time
import traceback
import uuid
from pathlib import Path

ROOT=Path(__file__).resolve().parents[3]
sys.path.insert(0,str(ROOT/'tools/sim_harness'))
from simxr import Sim, SIM_DIR, _send, _modules, _exports

# The shared attach protocol already preserves a timed-out request and never
# restarts its game. Importing the module does not construct a Probe or attach.
spec=importlib.util.spec_from_file_location('corpse_attach_protocol',ROOT/'tools/grasp_tests/corpse_live/run.py')
protocol=importlib.util.module_from_spec(spec);spec.loader.exec_module(protocol)
Bridge=protocol.Bridge

def literal(value):
    return json.dumps(value,ensure_ascii=False)

class Audit:
    def __init__(self,args):
        self.args=args;self.out=args.out.resolve();self.out.mkdir(parents=True,exist_ok=False)
        self.plugin=args.game/'red4ext/plugins/cyberpunkvrport'
        self.bridge=Bridge(self.plugin/'lua/CVRBridge')
        self.sim=None;self.loaded=False;self.begun=False;self.recording=False;self.config={};self.report={'phases':[],'errors':[]}
        self.save_lock_name='cpvr_weapon_audit_'+uuid.uuid4().hex
        self.save_lock_confirmed=False;self.save_lock_released=False
    def call(self,code):return self.bridge.call(code)
    def execute(self,code):return self.bridge.call(code,'exec')
    def save(self,name,value):
        (self.out/(name+'.json')).write_text(json.dumps(value,indent=2,ensure_ascii=False),encoding='utf-8')
    def wait(self,condition,seconds=8):
        end=time.monotonic()+seconds;last=None
        while time.monotonic()<end:
            last=self.call('WeaponAudit.sample()')
            if condition(last):return last
            time.sleep(.05)
        raise RuntimeError('State condition timed out; last='+str(last))
    def wait_save_lock(self,removing=False):
        end=time.monotonic()+8;last=None
        while time.monotonic()<end:
            last=self.call('WeaponAudit.saveLockState()')
            protected=last['protectedByVerifiedRoot'] or last.get('protectedByNativeBootLock',False)
            ready=protected and (last['ownRemoveRequested'] if removing else last['ownAddRequested'])
            if ready:
                return last
            time.sleep(.05)
        raise RuntimeError('Verified root save protection unavailable: '+str(last))
    def begin(self):
        if 'optics' in self.cases and not self.args.generator.is_file():
            raise RuntimeError('Build render_audit_math before the optical fixture')
        if not self.call('WeaponAudit == nil'):raise RuntimeError('Previous audit snapshot must be restored first')
        if self.call('Game.GetPlayer()==nil'):raise RuntimeError('No player')
        if self.call('Game.GetBlackboardSystem():Get(GetAllBlackboardDefs().UI_System):GetBool(GetAllBlackboardDefs().UI_System.IsInMenu)'):
            raise RuntimeError('Close native menus before weapon audit')
        self.execute(Path(__file__).with_name('bridge.lua').read_text(encoding='utf-8'));self.loaded=True
        self.execute('WeaponAudit.acquireSaveLock('+literal(self.save_lock_name)+')')
        self.save('save-lock-acquired',self.wait_save_lock())
        self.save_lock_confirmed=True
        self.original=self.call('WeaponAudit.begin()');self.save('original',self.original)
        if getattr(self.args,'expected_inventory',None) is not None and len(self.original['inventory'])!=self.args.expected_inventory:
            raise RuntimeError('Wrong save baseline: inventory count '+str(len(self.original['inventory'])))
        if getattr(self.args,'expected_money',None) is not None and self.original['money']!=self.args.expected_money:
            raise RuntimeError('Wrong save baseline: money '+str(self.original['money']))
        self.sim=Sim(self.args.pid)
        if self.sim.status().get('recording'):raise RuntimeError('Another simulator capture is active')
        status=SIM_DIR/'runtime_status.json'
        if time.time()-status.stat().st_mtime>5:raise RuntimeError('Simulator head status is stale')
        head=json.loads(status.read_text())['head_tracking'];p=head['position']
        self.head=dict(x=p['x'],y=p['y'],z=p['z'],yaw=head['yaw'],pitch=head['pitch'],roll=head['roll'])
        self.save('simulator-original',dict(pid=self.args.pid,head=self.head,controllers=[v.hex() for v in self.sim.saved],
            body=[v.hex() for v in self.sim.saved_body] if self.sim.body_state else None))
        mod=next(m for m in _modules(self.args.pid) if m[0].lower()=='cyberpunkvrport.dll')
        self.exports={k:mod[1]+v for k,v in _exports(mod[2]).items()}
        self.diag=self.sim._read(self.exports['CyberpunkVR_RuntimeDiagnostics'],4)
        self.sim._write(self.exports['CyberpunkVR_RuntimeDiagnostics'],struct.pack('<i',1))
        for name in ('config/vrport.ini','lua/CyberpunkVRPort_Holster/holster_places.json','lua/CyberpunkVRPort_TacticalBelt/belt.cfg'):
            path=self.plugin/name;data=path.read_bytes() if path.exists() else None;self.config[path]=data
            if data is not None:(self.out/(path.name+'.before')).write_bytes(data)
        self.begun=True
        self.neutral()
        self.sim.hands_follow_head(False)
        # No object/actor spawns. Firing poses point both barrels up, away from a
        # normal street line of fire; root must validate the location before GO.
        self.sim.hand('left',pos=(-.22,-.2,-.4),quat=(.7071067812,0,0,.7071067812),mode='body')
        self.sim.hand('right',pos=(.22,-.2,-.4),quat=(.7071067812,0,0,.7071067812),mode='body')
        self.execute('WeaponAudit.holster()');self.wait(lambda s:not s['left'] and not s['right'])
        self.execute('WeaponAudit.unequipSlots()')
        if any(case in self.cases for case in ('same','different')):self.execute('WeaponAudit.perkReady()')
        time.sleep(.5)
        records=('Items.Preset_Nue_Default','Items.Preset_Nue_Default','Items.Preset_Overture_Default',
                 'Items.Preset_Osprey_Default','Items.GrenadeFragRegular')
        needed=5 if 'belt' in self.cases else 4 if 'optics' in self.cases else 3 if 'different' in self.cases else 2 if 'same' in self.cases else 1
        for i,record in enumerate(records[:needed],1):
            self.save('created-'+str(i),self.call('WeaponAudit.create('+literal(record)+','+str(i)+')'))
        self.set_ini({'xr_reload_glow':'1','xr_body_hands_only':'0','xr_immersive_holsters':'1','xr_framegen_overlay':'0'})
    def neutral(self):
        for side in ('left','right'):
            self.sim.trigger(side,0);self.sim.grip(side,0);self.sim.stick(side,0,0)
    def set_ini(self,settings):
        path=self.plugin/'config/vrport.ini';text=path.read_bytes()
        for key,value in settings.items():
            pattern=rb'(?m)^'+key.encode()+rb'=[^\r\n]*'
            line=(key+'='+value).encode()
            text=re.sub(pattern,lambda _:line,text) if re.search(pattern,text) else text+b'\r\n'+line+b'\r\n'
        path.write_bytes(text)
    def loadout(self,right,left=None):
        self.neutral();self.execute('WeaponAudit.holster()');self.wait(lambda s:not s['left'] and not s['right'])
        self.execute('WeaponAudit.unequipSlots()');time.sleep(.5)
        self.execute('WeaponAudit.loadout('+str(right)+(','+str(left) if left else '')+')');time.sleep(.7)
        self.execute('WeaponAudit.draw('+str(right)+')')
        self.wait(lambda s: bool(s['left']) and bool(s['right']) if left else bool(s['left']) != bool(s['right']))
        self.execute('WeaponAudit.fill()');time.sleep(.7)
        return self.call('WeaponAudit.sample()')
    def native(self):
        read=self.sim._read;ex=self.exports
        return dict(shot=struct.unpack('<4f',read(ex['CyberpunkVR_DebugDualShot'],16)),
            mine=struct.unpack('<Q',read(ex['CyberpunkVR_DebugPhysicalRayMine'],8))[0],
            left=struct.unpack('<Q',read(ex['CyberpunkVR_DebugPhysicalRayLeftGun'],8))[0])
    def optics_geometry(self,name,scene=False,controller_position=None):
        sample=self.call('WeaponAudit.opticsGeometry()')
        sample['binding']=json.loads(Path(__file__).with_name('osprey-binding.json').read_text(encoding='utf-8'))
        if controller_position is not None:sample['controllerPosition']=controller_position
        def measure(suffix):
            path=self.out/(name+suffix+'-geometry-input.json');path.write_text(json.dumps(sample),encoding='utf-8')
            done=subprocess.run([str(self.args.generator),'optics',str(path)],capture_output=True,text=True,check=True)
            result=json.loads(done.stdout);self.save(name+suffix+'-geometry',result);return result
        result=measure('')
        if scene:
            sample['staticHit']=self.call('WeaponAudit.opticsRay('+protocol.lua_literal(result['ray']['a'])+','+protocol.lua_literal(result['ray']['b'])+')')
            result=measure('-target')
        return result
    def optics_probe(self,name,component):
        def read(field):return struct.unpack('<I',self.sim._read(self.exports['CyberpunkVR_OpticsProbe'+field],4))[0]
        if read('State') in (1,2):raise RuntimeError('Another optics readback is pending')
        if not self.call('WeaponAudit.opticsProbe('+literal(component)+')'):raise RuntimeError('Native optics probe refused '+component)
        deadline=time.monotonic()+8
        while read('State') in (1,2) and time.monotonic()<deadline:time.sleep(.03)
        if read('State')!=3:raise RuntimeError('Native optics readback did not complete: '+str(read('State')))
        width,height,fmt=read('Width'),read('Height'),read('Format')
        if not 0<width<=4096 or not 0<height<=4096 or fmt not in (9,10,27,28,29):raise RuntimeError('Unexpected optics readback layout')
        pointer=struct.unpack('<Q',self.sim._read(self.exports['CyberpunkVR_OpticsProbePixels'],8))[0]
        path=self.out/(name+'.bin');path.write_bytes(self.sim._read(pointer,width*height*(8 if fmt in (9,10) else 4)))
        self.save(name,dict(width=width,height=height,format=fmt,component=component,raw=str(path),scope='Native UI texture pixels, not final optical-surface shader output'))
    def hinge_counts(self):
        address=self.exports.get('CyberpunkVR_OpticsHingeStats')
        if address is None:raise RuntimeError('The owner-qualified hinge native is not deployed')
        stage_address=self.exports.get('CyberpunkVR_OpticsHingeStages')
        if stage_address is not None:
            names=('none','disabled','context','player','signature','query','activeMismatch','barrel','control','graph','binding',
                   'accepted','ownerScope','descriptor','currentBinding','contextMeta','sourceRig','track','remap','conflict','applied','expired')
            counts=struct.unpack('<22Q',self.sim._read(stage_address,176))
            last=struct.unpack('<I',self.sim._read(self.exports['CyberpunkVR_OpticsHingeLastStage'],4))[0]
            number=getattr(self,'hinge_stage_snapshots',0);self.hinge_stage_snapshots=number+1
            self.save('hinge-stages-'+str(number),dict(counts=dict(zip(names,counts)),lastStage=last,
                lastName=names[last] if last<len(names) else 'invalid',scope='independent cumulative counters, not a seqlocked frame'))
        return list(struct.unpack('<5Q',self.sim._read(address,40)))
    def dual(self,name,left):
        initial=self.loadout(1,left);self.save(name+'-equipped',initial)
        # Outside the native ability the existing press-only semi-auto behavior
        # must survive: one held trigger causes exactly one real ray/shot.
        hold_before=self.native();self.sim.trigger('right',1);time.sleep(.65)
        self.sim.trigger('right',0);time.sleep(.3)
        hold_after=self.native();hold_state=self.call('WeaponAudit.sample()')
        ordinary=dict(name=name+'-ordinary-hold',shots=hold_after['mine']-hold_before['mine'],
            frenzy=hold_state['frenzy'],pass_=hold_after['mine']-hold_before['mine']==1 and not hold_state['frenzy'])
        self.report['phases'].append(ordinary)
        if not ordinary['pass_']:raise AssertionError('Ordinary semi-auto changed: '+str(ordinary))
        self.execute('WeaponAudit.fill()');time.sleep(.7)
        self.sim.record_start(str(self.out/(name+'.mp4')),eyes='both',eye_size=1080,fps=30,max_seconds=8)
        self.recording=True
        stop=threading.Event();native=[];states=[];errors=[];start=time.monotonic()
        def native_poll():
            while not stop.is_set():
                try:native.append(dict(t=time.monotonic()-start,**self.native()))
                except Exception as e:errors.append(str(e));break
                stop.wait(.005)
        def state_poll():
            while not stop.is_set():
                try:states.append(dict(t=time.monotonic()-start,state=self.call('WeaponAudit.sample()')))
                except Exception as e:errors.append(str(e));break
                stop.wait(.04)
        threads=[threading.Thread(target=native_poll),threading.Thread(target=state_poll)]
        for thread in threads:thread.start()
        # Two full squeezes per hand in 390 ms, then both completely released.
        schedule=[(0,'right',1),(.08,'right',0),(.13,'left',1),(.21,'left',0),
                  (.26,'right',1),(.34,'right',0),(.39,'left',1),(.47,'left',0)]
        sent=[]
        try:
            for at,side,value in schedule:
                while time.monotonic()-start<at:time.sleep(.001)
                self.sim.trigger(side,value);sent.append(dict(t=time.monotonic()-start,side=side,value=value))
            released=time.monotonic()-start;time.sleep(2.6)
        finally:
            self.neutral();stop.set()
            for thread in threads:thread.join(20)
            if any(t.is_alive() for t in threads):errors.append('observation thread still pending')
            self.sim.record_stop();self.recording=False
        self.save(name,dict(initial=initial,input=sent,released=released,native=native,states=states,errors=errors))
        # The last manual press can reach the game a frame after release. Leave
        # a 100ms margin, but do not gate on a later bridge observation of state 5:
        # its first forced right-hand shot can precede that observation.
        force_start=next((row['t'] for row in states if row['state']['frenzy'] and row['state']['shoot'] in (5,6)),float('inf'))
        qualified=released+.10
        seen=set();last=None;before_auto=None;after_auto=None
        for row in native:
            if row['t']<qualified:before_auto=row
            else:after_auto=row
            if row['t']>=qualified and row['mine']!=last:
                if row['shot'][1]==4:seen.add(int(row['shot'][0]))
            last=row['mine']
        ability=any(row['state']['frenzy'] and row['state']['shoot'] in (4,5,6) for row in states)
        automatic=(after_auto['mine']-before_auto['mine']) if before_auto and after_auto else 0
        automatic_left=(after_auto['left']-before_auto['left']) if before_auto and after_auto else 0
        automatic_right=automatic-automatic_left
        result=dict(name=name,ability=ability,postReleaseIdentitySides=sorted(seen),automaticShots=automatic,
            automaticLeft=automatic_left,automaticRight=automatic_right,nativeForceStateAt=force_start,
            pass_=ability and seen=={0,1} and automatic_left>0 and automatic_right>0 and not errors)
        self.report['phases'].append(result)
        if not result['pass_']:raise AssertionError('Native Hurricane/identity not proved: '+str(result))
        self.wait(lambda s:not s['frenzy'])
    def shot(self,name):
        files=self.sim.shot(str(self.out/name),eyes='both',eye_size=1600,fmt='jpg')
        self.save(name+'-files',files);return files
    def glow(self):
        self.loadout(1);self.sim.hand('right',pos=(.18,-.25,-.42),quat=(0,0,0,1),mode='body')
        self.execute('WeaponAudit.dry()')
        self.wait(lambda s:any(v['enabled'] for v in s['glow']))
        self.sim.record_start(str(self.out/'glow-holster.mp4'),eyes='both',eye_size=1080,fps=30,max_seconds=12)
        self.recording=True;time.sleep(3.0)
        self.shot('glow-before-holster');rows=[]
        self.execute('WeaponAudit.holster()');start=time.monotonic()
        while time.monotonic()-start<1.5:
            rows.append(dict(t=time.monotonic()-start,state=self.call('WeaponAudit.sample()')))
            time.sleep(.02)
        self.save('glow-holster-trace',rows);self.shot('glow-after-holster')
        self.sim.record_stop();self.recording=False
        empty=next((r for r in rows if not r['state']['left'] and not r['state']['right']),None)
        gone=next((r for r in rows if empty and r['t']>=empty['t'] and not any(g['enabled'] for g in r['state']['glow'])),None)
        result=dict(name='reload-glow',emptyAt=empty and empty['t'],goneAt=gone and gone['t'],
            ticksAfterEmpty=(gone['state']['tick']-empty['state']['tick']) if gone and empty else None,
            pass_=bool(empty and gone and gone['t']-empty['t']<.15))
        self.report['phases'].append(result)
        if not result['pass_']:raise AssertionError('Reload glow persisted: '+str(result))
    def optics(self):
        if self.original['leftHanded']:
            raise RuntimeError('This fixed Osprey fixture requires the reviewed right-handed baseline')
        self.loadout(4)
        hinge_before=self.hinge_counts();self.save('hinge-before',hinge_before)
        # Legacy mode follows the head's orientation. A body-mode identity
        # quaternion kept the bore sideways when the previous fixture turned its HMD.
        self.sim.hands_follow_head(True)
        self.sim.hand('right',pos=(.25,-.50,-.40),ypr=(0,0,0),mode='legacy')
        self.sim.hand('left',pos=(-.23,-.3,-.36),ypr=(0,0,0),mode='legacy')
        target=None
        for index,yaw in enumerate(dict.fromkeys((self.args.scope_yaw,0,60,-60,120,-120,180))):
            self.sim.head(yaw=yaw,pitch=0);time.sleep(.6)
            candidate=self.optics_geometry('scene-'+str(index),scene=True)
            if candidate['distantStaticTarget']:
                target=dict(yaw=yaw,geometry=candidate);break
        self.save('osprey-target-selection',target)
        if target is None:raise RuntimeError('No static target at least 20m away in bounded yaw sweep; relocate scene before retry')
        self.shot('osprey-target-wide')
        # Osprey's authored weapon-to-hand grip has an offset. Calibrate from
        # actual bore measurements using fixed controller orientations; never
        # substitute the controller axis or relax the 2-degree optical gate.
        orientations=[]
        for index,ypr in enumerate(((0,0,0),(11,-6,0),(-11,-6,0),(11,6,0),(-11,6,0))):
            self.sim.hand('right',pos=(.06,-.14,-.25),ypr=ypr,mode='legacy');time.sleep(.5)
            orientations.append(dict(ypr=ypr,geometry=self.optics_geometry('orientation-'+str(index))))
        self.save('osprey-orientations',orientations)
        orientation=min(orientations,key=lambda r:r['geometry']['opticAxisAngleDegrees'])
        if orientation['geometry']['opticAxisAngleDegrees']>2:
            raise RuntimeError('Fixed controller orientation candidates did not align the actual weapon bore')
        ypr=orientation['ypr']
        position=(.06,-.18,-.25)
        observations=[]
        for index in range(3):
            self.sim.hand('right',pos=position,ypr=ypr,mode='legacy');time.sleep(.5)
            geometry=self.optics_geometry('pose-'+str(index),controller_position=position)
            observations.append(dict(position=position,state=self.call('WeaponAudit.optics()'),geometry=geometry))
            if geometry['aligned']:break
            position=geometry['nextControllerPosition']
        self.save('osprey-fixed-positions',observations)
        eligible=[r for r in observations if r['geometry']['aligned'] and isinstance(r['state'].get('eyeToAxis'),(int,float))]
        if not eligible:raise RuntimeError('No measured optic alignment: require bore<=2deg, eye-axis<=15mm, relief6–30cm; no fallback pose')
        chosen=min(eligible,key=lambda r:r['geometry']['bestAxisDistanceMeters'])
        self.sim.hand('right',pos=chosen['position'],ypr=ypr,mode='legacy');time.sleep(.6)
        aim='right' if self.original['leftHanded'] else 'left'
        self.sim.record_start(str(self.out/'osprey-hip-ads-hip.mp4'),eyes='both',eye_size=1080,fps=30,max_seconds=20);self.recording=True
        try:
            hip_geometry=self.optics_geometry('osprey-hip',scene=True)
            hip=self.call('WeaponAudit.optics()');self.save('osprey-hip-state',hip);self.shot('osprey-hip')
            self.sim.trigger(aim,1);time.sleep(.8)
            self.wait(lambda s:s['upper']==6,seconds=3)
            ads_geometry=self.optics_geometry('osprey-ads',scene=True)
            ads=self.call('WeaponAudit.optics()');self.save('osprey-ads-state',ads);self.shot('osprey-ads')
            hinge_ads=self.hinge_counts();self.save('hinge-ads',hinge_ads)
            if not ads.get('hingeRequested') or hinge_ads[2]<=hinge_before[2]:
                raise RuntimeError('Native hinge owner request/apply was not proved: '+str(hinge_ads))
            if self.args.optics_probe:
                self.save('osprey-panel-tree',self.call('WeaponAudit.opticsPanels()'))
                self.optics_probe('osprey-atlas-camera','vr_optic_panel')
                self.optics_probe('osprey-atlas-reticle','vr_optic_reticle_rear_panel')
            self.sim.trigger(aim,0);time.sleep(.6)
            self.wait(lambda s:s['upper']!=6,seconds=3)
            back_geometry=self.optics_geometry('osprey-back-hip',scene=True);self.shot('osprey-back-hip')
            hinge_back=self.hinge_counts();time.sleep(.25);hinge_idle=self.hinge_counts()
            self.save('hinge-released',dict(first=hinge_back,after=hinge_idle))
            if hinge_idle[2]!=hinge_back[2]:raise RuntimeError('Hinge override continued after native ADS exit')
        finally:
            self.sim.trigger(aim,0);self.sim.record_stop();self.recording=False
        if not all(g['aligned'] and g['distantStaticTarget'] for g in (hip_geometry,ads_geometry,back_geometry)):
            raise RuntimeError('Optic axis/relief or distant-target check failed across hip/ADS/hip')
        def glass(state):return next((c for c in state['components'] if c['name']=='w_rifle_sniper__nokota_osprey__base1_3648'),None)
        a,b=glass(hip),glass(ads)
        cap_names=('w_rifle_sniper__nokota_osprey__base1_7354',
                   'w_rifle_sniper__nokota_osprey__base1_02_decal_01_d6247',
                   'w_rifle_sniper__nokota_osprey__base1_02_decal_01_n4054')
        caps=[]
        for state in (hip,ads):
            lookup={c['name']:c for c in state['components']}
            caps.append(all(name in lookup and not lookup[name]['enabled'] for name in cap_names))
        result=dict(name='osprey-physical-glass',capsHidden=caps,adsUpperBody=ads['state']['upper'],
            pass_=bool(a and b and a['enabled'] and b['enabled'] and all(caps) and ads['state']['upper']==6),visualReviewRequired=True)
        self.report['phases'].append(result)
        if not result['pass_']:raise AssertionError('Osprey glass not visible in both states')
    def belt(self):
        self.loadout(1);self.execute('WeaponAudit.holster()');self.wait(lambda s:not s['left'] and not s['right'])
        self.set_ini({'xr_hybrid_body_rotation':'0','xr_body_free_look_deg':'180.000'})
        self.execute("WeaponAudit.grenade(5);GetMod('CyberpunkVRPort_Holster').setHidden(false);GetMod('CyberpunkVRPort_Holster').setPlace(0,'hip_r')")
        self.sim.hand('right',pos=(.65,-.2,-.1),quat=(0,0,0,1),mode='body')
        self.sim.hand('left',pos=(-.65,-.2,-.1),quat=(0,0,0,1),mode='body');time.sleep(1)
        self.save('belt-state',self.call('WeaponAudit.belt()'))
        self.execute('Game.GetPlayer():GetFPPCameraComponent():SetLocalPosition(WeaponAudit.original.cameraPosition)')
        for name,yaw,pitch in (('belt-down',0,-75),('hip-down-right',-20,-65),('belt-down-left',20,-70)):
            self.sim.head(yaw=yaw,pitch=pitch);time.sleep(.8);self.shot(name)
        self.report['phases'].append(dict(name='belt-placement',visualReviewRequired=True))
    def cleanup(self):
        errors=[]
        if self.sim:
            try:
                self.neutral()
                if self.recording:self.sim.record_stop();self.recording=False
            except Exception as e:errors.append('neutral: '+str(e))
        if self.begun:
            try:
                self.execute('WeaponAudit.restoreSlots()');time.sleep(1)
                self.execute('WeaponAudit.restoreDraw()');time.sleep(1)
                self.execute('WeaponAudit.restoreFinish()');time.sleep(.7)
                self.execute('WeaponAudit.restoreAmmo()');time.sleep(.8)
            except Exception as e:errors.append('game restoration: '+str(e))
        if self.sim:
            try:
                self.sim.restore()
                if hasattr(self,'head'):_send('head_pose_command.json',self.head)
                if hasattr(self,'diag'):self.sim._write(self.exports['CyberpunkVR_RuntimeDiagnostics'],self.diag)
            except Exception as e:errors.append('simulator/diagnostic restoration: '+str(e))
        for path,data in self.config.items():
            try:
                if data is None:path.unlink(missing_ok=True)
                else:path.write_bytes(data)
            except Exception as e:errors.append('settings restoration: '+str(e))
        if self.begun:
            try:
                state=self.call('WeaponAudit.restored()');self.save('restored',state)
                if not state['ok']:errors.append('restoration mismatch: '+str(state['differences']))
            except Exception as e:errors.append('restoration verification: '+str(e))
        if self.loaded and not errors:
            try:
                self.execute('Game.GetAutoSaveSystem():InvalidateAutoSaveRequests()')
                time.sleep(.4)
                if self.begun:
                    drained=self.call('WeaponAudit.restored()');self.save('restored-after-drain',drained)
                    if not drained['ok']:raise RuntimeError('Restoration changed while draining: '+str(drained['differences']))
                self.execute('WeaponAudit.releaseSaveLock()')
                time.sleep(.4) # allow the exact native REMOVE request to be processed
                self.save('save-lock-own-remove-requested',self.wait_save_lock(removing=True))
                self.save_lock_released=True
                self.execute('Game.GetAutoSaveSystem():InvalidateAutoSaveRequests();WeaponAudit=nil')
            except Exception as e:errors.append('own save-lock release: '+str(e))
        if self.sim:
            close=ctypes.WinDLL('kernel32').CloseHandle;close.argtypes=[wintypes.HANDLE];close(self.sim.h)
        self.save('cleanup',dict(errors=errors,gameLeftRunning=True,saveLockName=self.save_lock_name,
            saveLockStatus='own-remove-queued-root-protected' if self.save_lock_released else 'root-protected-own-add-queued' if self.save_lock_confirmed else 'unverified-request' if self.loaded else 'not-requested'));return errors
    def run(self):
        self.cases=self.args.case or ['same','different','glow','optics','belt']
        try:
            self.begin()
            if 'same' in self.cases:self.dual('same-nue-pair',2)
            if 'different' in self.cases:self.dual('nue-overture-pair',3)
            if 'glow' in self.cases:self.glow()
            if 'optics' in self.cases:self.optics()
            if 'belt' in self.cases:self.belt()
        except Exception as e:self.report['errors'].append(dict(error=str(e),traceback=traceback.format_exc()))
        finally:
            self.report['errors'].extend(self.cleanup());self.save('report',self.report)
            print(json.dumps(dict(report=str(self.out/'report.json'),errors=self.report['errors'],gameLeftRunning=True)),flush=True)
        return not self.report['errors']

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--pid',required=True,type=int)
    parser.add_argument('--out',required=True,type=Path)
    parser.add_argument('--game',type=Path,default=Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077'))
    parser.add_argument('--root-exclusive-lease',action='store_true',help='Use only after explicit root GO and location review')
    parser.add_argument('--case',action='append',choices=['same','different','glow','optics','belt'])
    parser.add_argument('--expected-inventory',type=int,help='Fail before mutations if the loaded save has a different inventory count')
    parser.add_argument('--expected-money',type=int,help='Fail before mutations if the loaded save has different money')
    parser.add_argument('--scope-yaw',type=float,default=0,help='First yaw in bounded distant-static-target search; not assumed visually valid')
    parser.add_argument('--generator',type=Path,default=ROOT/'build/render-runtime-audit/Release/render_audit_math.exe')
    parser.add_argument('--optics-probe',action='store_true',help='Read back the owned optic atlas and widget tree after ADS; needs explicit exclusive live lease')
    args=parser.parse_args()
    if not args.root_exclusive_lease:parser.error('Root GO/exclusive runtime lease is required; no attachment performed')
    return 0 if Audit(args).run() else 1
if __name__=='__main__':sys.exit(main())
