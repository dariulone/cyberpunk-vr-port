"""VR rendering benchmark and stereo regression harness (OpenXR Simulator, one owned game launch per run).

    py -3.13 -B tools/vr_bench/vr_bench.py run --label baseline [--save ManualSave-491] [--traj still,yaw_fast,...]
    py -3.13 -B tools/vr_bench/vr_bench.py compare build/vr-bench/<A> build/vr-bench/<B>
    py -3.13 -B tools/vr_bench/vr_bench.py deploy          # install the built plugin DLL if it differs

A run launches the game into a save (tools/single_pass_tests/geometry_cost_live.py: launch, Continue), lets the
world stream in, then plays recorded head trajectories in the simulator (head_trajectory.h: sampled at each frame's
display time). For every trajectory it reads, with no polling loss:
  * every frame's CPU interval and GPU frame time (the plugin's CyberpunkVR_FrameRing*): mean, P50/P95/P99/max,
    1% / 0.1% low FPS, frames over the display period;
  * repeated (reprojected) submissions from the plugin's frame report;
  * GPU utilisation / VRAM / SM clock (nvidia-smi at 4 Hz), per-thread CPU (GameThread, render workers);
and, on the still head, the GPU time of every render-graph node per frame, split by eye where the pass draws into
one half of the side-by-side frame (CyberpunkVR_SceneGpuFrameRequest + ViewportDetail). Stereo captures (both
eyes' colour, depth, cameras) are taken on the still head and in the middle of the motions and checked with
stereo_check.exe (one eye reprojected into the other by depth: colour and geometry agreement, heat maps); only the
pictures and numbers are kept, the raw frames are deleted.

Then the game is closed and the preview window restored. Results: build/vr-bench/<stamp>-<label>/run.json and
summary.md; a short report and the side-by-side still go to Telegram (tools/vr_bench/telegram.py).
"""
import argparse
import ctypes as c
import ctypes.wintypes as w
import datetime
import json
import math
import os
from pathlib import Path
import shutil
import statistics
import struct
import subprocess
import sys
import threading
import time

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path[:0] = [str(HERE), str(ROOT / 'tools/swimming_tests'), str(ROOT / 'tools/single_pass_tests'),
                str(ROOT / 'tools/quickboot_tests'), str(ROOT / 'tools/roomscale_tests')]
import trajectories  # noqa: E402
import telegram  # noqa: E402
import etw_cpu  # noqa: E402

GAME = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
DLL = GAME / 'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll'
EXE = GAME / 'bin/x64/Cyberpunk2077.exe'
PLUGIN_LOG = GAME / 'red4ext/plugins/cyberpunkvrport/log'
INI = GAME / 'red4ext/plugins/cyberpunkvrport/config/vrport.ini'
BUILT = ROOT / 'build/patchday/sdk25-build/bin/red4ext/plugins/cyberpunkvrport/Release/cyberpunkvrport.dll'
SIM = Path(os.environ['LOCALAPPDATA']) / 'OpenXR-Simulator'
SIM_DLL = Path(r'C:\Users\lone33\Desktop\OpenXR-Simulator\bin\openxr_simulator.dll')
LIVE = ROOT / 'tools/single_pass_tests/geometry_cost_live.py'
STEREO_CHECK = HERE / 'stereo_check.exe'
OUT_ROOT = ROOT / 'build/vr-bench'
NODE_NAMES = ROOT / 'src/Stereo/NodeNames.inc'
KEEP_STEREO = 10   # stereo capture folders kept across runs (the newest)

SYMBOLS = ['CyberpunkVR_FrameRingCount', 'CyberpunkVR_FrameRingCpu', 'CyberpunkVR_FrameRingGpu',
           'CyberpunkVR_SceneGpuFrameRequest', 'CyberpunkVR_SceneGpuViewportDetail', 'CyberpunkVR_SceneGpuGroupMode',
           'CyberpunkVR_SceneGpuPipelineStats', 'CyberpunkVR_XrProjectionCycles', 'CyberpunkVR_XrRepeatCycles',
           'CyberpunkVR_SceneGpuDrawOcclusion',
           'CyberpunkVR_SceneGpuNodeFilter', 'CyberpunkVR_SceneGpuReport', 'CyberpunkVR_SceneGpuReportSeq',
           'CyberpunkVR_SceneGpuRequest', 'CyberpunkVR_FramegenCapture', 'CyberpunkVR_FramegenReport',
           'CyberpunkVR_FramegenReportSeq', 'CyberpunkVR_RuntimeDiagnostics', 'CyberpunkVR_PresentTiming',
           'CyberpunkVR_GpuTimelineRequest', 'CyberpunkVR_GpuTimelineState', 'CyberpunkVR_GpuTimelineCount',
           'CyberpunkVR_GpuTimelineCalib', 'CyberpunkVR_GpuTimelineRecords', 'CyberpunkVR_ShaderCensus',
           'CyberpunkVR_ShaderCensusLog', 'CyberpunkVR_ShaderCensusRecording']


def log(text):
    print(f'[{datetime.datetime.now():%H:%M:%S}] {text}', flush=True)


# ---- the live game ------------------------------------------------------------------------------------------------
class Game:
    def __init__(self, pid):
        from live_symbols import resolve
        self.pid = pid
        self.symbols = resolve(pid, DLL, SYMBOLS)
        self.k = c.WinDLL('kernel32', use_last_error=True)
        self.k.OpenProcess.restype = w.HANDLE
        self.k.OpenProcess.argtypes = [w.DWORD, w.BOOL, w.DWORD]
        self.k.ReadProcessMemory.argtypes = [w.HANDLE, c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]
        self.k.WriteProcessMemory.argtypes = self.k.ReadProcessMemory.argtypes
        self.h = self.k.OpenProcess(0x0008 | 0x0010 | 0x0020 | 0x1000, False, pid)
        if not self.h:
            raise c.WinError(c.get_last_error())

    def addr(self, name):
        return int(self.symbols[name], 16)

    def read(self, address, size):
        buf = c.create_string_buffer(size)
        got = c.c_size_t()
        if not self.k.ReadProcessMemory(self.h, address, buf, size, c.byref(got)) or got.value != size:
            raise c.WinError(c.get_last_error())
        return buf.raw

    def write(self, address, data):
        buf = c.create_string_buffer(bytes(data))
        got = c.c_size_t()
        if not self.k.WriteProcessMemory(self.h, address, buf, len(data), c.byref(got)) or got.value != len(data):
            raise c.WinError(c.get_last_error())

    def u32(self, name, offset=0):
        return struct.unpack('<I', self.read(self.addr(name) + offset, 4))[0]

    def i32(self, name):
        return struct.unpack('<i', self.read(self.addr(name), 4))[0]

    def set32(self, name, value):
        self.write(self.addr(name), struct.pack('<I', value & 0xFFFFFFFF))

    def u64(self, name, index=0):
        return struct.unpack('<Q', self.read(self.addr(name) + 8 * index, 8))[0]

    def seqlock(self, body, seq, size):
        for _ in range(400):
            before = self.u32(seq)
            if before & 1:
                time.sleep(0.002)
                continue
            raw = self.read(self.addr(body), size).split(b'\0', 1)[0]
            if self.u32(seq) == before:
                return json.loads(raw) if raw else {}
        raise RuntimeError(f'{body} stayed busy')

    # every frame's times
    def ring_counts(self):
        return self.u64('CyberpunkVR_FrameRingCount', 0), self.u64('CyberpunkVR_FrameRingCount', 1)

    def ring(self, name, start, end):
        end = min(end, start + 8192)
        if end <= start:
            return []
        raw = self.read(self.addr(name), 8192 * 4)
        values = struct.unpack('<8192f', raw)
        return [values[i % 8192] for i in range(start, end)]

    def frame_report(self):
        try:
            return self.seqlock('CyberpunkVR_FramegenReport', 'CyberpunkVR_FramegenReportSeq', 2048)
        except (ValueError, RuntimeError):
            return {}

    def close(self):
        if self.h:
            self.k.CloseHandle(self.h)
            self.h = None


# ---- simulator --------------------------------------------------------------------------------------------------
def sim_status():
    try:
        return json.loads((SIM / 'runtime_status.json').read_text(encoding='utf-8'))
    except (OSError, ValueError):
        return {}


def sim_base():
    head = sim_status().get('head_tracking') or {}
    pos = head.get('position') or {'x': 0.0, 'y': 1.7, 'z': 0.0}
    return dict(x=pos['x'], y=pos['y'], z=pos['z'], yaw=head.get('yaw', 0.0), pitch=head.get('pitch', 0.0),
                roll=head.get('roll', 0.0))


def sim_send(name, text):
    path = SIM / name
    tmp = path.with_suffix('.tmp')
    tmp.write_text(text, encoding='utf-8')
    for _ in range(500):
        try:
            os.replace(tmp, path)
            return
        except PermissionError:
            time.sleep(0.004)
    raise PermissionError(f'the simulator holds {path}')


def trajectory_status():
    try:
        return json.loads((SIM / 'head_trajectory_status.json').read_text(encoding='utf-8'))
    except (OSError, ValueError):
        return {}


def play(name, base, ident, timeout=10.0):
    """Starts trajectory `name`; returns (seconds, start wall time) once the simulator reports it running. The
    simulator now and then misses a command file (seen twice on 2026-11-05 with the game presenting normally): the
    command is sent once more before giving up."""
    seconds = trajectories.TRAJECTORIES[name][0]
    for attempt in range(2):
        sim_send('head_trajectory_command.txt', trajectories.text(name, base, delay=0.3, ident=ident))
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            st = trajectory_status()
            if st.get('id') == ident and st.get('state') in ('running', 'done'):
                return seconds
            time.sleep(0.01)
        log(f'  the simulator did not start trajectory {name} (attempt {attempt + 1})')
    raise TimeoutError(f'the simulator did not start trajectory {name} (needs the head_trajectory build)')


def xr_cycles(game):
    """(display cycles with a projection layer, of them showing the previous cycle's game frame again) -- the
    reprojection the runtime does; None for a plugin without the counters."""
    try:
        return game.u64('CyberpunkVR_XrProjectionCycles'), game.u64('CyberpunkVR_XrRepeatCycles')
    except (KeyError, RuntimeError, OSError):
        return None


def stalled(row, seconds):
    """A window the game presented (almost) nothing in: fewer than 5 frames a second. The simulator's trajectory runs
    on the XR thread's display clock, so it completes even while the game's frames stop (seen once after a settings
    change: 1 frame in a 7 s window, nvidia-smi silent too). Such a window has no frame statistics; it is kept under
    'stalls' and measured again."""
    return (row.get('frames') or 0) < max(10, int(seconds * 5))


def wait_done(ident, timeout):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        st = trajectory_status()
        if st.get('id') == ident and st.get('state') == 'done':
            return st
        time.sleep(0.02)
    raise TimeoutError('trajectory did not finish')


# ---- side measurements ------------------------------------------------------------------------------------------
class GpuSampler(threading.Thread):
    """nvidia-smi at 4 Hz: utilisation %, memory used MiB, SM clock MHz, power W."""
    def __init__(self):
        super().__init__(daemon=True)
        self.rows, self.process = [], None

    def run(self):
        try:
            self.process = subprocess.Popen(['nvidia-smi', '--query-gpu=utilization.gpu,memory.used,clocks.sm,power.draw',
                                             '--format=csv,noheader,nounits', '-lms', '250'],
                                            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
            for line in self.process.stdout:
                parts = [p.strip() for p in line.split(',')]
                try:
                    self.rows.append([float(p) for p in parts])
                except ValueError:
                    pass
        except OSError:
            pass

    def stop(self):
        if self.process:
            self.process.terminate()
            try:
                self.process.wait(3)
            except subprocess.TimeoutExpired:
                self.process.kill()
        rows = self.rows[1:] or self.rows
        if not rows:
            return {}
        col = lambda i: [r[i] for r in rows if len(r) > i]
        return dict(utilization=statistics.mean(col(0)), memoryMiB=max(col(1)), smClock=statistics.median(col(2)),
                    powerW=statistics.mean(col(3)) if col(3) else None, samples=len(rows))


def steady(prev, cur):
    """Two warm-up windows agree: at least 15 frames, frame p50 within 4 % and median SM clock within 3 %."""
    if not prev or not cur or not all((prev.get('p50'), cur.get('p50'), prev.get('clock'), cur.get('clock'))):
        return False
    return (cur['frames'] >= 15 and abs(cur['p50'] - prev['p50']) <= 0.04 * prev['p50'] and
            abs(cur['clock'] - prev['clock']) <= 0.03 * prev['clock'])


def wait_steady(game, timeout=60.0, window=2.0):
    """After the settle: 2 s windows until the frame time and the GPU clock hold (steady()). A first window measured
    while the GPU still ramps out of a low power state ran up to 4x slower at its start (SM 1190-1310 MHz instead of
    2040, busy GPU frames from 102 down to 22 ms over 3 s)."""
    start = time.monotonic()
    windows, prev = [], None
    while time.monotonic() - start < timeout:
        sampler = GpuSampler()
        sampler.start()
        c0, _ = game.ring_counts()
        time.sleep(window)
        c1, _ = game.ring_counts()
        gpu = sampler.stop()
        cpu = game.ring('CyberpunkVR_FrameRingCpu', c0, c1)
        cur = dict(frames=len(cpu), p50=statistics.median(cpu) if cpu else None, clock=gpu.get('smClock'),
                   util=gpu.get('utilization'))
        windows.append(cur)
        log(f'  warm-up: {cur["frames"]} frames  p50 {fmt(cur["p50"])} ms  SM {fmt(cur["clock"], 0)} MHz  '
            f'util {fmt(cur["util"], 0)}%')
        if steady(prev, cur):
            return dict(steady=True, seconds=round(time.monotonic() - start, 1), windows=windows)
        prev = cur
    log('  warm-up: not steady within the timeout -- measuring anyway')
    return dict(steady=False, seconds=round(time.monotonic() - start, 1), windows=windows)


def thread_cpu(pid, seconds, out):
    try:
        from thread_cpu_sample import measure
        value = measure(pid, seconds)
        rows = value['threads']
        gt = [r for r in rows if r['name'] == 'GameThread']
        workers = [r for r in rows if r['name'].startswith('redDispatcher')]
        out.update(gameThread=gt[0]['percentOfCore'] if gt else None,
                   workersSum=sum(r['percentOfCore'] for r in workers), workers=len(workers),
                   workerMax=max((r['percentOfCore'] for r in workers), default=0), total=value['total'],
                   top=[dict(name=r['name'], pct=round(r['percentOfCore'], 1)) for r in
                        sorted(rows, key=lambda r: -r['percentOfCore'])[:8]])
    except Exception as error:   # diagnostics only, never fail a run on them
        out['error'] = str(error)


def frame_stats(cpu, gpu, period_ms):
    def pct(v, p):
        if not v:
            return None
        s = sorted(v)
        return s[min(len(s) - 1, int(round(p * (len(s) - 1))))]

    def low(v, fraction):
        if not v:
            return None
        s = sorted(v)
        worst = s[-max(1, int(len(s) * fraction)):]
        return 1000.0 / statistics.mean(worst)
    out = dict(frames=len(cpu))
    if cpu:
        out.update(cpuMean=statistics.mean(cpu), cpuP50=pct(cpu, .5), cpuP95=pct(cpu, .95), cpuP99=pct(cpu, .99),
                   cpuMax=max(cpu), fps=1000.0 / statistics.mean(cpu), low1=low(cpu, .01), low01=low(cpu, .001),
                   overPeriod=sum(1 for x in cpu if x > period_ms * 1.05),
                   overTwoPeriods=sum(1 for x in cpu if x > period_ms * 2.05))
    if gpu:
        out.update(gpuMean=statistics.mean(gpu), gpuP50=pct(gpu, .5), gpuP95=pct(gpu, .95), gpuP99=pct(gpu, .99),
                   gpuMax=max(gpu), gpuFrames=len(gpu))
    return out


# ---- GPU per node -----------------------------------------------------------------------------------------------
def node_names():
    import re
    try:
        text = NODE_NAMES.read_text(encoding='utf-8')
    except OSError:
        return {}
    names = {int(rva, 16): name for rva, name in re.findall(r'\{\s*(0x[0-9A-Fa-f]+)\s*,\s*"([^"]+)"', text)}
    # pseudo-nodes the port marks itself (Stereo/EngineStereoDlss.cpp)
    for eye in (0, 1):
        names[0xD15A0010 + eye] = f'DLSS eye {eye} input copies (port)'
        names[0xD15A0020 + eye] = f'DLSS eye {eye} evaluate'
        names[0xD15A0030 + eye] = f'DLSS eye {eye} output copy (port)'
    return names


def scene_profile(game, frames=4, timeout=15):
    if game.u32('CyberpunkVR_SceneGpuFrameRequest') or game.u32('CyberpunkVR_SceneGpuRequest'):
        raise RuntimeError('a scene profile is already requested')
    before = game.u32('CyberpunkVR_SceneGpuReportSeq')
    game.set32('CyberpunkVR_SceneGpuGroupMode', 1)
    game.set32('CyberpunkVR_SceneGpuNodeFilter', 0)
    game.set32('CyberpunkVR_SceneGpuViewportDetail', 1)
    game.set32('CyberpunkVR_SceneGpuFrameRequest', frames)
    deadline = time.monotonic() + timeout
    try:
        while time.monotonic() < deadline:
            time.sleep(0.25)
            if game.u32('CyberpunkVR_SceneGpuReportSeq') == before:
                continue
            value = game.seqlock('CyberpunkVR_SceneGpuReport', 'CyberpunkVR_SceneGpuReportSeq', 262144)
            if value.get('mode') == 'frames' and not value.get('active') and value.get('completed'):
                return value
        raise TimeoutError('scene profile did not finish')
    finally:
        game.set32('CyberpunkVR_SceneGpuViewportDetail', 0)


def eye_of(detail, right_widths):
    """'L' / 'R' / 'F' (whole frame or a mono pass) / 'C' (no viewport: compute, clears on the graphics queue) /
    'A' (recorded on a compute list: async compute, whose span overlaps the graphics queue) for a range detail."""
    if detail & 0x200000:
        return 'A'
    if not detail & 0x100000:
        return 'C'
    kind, width = (detail >> 16) & 3, detail & 0xFFFF
    if kind == 2:
        return 'R'
    if kind == 1:
        return 'L' if width in right_widths else 'F'
    return 'O'


def summarize_profiles(profiles, names):
    """Per node: ms per frame, split L / R / F / C / O, averaged over the profiles; also per-eye totals."""
    nodes, totals, frames_total = {}, {}, 0
    for value in profiles:
        frames = value.get('frames') or 0
        if frames <= 0:
            continue
        frames_total += frames
        groups = value.get('groups', [])
        rights = {}
        for g in groups:
            d = g['detail']
            if d & 0x100000 and ((d >> 16) & 3) == 2:
                rights.setdefault(g['node'], set()).add(d & 0xFFFF)
        for g in groups:
            eye = eye_of(g['detail'], rights.get(g['node'], set()))
            row = nodes.setdefault(g['node'], dict(L=0.0, R=0.0, F=0.0, C=0.0, A=0.0, O=0.0, ranges=0, commands=0))
            row[eye] += g['sum_ms']
            row['ranges'] += g['ranges']
            row['commands'] += g['commands']
    if not frames_total:
        return dict(frames=0, nodes=[], totals={})
    rows = []
    for node, row in nodes.items():
        r = {k: v / frames_total for k, v in row.items()}
        r['total'] = r['L'] + r['R'] + r['F'] + r['C'] + r['A'] + r['O']
        r['node'] = hex(node)
        r['name'] = names.get(node, '?')
        rows.append(r)
    rows.sort(key=lambda r: -r['total'])
    for key in 'LRFCAO':
        totals[key] = sum(r[key] for r in rows)
    totals['all'] = sum(r['total'] for r in rows)
    return dict(frames=frames_total, nodes=rows, totals=totals)


def port_stages(game, frames=32, timeout=20):
    """The port's own GPU passes (src/Render/GpuStageProfile.cpp): eye captures, XR submits, HUD."""
    from live_symbols import resolve
    game.symbols.update(resolve(game.pid, DLL, ['CyberpunkVR_GpuStageRequest', 'CyberpunkVR_GpuStageReport',
                                                'CyberpunkVR_GpuStageReportSeq']))
    game.set32('CyberpunkVR_GpuStageRequest', min(frames, 32))
    deadline = time.monotonic() + timeout
    report = {}
    while time.monotonic() < deadline:
        time.sleep(0.5)
        report = game.seqlock('CyberpunkVR_GpuStageReport', 'CyberpunkVR_GpuStageReportSeq', 2048)
        if not report.get('pending') and report.get('batches'):
            break
    stages = [dict(name=s['name'], count=s['count'], meanUs=s['mean_us'], maxUs=s['max_us'])
              for s in report.get('stages', []) if s.get('count')]
    n = max((s['count'] for s in stages), default=0)
    per_frame = sum(s['meanUs'] * s['count'] for s in stages) / n if n else 0.0
    return dict(frames=n, perFrameUs=per_frame, stages=sorted(stages, key=lambda s: -s['meanUs']))


def node_detail_profile(game, node, frames=4):
    """One node per pipeline: (pso, ms per frame, commands per frame, ranges per frame), ordered by time."""
    game.set32('CyberpunkVR_SceneGpuGroupMode', 0)
    game.set32('CyberpunkVR_SceneGpuNodeFilter', node)
    try:
        before = game.u32('CyberpunkVR_SceneGpuReportSeq')
        game.set32('CyberpunkVR_SceneGpuViewportDetail', 1)
        game.set32('CyberpunkVR_SceneGpuFrameRequest', frames)
        deadline = time.monotonic() + 15
        value = {}
        while time.monotonic() < deadline:
            time.sleep(0.25)
            if game.u32('CyberpunkVR_SceneGpuReportSeq') == before:
                continue
            value = game.seqlock('CyberpunkVR_SceneGpuReport', 'CyberpunkVR_SceneGpuReportSeq', 262144)
            if value.get('mode') == 'frames' and not value.get('active') and value.get('completed'):
                break
        n = value.get('frames') or 0
        rows = {}
        for g in value.get('groups', []):
            r = rows.setdefault((g['pso'], g['detail']), dict(pso=hex(g['pso']), detail=hex(g['detail']), ms=0.0, commands=0,
                                                              ranges=0))
            r['ms'] += g['sum_ms']
            r['commands'] += g['commands']
            r['ranges'] += g['ranges']
        out = sorted(rows.values(), key=lambda r: -r['ms'])
        for r in out:
            for k in ('ms', 'commands', 'ranges'):
                r[k] = r[k] / n if n else 0
        return dict(node=hex(node), frames=n, rows=out)
    finally:
        game.set32('CyberpunkVR_SceneGpuNodeFilter', 0)
        game.set32('CyberpunkVR_SceneGpuGroupMode', 1)
        game.set32('CyberpunkVR_SceneGpuViewportDetail', 0)


PIPELINE_STATS = ('iaVertices', 'iaPrimitives', 'vsInvocations', 'gsInvocations', 'gsPrimitives', 'cInvocations',
                  'cPrimitives', 'psInvocations', 'hsInvocations', 'dsInvocations', 'csInvocations')


def pipeline_stats_summary(groups, frames):
    """Per eye (eye_of): the D3D12 pipeline statistics of a node's ranges per frame, and the share of primitives the
    clipper dropped (1 - CPrimitives / CInvocations: outside the eye's frustum, or degenerate)."""
    right_widths = {g['detail'] & 0xFFFF for g in groups
                    if g['detail'] & 0x100000 and (g['detail'] >> 16) & 3 == 2}
    eyes = {}
    for g in groups:
        if 'ps' not in g:
            continue
        e = eyes.setdefault(eye_of(g['detail'], right_widths), dict.fromkeys(PIPELINE_STATS, 0))
        for k, v in zip(PIPELINE_STATS, g['ps']):
            e[k] += v
    for e in eyes.values():
        for k in PIPELINE_STATS:
            e[k] = e[k] / frames if frames else 0
        e['clippedShare'] = 1 - e['cPrimitives'] / e['cInvocations'] if e['cInvocations'] else None
    return eyes


def pipeline_stats_profile(game, node, frames=4):
    game.set32('CyberpunkVR_SceneGpuGroupMode', 1)
    game.set32('CyberpunkVR_SceneGpuNodeFilter', node)
    game.set32('CyberpunkVR_SceneGpuPipelineStats', 1)
    try:
        before = game.u32('CyberpunkVR_SceneGpuReportSeq')
        game.set32('CyberpunkVR_SceneGpuViewportDetail', 1)
        game.set32('CyberpunkVR_SceneGpuFrameRequest', frames)
        deadline = time.monotonic() + 15
        value = {}
        while time.monotonic() < deadline:
            time.sleep(0.25)
            if game.u32('CyberpunkVR_SceneGpuReportSeq') == before:
                continue
            value = game.seqlock('CyberpunkVR_SceneGpuReport', 'CyberpunkVR_SceneGpuReportSeq', 262144)
            if value.get('mode') == 'frames' and not value.get('active') and value.get('completed'):
                break
        n = value.get('frames') or 0
        return dict(node=hex(node), frames=n, eyes=pipeline_stats_summary(value.get('groups', []), n))
    finally:
        game.set32('CyberpunkVR_SceneGpuPipelineStats', 0)
        game.set32('CyberpunkVR_SceneGpuNodeFilter', 0)
        game.set32('CyberpunkVR_SceneGpuViewportDetail', 0)


OCC_BUCKETS = ('draws', 'zero', 'samples1to64', 'samples65to4096', 'more')


def draw_occlusion_summary(occ, frames):
    """Per viewport kind (1 left eye / whole frame, 2 right half, 3 other): draws per frame and the shares by visible
    samples (a draw with 0 left nothing on screen: hidden behind others, outside the eye's view or degenerate)."""
    out = {}
    for kind, name in (('1', 'L'), ('2', 'R'), ('3', 'O'), ('0', '?')):
        c = occ.get(kind)
        if not c or not c[0]:
            continue
        out[name] = dict(drawsPerFrame=c[0] / frames if frames else 0,
                         **{OCC_BUCKETS[b] + 'Share': c[b] / c[0] for b in range(1, 5)})
    return out


def draw_occlusion_pipelines(groups, frames, top=12):
    """The pipelines with the most draws that left no visible sample, per frame (both eyes together)."""
    by = {}
    for g in groups:
        if 'occ' not in g:
            continue
        r = by.setdefault(g['pso'], [0] * 5)
        for b in range(5):
            r[b] += g['occ'][b]
    rows = [dict(pso=hex(p), drawsPerFrame=c[0] / frames, zeroPerFrame=c[1] / frames, zeroShare=c[1] / c[0])
            for p, c in by.items() if c[0] and frames]
    rows.sort(key=lambda r: -r['zeroPerFrame'])
    return dict(pipelines=len(rows), top=rows[:top])


def draw_occlusion_profile(game, node, frames=4):
    game.set32('CyberpunkVR_SceneGpuGroupMode', 0)   # ranges per pipeline: the draws are attributed to them
    game.set32('CyberpunkVR_SceneGpuNodeFilter', node)
    game.set32('CyberpunkVR_SceneGpuDrawOcclusion', 1)
    try:
        before = game.u32('CyberpunkVR_SceneGpuReportSeq')
        game.set32('CyberpunkVR_SceneGpuViewportDetail', 1)
        game.set32('CyberpunkVR_SceneGpuFrameRequest', frames)
        deadline = time.monotonic() + 15
        value = {}
        while time.monotonic() < deadline:
            time.sleep(0.25)
            if game.u32('CyberpunkVR_SceneGpuReportSeq') == before:
                continue
            value = game.seqlock('CyberpunkVR_SceneGpuReport', 'CyberpunkVR_SceneGpuReportSeq', 262144)
            if value.get('mode') == 'frames' and not value.get('active') and value.get('completed'):
                break
        n = value.get('frames') or 0
        return dict(node=hex(node), frames=n, eyes=draw_occlusion_summary(value.get('occ') or {}, n),
                    byPipeline=draw_occlusion_pipelines(value.get('groups', []), n))
    finally:
        game.set32('CyberpunkVR_SceneGpuGroupMode', 1)
        game.set32('CyberpunkVR_SceneGpuDrawOcclusion', 0)
        game.set32('CyberpunkVR_SceneGpuNodeFilter', 0)
        game.set32('CyberpunkVR_SceneGpuViewportDetail', 0)


CENSUS_MARKER = PLUGIN_LOG / 'cvr_shader_census.txt'
CENSUS_KINDS = {1: 'dispatch', 2: 'executeIndirect', 3: 'draw', 4: 'drawIndexed', 5: 'dispatchRays'}


def census(game, seconds=0.5, mode=1):
    """The shader census (src/Render/HiddenAreaShaders.cpp) for `seconds`: per (node, view, kind, size, shader) the
    command count. Needs CENSUS_MARKER at game start (run --census)."""
    if not game.u32('CyberpunkVR_ShaderCensusRecording'):
        return dict(error='census marker was not present at start')
    size = 512 * 48
    game.write(game.addr('CyberpunkVR_ShaderCensusLog'), bytes(size))
    game.set32('CyberpunkVR_ShaderCensus', mode)
    time.sleep(seconds)
    game.set32('CyberpunkVR_ShaderCensus', 0)
    time.sleep(0.1)
    raw = game.read(game.addr('CyberpunkVR_ShaderCensusLog'), size)
    names = node_names()
    rows = []
    for i in range(512):
        node, side, kind, x, y, z, h, count, nbytes, pso = struct.unpack_from('<IiIIIIQIIQ', raw, 48 * i)
        if not count:
            break
        rows.append(dict(node=hex(node), name=names.get(node, '?'), side=side, kind=CENSUS_KINDS.get(kind, kind),
                         size=[x, y, z], shader=f'{h:016x}', count=count, pso=hex(pso)))
    return dict(seconds=seconds, rows=rows)


# ---- L/R pair probe (src/Stereo/StereoPairProbe.cpp) -----------------------------------------------------------
PAIR_MARKER = PLUGIN_LOG / 'cvr_pair_probe.txt'
PAIR_TYPES = {0: '-', 1: 'table', 2: 'cbv', 3: 'srv', 4: 'uav', 5: 'const'}
PAIR_PARAMS = 40
PAIR_FORMAT = '<5Q40Q40B3Ii6III'   # list pso root ib vb, params[40], types[40], node serial kind, vpx, vpw args[5] seq pad


def pair_records(game, frames=2, node=None, timeout=20):
    names = ['CyberpunkVR_PairProbeRequest', 'CyberpunkVR_PairProbeState', 'CyberpunkVR_PairProbeNode',
             'CyberpunkVR_PairProbeCount', 'CyberpunkVR_PairProbeRecords', 'CyberpunkVR_PairProbeRecordBytes']
    from live_symbols import resolve
    game.symbols.update(resolve(game.pid, DLL, names))
    size = game.u32('CyberpunkVR_PairProbeRecordBytes')
    if size != struct.calcsize(PAIR_FORMAT):
        raise RuntimeError(f'pair record size {size} != {struct.calcsize(PAIR_FORMAT)}')
    if node is not None:
        game.set32('CyberpunkVR_PairProbeNode', node)
    game.set32('CyberpunkVR_PairProbeRequest', frames)
    deadline = time.monotonic() + timeout
    state = 0
    while time.monotonic() < deadline:
        state = game.u32('CyberpunkVR_PairProbeState')
        if state in (3, 4):
            break
        time.sleep(0.1)
    if state != 3:
        return dict(error=f'pair probe state {state} (4: the marker was not present at start)')
    count = min(game.u32('CyberpunkVR_PairProbeCount'), 16384)
    raw = game.read(game.addr('CyberpunkVR_PairProbeRecords'), count * size)
    rows = []
    for i in range(count):
        v = struct.unpack_from(PAIR_FORMAT, raw, i * size)
        n = 5 + 2 * PAIR_PARAMS
        rows.append(dict(list=v[0], pso=v[1], root=v[2], ib=v[3], vb=v[4], params=list(v[5:5 + PAIR_PARAMS]),
                         types=list(v[5 + PAIR_PARAMS:n]), node=v[n], serial=v[n + 1], kind=v[n + 2], vpx=v[n + 3],
                         vpw=v[n + 4], args=list(v[n + 5:n + 10]), seq=v[n + 10]))
    return analyze_pairs(rows)


def analyze_pairs(rows):
    """Left batch + the right batch after it, per list; which fields differ draw by draw."""
    def eye(r):
        if r['vpx'] < 0:
            return '?'
        if r['vpx'] == 0:
            return 'L'
        return 'R' if abs(r['vpx'] - r['vpw']) <= 1 else 'O'
    lists = {}
    for r in rows:
        lists.setdefault((r['serial'], r['list']), []).append(r)
    out = dict(draws=len(rows), lists=len(lists), eyes={}, pairs=0, pairsEqual=0, compared=0, fieldDiff={},
               paramDiff={}, argDiff={}, argExamples=[], cameraDelta={}, rightKnownBeforeLeft=0, rightKnownChecked=0, patterns=[])
    for e in (eye(r) for r in rows):
        out['eyes'][e] = out['eyes'].get(e, 0) + 1
    for (serial, lst), seq in lists.items():
        seq.sort(key=lambda r: r['seq'])
        batches = []
        for r in seq:
            e = eye(r)
            if batches and batches[-1][0] == e:
                batches[-1][1].append(r)
            else:
                batches.append((e, [r]))
        if len(out['patterns']) < 12:
            out['patterns'].append(''.join(f'{e}{len(b)} ' for e, b in batches)[:200])
        # A view-instanced draw needs both eyes' camera tables at once: how often does each table change inside
        # one eye's batch (0 = one handle for the whole batch)?
        for e, b in batches:
            if len(b) > 1:
                for p in range(len(b[0]['params'])):
                    if not any(r['types'][p] == 1 for r in b):
                        continue
                    changes = sum(1 for x, y in zip(b, b[1:]) if x['types'][p] == y['types'][p] == 1 and
                                  x['params'][p] != y['params'][p])
                    c_ = out.setdefault('withinBatch', {}).setdefault(f'{e}{p}', dict(batches=0, draws=0, changes=0))
                    c_['batches'] += 1
                    c_['draws'] += len(b)
                    c_['changes'] += changes
        seen_cbv = set()   # every CBV address bound so far in this list
        i = 0
        while i < len(batches):
            e, b = batches[i]
            for r in b:
                for p in range(len(r['params'])):
                    if r['types'][p] == 2:
                        seen_cbv.add(r['params'][p])
            if i + 1 < len(batches) and e == 'L' and batches[i + 1][0] == 'R':
                a, c = b, batches[i + 1][1]
                out['pairs'] += 1
                if len(a) == len(c):
                    out['pairsEqual'] += 1
                    # Single pass needs the right draw's data from the left one: is each table handle and the start
                    # instance the left's plus ONE offset for the whole batch?
                    deltas = {}
                    for x, y in zip(a, c):
                        for p in range(len(x['params'])):
                            if x['types'][p] == 1 and y['types'][p] == 1:
                                deltas.setdefault(f'table{p}', set()).add(y['params'][p] - x['params'][p])
                        k = 4 if x['kind'] == 2 else 3
                        deltas.setdefault('startInstance', set()).add(y['args'][k] - x['args'][k])
                    for key, values in deltas.items():
                        c_ = out.setdefault('batchDelta', {}).setdefault(key, dict(constant=0, batches=0, draws=0,
                                                                                    examples=[]))
                        c_['batches'] += 1
                        c_['draws'] += len(a)
                        if len(values) == 1:
                            c_['constant'] += 1
                            if len(c_['examples']) < 4:
                                c_['examples'].append(next(iter(values)))
                        elif len(c_['examples']) < 4:
                            c_['examples'].append(f'{len(values)} values')
                    for x, y in zip(a, c):
                        out['compared'] += 1
                        for key in ('pso', 'root', 'ib', 'vb', 'args', 'kind'):
                            if x[key] != y[key]:
                                out['fieldDiff'][key] = out['fieldDiff'].get(key, 0) + 1
                        # which native argument: DrawIndexedInstanced (index count, instances, start index, base
                        # vertex, start instance), DrawInstanced (vertices, instances, start vertex, start instance)
                        for k, (u, v) in enumerate(zip(x['args'], y['args'])):
                            if u != v:
                                tag = f"{x['kind']}.{k}"
                                out['argDiff'][tag] = out['argDiff'].get(tag, 0) + 1
                                if len(out['argExamples']) < 16:
                                    out['argExamples'].append(dict(kind=x['kind'], left=x['args'], right=y['args']))
                        for p in range(len(x['params'])):
                            tx, ty = x['types'][p], y['types'][p]
                            if tx != ty or x['params'][p] != y['params'][p]:
                                tag = f'{p}:{PAIR_TYPES.get(tx or ty)}'
                                out['paramDiff'][tag] = out['paramDiff'].get(tag, 0) + 1
                                if tx == ty == 2:
                                    d = str(y['params'][p] - x['params'][p])
                                    out['cameraDelta'][d] = out['cameraDelta'].get(d, 0) + 1
                                    out['rightKnownChecked'] += 1
                                    if y['params'][p] in seen_cbv:
                                        out['rightKnownBeforeLeft'] += 1
                i += 2
                for r in c:
                    for p in range(len(r['params'])):
                        if r['types'][p] == 2:
                            seen_cbv.add(r['params'][p])
            else:
                i += 1
    out['cameraDelta'] = dict(sorted(out['cameraDelta'].items(), key=lambda kv: -kv[1])[:12])
    return out


# ---- GPU queue timeline (src/Render/GpuQueueTimeline.cpp) ------------------------------------------------------
def timeline_request(game, submissions=4000):
    if game.u32('CyberpunkVR_GpuTimelineState') in (1, 2):
        raise RuntimeError('a GPU timeline capture is already running')
    game.set32('CyberpunkVR_GpuTimelineRequest', submissions)


def timeline_idle(raw_path):
    """Idle GPU time per frame split by tools/hitch_tests/gpu_queue_timeline.py: 'starved' (the next batch reached
    the driver after the GPU went idle: the CPU was late) and 'held' (submitted earlier, started later: a fence or
    the driver). Returns the means (ms per frame) and the analyser's report text."""
    import contextlib
    import io
    sys.path.insert(0, str(ROOT / 'tools/hitch_tests'))
    try:
        import gpu_queue_timeline
        text = io.StringIO()
        with contextlib.redirect_stdout(text):
            frames = gpu_queue_timeline.analyse(str(raw_path), 0)
    finally:
        sys.path.pop(0)
    if not frames:
        return dict(error='too few frames', report=text.getvalue())
    mean = lambda key: statistics.mean(f[key] for f in frames)
    return dict(frames=len(frames), frame=mean('length'), busy=mean('busy'), idle=mean('idle'), starved=mean('starved'),
                held=mean('held'), gaps=mean('gaps'), report=text.getvalue())


def etw_summary(pending, pid):
    """CPU ms per frame of the game's process in the sampled window, by module, thread and port function (one sample
    = 1 ms on one core, calibrated: a 3.0 s busy loop gave 3015). The trace is deleted afterwards."""
    etl, frames = pending['etl'], pending['frames']
    try:
        v = etw_cpu.analyse(etl, pid=pid, symbols=DLL, window=pending['window'],
                            exe_functions=EXE_FUNCTIONS if EXE_FUNCTIONS.exists() else None)
    except Exception as error:   # diagnostics only
        return dict(error=str(error), trajectory=pending['trajectory'])
    finally:
        Path(etl).unlink(missing_ok=True)
        shutil.rmtree(str(etl) + '.NGENPDB', ignore_errors=True)   # wpr's managed-code symbol folder, ~50 MB
    per = (lambda n: n / frames) if frames else (lambda n: 0.0)
    return dict(trajectory=pending['trajectory'], frames=frames, seconds=v['seconds'], samples=v['samples'],
                cpuMsPerFrame=per(v['samples']),
                modules=[(m, round(per(n), 3)) for m, n in v['modules']],
                threads=[(t, round(per(n), 3)) for t, n in v['threads']],
                functions=[(f, round(per(n), 3)) for f, n in v['functions']],
                exeFunctions=[(r, f, round(per(n), 3)) for r, f, n in v.get('exeFunctions', [])],
                exeByThread=[(th, r, f, round(per(n), 3)) for th, r, f, n in v.get('exeByThread', [])])


def timeline_collect(game, timeout=15, raw_path=None):
    deadline = time.monotonic() + timeout
    state = 0
    while time.monotonic() < deadline:
        state = game.u32('CyberpunkVR_GpuTimelineState')
        if state in (3, 4):
            break
        time.sleep(0.1)
    if state != 3:
        return dict(error=f'timeline state {state}')
    count = game.u32('CyberpunkVR_GpuTimelineCount')
    calib = struct.unpack('<8Q', game.read(game.addr('CyberpunkVR_GpuTimelineCalib'), 64))
    raw = game.read(game.addr('CyberpunkVR_GpuTimelineRecords'), count * 48)
    records = [struct.unpack_from('<6Q', raw, 48 * i) for i in range(count)]
    value = timeline_frames(records, calib)
    if raw_path and not value.get('error'):
        raw_path.parent.mkdir(parents=True, exist_ok=True)
        raw_path.write_text(json.dumps(dict(calib=list(calib), records=[list(r) for r in records])), encoding='utf-8')
        try:
            value['idle'] = timeline_idle(raw_path)
        except Exception as error:   # diagnostics only
            value['idle'] = dict(error=str(error))
    return value


def timeline_frames(records, calib):
    """Per frame (between consecutive Presents): GPU busy ms (union of every queue's batch intervals) and the
    window length. A long window with normal busy time is the CPU (or a fence) holding the GPU, not GPU work."""
    qpcf = calib[6]
    if not qpcf:
        return dict(error='no calibration')

    def to_ms(kind, gpu):
        freq, g0, c0 = calib[kind * 3], calib[kind * 3 + 1], calib[kind * 3 + 2]
        return ((gpu - g0) / freq + c0 / qpcf) * 1000.0 if freq else None
    intervals, presents = [], []
    for r in records:
        kind = r[0] & 0xFF
        if kind == 2:
            presents.append(r[2] * 1000.0 / qpcf)
        elif kind in (0, 1) and r[4] and r[5]:
            b, e = to_ms(kind, r[4]), to_ms(kind, r[5])
            if b is not None and e is not None and e >= b:
                intervals.append((b, e))
    intervals.sort()
    frames = []
    for a, b in zip(presents, presents[1:]):
        busy, cur_b, cur_e = 0.0, None, None
        for s, e in intervals:
            s, e = max(s, a), min(e, b)
            if e <= s:
                continue
            if cur_e is None or s > cur_e:
                if cur_e is not None:
                    busy += cur_e - cur_b
                cur_b, cur_e = s, e
            else:
                cur_e = max(cur_e, e)
        if cur_e is not None:
            busy += cur_e - cur_b
        frames.append(dict(window=b - a, busy=busy))
    if not frames:
        return dict(error='no whole frames in the timeline')
    busy = sorted(f['busy'] for f in frames)
    window = sorted(f['window'] for f in frames)
    pick = lambda v, p: v[min(len(v) - 1, int(round(p * (len(v) - 1))))]
    worst = sorted(frames, key=lambda f: -f['window'])[:5]
    return dict(frames=len(frames), busyP50=pick(busy, .5), busyP95=pick(busy, .95), busyP99=pick(busy, .99),
                busyMax=busy[-1], windowP50=pick(window, .5), windowP99=pick(window, .99),
                worst=[dict(window=round(f['window'], 2), busy=round(f['busy'], 2)) for f in worst])


# ---- stereo captures --------------------------------------------------------------------------------------------
# Stereo under other eye geometry (goal: different IPDs, FOVs and asymmetric projections). The simulator applies these
# live (its mcp_integration.h: ipd_command.json {"ipd_mm"}, fov_command.json {"left"/"right": {aL, aR, aU, aD}} in
# radians, headset_profile_command.json {"name"} restores a profile's FOV and IPD).
STEREO_SWEEP = [
    ('ipd58', dict(ipd=58)),
    ('ipd70', dict(ipd=70)),
    ('ipd64', dict(ipd=64)),
    ('fov-symmetric', dict(fov={'left': dict(aL=-0.85, aR=0.85, aU=0.85, aD=-0.95),
                                'right': dict(aL=-0.85, aR=0.85, aU=0.85, aD=-0.95)})),
    ('fov-wide-asym', dict(fov={'left': dict(aL=-1.0, aR=0.75, aU=0.9, aD=-1.0),
                                'right': dict(aL=-0.75, aR=1.0, aU=0.9, aD=-1.0)})),
]


def stereo_sweep(game, base, out, settle=3.0):
    """Each STEREO_SWEEP geometry on the still head: one stereo capture (stereo_check), then the Quest 3 profile back."""
    rows = {}
    try:
        for name, config in STEREO_SWEEP:
            if 'ipd' in config:
                sim_send('ipd_command.json', json.dumps({'ipd_mm': config['ipd']}))
            if 'fov' in config:
                sim_send('fov_command.json', json.dumps(config['fov']))
            time.sleep(settle)
            sim_send('head_pose_command.json', json.dumps(dict(x=base['x'], y=base['y'], z=base['z'], yaw=base['yaw'],
                                                                pitch=base['pitch'], roll=base['roll'])))
            time.sleep(1.0)
            capture = arm_capture(game)
            value = collect_capture(game, capture, out / 'stereo' / f'sweep-{name}')
            value['config'] = config
            rows[name] = value
            s = value.get('leftToRight') or {}
            log(f'  sweep {name}: separation {value.get("separation", 0) * 1000:.1f} mm  agreement '
                f'{fmt(s.get("geometricAgreement"), 3)}  mean diff {fmt(s.get("meanDiff"))}  depth error p95 '
                f'{fmt(s.get("depthErrorP95"), 5)}' if s else f'  sweep {name}: {value.get("error")}')
    finally:
        sim_send('headset_profile_command.json', json.dumps({'name': 'quest3'}))
        time.sleep(settle)
    return rows


def arm_capture(game):
    game.set32('CyberpunkVR_FramegenCapture', 1)
    return time.time()


def collect_capture(game, armed, out_dir, timeout=20):
    """Waits for the capture armed at `armed`, runs stereo_check into out_dir, deletes the raw frames."""
    deadline = time.monotonic() + timeout
    state = 1
    while time.monotonic() < deadline:
        state = game.i32('CyberpunkVR_FramegenCapture')
        if state in (2, -1):
            break
        time.sleep(0.1)
    game.set32('CyberpunkVR_FramegenCapture', 0)
    if state != 2:
        return dict(error=f'capture state {state}')
    root = PLUGIN_LOG / 'framegen-capture'
    found = [d for d in root.iterdir() if d.is_dir() and d.stat().st_mtime >= armed - 1] if root.exists() else []
    if not found:
        return dict(error='no capture folder')
    raw = max(found, key=lambda d: d.stat().st_mtime)
    out_dir.mkdir(parents=True, exist_ok=True)
    result = subprocess.run([str(STEREO_CHECK), str(raw), str(out_dir)], capture_output=True, text=True, timeout=120)
    try:
        value = json.loads(result.stdout)
    except ValueError:
        value = dict(error=(result.stderr or result.stdout).strip()[:400])
    shutil.copy2(raw / 'capture.json', out_dir / 'capture.json')
    shutil.rmtree(raw, ignore_errors=True)   # ~300 MB of raw frames: the numbers and pictures are kept
    return value


def prune_stereo(keep=KEEP_STEREO):
    """Keeps the newest `keep` stereo folders over all runs, deletes the rest (no capture hoarding)."""
    if not OUT_ROOT.exists():
        return 0
    folders = sorted((d for d in OUT_ROOT.glob('*/stereo/*') if d.is_dir()), key=lambda d: d.stat().st_mtime, reverse=True)
    for d in folders[keep:]:
        shutil.rmtree(d, ignore_errors=True)
    return max(0, len(folders) - keep)


# ---- the launch -------------------------------------------------------------------------------------------------
def live(step, state, *extra, timeout=400):
    cmd = [sys.executable, '-B', str(LIVE), step, '--state', str(state), *extra]
    log('live ' + step)
    result = subprocess.run(cmd, capture_output=True, text=True, errors='replace', timeout=timeout)
    for line in (result.stdout or '').splitlines()[-6:]:
        print('   ' + line, flush=True)
    if result.returncode:
        for line in (result.stderr or '').splitlines()[-12:]:
            print('   ! ' + line, flush=True)
        raise RuntimeError(f'live {step} failed ({result.returncode})')
    return result.stdout


USER_SETTINGS = Path(os.environ['LOCALAPPDATA']) / 'CD Projekt Red/Cyberpunk 2077/UserSettings.json'


def apply_settings(overrides):
    """Game settings for one launch: ['ScreenSpaceReflectionsQuality=High', ...] -> the file's original bytes (restore
    them with restore_settings once the game is closed; the game also writes this file on exit). A list option takes
    its value AND its index (the game falls back to the default when they disagree); a list the game fills at run time
    (no 'values' in the file, e.g. DLSS) takes both as 'Name=Value#index'."""
    if not overrides:
        return None
    original = USER_SETTINGS.read_bytes()
    data = json.loads(original)
    for item in overrides:
        name, _, value = item.partition('=')
        hits = [o for g in data['data'] for o in g['options'] if o['name'] == name]
        if len(hits) != 1:
            raise SystemExit(f'setting {name}: {len(hits)} matches')
        option = hits[0]
        if 'values' in option:
            if value not in option['values']:
                raise SystemExit(f'setting {name}: {value} not in {option["values"]}')
            option['value'], option['index'] = value, option['values'].index(value)
        elif option.get('type') == 'string_list':
            text, hash_, index = value.rpartition('#')
            if not hash_ or not index.isdigit():
                raise SystemExit(f'setting {name}: a run-time list needs Value#index')
            option['value'], option['index'] = text, int(index)
        elif isinstance(option.get('value'), bool):
            option['value'] = value.lower() in ('1', 'true', 'on')
        elif isinstance(option.get('value'), (int, float)):
            option['value'] = type(option['value'])(value)
        else:
            option['value'] = value
    USER_SETTINGS.write_text(json.dumps(data, indent=2), encoding='utf-8')
    log('settings for this launch: ' + ', '.join(overrides))
    return original


def restore_settings(original):
    if original is not None:
        USER_SETTINGS.write_bytes(original)
        log('game settings restored')


def game_running():
    from launch_to_save import process_ids
    return bool(process_ids('Cyberpunk2077.exe'))


def ini_values(keys):
    out = {}
    try:
        for line in INI.read_text(encoding='utf-8', errors='replace').splitlines():
            k, _, v = line.partition('=')
            if k.strip() in keys:
                out[k.strip()] = v.strip()
    except OSError:
        pass
    return out


def sha(path):
    import hashlib
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def preflight():
    problems = []
    if game_running():
        problems.append('the game is running: close it first (one owned launch per run)')
    if not STEREO_CHECK.exists():
        problems.append('stereo_check.exe is not built (tools/vr_bench/build_stereo_check.bat)')
    try:
        if b'head_trajectory' not in SIM_DLL.read_bytes():
            problems.append(f'{SIM_DLL} has no head_trajectory support: rebuild the simulator')
    except OSError:
        problems.append(f'simulator DLL not found: {SIM_DLL}')
    if BUILT.exists() and DLL.exists() and sha(BUILT) != sha(DLL):
        problems.append('the installed plugin DLL differs from the build: run `vr_bench.py deploy` (or pass --any-dll)')
    renderdoc = [p for p in ('qrenderdoc.exe', 'renderdoccmd.exe') if _running(p)]
    if renderdoc:
        problems.append('RenderDoc is running (' + ', '.join(renderdoc) + '): never alongside the game')
    return problems


def _running(image):
    result = subprocess.run(['tasklist', '/FI', f'IMAGENAME eq {image}', '/NH'], capture_output=True, text=True)
    return image.lower() in result.stdout.lower()


def run(args):
    problems = [p for p in preflight() if not (args.any_dll and 'differs from the build' in p)]
    if problems:
        raise SystemExit('preflight:\n  ' + '\n  '.join(problems))
    stamp = datetime.datetime.now().strftime('%Y%m%d-%H%M%S')
    out = OUT_ROOT / f'{stamp}-{args.label}'
    out.mkdir(parents=True, exist_ok=True)
    state = out / 'launch-state.json'
    names = node_names()
    traj = [t.strip() for t in args.traj.split(',') if t.strip()]
    for t in traj:
        if t not in trajectories.TRAJECTORIES:
            raise SystemExit(f'unknown trajectory {t}')
    meta = dict(label=args.label, stamp=stamp, save=args.save, dll=sha(DLL), simulator=sha(SIM_DLL),
                ini=ini_values({'xr_engine_stereo', 'xr_afw', 'xr_osr', 'xr_framegen', 'xr_foveated_dlss', 'xr_vrs',
                                'xr_hidden_area', 'xr_fps_cap', 'xr_depth_submit'}),
                trajectories=traj, note=args.note)
    if meta['ini'].get('xr_framegen') not in (None, '0'):
        raise SystemExit('frame generation must be off for a base benchmark (xr_framegen=0)')
    if meta['ini'].get('xr_foveated_dlss') not in (None, '0') or meta['ini'].get('xr_vrs') not in (None, '0'):
        raise SystemExit('foveated rendering (xr_foveated_dlss / xr_vrs) must be off')
    result = dict(meta=meta, runs={}, profiles=None, stereo={})
    if args.telegram:
        telegram.send(f'vr_bench "{args.label}": старт ({args.save}, траектории: {", ".join(traj)})')
    launched = False
    game = None
    etw_pending = None
    etw_live = False
    pid = None
    settings_original = apply_settings(args.setting)
    try:
        if args.census:
            CENSUS_MARKER.write_text('vr_bench census\n', encoding='ascii')
        if args.pairs:
            PAIR_MARKER.write_text('vr_bench pairs\n', encoding='ascii')
        launch_args = ['--timeout', '300'] + (['--save', args.save] if args.save else [])
        live('launch', state, *launch_args, timeout=420)
        launched = True
        launch_state = json.loads(state.read_text(encoding='utf-8'))
        if launch_state.get('startup', {}).get('awaitingContinue'):
            live('continue', state, '--timeout', '90', timeout=150)
        pid = json.loads(state.read_text(encoding='utf-8'))['pid']
        game = Game(pid)
        if args.diagnostics:   # the frame report (repeat rate) needs it; it also turns on per-draw diagnostics
            game.set32('CyberpunkVR_RuntimeDiagnostics', 1)
        log(f'settling {args.settle:.0f} s (streaming)')
        time.sleep(args.settle)
        if args.timeline_on:
            # The timeline's first request builds its lists, query heaps and queue hooks; the window that carried it
            # ran at 25-26 FPS for seconds (SM 1190-1320 MHz) in every launch that had one, and at 43 without. Built
            # here, the warm-up below waits until the frame time recovers.
            timeline_request(game, 64)
            timeline_collect(game)
        meta['warmUp'] = wait_steady(game)
        base = sim_base()
        meta['base'] = base
        period = 1000.0 / float(sim_status().get('target_fps') or 90)
        meta['displayPeriodMs'] = period
        ident = int(time.time()) % 100000
        for name in traj:
          for attempt in range(2):
            ident += 1
            log(f'trajectory {name}' + (' (again)' if attempt else ''))
            sampler = GpuSampler()
            threads = {}
            etw = name == args.etw_cpu and not attempt and etw_pending is None
            if etw:
                etw_cpu.start()
                etw_live = True
                log('  ETW CPU sampling on (wpr)')
            report0 = game.frame_report()
            xr0 = xr_cycles(game)
            seconds = play(name, base, ident)
            c0, g0 = game.ring_counts()
            ft0 = etw_cpu.filetime_now()
            if name in args.timeline_on:
                timeline_request(game)
            sampler.start()
            tthread = threading.Thread(target=thread_cpu, args=(pid, max(1.0, seconds - 1.0), threads), daemon=True)
            tthread.start()
            capture = None
            if name in args.stereo_on:
                time.sleep(seconds * 0.5)
                capture = arm_capture(game)
            wait_done(ident, seconds + 10)
            c1, g1 = game.ring_counts()
            ft1 = etw_cpu.filetime_now()
            xr1 = xr_cycles(game)
            report1 = game.frame_report()
            gpu_util = sampler.stop()
            tthread.join(10)
            cpu = game.ring('CyberpunkVR_FrameRingCpu', c0, c1)
            gpu = game.ring('CyberpunkVR_FrameRingGpu', g0, g1)
            row = frame_stats(cpu, gpu, period)
            if etw:
                etw_pending = dict(etl=etw_cpu.stop(out / 'cpu.etl'), window=(ft0, ft1), frames=row.get('frames') or 0,
                                   trajectory=name)
                etw_live = False
                log('  ETW CPU sampling saved (analysed after the game closes)')
            row.update(seconds=seconds, peak=trajectories.peak_speed(name), gpuUtil=gpu_util, threads=threads)
            if xr0 and xr1 and xr1[0] > xr0[0]:
                row['displayCycles'] = xr1[0] - xr0[0]
                row['reprojection'] = (xr1[1] - xr0[1]) / (xr1[0] - xr0[0])
            if name in args.timeline_on:
                row['gpuTimeline'] = timeline_collect(game, raw_path=out / 'timeline' / f'{name}.json')
            if not attempt and stalled(row, seconds):
                if capture:
                    collect_capture(game, capture, out / 'stereo' / name)
                result.setdefault('stalls', []).append(dict(trajectory=name, frames=row.get('frames'), seconds=seconds,
                                                            gpuUtil=gpu_util, threads=threads))
                log(f'  STALL: {row.get("frames")} frames in a {seconds:.1f} s window -- measured again')
                continue
            if report0 and report1:
                rep = report1.get('repeated', 0) - report0.get('repeated', 0)
                sub = report1.get('real_submitted', 0) - report0.get('real_submitted', 0)
                row['repeatRate'] = rep / max(1, rep + sub)
            if capture:
                row['stereo'] = collect_capture(game, capture, out / 'stereo' / name)
                result['stereo'][name] = row['stereo']
            key = name if name not in result['runs'] else f'{name}#{sum(k.split("#")[0] == name for k in result["runs"]) + 1}'
            result['runs'][key] = row
            log(f'  {row.get("frames")} frames  fps {row.get("fps", 0):.1f}  cpu p50/p95/p99 '
                f'{row.get("cpuP50", 0):.2f}/{row.get("cpuP95", 0):.2f}/{row.get("cpuP99", 0):.2f}  gpu p50/p99 '
                f'{row.get("gpuP50", 0):.2f}/{row.get("gpuP99", 0):.2f}  util {gpu_util.get("utilization", 0):.0f}%')
            if name == 'still' and args.profile_frames:
                profiles = []
                for _ in range(args.profiles):
                    profiles.append(scene_profile(game, args.profile_frames))
                    time.sleep(0.3)
                (out / 'profiles-raw.json').write_text(json.dumps(profiles), encoding='utf-8')
                result['profiles'] = summarize_profiles(profiles, names)
                log(f'  GPU per node: {result["profiles"]["frames"]} frames profiled, '
                    f'{result["profiles"]["totals"].get("all", 0):.2f} ms/frame in marked ranges')
                for node in args.node_detail:
                    result.setdefault('nodeDetail', []).append(node_detail_profile(game, node, args.profile_frames))
                for node in args.draw_occlusion:
                    oc = draw_occlusion_profile(game, node, args.profile_frames)
                    result.setdefault('drawOcclusion', []).append(oc)
                    log(f'  draw occlusion {oc["node"]}: ' + '; '.join(
                        f'{eye} {v["drawsPerFrame"]:.0f} draws/frame, no visible sample {v["zeroShare"] * 100:.1f}%, '
                        f'1-64 samples {v["samples1to64Share"] * 100:.1f}%' for eye, v in sorted(oc['eyes'].items())))
                for node in args.pipeline_stats:
                    ps = pipeline_stats_profile(game, node, args.profile_frames)
                    result.setdefault('pipelineStats', []).append(ps)
                    log(f'  pipeline stats {ps["node"]}: ' + '; '.join(
                        f'{eye} prims {v["cInvocations"]:.0f} clipped {fmt((v["clippedShare"] or 0) * 100, 1)}% '
                        f'VS {v["vsInvocations"]:.0f} PS {v["psInvocations"]:.0f}' for eye, v in sorted(ps['eyes'].items())))
                result['portStages'] = port_stages(game)
                log(f'  port GPU stages: {result["portStages"]["perFrameUs"]:.0f} us/frame')
                if args.pairs:
                    result['pairs'] = pair_records(game, 2)
                    log(f'  pairs: {json.dumps({k: v for k, v in result["pairs"].items() if k != "patterns"})[:400]}')
                if args.census:
                    modes = [int(m) for m in args.census_modes.split(',') if m.strip()]
                    result['census'] = census(game, mode=modes[0])
                    log(f'  census mode {modes[0]}: {len(result["census"].get("rows", []))} rows')
                    for m in modes[1:]:
                        result[f'census{m}'] = census(game, mode=m)
                        log(f'  census mode {m}: {len(result[f"census{m}"].get("rows", []))} rows')
            time.sleep(0.6)
            break
        if args.stereo_sweep:
            result['stereoSweep'] = stereo_sweep(game, base, out)
        sim_send('head_pose_command.json', json.dumps(dict(x=base['x'], y=base['y'], z=base['z'], yaw=base['yaw'],
                                                            pitch=base['pitch'], roll=base['roll'])))
    finally:
        if game:
            try:
                game.set32('CyberpunkVR_FramegenCapture', 0)
                game.set32('CyberpunkVR_SceneGpuViewportDetail', 0)
            except OSError:
                pass
            game.close()
        if launched or state.exists():
            try:
                live('close', state, timeout=120)
            except Exception as error:
                log(f'close failed: {error}')
        restore_settings(settings_original)
        if etw_live:   # a failure between wpr -start and -stop: never leave the profiler running
            subprocess.run(['wpr.exe', '-cancel'], capture_output=True)
        (out / 'run.json').write_text(json.dumps(result, indent=1), encoding='utf-8')
        if args.pairs:
            PAIR_MARKER.unlink(missing_ok=True)
        if args.census:   # never leave the marker: it keeps every shader's bytecode in memory on the next launch
            CENSUS_MARKER.unlink(missing_ok=True)
            dumps = PLUGIN_LOG / 'cvr_shaders'
            if dumps.exists() and not game_running():   # every shader the census saw: kept with the run
                shutil.move(str(dumps), str(out / 'cvr_shaders'))
    if etw_pending:
        result['etwCpu'] = etw_summary(etw_pending, pid)
        (out / 'run.json').write_text(json.dumps(result, indent=1), encoding='utf-8')
    pruned = prune_stereo()
    summary = render_summary(result)
    (out / 'summary.md').write_text(summary, encoding='utf-8')
    print(summary)
    log(f'results: {out}  (pruned {pruned} old stereo folders)')
    if args.telegram:
        telegram.send(summary)
        shot = next((out / 'stereo' / n / 'SBS.jpg' for n in ('still',) + tuple(result['stereo'])
                     if (out / 'stereo' / n / 'SBS.jpg').exists()), None)
        if shot:
            telegram.photo(shot, f'{args.label}: левый | правый глаз (каждый в своём аспекте)')
    return out


# ---- reports ----------------------------------------------------------------------------------------------------
def fmt(v, digits=2):
    return '-' if v is None else f'{v:.{digits}f}'


def render_summary(result):
    m = result['meta']
    lines = [f'vr_bench {m["label"]} {m["stamp"]} | {m.get("save")} | DLL {m["dll"][:12]}',
             'ini: ' + ' '.join(f'{k}={v}' for k, v in sorted(m['ini'].items())), '',
             'trajectory    fps   cpu p50/p95/p99 ms    gpu p50/p95/p99 ms   1%low  0.1%low  >1per  util  GT%  repro%']
    for s in result.get('stalls', []):
        lines.append(f'STALL {s["trajectory"]}: {s["frames"]} frames in {s["seconds"]:.1f} s (window measured again)')
    for name, r in result['runs'].items():
        th = r.get('threads') or {}
        lines.append(f'{name:12s} {fmt(r.get("fps"), 1):>5} {fmt(r.get("cpuP50"))}/{fmt(r.get("cpuP95"))}/'
                     f'{fmt(r.get("cpuP99"))}   {fmt(r.get("gpuP50"))}/{fmt(r.get("gpuP95"))}/{fmt(r.get("gpuP99"))}  '
                     f'{fmt(r.get("low1"), 1):>6} {fmt(r.get("low01"), 1):>7} {r.get("overPeriod", "-"):>5}  '
                     f'{fmt((r.get("gpuUtil") or {}).get("utilization"), 0):>4}  {fmt(th.get("gameThread"), 0):>3}  '
                     f'{fmt(r["reprojection"] * 100 if r.get("reprojection") is not None else None, 1):>5}')
        tl = r.get('gpuTimeline') or {}
        if tl.get('frames'):
            lines.append(f'{"":12s}   GPU busy/frame p50/p95/p99/max {fmt(tl["busyP50"])}/{fmt(tl["busyP95"])}/{fmt(tl["busyP99"])}/'
                         f'{fmt(tl["busyMax"])} ms over {tl["frames"]} frames; longest frames (window/busy): '
                         + ', '.join(f'{w["window"]}/{w["busy"]}' for w in tl['worst']))
            idle = tl.get('idle') or {}
            if idle.get('frames'):
                lines.append(f'{"":12s}   GPU idle/frame {fmt(idle["idle"])} ms: starved (CPU late) {fmt(idle["starved"])}, '
                             f'held (fence/driver) {fmt(idle["held"])}, {fmt(idle["gaps"], 1)} gaps')
    p = result.get('profiles')
    if p and p.get('frames'):
        t = p['totals']
        lines += ['', f'GPU по узлам (ms/кадр, {p["frames"]} кадров): всего {fmt(t["all"])}  L {fmt(t["L"])}  '
                      f'R {fmt(t["R"])}  весь кадр {fmt(t["F"])}  compute/clear {fmt(t["C"])}']
        for r in p['nodes'][:18]:
            lines.append(f'  {r["name"][:30]:30s} {fmt(r["total"])}  (L {fmt(r["L"])} R {fmt(r["R"])} '
                         f'F {fmt(r["F"])} C {fmt(r["C"])})')
    if result.get('stereo'):
        lines += ['', 'стерео (L->R / R->L): геом. согласие, ср. разн., p95, доля >32/255, внутр. край']
        for name, s in result['stereo'].items():
            if 'error' in s:
                lines.append(f'  {name}: {s["error"]}')
                continue
            a, b = s['leftToRight'], s['rightToLeft']
            lines.append(f'  {name:10s} {a["geometricAgreement"]:.3f}/{b["geometricAgreement"]:.3f}  '
                         f'{a["meanDiff"]:.2f}/{b["meanDiff"]:.2f}  {a["p95"]:.1f}/{b["p95"]:.1f}  '
                         f'{a["over32"] * 100:.2f}%/{b["over32"] * 100:.2f}%  {a["innerEdgeMean"]:.2f}/{b["innerEdgeMean"]:.2f}')
    sweep = result.get('stereoSweep')
    if sweep:
        lines += ['', 'стерео при другой геометрии глаз (неподвижная голова): база, согласие L->R/R->L, ср. разн., '
                      'ошибка глубины p95']
        for name, s in sweep.items():
            if 'error' in s:
                lines.append(f'  {name}: {s["error"]}')
                continue
            a, b = s['leftToRight'], s['rightToLeft']
            lines.append(f'  {name:14s} {s["separation"] * 1000:5.1f} mm  {a["geometricAgreement"]:.3f}/'
                         f'{b["geometricAgreement"]:.3f}  {a["meanDiff"]:.2f}/{b["meanDiff"]:.2f}  '
                         f'{a["depthErrorP95"]:.5f}/{b["depthErrorP95"]:.5f}')
    e = result.get('etwCpu')
    if e:
        if e.get('error'):
            lines += ['', f'CPU (ETW): {e["error"]}']
        else:
            lines += ['', f'CPU процесса игры (ETW, {e["trajectory"]}, {e["frames"]} кадров): {fmt(e["cpuMsPerFrame"])} '
                          f'мс CPU на кадр (сумма по ядрам)']
            lines.append('  модули: ' + ', '.join(f'{m} {v:.2f}' for m, v in e['modules'][:10]))
            lines.append('  потоки: ' + ', '.join(f'{t} {v:.2f}' for t, v in e['threads'][:8]))
            if e['functions']:
                lines.append('  функции порта:')
                for f, v in e['functions'][:15]:
                    lines.append(f'    {v:6.3f}  {f[:90]}')
    return '\n'.join(lines) + '\n'


def compare(args):
    a = json.loads((Path(args.a) / 'run.json').read_text(encoding='utf-8'))
    b = json.loads((Path(args.b) / 'run.json').read_text(encoding='utf-8'))
    lines = [f'сравнение: {a["meta"]["label"]} -> {b["meta"]["label"]}', '',
             'trajectory    cpu p50        cpu p99        gpu p50        gpu p99        fps']
    for name in a['runs']:
        if name not in b['runs']:
            continue
        ra, rb = a['runs'][name], b['runs'][name]
        def d(key):
            x, y = ra.get(key), rb.get(key)
            if x is None or y is None:
                return '-'
            return f'{x:.2f}->{y:.2f}({(y - x) / x * 100:+.0f}%)'
        lines.append(f'{name:12s} {d("cpuP50"):14s} {d("cpuP99"):14s} {d("gpuP50"):14s} {d("gpuP99"):14s} {d("fps")}')
    pa, pb = a.get('profiles') or {}, b.get('profiles') or {}
    if pa.get('nodes') and pb.get('nodes'):
        rows_b = {r['node']: r for r in pb['nodes']}
        lines += ['', 'GPU узлы (ms/кадр), наибольшие изменения:']
        deltas = []
        for r in pa['nodes']:
            other = rows_b.get(r['node'])
            if other:
                deltas.append((other['total'] - r['total'], r['name'], r['total'], other['total']))
        for delta, name, x, y in sorted(deltas, key=lambda v: v[0])[:10] + sorted(deltas, key=lambda v: -v[0])[:5]:
            lines.append(f'  {name[:30]:30s} {x:.3f} -> {y:.3f} ({delta:+.3f})')
        lines.append(f'  всего: {pa["totals"]["all"]:.2f} -> {pb["totals"]["all"]:.2f}')
    text = '\n'.join(lines) + '\n'
    print(text)
    if args.telegram:
        telegram.send(text)


def parse_variant(text):
    """'CyberpunkVR_A=1;CyberpunkVR_B=0' -> {'CyberpunkVR_A': 1, 'CyberpunkVR_B': 0}"""
    out = {}
    for part in text.split(';'):
        if part.strip():
            name, _, value = part.partition('=')
            out[name.strip()] = int(value, 0)
    return out


def counter_symbol(name):
    """'Name' or 'Name[i]' (the i-th 64-bit counter of an exported array) -> (symbol, index)."""
    if name.endswith(']') and '[' in name:
        base, index = name[:-1].split('[', 1)
        return base, int(index)
    return name, 0


def ab(args):
    """Same-launch A/B: the variants (sets of live 32-bit exports) take turns, `reps` times, each measured on the
    same trajectory with a GPU profile. One launch, so the scene, streaming and clocks are shared."""
    problems = [p for p in preflight() if not (args.any_dll and 'differs from the build' in p)]
    if problems:
        raise SystemExit('preflight:\n  ' + '\n  '.join(problems))
    variants = [parse_variant(v) for v in args.variant]
    if len(variants) < 2:
        raise SystemExit('give at least two --variant')
    names_needed = sorted({k for v in variants for k in v})
    stamp = datetime.datetime.now().strftime('%Y%m%d-%H%M%S')
    out = OUT_ROOT / f'{stamp}-ab-{args.label}'
    out.mkdir(parents=True, exist_ok=True)
    state = out / 'launch-state.json'
    labels = [args.labels[i] if args.labels and i < len(args.labels) else f'v{i}' for i in range(len(variants))]
    result = dict(meta=dict(label=args.label, stamp=stamp, save=args.save, dll=sha(DLL), variants=variants,
                            labels=labels, traj=args.traj, reps=args.reps, note=args.note), rows=[])
    if args.telegram:
        telegram.send(f'vr_bench A/B "{args.label}": ' + ' vs '.join(f'{l}: {v}' for l, v in zip(labels, variants)))
    game = None
    launched = False
    pid = None
    etw_pending, etw_live = [], False
    settings_original = apply_settings(args.setting)
    try:
        if args.pairs:
            PAIR_MARKER.write_text('vr_bench pairs\n', encoding='ascii')
        live('launch', state, '--timeout', '300', *(['--save', args.save] if args.save else []), timeout=420)
        launched = True
        if json.loads(state.read_text(encoding='utf-8')).get('startup', {}).get('awaitingContinue'):
            live('continue', state, '--timeout', '90', timeout=150)
        pid = json.loads(state.read_text(encoding='utf-8'))['pid']
        global SYMBOLS
        primes = parse_variant(args.prime) if args.prime else {}
        counter_names = [counter_symbol(n)[0] for n in args.counters]
        SYMBOLS = SYMBOLS + [n for n in names_needed + counter_names + sorted(primes) if n not in SYMBOLS]
        game = Game(pid)
        original = {n: game.u32(n) for n in sorted(set(names_needed) | set(primes))}
        result['meta']['original'] = original
        if primes:   # e.g. pipelines a variant needs, built in the background before any window is measured
            for n, v in primes.items():
                game.set32(n, v)
            log(f'priming {primes} for {args.prime_seconds:.0f} s')
            time.sleep(args.prime_seconds)
            for n in primes:
                game.set32(n, original[n])
        log(f'settling {args.settle:.0f} s')
        time.sleep(args.settle)
        result['meta']['warmUp'] = wait_steady(game)
        base = sim_base()
        period = 1000.0 / float(sim_status().get('target_fps') or 90)
        ident = int(time.time()) % 100000 + 50000
        names = node_names()
        order = []
        for rep in range(args.reps):
            seq = list(range(len(variants)))
            order += seq if rep % 2 == 0 else seq[::-1]          # ABBA keeps drift out of the difference
        for i in order:
            # Every export any variant names: this variant's value, else the launch's own (a switch one variant
            # turns off must not stay off for the next).
            for n in names_needed:
                game.set32(n, variants[i].get(n, original[n]))
            time.sleep(args.switch_settle)
            for attempt in range(2):
                ident += 1
                if args.etw_cpu:
                    etw_cpu.start()
                    etw_live = True
                seconds = play(args.traj, base, ident)
                c0, g0 = game.ring_counts()
                ft0 = etw_cpu.filetime_now()
                counters0 = {n: game.u64(*counter_symbol(n)) for n in args.counters}
                capture = None
                if args.stereo:   # mid-trajectory with the head moving (the same point of the motion for every variant)
                    if args.traj != 'still':
                        time.sleep(seconds * 0.5)
                    capture = arm_capture(game)
                wait_done(ident, seconds + 10)
                c1, g1 = game.ring_counts()
                ft1 = etw_cpu.filetime_now()
                row = frame_stats(game.ring('CyberpunkVR_FrameRingCpu', c0, c1),
                                  game.ring('CyberpunkVR_FrameRingGpu', g0, g1), period)
                if args.etw_cpu:
                    etl = etw_cpu.stop(out / f'cpu-{len(result["rows"])}-{attempt}.etl')
                    etw_live = False
                    etw_pending.append(dict(etl=etl, window=(ft0, ft1), frames=row.get('frames') or 0,
                                            trajectory=args.traj, row=len(result['rows']), attempt=attempt))
                if attempt or not stalled(row, seconds):
                    break
                if capture:
                    collect_capture(game, capture, out / 'stereo' / f'{labels[i]}-stall')
                result.setdefault('stalls', []).append(dict(variant=i, label=labels[i], frames=row.get('frames'),
                                                            seconds=seconds))
                log(f'  STALL {labels[i]}: {row.get("frames")} frames in a {seconds:.1f} s window -- measured again')
            row.update(variant=i, label=labels[i], attempt=attempt,
                       counters={n: game.u64(*counter_symbol(n)) - v for n, v in counters0.items()})
            if args.port_stages:
                row['portStages'] = port_stages(game)
            if args.profile_frames:
                profile = scene_profile(game, args.profile_frames)
                row['profile'] = summarize_profiles([profile], names)
            if capture:
                row['stereo'] = collect_capture(game, capture, out / 'stereo' / f'{labels[i]}-{len(result["rows"])}')
            result['rows'].append(row)
            log(f'  {labels[i]:>10s}: gpu p50 {fmt(row.get("gpuP50"))} p99 {fmt(row.get("gpuP99"))}  cpu p50 '
                f'{fmt(row.get("cpuP50"))}  fps {fmt(row.get("fps"), 1)}  nodes '
                f'{fmt((row.get("profile") or {}).get("totals", {}).get("all"))}')
        for n, v in original.items():
            game.set32(n, v)
    finally:
        if game:
            game.close()
        if launched or state.exists():
            try:
                live('close', state, timeout=120)
            except Exception as error:
                log(f'close failed: {error}')
        restore_settings(settings_original)
        if etw_live:   # a failure between wpr -start and -stop: never leave the profiler running
            subprocess.run(['wpr.exe', '-cancel'], capture_output=True)
        if args.pairs:
            PAIR_MARKER.unlink(missing_ok=True)
        (out / 'ab.json').write_text(json.dumps(result, indent=1), encoding='utf-8')
    for pending in etw_pending:   # after the game closed: tracerpt and the symbol lookups off the measurement
        summary = etw_summary(pending, pid)
        rows = result['rows']
        if pending['row'] < len(rows) and pending['attempt'] == rows[pending['row']].get('attempt', pending['attempt']):
            rows[pending['row']]['etwCpu'] = summary
    if etw_pending:
        (out / 'ab.json').write_text(json.dumps(result, indent=1), encoding='utf-8')
    prune_stereo()
    text = render_ab(result)
    (out / 'summary.md').write_text(text, encoding='utf-8')
    print(text)
    if args.telegram:
        telegram.send(text)
    return out


def render_ab(result):
    m = result['meta']
    labels = m['labels']
    lines = [f'A/B {m["label"]} ({m["traj"]}, x{m["reps"]}) | DLL {m["dll"][:12]}', '']
    per = {i: [r for r in result['rows'] if r['variant'] == i] for i in range(len(labels))}

    def mean(rows, key):
        v = [r[key] for r in rows if r.get(key) is not None]
        return statistics.mean(v) if v else None

    def spread(rows, key):
        v = [r[key] for r in rows if r.get(key) is not None]
        return (min(v), max(v)) if v else (None, None)
    for s in result.get('stalls', []):
        lines.append(f'STALL {s["label"]}: {s["frames"]} frames in {s["seconds"]:.1f} s (window measured again)')
    lines.append('variant     gpu p50 (min..max)        gpu p99   cpu p50   fps')
    for i, label in enumerate(labels):
        lo, hi = spread(per[i], 'gpuP50')
        lines.append(f'{label:10s} {fmt(mean(per[i], "gpuP50"))} ({fmt(lo)}..{fmt(hi)})   {fmt(mean(per[i], "gpuP99"))}   '
                     f'{fmt(mean(per[i], "cpuP50"))}   {fmt(mean(per[i], "fps"), 1)}')
    profiled = {i: [r['profile'] for r in per[i] if r.get('profile', {}).get('frames')] for i in per}
    if all(profiled.values()):
        nodes = {}
        for i, profiles in profiled.items():
            for p in profiles:
                for r in p['nodes']:
                    nodes.setdefault(r['name'] + ' ' + r['node'], {}).setdefault(i, []).append(r['total'])
        base = 0
        deltas = []
        for key, by in nodes.items():
            a = statistics.mean(by.get(base, [0]))
            for i in range(1, len(labels)):
                b = statistics.mean(by.get(i, [0]))
                deltas.append((b - a, key, a, b, i))
        lines += ['', f'узлы с наибольшим изменением относительно {labels[0]} (ms/кадр):']
        for d, key, a, b, i in sorted(deltas, key=lambda v: -abs(v[0]))[:12]:
            lines.append(f'  {key[:40]:40s} {a:6.3f} -> {b:6.3f} ({d:+.3f}) [{labels[i]}]')
        for i in range(len(labels)):
            tot = [p['totals']['all'] for p in profiled[i]]
            lines.append(f'  всего {labels[i]}: {statistics.mean(tot):.2f}')
    if any(r.get('counters') for r in result['rows']):
        lines += ['', 'счётчики за окно варианта:']
        for r in result['rows']:
            lines.append(f'  {r["label"]}: ' + ', '.join(f'{k.replace("CyberpunkVR_", "")}={v}' for k, v in r['counters'].items()))
    if any(r.get('portStages') for r in result['rows']):
        lines += ['', 'GPU-стадии порта (мкс x раз):']
        for r in result['rows']:
            st = r.get('portStages') or {}
            lines.append(f'  {r["label"]}: ' + ', '.join(f'{s["name"]} {s["meanUs"]:.0f}x{s["count"]}' for s in st.get('stages', [])))
    etw = [r for r in result['rows'] if (r.get('etwCpu') or {}).get('modules')]
    if etw:
        lines += ['', 'CPU процесса игры по вариантам (ETW, мс CPU на кадр, сумма по ядрам, среднее по окнам):']
        focus = ('nvwgf2umx.dll', 'D3D12Core.dll', 'cyberpunkvrport.dll', 'Cyberpunk2077.exe', 'ntoskrnl.exe')
        for i, label in enumerate(labels):
            mine = [r['etwCpu'] for r in etw if r['variant'] == i]
            if not mine:
                continue
            total = statistics.mean(e['cpuMsPerFrame'] for e in mine)
            mods = {m: statistics.mean(dict(e['modules']).get(m, 0.0) for e in mine) for m in focus}
            lines.append(f'  {label:10s} всего {total:7.2f}  ' + '  '.join(f'{m.split(".")[0]} {v:6.2f}' for m, v in mods.items()))
    stereo = [r for r in result['rows'] if r.get('stereo') and 'leftToRight' in r['stereo']]
    if stereo:
        lines += ['', 'стерео (L->R геом. согласие / ср. разн.):']
        for r in stereo:
            s = r['stereo']['leftToRight']
            lines.append(f'  {r["label"]}: {s["geometricAgreement"]:.3f} / {s["meanDiff"]:.2f}')
    return '\n'.join(lines) + '\n'


DXC_SOURCE = Path(r'C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64')
# The exe's function bounds and names (IDA export of the same build): ETW samples inside the exe by function.
EXE_FUNCTIONS = ROOT / 'build/patchday/ida/2.5/functions.tsv'


def deploy_dxc():
    """dxcompiler.dll + dxil.dll beside the plugin (dxc/): single-pass stereo rewrites shaders in process
    (src/Render/ViewTwinCompiler.cpp)."""
    target = DLL.parent / 'dxc'
    target.mkdir(exist_ok=True)
    for name in ('dxcompiler.dll', 'dxil.dll'):
        source, dest = DXC_SOURCE / name, target / name
        if not source.exists():
            print(f'{source} missing: single pass stays off')
            continue
        if not dest.exists() or sha(source) != sha(dest):
            shutil.copy2(source, dest)
            print(f'{name} -> {dest}')


def deploy(_args):
    if game_running():
        raise SystemExit('close the game first')
    deploy_dxc()
    if sha(BUILT) == sha(DLL):
        print('installed DLL is the build already')
        return
    subprocess.run(['pwsh', '-NoProfile', '-File', str(ROOT / 'scripts/deploy_plugin.ps1'), '-GameRoot', str(GAME),
                    '-BuildDir', 'build/patchday/sdk25-build'], check=True)
    # The matching PDB beside the installed DLL: live symbol lookups (live_symbols.resolve searches the DLL's folder)
    # keep working after the build tree's PDB has moved on to a newer build.
    pdb = BUILT.with_suffix('.pdb')
    if pdb.exists():
        shutil.copy2(pdb, DLL.with_suffix('.pdb'))
        print(f'PDB copied beside the installed DLL: {DLL.with_suffix(".pdb")}')


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest='command', required=True)
    r = sub.add_parser('run')
    r.add_argument('--label', required=True)
    r.add_argument('--save', default='ManualSave-491')
    r.add_argument('--traj', default=','.join(trajectories.DEFAULT))
    r.add_argument('--stereo-on', default='still,yaw_slow,yaw_fast,translate_lr',
                   type=lambda s: [x for x in s.split(',') if x])
    r.add_argument('--settle', type=float, default=12.0)
    r.add_argument('--node-detail', default='', type=lambda s: [int(x, 0) for x in s.split(',') if x],
                   help='render-graph node RVAs profiled per pipeline on the still head')
    r.add_argument('--census', action='store_true', help='shader census on the still head (marker file at start)')
    r.add_argument('--census-modes', default='1', help='census modes in turn (1 screen passes, 2 instanced VS, '
                   '3 every draw by VS, 4 every draw by PS); the shaders seen are kept in cvr_shaders/')
    r.add_argument('--draw-occlusion', default='', type=lambda s: [int(x, 0) for x in s.split(',') if x],
                   help='render-graph node RVAs: an occlusion query per draw on the still head -- per eye the draws '
                        'that left no visible sample')
    r.add_argument('--pipeline-stats', default='', type=lambda s: [int(x, 0) for x in s.split(',') if x],
                   help='render-graph node RVAs: D3D12 pipeline statistics per eye on the still head (primitives the '
                        'clipper drops, VS / PS invocations)')
    r.add_argument('--pairs', action='store_true', help='L/R pair probe of RenderElements on the still head (marker at start)')
    r.add_argument('--timeline-on', default='still,snap90', type=lambda s: [x for x in s.split(',') if x],
                   help='trajectories with a GPU queue timeline (busy GPU ms per frame vs the frame window)')
    r.add_argument('--profile-frames', type=int, default=4)
    r.add_argument('--profiles', type=int, default=3)
    r.add_argument('--note', default='')
    r.add_argument('--setting', action='append', default=[], help='game setting for this launch, Name=Value (restored after)')
    r.add_argument('--diagnostics', action='store_true', help='CyberpunkVR_RuntimeDiagnostics=1 (frame report, repeat rate)')
    r.add_argument('--stereo-sweep', action='store_true',
                   help='after the trajectories: stereo checks at IPD 58/70/64 mm and a symmetric and a wide asymmetric FOV')
    r.add_argument('--etw-cpu', default='', help='trajectory sampled with wpr (CPU per module / thread / port function; '
                                                  'elevated shell)')
    r.add_argument('--any-dll', action='store_true', help='run even if the installed DLL is not the build')
    r.add_argument('--no-telegram', dest='telegram', action='store_false')
    c_ = sub.add_parser('compare')
    c_.add_argument('a')
    c_.add_argument('b')
    c_.add_argument('--no-telegram', dest='telegram', action='store_false')
    a = sub.add_parser('ab')
    a.add_argument('--label', required=True)
    a.add_argument('--variant', action='append', required=True,
                   help="live exports for one variant, 'Name=value;Name2=value' (repeat per variant)")
    a.add_argument('--labels', type=lambda s: s.split(','), default=None)
    a.add_argument('--save', default='ManualSave-491')
    a.add_argument('--traj', default='still')
    a.add_argument('--reps', type=int, default=3)
    a.add_argument('--settle', type=float, default=12.0)
    a.add_argument('--switch-settle', type=float, default=2.0)
    a.add_argument('--profile-frames', type=int, default=4)
    a.add_argument('--stereo', action='store_true')
    a.add_argument('--etw-cpu', action='store_true', help='CPU sampling (wpr) of every window, by module (elevated shell)')
    a.add_argument('--pairs', action='store_true', help='install the L/R pair probe (marker at start): its live switches')
    a.add_argument('--prime', default='', help='exports set during warm-up and restored before the first window '
                   '(as a --variant), e.g. to build pipelines a variant needs')
    a.add_argument('--prime-seconds', type=float, default=30.0)
    a.add_argument('--counters', default='', type=lambda s: [x for x in s.split(',') if x],
                   help='u64 exports whose change over each variant window is recorded')
    a.add_argument('--port-stages', action='store_true', help="the port's own GPU stages per variant")
    a.add_argument('--note', default='')
    a.add_argument('--setting', action='append', default=[], help='game setting for this launch, Name=Value (restored after)')
    a.add_argument('--any-dll', action='store_true')
    a.add_argument('--no-telegram', dest='telegram', action='store_false')
    sub.add_parser('deploy')
    args = parser.parse_args()
    dict(run=run, compare=compare, deploy=deploy, ab=ab)[args.command](args)


if __name__ == '__main__':
    main()
