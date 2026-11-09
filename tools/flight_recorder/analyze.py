#!/usr/bin/env python3
"""Analyze recorder schema 1; never substitute submitted FPS for display delivery."""
import argparse
from collections import Counter
import json
from pathlib import Path
import sys


def read(path):
    rows=[]
    lines=Path(path).read_text(encoding='utf-8-sig').splitlines()
    for i,line in enumerate(lines,1):
        if not line.strip(): continue
        try: row=json.loads(line)
        except json.JSONDecodeError:
            if i==len(lines):
                print(f'Warning: ignoring incomplete final record in {path}',file=sys.stderr)
                break
            raise ValueError(f'{path}:{i}: invalid JSON')
        if row.get('type')=='header':
            if row.get('schema')!=1: raise ValueError('Unsupported recorder schema')
        elif row.get('type') in ('sample','stop') and row.get('window_s',0)>0: rows.append(row)
    return rows


def summarize(rows):
    seconds=sum(r['window_s'] for r in rows)
    counts=Counter()
    for r in rows: counts.update(r.get('events',{}))
    def rate(k): return counts[k]/seconds if seconds else 0
    valid=[r for r in rows if r.get('compositor',{}).get('estimate_fps') is not None]
    valid_s=sum(r['window_s'] for r in valid)
    est=sum(r['compositor']['estimate_fps']*r['window_s'] for r in valid)/valid_s if valid_s else None
    timings={}
    for name in {k for r in rows for k in r.get('timings',{})}:
        values=[r['timings'][name] for r in rows if r.get('timings',{}).get(name,{}).get('n',0)]
        n=sum(v['n'] for v in values)
        timings[name]={'n':n,'mean':sum(v['mean']*v['n'] for v in values)/n if n else None,'max':max((v['max'] for v in values),default=None)}
    comp={k:sum(r.get('compositor',{}).get(k,0) for r in rows) for k in ('records','presented_records','zero_present_records','history_holes','dropped_reports','mispresented_reports','extra_presents')}
    return {'seconds':seconds,'capture_fps':rate('capture'),'accepted_real_fps':rate('accepted_real'),
        'accepted_synthetic_fps':rate('accepted_synthetic'),'app_repeat_fps':rate('accepted_repeat')+rate('fallback_repeat'),
        'compositor_estimate_fps':est,'compositor_coverage_seconds':valid_s,'counts':dict(counts),'timings':timings,'compositor':comp}


def segments(rows):
    groups=[]
    for r in rows:
        # A transition window can straddle modes; retain it separately rather than
        # charging all its work to whichever backend happens to be active last.
        mixed=r.get('settings_changed',False) or r.get('events',{}).get('settings_change',0)>0
        key=(json.dumps(r.get('settings',{}),sort_keys=True),r.get('effective_backend'),bool(mixed))
        if not groups or groups[-1][0]!=key: groups.append((key,[r]))
        else: groups[-1][1].append(r)
    return groups


def display(rows):
    print('Rates are application capture/submission rates. Compositor FPS is an estimate, not unique-content FPS.\n')
    print('| Phase | Seconds | Mode | Capture/s | Real accepted/s | FG accepted/s | App repeats/s | Compositor estimate |')
    print('|---|---:|---|---:|---:|---:|---:|---:|')
    for i,(key,part) in enumerate(segments(rows),1):
        s=summarize(part);settings=part[-1].get('settings',{});mode=f"backend {key[1]} / FG {'on' if settings.get('enabled') else 'off'}"
        if key[2]:mode+=' (transition)'
        est=s['compositor_estimate_fps'];es=f'{est:.1f}' if est is not None else 'unavailable'
        print(f"| {i} | {s['seconds']:.1f} | {mode} | {s['capture_fps']:.1f} | {s['accepted_real_fps']:.1f} | {s['accepted_synthetic_fps']:.1f} | {s['app_repeat_fps']:.1f} | {es} |")
        print(f"\nPhase {i} counters: "+', '.join(f'{k}={v}' for k,v in s['counts'].items() if v and k not in ('capture','cycle','input_left','input_right','accepted_real','accepted_synthetic')))
        for k in ('generation_gpu_ms','generation_span_ms','pacing_budget_ms','capture_wait_ms','xr_end_ms','xr_wait_ms','source_age_ms','submit_hold_ms','submit_probe_ms','real_remaining_ms','synthetic_remaining_ms','repeat_remaining_ms','submit_margin_ms','phase_step_abs_ms','real_handoff_gap_ms','synthetic_handoff_gap_ms','repeat_handoff_gap_ms','real_xr_end_ms','synthetic_xr_end_ms','repeat_xr_end_ms','prepare_before_hold_ms'):
            t=s['timings'].get(k,{})
            if t.get('n'): print(f"  {k}: mean {t['mean']:.2f}, max {t['max']:.2f}, n={t['n']}")
        print('  Settled compositor records:',s['compositor'])
        print('  Display estimate coverage:',round(s['compositor_coverage_seconds'],2),'seconds\n')
    print('Interpretation: pair loss/re-priming, generator busy, allocator busy, and pacing growth are separate signals. High accepted rates with poor compositor delivery point downstream of generation. Dropped/mispresented/extra-present fields overlap: do not sum them into a loss total.')

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('log',type=Path);p.add_argument('--json',action='store_true');a=p.parse_args()
    rows=read(a.log)
    if a.json: print(json.dumps([{'settings':g[-1].get('settings',{}),'transition':key[2],**summarize(g)} for key,g in segments(rows)],indent=2))
    else: display(rows)
