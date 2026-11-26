"""Drive this PC's simulated headset and hands into a named pose, or take a composited still: the co-op tests' hands.

    py -3.13 -B tools/mp/sim_pose.py pose NAME [--yaw DEG]     # head yaw (degrees) and the named hands, held
    py -3.13 -B tools/mp/sim_pose.py shot PATH [--eyes left]   # the composited frame (simxr) as PATH_L.jpg / _SBS ...

Poses (body frame: metres from the head, x right, y up, z back):
    rest      hands by the hips
    wave      right hand high above the head, left down
    reach     both hands straight ahead at chest height
    tpose     arms out to the sides
    crouch    the head 0.45 m lower (NightCoop's full squat), hands at rest
Runs on either PC (the second PC's copy of the repo has it); the game must not have the foreground (Sim hands it away).
"""
import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'sim_harness'))
from simxr import Sim  # noqa: E402

POSES = {
    'rest': dict(right=((0.22, -0.65, -0.05), (0, -70, 0)), left=((-0.22, -0.65, -0.05), (0, -70, 0))),
    'wave': dict(right=((0.25, 0.35, -0.15), (0, 80, 0)), left=((-0.22, -0.65, -0.05), (0, -70, 0))),
    'reach': dict(right=((0.18, -0.30, -0.60), (0, 0, 0)), left=((-0.18, -0.30, -0.60), (0, 0, 0))),
    'tpose': dict(right=((0.70, -0.25, 0.0), (-90, 0, 0)), left=((-0.70, -0.25, 0.0), (90, 0, 0))),
    'crouch': dict(right=((0.22, -0.65, -0.05), (0, -70, 0)), left=((-0.22, -0.65, -0.05), (0, -70, 0))),
}


def main():
    a = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = a.add_subparsers(dest='cmd', required=True)
    p = sub.add_parser('pose')
    p.add_argument('name', choices=sorted(POSES))
    p.add_argument('--yaw', type=float, default=0.0)
    s = sub.add_parser('shot')
    s.add_argument('path')
    s.add_argument('--eyes', default='left')
    args = a.parse_args()
    sim = Sim()
    if args.cmd == 'shot':
        # the simulator writes the file from the game's process (its own working folder): an absolute path
        print(sim.shot(str(Path(args.path).resolve()), eyes=args.eyes, fmt='jpg'))
        return
    height = 1.7 - (0.45 if args.name == 'crouch' else 0.0)
    # simulator v3: body-mode hands hang off the body's yaw, so the body turns with the head
    if sim.body_state:
        sim.body_yaw(args.yaw)
    sim.head(yaw=args.yaw, pitch=0.0, pos=(0.0, height, 0.0))
    for side, (pos, ypr) in POSES[args.name].items():
        sim.hand(side, pos=pos, ypr=ypr)
    print(f'pose {args.name}, head yaw {args.yaw}, height {height}')


if __name__ == '__main__':
    main()
