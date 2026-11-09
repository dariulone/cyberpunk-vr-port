#!/usr/bin/env python3
"""Frame-matched GPU checkpoints; upscaler return is not final postprocess color."""
import argparse,json,math
from pathlib import Path
from statistics import mean

def attribute(s):
    if s['mask']!=63:return None
    ends=[[v['end_qpc_ms'] for v in eye['stages']] for eye in s['eyes']]
    cap=s['capture_start_qpc_ms'];enq=s['capture_enqueue_qpc_ms'];gen=s['generation_start_qpc_ms']
    points=[v for eye in ends for v in eye]+[cap,enq,gen,s['capture_end_qpc_ms']]
    if not all(math.isfinite(v) and v>0 for v in points) or cap<enq or max(points[:6])>cap+.25:return None
    upscale=max(ends[0][2],ends[1][2]);guides=max(ends[0][0:2]+ends[1][0:2]);latest=max(upscale,guides)
    return {'upscale_to_capture_start_ms':cap-upscale,'guides_to_capture_start_ms':cap-guides,
        'upscale_eye_gap_ms':abs(ends[0][2]-ends[1][2]),
        'capture_queue_ms':cap-enq,
        'capture_queue_before_checkpoints_ms':min(cap-enq,max(0,latest-enq)),
        'capture_queue_after_checkpoints_ms':min(cap-enq,max(0,cap-latest)),
        'capture_end_to_generation_start_ms':gen-s['capture_end_qpc_ms'],
        'upscale_to_generation_start_ms':gen-upscale}

def read(path,start=None,end=None):
    data=[];origin=None
    for line in Path(path).read_text(encoding='utf-8-sig').splitlines():
        row=json.loads(line)
        if row.get('type') not in ('sample','stop'):continue
        if origin is None:origin=row['clock_ms']-row['window_s']*1000
        for s in row.get('source_probes',[]):
            t=(s['capture_enqueue_qpc_ms']-origin)/1000
            if start is not None and t<start:continue
            if end is not None and t>=end:continue
            data.append(s)
    return data

def summary(data):
    out={}
    for rr in (False,True):
        samples=[s for s in data if s['rr']==rr];valid=[a for s in samples if (a:=attribute(s)) is not None]
        out['rr_on' if rr else 'rr_off']={'samples':len(samples),'complete_valid':len(valid),
            'partial_or_invalid':len(samples)-len(valid),
            'ms':{k:{'mean':mean(a[k] for a in valid),'max':max(a[k] for a in valid)} for k in valid[0]} if valid else {}}
    return out

if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('log',type=Path);p.add_argument('--from-seconds',type=float);p.add_argument('--to-seconds',type=float);a=p.parse_args()
    print(json.dumps({'semantics':__doc__,'groups':summary(read(a.log,a.from_seconds,a.to_seconds))},indent=2))
