r"""Read a REDengine crash dump and say which of OUR code faulted.

    python tools\crash\read_dump.py <path to Cyberpunk2077.dmp> [more.dmp ...]

Why this exists: a crash landed inside CyberpunkVR_Stereo.dll and there was nothing on the machine
to open the dump with -- no cdb, no windbg, and dumpbin only reads PE files. REDengine's own
stacktrace.txt is three lines and never names a module. Placing the fault took disassembling the
whole image by hand once; this does it in a second, every time.

Two halves:

  * The minidump is parsed directly -- module list, exception record, thread context, memory --
    so the faulting address always resolves at least to module+RVA. That much needs nothing
    installed and works on a tester's dump too.
  * If a PDB for one of our modules is in the build tree, dbghelp turns module+RVA into a function
    name and a source line. Release now builds with /Zi /DEBUG for exactly this.

A note on the stack: this is a SCAN, not an unwind. Real unwinding needs the PE unwind tables, and
an optimised frame pointer-less build gives no shortcut. Every 8-aligned qword on the stack that
lands inside a loaded module is printed in order, so the genuine return addresses are in the list
mixed with stale slots -- read it top-down and the first few are the recent frames. Anything it
prints from our own module is worth looking at; the rest is noise by construction.
"""
import collections
import argparse
import ctypes
import ctypes.wintypes as wt
import glob
import os
import struct
import sys

OURS = ("cyberpunkvrport", "CyberpunkVR_Stereo", "CyberpunkVR_Hands", "XR_APILAYER_CPVR_probe")
REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))


# ---- dbghelp ---------------------------------------------------------------------------------
class SYMBOL_INFO(ctypes.Structure):
    _fields_ = [("SizeOfStruct", wt.ULONG), ("TypeIndex", wt.ULONG), ("Reserved", ctypes.c_ulonglong * 2),
                ("Index", wt.ULONG), ("Size", wt.ULONG), ("ModBase", ctypes.c_ulonglong),
                ("Flags", wt.ULONG), ("Value", ctypes.c_ulonglong), ("Address", ctypes.c_ulonglong),
                ("Register", wt.ULONG), ("Scope", wt.ULONG), ("Tag", wt.ULONG),
                ("NameLen", wt.ULONG), ("MaxNameLen", wt.ULONG), ("Name", ctypes.c_char * 1024)]


class IMAGEHLP_LINE64(ctypes.Structure):
    _fields_ = [("SizeOfStruct", wt.DWORD), ("Key", ctypes.c_void_p), ("LineNumber", wt.DWORD),
                ("FileName", ctypes.c_char_p), ("Address", ctypes.c_ulonglong)]


class Symbols:
    """dbghelp wrapper. Silently degrades to module+RVA when a PDB is not around."""

    def __init__(self, image_dir=None):
        self.ok = False
        self.image_dir = os.path.abspath(image_dir) if image_dir else None
        try:
            self.dh = ctypes.WinDLL("dbghelp.dll")
        except OSError:
            return
        self.h = ctypes.c_void_p(0x5EED)          # any unique token; no live process is read
        self.dh.SymSetOptions(0x00000002 | 0x00000004 | 0x00000010 | 0x400 | 0x80000)
        search = self.image_dir or ";".join(sorted({os.path.dirname(p) for p in
                                  glob.glob(os.path.join(REPO, "build", "**", "*.pdb"), recursive=True)}))
        self.dh.SymInitialize.argtypes = [ctypes.c_void_p, ctypes.c_char_p, wt.BOOL]
        self.ok = bool(self.dh.SymInitialize(self.h, search.encode() or None, False))
        self.dh.SymLoadModuleExW.restype = ctypes.c_ulonglong
        self.dh.SymFromAddr.argtypes = [ctypes.c_void_p, ctypes.c_ulonglong,
                                        ctypes.POINTER(ctypes.c_ulonglong), ctypes.POINTER(SYMBOL_INFO)]
        self.loaded = set()

    def load(self, name, base, size):
        """Point dbghelp at the built copy of one of our modules."""
        if not self.ok or base in self.loaded:
            return
        hits = ([os.path.join(self.image_dir,name)] if self.image_dir else
                glob.glob(os.path.join(REPO, "build", "**", name), recursive=True))
        hits = [p for p in hits if os.path.isfile(p)]
        if not hits:
            return
        self.loaded.add(base)
        self.dh.SymLoadModuleExW(self.h, None, ctypes.c_wchar_p(hits[0]), None,
                                 ctypes.c_ulonglong(base), ctypes.c_ulong(size), None, 0)

    def resolve(self, addr):
        if not self.ok:
            return None
        si = SYMBOL_INFO()
        si.SizeOfStruct = ctypes.sizeof(SYMBOL_INFO) - 1024
        si.MaxNameLen = 1023
        disp = ctypes.c_ulonglong(0)
        if not self.dh.SymFromAddr(self.h, ctypes.c_ulonglong(addr), ctypes.byref(disp), ctypes.byref(si)):
            return None
        out = si.Name.decode("utf-8", "replace")
        if disp.value:
            out += "+0x%X" % disp.value
        line = IMAGEHLP_LINE64()
        line.SizeOfStruct = ctypes.sizeof(IMAGEHLP_LINE64)
        d32 = wt.DWORD(0)
        if self.dh.SymGetLineFromAddr64(self.h, ctypes.c_ulonglong(addr), ctypes.byref(d32),
                                        ctypes.byref(line)) and line.FileName:
            out += "   %s:%d" % (os.path.basename(line.FileName.decode("utf-8", "replace")),
                                 line.LineNumber)
        return out


# ---- minidump --------------------------------------------------------------------------------
def report(path, syms, threads=False):
    with open(path, "rb") as source:
        D = source.read()
    sig, _ver, nstreams, dir_rva = struct.unpack_from("<4sIII", D, 0)
    if sig != b"MDMP":
        print("not a minidump: %s" % path)
        return
    streams = {}
    for i in range(nstreams):
        t, size, rva = struct.unpack_from("<III", D, dir_rva + 12 * i)
        streams[t] = (size, rva)

    def mdstring(rva):
        n, = struct.unpack_from("<I", D, rva)
        return D[rva + 4: rva + 4 + n].decode("utf-16-le", "replace")

    mods = []
    if 4 in streams:
        _, rva = streams[4]
        n, = struct.unpack_from("<I", D, rva)
        for i in range(n):
            base, size, _c, _t, nrva = struct.unpack_from("<QIIII", D, rva + 4 + 108 * i)
            mods.append((base, size, mdstring(nrva).split("\\")[-1]))
    mods.sort()
    for base, size, name in mods:
        if os.path.splitext(name)[0] in OURS:
            syms.load(name, base, size)

    def whose(a):
        for base, size, name in mods:
            if base <= a < base + size:
                return name, a - base
        return None, 0

    def fmt(a):
        name, rva = whose(a)
        if not name:
            return "%016X  <unmapped>" % a
        s = "%016X  %-26s +0x%-8X" % (a, name, rva)
        if os.path.splitext(name)[0] in OURS:
            s += "  " + (syms.resolve(a) or "<no pdb for this build>")
        return s

    print("=" * 100)
    print(os.path.basename(os.path.dirname(path)))
    if threads and 3 in streams:
        _, thread_rva = streams[3]
        count, = struct.unpack_from('<I', D, thread_rva)
        if count > 4096 or thread_rva + 4 + count * 48 > len(D):
            raise ValueError('Invalid thread-list extent')
        print('  THREAD STACK SCANS (candidates, not an unwind):')
        for index in range(count):
            row = struct.unpack_from('<IIIIQQIIII', D, thread_rva + 4 + index * 48)
            tid, _, _, _, _, start, size, stack_rva, context_size, context_rva = row
            if context_size < 256 or context_rva + context_size > len(D) or stack_rva + size > len(D):
                continue
            rsp, = struct.unpack_from('<Q', D, context_rva + 0x98)
            rip, = struct.unpack_from('<Q', D, context_rva + 0xF8)
            print('  THREAD %u RIP %s' % (tid, fmt(rip)))
            offset = max(0, rsp-start)
            offset = (offset+7) & ~7
            shown = 0
            for position in range(offset, min(size-7, offset+131072), 8):
                value, = struct.unpack_from('<Q', D, stack_rva+position)
                name, _ = whose(value)
                if name and (shown < 12 or os.path.splitext(name)[0] in OURS):
                    print('    +%05X  %s' % (start+position-rsp, fmt(value)))
                    shown += 1
                    if shown >= 48:
                        break
    if 6 not in streams:
        print("  no exception stream")
        return
    _, rva = streams[6]
    tid, = struct.unpack_from("<I", D, rva)
    code, _flags, _rec, addr, _np = struct.unpack_from("<IIQQI", D, rva + 8)
    params = struct.unpack_from("<15Q", D, rva + 40)
    ctx_size, ctx_rva = struct.unpack_from("<II", D, rva + 160)

    print("  EXCEPTION 0x%08X on thread %u" % (code, tid))
    if code == 0xC0000005:
        kind = {0: "read", 1: "write", 8: "execute"}.get(params[0], str(params[0]))
        bad = params[1]
        note = "  (non-canonical -- a #GP, so Windows reports -1 and there is no fault address)" \
            if bad == 0xFFFFFFFFFFFFFFFF else ""
        print("    access violation: %s at %016X%s" % (kind, bad, note))
    print("    FAULT  " + fmt(addr))

    C = D[ctx_rva: ctx_rva + ctx_size]
    regs = collections.OrderedDict(
        (n, struct.unpack_from("<Q", C, o)[0]) for n, o in
        (("Rax", 0x78), ("Rcx", 0x80), ("Rdx", 0x88), ("Rbx", 0x90), ("Rsp", 0x98), ("Rbp", 0xA0),
         ("Rsi", 0xA8), ("Rdi", 0xB0), ("R8", 0xB8), ("R9", 0xC0), ("R10", 0xC8), ("R11", 0xD0),
         ("R12", 0xD8), ("R13", 0xE0), ("R14", 0xE8), ("R15", 0xF0), ("Rip", 0xF8)))
    items = list(regs.items())
    print("    registers:")
    for i in range(0, len(items), 4):
        print("      " + "  ".join("%-4s %016X" % kv for kv in items[i:i + 4]))

    ranges = []
    if 9 in streams:
        _, rva = streams[9]
        n, base_rva = struct.unpack_from("<QQ", D, rva)
        off = base_rva
        for i in range(n):
            s, sz = struct.unpack_from("<QQ", D, rva + 16 + 16 * i)
            ranges.append((s, sz, off))
            off += sz
    elif 5 in streams:
        _, rva = streams[5]
        n, = struct.unpack_from("<I", D, rva)
        for i in range(n):
            s, sz, o = struct.unpack_from("<QII", D, rva + 4 + 16 * i)
            ranges.append((s, sz, o))

    def read8(a):
        for s, sz, off in ranges:
            if s <= a and a + 8 <= s + sz:
                return struct.unpack_from("<Q", D, off + (a - s))[0]
        return None

    print("    stack scan from RSP (candidates, most recent first -- a scan, not an unwind):")
    seen, shown = set(), 0
    for i in range(4096):
        v = read8(regs["Rsp"] + 8 * i)
        if v is None:
            break
        if whose(v)[0] and v not in seen:
            seen.add(v)
            print("      +%05X  %s" % (8 * i, fmt(v)))
            shown += 1
            if shown >= 30:
                break
    if not shown:
        print("      (no stack memory captured)")


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--image-dir',help='Exact archived DLL/PDB directory for this crash build')
    parser.add_argument('--threads',action='store_true',help='Include bounded stack scans for all captured threads, including live dumps')
    parser.add_argument('dumps',nargs='*')
    options=parser.parse_args();args=options.dumps
    if not args:
        q = os.path.join(os.environ.get("LOCALAPPDATA", ""), "REDEngine", "ReportQueue")
        args = sorted(glob.glob(os.path.join(q, "*", "Cyberpunk2077.dmp")))[-1:]
        if not args:
            print(__doc__)
            return
        print("no path given -- reading the newest report in %s\n" % q)
    syms = Symbols(options.image_dir)
    if not syms.ok:
        print("dbghelp unavailable: addresses will be module+RVA only\n")
    for a in args:
        report(a, syms, options.threads)


if __name__ == "__main__":
    main()
