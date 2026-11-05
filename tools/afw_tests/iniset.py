"""iniset.py KEY=VALUE ...: set (or append) keys of the port's vrport.ini, its CRLF line ends kept."""
import sys
from pathlib import Path
INI = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077\red4ext\plugins\cyberpunkvrport\config\vrport.ini')
text = INI.read_bytes().decode('utf-8')
nl = '\r\n' if '\r\n' in text else '\n'
lines = text.splitlines()
for arg in sys.argv[1:]:
    key, value = arg.split('=', 1)
    hit = False
    for i, l in enumerate(lines):
        if l.split('=', 1)[0].strip() == key:
            lines[i] = key + '=' + value
            hit = True
    if not hit:
        lines.append(key + '=' + value)
    print(key, '=', value, '(set)' if hit else '(appended)')
INI.write_bytes((nl.join(lines) + nl).encode('utf-8'))
