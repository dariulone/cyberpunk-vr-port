"""probe_session.ps1 -ScopeTest's hands, once the CET scope probe has drawn its scoped rifle: the left trigger held 6 s
(the game's aim: the scope's view and glass drawn), then the port's scanner gesture -- the left hand to the left ear,
its grip squeezed there (README: the scanner is toggled by a squeeze that begins at the ear), the scanner held open
6 s and closed the same way -- so the engine creates the scope glass's and the scanner outline's pipelines.

The simulator's controllers are written where PadGate finds them in the loaded image (native_body_live.controller_rvas:
the layout starts with the head-relative position x, y, z, the yaw and the pitch and the active flag, then the trigger
and grip clicks at +21/+22, their values at +28/+32 and the stick at +36). The left controller's bytes are put back as
they were. Prints one JSON line."""
import json
import struct
import sys
import time
from pathlib import Path

sys.path[:0] = [r'X:\Dev\CyberpunkVRPort\tools\roomscale_tests', r'X:\Dev\CyberpunkVRPort\tools\sim_harness']
from simulator_input import PadGate  # noqa: E402
from simxr import game_pid  # noqa: E402

GAME = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
SIM = Path(json.loads(Path(r'C:\Users\lone33\AppData\Local\openxr\1\active_runtime.json').read_text())['runtime']['library_path'])
# The left ear, head-relative (x right, y up, -z ahead): a hand's width left of the head, a little behind its centre.
EAR = (-0.10, -0.02, 0.03)
SIZE = 44

pid = game_pid()
steps = []
with PadGate(pid, GAME, SIM) as pad:
    left = pad.controllers[0]
    saved = pad.r.read(left, SIZE)

    def trigger(v):
        pad.write(left + 21, bytes([1 if v > 0.5 else 0]))
        pad.write(left + 28, struct.pack('<f', v))

    def grip(v):
        pad.write(left + 22, bytes([1 if v > 0.5 else 0]))
        pad.write(left + 32, struct.pack('<f', v))

    def glide(to, seconds=0.5):
        start = struct.unpack('<3f', pad.r.read(left, 12))
        n = max(1, int(seconds / 0.011))
        for i in range(1, n + 1):
            t = i / n
            pad.write(left, struct.pack('<3f', *(a + (b - a) * t for a, b in zip(start, to))))
            time.sleep(seconds / n)

    try:
        trigger(1.0)
        time.sleep(6.0)
        trigger(0.0)
        steps.append('aimed 6 s')
        glide(EAR)
        time.sleep(0.3)
        for what in ('open', 'close'):
            grip(1.0)
            time.sleep(0.35)
            grip(0.0)
            steps.append(f'scanner {what}')
            time.sleep(6.0 if what == 'open' else 0.5)
        glide(struct.unpack('<3f', saved[:12]))
        steps.append('hand back')
    finally:
        pad.write(left, saved)
print(json.dumps({'steps': steps}))
