#!/usr/bin/env python3
"""Read the port's dllexport counters out of the running game and print WINDOWED DELTAS.

Why this exists: the counters the plugin exports are cumulative, and comparing cumulative
counters is how three "defects" turned out to be artefacts (they include the menu, the
loading screens and every period the process spent on a breakpoint). The only reading that
means anything is the change over a fixed window while the user is doing one named thing.

    python tools/live/probe.py --preset pose --window 2
    python tools/live/probe.py --preset body --window 2
    python tools/live/probe.py --list Vrik            # what is exported at all
    python tools/live/probe.py CyberpunkVR_DebugFinalNoMatch:u64 ...

No debugger, no breakpoints: OpenProcess + ReadProcessMemory over the export table, so the
game keeps running at full speed while it is measured.
"""

import argparse
import ctypes
import ctypes.wintypes as wt
import struct
import sys
import time

PROCESS_QUERY_INFORMATION = 0x0400
PROCESS_VM_READ = 0x0010
PROCESS_VM_WRITE = 0x0020
PROCESS_VM_OPERATION = 0x0008
LIST_MODULES_ALL = 0x03

k32 = ctypes.WinDLL("kernel32", use_last_error=True)
psapi = ctypes.WinDLL("psapi", use_last_error=True)

k32.OpenProcess.restype = wt.HANDLE
k32.OpenProcess.argtypes = [wt.DWORD, wt.BOOL, wt.DWORD]
k32.ReadProcessMemory.argtypes = [wt.HANDLE, wt.LPCVOID, wt.LPVOID,
                                  ctypes.c_size_t, ctypes.POINTER(ctypes.c_size_t)]
k32.WriteProcessMemory.argtypes = [wt.HANDLE, wt.LPVOID, wt.LPCVOID,
                                   ctypes.c_size_t, ctypes.POINTER(ctypes.c_size_t)]
# HMODULE is a pointer: without argtypes ctypes narrows it to a C int and a real module
# handle overflows.
psapi.GetModuleBaseNameW.argtypes = [wt.HANDLE, wt.HMODULE, wt.LPWSTR, wt.DWORD]
psapi.GetModuleBaseNameW.restype = wt.DWORD
psapi.EnumProcessModulesEx.argtypes = [wt.HANDLE, ctypes.POINTER(wt.HMODULE), wt.DWORD,
                                       ctypes.POINTER(wt.DWORD), wt.DWORD]
psapi.EnumProcessModulesEx.restype = wt.BOOL
psapi.EnumProcesses.argtypes = [ctypes.POINTER(wt.DWORD), wt.DWORD,
                                ctypes.POINTER(wt.DWORD)]
psapi.EnumProcesses.restype = wt.BOOL


def find_pid(name="Cyberpunk2077.exe"):
    """PID by image name, without a WMI/tasklist round trip."""
    arr = (wt.DWORD * 4096)()
    got = wt.DWORD()
    if not psapi.EnumProcesses(ctypes.cast(arr, ctypes.POINTER(wt.DWORD)),
                               ctypes.sizeof(arr), ctypes.byref(got)):
        raise OSError("EnumProcesses failed")
    want = name.lower()
    for i in range(got.value // ctypes.sizeof(wt.DWORD)):
        pid = arr[i]
        h = k32.OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, False, pid)
        if not h:
            continue
        try:
            buf = ctypes.create_unicode_buffer(260)
            if psapi.GetModuleBaseNameW(h, None, buf, 260) and buf.value.lower() == want:
                return pid
        finally:
            k32.CloseHandle(h)
    return None


class Target:
    def __init__(self, pid, writable=False):
        access = PROCESS_QUERY_INFORMATION | PROCESS_VM_READ
        if writable:
            access |= PROCESS_VM_WRITE | PROCESS_VM_OPERATION
        self.h = k32.OpenProcess(access, False, pid)
        if not self.h:
            raise OSError(f"OpenProcess({pid}) failed: {ctypes.get_last_error()}")
        self.pid = pid

    def read(self, addr, size):
        buf = (ctypes.c_char * size)()
        done = ctypes.c_size_t()
        if not k32.ReadProcessMemory(self.h, ctypes.c_void_p(addr), buf, size,
                                     ctypes.byref(done)) or done.value != size:
            return None
        return bytes(buf)

    def write(self, addr, data):
        done = ctypes.c_size_t()
        buf = (ctypes.c_char * len(data)).from_buffer_copy(data)
        ok = k32.WriteProcessMemory(self.h, ctypes.c_void_p(addr), buf, len(data),
                                    ctypes.byref(done))
        return bool(ok) and done.value == len(data)

    def module_base(self, name):
        mods = (wt.HMODULE * 2048)()
        got = wt.DWORD()
        if not psapi.EnumProcessModulesEx(self.h,
                                          ctypes.cast(mods, ctypes.POINTER(wt.HMODULE)),
                                          ctypes.sizeof(mods),
                                          ctypes.byref(got), LIST_MODULES_ALL):
            raise OSError("EnumProcessModulesEx failed")
        want = name.lower()
        # the plugin's file up to 0.1.9, for a game still running an older build
        alias = {'cyberpunkvrport.dll': 'cyberpunkvr_stereo.dll', 'cyberpunkvr_stereo.dll': 'cyberpunkvrport.dll'}
        wants = {want, alias.get(want, want)}
        for i in range(got.value // ctypes.sizeof(wt.HMODULE)):
            buf = ctypes.create_unicode_buffer(260)
            if psapi.GetModuleBaseNameW(self.h, mods[i], buf, 260) \
                    and buf.value.lower() in wants:
                return mods[i]
        return None


def exports(t, base):
    """{name: absolute address} from the module's export directory, read out of the process."""
    dos = t.read(base, 0x40)
    if not dos or dos[:2] != b"MZ":
        raise OSError("not a PE at the module base")
    e_lfanew = struct.unpack_from("<I", dos, 0x3C)[0]
    nt = t.read(base + e_lfanew, 0x108)
    if nt[:4] != b"PE\0\0":
        raise OSError("no PE signature")
    # optional header magic: 0x20b = PE32+, and the data directory then starts at +0x70
    magic = struct.unpack_from("<H", nt, 0x18)[0]
    dd = 0x18 + (0x70 if magic == 0x20B else 0x60)
    exp_rva, exp_size = struct.unpack_from("<II", nt, dd)
    if not exp_rva:
        return {}
    d = t.read(base + exp_rva, 40)
    n_funcs, n_names = struct.unpack_from("<II", d, 0x14)
    a_funcs, a_names, a_ords = struct.unpack_from("<III", d, 0x1C)
    funcs = t.read(base + a_funcs, 4 * n_funcs)
    names = t.read(base + a_names, 4 * n_names)
    ords = t.read(base + a_ords, 2 * n_names)
    out = {}
    for i in range(n_names):
        nrva = struct.unpack_from("<I", names, 4 * i)[0]
        # names are short; one page is always enough and one read per symbol is cheap here
        raw = t.read(base + nrva, 128) or b""
        nm = raw.split(b"\0", 1)[0].decode("ascii", "replace")
        o = struct.unpack_from("<H", ords, 2 * i)[0]
        if o < n_funcs:
            out[nm] = base + struct.unpack_from("<I", funcs, 4 * o)[0]
    return out


FMT = {"u64": ("<Q", 8), "i64": ("<q", 8), "u32": ("<I", 4), "i32": ("<i", 4),
       "f32": ("<f", 4), "f64": ("<d", 8)}


def decode(t, addr, kind):
    if kind.endswith("]") and "[" in kind:
        base_kind, n = kind[:-1].split("[")
        n = int(n)
        f, sz = FMT[base_kind]
        raw = t.read(addr, sz * n)
        if raw is None:
            return None
        return tuple(struct.unpack_from(f, raw, sz * i)[0] for i in range(n))
    f, sz = FMT[kind]
    raw = t.read(addr, sz)
    return None if raw is None else struct.unpack(f, raw)[0]


# name:kind, and whether the interesting reading is the DELTA over the window (counters)
# or the LIVE value (flags, angles, positions).
PRESETS = {
    # Symptom "the whole picture judders on a head turn": does every submitted frame carry
    # the pose it was actually rendered with, or does it fall back to the raw locate?
    #
    # THE COMPOSITION COUNTERS ARE HERE ON PURPOSE. A climbing `nomatch` on its own proves
    # nothing: a menu frame, a cutscene and a frame the engine composed without our write all
    # produce one, and the cumulative totals are full of those. `Composed` is the discriminator
    # -- it counts the frames on which a quaternion actually went into the ring. Composed ~=
    # submits WITH nomatch still climbing is a real disagreement between the ring and the render
    # camera; Composed far below submits means there was simply nothing to find.
    "pose": [
        ("CyberpunkVR_DebugMonoSubmits", "u64", "d"),
        ("CyberpunkVR_DebugXrCycles", "u64", "d"),
        ("CyberpunkVR_DebugCamComposed", "u64", "d"),
        ("CyberpunkVR_DebugCamNoHmd", "u64", "d"),
        ("CyberpunkVR_DebugCamVrcamFirst", "u64", "d"),
        ("CyberpunkVR_DebugViewYawFromEngine", "u64", "d"),
        ("CyberpunkVR_DebugFinalExact", "u64", "d"),
        ("CyberpunkVR_DebugFinalApprox", "u64", "d"),
        ("CyberpunkVR_DebugFinalNoMatch", "u64", "d"),
        ("CyberpunkVR_DebugPoseFromWrite", "u64", "d"),
        ("CyberpunkVR_DebugPoseFromSlot", "u64", "d"),
        ("CyberpunkVR_DebugPoseReadBack", "u64", "d"),
        ("CyberpunkVR_DebugFinalAge", "u32", "v"),
        ("CyberpunkVR_PoseReadBack", "i32", "v"),
    ],
    # Symptom "walking backwards I see my own body": is the anchor pair coming from the
    # plugin's own two halves, and does the census still recognise the player while moving?
    "body": [
        ("CyberpunkVR_VrikTransformsFromPlugin", "i32", "v"),
        ("CyberpunkVR_PlayerEntityValid", "i32", "v"),
        ("CyberpunkVR_PlayerEntityPos", "f32[3]", "v"),
        ("CyberpunkVR_PlayerEntityQuat", "f32[4]", "v"),
        ("CyberpunkVR_DebugYawWritesPlayer", "u64", "d"),
        ("CyberpunkVR_DebugYawWritesAll", "u64", "d"),
        # Is the anchor pair actually coming from the plugin's own two per-frame halves, or
        # did it fall back to the Lua push for this frame? A fallback is not an error the
        # code can report -- it is the old behaviour returning silently -- so the only way
        # to know the fix is live is to watch these three next to each other.
        ("CyberpunkVR_VrikNativeFramePair", "i32", "v"),
        ("CyberpunkVR_DebugVrikNativePairUsed", "u64", "d"),
        ("CyberpunkVR_DebugVrikLuaPairFallback", "u64", "d"),
        ("CyberpunkVR_DebugVrikNativePairPublished", "u64", "d"),
        ("CyberpunkVR_DebugVrikNativePairRejected", "u64", "d"),
        ("CyberpunkVR_DebugVrikPairAgeMaxFrames", "f32", "p"),
        ("CyberpunkVR_DebugVrikPairStaleSolves", "u64", "d"),
        ("CyberpunkVR_DebugVrikNativePairPhaseMiss", "u64", "d"),
        # How many of the accepted native pairs were one epoch old rather than current. The
        # read gate accepts both, so without this column "the pair is current" and "the pair
        # is a frame behind" are indistinguishable -- and a frame behind is the exact thing
        # the native pair was introduced to remove.
        ("CyberpunkVR_DebugVrikNativePairPreviousEpochUsed", "u64", "d"),
        # THE ANGULAR INSTRUMENT. [vrikshake] measures the wrist target's POSITION, so a defect
        # that rotates the shoulder girdle while the wrist stays put is invisible to it. These
        # are the missing half. The two Step values are running maxima the plugin never clears,
        # so they are read in 'p' mode: zeroed at the start of the window, read at the end, which
        # makes each printed number the peak for that window and nothing older.
        ("CyberpunkVR_DebugArmFrameFromBody", "u64", "d"),
        ("CyberpunkVR_DebugArmFrameCameraFallback", "u64", "d"),
        ("CyberpunkVR_DebugShoulderLineYawDeg", "f32", "v"),
        # Reach = the roomscale offset the placement must absorb (measured before it runs).
        # Miss = what is left after it ran. Only meaningful with EyeBakePublish = 0, which
        # opens the loop through the renderer that made every earlier miss reading circular.
        ("CyberpunkVR_VrikFinalIkSpine", "i32", "v"),
        ("CyberpunkVR_VrikBodyPosStiffness", "f32", "v"),
        ("CyberpunkVR_VrikBodyRotStiffness", "f32", "v"),
        ("CyberpunkVR_VrikNeckStiffness", "f32", "v"),
        ("CyberpunkVR_VrikChestClampWeight", "f32", "v"),
        ("CyberpunkVR_VrikHeadClampWeight", "f32", "v"),
        ("CyberpunkVR_VrikMaintainPelvisPos", "f32", "v"),
        ("CyberpunkVR_VrikMoveBodyBackCrouch", "f32", "v"),
        ("CyberpunkVR_VrikMinHeadHeightM", "f32", "v"),
        ("CyberpunkVR_VrikLeanRadiusM", "f32", "v"),
        ("CyberpunkVR_VrikStepRateMps", "f32", "v"),
        ("CyberpunkVR_VrikPairBodyPreHmd", "i32", "v"),
        ("CyberpunkVR_DebugPairPreHmdStepMm", "f32", "p"),
        ("CyberpunkVR_DebugPairFullStepMm", "f32", "p"),
        ("CyberpunkVR_ViewPktFromPatch", "i32", "v"),
        ("CyberpunkVR_VrikAnchorFromXr", "i32", "v"),
        ("CyberpunkVR_DebugHeadVsCamDeg", "f32", "v"),
        ("CyberpunkVR_DebugAnchorStepMm", "f32", "p"),
        ("CyberpunkVR_DebugCamModelStepMm", "f32", "p"),
        ("CyberpunkVR_DebugXrOffStepMm", "f32", "p"),
        ("CyberpunkVR_DebugXrAnchorOffsetMm", "f32", "v"),
        ("CyberpunkVR_VrikRootAlign", "i32", "v"),
        ("CyberpunkVR_DebugAnimHipsFwdMm", "f32", "v"),
        ("CyberpunkVR_DebugAnimChestFwdMm", "f32", "v"),
        ("CyberpunkVR_DebugAnimHeadUpMm", "f32", "v"),
        ("CyberpunkVR_DebugAnimHeadFwdMm", "f32", "v"),
        ("CyberpunkVR_DebugAnimHeadRightMm", "f32", "v"),
        ("CyberpunkVR_DebugSpineBones", "i32", "v"),
        ("CyberpunkVR_DebugSpineChestIdx", "i32", "v"),
        ("CyberpunkVR_DebugSpineNeckIdx", "i32", "v"),
        ("CyberpunkVR_DebugSpineSlackMm", "f32", "v"),
        ("CyberpunkVR_DebugPelvisFwdMm", "f32", "v"),
        ("CyberpunkVR_DebugPelvisRightMm", "f32", "v"),
        ("CyberpunkVR_DebugChestFwdMm", "f32", "v"),
        ("CyberpunkVR_DebugLeanOffsetMm", "f32", "v"),
        ("CyberpunkVR_VrikArmFrameFromChestRot", "i32", "v"),
        ("CyberpunkVR_VrikEyeBakePublish", "i32", "v"),
        ("CyberpunkVR_DebugHeadReachMm", "f32", "p"),
        ("CyberpunkVR_DebugShoulderLineStepDeg", "f32", "p"),
        ("CyberpunkVR_DebugBodyUpStepDeg", "f32", "p"),
        # Which reference the up axis is measured from, and whether the median patch is on top.
        # Both are live: chest=1/median=0 is the fix, chest=0/median=0 is the old behaviour, and
        # chest=0/median=1 is the symptom patch -- three states, no rebuild.
        # The last unmeasured quantity in the body solve: how far the head bone ends up from
        # the anchor the whole solve aims it at. Peak per window, cleared here.
        ("CyberpunkVR_DebugHeadAnchorMissMm", "f32", "p"),
        ("CyberpunkVR_VrikElbowPolicy", "i32", "v"),
        ("CyberpunkVR_VrikArmAnchorFromBody", "i32", "v"),
        ("CyberpunkVR_BodyYawFinalRad", "f32", "v"),
        ("CyberpunkVR_BodyYawFinalValid", "i32", "v"),
        ("CyberpunkVR_DebugYawWriteMs", "f64", "v"),
    ],
}


def derive(delta):
    """The three fractions the raw deltas are read for, computed only from THIS window.

    Each one exists because the corresponding cumulative total cannot answer the question:

      identified  -- of the frames on which the render side actually attempted to recognise
                     the camera quaternion, how many found it. The denominator is attempts,
                     not frames: a frame with no attempt is not a failure.
      composed    -- did a quaternion go into the ring at all, per submitted frame. This is
                     what separates "the ring and the render camera disagree" (composed ~= 1)
                     from "there was nothing to find" (composed well under 1: menu, cutscene,
                     a camera the engine built without us).
      labelled    -- of the submitted frames, how many carried the pose identified by the
                     read-back rather than a serial-keyed guess or the raw slot locate.
    """
    def d(name):
        return delta.get("CyberpunkVR_Debug" + name)

    exact, nomatch = d("FinalExact"), d("FinalNoMatch")
    approx = d("FinalApprox") or 0
    submits = d("MonoSubmits")
    composed, readback = d("CamComposed"), d("PoseReadBack")
    parts = []
    if exact is not None and nomatch is not None:
        tries = exact + approx + nomatch
        parts.append(f"identified {exact + approx}/{tries}"
                     + (f" = {100.0 * (exact + approx) / tries:.1f}%" if tries else ""))
    if composed is not None and submits:
        parts.append(f"composed/submit {composed}/{submits} = {composed / submits:.2f}")
    if readback is not None and submits:
        parts.append(f"labelled {readback}/{submits} = {100.0 * readback / submits:.1f}%")
    return " | ".join(parts)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("symbols", nargs="*", help="NAME[:kind][:d|v]  kind = u64/i32/f32/f32[3]/...")
    ap.add_argument("--preset", choices=sorted(PRESETS), action="append", default=[])
    ap.add_argument("--module", default="cyberpunkvrport.dll")
    ap.add_argument("--process", default="Cyberpunk2077.exe")
    ap.add_argument("--window", type=float, default=2.0, help="seconds per printed line")
    ap.add_argument("--count", type=int, default=0, help="lines to print (0 = forever)")
    ap.add_argument("--list", metavar="SUBSTR", nargs="?", const="",
                    help="print matching export names and exit")
    ap.add_argument("--set", metavar="NAME:kind=VALUE", action="append", default=[],
                    help="write a live value (A/B a flag with no rebuild)")
    args = ap.parse_args()

    pid = find_pid(args.process)
    if pid is None:
        print(f"{args.process} is not running", file=sys.stderr)
        return 2
    # 'p' fields are zeroed at the top of every window, so the handle has to be writable even
    # when --set was not asked for.
    wantsPeak = any(len(s.split(":")) > 2 and s.split(":")[2] == "p" for s in args.symbols) \
        or any(mode == "p" for p in args.preset for _, _, mode in PRESETS[p])
    try:
        t = Target(pid, writable=bool(args.set) or wantsPeak)
    except OSError:
        # Write access can be refused where read access is not (elevation mismatch). Degrade
        # loudly rather than not measuring at all: the peaks then read as running maxima, which
        # is still usable as long as the caller knows it.
        if args.set:
            raise
        print("! write access refused -- 'p' columns are cumulative maxima, not per-window peaks",
              file=sys.stderr)
        wantsPeak = False
        t = Target(pid, writable=False)
    base = t.module_base(args.module)
    if base is None:
        print(f"{args.module} is not loaded in pid {pid}", file=sys.stderr)
        return 2
    ex = exports(t, base)
    print(f"# pid {pid}  {args.module} @ 0x{base:X}  {len(ex)} exports")

    if args.list is not None:
        for nm in sorted(ex):
            if args.list.lower() in nm.lower():
                print(f"  {nm}")
        return 0

    for spec in args.set:
        nm_kind, _, val = spec.partition("=")
        nm, _, kind = nm_kind.partition(":")
        kind = kind or "i32"
        if nm not in ex:
            print(f"! {nm} is not exported", file=sys.stderr)
            continue
        f, sz = FMT[kind]
        raw = struct.pack(f, float(val) if kind.startswith("f") else int(val))
        ok = t.write(ex[nm], raw)
        print(f"# set {nm}:{kind} = {val}  {'ok' if ok else 'FAILED'}")

    fields = []
    for p in args.preset:
        fields.extend(PRESETS[p])
    for s in args.symbols:
        parts = s.split(":")
        nm = parts[0]
        kind = parts[1] if len(parts) > 1 else "u64"
        mode = parts[2] if len(parts) > 2 else ("d" if kind.startswith(("u", "i")) else "v")
        fields.append((nm, kind, mode))
    if not fields:
        fields = PRESETS["pose"]

    missing = [nm for nm, _, _ in fields if nm not in ex]
    for nm in missing:
        print(f"! not exported: {nm}", file=sys.stderr)
    fields = [f for f in fields if f[0] in ex]
    if not fields:
        return 2

    prev = {}
    line = 0
    print(f"# window {args.window:.2f}s | 'd' columns are the CHANGE over the window, "
          f"'v' columns the live value")
    try:
        while args.count == 0 or line < args.count:
            # Zero the running maxima FIRST, so what is read at the end of the window is that
            # window's peak. Without this the plugin's value is the largest since the process
            # started, which never falls and therefore never distinguishes one action from
            # another -- the same trap the cumulative counters set.
            for nm, kind, mode in fields:
                if mode == "p" and wantsPeak:
                    f, _ = FMT[kind]
                    t.write(ex[nm], struct.pack(f, 0.0 if kind.startswith("f") else 0))
            t0 = time.perf_counter()
            time.sleep(args.window)
            dt = time.perf_counter() - t0
            out = [f"[{dt:5.2f}s]"]
            delta = {}
            for nm, kind, mode in fields:
                v = decode(t, ex[nm], kind)
                short = nm.replace("CyberpunkVR_Debug", "").replace("CyberpunkVR_", "")
                if v is None:
                    out.append(f"{short}=?")
                elif mode == "d" and not isinstance(v, tuple):
                    p = prev.get(nm)
                    if p is not None:
                        delta[nm] = v - p
                    d = "-" if p is None else f"{v - p:+d}" if isinstance(v, int) \
                        else f"{v - p:+.4f}"
                    out.append(f"{short}={d}")
                    prev[nm] = v
                elif isinstance(v, tuple):
                    out.append(f"{short}=({','.join(f'{x:.4f}' for x in v)})")
                elif isinstance(v, float):
                    out.append(f"{short}={v:.4f}")
                else:
                    out.append(f"{short}={v}")
            print(" ".join(out), flush=True)
            summary = derive(delta)
            if summary:
                print(f"          -> {summary}", flush=True)
            line += 1
    except KeyboardInterrupt:
        pass
    return 0


if __name__ == "__main__":
    sys.exit(main())
