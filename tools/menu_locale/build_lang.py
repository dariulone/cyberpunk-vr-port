"""The VR menu's language tables, checked on the way:

  * the menu's own words: tools/menu_locale/tr/<code>.json (English text -> translation, as extract.py lists it in
    strings.json) -> mods/cet/CyberpunkVRPort_Menu/modules/lang/<code>.lua;
  * the settings pages' words (the plugin's ImGui panels): tr_settings/<code>.json (as extract_settings.py lists
    them in settings_strings.json; Russian included, the panels being English) -> modules/lang/settings/<code>.lua.

    py -3.13 tools/menu_locale/build_lang.py [code ...]      every language of either set if none given

Checks: every string translated (missing ones are listed and stay English in game), no keys the string list does
not have, the same leading/trailing spaces and line count as the English (Japanese, Chinese and Korean write a unit
against its number, "12時間30分", so their spaces may differ), a list of forms for each `#noun` plural entry, where
{n} may say where the number goes, and for a settings FORMAT the same printf conversions in the same order.
"""
import json
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
LANG = ROOT / 'mods' / 'cet' / 'CyberpunkVRPort_Menu' / 'modules' / 'lang'
NAMES = {'pl-pl': 'Polish', 'es-es': 'Spanish (Spain)', 'fr-fr': 'French', 'it-it': 'Italian', 'de-de': 'German',
         'es-mx': 'Spanish (Latin America)', 'kr-kr': 'Korean', 'zh-cn': 'Chinese (Simplified)',
         'pt-br': 'Portuguese (Brazil)', 'jp-jp': 'Japanese', 'zh-tw': 'Chinese (Traditional)', 'ar-ar': 'Arabic',
         'cz-cz': 'Czech', 'hu-hu': 'Hungarian', 'tr-tr': 'Turkish', 'th-th': 'Thai', 'ua-ua': 'Ukrainian',
         'ru-ru': 'Russian'}
UNIT_AGAINST_NUMBER = {'jp-jp', 'zh-cn', 'zh-tw', 'kr-kr'}
CONV = re.compile(r'%%|%[-+ #0]*\d*(?:\.\d+)?(?:hh|h|ll|l|L|z|j|t)?[diouxXeEfgGcsp]')
# (string list, translations, output, what the tables hold)
SETS = [
    (HERE / 'strings.json', HERE / 'tr', LANG, "the VR menu's own words"),
    (HERE / 'settings_strings.json', HERE / 'tr_settings', LANG / 'settings', "the VR settings pages' words"),
]


def lua(s):
    return '"' + s.replace('\\', '\\\\').replace('"', '\\"').replace('\n', '\\n') + '"'


def edges(s):
    return len(s) - len(s.lstrip(' ')), len(s) - len(s.rstrip(' '))


def conversions(s):
    return [m.group(0) for m in CONV.finditer(s) if m.group(0) != '%%']


def build(code, strings, trdir, outdir, what):
    tr = json.loads((trdir / f'{code}.json').read_text(encoding='utf-8'))
    problems, lines = [], []
    for e in strings:
        en = e['en']
        t = tr.get(en)
        if t is None:
            problems.append('missing: %r' % en)
            continue
        if en.startswith('#'):
            if not (isinstance(t, list) and t and all(isinstance(x, str) and x for x in t)):
                problems.append('plural %r needs a list of forms' % en)
                continue
            lines.append('  [%s] = { %s },' % (lua(en), ', '.join(lua(x) for x in t)))
            continue
        if not isinstance(t, str) or not t.strip():
            problems.append('empty: %r' % en)
            continue
        if edges(t) != edges(en) and code not in UNIT_AGAINST_NUMBER:
            problems.append('spaces at the ends differ: %r -> %r' % (en, t))
        if t.count('\n') != en.count('\n'):
            problems.append('line count differs: %r' % en)
        if e.get('format') and conversions(t) != conversions(en):
            problems.append('conversions differ: %r -> %r' % (en, t))
        lines.append('  [%s] = %s,' % (lua(en), lua(t)))
    known = {e['en'] for e in strings}
    problems += ['not in the string list: %r' % k for k in tr if k not in known]
    outdir.mkdir(parents=True, exist_ok=True)
    rel = trdir.relative_to(HERE).as_posix()
    head = ('-- %s (%s): %s, keyed by the English (modules/locale.lua).\n'
            '-- Written by tools/menu_locale/build_lang.py from tools/menu_locale/%s/%s.json -- edit that, not this.\n'
            % (NAMES.get(code, code), code, what, rel, code))
    (outdir / f'{code}.lua').write_bytes((head + 'return {\n' + '\n'.join(lines) + '\n}\n').encode('utf-8'))
    print('%s %s: %d of %d translated%s' % (rel, code, len(lines), len(strings),
                                            '' if not problems else ', %d problem(s)' % len(problems)))
    for p in problems:
        print('   ', p)
    return not problems


def main():
    ok = True
    for listing, trdir, outdir, what in SETS:
        strings = json.loads(listing.read_text(encoding='utf-8'))
        have = sorted(p.stem for p in trdir.glob('*.json')) if trdir.exists() else []
        codes = [c for c in sys.argv[1:] if c in have] if sys.argv[1:] else have
        ok = all([build(c, strings, trdir, outdir, what) for c in codes]) and ok
    return 0 if ok else 1


if __name__ == '__main__':
    raise SystemExit(main())
