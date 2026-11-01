"""CPU single-pass ceiling: frame rate with selected second-view nodes not recorded at all.

A single-pass that records one instanced geometry stream for both eyes removes the second
view's geometry RECORDING (CPU) as well as its vertex work (GPU). Skipping the same nodes for
VRCAM through the existing node-cut table (CyberpunkVR_NodeCutRequest, applied per Present)
bounds that gain from above in the current scene. The left eye shows no geometry while cut;
rules are always removed and CyberpunkVR_NodeCutEnable restored.
"""
import argparse
import json
from pathlib import Path
import sys
import time

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path[:0] = [str(ROOT / 'tools/quickboot_tests'), str(ROOT / 'tools/roomscale_tests'),
                str(ROOT / 'tools/swimming_tests'), str(HERE)]
NAMES = ('CyberpunkVR_NodeCutEnable', 'CyberpunkVR_NodeCutRequest', 'CyberpunkVR_NodeCutApplied',
         'CyberpunkVR_DebugNodeCutSkips')
CUT_VRCAM = 1


def pack(rva, mode, rtid=0xFF):
    if not 0 < rva < 2 ** 32 or not 0 <= rtid <= 0xFF or not 0 <= mode <= 3:
        raise ValueError('Invalid node-cut rule')
    return (1 << 63) | (mode << 40) | (rtid << 32) | rva


def parse_rule(text):
    rva, _, rtid = text.partition(':')
    return int(rva, 16), int(rtid, 0) if rtid else 0xFF


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--state', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--rule', action='append', required=True, help='work RVA in hex, optional :rtId')
    parser.add_argument('--rounds', type=int, default=2)
    parser.add_argument('--seconds', type=float, default=6)
    parser.add_argument('--settle', type=float, default=2)
    args = parser.parse_args()
    import struct
    import ctypes as c
    import control_ab
    import geometry_cost_live as live
    import geometry_cost_report as cost
    import launch_to_save as boot
    from live_read import Reader
    from live_symbols import resolve
    from second_view_cost import summarize_threads
    from thread_cpu_sample import measure as thread_measure
    rules = [parse_rule(rule) for rule in args.rule]
    state = live.load_state(args.state)
    pid = state['pid']
    game = boot.DEFAULT_GAME
    check = live.checker(pid, state['started'], state['baselineReporters'])
    symbols = resolve(pid, game / live.DLL, list(cost.NAMES) + list(NAMES))
    reader = Reader(pid, game / 'bin/x64/Cyberpunk2077.exe', 0, 0)
    probe = cost.LiveProbe(reader, symbols, pid, check)
    original_enable = probe.read32('CyberpunkVR_NodeCutEnable')

    def write64(name, value):
        data, done = c.c_uint64(value), c.c_size_t()
        if not probe.k.WriteProcessMemory(probe.handle, probe.address(name), c.byref(data), 8, c.byref(done)) or done.value != 8:
            raise c.WinError(c.get_last_error())

    def apply(mode):
        for rva, rtid in rules:
            before = probe.read32('CyberpunkVR_NodeCutApplied')
            write64('CyberpunkVR_NodeCutRequest', pack(rva, mode, rtid))
            deadline = time.monotonic() + 3
            while probe.read32('CyberpunkVR_NodeCutApplied') == before:
                check()
                if time.monotonic() > deadline:
                    raise RuntimeError('Node-cut request was not applied')
                time.sleep(.02)

    ini = game / 'red4ext/plugins/cyberpunkvrport/config/vrport.ini'
    original_ini = ini.read_bytes()
    overlay = next(line for line in original_ini.splitlines() if line.startswith(b'xr_framegen_overlay='))
    modified = original_ini.replace(overlay, b'xr_framegen_overlay=1', 1)
    rows, images = [], {}
    try:
        probe.write32('CyberpunkVR_RuntimeDiagnostics', 1)
        ini.write_bytes(modified)
        time.sleep(3)
        for index, label in enumerate(control_ab.order(args.rounds)):
            cut = label == 'B'
            apply(CUT_VRCAM if cut else 0)
            probe.write32('CyberpunkVR_NodeCutEnable', 1 if cut else 0)
            time.sleep(args.settle)
            skips = probe.read64('CyberpunkVR_DebugNodeCutSkips')
            frames = probe.frames(args.seconds)
            threads = thread_measure(pid, 3)
            skips = probe.read64('CyberpunkVR_DebugNodeCutSkips') - skips
            if cut and 'B' not in images:
                images['B'] = live.screenshot(args.out.with_name(args.out.stem + '-cut.bmp'), check)
            rows.append(dict(label=label, cut=cut, nodeCutSkips=skips, frameRate=cost.frame_rate(frames),
                             threads=summarize_threads(threads)))
            print(json.dumps(dict(label=label, fps=round(rows[-1]['frameRate']['fps'], 2), skips=skips,
                                  gameThread=round(rows[-1]['threads']['gameThread'] or 0, 1))), flush=True)
    finally:
        probe.write32('CyberpunkVR_NodeCutEnable', 0)
        try:
            apply(0)
        finally:
            probe.write32('CyberpunkVR_NodeCutEnable', original_enable)
            current = ini.read_bytes()
            ini.write_bytes(original_ini if current == modified else current.replace(b'xr_framegen_overlay=1', overlay, 1))
            cleanup = dict(probe.restore(), CyberpunkVR_NodeCutEnable=probe.read32('CyberpunkVR_NodeCutEnable'))
            reader.close()
        value = dict(pid=pid, rules=[f'{rva:#x}:{rtid}' for rva, rtid in rules], images=images, cleanup=cleanup, rows=rows)
        if len(rows) == 4 * args.rounds:
            value['summary'] = control_ab.summarize(rows)
        args.out.write_text(json.dumps(value, indent=2) + '\n', encoding='utf-8')
    if any(row['cut'] != (row['nodeCutSkips'] > 0) for row in rows):
        raise RuntimeError('Node cut did not follow the requested intervals')
    print(json.dumps(value.get('summary'), indent=2), flush=True)


if __name__ == '__main__':
    main()
