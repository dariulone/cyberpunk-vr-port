"""The VR settings pages' words, for translation: every English text the plugin's ImGui panels hand the VR menu
(labels, tooltips, texts, combo options, binding rows, slider units), into tools/menu_locale/settings_strings.json.

    py -3.13 tools/menu_locale/extract_settings.py

The panels are C++ (src/Overlay/OverlayPanels.cpp, VrWidgets.cpp, the OVERLAY page of VrStyle.cpp); the VR menu
gets what they draw through the UiRec recorder (include/Overlay/UiRec.hpp) and translates it on the Lua side
(modules/locale.lua L.setting), so the desktop ImGui panel stays English. Every string literal of those panels is
taken -- adjacent literals joined the way C++ joins them, "##id" suffixes cut, "\\0"-separated combo lists split --
except identifiers, file names and the arguments of calls that never reach the menu (Log, ImGui ids).

A literal with a printf conversion ("%.1f deg", "GPU load %.1f%%") is a FORMAT: the panel fills it in, so the menu
matches the finished text against it and puts the numbers into the translated format, in the same order. A
format's %% stands for one %.
"""
import json
import re
from collections import OrderedDict
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
OUT = HERE / 'settings_strings.json'
# file, and the part of it the VR menu sees (from the first marker up to the second), or None for all of it
SOURCES = [
    ('src/Overlay/OverlayPanels.cpp', None),
    ('src/Overlay/VrWidgets.cpp', None),
    ('src/Overlay/VrStyle.cpp', ('bool DrawOverlayPage', 'void ApplyVrStyle')),
]
SKIP_CALLS = {'Log', 'OutputDebugStringA', 'BeginTabBar', 'PushID', 'TableSetupColumn', 'BeginTable', 'Begin',
              'BeginChild', 'GetProcAddress', 'GetModuleHandleA'}
CONV = re.compile(r'%[-+ #0]*\d*(?:\.\d+)?(?:hh|h|ll|l|L|z|j|t|I64)?[diouxXeEfgGcsp]')
# not words: snake_case and camelCase names, paths and file names (single tokens, no spaces)
IDENT = re.compile(r'^(?:\w*_\w*|[a-z]+[A-Z]\w*|[a-z]+(?:-[a-z]+)+|[\w.-]*[/\\][\w./\\-]*|[\w-]+\.(?:txt|ini|dll|log|json|exe|h|hpp|cpp))$')
# texts the panels show that are written elsewhere: (file, regex whose group 1 is the literal)
EXTRA = [
    ('include/Runtimes/HudLayout.hpp', r'\{"[^"]*","([^"]*)","[^"]*"\}'),        # HUD element labels
    ('include/Framegen/Framegen.hpp', r'char status\[\d+\]="([^"]*)"'),           # framegen status
    ('src/Framegen/Presenter.cpp', r'Status\("([^"]*)"'),
    ('src/Framegen/Presenter.cpp', r'Status\(settings\.enabled \? "([^"]*)" : "[^"]*"'),
    ('src/Framegen/Presenter.cpp', r'Status\(settings\.enabled \? "[^"]*" : "([^"]*)"'),
    ('src/Framegen/Generator.cpp', r'Status\("([^"]*)"'),
    ('src/Framegen/DlssFrameGen.cpp', r'reason="([^"]*)"'),                       # DLSS-G reasons
    ('src/Framegen/DlssFrameGen.cpp', r'needsDriver\?"([^"]*)"'),
    ('src/Framegen/DlssFrameGen.cpp', r'needsDriver\?"[^"]*":"([^"]*)"'),
    ('src/Framegen/DlssFrameGen.cpp', r'snprintf\(text,sizeof\(text\),"([^"]*)"'),
]
ESC = {'n': '\n', 't': '\t', '0': '\0', '\\': '\\', '"': '"', "'": "'"}


def tokens(src):
    """(raw literal, line, call, start, end) for each string literal outside comments; call = the function whose
    argument list it sits in."""
    i, n, line = 0, len(src), 1
    stack, word = [], ''
    while i < n:
        c = src[i]
        if c == '\n':
            line += 1
        if src.startswith('//', i):
            j = src.find('\n', i)
            i = n if j < 0 else j
            continue
        if src.startswith('/*', i):
            j = src.find('*/', i)
            line += src.count('\n', i, j)
            i = j + 2
            continue
        if c == "'":
            j = i + 1
            while src[j] != "'":
                j += 2 if src[j] == '\\' else 1
            i = j + 1
            continue
        if c == '"':
            if src[i - 1] == 'R':
                raise SystemExit('raw string literal at line %d' % line)
            j = i + 1
            while src[j] != '"':
                j += 2 if src[j] == '\\' else 1
            yield src[i + 1:j], line, (stack[-1] if stack else ''), i, j + 1
            i = j + 1
            word = ''
            continue
        if c.isalnum() or c == '_':
            word += c
        else:
            if c == '(':
                stack.append(word)
            elif c == ')' and stack:
                stack.pop()
            if not c.isspace():
                word = ''
        i += 1


def unescape(s):
    out, i = [], 0
    while i < len(s):
        if s[i] == '\\':
            out.append(ESC.get(s[i + 1], s[i + 1]))
            i += 2
        else:
            out.append(s[i])
            i += 1
    return ''.join(out)


strings = OrderedDict()


def add(text, where, call):
    if '##' in text:
        text = text.split('##')[0]
    if not re.search('[A-Za-z]', CONV.sub('', text)) or IDENT.match(text.strip()):
        return
    e = strings.setdefault(text, {'en': text, 'format': bool(CONV.search(text)) or '%%' in text,
                                  'where': [], 'calls': []})
    if where not in e['where']:
        e['where'].append(where)
    if call and call not in e['calls']:
        e['calls'].append(call)


def main():
    for path, part in SOURCES:
        src = (ROOT / path).read_text(encoding='utf-8')
        base = 0
        if part:
            a = src.index(part[0])
            base = src.count('\n', 0, a)
            src = src[a:src.index(part[1], a)]
        groups = []
        for raw, line, call, start, end in tokens(src):
            before = src[src.rfind('\n', 0, start) + 1:start].strip()
            if before.startswith('#') or before.endswith('extern'):
                continue                # #include "...", extern "C"
            if groups and src[groups[-1]['end']:start].strip() == '':
                groups[-1]['raw'] += raw
                groups[-1]['end'] = end
            else:
                groups.append({'raw': raw, 'line': line, 'call': call, 'end': end})
        for g in groups:
            if g['call'] in SKIP_CALLS:
                continue
            text = unescape(g['raw'])
            where = '%s:%d' % (Path(path).name, base + g['line'])
            if '\0' in text:            # a combo list "A\0B\0C\0"
                for option in text.split('\0'):
                    if option:
                        add(option, where, g['call'] + ' option')
            else:
                add(text, where, g['call'])
    for path, rx in EXTRA:
        src = (ROOT / path).read_text(encoding='utf-8')
        for m in re.finditer(rx, src):
            add(unescape(m.group(1)), '%s:%d' % (Path(path).name, src.count('\n', 0, m.start()) + 1), 'status')
    out = list(strings.values())
    OUT.write_text(json.dumps(out, ensure_ascii=False, indent=1), encoding='utf-8')
    formats = sum(1 for e in out if e['format'])
    print('%d strings (%d formats), %d characters -> %s' % (len(out), formats, sum(len(e['en']) for e in out), OUT))


if __name__ == '__main__':
    main()
