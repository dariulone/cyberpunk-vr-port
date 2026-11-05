"""2.5 engine stereo: which passes trace rays, how often per frame and over what size (reflections showed one
perspective in both eyes with RT on, 2026-11-01).

The reflection raygens (rgs_reflection_opaque_main / _transparent_main, staticshader_final.cache) take the pixel
from DispatchRaysIndex with no fold into an eye's half and read the eye from their own block (gCommonConstants
row 204.w) and a layout flag (row 206.x) beside the two-halves flag (b0 row 32.x). The engine also has
CRenderNode_PrepareStereoRayTracing / MergeStereoRayTracing. Three shapes fit that, and each needs another fix:

  per eye, whole frame   two DispatchRays per frame at the full side-by-side width: the last eye wins
                         (the planar SSR bug, fixed by an eye_half_exit twin of the raygen)
  per eye, own half      two at half width: the eyes trace apart; look at the denoiser / RR instead
  once, shared layout    one DispatchRays per frame, Prepare/Merge around it: one camera for both eyes,
                         reprojected at the surface's depth -- the reflections are painted on

This turns the plugin's shader census on for a few seconds and prints every node that traced rays, with its
DispatchRays size and count per frame, and the compute work of the nodes around it (Prepare / Merge show up as
unnamed RVAs). Needs `cvr_shader_census.txt` in the plugin's log folder at game start (Render/HiddenAreaShaders.cpp
Recording) and the plugin's PDB beside the installed DLL.

  py -3.11 -B rt_dispatch_census.py --pid PID [--seconds 3]
"""
import argparse
import ctypes as c
import re
import struct
import sys
import time
from ctypes import wintypes as w
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tools/swimming_tests'))
from live_symbols import Module

DLL_NAMES = ('cyberpunkvrport.dll', 'CyberpunkVR_Stereo.dll')     # 0.2.0 on, 0.1.9 and older
RECORD = struct.Struct('<IiIIIIQIIQ')        # CensusRecord: node side kind x y z hash count bytes pso
RECORDS = 512
KIND = {1: 'Dispatch', 2: 'ExecuteIndirect', 3: 'Draw', 4: 'DrawIndexed', 5: 'DispatchRays'}
RT_NODES = {0xCEEA0C, 0x778C04, 0x22A6820, 0xD632DC, 0xE3F9E0, 0xE9A9B0, 0xE9B338, 0xE9BBC4, 0x77F47C}


def node_names():
    names = {}
    for rva, name in re.findall(r'\{ 0x([0-9A-Fa-f]+), "([^"]+)" \}', (ROOT / 'src/Stereo/NodeNames.inc').read_text()):
        names[int(rva, 16)] = name
    return names


def resolve_exports(pid, names):
    """Addresses of the plugin's exported variables in the running game: its export table (no PDB needed --
    the shipped plugin has none) plus the base the process loaded it at."""
    k = c.WinDLL('kernel32', use_last_error=True)
    k.CreateToolhelp32Snapshot.argtypes = [w.DWORD, w.DWORD]
    k.CreateToolhelp32Snapshot.restype = w.HANDLE
    k.Module32FirstW.argtypes = [w.HANDLE, c.POINTER(Module)]
    k.Module32NextW.argtypes = k.Module32FirstW.argtypes
    k.CloseHandle.argtypes = [w.HANDLE]
    snap = k.CreateToolhelp32Snapshot(0x18, pid)
    if snap == c.c_void_p(-1).value:
        raise c.WinError(c.get_last_error())
    m = Module()
    m.size = c.sizeof(m)
    found = None
    try:
        more = k.Module32FirstW(snap, c.byref(m))
        while more:
            if m.name.lower() in (n.lower() for n in DLL_NAMES):
                found = (m.base, Path(m.path))
                break
            more = k.Module32NextW(snap, c.byref(m))
    finally:
        k.CloseHandle(snap)
    if not found:
        raise RuntimeError('neither %s is loaded in process %d' % (' nor '.join(DLL_NAMES), pid))
    base, path = found
    table = exports(path)
    missing = [n for n in names if n not in table]
    if missing:
        raise RuntimeError('not exported by %s: %s' % (path, ', '.join(missing)))
    return {n: base + table[n] for n in names}


def exports(path):
    """name -> RVA of every named export of a PE32+ file."""
    pe = path.read_bytes()
    nt = struct.unpack_from('<I', pe, 0x3C)[0]
    sections = [struct.unpack_from('<8sIIII', pe, nt + 24 + struct.unpack_from('<H', pe, nt + 20)[0] + 40 * i)
                for i in range(struct.unpack_from('<H', pe, nt + 6)[0])]

    def offset(rva):
        for _, vsize, vaddr, rsize, raw in sections:
            if vaddr <= rva < vaddr + max(vsize, rsize):
                return rva - vaddr + raw
        raise ValueError('rva %#x outside the sections' % rva)

    export_rva = struct.unpack_from('<I', pe, nt + 24 + 112)[0]       # PE32+ data directory 0
    e = offset(export_rva)
    count, = struct.unpack_from('<I', pe, e + 24)                      # IMAGE_EXPORT_DIRECTORY.NumberOfNames
    functions, names_at, ordinals = struct.unpack_from('<III', pe, e + 28)
    table = {}
    for i in range(count):
        at = offset(struct.unpack_from('<I', pe, offset(names_at) + 4 * i)[0])
        name = pe[at:pe.index(b'\0', at)].decode()
        ordinal = struct.unpack_from('<H', pe, offset(ordinals) + 2 * i)[0]
        table[name] = struct.unpack_from('<I', pe, offset(functions) + 4 * ordinal)[0]
    return table


class Process:
    def __init__(self, pid):
        self.k = c.WinDLL('kernel32', use_last_error=True)
        self.k.OpenProcess.restype = w.HANDLE
        self.k.OpenProcess.argtypes = [w.DWORD, w.BOOL, w.DWORD]
        self.k.WriteProcessMemory.argtypes = [w.HANDLE, c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]
        self.k.ReadProcessMemory.argtypes = self.k.WriteProcessMemory.argtypes
        self.h = self.k.OpenProcess(0x0008 | 0x0020 | 0x0010, False, pid)
        if not self.h:
            raise c.WinError(c.get_last_error())

    def read(self, address, size):
        b = c.create_string_buffer(size)
        n = c.c_size_t()
        if not self.k.ReadProcessMemory(self.h, address, b, size, c.byref(n)) or n.value != size:
            raise RuntimeError('read failed at %#x' % address)
        return b.raw

    def write(self, address, data):
        n = c.c_size_t()
        if not self.k.WriteProcessMemory(self.h, address, data, len(data), c.byref(n)):
            raise RuntimeError('write failed at %#x' % address)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--pid', type=int, required=True)
    ap.add_argument('--seconds', type=float, default=3.0)
    ap.add_argument('--nodes', default='', help='comma-separated node RVAs to list instead of the RT set (e.g. the SSR '
                                                'chain: 0x77A100,0x77D450,0x77E104,0xDD3D24,0xDAC960,0xE9CE6C)')
    a = ap.parse_args()
    if a.nodes:
        RT_NODES.clear()
        RT_NODES.update(int(n, 0) for n in a.nodes.split(','))
    at = resolve_exports(a.pid, ['CyberpunkVR_ShaderCensus', 'CyberpunkVR_ShaderCensusLog',
                                 'CyberpunkVR_ShaderCensusRecording', 'CyberpunkVR_HudSubmittedFrames',
                                 'CyberpunkVR_RuntimeDiagnostics'])
    p = Process(a.pid)
    if struct.unpack('<I', p.read(at['CyberpunkVR_ShaderCensusRecording'], 4))[0] != 1:
        sys.exit('census not recording: put cvr_shader_census.txt in the plugin log folder and restart the game')
    p.write(at['CyberpunkVR_ShaderCensus'], struct.pack('<I', 0))
    time.sleep(0.2)
    p.write(at['CyberpunkVR_ShaderCensusLog'], bytes(RECORD.size * RECORDS))
    # the frame counter only counts with the runtime diagnostics on (Utils/DebugGate.hpp)
    diagnostics = p.read(at['CyberpunkVR_RuntimeDiagnostics'], 4)
    p.write(at['CyberpunkVR_RuntimeDiagnostics'], struct.pack('<i', 1))
    f0 = struct.unpack('<Q', p.read(at['CyberpunkVR_HudSubmittedFrames'], 8))[0]
    p.write(at['CyberpunkVR_ShaderCensus'], struct.pack('<I', 1))
    time.sleep(a.seconds)
    p.write(at['CyberpunkVR_ShaderCensus'], struct.pack('<I', 0))
    f1 = struct.unpack('<Q', p.read(at['CyberpunkVR_HudSubmittedFrames'], 8))[0]
    p.write(at['CyberpunkVR_RuntimeDiagnostics'], diagnostics)
    time.sleep(0.2)
    raw = p.read(at['CyberpunkVR_ShaderCensusLog'], RECORD.size * RECORDS)
    rows = [RECORD.unpack_from(raw, i * RECORD.size) for i in range(RECORDS)]
    rows = [r for r in rows if r[7]]
    frames = max(f1 - f0, 1)
    names = node_names()
    print('%d frames, %d rows%s' % (frames, len(rows), ' (log full: shorten --seconds)' if len(rows) == RECORDS else ''))

    traced = sorted({r[0] for r in rows if r[2] == 5})
    print('\nnodes that traced rays:')
    for node in traced:
        for r in sorted((r for r in rows if r[0] == node and r[2] == 5), key=lambda r: (r[1], r[3])):
            print('  %-34s side %2d  DispatchRays %5d x %5d x %d  %.2f per frame'
                  % ('%#x %s' % (node, names.get(node, '?')), r[1], r[3], r[4], r[5], r[7] / frames))
    if not traced:
        print('  none -- is ray tracing on, and are reflections in view?')

    # every node in the RT set, and every unnamed node, with its compute and draw work: Prepare / Merge
    # StereoRayTracing have no entry in NodeNames.inc, so they show here by RVA
    print('\ncompute and full-screen work of the RT nodes and of unnamed nodes:')
    for r in sorted(rows, key=lambda r: (r[0], r[1], r[2], r[3], r[4])):
        if r[2] == 5 or not (r[0] in RT_NODES or r[0] not in names):
            continue
        print('  %-34s side %2d  %-15s %5d x %5d x %d  shader %016x  %.2f per frame'
              % ('%#x %s' % (r[0], names.get(r[0], '?')), r[1], KIND.get(r[2], r[2]), r[3], r[4], r[5], r[6], r[7] / frames))


if __name__ == '__main__':
    main()
