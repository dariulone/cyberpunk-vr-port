"""Read-only LuaJIT 2.1 (GC64, FR2) stack decoding from another process (CET's Lua state).

Layouts (GC64): TValue 8 bytes, GC pointers in the low 47 bits, function itype 0x1FFF7 in bits 47+.
GCfunc: ffid @10, pc @32 (bytecode, = proto + 104). GCproto: chunkname @64, firstline @72.
GCstr: len @20, chars @24. lua_State: glref @16, base @32, top @40, stack @56, stacksize @88; the main
state sits right before its global_State (glref == L + 96) at the start of GG_State, so it is found
by scanning backwards from DISPATCH (rbx while the interpreter runs).
Frames (FR2): link slot = base-1, function = base-2; link & 3 == 0 -> Lua frame, previous base is
link_slot - (2 + A(pc[-1])); otherwise previous = link_slot - (link & ~7) bytes.
"""
import ctypes as c, ctypes.wintypes as w, struct

MASK = (1 << 47) - 1
k = c.WinDLL('kernel32', use_last_error=True)
k.ReadProcessMemory.argtypes = [w.HANDLE, c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]


class Reader:
    def __init__(self, process):
        self.proc = process
        self.L = None
        self.names = {}

    def rd(self, a, n):
        b = c.create_string_buffer(n); got = c.c_size_t()
        if not a or not k.ReadProcessMemory(self.proc, a, b, n, c.byref(got)) or got.value != n:
            return None
        return b.raw

    def q(self, a):
        r = self.rd(a, 8)
        return struct.unpack('<Q', r)[0] if r else None

    def gcstr(self, p):
        hdr = self.rd(p, 24)
        if not hdr:
            return None
        ln = struct.unpack_from('<I', hdr, 20)[0]
        if not 0 < ln < 1024:
            return None
        s = self.rd(p + 24, ln)
        return s.decode('latin1') if s else None

    def func(self, tv):
        """'chunk:firstline' for a Lua function TValue, 'C' for C functions, None if invalid."""
        if tv is None or (tv >> 47) != 0x1FFF7:
            return None
        fn = tv & MASK
        if fn in self.names:
            return self.names[fn]
        hdr = self.rd(fn, 40)
        if not hdr:
            return None
        if hdr[10] != 0:
            name = 'C'
        else:
            pc = struct.unpack_from('<Q', hdr, 32)[0]
            ph = self.rd(pc - 104, 80)
            if not ph:
                return None
            chunk = self.gcstr(struct.unpack_from('<Q', ph, 64)[0])
            if not chunk:
                return None
            name = '%s:%d' % (chunk, struct.unpack_from('<I', ph, 72)[0])
        if len(self.names) < 200000:
            self.names[fn] = name
        return name

    def is_state(self, x, base=None):
        hdr = self.rd(x, 96)
        if not hdr or hdr[9] != 6:  # gct of a thread = ~LJ_TTHREAD = 6
            return False
        stack = struct.unpack_from('<Q', hdr, 56)[0]
        size = struct.unpack_from('<I', hdr, 88)[0]
        if not 0 < size < 1 << 22:
            return False
        return base is None or stack <= base < stack + 8 * size

    def find_state(self, dispatch, base=None):
        """Main lua_State from DISPATCH: scan back for a thread object whose glref points just past it."""
        page = dispatch & ~0xFFF
        for i in range(64):
            blob = self.rd(page - 0x1000 * i, 0x1000)
            if not blob:
                return None
            start = page - 0x1000 * i
            for off in range(0x1000 - 8, -8, -8):
                x = start + off - 16
                v = struct.unpack_from('<Q', blob, off)[0]
                if x < dispatch and 0 < v - x <= 0x100 and self.is_state(x, base):
                    self.L = x
                    return x
        return None

    def stack_range(self):
        hdr = self.rd(self.L, 96)
        if not hdr:
            return None
        stack = struct.unpack_from('<Q', hdr, 56)[0]
        size = struct.unpack_from('<I', hdr, 88)[0]
        return stack, stack + 8 * size, struct.unpack_from('<Q', hdr, 32)[0]

    def chain(self, base, limit=24):
        """Function names from the frame at `base` outwards (innermost first)."""
        rng = self.stack_range() if self.L else None
        out = []
        for _ in range(limit):
            if rng and not (rng[0] < base <= rng[1]):
                break
            name = self.func(self.q(base - 16))
            if name is None:
                break
            out.append(name)
            link = self.q(base - 8)
            if link is None:
                break
            slot = base - 8
            if link & 3 == 0:  # Lua frame: link is the caller's PC
                ins = self.rd(link - 4, 4)
                if not ins:
                    break
                a = ins[1]
                base = slot - 8 * (2 + a) + 8  # previous link slot + 1 slot = previous base
            else:
                base = slot - (link & ~7) + 8
            if base <= (rng[0] + 16 if rng else 0):
                break
        return out
