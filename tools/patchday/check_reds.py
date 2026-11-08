"""Compile the port's redscript against a game's script bundle, outside the game.

    py -3.13 -B tools/patchday/check_reds.py [--game <dir>] [--scc <dir with scc.exe>] [--all]

A patch changes the game's scripts; any @wrapMethod/@replaceMethod whose target changed signature,
any removed class or field, fails redscript's compile -- and a failed compile takes EVERY script mod
down with it at launch. This runs the same compiler (engine/tools/scc.exe + scc_lib.dll) in a sandbox
that looks like a game folder:

    build/patchday/redsbox-<label>/engine/tools/scc.exe, scc_lib.dll
    build/patchday/redsbox-<label>/r6/cache/final.redscripts      <- the game's own bundle
    build/patchday/redsbox-<label>/r6/scripts/                    <- mods/redscript/* from the repo,
                                                                     framework scripts (Codeware,
                                                                     ArchiveXL, TweakXL ...) and, with
                                                                     --all, the other installed mods

and reports the compiler's errors by file. Nothing in the game folder is written.
"""
import argparse
import os
import re
import shutil
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, '..', '..'))
GAME = r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077'


def base_bundle(game):
    cache = os.path.join(game, 'r6', 'cache')
    # Older redscript renamed the game's bundle to .bk and wrote its output in its place.
    for name in ('final.redscripts.bk', 'final.redscripts'):
        p = os.path.join(cache, name)
        if os.path.exists(p):
            return p
    raise FileNotFoundError(f'no final.redscripts under {cache}')


def build_sandbox(game, scc_dir, box, include_all, frameworks=()):
    shutil.rmtree(box, ignore_errors=True)
    tools = os.path.join(box, 'engine', 'tools')
    cache = os.path.join(box, 'r6', 'cache')
    scripts = os.path.join(box, 'r6', 'scripts')
    for d in (tools, cache, scripts):
        os.makedirs(d)
    for f in ('scc.exe', 'scc_lib.dll'):
        shutil.copy2(os.path.join(scc_dir, f), tools)
    shutil.copy2(base_bundle(game), os.path.join(cache, 'final.redscripts'))
    ours = os.path.join(REPO, 'mods', 'redscript')
    for name in sorted(os.listdir(ours)):
        src = os.path.join(ours, name)
        if os.path.isdir(src):
            shutil.copytree(src, os.path.join(scripts, name), ignore=shutil.ignore_patterns('*.log'))
    plugins = os.path.join(game, 'red4ext', 'plugins')
    # A staged release (frameworks.py fetch) stands in for the installed plugin of the same folder name.
    staged = {os.path.basename(os.path.normpath(d)): d for d in frameworks}
    names = set(os.listdir(plugins)) if os.path.isdir(plugins) else set()
    for name in sorted(names | set(staged)):
        # The installed port itself (one-folder layout: plugins/cyberpunkvrport/scripts) is what the repo's
        # mods/redscript above stands for; a second copy redefines every type.
        if name.lower() == 'cyberpunkvrport':
            continue
        for sub in ('Scripts', 'scripts'):
            src = os.path.join(staged.get(name, os.path.join(plugins, name)), sub)
            if os.path.isdir(src):
                shutil.copytree(src, os.path.join(scripts, '_framework', name), dirs_exist_ok=True)
    if include_all:
        installed = os.path.join(game, 'r6', 'scripts')
        for name in sorted(os.listdir(installed)):
            if name.startswith('CyberpunkVRPort_') or name in ('logs',) or name.startswith('__'):
                continue
            src = os.path.join(installed, name)
            dst = os.path.join(scripts, '_installed', name)
            if os.path.isdir(src):
                shutil.copytree(src, dst)
            elif name.endswith('.reds'):
                os.makedirs(os.path.dirname(dst), exist_ok=True)
                shutil.copy2(src, dst)
    return tools, scripts


VERDICTS = ('Compilation complete',                                        # redscript 0.5
            'Compilation successful', 'Completed with', 'Compilation failed')  # redscript 1.0
SUCCESS = ('Compilation complete', 'Compilation successful')


def cpu_seconds(p):
    import ctypes
    from ctypes import wintypes
    t = [wintypes.FILETIME() for _ in range(4)]
    if not ctypes.windll.kernel32.GetProcessTimes(int(p._handle), *[ctypes.byref(x) for x in t]):
        return None
    return sum((x.dwHighDateTime << 32 | x.dwLowDateTime) for x in t[2:]) / 1e7


def dialogs_of(pid):
    """(hwnd, title, text) of the visible top-level dialogs (#32770) the process owns."""
    import ctypes
    from ctypes import wintypes
    user32 = ctypes.windll.user32
    found = []

    def text_of(h):
        buf = ctypes.create_unicode_buffer(1024)
        user32.GetWindowTextW(h, buf, 1024)
        return buf.value

    def class_of(h):
        buf = ctypes.create_unicode_buffer(64)
        user32.GetClassNameW(h, buf, 64)
        return buf.value

    proc = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)

    def top(h, _):
        owner = wintypes.DWORD()
        user32.GetWindowThreadProcessId(h, ctypes.byref(owner))
        if owner.value == pid and user32.IsWindowVisible(h) and class_of(h) == '#32770':
            parts = []

            def child(c, _):
                if class_of(c) == 'Static' and text_of(c):
                    parts.append(text_of(c))
                return True
            user32.EnumChildWindows(h, proc(child), 0)
            found.append((h, text_of(h), '\n'.join(parts)))
        return True
    user32.EnumWindows(proc(top), 0)
    return found


def compile_box(tools, scripts, box, timeout=900, idle=120):
    """scc writes its log, then on errors opens a message box and waits for it -- which nobody here
    answers. So the log is watched: once it holds a verdict and stops growing, a still-running
    compiler is that dialog and is killed. redscript 1.0 also asks BEFORE compiling when a script is
    in an older syntax ("REDscript has detected scripts that require migration"): no verdict, no
    log line, no CPU. In the sandbox the scripts are copies, so that question is answered OK (the dialog's text is
    kept in the output); anything else still counts as stalled after `idle` seconds of neither."""
    log = os.path.join(box, 'r6', 'logs', 'redscript_rCURRENT.log')
    # Its console output goes to a file: redscript 1.0 prints every diagnostic there too, and a pipe
    # nobody drains until the end fills up and parks the compiler mid-run.
    console = open(os.path.join(box, 'scc_console.txt'), 'w', encoding='utf-8', errors='replace')
    p = subprocess.Popen([os.path.join(tools, 'scc.exe'), '-compile', scripts], cwd=tools,
                         stdin=subprocess.DEVNULL, stdout=console, stderr=subprocess.STDOUT)
    start = time.time()
    last = (None, time.time())
    cpu = (cpu_seconds(p), time.time())
    killed = stalled = False
    answered = []
    while p.poll() is None:
        time.sleep(1)
        text = _read(log)
        if not ('[ERROR' in text or any(v in text for v in VERDICTS)):
            for h, title, body in dialogs_of(p.pid):
                if 'migrat' in (title + body).lower() and h not in [a[0] for a in answered]:
                    import ctypes
                    ctypes.windll.user32.PostMessageW(h, 0x0111, 1, 0)   # WM_COMMAND, IDOK
                    answered.append((h, f'{title}: {body}'))
                    cpu = (cpu_seconds(p), time.time())
        if text != last[0]:
            last = (text, time.time())
        c = cpu_seconds(p)
        if c is not None and cpu[0] is not None and c - cpu[0] > 0.5:
            cpu = (c, time.time())
        verdict = '[ERROR' in text or any(v in text for v in VERDICTS)
        quiet = min(time.time() - last[1], time.time() - cpu[1])
        if (verdict and time.time() - last[1] > 5) or time.time() - start > timeout or (not verdict and quiet > idle):
            stalled = not verdict
            p.kill()
            killed = True
            break
    p.wait()
    console.close()
    out = _read(console.name)
    out += ''.join(f'\nANSWERED OK: {a[1]}' for a in answered)
    return (p.returncode if not killed else -1), out + ('\nSTALLED' if stalled else ''), _read(log)


def _read(path):
    try:
        with open(path, encoding='utf-8', errors='replace') as f:
            return f.read()
    except OSError:
        return ''


def summarize(log_text):
    errors, warnings = [], []
    block = None
    for line in log_text.splitlines():
        m = re.match(r'\[(ERROR|WARN)[^\]]*\]\s*(.*)', line)
        if m:
            block = [m.group(2)]
            (errors if m.group(1) == 'ERROR' else warnings).append(block)
        elif block is not None and not line.startswith('['):
            # The source line, the caret line and the message follow the header.
            if line.strip():
                block.append(line.rstrip())
        else:
            block = None
    return errors, warnings


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--game', default=GAME, help='game dir (its r6/cache bundle and framework scripts)')
    ap.add_argument('--scc', default=None, help='dir with scc.exe + scc_lib.dll (default: <game>/engine/tools)')
    ap.add_argument('--all', action='store_true', help='also compile the other installed script mods')
    ap.add_argument('--label', default=None)
    ap.add_argument('--framework', action='append', default=[],
                    help='a staged plugin folder (.../files/red4ext/plugins/Codeware) used instead of the installed one')
    ap.add_argument('--inject-failure', action='store_true', help='TEST: add a wrapper of a missing method')
    args = ap.parse_args()
    scc = args.scc or os.path.join(args.game, 'engine', 'tools')
    label = args.label or ('all' if args.all else 'port')
    box = os.path.join(REPO, 'build', 'patchday', f'redsbox-{label}')
    tools, scripts = build_sandbox(args.game, scc, box, args.all, args.framework)
    for d in args.framework:
        print('framework scripts from', d)
    if args.inject_failure:
        # What a patch does to a wrapper: its target no longer exists. Proves the errors are read.
        with open(os.path.join(scripts, 'CyberpunkVRPort_Selftest.reds'), 'w', encoding='utf-8') as f:
            f.write('@wrapMethod(PlayerPuppet)\nprotected cb func OnVRPortSelftestGone() -> Bool {\n'
                    '  return wrappedMethod();\n}\n')
    code, out, log = compile_box(tools, scripts, box)
    errors, warnings = summarize(log)
    ok = any(v in log for v in SUCCESS) and not errors
    print(f'bundle: {base_bundle(args.game)}')
    print(f'compiler exit {code}; {len(errors)} errors, {len(warnings)} warnings')
    for line in out.splitlines():
        if line.startswith('ANSWERED OK: '):
            print('dialog', line[len('ANSWERED '):].replace('\n', ' / ')[:300])
    if 'STALLED' in out:
        print('STALLED: no log line and no CPU for 2 minutes without a verdict (a dialog nobody answers?)')
    # redscript 1.0 rewrites scripts of the older syntax in place, then compiles again. In the sandbox that
    # touches copies only; in the game it would rewrite the installed files (and say so to the player).
    for m in re.finditer(r"Applied (\d+) migration changes to '([^']+)'", log):
        rel = os.path.relpath(m.group(2), scripts)
        print(f"MIGRATED {rel} ({m.group(1)} changes){'  <- the port: bring the source to the new syntax' if rel.startswith('CyberpunkVRPort_') else ''}")
    for e in errors[:60]:
        print('ERROR', '\n      '.join(e))
    for w in warnings[:20]:
        print('WARN ', w[0][:200])
    print('log:', os.path.join(box, 'r6', 'logs', 'redscript_rCURRENT.log'))
    print('REDSCRIPT', 'OK' if ok else 'FAILED')
    sys.exit(0 if ok else 1)


if __name__ == '__main__':
    main()
