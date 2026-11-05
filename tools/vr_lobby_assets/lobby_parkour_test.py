"""Runs the lobby's parkour routes in the live game: puts the player on each block (lobby frame), runs at the next
block and jumps, and reports the height the player lands at against the block's top.

    py -3.13 tools/vr_lobby_assets/lobby_parkour_test.py [west|east]

The routes and blocks come from lobby_layout.py (the same data the scene is built from).
"""
import math
import struct
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
for d in ('tools/roomscale_tests', 'tools/quickboot_tests', 'tools/swimming_tests', 'build/shotgun-reload-20261019/harness'):
    sys.path.insert(0, str(ROOT / d))
sys.path.insert(0, str(HERE))
import launch_to_save as L  # noqa: E402
import lobby_layout as LL  # noqa: E402
from common import lua  # noqa: E402
from live_symbols import loaded_module  # noqa: E402
from simulator_input import PadGate  # noqa: E402
from lobby_walk_test import where  # noqa: E402

PUT = '''
local d = GetMod("CyberpunkVRPort_Lobby").debug()
local a = d.anchor
local yaw = math.rad(a.yaw)
local x, y, z, h = %f, %f, %f, %f
local c, s = math.cos(yaw), math.sin(yaw)
local p = Vector4.new(a.pos.x + c * x - s * y, a.pos.y + s * x + c * y, a.pos.z + z, 1.0)
Game.GetTeleportationFacility():Teleport(Game.GetPlayer(), p, EulerAngles.new(0, 0, a.yaw + h))
print("ok")
'''


def top_of(L_, name):
    s = next(x for x in L_.solids if x.name == name)
    return s, s.origin[2] + s.size[2]


def main():
    route_name = sys.argv[1] if len(sys.argv) > 1 else 'west'
    L_ = LL.build()
    route = next(r for n, r in L_.parkour if n == route_name)
    pid = int(next(line for line in subprocess.run(
        ['tasklist', '/FO', 'CSV', '/NH', '/FI', 'IMAGENAME eq Cyberpunk2077.exe'], capture_output=True,
        text=True).stdout.splitlines() if 'Cyberpunk' in line).split(',')[1].strip('"'))
    sim = loaded_module(pid, 'openxr_simulator.dll')
    results = []
    with PadGate(pid, L.DEFAULT_GAME, sim) as pad:
        for a_name, b_name in zip(route[1:], route[2:]):
            a, a_top = top_of(L_, a_name)
            b, b_top = top_of(L_, b_name)
            # Stand on a's centre, facing b; run, jump half-way, keep running.
            dx, dy = b.origin[0] - a.origin[0], b.origin[1] - a.origin[1]
            lua(PUT % (a.origin[0], a.origin[1], a_top + 0.05, 0.0))
            time.sleep(1.2)
            # Facing the lobby's +Y (the teleport sets the heading too): run along a->b with the stick.
            n = math.hypot(dx, dy)
            pad.write(pad.controllers[0] + 36, struct.pack('<2f', dx / n, dy / n))
            time.sleep(0.35)
            # A running jump: about 1 m up at its top, 0.75 s in the air, 3 m along (measured live). Let go of the
            # stick on landing, or the player runs on off the far side of a 2.5 m block.
            pad.jump(True)
            time.sleep(0.2)
            pad.jump(False)
            time.sleep(0.5)
            pad.write(pad.controllers[0] + 36, struct.pack('<2f', 0.0, 0.0))
            time.sleep(1.0)
            x, y, z = where()
            ok = abs(z - b_top) < 0.25
            results.append((a_name, b_name, round(b_top - a_top, 2), z, ok))
            print('%-22s -> %-22s rise %+.2f  landed z %.2f (top %.2f) %s' % (a_name, b_name, b_top - a_top, z, b_top,
                                                                             'OK' if ok else 'MISS'), flush=True)
    print('route', route_name, sum(r[4] for r in results), '/', len(results))


if __name__ == '__main__':
    main()
