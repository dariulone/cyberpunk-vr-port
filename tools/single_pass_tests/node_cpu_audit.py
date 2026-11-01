"""Per-node CPU recording cost of both views in an owned live launch (no overlay interaction).

Turns CyberpunkVR_ProfEnable on, requests one CyberpunkVR_ProfDumpNodes to start a clean window,
waits, requests a second dump, then parses that dump with tools/node_audit_md.py. Self time per
view-frame is the only rankable column (inclusive double-counts the scene driver's children).
This answers which second-view nodes carry its CPU cost -- the candidates for CPU sharing.
"""
import argparse
import json
from pathlib import Path
import sys
import time

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path[:0] = [str(ROOT / 'tools/quickboot_tests'), str(ROOT / 'tools/roomscale_tests'),
                str(ROOT / 'tools/swimming_tests'), str(ROOT / 'tools'), str(HERE)]
NAMES = ('CyberpunkVR_ProfEnable', 'CyberpunkVR_ProfDumpRequest', 'CyberpunkVR_ProfDumpCount')


def rows_from_dump(lines):
    import node_audit_md as audit
    cfg, header, nodes, passes, pairs = audit.parse_dump(lines)
    if not nodes:
        raise ValueError('Dump has no node rows')
    fm, _ = audit.infer_frames(nodes, 'calls_main')
    fv, _ = audit.infer_frames(nodes, 'calls_vrcam')
    if fm <= 0 or fv <= 0:
        raise ValueError(f'Could not infer frames main={fm} vrcam={fv}')
    out = []
    for row in nodes:
        out.append(dict(rva=row['rva'], name=row['name'], callsMainPerFrame=row['calls_main'] / fm,
                        callsVrcamPerFrame=row['calls_vrcam'] / fv, selfMainMs=row['self_main'] / fm,
                        selfVrcamMs=row['self_vrcam'] / fv))
    out.sort(key=lambda r: -(r['selfMainMs'] + r['selfVrcamMs']))
    window = float(header.get('window_ms', 0) or 0)
    return dict(framesMain=fm, framesVrcam=fv, windowMs=window,
                totalSelfMainMs=sum(r['selfMainMs'] for r in out), totalSelfVrcamMs=sum(r['selfVrcamMs'] for r in out),
                config=cfg, rows=out)


def last_dump(log_path, after_offset):
    import node_audit_md as audit
    with open(log_path, encoding='utf-8', errors='replace') as handle:
        handle.seek(after_offset)
        lines = [line for line in handle if '[prof] ' in line]
    dumps = audit.split_dumps(lines)
    if not dumps:
        raise ValueError('No profiler dump after the request')
    return dumps[-1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--state', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--seconds', type=float, default=6)
    args = parser.parse_args()
    import struct
    import geometry_cost_live as live
    import geometry_cost_report as cost
    import launch_to_save as boot
    from live_read import Reader
    from live_symbols import resolve
    state = live.load_state(args.state)
    pid = state['pid']
    game = boot.DEFAULT_GAME
    log_path = game / 'red4ext/plugins/cyberpunkvrport/log/cyberpunkvrport.log'
    check = live.checker(pid, state['started'], state['baselineReporters'])
    symbols = resolve(pid, game / live.DLL, list(cost.NAMES) + list(NAMES))
    reader = Reader(pid, game / 'bin/x64/Cyberpunk2077.exe', 0, 0)
    probe = cost.LiveProbe(reader, symbols, pid, check)
    original = probe.read32('CyberpunkVR_ProfEnable')

    def dump():
        count = probe.read32('CyberpunkVR_ProfDumpCount')
        probe.write32('CyberpunkVR_ProfDumpRequest', 1)
        deadline = time.monotonic() + 5
        while probe.read32('CyberpunkVR_ProfDumpCount') == count:
            check()
            if time.monotonic() > deadline:
                raise RuntimeError('Profiler dump was not serviced')
            time.sleep(.05)

    try:
        probe.write32('CyberpunkVR_ProfEnable', 1)
        time.sleep(1)
        dump()
        offset = log_path.stat().st_size
        time.sleep(args.seconds)
        dump()
        time.sleep(.5)
        value = rows_from_dump(last_dump(log_path, offset))
    finally:
        probe.write32('CyberpunkVR_ProfDumpRequest', 0)
        probe.write32('CyberpunkVR_ProfEnable', original)
        restored = probe.read32('CyberpunkVR_ProfEnable')
        probe.restore()
        reader.close()
    value.update(pid=pid, restoredProfEnable=restored)
    args.out.write_text(json.dumps(value, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({k: value[k] for k in ('framesMain', 'framesVrcam', 'windowMs', 'totalSelfMainMs', 'totalSelfVrcamMs')}, indent=1))
    for row in value['rows'][:30]:
        print(f"{row['selfMainMs']:7.3f} {row['selfVrcamMs']:7.3f}  x{row['callsMainPerFrame']:.1f}/{row['callsVrcamPerFrame']:.1f}  {row['rva']} {row['name']}")


if __name__ == '__main__':
    main()
