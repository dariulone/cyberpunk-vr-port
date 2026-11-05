"""Read this launch's world-marker counters with an exactly matching DLL/PDB."""
import argparse
import json
from pathlib import Path
import struct
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(ROOT / 'tools/roomscale_tests'), str(ROOT / 'tools/swimming_tests')]
from live_read import Reader
from live_symbols import resolve


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--pid', type=int, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--dll', type=Path, default=ROOT / 'build/bin/red4ext/plugins/cyberpunkvrport/Release/cyberpunkvrport.dll')
    parser.add_argument('--game', type=Path, default=Path('C:/Program Files (x86)/Steam/steamapps/common/Cyberpunk 2077'))
    args = parser.parse_args()
    if args.out.exists() or not args.out.parent.is_dir():
        raise RuntimeError('A fresh output file under an existing directory is required')
    fields = {
        'CyberpunkVR_WorldMarkerStereo': 'i',
        'CyberpunkVR_WorldMarkerRoots': 'Q',
        'CyberpunkVR_WorldMarkerQuads': 'Q',
        'CyberpunkVR_WorldMarkerTexts': 'Q',
        'CyberpunkVR_WorldMarkerCameraUploads': '2Q',
        'CyberpunkVR_WorldMarkerTextUploads': '2Q',
        'CyberpunkVR_WorldMarkerMappinStages': '7Q',
        'CyberpunkVR_WorldMarkerProjectionStages': '5Q',
    }
    symbols = resolve(args.pid, args.dll.resolve(), list(fields))
    reader = Reader(args.pid, args.game / 'bin/x64/Cyberpunk2077.exe', 0, 0)
    def sample():
        result = {'time_ns': time.time_ns()}
        for name, fmt in fields.items():
            value = struct.unpack('<' + fmt, reader.read(int(symbols[name], 16), struct.calcsize('<' + fmt)))
            result[name] = value[0] if len(value) == 1 else list(value)
        return result
    try:
        before = sample()
        time.sleep(.5)
        after = sample()
    finally:
        reader.close()
    log = (args.game / 'red4ext/plugins/cyberpunkvrport/log/cyberpunkvrport.log').read_text(encoding='utf-8', errors='replace')
    result = {'symbols': symbols, 'before': before, 'after': after,
              'log': [line for line in log.splitlines() if '[world-markers]' in line],
              'game_memory_written': False}
    args.out.write_text(json.dumps(result, indent=2), encoding='utf-8')
    print(json.dumps({'before': before, 'after': after, 'log': result['log']}, indent=2))


if __name__ == '__main__':
    main()
