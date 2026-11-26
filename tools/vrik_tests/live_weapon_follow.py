"""Read a user-operated take of stationary XR inputs and both solved wrist poses.

--check-image verifies all probe symbols offline. With a PID, the operator
controls the session, support grip and shot. Record magazine counts separately.
"""
import argparse
import ctypes as c
import hashlib
import json
from pathlib import Path
import struct
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
for group in ('sim_harness', 'single_pass_tests', 'roomscale_tests'):
    sys.path.insert(0, str(ROOT / 'tools' / group))
from simxr import _modules
from live_read import Reader
from symbol_identity import pe_identity, live_pe_identity
from symbol_session import load_dbghelp, initialize, module_info
from thread_stack_sampler import SYMBOL_INFOW

FIELDS = {
    'freshSolve': ('g_VRIKFreshTotal', 'i'),
    'weaponName': ('CyberpunkVR_WeaponName', '64s'),
    'leftEquipped': ('g_hasLeftWeaponEquipped', '?'),
    'rightEquipped': ('g_hasRightWeaponEquipped', '?'),
    'support': ('CyberpunkVR_TwoHandActive', 'i'),
    'supportWeight': ('CyberpunkVR_TwoHandWeight', 'f'),
    'aimMode': ('CyberpunkVR_TwoHandAim', 'i'),
    'holdIndex': ('CyberpunkVR_DebugTwoHandHoldIdx', 'i'),
    'supportCtrlModel': ('g_VRTwoHandCtrlL', '3f'),
    'supportTargetValid': ('g_supValid', 'i'),
    'supportTargetModel': ('g_supPos', '3f'),
    'reachM': ('CyberpunkVR_DebugTwoHandDist', 'f'),
    'steerDeg': ('CyberpunkVR_DebugTwoHandAimDeg', 'f'),
    'primaryRiseDeg': ('CyberpunkVR_DebugRecoilAppliedDeg', 'f'),
    'poseShotCount': ('CyberpunkVR_DebugRecoilShots', 'Q'),
    'physicalRayRecoilCount': ('CyberpunkVR_DebugPhysicalRayRecoil', 'Q'),
    'runtimeDiagnostics': ('CyberpunkVR_RuntimeDiagnostics', 'i'),
    'carryLeft': ('CyberpunkVR_CarryLeft', 'i'),
    'rawTargetsLR': ('g_VRHandRawModel', '6f'),
    'rightIKTarget': ('g_VRIKDbgTarget', '4f'), 'leftIKTarget': ('g_VRIKDbgTargetL', '4f'),
    'fkCapture': ('g_VRDiagCapture', 'i'), 'fkCount': ('g_VRFKSnapCount', 'i'),
    'fkSequence': ('g_VRFKSnapSeq', 'I'), 'fkArray': ('g_VRFKSnapPos', '3f'),
    'leftBone': ('g_VRLeftBoneIdx', 'i'), 'rightBone': ('g_VRRightBoneIdx', 'i'),
    'wristLeft': ('g_VRPalmModelL', '3f'), 'rotLeft': ('g_VRPalmModelRotL', '4f'),
    'wristRight': ('g_VRPalmModelR', '3f'), 'rotRight': ('g_VRPalmModelRotR', '4f'),
    'shared': ('g_pSharedHands', 'Q'),
}


def probe_map(image):
    ident = pe_identity(image)
    dbg = load_dbghelp()
    session = c.c_void_p(0x43475250)
    base = 0x180000000
    initialize(dbg, session, [image], invade=False)
    try:
        if not dbg.SymLoadModuleExW(session, None, str(image), 'gunprobe', base, ident['imageSize'], None, 0):
            raise c.WinError(c.get_last_error())
        dbg.SymFromNameW.argtypes = [c.c_void_p, c.c_wchar_p, c.POINTER(SYMBOL_INFOW)]
        rvas = {}
        for key, (name, fmt) in FIELDS.items():
            info = SYMBOL_INFOW()
            info.SizeOfStruct, info.MaxNameLen = 88, 511
            if not dbg.SymFromNameW(session, 'gunprobe!' + name, c.byref(info)):
                raise RuntimeError(f'Missing PDB symbol {name}')
            rva = info.Address - base
            if not 0 <= rva <= ident['imageSize'] - struct.calcsize('<' + fmt):
                raise RuntimeError(f'Invalid symbol {name}')
            rvas[key] = rva
        provenance = module_info(dbg, session, base)
        if not provenance.get('identityVerified') or provenance.get('pdbUnmatched'):
            raise RuntimeError('PDB identity mismatch')
        return ident, rvas
    finally:
        dbg.SymCleanup(session)


def record(pid, seconds, ident, rvas):
    modules = {name.lower(): (base, path) for name, base, path in _modules(pid)}
    base, _ = modules['cyberpunkvrport.dll']
    reader = Reader(pid, modules['cyberpunk2077.exe'][1], 0, 0)
    try:
        mapped = live_pe_identity(reader.handle, base)
        if any(mapped.get(k) != ident[k] for k in ('guid', 'age', 'imageSize')):
            raise RuntimeError('Running module differs from the selected image')

        def field(key):
            fmt = '<' + FIELDS[key][1]
            values = struct.unpack(fmt, reader.read(base + rvas[key], struct.calcsize(fmt)))
            if key == 'weaponName':
                return values[0].split(b'\0', 1)[0].decode('ascii', errors='replace')
            return values if len(values) > 1 else values[0]

        samples = []
        last = None
        start = time.perf_counter()
        print('Recording now. Keep the inputs still, fire once, and hold through recovery.', flush=True)
        while (elapsed := time.perf_counter() - start) < seconds:
            seq = field('freshSolve')
            if seq != last:
                row = {key: field(key) for key in FIELDS if key not in ('freshSolve', 'shared', 'fkArray')}
                if row['fkCapture'] and all(0 <= row[key] < row['fkCount'] for key in ('leftBone', 'rightBone')):
                    # Pose hook's completed-bone FK publication. The arm's
                    # input goal and pin can differ from the final bone.
                    for key in ('left', 'right'):
                        address = base + rvas['fkArray'] + 12 * row[key + 'Bone']
                        row[key + 'WristFK'] = struct.unpack('<3f', reader.read(address, 12))
                    if row['fkSequence'] != field('fkSequence'):
                        continue
                shared = field('shared')
                if not shared:
                    raise RuntimeError('XR hand data unavailable')
                xrseq, = struct.unpack('<I', reader.read(shared + 127 * 4, 4))
                row['xrLeftRight'] = struct.unpack('<16f', reader.read(shared, 64))
                xrend, = struct.unpack('<I', reader.read(shared + 127 * 4, 4))
                if xrseq == xrend and not (xrseq & 1) and seq == field('freshSolve'):
                    samples.append(dict(t=elapsed, freshSolve=seq, xrSequence=xrseq, **row))
                    last = seq
            time.sleep(.002)
        return samples
    finally:
        reader.close()


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    mode = p.add_mutually_exclusive_group(required=True)
    mode.add_argument('--pid', type=int)
    mode.add_argument('--check-image', action='store_true')
    p.add_argument('--image', type=Path, default=ROOT / 'build/patchday/sdk25-build/bin/red4ext/plugins/cyberpunkvrport/Release/cyberpunkvrport.dll')
    p.add_argument('--seconds', type=float, default=8)
    p.add_argument('--out', type=Path)
    args = p.parse_args()
    if args.pid is not None and (not args.out or not 1 <= args.seconds <= 120):
        p.error('A take requires --out and a duration of 1 to 120 seconds')
    image = args.image.resolve()
    identity, rvas = probe_map(image)
    sha = hashlib.sha256(image.read_bytes()).hexdigest()
    print(f'{sha} / PDB {identity["guid"]}:{identity["age"]} / {len(rvas)} probes', flush=True)
    if args.pid is not None:
        rows = record(args.pid, args.seconds, identity, rvas)
        report = dict(pid=args.pid, image=str(image), sha256=sha, identity=identity, samples=rows)
        args.out.write_text(json.dumps(report, allow_nan=False) + '\n', encoding='utf-8')
        print(f'{len(rows)} observed solves written to {args.out}')
        if len(rows) < 2:
            sys.exit('Insufficient fresh pose solves')
