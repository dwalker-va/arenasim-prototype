#!/usr/bin/env python3
"""AS-383 diagnosis: per landed Fear/Polymorph and per Frost Shock on the
TeamPlan Priest, what the Priest's posture, crowd-control field and reach
repair were when the cast began.

    2026-10-22-as383-scan.py <binary> <map> <seeds> <out.jsonl> [jobs] [enemy healer]

The binary must carry `2026-10-22-as383-instrument-and-knobs.patch` (the
`AS383_LOG` instrument; its `AS383_EXP` knobs are experiments, inert unset).
Run with `AS383_OFFSET=0` (the instrument and the match log share a clock).
Plays every <DPS>+Priest (TeamPlan) v <DPS>+<enemy healer> (Legacy) cell at
seeds 0..<seeds>, the AS-292 logscan's cells, and writes one row per match:
each landed Fear/Polymorph and each Frost Shock on the Priest with the
Priest's posture, field and repair state at the cast's start, the scream-dip
episodes, and the posture time budget. `2026-10-22-as383-reduce.py` reads it.
"""
import json, math, os, re, subprocess, sys, tempfile
from concurrent.futures import ThreadPoolExecutor

binary, mapn, seeds, outp = sys.argv[1], sys.argv[2], int(sys.argv[3]), sys.argv[4]
jobs_n = int(sys.argv[5]) if len(sys.argv) > 5 else 4
enemy_healer = sys.argv[6] if len(sys.argv) > 6 else "Shaman"
DPS = ["Warrior", "Mage", "Rogue", "Warlock", "Hunter"]
P = "Team 1 Priest #2"
TS = re.compile(r"^\[\s*([\d.]+)s\]")
OFFSET = float(os.environ.get("AS383_OFFSET", "10.0"))  # instrument t - log t
CASTERS = {"Fear": "Warlock", "Polymorph": "Mage", "Death Coil": "Warlock"}


def dist(a, b):
    return math.hypot(a[0] - b[0], a[1] - b[1])


def at(frames, t):
    """The last instrument frame at or before t (instrument clock)."""
    lo, hi = 0, len(frames) - 1
    if not frames or frames[0]["t"] > t:
        return None
    while lo < hi:
        mid = (lo + hi + 1) // 2
        if frames[mid]["t"] <= t:
            lo = mid
        else:
            hi = mid - 1
    frames[lo]["_i"] = lo
    return frames[lo]


def unit(fr, cls, ally=False):
    for u in fr["u"]:
        if u["cls"] == cls and u["ally"] == ally and not u["pet"]:
            return u
    return None


def run(job):
    t1, t2, s = job
    tmp = tempfile.mkdtemp(dir=os.path.dirname(os.path.abspath(outp)))
    cfgp, logp, instp = f"{tmp}/c.json", f"{tmp}/m.txt", f"{tmp}/i.jsonl"
    with open(cfgp, "w") as f:
        json.dump({"team1": t1, "team2": t2, "map": mapn, "team1_ai_profile": "TeamPlan",
                   "team2_ai_profile": "Legacy", "random_seed": s, "max_duration_secs": 300}, f)
    env = dict(os.environ, AS383_LOG=instp)
    subprocess.run([binary, "--headless", cfgp, "--output", logp], env=env,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
    frames = []
    with open(instp) as f:
        for line in f:
            try:
                frames.append(json.loads(line))
            except ValueError:
                pass
    txt = open(logp).read().splitlines()
    for p_ in (cfgp, logp, instp):
        os.remove(p_)
    os.rmdir(tmp)

    rows = []
    begins = []
    heal = 0
    win = 0
    for line in txt:
        m = TS.match(line)
        t = float(m.group(1)) if m else None
        mm = re.search(r"\[CAST\] (Team 2 \w+ #\d) begins casting (Fear|Polymorph) on " + re.escape(P), line)
        if mm:
            begins.append((t, mm.group(1), mm.group(2)))
        mm = re.search(r"\[CC\] (Fear|Polymorph|Death Coil) on " + re.escape(P), line)
        if mm and t is not None:
            ab = mm.group(1)
            start = None
            for b in reversed(begins):
                if b[2] == ab and b[0] <= t:
                    start = b[0]
                    break
            if start is None:
                start = t  # instant (Death Coil)
            rows.append(("cc", ab, start, t))
        mm = re.search(r"\[CAST\] Team 2 Shaman #\d casts Frost Shock on " + re.escape(P), line)
        if mm and t is not None:
            rows.append(("fs", "Frost Shock", t, t))
        if line.startswith("[") and f"{P}'s" in line and "heals" in line:
            h = re.search(r"for (\d+)", line)
            if h:
                heal += int(h.group(1))
        w = re.search(r"Winner: (.*)", line)
        if w:
            win = 1 if "Team 1" in w.group(1) else 0
    begun = {}
    for b in begins:
        begun[b[2]] = begun.get(b[2], 0) + 1

    events = []
    for kind, ab, start, land in rows:
        fr = at(frames, start + OFFSET)
        if fr is None:
            continue
        fr_prev = at(frames, start + OFFSET - 1.0)
        ccls = CASTERS.get(ab, enemy_healer if kind == "fs" else None)
        caster = unit(fr, ccls)
        ally = next((u for u in fr["u"] if u["ally"] and not u["pet"]), None)
        ev = {"kind": kind, "ab": ab, "t": start, "post": fr["post"], "trig": fr["trig"],
              "onset": fr["onset"], "repair": fr["repair"], "kept": fr["kept"] is not None,
              "goal": fr["goal"][0] if fr["goal"] else None}
        if caster:
            ev["d_caster"] = dist(fr["p"], caster["p"])
            ev["caster_tgt_me"] = caster["tme"]
            if fr_prev:
                cprev = unit(fr_prev, ccls)
                if cprev:
                    # who closed the distance over the last second
                    ev["d_prev"] = dist(fr_prev["p"], cprev["p"])
                    ev["closed_by_me"] = dist(fr_prev["p"], cprev["p"]) - dist(fr["p"], cprev["p"])
                    ev["closed_by_caster"] = dist(fr_prev["p"], cprev["p"]) - dist(fr_prev["p"], caster["p"])
                    ev["prev_post"] = fr_prev["post"]
                    thp = [x for x in fr_prev["thr"] if x["ab"] == ab.replace(" ", "")]
                    ev["listed_prev"] = bool(thp)
        if ally:
            ev["d_ally"] = dist(fr["p"], ally["p"])
            if caster:
                ev["ally_to_caster"] = dist(ally["p"], caster["p"])
            ev["ally_cls"] = ally["cls"]
        if fr["goal"] and fr["goal"][0] == "pt" and caster:
            g = (fr["goal"][1], fr["goal"][2])
            ev["goal_to_caster"] = dist(g, caster["p"])
            ev["me_to_goal"] = dist(fr["p"], g)
        # how long the priest has been continuously inside this disc (outer)
        k = fr["_i"]
        j = k
        while j > 0 and any(x["ab"] == ab.replace(" ", "") and x["outer"] for x in frames[j - 1]["thr"]):
            j -= 1
        ev["in_disc_for"] = fr["t"] - frames[j]["t"]
        ev["entered_post"] = frames[j]["post"]
        thr = [x for x in fr["thr"] if x["ab"] == ab.replace(" ", "")]
        ev["listed"] = bool(thr)
        if thr:
            ev["inner"] = thr[0]["inner"]
            ev["outer"] = thr[0]["outer"]
        # readiness / field over the cast (frames between start and land)

        # find the last frame, before start, at which the priest was outside the outer disc
        events.append(ev)

    # dip transitions
    dips = {"enter": 0}
    for a, b in zip(frames, frames[1:]):
        if a["post"] != "Dip" and b["post"] == "Dip":
            dips["enter"] += 1
        if a["post"] == "Dip" and b["post"] != "Dip":
            dips["to_" + b["post"]] = dips.get("to_" + b["post"], 0) + 1
    dips["scream"] = sum(1 for l in txt if (P + " casts Psychic Scream") in l)
    # dip episodes: did one end in a scream that feared the enemy healer, and
    # what did the Priest take during it and the 5s after?
    screams = []
    healer_feared = []
    for line in txt:
        m = TS.match(line)
        if not m:
            continue
        t = float(m.group(1))
        if (P + " casts Psychic Scream") in line:
            screams.append(t)
        if "[CC]" in line and "Psychic Scream" in line and f"Team 2 {enemy_healer}" in line:
            healer_feared.append(t)
    start = None
    for a, b in zip(frames, frames[1:]):
        if a["post"] != "Dip" and b["post"] == "Dip":
            start = b["t"]
        if a["post"] == "Dip" and b["post"] != "Dip" and start is not None:
            end = b["t"]
            sc = any(end - 0.7 <= s <= end + 0.7 for s in screams)
            hf = any(end - 0.7 <= s <= end + 1.0 for s in healer_feared)
            took = sum(1 for e in events if start <= e["t"] <= end + 5.0)
            dips.setdefault("ep", []).append([round(end - start, 2), b["post"], sc, hf, took])
            start = None
    # per-match time budget
    post_t = {}
    in_fs = 0.0
    in_disc = 0.0
    shaman_d = []
    repairs = {}
    last = None
    for fr in frames:
        if fr["t"] < OFFSET:
            last = fr
            continue
        dt = fr["t"] - last["t"] if last else 0.0
        last = fr
        post_t[fr["post"]] = post_t.get(fr["post"], 0) + dt
        sh = unit(fr, enemy_healer)
        if sh:
            d = dist(fr["p"], sh["p"])
            shaman_d.append(d)
            if d < 20.0:
                in_fs += dt
        if any(x["outer"] for x in fr["thr"]):
            in_disc += dt
        r = (fr["repair"] or "-").split(" ")[0]
        repairs[(fr["post"], r)] = repairs.get((fr["post"], r), 0) + dt
    return {"cell": f"{'+'.join(t1)}_v_{'+'.join(t2)}", "seed": s, "win": win, "heal": heal,
            "begun": begun, "dips": dips, "events": events, "post_t": post_t, "in_fs20": in_fs,
            "in_disc": in_disc,
            "sham_d_mean": sum(shaman_d) / len(shaman_d) if shaman_d else None,
            "repairs": {f"{k[0]}:{k[1]}": v for k, v in repairs.items()}}


jobs = [([d1, "Priest"], [d2, enemy_healer], s) for d1 in DPS for d2 in DPS for s in range(seeds)]
with ThreadPoolExecutor(jobs_n) as ex, open(outp, "w") as out:
    for r in ex.map(run, jobs):
        out.write(json.dumps(r) + "\n")
        out.flush()
