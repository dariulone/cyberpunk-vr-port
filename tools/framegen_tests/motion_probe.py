"""Bounded simulator head-motion test; game memory is read-only, no keys/focus.

Uses the active simulator's documented head_pose_command.json protocol. Restores
the initial HMD pose in finally. Input/camera snapshots are double-read; exported
counters are adjacent asynchronous observations, not a stopped same-call proof.
"""
import argparse
import json
import math
import os
from pathlib import Path
import statistics
import struct
import sys
import time
import uuid

ROOT=Path(__file__).resolve().parents[2]
sys.path[:0]=[str(ROOT/'tools/roomscale_tests'),str(ROOT/'tools/swimming_tests')]
from live_read import Reader
from live_symbols import resolve
from simulator_probe import read_status,await_pose

ARRAYS=['CyberpunkVR_PoseIdComponent','CyberpunkVR_PoseIdFinal','CyberpunkVR_PoseIdFinalMiss',
        'CyberpunkVR_PoseIdImageWrites','CyberpunkVR_PoseIdCaptured','CyberpunkVR_PoseIdCaptureMiss',
        'CyberpunkVR_PoseIdLastCapture','CyberpunkVR_PoseIdLastSubmit','CyberpunkVR_PoseIdImageGeneration']
COUNTERS=['CyberpunkVR_PoseIdCopied','CyberpunkVR_PoseIdCopyChanged','CyberpunkVR_PoseIdVrikInputChanged',
          'CyberpunkVR_DebugVrcamEyePaired','CyberpunkVR_DebugVrcamEyeUnpaired','CyberpunkVR_DebugVrcamEyeReused',
          'CyberpunkVR_DebugXrEndFailed','CyberpunkVR_DebugXrWaitFailed','CyberpunkVR_DebugCapSkipNoView',
          'CyberpunkVR_DebugCapSkipNoRes','CyberpunkVR_DebugCapSkipNoSlot','CyberpunkVR_DebugCapSkipFence',
          'CyberpunkVR_DebugCapSkipReset']

def camera(data,offset=0):
    floats=struct.unpack_from('<20f',data,offset)
    return dict(position=floats[4:7],up=floats[7:10],right=floats[10:13],forward=floats[13:16],
                jitter=floats[:2],motion_scale=floats[2:4],near=floats[16],far=floats[17],
                fov=floats[18],aspect=floats[19],flags=list(data[offset+80:offset+86]))

def distribution(values):
    if not values:return None
    s=sorted(values)
    return dict(min=s[0],median=statistics.median(s),p99=s[min(len(s)-1,math.ceil(.99*len(s))-1)],max=s[-1])

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--pid',type=int,required=True)
    p.add_argument('--backend',type=int,choices=[0,1],required=True)
    p.add_argument('--out',type=Path,required=True)
    p.add_argument('--dll',type=Path,default=ROOT/'build/bin/red4ext/plugins/cyberpunkvrport/Release/cyberpunkvrport.dll')
    p.add_argument('--directory',type=Path,default=Path(os.environ['LOCALAPPDATA'])/'OpenXR-Simulator')
    a=p.parse_args()
    if a.out.exists() or not a.out.parent.is_dir():raise RuntimeError('Fresh output under an existing directory required')
    if time.time()-(a.directory/'runtime_status.json').stat().st_mtime>3:raise RuntimeError('Simulator is not publishing fresh status')
    if (a.directory/'head_pose_command.json').exists():raise RuntimeError('Simulator head command channel is busy')
    initial=read_status(a.directory);head=initial['head_tracking']
    original={**head['position'],**{k:head[k] for k in ['yaw','pitch','roll']}}
    if any(abs(original[k])>.001 for k in ['yaw','pitch','roll']):raise RuntimeError('Neutral initial angles required for this reproducible test')
    names=ARRAYS+COUNTERS+['CyberpunkVR_FramegenReport','CyberpunkVR_FramegenReportSeq','CyberpunkVR_FramegenInputStages','CyberpunkVR_RuntimeDiagnostics',
          'CyberpunkVR_RoomscaleDebug','CyberpunkVR_RoomscaleDebugSeq','CyberpunkVR_MainIsRightEye',
          'cvr::framegen::cameras','cvr::framegen::pools']
    symbols=resolve(a.pid,a.dll.resolve(),names)
    addresses={k:int(symbols[k],16) for k in names}
    exe=Path('C:/Program Files (x86)/Steam/steamapps/common/Cyberpunk 2077/bin/x64/Cyberpunk2077.exe')
    reader=Reader(a.pid,exe,addresses['CyberpunkVR_RoomscaleDebug'],addresses['CyberpunkVR_RoomscaleDebugSeq'])
    if not struct.unpack('<i',reader.read(addresses['CyberpunkVR_RuntimeDiagnostics'],4))[0]:
        reader.close()
        raise RuntimeError('Runtime diagnostics are disabled; enable DEBUG or CyberpunkVR_RuntimeDiagnostics before this probe')
    rows=[];unstable=0;started=time.monotonic();current=dict(original);commands=0
    result=dict(pid=a.pid,dll_sha256=symbols['sha256'],backend=a.backend,original=original,initial_status=initial,
                memory_written=False,keys_sent=False,focus_changed=False,samples=rows)

    def read(name,size):return reader.read(addresses[name],size)
    def stable(address,size):
        nonlocal unstable
        for _ in range(3):
            first=reader.read(address,size)
            if first==reader.read(address,size):return first
        unstable+=1;return None
    def send(pose):
        nonlocal commands
        target=a.directory/'head_pose_command.json'
        temp=a.directory/('head_pose_command.'+uuid.uuid4().hex+'.writing')
        temp.write_text(json.dumps(pose),encoding='ascii')
        deadline=time.monotonic()+3
        while True:
            try:os.replace(temp,target);break
            except PermissionError:
                if time.monotonic()>deadline:raise RuntimeError('Simulator channel locked')
                time.sleep(.001)
        while target.exists():
            if time.monotonic()>deadline:raise RuntimeError('Simulator did not consume head pose')
            time.sleep(.001)
        commands+=1
    def report():
        for _ in range(30):
            seq=struct.unpack('<I',read('CyberpunkVR_FramegenReportSeq',4))[0]
            if seq&1:continue
            data=read('CyberpunkVR_FramegenReport',2048).split(b'\0',1)[0]
            if seq==struct.unpack('<I',read('CyberpunkVR_FramegenReportSeq',4))[0]:return json.loads(data)
        raise RuntimeError('Busy framegen report')
    def sample(phase):
        state=report()
        if not state.get('metrics',1):raise RuntimeError('Enable FPS overlay before a metrics probe')
        if not state['enabled'] or state['backend']!=a.backend:raise RuntimeError('Generation or backend changed during trajectory')
        room=reader.sample()
        if room['gates']!=15:raise RuntimeError('Gameplay/roomscale gate changed')
        if rows and (room['origin']!=rows[0]['roomscale']['origin'] or math.dist(room['after'],rows[0]['roomscale']['after'])>.10):
            raise RuntimeError('Origin changed or physical travel exceeded 10cm bound')
        row=dict(t=time.monotonic()-started,phase=phase,commanded=dict(current),framegen=state,
                 stages=struct.unpack('<10Q',read('CyberpunkVR_FramegenInputStages',80)),roomscale=room)
        row['counters']={name:struct.unpack('<Q',read(name,8))[0] for name in COUNTERS}
        row['arrays']={name:struct.unpack('<2Q',read(name,16)) for name in ARRAYS}
        # Camera constants ABI: CameraData88B, frame+valid8B, pose+origin16B.
        raw=stable(addresses['cvr::framegen::cameras'],2*8*112)
        cams=[]
        if raw:
            for side in range(2):
                for index in range(8):
                    offset=(side*8+index)*112
                    if not raw[offset+92]:continue
                    c=camera(raw,offset);c.update(view=side+1,frame=struct.unpack_from('<I',raw,offset+88)[0],
                        pose=hex(struct.unpack_from('<Q',raw,offset+96)[0]),origin=struct.unpack_from('<Q',raw,offset+104)[0])
                    if not .01<c['fov']<3.13 or c['aspect']<=0:raise RuntimeError('Camera ABI/values invalid')
                    cams.append(c)
        row['cameras']=cams
        paired=[]
        by_frame={}
        for c in cams:by_frame.setdefault(c['frame'],{})[c['view']]=c
        for frame,views in by_frame.items():
            if len(views)!=2:continue
            x,y=views[1],views[2]
            dot=sum(a*b for a,b in zip(x['forward'],y['forward']))
            lengths=math.sqrt(sum(v*v for v in x['forward'])*sum(v*v for v in y['forward']))
            paired.append(dict(frame=frame,ipd=math.dist(x['position'],y['position']),
                orientation_gap_deg=math.degrees(math.acos(max(-1,min(1,dot/lengths)))),origin_match=x['origin']==y['origin']))
        row['paired_cameras']=paired
        # Input pools contain6 shared_ptr<Record> per view. Validate only records
        # that are unchanged across two reads and have finished publication.
        pools=stable(addresses['cvr::framegen::pools'],192);inputs=[]
        if pools:
            for side in range(2):
                for index in range(6):
                    ptr=struct.unpack_from('<Q',pools,(side*6+index)*16)[0]
                    if not ptr:continue
                    try:raw=stable(ptr,200)
                    except OSError:unstable+=1;continue
                    if not raw:continue
                    if raw[194] or not raw[178]:continue
                    frame,view,w,h,mw,mh=struct.unpack_from('<6I',raw,152)
                    if view!=side+1 or not(16<=w<=16384 and 16<=h<=16384 and 16<=mw<=16384 and 16<=mh<=16384):
                        raise RuntimeError('Input pool ABI/size invalid')
                    item=dict(view=view,frame=frame,pose=hex(struct.unpack_from('<Q',raw,120)[0]),
                              origin=struct.unpack_from('<Q',raw,128)[0],depth=bool(raw[176]),motion=bool(raw[177]),
                              evaluated=bool(raw[179]),expected=raw[192],submitted=raw[193],size=[w,h],motion_size=[mw,mh],
                              camera=camera(raw,32))
                    inputs.append(item)
        row['inputs']=inputs
        rows.append(row)
        return row
    def hold(phase,seconds):
        until=time.monotonic()+seconds
        while time.monotonic()<until:sample(phase);time.sleep(.02)
    def move(phase,seconds,yaw,pitch,roll,cycles):
        nonlocal current
        begin=time.monotonic();next_tick=begin;next_sample=begin
        while True:
            elapsed=time.monotonic()-begin;u=min(1,elapsed/seconds);envelope=math.sin(math.pi*u)**2
            current=dict(original)
            current['yaw']+=math.radians(yaw)*math.sin(2*math.pi*cycles*u)*envelope
            current['pitch']+=math.radians(pitch)*math.sin(4*math.pi*u)*envelope
            current['roll']+=math.radians(roll)*math.sin(6*math.pi*u)*envelope
            current['x']+=envelope*(.0015*math.sin(2*math.pi*2.9*elapsed)+.0005*math.sin(2*math.pi*5.3*elapsed))
            current['y']+=envelope*.001*math.sin(2*math.pi*4.1*elapsed)
            current['z']+=envelope*.0015*math.sin(2*math.pi*3.7*elapsed)
            send(current)
            if time.monotonic()>=next_sample or u>=1:
                sample(phase);next_sample=time.monotonic()+1/30
            if u>=1:break
            next_tick+=1/90;time.sleep(max(0,min(.02,next_tick-time.monotonic())))
        await_pose(a.directory,current)
    error=None
    try:
        hold('baseline',2)
        if not rows[-1]['inputs'] or len(rows[-1]['paired_cameras'])<2:raise RuntimeError('No valid input/camera snapshot before movement')
        move('slow_turn_with_jitter',10,30,10,3,1)
        hold('slow_settled',2)
        move('fast_turn_with_jitter',8,45,15,5,3)
        hold('fast_settled',2)
    except BaseException as exc:
        error=f'{type(exc).__name__}: {exc}';result['error']=error
    finally:
        try:
            send(original);current=dict(original);result['restored']=await_pose(a.directory,original)
            hold('restored',1)
        except BaseException as exc:result['restore_error']=f'{type(exc).__name__}: {exc}'
        reader.close()
        result.update(commands=commands,unstable_reads=unstable,elapsed=time.monotonic()-started)
        if rows:
            result['counter_deltas']={name:rows[-1]['counters'][name]-rows[0]['counters'][name] for name in COUNTERS}
            result['array_deltas']={name:[b-a for a,b in zip(rows[0]['arrays'][name],rows[-1]['arrays'][name])] for name in ARRAYS if 'Last' not in name and 'Generation' not in name}
            result['phases']={}
            for phase in sorted(set(r['phase'] for r in rows)):
                batch=[r for r in rows if r['phase']==phase]
                result['phases'][phase]={key:distribution([r['framegen'][key] for r in batch]) for key in ['real_fps','fg_fps','output_fps','repeats_fps','generation_ms','vram_bytes']}
                result['phases'][phase].update(samples=len(batch),statuses=sorted(set(r['framegen']['status'] for r in batch)),
                    ipd=distribution([p['ipd'] for r in batch for p in r['paired_cameras']]),
                    orientation_gap_deg=distribution([p['orientation_gap_deg'] for r in batch for p in r['paired_cameras']]))
        a.out.write_text(json.dumps(result,indent=2),encoding='utf-8')
    print(json.dumps({k:v for k,v in result.items() if k!='samples'},indent=2))
    if error or 'restore_error' in result:raise SystemExit(1)

if __name__=='__main__':main()
