"""C global definitions (a *State.cpp file) -> Rust #[no_mangle] statics.

usage: globals_gen.py <cpp> <out.rs> <exports.txt> [NAME=VALUE ...]   (constants for array dimensions)
Prints a report of every declaration it understood; fails on anything it did not. The statics it writes are then
checked byte for byte against the C++ object's initial data (coff_data.py compare <cpp.obj> <rust.lib> <names>), so
a literal it renders differently (NAN's sign, a float's rounding, C's zero padding) cannot pass unnoticed.
"""
import re, sys, math

args = sys.argv[1:]
obj = None
if '--obj' in args:
    i = args.index('--obj'); obj = args[i + 1]; del args[i:i + 2]
src, out_rs, out_exports = args[:3]
consts = dict(a.split('=') for a in args[3:])
objdata = None
if obj:
    sys.path.insert(0, __import__('os').path.dirname(__file__))
    import coff_data
    objdata = coff_data.load(obj)
text = open(src, encoding='utf-8').read()
# strip comments
text = re.sub(r'/\*.*?\*/', '', text, flags=re.S)
text = re.sub(r'//[^\n]*', '', text)
# drop preprocessor lines
text = '\n'.join(l for l in text.split('\n') if not l.strip().startswith('#'))
# only data: a type or function body would be split into pieces that look like declarations
body = re.search(r'\b(struct|class|union|enum)\s+\w*\s*(:[^{;]*)?\{|\)\s*(const\s*)?(noexcept\s*)?\{', text)
if body:
    raise SystemExit(f'not a data-only file: {text[body.start():body.start() + 60]!r}')
# remove namespace / extern "C" block wrappers
text = re.sub(r'namespace\s+[\w:]+\s*\{', '', text)
text = re.sub(r'extern\s+"C"\s*\{', '', text)

TYPES = {
    'bool': ('bool', 'false'), 'int': ('i32', '0'), 'int32_t': ('i32', '0'), 'unsigned': ('u32', '0'),
    'uint32_t': ('u32', '0'), 'unsigned int': ('u32', '0'), 'uint64_t': ('u64', '0'),
    'unsigned long long': ('u64', '0'), 'int64_t': ('i64', '0'), 'long long': ('i64', '0'),
    'float': ('f32', '0.0'), 'double': ('f64', '0.0'), 'uintptr_t': ('usize', '0'), 'uint8_t': ('u8', '0'),
    'int8_t': ('i8', '0'), 'uint16_t': ('u16', '0'), 'int16_t': ('i16', '0'),
}
ATOMICS = {'uint32_t': 'AtomicU32', 'int': 'AtomicI32', 'int32_t': 'AtomicI32', 'uint64_t': 'AtomicU64',
           'float': 'AtomicU32', 'bool': 'AtomicBool', 'int64_t': 'AtomicI64'}

def split_top(s, sep=','):
    parts, depth, cur = [], 0, ''
    for ch in s:
        if ch in '{([': depth += 1
        if ch in '})]': depth -= 1
        if ch == sep and depth == 0:
            parts.append(cur); cur = ''
        else:
            cur += ch
    parts.append(cur)
    return [p.strip() for p in parts]

def literal(v, rtype):
    v = v.strip()
    if v in ('NAN',):
        return 'f32::NAN' if rtype == 'f32' else 'f64::NAN'
    if v in ('true', 'false'):
        assert rtype == 'bool', (v, rtype); return v
    if v == 'nullptr':
        return 'None'
    m = re.fullmatch(r'(-?)(0[xX][0-9a-fA-F]+|\d+)([uUlL]*)', v)
    if m and rtype not in ('f32', 'f64'):
        if rtype == 'bool':
            return 'true' if int(m.group(2), 0) else 'false'
        return f'{m.group(1)}{m.group(2)}'
    m = re.fullmatch(r'(-?)(\d+\.?\d*(?:[eE][-+]?\d+)?|\.\d+)[fF]?', v)
    if m and rtype in ('f32', 'f64'):
        num = m.group(2)
        if num.startswith('.'): num = '0' + num
        if '.' not in num and 'e' not in num.lower(): num += '.0'
        if num.endswith('.'): num += '0'
        return f'{m.group(1)}{num}'
    raise ValueError(f'literal {v!r} for {rtype}')

def zero(rtype):
    return TYPES_R0[rtype]
TYPES_R0 = {'bool': 'false', 'i32': '0', 'u32': '0', 'u64': '0', 'i64': '0', 'f32': '0.0', 'f64': '0.0', 'usize': '0',
            'u8': '0', 'i8': '0', 'u16': '0', 'i16': '0'}

def init_value(dims, rtype, init):
    """Rust value for a C initializer (None = zero), padded like C."""
    if not dims:
        if init is None or init.strip() in ('{}', ''):
            return TYPES_R0[rtype]
        s = init.strip()
        if s.startswith('{'):
            inner = s[1:-1].strip()
            return TYPES_R0[rtype] if not inner else literal(inner, rtype)
        return literal(s, rtype)
    n = dims[0]
    if init is None or init.strip() in ('{}', ''):
        sub = init_value(dims[1:], rtype, None)
        return f'[{sub}; {n}]'
    s = init.strip()
    assert s.startswith('{') and s.endswith('}'), s
    items = [i for i in split_top(s[1:-1]) if i != '']
    assert len(items) <= n, (items, n)
    vals = [init_value(dims[1:], rtype, i) for i in items]
    if len(vals) < n:
        sub = init_value(dims[1:], rtype, None)
        if not vals:
            return f'[{sub}; {n}]'
        vals += [sub] * (n - len(vals))
    return '[' + ', '.join(vals) + ']'

def rust_type(dims, rtype):
    t = rtype
    for d in reversed(dims):
        t = f'[{t}; {d}]'
    return t

FN_RE = re.compile(r'^(?P<ret>[\w:]+)\s*\(\s*\*\s*(?P<name>\w+)\s*\)\s*\((?P<args>[^)]*)\)\s*(?:=\s*nullptr)?$')

decls = [d.strip() for d in text.split(';')]
rows, exports, report = [], [], []
for d in decls:
    d = ' '.join(d.split())
    while d.startswith('}'):
        d = d[1:].strip()
    if not d:
        continue
    exported = '__declspec(dllexport)' in d
    d = d.replace('extern "C"', '').replace('__declspec(dllexport)', '').replace('volatile', '').strip()
    if d.startswith('extern ') or d.startswith('using ') or d.startswith('static_assert') or d.startswith('struct ')             or d.startswith('typedef '):
        continue
    m = FN_RE.match(d)
    if m:
        ret = {'int64_t': 'i64', 'uint64_t': 'u64', 'bool': 'bool', 'void': None}[m.group('ret')]
        args = m.group('args').strip()
        argt = '' if not args else '*mut core::ffi::c_void'
        t = f'Option<unsafe extern "C" fn({argt})' + (f' -> {ret}' if ret else '') + '>'
        rows.append((m.group('name'), t, 'None', exported, False)); report.append(f'fnptr {m.group("name")}')
        continue
    m = re.match(r'^std::atomic<(\w+)>\s+(\w+)\s*(\[[^\]]*\])?\s*(\{[^}]*\}|=\s*.+)?$', d)
    if m:
        ct, name, dim, init = m.groups()
        at = ATOMICS[ct]
        v = '0'
        if init:
            inner = init.strip().lstrip('=').strip().strip('{}').strip()
            if inner:
                if ct == 'float':
                    fv = float(inner.rstrip('fF'))
                    assert fv == 0.0, inner
                    v = '0'
                elif ct == 'bool':
                    v = inner
                else:
                    v = literal(inner, TYPES[ct][0])
        if dim:
            n = dim.strip('[]').strip()
            n = consts.get(n, n)
            assert not init or init.strip() == '{}', d
            rows.append((name, f'[{at}; {n}]', f'[const {{ {at}::new({0 if ct != "bool" else "false"}) }}; {n}]', exported, True))
        else:
            if ct == 'bool' and v == '0': v = 'false'
            rows.append((name, at, f'{at}::new({v})', exported, True))
        report.append(f'atomic {name}')
        continue
    # pointers: T* name{} / = nullptr (null)
    m = re.match(r'^(?:const\s+)?[\w:]+\s*\*\s*(\w+)\s*(?:\{\}|=\s*nullptr)?$', d)
    if m:
        rows.append((m.group(1), '*mut core::ffi::c_void', 'core::ptr::null_mut()', exported, False))
        report.append(f'pointer {m.group(1)}')
        continue
    # plain: TYPE decl[, decl]...
    m = re.match(r'^(unsigned long long|long long|unsigned int|[\w:]+)\s+(.*)$', d)
    if not m:
        raise SystemExit(f'cannot parse: {d!r}')
    ctype, rest = m.groups()
    if ctype not in TYPES:
        dm = re.match(r'^(\w+)\s*(?:\[[^\]]+\]\s*)*(\{\})?$', rest.strip())
        if not dm or objdata is None or dm.group(1) not in objdata:
            raise SystemExit(f'unknown type {ctype!r} in {d!r} (an opaque global needs --obj)')
        body, size, nrel, sec = objdata[dm.group(1)]
        if nrel:
            raise SystemExit(f'{dm.group(1)}: its section has relocations; write it by hand')
        align = min(16, size & -size)
        ty = f'Opaque{align}'
        rows.append((dm.group(1), f'{ty}<{size}>', f'{ty}([' + ', '.join(str(x) for x in body) + '])' if any(body)
                     else f'{ty}([0; {size}])', exported, False))
        report.append(f'opaque {ctype} {dm.group(1)} ({size} bytes)')
        continue
    rtype = TYPES[ctype][0]
    for decl in split_top(rest):
        dm = re.match(r'^(\w+)\s*((?:\[[^\]]+\]\s*)*)\s*(?:=\s*(.+)|(\{.*\}))?$', decl)
        if not dm:
            raise SystemExit(f'cannot parse declarator {decl!r} in {d!r}')
        name, dimtxt, init1, init2 = dm.groups()
        dims = [consts.get(x.strip(), x.strip()) for x in re.findall(r'\[([^\]]+)\]', dimtxt or '')]
        dims = [int(x, 0) for x in dims]
        init = init1 if init1 is not None else init2
        value = init_value(dims, rtype, init)
        rows.append((name, rust_type(dims, rtype), value, exported, False))
        report.append(f'{ctype}{"".join(f"[{x}]" for x in dims)} {name} = {(init or "{}")[:40]}')

names = [r[0] for r in rows]
dupes = {n for n in names if names.count(n) > 1}
assert not dupes, dupes
with open(out_rs, 'w', encoding='utf-8', newline='\n') as f:
    for align in sorted({int(t[6:t.index('<')]) for _, t, _, _, _ in rows if t.startswith('Opaque')}):
        f.write('/// A C++ object kept as its bytes: its C++ users see its struct; the size and the initial bytes are the\n'
                '/// C++ definition\'s (taken from its object file), the alignment the largest its size allows (at most 16).\n'
                f'#[repr(C, align({align}))]\npub struct Opaque{align}<const N: usize>(pub [u8; N]);\n')
    for name, t, v, exported, atomic in rows:
        f.write('#[no_mangle]\n')
        f.write(f'pub static {"" if atomic else "mut "}{name}: {t} = {v};\n')
with open(out_exports, 'w', encoding='utf-8', newline='\n') as f:
    for name, t, v, exported, atomic in rows:
        if exported:
            f.write(name + '\n')
print('\n'.join(report))
print(f'{len(rows)} statics, {sum(r[3] for r in rows)} exported')
