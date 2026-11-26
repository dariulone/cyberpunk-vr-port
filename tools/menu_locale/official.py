"""The game's own translation of every menu string that the game itself has word for word in English: strings.json
against base\\localization\\<code>\\onscreens\\onscreens.json of all 19 languages -> official.json, so the menu says
"Street Cred", "Ripperdoc" or "Save Game" the way the game does in each language.

    py -3.13 tools/menu_locale/official.py <dir with <code>/onscreens.json.json>

The string tables come out of lang_<x>_text.archive (cp77tools unbundle --regex 'onscreens.onscreens\\.json$', then
cp77tools convert serialize per language). A match ignores case and the menu's own framing (a trailing colon, "…",
"+", arrows, brackets), which is put back around the game's text, and the menu's capitals are kept. Several game
keys with one English text can translate it differently: UI-* keys win, then the most common translation. A text
that depends on V's gender takes its masculine form.
"""
import json
import re
import sys
from collections import Counter, defaultdict
from pathlib import Path

HERE = Path(__file__).resolve().parent
FRAME = re.compile(r'^([\s<\[←]*)(.*?)([\s:+\-…\]→]*)$', re.S)


def core(s):
    m = FRAME.match(s)
    return m.group(1), m.group(2), m.group(3)


def norm(s):
    return re.sub(r'\s+', ' ', s.strip()).lower()


def entries(path):
    d = json.loads(path.read_text(encoding='utf-8'))
    return d['Data']['RootChunk']['root']['Data']['entries']


def key_of(x):
    """An entry's id: its secondary key, or its primary key when it has none (several thousand have none)."""
    return x.get('secondaryKey') or '#' + str(x.get('primaryKey'))


# NPC names share English words with the menu (Max, Min, Day): never a translation of them.
NOT_A_TERM = re.compile(r'EthnicNames|-Names-|DisplayName-Npc')
# The game's own menu texts: their translation is taken whatever our Russian says.
MENU_KEY = re.compile(r'^UI-(Labels|Menus|ResourceExports|Filters|Phone|UserActions|PanelNames|Crafting|Quickhacks|Modio)-')


def same(a, b):
    f = lambda s: re.sub(r'[\s.:…!]+$', '', re.sub(r'\s+', ' ', s.strip())).lower()
    return f(a) == f(b)


def upper(s, code):
    if code == 'tr-tr':
        s = s.replace('i', 'İ')
    return s.upper()


def main():
    src = Path(sys.argv[1])
    strings = json.loads((HERE / 'strings.json').read_text(encoding='utf-8'))
    wanted = {}
    for e in strings:
        if e['en'].startswith('#'):
            continue
        pre, body, post = core(e['en'])
        if re.search('[A-Za-z]{2}', body):
            wanted.setdefault(norm(body), []).append(e['en'])
    keys = defaultdict(list)          # normalised English body -> game keys
    for x in entries(src / 'en-us' / 'onscreens.json.json'):
        k = key_of(x)
        if NOT_A_TERM.search(k):
            continue
        for v in (x.get('femaleVariant'), x.get('maleVariant')):
            if v and norm(v) in wanted:
                keys[norm(v)].append(k)
    ours_ru = {e['en']: e['ru'] or '' for e in strings}
    picked = {}                       # normalised English -> (text per language, key it came from)
    for code_dir in sorted(p for p in src.iterdir() if p.is_dir() and p.name != 'en-us'):
        code = code_dir.name
        by = {key_of(x): (x.get('maleVariant') or x.get('femaleVariant') or '')
              for x in entries(code_dir / 'onscreens.json.json')}
        for n, ks in keys.items():
            votes, origin = Counter(), {}
            for k in sorted(set(ks)):
                t = by.get(k, '').strip()
                if t and '{' not in t and '<' not in t:
                    w = 100 if MENU_KEY.match(k) else 10 if k.startswith('UI-') else 1
                    votes[t] += w
                    if t not in origin or w > 1:
                        origin[t] = k
            if votes:
                text = votes.most_common(1)[0][0]
                picked.setdefault(n, {})[code] = (text, origin[text])
        print(code, 'done', flush=True)
    out = {}
    for n, per in picked.items():
        ru_text, ru_key = per.get('ru-ru', ('', ''))
        for en in wanted[n]:
            pre, body, post = core(en)
            # the sense is right if the game's key is one of its menu texts, or if its Russian is ours
            if not (MENU_KEY.match(ru_key) or same(core(ours_ru[en])[1], ru_text)):
                continue
            rec = out.setdefault(en, {'key': ru_key, 'tr': {}})
            for code, (text, _) in per.items():
                t = upper(text, code) if body.isupper() else text
                rec['tr'][code] = pre + t + post
    (HERE / 'official.json').write_text(json.dumps(out, indent=1, ensure_ascii=False), encoding='utf-8')
    print(len(out), 'of', len(strings), 'strings have the game\'s own translation ->', HERE / 'official.json')


if __name__ == '__main__':
    main()
