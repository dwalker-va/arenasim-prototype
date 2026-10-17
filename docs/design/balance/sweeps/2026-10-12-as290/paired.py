#!/usr/bin/env python3
"""Paired comparison of two arms, scored from the PEELING side.

usage: paired.py <with.csv[.gz]> <without.csv[.gz]> [--nn] [--by class|comp|map|opp] [--only CLASS]

Rows are matched by (label, seed). A label's arm suffix says which side peels:
#PN -> team1, #NP -> team2; #NN rows are never scored. `with` is the arm in
which the context under test peels, `without` the arm in which it does not.
With --nn, `without` is the same file's (or another's) NN twin of each row:
the AS-274 comparison, the play against no play. Prints the delta for the
peeling side, McNemar on the win flips and the run's resolution floor, overall
and per slice (--by comp: the peeling team; --by class: each class on it).
"""
import csv, gzip, math, sys
from collections import defaultdict


def read(p):
    op = gzip.open if p.endswith('.gz') else open
    with op(p, 'rt') as f:
        return {(r['label'], r['seed']): r for r in csv.DictReader(f)}


def score(r, side):
    return 1 if r['winner'] == side else (0 if r['winner'] == 'draw' else -1)


def mcnemar(a, b):
    d = a + b
    return 0.0 if d == 0 else max(0.0, abs(a - b) - 1) / math.sqrt(d) * (1 if a >= b else -1)


class S:
    def __init__(s):
        s.n = s.w0 = s.w1 = s.up = s.dn = 0

    def add(s, b, p):
        s.n += 1; s.w0 += b == 1; s.w1 += p == 1
        s.up += b != 1 and p == 1; s.dn += b == 1 and p != 1

    def line(s, name):
        d = 100 * (s.w1 - s.w0) / s.n if s.n else 0
        z = mcnemar(s.up, s.dn)
        fl = 100 * (1.96 * math.sqrt(s.up + s.dn) + 1) / s.n if s.n else 0
        return f"{name:<34} n={s.n:5d} {100*s.w0/max(s.n,1):5.1f}% -> {100*s.w1/max(s.n,1):5.1f}% delta {d:+5.2f}pt flips +{s.up}/-{s.dn} z={z:+.2f} floor {fl:.1f}"


def main():
    args = sys.argv[1:]
    by = None; only = None
    nn = '--nn' in args
    if nn:
        args.remove('--nn')
    if '--by' in args:
        i = args.index('--by'); by = args[i + 1]; del args[i:i + 2]
    if '--only' in args:
        i = args.index('--only'); only = args[i + 1]; del args[i:i + 2]
    w, wo = read(args[0]), read(args[1])
    keys = sorted(w)
    tot = S(); sl = defaultdict(S)
    for k in keys:
        label = k[0]
        base, arm = label.rsplit('#', 1)
        if arm not in ('PN', 'NP'):
            continue
        side = 'team1' if arm == 'PN' else 'team2'
        cells, mp = base.rsplit('@', 1)
        t1, t2 = cells.split('_vs_')
        peeling = (t1 if side == 'team1' else t2).split('+')
        opp = t2 if side == 'team1' else t1
        if only and only not in peeling:
            continue
        wk = (base + '#NN', k[1]) if nn else k
        if wk not in wo:
            continue
        b, p = score(wo[wk], side), score(w[k], side)
        tot.add(b, p)
        if by == 'class':
            for c in peeling:
                sl[c].add(b, p)
        elif by == 'comp':
            sl['+'.join(peeling)].add(b, p)
        elif by == 'map':
            sl[f"{mp} {arm}"].add(b, p)
        elif by == 'opp':
            for c in opp.split('+'):
                sl['vs ' + c].add(b, p)
    for name in sorted(sl):
        print('  ' + sl[name].line(name))
    print(tot.line('TOTAL'))


main()
