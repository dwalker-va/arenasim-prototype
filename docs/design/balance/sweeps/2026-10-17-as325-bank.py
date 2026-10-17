#!/usr/bin/env python3
"""AS-325 seed bank: does deleting the Priest's Mana Burn CC-window floor
change any match?

Runs every config on two binaries, compares the match logs byte for byte, and
counts, from the BEFORE binary's decision traces, the Priest decisions taken
while its enemy healer was locked (`situation.enemy_healer_locked`).

    2026-10-17-as325-bank.py --before <checkout>/target/release/arenasim \\
        --after <other checkout>/target/release/arenasim --work /tmp/as325-bank

Each binary must run as a development build (an ancestor directory holds a
`Cargo.toml`, as `target/release/arenasim` in a checkout does), so its trace
lands in `match_logs/` under the run's own working directory rather than the
per-user directory every parallel run would share. A run whose logs match is
deleted; a diverging run keeps `cfg.json`, `before.txt` and `after.txt`.
"""
import argparse
import concurrent.futures as cf
import hashlib
import json
import os
import shutil
import subprocess
from collections import Counter

DPS = ["Warrior", "Mage", "Rogue", "Warlock", "Hunter"]
HEALERS = ["Priest", "Paladin", "Shaman", "Druid"]
MAPS = ["BasicArena", "PillaredArena"]
PROFILES = ["Legacy", "TeamPlan"]
SEEDS = [1, 2, 3]
THREES = [
    (["Warrior", "Mage", "Priest"], ["Rogue", "Warlock", "Paladin"]),
    (["Rogue", "Mage", "Priest"], ["Warrior", "Hunter", "Druid"]),
    (["Warlock", "Hunter", "Priest"], ["Warrior", "Rogue", "Shaman"]),
    (["Warrior", "Warlock", "Priest"], ["Mage", "Hunter", "Priest"]),
]


def configs():
    """Priest + DPS against every healer + DPS, both sides, both maps, both
    profiles, 3 seeds (2,400); four 3v3 comps, 10 seeds (160)."""
    out = []
    for d1 in DPS:
        for h in HEALERS:
            for d2 in DPS:
                priest_side, other = ["Priest", d1], [h, d2]
                for t1, t2 in ((priest_side, other), (other, priest_side)):
                    for mp in MAPS:
                        for prof in PROFILES:
                            for seed in SEEDS:
                                out.append((t1, t2, mp, prof, seed))
    for t1, t2 in THREES:
        for mp in MAPS:
            for prof in PROFILES:
                for seed in range(1, 11):
                    out.append((t1, t2, mp, prof, seed))
    return out


def trace_counts(path):
    c = Counter()
    with open(path) as f:
        for line in f:
            try:
                ev = json.loads(line)
            except json.JSONDecodeError:
                continue
            if ev.get("kind") != "ability_decision":
                continue
            if (ev.get("actor") or {}).get("class") != "Priest":
                continue
            locked = (ev.get("situation") or {}).get("enemy_healer_locked") is not None
            key = "locked" if locked else "free"
            outcome = ev.get("outcome") or {}
            chosen = outcome.get("ability") if outcome.get("type") == "action_taken" else None
            c[f"{key}_decisions"] += 1
            if chosen in ("ManaBurn", "FlashHeal"):
                c[f"{key}_{chosen}_chosen"] += 1
    return c


def run_one(args, i, cfg):
    t1, t2, mp, prof, seed = cfg
    d = os.path.join(args.work, str(i))
    os.makedirs(d, exist_ok=True)
    cfg_path = os.path.join(d, "cfg.json")
    with open(cfg_path, "w") as f:
        json.dump({"team1": t1, "team2": t2, "map": mp, "ai_profile": prof,
                   "random_seed": seed, "max_duration_secs": 300}, f)
    digests, kills, counts = {}, 0, Counter()
    for name, binary, traced in (("before", args.before, True), ("after", args.after, False)):
        log = os.path.join(d, f"{name}.txt")
        cwd = os.path.join(d, f"cwd_{name}")
        os.makedirs(cwd, exist_ok=True)
        cmd = [binary, "--headless", cfg_path, "--output", log]
        if traced:
            cmd += ["--trace-mode", "on"]
        p = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True)
        if p.returncode != 0:
            return i, cfg, None, f"{name} exit {p.returncode}: {p.stderr[-400:]}"
        with open(log, "rb") as f:
            data = f.read()
        digests[name] = hashlib.sha256(data).hexdigest()
        if traced:
            kills = data.decode("utf-8", "replace").count("[DEATH]")
            tdir = os.path.join(cwd, "match_logs")
            traces = [os.path.join(tdir, x) for x in os.listdir(tdir)
                      if x.endswith("_trace.jsonl")] if os.path.isdir(tdir) else []
            if len(traces) != 1:
                return i, cfg, None, f"expected one trace in {tdir}, found {len(traces)}"
            counts = trace_counts(traces[0])
        shutil.rmtree(cwd)
    same = digests["before"] == digests["after"]
    divergence = None
    if same:
        shutil.rmtree(d)
    else:
        divergence = classify_divergence(cfg, os.path.join(d, "before.txt"),
                                         os.path.join(d, "after.txt"))
    return i, cfg, {"same": same, "kills": kills, "counts": counts,
                    "divergence": divergence}, None


def read_log(path):
    """The timestamped event lines, and the `Winner:` value."""
    events, winner = [], None
    with open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            line = line.rstrip("\n")
            if line.startswith("["):
                events.append(line)
            elif line.startswith("Winner:"):
                winner = line.split(":", 1)[1].strip()
    return events, winner


def classify_divergence(cfg, before_path, after_path):
    """What a diverging match's first different event is, and how its winner
    moved, scored from the Priest's side when only one side fields a Priest."""
    t1, t2 = cfg[0], cfg[1]
    before, w_before = read_log(before_path)
    after, w_after = read_log(after_path)
    first = next((b for b, a in zip(before, after) if b != a), None)
    starts_at_burn = first is not None and "begins casting Mana Burn" in first
    p1, p2 = "Priest" in t1, "Priest" in t2
    if p1 and p2:
        outcome = "mirror: winner unchanged" if w_before == w_after else "mirror: winner changed"
    else:
        priest_team = "Team 1" if p1 else "Team 2"
        if w_before == w_after:
            outcome = "single Priest: winner unchanged"
        elif w_after == priest_team:
            outcome = "single Priest: Priest side gains"
        elif w_before == priest_team:
            outcome = "single Priest: Priest side loses"
        else:
            outcome = "single Priest: other winner change"
    return {"starts_at_burn": starts_at_burn, "first_before": first, "outcome": outcome}


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--before", required=True)
    ap.add_argument("--after", required=True)
    ap.add_argument("--work", required=True)
    ap.add_argument("--jobs", type=int, default=6)
    args = ap.parse_args()
    cs = configs()
    os.makedirs(args.work, exist_ok=True)
    total, diffs, errors = Counter(), [], []
    by_arm, outcomes, other_starts = Counter(), Counter(), []
    starts_at_burn = 0
    with cf.ThreadPoolExecutor(args.jobs) as ex:
        for fut in cf.as_completed([ex.submit(run_one, args, i, c) for i, c in enumerate(cs)]):
            i, cfg, out, err = fut.result()
            if err:
                errors.append((i, cfg, err))
                continue
            total.update(out["counts"])
            total["kills"] += out["kills"]
            if not out["same"]:
                diffs.append((i, cfg))
                div = out["divergence"]
                by_arm[(cfg[3], cfg[2])] += 1
                outcomes[div["outcome"]] += 1
                if div["starts_at_burn"]:
                    starts_at_burn += 1
                else:
                    other_starts.append((i, div["first_before"]))
    print(f"matches: {len(cs)}  errors: {len(errors)}  logs that differ: {len(diffs)}")
    for k in sorted(total):
        print(f"  {k}: {total[k]}")
    print("logs that differ, by profile and map:")
    for (prof, mp), n in sorted(by_arm.items()):
        print(f"  {prof} {mp}: {n}")
    print(f"first differing event is a before-side 'begins casting Mana Burn': "
          f"{starts_at_burn} of {len(diffs)}")
    for i, line in sorted(other_starts):
        print(f"  OTHER FIRST DIFFERENCE {i}: {line}")
    print("winner, after vs before:")
    for k in sorted(outcomes):
        print(f"  {k}: {outcomes[k]}")
    for d in sorted(diffs):
        print("DIFF", d)
    for e in errors:
        print("ERROR", e)
    return 1 if errors else 0


if __name__ == "__main__":
    raise SystemExit(main())
