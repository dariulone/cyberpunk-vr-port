"""Roomscale and the physical crouch in the game, through the OpenXR Simulator's head-pose file channel (no keys, no
focus change). The head walks 0.6 m forward, back, 0.4 m right and back; then squats 0.5 m and stands up. The player
capsule's position (CyberpunkVR_PlayerEntityPos) and the port's crouch exports are read from the game process.
Prints one JSON line. The game must be in the world (tools/quickboot_tests/launch_to_save.py) and is left running."""
import ctypes
import json
import math
import os
import struct
import sys
import time
import uuid
from pathlib import Path

sys.path.insert(0, r'X:\Dev\CyberpunkVRPort\tools\sim_harness')
from simxr import game_pid, _exports, _modules  # noqa: E402

SIM = Path(os.environ['LOCALAPPDATA']) / 'OpenXR-Simulator'
k = ctypes.WinDLL('kernel32', use_last_error=True)
k.OpenProcess.restype = ctypes.c_void_p
k.ReadProcessMemory.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t,
                                ctypes.POINTER(ctypes.c_size_t)]

pid = game_pid()
port = next(m for m in _modules(pid) if m[0].lower() == 'cyberpunkvrport.dll')
ex = _exports(port[2])
handle = k.OpenProcess(0x1010, False, pid)


def read(name, fmt):
    size = struct.calcsize(fmt)
    buf = ctypes.create_string_buffer(size)
    got = ctypes.c_size_t()
    if not k.ReadProcessMemory(handle, ctypes.c_void_p(port[1] + ex[name]), buf, size, ctypes.byref(got)):
        raise OSError(ctypes.get_last_error())
    return struct.unpack(fmt, buf.raw)


def player():
    return read('CyberpunkVR_PlayerEntityPos', '<3f')


def status():
    for _ in range(50):
        try:
            return json.loads((SIM / 'runtime_status.json').read_text(encoding='utf-8'))
        except (OSError, ValueError):
            time.sleep(.02)
    raise RuntimeError('no simulator status')


def send(pose):
    target = SIM / 'head_pose_command.json'
    temporary = SIM / ('head_pose_command.json.' + uuid.uuid4().hex + '.writing')
    temporary.write_text(json.dumps(pose), encoding='ascii')
    for _ in range(50):
        try:
            os.replace(temporary, target)
            return
        except PermissionError:
            time.sleep(.01)
    raise RuntimeError('command file locked')


tracking = status()['head_tracking']   # {"position": {x, y, z}, "yaw", "pitch", "roll"}
start = dict(tracking['position'], yaw=tracking['yaw'], pitch=tracking['pitch'], roll=tracking['roll'])
base = {axis: start[axis] for axis in ('x', 'y', 'z', 'yaw', 'pitch', 'roll')}


def glide(dx=0.0, dy=0.0, dz=0.0, seconds=1.5):
    """Moves the head from where it is by (dx, dy, dz) metres in tracking axes, at an even pace."""
    pose = dict(base)
    steps = max(1, int(seconds / .02))
    for i in range(1, steps + 1):
        t = i / steps
        pose = dict(base, x=base['x'] + dx * t, y=base['y'] + dy * t, z=base['z'] + dz * t)
        send(pose)
        time.sleep(seconds / steps)
    base.update(x=pose['x'], y=pose['y'], z=pose['z'])


def planar(a, b):
    return round(math.hypot(b[0] - a[0], b[1] - a[1]), 3)


def main():
    result = {'head_start': [round(start[a], 3) for a in ('x', 'y', 'z')]}
    try:
        session(result)
    finally:
        send({axis: start[axis] for axis in ('x', 'y', 'z', 'yaw', 'pitch', 'roll')})
    print(json.dumps(result))


def session(result):
    time.sleep(1.0)
    p0 = player()
    glide(dz=-0.6)            # forward (OpenXR -Z)
    time.sleep(1.0)
    p1 = player()
    result['forward_0.6'] = planar(p0, p1)
    glide(dz=0.6)
    time.sleep(1.0)
    p2 = player()
    result['back_0.6'] = planar(p1, p2)
    result['round_trip_residual'] = planar(p0, p2)
    glide(dx=0.4)
    time.sleep(1.0)
    p3 = player()
    result['right_0.4'] = planar(p2, p3)
    glide(dx=-0.4)
    time.sleep(1.0)
    result['crouch_before'] = read('CyberpunkVR_PhysicalCrouchDesired', '<i')[0]
    glide(dy=-0.5, seconds=1.0)
    time.sleep(1.0)
    result['crouch_depth'] = round(read('CyberpunkVR_PhysicalCrouchDepth', '<f')[0], 3)
    result['crouch_desired'] = read('CyberpunkVR_PhysicalCrouchDesired', '<i')[0]
    glide(dy=0.5, seconds=1.0)
    time.sleep(1.0)
    result['crouch_after'] = read('CyberpunkVR_PhysicalCrouchDesired', '<i')[0]


if __name__ == '__main__':
    main()
