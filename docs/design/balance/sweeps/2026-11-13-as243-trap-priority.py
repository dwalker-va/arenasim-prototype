#!/usr/bin/env python3
"""AS-243: paired Hunter trap and win metrics from `tests/milestone_mechanisms.rs` rows.

Usage: 2026-11-13-as243-trap-priority.py <rows-dir> <profile-prefix>

Reads <rows-dir>/out_{base,new}_<prefix>_{BasicArena,PillaredArena,TwinPillars}.jsonl
(one arm per binary, same JSONL), writes <prefix>_{base,new}.csv for
`scripts/paired_sweep.py`, and prints per Hunter side, paired: Freezing Traps
that sprang on an enemy healer, Frost Traps and Freezing Traps thrown, and the
Hunter side's wins with its McNemar z — over everything, by map, by seed block.
A mirror's two Hunters are two sides.
"""
import collections
import csv
import json
import math
import sys

MAPS = ["BasicArena", "PillaredArena", "TwinPillars"]
HEALERS = {"Priest", "Paladin", "Shaman", "Druid"}


def load(rows_dir, arm, prefix):
    rows = {}
    for m in MAPS:
        with open(f"{rows_dir}/out_{arm}_{prefix}_{m}.jsonl") as handle:
            for line in handle:
                r = json.loads(line)
                rows[(r["label"], r["seed"])] = r
    return rows


def to_csv(rows, path):
    with open(path, "w", newline="") as handle:
        w = csv.writer(handle)
        w.writerow(["label", "team1", "team2", "seed", "winner", "end_reason", "duration_secs"])
        for (label, seed), r in rows.items():
            w.writerow([label, r["team1"], r["team2"], seed, r["winner"], "-", f'{r["duration"]:.2f}'])


def hunter_sides(r):
    for u in r["units"]:
        if u["class"] != "Hunter":
            continue
        freezes = sum(v for k, v in u["trap_triggers"].items()
                      if k.startswith("Freezing:") and k.split(":")[1] in HEALERS)
        frost = sum(1 for _, n in u["trap_uses"] if n == "Frost Trap")
        freezing = sum(1 for _, n in u["trap_uses"] if n == "Freezing Trap")
        yield freezes, frost, freezing, r["winner"] == f"team{u['team']}"


def main(rows_dir, prefix):
    base, new = load(rows_dir, "base", prefix), load(rows_dir, "new", prefix)
    assert base.keys() == new.keys(), "the arms ran different configs"
    to_csv(base, f"{rows_dir}/{prefix}_base.csv")
    to_csv(new, f"{rows_dir}/{prefix}_new.csv")
    agg = collections.defaultdict(lambda: collections.defaultdict(float))
    for key in base:
        label = key[0]
        groups = ["all", "map:" + label.split("#")[1].split("_")[0], "block:" + label.split("_b")[-1]]
        for b, n in zip(hunter_sides(base[key]), hunter_sides(new[key])):
            for g in groups:
                a = agg[g]
                a["n"] += 1
                for i, name in enumerate(["freeze", "frost", "freezing"]):
                    a[name + "_b"] += b[i]
                    a[name + "_n"] += n[i]
                a["any_b"] += b[0] > 0
                a["any_n"] += n[0] > 0
                a["win_b"] += b[3]
                a["win_n"] += n[3]
                a["gain"] += n[3] and not b[3]
                a["loss"] += b[3] and not n[3]
    print("group | Hunter sides | healer freezes/side | sides with a freeze % | Frost Traps/side"
          " | Freezing Traps/side | Hunter win % (flips +/-, McNemar z)")
    for g, a in sorted(agg.items()):
        n = a["n"]
        d = a["gain"] + a["loss"]
        z = (a["gain"] - a["loss"]) / math.sqrt(d) if d else 0.0
        print(f"{g} | {int(n)} | {a['freeze_b']/n:.4f} -> {a['freeze_n']/n:.4f}"
              f" | {100*a['any_b']/n:.2f} -> {100*a['any_n']/n:.2f}"
              f" | {a['frost_b']/n:.3f} -> {a['frost_n']/n:.3f}"
              f" | {a['freezing_b']/n:.3f} -> {a['freezing_n']/n:.3f}"
              f" | {100*a['win_b']/n:.2f} -> {100*a['win_n']/n:.2f}"
              f" (+{int(a['gain'])}/-{int(a['loss'])}, z {z:+.2f})")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
