"""After a launch: what loaded, what hooked, what failed -- against the last good build's launch.

    py -3.13 -B tools/patchday/check_logs.py [--game <dir>]              # compare with the baseline
    py -3.13 -B tools/patchday/check_logs.py --save-baseline             # record this launch as good

Reads the newest of each log the port's start writes:
  red4ext/plugins/cyberpunkvrport/log/cyberpunkvrport.log          [build] verdict, every [hooks] / [detour] line, guard refusals
  red4ext/logs/red4ext-*.log           loader version, game version, plugins loaded / refused
  r6/logs/redscript_rCURRENT.log       script compile errors
  red4ext/plugins/TweakXL/TweakXL-*.log, ArchiveXL/ArchiveXL-*.log   errors and warnings
  bin/x64/plugins/cyber_engine_tweaks/cyber_engine_tweaks.log and mods/CyberpunkVRPort_*/*.log

The baseline (tools/patchday/data/log_baseline-2.31.json) holds the status of every named hook and
detour on the good build, so the report says exactly which ones a new build lost.
"""
import argparse
import glob
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
GAME = r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077'
BASELINE = os.path.join(HERE, 'data', 'log_baseline-2.31.json')
HOOK = re.compile(r'\[hooks\]\s+(\S+)\s+(.+?)\s+(ok|FAILED|skipped \(disabled\))\s*$')
DETOUR = re.compile(r'\[detour\]\s+(.+?)\s+(ok|FAILED|skipped \([^)]*\))(?:\s+@\S+)?\s*$')
SUSPECT = re.compile(r'mismatch|incompatib|REFUSING|not carried|FAILED|unexpected bytes|guard (?:failed|refused)|'
                     r'not the same build|exception|access violation', re.I)


# Detours the port retired on purpose on a later build (key() form): the engine function they hooked is gone,
# and the job moved elsewhere. Not reported as LOST.
RETIRED = {
    # 2.5 links amd_fidelityfx_loader_dx12 statically; ffxapi::Install hooks the module's exports instead.
    '[upscaler] FidelityFX API loader *',
}


def normal(line):
    """A problem line without its time stamp and thread id, to recognise it across launches."""
    return re.sub(r'\[\d{4}-\d\d-\d\d [^\]]*\]\s*(\[\s*\d+\]\s*)?', '', line).strip()


def key(name):
    """A hook or detour name across builds: the engine addresses in it (sub_1401FAFA4, 0x1d4dea0) move with
    every patch, the tag and the description do not."""
    return re.sub(r'sub_[0-9A-Fa-f]{6,}|0x[0-9A-Fa-f]+', '*', name)


def newest(pattern):
    files = glob.glob(pattern)
    return max(files, key=os.path.getmtime) if files else None


def read(path):
    if not path or not os.path.exists(path):
        return ''
    with open(path, encoding='utf-8', errors='replace') as f:
        return f.read()


def port_log(game):
    text = read(os.path.join(game, 'bin', 'x64', 'cyberpunkvrport.log'))
    hooks, detours, build, suspect, sdk = {}, {}, [], [], []
    for line in text.splitlines():
        if '[sdk-layout]' in line:
            sdk.append(line.split('[sdk-layout]', 1)[1].strip())
            continue
        m = HOOK.search(line)
        if m:
            hooks[f'{m.group(1)}:{m.group(2).strip()}'] = m.group(3)
            continue
        m = DETOUR.search(line)
        if m:
            detours[m.group(1).strip()] = m.group(2)
            continue
        if '[build]' in line:
            build.append(line.strip())
        elif SUSPECT.search(line):
            suspect.append(line.strip()[:220])
    return dict(hooks=hooks, detours=detours, build=build, suspect=suspect, sdk=sdk, empty=not text)


def red4ext_log(game):
    path = newest(os.path.join(game, 'red4ext', 'logs', 'red4ext-*.log'))
    text = read(path)
    info = dict(path=path, plugins=[], problems=[])
    for line in text.splitlines():
        if 'Product version' in line or 'is initializing' in line or 'game addresses loaded' in line:
            info.setdefault('head', []).append(line.split('] ')[-1])
        m = re.search(r'\[RED4ext\] (.+?) \(version: ([^,]+),', line)
        if m and 'has been loaded' in line:
            info['plugins'].append(f'{m.group(1)} {m.group(2)}')
        if re.search(r'\[(error|warning|critical)', line, re.I) or 'could not' in line.lower() or 'incompatible' in line.lower():
            info['problems'].append(line.strip()[:220])
    return info


def framework_logs(game):
    out = {}
    for name, pattern in (('TweakXL', 'red4ext/plugins/TweakXL/TweakXL-*.log'),
                          ('ArchiveXL', 'red4ext/plugins/ArchiveXL/ArchiveXL-*.log'),
                          ('Codeware', 'red4ext/plugins/Codeware/Codeware-*.log')):
        path = newest(os.path.join(game, pattern))
        lines = [ln.strip()[:220] for ln in read(path).splitlines() if re.search(r'\[(error|warn|warning|critical)\]', ln, re.I)]
        out[name] = dict(path=path, problems=lines)
    reds = read(os.path.join(game, 'r6', 'logs', 'redscript_rCURRENT.log'))
    out['redscript'] = dict(path='r6/logs/redscript_rCURRENT.log',
                            problems=[ln.strip()[:220] for ln in reds.splitlines() if ln.startswith('[ERROR')],
                            complete=any(v in reds for v in ('Compilation complete', 'Output successfully saved',
                                                              'Compilation successful')))
    cet_dir = os.path.join(game, 'bin', 'x64', 'plugins', 'cyber_engine_tweaks')
    since = launch_time(game)
    cet = [ln.strip()[:220] for ln in this_launch(read(os.path.join(cet_dir, 'cyber_engine_tweaks.log')), since)
           if re.search(r'\[error\]|error', ln, re.I) and 'CyberpunkVRPort' in ln]
    for mod in glob.glob(os.path.join(cet_dir, 'mods', 'CyberpunkVRPort_*')):
        for log in glob.glob(os.path.join(mod, '*.log')):
            for ln in this_launch(read(log), since):
                if re.search(r'\berror\b|attempt to|stack traceback', ln, re.I):
                    cet.append(f'{os.path.basename(mod)}: {ln.strip()[:200]}')
    out['CET'] = dict(path=cet_dir, problems=cet)
    return out


def launch_time(game):
    """Start of the newest launch, from the loader log's name (red4ext-YYYY-MM-DD-HH-MM-SS.log)."""
    path = newest(os.path.join(game, 'red4ext', 'logs', 'red4ext-*.log'))
    m = path and re.search(r'(\d{4})-(\d\d)-(\d\d)-(\d\d)-(\d\d)-(\d\d)\.log$', path)
    return '{}-{}-{} {}:{}:{}'.format(*m.groups()) if m else ''


def this_launch(text, since):
    """CET appends across launches: keep the lines stamped at or after `since`, and the untimed lines
    (tracebacks) that follow a kept one."""
    keep = False
    out = []
    for ln in text.splitlines():
        m = re.match(r'\[(\d{4}-\d\d-\d\d \d\d:\d\d:\d\d)', ln)
        if m:
            keep = m.group(1) >= since
        if keep:
            out.append(ln)
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--game', default=GAME)
    ap.add_argument('--baseline', default=BASELINE)
    ap.add_argument('--save-baseline', action='store_true')
    args = ap.parse_args()
    port = port_log(args.game)
    loader = red4ext_log(args.game)
    fw = framework_logs(args.game)
    if args.save_baseline:
        known = sorted({normal(p) for info in fw.values() for p in info['problems']} |
                       {normal(p) for p in loader['problems']})
        with open(args.baseline, 'w', encoding='utf-8') as f:
            json.dump(dict(hooks=port['hooks'], detours=port['detours'], plugins=loader['plugins'],
                           known_problems=known, sdk_layout=[l for l in port['sdk'] if l.startswith(('MISMATCH', '['))
                                                             or 'not a class' in l]), f, indent=1)
        print(f"baseline: {len(port['hooks'])} hooks, {len(port['detours'])} detours -> {args.baseline}")
        return
    base = {}
    if os.path.exists(args.baseline):
        with open(args.baseline, encoding='utf-8') as f:
            base = json.load(f)
    bad = 0
    print('== loader')
    for h in loader.get('head', []):
        print('  ', h)
    print('   plugins:', ', '.join(loader['plugins']) or 'NONE')
    for p in loader['problems']:
        print('   PROBLEM', p)
        bad += 1
    print('== port')
    if port['empty']:
        print('   no cyberpunkvrport.log -- the plugin did not start')
        bad += 1
    for b in port['build']:
        print('  ', b)
        if 'NOT the same build' in b:
            bad += 1
    for kind in ('hooks', 'detours'):
        now = {key(k): v for k, v in port[kind].items()}
        then = {key(k): v for k, v in base.get(kind, {}).items()}
        ok = sum(1 for v in now.values() if v == 'ok')
        print(f'   {kind}: {ok} ok, {sum(1 for v in now.values() if v == "FAILED")} FAILED, '
              f'{sum(1 for v in now.values() if v.startswith("skipped"))} skipped (baseline {sum(1 for v in then.values() if v == "ok")} ok)')
        for name, state in sorted(then.items()):
            cur = now.get(name)
            if state == 'ok' and cur != 'ok' and not (cur is None and name in RETIRED):
                print(f'     LOST {name}: {cur or "not attempted"}')
                bad += 1
        for name, state in sorted(now.items()):
            if state == 'FAILED' and then.get(name) != 'FAILED':
                print(f'     FAILED {name}')
    for s in port['suspect'][:40]:
        print('   suspect:', s)
    # Core/SdkLayouts.cpp: the RED4ext.SDK classes the port includes against this game's RTTI.
    known_sdk = set(base.get('sdk_layout', []))
    summary = [ln for ln in port['sdk'] if ln[:1].isdigit() or 'faulted' in ln]
    print('== SDK layouts:', summary[0] if summary else 'NO [sdk-layout] line (plugin older than the check?)')
    for ln in port['sdk']:
        if ln not in summary and ln not in known_sdk:
            print('  ', ln)
            bad += 1
    if summary and 'faulted' in summary[0]:
        bad += 1
    known = set(base.get('known_problems', []))
    for name, info in fw.items():
        probs = [p for p in info['problems'] if normal(p) not in known]
        old = len(info['problems']) - len(probs)
        extra = '' if name != 'redscript' else (' (compiled)' if info.get('complete') else ' (NOT compiled)')
        print(f'== {name}: {len(probs)} problems{extra}' + (f', {old} known from the baseline' if old else ''))
        for p in probs[:20]:
            print('  ', p)
        bad += len(probs) + (0 if name != 'redscript' or info.get('complete') else 1)
    print('LOGS', 'OK' if not bad else f'{bad} PROBLEMS')
    sys.exit(0 if not bad else 1)


if __name__ == '__main__':
    main()
