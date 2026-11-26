"""Verify a lossless pre-Present RGB export against eye images from the same frame.

Acquisition and rendering assumptions are in monitor_runtime.md. Coordinates are
half-open and relative to the backbuffer. Exit 0 = pass, 1 = mismatch, 2 = inconclusive.
"""
import argparse
import json
from pathlib import Path

from PIL import Image, ImageChops


def difference_box(a, b):
    return ImageChops.difference(a, b).getbbox()


def compare(presented, left, right, mode):
    width, height = presented.size
    eye_width, eye_height = left.size
    if left.size != right.size or eye_height != height or eye_width > width:
        raise ValueError("eye and backbuffer sizes are incompatible")
    start = (width - eye_width) // 2
    roi = (start, 0, start + eye_width, height)
    report = {"mode": mode, "backbuffer_size": presented.size, "eye_roi": roi}
    if difference_box(left, right) is None:
        return {**report, "result": "inconclusive", "reason": "eye references are identical"}
    expected, other = (left, right) if mode == "left" else (right, left)
    if all(low == high for low, high in expected.getextrema()):
        return {**report, "result": "inconclusive", "reason": "selected reference has no scene detail"}
    center = presented.crop(roi)
    mismatch = difference_box(center, expected)
    matches_other = difference_box(center, other) is None
    margins = [(0, 0, start, height), (start + eye_width, 0, width, height)]
    lit_margins = [box for box in margins if box[0] < box[2] and presented.crop(box).getbbox()]
    return {**report, "eye_mismatch_bbox_in_roi": mismatch, "matches_opposite_eye": matches_other,
            "nonblack_margins": lit_margins, "result": "fail" if mismatch or lit_margins else "pass"}


def rgb(path):
    with Image.open(path) as image:
        if image.format != "PNG" or image.mode not in ("RGB", "RGBA"):
            raise ValueError(f"expected RGB or RGBA PNG export: {path}")
        return image.convert("RGB")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("left", "right"))
    parser.add_argument("presented", type=Path)
    refs = parser.add_mutually_exclusive_group(required=True)
    refs.add_argument("--packed", type=Path, help="original native stereo backbuffer before the mirror")
    refs.add_argument("--eyes", type=Path, nargs=2, metavar=("LEFT", "RIGHT"), help="finished eye textures")
    args = parser.parse_args()
    try:
        presented = rgb(args.presented)
        if args.packed:
            packed = rgb(args.packed)
            if packed.size != presented.size or packed.width < 2:
                raise ValueError("packed input must have the same size as the final backbuffer")
            w, h = packed.size
            left = packed.crop((0, 0, w // 2, h))
            right = packed.crop((w - w // 2, 0, w, h))
        else:
            left, right = map(rgb, args.eyes)
        report = compare(presented, left, right, args.mode)
    except (OSError, ValueError) as error:
        report = {"result": "inconclusive", "reason": str(error)}
    print(json.dumps(report))
    return {"pass": 0, "fail": 1, "inconclusive": 2}[report["result"]]


if __name__ == "__main__":
    raise SystemExit(main())
