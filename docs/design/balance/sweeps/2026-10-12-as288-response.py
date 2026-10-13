#!/usr/bin/env python3
"""AS-288: how much later does a danger window open, timed from when the danger began?

usage: 2026-10-12-as288-response.py <windows-main.csv> <windows-branch.csv>

The inputs are `tests/ally_in_danger_windows.rs`'s per-window rows
(`ALLY_IN_DANGER_CSV=...`) from main and the branch. Only the `Legacy` rows
are paired. The debounce changes nothing a `Legacy` team decides, so its
matches play out frame for frame the same on both arms. On main a window
opens on the first frame of the raw danger (no debounce), so main's onsets ARE
the moments the danger began.

For each branch window, the delay is its onset minus the latest main onset of
the same teammate at or before it, in frames.
"""
import collections
import csv
import sys


def read(path):
    rows = collections.defaultdict(list)
    with open(path) as f:
        for r in csv.DictReader(f):
            if r["profile"] != "Legacy":
                continue
            key = (r["map"], r["comp"], r["seed"], r["team"], r["subject"])
            rows[key].append(int(r["onset_frame"]))
    return rows


def main(main_path, branch_path):
    before, after = read(main_path), read(branch_path)
    delays = collections.Counter()
    unpaired = 0
    for key, onsets in after.items():
        starts = sorted(before.get(key, []))
        for onset in onsets:
            prior = [s for s in starts if s <= onset]
            if not prior:
                unpaired += 1
                continue
            delays[onset - prior[-1]] += 1
    total = sum(delays.values())
    print(f"Legacy branch windows paired to a main onset: {total} (unpaired {unpaired})")
    for d, n in sorted(delays.items()):
        print(f"  opened {d:3d} frames ({d / 60:.3f}s) after the danger began: {n:4d} ({100 * n / total:.1f}%)")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
