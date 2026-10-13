#!/usr/bin/env python3
"""AS-276 Chain head-to-head, scored from the CHAINING side.

usage: 2026-10-12-as276-oriented.py <h2h.csv[.gz]> [--expect improvement]
       2026-10-12-as276-oriented.py --control <nn-branch.csv[.gz]> <teamplan-main.csv[.gz]>

The batch (2026-10-12-as276-gen.py h2h) runs every cell and seed three times,
both teams under TeamPlan, every class's chain on (`chain_exceptions: []`):
NN (Chain withheld from both), CN (team 1 chains) and NC (team 2 chains). Each
CN match is paired with its NN twin at the same seed and scored for TEAM 1;
each NC match with its NN twin and scored for TEAM 2. So every pair asks one
question — did giving THIS side the play change its result — and no team-1
rate is pooled across assignments (pooling the two nets a per-team change
out, as `paired_sweep.py`'s docstring warns).

Per slice (map x assignment) and overall it prints:

- the chaining side's win rate without the play (NN) and with it, and the
  delta in points;
- McNemar on the win flips (to_win: a loss or draw that became a win;
  to_loss: a win that did not survive), continuity-corrected, the instrument
  `paired_sweep.py` uses, and the RESOLUTION FLOOR from the discordance the run
  produced (`paired_sweep.min_detectable_delta`): the smallest delta this run
  could have called significant;
- an exact two-sided sign test on the ordinal outcome (win > draw > loss).

Then PER CHAINING CLASS (a class with a hand-off: `class_ai::chain::chain_tools`),
in two cuts:

- `chaining team has X`: every pair whose chaining team fields X. A team of
  two chainers counts once for each, so these overlap;
- `X is the only chainer`: the pairs whose chaining team fields X and no other
  class with a hand-off. Clean attribution — nothing else on the team hands
  off — on fewer pairs.

A class whose `only chainer` slice is significantly negative (z <= -1.96) is
flagged EXCEPTION: the ruling (2026-10-12) is that the play is not used where
it measures negative, so it goes in `situation::CHAIN_EXCEPTIONS` with this
row as its finding.

`--expect improvement` answers the card's success condition: the overall
delta positive and significant (z >= 1.96).

`--control` compares the branch's NN rows (Chain withheld from both teams)
with main's TeamPlan at the same seeds (`gen.py teamplan`): every row must
match in winner and duration, which is the claim that the branch changes
nothing under TeamPlan but the Chain play.
"""
from __future__ import annotations

import csv
import gzip
import math
import sys
from collections import defaultdict

CHAINERS = ["Paladin", "Rogue", "Warlock", "Mage", "Druid"]


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
        self.play_wins = 0
        self.to_win = 0
        self.to_loss = 0
        self.better = 0
        self.worse = 0

    def add(self, base, play):
        self.n += 1
        self.base_wins += base == 1
        self.play_wins += play == 1
        self.to_win += base != 1 and play == 1
        self.to_loss += base == 1 and play != 1
        self.better += play > base
        self.worse += play < base

    @property
    def delta(self):
        return 100.0 * (self.play_wins - self.base_wins) / self.n if self.n else 0.0

    @property
    def z(self):
        return mcnemar(self.to_win, self.to_loss)

    def line(self):
        f = floor(self.to_win + self.to_loss, self.n)
        return (
            f"{self.name:<34} n={self.n:5d}  chaining side wins {100*self.base_wins/self.n:5.1f}% "
            f"-> {100*self.play_wins/self.n:5.1f}%  delta {self.delta:+5.1f}pt  "
            f"flips +{self.to_win}/-{self.to_loss} z={self.z:+.2f} "
            f"{'SIG' if abs(self.z) >= 1.96 else 'ns '}  "
            f"{'resolves >=%.1fpt' % f if f is not None else 'nothing flipped'}  "
            f"ordinal +{self.better}/-{self.worse} p={sign_test(self.better, self.worse):.3f}"
        )


def get(table, key):
    if key not in table:
        table[key] = Slice(key)
    return table[key]


def analyse(path, expect):
    rows = read(path)
    by = {}
    for r in rows:
        base, arm = r["label"].rsplit("#", 1)
        by[(base, r["seed"], arm)] = r
    keys = sorted({(b, s) for (b, s, _) in by})
    slices = {}
    has = {}
    only = {}
    overall = Slice("OVERALL")
    missing = 0
    for base, seed in keys:
        nn = by.get((base, seed, "NN"))
        map_name = base.rsplit("@", 1)[1]
        for arm, side in (("CN", "team1"), ("NC", "team2")):
            play = by.get((base, seed, arm))
            if nn is None or play is None:
                missing += 1
                continue
            s0, s1 = score(nn, side), score(play, side)
            get(slices, f"{map_name} {arm} ({side} chains)").add(s0, s1)
            overall.add(s0, s1)
            teams = base.rsplit("@", 1)[0].split("_vs_")
            chaining = (teams[0] if side == "team1" else teams[1]).split("+")
            chainers = [c for c in chaining if c in CHAINERS]
            for cls in chainers:
                get(has, f"chaining team has {cls}").add(s0, s1)
            if len(chainers) == 1:
                get(only, f"{chainers[0]} is the only chainer").add(s0, s1)
    decisive = sum(1 for r in rows if r["end_reason"] == "kill")
    durations = len({r["duration_secs"] for r in rows})
    print(f"{len(rows)} matches, {decisive} decisive, {durations} distinct durations, "
          f"{missing} unpaired")
    moved = overall.better + overall.worse
    print(f"{overall.n} chain/no-chain pairs; {moved} changed the chaining side's result "
          f"({100*moved/max(overall.n,1):.1f}%)")
    print(f"{len(slices)} map x assignment slices:")
    for name in sorted(slices):
        print("  " + slices[name].line())
    print("  " + overall.line())
    print("per chaining class on the chaining team (each pair counts once per chainer on "
          "the team, so these overlap):")
    for name in sorted(has):
        print("  " + has[name].line())
    print("per chaining class as the chaining team's only chainer (clean attribution):")
    for name in sorted(only):
        s = only[name]
        flag = "  EXCEPTION (significantly negative)" if s.z <= -1.96 else ""
        print("  " + s.line() + flag)
    if expect == "improvement":
        f = floor(overall.to_win + overall.to_loss, overall.n)
        if overall.z >= 1.96:
            print(f"VERDICT: improvement, {overall.delta:+.1f}pt for the chaining side "
                  f"(z={overall.z:.2f}), resolved (floor {f:.1f}pt).")
        elif overall.z <= -1.96:
            print(f"VERDICT: the run CONTRADICTS its success condition: "
                  f"{overall.delta:+.1f}pt for the chaining side (z={overall.z:.2f}).")
        else:
            print(f"VERDICT: NOT an improvement at this size: {overall.delta:+.1f}pt "
                  f"(z={overall.z:.2f}); the run could have resolved {f:.1f}pt or more, "
                  f"so it rules out an effect above that and says nothing below it.")


def control(branch_path, main_path):
    b = {}
    for r in read(branch_path):
        base, arm = r["label"].rsplit("#", 1)
        if arm == "NN":
            b[(base, r["seed"])] = r
    m = {(r["label"], r["seed"]): r for r in read(main_path)}
    assert b.keys() == m.keys(), "the two arms are not paired"
    same = sum(
        1 for k in b
        if (b[k]["winner"], b[k]["duration_secs"]) == (m[k]["winner"], m[k]["duration_secs"])
    )
    decisive = sum(1 for r in b.values() if r["end_reason"] == "kill")
    print(f"CONTROL: {same}/{len(b)} NN rows identical in winner and duration "
          f"between the branch (Chain withheld from both) and main (TeamPlan); "
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
