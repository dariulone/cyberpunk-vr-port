"""Walks the player through the live lobby with the simulator's left stick and reports where the player ends up in
the lobby's frame (X right, Y forward, Z up from the spot it came up on): whether colliders stop the player, whether
stairs and the dais are climbed.

    py -3.13 tools/vr_lobby_assets/lobby_walk_test.py "X,Y,SECONDS" ["X,Y,SECONDS" ...]

Each step holds the left stick at (X, Y) for SECONDS, then lets go and prints the position.
"""
import math
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
for d in ('tools/roomscale_tests', 'tools/quickboot_tests', 'tools/swimming_tests', 'build/shotgun-reload-20261019/harness'):
    sys.path.insert(0, str(ROOT / d))
import struct  # noqa: E402

import launch_to_save as L  # noqa: E402
from common import lua  # noqa: E402
from live_symbols import loaded_module  # noqa: E402
from simulator_input import PadGate  # noqa: E402

WHERE = '''
local d = GetMod("CyberpunkVRPort_Lobby").debug()
local p = Game.GetPlayer():GetWorldPosition()
local a = d.anchor
local yaw = math.rad(a.yaw)
local dx, dy = p.x - a.pos.x, p.y - a.pos.y
local c, s = math.cos(-yaw), math.sin(-yaw)
print(string.format("%.2f %.2f %.2f", c * dx - s * dy, s * dx + c * dy, p.z - a.pos.z))
'''


def where():
    return tuple(float(v) for v in lua(WHERE).split())


def main():
    pid = int(next(line for line in __import__('subprocess').run(
        ['tasklist', '/FO', 'CSV', '/NH', '/FI', 'IMAGENAME eq Cyberpunk2077.exe'], capture_output=True,
        text=True).stdout.splitlines() if 'Cyberpunk' in line).split(',')[1].strip('"'))
    sim = loaded_module(pid, 'openxr_simulator.dll')
    print('start', where(), flush=True)
    with PadGate(pid, L.DEFAULT_GAME, sim) as pad:
        for step in sys.argv[1:]:
            x, y, secs = (float(v) for v in step.split(','))
            pad.write(pad.controllers[0] + 36, struct.pack('<2f', x, y))
            time.sleep(secs)
            pad.write(pad.controllers[0] + 36, struct.pack('<2f', 0, 0))
            time.sleep(0.6)
            print('after', step, '->', where(), flush=True)


if __name__ == '__main__':
    main()
