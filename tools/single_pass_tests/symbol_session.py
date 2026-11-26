"""Explicit DbgHelp loading and auditable module/symbol provenance for samplers."""
import ctypes as c
from ctypes import wintypes as w
import os
from pathlib import Path
import uuid

from symbol_identity import pe_identity, pdb_identity

SYMBOL_TYPES = ('none', 'coff', 'codeview', 'pdb', 'export', 'deferred', 'sym', 'dia', 'virtual')


class GUID(c.Structure):
    _fields_ = [('data1', w.DWORD), ('data2', w.WORD), ('data3', w.WORD), ('data4', c.c_ubyte * 8)]


class MODULE_INFO(c.Structure):
    _fields_ = [('SizeOfStruct', w.DWORD), ('BaseOfImage', c.c_uint64), ('ImageSize', w.DWORD),
                ('TimeDateStamp', w.DWORD), ('CheckSum', w.DWORD), ('NumSyms', w.DWORD), ('SymType', w.DWORD),
                ('ModuleName', w.WCHAR * 32), ('ImageName', w.WCHAR * 256), ('LoadedImageName', w.WCHAR * 256),
                ('LoadedPdbName', w.WCHAR * 256), ('CVSig', w.DWORD), ('CVData', w.WCHAR * 780),
                ('PdbSig', w.DWORD), ('PdbSig70', GUID), ('PdbAge', w.DWORD),
                ('PdbUnmatched', w.BOOL), ('DbgUnmatched', w.BOOL), ('LineNumbers', w.BOOL),
                ('GlobalSymbols', w.BOOL), ('TypeInfo', w.BOOL), ('SourceIndexed', w.BOOL), ('Publics', w.BOOL),
                ('MachineType', w.DWORD), ('Reserved', w.DWORD)]


def load_dbghelp():
    # Prefer the already installed debugger distribution with the current module
    # info ABI. Both OS and SDK copies were verified against the archived PDB.
    sdk = Path(os.environ.get('ProgramFiles(x86)', r'C:\Program Files (x86)')) / 'Windows Kits/10/Debuggers/x64/dbghelp.dll'
    return c.WinDLL(str(sdk) if sdk.exists() else 'dbghelp', use_last_error=True)


def configure(d):
    d.SymSetOptions.argtypes = [w.DWORD]
    d.SymSetOptions.restype = w.DWORD
    d.SymInitializeW.argtypes = [w.HANDLE, w.LPCWSTR, w.BOOL]
    d.SymInitializeW.restype = w.BOOL
    d.SymCleanup.argtypes = [w.HANDLE]
    d.SymCleanup.restype = w.BOOL
    d.SymGetModuleInfoW64.argtypes = [w.HANDLE, c.c_uint64, c.POINTER(MODULE_INFO)]
    d.SymGetModuleInfoW64.restype = w.BOOL
    d.SymLoadModuleExW.argtypes = [w.HANDLE, w.HANDLE, w.LPCWSTR, w.LPCWSTR, c.c_uint64, w.DWORD, c.c_void_p, w.DWORD]
    d.SymLoadModuleExW.restype = c.c_uint64


def initialize(d, process, module_paths, extra_path=None, invade=True):
    configure(d)
    paths = [p for p in (extra_path or '').split(';') if p]
    paths += list(dict.fromkeys(str(Path(p).parent) for p in module_paths if p))
    inherited = os.environ.get('_NT_SYMBOL_PATH')
    if inherited:
        paths.append(inherited)
    search_path = ';'.join(dict.fromkeys(paths))
    # UNDNAME | DEFERRED_LOADS | LOAD_LINES | FAIL_CRITICAL_ERRORS | NO_PROMPTS.
    # EXACT_SYMBOLS also suppresses export fallback for system modules without a
    # local PDB. Keep those *qualified hints*, and enforce GUID+age ourselves for
    # any loaded PDB against both disk and mapped image. Never enable LOAD_ANYTHING.
    options = 0x2 | 0x4 | 0x10 | 0x200 | 0x80000
    d.SymSetOptions(options)
    if not d.SymInitializeW(process, search_path or None, invade):
        raise c.WinError(c.get_last_error())
    kernel = c.WinDLL('kernel32', use_last_error=True)
    kernel.GetModuleFileNameW.argtypes = [w.HMODULE, w.LPWSTR, w.DWORD]
    loaded = c.create_unicode_buffer(1024)
    kernel.GetModuleFileNameW(d._handle, loaded, len(loaded))
    return {'searchPath': search_path, 'options': hex(options), 'dbghelp': loaded.value}


def module_info(d, process, address):
    info = MODULE_INFO()
    info.SizeOfStruct = c.sizeof(info)
    if not d.SymGetModuleInfoW64(process, address, c.byref(info)):
        return {'error': c.get_last_error()}
    result = {'base': info.BaseOfImage, 'imageSize': info.ImageSize,
              'symbolType': SYMBOL_TYPES[info.SymType] if info.SymType < len(SYMBOL_TYPES) else str(info.SymType),
              'symbolTypeValue': info.SymType, 'loadedImage': info.LoadedImageName,
              'loadedPdb': info.LoadedPdbName, 'pdbGuid': str(uuid.UUID(bytes_le=bytes(info.PdbSig70))),
              'pdbAge': info.PdbAge, 'pdbUnmatched': bool(info.PdbUnmatched), 'globals': bool(info.GlobalSymbols)}
    if info.LoadedPdbName and info.LoadedImageName:
        try:
            expected, actual = pe_identity(info.LoadedImageName), pdb_identity(info.LoadedPdbName)
            result['identityVerified'] = bool(expected.get('guid') and expected['guid'] == actual['guid'] and expected['age'] == actual['age'])
            result['imageCodeView'] = expected
        except (OSError, ValueError) as error:
            result['identityError'] = str(error)
            result['identityVerified'] = False
    return result


def symbol_label(name, address, size, pc, module_base, symbol_type, flags=0):
    displacement = pc - address
    # Size is reliable only for PDB symbols; never collapse an arbitrary PC to
    # the closest exported name. A zero-size PDB public also has no proven span.
    within = symbol_type in ('pdb', 'dia') and not flags & 0x200 and size > 0 and 0 <= displacement < size
    if within or displacement == 0:
        return name, within or displacement == 0
    return f'{name}+0x{displacement:X} [nearest {symbol_type}; rva=0x{pc - module_base:X}]', False
