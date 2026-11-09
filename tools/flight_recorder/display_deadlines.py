#!/usr/bin/env python3
"""Summarize paired samples, not displayed/unique FPS or exact photon deadlines."""
import argparse
import json
from pathlib import Path
from statistics import mean


def summarize(records):
    out={}
    for kind,name in enumerate(('real','synthetic','repeat')):
        for future in (False,True):
            samples=[p for p in records if p['kind']==kind and (p['remaining_ms']>p['period_ms'])==future]
            if not samples: continue
            offsets=[]
            for p in samples:
                boundary=p['probe_qpc_ms']+p['remaining_ms'];target=boundary-p['margin_ms']
                offsets.append({'gpu_boundary':p['gpu_end_qpc_ms']-boundary,
                    'gpu_handoff':p['gpu_end_qpc_ms']-p['handoff_qpc_ms'],
                    'handoff_target':p['handoff_qpc_ms']-target,
                    'handoff_boundary':p['handoff_qpc_ms']-boundary,
                    'gpu_target':p['gpu_end_qpc_ms']-target,
                    'queue':p['queue_ms']})
            counts={'samples':len(samples),'gpu_late_boundary':sum(v['gpu_boundary']>.25 for v in offsets),
                'gpu_after_handoff':sum(v['gpu_handoff']>.25 for v in offsets),
                'handoff_after_target':sum(v['handoff_target']>.25 for v in offsets),
                'handoff_before_target':sum(v['handoff_target']<-.25 for v in offsets),
                'gpu_late_cpu_early_boundary':sum(v['gpu_boundary']>.25 and v['handoff_boundary']<-.25 for v in offsets),
                'gpu_early_cpu_late_boundary':sum(v['gpu_boundary']<-.25 and v['handoff_boundary']>.25 for v in offsets)}
            counts['mean_ms']={k:mean(v[k] for v in offsets) for k in offsets[0]}
            out[name+('_future_boundary' if future else '_within_period')]=counts
    return out


def read(path,start=None,end=None):
    result=[];origin=None
    for line in Path(path).read_text(encoding='utf-8-sig').splitlines():
        if not line.strip():continue
        row=json.loads(line)
        if row.get('type') not in ('sample','stop'):continue
        if origin is None:origin=row['clock_ms']-row['window_s']*1000
        for p in row.get('display_probes',[]):
            t=(p['handoff_qpc_ms']-origin)/1000
            if start is not None and t<start:continue
            if end is not None and t>=end:continue
            result.append(p)
    return result

if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('log',type=Path)
    p.add_argument('--from-seconds',type=float);p.add_argument('--to-seconds',type=float);a=p.parse_args()
    print(json.dumps({'semantics':'Runtime-reported boundaries can roll early. Target is the pacing setpoint, not compositor cutoff. +/-0.25ms ambiguous. Samples do not prove display/scanout. Positive signed offsets mean after.',
        'groups':summarize(read(a.log,a.from_seconds,a.to_seconds))},indent=2))
