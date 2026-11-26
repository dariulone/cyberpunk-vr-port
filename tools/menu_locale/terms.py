"""How the game itself says a term in every language, to keep the menu's own sentences in its words: each English
term (or a key pattern with `key:`) against the 19 onscreens tables official.py reads.

    py -3.13 tools/menu_locale/terms.py <dir with <code>/onscreens.json.json> <term> [term ...]

Per term: the game keys whose English is the term (ignoring case), then per language the most common translation
with its count. UI-* keys weigh more, as in official.py.
"""
import re
import sys
from collections import Counter, defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from official import NOT_A_TERM, entries, key_of, norm  # noqa: E402


def text(x):
    return x.get('maleVariant') or x.get('femaleVariant') or ''


def main():
    sys.stdout.reconfigure(encoding='utf-8')
    src = Path(sys.argv[1])
    terms = sys.argv[2:]
    en = {key_of(x): text(x) for x in entries(src / 'en-us' / 'onscreens.json.json')}
    keys = defaultdict(list)
    for t in terms:
        if t.startswith('key:'):
            pat = re.compile(t[4:])
            keys[t] = [k for k in en if pat.search(k)]
        else:
            keys[t] = [k for k, v in en.items() if norm(v) == norm(t) and not NOT_A_TERM.search(k)]
    langs = {}
    for d in sorted(p for p in src.iterdir() if p.is_dir() and p.name != 'en-us'):
        langs[d.name] = {key_of(x): text(x) for x in entries(d / 'onscreens.json.json')}
    for t in terms:
        ks = keys[t]
        print('== %s  (%d keys: %s)' % (t, len(ks), ', '.join(ks[:4])))
        if t.startswith('key:'):
            for k in ks[:12]:
                print('   %s = %s' % (k, en[k]))
                print('      ' + ' | '.join('%s:%s' % (c, tab.get(k, '')) for c, tab in langs.items()))
            continue
        for c, tab in langs.items():
            votes = Counter()
            for k in ks:
                v = tab.get(k, '').strip()
                if v:
                    votes[v] += 10 if k.startswith('UI-') else 1
            print('   %s: %s' % (c, '; '.join('%s (%d)' % kv for kv in votes.most_common(3))))


if __name__ == '__main__':
    main()
