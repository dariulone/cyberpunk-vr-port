"""Run PowerShell on the second PC (HOME-PC) over ssh without quoting trouble: the script goes as -EncodedCommand.

    py -3.13 -B tools/remote/home_pc.py run "Get-ChildItem X:\\"          # a command
    py -3.13 -B tools/remote/home_pc.py file script.ps1                  # a script file of ours
    py -3.13 -B tools/remote/home_pc.py put LOCAL REMOTE / get REMOTE LOCAL  (scp)

HOST from CVR_HOME_PC (default lone33@192.168.0.11). The VPN adapter here captures 192.168.0.x: a host route through
the LAN gateway must exist (route add 192.168.0.11 mask 255.255.255.255 192.168.2.1 metric 1).
"""
import base64
import os
import subprocess
import sys

HOST = os.environ.get('CVR_HOME_PC', 'lone33@192.168.0.11')
SSH = ['ssh', '-o', 'BatchMode=yes', '-o', 'ConnectTimeout=10', '-o', 'LogLevel=ERROR']


def run(script, timeout=600, check=False):
    encoded = base64.b64encode(("$ProgressPreference='SilentlyContinue'\n" + script).encode('utf-16-le')).decode()
    r = subprocess.run(SSH + [HOST, f'powershell -NoProfile -NonInteractive -EncodedCommand {encoded}'],
                       capture_output=True, text=True, errors='replace', timeout=timeout)
    if check and r.returncode:
        raise RuntimeError(f'remote script failed ({r.returncode}): {r.stderr.strip()[-500:]}')
    return r


def task(name, command, log, wait=False, timeout=600):
    """`command` (a cmd.exe line) in the logged-on user's interactive session on the second PC, through a one-shot
    scheduled task (home_task.ps1): a process started from ssh has no desktop, the game needs one."""
    body = open(os.path.join(os.path.dirname(__file__), 'home_task.ps1'), encoding='utf-8').read()
    body = body[body.index('\nparam(') + 1:].split('\n', 1)[1]   # after the comment lines and the param() line
    q = lambda s: "'" + s.replace("'", "''") + "'"
    script = f"$Name = {q(name)}\n$Command = {q(command)}\n$Log = {q(log)}\n$Wait = ${'true' if wait else 'false'}\n" + body
    return run(script, timeout=timeout)


def scp(src, dst, timeout=3600):
    return subprocess.run(['scp', '-o', 'BatchMode=yes', '-o', 'LogLevel=ERROR', src, dst], timeout=timeout).returncode


def main():
    cmd = sys.argv[1]
    if cmd == 'run':
        r = run(' '.join(sys.argv[2:]))
    elif cmd == 'file':
        r = run(open(sys.argv[2], encoding='utf-8').read())
    elif cmd == 'task':   # task NAME LOG [--wait] COMMAND...
        rest = sys.argv[4:]
        wait = bool(rest) and rest[0] == '--wait'
        r = task(sys.argv[2], ' '.join(rest[1:] if wait else rest), sys.argv[3], wait)
    elif cmd == 'put':
        sys.exit(scp(sys.argv[2], f'{HOST}:{sys.argv[3]}'))
    elif cmd == 'get':
        sys.exit(scp(f'{HOST}:{sys.argv[2]}', sys.argv[3]))
    else:
        sys.exit(__doc__)
    sys.stdout.write(r.stdout)
    sys.stderr.write(r.stderr)
    sys.exit(r.returncode)


if __name__ == '__main__':
    main()
