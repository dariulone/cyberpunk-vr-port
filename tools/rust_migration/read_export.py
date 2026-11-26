"""Read the port's exported variables in the running game (CyberpunkVR_* exports, by name), so a probe can report a
counter a path keeps. Each argument is NAME or NAME:FMT with a struct format (default '<Q', an unsigned 64-bit
counter). Prints one JSON line. The game must be running."""
import ctypes
import json
import struct
import sys

sys.path.insert(0, r'X:\Dev\CyberpunkVRPort\tools\sim_harness')
from simxr import _exports, _modules, game_pid  # noqa: E402

k = ctypes.WinDLL('kernel32', use_last_error=True)
k.OpenProcess.restype = ctypes.c_void_p
k.ReadProcessMemory.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t,
                                ctypes.POINTER(ctypes.c_size_t)]
pid = game_pid()
port = next(m for m in _modules(pid) if m[0].lower() == 'cyberpunkvrport.dll')
ex = _exports(port[2])
handle = k.OpenProcess(0x0010, False, pid)  # PROCESS_VM_READ
values = {}
for arg in sys.argv[1:]:
    name, _, fmt = arg.partition(':')
    fmt = fmt or '<Q'
    size = struct.calcsize(fmt)
    buf = ctypes.create_string_buffer(size)
    got = ctypes.c_size_t()
    if name not in ex:
        values[name] = 'not exported'
    elif k.ReadProcessMemory(handle, ctypes.c_void_p(port[1] + ex[name]), buf, size, ctypes.byref(got)):
        v = struct.unpack(fmt, buf.raw)
        if len(v) == 1 and isinstance(v[0], bytes):  # a char array ('2048s'): the C string in it
            values[name] = v[0].split(b'\0', 1)[0].decode('utf-8', 'replace')
        else:
            values[name] = v[0] if len(v) == 1 else list(v)
    else:
        values[name] = f'failed {ctypes.get_last_error()}'
print(json.dumps(values))
