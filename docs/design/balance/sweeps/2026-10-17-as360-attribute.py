#!/usr/bin/env python3
"""AS-360: attribute every match a paired sweep moved to the pursuit's sight walk.

    2026-10-17-as360-attribute.py --before <main binary> --probe <probe binary> \\
        --configs <sweep>.jsonl[.gz] --before-csv <b>.csv[.gz] --after-csv <a>.csv[.gz] \\
        --work <dir> [--jobs 4]

`--probe` is the branch built with `2026-10-17-as360-fire-probe.patch` applied:
the branch's behaviour, plus one stderr line (`AS360FIRE t=<app clock> ...`)
on every frame the new clause decides — a non-healer in range of its target,
out of its sight, walking where `main` stood. stderr never reaches the match
log, so the probe's logs are the branch's.

For every row whose winner or duration differs between the two CSVs, the
script replays that config singly under `main` and the probe, and prints:
the first timestamped log line on which the two logs differ, the first frame
the clause fired, which classes it fired for, and whether the probe's single
run reproduced the batch's after row. A row is ATTRIBUTED when the clause
fired at or before the first differing line. The log stamps are rounded to
0.01s, so a first difference stamped in the frame the clause first fired
prints up to 0.005s ahead of it; that is reported as SAME-FRAME, not as a
failure.
"""
import argparse
import csv
import gzip
import json
import os
import re
import subprocess
from collections import Counter
from concurrent.futures import ThreadPoolExecutor

TS = re.compile(r"^\[\s*([0-9.]+)s\]")
RESULT = re.compile(r"Result: .*\(([0-9.]+)s\)")


def opened(path):
    return gzip.open(path, "rt") if path.endswith(".gz") else open(path)


def main():
    ap = argparse.ArgumentParser()
    for flag in ("--before", "--probe", "--configs", "--before-csv", "--after-csv", "--work"):
        ap.add_argument(flag, required=True)
    ap.add_argument("--jobs", type=int, default=4)
    args = ap.parse_args()
    os.makedirs(args.work, exist_ok=True)

    with opened(args.before_csv) as f:
        before = list(csv.DictReader(f))
    with opened(args.after_csv) as f:
        after = list(csv.DictReader(f))
    with opened(args.configs) as f:
        configs = [json.loads(line) for line in f]
    moved = [
        i
        for i, (b, a) in enumerate(zip(before, after))
        if (b["winner"], b["duration_secs"]) != (a["winner"], a["duration_secs"])
    ]

    def replay(i):
        logs, err, out = {}, {}, {}
        for tag, binary in (("before", args.before), ("probe", args.probe)):
            cfg = dict(configs[i], output_path=os.path.join(args.work, f"{i}-{tag}.txt"))
            path = os.path.join(args.work, f"{i}-{tag}.json")
            with open(path, "w") as f:
                json.dump(cfg, f)
            p = subprocess.run([binary, "--headless", path], capture_output=True, text=True)
            with open(cfg["output_path"]) as f:
                logs[tag] = [line for line in f if TS.match(line)]
            os.remove(cfg["output_path"])
            os.remove(path)
            err[tag], out[tag] = p.stderr, p.stdout
        first_diff = next(
            (
                min(float(TS.match(x).group(1)), float(TS.match(y).group(1)))
                for x, y in zip(logs["before"], logs["probe"])
                if x != y
            ),
            None,
        )
        fires = [l for l in err["probe"].splitlines() if l.startswith("AS360FIRE")]
        first_fire = float(fires[0].split("t=")[1].split()[0]) if fires else None
        classes = Counter(
            l.split("class=")[1].split()[0] + ("(pet)" if "pet=true" in l else "")
            for l in fires
        )
        m = RESULT.search(out["probe"])
        reproduced = m is not None and abs(float(m.group(1)) - float(after[i]["duration_secs"])) < 0.006
        return i, first_diff, first_fire, classes, reproduced

    with ThreadPoolExecutor(args.jobs) as ex:
        results = list(ex.map(replay, moved))

    verdicts, kinds = Counter(), Counter()
    for i, diff, fire, classes, reproduced in results:
        b, a = before[i], after[i]
        if fire is None or diff is None or not reproduced:
            verdict = "UNATTRIBUTED"
        elif diff >= fire:
            verdict = "ATTRIBUTED"
        elif fire - diff <= 0.005:
            verdict = "SAME-FRAME"
        else:
            verdict = "UNATTRIBUTED"
        verdicts[verdict] += 1
        kinds[" + ".join(sorted(classes))] += 1
        print(
            f"{b['label']} seed={b['seed']} {b['winner']}/{b['duration_secs']} -> "
            f"{a['winner']}/{a['duration_secs']}  first diff {diff}  first fire {fire}  "
            f"fired for {dict(classes)}  {verdict}"
        )
    print(f"\nmoved {len(moved)}: " + ", ".join(f"{k} {v}" for k, v in sorted(verdicts.items())))
    print("fired for (per moved match):")
    for k, v in kinds.most_common():
        print(f"  {v:5d}  {k}")
    return 0 if verdicts["UNATTRIBUTED"] == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
