#!/usr/bin/env python3
"""AS-125 mechanism runner: traced headless matches -> per-trap rows.

usage: trapmech.py <binary> <outdir> <jobs> [seeds]
Each match runs in its own cwd (so the seconds-stamped trace cannot collide).
Writes <outdir>/traps.csv (one row per Freezing Trap thrown) and
<outdir>/matches.csv (one row per match: trap counts, winner, first throw in
seconds after the gates).
"""
import csv, json, os, re, subprocess, sys, glob
from concurrent.futures import ThreadPoolExecutor

COMPS = {
    "hp_v_rogp": (["Hunter", "Priest"], ["Rogue", "Priest"]),
    "hp_v_warp": (["Hunter", "Priest"], ["Warrior", "Priest"]),
    "hp_v_magp": (["Hunter", "Priest"], ["Mage", "Priest"]),
    "hp_v_wlkp": (["Hunter", "Priest"], ["Warlock", "Priest"]),
    "hp_v_wlkpal": (["Hunter", "Priest"], ["Warlock", "Paladin"]),
    "hp_v_palwar": (["Hunter", "Priest"], ["Paladin", "Warrior"]),
    "hp_v_rogwar": (["Hunter", "Priest"], ["Rogue", "Warrior"]),
    "hp_v_prpal": (["Hunter", "Priest"], ["Priest", "Paladin"]),
    "hp_v_wlkrog": (["Hunter", "Priest"], ["Warlock", "Rogue"]),
    "hp_v_shrog": (["Hunter", "Priest"], ["Shaman", "Rogue"]),
    "hw_v_rogp": (["Hunter", "Warrior"], ["Rogue", "Priest"]),
    "hpw_v_mpr": (["Hunter", "Priest", "Warrior"], ["Mage", "Priest", "Rogue"]),
    "hpw_v_wpr": (["Hunter", "Priest", "Warrior"], ["Warlock", "Priest", "Rogue"]),
    "h_v_war": (["Hunter"], ["Warrior"]),
    "h_v_rog": (["Hunter"], ["Rogue"]),
    "h_v_pri": (["Hunter"], ["Priest"]),
    "h_v_wlk": (["Hunter"], ["Warlock"]),
}

def run(job, binary, outdir):
    name, seed = job
    t1, t2 = COMPS[name]
    d = os.path.join(outdir, f"{name}_{seed}")
    os.makedirs(d, exist_ok=True)
    # assets resolve from cwd: link THIS binary's own tree's assets
    tree_assets = os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(binary))), "assets")
    if not os.path.exists(os.path.join(d, "assets")):
        os.symlink(tree_assets, os.path.join(d, "assets"))
    log = os.path.join(d, "match.txt")
    cfg = os.path.join(d, "cfg.json")
    with open(cfg, "w") as f:
        json.dump({"team1": t1, "team2": t2, "random_seed": seed,
                   "output_path": log, "max_duration_secs": 300}, f)
    if not os.path.exists(log):
        subprocess.run([binary, "--headless", cfg, "--trace-mode", "on"], cwd=d,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
    return analyse(name, seed, d, log)


# Log timestamps count from the start of the 10s countdown.
GATES_OPEN = 10.0

NAME = re.compile(r"(Team \d \w+ #\d(?:'s \w+)?)")


def analyse(name, seed, d, log):
    traces = glob.glob(os.path.join(d, "match_logs", "*_trace.jsonl"))
    ents = {}
    chosen = []
    for tpath in traces:
        for line in open(tpath):
            try:
                v = json.loads(line)
            except Exception:
                continue
            a = v.get("actor")
            if a:
                ents[a["entity_id"]] = (a["team"], a["class"], a.get("slot"))
            t = v.get("target")
            if t and "entity_id" in t:
                ents.setdefault(t["entity_id"], (None, t.get("class"), None))
            o = v.get("outcome") or {}
            if o.get("ability") == "FreezingTrap" and v.get("kind") == "ability_decision":
                chosen.append((v["sim_time"], a["entity_id"], o.get("target_id"),
                               (v.get("target") or {}).get("entity_id"),
                               (v.get("target") or {}).get("distance")))
    lines = open(log).read().splitlines()
    winner = next((l.split(":", 1)[1].strip() for l in lines if l.startswith("Winner:")), "")
    # events
    casts, triggers, removals, breaks = [], [], [], []
    for l in lines:
        m = re.match(r"\[\s*([\d.]+)s\]", l)
        if not m:
            continue
        ts = float(m.group(1))
        if "uses Freezing Trap" in l:
            casts.append(ts)
        elif "Freezing Trap triggers on" in l:
            victim = l.split("triggers on ")[1].split(" —")[0]
            triggers.append((ts, victim))
        elif "Freezing Trap broke from damage" in l:
            breaks.append(ts)
        elif "Freezing Trap" in l and ("[DISPEL]" in l or "[DEVOUR]" in l or "[CLEANSE]" in l
                                        or "removes" in l or "dispels" in l.lower()):
            removals.append((ts, l))
    rows = []
    for i, c in enumerate(casts):
        nxt = casts[i + 1] if i + 1 < len(casts) else 1e9
        trig = next(((t, v) for t, v in triggers if c <= t < nxt), None)
        rem = brk = None
        if trig:
            rem = next(((t, l) for t, l in removals if t >= trig[0] and t < trig[0] + 8.5), None)
            brk = next((t for t in breaks if t >= trig[0] and t < trig[0] + 8.5), None)
        ch = chosen[i] if i < len(chosen) else None
        intended = ents.get(ch[2]) if ch else None
        kill_t = ch[3] if ch else None
        rows.append({
            "comp": name, "seed": seed, "cast_t": c,
            "intended_class": intended[1] if intended else "",
            "intended_is_kill_target": (ch[2] == kill_t) if ch else "",
            "sprung_on": trig[1] if trig else "",
            "sprung_class": (trig[1].split("'s ")[1] if trig and "'s " in trig[1]
                             else trig[1].split()[2]) if trig else "",
            "trigger_delay": round(trig[0] - c, 2) if trig else "",
            "fate": ("removed" if rem else "broke" if brk else "ran_out") if trig else "unsprung",
            "removal": rem[1][:120] if rem else "",
            "removal_delay": round(rem[0] - trig[0], 2) if rem else "",
        })
    first = round(casts[0] - GATES_OPEN, 2) if casts else ""
    return name, seed, rows, len(casts), len(triggers), len(chosen), winner, first


def main(argv):
    binary, outdir, jobs = argv[0], argv[1], int(argv[2])
    seeds = range(int(argv[3])) if len(argv) > 3 else range(10)
    os.makedirs(outdir, exist_ok=True)
    jobs_list = [(n, s) for n in COMPS for s in seeds]
    all_rows, mrows = [], []
    with ThreadPoolExecutor(jobs) as ex:
        for name, seed, rows, nc, nt, nch, winner, first in ex.map(
                lambda j: run(j, binary, outdir), jobs_list):
            all_rows += rows
            mrows.append({"comp": name, "seed": seed, "casts": nc, "triggers": nt, "chosen": nch,
                          "winner": winner, "first_cast_after_gates": first})
    with open(os.path.join(outdir, "traps.csv"), "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=list(all_rows[0].keys()) if all_rows else ["comp"])
        w.writeheader(); w.writerows(all_rows)
    with open(os.path.join(outdir, "matches.csv"), "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=["comp", "seed", "casts", "triggers", "chosen",
                                          "winner", "first_cast_after_gates"])
        w.writeheader(); w.writerows(mrows)
    print("done", len(all_rows), "traps")


if __name__ == "__main__":
    main(sys.argv[1:])
