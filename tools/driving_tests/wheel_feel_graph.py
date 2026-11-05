"""Take the lag out of the cabin steering wheel in the common car graph (2026-11-05).

The cabin wheel (steering_wheel_turn, track steer_turn_rate) follows the drive model's turn input through
DampFloat (300 units/s) and CriticalSpringDamp (smoothTime 0.1 s). Under VR wheel grab the player's hands
are handed to that animation, so its 0.1 s spring left the hands trailing the controllers by about 30
degrees at a brisk 300 deg/s turn -- play in the wheel. This shortens the spring and lifts the rate limit
on the steer_turn_rate chains only; the curve, the doubled mulFactor and every other node stay as they are.

Usage: wheel_feel_graph.py <in.json> <out.json> [smoothTime] [speed]
"""
import json
import sys

SMOOTH = float(sys.argv[3]) if len(sys.argv) > 3 else 0.01
SPEED = float(sys.argv[4]) if len(sys.argv) > 4 else 3000.0


def walk(o, fn):
    if isinstance(o, dict):
        fn(o)
        for v in o.values():
            walk(v, fn)
    elif isinstance(o, list):
        for v in o:
            walk(v, fn)


doc = json.load(open(sys.argv[1], encoding='utf-8'))
chains = []
walk(doc['Data'], lambda n: chains.append(n) if n.get('$type') == 'animAnimNode_FloatTrackModifier'
     and (n.get('floatTrack') or {}).get('name', {}).get('$value') == 'steer_turn_rate' else None)
assert chains, 'no steer_turn_rate modifier'
changes = []


def edit(n):
    if n.get('$type') == 'animAnimNode_CriticalSpringDamp':
        assert abs(n['smoothTime'] - 0.1) < 1e-6, n['smoothTime']
        n['smoothTime'] = SMOOTH
        changes.append('smoothTime')
    elif n.get('$type') == 'animAnimNode_DampFloat':
        assert n['increaseSpeedNode']['node'] is None and n['decreaseSpeedNode']['node'] is None
        assert n['defaultIncreaseSpeed'] == 300 and n['defaultDecreaseSpeed'] == 300
        n['defaultIncreaseSpeed'] = n['defaultDecreaseSpeed'] = SPEED
        changes.append('damp')


for c in chains:
    walk(c['floatInputNode'], edit)
print(len(chains), 'chains;', changes)
assert changes.count('smoothTime') == len(chains) and changes.count('damp') == len(chains), changes
json.dump(doc, open(sys.argv[2], 'w', encoding='utf-8'), indent=2)
