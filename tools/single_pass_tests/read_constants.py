"""Verify/export owned native constant payloads; build a bank without recording draws."""
import argparse
from collections import Counter
import ctypes as c
import hashlib
import json
import math
from pathlib import Path
import struct
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(ROOT / 'tools/roomscale_tests'), str(ROOT / 'tools/swimming_tests')]
from live_read import Reader
from live_symbols import resolve
from select_constant_pass import select_pass


def decode(blocks):
    draws = [dict(zip(('family', 'sequence', 'first', 'count', 'flags', 'side', 'frame'), row))
             for row in struct.iter_unpack('<Q6I', blocks['Draws'])]
    bindings = [dict(zip(('descriptor', 'heap', 'address', 'hash', 'draw', 'parameter', 'offset', 'visibility',
                         'shaderRegister', 'bytes', 'expectedBytes', 'payload', 'status', 'reserved'), row))
                for row in struct.iter_unpack('<4Q10I', blocks['Bindings'])]
    payloads = [dict(zip(('offset', 'bytes', 'hash'), row)) for row in struct.iter_unpack('<IIQ', blocks['Payloads'])]
    blob = blocks['Blob']
    if not payloads or payloads[0] != dict(offset=0, bytes=0, hash=0):
        raise ValueError('Missing payload sentinel')
    for p in payloads[1:]:
        if not 0 < p['bytes'] <= 65536 or p['offset'] + p['bytes'] > len(blob):
            raise ValueError('Bad payload range')
        h = 14695981039346656037
        for b in blob[p['offset']:p['offset'] + p['bytes']]:
            h = ((h ^ b) * 1099511628211) & 0xffffffffffffffff
        if h != p['hash']:
            raise ValueError('Payload hash mismatch')
    sequences = set()
    for d in draws:
        if not d['family'] or not d['sequence'] or d['sequence'] in sequences or d['first'] + d['count'] > len(bindings):
            raise ValueError('Bad draw identity/range')
        sequences.add(d['sequence'])
        if d['side'] not in (0, 1) or not d['frame'] or d['flags'] & ~7:
            raise ValueError('Invalid draw frame/eye/status')
        for b in bindings[d['first']:d['first'] + d['count']]:
            if b['draw'] != d['sequence'] or b['payload'] >= len(payloads) or b['status'] not in (1, 2, 4, 8, 16, 32, 64):
                raise ValueError('Bad constant binding')
            if b['status'] == 1:
                p = payloads[b['payload']]
                expected = b['expectedBytes'] if b['reserved'] & 1 else b['bytes']
                if b['reserved'] not in (0, 1, 3, 7) or not b['payload'] or p['bytes'] != expected or p['hash'] != b['hash'] or not 0 < b['expectedBytes'] <= b['bytes']:
                    raise ValueError('Binding does not match owned payload')
            if d['flags'] == 7 and b['status'] != 1:
                raise ValueError('Stable draw has a failed binding')
    return draws, bindings, payloads


def report_summary(report):
    result = report.copy()
    if 'selectedPass' in result:
        selected = result['selectedPass'].copy()
        selected['pairCount'] = len(selected.pop('pairs'))
        result['selectedPass'] = selected
    return result


def decode_geometry_freeze(data, selected):
    rows = [dict(zip(('sequence', 'status', 'result', 'reserved', 'bytes'), r)) for r in struct.iter_unpack('<IIiIQ', data)]
    by_sequence = {r['sequence']: r for r in rows}
    if len(by_sequence) != len(rows) or any(not r['sequence'] or r['status'] not in range(1, 8) or
            r['reserved'] or r['bytes'] > 8*1024*1024 or (r['status'] in (1, 2, 3) and (r['result'] or not r['bytes'])) for r in rows):
        raise ValueError('Invalid GPU geometry snapshot publication')
    missing = [s for s in selected if s not in by_sequence or by_sequence[s]['status'] != 3]
    return dict(captured=len(rows), statuses=dict(Counter(r['status'] for r in rows)),
                bytes=sum(r['bytes'] for r in rows), selected=len(selected), unready=missing)


def decode_depth_comparison(data, *, minimum_written=(16,16)):
    row = struct.unpack('<4I8Q2f', data)
    result = dict(width=row[0], height=row[1], draws=row[2], complete=row[3], depthDifferent=list(row[4:6]),
                  stencilDifferent=list(row[6:8]), written=list(row[8:10]), invalidDepth=list(row[10:12]), maxDepthError=list(row[12:14]))
    pixels = result['width'] * result['height']
    if not 0 < result['width'] <= 4096 or not 0 < result['height'] <= 4096 or not 0 < result['draws'] <= 2048 or result['complete'] != 1:
        raise ValueError('Invalid depth comparison dimensions/completion')
    if any(v > pixels for v in row[4:12]):
        raise ValueError('Invalid depth comparison pixel counts')
    if any(not math.isfinite(v) or v < 0 for v in row[12:14]):
        raise ValueError('Invalid depth error values')
    if len(minimum_written)!=2 or any(not isinstance(v,int) or not 0<=v<=pixels for v in minimum_written):
        raise ValueError('Invalid expected depth coverage')
    result['equal'] = not any(row[4:8]) and not any(row[10:12]) and all(v >= minimum for v,minimum in zip(row[8:10],minimum_written))
    return result


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--pid', type=int, required=True)
    p.add_argument('--capture', type=Path, required=True)
    p.add_argument('--families', type=Path, required=True)
    p.add_argument('--manifest', type=Path, required=True)
    p.add_argument('--geometry', type=Path, required=True, help='Verified geometry-pair report from this exact capture')
    p.add_argument('--out', type=Path, required=True)
    p.add_argument('--compare-depth', action='store_true')
    a = p.parse_args()
    game = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
    symbols = resolve(a.pid, game / 'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll', ['CyberpunkVR_Constant*', 'CyberpunkVR_RegisteredCopy*', 'CyberpunkVR_GeometryFreeze*'])
    capture, family_report = [json.loads(path.read_text()) for path in (a.capture, a.families)]
    if a.compare_depth and not capture.get('frozenGeometry'):
        raise ValueError('Depth comparison requires GPU geometry snapshots')
    geometry = json.loads(a.geometry.read_text())
    if (geometry.get('pid') != a.pid or geometry.get('dll') != capture['dll'] or
            geometry.get('captureSha256') != hashlib.sha256(a.capture.read_bytes()).hexdigest()):
        raise ValueError('Geometry report belongs to a different capture')
    if any(row['pid'] != a.pid or row['dll'] != symbols['sha256'] for row in (capture, family_report)):
        raise ValueError('Capture/family process or DLL differs')
    if hashlib.sha256(a.manifest.read_bytes()).hexdigest() != family_report['manifestSha256']:
        raise ValueError('Shader family manifest changed since the live census')
    reader = Reader(a.pid, game / 'bin/x64/Cyberpunk2077.exe', 0, 0)
    address = lambda name: int(symbols['CyberpunkVR_Constant' + name], 16)
    read32 = lambda name: struct.unpack('<I', reader.read(address(name), 4))[0]
    k = reader.k
    k.WriteProcessMemory.argtypes = [c.c_void_p, c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]
    handle = None
    report = dict(pid=a.pid, dll=symbols['sha256'], passed=False)

    def write(name, data):
        count = c.c_size_t()
        if not k.WriteProcessMemory(handle, address(name), data, len(data), c.byref(count)) or count.value != len(data):
            raise c.WinError(c.get_last_error())

    try:
        deadline = time.monotonic() + 8
        while read32('PendingLists'):
            if time.monotonic() > deadline:
                raise TimeoutError('Constant-bearing lists not submitted/reset')
            time.sleep(.01)
        version = read32('CaptureSeq')
        if version & 1 or read32('CaptureEnable'):
            raise RuntimeError('Constant capture still active')
        deadline = time.monotonic() + 2
        while True:
            sequence = struct.unpack('<I', reader.read(int(symbols['CyberpunkVR_RegisteredCopyTraceSeq'], 16), 4))[0]
            count = struct.unpack('<I', reader.read(int(symbols['CyberpunkVR_RegisteredCopyTraceCount'], 16), 4))[0]
            if count and not sequence & 1:
                break
            if time.monotonic() > deadline:
                raise TimeoutError('Constant timeline trace was not published')
            time.sleep(.01)
        if count > 131072 or struct.unpack('<I', reader.read(int(symbols['CyberpunkVR_RegisteredCopyTraceOverflow'], 16), 4))[0]:
            raise RuntimeError('Constant timeline trace incomplete')
        pointer = struct.unpack('<Q', reader.read(int(symbols['CyberpunkVR_RegisteredCopyTraceData'], 16), 8))[0]
        trace = reader.read(pointer, count * 64)
        if struct.unpack('<I', reader.read(int(symbols['CyberpunkVR_RegisteredCopyTraceSeq'], 16), 4))[0] != sequence:
            raise RuntimeError('Constant timeline trace changed during read')
        trace_path = a.out.with_name(a.out.stem + '-timeline.bin')
        trace_path.write_bytes(trace)
        report['trace'] = dict(file=trace_path.name, schema=1, eventBytes=64, events=count, sha256=hashlib.sha256(trace).hexdigest())
        report['failure'] = read32('CaptureFailure')
        report['timelineResults'] = dict(zip(('ready', 'missing', 'pending', 'unknownWrite', 'crossQueue', 'abandoned', 'overflow', 'replayed'),
                                             struct.unpack('<8I', reader.read(int(symbols['CyberpunkVR_RegisteredCopyTimelineResults'], 16), 32))))
        report['timelineStats'] = dict(zip(('recordings', 'writes', 'submissions', 'replays', 'queues'),
                                          struct.unpack('<5I', reader.read(int(symbols['CyberpunkVR_RegisteredCopyTimelineStats'], 16), 20))))
        report['registeredCopies'] = {name: struct.unpack('<Q' if name == 'Reads' else '<I', reader.read(int(symbols['CyberpunkVR_RegisteredCopy' + name], 16), 8 if name == 'Reads' else 4))[0]
                                      for name in ('Count', 'Unscoped', 'Rejected', 'Changed', 'Reads', 'PendingReads', 'OrderRejected', 'Signals', 'Waits', 'CpuSignals')}
        report['uploads'] = dict(count=read32('UploadCount'), failures=read32('UploadFailures'),
                                 evictions=read32('UploadEvictions'),
                                 heapTypes=list(struct.unpack('<6I', reader.read(address('UploadTypes'), 24))),
                                 nativeImports=read32('UploadNativeImports'), nativeRejects=read32('UploadNativeRejects'),
                                 nativeApiHits=read32('UploadApiHits'),
                                 registeredTypes=list(struct.unpack('<6I', reader.read(address('RegisteredTypes'), 24))),
                                 registeredRejects=read32('RegisteredRejects'),
                                 bytes=struct.unpack('<Q', reader.read(address('UploadBytes'), 8))[0],
                                 reads=struct.unpack('<Q', reader.read(address('UploadReads'), 8))[0],
                                 misses=struct.unpack('<Q', reader.read(address('UploadMisses'), 8))[0])
        blocks = {}
        for name, counter, unit, limit in [('Draws', 'DrawCount', 32, 4096), ('Bindings', 'BindingCount', 72, 65536),
                                           ('Payloads', 'PayloadCount', 16, 65536), ('Blob', 'BlobBytes', 1, 64 * 1024 * 1024)]:
            count = read32(counter)
            if count > limit:
                raise ValueError('Constant capture capacity exceeded')
            pointer, = struct.unpack('<Q', reader.read(address(name), 8))
            blocks[name] = reader.read(pointer, count * unit) if count else b''
        if read32('CaptureSeq') != version:
            raise RuntimeError('Constant capture changed during read')
        report['files'] = {}
        for name, data in blocks.items():
            path = a.out.with_name(a.out.stem + '-' + name.lower() + '.bin')
            path.write_bytes(data)
            report['files'][name] = path.name
        if report['failure']:
            raise RuntimeError('Constant collector failed')
        draws, bindings, payloads = decode(blocks)
        native = (a.capture.parent / capture['drawDataFile']).read_bytes()
        for d in draws:
            at = (d['sequence'] - 1) * 1096
            if at + 1096 > len(native) or struct.unpack_from('<I', native, at + 40)[0] != d['frame'] or struct.unpack_from('<I', native, at + 48)[0] != d['side']:
                raise ValueError('Native draw sequence/frame/eye differs')
        report.update(draws=len(draws), bindings=len(bindings), uniquePayloads=len(payloads) - 1, blobBytes=len(blocks['Blob']),
                      stableDraws=sum(d['flags'] == 7 for d in draws), statuses=dict(Counter(b['status'] for b in bindings)),
                      declaredBytes=sum(b['expectedBytes'] for b in bindings), descriptorBytes=sum(b['bytes'] for b in bindings))
        report['timelineFailureReasons'] = dict(Counter(f"{b['visibility']}:b{b['shaderRegister']}:{(b['reserved'] >> 16) & 0x7fff}:{b['reserved'] & 0xffff}"
                                                        for b in bindings if b['reserved'] & 0x80000000))
        family_rows = {r['id']: r for r in family_report['records']}
        assets = json.loads(a.manifest.read_text())['families']
        eligible = {identity for identity, row in family_rows.items() if row['result'] == 0 and
                    all(u['type'] == 2 and u['space'] == 0 and u['count'] == 1 for u in assets[row['asset'] - 1]['uses'])}
        candidate = select_pass(draws, eligible, geometry['mappedPairs'], capture['records'])
        report['constantOnlyStableDraws'] = sum(d['flags'] == 7 and d['family'] in eligible for d in draws)
        if not candidate:
            raise RuntimeError('No unambiguous geometry pairs with complete two-eye constants in one pass')
        report['selectedPass'] = candidate
        handle = k.OpenProcess(0x1028, False, a.pid)
        if not handle:
            raise c.WinError(c.get_last_error())
        if read32('BankRequest'):
            raise RuntimeError('Another bank request is pending')
        if read32('BankCompareDepth') or read32('CompareState') == 1:
            raise RuntimeError('Another depth comparison is active')
        token = (read32('BankDone') + 1) & 0xffffffff or 1
        pairs = candidate['pairs']
        sequences = [sequence for pair in pairs for sequence in pair['draws']]
        if capture.get('frozenGeometry'):
            freeze = lambda name: struct.unpack('<I', reader.read(int(symbols['CyberpunkVR_GeometryFreeze' + name], 16), 4))[0]
            deadline = time.monotonic() + 8
            while freeze('Pending') or freeze('Seq') & 1:
                if time.monotonic() > deadline:
                    raise TimeoutError('Geometry copies did not complete or source recordings remain replayable')
                time.sleep(.02)
            version, count = freeze('Seq'), freeze('Count')
            if not 0 < count <= 2048 or freeze('Failure'):
                raise RuntimeError('GPU geometry snapshot collection failed')
            pointer = struct.unpack('<Q', reader.read(int(symbols['CyberpunkVR_GeometryFreezeRecords'], 16), 8))[0]
            data = reader.read(pointer, count*24)
            if freeze('Seq') != version:
                raise RuntimeError('GPU geometry publication changed while reading')
            path = a.out.with_name(a.out.stem + '-geometry-frozen.bin')
            path.write_bytes(data)
            report['gpuGeometry'] = decode_geometry_freeze(data, sequences)
            report['gpuGeometry']['file'] = path.name
            if report['gpuGeometry']['unready']:
                raise RuntimeError('Selected geometry lacks complete sealed GPU copies')
        write('BankSequences', struct.pack(f'<{len(sequences)}I', *sequences))
        write('BankPairCount', struct.pack('<I', len(pairs)))
        write('BankPrepareGeometry', struct.pack('<I', 1))
        write('BankCompareDepth', struct.pack('<I', int(a.compare_depth)))
        write('BankRequest', struct.pack('<I', token))
        deadline = time.monotonic() + 5
        while read32('BankDone') != token:
            if time.monotonic() > deadline:
                raise TimeoutError('Owned bank construction did not finish')
            time.sleep(.02)
        result, = struct.unpack('<i', reader.read(address('BankResult'), 4))
        report['bank'] = dict(pairs=len(pairs), draws=read32('BankDraws'), result=result, descriptors=read32('BankDescriptors'),
                             bytes=struct.unpack('<Q', reader.read(address('BankBytes'), 8))[0], drew=False)
        if result < 0 or report['bank']['draws'] != len(pairs) or not report['bank']['descriptors'] or not report['bank']['bytes']:
            raise RuntimeError('Owned bank construction failed')
        geometry_result = struct.unpack('<i', reader.read(address('BankGeometryResult'), 4))[0]
        report['geometryCommands'] = dict(result=geometry_result, draws=read32('BankPreparedDraws'),
                                         instanceBytes=struct.unpack('<Q', reader.read(address('BankInstanceBytes'), 8))[0], recorded=False)
        if geometry_result < 0 or report['geometryCommands']['draws'] != len(pairs) or not report['geometryCommands']['instanceBytes']:
            raise RuntimeError('Owned geometry command preparation failed')
        pass_result = struct.unpack('<i', reader.read(address('BankPassResult'), 4))[0]
        report['preparedPass'] = dict(result=pass_result, draws=read32('BankPassDraws'), stage=read32('BankPassStage'), recorded=False,
                                      targetLayers=2, validated=pass_result >= 0,
                                      validation=list(struct.unpack('<3I', reader.read(address('BankPassValidation'), 12))))
        if pass_result < 0 or report['preparedPass']['draws'] != len(pairs):
            raise RuntimeError('Complete geometry pass construction failed')
        if a.compare_depth:
            deadline = time.monotonic() + 15
            while True:
                sequence = read32('CompareSeq')
                state, identity = read32('CompareState'), read32('CompareToken')
                if not sequence & 1 and identity == token and state in (2, 3):
                    code = struct.unpack('<i', reader.read(address('CompareResult'), 4))[0]
                    data = reader.read(address('CompareData'), 88)
                    if read32('CompareSeq') == sequence:
                        break
                if time.monotonic() > deadline:
                    raise TimeoutError('Original/paired GPU depth comparison did not complete')
                time.sleep(.02)
            report['depthComparison'] = dict(state=state, result=code, stage=read32('CompareStage'), token=token, privateTargets=True, nativeDrawsReplaced=0)
            if code < 0 or state != 2:
                raise RuntimeError('Original/paired GPU depth comparison failed')
            report['depthComparison'].update(decode_depth_comparison(data))
            if not report['depthComparison']['equal'] or report['depthComparison']['draws'] != len(pairs):
                raise RuntimeError('Original and paired game shaders differ or produced blank depth')
            report['preparedPass']['recorded'] = True
            report['preparedPass']['destination'] = 'private comparison targets only'
        report['passed'] = True
    except Exception as error:
        report['error'] = str(error)
        try:
            report['pendingDiagnostics'] = dict(pending=read32('PendingLists'), failure=read32('CaptureFailure'),
                timeline=list(struct.unpack('<8I', reader.read(int(symbols['CyberpunkVR_RegisteredCopyTimelineResults'], 16), 32))),
                stats=list(struct.unpack('<5I', reader.read(int(symbols['CyberpunkVR_RegisteredCopyTimelineStats'], 16), 20))),
                synchronization={name: struct.unpack('<I', reader.read(int(symbols['CyberpunkVR_RegisteredCopy' + name], 16), 4))[0]
                                 for name in ('Signals', 'Waits', 'CpuSignals')})
        except Exception as diagnostic_error:
            report['diagnosticError'] = str(diagnostic_error)
    finally:
        if handle:
            try:
                write('BankRequest', struct.pack('<I', 0))
                write('BankPairCount', struct.pack('<I', 0))
                write('BankPrepareGeometry', struct.pack('<I', 0))
                write('BankCompareDepth', struct.pack('<I', 0))
            finally:
                k.CloseHandle(handle)
        reader.close()
        a.out.write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps(report_summary(report), indent=2))
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
