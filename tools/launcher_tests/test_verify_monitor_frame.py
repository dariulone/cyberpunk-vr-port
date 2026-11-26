"""Exercise the same PNG and command-line paths used with exported game frames."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from PIL import Image


class MonitorFrameTest(unittest.TestCase):
    def test_same_frame_and_negative_controls(self):
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            left = Image.new("RGB", (4, 3), "red")
            right = Image.new("RGB", left.size, "blue")
            left.putpixel((1, 1), (0, 255, 0))
            right.putpixel((3, 2), (255, 255, 0))
            for name, eye in [("l.png", left), ("r.png", right)]:
                eye.save(directory / name)
            packed = Image.new("RGB", (9, 3), "white")
            packed.paste(left, (0, 0))
            packed.paste(right, (5, 0))
            packed.save(directory / "packed.png")
            final = Image.new("RGB", packed.size)
            final.paste(right, (2, 0))
            final.save(directory / "final.png")

            def verify(mode, *inputs, code):
                result = subprocess.run(
                    [sys.executable, "-B", str(Path(__file__).with_name("verify_monitor_frame.py").resolve()),
                     mode, "final.png", *inputs], cwd=directory, capture_output=True, text=True)
                self.assertEqual(result.returncode, code, result.stdout + result.stderr)
                return json.loads(result.stdout)

            self.assertEqual(verify("right", "--packed", "packed.png", code=0)["eye_roi"], [2, 0, 6, 3])
            self.assertTrue(verify("left", "--packed", "packed.png", code=1)["matches_opposite_eye"])
            verify("right", "--eyes", "l.png", "r.png", code=0)
            verify("right", "--eyes", "r.png", "r.png", code=2)
            final.putpixel((8, 2), (0, 0, 1))
            final.save(directory / "final.png")
            self.assertTrue(verify("right", "--packed", "packed.png", code=1)["nonblack_margins"])
            right.save(directory / "final.png")
            verify("right", "--eyes", "l.png", "r.png", code=0)


if __name__ == "__main__":
    unittest.main()
