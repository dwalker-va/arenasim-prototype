#!/usr/bin/env python3
"""AS-166 mechanism runner: headless matches -> one row per match on the
stealth opener, the Flares and the Freezing Traps.

usage: flaremech.py <binary> <jsonl> <outdir> <jobs>

Runs every config line of <jsonl> (the `--batch` format; `label`, `seed` and
kill targets carried through) as its own `--headless` match with its log in
<outdir>/<n>/match.txt, assets resolved next to the binary. Writes
<outdir>/matches.csv, one row per match:

- hunter_team / enemy: the Hunter's side and the enemy comp (classes sorted);
  rows with a Hunter on both sides or neither are skipped;
- flares, flare_reveals, first_flare (s after the gates), first_flare_at;
- revealed_by: what ended the enemy Rogue's stealth before it opened
  (`Flare`, `Freezing Trap`, `Frost Trap`, ... from the `[STEALTH]` line), or
  `opened` when it opened from stealth, or `-` with no enemy Rogue;
- opener, opener_t, opener_on: the Rogue's stealth opener (Cheap Shot /
  Ambush), when and on whom;
- traps_on_rogue, traps_on_other: enemy Freezing Trap springs by victim;
- winner, hunter_won, duration.
"""
import csv, json, os, re, subprocess, sys
from concurrent.futures import ThreadPoolExecutor

GATES = 10.0  # log timestamps count from the start of the 10s countdown
TS = re.compile(r"\[\s*([\d.]+)s\]")


def run(i, cfg, binary, outdir):
    d = os.path.join(outdir, str(i))
    os.makedirs(d, exist_ok=True)
    assets = os.path.join(os.path.dirname(binary), "assets")
    if not os.path.exists(os.path.join(d, "assets")):
        os.symlink(assets, os.path.join(d, "assets"))
    log = os.path.join(d, "match.txt")
    c = dict(cfg)
    c["output_path"] = log
    with open(os.path.join(d, "cfg.json"), "w") as f:
        json.dump(c, f)
    if not os.path.exists(log):
        subprocess.run([binary, "--headless", "cfg.json", "--trace-mode", "off"], cwd=d,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
    return analyse(cfg, log)


def analyse(cfg, log):
    t1, t2 = cfg["team1"], cfg["team2"]
    if ("Hunter" in t1) == ("Hunter" in t2):
        return None
    hteam = "1" if "Hunter" in t1 else "2"
    eteam = "2" if hteam == "1" else "1"
    enemy = t2 if hteam == "1" else t1
    lines = open(log).read().splitlines()
    winner = next((l.split(":", 1)[1].strip() for l in lines if l.startswith("Winner:")), "")
    last = max((float(m.group(1)) for m in (TS.match(l) for l in lines) if m), default=0.0)
    row = {
        "label": cfg.get("label", ""), "seed": cfg.get("random_seed"),
        "kt1": cfg.get("team1_kill_target", ""), "kt2": cfg.get("team2_kill_target", ""),
        "hunter_team": hteam, "enemy": "+".join(sorted(enemy)),
        "flares": 0, "flare_reveals": 0, "first_flare": "", "first_flare_at": "",
        "revealed_by": "-" if "Rogue" not in enemy else "", "reveal_t": "",
        "opener": "", "opener_t": "", "opener_on": "",
        "traps_on_rogue": 0, "traps_on_other": 0,
        "winner": winner, "duration": round(last - GATES, 2),
    }
    row["hunter_won"] = int(("Team " + hteam) in winner)
    rogue = f"Team {eteam} Rogue"
    for l in lines:
        m = TS.match(l)
        if not m:
            continue
        t = round(float(m.group(1)) - GATES, 2)
        if f"Team {hteam} Hunter" in l and "uses Flare" in l:
            row["flares"] += 1
            if row["first_flare"] == "":
                row["first_flare"] = t
        elif "[FLARE]" in l and f"Team {hteam} Hunter" in l and row["first_flare_at"] == "":
            row["first_flare_at"] = l.split("lights ")[1]
        elif re.search(rf"\[STEALTH\] {rogue} #\d is revealed", l):
            src = l.split("revealed by ")[1].strip()
            if src == "Flare":
                row["flare_reveals"] += 1
            if row["revealed_by"] == "" and row["opener"] == "":
                row["revealed_by"], row["reveal_t"] = src, t
        elif re.search(rf"\[CAST\] {rogue} #\d uses (Cheap Shot|Ambush) on ", l):
            if row["opener"] == "":
                row["opener"] = "Cheap Shot" if "Cheap Shot" in l else "Ambush"
                row["opener_t"] = t
                row["opener_on"] = l.split(" on ")[-1].strip()
                if row["revealed_by"] == "":
                    row["revealed_by"] = "opened"
        elif f"Team {hteam} Hunter" in l and "Freezing Trap triggers on" in l:
            victim = l.split("triggers on ")[1].split(" —")[0]
            if victim.startswith(rogue):
                row["traps_on_rogue"] += 1
            else:
                row["traps_on_other"] += 1
    if row["revealed_by"] == "":
        row["revealed_by"] = "never"
    return row


def main(argv):
    binary, jsonl, outdir, jobs = os.path.abspath(argv[0]), argv[1], argv[2], int(argv[3])
    cfgs = [json.loads(l) for l in open(jsonl) if l.strip()]
    os.makedirs(outdir, exist_ok=True)
    with ThreadPoolExecutor(jobs) as ex:
        rows = list(ex.map(lambda ic: run(ic[0], ic[1], binary, outdir), enumerate(cfgs)))
    rows = [r for r in rows if r]
    with open(os.path.join(outdir, "matches.csv"), "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=list(rows[0].keys()))
        w.writeheader()
        w.writerows(rows)
    print(f"{len(rows)} rows -> {outdir}/matches.csv")


if __name__ == "__main__":
    main(sys.argv[1:])
