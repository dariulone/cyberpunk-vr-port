"""Call one of the port's functions without arguments in the running game, on a thread of its own, and print what it
returned (the thread's exit code: the low 32 bits of RAX). The function is found by name in cyberpunkvrport.pdb
through dbghelp (offline, at a base of its own, then moved to the game's) -- a Rust `#[no_mangle]` function the C++ DLL links but does not export works too.

    python call_in_game.py cvr_xr_save_calibration
"""
import ctypes
import ctypes.wintypes as wt
import json
import sys

sys.path.insert(0, r'X:\Dev\CyberpunkVRPort\tools\sim_harness')
from simxr import _modules, game_pid  # noqa: E402

k = ctypes.WinDLL('kernel32', use_last_error=True)
dbg = ctypes.WinDLL('dbghelp', use_last_error=True)
k.OpenProcess.restype = wt.HANDLE
k.CreateRemoteThread.restype = wt.HANDLE
k.CreateRemoteThread.argtypes = [wt.HANDLE, ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p, ctypes.c_void_p, wt.DWORD,
                                 ctypes.c_void_p]
dbg.SymInitializeW.argtypes = [wt.HANDLE, wt.LPCWSTR, wt.BOOL]
dbg.SymLoadModuleExW.restype = ctypes.c_uint64
dbg.SymLoadModuleExW.argtypes = [wt.HANDLE, wt.HANDLE, wt.LPCWSTR, wt.LPCWSTR, ctypes.c_uint64, wt.DWORD, ctypes.c_void_p, wt.DWORD]
dbg.SymFromName.argtypes = [wt.HANDLE, ctypes.c_char_p, ctypes.c_void_p]


class SYMBOL_INFO(ctypes.Structure):
    _fields_ = [('SizeOfStruct', wt.ULONG), ('TypeIndex', wt.ULONG), ('Reserved', ctypes.c_uint64 * 2),
                ('Index', wt.ULONG), ('Size', wt.ULONG), ('ModBase', ctypes.c_uint64), ('Flags', wt.ULONG),
                ('Value', ctypes.c_uint64), ('Address', ctypes.c_uint64), ('Register', wt.ULONG), ('Scope', wt.ULONG),
                ('Tag', wt.ULONG), ('NameLen', wt.ULONG), ('MaxNameLen', wt.ULONG), ('Name', ctypes.c_char * 256)]


def rva(dll, name):
    """The function's offset in the DLL, from its PDB (loaded at a base of our own, not the game's)."""
    session = wt.HANDLE(0xC0DE)
    base = 0x10000000
    dbg.SymSetOptions(0x2 | 0x4)  # SYMOPT_UNDNAME | SYMOPT_DEFERRED_LOADS
    if not dbg.SymInitializeW(session, None, False):
        raise OSError(ctypes.get_last_error())
    try:
        if not dbg.SymLoadModuleExW(session, None, dll, None, base, 0, None, 0):
            raise OSError(ctypes.get_last_error())
        info = SYMBOL_INFO()
        info.SizeOfStruct = 88  # sizeof(SYMBOL_INFO): Name[1] and the padding after it
        info.MaxNameLen = 255
        if not dbg.SymFromName(session, name.encode(), ctypes.byref(info)) or info.Address < base:
            return None
        return info.Address - base
    finally:
        dbg.SymCleanup(session)


def call(name, timeout_ms=10000):
    pid = game_pid()
    port = next(m for m in _modules(pid) if m[0].lower() == 'cyberpunkvrport.dll')
    offset = rva(port[2], name)
    if offset is None:
        return {'name': name, 'found': False}
    process = k.OpenProcess(0x1F0FFF, False, pid)
    if not process:
        raise OSError(ctypes.get_last_error())
    try:
        thread = k.CreateRemoteThread(process, None, 0, ctypes.c_void_p(port[1] + offset), None, 0, None)
        if not thread:
            raise OSError(ctypes.get_last_error())
        k.WaitForSingleObject(thread, timeout_ms)
        code = wt.DWORD()
        k.GetExitCodeThread(thread, ctypes.byref(code))
        k.CloseHandle(thread)
        return {'name': name, 'found': True, 'returned': code.value}
    finally:
        k.CloseHandle(process)


if __name__ == '__main__':
    print(json.dumps(call(sys.argv[1])))
