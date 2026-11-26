"""Inspect symbols for an archived DLL without loading or executing that DLL."""
import argparse
import ctypes as c
import hashlib
import json
import os
from pathlib import Path

import symbol_session as ss
from symbol_identity import pe_identity, pdb_identity
from thread_stack_sampler import SYMBOL_INFOW


def audit(image, rvas, default_search=False, system_dbghelp=False):
    image = Path(image).resolve()
    identity = pe_identity(image)
    d = c.WinDLL(str(Path(os.environ['WINDIR']) / 'System32/dbghelp.dll'), use_last_error=True) if system_dbghelp else ss.load_dbghelp()
    ss.configure(d)
    h = c.c_void_p(0x61554417)
    if default_search:
        d.SymSetOptions(0x2 | 0x4 | 0x10)
        if not d.SymInitializeW(h, None, False):
            raise c.WinError(c.get_last_error())
        session = {'searchPath': None, 'options': '0x16'}
    else:
        session = ss.initialize(d, h, [image], invade=False)
    d.SymFromAddrW.argtypes = [c.c_void_p, c.c_uint64, c.POINTER(c.c_uint64), c.POINTER(SYMBOL_INFOW)]
    d.SymFromAddrW.restype = c.c_int
    base = 0x180000000
    try:
        if not d.SymLoadModuleExW(h, None, str(image), None, base, identity['imageSize'], None, 0):
            raise c.WinError(c.get_last_error())
        samples = []
        for rva in rvas:
            info = SYMBOL_INFOW()
            info.SizeOfStruct, info.MaxNameLen = 88, 511
            offset = c.c_uint64()
            if d.SymFromAddrW(h, base + rva, c.byref(offset), c.byref(info)):
                module = ss.module_info(d, h, base)
                label, bounded = ss.symbol_label(info.Name, info.Address, info.Size, base + rva, base,
                                                module.get('symbolType', 'unknown'), info.Flags)
                samples.append({'rva': hex(rva), 'rawName': info.Name, 'symbolRva': hex(info.Address - base),
                                'displacement': offset.value, 'size': info.Size, 'label': label, 'bounded': bounded})
            else:
                samples.append({'rva': hex(rva), 'error': c.get_last_error()})
        result = {'image': str(image), 'sha256': hashlib.sha256(image.read_bytes()).hexdigest(),
                  'codeView': identity, 'session': session, 'module': ss.module_info(d, h, base), 'samples': samples}
        candidate = image.with_suffix('.pdb')
        if candidate.exists():
            result['adjacentPdb'] = pdb_identity(candidate)
        return result
    finally:
        d.SymCleanup(h)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--image', type=Path, required=True)
    parser.add_argument('--rva', type=lambda value: int(value, 0), action='append')
    parser.add_argument('--default-search', action='store_true', help='Reproduce the old sampler symbol initialization')
    parser.add_argument('--system-dbghelp', action='store_true')
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    report = audit(args.image, args.rva or [0x1000, 0x10000, 0x100000, 0x200000], args.default_search, args.system_dbghelp)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report, indent=2))
