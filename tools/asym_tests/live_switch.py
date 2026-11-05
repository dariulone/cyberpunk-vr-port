"""Read or set the plugin's exported u32 switches in the running game, by name, with no PDB (the export table of
cyberpunkvrport.dll, rt_dispatch_census.resolve_exports). For live A/B of the twin groups, e.g. the reflection
bisect of 2026-11-01:

  py -3.11 -B live_switch.py --pid PID CyberpunkVR_StereoMergePatch CyberpunkVR_AsymSsrPatch     (read)
  py -3.11 -B live_switch.py --pid PID CyberpunkVR_StereoMergePatch=0                            (set)

Only for std::atomic<uint32_t> / int exports: the value is written as 4 bytes.
"""
import argparse
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from rt_dispatch_census import Process, resolve_exports


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--pid', type=int, required=True)
    ap.add_argument('switches', nargs='+', help='NAME to read, NAME=VALUE to set')
    a = ap.parse_args()
    items = [s.split('=', 1) for s in a.switches]
    at = resolve_exports(a.pid, [i[0] for i in items])
    p = Process(a.pid)
    for item in items:
        name = item[0]
        before = struct.unpack('<I', p.read(at[name], 4))[0]
        if len(item) == 2:
            p.write(at[name], struct.pack('<I', int(item[1], 0)))
            print('%s %d -> %d' % (name, before, struct.unpack('<I', p.read(at[name], 4))[0]))
        else:
            print('%s %d' % (name, before))


if __name__ == '__main__':
    main()
