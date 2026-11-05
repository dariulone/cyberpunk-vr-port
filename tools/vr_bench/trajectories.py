"""Head trajectories for the OpenXR Simulator (head_trajectory.h in the simulator: one key per line, `t x y z yaw
pitch roll`, the simulator's own angle convention -- yaw is the NEGATED OpenXR yaw, in radians).

Every trajectory is defined in OpenXR terms around a base pose (position in metres, yaw positive = turned left,
pitch positive = up, roll positive = the top to the right as seen from behind) and sampled at 250 Hz; the simulator
interpolates between keys at the display time the game asks for, so the motion is continuous whatever the frame rate.
"""
import math

RATE = 250.0


def _ease(t):
    """Smooth 0..1 (cubic), the shape of a quick deliberate head turn."""
    t = min(max(t, 0.0), 1.0)
    return t * t * (3 - 2 * t)


def _sine(amplitude, hz, phase=0.0):
    return lambda t: amplitude * math.sin(2 * math.pi * hz * t + phase)


def _snap(angle, turn, hold, repeats):
    """0 -> angle in `turn` s, hold, back in `turn` s, hold; `repeats` times."""
    period = 2 * (turn + hold)

    def f(t):
        if t >= period * repeats:
            return 0.0
        u = t % period
        if u < turn:
            return angle * _ease(u / turn)
        if u < turn + hold:
            return angle
        if u < 2 * turn + hold:
            return angle * (1 - _ease((u - turn - hold) / turn))
        return 0.0
    return f


def _lean(distance, move, hold):
    def f(t):
        if t < move:
            return distance * _ease(t / move)
        if t < move + hold:
            return distance
        return distance * (1 - _ease((t - move - hold) / move))
    return f


ZERO = lambda t: 0.0

# name: (seconds, yaw(t), pitch(t), roll(t), dx(t), dy(t), dz(t)) -- degrees and metres, OpenXR axes
# (x right, y up, z back: forward is -z).
TRAJECTORIES = {
    'still':        (8.0, ZERO, ZERO, ZERO, ZERO, ZERO, ZERO),
    'yaw_slow':     (8.0, _sine(30, 0.25), ZERO, ZERO, ZERO, ZERO, ZERO),            # peak 47 deg/s
    'yaw_fast':     (6.0, _sine(45, 1.0), ZERO, ZERO, ZERO, ZERO, ZERO),             # peak 283 deg/s
    'snap90':       (6.0, _snap(90, 0.35, 0.65, 3), ZERO, ZERO, ZERO, ZERO, ZERO),
    'snap180':      (5.6, _snap(180, 0.6, 0.8, 2), ZERO, ZERO, ZERO, ZERO, ZERO),
    'pitch':        (6.0, ZERO, _sine(25, 0.5), ZERO, ZERO, ZERO, ZERO),
    'roll':         (6.0, ZERO, ZERO, _sine(20, 0.5), ZERO, ZERO, ZERO),
    'combo3':       (8.0, _sine(40, 0.6), _sine(20, 0.9, 1.0), _sine(15, 0.45, 2.0), ZERO, ZERO, ZERO),
    'reversals':    (6.0, _sine(15, 2.5), ZERO, ZERO, ZERO, ZERO, ZERO),             # frequent direction changes
    'translate_fb': (6.0, ZERO, ZERO, ZERO, ZERO, ZERO, _sine(0.35, 0.5)),
    'translate_lr': (6.0, ZERO, ZERO, ZERO, _sine(0.25, 0.5), ZERO, ZERO),
    'vertical':     (6.0, ZERO, ZERO, ZERO, ZERO, _sine(0.2, 0.5), ZERO),
    'lean_in':      (4.0, ZERO, _lean(-15, 1.5, 1.0), ZERO, ZERO, _lean(-0.15, 1.5, 1.0), _lean(-0.5, 1.5, 1.0)),
    'combined':     (8.0, _sine(35, 0.5), _sine(10, 0.8), ZERO, _sine(0.15, 0.7), ZERO, _sine(0.2, 0.4)),
}

# The quick set the benchmark runs by default: still, a slow and a fast turn, snaps, all three axes, translation.
DEFAULT = ['still', 'yaw_slow', 'yaw_fast', 'snap90', 'combo3', 'translate_lr', 'lean_in']


def keys(name, base):
    """[(t, x, y, z, yaw_sim, pitch, roll)] for trajectory `name` around `base` = dict(x, y, z, yaw, pitch, roll)
    as the simulator's status reports it (radians, simulator yaw convention)."""
    seconds, yaw, pitch, roll, dx, dy, dz = TRAJECTORIES[name]
    out = []
    n = int(seconds * RATE)
    cy, sy = math.cos(-base['yaw']), math.sin(-base['yaw'])   # translations follow the base heading
    for i in range(n + 1):
        t = i / RATE
        x, z = dx(t), dz(t)
        # rotate the body-frame offset by the base heading (OpenXR yaw about +y)
        wx, wz = x * cy + z * sy, -x * sy + z * cy
        out.append((t, base['x'] + wx, base['y'] + dy(t), base['z'] + wz,
                    base['yaw'] - math.radians(yaw(t)), base['pitch'] + math.radians(pitch(t)),
                    base['roll'] + math.radians(roll(t))))
    return out


def text(name, base, delay=0.3, ident=0):
    lines = [f'id {ident}', f'delay {delay:.3f}']
    lines += ['%.4f %.5f %.5f %.5f %.6f %.6f %.6f' % k for k in keys(name, base)]
    return '\n'.join(lines) + '\n'


def peak_speed(name):
    """Peak angular speed (deg/s) and linear speed (m/s), numerically."""
    seconds, yaw, pitch, roll, dx, dy, dz = TRAJECTORIES[name]
    dt = 1 / RATE
    best_a = best_l = 0.0
    for i in range(int(seconds * RATE)):
        t = i * dt
        a = math.sqrt(sum((f(t + dt) - f(t)) ** 2 for f in (yaw, pitch, roll))) / dt
        l = math.sqrt(sum((f(t + dt) - f(t)) ** 2 for f in (dx, dy, dz))) / dt
        best_a, best_l = max(best_a, a), max(best_l, l)
    return best_a, best_l


if __name__ == '__main__':
    for name in TRAJECTORIES:
        a, l = peak_speed(name)
        print(f'{name:14s} {TRAJECTORIES[name][0]:4.1f} s  peak {a:6.1f} deg/s  {l:5.2f} m/s')
