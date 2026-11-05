"""Bounded live family census, preserving native rendering and all test controls."""
import argparse
import ctypes as c
import hashlib
import json
from pathlib import Path
import struct
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(ROOT / 'tools/roomscale_tests'), str(ROOT / 'tools/swimming_tests')]
from live_read import Reader
from live_symbols import resolve


def validate_input_contracts(skinning, alpha):
    if type(skinning) is not int or skinning not in (0, 1, 2):
        raise ValueError('Unknown skinning input contract')
    if type(alpha) is not int or alpha not in (0, 1, 2):
        raise ValueError('Unknown alpha input contract')
    if (alpha == 2 and skinning != 2) or (alpha == 1 and skinning):
        raise ValueError('Incompatible skinning/alpha input contracts')


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--pid', type=int, required=True)
    p.add_argument('--manifest', type=Path, required=True)
    p.add_argument('--out', type=Path, required=True)
    p.add_argument('--seconds', type=float, default=2.0)
    a = p.parse_args()
    if not .5 <= a.seconds <= 10:
        p.error('Use a bounded 0.5..10-second census')
    game = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
    symbols = resolve(a.pid, game / 'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll', ['CyberpunkVR_StereoFamily*'])
    reader = Reader(a.pid, game / 'bin/x64/Cyberpunk2077.exe', 0, 0)
    address = lambda name: int(symbols['CyberpunkVR_StereoFamily' + name], 16)
    read32 = lambda name: struct.unpack('<I', reader.read(address(name), 4))[0]
    k = reader.k
    k.WriteProcessMemory.argtypes = [c.c_void_p, c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]
    write = None
    original = None
    report = dict(pid=a.pid, dll=symbols['sha256'], passed=False)

    def observe(value):
        data, count = c.c_uint32(value), c.c_size_t()
        if not k.WriteProcessMemory(write, address('Observe'), c.byref(data), 4, c.byref(count)) or count.value != 4:
            raise c.WinError(c.get_last_error())

    def snapshot():
        for _ in range(100):
            seq = read32('Seq')
            if seq & 1:
                time.sleep(.01)
                continue
            count = read32('Count')
            if count > 4096 or read32('RecordBytes') != 72:
                raise ValueError('Invalid family record ABI/count')
            if read32('NativeCameraRecordBytes') != 16:raise ValueError('Invalid native camera record ABI')
            data = reader.read(address('Records'), count * 72) if count else b''
            cameras=reader.read(address('NativeCameraPipelines'),count*16) if count else b''
            skinning=reader.read(address('SkinningReadContracts'),count*4) if count else b''
            alpha=reader.read(address('AlphaDepthContracts'),count*4) if count else b''
            if read32('Seq') == seq:
                rows = []
                for v,camera,skin,cutout in zip(struct.iter_unpack('<7Q3Ii',data),struct.iter_unpack('<2Q',cameras),struct.iter_unpack('<I',skinning),struct.iter_unpack('<I',alpha)):
                    if camera[1]>3 or bool(camera[0])!=bool(camera[1]):raise ValueError('Native camera pipeline/stage mismatch')
                    validate_input_contracts(skin[0],cutout[0])
                    rows.append(dict(id=v[0], original=hex(v[1]), root=hex(v[2]), pairedRoot=hex(v[3]), pipeline=hex(v[4]),
                                     draws=list(v[5:7]), asset=v[7], pairs=v[8], uses=v[9], result=v[10],
                                     nativeCameraPipeline=hex(camera[0]),nativeCameraStages=camera[1],skinningReadContract=skin[0],alphaDepthContract=cutout[0]))
                return rows
        raise RuntimeError('Family registry remained busy')

    try:
        report['status'] = read32('AssetStatus')
        report['errorText'] = reader.read(address('Error'), 256).split(b'\0')[0].decode('utf-8', 'replace')
        if report['status'] != 2:
            raise RuntimeError('Family package not loaded: ' + report['errorText'])
        digest = reader.read(address('ManifestHash'), 65).split(b'\0')[0].decode('ascii')
        if digest != hashlib.sha256(a.manifest.read_bytes()).hexdigest():
            raise ValueError('Reader manifest differs from the package actually loaded by this PID')
        report['manifestSha256'] = digest
        assets=json.loads(a.manifest.read_text())['families']
        baseline = {r['id']: r['draws'] for r in snapshot()}
        write = k.OpenProcess(0x1028, False, a.pid)
        if not write:
            raise c.WinError(c.get_last_error())
        original = read32('Observe')
        if original:
            raise RuntimeError('Another family census is active')
        unmatched_before = struct.unpack('<2Q', reader.read(address('Unmatched'), 16))
        mismatch_before = struct.unpack('<2Q', reader.read(address('RootMismatch'), 16))
        observe(1)
        deadline = time.monotonic() + a.seconds
        while time.monotonic() < deadline:
            read32('AssetStatus')  # A crashed/exited PID fails during the bounded wait.
            time.sleep(.1)
        observe(0)
        time.sleep(.03)
        rows = snapshot()
        for row in rows:
            row['observedDraws'] = [v - b for v, b in zip(row['draws'], baseline.get(row['id'], [0, 0]))]
        report['records'] = rows
        report['counters'] = {key: read32(key) for key in ('AssetCount', 'Count', 'Overflow', 'Failures', 'Misses', 'Hits', 'RootHits', 'BindingFailures')}
        report['matchedDraws'] = [sum(row['observedDraws'][side] for row in rows if row['result'] == 0) for side in range(2)]
        report['unmatchedDraws'] = [v - b for v, b in zip(struct.unpack('<2Q', reader.read(address('Unmatched'), 16)), unmatched_before)]
        report['rootMismatchDraws'] = [v - b for v, b in zip(struct.unpack('<2Q', reader.read(address('RootMismatch'), 16)), mismatch_before)]
        report['usedFamilies'] = len({r['asset'] for r in rows if any(r['observedDraws'])})
        report['linkedPipelines'] = sum(r['result'] == 0 for r in rows)
        report['nativeCameraPipelinesByStageMask']=[0,0,0,0]
        for row in rows:
            if row['result']!=0:continue
            if not 1<=row['asset']<=len(assets):raise ValueError('Unknown native camera family asset')
            asset=assets[row['asset']-1]
            expected=asset.get('nativeCamera',{}).get('stages',1 if 'nativeCameraVs' in asset else 0)
            if row['nativeCameraStages']!=expected:raise ValueError('Loaded pipeline camera stages differ from its manifest')
            report['nativeCameraPipelinesByStageMask'][expected]+=1
        report['nativeCameraAssets']=read32('NativeCameras')
        report['skinningReadPipelines']=sum(r['result']==0 and r['skinningReadContract'] in (1,2) for r in rows)
        report['skinnedAlphaPipelines']=sum(r['result']==0 and r['alphaDepthContract']==2 for r in rows)
        if report['nativeCameraAssets']!=sum('nativeCamera' in f or 'nativeCameraVs' in f for f in assets):
            raise ValueError('Native camera asset coverage is incomplete')
        if (report['counters']['Overflow'] or report['counters']['Failures'] or report['counters']['BindingFailures'] or
                any(report['rootMismatchDraws']) or min(report['matchedDraws']) <= 0):
            raise RuntimeError('Family creation/binding census failed; inspect the saved report')
        report['passed'] = True
    except Exception as error:
        report['error'] = str(error)
    finally:
        if write:
            try:
                if original is not None:
                    observe(original)
            except OSError as error:
                report['restoreError'] = str(error)
                report['passed'] = False
            k.CloseHandle(write)
        reader.close()
        a.out.write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps({key: value for key, value in report.items() if key != 'records'}, indent=2))
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
