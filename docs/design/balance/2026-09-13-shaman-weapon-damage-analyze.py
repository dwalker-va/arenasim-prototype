#!/usr/bin/env python3
"""AS-97 paired sweep analysis: split control + McNemar + Wilson."""
import csv
import math
import sys

before_path, after_path = sys.argv[1], sys.argv[2]


def load(path):
    rows = {}
    with open(path) as f:
        for r in csv.DictReader(f):
            key = (r["label"], r["team1"], r["team2"], r["seed"])
            assert key not in rows, "duplicate key %s" % (key,)
            rows[key] = r
    return rows


b, a = load(before_path), load(after_path)
assert set(b) == set(a), "the two arms do not cover the same matches"
print("paired matches: %d" % len(b))
print()

# ---------------------------------------------------------------- split control
print("=" * 72)
print("SPLIT CONTROL - rows that must be identical vs rows that must differ")
print("=" * 72)
FIELDS = ("winner", "end_reason", "duration_secs")
stats = {}
for key in b:
    sl = key[0].split("|")[1]
    same = all(b[key][f] == a[key][f] for f in FIELDS)
    d = stats.setdefault(sl, {"n": 0, "same": 0, "diff": 0})
    d["n"] += 1
    d["same" if same else "diff"] += 1

for sl in ("control", "clean", "mirrored"):
    d = stats[sl]
    print(
        "  %-9s n=%4d  identical=%4d  differing=%4d  (%.1f%% differ)"
        % (sl, d["n"], d["same"], d["diff"], 100.0 * d["diff"] / d["n"])
    )
print()
ok_control = stats["control"]["diff"] == 0
ok_shaman = stats["clean"]["diff"] > 0 and stats["mirrored"]["diff"] > 0
print("  control byte-identical : %s" % ("PASS" if ok_control else "FAIL"))
print("  Shaman slices differ   : %s" % ("PASS" if ok_shaman else "FAIL"))
if not ok_control:
    print("  !! control rows that differ:")
    for key in sorted(b):
        if key[0].split("|")[1] != "control":
            continue
        if any(b[key][f] != a[key][f] for f in FIELDS):
            print("     %s seed=%s  %s -> %s" % (key[0], key[3], b[key], a[key]))
print()


# ---------------------------------------------------------------- non-vacuity
print("=" * 72)
print("NON-VACUITY - decisive events in each arm")
print("=" * 72)
for name, arm in (("before", b), ("after", a)):
    for sl in ("control", "clean", "mirrored"):
        rows = [r for k, r in arm.items() if k[0].split("|")[1] == sl]
        wins = sum(1 for r in rows if r["winner"] in ("team1", "team2"))
        draws = sum(1 for r in rows if r["winner"] == "draw")
        durs = len({r["duration_secs"] for r in rows})
        reasons = sorted({r["end_reason"] for r in rows})
        print(
            "  %-6s %-9s n=%4d decisive=%4d draws=%3d distinct_durations=%4d end_reasons=%s"
            % (name, sl, len(rows), wins, draws, durs, reasons)
        )
print()


# ---------------------------------------------------------------- stats helpers
def wilson(k, n, z=1.96):
    if n == 0:
        return (0.0, 0.0, 0.0)
    p = k / n
    d = 1 + z * z / n
    c = (p + z * z / (2 * n)) / d
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return (p, c - h, c + h)


def norm_sf(x):
    return 0.5 * math.erfc(x / math.sqrt(2))


def mcnemar(bb, cc):
    """Continuity-corrected McNemar, plus the exact binomial two-sided p."""
    n = bb + cc
    if n == 0:
        return (0.0, 1.0, 1.0)
    chi2 = (abs(bb - cc) - 1) ** 2 / n if n > 0 else 0.0
    p_chi = math.erfc(math.sqrt(chi2 / 2)) if chi2 > 0 else 1.0
    # exact two-sided binomial at p=0.5
    k = min(bb, cc)
    tail = sum(math.comb(n, i) for i in range(0, k + 1)) / (2.0**n)
    p_exact = min(1.0, 2 * tail)
    return (chi2, p_chi, p_exact)


# ---------------------------------------------------------------- paired result
print("=" * 72)
print("PAIRED DELTA - team1 win rate, before -> after (team1 holds the Shaman")
print("in the clean slice). McNemar over per-seed flips.")
print("=" * 72)
for sl in ("control", "clean", "mirrored"):
    keys = sorted(k for k in b if k[0].split("|")[1] == sl)
    n = len(keys)
    bw = sum(1 for k in keys if b[k]["winner"] == "team1")
    aw = sum(1 for k in keys if a[k]["winner"] == "team1")
    pb, lb, hb = wilson(bw, n)
    pa, la, ha = wilson(aw, n)
    # flips
    b2a = sum(
        1 for k in keys if b[k]["winner"] != "team1" and a[k]["winner"] == "team1"
    )
    a2b = sum(
        1 for k in keys if b[k]["winner"] == "team1" and a[k]["winner"] != "team1"
    )
    chi2, p_chi, p_exact = mcnemar(b2a, a2b)
    print()
    print("  slice: %s  (n=%d)" % (sl, n))
    print(
        "    before team1 win rate: %5.1f%%  95%% CI [%4.1f, %4.1f]"
        % (100 * pb, 100 * lb, 100 * hb)
    )
    print(
        "    after  team1 win rate: %5.1f%%  95%% CI [%4.1f, %4.1f]"
        % (100 * pa, 100 * la, 100 * ha)
    )
    print("    delta                : %+.1f pt" % (100 * (pa - pb)))
    print(
        "    flips: gained=%d lost=%d  McNemar chi2=%.2f p=%.4g (exact p=%.4g)"
        % (b2a, a2b, chi2, p_chi, p_exact)
    )
