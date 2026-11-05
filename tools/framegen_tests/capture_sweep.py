"""Moves the OpenXR Simulator head during an owned launch and arms the plugin's frame capture
(CyberpunkVR_FramegenCapture) in the middle of the movement. Prints the capture directory.

    py -3.13 tools/framegen_tests/capture_sweep.py --pid <pid> --mode yaw|strafe|both
"""
import argparse
import ctypes as c
import json
import math
import os
import struct
import sys
import time
from ctypes import wintypes as w
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(ROOT / 'tools/roomscale_tests'), str(ROOT / 'tools/swimming_tests')]
from live_symbols import resolve
from simulator_probe import read_status, send

GAME = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
DLL = GAME / 'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll'
SIMULATOR = Path(os.environ['LOCALAPPDATA']) / 'OpenXR-Simulator'


class Process:
    def __init__(self, pid):
        self.k = c.WinDLL('kernel32', use_last_error=True)
        self.k.OpenProcess.restype = w.HANDLE
        self.k.OpenProcess.argtypes = [w.DWORD, w.BOOL, w.DWORD]
        self.k.WriteProcessMemory.argtypes = [w.HANDLE, c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]
        self.k.ReadProcessMemory.argtypes = self.k.WriteProcessMemory.argtypes
        self.h = self.k.OpenProcess(0x0008 | 0x0020 | 0x0010, False, pid)

    def read32(self, address):
        b = c.create_string_buffer(4)
        n = c.c_size_t()
        self.k.ReadProcessMemory(self.h, address, b, 4, c.byref(n))
        return struct.unpack('<i', b.raw)[0]

    def write32(self, address, value):
        n = c.c_size_t()
        data = struct.pack('<i', value)
        if not self.k.WriteProcessMemory(self.h, address, data, 4, c.byref(n)):
            raise RuntimeError('write failed')


def start_pose():
    # The head pose command is absolute: the sweep moves around the pose the simulator has (standing, y = 1.7 by
    # default) and puts it back. A sweep at y = 0 put the head on the floor.
    head = read_status(SIMULATOR)['head_tracking']
    return dict(x=head['position']['x'], y=head['position']['y'], z=head['position']['z'],
                yaw=head['yaw'], pitch=head['pitch'], roll=head['roll'])


def pose(mode, t, base):
    # one full cycle every 2 s: about 40 deg/s of yaw and 0.35 m/s of sideways motion at its peak
    s = math.sin(2 * math.pi * t / 2.0)
    yaw = 12.0 * s if mode in ('yaw', 'both') else 0.0
    x = 0.12 * s if mode in ('strafe', 'both') else 0.0
    return dict(base, x=base['x'] + x, yaw=base['yaw'] + math.radians(yaw))


def stream(pose):
    # Without waiting for the simulator to take it: waiting for each acknowledgement left the head on one pose for
    # two real frames at a time, and the captured middle frame was then a copy of one of its neighbours.
    command = SIMULATOR / 'head_pose_command.json'
    temporary = SIMULATOR / 'head_pose_command.json.sweep'
    temporary.write_text(json.dumps(pose), encoding='ascii')
    for _ in range(40):
        try:
            os.replace(temporary, command)
            return
        except PermissionError:
            time.sleep(0.002)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--pid', type=int, required=True)
    p.add_argument('--mode', choices=('yaw', 'strafe', 'both', 'still'), default='both')
    a = p.parse_args()
    address = int(resolve(a.pid, DLL, ['CyberpunkVR_FramegenCapture'])['CyberpunkVR_FramegenCapture'], 16)
    process = Process(a.pid)
    process.write32(address, 0)
    base = start_pose()
    start = time.monotonic()
    armed = False
    result = 0
    try:
        while time.monotonic() - start < 20:
            t = time.monotonic() - start
            stream(pose(a.mode if a.mode != 'still' else 'none', t, base))
            # arm where the motion is fastest (sin crosses zero at t = 1 s)
            if not armed and t >= 0.85:
                process.write32(address, 1)
                armed = True
            if armed:
                result = process.read32(address)
                if result in (2, -1):
                    break
            time.sleep(0.004)
    finally:
        send(SIMULATOR, base)
    print('capture', {2: 'written', -1: 'FAILED'}.get(result, 'timed out (%d)' % result))
    folder = GAME / 'bin/x64/framegen-capture'
    if folder.exists():
        newest = max(folder.iterdir(), key=lambda d: d.stat().st_mtime)
        print(newest)
        # The simulator takes head commands at its preview rate (30 Hz), so one interval of three real frames at
        # 45 Hz can hold no motion: frame 1 is then a copy of a neighbour and `replay` scores nothing. Retake it.
        try:
            eyes = [f['eyes'][0] for f in json.loads((newest / 'capture.json').read_text(encoding='utf-8'))['frames']]
            for i in (1, 2):
                a, b = eyes[i - 1], eyes[i]
                turn = math.degrees(math.acos(max(-1.0, min(1.0, sum(p * q for p, q in zip(a['forward'], b['forward']))))))
                print('frames %d->%d: turn %.3f deg, move %.4f m' % (i - 1, i, turn, math.dist(a['position'], b['position'])))
        except (OSError, ValueError, KeyError, IndexError):
            pass


if __name__ == '__main__':
    main()
