"""Capture thread stacks from an existing live process, without terminating it."""

import ctypes as c
from ctypes import wintypes as w
import os
from pathlib import Path
import msvcrt


def capture(pid, path):
    if not isinstance(pid, int) or pid <= 0 or pid == os.getpid():
        raise ValueError('A separate live process is required')
    kernel = c.WinDLL('kernel32', use_last_error=True)
    dbghelp = c.WinDLL('dbghelp', use_last_error=True)
    kernel.OpenProcess.argtypes = [w.DWORD, w.BOOL, w.DWORD]
    kernel.OpenProcess.restype = w.HANDLE
    kernel.CloseHandle.argtypes = [w.HANDLE]
    kernel.GetExitCodeProcess.argtypes = [w.HANDLE, c.POINTER(w.DWORD)]
    dbghelp.MiniDumpWriteDump.argtypes = [w.HANDLE, w.DWORD, w.HANDLE, w.DWORD, c.c_void_p, c.c_void_p, c.c_void_p]
    dbghelp.MiniDumpWriteDump.restype = w.BOOL
    process = kernel.OpenProcess(0x400 | 0x10, False, pid)
    if not process:
        raise c.WinError(c.get_last_error())
    path = Path(path)
    try:
        code = w.DWORD()
        if not kernel.GetExitCodeProcess(process, c.byref(code)):
            raise c.WinError(c.get_last_error())
        if code.value != 259:
            raise RuntimeError('Process has already exited')
        with path.open('xb') as output:
            # Thread/context information only; no full private-memory dump.
            if not dbghelp.MiniDumpWriteDump(process, pid, msvcrt.get_osfhandle(output.fileno()),
                                             0x1000 | 0x100, None, None, None):
                raise c.WinError(c.get_last_error())
        with path.open('rb') as saved:
            if saved.read(4) != b'MDMP':
                raise RuntimeError('MiniDumpWriteDump produced no valid header')
        return dict(file=str(path), bytes=path.stat().st_size, pid=pid, exception=False)
    finally:
        kernel.CloseHandle(process)
