"""Does the port read the game's data as the types it is? Three checks, one report:

1. HOOKS (offline): every hook the Rust port installs on a game function, and every function it defines itself, is
   matched to the signature the engine has for it (log/audit_signatures.txt, written by the port once its hooks are
   in -- in a launch with the launcher's DEBUG or CVR_LOG_VERBOSE=1; a hook "not in the signatures" asks for one) and to what the hook's body reads (call.arg_handle(N), arg_u64, arg_bool, arg_number, listener_arg, the result
   accessors...). A handle read of a struct, an 8-byte read of a 16-byte value, a Bool result written into a void
   function: each is a silent failure in the game (null, 0, nothing written) or worse.
2. CALLBACKS the port defines (a cb func of its own class the engine calls, OnAction...): their declared parameter
   types against the game's own callbacks of that name (decompiled scripts): a handle where the game passes a struct
   was the ArmCyberwareB crash.
3. RUNTIME: every "[audit]" line of a game log (the port says each wrong read once: hooks, dynamic calls given
   arguments their parameters do not take, missing methods, statics and fields), its callers named with the PDB.

    py -3.13 -B tools/audit/check_reads.py [--log PATH] [--signatures PATH] [--scripts DIR] [--pdb-dll DLL]
"""
import argparse
import ctypes as c
import re
import sys
from ctypes import wintypes as w
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SRC = ROOT / 'rust/crates/cvr-ffi/src'
GAME = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
LOG_DIR = GAME / 'cyberpunkvrport/log'
SCRIPTS = Path(r'C:\Users\dariulone\Desktop\cyberpunk')
DLL = ROOT / 'build/patchday/sdk25-build/bin/red4ext/plugins/cyberpunkvrport/Release/cyberpunkvrport.dll'

KIND_HANDLE, KIND_WEAK, KIND_ENUM, KIND_ARRAY = 9, 10, 5, 3
# what each accessor needs of the parameter (kind, name, size) -> None when fine, else what it reads it as
ACCESSORS = {
    'arg_handle': ('a handle', lambda k, n, s: k in (KIND_HANDLE, KIND_WEAK)),
    'arg_u64': ('an 8-byte value', lambda k, n, s: s == 8),
    'set_arg_u64': ('an 8-byte value (written)', lambda k, n, s: s == 8),
    'arg_bool': ('a Bool', lambda k, n, s: n == 'Bool'),
    'set_arg_bool': ('a Bool (written)', lambda k, n, s: n == 'Bool'),
    'arg_number': ('a number', lambda k, n, s: n in ('Int32', 'Uint32', 'Float', 'Int64')),
    'set_arg_float': ('a Float (written)', lambda k, n, s: n == 'Float'),
    'arg_strings': ('an array of Strings', lambda k, n, s: k == KIND_ARRAY and 'String' in n),
    'arg_variant_handle': ('a Variant', lambda k, n, s: n == 'Variant'),
    'listener_arg': ('a listener struct (in place)', lambda k, n, s: k not in (KIND_HANDLE, KIND_WEAK)),
}
RESULTS = {
    'set_result_bool': ('a Bool (written)', lambda k, n: n == 'Bool'),
    'result_bool': ('a Bool', lambda k, n: n == 'Bool'),
    'set_result_float': ('a Float (written)', lambda k, n: n == 'Float'),
    'set_result_enum': ('an enum (written)', lambda k, n: k == KIND_ENUM),
}


def parse_type(text):
    """'Name (kind 9, 16 bytes)' -> (name, kind, size)."""
    m = re.match(r'(.*) \(kind (\d+), (\d+) bytes\)$', text.strip())
    if not m:
        return text.strip(), -1, -1
    return m.group(1), int(m.group(2)), int(m.group(3))


def read_signatures(path):
    sigs = {}
    for line in path.read_text(encoding='utf-8', errors='replace').splitlines():
        parts = line.split('\t')
        if len(parts) < 4:
            continue
        name, kind, ret, params = parts[0], parts[1], parts[2], parts[3]
        plist = []
        if params.strip():
            # "name:Type (kind k, n bytes), name:Type (...)" -- split at "), " keeping the parenthesis
            for p in re.findall(r'([^,:]+):([^()]*\(kind -?\d+, -?\d+ bytes\)|\?)', params):
                plist.append((p[0].strip(), parse_type(p[1])))
        sigs[name] = dict(kind=kind, ret=None if ret == 'void' else parse_type(ret), params=plist)
    return sigs


def rust_sources():
    return {p: p.read_text(encoding='utf-8', errors='replace') for p in SRC.rglob('*.rs')}


REG = re.compile(r'\(\s*"([A-Za-z0-9_]+)"\s*,\s*"([A-Za-z0-9_]+)"\s*,\s*(?:crate::script_hooks::)?Kind::(Before|After|Override)\s*,\s*([A-Za-z0-9_:]+)\s*\)')
INSTALL = re.compile(r'install(?:_inner)?\(\s*"([A-Za-z0-9_]+)"\s*,\s*"([A-Za-z0-9_]+)"\s*,\s*Kind::(Before|After|Override)\s*,\s*([A-Za-z0-9_:]+)\s*\)')
STATIC = re.compile(r'install_static(?:_inner)?\(\s*"([A-Za-z0-9_]+)"\s*,\s*"([A-Za-z0-9_]+)"\s*,\s*Kind::(Before|After|Override)\s*,\s*([A-Za-z0-9_:]+)\s*\)')
CLASSDEF = re.compile(r'ClassDef\s*\{\s*name:\s*"([A-Za-z0-9_]+)"(.*?)\n\s*\}\s*[,\]]', re.S)
METHOD = re.compile(r'Method\s*\{\s*name:\s*"([A-Za-z0-9_]+)"\s*,\s*sig:\s*([A-Za-z0-9_]+)\s*,\s*flags:\s*([A-Za-z0-9_| ]+?)\s*,\s*hook:\s*([A-Za-z0-9_:]+)\s*\}')


def registrations(sources):
    """(chain name as the port names it, handler name, file)."""
    out = []
    for path, text in sources.items():
        for m in STATIC.finditer(text):
            out.append((f'{m.group(1)}::{m.group(2)}', m.group(4).split('::')[-1], path))
        for rx in (REG, INSTALL):
            for m in rx.finditer(text):
                if text[max(0, m.start() - 15):m.start()].endswith('install_static'):
                    continue
                out.append((f'{m.group(1)}.{m.group(2)}', m.group(4).split('::')[-1], path))
        for cm in CLASSDEF.finditer(text):
            for mm in METHOD.finditer(cm.group(2)):
                out.append((f'{cm.group(1)}.{mm.group(1)}', mm.group(4).split('::')[-1], path))
    return out


def function_body(sources, name, near):
    """The body of `fn name(` (the file it was registered in first, then any)."""
    order = [near] + [p for p in sources if p != near]
    rx = re.compile(r'\bfn ' + re.escape(name) + r'\s*(?:<[^>]*>)?\s*\(')
    for p in order:
        text = sources[p]
        m = rx.search(text)
        if not m:
            continue
        i = text.find('{', m.end())
        depth, j = 0, i
        while j < len(text):
            if text[j] == '{':
                depth += 1
            elif text[j] == '}':
                depth -= 1
                if depth == 0:
                    return p, text[i:j + 1], text.count('\n', 0, m.start()) + 1
            j += 1
    return None, '', 0


def reads_in(body):
    """(accessor, index or None) the body makes on its Call."""
    out = []
    for m in re.finditer(r'\bcall\.(arg_handle|arg_u64|set_arg_u64|arg_bool|set_arg_bool|arg_number|set_arg_float|arg_strings|arg_variant_handle)\((\d+)', body):
        out.append((m.group(1), int(m.group(2))))
    for m in re.finditer(r'listener_arg\(\s*call\s*,\s*(\d+)', body):
        out.append(('listener_arg', int(m.group(1))))
    for m in re.finditer(r'\bcall\.(set_result_bool|result_bool|set_result_float|set_result_enum)\(', body):
        out.append((m.group(1), None))
    return out


def check_hooks(sigs, sources):
    findings, missing, checked = [], [], 0
    for chain, handler, path in registrations(sources):
        # a scripted static is named Class::name; an inherited function shares its parent's chain (LadderSlideEvents'
        # OnUpdate is LadderEvents'): any chain of the same function name stands for it then
        sig = sigs.get(chain) or sigs.get(chain.replace('.', '::', 1))
        if sig is None:
            short = chain.split('.')[-1]
            shared = [k for k, v in sigs.items() if k.endswith('.' + short) and v['kind'] == 'hooked']
            if shared:
                sig = sigs[shared[0]]
                chain = f'{chain} (as {shared[0]})'
        hpath, body, line = function_body(sources, handler, path)
        if sig is None:
            missing.append(f'{chain} ({handler}, {path.relative_to(SRC)}): not in the signatures (not installed this session?)')
            continue
        if not body:
            continue
        where = f'{hpath.relative_to(SRC)}:{line} {handler}'
        for acc, i in reads_in(body):
            checked += 1
            if acc in RESULTS:
                want, ok = RESULTS[acc]
                ret = sig['ret']
                if ret is None or not ok(ret[1], ret[0]):
                    findings.append(f'{chain}: the result is {ret[0] if ret else "void"}, {where} reads it as {want}')
                continue
            want, ok = ACCESSORS[acc]
            if i >= len(sig['params']):
                findings.append(f'{chain}: argument {i} does not exist ({len(sig["params"])} parameters), {where} reads it as {want}')
                continue
            pname, (tname, kind, size) = sig['params'][i]
            if not ok(kind, tname, size):
                findings.append(f'{chain}: argument {i} {pname} is {tname} (kind {kind}, {size} bytes), {where} reads it as {want}')
    return findings, missing, checked


def script_structs(scripts):
    """Script names declared as structs in the decompiled scripts."""
    structs = set()
    for p in scripts.rglob('*.swift'):
        for m in re.finditer(r'^\s*(?:public |private |protected |final |native |importonly |abstract )*struct (\w+)', p.read_text(encoding='utf-8', errors='replace'), re.M):
            structs.add(m.group(1))
    return structs


def game_callbacks(scripts, names):
    """The game's own cb funcs of these names: name -> [(class file, [param types])]."""
    out = {n: [] for n in names}
    rx = re.compile(r'cb func (' + '|'.join(map(re.escape, names)) + r')\(([^)]*)\)')
    for p in scripts.rglob('*.swift'):
        for m in rx.finditer(p.read_text(encoding='utf-8', errors='replace')):
            types = [t.split(':', 1)[1].strip() for t in m.group(2).split(',') if ':' in t]
            out[m.group(1)].append((p.name, types))
    return out


def check_callbacks(sigs, scripts):
    defined = {k: v for k, v in sigs.items() if v['kind'] == 'defined' and k.split('.')[-1].startswith('On')}
    if not defined or not scripts.exists():
        return []
    structs = script_structs(scripts)
    games = game_callbacks(scripts, sorted({k.split('.')[-1] for k in defined}))
    findings = []
    for chain, sig in defined.items():
        name = chain.split('.')[-1]
        ours = sig['params']
        for file, types in games.get(name, []):
            if len(types) != len(ours):
                continue
            for i, (t, (pname, (tname, kind, _))) in enumerate(zip(types, ours)):
                base = re.sub(r'^(ref|wref|script_ref)<(.*)>$', r'\2', t)
                game_struct = base in structs and not t.startswith(('ref<', 'wref<'))
                ours_handle = kind in (KIND_HANDLE, KIND_WEAK)
                if game_struct and ours_handle:
                    findings.append(f'{chain}: parameter {i} {pname} declared {tname} (a handle); the game passes {t}, a struct ({file})')
                elif not game_struct and t.startswith(('ref<', 'wref<')) and not ours_handle:
                    findings.append(f'{chain}: parameter {i} {pname} declared {tname} (not a handle); the game passes {t} ({file})')
            break
    return sorted(set(findings))


class Pdb:
    """dll RVAs -> function names through dbghelp and the build's PDB."""

    def __init__(self, dll):
        self.d = c.WinDLL('dbghelp')
        self.d.SymInitializeW.argtypes = [w.HANDLE, w.LPCWSTR, w.BOOL]
        self.d.SymLoadModuleExW.argtypes = [w.HANDLE, w.HANDLE, w.LPCWSTR, w.LPCWSTR, c.c_uint64, w.DWORD, c.c_void_p, w.DWORD]
        self.d.SymLoadModuleExW.restype = c.c_uint64
        self.h = w.HANDLE(0x4321)
        self.d.SymSetOptions(0x2 | 0x10)
        self.d.SymInitializeW(self.h, None, False)
        self.base = self.d.SymLoadModuleExW(self.h, None, str(dll), None, 0x180000000, 0, None, 0)

    def name(self, rva):
        class SI(c.Structure):
            _fields_ = [('SizeOfStruct', w.ULONG), ('TypeIndex', w.ULONG), ('Reserved', c.c_uint64 * 2), ('Index', w.ULONG),
                        ('Size', w.ULONG), ('ModBase', c.c_uint64), ('Flags', w.ULONG), ('Value', c.c_uint64),
                        ('Address', c.c_uint64), ('Register', w.ULONG), ('Scope', w.ULONG), ('Tag', w.ULONG),
                        ('NameLen', w.ULONG), ('MaxNameLen', w.ULONG), ('Name', c.c_char * 512)]

        class LINE(c.Structure):
            _fields_ = [('SizeOfStruct', w.DWORD), ('Key', c.c_void_p), ('LineNumber', w.DWORD), ('FileName', c.c_char_p), ('Address', c.c_uint64)]
        s = SI()
        s.SizeOfStruct = c.sizeof(SI) - 512 + 1
        s.MaxNameLen = 500
        disp = c.c_uint64()
        ok = self.d.SymFromAddr(self.h, c.c_uint64(self.base + rva), c.byref(disp), c.byref(s))
        ln = LINE()
        ln.SizeOfStruct = c.sizeof(LINE)
        d2 = w.DWORD()
        where = ''
        if self.d.SymGetLineFromAddr64(self.h, c.c_uint64(self.base + rva), c.byref(d2), c.byref(ln)):
            f = ln.FileName.decode(errors='replace').replace('\\', '/')
            f = f.split('/rust/crates/')[-1] if '/rust/crates/' in f else f.split('/')[-1]
            where = f'{f}:{ln.LineNumber}'
        nm = s.Name.decode(errors='replace') if ok else '?'
        return f'{nm.split("::")[-1]} {where}'.strip()


def runtime(log, pdb):
    lines = [l for l in log.read_text(encoding='utf-8', errors='replace').splitlines() if l.startswith('[audit] ')]
    out = []
    own = ('script_hooks', 'lobby_rtti', 'audit.rs')

    def callers(m):
        rvas = [int(x, 16) for x in m.group(1).split(',') if x]
        named = [pdb.name(r) for r in rvas] if pdb else [hex(r) for r in rvas]
        # the first callers outside the audit and the call machinery
        mine = [n for n in named if not any(o in n for o in own)]
        return 'from ' + ' <- '.join(mine[:3] or named[:3])

    def hook_at(m):
        return 'by ' + (pdb.name(int(m.group(1), 16)) if pdb else m.group(1))
    for l in lines:
        l = re.sub(r'from dll\+\[([0-9a-fx,]*)\]', callers, l[8:])
        l = re.sub(r'by the hook at cyberpunkvrport\.dll\+(0x[0-9a-f]+)', hook_at, l)
        out.append(l)
    return out


def main():
    a = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    a.add_argument('--log', type=Path, default=LOG_DIR / 'cyberpunkvrport.log')
    a.add_argument('--signatures', type=Path, default=LOG_DIR / 'audit_signatures.txt')
    a.add_argument('--scripts', type=Path, default=SCRIPTS)
    a.add_argument('--pdb-dll', type=Path, default=DLL)
    args = a.parse_args()
    sources = rust_sources()
    if args.signatures.exists():
        sigs = read_signatures(args.signatures)
        findings, missing, checked = check_hooks(sigs, sources)
        print(f'== hooks: {len(sigs)} signatures, {checked} reads checked, {len(findings)} wrong')
        for f in findings:
            print('  WRONG', f)
        for m in missing:
            print('  not installed:', m)
        cb = check_callbacks(sigs, args.scripts)
        print(f'== callbacks the port defines: {len(cb)} wrong')
        for f in cb:
            print('  WRONG', f)
    else:
        print(f'== no {args.signatures} (start the game with this build once)')
    if args.log.exists():
        pdb = Pdb(args.pdb_dll) if args.pdb_dll.exists() else None
        rt = runtime(args.log, pdb)
        print(f'== runtime ({args.log.name}): {len(rt)} wrong reads said')
        for l in rt:
            print('  ', l)
    return 0


if __name__ == '__main__':
    sys.exit(main())
