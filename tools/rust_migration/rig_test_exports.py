"""Generates rust/crates/cvr-render/src/rig_test_exports.rs: the exported state tools/vrik_tests' Rust variants need and do
not define themselves, copied verbatim from cvr-render's export modules (the plugin's own types and initial values).

    python tools/rust_migration/rig_test_exports.py <names.txt>

<names.txt>: one symbol per line -- the names the Rust variants' link reports unresolved without this module (the union
over the variants). Names the export modules do not define are left to tools/vrik_tests/stubs.cpp. The export modules
are not linked whole: the test programs define their own values for the rest of the exports.
"""
import sys
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[2]
SRC = ROOT / 'rust' / 'crates' / 'cvr-render' / 'src'
MODULES = ['input_exports', 'render_moved_exports', 'declared_moved_exports', 'used_moved_exports',
           'used_moved_exports_rest', 'typed_moved_exports']
ITEM = re.compile(r'#\[no_mangle\]\s*pub (?:static mut|static) ([A-Za-z_][A-Za-z0-9_]*)')


def items(text):
    """Each #[no_mangle] static with the doc and attribute lines right above it: (name, source)."""
    lines = text.split('\n')
    out = []
    i = 0
    while i < len(lines):
        if lines[i].strip() == '#[no_mangle]':
            start = i
            while start > 0 and (lines[start - 1].strip().startswith('///') or lines[start - 1].strip().startswith('#[')):
                start -= 1
            end = i + 1
            depth = 0
            while True:
                line = lines[end]
                depth += line.count('[') + line.count('(') + line.count('{') - line.count(']') - line.count(')') - line.count('}')
                if line.rstrip().endswith(';') and depth <= 0:
                    break
                end += 1
            source = '\n'.join(lines[start:end + 1])
            m = ITEM.search(source)
            if m:
                out.append((m.group(1), source))
            i = end + 1
        else:
            i += 1
    return out


def helpers(text):
    """typed_moved_exports' own consts and const fns (its items' initialisers use them)."""
    keep, block, depth = [], [], 0
    for line in text.split('\n'):
        s = line.strip()
        if not block and (s.startswith('const ') or s.startswith('pub(crate) const')):
            block = [line]
            depth = line.count('{') - line.count('}')
            if s.endswith(';') and depth == 0:
                keep.append('\n'.join(block))
                block = []
            continue
        if block:
            block.append(line)
            depth += line.count('{') - line.count('}')
            if depth <= 0 and (s.endswith('}') or s.endswith(';')):
                keep.append('\n'.join(block))
                block = []
    return keep


def main():
    wanted = set(Path(sys.argv[1]).read_text(encoding='utf-8-sig').split())
    parts, seen, helper_text = [], set(), []
    for module in MODULES:
        text = (SRC / (module + '.rs')).read_text(encoding='utf-8').replace('\r\n', '\n')
        chosen = [(n, s) for n, s in items(text) if n in wanted and n not in seen]
        if not chosen:
            continue
        if module == 'typed_moved_exports':
            helper_text = helpers(text)
        parts.append('// ---- from %s.rs' % module)
        for name, source in chosen:
            seen.add(name)
            parts.append(source)
    header = [
        '//! The exported state tools/vrik_tests\' Rust variants need and do not define themselves, copied verbatim from this',
        '//! crate\'s export modules (the plugin\'s own types and initial values) by tools/rust_migration/rig_test_exports.py:',
        '//! regenerate it after changing one of those definitions. Only the test build (feature rig-exports, without',
        '//! rust-build) compiles it; the plugin has the modules themselves.',
        '#![allow(non_upper_case_globals, dead_code, unused_imports)]',
        'use core::sync::atomic::{AtomicI32, AtomicI64, AtomicU32, AtomicU64, AtomicUsize};',
        '',
    ]
    out = SRC / 'rig_test_exports.rs'
    out.write_text('\n'.join(header + helper_text + [''] + parts) + '\n', encoding='utf-8', newline='\n')
    print('wrote', out, len(seen), 'items; not in the export modules:', ' '.join(sorted(wanted - seen)))


if __name__ == '__main__':
    main()
