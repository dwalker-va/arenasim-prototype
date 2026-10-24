#!/usr/bin/env python3
"""AS-391 scorer: chain hand-offs, paired at identical seeds.

    2026-10-24-as391-chain-score.py <A=rows.jsonl> <B=rows.jsonl> [--slices]

Rows are `tests/chain_handoff_census.rs` output. Every match in A is paired
with the match of the same label in B (the same cell, seed and seat), and B
is scored against A:

- the windows' outcomes per arm, and the hand-off rate (in-lead hand-offs per
  window, and per window that could be chained: not void, not broken);
- the missed rate (a freed window with a ready teammate), and its share
  attributed to a ready unit under the Pillar play;
- per-match paired means and z for hand-offs, windows, misses and the
  lockout metrics;
- win rate against Legacy with McNemar on the flips.

A rate difference's standard error is a paired bootstrap over matches (2,000
resamples, fixed seed), so it carries the correlation between twins; the
RESOLUTION printed is 1.96 of it.
"""
import gzip
import json
import math
import random
import sys

args = [a for a in sys.argv[1:] if not a.startswith("--")]
slices = "--slices" in sys.argv


def load(path):
    out = {}
    opener = gzip.open if path.endswith(".gz") else open
    with opener(path, "rt") as f:
        for line in f:
            r = json.loads(line)
            out[r["label"]] = r
    return out


(la, pa), (lb, pb) = (a.split("=", 1) for a in args[:2])
A, B = load(pa), load(pb)
keys = sorted(set(A) & set(B))
print(f"paired matches: {len(keys)} (A only {len(set(A) - set(B))}, B only {len(set(B) - set(A))})")

OUT = ["in_lead", "early", "broken", "extended", "late", "void", "freed"]


def w(r, k):
    return r["windows"][k]


def missed(r):
    x = r["windows"]
    return x["missed_pillar"] + x["missed_chain"] + x["missed_other"]


def chainable(r):
    return w(r, "total") - w(r, "void") - w(r, "broken")


def ratio(rows, num, den):
    n = sum(num(r) for r in rows)
    d = sum(den(r) for r in rows)
    return n / d if d else float("nan")


def boot_diff(pairs, num, den, reps=2000):
    rng = random.Random(391)
    k = len(pairs)
    if k == 0:
        return float("nan")
    na = [num(a) for a, _ in pairs]
    da = [den(a) for a, _ in pairs]
    nb = [num(b) for _, b in pairs]
    db = [den(b) for _, b in pairs]
    diffs = []
    for _ in range(reps):
        idx = [rng.randrange(k) for _ in range(k)]
        sa = sum(da[i] for i in idx)
        sb = sum(db[i] for i in idx)
        if sa == 0 or sb == 0:
            continue
        diffs.append(sum(nb[i] for i in idx) / sb - sum(na[i] for i in idx) / sa)
    m = sum(diffs) / len(diffs)
    return math.sqrt(sum((d - m) ** 2 for d in diffs) / (len(diffs) - 1))


def paired(diffs):
    n = len(diffs)
    if n < 2:
        return 0.0, 0.0
    m = sum(diffs) / n
    v = sum((d - m) ** 2 for d in diffs) / (n - 1)
    return m, (m / math.sqrt(v / n) if v > 0 else 0.0)


def report(title, ks):
    pairs = [(A[k], B[k]) for k in ks]
    ra = [a for a, _ in pairs]
    rb = [b for _, b in pairs]
    print(f"\n## {title} (n={len(ks)} paired matches)\n")
    print(f"| outcome | {la} | {lb} |")
    print("|---|---|---|")
    for o in ["total"] + OUT:
        print(f"| {o} | {sum(w(r, o) for r in ra)} | {sum(w(r, o) for r in rb)} |")
    for name, f in [("missed (ready, nothing landed)", missed),
                    ("  ready unit under Pillar", lambda r: w(r, "missed_pillar")),
                    ("  ready unit under Chain", lambda r: w(r, "missed_chain")),
                    ("  ready unit under neither", lambda r: w(r, "missed_other")),
                    ("windows a hand-off holder spent under Pillar", lambda r: w(r, "pillar_exposed")),
                    ("chain-step casts", lambda r: r["chain_casts"]),
                    ("chain-step held GCDs", lambda r: r["chain_holds"]),
                    ("hand-off landings, any timing", lambda r: r["handoff_landings"])]:
        print(f"| {name} | {sum(f(r) for r in ra)} | {sum(f(r) for r in rb)} |")
    print()
    print("| rate | " + la + " | " + lb + " | delta (pt) | resolution (1.96 SE, pt) |")
    print("|---|---|---|---|---|")
    for name, num, den in [
        ("hand-off rate, per window", lambda r: w(r, "in_lead"), lambda r: w(r, "total")),
        ("hand-off rate, per chainable window", lambda r: w(r, "in_lead"), chainable),
        ("missed rate, per chainable window", missed, chainable),
        ("missed under Pillar, per chainable window", lambda r: w(r, "missed_pillar"), chainable),
    ]:
        a = ratio(ra, num, den)
        b = ratio(rb, num, den)
        se = boot_diff(pairs, num, den)
        print(f"| {name} | {100*a:.2f}% | {100*b:.2f}% | {100*(b-a):+.2f} | {196*se:.2f} |")
    print()
    print("| per match | " + la + " | delta | z |")
    print("|---|---|---|---|")
    for name, f in [
        ("windows", lambda r: w(r, "total")),
        ("hand-offs in the lead", lambda r: w(r, "in_lead")),
        ("missed", missed),
        ("denied s", lambda r: r["lockout"]["denied"]),
        ("healer-locked s", lambda r: r["lockout"]["healer_locked"]),
        ("counterplay-free s", lambda r: r["lockout"]["counterplay_free"]),
        ("longest lockout s", lambda r: r["lockout"]["longest_lockout"]),
        ("longest healer lockout s", lambda r: r["lockout"]["longest_healer_lockout"]),
    ]:
        m, z = paired([f(b) - f(a) for a, b in pairs])
        base = sum(f(a) for a in ra) / max(len(ra), 1)
        print(f"| {name} | {base:.3f} | {m:+.3f} | {z:+.2f} |")
    wa = sum(a["won"] for a in ra) / len(ra)
    wb = sum(b["won"] for b in rb) / len(rb)
    up = sum(1 for a, b in pairs if b["won"] and not a["won"])
    dn = sum(1 for a, b in pairs if a["won"] and not b["won"])
    z = (up - dn) / math.sqrt(up + dn) if up + dn else 0.0
    print(f"\nwin vs Legacy: {100*wa:.1f}% -> {100*wb:.1f}% ({100*(wb-wa):+.1f}pt), flips +{up}/-{dn}, McNemar z {z:+.2f}")


report("all", keys)
if slices:
    def team(k):
        return A[k]["team"].split("+")
    for title, pred in [
        ("PillaredArena", lambda k: A[k]["map"] == "PillaredArena"),
        ("TwinPillars", lambda k: A[k]["map"] == "TwinPillars"),
        ("team has a Druid", lambda k: "Druid" in team(k)),
        ("team has a Mage", lambda k: "Mage" in team(k)),
        ("Mage+Druid", lambda k: sorted(team(k)) == ["Druid", "Mage"]),
        ("3v3", lambda k: len(team(k)) == 3),
    ]:
        report(title, [k for k in keys if pred(k)])
