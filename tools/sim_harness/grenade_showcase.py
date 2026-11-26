"""Record an already-held belt prop using virtual controller input only."""
import argparse
from pathlib import Path
import time

from simxr import Sim


def present(sim, hand):
    side = -1 if hand == "left" else 1
    # OpenXR body frame: +Y up, -Z forward. From neutral, pitch up
    # and yaw inward toward the viewer, with no roll of the controller.
    sim.hand(hand, pos=(side * .07, -.125, -.29),
             ypr=(side * 8, 8, 0), mode="body")


def record(sim, hand, output):
    sim.record_start(output, eyes="right", eye_size=1120, fps=45,
                     codec="h264", max_seconds=5)
    try:
        time.sleep(.2)
        sim.tap(hand, "stick", hold=.12)
        time.sleep(1.25)
    finally:
        sim.record_stop()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("pid", type=int)
    parser.add_argument("hand", choices=("left", "right"))
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    sim = Sim(args.pid, focus_away=False)
    try:
        present(sim, args.hand)
        time.sleep(.4)
        record(sim, args.hand, args.output.resolve())
    finally:
        sim.restore()
