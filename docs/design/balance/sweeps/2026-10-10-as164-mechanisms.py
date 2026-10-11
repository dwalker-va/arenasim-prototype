#!/usr/bin/env python3
"""AS-164 (0.7 milestone sweep): the mechanism tables, from tests/milestone_mechanisms.rs rows.

    mechanisms.py economy  mech_basic.rows.jsonl.gz
    mechanisms.py druid    mech_basic.rows.jsonl.gz [mech_nagrand.rows.jsonl.gz]
    mechanisms.py hunter   mech_hunter.rows.jsonl.gz   (the Hunter panel alone)
    mechanisms.py duels    mech_duels.rows.jsonl.gz
    mechanisms.py h2h_hunter mech_h2h_hunter.rows.jsonl.gz

Every per-match behaviour is reported as a RATE (per match, per minute alive,
or a share of a stated denominator), with the count it came from beside it.
Times are seconds after the gates.
"""

import gzip
import json
import math
import statistics
import sys
from collections import defaultdict

HEALERS = ["Priest", "Paladin", "Shaman", "Druid"]
DPS = ["Warrior", "Mage", "Rogue", "Warlock", "Hunter"]
SHIFT_COST = 25.0  # Travel Form's mana cost (the harness prints it on every run)


def rows(path):
    """One dict per match. Reads the committed `.jsonl.gz` or a plain `.jsonl`."""
    opener = gzip.open if path.endswith(".gz") else open
    with opener(path, "rt") as f:
        for line in f:
            if line.strip():
                yield json.loads(line)


def wilson(k, n, z=1.96):
    if n == 0:
        return (0.0, 1.0)
    p = k / n
    d = 1 + z * z / n
    c = (p + z * z / (2 * n)) / d
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return (max(0.0, c - h), min(1.0, c + h))


def share(k, n):
    if n == 0:
        return "n/a"
    lo, hi = wilson(k, n)
    return f"{100 * k / n:.1f}% [{100 * lo:.1f}, {100 * hi:.1f}] ({k}/{n})"


def side_won(m, team):
    return m["winner"] == f"team{team}"


def healer_units(m):
    """(healer unit, its team string) for the panel's healer side: the team
    that is <DPS>+<healer>. In a panel row the opponent can carry a healer too;
    each healer on either side is a sample, since the panel fields every
    healer against the same opponent set."""
    for u in m["units"]:
        if u["class"] in HEALERS:
            yield u


# ----------------------------------------------------------------------------
def economy(path):
    """Mana longevity per healer class. Samples are the healer on the PANEL side
    (the <DPS>+<healer> team) only, so every healer class meets the same
    opponent set."""
    marks = [30, 60, 90, 120, 150]
    frac = {h: {t: [] for t in marks} for h in HEALERS}
    oom_by = {h: {t: [0, 0] for t in marks} for h in HEALERS}
    pre_half = defaultdict(list)
    first = defaultdict(list)
    innervates = []
    n_match = defaultdict(int)
    for m in rows(path):
        a, side = m["label"].split("|")
        panel_team = a.split("_vs_")[0]
        panel_side = 1 if side == "a" else 2
        for u in m["units"]:
            if u["team"] != panel_side or u["class"] not in HEALERS:
                continue
            h = u["class"]
            n_match[h] += 1
            pre_half[h].append(u["oom_secs_before_half_dampening"])
            if u["first_oom"] is not None:
                first[h].append(u["first_oom"])
            if h == "Druid":
                innervates.append(u["casts"].get("Innervate", 0))
            for t in marks:
                i = t // 5 - 1
                if i < len(u["mana_frac"]):
                    frac[h][t].append(u["mana_frac"][i])
                    oom_by[h][t][1] += 1
                    if u["first_oom"] is not None and u["first_oom"] <= t:
                        oom_by[h][t][0] += 1
    print("Mean mana fraction (healers alive, match running) at seconds after the gates:")
    print()
    print("| Healer | " + " | ".join(f"{t}s" for t in marks) + " |")
    print("|---|" + "---|" * len(marks))
    for h in HEALERS:
        cells = [f"{100 * statistics.mean(frac[h][t]):.0f}% (n={len(frac[h][t])})" if frac[h][t] else "n/a" for t in marks]
        print(f"| {h} | " + " | ".join(cells) + " |")
    print()
    print("Share that has been out of mana for its cheapest heal at least once by that time "
          "(of healers alive with the match running):")
    print()
    print("| Healer | " + " | ".join(f"by {t}s" for t in marks) + " | median first OOM, of those that go OOM |")
    print("|---|" + "---|" * (len(marks) + 1))
    for h in HEALERS:
        cells = []
        for t in marks:
            k, n = oom_by[h][t]
            cells.append(f"{100 * k / n:.0f}% (n={n})" if n else "n/a")
        med = f"{statistics.median(first[h]):.1f}s (n={len(first[h])})" if first[h] else "never"
        print(f"| {h} | " + " | ".join(cells) + f" | {med} |")
    print()
    print("Seconds out of mana before dampening reaches 50% (135s), per healer-match:")
    for h in HEALERS:
        v = pre_half[h]
        if not v:
            continue
        print(f"  {h}: mean {statistics.mean(v):.1f}s, median {statistics.median(v):.1f}s, n={len(v)}")
    if innervates:
        print(f"Druid Innervate casts per match: {statistics.mean(innervates):.2f} "
              f"(share casting at least one: {share(sum(1 for x in innervates if x), len(innervates))})")


# ----------------------------------------------------------------------------
def druid(*paths):
    for path in paths:
        print(f"## {path}")
        n_druid = 0
        alive_min = 0.0
        cyc_attempt = cyc_land = 0
        matches_with_cyc = 0
        roots_attempt = 0
        deaths = deaths_shifted = 0
        losses = losses_shifted_death = 0
        shifts = 0
        shifted_secs = 0.0
        root_eps = root_broken = 0
        unbroken_unaffordable_start = unbroken_never_affordable = unbroken = 0
        druid_lockouts = 0
        lock = defaultdict(lambda: [0, 0.0])  # healer -> [lockouts, minutes alive]
        for m in rows(path):
            for u in m["units"]:
                if u["class"] in HEALERS:
                    lock[u["class"]][0] += u["lockouts_taken"]
                    lock[u["class"]][1] += u["alive_secs"] / 60
            for u in m["units"]:
                if u["class"] != "Druid":
                    continue
                n_druid += 1
                alive_min += u["alive_secs"] / 60
                a = u["casts"].get("Cyclone", 0)
                cyc_attempt += a
                landed = sum(v["cyclones_taken"] for v in m["units"] if v["team"] != u["team"])
                cyc_land += landed
                matches_with_cyc += 1 if a else 0
                roots_attempt += u["casts"].get("Entangling Roots", 0)
                shifts += u["shifts"]
                shifted_secs += u["shifted_secs"]
                if u["death"] is not None:
                    deaths += 1
                    deaths_shifted += 1 if u["died_shifted"] else 0
                if not side_won(m, u["team"]) and m["winner"] != "draw":
                    losses += 1
                    if u["death"] is not None and u["died_shifted"]:
                        losses_shifted_death += 1
                for r in u["roots"]:
                    root_eps += 1
                    if r["broken_by_shift"]:
                        root_broken += 1
                    else:
                        unbroken += 1
                        if r["mana_at_start"] < SHIFT_COST:
                            unbroken_unaffordable_start += 1
                        if r["max_mana_during"] < SHIFT_COST:
                            unbroken_never_affordable += 1
        print(f"Druid-matches: {n_druid}; Druid minutes alive: {alive_min:.0f}")
        print(f"Cyclone attempts per match: {cyc_attempt / n_druid:.3f} ({cyc_attempt}); "
              f"landed per match: {cyc_land / n_druid:.3f} ({cyc_land}); per Druid-minute alive: {cyc_land / alive_min:.3f}")
        print(f"  matches with at least one Cyclone attempt: {share(matches_with_cyc, n_druid)}")
        print(f"Entangling Roots attempts per match: {roots_attempt / n_druid:.2f}")
        print(f"Travel Form shifts per match: {shifts / n_druid:.2f}; seconds shifted per match: {shifted_secs / n_druid:.1f}")
        print(f"Druid deaths: {deaths} ({100 * deaths / n_druid:.1f}% of Druid-matches); died shifted: {share(deaths_shifted, deaths)} of deaths")
        print(f"Druid-side losses: {losses}; of them the Druid died shifted: {share(losses_shifted_death, losses)}")
        print(f"Roots on the Druid: {root_eps} ({root_eps / n_druid:.2f}/match); broken by a shift: {share(root_broken, root_eps)}")
        print(f"  never broken: {unbroken}; of them the Druid could not afford the {SHIFT_COST:.0f}-mana shift "
              f"at the root's onset: {share(unbroken_unaffordable_start, unbroken)}; at ANY frame of the root: "
              f"{share(unbroken_never_affordable, unbroken)}")
        print("Interrupt lockouts taken per minute alive, by healer class (all healers in these matches):")
        for h in HEALERS:
            k, mins = lock[h]
            if mins:
                print(f"  {h}: {k / mins:.3f}/min ({k} over {mins:.0f} min)")
        print()


# ----------------------------------------------------------------------------
def hunter(*paths):
    """Healer Freezing Traps per match by enemy comp, and the Frost-Trap-first lockout."""
    per = defaultdict(lambda: defaultdict(float))
    for path in paths:
        for m in rows(path):
            for u in m["units"]:
                if u["class"] != "Hunter":
                    continue
                enemy = [v["class"] for v in m["units"] if v["team"] != u["team"]]
                eh = [c for c in enemy if c in HEALERS]
                if len(eh) != 1:
                    continue
                key = (eh[0], "+".join(sorted(c for c in enemy if c not in HEALERS)) or "-")
                d = per[key]
                d["n"] += 1
                d["won"] += 1 if side_won(m, u["team"]) else 0
                tt = u["trap_triggers"]
                hf = tt.get(f"Freezing:{eh[0]}", 0)
                d["healer_freezes"] += hf
                d["other_freezes"] += sum(v for k, v in tt.items() if k.startswith("Freezing:")) - hf
                d["frost_triggers"] += sum(v for k, v in tt.items() if k.startswith("Frost:"))
                d["zero_healer_freeze"] += 1 if hf == 0 else 0
                uses = u["trap_uses"]
                d["freezing_thrown"] += sum(1 for _, n in uses if n == "Freezing Trap")
                d["frost_thrown"] += sum(1 for _, n in uses if n == "Frost Trap")
                if uses and uses[0][1] == "Frost Trap":
                    d["frost_first"] += 1
                    if hf == 0:
                        d["frost_first_no_healer_freeze"] += 1
    print("Per Hunter-match, by enemy comp (enemy healer, enemy DPS):")
    print()
    print("| enemy healer | enemy DPS | Hunter-matches | Hunter side won | healer freezes /match | other freezes /match | "
          "Frost Trap triggers /match | no healer freeze | Frost Trap thrown first | ...and no healer freeze |")
    print("|---|---|---|---|---|---|---|---|---|---|")
    tot = defaultdict(float)
    for k in sorted(per):
        d = per[k]
        n = d["n"]
        for f, v in d.items():
            tot[f] += v
        print(f"| {k[0]} | {k[1]} | {int(n)} | {100 * d['won'] / n:.0f}% | {d['healer_freezes'] / n:.2f} | "
              f"{d['other_freezes'] / n:.2f} | {d['frost_triggers'] / n:.2f} | {100 * d['zero_healer_freeze'] / n:.0f}% | "
              f"{100 * d['frost_first'] / n:.0f}% | {100 * d['frost_first_no_healer_freeze'] / n:.0f}% |")
    n = tot["n"]
    print()
    print(f"ALL: {int(n)} Hunter-matches; healer freezes/match {tot['healer_freezes'] / n:.2f}; "
          f"no healer freeze {share(int(tot['zero_healer_freeze']), int(n))}; "
          f"Frost Trap first {share(int(tot['frost_first']), int(n))}; "
          f"of those, no healer freeze {share(int(tot['frost_first_no_healer_freeze']), int(tot['frost_first']))}")
    by_h = defaultdict(lambda: [0, 0.0])
    for (h, _), d in per.items():
        by_h[h][0] += d["n"]
        by_h[h][1] += d["healer_freezes"]
    print("Healer freezes per match by enemy healer: " + ", ".join(
        f"{h} {v[1] / v[0]:.2f} (n={int(v[0])})" for h, v in sorted(by_h.items())))


def duels(path):
    """1v1 Druid against each healer: per Druid-match, its damage dealt, its
    Moonfires, and its Lifeblooms. In a 1v1 the Druid has no teammate, so
    every Lifebloom it casts is on itself."""
    by = defaultdict(lambda: defaultdict(list))
    for m in rows(path):
        for u in m["units"]:
            if u["class"] != "Druid":
                continue
            opp = next(v["class"] for v in m["units"] if v["team"] != u["team"])
            d = by[opp]
            d["damage"].append(u["damage_dealt"])
            d["moonfire"].append(u["casts"].get("Moonfire", 0))
            d["lifebloom"].append(u["casts"].get("Lifebloom", 0))
            d["draw"].append(m["winner"] == "draw")
    print("| Druid vs | Druid-matches | draws | damage dealt /match | Moonfire /match | self-Lifebloom /match |")
    print("|---|---|---|---|---|---|")
    for opp in HEALERS:
        d = by.get(opp)
        if not d:
            continue
        n = len(d["damage"])
        print(f"| {opp} | {n} | {100 * sum(d['draw']) / n:.0f}% | {statistics.mean(d['damage']):.0f} "
              f"(median {statistics.median(d['damage']):.0f}) | {statistics.mean(d['moonfire']):.1f} | "
              f"{statistics.mean(d['lifebloom']):.1f} |")


def h2h_hunter(path):
    """The head-to-head hunter cell, observed: per arm, the Hunter's traps and
    each Priest's dispels, per match."""
    by = defaultdict(lambda: defaultdict(list))
    for m in rows(path):
        d = by[m["label"]]
        d["rp_won"].append(m["winner"] == "team2")
        for u in m["units"]:
            if u["team"] == 1 and u["class"] == "Hunter":
                d["freezing"].append(u["casts"].get("Freezing Trap", 0))
                d["frost"].append(u["casts"].get("Frost Trap", 0))
            if u["class"] == "Priest":
                d[f"dispel_t{u['team']}"].append(u["casts"].get("Dispel Magic", 0))
    print("| arm | matches | Rogue+Priest won | Hunter Freezing Trap /match | Hunter Frost Trap /match | "
          "Hunter-side Priest dispels /match | Rogue-side Priest dispels /match |")
    print("|---|---|---|---|---|---|---|")
    for arm in sorted(by):
        d = by[arm]
        n = len(d["rp_won"])
        mean = lambda k: statistics.mean(d[k]) if d[k] else 0.0  # noqa: E731
        print(f"| {arm} | {n} | {sum(d['rp_won'])}/{n} | {mean('freezing'):.2f} | {mean('frost'):.2f} | "
              f"{mean('dispel_t1'):.2f} | {mean('dispel_t2'):.2f} |")


def main():
    cmd, args = sys.argv[1], sys.argv[2:]
    {
        "duels": lambda: duels(args[0]),
        "h2h_hunter": lambda: h2h_hunter(args[0]),"economy": lambda: economy(args[0]), "druid": lambda: druid(*args), "hunter": lambda: hunter(*args)}[cmd]()


if __name__ == "__main__":
    main()
