"""Stage 1a in the game: the port DLL with the Rust stick mapping, a save loaded through the OpenXR Simulator.
The left stick is written into the simulator's controller (PadGate: OpenXR -> the port's XInput path -> the game);
the player's position is read from the port's export CyberpunkVR_PlayerEntityPos, the heading after a right-stick turn from
CyberpunkVR_BodyYawFinalRad. Prints one JSON line."""
import json
import math
import struct
import sys
import time
from pathlib import Path

sys.path[:0] = [r'X:\Dev\CyberpunkVRPort\tools\roomscale_tests', r'X:\Dev\CyberpunkVRPort\tools\sim_harness']
from simulator_input import PadGate  # noqa: E402
from simxr import game_pid, _exports, _modules  # noqa: E402

GAME = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
SIM = Path(json.loads(Path(r'C:\Users\lone33\AppData\Local\openxr\1\active_runtime.json').read_text())['runtime']['library_path'])

pid = game_pid()
port = next(m for m in _modules(pid) if m[0].lower() == 'cyberpunkvrport.dll')
ex = _exports(port[2])

with PadGate(pid, GAME, SIM) as pad:
    def pos():
        valid = pad.r.read(port[1] + ex['CyberpunkVR_PlayerEntityValid'], 1)[0]
        return valid, struct.unpack('<3f', pad.r.read(port[1] + ex['CyberpunkVR_PlayerEntityPos'], 12))

    def yaw():
        return struct.unpack('<f', pad.r.read(port[1] + ex['CyberpunkVR_BodyYawFinalRad'], 4))[0]

    def turn(x, seconds):
        # The right stick: snap or smooth turn, through the port's DeltaHead hook (src/Hooks/OnFootDeltaHead.cpp).
        y0 = yaw()
        pad.write(pad.controllers[1] + 36, struct.pack('<2f', x, 0.0))
        time.sleep(seconds)
        pad.write(pad.controllers[1] + 36, struct.pack('<2f', 0.0, 0.0))
        time.sleep(1.0)
        d = math.degrees(yaw() - y0)
        return round((d + 180.0) % 360.0 - 180.0, 1)

    def push(x, y, seconds):
        _, p0 = pos()
        pad.stick(x, y)
        time.sleep(seconds)
        pad.stick(0.0, 0.0)
        time.sleep(1.0)
        _, p1 = pos()
        return round(math.hypot(p1[0] - p0[0], p1[1] - p0[1]), 3)

    # The pose identity end to end (src/Camera/PoseIdentity.cpp or its Rust port): setups labelled at the locate site,
    # copies carried through the engine, and the final camera finding its label per view. The counters run with the
    # runtime diagnostics on, for the length of the session.
    POSE = [('CyberpunkVR_PoseIdComponent', 2), ('CyberpunkVR_PoseIdCopied', 1), ('CyberpunkVR_PoseIdCopyChanged', 1),
            ('CyberpunkVR_PoseIdFinal', 2), ('CyberpunkVR_PoseIdFinalMiss', 2)]

    def pose_counters():
        out = {}
        for name, n in POSE:
            for k in range(n):
                out[name[len('CyberpunkVR_PoseId'):] + (str(k) if n > 1 else '')] = struct.unpack('<Q', pad.r.read(port[1] + ex[name] + 8 * k, 8))[0]
        return out

    diagnostics = port[1] + ex['CyberpunkVR_RuntimeDiagnostics']
    diagnostics_was = pad.r.read(diagnostics, 4)
    pad.write(diagnostics, struct.pack('<i', 1))
    pose_before = pose_counters()

    results = {}
    valid, p = pos()
    results['valid'] = valid
    results['start'] = [round(v, 3) for v in p]
    time.sleep(1.0)
    results['idle_2s'] = push(0.0, 0.0, 2.0)
    results['inside_deadzone_0.10_2s'] = push(0.0, 0.10, 2.0)
    results['full_forward_2s'] = push(0.0, 1.0, 2.0)
    results['full_back_2s'] = push(0.0, -1.0, 2.0)
    results['half_forward_2s'] = push(0.0, 0.5, 2.0)
    results['strafe_right_2s'] = push(1.0, 0.0, 2.0)
    results['turn_right_1s_deg'] = turn(1.0, 1.0)
    results['turn_left_1s_deg'] = turn(-1.0, 1.0)
    _, p = pos()
    results['end'] = [round(v, 3) for v in p]
    pose_after = pose_counters()
    pad.write(diagnostics, diagnostics_was)
    results['pose_ids'] = {k: pose_after[k] - pose_before[k] for k in pose_after}
print(json.dumps(results))
