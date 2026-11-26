"""Lua in the running game through CET's CETBridge mod (file transport: command.json -> response.json in the mod's
folder), on this PC or on the second one (tools/remote/home_pc.py):

    py -3.13 -B tools/mp/cet.py [--home] exec "print(Game.NightCoopSdkStatus())"
    py -3.13 -B tools/mp/cet.py [--home] eval "Game.GetPlayer():GetWorldPosition().x"
"""
import argparse
import base64
import json
import sys
import time
import uuid
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / 'remote'))
import home_pc  # noqa: E402

BRIDGE = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077\bin\x64\plugins\cyber_engine_tweaks\mods\CETBridge')


def _request(kind, text):
    rid = uuid.uuid4().hex[:12]
    return rid, json.dumps({'id': rid, 'type': kind, 'code' if kind == 'exec' else 'expr': text})


def local(kind, text, timeout=15.0):
    rid, body = _request(kind, text)
    res = BRIDGE / 'response.json'
    tmp = BRIDGE / 'command.json.tmp'
    # a stale answer left by another client keeps the bridge from renaming its new one into place
    res.unlink(missing_ok=True)
    (BRIDGE / 'response.json.tmp').unlink(missing_ok=True)
    tmp.write_text(body, encoding='utf-8')
    tmp.replace(BRIDGE / 'command.json')
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            r = json.loads(res.read_text(encoding='utf-8'))
            if r.get('id') == rid:
                res.unlink(missing_ok=True)
                return r
        except (OSError, ValueError):
            pass
        time.sleep(0.05)
    raise TimeoutError('no answer from CETBridge (is the game running with CET?)')


def home(kind, text, timeout=15.0):
    rid, body = _request(kind, text)
    b64 = base64.b64encode(body.encode('utf-8')).decode()
    script = f"""
$d = '{BRIDGE}'
[IO.File]::WriteAllText("$d\\command.json.tmp", [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{b64}')))
Move-Item -Force "$d\\command.json.tmp" "$d\\command.json"
$end = (Get-Date).AddSeconds({timeout})
while ((Get-Date) -lt $end) {{
  if (Test-Path "$d\\response.json") {{
    try {{ $t = [IO.File]::ReadAllText("$d\\response.json"); $j = $t | ConvertFrom-Json; if ($j.id -eq '{rid}') {{ Remove-Item "$d\\response.json"; $t; exit 0 }} }} catch {{}}
  }}
  Start-Sleep -Milliseconds 50
}}
'TIMEOUT'
"""
    r = home_pc.run(script, timeout=timeout + 30)
    out = r.stdout.strip()
    if out.endswith('TIMEOUT') or not out:
        raise TimeoutError(f'no answer from the second PC\'s CETBridge: {r.stderr.strip()[-300:]}')
    return json.loads(out[out.index('{'):])


def call(kind, text, on_home=False, timeout=15.0):
    return (home if on_home else local)(kind, text, timeout)


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument('--home', action='store_true', help='the second PC')
    p.add_argument('kind', choices=['exec', 'eval'])
    p.add_argument('text')
    p.add_argument('--timeout', type=float, default=15.0)
    a = p.parse_args()
    print(json.dumps(call(a.kind, a.text, a.home, a.timeout), indent=1, ensure_ascii=False))


if __name__ == '__main__':
    main()
