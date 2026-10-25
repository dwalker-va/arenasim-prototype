#!/usr/bin/env python3
"""AS-398 scoring, from the `--batch` CSVs of `2026-10-25-as398-h2h-gen.py`.

    2026-10-25-as398-score.py identity <a.csv> <b.csv>
    2026-10-25-as398-score.py moved <main-plain.csv> <as398-plain.csv>
    2026-10-25-as398-score.py h2h <main-plain.csv> <main-sim1.csv> <main-sim2.csv>

identity  row for row, winner, end reason and duration: the AS-398 build's
          `plain` against main's `sim` (AS-398's change in main's controls).
          Prints the count that differ (must be 0) and the non-vacuity of the
          bank (decisive matches, distinct durations).
moved     main's `plain` against AS-398's `plain`, uniform TeamPlan: which
          rows changed, attributed to the change that can reach them -- a
          3v3 team with a Mage or a Druid (the format gates), a 2v2 team of
          Paladin+Hunter or Hunter+Priest (the two context gates), else
          TakeCover (a team down to a lone healer) -- then the paired slices
          from `2026-10-25-0.8-score.py` (both seats; blocks 901-902 /
          911-912 / 921-922, 3v3 901 / 911 / 921).
h2h       the head-to-head: one team runs AS-398's behaviour (main's `sim1` /
          `sim2` arms, which `identity` shows are AS-398's own), the other
          main's; each side against the same side in the all-main `plain`
          twin, both seats.
"""
import collections
import csv
import gzip
import importlib.util
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("s08", os.path.join(HERE, "2026-10-25-0.8-score.py"))
s08 = importlib.util.module_from_spec(spec)
spec.loader.exec_module(s08)


def rows(path):
    opener = gzip.open if path.endswith(".gz") else open
    out = {}
    with opener(path, "rt") as f:
        for r in csv.DictReader(f):
            out[r["label"]] = (r["winner"], r["end_reason"], r["duration_secs"])
    return out


def identity(a_path, b_path):
    a, b = rows(a_path), rows(b_path)
    assert set(a) == set(b), "the two files hold different rows"
    differ = [k for k in a if a[k] != b[k]]
    decisive = sum(1 for v in a.values() if v[0] in ("team1", "team2"))
    durations = len({v[2] for v in a.values()})
    print(f"rows {len(a)}, differing {len(differ)}; decisive {decisive}, "
          f"distinct durations {durations}")
    for k in differ[:20]:
        print("  DIFFER", k, a[k], b[k])
    return 1 if differ else 0


def reach(label):
    """The changes that can reach a row: a team holding a Mage or a Druid in
    3v3 (the format gates), a team holding Paladin+Hunter or Hunter+Priest
    (the two context gates, in either format), and a team left with a lone
    healer (TakeCover). On BasicArena TakeCover's movement was inert, so only
    its place in the walk reaches a row there: it held a last-stand healer
    off the Chain play below it."""
    _, m, t1, t2, _ = label.split("|")
    teams = [set(t1.split("+")), set(t2.split("+"))]
    size = len(teams[0])
    gates = []
    if size == 3 and any(t & {"Mage", "Druid"} for t in teams):
        gates.append("format")
    if any({"Paladin", "Hunter"} <= t or {"Hunter", "Priest"} <= t for t in teams):
        gates.append("context")
    if gates:
        return f"{size}v{size} {' + '.join(gates)} gates"
    if m == "BasicArena":
        return f"{size}v{size} BasicArena, no gate (TakeCover's place in the walk)"
    return f"{size}v{size} TakeCover only"


def moved(main_path, new_path):
    a, b = rows(main_path), rows(new_path)
    assert set(a) == set(b), "the two files hold different rows"
    total = collections.Counter(reach(k) for k in a)
    changed = collections.Counter(reach(k) for k in a if a[k] != b[k])
    flipped = collections.Counter(reach(k) for k in a if a[k][0] != b[k][0])
    print("| rows the change can reach | rows | changed | winner changed |")
    print("|---|---|---|---|")
    for k in sorted(total):
        print(f"| {k} | {total[k]} | {changed[k]} | {flipped[k]} |")
    data = {}
    for tag, src in (("main", a), ("as398", b)):
        for label, (w, reason, dur) in src.items():
            _, m, t1, t2, s = label.split("|")
            data[(tag, m, t1, t2, int(s))] = (w, reason, float(dur))
    acc = {}
    s08.accumulate(acc, s08.pair_sides(data, "as398", "main"))
    print()
    s08.report(acc, minimum=1, only=lambda k, p: (p.up + p.down) > 0 or k == "all")
    return 0


def h2h(plain_path, sim1_path, sim2_path):
    """The side running AS-398's behaviour (`sim1`: team 1, `sim2`: team 2,
    both on main) against the same side in main's own `plain` twin: AS-398
    head-to-head against main, both seats."""
    data = {}
    for tag, path in (("plain", plain_path), ("s1", sim1_path), ("s2", sim2_path)):
        for label, (w, reason, dur) in rows(path).items():
            _, m, t1, t2, s = label.split("|")
            data[(tag, m, t1, t2, int(s))] = (w, reason, float(dur))
    acc = {}
    s08.accumulate(acc, s08.pair_sides(data, "s1", "plain", seat_of=1))
    s08.accumulate(acc, s08.pair_sides(data, "s2", "plain", seat_of=2))
    s08.report(acc, minimum=1, only=lambda k, p: (p.up + p.down) > 0 or k == "all")
    return 0


if __name__ == "__main__":
    cmd = sys.argv[1]
    sys.exit({"identity": identity, "moved": moved, "h2h": h2h}[cmd](*sys.argv[2:]))
