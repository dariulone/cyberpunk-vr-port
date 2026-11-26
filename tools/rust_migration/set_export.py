"""Write the port's exported tuning variables in the running game (CyberpunkVR_* exports, by name), so a probe can run
its tests on a non-default path. Each argument is NAME=VALUE, the value an int unless it has a '.' (then a float);
VALUE:Q writes it as a 64-bit unsigned integer (hex with 0x, else decimal).
Prints one JSON line with what was written. The game must be running."""
import ctypes
import json
import struct
import sys

sys.path.insert(0, r'X:\Dev\CyberpunkVRPort\tools\sim_harness')
from simxr import _exports, _modules, game_pid  # noqa: E402

k = ctypes.WinDLL('kernel32', use_last_error=True)
k.OpenProcess.restype = ctypes.c_void_p
k.WriteProcessMemory.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t,
                                 ctypes.POINTER(ctypes.c_size_t)]

pid = game_pid()
port = next(m for m in _modules(pid) if m[0].lower() == 'cyberpunkvrport.dll')
ex = _exports(port[2])
handle = k.OpenProcess(0x0028, False, pid)  # PROCESS_VM_OPERATION | PROCESS_VM_WRITE
written = {}
for arg in sys.argv[1:]:
    name, value = arg.split('=', 1)
    if value.endswith(':Q'):
        data = struct.pack('<Q', int(value[:-2], 0))
    else:
        data = struct.pack('<f', float(value)) if '.' in value else struct.pack('<i', int(value))
    done = ctypes.c_size_t()
    ok = k.WriteProcessMemory(handle, ctypes.c_void_p(port[1] + ex[name]), data, len(data), ctypes.byref(done))
    written[name] = value if ok else f'failed {ctypes.get_last_error()}'
print(json.dumps(written))
