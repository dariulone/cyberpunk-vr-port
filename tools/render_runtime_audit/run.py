"""Attach-only rendering evidence: markers, geometry, traffic/bike FG clips and owned fixtures.

Never launches/exits the game, switches runtimes, loads/saves, or opens replay.
The parent controller owns the game/input/save-guard lease for the entire call.
"""
import argparse
import ctypes as c
from ctypes import wintypes as w
import importlib.util
import hashlib
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import time
import uuid

ROOT = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(ROOT / 'tools/single_pass_tests'), str(ROOT / 'tools/sim_harness')]
import thread_stack_sampler as symbols
from simxr import Sim, SIM_DIR, _send

spec = importlib.util.spec_from_file_location('_corpse_audit_bridge', ROOT / 'tools/grasp_tests/corpse_live/run.py')
bridge_module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = bridge_module
spec.loader.exec_module(bridge_module)
Bridge = bridge_module.Bridge
lua_literal = bridge_module.lua_literal

FIELDS = {
    'CyberpunkVR_RuntimeDiagnostics': 'I', 'CyberpunkVR_WorldMarkerProbe': 'i',
    'CyberpunkVR_WorldMarkerRoots': 'Q', 'CyberpunkVR_WorldMarkerQuads': 'Q', 'CyberpunkVR_WorldMarkerTexts': 'Q',
    'CyberpunkVR_WorldMarkerCameraUploads': '2Q', 'CyberpunkVR_WorldMarkerWorldCameras': '2Q',
    'CyberpunkVR_WorldMarkerWorldVertices': 'Q', 'CyberpunkVR_WorldMarkerWorldTexts': 'Q',
    'CyberpunkVR_DebugEngineStereoEyes': '14f', 'CyberpunkVR_DebugEngineStereoEyeJitter': '8f',
    'CyberpunkVR_DebugAsymWindow': '8f', 'CyberpunkVR_DebugEngineStereoEyeBuilds': '2Q',
    'CyberpunkVR_DebugXrEndsWithLayer': 'Q', 'CyberpunkVR_DebugXrEndsEmpty': 'Q',
    'CyberpunkVR_XrCapturePrevious': 'Q', 'CyberpunkVR_XrCaptureNotDone': 'Q',
    'CyberpunkVR_XrSubmitResourceCache': '3Q', 'CyberpunkVR_FramegenInputStages': '10Q',
    'CyberpunkVR_XrRateLog': 'i',
    'g_launcherWidth': 'i', 'g_launcherHeight': 'i', 'CyberpunkVR_EngineStereo': 'i',
    'g_isDriving': 'B', 'cvr::anim::g_wheelMountedBike': 'B', 'g_menuModeValue': 'i',
}
EXTRA = ['CyberpunkVR_FramegenReport', 'CyberpunkVR_FramegenReportSeq', 'CyberpunkVR_FramegenCapture',
         'CyberpunkVR_XrCycleTrace', 'CyberpunkVR_XrCycleTraceCount', 'CyberpunkVR_WorldMarkerTraceSequence',
         'CyberpunkVR_WorldMarkerTraceWords', 'CyberpunkVR_WorldMarkerTraceFloats']


class Process:
    def __init__(self, pid):
        self.k = symbols.k
        self.k.ReadProcessMemory.argtypes = [w.HANDLE, c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]
        self.k.WriteProcessMemory.argtypes = self.k.ReadProcessMemory.argtypes
        self.k.CloseHandle.argtypes = [w.HANDLE]
        self.h = self.k.OpenProcess(0x438, False, pid)
        if not self.h:
            raise c.WinError(c.get_last_error())
        try:
            symbols.initialize_symbols(self.h)
            self.addresses = {name: symbols.resolve_named_symbol(self.h, 'cyberpunkvrport.dll', name) for name in list(FIELDS) + EXTRA}
            self.provenance = {k: v for k, v in symbols.initialize_symbols.state.items() if k != 'symbols'}
        except Exception:
            symbols.d.SymCleanup(self.h); self.k.CloseHandle(self.h); self.h = None
            raise

    def read(self, name, size):
        data = c.create_string_buffer(size); got = c.c_size_t()
        if not self.k.ReadProcessMemory(self.h, self.addresses[name], data, size, c.byref(got)) or got.value != size:
            raise c.WinError(c.get_last_error())
        return data.raw

    def value(self, name, fmt):
        value = struct.unpack('<' + fmt, self.read(name, struct.calcsize('<' + fmt)))
        return value[0] if len(value) == 1 else list(value)

    def write32(self, name, value):
        raw = struct.pack('<I', value & 0xffffffff); got = c.c_size_t()
        if not self.k.WriteProcessMemory(self.h, self.addresses[name], raw, len(raw), c.byref(got)) or got.value != len(raw):
            raise c.WinError(c.get_last_error())

    def locked(self, seq, fields):
        for _ in range(80):
            first = self.value(seq, 'I')
            if first & 1:
                time.sleep(.001); continue
            values = [self.read(name, size) for name, size in fields]
            if first == self.value(seq, 'I'):
                return values
        raise RuntimeError('Diagnostic publication remained busy')

    def report(self):
        raw = self.locked('CyberpunkVR_FramegenReportSeq', [('CyberpunkVR_FramegenReport', 2048)])[0]
        return json.loads(raw.split(b'\0', 1)[0])

    def trace(self, start):
        before = self.value('CyberpunkVR_XrCycleTraceCount', 'Q')
        raw = self.read('CyberpunkVR_XrCycleTrace', 8192 * 12 * 8)
        after = self.value('CyberpunkVR_XrCycleTraceCount', 'Q')
        # The producer writes slot [count % 8192] before publishing count+1.
        # Drop that possibly in-flight oldest slot even when count did not move.
        first = max(start, after - 8192 + 1)
        return {'start': start, 'end': before, 'dropped': max(0, first - start), 'counterReset': before < start,
                'readCompletedCount': after, 'counterChangedBackwards': after < before,
                'rows': [list(struct.unpack_from('<12d', raw, (i % 8192) * 96)) for i in range(first, before)]}

    def close(self):
        if self.h:
            symbols.d.SymCleanup(self.h); self.k.CloseHandle(self.h); self.h = None


def log_geometry(path):
    if not path.exists():
        return []
    # Initialization plus a bounded tail, never a complete multi-hundred-MB log.
    with path.open('rb') as source:
        first = source.read(512 * 1024)
        source.seek(max(0, path.stat().st_size - 256 * 1024))
        tail = source.read(256 * 1024)
    lines = (first + b'\n' + tail).decode('utf-8', 'replace').splitlines()
    return list(dict.fromkeys(line for line in lines if '[FOV]' in line or 'Mono submit resources ready' in line or 'Mono snapshot resource ready' in line))[-24:]


def xr_events(path, pid):
    if path is None:
        return [], 'no in-process xr_probe trace supplied'
    if not path.stem.endswith('-' + str(pid)):
        raise RuntimeError('xr_probe filename must belong to this exact PID')
    if time.time() - path.stat().st_mtime > 10:
        return [], 'xr_probe trace is stale'
    with path.open('rb') as source:
        offset = max(0, path.stat().st_size - 2 * 1024 * 1024)
        source.seek(offset)
        if offset:
            source.readline()
        lines = source.read(2 * 1024 * 1024).splitlines()
    found = {}
    for line in lines:
        try:
            event = json.loads(line)
        except ValueError:
            continue
        if event.get('ev') in ('locateViews', 'endFrame'):
            found[event['ev']] = event
    return list(found.values()), 'sampled API calls in this process, not frame-paired to engine diagnostics'


class Audit:
    def __init__(self, args):
        self.args = args; self.out = args.out.resolve(); self.out.mkdir(parents=True, exist_ok=False)
        self.plugin = args.game / 'red4ext/plugins/cyberpunkvrport'
        self.bridge = Bridge(self.plugin / 'lua/CVRBridge')
        self.process = Process(args.pid)
        self.saved = {}; self.sim = None; self.head = None; self.recording = False; self.owner = None
        self.fg_original = None; self.result = {'pid': args.pid, 'case': args.case, 'label': args.label,
                                             'runtimeLabel': args.runtime, 'artifacts': [], 'cleanupErrors': [],
                                             'symbolSession': self.process.provenance}
        ini = self.plugin / 'config/vrport.ini'
        if ini.exists():
            content = ini.read_bytes(); (self.out / 'settings-before.ini').write_bytes(content)
            self.result['settingsSha256'] = hashlib.sha256(content).hexdigest()

    def save(self):
        (self.out / 'run.json').write_text(json.dumps(self.result, indent=2) + '\n', encoding='utf-8')

    def math(self, command, value, name):
        path = self.out / (name + '-input.json'); path.write_text(json.dumps(value), encoding='utf-8')
        done = subprocess.run([str(self.args.generator), command, str(path)], capture_output=True, text=True, check=True)
        result = json.loads(done.stdout); (self.out / (name + '.json')).write_text(json.dumps(result, indent=2), encoding='utf-8')
        return result

    def set_temporary(self, name, value):
        if name not in self.saved:
            self.saved[name] = self.process.value(name, 'i')
        self.process.write32(name, value)

    def state(self):
        return self.bridge.call('RenderRuntimeAudit.state()')

    def protection(self):
        self.result['saveProtection'] = self.bridge.call('RenderRuntimeAudit.requireProtection()')
        return self.result['saveProtection']

    def cleanup_owned(self, token):
        deadline = time.monotonic() + 5
        while True:
            self.result['cleanup'] = self.bridge.call('RenderRuntimeAudit.cleanup(' + lua_literal(token) + ')')
            if self.result['cleanup'].get('complete'):
                return
            if time.monotonic() >= deadline:
                raise TimeoutError('Owned fixture cleanup remains pending; token and root save guard must be retained')
            time.sleep(.1)

    def snapshot(self, label):
        fields = {}; unstable = []
        for name, fmt in FIELDS.items():
            fields[name] = self.process.value(name, fmt)
            if fmt.endswith('f'):
                second = self.process.value(name, fmt)
                if second != fields[name]:
                    unstable.append(name)
        events, evidence = xr_events(self.args.xr_trace, self.args.pid)
        row = {'label': label, 'timeNs': time.time_ns(), 'state': self.state(), 'fields': fields,
               'unstableFields': unstable, 'framegen': self.process.report(), 'xrEvents': events,
               'xrEvidence': evidence, 'geometryLog': log_geometry(self.plugin / 'log/cyberpunkvrport.log')}
        (self.out / (label + '.json')).write_text(json.dumps(row, indent=2), encoding='utf-8')
        self.result['artifacts'].append(str(self.out / (label + '.json')))
        self.save(); return row

    def simulator(self):
        if self.args.runtime != 'simulator':
            raise RuntimeError('This command needs the simulator capture API; null-driver geometry snapshots remain available')
        status_path = SIM_DIR / 'runtime_status.json'
        if time.time() - status_path.stat().st_mtime > 5:
            raise RuntimeError('Simulator status is not fresh')
        status = json.loads(status_path.read_text(encoding='utf-8')); head = status['head_tracking']
        self.head = {**head['position'], **{k: head[k] for k in ('yaw', 'pitch', 'roll')}}
        self.sim = Sim(self.args.pid)
        if self.sim.status().get('state') == 'recording':
            raise RuntimeError('Another simulator recording is active')

    def fg(self, value):
        self.bridge.call('VRSettingsEdit(1,"1/Enable frame generation",%d)' % value)
        deadline = time.monotonic() + 8
        while time.monotonic() < deadline:
            if self.process.report().get('enabled') == value:
                return
            time.sleep(.2)
        raise RuntimeError('Framegen setting was not applied')

    def original_fg(self):
        self.bridge.call('VRSettingsPage(1)'); time.sleep(.3)
        page = self.bridge.call('VRSettingsPage(1)')
        for row in page.split('\x1e')[1:]:
            fields = row.split('\x1f')
            if len(fields) > 3 and fields[1] == '1/Enable frame generation':
                self.fg_original = int(fields[3]); return
        raise RuntimeError('Cannot read original framegen setting')

    def capture_inputs(self, label):
        flag = self.process.value('CyberpunkVR_FramegenCapture', 'i')
        if flag == 1:
            raise RuntimeError('Another native frame capture is pending')
        directory = self.plugin / 'log/framegen-capture'
        before = set(directory.iterdir()) if directory.exists() else set()
        self.set_temporary('CyberpunkVR_FramegenCapture', 1)
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            state = self.process.value('CyberpunkVR_FramegenCapture', 'i')
            if state in (2, -1):
                break
            time.sleep(.1)
        else:
            raise TimeoutError('Native frame capture did not complete; game left running')
        if state != 2:
            raise RuntimeError('Native frame capture failed')
        fresh = [p for p in directory.iterdir() if p not in before and (p / 'capture.json').is_file()]
        if len(fresh) != 1:
            raise RuntimeError('Cannot uniquely identify this run\'s capture directory')
        self.result.setdefault('nativeCaptures', {})[label] = str(fresh[0])
        self.save()

    def run(self):
        self.bridge.call(Path(__file__).with_name('bridge.lua').read_text(encoding='utf-8'), 'exec')
        self.protection()
        state = self.state()
        if state['inMenu'] and self.args.case not in ('snapshot', 'cleanup'):
            raise RuntimeError('Gameplay, not a native menu, is required')
        self.set_temporary('CyberpunkVR_RuntimeDiagnostics', 1)
        if self.args.case == 'snapshot':
            time.sleep(.5); row = self.snapshot('snapshot')
            self.result['geometry'] = self.math('geometry', row, 'geometry'); return
        if self.args.case == 'native-eyes':
            self.snapshot('before-capture'); self.capture_inputs('current-pose'); self.snapshot('after-capture'); return
        if self.args.case == 'cleanup':
            if not self.args.token:
                raise RuntimeError('--token from the fixture run.json is required')
            self.result['fixtureToken'] = self.args.token
            self.cleanup_owned(self.args.token)
            return
        if self.args.case == 'vehicle-spawn':
            if not self.args.vehicle_record:
                raise RuntimeError('--vehicle-record must name a verified TweakDB vehicle record')
            self.owner = uuid.uuid4().hex
            self.result['fixtureToken'] = self.owner; self.save()
            self.bridge.call('RenderRuntimeAudit.begin(' + lua_literal(self.owner) + ')')
            head = dict(x=0,y=1.7,z=0,yaw=0,pitch=0,roll=0)
            fixture = self.math('fixtures', {**state, 'head': head}, 'fixture')
            self.result['vehicle'] = self.bridge.call('RenderRuntimeAudit.spawnVehicle(%s,%s,%s)' %
                (lua_literal(self.owner), lua_literal(self.args.vehicle_record), lua_literal(fixture['vehiclePosition'])))
            deadline = time.monotonic() + 10
            while True:
                self.result['vehicleState'] = self.bridge.call('RenderRuntimeAudit.vehicle(' + lua_literal(self.owner) + ')')
                if self.result['vehicleState'].get('spawned') and self.result['vehicleState'].get('materialized'):
                    break
                if time.monotonic() >= deadline:
                    raise TimeoutError('Owned vehicle did not materialize; cleanup must cancel the exact pending ID')
                time.sleep(.1)
            self.result['cleanupCommand'] = 'Run cleanup with this fixtureToken after dismounting the temporary vehicle'
            self.owner = None  # Deliberately retained for the separate mounted-bike test; owner token is saved.
            return
        if self.args.runtime == 'simulator':
            self.simulator()
        if self.args.case == 'markers':
            if state['mounted']:
                raise RuntimeError('Marker fixture requires a stationary player on foot')
            self.owner = uuid.uuid4().hex; self.result['fixtureToken'] = self.owner; self.save()
            self.bridge.call('RenderRuntimeAudit.begin(' + lua_literal(self.owner) + ')')
            fixture = self.math('fixtures', {**state, 'head': self.head or dict(x=0,y=1.7,z=0,yaw=0,pitch=0,roll=0)}, 'fixture')
            self.result['markers'] = self.bridge.call('RenderRuntimeAudit.markers(%s,%s)' % (lua_literal(self.owner), lua_literal(fixture['markers'])))
            self.set_temporary('CyberpunkVR_WorldMarkerProbe', 1)
            poses = fixture['headPoses'] if self.sim else [{'label': 'null-fixed-pose'}]
            for pose in poses:
                if self.sim:
                    _send('head_pose_command.json', pose['head'])
                time.sleep(1)
                label = pose['label']; row = self.snapshot(label)
                if self.sim:
                    self.result['artifacts'] += self.sim.shot(self.out / label, eyes='both', fmt='png')
                else:
                    self.capture_inputs(label)
                    self.result['captureScope'] = 'application eye colors; null driver has no simulator compositor capture or scripted yaw/roll'
                words, floats = self.process.locked('CyberpunkVR_WorldMarkerTraceSequence',
                    [('CyberpunkVR_WorldMarkerTraceWords', 64 * 8 * 8), ('CyberpunkVR_WorldMarkerTraceFloats', 64 * 48 * 4)])
                (self.out / (label + '-marker-trace.json')).write_text(json.dumps({'words': struct.unpack('<512Q', words), 'floats': struct.unpack('<3072f', floats)}), encoding='utf-8')
                self.save()
            return
        if self.args.case == 'bike' and (not state['mounted'] or not self.process.value('cvr::anim::g_wheelMountedBike', 'B')):
            raise RuntimeError('Mount the verified windscreen motorcycle first; the probe does not guess a mount API')
        self.original_fg()
        modes = (0, 1) if self.args.fg == 'both' else (self.fg_original,) if self.args.fg == 'keep' else (int(self.args.fg == 'on'),)
        if self.args.throttle and self.args.case != 'bike':
            raise RuntimeError('--throttle is only valid for the mounted-bike case')
        if self.args.throttle and self.sim is None:
            raise RuntimeError('Scripted throttle requires the simulator controller API')
        for mode in modes:
            self.protection()
            self.fg(mode); time.sleep(3)
            # The native wait-duration field is gated separately. Its file
            # reporting is throttled to one batch/second (ReportXrFrameRates),
            # not per frame. Preserve the exact prior value for each window.
            rate_log_before = self.process.value('CyberpunkVR_XrRateLog', 'i')
            self.set_temporary('CyberpunkVR_XrRateLog', 1)
            time.sleep(.1)
            label = 'fg-' + str(mode); before = self.snapshot(label + '-before')
            start = self.process.value('CyberpunkVR_XrCycleTraceCount', 'Q')
            if self.sim:
                self.sim.record_start(self.out / (label + '.mp4'), eyes='both', eye_size=self.args.eye_size, fps=60, max_seconds=self.args.seconds + 3)
                self.recording = True
            print(json.dumps({'phase': 'recording' if self.sim else 'tracing', 'label': label, 'seconds': self.args.seconds, 'throttle': self.args.throttle}), flush=True)
            deadline = time.monotonic() + self.args.seconds
            protection_deadline = 0
            try:
                while time.monotonic() < deadline:
                    if self.args.throttle:
                        if time.monotonic() >= protection_deadline:
                            self.protection(); protection_deadline = time.monotonic() + .5
                        if not self.process.value('g_isDriving', 'B') or not self.process.value('cvr::anim::g_wheelMountedBike', 'B') or self.process.value('g_menuModeValue', 'i'):
                            raise RuntimeError('Driver/bike/gameplay gate changed; throttle released')
                        self.sim.trigger('right', self.args.throttle)
                    time.sleep(.05)
            finally:
                if self.args.throttle:
                    self.sim.trigger('right', 0)
            if self.sim:
                self.sim.record_stop(); self.recording = False
            after = self.snapshot(label + '-after')
            trace = self.process.trace(start)
            before_wait = bool(before['fields']['CyberpunkVR_XrRateLog'])
            after_wait = bool(after['fields']['CyberpunkVR_XrRateLog'])
            if before_wait == after_wait:
                trace['waitTimingEnabled'] = before_wait
            trace['waitTimingGateEndpoints'] = [before_wait, after_wait]
            self.result.setdefault('delivery', {})[label] = self.math('trace', trace, label + '-delivery')
            self.process.write32('CyberpunkVR_XrRateLog', rate_log_before)
            if self.sim:
                self.result['artifacts'].append(str(self.out / (label + '.mp4')))
            else:
                self.result['captureScope'] = 'application delivery trace only; no simulator compositor video on SteamVR null'
            # Capture readbacks are deliberately after the measured video window.
            # FG-off omits motion inputs; it is not a valid interpolation replay fixture.
            if mode == 1 and self.args.native_capture:
                self.capture_inputs(label)
            self.save()

    def close(self):
        def attempt(label, action):
            try:
                action()
            except Exception as error:
                self.result['cleanupErrors'].append({'action': label, 'error': str(error)})
        if self.recording:
            attempt('stop owned recording', self.sim.record_stop)
        if self.head is not None:
            attempt('restore head', lambda: _send('head_pose_command.json', self.head))
        if self.sim is not None:
            attempt('restore controllers', self.sim.restore)
            if self.sim.h:
                attempt('close simulator process handle', lambda: self.process.k.CloseHandle(self.sim.h)); self.sim.h = None
        if self.owner:
            attempt('remove owned markers/fixture', lambda: self.cleanup_owned(self.owner))
        if self.fg_original is not None:
            attempt('restore framegen', lambda: self.fg(self.fg_original))
        for name, value in self.saved.items():
            attempt('restore ' + name, lambda n=name, v=value: self.process.write32(n, v))
        ini = self.plugin / 'config/vrport.ini'
        if ini.exists():
            attempt('record restored settings', lambda: (self.out / 'settings-after.ini').write_bytes(ini.read_bytes()))
        self.process.close()
        if self.result['cleanupErrors']:
            self.result['status'] = 'cleanup-pending'
        self.save()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--pid', type=int, required=True)
    parser.add_argument('--case', choices=('snapshot', 'native-eyes', 'markers', 'traffic', 'bike', 'vehicle-spawn', 'cleanup'), required=True)
    parser.add_argument('--runtime', choices=('simulator', 'steamvr-null'), required=True)
    parser.add_argument('--game', type=Path, default=Path('C:/Program Files (x86)/Steam/steamapps/common/Cyberpunk 2077'))
    parser.add_argument('--generator', type=Path, default=ROOT / 'build/render-runtime-audit/Release/render_audit_math.exe')
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--label', default='unspecified scene/settings')
    parser.add_argument('--xr-trace', type=Path, help='Already active in-process xr_probe JSONL for this PID; never registered by this script')
    parser.add_argument('--seconds', type=float, default=10)
    parser.add_argument('--eye-size', type=int, default=1440)
    parser.add_argument('--native-capture', action='store_true')
    parser.add_argument('--fg', choices=('both', 'off', 'on', 'keep'), default='both')
    parser.add_argument('--throttle', type=float, default=0, help='Optional fixed RT level on a verified motorcycle driver, released in finally')
    parser.add_argument('--vehicle-record')
    parser.add_argument('--token')
    args = parser.parse_args()
    if not .5 <= args.seconds <= 30:
        parser.error('--seconds must be between .5 and 30')
    if not 0 <= args.throttle <= .4:
        parser.error('--throttle must be between 0 and .4')
    audit = Audit(args)
    try:
        audit.run(); audit.result['status'] = 'completed'
    except BaseException as error:
        audit.result['status'] = 'failed'; audit.result['error'] = str(error)
        raise
    finally:
        audit.close()
    if audit.result['cleanupErrors']:
        raise RuntimeError('Cleanup/restoration was incomplete; see run.json and retain the root save guard')


if __name__ == '__main__':
    main()
