#!/usr/bin/env python3
"""AS-164 (0.7 milestone sweep): every table in 2026-10-10-as164-milestone-sweep.md
that is computed from a canonical batch CSV or the head-to-head CSV.

    analyze.py standings  1v1.csv 2v2.csv 3v3.csv
    analyze.py healer     2v2.csv
    analyze.py interrupts 2v2.csv
    analyze.py mirrors    1v1.csv 2v2.csv 3v3.csv
    analyze.py draws      1v1.csv 2v2.csv 3v3.csv
    analyze.py duels      1v1.csv
    analyze.py h2h        h2h.csv [2026-08-06-team-solve-headtohead-n100.csv]
    analyze.py bisect     2026-10-10-as164-teamplan-bisect.csv
    analyze.py comps      2v2.csv <Class>
    analyze.py hunter     2v2.csv 3v3.csv

Every CSV argument may be the committed `.csv.gz`.

The mechanism tables (mana, Cyclone, Travel Form, roots, traps) are
2026-10-10-as164-mechanisms.py, over the milestone_mechanisms rows.

Conventions, all inherited from the Shaman report and sweep-tiers.md:
- A side's WIN RATE counts a draw as a non-win; DECISIVE % divides by wins +
  losses only.
- Every rate carries its Wilson 95% interval, over MATCHES. A class or comp
  standing pools many cells, so its matches are clustered by cell and the
  Wilson interval understates the uncertainty of the standing as a claim
  about the class; it is printed as a resolution floor, not as the interval.
- Healer-framed cells are symmetrised: each comparison pools the cell and its
  swap (both team orders, the same seeds), so team-order bias cancels.
"""

import csv
import gzip
import math
import sys
from collections import defaultdict

CLASSES = ["Warrior", "Mage", "Rogue", "Priest", "Warlock", "Paladin", "Hunter", "Shaman", "Druid"]
HEALERS = ["Priest", "Paladin", "Shaman", "Druid"]
DPS = [c for c in CLASSES if c not in HEALERS]
# Classes whose kit interrupts a cast: Pummel, Kick, the Felhunter's Spell
# Lock (the Warlock's default pet), Wind Shear. The Mage has no Counterspell.
INTERRUPTERS = {"Warrior", "Rogue", "Warlock", "Shaman"}


def wilson(k, n, z=1.96):
    if n == 0:
        return (0.0, 1.0)
    p = k / n
    d = 1 + z * z / n
    c = (p + z * z / (2 * n)) / d
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return (max(0.0, c - h), min(1.0, c + h))


def fmt(k, n):
    if n == 0:
        return "n/a"
    lo, hi = wilson(k, n)
    return f"{100 * k / n:.1f}% [{100 * lo:.1f}, {100 * hi:.1f}]"


def load(path):
    """Rows of a batch CSV, plain or gzipped (`.csv.gz`)."""
    opener = gzip.open if path.endswith(".gz") else open
    with opener(path, "rt", newline="") as f:
        return list(csv.DictReader(f))


def team(s):
    return s.split("+")


def key(classes):
    return "+".join(sorted(classes, key=CLASSES.index))


def side_result(r, side):
    """'w', 'l' or 'd' for team `side` (1 or 2) of row r."""
    w = r["winner"]
    if w not in ("team1", "team2"):
        return "d"
    return "w" if w == f"team{side}" else "l"


class Tally:
    def __init__(self):
        self.w = self.l = self.d = 0

    def add(self, res):
        if res == "w":
            self.w += 1
        elif res == "l":
            self.l += 1
        else:
            self.d += 1

    @property
    def n(self):
        return self.w + self.l + self.d

    def rate(self):
        return fmt(self.w, self.n)

    def decisive(self):
        dn = self.w + self.l
        return f"{100 * self.w / dn:.1f}%" if dn else "n/a"


def healers_in(classes):
    return sum(c in HEALERS for c in classes)


def competitive(classes):
    """2v2: at most one healer. 3v3: one or two healers (sweep-tiers.md and
    the canonical summary's definition)."""
    h = healers_in(classes)
    if len(classes) == 2:
        return h <= 1
    if len(classes) == 3:
        return 1 <= h <= 2
    return True


# ----------------------------------------------------------------------------
def standings(paths):
    labels = ["1v1", "2v2", "3v3"]
    per = {}
    meta = {}
    for label, p in zip(labels, paths):
        rows = load(p)
        allc = defaultdict(Tally)
        comp = defaultdict(Tally)
        for r in rows:
            t1, t2 = team(r["team1"]), team(r["team2"])
            both_comp = competitive(t1) and competitive(t2)
            for side, t in ((1, t1), (2, t2)):
                res = side_result(r, side)
                for c in set(t):
                    allc[c].add(res)
                    if both_comp:
                        comp[c].add(res)
        draws = sum(1 for r in rows if r["winner"] not in ("team1", "team2"))
        durs = sorted(float(r["duration_secs"]) for r in rows)
        meta[label] = (len(rows), draws, durs[len(durs) // 2], len(set(r["duration_secs"] for r in rows)))
        per[label] = (allc, comp)
    print("| Class | 1v1 | 2v2 all | **2v2 competitive** | 3v3 all | **3v3 competitive** |")
    print("|---|---|---|---|---|---|")
    order = sorted(CLASSES, key=lambda c: -per["2v2"][1][c].w / max(1, per["2v2"][1][c].n))
    for c in order:
        cells = []
        for label in labels:
            allc, comp = per[label]
            a = allc[c]
            cells.append(f"{100 * a.w / a.n:.1f}")
            if label != "1v1":
                k = comp[c]
                cells.append(f"**{100 * k.w / k.n:.1f}**")
        print(f"| {c} | " + " | ".join(cells) + " |")
    print()
    for label in labels:
        n, d, med, distinct = meta[label]
        allc, comp = per[label]
        widest = max(
            (wilson(t.w, t.n)[1] - wilson(t.w, t.n)[0]) / 2 for t in list(allc.values()) + list(comp.values()) if t.n
        )
        print(f"{label}: {n} matches, {d} draws ({100 * d / n:.2f}%), median duration {med:.1f}s, "
              f"{distinct} distinct durations; widest per-class Wilson half-width {100 * widest:.2f}pt")


# ----------------------------------------------------------------------------
def healer(path):
    rows = load(path)
    by = defaultdict(list)
    for r in rows:
        by[(r["team1"], r["team2"])].append(r)

    def compare(a, b):
        t = Tally()
        for r in by.get((a, b), []):
            t.add(side_result(r, 1))
        for r in by.get((b, a), []):
            t.add(side_result(r, 2))
        return t

    print("Druid-healed team vs the same DPS healed by another healer (symmetrised, both team orders).")
    print()
    print("| DPS | vs Priest | vs Paladin | vs Shaman | mirror control (Druid v Druid, team 1) |")
    print("|---|---|---|---|---|")
    for x in DPS:
        a = key([x, "Druid"])
        cells = []
        for h in ["Priest", "Paladin", "Shaman"]:
            t = compare(a, key([x, h]))
            cells.append(f"{t.rate()} (dec {t.decisive()}, n={t.n})")
        m = Tally()
        for r in by.get((a, a), []):
            m.add(side_result(r, 1))
        cells.append(f"{m.rate()} (dec {m.decisive()}, n={m.n})")
        print(f"| {x} | " + " | ".join(cells) + " |")
    print()
    print("Full healer grid: row healer's team win rate against the column healer's team, same DPS, "
          "pooled over the five DPS partners (n per cell = 5 x 200).")
    print()
    print("| row \\ col | " + " | ".join(HEALERS) + " |")
    print("|---|" + "---|" * len(HEALERS))
    for h1 in HEALERS:
        cells = []
        for h2 in HEALERS:
            t = Tally()
            for x in DPS:
                a, b = key([x, h1]), key([x, h2])
                if h1 == h2:
                    for r in by.get((a, a), []):
                        t.add(side_result(r, 1))
                else:
                    tt = compare(a, b)
                    t.w += tt.w
                    t.l += tt.l
                    t.d += tt.d
            cells.append(t.rate())
        print(f"| {h1} | " + " | ".join(cells) + " |")
    print()
    print("Which healer each DPS prefers: the DPS+healer comp's win rate against the whole 2v2 field "
          "(all comps, both orders).")
    print()
    field = defaultdict(Tally)
    for r in rows:
        field[r["team1"]].add(side_result(r, 1))
        field[r["team2"]].add(side_result(r, 2))
    print("| DPS | " + " | ".join(f"+{h}" for h in HEALERS) + " |")
    print("|---|" + "---|" * len(HEALERS))
    for x in DPS:
        print(f"| {x} | " + " | ".join(field[key([x, h])].rate() for h in HEALERS) + " |")


# ----------------------------------------------------------------------------
def interrupts(path):
    rows = load(path)
    print("Each <DPS>+<healer> comp's win rate against COMPETITIVE opponents (at most one healer), by how")
    print("many interrupt classes the opponent fields. The opponent set is the same for every healer, so")
    print("the column to read is the DROP from 0 to 2 interrupters, compared ACROSS healers.")
    print()
    print("| Healer | vs 0 interrupters | vs 1 | vs 2 | drop 0 -> 2 |")
    print("|---|---|---|---|---|")
    for h in HEALERS:
        t = [Tally(), Tally(), Tally()]
        for r in rows:
            for side, mine, theirs in ((1, r["team1"], r["team2"]), (2, r["team2"], r["team1"])):
                m, o = team(mine), team(theirs)
                if len(m) != 2 or h not in m or healers_in(m) != 1 or not competitive(o):
                    continue
                k = sum(c in INTERRUPTERS for c in o)
                t[k].add(side_result(r, side))
        drop = 100 * (t[0].w / t[0].n - t[2].w / t[2].n)
        print(f"| {h} | {t[0].rate()} n={t[0].n} | {t[1].rate()} n={t[1].n} | {t[2].rate()} n={t[2].n} | {drop:+.1f}pt |")


# ----------------------------------------------------------------------------
def mirrors(paths):
    print("Healer mirrors: matches where both sides field the SAME healer class (and no other healer).")
    print("Dampening starts 75s after the gates and reaches 100% at 195s.")
    print()
    print("| Format | Healer | matches | draws | median duration | ends after 75s | ends after 195s |")
    print("|---|---|---|---|---|---|---|")
    for label, p in zip(["1v1", "2v2", "3v3"], paths):
        rows = load(p)
        for h in HEALERS:
            sel = []
            for r in rows:
                t1, t2 = team(r["team1"]), team(r["team2"])
                if h in t1 and h in t2 and healers_in(t1) == 1 and healers_in(t2) == 1:
                    sel.append(r)
            if not sel:
                continue
            n = len(sel)
            d = sum(1 for r in sel if r["winner"] not in ("team1", "team2"))
            durs = sorted(float(r["duration_secs"]) for r in sel)
            a75 = sum(1 for x in durs if x > 75)
            a195 = sum(1 for x in durs if x > 195)
            print(f"| {label} | {h} | {n} | {fmt(d, n)} | {durs[n // 2]:.1f}s | {100 * a75 / n:.1f}% | {100 * a195 / n:.1f}% |")


# ----------------------------------------------------------------------------
def ztest(k1, n1, k2, n2):
    if n1 == 0 or n2 == 0:
        return 0.0
    p = (k1 + k2) / (n1 + n2)
    if p in (0.0, 1.0):
        return 0.0
    return (k2 / n2 - k1 / n1) / math.sqrt(p * (1 - p) * (1 / n1 + 1 / n2))


def h2h(path, old=None):
    rows = load(path)
    olds = load(old) if old else []
    cells = sorted({r["label"].rsplit("_", 1)[0] for r in rows})

    def tally(rs, label, side, seeds):
        t = Tally()
        for r in rs:
            if r["label"] == label and int(r["seed"]) in seeds:
                t.add(side_result(r, side))
        return t

    max_seed = max(int(r["seed"]) for r in rows)
    windows = [("seeds 1-100 (the 2026-08-06 seeds)", range(1, 101))]
    if max_seed > 100:
        # Only a run that goes past the 2026-08-06 seeds has a second window;
        # on a 100-seed CSV it would print the same table twice.
        windows.append((f"seeds 1-{max_seed}", range(1, max_seed + 1)))
    for seeds_name, seeds in windows:
        print(f"### {seeds_name}")
        print()
        print("| Side given TeamPlan | vs | baseline (LL) | with TeamPlan | effect | z |")
        print("|---|---|---|---|---|---|")
        for c in cells:
            r0 = next(r for r in rows if r["label"].startswith(c + "_"))
            t1, t2 = r0["team1"], r0["team2"]
            for side, me, them, arm in ((1, t1, t2, "TL"), (2, t2, t1, "LT")):
                base = tally(rows, f"{c}_LL", side, seeds)
                got = tally(rows, f"{c}_{arm}", side, seeds)
                eff = 100 * (got.w / got.n - base.w / base.n)
                print(f"| {me} | {them} | {base.w}/{base.n} ({100 * base.w / base.n:.0f}%) | "
                      f"{got.w}/{got.n} ({100 * got.w / got.n:.0f}%) | {eff:+.0f}pt | {ztest(base.w, base.n, got.w, got.n):+.2f} |")
        print()
    if olds:
        print("### Against the 2026-08-06 CSV, same seeds (1-100): level change per arm")
        print()
        print("| Cell | arm | 2026-08-06 team-1 wins | now team-1 wins | seeds whose winner changed |")
        print("|---|---|---|---|---|")
        name_map = {"pillar": "pillar", "hunter": "hunter"}
        for c, oc in name_map.items():
            for arm in ("LL", "TL", "LT"):
                o = {int(r["seed"]): r["winner"] for r in olds if r["label"] == f"{oc}_{arm}"}
                nrows = {int(r["seed"]): r["winner"] for r in rows if r["label"] == f"{c}_{arm}" and int(r["seed"]) <= 100}
                if not o:
                    continue
                ow = sum(1 for w in o.values() if w == "team1")
                nw = sum(1 for w in nrows.values() if w == "team1")
                flips = sum(1 for s in o if s in nrows and o[s] != nrows[s])
                print(f"| {c} | {arm} | {ow}/{len(o)} | {nw}/{len(nrows)} | {flips} |")


# ----------------------------------------------------------------------------
def hunter(paths):
    print("Hunter comps' standing against the field.")
    for label, p in zip(["2v2", "3v3"], paths):
        rows = load(p)
        allc, comp = Tally(), Tally()
        by_partner = defaultdict(Tally)
        for r in rows:
            t1, t2 = team(r["team1"]), team(r["team2"])
            both = competitive(t1) and competitive(t2)
            for side, t in ((1, t1), (2, t2)):
                if "Hunter" not in t:
                    continue
                res = side_result(r, side)
                allc.add(res)
                if both:
                    comp.add(res)
                if label == "2v2":
                    by_partner[[c for c in t if c != "Hunter"][0]].add(res)
        print(f"{label}: all {allc.rate()} n={allc.n}; competitive {comp.rate()} n={comp.n}")
        if by_partner:
            for pp in CLASSES:
                if pp in by_partner:
                    print(f"  Hunter+{pp}: {by_partner[pp].rate()} n={by_partner[pp].n}")


def comps(path, cls):
    """Every comp containing `cls`: win rate against the COMPETITIVE field,
    both team orders, Wilson interval over matches."""
    rows = load(path)
    t = defaultdict(Tally)
    for r in rows:
        t1, t2 = team(r["team1"]), team(r["team2"])
        if not (competitive(t1) and competitive(t2)):
            continue
        for side, me in ((1, t1), (2, t2)):
            if cls in me:
                t[key(me)].add(side_result(r, side))
    print(f"| Comp with {cls} | vs competitive field | n |")
    print("|---|---|---|")
    for k, v in sorted(t.items(), key=lambda kv: -kv[1].w / kv[1].n):
        print(f"| {k} | {v.rate()} | {v.n} |")


def draws(paths):
    """Draw rate by how many sides field a Druid (and, for comparison, a
    Shaman), over every match and over the competitive slice only; and how
    many draws are the 300s cap rather than a mutual kill."""
    for label, p in zip(["1v1", "2v2", "3v3"], paths):
        rows = load(p)
        n_draw = sum(1 for r in rows if r["winner"] not in ("team1", "team2"))
        n_cap = sum(1 for r in rows if r["winner"] not in ("team1", "team2") and r["end_reason"] == "cap")
        print(f"### {label}: {n_draw} draws of {len(rows)} matches; {n_cap} of the draws are the 300s cap")
        print()
        print("| Class | slice | 0 sides | 1 side | 2 sides |")
        print("|---|---|---|---|---|")
        for cls in ("Druid", "Shaman"):
            for slice_name in ("all", "competitive"):
                cells = []
                for k in range(3):
                    d = n = 0
                    for r in rows:
                        t1, t2 = team(r["team1"]), team(r["team2"])
                        if slice_name == "competitive" and not (competitive(t1) and competitive(t2)):
                            continue
                        if (cls in t1) + (cls in t2) != k:
                            continue
                        n += 1
                        d += r["winner"] not in ("team1", "team2")
                    cells.append(f"{100 * d / n:.2f}% ({d}/{n})" if n else "n/a")
                print(f"| {cls} | {slice_name} | " + " | ".join(cells) + " |")
        print()


def duels(path):
    """1v1 cells with a Druid against a healer: team-1 wins, draws, and how
    many of the draws are the 300s cap."""
    rows = load(path)
    by = defaultdict(lambda: [0, 0, 0, 0])  # t1 wins, draws, cap draws, n
    for r in rows:
        k = (r["team1"], r["team2"])
        by[k][3] += 1
        if r["winner"] == "team1":
            by[k][0] += 1
        elif r["winner"] not in ("team1", "team2"):
            by[k][1] += 1
            by[k][2] += r["end_reason"] == "cap"
    print("| team 1 | team 2 | team-1 wins | draws | of them the 300s cap |")
    print("|---|---|---|---|---|")
    for a in HEALERS:
        for b in HEALERS:
            if "Druid" not in (a, b):
                continue
            w, d, c, n = by[(a, b)]
            print(f"| {a} | {b} | {w}/{n} | {d} | {c} |")


def bisect(path):
    """The TeamPlan bisect table: one row per match with its commit, cell, arm
    and whether the QUOTED side won. Effect = the arm's win rate minus the same
    commit's Legacy (LL) rate, for the quoted side."""
    rows = load(path)
    t = defaultdict(lambda: [0, 0])
    order = []
    side_of = {}
    for r in rows:
        k = (r["commit"], r["cell"], r["arm"])
        if k not in t:
            order.append(k)
        t[k][0] += r["side_won"] == "1"
        t[k][1] += 1
        side_of[(r["commit"], r["cell"])] = r["side"]
    print("| commit | cell | quoted side | Legacy (LL) | arm | with TeamPlan | effect |")
    print("|---|---|---|---|---|---|---|")
    for commit, cell, arm in order:
        if arm == "LL":
            continue
        bw, bn = t[(commit, cell, "LL")]
        aw, an = t[(commit, cell, arm)]
        print(f"| {commit} | {cell} | {side_of[(commit, cell)]} | {bw}/{bn} | {arm} | {aw}/{an} | "
              f"{100 * (aw / an - bw / bn):+.0f}pt |")


def main():
    cmd, args = sys.argv[1], sys.argv[2:]
    {
        "draws": lambda: draws(args),
        "duels": lambda: duels(args[0]),
        "bisect": lambda: bisect(args[0]),
        "comps": lambda: comps(args[0], args[1]),
        "standings": lambda: standings(args),
        "healer": lambda: healer(args[0]),
        "interrupts": lambda: interrupts(args[0]),
        "mirrors": lambda: mirrors(args),
        "h2h": lambda: h2h(args[0], args[1] if len(args) > 1 else None),
        "hunter": lambda: hunter(args),
    }[cmd]()


if __name__ == "__main__":
    main()
