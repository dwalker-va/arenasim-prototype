#!/usr/bin/env python3
"""Reduce `tests/healer_pillar_modes.rs` rows into the AS-373 tables.

    2026-10-19-as373-reduce.py [--hug 4] rows.jsonl [rows2.jsonl ...]

Every figure is a RATE (per healer-match, per minute, or a share of time),
with the totals it rests on printed beside it. "Hug" is a healer within
`--hug` yards of the nearest pillar's skin (its footprint disc).
"""
import argparse
import collections
import gzip
import json
import math
import statistics

ap = argparse.ArgumentParser()
ap.add_argument("--hug", type=float, default=4.0)
ap.add_argument("rows", nargs="+")
args = ap.parse_args()
HUG = args.hug
BINS = ["00-02", "02-04", "04-06", "06-10", "10-15", "15-25", "25+"]
BIN_LO = {"00-02": 0, "02-04": 2, "04-06": 4, "06-10": 6, "10-15": 10, "15-25": 15, "25+": 25}
RADIUS = {"PillaredArena": 6.0, "TwinPillars": 2.5}

rows = []
for p in args.rows:
    with (gzip.open(p, "rt") if p.endswith(".gz") else open(p)) as f:
        rows += [json.loads(l) for l in f if l.strip()]


def fmt(team):
    return "3v3" if team.count("+") == 2 else "2v2"


def mode_of_bin(b):
    return "hug" if BIN_LO[b] < HUG else "open"


def mode_of_skin(s):
    return "hug" if s < HUG else "open"


def pgroup(p):
    return {"free": "free", "pressured": "pressured"}.get(p, "escape/dip")


groups = collections.defaultdict(list)
for r in rows:
    groups[(r["profile"], r["map"])].append(r)


def pct(a, b):
    return f"{100 * a / b:.1f}%" if b else "-"


print(f"rows: {len(rows)} healer-matches; hug = within {HUG}yd of a pillar's skin")
print()
print("## 1. Where healers stand: share of alive time by distance from the nearest pillar's skin (after first contact)")
print()
print("| profile | map | posture | alive s/match | " + " | ".join(BINS) + " |")
print("|---|---|---|---|" + "---|" * len(BINS))
for (prof, m), rs in sorted(groups.items()):
    for post in ["all", "free", "pressured", "escape/dip"]:
        tot = collections.Counter()
        for r in rs:
            for k, o in r["occ"].items():
                ph, p, b = k.split("|")
                if ph != "post" or (post != "all" and pgroup(p) != post):
                    continue
                tot[b] += o["secs"]
        s = sum(tot.values())
        print(f"| {prof} | {m} | {post} | {s/len(rs):.1f} | " + " | ".join(pct(tot[b], s) for b in BINS) + " |")
print()
print("Before first contact (the opening):")
print()
print("| profile | map | alive s/match | " + " | ".join(BINS) + " |")
print("|---|---|---|" + "---|" * len(BINS))
for (prof, m), rs in sorted(groups.items()):
    tot = collections.Counter()
    for r in rs:
        for k, o in r["occ"].items():
            ph, p, b = k.split("|")
            if ph == "pre":
                tot[b] += o["secs"]
    s = sum(tot.values())
    print(f"| {prof} | {m} | {s/len(rs):.1f} | " + " | ".join(pct(tot[b], s) for b in BINS) + " |")
print()

print("## 2. What each mode costs and buys (after first contact), as shares of the time in that mode")
print()
print("| profile | map | fmt | mode | share of time | hard CC'd | in CC reach | blind to anchor | teammate dying | blind to a dying teammate (of dying time) | a teammate at a pillar |")
print("|---|---|---|---|---|---|---|---|---|---|---|")
for (prof, m), rs in sorted(groups.items()):
    for f in ["2v2", "3v3"]:
        acc = {"hug": collections.Counter(), "open": collections.Counter()}
        for r in rs:
            if fmt(r["team"]) != f:
                continue
            for k, o in r["occ"].items():
                ph, p, b = k.split("|")
                if ph != "post":
                    continue
                a = acc[mode_of_bin(b)]
                for kk, v in o.items():
                    a[kk] += v
        total = acc["hug"]["secs"] + acc["open"]["secs"]
        for mode in ["hug", "open"]:
            a = acc[mode]
            print(f"| {prof} | {m} | {f} | {mode} | {pct(a['secs'], total)} | {pct(a['hard_cc'], a['secs'])} | "
                  f"{pct(a['exposed'], a['secs'])} | {pct(a['blind_anchor'], a['secs'])} | {pct(a['dying'], a['secs'])} | "
                  f"{pct(a['blind_dying'], a['dying'])} | {pct(a['mate_near'], a['secs'])} |")
print()

print("## 3. Hard-CC landings by mode (onsets on the healer, after the gates)")
print()
print("| profile | map | mode | onsets | alive min in mode | onsets per min |")
print("|---|---|---|---|---|---|")
for (prof, m), rs in sorted(groups.items()):
    mins = collections.Counter()
    for r in rs:
        for k, o in r["occ"].items():
            ph, p, b = k.split("|")
            mins[mode_of_bin(b)] += o["secs"] / 60
    n = collections.Counter(mode_of_skin(c["skin"]) for r in rs for c in r["cc"])
    for mode in ["hug", "open"]:
        print(f"| {prof} | {m} | {mode} | {n[mode]} | {mins[mode]:.0f} | {n[mode]/mins[mode]:.3f} |")
print()

print("## 4. Quarter turns round one pillar (within 15yd of its skin): what a side switch costs")
print()
print("| profile | map | mean skin distance | quarter turns | per healer-match | median path yd | median secs | path / (r + skin) quarter arc |")
print("|---|---|---|---|---|---|---|---|")
for (prof, m), rs in sorted(groups.items()):
    obins = [(0, 2), (2, 4), (4, 6), (6, 10), (10, 15)]
    for lo, hi in obins:
        os = [o for r in rs for o in r["orbits"] if lo <= o["mean_skin"] < hi]
        if not os:
            continue
        ratio = statistics.median(o["path"] / (math.pi / 2 * (RADIUS[m] + o["mean_skin"])) for o in os)
        print(f"| {prof} | {m} | {lo}-{hi} | {len(os)} | {len(os)/len(rs):.2f} | "
              f"{statistics.median(o['path'] for o in os):.1f} | {statistics.median(o['secs'] for o in os):.1f} | {ratio:.2f} |")
print()

print("## 5. Blind episodes: a pillar hides a living teammate from its living healer")
print()
print("| profile | map | fmt | episodes/match | >=1s | >=3s | median s (>=1s) | broke: healer / mate / both / joint (>=1s) |")
print("|---|---|---|---|---|---|---|---|")
for (prof, m), rs in sorted(groups.items()):
    for f in ["2v2", "3v3"]:
        rr = [r for r in rs if fmt(r["team"]) == f]
        if not rr:
            continue
        eps = [e for r in rr for e in r["blind"]]
        long = [e for e in eps if e["secs"] >= 1.0]
        b = collections.Counter(e["broke"] for e in long)
        print(f"| {prof} | {m} | {f} | {len(eps)/len(rr):.1f} | {len(long)/len(rr):.2f} | "
              f"{sum(e['secs'] >= 3 for e in eps)/len(rr):.2f} | "
              f"{statistics.median(e['secs'] for e in long) if long else 0:.1f} | "
              f"{pct(b['healer'], len(long))} / {pct(b['mate'], len(long))} / {pct(b['both'], len(long))} / {pct(b['joint'], len(long))} |")
print()
print("Episodes of 1s or more that ended with sight regained, by the healer's mode when sight was lost:")
print()
print("| profile | map | healer mode | episodes | median secs | median healer path yd | median teammate path yd | healer walked more |")
print("|---|---|---|---|---|---|---|---|")
for (prof, m), rs in sorted(groups.items()):
    for mode in ["hug", "open"]:
        es = [e for r in rs for e in r["blind"] if e["secs"] >= 1 and e["end"] == "regained"
              and mode_of_skin(e["healer_skin"]) == mode and e["healer_hard_cc_secs"] == 0]
        if not es:
            continue
        print(f"| {prof} | {m} | {mode} | {len(es)} | {statistics.median(e['secs'] for e in es):.1f} | "
              f"{statistics.median(e['healer_path'] for e in es):.1f} | {statistics.median(e['mate_path'] for e in es):.1f} | "
              f"{pct(sum(e['healer_path'] > e['mate_path'] for e in es), len(es))} |")
print()

print("## 6. Teammate deaths with the healer alive, and pillar deaths")
print()
print("A PILLAR DEATH: the teammate was out of its healer's sight (a pillar between them) for the last 2s or more before it died, and the healer was not hard-CC'd for most of those 5s. `sight-only` adds: within heal range of the healer for most of the last 5s.")
print()
print("| profile | map | fmt | deaths | final blind >=2s | pillar deaths | of which sight-only | pillar deaths/100 matches | final sight broken by: healer / mate / both / joint | healer hug at death |")
print("|---|---|---|---|---|---|---|---|---|---|")
for (prof, m), rs in sorted(groups.items()):
    for f in ["2v2", "3v3"]:
        rr = [r for r in rs if fmt(r["team"]) == f]
        if not rr:
            continue
        nmatch = len({(r["label"], r["side"]) for r in rr})
        ds = [d for r in rr for d in r["deaths"]]
        fb = [d for d in ds if d["final_blind_secs"] >= 2.0]
        pdth = [d for d in fb if d["healer_cc_5s"] < 0.5]
        so = [d for d in pdth if d["out_of_range_5s"] < 0.5]
        b = collections.Counter(d["final_broke"] for d in pdth)
        hug = sum(mode_of_skin(d["healer_skin"]) == "hug" for d in pdth)
        print(f"| {prof} | {m} | {f} | {len(ds)} | {pct(len(fb), len(ds))} | {len(pdth)} ({pct(len(pdth), len(ds))}) | {len(so)} | "
              f"{100*len(pdth)/nmatch:.1f} | {pct(b['healer'], len(pdth))} / {pct(b['mate'], len(pdth))} / {pct(b['both'], len(pdth))} / {pct(b['joint'], len(pdth))} | {pct(hug, len(pdth))} |")
print()
print("Pillar deaths by the dead teammate's class (all maps and profiles):")
print()
by = collections.Counter()
alld = collections.Counter()
for r in rows:
    for d in r["deaths"]:
        alld[d["mate_class"]] += 1
        if d["final_blind_secs"] >= 2.0 and d["healer_cc_5s"] < 0.5:
            by[d["mate_class"]] += 1
print("| class | deaths | pillar deaths |")
print("|---|---|---|")
for c in sorted(alld):
    print(f"| {c} | {alld[c]} | {by[c]} ({pct(by[c], alld[c])}) |")
print()
print("Occlusion over the last 5s before every death with the healer alive and not hard-CC'd for most of it:")
print()
print("| profile | map | deaths | occluded >=50% of last 5s | >=80% of last 3s |")
print("|---|---|---|---|---|")
for (prof, m), rs in sorted(groups.items()):
    ds = [d for r in rs for d in r["deaths"] if d["healer_cc_5s"] < 0.5]
    print(f"| {prof} | {m} | {len(ds)} | {pct(sum(d['occl_5s'] >= 0.5 for d in ds), len(ds))} | {pct(sum(d['occl_3s'] >= 0.8 for d in ds), len(ds))} |")
print()
print("## 7. Share of post-contact time in hug, by healer class")
print()
print("| profile | map | Priest | Paladin | Shaman | Druid |")
print("|---|---|---|---|---|---|")
for (prof, m), rs in sorted(groups.items()):
    cells = []
    for c in ["Priest", "Paladin", "Shaman", "Druid"]:
        h = t = 0.0
        for r in rs:
            if r["class"] != c:
                continue
            for k, o in r["occ"].items():
                ph, p, b = k.split("|")
                if ph != "post":
                    continue
                t += o["secs"]
                if mode_of_bin(b) == "hug":
                    h += o["secs"]
        cells.append(pct(h, t))
    print(f"| {prof} | {m} | " + " | ".join(cells) + " |")
print()
print("## 8. Hug against enemy 2v2 comps with and without caster CC (Mage, Warlock, Druid), post contact")
print()
print("| profile | map | enemy | hug share | hard CC'd in hug | in open | in CC reach in hug | in open | blind to anchor in hug | in open |")
print("|---|---|---|---|---|---|---|---|---|---|")
for (prof, m), rs in sorted(groups.items()):
    for kind in ["caster CC", "no caster CC"]:
        acc = {"hug": collections.Counter(), "open": collections.Counter()}
        for r in rs:
            if fmt(r["team"]) != "2v2":
                continue
            has = bool(set(r["enemy"].split("+")) & {"Mage", "Warlock", "Druid"})
            if (kind == "caster CC") != has:
                continue
            for k, o in r["occ"].items():
                ph, p, b = k.split("|")
                if ph != "post":
                    continue
                for kk, v in o.items():
                    acc[mode_of_bin(b)][kk] += v
        hu, op = acc["hug"], acc["open"]
        tot = hu["secs"] + op["secs"]
        print(f"| {prof} | {m} | {kind} | {pct(hu['secs'], tot)} | {pct(hu['hard_cc'], hu['secs'])} | {pct(op['hard_cc'], op['secs'])} | "
              f"{pct(hu['exposed'], hu['secs'])} | {pct(op['exposed'], op['secs'])} | "
              f"{pct(hu['blind_anchor'], hu['secs'])} | {pct(op['blind_anchor'], op['secs'])} |")
