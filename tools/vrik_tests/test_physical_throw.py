"""Replay real release sequencing and tracking discontinuities at several XR rates."""
import math
from pathlib import Path
from lupa import LuaRuntime

ROOT = Path(__file__).resolve().parents[2]


def stroke(yaw_deg, pitch_deg):
    """Unit vector in tracking space (Y up, -Z forward, positive yaw towards +X)."""
    yaw, pitch = math.radians(yaw_deg), math.radians(pitch_deg)
    return math.sin(yaw)*math.cos(pitch), math.sin(pitch), -math.cos(yaw)*math.cos(pitch)


def case(hz, action, motion_dir=(0, 0, -1), gaze=None, gaze_in_stroke=None):
    lua = LuaRuntime(unpack_returned_tuples=True)
    lua.execute('''
      function v(x,y,z,w) return {x=x,y=y,z=z,w=w or 0} end
      Vector4={new=v};stamp=0;seq=0;origin=1;x=0;y=0;z=0;grip=0;ready=true;valid=true
      paused=false;calls=0;key=1;c=1;s=0;gaze=nil;head=nil
      Game={GetSystemRequestsHandler=function() return {IsGamePaused=function() return paused end} end,
        VRMeleeThrowFrame=function() if not valid then return {} end
          local f={v(stamp,seq,origin),v(x,y,z,1),v(c,s,1,grip),v(0,0,-1)}
          if gaze then f[5]=gaze;f[6]=head end
          return f end}
      player={VRMeleeThrowReady=function() return ready end,VRMeleeThrowCancel=function() end,
        VRMeleeThrowRecovering=function() return false end,
        VRMeleeThrowRelease=function(_,w,d,s,h,o,l,e) calls=calls+1;direction=d;speed=s;handSpeed=h;own=o;look=l;eye=e;return true end}
      weapon={GetEntityID=function() return {hash=key} end}
    ''')
    m = lua.execute((ROOT/'mods/cet/CyberpunkVRPort_Weapon/physical_throw.lua').read_text())
    g = lua.globals()
    if gaze: g.gaze = g.v(*gaze); g.head = g.v(0, .3, .2, 1)
    elapsed = 0

    def tick(motion=0, duplicate=False):
        nonlocal elapsed
        elapsed += 1/hz
        if not duplicate:
            g.stamp = round(elapsed*1000) % 16777216
            g.seq += 1
        g.x += motion_dir[0]*motion/hz
        g.y += motion_dir[1]*motion/hz
        g.z += motion_dir[2]*motion/hz
        return m.update(1/hz, g.player, g.weapon)

    if action == 'already_held': g.grip = 1
    for _ in range(3):
        assert not tick(), 'aiming before a fresh grip suppressed melee contact'
    g.grip = 1
    for _ in range(3):
        reserved=tick()
        assert bool(reserved)==(action!='already_held'), 'throw reservation did not follow the grip edge'
    if action == 'yaw': g.c, g.s = 0, 1
    if gaze_in_stroke: g.gaze = g.v(*gaze_in_stroke)  # the head follows the arm during the stroke
    for _ in range(round(.12*hz)): tick(0 if action == 'gentle' else 4)
    if action == 'late':
        for _ in range(round(.06*hz)): tick()
    if action == 'lost': g.valid = False; tick(); g.valid = True
    if action == 'paused': g.paused = True; tick(); g.paused = False
    if action == 'swap': g.key = 2; tick()
    if action == 'recenter': g.origin = 2; tick()
    if action == 'teleport': g.z -= 2; tick()
    if action == 'not_ready': g.ready = False; tick()
    g.grip = 0
    tick(duplicate=action == 'duplicate')
    for _ in range(hz): tick()
    expected = action in ('normal', 'late', 'yaw', 'duplicate', 'gentle', 'aim')
    assert g.calls == int(expected), (hz, action, g.calls)
    if expected:
        if action == 'gentle':
            assert g.speed == 10 and g.handSpeed == 0
        else:
            assert 25 < g.speed < 35, (hz, action, g.speed)
            assert 3 < g.handSpeed < 5, (hz, action, g.handSpeed)
        if action != 'aim':
            assert abs(g.direction.z) < 1e-6
            assert abs(g.direction.x - (-1 if action == 'yaw' else 0)) < 1e-6
            assert abs(g.direction.y - (0 if action == 'yaw' else 1)) < 1e-6
    return g


def angles(d):
    """Yaw and pitch in degrees of a game-space vector (body yaw 0: game +Y is tracking -Z, +X is +X)."""
    return math.degrees(math.atan2(d.x, d.y)), math.degrees(math.asin(d.z))


def aimed(hz, hand, gaze, turned=None):
    """Released along `hand` while looking along `gaze` (yaw, pitch in degrees) until the stroke,
    then along `turned`; returns the throw's and the stroke's own yaw and pitch, and the globals."""
    g = case(hz, 'aim', stroke(*hand), stroke(*gaze) if gaze else None, stroke(*turned) if turned else None)
    return angles(g.direction), angles(g.own), g


# hand stroke, gaze, expected throw, expected own heading: pitch mostly the head's, heading leans to
# the gaze within 30 deg and is the stroke's own past 60 deg; a near-vertical stroke takes the gaze's
# heading. The own heading keeps the stroke's yaw at the throw's corrected pitch.
AIMS = [
    ((0, -30), (0, 0), (0, -6), (0, -6)),
    ((20, -25), (0, 0), (7, -5), (20, -5)),
    ((0, -30), (0, -20), (0, -22), (0, -22)),
    ((-20, 10), (10, 5), (-0.5, 6), (-20, 6)),
    ((45, 0), (0, 0), (30.375, 0), (45, 0)),
    ((0, 0), (40, 0), (17.3333, 0), (0, 0)),
    ((100, 0), (0, 0), (100, 0), (100, 0)),
    ((0, -80), (15, 0), (15, -16), (15, -16)),
    ((0, -30), None, (0, -30), (0, -30)),
]


if __name__ == '__main__':
    for hz in (20,30,60,90,120):
        for action in ('normal','late','yaw','duplicate','gentle','already_held','lost','paused','swap','recenter','teleport','not_ready'):
            case(hz,action)
        for hand, gaze, want, own in AIMS:
            got, got_own, g = aimed(hz, hand, gaze)
            assert all(abs(a-b) < 0.01 for a, b in zip(got, want)), (hz, hand, gaze, got, want)
            assert all(abs(a-b) < 0.01 for a, b in zip(got_own, own)), (hz, hand, gaze, got_own, own)
            if gaze:
                # The look handed to assistance is the gaze in game space; the eye offset is head - hand.
                assert all(abs(a-b) < 0.01 for a, b in zip(angles(g.look), gaze)), (hz, gaze, angles(g.look))
                hand_end = (g.x, g.y, g.z)
                want_eye = (0-hand_end[0], -(.2-hand_end[2]), .3-hand_end[1])
                got_eye = (g.eye.x, g.eye.y, g.eye.z)
                assert all(abs(a-b) < 1e-4 for a, b in zip(got_eye, want_eye)), (hz, got_eye, want_eye)
        # The head turning with the arm during the stroke does not move the aim: the gaze is the one
        # from the last still moment before the stroke.
        got, _, g = aimed(hz, (0, -30), (10, 0), turned=(-35, -20))
        assert all(abs(a-b) < 0.01 for a, b in zip(got, (6.5, -6))), (hz, got)
        assert all(abs(a-b) < 0.01 for a, b in zip(angles(g.look), (10, 0))), (hz, angles(g.look))
    print(f'{5*12} physical throw release/motion/lifecycle, {5*len(AIMS)} gaze aim and 5 pre-stroke gaze cases passed')
