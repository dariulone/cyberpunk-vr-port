"""Every game address the native code names, tied to something that survives a patch.

    py -3.13 -B tools/patchday/inventory.py [--game <dir with bin/x64>] [--out <json>]

Scans src/ and include/ for hex literals inside the image (0x10000 .. SizeOfImage), classifies each
by its context, and for every distinct value records:

  * where it lands (section, the .pdata function holding it),
  * an anchor in CDPR's address library (exact entry, the function's entry + offset, or the nearest
    entry before it for data),
  * the masked hash of the function holding it (same hash in a new build = same code, so the offset
    carries over) and a masked instruction window around it for a search when the function changed,
  * for .rdata, the library hashes of the first pointers there (a vtable is re-found by its slots).

It also re-checks what the source already asserts about 2.31 -- the {rva, length, FNV} code guards,
the Site{rva,"hex"} prologues and {rva,{bytes}} arrays -- against the exe, so a guard that is wrong
today is reported before a patch is blamed for it.

The result (tools/patchday/data/rva_db-<exe timestamp>.json) is what remap.py reads on patch day.
"""
import argparse
import collections
import datetime
import json
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import pe  # noqa: E402
import sigs  # noqa: E402

REPO = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..'))
DEFAULT_GAME = os.path.join(REPO, 'build', 'patchday', 'base-2.31')
SOURCE_DIRS = ('src', 'include')
# The build identity remap.py itself writes: its unmapped list holds OLD values on purpose.
SKIP_FILES = {'include/Core/GameBuild.hpp'}
SOURCE_EXT = ('.cpp', '.hpp', '.h', '.inc', '.asm', '.c', '.cc', '.cxx')

LIT = re.compile(r'(?<![\w.])0[xX]([0-9A-Fa-f]+)([uUlL]*)\b')
SUBNAME = re.compile(r'\b(sub|loc|off|unk|qword|dword|byte|word|stru|vtbl|j_sub)_(14[0-9A-Fa-f]{7})\b')
ABS = re.compile(r'(?<![\w.])0[xX](14[0-9A-Fa-f]{7})\b')
FNV_TRIPLE = re.compile(r'\{\s*0[xX]([0-9A-Fa-f]+)[uU]?\s*,\s*(0[xX][0-9A-Fa-f]+|\d+)[uU]?\s*,\s*0[xX]([0-9A-Fa-f]{9,16})(?:ull|ULL|ul|UL|u|U)?\s*\}')
HEX_SITE = re.compile(r'\{\s*0[xX]([0-9A-Fa-f]+)[uU]?\s*,\s*"([0-9A-Fa-f\s]+)"\s*\}')
BYTES_SITE = re.compile(r'\{\s*0[xX]([0-9A-Fa-f]+)[uU]?\s*,\s*\{((?:\s*0[xX][0-9A-Fa-f]{1,2}\s*,?)+)\}\s*\}')
BYTE_ARRAY = re.compile(r'(?:const\s+|static\s+|constexpr\s+)*(?:uint8_t|std::uint8_t|unsigned\s+char|BYTE)\s+(\w+)\s*\[\s*\d*\s*\]\s*=?\s*\{([^}]*)\}')
BYTE_TOKEN = re.compile(r'0[xX][0-9A-Fa-f]{1,2}\b|(?<![\w.])\d{1,3}\b')
NAMED = re.compile(r'(\w*(?:rva|Rva|RVA)\w*)\s*(?:=|\{|\()\s*$')
NAMED_ANY = re.compile(r'\b(\w+)\s*=\s*$')
BASEISH = re.compile(r'(?:\b(?:base|g_exe_base|s_base|s_exe|exe|exeBase|exe_base|g_exe|module|moduleBase|image|imageBase|imageBase_|game|gameBase|codeBase|g_base|kBase|mod|hMod|hModule|imageStart)\b\)?|Base\(\)|GetModuleHandle[AW]?\([^)]*\)\)?|reinterpret_cast<uintptr_t>\([^)]*(?:g_exe_base|s_exe|hMod)[^)]*\))\s*\+\s*\(?[^;]{0,40}$', re.I)
FIELDISH = re.compile(r'(?:[A-Za-z_]\w*|\]|\))\s*\+\s*$')


def strip_comment_state(lines):
    """Per line: index where a comment starts (len if none) and whether the line starts inside /* */."""
    out = []
    in_block = False
    for line in lines:
        i = 0
        n = len(line)
        start = n
        in_str = None
        code_mask = [True] * n
        while i < n:
            c = line[i]
            if in_block:
                code_mask[i] = False
                if line.startswith('*/', i):
                    code_mask[i + 1] = False
                    in_block = False
                    i += 2
                    continue
                i += 1
                continue
            if in_str:
                if c == '\\':
                    i += 2
                    continue
                if c == in_str:
                    in_str = None
                i += 1
                continue
            if c in '"\'':
                in_str = c
                i += 1
                continue
            if line.startswith('//', i) or (line.startswith(';', i) and False):
                for k in range(i, n):
                    code_mask[k] = False
                start = min(start, i)
                break
            if line.startswith('/*', i):
                in_block = True
                code_mask[i] = False
                code_mask[i + 1] = False
                start = min(start, i)
                i += 2
                continue
            i += 1
        out.append(code_mask)
    return out


def classify(line, col, end, value, in_code, fname):
    before = line[:col]
    after = line[end:]
    if re.search(r'\.(?:dll|asi)\s*\+\s*$', before, re.I):
        # An offset inside another module ("cyberpunkvrport.dll+0x2CDBF" in a crash note).
        return 'foreign-module', 'none'
    if not in_code:
        return 'comment', 'weak'
    if fname.endswith('.asm'):
        return 'asm', 'weak'
    if re.search(r'CVR_DETOUR(?:_IF)?\s*\(', before):
        return 'detour', 'strong'
    if re.search(r'(?:Cet|CET|cet)' + chr(92) + 'w*' + chr(92) + 's*(?:=|,)' + chr(92) + 's*$', before) or re.search(r'Field' + chr(92) + '(read,user,cet,', before) or re.search(r'cet' + chr(92) + 's*>' + chr(92) + 's*UINT64_MAX', before):
        return 'foreign', 'none'
    if NAMED.search(before):
        return 'named', 'strong'
    if re.search(r'UINT64_MAX\s*-\s*$', before):
        return 'bound', 'medium'
    if BASEISH.search(before):
        return 'base-plus', 'strong'
    if FIELDISH.search(before):
        return 'offset-like', 'weak'
    if re.search(r'\{\s*$', before) and re.match(r'\s*,\s*"', after):
        return 'table', 'strong'
    if re.search(r'"\s*,\s*$', before) and re.match(r'\s*\}', after):
        return 'table', 'strong'
    if re.search(r'(?:callers|returns|Callers|Returns|sites|Sites|caller|Caller|Return|ret)\w*\s*(?:\[\d*\])?\s*[\{=(][^;]*$', before):
        return 'caller-list', 'strong'
    if re.search(r'\b\w*(?:rva|Rva|RVA|caller|Caller|ret|Ret|return|Return|rip|Rip|site|Site|work|Work)\w*\s*(?:==|!=)\s*\(?\s*$', before):
        return 'compare', 'strong'
    m = NAMED_ANY.search(before)
    if m:
        if re.search(r'(?:Size|SIZE|Bytes|BYTES|Count|COUNT|Stride|STRIDE|Capacity|Mask|MASK|Limit|Max|MAX|Min)', m.group(1)):
            return 'size-name', 'none'
        return 'assigned', 'medium'
    if re.search(r'(?:==|!=)\s*\(?\s*$', before):
        return 'compare', 'medium'
    return 'other', 'weak'


def scan(repo):
    occurrences = []
    guards = []
    for d in SOURCE_DIRS:
        for dp, _, files in os.walk(os.path.join(repo, d)):
            for f in sorted(files):
                if not f.endswith(SOURCE_EXT):
                    continue
                path = os.path.join(dp, f)
                rel = os.path.relpath(path, repo).replace('\\', '/')
                if rel in SKIP_FILES:
                    continue
                with open(path, encoding='utf-8', errors='replace') as fh:
                    text = fh.read()
                lines = text.split('\n')
                masks = strip_comment_state(lines)
                for ln, line in enumerate(lines, 1):
                    mask = masks[ln - 1]
                    for m in LIT.finditer(line):
                        v = int(m.group(1), 16)
                        in_code = mask[m.start()] if m.start() < len(mask) else True
                        kind, conf = classify(line, m.start(), m.end(), v, in_code, f)
                        occurrences.append(dict(file=rel, line=ln, col=m.start(), end=m.end(),
                                                literal=m.group(0), value=v, kind=kind, confidence=conf,
                                                text=line.strip()[:200]))
                    for m in SUBNAME.finditer(line):
                        occurrences.append(dict(file=rel, line=ln, col=m.start(2), end=m.end(2),
                                                literal=m.group(2), value=int(m.group(2), 16) - pe.IMAGE_BASE,
                                                kind='ida-name', confidence='weak', text=line.strip()[:200]))
                    for m in FNV_TRIPLE.finditer(line):
                        guards.append(dict(type='fnv', file=rel, line=ln, rva=int(m.group(1), 16),
                                           length=int(m.group(2), 0), fnv=int(m.group(3), 16),
                                           len_literal=m.group(2), fnv_span=[m.start(3) - 2, m.end(3)],
                                           text=m.group(0)))
                    for m in HEX_SITE.finditer(line):
                        guards.append(dict(type='hex', file=rel, line=ln, rva=int(m.group(1), 16),
                                           bytes=re.sub(r'\s+', '', m.group(2)),
                                           hex_span=[m.start(2), m.end(2)], text=m.group(0)))
                    for m in BYTES_SITE.finditer(line):
                        bs = [int(x, 16) for x in re.findall(r'0[xX]([0-9A-Fa-f]{1,2})', m.group(2))]
                        guards.append(dict(type='bytes', file=rel, line=ln, rva=int(m.group(1), 16),
                                           bytes=bytes(bs).hex(), text=m.group(0)[:120]))
                for m in BYTE_ARRAY.finditer(text):
                    toks = BYTE_TOKEN.findall(m.group(2))
                    if not toks or any(int(t, 0) > 255 for t in toks):
                        continue
                    guards.append(dict(type='array', file=rel, line=text.count('\n', 0, m.start()) + 1,
                                       name=m.group(1), bytes=bytes(int(t, 0) for t in toks).hex(),
                                       init_span=[m.start(2), m.end(2)], rva=None, text=m.group(0)[:120]))
    return occurrences, guards


def resolve_array_guards(img, guards, occurrences):
    """A named byte array is a guard when the bytes it holds are the game's at an address the same
    code names nearby (memcmp(base+0x.., name, ..), or an address computed a line or two away).
    Only pairs the exe confirms byte for byte are kept; jump templates and patch bytes never match
    the game and drop out by themselves."""
    by_file = collections.defaultdict(list)
    for o in occurrences:
        if o['kind'] not in ('comment', 'ida-name', 'fnv-value') and 0x10000 <= o['value'] < img.size_of_image:
            by_file[o['file']].append(o)
    kept = []
    for g in guards:
        if g['type'] != 'array':
            kept.append(g)
            continue
        want = bytes.fromhex(g['bytes'])
        if len(want) < 3 or want.count(0) == len(want):
            continue
        best = None
        for o in sorted(by_file[g['file']], key=lambda o: abs(o['line'] - g['line'])):
            if abs(o['line'] - g['line']) > 25:
                break
            for d in range(-16, 17):
                if img.read(o['value'] + d, len(want)) == want:
                    best = (o['value'], d)
                    break
            if best:
                break
        if best:
            g['rva'], g['base_value'], g['delta'] = best[0] + best[1], best[0], best[1]
            kept.append(g)
    return kept


def mark_guard_roles(occurrences, guards):
    """The rva inside a guard tuple is strong whatever classify() made of it; the FNV and length
    literals of a triple are not addresses."""
    by_line = collections.defaultdict(list)
    for o in occurrences:
        by_line[(o['file'], o['line'])].append(o)
    for g in guards:
        for o in by_line[(g['file'], g['line'])]:
            if o['value'] == g['rva'] and o['kind'] not in ('comment', 'ida-name'):
                o['kind'], o['confidence'] = 'guard-' + g['type'], 'strong'
            if g['type'] == 'fnv' and o['col'] >= 0 and o['literal'].lower().endswith(
                    ('%x' % g['fnv']).lower()[-6:]) and o['value'] == g['fnv']:
                o['kind'], o['confidence'] = 'fnv-value', 'none'


def anchor_for(img, lib, v):
    s = img.section_of(v)
    if s is None:
        return None, None
    exact = lib.exact(v)
    uniq = [h for h in exact if len(lib.by_hash[h]) == 1]
    if s.code:
        f = img.function_at(v)
        if uniq:
            return dict(kind='exact', hash=uniq[0], rva=v, delta=0, symbol=lib.symbol(uniq[0])), f
        if f:
            b, e, root = f
            fe = [h for h in lib.exact(b) if len(lib.by_hash[h]) == 1]
            if fe:
                return dict(kind='func', hash=fe[0], rva=b, delta=v - b, symbol=lib.symbol(fe[0])), f
            if root != b:
                re_ = [h for h in lib.exact(root) if len(lib.by_hash[h]) == 1]
                if re_:
                    return dict(kind='fragment', hash=re_[0], rva=root, delta=v - root,
                                symbol=lib.symbol(re_[0])), f
        else:
            # The exclusive end of a function (a range literal's upper bound).
            g = img.function_at(v - 1)
            if g:
                fe = [h for h in lib.exact(g[0]) if len(lib.by_hash[h]) == 1]
                if fe:
                    return dict(kind='func-end', hash=fe[0], rva=g[0], delta=v - g[0],
                                symbol=lib.symbol(fe[0])), g
        p = lib.preceding(v)
        if p and len(lib.by_hash[p[0]]) == 1 and v - p[1] < 0x2000:
            return dict(kind='preceding', hash=p[0], rva=p[1], delta=v - p[1], symbol=lib.symbol(p[0])), f
        return None, f
    if uniq:
        return dict(kind='exact', hash=uniq[0], rva=v, delta=0, symbol=lib.symbol(uniq[0])), None
    # Data: walk back to the nearest entry whose hash is unique.
    i = int(lib.rvas.searchsorted(v, side='right')) - 1
    while i >= 0 and v - int(lib.rvas[i]) < 0x40000:
        h = int(lib.hashes[i])
        if len(lib.by_hash[h]) == 1 and img.section_of(int(lib.rvas[i])) is s:
            return dict(kind='preceding', hash=h, rva=int(lib.rvas[i]), delta=v - int(lib.rvas[i]),
                        symbol=lib.symbol(h)), None
        i -= 1
    return None, None


def rdata_slots(img, lib, v, count=8):
    out = []
    raw = img.read(v, 8 * count)
    if raw is None:
        return out
    relocs = set(int(r) for r in img.relocs_in(v, v + 8 * count))
    for k in range(count):
        if v + 8 * k not in relocs:
            break
        ptr = int.from_bytes(raw[8 * k:8 * k + 8], 'little') - img.image_base
        # EVERY hash at the slot's target: functions the linker folded together (ICF) share one
        # address and one of several names, and which ones are folded changes from build to build.
        out.append(lib.exact(ptr))
    return out


def build(game_dir, repo=REPO, overrides=None):
    exe, libpath = pe.find_game_files(game_dir)
    img = pe.Image(exe)
    lib = pe.AddressLibrary(libpath, img)
    occurrences, guards = scan(repo)
    mark_guard_roles(occurrences, guards)
    guards = resolve_array_guards(img, guards, occurrences)

    # IDA names (sub_14...) are kept apart: they are added once below, for values that are entries.
    in_image = [o for o in occurrences if o['kind'] not in ('fnv-value', 'ida-name') and
                (0x10000 <= o['value'] < img.size_of_image)]
    values = sorted(set(o['value'] for o in in_image) | set(g['rva'] for g in guards))
    func_cache = {}

    def insns_of(f):
        if f is None:
            return None
        key = (f[0], f[1])
        if key not in func_cache:
            func_cache[key] = sigs.decode(img, f[0], f[1])
        return func_cache[key]

    entries = {}
    for v in values:
        s = img.section_of(v)
        if s is None:
            continue
        anchor, f = anchor_for(img, lib, v)
        e = dict(rva=v, section=s.name, anchor=anchor)
        if s.code:
            fn = img.function_at(v)
            if fn is None and anchor and anchor['kind'] == 'func-end':
                fn = img.function_at(v - 1)
                e['func_end'] = True
            if fn is None and anchor and anchor['kind'] == 'exact':
                # A leaf: no unwind info, so no .pdata entry; its extent is its first return.
                ins = sigs.decode_leaf(img, v)
                if ins:
                    e['leaf'] = [v, ins[-1].rva + ins[-1].size]
                    e['fhash'] = sigs.masked_hash(img, ins)
                    e['window'], e['wofs'] = sigs.window(img, ins, v, before=0, after=10)
                    e['on_insn'] = True
            if fn:
                ins = insns_of(fn)
                e['func'] = [fn[0], fn[1], fn[2]]
                e['fhash'] = sigs.masked_hash(img, ins)
                if not e.get('func_end'):
                    pat, wofs = sigs.window(img, ins, v, before=0 if v == fn[0] else 6, after=10)
                    e['window'], e['wofs'] = pat, wofs
                    e['on_insn'] = any(i.rva == v for i in ins)
        elif s.name == '.rdata':
            e['slots'] = rdata_slots(img, lib, v)
        e['refs'] = 0
        entries[v] = e
    for o in in_image:
        if o['value'] in entries:
            entries[o['value']]['refs'] += 1

    # Guards: check what the source asserts about this exe today.
    checks = []
    for g in guards:
        raw = None
        if g['type'] == 'fnv':
            raw = img.read(g['rva'], g['length'])
            ok = raw is not None and pe.fnv1a64(raw) == g['fnv']
            f = img.function_at(g['rva'])
            g['mhash'] = sigs.range_masked_hash(img, g['rva'], g['length'], insns_of(f) if f else None)
        else:
            want = bytes.fromhex(g['bytes'])
            raw = img.read(g['rva'], len(want))
            ok = raw == want
            f = img.function_at(g['rva'])
            g['mhash'] = sigs.range_masked_hash(img, g['rva'], len(want), insns_of(f) if f else None)
        g['ok_on_base'] = bool(ok)
        checks.append(ok)

    decide(entries, in_image + [o for o in occurrences if o['kind'] in ('ida-name', 'comment')], overrides)

    db = dict(
        schema=1,
        created=datetime.datetime.now().isoformat(timespec='seconds'),
        exe=dict(path=exe, timestamp=img.timestamp, size_of_image=img.size_of_image,
                 linker=lib.timestamp, sha256=_sha256(exe)),
        entries=[entries[k] for k in sorted(entries)],
        occurrences=in_image + [o for o in occurrences if o['kind'] == 'ida-name' and o['value'] in entries],
        guards=guards,
    )
    return db, img, lib


OVERRIDES = os.path.join(os.path.dirname(os.path.abspath(__file__)), 'data', 'literal_overrides.json')


def load_overrides(path=OVERRIDES):
    with open(path, encoding='utf-8') as f:
        j = json.load(f)
    return ({int(k, 16) for k in j.get('not_rva', {})}, {int(k, 16) for k in j.get('rva', {})},
            {int(k, 16) for k in j.get('text_end', [])})


def decide(entries, occurrences, overrides=None):
    """Per value: rva / not-rva / foreign / text-end / doc / undecided.

    A value is one thing everywhere it appears (an address is not also a struct offset), so the
    strongest context anywhere decides for every occurrence; the hand overrides win over that."""
    not_rva, force, text_end = load_overrides(overrides or OVERRIDES)
    by = collections.defaultdict(list)
    for o in occurrences:
        # Another module's offset says nothing about what the value is in the game.
        if o['kind'] != 'foreign-module':
            by[o['value']].append(o)
    for v, e in entries.items():
        occ = by.get(v, [])
        code = [o for o in occ if o['kind'] not in ('comment', 'ida-name')]
        kinds = {o['kind'] for o in code}
        a = e.get('anchor') or {}
        if v in not_rva:
            d = 'not-rva'
        elif v in text_end:
            d = 'text-end'
        elif 'foreign' in kinds:
            d = 'foreign'
        elif v in force or any(o['confidence'] == 'strong' for o in code):
            d = 'rva'
        elif code and a.get('kind') == 'exact' and not kinds & {'offset-like', 'size-name'}:
            d = 'rva'
        elif not code:
            d = 'doc'
        else:
            d = 'undecided'
        e['decision'] = d


def _sha256(path):
    import hashlib
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for chunk in iter(lambda: f.read(1 << 20), b''):
            h.update(chunk)
    return h.hexdigest()


def summary(db):
    ents = db['entries']
    occ = db['occurrences']
    lines = []
    kinds = collections.Counter(o['kind'] for o in occ)
    confs = collections.Counter(o['confidence'] for o in occ)
    anchors = collections.Counter((e['anchor'] or {}).get('kind', 'NONE') for e in ents)
    secs = collections.Counter(e['section'] for e in ents)
    lines.append(f"exe {db['exe']['sha256'][:16]} ts {db['exe']['timestamp']:08x}: "
                 f"{len(ents)} distinct addresses, {len(occ)} occurrences, {len(db['guards'])} guards")
    lines.append('sections: ' + ', '.join(f'{k} {v}' for k, v in secs.most_common()))
    lines.append('anchors:  ' + ', '.join(f'{k} {v}' for k, v in anchors.most_common()))
    lines.append('kinds:    ' + ', '.join(f'{k} {v}' for k, v in kinds.most_common()))
    lines.append('conf:     ' + ', '.join(f'{k} {v}' for k, v in confs.most_common()))
    dec = collections.Counter(e.get('decision') for e in ents)
    lines.append('decision: ' + ', '.join(f'{k} {v}' for k, v in dec.most_common()))
    und = [e for e in ents if e.get('decision') == 'undecided']
    for e in und[:60]:
        o = next((o for o in occ if o['value'] == e['rva'] and o['kind'] not in ('comment', 'ida-name')), None)
        lines.append(f"   UNDECIDED {e['rva']:#x} {o and o['file']}:{o and o['line']}  {o and o['text'][:110]}")
    odd = [e for e in ents if e.get('decision') == 'rva' and e['section'] == '.text' and e.get('on_insn') is False]
    lines.append(f'rva values not on an instruction boundary: {len(odd)}')
    for e in odd[:60]:
        o = next((o for o in occ if o['value'] == e['rva'] and o['kind'] not in ('comment', 'ida-name')), None)
        lines.append(f"   MIDINSN {e['rva']:#x} {(e.get('anchor') or {}).get('kind')} {o and o['file']}:{o and o['line']}  {o and o['text'][:100]}")
    bad = [g for g in db['guards'] if not g['ok_on_base']]
    lines.append(f"guards failing on this exe: {len(bad)} of {len(db['guards'])}")
    for g in bad[:40]:
        lines.append(f"   {g['file']}:{g['line']} {g['type']} {g['rva']:#x} {g['text'][:80]}")
    return '\n'.join(lines)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--game', default=DEFAULT_GAME, help='directory holding bin/x64/Cyberpunk2077.exe')
    ap.add_argument('--out', default=None)
    ap.add_argument('--repo', default=REPO, help='tree to scan (a scratch copy in tests)')
    ap.add_argument('--overrides', default=OVERRIDES, help='hand decisions (literal_overrides.json)')
    args = ap.parse_args()
    db, img, lib = build(args.game, os.path.abspath(args.repo), args.overrides)
    out = args.out or os.path.join(os.path.dirname(os.path.abspath(__file__)), 'data',
                                   f"rva_db-{db['exe']['timestamp']:08x}.json")
    os.makedirs(os.path.dirname(out), exist_ok=True)
    with open(out, 'w', encoding='utf-8') as f:
        json.dump(db, f, indent=1)
    print(summary(db))
    print('wrote', out)


if __name__ == '__main__':
    main()
