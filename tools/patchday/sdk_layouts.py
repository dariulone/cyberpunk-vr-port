"""The RED4ext.SDK class layouts the port compiles against: generate the live check, diff two SDKs.

    py -3.13 -B tools/patchday/sdk_layouts.py gen [--sdk <dir>]
        src/Core/SdkLayouts.gen.inc: every class/struct with an RTTI NAME in the SDK headers the port
        includes (and what they include), with its fields. SdkLayouts.cpp compares them with the
        game's RTTI once the types are registered and logs "[sdk-layout] ..." -- on every build,
        before the build gate, so the first launch of a new game build already names each class of
        ours that moved. Re-run after the SDK or the port's SDK includes change (a removed field is a
        compile error in the .inc, a new one just goes unchecked until then).

    py -3.13 -B tools/patchday/sdk_layouts.py diff [--old <git ref>] [--new <git ref>] [--repo <sdk clone>]
        The same classes between two SDK revisions (default: the submodule's HEAD against upstream
        wopss/RED4ext.SDK master, cloned once into build/patchday/frameworks/RED4ext.SDK): sizes,
        fields added / removed / moved, read from the headers' RED4EXT_ASSERT_SIZE and offset comments.

    py -3.13 -B tools/patchday/sdk_layouts.py build [--ref origin/master]
        A trial build of the plugin against that SDK revision (a worktree of the clone, CMake's
        CVR_RED4EXT_SDK_DIR, its own build folder): does the port compile on the new SDK at all.
"""
import argparse
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, '..', '..'))
SDK = os.path.join(REPO, 'externals', 'RED4ext.SDK')
CLONE = os.path.join(REPO, 'build', 'patchday', 'frameworks', 'RED4ext.SDK')
UPSTREAM = 'https://github.com/wopss/RED4ext.SDK.git'
OUT = os.path.join(REPO, 'src', 'Core', 'SdkLayouts.gen.inc')
INCLUDE = re.compile(r'#include\s*<(RED4ext/[^>]+)>')
STRUCT = re.compile(r'^\s*struct\s+(?:__declspec\(align\(0x[0-9A-Fa-f]+\)\)\s+)?(\w+)\s*(?::\s*([^{]+?))?\s*(\{)?\s*$')
FIELD = re.compile(r'^\s*((?:const\s+)?[A-Za-z_][\w:<>, *&]*?[\w>*&])\s+(\w+)\s*(\[[^\]]*\])?\s*;\s*(?://\s*([0-9A-Fa-f]+))?')
NOT_FIELD = re.compile(r'^\s*(static|using|virtual|friend|template|return|typedef|enum|struct|union|RED4EXT_|#|//)')


def port_includes():
    seeds = set()
    for d in ('src', 'include'):
        for dp, _, fs in os.walk(os.path.join(REPO, d)):
            for f in fs:
                if f.endswith(('.cpp', '.hpp', '.h')) and not f.endswith('.gen.inc'):
                    with open(os.path.join(dp, f), encoding='utf-8', errors='replace') as fh:
                        seeds.update(INCLUDE.findall(fh.read()))
    return seeds


def read(sdk_root, rel, git=None):
    """A header of the SDK at a path (or, with git=(clone, ref), at a revision)."""
    if git:
        r = subprocess.run(['git', '-C', git[0], 'show', f'{git[1]}:include/{rel}'], capture_output=True)
        return r.stdout.decode('utf-8', errors='replace') if r.returncode == 0 else None
    p = os.path.join(sdk_root, 'include', rel)
    if not os.path.exists(p):
        return None
    with open(p, encoding='utf-8', errors='replace') as f:
        return f.read()


def closure(sdk_root, git=None):
    """{header: text} of the type headers (Scripting/Natives/...) reachable from the port's includes."""
    todo = [h for h in port_includes() if h.startswith('RED4ext/Scripting/Natives/')]
    out = {}
    while todo:
        h = todo.pop()
        if h in out:
            continue
        text = read(sdk_root, h, git)
        if text is None:
            out[h] = None
            continue
        out[h] = text
        todo += [i for i in INCLUDE.findall(text) if i.startswith('RED4ext/Scripting/Natives/') and i not in out]
    return out


def parse(text):
    """[(qualified C++ name, RTTI NAME, asserted size or None, [(field, offset comment or None)])]."""
    types = []
    lines = text.split('\n')
    ns = []          # (name, depth at which it opened)
    depth = 0
    i = 0
    pending_ns = None
    while i < len(lines):
        line = lines[i].split('//', 1)[0] if not FIELD.match(lines[i]) else lines[i]
        code = lines[i].split('//', 1)[0]
        m_ns = re.match(r'^\s*namespace\s+([\w:]+)\s*(\{)?(.*)$', code)
        if m_ns:
            if m_ns.group(2) and '}' in m_ns.group(3):
                i += 1      # namespace x { struct Y; } -- a forward declaration
                continue
            if m_ns.group(2):
                depth += 1
                ns.append((m_ns.group(1), depth))
            else:
                pending_ns = m_ns.group(1)
            i += 1
            continue
        if pending_ns and code.strip() == '{':
            depth += 1
            ns.append((pending_ns, depth))
            pending_ns = None
            i += 1
            continue
        m = STRUCT.match(code)
        if m and not code.rstrip().endswith(';') and not (i > 0 and lines[i - 1].strip().startswith('template')):
            name = m.group(1)
            # the body: from the brace to the matching "};"
            j = i if m.group(3) else i + 1
            while j < len(lines) and '{' not in lines[j].split('//', 1)[0]:
                j += 1
            d = 0
            body = []
            k = j
            while k < len(lines):
                c = lines[k].split('//', 1)[0]
                body.append((d, lines[k]))
                d += c.count('{') - c.count('}')
                if d == 0:
                    break
                k += 1
            rtti = None
            fields = []
            for bd, bl in body:
                if bd != 1:
                    continue
                n = re.search(r'static\s+constexpr\s+const\s+char\s*\*\s*NAME\s*=\s*"([^"]+)"', bl)
                if n:
                    rtti = n.group(1)
                    continue
                if NOT_FIELD.match(bl) or '(' in bl.split('//', 1)[0]:
                    continue
                f = FIELD.match(bl)
                if not f or re.search(r':\s*\d+\s*;', bl.split('//', 1)[0]):
                    continue
                fname = f.group(2)
                if re.match(r'^unk[0-9A-Fa-f]*$', fname) or fname.startswith('unk'):
                    continue
                fields.append((fname, int(f.group(4), 16) if f.group(4) else None))
            size = None
            tail = '\n'.join(lines[k + 1:k + 4])
            s = re.search(r'RED4EXT_ASSERT_SIZE\(\s*' + name + r'\s*,\s*(0x[0-9A-Fa-f]+|\d+)\s*\)', tail)
            if s:
                size = int(s.group(1), 0)
            if rtti:
                # "namespace RED4ext::anim" and "namespace game::state" are nested namespaces in one line.
                parts = [p for n, _ in ns for p in n.split('::') if p and p != 'RED4ext']
                qual = '::'.join(['RED4ext'] + parts + [name])
                types.append((qual, rtti, size, fields))
            i = k + 1
            continue
        depth += code.count('{') - code.count('}')
        while ns and depth < ns[-1][1]:
            ns.pop()
        i += 1
    return types


def collect(sdk_root, git=None):
    out = {}
    for h, text in sorted(closure(sdk_root, git).items()):
        if text is None:
            continue
        for qual, rtti, size, fields in parse(text):
            out[rtti] = dict(header=h, cpp=qual, size=size, fields=fields)
    return out


def gen(args):
    types = collect(args.sdk)
    headers = sorted({t['header'] for t in types.values()})
    lines = ['// GENERATED by tools/patchday/sdk_layouts.py gen -- do not edit. The RED4ext.SDK classes the port',
             '// includes, with their fields, for SdkLayouts.cpp to compare with the game\'s RTTI.', '']
    lines += [f'#include <{h}>' for h in headers]
    lines.append('')
    nf = 0
    for rtti, t in sorted(types.items()):
        if not t['fields']:
            continue
        ident = re.sub(r'\W', '_', rtti)
        lines.append(f'static const SdkField kF_{ident}[] = {{')
        for fname, _ in t['fields']:
            lines.append(f'    {{"{fname}", static_cast<uint32_t>(offsetof({t["cpp"]}, {fname}))}},')
            nf += 1
        lines.append('};')
    lines.append('')
    lines.append('static const SdkType kSdkTypes[] = {')
    for rtti, t in sorted(types.items()):
        ident = re.sub(r'\W', '_', rtti)
        fl = f'kF_{ident}, static_cast<uint32_t>(std::size(kF_{ident}))' if t['fields'] else 'nullptr, 0'
        lines.append(f'    {{"{rtti}", static_cast<uint32_t>(sizeof({t["cpp"]})), {fl}}},')
    lines.append('};')
    text = '\n'.join(lines) + '\n'
    if args.check:
        with open(OUT, encoding='utf-8', newline='') as f:
            current = f.read()
        same = current == text
        print(f'{len(types)} classes, {nf} fields: {OUT} is ' + ('current' if same else 'STALE -- run sdk_layouts.py gen'))
        sys.exit(0 if same else 1)
    with open(OUT, 'w', encoding='utf-8', newline='\n') as f:
        f.write(text)
    print(f'{len(types)} classes, {nf} fields from {len(headers)} headers -> {OUT}')


def ensure_clone():
    if not os.path.isdir(os.path.join(CLONE, '.git')):
        os.makedirs(os.path.dirname(CLONE), exist_ok=True)
        subprocess.run(['git', 'clone', '--filter=blob:none', '--no-checkout', UPSTREAM, CLONE], check=True)
    subprocess.run(['git', '-C', CLONE, 'fetch', '--quiet', 'origin'], check=True)
    head = subprocess.run(['git', '-C', SDK, 'rev-parse', 'HEAD'], capture_output=True, text=True).stdout.strip()
    # The submodule's own commit, for a diff from it (fetched from the submodule if the clone lacks it: a local
    # commit, or one from a fork).
    if subprocess.run(['git', '-C', CLONE, 'cat-file', '-e', head + '^{commit}'], capture_output=True).returncode:
        subprocess.run(['git', '-C', CLONE, 'fetch', '--quiet', SDK, 'HEAD'], check=False)
    return head


def diff(args):
    head = ensure_clone()
    old_ref = args.old or head
    new_ref = args.new
    log = subprocess.run(['git', '-C', CLONE, 'log', '--oneline', f'{old_ref}..{new_ref}'], capture_output=True, text=True)
    commits = log.stdout.strip().splitlines()
    print(f'SDK {old_ref[:10]} -> {new_ref}: {len(commits)} commits')
    for c in commits[:25]:
        print('   ', c)
    old = collect(None, (CLONE, old_ref))
    new = collect(None, (CLONE, new_ref))
    problems = 0
    for rtti in sorted(old):
        o, n = old[rtti], new.get(rtti)
        if n is None:
            print(f'  GONE     {rtti} ({o["header"]})')
            problems += 1
            continue
        notes = []
        if o['size'] != n['size']:
            notes.append(f'size {o["size"] and hex(o["size"])} -> {n["size"] and hex(n["size"])}')
        of, nf = dict(o['fields']), dict(n['fields'])
        for fname, off in o['fields']:
            if fname not in nf:
                notes.append(f'-{fname}')
            elif off != nf[fname]:
                notes.append(f'{fname} {off and hex(off)} -> {nf[fname] and hex(nf[fname])}')
        notes += [f'+{fname}' for fname in nf if fname not in of]
        if notes:
            problems += 1
            print(f'  CHANGED  {rtti}: ' + ', '.join(notes[:12]) + (' ...' if len(notes) > 12 else ''))
    print(f'{len(old)} classes the port includes; {problems} changed or gone')


def collect_all(sdk_root):
    """Every RTTI-named class of the SDK's type headers, not only the ones the port includes."""
    out = {}
    root = os.path.join(sdk_root, 'include')
    for dp, _, fs in os.walk(os.path.join(root, 'RED4ext', 'Scripting', 'Natives')):
        for f in fs:
            if not f.endswith('.hpp'):
                continue
            rel = os.path.relpath(os.path.join(dp, f), root).replace(os.sep, '/')
            for qual, rtti_name, size, fields in parse(read(sdk_root, rel) or ''):
                out[rtti_name] = dict(header=rel, cpp=qual, size=size, fields=fields)
    return out


def rtti(args):
    """The SDK against a game's RTTI dump (tools/rtti_dump): what the regenerated SDK has to change."""
    import json
    dump = json.load(open(args.dump, encoding='utf-8'))
    game = {c['name']: c for c in dump['classes']}
    sdk = collect_all(args.sdk)
    used = set(collect(args.sdk))
    rows = []
    for name, t in sorted(sdk.items()):
        g = game.get(name)
        if g is None:
            rows.append((name in used, 'GONE', name, t['header'], []))
            continue
        props = {p['name']: p for p in g['props']}
        notes = []
        if t['size'] is not None and t['size'] != g['size']:
            notes.append(f'size {t["size"]:#x} -> {g["size"]:#x}')
        for fname, off in t['fields']:
            p = props.get(fname)
            if p is None:
                continue        # hand-reversed or renamed: not RTTI's to judge
            if off is not None and off != p['offset']:
                notes.append(f'{fname} {off:#x} -> {p["offset"]:#x}')
        known = {f for f, _ in t['fields']}
        new = [p['name'] for p in g['props'] if p['name'] not in known]
        if new:
            notes.append('+' + ' +'.join(new[:6]) + (f' (+{len(new) - 6})' if len(new) > 6 else ''))
        if notes:
            rows.append((name in used, 'CHANGED', name, t['header'], notes))
    added = sorted(set(game) - set(sdk))
    lines = [f'# RED4ext.SDK vs RTTI dump {os.path.basename(args.dump)}', '',
             f'- SDK classes: {len(sdk)}; game classes: {len(game)}; game enums: {len(dump.get("enums", []))}',
             f'- changed: {sum(1 for r in rows if r[1] == "CHANGED")} '
             f'(port includes {sum(1 for r in rows if r[1] == "CHANGED" and r[0])}); '
             f'gone: {sum(1 for r in rows if r[1] == "GONE")}; classes only in the game: {len(added)}', '']
    for title, want in (('Classes the port includes', True), ('Other SDK classes', False)):
        lines += [f'## {title}', '']
        for u, kind, name, header, notes in rows:
            if u == want:
                lines.append(f'- {kind} `{name}` ({header}): ' + ', '.join(notes[:14]) + (' ...' if len(notes) > 14 else ''))
        lines.append('')
    lines += ['## Classes only in the game', '']
    lines += [f'- `{n}` size {game[n]["size"]:#x} parent {game[n]["parent"] or "-"}' for n in added]
    os.makedirs(os.path.dirname(args.out), exist_ok=True)
    with open(args.out, 'w', encoding='utf-8') as f:
        f.write('\n'.join(lines) + '\n')
    print('\n'.join(lines[:6]))
    print('report:', args.out)


def build(args):
    ensure_clone()
    wt = os.path.join(REPO, 'build', 'patchday', 'frameworks', 'RED4ext.SDK-next')
    if os.path.isdir(wt):
        subprocess.run(['git', '-C', wt, 'checkout', '--quiet', '--detach', args.ref], check=True)
    else:
        subprocess.run(['git', '-C', CLONE, 'worktree', 'add', '--quiet', '--detach', wt, args.ref], check=True)
    out = os.path.join(REPO, 'build', 'patchday', 'sdk-next-build')
    cmake = args.cmake
    r = subprocess.run([cmake, '-S', REPO, '-B', out, '-A', 'x64', f'-DCVR_RED4EXT_SDK_DIR={wt}'],
                       capture_output=True, text=True)
    if r.returncode:
        print(r.stdout[-3000:], r.stderr[-3000:])
        sys.exit('configure failed')
    r = subprocess.run([cmake, '--build', out, '--config', 'Release', '--target', 'cyberpunkvrport_stereo'],
                       capture_output=True, text=True)
    errors = [ln for ln in r.stdout.splitlines() if ': error ' in ln]
    for ln in errors[:60]:
        print(ln.strip()[:300])
    print(f'trial build against SDK {args.ref}: {"OK" if r.returncode == 0 else f"FAILED, {len(errors)} errors"}')
    sys.exit(r.returncode)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest='cmd', required=True)
    g = sub.add_parser('gen')
    g.add_argument('--sdk', default=SDK)
    g.add_argument('--check', action='store_true', help='only say whether the .inc matches the SDK and includes')
    d = sub.add_parser('diff')
    d.add_argument('--old', default=None)
    d.add_argument('--new', default='origin/master')
    b = sub.add_parser('build')
    b.add_argument('--ref', default='origin/master')
    b.add_argument('--cmake', default='cmake', help='the one on PATH: Visual Studio\'s bundled CMake')
    r = sub.add_parser('rtti')
    r.add_argument('--dump', required=True, help='rtti_dump-<exe timestamp>.json from tools/rtti_dump')
    r.add_argument('--sdk', default=SDK)
    r.add_argument('--out', default=os.path.join(REPO, 'build', 'patchday', 'sdk-rtti', 'REPORT.md'))
    args = ap.parse_args()
    {'gen': gen, 'diff': diff, 'build': build, 'rtti': rtti}[args.cmd](args)


if __name__ == '__main__':
    main()
