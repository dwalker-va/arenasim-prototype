#!/usr/bin/env python3
"""AS-242: the dying Druid's emergency leave from Travel Form, paired.

usage: 2026-10-31-as242-leave.py before.rows.jsonl[.gz] after.rows.jsonl[.gz]

Rows are `tests/milestone_mechanisms.rs` output (full, or slimmed to the Druid
as committed here) over the AS-241 panel: the AS-164 Nagrand Druid panel under
`Legacy` plus every config again under `TeamPlan`
(`2026-10-31-as241-panel.py`). Matches pair by (label, seed, map); a profile is
read off the label.

Per profile and arm:
- Druid deaths, and the share of them in Travel Form;
- seconds per match the Druid spent shifted below 45% (dying), and the part of
  them with no melee or pet enemy within 7.5yd — the frames an emergency
  leave can use;
- leaves of the form: every one, the unrooted ones, and the DYING unrooted
  ones (below 45% on the first frame out). For the dying ones: how many cast a
  heal within 0.1s (the same beat), and how many died within 3s;
- STROBES: unrooted leaves followed by a re-shift within 1.5s (the reshift
  hold, so none should) and within 3s (the dwell the form is meant to buy);
- McNemar on the Druid side's wins.
"""
import gzip
import json
import math
import sys
from collections import defaultdict

DYING = 0.45
SAME_BEAT = 0.1
DIED_SOON = 3.0
STROBE_HOLD = 1.5
STROBE_DWELL = 3.0


def lines(path):
    return gzip.open(path, 'rt') if path.endswith('.gz') else open(path)


def load(path):
    out = {}
    for line in lines(path):
        r = json.loads(line)
        team = 1 if 'Druid' in r['team1'].split('+') else 2
        d = [u for u in r['units'] if u['class'] == 'Druid' and u['team'] == team][0]
        out[(r['label'], r['seed'], r['map'])] = (r, d, team)
    return out


def profile(row):
    return row['label'].split('@')[-1] if '@' in row['label'] else 'Legacy'


def summarize(rows):
    s = defaultdict(float)
    for r, d, team in rows.values():
        s['n'] += 1
        s['win'] += r['winner'] == f'team{team}'
        s['deaths'] += d['death'] is not None
        s['died_shifted'] += d['died_shifted']
        s['dying_secs'] += d['dying_shifted_secs']
        s['dying_unstruck_secs'] += d['dying_unstruck_shifted_secs']
        for l in d['leaves']:
            s['leaves'] += 1
            if l['rooted']:
                continue
            s['unrooted'] += 1
            re = l['reshift_after']
            s['strobe_hold'] += re is not None and re < STROBE_HOLD
            s['strobe_dwell'] += re is not None and re < STROBE_DWELL
            if l['hp'] < DYING:
                s['dying_leaves'] += 1
                s['dying_heal'] += l['heal_after'] is not None and l['heal_after'] <= SAME_BEAT
                s['dying_died'] += l['death_after'] is not None and l['death_after'] <= DIED_SOON
                s['dying_strobe'] += re is not None and re < STROBE_DWELL
    return s


def report(name, s):
    n = s['n']
    print(f"  {name}: {int(n)} matches, Druid side won {s['win'] / n:.1%}")
    print(f"    deaths {int(s['deaths'])}, shifted {int(s['died_shifted'])} "
          f"({s['died_shifted'] / max(s['deaths'], 1):.1%})")
    print(f"    shifted and dying {s['dying_secs'] / n:.2f}s/match, "
          f"{s['dying_unstruck_secs'] / n:.2f}s of it with no chaser within 7.5yd")
    print(f"    leaves {int(s['leaves'])}, unrooted {int(s['unrooted'])}: "
          f"re-shift within {STROBE_HOLD}s {int(s['strobe_hold'])}, "
          f"within {STROBE_DWELL:.0f}s {int(s['strobe_dwell'])}")
    print(f"    dying unrooted leaves {int(s['dying_leaves'])}: heal on the same beat "
          f"{int(s['dying_heal'])}, re-shift within {STROBE_DWELL:.0f}s {int(s['dying_strobe'])}, "
          f"died within {DIED_SOON:.0f}s {int(s['dying_died'])}")


def mcnemar(a, b):
    up = down = 0
    keys = sorted(set(a) & set(b))
    for k in keys:
        (ra, _, ta), (rb, _, tb) = a[k], b[k]
        wa, wb = ra['winner'] == f'team{ta}', rb['winner'] == f'team{tb}'
        up += wb and not wa
        down += wa and not wb
    z = (up - down) / math.sqrt(up + down) if up + down else 0.0
    return len(keys), up, down, z


def main():
    before, after = load(sys.argv[1]), load(sys.argv[2])
    groups = defaultdict(lambda: ({}, {}))
    for k, v in before.items():
        groups[profile(v[0])][0][k] = v
    for k, v in after.items():
        groups[profile(v[0])][1][k] = v
    for name, (b, a) in sorted(groups.items()):
        print(f"== {name} ==")
        report('before', summarize(b))
        report('after', summarize(a))
        n, up, down, z = mcnemar(b, a)
        print(f"  paired {n}: Druid side +{up}/-{down}, McNemar z={z:.2f}")


if __name__ == '__main__':
    main()
