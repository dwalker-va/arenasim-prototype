#!/usr/bin/env python3
"""Class / comp tier lists from an `arenasim --batch` results CSV.

Usage: python3 scripts/comp_tiers.py <results.csv> [--size {2,3}]

Without --size, prints the legacy class tiers plus top/bottom comps. With it,
prints the fuller report the canonical baselines are written from: all-comps
and competitive-only class tiers, top/bottom comps, the non-competitive anomaly
canary, and (3v3) the dominant-shape watch.
"""
import argparse
import csv
import sys
from collections import defaultdict


def load(p):
    with open(p) as f:
        return list(csv.DictReader(f))

def class_tiers(rows):
    """Winrate of comps containing the class: for each match, each side's
    classes get a win/loss for that side. Draws count as losses for both."""
    stats = defaultdict(lambda: [0, 0])  # class -> [wins, games]
    for r in rows:
        t1, t2 = r['team1'].split('+'), r['team2'].split('+')
        w = r['winner']
        for c in set(t1):
            stats[c][0] += 1 if w == 'team1' else 0
            stats[c][1] += 1
        for c in set(t2):
            stats[c][0] += 1 if w == 'team2' else 0
            stats[c][1] += 1
    return {c: 100*w/n for c, (w, n) in stats.items()}

def comp_tiers(rows):
    stats = defaultdict(lambda: [0, 0])
    for r in rows:
        t1 = '+'.join(sorted(r['team1'].split('+')))
        t2 = '+'.join(sorted(r['team2'].split('+')))
        w = r['winner']
        stats[t1][0] += 1 if w == 'team1' else 0
        stats[t1][1] += 1
        stats[t2][0] += 1 if w == 'team2' else 0
        stats[t2][1] += 1
    return {c: 100*w/n for c, (w, n) in stats.items()}

def draws(rows):
    """Draw rate as a percentage, or None when there are no matches.

    No matches has no draw rate. Returning 0.0 would put "draws 0.0%" at the
    top of a report over an empty sweep, which reads as a finding.
    """
    if not rows:
        return None
    return 100*sum(1 for r in rows if r['winner'] not in ('team1','team2'))/len(rows)


def legacy_report(path, rows):
    print(f"# {path}: {len(rows)} matches, draws {draws(rows):.1f}%")
    print("## class tiers")
    for c, wr in sorted(class_tiers(rows).items(), key=lambda x: -x[1]):
        print(f"  {c:8s} {wr:5.1f}")
    ct = comp_tiers(rows)
    if len(ct) > 8:
        print("## top comps")
        for c, wr in sorted(ct.items(), key=lambda x: -x[1])[:8]:
            print(f"  {wr:5.1f}  {c}")
        print("## bottom comps")
        for c, wr in sorted(ct.items(), key=lambda x: -x[1])[-8:]:
            print(f"  {wr:5.1f}  {c}")

HEALERS = {'Priest', 'Paladin', 'Shaman'}

def is_competitive(team, size):
    """2v2: at most 1 healer (double-DPS is playable, double-healer is not).
    3v3: 1 or 2 healers — double-healer 3v3 is a legitimate WoW meta shape;
    only triple-DPS (0 healers) and triple-healer are non-competitive."""
    h = sum(1 for c in team if c in HEALERS)
    return h <= 1 if size == 2 else h in (1, 2)

def competitive_rows(rows):
    out = []
    for r in rows:
        t1, t2 = r['team1'].split('+'), r['team2'].split('+')
        if is_competitive(t1, len(t1)) and is_competitive(t2, len(t2)):
            out.append(r)
    return out

def noncompetitive_anomalies(rows):
    """Non-competitive comps (per is_competitive) that perform like competitive
    ones anyway. Reports winrate vs the FULL field and vs COMPETITIVE opponents
    only — the latter is the real alarm (beating real comps without a healer /
    with too many healers points at a fundamental balance issue).

    Each team is judged at its OWN size, exactly as `competitive_rows` judges
    it. Taking the bracket from the command line instead of from the team
    scored a row of a different size against the wrong healer rule, and the
    canary then pointed at comps that are perfectly ordinary."""
    full = defaultdict(lambda: [0, 0])   # comp -> [wins, games] vs anyone
    vs_comp = defaultdict(lambda: [0, 0])  # comp -> [wins, games] vs competitive opponents
    for r in rows:
        t1, t2 = r['team1'].split('+'), r['team2'].split('+')
        k1, k2 = '+'.join(sorted(t1)), '+'.join(sorted(t2))
        w = r['winner']
        for team, key, opp, won in [(t1, k1, t2, w == 'team1'), (t2, k2, t1, w == 'team2')]:
            if is_competitive(team, len(team)):
                continue
            full[key][0] += won; full[key][1] += 1
            if is_competitive(opp, len(opp)):
                vs_comp[key][0] += won; vs_comp[key][1] += 1
    out = []
    for key, (w, n) in full.items():
        wr = 100 * w / n
        cw, cn = vs_comp.get(key, (0, 0))
        cwr = 100 * cw / cn if cn else None
        out.append((key, wr, cwr))
    return sorted(out, key=lambda x: -x[1])

# ---------------------------------------------------------------------------
# CLI: python3 scripts/comp_tiers.py <canonical.csv> [--size 2|3]
# Prints all-comps + competitive class tiers, top/bottom comps, the
# non-competitive anomaly canary, and (3v3) the dominant-shape watch.
# Used to regenerate docs/design/balance/canonical_baselines_summary.md
# after a shipped balance change (see the balance-sweep skill).
# ---------------------------------------------------------------------------
def report(path, size, rows=None):
    rows = load(path) if rows is None else rows
    comp = competitive_rows(rows)
    print(f"# {path}: {len(rows)} matches, draws {draws(rows):.1f}%")
    all_t, comp_t = class_tiers(rows), class_tiers(comp)
    print(f"## class tiers (all-comps / competitive, sorted by competitive)")
    for c in sorted(comp_t, key=lambda c: -comp_t[c]):
        print(f"  {c:8s} {all_t[c]:5.1f} / {comp_t[c]:5.1f}")
    ct = comp_tiers(rows)
    print("## top comps (healer-count)")
    for k, wr in sorted(ct.items(), key=lambda x: -x[1])[:8]:
        h = sum(1 for c in k.split('+') if c in HEALERS)
        print(f"  {wr:5.1f}  {h}h  {k}")
    print("## bottom comps")
    for k, wr in sorted(ct.items(), key=lambda x: -x[1])[-8:]:
        print(f"  {wr:5.1f}  {k}")
    print("## anomaly canary: non-competitive comps (full-field / vs-competitive)")
    for key, wr, cwr in noncompetitive_anomalies(rows)[:5]:
        flag = "  << ANOMALY" if wr >= 50 or (cwr or 0) >= 50 else ""
        print(f"  {wr:5.1f} / {(cwr if cwr is not None else 0):5.1f}  {key}{flag}")
    if size == 3:
        top10 = sorted(ct.items(), key=lambda x: -x[1])[:10]
        two_h = sum(1 for k, _ in top10 if sum(1 for c in k.split('+') if c in HEALERS) == 2)
        print(f"## dominant-shape watch: {two_h}/10 top-10 comps are double-healer")

def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("csv", help="batch results CSV")
    ap.add_argument("--size", type=int, choices=(2, 3),
                    help="team size; adds the competitive slice, anomaly canary "
                         "and (3v3) dominant-shape watch")
    args = ap.parse_args(argv)

    try:
        rows = load(args.csv)
    except OSError as e:
        sys.exit(f"cannot read {args.csv}: {e}")
    if not rows:
        # An empty sweep has no tiers, no draw rate and no anomalies. Printing
        # a report full of zeroes over it would read as a set of findings.
        sys.exit(f"no matches in {args.csv}")

    if args.size is None:
        legacy_report(args.csv, rows)
    else:
        report(args.csv, args.size, rows)
    return 0


if __name__ == '__main__':
    sys.exit(main())
