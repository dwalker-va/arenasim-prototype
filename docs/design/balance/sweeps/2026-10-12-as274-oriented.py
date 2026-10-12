#!/usr/bin/env python3
"""AS-274 Peel head-to-head, scored from the PEELING side.

usage: 2026-10-12-as274-oriented.py <h2h.csv[.gz]> [--expect improvement]
       2026-10-12-as274-oriented.py --control <nn-branch.csv[.gz]> <nn-main.csv[.gz]>

The batch (2026-10-12-as274-gen.py h2h) runs every cell and seed three times,
both teams under TeamPlan: NN (Peel withheld from both), PN (team 1 peels) and
NP (team 2 peels). Each PN match is paired with its NN twin at the same seed
and scored for TEAM 1; each NP match with its NN twin and scored for TEAM 2.
So every pair asks one question — did giving THIS side the play change its
result — and no team-1 rate is pooled across assignments (pooling the two
nets a per-team change out, as `paired_sweep.py`'s docstring warns).

Per slice (map x assignment) and overall it prints:

- the peeling side's win rate without the play (NN) and with it, and the
  delta in points;
- McNemar on the win flips (to_win: a loss or draw that became a win;
  to_loss: a win that did not survive), continuity-corrected, the instrument
  `paired_sweep.py` uses, and the RESOLUTION FLOOR from the discordance the run
  produced (`paired_sweep.min_detectable_delta`): the smallest delta this run
  could have called significant;
- an exact two-sided sign test on the ordinal outcome (win > draw > loss).

`--expect improvement` answers the card's success condition: the overall
delta positive and significant (z >= 1.96).

`--control` compares two arms' NN rows (the branch with Peel withheld from
both teams, and main's TeamPlan at the same seeds): every row must match in
winner and duration, which is the claim that the branch changes nothing under
TeamPlan but the Peel play.
"""
from __future__ import annotations

import csv
import gzip
import math
import sys
from collections import defaultdict


def read(path):
    opener = gzip.open if path.endswith(".gz") else open
    with opener(path, "rt") as f:
        return list(csv.DictReader(f))


def score(row, side):
    if row["winner"] == side:
        return 1
    if row["winner"] == "draw":
        return 0
    return -1


def mcnemar(to_win, to_loss):
    d = to_win + to_loss
    if d == 0:
        return 0.0
    return max(0.0, abs(to_win - to_loss) - 1) / math.sqrt(d) * (1 if to_win >= to_loss else -1)


def floor(discordant, n):
    if n == 0 or discordant == 0:
        return None
    return 100.0 * (1.96 * math.sqrt(discordant) + 1) / n


def sign_test(plus, minus):
    n = plus + minus
    if n == 0:
        return 1.0
    k = min(plus, minus)
    tail = sum(math.comb(n, i) for i in range(k + 1)) / 2.0 ** n
    return min(1.0, 2 * tail)


class Slice:
    def __init__(self, name):
        self.name = name
        self.n = 0
        self.base_wins = 0
        self.peel_wins = 0
        self.to_win = 0
        self.to_loss = 0
        self.better = 0
        self.worse = 0

    def add(self, base, peel):
        self.n += 1
        self.base_wins += base == 1
        self.peel_wins += peel == 1
        self.to_win += base != 1 and peel == 1
        self.to_loss += base == 1 and peel != 1
        self.better += peel > base
        self.worse += peel < base

    @property
    def delta(self):
        return 100.0 * (self.peel_wins - self.base_wins) / self.n if self.n else 0.0

    @property
    def z(self):
        return mcnemar(self.to_win, self.to_loss)

    def line(self):
        f = floor(self.to_win + self.to_loss, self.n)
        return (
            f"{self.name:<28} n={self.n:5d}  peeling side wins {100*self.base_wins/self.n:5.1f}% "
            f"-> {100*self.peel_wins/self.n:5.1f}%  delta {self.delta:+5.1f}pt  "
            f"flips +{self.to_win}/-{self.to_loss} z={self.z:+.2f} "
            f"{'SIG' if abs(self.z) >= 1.96 else 'ns '}  "
            f"{'resolves >=%.1fpt' % f if f is not None else 'nothing flipped'}  "
            f"ordinal +{self.better}/-{self.worse} p={sign_test(self.better, self.worse):.3f}"
        )


def analyse(path, expect):
    rows = read(path)
    by = {}
    for r in rows:
        base, arm = r["label"].rsplit("#", 1)
        by[(base, r["seed"], arm)] = r
    keys = sorted({(b, s) for (b, s, _) in by})
    slices = defaultdict(lambda: None)
    overall = Slice("OVERALL")
    order = []
    by_class = {}
    missing = 0
    for base, seed in keys:
        nn = by.get((base, seed, "NN"))
        map_name = base.rsplit("@", 1)[1]
        for arm, side in (("PN", "team1"), ("NP", "team2")):
            peel = by.get((base, seed, arm))
            if nn is None or peel is None:
                missing += 1
                continue
            name = f"{map_name} {arm} ({side} peels)"
            if slices[name] is None:
                slices[name] = Slice(name)
                order.append(name)
            s0, s1 = score(nn, side), score(peel, side)
            slices[name].add(s0, s1)
            overall.add(s0, s1)
            teams = base.rsplit("@", 1)[0].split("_vs_")
            peeling = teams[0] if side == "team1" else teams[1]
            for cls in peeling.split("+"):
                key = f"peeling team has {cls}"
                if by_class.get(key) is None:
                    by_class[key] = Slice(key)
                by_class[key].add(s0, s1)
    decisive = sum(1 for r in rows if r["end_reason"] == "kill")
    durations = len({r["duration_secs"] for r in rows})
    print(f"{len(rows)} matches, {decisive} decisive, {durations} distinct durations, "
          f"{missing} unpaired")
    moved = overall.better + overall.worse
    print(f"{overall.n} peel/no-peel pairs; {moved} changed the peeling side's result "
          f"({100*moved/max(overall.n,1):.1f}%)")
    print(f"{len(order)} slices tested:")
    for name in sorted(order):
        print("  " + slices[name].line())
    print("  " + overall.line())
    print(f"per class on the peeling team ({len(by_class)} more slices; each pair "
          f"counts once per class on the team, so these overlap):")
    for name in sorted(by_class):
        print("  " + by_class[name].line())
    if expect == "improvement":
        f = floor(overall.to_win + overall.to_loss, overall.n)
        if overall.z >= 1.96:
            print(f"VERDICT: improvement, {overall.delta:+.1f}pt for the peeling side "
                  f"(z={overall.z:.2f}), resolved (floor {f:.1f}pt).")
        elif overall.z <= -1.96:
            print(f"VERDICT: the run CONTRADICTS its success condition: "
                  f"{overall.delta:+.1f}pt for the peeling side (z={overall.z:.2f}).")
        else:
            print(f"VERDICT: NOT an improvement at this size: {overall.delta:+.1f}pt "
                  f"(z={overall.z:.2f}); the run could have resolved {f:.1f}pt or more, "
                  f"so it rules out an effect above that and says nothing below it.")


def control(branch_path, main_path):
    b = {(r["label"], r["seed"]): r for r in read(branch_path)}
    m = {(r["label"], r["seed"]): r for r in read(main_path)}
    assert b.keys() == m.keys(), "the two arms are not paired"
    same = sum(
        1 for k in b
        if (b[k]["winner"], b[k]["duration_secs"]) == (m[k]["winner"], m[k]["duration_secs"])
    )
    decisive = sum(1 for r in b.values() if r["end_reason"] == "kill")
    print(f"CONTROL: {same}/{len(b)} NN rows identical in winner and duration "
          f"between the branch (Peel withheld from both) and main (TeamPlan); "
          f"{decisive} decisive, {len({r['duration_secs'] for r in b.values()})} distinct durations")
    return same == len(b)


if __name__ == "__main__":
    args = sys.argv[1:]
    if args and args[0] == "--control":
        sys.exit(0 if control(args[1], args[2]) else 1)
    expect = None
    if "--expect" in args:
        i = args.index("--expect")
        expect = args[i + 1]
        del args[i:i + 2]
    analyse(args[0], expect)
