"""Read-only save-load telemetry from an exactly matching DLL/PDB.

No debugger attachment, input injection, or game-memory writes. Individual
samples can straddle publication; conclusions require sustained observations.
Create --stop-file to end an armed recording after the transition.
"""
from pathlib import Path
import argparse
import json
import struct
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
for folder in ('swimming_tests', 'body_rotation_tests', 'roomscale_tests'):
    sys.path.insert(0, str(ROOT / 'tools' / folder))
from live_symbols import resolve
from live_types import describe
from live_read import Reader

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--pid', type=int, required=True)
p.add_argument('--dll', type=Path, required=True)
p.add_argument('--seconds', type=float, default=300)
p.add_argument('--interval', type=float, default=.02)
p.add_argument('--out', type=Path, required=True)
p.add_argument('--stop-file', type=Path)
a = p.parse_args()
if a.seconds <= 0 or a.interval < .01:
    p.error('positive duration and sampling interval >= 0.01 required')
formats = {
    'g_menuModeValue': 'i', 'CyberpunkVR_RuntimeDiagnostics': 'i',
    'CyberpunkVR_DebugEngineStereoLoadingLayer': '2i',
    'CyberpunkVR_DebugEngineStereoLoadingTicks': 'Q',
    'CyberpunkVR_LoadingInSpace': 'i', 'CyberpunkVR_EngineStereoLoadingPanel': 'i',
    'CyberpunkVR_EngineStereoMainMenus': 'i', 'CyberpunkVR_LobbyStats': '12Q',
}
for name in ('XrRepeatCycles', 'XrProjectionCycles', 'XrCaptureNotDone',
             'XrCapturePrevious', 'XrCaptureBuffered', 'XrAllocatorBusy',
             'XrResubmits', 'XrEmptyEnds', 'DebugXrCycles', 'DebugMonoSubmits',
             'DebugFreshSubmits', 'DebugXrEnds', 'DebugXrEndFailed',
             'DebugXrWaits', 'DebugXrWaitsReturned', 'DebugVrcamEyePaired',
             'DebugVrcamEyeUnpaired', 'DebugVrcamEyeReused'):
    formats['CyberpunkVR_' + name] = 'Q'
symbols = resolve(a.pid, a.dll.resolve(), ['OpenXRManager::Get', *formats])
types = describe(a.dll.resolve(), ('OpenXRManager::CapturedMonoFrame',
    'OpenXRManager::LastSubmission', 'cvr::loading::Screen'))
assert symbols['sha256'] == types['sha256']
reader = Reader(a.pid, r'C:\Program Files (x86)\GOG Galaxy\Games\Cyberpunk 2077\bin\x64\Cyberpunk2077.exe', 0, 0)

def unpack(data, fmt, offset=0):
    values = struct.unpack_from('<' + fmt, data, offset)
    return values[0] if len(values) == 1 else values

def fields(address, kind, wanted):
    layout = types[kind]
    data = reader.read(address, layout['size'])
    return {name: unpack(data, fmt, layout['fields'][name]['offset'])
            for name, fmt in wanted.items() if name in layout['fields']}

try:
    entry = int(symbols['OpenXRManager::Get'], 16)
    code = reader.read(entry, 128)
    found = [entry+i+7+struct.unpack_from('<i', code, i+3)[0]
             for i in range(len(code)-6) if code[i:i+3] in (b'\x48\x8d\x05', b'\x48\x8d\x1d')]
    assert len(found) == 2 and found[0] == found[1], 'Unrecognized singleton code'
    instance = found[0]
    assert int(symbols['module_base'], 16) < instance < int(symbols['module_base'], 16) + 0x10000000
    def member(name):
        return instance + types['OpenXRManager']['fields'][name]['offset']
    a.out.parent.mkdir(parents=True, exist_ok=True)
    with a.out.open('x', encoding='utf-8') as out:
        out.write(json.dumps({'metadata': symbols, 'wall_time': time.time(),
            'note': 'Read-only samples may straddle publication; counters can be gated by diagnostics.'})+'\n')
        out.flush()
        print('Recording armed: ' + str(a.out), flush=True)
        start = time.monotonic()
        count = 0
        while time.monotonic()-start < a.seconds and not (a.stop_file and a.stop_file.exists()):
            row = {'seconds': round(time.monotonic()-start, 4), 'wall_time': time.time()}
            row['globals'] = {name: unpack(reader.read(int(symbols[name], 16), struct.calcsize('<'+fmt)), fmt)
                              for name, fmt in formats.items()}
            row['runtime'] = fields(instance, 'OpenXRManager', {
                'm_sessionRunning': '?', 'm_sessionFocused': '?', 'm_sessionState': 'i',
                'm_lastSubmittedSerial': 'Q', 'm_trackingOriginSerial': 'Q',
                'm_loadingArtworkReady': '?', 'm_loadingScreenIsNotice': '?'})
            for member_name, label in (('m_monoCapturedFrame', 'capture'), ('m_monoPreviousFrame', 'previous')):
                row[label] = fields(member(member_name), 'OpenXRManager::CapturedMonoFrame', {
                    'serial': 'Q', 'captureMs': 'd', 'captureFence': 'Q', 'dark': '?',
                    'oneTurn': '?', 'presentation': 'B', 'hasView': '2?', 'mainEye': 'b'})
            row['submit'] = fields(member('m_lastSubmission'), 'OpenXRManager::LastSubmission', {
                'valid': '?', 'menuQuad': '?', 'submittedAt': 'q', 'presentation': 'B',
                'origin': 'Q', 'extraCount': 'I'})
            row['screen'] = fields(member('m_loadingScreen'), 'cvr::loading::Screen', {
                'up_': '?', 'held_': '?', 'fresh_': '?', 'ready_': '?',
                'opacity_': 'f', 'shown_': 'f', 'anchor_': '7f'})
            out.write(json.dumps(row)+'\n')
            out.flush()
            count += 1
            time.sleep(a.interval)
        print('Recorded ' + str(count) + ' samples.', flush=True)
finally:
    reader.close()
