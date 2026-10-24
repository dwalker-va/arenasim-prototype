#!/usr/bin/env python3
"""Post-contact hard-CC'd share per healer class x map, before and after
(design §9: "no healer x map cell gains hard CC").

    2026-10-23-as373-cc-cells.py before.jsonl after.jsonl
"""
import collections
import gzip
import json
import sys


def load(path):
    acc = collections.defaultdict(lambda: [0.0, 0.0])
    with (gzip.open(path, "rt") if path.endswith(".gz") else open(path)) as f:
        for line in f:
            if not line.strip():
                continue
            r = json.loads(line)
            for k, o in r["occ"].items():
                if not k.startswith("post|"):
                    continue
                a = acc[(r["class"], r["map"])]
                a[0] += o["secs"]
                a[1] += o["hard_cc"]
    return acc


before, after = load(sys.argv[1]), load(sys.argv[2])
print("| healer | map | hard CC'd before | after | change |")
print("|---|---|---|---|---|")
for key in sorted(before):
    b = 100 * before[key][1] / before[key][0]
    a = 100 * after[key][1] / after[key][0]
    print(f"| {key[0]} | {key[1]} | {b:.2f}% | {a:.2f}% | {a - b:+.2f} |")
