"""The VR menu's own UI strings, for translation: every English text the menu draws, with the Russian written beside
it and where it is used, into tools/menu_locale/strings.json.

    py -3.13 tools/menu_locale/extract.py

Sources (mods/cet/CyberpunkVRPort_Menu/modules):
  * RU/EN literal pairs side by side -- tr(kit,"ru","en"), action(...,"ru","en",...), L.tr("ru","en"), and the tables
    {"key","RU","EN"} / {"RU","EN"};
  * the rest of a line's Russian literals against its English ones, in order -- the ternaries
    `cond and "RU1" or "RU2", cond and "EN1" or "EN2"`, also split over `local ru=` / `local en=` lines;
  * L.t("en") literals (texts put together around a number);
  * menu.lua's TEXT and gameplay.lua's LABELS (en/ru tables by key) and settings.lua's PAGES / PAGES_RU;
  * the plural nouns of L.count, as `#<English singular>` entries.
"""
import json
import re
from collections import OrderedDict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MOD = ROOT / 'mods' / 'cet' / 'CyberpunkVRPort_Menu' / 'modules'
OUT = Path(__file__).resolve().parent / 'strings.json'
CYR = re.compile('[Ѐ-ӿ]')
LIT = r'"((?:[^"\\]|\\.)*)"'
PAIR = re.compile(LIT + r'\s*,\s*' + LIT)
ANY = re.compile(LIT)
LT = re.compile(r'\bL\.t\(' + LIT + r'\)')
COUNT = re.compile(r'(?:counted\(kit,[^,]+|L\.count\([^,]+),' + ','.join([LIT] * 5))
# literals that are keys, ids or format pieces, never shown as words
NOT_TEXT = re.compile(r'^([a-z][a-z0-9]*[A-Z][A-Za-z0-9]*:?|[a-z_]+:?|%[^ ]*|[^A-Za-z]*)$')
# a field of a label table (TEXT / LABELS / PAGES_RU), read by key below rather than by line
TABLE_LINE = re.compile(r'\b\w+\s*=\s*"[^"]*[Ѐ-ӿ]|PAGES_RU')
# a line that ends on a Russian literal and a comma, its English partner on the next line
SPLIT_RU = re.compile(r'"[^"]*[Ѐ-ӿ][^"]*"\s*,\s*$')


def unescape(s):
    return s.encode('utf-8').decode('unicode_escape').encode('latin-1').decode('utf-8') if '\\' in s else s


strings = OrderedDict()


def add(en, ru, where, note=None):
    en, ru = unescape(en), (unescape(ru) if ru is not None else None)
    if not re.search('[A-Za-z]', en):
        return
    e = strings.setdefault(en, {'en': en, 'ru': ru, 'where': []})
    if e['ru'] is None and ru is not None:
        e['ru'] = ru
    if where not in e['where']:
        e['where'].append(where)
    if note:
        e['note'] = note


def table(src, name):
    """{lang = {key = "value", ...}} blocks inside `local NAME = {`."""
    out = {}
    start = src.index('local %s = {' % name)
    for lang in ('en', 'ru'):
        m = re.compile(r'\b%s\s*=\s*\{' % lang).search(src, start)
        depth, i = 1, m.end()
        while depth:
            depth += {'{': 1, '}': -1}.get(src[i], 0)
            i += 1
        out[lang] = dict(re.findall(r'(\w+)\s*=\s*' + LIT, src[m.end():i - 1]))
    return out


for f in sorted(MOD.glob('*.lua')):
    if f.parent.name == 'lang':
        continue
    raw = f.read_text(encoding='utf-8').splitlines()
    # a pair split over two lines, tr("RU",<newline> "EN"): the English line joined on to the Russian one
    lines, i = [], 0
    while i < len(raw):
        first, line = i, raw[i]
        while i + 1 < len(raw) and SPLIT_RU.search(line.split('--')[0]) and re.match(r'\s*"', raw[i + 1]):
            i += 1
            line = line + ' ' + raw[i].strip()
        lines.append((first + 1, line))
        i += 1
    pending_ru = None
    for n, line in lines:
        code = line.split('--')[0] if not line.lstrip().startswith('--') else ''
        if not code.strip() or TABLE_LINE.search(code):
            continue
        where = '%s:%d' % (f.name, n)
        used = set()
        for m in COUNT.finditer(code):
            one, few, many, en1, en2 = m.groups()
            add('#' + en1, '|'.join((one, few, many)), where, 'plural: forms of "%s" / "%s" after a number' % (en1, en2))
            used.update((m.start(i) for i in range(1, 6)))
        for m in PAIR.finditer(code):
            ru, en = m.group(1), m.group(2)
            if m.start(1) in used or m.start(2) in used:
                continue
            if CYR.search(ru) and not CYR.search(en):      # a Russian partner makes it text, even 'km'
                add(en, ru, where)
                used.update((m.start(1), m.start(2)))
        for m in LT.finditer(code):
            add(m.group(1), None, where)
            used.add(m.start(1))
        rest = [(m.start(1), m.group(1)) for m in ANY.finditer(code) if m.start(1) not in used]
        ru_rest = [t for _, t in rest if CYR.search(t)]
        en_rest = [t for _, t in rest if not CYR.search(t) and not NOT_TEXT.match(t) and re.search('[A-Za-z]{2}', t)]
        if re.match(r'\s*local ru\s*=', code) and ru_rest and not en_rest:
            pending_ru = (ru_rest, where)
            continue
        if pending_ru and re.match(r'\s*local en\s*=', code):
            ru_rest, where = pending_ru[0], pending_ru[1] + '-' + str(n)
        pending_ru = None
        if ru_rest and len(ru_rest) == len(en_rest):
            for ru, en in zip(ru_rest, en_rest):
                add(en.strip(), ru.strip(), where)
        elif ru_rest:
            print('!! unpaired Russian at %s: %s | English: %s' % (where, ru_rest, en_rest))

for fname, name in (('menu.lua', 'TEXT'), ('gameplay.lua', 'LABELS')):
    t = table((MOD / fname).read_text(encoding='utf-8'), name)
    for key, en in t['en'].items():
        if key != 'lang':
            add(en, t['ru'].get(key), '%s:%s.%s' % (fname, name, key))

# the settings tabs (settings_data.lua's D.PAGES / PAGES_RU since the settings data moved out of settings.lua)
src = (MOD / 'settings_data.lua').read_text(encoding='utf-8')
pages = re.search(r'(?:local |D\.)PAGES = \{([^}]*)\}', src).group(1)
pages_ru = re.search(r'local PAGES_RU = \{([^}]*)\}', src).group(1)
for en, ru in zip(ANY.findall(pages), ANY.findall(pages_ru)):
    add(en, ru, 'settings_data.lua:PAGES', 'settings tab: keep it short, about 12 letters at most')

# put together around a price at its call site (cyberware_view.lua)
if 'BUY AND INSTALL' in strings and strings['BUY AND INSTALL']['ru'] is None:
    strings['BUY AND INSTALL']['ru'] = 'КУПИТЬ И УСТАНОВИТЬ'

OUT.write_text(json.dumps(list(strings.values()), indent=1, ensure_ascii=False), encoding='utf-8')
print(len(strings), 'strings ->', OUT)
