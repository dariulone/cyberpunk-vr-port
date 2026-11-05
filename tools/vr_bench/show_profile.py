"""Prints a run's GPU-per-node table: py -3.13 -B tools/vr_bench/show_profile.py build/vr-bench/<run> [--top 40]"""
import argparse
import json
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
import vr_bench  # noqa: E402


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('run', type=Path)
    parser.add_argument('--top', type=int, default=40)
    args = parser.parse_args()
    raw = args.run / 'profiles-raw.json'
    s = vr_bench.summarize_profiles(json.loads(raw.read_text(encoding='utf-8')), vr_bench.node_names())
    t = s['totals']
    print(f"frames {s['frames']}  " + '  '.join(f'{k} {t[k]:.2f}' for k in t))
    print(f"{'node':34s} {'rva':>9s} {'total':>7s} {'L':>7s} {'R':>7s} {'F':>7s} {'C':>7s} {'O':>7s} {'ranges':>7s} {'cmds':>7s}")
    for r in s['nodes'][:args.top]:
        print(f"{r['name'][:34]:34s} {r['node']:>9s} {r['total']:7.3f} {r['L']:7.3f} {r['R']:7.3f} {r['F']:7.3f} "
              f"{r['C']:7.3f} {r['O']:7.3f} {r['ranges']:7.1f} {r['commands']:7.1f}")


if __name__ == '__main__':
    main()
