"""Read the installed face adapter's counters without taking its OSC port.

Run with a loaded save and an interactive mirror open. This requests query and
read access only: no debugger, game memory writes, or recorded facial weights.
"""
import argparse
import json
from pathlib import Path
import struct
import sys
import time

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--pid', type=int, required=True)
    parser.add_argument('--seconds', type=float, default=5)
    parser.add_argument('--game', type=Path, default=Path(r'C:\Program Files (x86)\GOG Galaxy\Games\Cyberpunk 2077'))
    args = parser.parse_args()
    if not 1 <= args.seconds <= 30:
        parser.error('Sample for 1 to 30 seconds')
    sys.path[:0] = [str(ROOT/'tools/swimming_tests'), str(ROOT/'tools/roomscale_tests')]
    from live_symbols import resolve
    from live_read import Reader
    name = 'CyberpunkVR_FaceTracking'
    symbols = resolve(args.pid, args.game/'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll', [name])
    address = int(symbols[name], 16)
    reader = Reader(args.pid, args.game/'bin/x64/Cyberpunk2077.exe', 0, 0)

    def sample():
        for _ in range(50):
            data = reader.read(address, 128)
            v = struct.unpack('<16Q', data)
            if not v[0] & 1 and data[:8] == reader.read(address, 8):
                if v[1] != 1:
                    raise RuntimeError('Face diagnostic not initialized or incompatible')
                row = dict(zip(('port', 'socket_error', 'fresh_channels', 'mapped_controls',
                                'packets', 'malformed', 'owner_passes', 'facial_passes',
                                'writes', 'faults', 'rejected_layouts', 'sample_tick', 'save_error'), v[3:]))
                row.update(enabled=bool(v[2] & 1), listening=bool(v[2] & 2),
                           hooks=bool(v[2] & 4), mirror_bound=bool(v[2] & 8))
                return row
            time.sleep(.005)
        raise RuntimeError('No coherent face diagnostic snapshot')

    try:
        before = sample()
        time.sleep(args.seconds)
        after = sample()
        delta = {k: after[k]-before[k] for k in ('packets', 'malformed', 'owner_passes', 'facial_passes', 'writes', 'faults', 'rejected_layouts')}
        print(json.dumps({'pid': args.pid, 'dll_sha256': symbols['sha256'], 'status': after,
                          'delta': delta, 'updated': after['sample_tick'] > before['sample_tick'],
                          'note': 'Writes confirm solver input; visible facial motion still needs a headset check.'}, indent=2))
    finally:
        reader.close()


if __name__ == '__main__':
    main()
