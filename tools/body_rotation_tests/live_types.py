"""Read test telemetry layouts from the exact DLL's PDB, never the game process."""
import argparse
import ctypes as c
from ctypes import wintypes as w
import hashlib
import json
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'swimming_tests'))
from live_symbols import Symbol


def describe(dll, extra_types=()):
    d = c.WinDLL('dbghelp', use_last_error=True)
    k = c.WinDLL('kernel32')
    handle = c.c_void_p(0x424f4459)
    d.SymInitializeW.argtypes = [c.c_void_p, w.LPCWSTR, w.BOOL]
    d.SymLoadModuleExW.argtypes = [c.c_void_p, w.HANDLE, w.LPCWSTR, w.LPCWSTR,
                                  c.c_ulonglong, w.DWORD, c.c_void_p, w.DWORD]
    d.SymLoadModuleExW.restype = c.c_ulonglong
    d.SymGetTypeFromName.argtypes = [c.c_void_p, c.c_ulonglong, c.c_char_p, c.POINTER(Symbol)]
    d.SymFromName.argtypes = [c.c_void_p,c.c_char_p,c.POINTER(Symbol)]
    d.SymGetTypeInfo.argtypes = [c.c_void_p, c.c_ulonglong, w.ULONG, c.c_int, c.c_void_p]
    d.SymCleanup.argtypes = [c.c_void_p]
    k.LocalFree.argtypes = [c.c_void_p]
    d.SymSetOptions(2 | 4 | 0x200 | 0x80000)  # exact symbols; no UI
    if not d.SymInitializeW(handle, str(dll.parent), False):
        raise c.WinError(c.get_last_error())
    result = {'sha256': hashlib.sha256(dll.read_bytes()).hexdigest()}
    try:
        base = d.SymLoadModuleExW(handle, None, str(dll), None, 0x180000000, 0, None, 0)
        if not base:
            raise c.WinError(c.get_last_error())

        def info(type_id, kind, value_type=w.ULONG):
            value = value_type()
            if not d.SymGetTypeInfo(handle, base, type_id, kind, c.byref(value)):
                return None
            return value.value

        def name(type_id):
            pointer = info(type_id, 1, c.c_void_p)  # TI_GET_SYMNAME
            if not pointer:
                return None
            try:
                return c.wstring_at(pointer)
            finally:
                k.LocalFree(pointer)

        required_types=('OpenXRManager', 'cvr::body::TrackingFrame', 'cvr::body::TrackedBodyYaw', 'LiveControls')+tuple(extra_types)
        for type_name in required_types+('cvr::body::HybridBodyYaw','OpenXRHeadPose','cvr::body::BendSample'):
            symbol = Symbol()
            symbol.size, symbol.maxLength = 88, 1023
            is_symbol=type_name.startswith('symbol:')
            # A private nested type can be reached through a data symbol, e.g.
            # symbol:cvr::physhands::steps/data/motor/touch. Arrays describe one element.
            symbol_path = type_name[7:].split('/') if is_symbol else []
            found=(d.SymFromName(handle,symbol_path[0].encode(),c.byref(symbol)) if is_symbol else
                   d.SymGetTypeFromName(handle, base, type_name.encode(), c.byref(symbol)))
            if not found:
                if type_name not in required_types:
                    continue
                raise RuntimeError('Missing PDB type: ' + type_name)
            def members(type_id):
                count = info(type_id, 13)
                if count is None:
                    raise RuntimeError('Missing type members: ' + type_name)
                children = (w.ULONG * (count + 2))()
                children[0] = count
                if not d.SymGetTypeInfo(handle, base, type_id, 7, c.byref(children)):
                    raise c.WinError(c.get_last_error())
                return {name(child): child for child in children[2:] if info(child, 10) is not None}

            for part in symbol_path[1:]:
                while info(symbol.type,0) in (14,15):
                    symbol.type=info(symbol.type,3)
                symbol.type = info(members(symbol.type)[part], 3)
            if is_symbol:
                while info(symbol.type,0) in (14,15):  # pointer/array -> element type
                    symbol.type=info(symbol.type,3)
            fields = {key: {'offset': info(child, 10)} for key, child in members(symbol.type).items()}
            result[type_name] = {'size': info(symbol.type, 2, c.c_ulonglong), 'fields': fields}
    finally:
        d.SymCleanup(handle)
    return result


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dll', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--extra-type', action='append', default=[])
    args = parser.parse_args()
    args.out.write_text(json.dumps(describe(args.dll.resolve(),args.extra_type), indent=2), encoding='utf-8')
    print(args.out)
