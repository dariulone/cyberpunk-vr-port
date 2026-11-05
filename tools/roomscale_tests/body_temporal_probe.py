"""Read-only dense body/camera telemetry with bounded simulator or native-input drive."""
import argparse
import json
import math
import os
from pathlib import Path
import struct
import subprocess
import sys
import threading
import time

from live_read import Reader
from simulator_probe import read_status, send, await_pose


def bone_model_positions(raw, parents, indices):
    # Same local->model convention as VRIK_ComputeFK; independent of the
    # published g_VRBodyBone positions being checked. Scales are retained in
    # the capture as a validation bound, but VRIK itself does not apply them.
    cache={}
    def rotate(q,v):
        x,y,z,w=q;vx,vy,vz=v
        tx,ty,tz=2*(y*vz-z*vy),2*(z*vx-x*vz),2*(x*vy-y*vx)
        return [vx+w*tx+y*tz-z*ty,vy+w*ty+z*tx-x*tz,vz+w*tz+x*ty-y*tx]
    def multiply(a,b):
        x,y,z,w=a;xx,yy,zz,ww=b
        q=[w*xx+x*ww+y*zz-z*yy,w*yy-x*zz+y*ww+z*xx,
           w*zz+x*yy-y*xx+z*ww,w*ww-x*xx-y*yy-z*zz]
        n=math.sqrt(sum(v*v for v in q))
        if n<1e-6:raise ValueError('Invalid bone quaternion')
        return [v/n for v in q]
    def get(index):
        if index in cache:return cache[index]
        pos=list(struct.unpack_from('<3f',raw,index*48))
        q=struct.unpack_from('<4f',raw,index*48+16);q=multiply([0,0,0,1],q)
        parent=parents[index]
        if 0<=parent<index:
            pp,pq=get(parent);pos=[a+b for a,b in zip(pp,rotate(pq,pos))];q=multiply(pq,q)
        cache[index]=(pos,q);return pos,q
    result={name:get(index)[0] for name,index in indices.items()}
    result['max_scale_error']=max(abs(v-1) for index in cache for v in struct.unpack_from('<3f',raw,index*48+32))
    return result


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--addresses',type=Path,required=True)
    parser.add_argument('--out',type=Path,required=True)
    parser.add_argument('--mode',choices=('hmd','wasd','idle'),required=True)
    parser.add_argument('--distance',type=float,default=.12)
    parser.add_argument('--duration',type=float,default=1.5)
    parser.add_argument('--axis',choices=('x','y','z'),default='x')
    parser.add_argument('--direction',type=int,choices=(-1,1),default=1)
    parser.add_argument('--native-seconds',type=float,default=.12)
    args=parser.parse_args()
    if args.out.exists() or not args.out.parent.is_dir():raise ValueError('Fresh output under existing parent required')
    if not .02<=args.distance<=1.0 or not .3<=args.duration<=3:raise ValueError('Unbounded probe: max1m, min0.3s')
    if not .03<=args.native_seconds<=.5:raise ValueError('Native pulse must be0.03..0.5s')
    cfg=json.loads(args.addresses.read_text(encoding='utf-8'))
    pid=cfg['pid']; addresses={k:int(v,16) for k,v in cfg.items() if isinstance(v,str) and v.startswith('0x')}
    game=Path('C:/Program Files (x86)/Steam/steamapps/common/Cyberpunk 2077')
    reader=Reader(pid,game/'bin/x64/Cyberpunk2077.exe',addresses['debug'],addresses['debug_seq'])
    directory=Path(os.environ['LOCALAPPDATA'])/'OpenXR-Simulator'
    initial=read_status(directory); head=initial['head_tracking']
    original={**head['position'],**{k:head[k] for k in ('yaw','pitch','roll')}}
    current=dict(original); rows=[]; failures=[]; stop=threading.Event(); phase=['baseline']
    result={'pid':pid,'mode':args.mode,'distance':args.distance,'duration':args.duration,
            'axis':args.axis,'direction':args.direction,
            'native_seconds':args.native_seconds,'addresses':cfg,'initial_status':initial,
            'original':original,'samples':rows,'read_failures':failures,
            'boundary':'Each atomic packet has its own seqlock. Cross-packet/body reads are asynchronous observations; stable epoch/counter/body checks are recorded and not asserted to be one engine invocation.'}
    def read(name,size):return reader.read(addresses[name],size)
    def u32(name):return struct.unpack('<I',read(name,4))[0]
    def packet(name,size):
        for _ in range(10):
            before=u32(name+'_seq')
            if not before or before&1:continue
            raw=read(name,size)
            if u32(name+'_seq')==before:return before,raw
        raise RuntimeError(name+' packet raced')
    def vector(name,offset=0,fmt='<3f'):
        return struct.unpack(fmt,reader.read(addresses[name]+offset,struct.calcsize(fmt)))
    def get_sample():
        t=time.perf_counter_ns(); epoch=u32('epoch'); used=vector('used_counters',fmt='<2Q')
        row=reader.sample(); row.update(perf_ns=t,phase=phase[0])
        with_base=cfg.get('body_base_packets',False)
        for name,size in (('native',64 if with_base else 52),('lua',68 if with_base else 56)):
            sequence,raw=packet(name,size)
            q=struct.unpack_from('<8f',raw); span=struct.unpack_from('<3f',raw,32)
            row[name]={'seq':sequence,'cam_q':q[:4],'entity_q':q[4:],'span':span,
                       'valid':struct.unpack_from('<I',raw,44)[0]}
            if name=='native':row[name]['epoch']=struct.unpack_from('<I',raw,48)[0]
            else:row[name]['ms']=struct.unpack_from('<Q',raw,48)[0]
            if with_base:row[name]['body_span']=struct.unpack_from('<3f',raw,52 if name=='native' else 56)
        seq,raw=packet('located',52 if with_base else 40)
        row['located']={'seq':seq,'pos':struct.unpack_from('<3f',raw),'q':struct.unpack_from('<4f',raw,12),
                         'frame':struct.unpack_from('<I',raw,28)[0],'epoch':struct.unpack_from('<I',raw,32)[0]}
        if with_base:row['located']['body_base']=struct.unpack_from('<3f',raw,40)
        row['head_delta']=[v/131072 for v in vector('head_delta',fmt='<3i')]
        body_raw=read('body',132); body=struct.unpack('<33f',body_raw)
        row['hips']=body[:3];row['head']=body[12:15]
        row['cam_model']=vector('cam_model')
        row['entity_pos']=vector('entity_pos');row['entity_quat']=vector('entity_quat',fmt='<4f')
        row['main']=[x/131072 for x in vector('main',0xe0,'<3i')]
        row['vrcam']=[x/131072 for x in vector('vrcam',0xe0,'<3i')]
        shared_raw=read('shared',1024)
        shared=struct.unpack('<256f',shared_raw)
        row['hand_packet']={'left':shared[1:4],'right':shared[9:12],'hmd_q':shared[16:20],
            'stamp_ms':shared[67],
            'view_q':shared[104:108],'view_delta':shared[108:112],
            'coherent_anchor':shared[112:116],'head_base':shared[124:127],
            'sequence':struct.unpack_from('<I',shared_raw,127*4)[0],
            'stable':shared_raw==read('shared',1024)}
        row['eye_bake']=shared[116:119];row['hand_head_base']=shared[124:127];row['hand_delta']=shared[108:111]
        row['calibration_bake']=shared[91:94];row['bake_candidate']=shared[85:89]
        if 'bone_buffer_cell' in addresses:
            indices={name:struct.unpack('<i',read('bone_'+name+'_index',4))[0]
                     for name in ('head','hips','right_hand','left_hand')}
            if any(i<0 or i>=2048 for i in indices.values()):raise ValueError('Invalid player bone index')
            count=max(indices.values())+1
            pointer=read('bone_buffer_cell',8);bone_ptr=struct.unpack('<Q',pointer)[0]
            local=reader.read(bone_ptr,count*48);parents=struct.unpack('<'+'h'*count,read('bone_parents',count*2))
            row['bone_buffer']={'pointer':hex(bone_ptr),'indices':indices,
                'if_local':bone_model_positions(local,parents,indices),
                'positions':{name:struct.unpack_from('<3f',local,index*48) for name,index in indices.items()},
                'rotations':{name:struct.unpack_from('<4f',local,index*48+16) for name,index in indices.items()},
                'stable':pointer==read('bone_buffer_cell',8) and local==reader.read(bone_ptr,count*48)}
        if 'placed_pose_counters' in addresses:
            row['placed_pose_counters']=vector('placed_pose_counters',fmt='<3Q')
        if 'hand_publish_counters' in addresses:
            row['hand_publish_counters']=vector('hand_publish_counters',fmt='<3Q')
        if 'placed_history' in addresses:
            history_head=read('placed_history_head',8)
            history=read('placed_history',64*88)
            head=struct.unpack('<Q',history_head)[0]
            recent=[]
            for identity in range(max(0,head-8),head):
                offset=(identity%64)*88
                if struct.unpack_from('<Q',history,offset+72)[0]!=identity:continue
                recent.append({'id':identity,'view':struct.unpack_from('<I',history,offset+80)[0],
                    'q':struct.unpack_from('<4f',history,offset),
                    'world_fp':struct.unpack_from('<3i',history,offset+16),
                    'head_pos':struct.unpack_from('<3f',history,offset+32),
                    'head_q':struct.unpack_from('<4f',history,offset+44),
                    'origin':struct.unpack_from('<Q',history,offset+64)[0]})
            row['placed_history']={'head':head,'recent':recent,
                'stable':history_head==read('placed_history_head',8) and history==read('placed_history',64*88)}
        if 'cam_write_ring' in addresses:
            ring_head=read('cam_write_head',8); ring=read('cam_write_ring',16*72)
            entries=[]
            for i in range(16):
                offset=i*72
                if not struct.unpack_from('<I',ring,offset+64)[0]:continue
                entries.append({'q':struct.unpack_from('<4f',ring,offset),
                    'position':struct.unpack_from('<3f',ring,offset+16),
                    'id':struct.unpack_from('<Q',ring,offset+56)[0]})
            final=reader.read(addresses['telemetry']+72,16)
            render_ptr=struct.unpack_from('<Q',final,8)[0]
            render=reader.read(render_ptr,32)
            row['render_observation']={'pointer':hex(render_ptr),
                'hits':struct.unpack_from('<I',final)[0],
                'position_fp':struct.unpack_from('<3i',render),
                'q':struct.unpack_from('<4f',render,16),
                'ring_head':struct.unpack('<Q',ring_head)[0],'entries':entries,
                'stable':ring_head==read('cam_write_head',8) and
                    ring==read('cam_write_ring',16*72) and
                    final==reader.read(addresses['telemetry']+72,16) and
                    render==reader.read(render_ptr,32)}
        if 'raw_roomscale' in addresses:
            raw=read('raw_roomscale',40); pose=read('frame_pose',40)
            row['raw_roomscale']={'head':struct.unpack_from('<2f',raw),'seq':struct.unpack_from('<Q',raw,8)[0],
                'origin':struct.unpack_from('<Q',raw,16)[0],'stamp_us':struct.unpack_from('<Q',raw,24)[0],'valid':bool(raw[32])}
            row['frame_pose']={'position':struct.unpack_from('<3f',pose),'valid':bool(pose[28]),
                'origin':struct.unpack_from('<Q',pose,32)[0],'epoch':struct.unpack('<Q',read('frame_pose_epoch',8))[0]}
            if 'frame_pose_sequence' in addresses:
                row['frame_pose']['sequence']=struct.unpack('<Q',read('frame_pose_sequence',8))[0]
            if 'frame_pose_stamp' in addresses:
                row['frame_pose']['stamp_us']=struct.unpack('<Q',read('frame_pose_stamp',8))[0]
            row['stable_pose_reads']=raw==read('raw_roomscale',40) and pose==read('frame_pose',40)
        row['stable_body']=body_raw==read('body',132)
        row['epoch']=epoch;row['used']=used
        row['stable_epoch']=epoch==u32('epoch')
        row['stable_used']=used==vector('used_counters',fmt='<2Q')
        row['read_ns']=time.perf_counter_ns()-t
        return row
    def collect():
        while not stop.is_set():
            try:rows.append(get_sample())
            except BaseException as exc:failures.append(str(exc))
            stop.wait(.002)
    thread=threading.Thread(target=collect,daemon=True)
    def hold(name,seconds):phase[0]=name;time.sleep(seconds)
    def sweep(goal,name):
        nonlocal current
        phase[0]=name; before=dict(current);start=time.monotonic()
        while True:
            t=min(1,(time.monotonic()-start)/args.duration);u=t*t*(3-2*t)
            current={k:before[k]+(goal[k]-before[k])*u for k in before}
            send(directory,current)
            if t>=1:break
            time.sleep(.01)
        await_pose(directory,current)
    helper=Path(__file__).resolve().parents[2]/'render_camera_RE/scripts/native_re_input.py'
    def native(keys=None):
        argv=[sys.executable,'-B',str(helper),'--pid',str(pid),'--exe',str(game/'bin/x64/Cyberpunk2077.exe'),
              '--log',str(args.out.with_suffix('.input.jsonl'))]
        argv+=['--keys',*keys,'--seconds',str(args.native_seconds)] if keys else ['--release-all']
        p=subprocess.run(argv,capture_output=True,text=True,timeout=8)
        result.setdefault('input_helpers',[]).append({'args':argv,'code':p.returncode,'stdout':p.stdout,'stderr':p.stderr})
        if p.returncode:raise RuntimeError('Native helper failed')
    try:
        get_sample();thread.start();hold('baseline',1.0)
        if args.mode=='hmd':
            goal={**original,args.axis:original[args.axis]+args.distance*args.direction}
            sweep(goal,'hmd_out');hold('hmd_hold',.7);sweep(original,'hmd_back');hold('after',1.2)
        elif args.mode=='wasd':
            phase[0]='wasd';native(['D']);hold('after',2)
        else:hold('idle',3)
    except BaseException as exc:result['error']=f'{type(exc).__name__}: {exc}'
    finally:
        try:
            if args.mode=='hmd':send(directory,original);result['final_status']=await_pose(directory,original)
            if args.mode=='wasd':native()
        except BaseException as exc:result['cleanup_error']=str(exc)
        finally:
            stop.set()
            if thread.is_alive():thread.join(2)
            reader.close()
            args.out.write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8')
    print(json.dumps({'pid':pid,'mode':args.mode,'samples':len(rows),'read_failures':len(failures),
        'stable_body':sum(r['stable_body'] for r in rows),'stable_epoch':sum(r['stable_epoch'] for r in rows),
        'output':str(args.out),'error':result.get('error'),'cleanup_error':result.get('cleanup_error')},indent=2))
    if result.get('error') or result.get('cleanup_error'):raise SystemExit(1)


if __name__=='__main__':main()
