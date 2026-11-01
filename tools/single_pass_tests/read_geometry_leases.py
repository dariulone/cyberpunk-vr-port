"""Verify native IA resource ownership for the selected pass, without drawing."""
import argparse
from collections import Counter
import json
from pathlib import Path
import struct
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(ROOT / 'tools/roomscale_tests'), str(ROOT / 'tools/swimming_tests')]
from live_read import Reader
from live_symbols import resolve


def decode(draw_data, resource_data, native):
    resources = [dict(zip(('resource', 'gpu', 'bytes', 'heapType', 'flags'), row)) for row in struct.iter_unpack('<3Q2I', resource_data)]
    if not resources or any(resources[0].values()) or len(native) % 1096:
        raise ValueError('Invalid geometry lease data')
    draws = []
    for row in struct.iter_unpack('<24I', draw_data):
        sequence, index, used, owned = row[:4]
        vertices, flags, frame, side = row[4:20], row[20], row[21], row[22]
        if not sequence or sequence * 1096 > len(native) or flags & ~31 or used & ~65535 or owned & ~used:
            raise ValueError('Invalid geometry lease record')
        source = native[(sequence-1)*1096:sequence*1096]
        if struct.unpack_from('<I', source, 40)[0] != frame or struct.unpack_from('<I', source, 48)[0] != side or struct.unpack_from('<I', source, 1092)[0] != used:
            raise ValueError('Lease does not match the captured native draw')
        def bound(resource, address, size):
            if not resource or resource >= len(resources):
                raise ValueError('Invalid geometry resource identity')
            value = resources[resource]
            if not value['resource'] or not size or address < value['gpu'] or address + size > value['gpu'] + value['bytes']:
                raise ValueError('Bound geometry view exceeds its owned resource')
        if index:
            address, size = struct.unpack_from('<QI', source, 96)
            bound(index, address, size)
        for slot, identity in enumerate(vertices):
            if identity:
                address, size = struct.unpack_from('<QI', source, 112+slot*16)
                bound(identity, address, size)
            if bool(identity) != bool(owned & (1 << slot)):
                raise ValueError('Vertex ownership mask differs')
        if flags & 15 == 15 and (not index or owned != used):
            raise ValueError('Incomplete lease was marked complete')
        draws.append(dict(sequence=sequence, index=index, vertices=list(vertices), usedMask=used, ownedMask=owned,
                          flags=flags, frame=frame, side=side))
    if len({d['sequence'] for d in draws}) != len(draws):
        raise ValueError('Duplicate geometry lease sequence')
    return draws, resources


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--pid', type=int, required=True)
    p.add_argument('--capture', type=Path, required=True)
    p.add_argument('--constants', type=Path, required=True)
    p.add_argument('--out', type=Path, required=True)
    a = p.parse_args()
    capture, constants = [json.loads(path.read_text()) for path in (a.capture, a.constants)]
    game = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
    symbols = resolve(a.pid, game / 'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll', ['CyberpunkVR_GeometryLease*'])
    if any(row['pid'] != a.pid or row['dll'] != symbols['sha256'] for row in (capture, constants)):
        raise ValueError('Geometry lease process/build differs')
    reader = Reader(a.pid, game / 'bin/x64/Cyberpunk2077.exe', 0, 0)
    read32 = lambda name: struct.unpack('<I', reader.read(int(symbols['CyberpunkVR_GeometryLease' + name], 16), 4))[0]
    report = dict(pid=a.pid, dll=symbols['sha256'], passed=False, runtimeEligible=False)
    try:
        version = read32('Seq')
        report.update(compatible=read32('Compatible'), failure=read32('Failure'))
        if version & 1 or report['compatible'] != 1:
            raise RuntimeError('Native geometry lease collector unavailable or incomplete')
        blocks = {}
        for name, size, limit in (('Draws', 96, 4096), ('Resources', 32, 8192), ('ReadStates', 208, 4096)):
            count = read32('ResourceCount' if name == 'Resources' else 'DrawCount')
            if count > limit:
                raise ValueError('Geometry lease capacity exceeded')
            pointer = struct.unpack('<Q', reader.read(int(symbols['CyberpunkVR_GeometryLease' + name], 16), 8))[0]
            blocks[name] = reader.read(pointer, count*size) if count else b''
        if read32('Seq') != version:
            raise RuntimeError('Geometry ownership changed during read')
        native = (a.capture.parent / capture['drawDataFile']).read_bytes()
        draws, resources = decode(blocks['Draws'], blocks['Resources'], native)
        selected = {s for pair in constants['selectedPass']['pairs'] for s in pair['draws']}
        by_sequence = {d['sequence']: d for d in draws}
        state_rows = list(struct.iter_unpack('<52I', blocks['ReadStates']))
        if len(state_rows) != len(draws) or any(row[0] != draw['sequence'] for row, draw in zip(state_rows, draws)):
            raise ValueError('Native read-state records do not match geometry')
        source_states, selected_states, all_known = Counter(), Counter(), 0
        for row, draw in zip(state_rows, draws):
            known = True
            for slot in range(17):
                if slot and not draw['usedMask'] & (1 << (slot-1)):
                    continue
                value, status, kind = row[1+slot*3:4+slot*3]
                key = f'{status}:{kind}:{value:#x}'
                source_states[key] += 1
                if draw['sequence'] in selected:
                    selected_states[key] += 1
                known &= status in (1, 2, 8, 9)
            all_known += known and draw['sequence'] in selected
        failures = [sequence for sequence in selected if sequence not in by_sequence or by_sequence[sequence]['flags'] != 31]
        report.update(draws=len(draws), resources=len(resources)-1, complete=sum(d['flags'] & 15 == 15 for d in draws),
                      dynamicStateOwned=sum(bool(d['flags'] & 16) for d in draws),
                      statuses=dict(Counter(d['flags'] for d in draws)), selectedDraws=len(selected), selectedFailures=failures)
        report['readStates'] = dict(all=dict(source_states), selected=dict(selected_states), selectedDrawsKnown=all_known)
        report['files'] = {}
        for name, data in blocks.items():
            path = a.out.with_name(a.out.stem + '-' + name.lower() + '.bin')
            path.write_bytes(data)
            report['files'][name] = path.name
        report['limitation'] = 'Strong geometry/PSO references, dynamic draw state and view ranges; mutable GPU contents and shader-specific dynamic-state support require separate verification.'
        if report['failure']:
            raise RuntimeError('Native geometry lease collector reported an error')
        if not selected or failures:
            raise RuntimeError('Selected native geometry is not completely owned')
        report['passed'] = True
    except Exception as error:
        report['error'] = str(error)
    finally:
        reader.close()
        a.out.write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps(report, indent=2))
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
