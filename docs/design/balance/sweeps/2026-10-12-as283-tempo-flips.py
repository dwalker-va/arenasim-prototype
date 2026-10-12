#!/usr/bin/env python3
"""Per-match tempo flip counts, read off the situation_change trace (AS-283).

usage: [PROFILE=Legacy] 2026-10-12-as283-tempo-flips.py <arm-binary> <out-dir>

Runs a fixed seed bank (8 comps x 2 maps x seeds 1-5) of uniform-profile
matches (TeamPlan unless PROFILE says otherwise) one at a time with
--trace-mode on, moves each match log and trace into <out-dir>, and prints one
CSV row per match: label,map,seed,tempo_team1,tempo_team2,tempo_total — the
count of `situation_change` events named `tempo`.

The arm binary must sit beside its own `assets/` (so it reads that arm's
movement.ron), outside any checkout; its traces then land in the per-user
match_logs directory, which is why the runs are one second apart.
"""
import glob, json, os, shutil, subprocess, sys, time


COMPS = [
    (["Warrior", "Priest"], ["Rogue", "Mage"]),
    (["Warrior", "Paladin"], ["Mage", "Priest"]),
    (["Warrior", "Druid"], ["Warlock", "Shaman"]),
    (["Warrior", "Shaman"], ["Hunter", "Priest"]),
    (["Rogue", "Priest"], ["Warrior", "Paladin"]),
    (["Warlock", "Priest"], ["Warrior", "Druid"]),
    (["Mage", "Shaman"], ["Warrior", "Priest"]),
    (["Hunter", "Druid"], ["Rogue", "Paladin"]),
]
MAPS = ["PillaredArena", "BasicArena"]
SEEDS = range(1, 6)

def main(binary, out_dir):
    os.makedirs(out_dir, exist_ok=True)
    log_dirs = [
        os.path.expanduser("~/Library/Application Support/ArenaSim/match_logs"),
        os.path.join(os.path.dirname(binary), "match_logs"),
    ]
    print("label,map,seed,tempo_team1,tempo_team2,tempo_total")
    for t1, t2 in COMPS:
        for m in MAPS:
            for seed in SEEDS:
                label = "+".join(t1) + "_v_" + "+".join(t2)
                cfg = {"team1": t1, "team2": t2, "map": m, "random_seed": seed,
                       "ai_profile": os.environ.get("PROFILE", "TeamPlan")}
                cfg_path = os.path.join(out_dir, "cfg.json")
                with open(cfg_path, "w") as f:
                    json.dump(cfg, f)
                before = set()
                for d in log_dirs:
                    before |= set(glob.glob(os.path.join(d, "*_trace.jsonl")))
                # one second apart, so the timestamped trace names never collide
                time.sleep(1.05)
                subprocess.run([binary, "--headless", cfg_path, "--trace-mode", "on",
                                "--output", os.path.join(out_dir, f"{label}_{m}_{seed}.txt")],
                               check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                after = set()
                for d in log_dirs:
                    after |= set(glob.glob(os.path.join(d, "*_trace.jsonl")))
                new = sorted(after - before)
                if len(new) != 1:
                    sys.exit(f"expected one new trace, got {new}")
                dest = os.path.join(out_dir, f"{label}_{m}_{seed}_trace.jsonl")
                shutil.move(new[0], dest)
                counts = {1: 0, 2: 0}
                with open(dest) as f:
                    for line in f:
                        try:
                            e = json.loads(line)
                        except ValueError:
                            continue
                        if e.get("kind") == "situation_change" and e.get("name") == "tempo":
                            counts[e["team"]] += 1
                print(f"{label},{m},{seed},{counts[1]},{counts[2]},{counts[1]+counts[2]}", flush=True)


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
