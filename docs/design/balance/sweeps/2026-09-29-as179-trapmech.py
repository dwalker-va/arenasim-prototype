#!/usr/bin/env python3
"""AS-125 mechanism runner: traced headless matches -> per-trap rows.

usage: trapmech.py [--kill-target N] <binary> <outdir> <jobs> [seeds] [comp,comp,...]
Each match runs in its own cwd (so the seconds-stamped trace cannot collide).
`--kill-target N` sets both teams' configured kill target to slot N (the
graphical client's default is 0), `--kill-target N,M` team 1's to N and team
2's to M, with `-` for none; without it neither team has one.
Writes <outdir>/traps.csv (one row per Freezing Trap thrown: its decided
victim, whom it sprang on, its fate, the incapacitate `duration` applied and
the seconds the victim was actually `held` — the duration, cut short by a
removal or a break — `hunter_hits_held`, the trapping Hunter's own damage
events while it was held, and `hunter_on_victim_held`, the Hunter's decisions
in that time still targeting the victim it held fire on while another enemy
was alive to fight; empty unless the trap sprang on the enemy it was thrown
at) and <outdir>/matches.csv (one row per match: trap counts,
winner, first throw in seconds after the gates). The optional last argument
runs only the named comps.
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
    "hp_v_rogpal": (["Hunter", "Priest"], ["Rogue", "Paladin"]),
    "hp_v_warsh": (["Hunter", "Priest"], ["Warrior", "Shaman"]),
    "hw_v_warp": (["Hunter", "Warrior"], ["Warrior", "Priest"]),
    "hm_v_rogp": (["Hunter", "Mage"], ["Rogue", "Priest"]),
    "hwl_v_rogp": (["Hunter", "Warlock"], ["Rogue", "Priest"]),
    "hr_v_rogp": (["Hunter", "Rogue"], ["Rogue", "Priest"]),
    "hpal_v_rogp": (["Hunter", "Paladin"], ["Rogue", "Priest"]),
    "ph_v_rogp": (["Priest", "Hunter"], ["Rogue", "Priest"]),
    "wh_v_rogp": (["Warrior", "Hunter"], ["Rogue", "Priest"]),
    "palh_v_rogp": (["Paladin", "Hunter"], ["Rogue", "Priest"]),
    "h_v_war": (["Hunter"], ["Warrior"]),
    "h_v_rog": (["Hunter"], ["Rogue"]),
    "h_v_pri": (["Hunter"], ["Priest"]),
    "h_v_wlk": (["Hunter"], ["Warlock"]),
}

KILL_TARGET = (None, None)


def run(job, binary, outdir):
    name, seed = job
    t1, t2 = COMPS[name]
    d = os.path.join(outdir, f"{name}_{seed}")
    os.makedirs(d, exist_ok=True)
    # assets resolve from cwd: link THIS binary's own tree's assets
    tree_assets = os.path.join(os.path.dirname(binary), "assets")
    if not os.path.exists(os.path.join(d, "assets")):
        os.symlink(tree_assets, os.path.join(d, "assets"))
    log = os.path.join(d, "match.txt")
    cfg = os.path.join(d, "cfg.json")
    config = {"team1": t1, "team2": t2, "random_seed": seed,
              "output_path": log, "max_duration_secs": 300}
    for team, slot in zip(("team1_kill_target", "team2_kill_target"), KILL_TARGET):
        if slot is not None:
            config[team] = slot
    with open(cfg, "w") as f:
        json.dump(config, f)
    if not os.path.exists(log):
        subprocess.run([binary, "--headless", cfg, "--trace-mode", "on"], cwd=d,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
    return analyse(name, seed, d, log, t1, t2)


# Log timestamps count from the start of the 10s countdown.
GATES_OPEN = 10.0

NAME = re.compile(r"(Team \d \w+ #\d(?:'s \w+)?)")


def analyse(name, seed, d, log, team1=(), team2=()):
    traces = glob.glob(os.path.join(d, "match_logs", "*_trace.jsonl"))
    ents = {}
    chosen = []
    decisions = []
    for tpath in traces:
        for line in open(tpath):
            try:
                v = json.loads(line)
            except Exception:
                continue
            a = v.get("actor")
            if a:
                # A pet's actor view carries its owner's class; its own kind
                # is the pet decision's top-level `pet_type`.
                ents[a["entity_id"]] = (a["team"], v.get("pet_type") or a["class"], a.get("slot"))
            t = v.get("target")
            if t and "entity_id" in t:
                ents.setdefault(t["entity_id"], (None, t.get("class"), None))
            if v.get("kind") == "ability_decision" and a and v.get("target"):
                decisions.append((v["sim_time"], a["entity_id"], v["target"].get("entity_id")))
            o = v.get("outcome") or {}
            if o.get("ability") == "FreezingTrap" and v.get("kind") == "ability_decision":
                chosen.append((v["sim_time"], a["entity_id"], o.get("target_id"),
                               (v.get("target") or {}).get("entity_id"),
                               (v.get("target") or {}).get("distance")))
    lines = open(log).read().splitlines()
    winner = next((l.split(":", 1)[1].strip() for l in lines if l.startswith("Winner:")), "")
    # events
    casts, triggers, removals, breaks, applied, bubbles, damage, deaths = (
        [], [], [], [], [], [], [], [])
    for l in lines:
        m = re.match(r"\[\s*([\d.]+)s\]", l)
        if not m:
            continue
        ts = float(m.group(1))
        if "uses Freezing Trap" in l:
            casts.append(ts)
        elif "Freezing Trap triggers on" in l:
            victim = l.split("triggers on ")[1].split(" —")[0]
            owner = l.split("[TRAP] ")[1].split("'s Freezing Trap")[0]
            triggers.append((ts, victim, owner))
        elif re.search(r"\] Freezing Trap on .* \(([\d.]+)s", l):
            applied.append((ts, float(re.search(r"\(([\d.]+)s", l).group(1))))
        elif "Freezing Trap broke from damage" in l:
            breaks.append(ts)
        elif "[DEATH]" in l:
            deaths.append((ts, l))
        elif "[DMG]" in l:
            damage.append((ts, l))
        elif "'s Divine Shield removes " in l:
            # The bubble names the debuff count, not the trap: attributed to
            # a trap below only when its own victim is the one bubbling.
            bubbles.append((ts, l))
        elif "Freezing Trap" in l and ("[DISPEL]" in l or "[DEVOUR]" in l or "[CLEANSE]" in l
                                        or "removes" in l or "dispels" in l.lower()):
            removals.append((ts, l))
    rows = []
    for i, c in enumerate(casts):
        nxt = casts[i + 1] if i + 1 < len(casts) else 1e9
        trig = next(((t, v, o) for t, v, o in triggers if c <= t < nxt), None)
        rem = brk = None
        if trig:
            rem = next(((t, l) for t, l in removals if t >= trig[0] and t < trig[0] + 8.5), None)
            bubble = next(((t, l) for t, l in bubbles
                           if trig[0] <= t < trig[0] + 8.5 and f"{trig[1]}'s Divine Shield" in l),
                          None)
            if bubble and (rem is None or bubble[0] < rem[0]):
                rem = bubble
            brk = next((t for t in breaks if t >= trig[0] and t < trig[0] + 8.5), None)
        dur = next((a for t, a in applied if trig and trig[0] <= t < trig[0] + 0.5), None)
        if trig and dur is not None:
            ends = [dur] + ([rem[0] - trig[0]] if rem else []) + ([brk - trig[0]] if brk else [])
            held = round(min(ends), 2)
            hits = sum(1 for t, l in damage
                       if trig[0] <= t <= trig[0] + held and f"] {trig[2]}'s " in l)
        else:
            held = hits = ""
        ch_i = chosen[i] if i < len(chosen) else None
        on_victim = ""
        intended_i = ents.get(ch_i[2]) if ch_i else None
        sprung_class = ((trig[1].split("'s ")[1] if "'s " in trig[1] else trig[1].split()[2])
                        if trig else "")
        # Only a trap that sprang on the enemy it was thrown at: the decision
        # trace names that enemy's entity, and no other.
        if (trig and held != "" and ch_i and ch_i[2] is not None and intended_i
                and intended_i[1] == sprung_class):
            start = trig[0] - GATES_OPEN
            team = trig[1].split()[1]
            classes = set(team1 if team == "1" else team2)
            others = len(team1 if team == "1" else team2) - 1
            # Primaries only: the Hunter is right to hold fire when nothing
            # but a pet is left beside its frozen victim.
            def others_alive(t):
                dead = sum(1 for dt, l in deaths if dt - GATES_OPEN <= t
                           and f"] Team {team} " in l and "has been eliminated" in l
                           and l.split(f"] Team {team} ")[1].split()[0] in classes)
                return others - dead > 0
            on_victim = sum(1 for t, actor, tgt in decisions
                            if actor == ch_i[1] and tgt == ch_i[2]
                            and start <= t <= start + held and others_alive(t))
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
            "duration": dur if dur is not None else "",
            "held": held,
            "breaker": (next((l.split("] ", 2)[-1].split("'s ")[0] + "'s " + l.split("'s ")[1].split(" ")[0] for t, l in damage if brk is not None and abs(t - brk) < 0.03 and trig[1] in l), "?") if brk else ""),
            "hunter_hits_held": hits,
            "hunter_on_victim_held": on_victim,
        })
    first = round(casts[0] - GATES_OPEN, 2) if casts else ""
    return name, seed, rows, len(casts), len(triggers), len(chosen), winner, first


def main(argv):
    global KILL_TARGET
    if argv and argv[0] == "--kill-target":
        slots = argv[1].split(",")
        slots = slots * 2 if len(slots) == 1 else slots
        KILL_TARGET = tuple(None if s == "-" else int(s) for s in slots)
        argv = argv[2:]
    if len(argv) > 4:
        keep = set(argv[4].split(","))
        for k in list(COMPS):
            if k not in keep:
                del COMPS[k]
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
