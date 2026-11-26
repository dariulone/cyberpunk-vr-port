"""Bounded, attach-only A/B on the existing right-hand rifle and one stationary player."""
import argparse
from contextlib import ExitStack
import ctypes
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import struct
import subprocess
import sys
import time

import numpy as np

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tools/sim_harness'))
from simxr import Sim, SIM_DIR, _exports, _modules

DECODER = ROOT / 'tools/vr_bench/stereo_check.exe'
TARGET = np.array([-.10, .07, .06])  # shoulder to wrist, metres, right / forward / up
START = np.array([.06,-.12,-.14])
CAM = 'Game.GetPlayer():GetFPPCameraComponent()'
GET_CAM = '(function() local p=' + CAM + ':GetLocalPosition(); return {p.x,p.y,p.z} end)()'
WEAPON = """(function()
  local p=assert(Game.GetPlayer())
  local w=assert(p:GetActiveWeapon(),'draw a rifle first')
  local ts=Game.GetTransactionSystem()
  local r=assert(ts:GetItemInSlot(p,TweakDBID.new('AttachmentSlots.WeaponRight')))
  assert(r:GetEntityID().hash==w:GetEntityID().hash and not ts:GetItemInSlot(p,TweakDBID.new('AttachmentSlots.WeaponLeft')))
  return {record=TDBID.ToStringDEBUG(ItemID.GetTDBID(w:GetItemID())),entity=tostring(w:GetEntityID().hash)}
end)()"""


def save(path, data):
    tmp = path.with_suffix('.tmp')
    tmp.write_text(json.dumps(data, indent=2, allow_nan=False), encoding='utf-8')
    tmp.replace(path)


def measure_sample(s, w, e):
    u, f, axis = e-s, w-e, w-s
    if min(np.linalg.norm(u),np.linalg.norm(f),np.linalg.norm(axis))<.01:
        raise ValueError('Degenerate arm')
    pole=u-axis*(u@axis)/(axis@axis)
    if np.linalg.norm(pole)<.01:
        raise ValueError('Straight arm has no measurable pole')
    def elevation(v):
        return float(np.degrees(np.arctan2(v[2],np.linalg.norm(v[:2]))))
    return dict(openingDeg=float(np.degrees(np.arctan2(np.linalg.norm(np.cross(-u,f)), -u@f))),
                upperArmElevationDeg=elevation(u),poleElevationDeg=elevation(pole))


def matched_frame(frames,mode):
    for index in (1,0,2):
        frame=frames[index]
        left,right=frame['eyes']
        if (not left['pose'] or left['pose']!=right['pose'] or
            any(eye.get('elbowMode')!=mode for eye in (left,right)) or
            left.get('rightArmModel')!=right.get('rightArmModel')):
            continue
        joints=np.asarray(left['rightArmModel']).reshape(3,3)
        if np.isfinite(joints).all():
            return index,frame,measure_sample(joints[0],joints[2],joints[1])
    raise RuntimeError('No captured stereo frame matches a completed right-arm pose in this phase')


def place_wrist(observe, journal, persist):
    """Measure d(body-space shoulder-to-wrist)/d(simulator translation) with the observer fixed."""
    def read(pos, kind):
        if np.linalg.norm(pos-START)>1.30:
            raise RuntimeError('Placement exceeded the 1.30 m controller travel envelope')
        actual=np.asarray(observe(pos),dtype=float)
        if actual.shape!=(3,) or not np.isfinite(actual).all():
            raise RuntimeError('Invalid wrist observation')
        error=TARGET-actual
        journal['placements'].append(dict(kind=kind,controller=pos.tolist(),wristRFU=actual.tolist(),
                                         residualRFU=error.tolist(),errorM=float(np.linalg.norm(error))))
        persist()
        return actual

    pos=START.copy()
    value=read(pos,'observer origin')
    if 'originalCameraRFU' in journal:
        journal['observerGuideShiftRFU']=(value-journal['originalCameraRFU']).tolist()
        persist()
    radius=.25
    for attempt in range(2):
        if np.linalg.norm(TARGET-value)<=.010:
            return pos
        columns=[]
        for axis in np.eye(3)*.035:
            plus=read(pos+axis,'probe +')
            minus=read(pos-axis,'probe -')
            columns.append((plus-minus)/.070)
        jac=np.column_stack(columns)
        repeat=read(pos,'probe origin repeat')
        sigma=np.linalg.svd(jac,compute_uv=False)
        journal['jacobians'].append(dict(controller=pos.tolist(),rowsRFU_columnsXYZ=jac.tolist(),
                                         singularValues=sigma.tolist(),originDriftM=float(np.linalg.norm(repeat-value))))
        persist()
        if np.linalg.norm(repeat-value)>.012:
            raise RuntimeError('Wrist guide drifted during Jacobian probes')
        if sigma[-1]<.12 or sigma[0]>3. or sigma[0]/sigma[-1]>12.:
            raise RuntimeError('Wrist guide is constrained or unresponsive; see measured Jacobian')
        value=repeat
        for _ in range(6):
            error=TARGET-value
            step=np.linalg.solve(jac,error)
            trial=pos+step*min(1.,radius/max(np.linalg.norm(step),1e-9))
            measured=read(trial,'correction')
            miss=np.linalg.norm(TARGET-measured)
            if miss<=.010:
                return trial
            if miss>=np.linalg.norm(error)-.003:
                # Return to the best pose and remeasure the mapping once with smaller steps.
                value=read(pos,'rejected correction')
                break
            pos,value=trial,measured
        radius=.12
    raise RuntimeError('Bounded placement failed; last wrist residual '+str((TARGET-value).tolist()))


def run(pid, out):
    if pid <= 0 or not DECODER.is_file():
        raise ValueError('Supply an already running game PID and build tools/vr_bench/stereo_check.exe')
    modules = {m[0].lower(): m for m in _modules(pid)}
    if 'cyberpunk2077.exe' not in modules:
        raise ValueError('PID must be Cyberpunk2077.exe')
    _, base, dll = modules['cyberpunkvrport.dll']
    plugin = Path(dll).parent
    exports = _exports(dll)
    ab, seq, data, request = [base+exports['CyberpunkVR_'+name] for name in
                              ('RifleElbowAB','RifleElbowSeq','RifleElbowSample','FramegenCapture')]
    spec = importlib.util.spec_from_file_location('bridge_protocol', ROOT/'tools/grasp_tests/corpse_live/run.py')
    protocol = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(protocol)
    bridge = protocol.Bridge(plugin/'lua/CVRBridge')
    call = lambda expr: bridge.call(expr,timeout=3)
    def camera(pos):
        args = ','.join(format(float(x),'.9g') for x in pos)
        bridge.call(CAM+':SetLocalPosition(Vector4.new('+args+',1))','exec',timeout=3)
        if np.linalg.norm(np.array(call(GET_CAM))-pos)>.002:
            raise RuntimeError('Camera readback disagrees with requested position')

    out.mkdir(parents=True,exist_ok=False)
    report = dict(pid=pid, dll=dll, sha256=hashlib.sha256(Path(dll).read_bytes()).hexdigest(),
                  targetRFU=TARGET.tolist(), phases={}, complete=False, restored={})
    sim = Sim(pid,focus_away=False)
    def flag(address, value):
        sim._write(address,struct.pack('<i',value))
        if address==ab and sim._i32(address)!=value:
            raise RuntimeError('Elbow A/B mode readback failed')

    def sample(mode,until=float('inf')):
        initial=sim._i32(seq)
        deadline=min(time.monotonic()+1.5,until)
        while time.monotonic()<deadline:
            n=sim._i32(seq)
            if n!=initial and not n&1:
                a=np.array(struct.unpack('<13f',sim._read(data,52)))
                if sim._i32(seq)==n and np.isfinite(a).all() and a[0]==mode and a[12] in (2,4):
                    return a
            time.sleep(.005)
        raise RuntimeError('No fresh right-hand rifle solve in requested mode')

    def window(mode,until=float('inf')):
        samples=np.stack([sample(mode,until) for _ in range(8)])
        if (np.linalg.norm(samples[:,2:5]-TARGET,axis=1).max()>.015 or
            np.ptp(samples[:,2:8],axis=0).max()>.004 or
            (mode==2 and samples[:,1].min()<.9)):
            raise RuntimeError('Input, elbow stability or rifle-pole engagement outside fixture tolerance')
        return samples

    def restore_controls():
        sim.restore()
        report['restored']['controllers']=all(sim._read(c,len(v))==v for c,v in zip(sim.ctrl,sim.saved))
        if sim.body_state:
            report['restored']['body']=all(sim._read(c,len(v))==v for c,v in zip(sim.body_state,sim.saved_body))
        if not all(report['restored'].values()):
            raise RuntimeError('Simulator restore readback failed')

    def restore_camera(pos):
        camera(pos)
        report['restored']['camera']=True

    def restore_mode():
        flag(ab,0)
        report['restored']['solver']=True

    def capture(mode, folder):
        batches=plugin/'log/framegen-capture'
        old=set(batches.iterdir()) if batches.is_dir() else set()
        folder.mkdir()
        first=window(mode)
        flag(request,1)
        try:
            until=time.monotonic()+15
            checks=[first]
            while sim._i32(request)==1 and time.monotonic()<until:
                checks.append(window(mode,until))
            if sim._i32(request)!=2:
                raise RuntimeError('Stereo color/depth capture failed or exceeded 15 seconds')
            checks.append(window(mode))
        finally:
            flag(request,0)
        new=[p for p in batches.iterdir() if p not in old and (p/'capture.json').is_file()]
        if len(new)!=1:
            raise RuntimeError('Expected exactly one new three-frame capture')
        raw=new[0]
        (folder/'raw-path.txt').write_text(str(raw)+'\n',encoding='utf-8')
        # The engine prints C "inf" for an infinite far plane. The stereo decoder accepts it.
        manifest=json.loads(re.sub(r'(?<=[,\[])inf(?=[,\]])','1e30',(raw/'capture.json').read_text()))
        if len(manifest['frames'])!=3:
            raise RuntimeError('Incomplete render capture')
        index,frame,angles=matched_frame(manifest['frames'],mode)
        rows=np.concatenate(checks)
        if np.ptp(rows[:,2:8],axis=0).max()>.004:
            raise RuntimeError('Arm moved during the capture window')
        if np.min(np.abs(rows[:,8:12]@rows[0,8:12]))<.99996:
            raise RuntimeError('Wrist rotated during capture')
        snap=np.median(rows,axis=0)
        joints=np.array(frame['eyes'][0]['rightArmModel']).reshape(3,3)
        if abs(np.linalg.norm(joints[2]-joints[0])-np.linalg.norm(snap[2:5]))>.01:
            raise RuntimeError('Final shoulder-to-wrist distance differs from fixture')
        return dict(raw=str(raw),frame=frame,index=index,finalModelAngles=angles,samples=rows.tolist(),inputRFU=snap[2:5].tolist(),
                    wristQ=snap[8:12].tolist(),solve=measure_sample(np.zeros(3),snap[2:5],snap[5:8]))

    try:
        with ExitStack() as cleanup:
            if sim._i32(ab)!=0 or sim._i32(request)!=0 or sim.status().get('recording'):
                raise RuntimeError('An existing audit or capture is active')
            if time.time()-(SIM_DIR/'runtime_status.json').stat().st_mtime>5:
                raise RuntimeError('Simulator status is stale')
            for ctrl in sim.saved:
                if any(ctrl[i] for i in set(sim.BUTTONS.values())) or max(map(abs,struct.unpack_from('<4f',ctrl,sim.lay['grip']-4)))>.02:
                    raise RuntimeError('Release simulator buttons, triggers and sticks before capture')
            report['weapon']=call(WEAPON)
            original=np.array(call(GET_CAM))
            report['originalCamera']=original.tolist()
            report['originalControllers']=[v.hex() for v in sim.saved]
            save(out/'capture.json',report)
            cleanup.callback(restore_controls)
            cleanup.callback(restore_camera,original)
            cleanup.callback(restore_mode)
            flag(ab,1)
            sim.hand('left',pos=(-.10,-.16,-.40),ypr=(0,0,0),mode='legacy')
            journal=dict(placements=[],jacobians=[])
            def persist():
                save(out/'placement.json',journal)
            def observe(pos):
                sim.hand('right',pos=pos,ypr=(0,0,0),mode='legacy')
                time.sleep(.40)
                observations=np.stack([sample(1)[2:5] for _ in range(3)])
                if np.ptp(observations,axis=0).max()>.005:
                    raise RuntimeError('Moving wrist guide during placement')
                mode,readback,_=sim.pose('right')
                if mode!='legacy' or np.linalg.norm(np.subtract(pos,readback))>.001:
                    raise RuntimeError('Simulator controller command was overwritten')
                return np.median(observations,axis=0)
            # Moving the observer can translate the guide even with identical controller inputs.
            journal['originalCameraRFU']=observe(START).tolist()
            persist()
            camera(original+np.array([.42,-.95,-.18]))
            pos=place_wrist(observe,journal,persist)
            report['rightController']=pos.tolist()
            for mode,label in ((1,'before'),(2,'after')):
                flag(ab,mode)
                time.sleep(1.2)
                phase=capture(mode,out/label)
                if call(WEAPON)!=report['weapon']:
                    raise RuntimeError('Weapon changed during capture')
                report['phases'][label]=phase
                save(out/'capture.json',report)
            a,b=report['phases'].values()
            if np.linalg.norm(np.subtract(a['inputRFU'],b['inputRFU']))>.003 or abs(np.dot(a['wristQ'],b['wristQ']))<.99996:
                raise RuntimeError('A/B wrist position or orientation differs')
            for first,second in zip(a['frame']['eyes'],b['frame']['eyes']):
                for field,tol in [('position',.005),('right',.005),('up',.005),('forward',.005),('fov',.0002)]:
                    if np.linalg.norm(np.subtract(first[field],second[field]))>tol:
                        raise RuntimeError('Camera or projection changed: '+field)
        for label,phase in report['phases'].items():
            subprocess.run([str(DECODER),phase['raw'],str(out/label),'--frame',str(phase['index'])],check=True,timeout=90)
        angles={label:p['finalModelAngles'] for label,p in report['phases'].items()}
        angles['afterMinusBefore']={k:b['finalModelAngles'][k]-a['finalModelAngles'][k] for k in a['finalModelAngles']}
        angles['baselineRaised']=angles['before']['upperArmElevationDeg']>0
        angles['correctedBelow']=angles['after']['upperArmElevationDeg']<0 and angles['after']['poleElevationDeg']<0
        save(out/'angles.json',dict(source='Final player model joints matching both rendered-eye pose IDs',**angles))
        report['complete']=True
    except BaseException as exc:
        report['error']=str(exc)
        raise
    finally:
        ctypes.windll.kernel32.CloseHandle(ctypes.c_void_p(sim.h))
        save(out/'capture.json',report)
    print('Captured both phases. Review the paired landmarks with rifle_elbow_review.py',out)


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--pid',required=True,type=int)
    parser.add_argument('--out',required=True,type=Path)
    args=parser.parse_args()
    run(args.pid,args.out.resolve())
