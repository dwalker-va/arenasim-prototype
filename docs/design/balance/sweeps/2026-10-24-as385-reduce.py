#!/usr/bin/env python3
"""AS-385 reducer for `tests/team_sight_cohesion.rs` rows.

    2026-10-24-as385-reduce.py rows.jsonl[.gz] [more rows ...]

Prints the tables of `docs/design/2026-10-24-team-sight-cohesion.md` §2, keyed
by profile x map (x format where it matters):

1. mutual sight: DPS blind to its own healer (share of time, while dying, while
   in danger) and the episodes' "who broke it", overall and while in danger;
2. self-CC from lining: per class, the share of its free time spent blind to
   the unit it should damage (DPS, target within reach) or heal (healer,
   anchor below 80%), set against the share it spent hard-CC'd;
3. pillar stance today: team frame shares (all hidden, all hidden and
   mutually sighted, the healer hidden with a DPS exposed, ...);
4. downtime: what a lull converts into per healer class (team health and
   healer mana per lull second), and healers' mana over the match;
5. pillar deaths by cause.
"""
import collections
import gzip
import json
import sys


def rows(paths):
    for p in paths:
        op = gzip.open if p.endswith(".gz") else open
        with op(p, "rt") as f:
            for line in f:
                line = line.strip()
                if line:
                    yield json.loads(line)


def fmt_of(r):
    return "3v3" if r["team1"].count("+") == 2 else "2v2"


def pct(a, b):
    return f"{100 * a / b:.1f}%" if b > 0 else "-"


def main(paths):
    R = list(rows(paths))
    keys = sorted({(r["profile"], r["map"]) for r in R})

    # ---- 1. mutual sight -------------------------------------------------
    print("## 1. Mutual sight: DPS blind to its own healer (post-contact)\n")
    print("| profile | map | fmt | blind share | while dying | while in danger | episodes >=1s per DPS-match | broke dps/healer/both/joint | in-danger episodes: dps/healer/both/joint |")
    print("|---|---|---|---|---|---|---|---|---|")
    for prof, m in keys:
        for fm in ("2v2", "3v3"):
            sub = [r for r in R if r["profile"] == prof and r["map"] == m and fmt_of(r) == fm]
            if not sub:
                continue
            a = collections.Counter()
            dps_matches = 0
            for r in sub:
                for u in r["units"]:
                    if u["role"] != "healer" and u["healer_secs"] > 0:
                        dps_matches += 1
                        for k in ("healer_secs", "blind_healer", "dying", "blind_healer_dying",
                                  "danger", "blind_healer_danger"):
                            a[k] += u[k]
            eps = [e for r in sub for e in r["pairs"] if e["secs"] >= 1.0]
            br = collections.Counter(e["broke"] for e in eps)
            brd = collections.Counter(e["broke"] for e in eps if e["dps_danger"])
            nb, nd = sum(br.values()), sum(brd.values())
            sh = lambda c, n: " / ".join(f"{100 * c[k] / n:.0f}" if n else "-" for k in ("dps", "healer", "both", "joint"))
            print(f"| {prof} | {m} | {fm} | {pct(a['blind_healer'], a['healer_secs'])} | "
                  f"{pct(a['blind_healer_dying'], a['dying'])} | {pct(a['blind_healer_danger'], a['danger'])} | "
                  f"{len(eps) / max(dps_matches, 1):.2f} | {sh(br, nb)} (n={nb}) | {sh(brd, nd)} (n={nd}) |")
    print()
    print("### 1b. By DPS class, all formats (blind to healer share / while in danger / episodes>=1s per DPS-match / share broken by the DPS's step)\n")
    print("| profile | map | class | blind | in danger | eps/match | broke by dps |")
    print("|---|---|---|---|---|---|---|")
    for prof, m in keys:
        sub = [r for r in R if r["profile"] == prof and r["map"] == m]
        per = collections.defaultdict(collections.Counter)
        nmatch = collections.Counter()
        for r in sub:
            for u in r["units"]:
                if u["role"] != "healer" and u["healer_secs"] > 0:
                    nmatch[u["class"]] += 1
                    for k in ("healer_secs", "blind_healer", "danger", "blind_healer_danger"):
                        per[u["class"]][k] += u[k]
            for e in r["pairs"]:
                if e["secs"] >= 1.0:
                    per[e["dps"]]["eps"] += 1
                    per[e["dps"]]["eps_dps"] += e["broke"] in ("dps", "both")
        for c in sorted(per):
            p = per[c]
            print(f"| {prof} | {m} | {c} | {pct(p['blind_healer'], p['healer_secs'])} | "
                  f"{pct(p['blind_healer_danger'], p['danger'])} | {p['eps'] / max(nmatch[c], 1):.2f} | "
                  f"{pct(p['eps_dps'], p['eps'])} |")
    print()
    print("### 1c. Episodes >=1s by the healer's posture and traced mode as sight was lost (TeamPlan): who broke it\n")
    print("| profile | map | posture | mode | episodes | median s | dps / healer / both / joint | DPS within 1yd of skin |")
    print("|---|---|---|---|---|---|---|---|")
    for prof, m in keys:
        g = collections.defaultdict(list)
        for r in R:
            if r["profile"] == prof and r["map"] == m:
                for e in r["pairs"]:
                    if e["secs"] >= 1.0:
                        g[(e["healer_posture"], e["healer_mode"])].append(e)
        for k in sorted(g):
            v = g[k]
            br = collections.Counter(e["broke"] for e in v)
            med = sorted(e["secs"] for e in v)[len(v) // 2]
            skin = sum(e["dps_skin"] <= 1.0 for e in v)
            print(f"| {prof} | {m} | {k[0]} | {k[1]} | {len(v)} | {med:.1f} | "
                  + " / ".join(f"{100 * br[x] / len(v):.0f}" for x in ("dps", "healer", "both", "joint"))
                  + f" | {pct(skin, len(v))} |")
    print()

    # ---- 2. self-CC from lining -------------------------------------------
    print("## 2. Self-CC from lining, against enemy hard CC (post-contact, share of alive time)\n")
    print("| profile | map | class | hard-CC'd | lining (blind to target in reach / anchor hurt, free) | lining as share of duty time | blind to target any range | DPS step broke the line |")
    print("|---|---|---|---|---|---|---|---|")
    for prof, m in keys:
        sub = [r for r in R if r["profile"] == prof and r["map"] == m]
        per = collections.defaultdict(collections.Counter)
        for r in sub:
            for u in r["units"]:
                p = per[u["class"]]
                p["secs"] += u["secs"]
                p["hard"] += u["hard_cc"]
                if u["role"] == "healer":
                    p["lining"] += u["lining_anchor_hurt"]
                    p["duty"] += u["hurt"]
                else:
                    p["lining"] += u["lining_target"]
                    p["duty"] += u["in_reach"]
                    p["blind_any"] += u["blind_target"]
                    p["has_target"] += u["has_target"]
            for e in r["lines"]:
                if e["secs"] >= 1.0 and e["in_reach"]:
                    per[e["class"]]["lines"] += 1
                    per[e["class"]]["lines_dps"] += e["broke"] in ("dps", "both")
        for c in sorted(per):
            p = per[c]
            healer = c in ("Priest", "Paladin", "Shaman", "Druid")
            print(f"| {prof} | {m} | {c} | {pct(p['hard'], p['secs'])} | {pct(p['lining'], p['secs'])} | "
                  f"{pct(p['lining'], p['duty'])} | {'-' if healer else pct(p['blind_any'], p['has_target'])} | "
                  f"{'-' if healer else pct(p['lines_dps'], p['lines']) + ' (n=' + str(p['lines']) + ')'} |")
    print()

    print("### 2b. Ranged DPS lines on a target within reach (episodes >= 0.25s): by the target's role, who broke it, and whether another enemy stood in reach and sight\n")
    print("| profile | map | target | episodes | seconds | dps / target / both / joint | alternative in sight (share of the seconds) |")
    print("|---|---|---|---|---|---|---|")
    for prof, m in keys:
        per = collections.defaultdict(collections.Counter)
        for r in R:
            if r["profile"] == prof and r["map"] == m:
                for e in r["lines"]:
                    if not e["in_reach"] or e["class"] not in ("Mage", "Warlock", "Hunter"):
                        continue
                    tg = e["target"]
                    k = "healer" if tg in ("Priest", "Paladin", "Shaman", "Druid") else ("pet" if tg.endswith("Pet") else "dps")
                    p = per[k]
                    p["n"] += 1
                    p["s"] += e["secs"]
                    p[e["broke"]] += 1
                    p["alt"] += e.get("alt_share", 0.0) * e["secs"]
        for k in sorted(per):
            p = per[k]
            print(f"| {prof} | {m} | {k} | {p['n']} | {p['s']:.0f} | "
                  + " / ".join(f"{100 * p[b] / p['n']:.0f}" for b in ("dps", "target", "both", "joint"))
                  + f" | {pct(p['alt'], p['s'])} |")
    print()

    # ---- 3. pillar stance today -------------------------------------------
    print("## 3. Pillar stance today: team frame shares (post-contact, teams with a healer and >=2 alive)\n")
    print("| profile | map | fmt | all hidden | all hidden + mutually sighted | healer hidden, a DPS exposed | a DPS hidden, healer exposed | none hidden | mutually sighted | all at a pillar | healer at pillar, a DPS not | hug: all DPS hidden | hug: mutually sighted |")
    print("|---|---|---|---|---|---|---|---|---|---|---|---|---|")
    for prof, m in keys:
        for fm in ("2v2", "3v3"):
            a = collections.Counter()
            for r in R:
                if r["profile"] == prof and r["map"] == m and fmt_of(r) == fm:
                    for t in r["teams"]:
                        if t["has_healer"]:
                            for k, v in t.items():
                                if isinstance(v, float):
                                    a[k] += v
            if not a["secs"]:
                continue
            s = a["secs"]
            print(f"| {prof} | {m} | {fm} | {pct(a['all_hidden'], s)} | {pct(a['all_hidden_cohesive'], s)} | "
                  f"{pct(a['healer_hidden_dps_exposed'], s)} | {pct(a['dps_hidden_healer_exposed'], s)} | "
                  f"{pct(a['none_hidden'], s)} | {pct(a['cohesive'], s)} | {pct(a['all_at_pillar'], s)} | "
                  f"{pct(a['healer_at_pillar_dps_not'], s)} | {pct(a['healer_hug_dps_hidden'], a['healer_hug'])} | "
                  f"{pct(a['healer_hug_cohesive'], a['healer_hug'])} |")
    print()
    print("### 3b. Hidden share by role (post-contact)\n")
    print("| profile | map | role | hidden | in enemy CC reach | hard-CC'd |")
    print("|---|---|---|---|---|---|")
    for prof, m in keys:
        per = collections.defaultdict(collections.Counter)
        for r in R:
            if r["profile"] == prof and r["map"] == m:
                for u in r["units"]:
                    p = per[u["role"]]
                    p["secs"] += u["secs"]
                    p["hidden"] += u["hidden"]
                    p["exp"] += u["exposed_cc"]
                    p["hard"] += u["hard_cc"]
        for role in sorted(per):
            p = per[role]
            print(f"| {prof} | {m} | {role} | {pct(p['hidden'], p['secs'])} | {pct(p['exp'], p['secs'])} | {pct(p['hard'], p['secs'])} |")
    print()

    # ---- 4. downtime ------------------------------------------------------
    print("## 4. Downtime: what a lull converts into\n")
    print("Lulls: spans >= 2s with no non-pet health lost on either side, after contact.\n")
    print("| profile | map | lull s per match | lulls per match | share of post-contact time |")
    print("|---|---|---|---|---|")
    for prof, m in keys:
        sub = [r for r in R if r["profile"] == prof and r["map"] == m]
        ls = sum(l["secs"] for r in sub for l in r["lulls"])
        ln = sum(len(r["lulls"]) for r in sub)
        post = sum(max(r["duration"] - r["contact"], 0) for r in sub)
        print(f"| {prof} | {m} | {ls / len(sub):.1f} | {ln / len(sub):.2f} | {pct(ls, post)} |")
    print()
    print("Per team with a healer, by healer class, over lulls where that team stood below 95% health, before / after dampening onset (75s): team health fraction gained per lull second (the enemy's in brackets), and healer mana fraction per lull second.\n")
    print("| profile | healer | phase | lull s | team hp/s (enemy hp/s) | healer mana/s | net hp/s |")
    print("|---|---|---|---|---|---|---|")
    for prof in sorted({k[0] for k in keys}):
        per = collections.defaultdict(collections.Counter)
        for r in R:
            if r["profile"] != prof:
                continue
            comps = [r["team1"].split("+"), r["team2"].split("+")]
            for l in r["lulls"]:
                for t in (0, 1):
                    hs = [c for c in comps[t] if c in ("Priest", "Paladin", "Shaman", "Druid")]
                    if not hs or l["hp0"][t] >= 0.95 or l["hmana0"][t] < 0:
                        continue
                    ph = "early" if l["t0"] < 75 else "damp"
                    p = per[(hs[0], ph)]
                    p["secs"] += l["secs"]
                    p["hp"] += l["hp1"][t] - l["hp0"][t]
                    p["ehp"] += l["hp1"][1 - t] - l["hp0"][1 - t]
                    if l["hmana1"][t] >= 0:
                        p["mana"] += l["hmana1"][t] - l["hmana0"][t]
        for k in sorted(per):
            p = per[k]
            s = p["secs"]
            print(f"| {prof} | {k[0]} | {k[1]} | {s:.0f} | {100 * p['hp'] / s:.2f}% ({100 * p['ehp'] / s:.2f}%) | "
                  f"{100 * p['mana'] / s:.2f}% | {100 * (p['hp'] - p['ehp']) / s:+.2f}% |")
    print()
    print("Per 2v2 comp with a healer, early lulls (before 75s) with that team below 95% health: its health gain per lull second net of the enemy's, over all such lulls and over those in which the whole team stood hidden for most of the lull. An upper estimate of a stall's value: it selects lulls in which the comp had health to regain.\n")
    print("| profile | comp | lull s | net hp/s | own hp/s | hidden lull s | hidden net hp/s |")
    print("|---|---|---|---|---|---|---|")
    for prof in sorted({k[0] for k in keys}):
        per = collections.defaultdict(collections.Counter)
        for r in R:
            if r["profile"] != prof or fmt_of(r) != "2v2":
                continue
            comps = [r["team1"], r["team2"]]
            for l in r["lulls"]:
                if l["t0"] >= 75:
                    continue
                for t in (0, 1):
                    if l["hp0"][t] >= 0.95 or not any(h in comps[t] for h in ("Priest", "Paladin", "Shaman", "Druid")):
                        continue
                    p = per[comps[t]]
                    net = (l["hp1"][t] - l["hp0"][t]) - (l["hp1"][1 - t] - l["hp0"][1 - t])
                    p["s"] += l["secs"]
                    p["net"] += net
                    p["own"] += l["hp1"][t] - l["hp0"][t]
                    if l["hidden_share"][t] > 0.5:
                        p["hs"] += l["secs"]
                        p["hnet"] += net
        for c, p in sorted(per.items(), key=lambda kv: -kv[1]["net"] / kv[1]["s"]):
            hn = f"{100 * p['hnet'] / p['hs']:+.2f}%" if p["hs"] else "-"
            print(f"| {prof} | {c} | {p['s']:.0f} | {100 * p['net'] / p['s']:+.2f}% | {100 * p['own'] / p['s']:.2f}% | {p['hs']:.0f} | {hn} |")
    print()
    print("Healer mana fraction at 30/60/90/120s after the gates (alive), and the share of its alive time below 10%:\n")
    print("| profile | map | healer | 30s | 60s | 90s | 120s | below 10% |")
    print("|---|---|---|---|---|---|---|---|")
    for prof, m in keys:
        per = collections.defaultdict(lambda: collections.defaultdict(list))
        low = collections.Counter()
        alive = collections.Counter()
        for r in R:
            if r["profile"] == prof and r["map"] == m:
                for h in r["mana"]:
                    for t, v in h["at"].items():
                        per[h["class"]][t].append(v)
                    low[h["class"]] += h["low_secs"]
                    alive[h["class"]] += h["alive_secs"]
        for c in sorted(per):
            cells = []
            for t in ("030", "060", "090", "120"):
                v = per[c][t]
                cells.append(f"{100 * sum(v) / len(v):.0f}% (n={len(v)})" if v else "-")
            print(f"| {prof} | {m} | {c} | " + " | ".join(cells) + f" | {pct(low[c], alive[c])} |")
    print()

    # ---- 5. pillar deaths -------------------------------------------------
    print("## 5. Deaths with the healer alive, by cause\n")
    print("Pillar death: hidden from its healer for the last 2s or more. Cause: the healer hard-CC'd for most of the last 5s, else whose step broke the final sight.\n")
    print("| profile | map | fmt | deaths (healer alive) | pillar deaths | cause: healer CC'd | healer step | DPS step | both | joint | mostly hidden (>=50% of 5s) |")
    print("|---|---|---|---|---|---|---|---|---|---|---|")
    for prof, m in keys:
        for fm in ("2v2", "3v3"):
            ds = [d for r in R if r["profile"] == prof and r["map"] == m and fmt_of(r) == fm
                  for d in r["deaths"] if d["healer_alive"] and d["healer"] and d["class"] not in ("Priest", "Paladin", "Shaman", "Druid")]
            if not ds:
                continue
            pdth = [d for d in ds if d["final_blind_secs"] >= 2.0]
            c = collections.Counter()
            for d in pdth:
                if d["healer_hard_5s"] >= 0.5:
                    c["cc"] += 1
                else:
                    c[{"healer": "healer", "dps": "dps", "both": "both"}.get(d["final_broke"], "joint")] += 1
            mostly = sum(d["hidden_from_healer_5s"] >= 0.5 for d in ds)
            print(f"| {prof} | {m} | {fm} | {len(ds)} | {len(pdth)} ({pct(len(pdth), len(ds))}) | {c['cc']} | {c['healer']} | {c['dps']} | {c['both']} | {c['joint']} | {mostly} ({pct(mostly, len(ds))}) |")
    print()
    print("### 5b. Pillar deaths by the dead DPS's class\n")
    print("| profile | map | class | deaths (healer alive) | pillar deaths |")
    print("|---|---|---|---|---|")
    for prof, m in keys:
        per = collections.Counter()
        pd = collections.Counter()
        for r in R:
            if r["profile"] == prof and r["map"] == m:
                for d in r["deaths"]:
                    if d["healer_alive"] and d["healer"] and d["class"] not in ("Priest", "Paladin", "Shaman", "Druid"):
                        per[d["class"]] += 1
                        pd[d["class"]] += d["final_blind_secs"] >= 2.0
        for c in sorted(per):
            print(f"| {prof} | {m} | {c} | {per[c]} | {pd[c]} ({pct(pd[c], per[c])}) |")


if __name__ == "__main__":
    main(sys.argv[1:])
