"""The C++ build (branch cpp, installed under RED4ext) against the Rust build (this tree, its own loader) in one game
folder: one owned vr_bench.py run per build, the same save, simulator, trajectories and vrport.ini.

    py -3.13 -B tools/vr_bench/ab_builds.py install-rust dist/CyberpunkVRPort-<ver>   # the Rust package into the game
    py -3.13 -B tools/vr_bench/ab_builds.py run [--save S] [--order cpp,rust] [--traj ...] [--keep rust|cpp]
    py -3.13 -B tools/vr_bench/ab_builds.py adopt-settings   # the player's C++ port settings into the Rust install
    py -3.13 -B tools/vr_bench/ab_builds.py status

The two installs:
  C++   <game>\\red4ext\\plugins\\cyberpunkvrport\\ (RED4ext loads it)
  Rust  <game>\\cyberpunkvrport\\ and <game>\\bin\\x64\\powrprof.dll (the port's own loader)
Each run sets the other build's files aside (build/bench-aside-<stamp>/; on the game's drive, so a rename) -- the
Rust loader retires an install it finds under red4ext\\plugins, so the C++ folder must not be there for its run --
writes one vrport.ini into the running build's config (the C++ install's own, with the base-benchmark keys below),
registers the benchmark simulator (head_trajectory build, CVR_SIM_DLL) as HKLM's OpenXR runtime -- an elevated
launch ignores XR_RUNTIME_JSON -- and puts the previous runtime, both ini files and the chosen install (--keep) back
whatever happens.
"""
import argparse
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
GAME = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
CPP = GAME / 'red4ext/plugins/cyberpunkvrport'
RUST = GAME / 'cyberpunkvrport'
LOADER = GAME / 'bin/x64/powrprof.dll'
SIM_DLL = Path(os.environ.get('CVR_SIM_DLL') or Path.home() / 'Desktop/OpenXR-Simulator-bench/bin/openxr_simulator.dll')
SIM_JSON = SIM_DLL.with_suffix('.json')
KEY = r'HKLM\SOFTWARE\Khronos\OpenXR\1'
# a base benchmark: no frame generation, no reconstruction, no foveation, the depth submitted, the FPS cap on
# xr_framegen_overlay=1: the port fills the frame rings vr_bench reads (CyberpunkVR_FrameRingCpu/Gpu) only with the
# FPS overlay on -- without it every window reads 0 frames
BENCH_INI = dict(xr_framegen='0', xr_foveated_dlss='0', xr_vrs='0', xr_hidden_area='0', xr_afw='0', xr_osr='0',
                 xr_fps_cap='1', xr_depth_submit='1', xr_engine_stereo='1', xr_framegen_overlay='1')
# the files both runs share (the C++ install's): the settings and the launcher's resolution / headset type
SHARED = ('vrport.ini', 'vrport-launcher.ini')
# the player's own port settings the Rust install takes over from the C++ one (adopt-settings)
ADOPT = ['config/vrport.ini', 'config/vrport-launcher.ini', 'config/vrik_calibration.ini', 'config/fbt_rig_male_*.txt',
         'lua/CyberpunkVRPort_VRIK/vrik_settings.ini', 'lua/CyberpunkVRPort_VRIK/vrik_recenter.ini',
         'lua/CyberpunkVRPort_Holster/holster_places.json']
STATE = ROOT / 'build/bench-aside-state.json'


def log(text):
    print(f'[ab_builds {time.strftime("%H:%M:%S")}] {text}', flush=True)


def active_runtime():
    q = subprocess.run(['reg', 'query', KEY, '/v', 'ActiveRuntime'], capture_output=True, text=True).stdout
    return q.split('REG_SZ')[-1].strip() if 'REG_SZ' in q else ''


def set_runtime(path):
    subprocess.run(['reg', 'add', KEY, '/v', 'ActiveRuntime', '/t', 'REG_SZ', '/d', str(path), '/f'], check=True,
                   capture_output=True)


def game_running():
    out = subprocess.run(['tasklist', '/FI', 'IMAGENAME eq Cyberpunk2077.exe', '/NH'], capture_output=True, text=True)
    return 'Cyberpunk2077.exe' in out.stdout


def parts(build):
    return [CPP] if build == 'cpp' else [RUST, LOADER]


def installed(build):
    return all(p.exists() for p in parts(build)) and (parts(build)[0] / 'cyberpunkvrport.dll').exists()


def set_aside(build, aside):
    for p in parts(build):
        if p.exists():
            dest = aside / build / p.relative_to(GAME)
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.move(str(p), str(dest))
            log(f'{build}: {p.relative_to(GAME)} set aside')


def bring_back(build, aside):
    for p in parts(build):
        src = aside / build / p.relative_to(GAME)
        if src.exists() and not p.exists():
            p.parent.mkdir(parents=True, exist_ok=True)
            shutil.move(str(src), str(p))
            log(f'{build}: {p.relative_to(GAME)} back')


def settle(build):
    """Read the build's DLLs end to end and give the disk a moment: a launch one second after the files came back by a
    rename once got System32's powrprof.dll instead of the port's loader (the port never started, the game ran flat)
    while two later launches of the same files loaded it -- the files freshly in place, still being scanned."""
    dlls = [CPP / 'cyberpunkvrport.dll'] if build == 'cpp' else [RUST / 'cyberpunkvrport.dll', LOADER]
    for dll in dlls:
        with open(dll, 'rb') as f:
            while f.read(1 << 22):
                pass
    time.sleep(5)


def port_started(build, since):
    """The Rust loader's own trace of this launch: written by DllMain when the port's log folder is there."""
    if build != 'rust':
        return None
    trace = RUST / 'log/loader-trace.txt'
    if not trace.exists() or trace.stat().st_mtime < since:
        return False
    return 'the port started' in trace.read_text(encoding='utf-8', errors='replace')


def ini_with(text, values):
    lines, seen = [], set()
    for line in text.splitlines():
        k, sep, _ = line.partition('=')
        if sep and k.strip() in values:
            lines.append(f'{k.strip()}={values[k.strip()]}')
            seen.add(k.strip())
        else:
            lines.append(line)
    lines += [f'{k}={v}' for k, v in values.items() if k not in seen]
    return '\n'.join(lines) + '\n'


def install_rust(args):
    pkg = (ROOT / args.package).resolve()
    if game_running():
        sys.exit('close the game first')
    if not (pkg / 'cyberpunkvrport/cyberpunkvrport.dll').exists():
        sys.exit(f'{pkg} is not a Rust package (no cyberpunkvrport\\cyberpunkvrport.dll)')
    # the plugin folder and bin\x64 only: the package's readme/uninstall files stay out of the game root
    for src in [*(pkg / 'cyberpunkvrport').rglob('*'), *(pkg / 'bin').rglob('*')]:
        if src.is_file():
            dst = GAME / src.relative_to(pkg)
            dst.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(src, dst)
    log(f'Rust package {pkg.name} installed into {GAME}')


def run(args):
    if game_running():
        sys.exit('close the game first')
    if not SIM_DLL.exists() or b'head_trajectory' not in SIM_DLL.read_bytes():
        sys.exit(f'{SIM_DLL}: not the head_trajectory simulator build (CVR_SIM_DLL)')
    stamp = time.strftime('%Y%m%d-%H%M%S')
    aside = ROOT / f'build/bench-aside-{stamp}'
    order = args.order.split(',')
    # whatever is aside from an interrupted run comes back first
    if STATE.exists():
        old = json.loads(STATE.read_text(encoding='utf-8'))
        for b in ('cpp', 'rust'):
            bring_back(b, Path(old['aside']))
    for b in order:
        if not installed(b) and not (aside / b).exists():
            sys.exit(f'the {b} build is not installed')
    STATE.write_text(json.dumps(dict(aside=str(aside)), indent=1), encoding='utf-8')
    # the same files for both: the C++ install's own, vrport.ini with the benchmark keys
    base = {name: (CPP / 'config' / name).read_bytes() for name in SHARED if (CPP / 'config' / name).exists()}
    keys = dict(BENCH_INI, **dict(kv.split('=', 1) for kv in args.ini))
    bench = {name: (ini_with(data.decode('utf-8', errors='replace'), keys).encode('utf-8')
                    if name == 'vrport.ini' else data) for name, data in base.items()}
    saved = {(b, name): ((parts(b)[0] / 'config' / name).read_bytes() if (parts(b)[0] / 'config' / name).exists()
                         else None) for b in ('cpp', 'rust') for name in base}
    previous_runtime = active_runtime()
    results = {}
    try:
        for b in order:
            other = 'rust' if b == 'cpp' else 'cpp'
            set_aside(other, aside)
            bring_back(b, aside)
            for name, data in bench.items():
                p = parts(b)[0] / 'config' / name
                p.parent.mkdir(parents=True, exist_ok=True)
                p.write_bytes(data)
            set_runtime(SIM_JSON)
            log(f'{b}: runtime {active_runtime()}')
            env = dict(os.environ, CVR_SIM_DLL=str(SIM_DLL), CVR_PORT_DIR=str(parts(b)[0]))
            label = f'{args.label}-{b}'
            cmd = [sys.executable, '-B', str(HERE / 'vr_bench.py'), 'run', '--label', label, '--save', args.save,
                   '--any-dll', '--no-telegram']
            if args.traj:
                cmd += ['--traj', args.traj]
            if args.etw_cpu:
                cmd += ['--etw-cpu', args.etw_cpu]
            if args.timeline_on:
                cmd += ['--timeline-on', args.timeline_on]
            if args.node_timeline:
                cmd += ['--node-timeline', args.node_timeline, '--node-timeline-seconds', str(args.node_timeline_seconds)]
            settle(b)
            log(f'{b}: ' + ' '.join(cmd[2:]))
            since = time.time()
            code = subprocess.run(cmd, cwd=ROOT, env=env).returncode
            runs = sorted((ROOT / 'build/vr-bench').glob(f'*-{label}'))
            results[b] = dict(code=code, out=str(runs[-1]) if runs else None, port_started=port_started(b, since))
            log(f'{b}: exit {code}, port started: {results[b]["port_started"]}')
            set_runtime(previous_runtime)
            if game_running():
                raise RuntimeError('the game is still running after the run')
    finally:
        set_runtime(previous_runtime)
        keep = args.keep
        set_aside('cpp' if keep == 'rust' else 'rust', aside)
        bring_back(keep, aside)
        for (b, name), data in saved.items():
            folder = parts(b)[0] if b == keep else aside / b / parts(b)[0].relative_to(GAME)
            p = folder / 'config' / name
            if p.parent.exists():
                # an install without settings of its own (the Rust one's first run) takes the player's C++ settings
                p.write_bytes(data if data is not None else base[name])
        if not any((aside / b).exists() for b in ('cpp', 'rust')):
            STATE.unlink(missing_ok=True)
        log(f'runtime {active_runtime()}; installed: {keep}; aside: {aside}')
    (ROOT / 'build/vr-bench').mkdir(parents=True, exist_ok=True)
    out = ROOT / f'build/vr-bench/{stamp}-{args.label}-ab.json'
    out.write_text(json.dumps(results, indent=1), encoding='utf-8')
    log(f'results {out}: {results}')
    # one build alone (--order rust: a new DLL against the other build's earlier run) is compared by hand
    if len(order) == 2 and all(results.get(b, {}).get('out') for b in order):
        subprocess.run([sys.executable, '-B', str(HERE / 'vr_bench.py'), 'compare', results[order[0]]['out'],
                        results[order[1]]['out'], '--no-telegram'], cwd=ROOT)


def ini_values(text):
    out = {}
    for line in text.splitlines():
        k, sep, v = line.partition('=')
        if sep and k.strip() and not k.lstrip().startswith((';', '#', '[')):
            out[k.strip()] = v.strip()
    return out


def adopt_settings(_args):
    """The player's port settings from the C++ install (in place or set aside) into the Rust one: the ini files
    merged (the C++ values over the Rust file, the Rust build's own keys kept), the rest copied; the Rust files
    replaced go to build/rust-settings-before-<stamp>/."""
    if game_running():
        sys.exit('close the game first')
    src = CPP
    if not (src / 'config').exists() and STATE.exists():
        src = Path(json.loads(STATE.read_text(encoding='utf-8'))['aside']) / 'cpp' / CPP.relative_to(GAME)
    if not (src / 'config').exists() or not RUST.exists():
        sys.exit('need both installs (the C++ one in place or set aside by a run)')
    backup = ROOT / f'build/rust-settings-before-{time.strftime("%Y%m%d-%H%M%S")}'
    for pattern in ADOPT:
        for s in sorted(src.glob(pattern)):
            rel = s.relative_to(src)
            d = RUST / rel
            if d.exists():
                (backup / rel).parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(d, backup / rel)
            d.parent.mkdir(parents=True, exist_ok=True)
            if s.suffix == '.ini' and d.exists():
                merged = ini_with(d.read_text(encoding='utf-8', errors='replace'),
                                  ini_values(s.read_text(encoding='utf-8', errors='replace')))
                d.write_text(merged, encoding='utf-8')
            else:
                shutil.copy2(s, d)
            log(f'{rel}: from {src}')
    log(f'replaced Rust files kept in {backup}' if backup.exists() else 'nothing replaced')


def status(_args):
    for b in ('cpp', 'rust'):
        log(f'{b}: installed={installed(b)}')
    log(f'runtime {active_runtime()}; aside state: {STATE.read_text() if STATE.exists() else "none"}')


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest='cmd', required=True)
    i = sub.add_parser('install-rust')
    i.add_argument('package')
    r = sub.add_parser('run')
    r.add_argument('--label', default='builds')
    r.add_argument('--save', default='ManualSave-491')
    r.add_argument('--order', default='cpp,rust')
    r.add_argument('--traj', default='')
    r.add_argument('--etw-cpu', default='', help='the trajectory vr_bench samples with wpr in both runs')
    r.add_argument('--ini', action='append', default=[], help='KEY=VALUE over the benchmark vrport.ini keys (e.g. xr_mono_mode=1)')
    r.add_argument('--timeline-on', default='', help="vr_bench's GPU queue timeline trajectories (its default if empty)")
    r.add_argument('--node-timeline', default='', help="vr_bench's node / render-chain timeline trajectories")
    r.add_argument('--node-timeline-seconds', type=float, default=0.0, help="vr_bench's --node-timeline-seconds")
    r.add_argument('--keep', default='rust', choices=['rust', 'cpp'], help='the install left in the game after the run')
    sub.add_parser('adopt-settings', help="the player's C++ port settings into the Rust install")
    sub.add_parser('status')
    a = p.parse_args()
    {'install-rust': install_rust, 'run': run, 'adopt-settings': adopt_settings, 'status': status}[a.cmd](a)


if __name__ == '__main__':
    main()
