"""GPU time per render-graph node and view in a live owned launch (geometry_cost_live.py launch).

Takes several grouped scene GPU profiles (CyberpunkVR_SceneGpuRequest, one 256-list sample each),
sums each (view, node) group, and normalizes it per frame of that view: the number of frames a
sample spans is the median range count of the once-per-frame nodes
(geometry_cost_report.FRAME_NODES). The result answers which node costs how much in MAIN (side 0,
right eye) and in VRCAM (side 1, left eye) -- the candidates for reuse between the views, and for
skipping. Node names come from src/Stereo/NodeNames.inc.
"""
import argparse
import json
from pathlib import Path
import re
import statistics
import sys
import time

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path[:0] = [str(ROOT / 'tools/quickboot_tests'), str(ROOT / 'tools/roomscale_tests'),
                str(ROOT / 'tools/swimming_tests'), str(HERE)]
NAMES_FILE = ROOT / 'src/Stereo/NodeNames.inc'


def node_names(text=None):
    text = NAMES_FILE.read_text(encoding='utf-8') if text is None else text
    return {int(rva, 16): name for rva, name in re.findall(r'\{\s*(0x[0-9A-Fa-f]+)\s*,\s*"([^"]+)"', text)}


def aggregate(profiles, frame_nodes):
    """Per (side, node): GPU ms and ranges per frame of that side, averaged over the profiles."""
    sums, frames = {}, {0: 0.0, 1: 0.0}
    for value in profiles:
        groups = value.get('groups', [])
        for side in (0, 1):
            counts = [sum(row['ranges'] for row in groups if row.get('side') == side and row.get('node') == node)
                      for node in frame_nodes]
            counts = [count for count in counts if count > 0]
            if not counts:
                raise ValueError(f'No once-per-frame node sampled for side {side}')
            frames[side] += statistics.median(counts)
        for row in groups:
            side = row.get('side')
            if side not in (0, 1):
                continue
            entry = sums.setdefault((row['node'], side), dict(ms=0.0, ranges=0, commands=0))
            entry['ms'] += row['sum_ms']
            entry['ranges'] += row['ranges']
            entry['commands'] += row.get('commands', 0)
    nodes = {}
    for (node, side), entry in sums.items():
        row = nodes.setdefault(node, {0: dict(ms=0.0, ranges=0.0, commands=0.0), 1: dict(ms=0.0, ranges=0.0, commands=0.0)})
        row[side] = {key: entry[key] / frames[side] for key in ('ms', 'ranges', 'commands')}
    return nodes, frames


def table(nodes, names):
    rows = []
    for node, sides in nodes.items():
        rows.append(dict(node=hex(node), name=names.get(node, '?'), mainMs=sides[0]['ms'], vrcamMs=sides[1]['ms'],
                         mainRanges=sides[0]['ranges'], vrcamRanges=sides[1]['ranges'],
                         mainCommands=sides[0]['commands'], vrcamCommands=sides[1]['commands']))
    rows.sort(key=lambda r: -(r['mainMs'] + r['vrcamMs']))
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--state', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--profiles', type=int, default=6)
    parser.add_argument('--top', type=int, default=40)
    args = parser.parse_args()
    import geometry_cost_live as live
    import geometry_cost_report as cost
    import launch_to_save as boot
    from live_read import Reader
    from live_symbols import resolve
    state = live.load_state(args.state)
    pid = state['pid']
    game = boot.DEFAULT_GAME
    check = live.checker(pid, state['started'], state['baselineReporters'])
    symbols = resolve(pid, game / live.DLL, list(cost.NAMES))
    reader = Reader(pid, game / 'bin/x64/Cyberpunk2077.exe', 0, 0)
    probe = cost.LiveProbe(reader, symbols, pid, check)
    ini = game / 'red4ext/plugins/cyberpunkvrport/config/vrport.ini'
    original_ini = ini.read_bytes()
    overlay = next(line for line in original_ini.splitlines() if line.startswith(b'xr_framegen_overlay='))
    modified = original_ini.replace(overlay, b'xr_framegen_overlay=1', 1)
    profiles = []
    try:
        probe.write32('CyberpunkVR_RuntimeDiagnostics', 1)
        probe.write32('CyberpunkVR_SceneGpuGroupMode', 1)
        probe.write32('CyberpunkVR_SceneGpuNodeFilter', 0)
        ini.write_bytes(modified)
        time.sleep(3)
        rate = cost.frame_rate(probe.frames(4))
        for _ in range(args.profiles):
            profiles.append(probe.profile())
            time.sleep(.3)
    finally:
        current = ini.read_bytes()
        ini.write_bytes(original_ini if current == modified else current.replace(b'xr_framegen_overlay=1', overlay, 1))
        cleanup = probe.restore()
        reader.close()
    nodes, frames = aggregate(profiles, cost.FRAME_NODES)
    rows = table(nodes, node_names())
    value = dict(pid=pid, frameRate=rate, profileFrames=frames, cleanup=cleanup,
                 totals=dict(mainMs=sum(r['mainMs'] for r in rows), vrcamMs=sum(r['vrcamMs'] for r in rows)), rows=rows)
    args.out.write_text(json.dumps(value, indent=1) + '\n', encoding='utf-8')
    print('fps %.1f  gpu frame %.2f ms  MAIN %.2f ms  VRCAM %.2f ms' % (
        rate.get('fps', 0), rate.get('medianGpuMs') or 0, value['totals']['mainMs'], value['totals']['vrcamMs']))
    for r in rows[:args.top]:
        print('%-34s %-9s MAIN %6.3f  VRCAM %6.3f  ranges %4.1f/%4.1f' % (
            r['name'][:34], r['node'], r['mainMs'], r['vrcamMs'], r['mainRanges'], r['vrcamRanges']))


if __name__ == '__main__':
    main()
