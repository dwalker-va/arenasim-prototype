#!/usr/bin/env python3
"""AS-241: the shifted Druid's escape on the Nagrand Druid panel, paired.

usage: 2026-10-31-as241-escape.py before.rows.jsonl[.gz] after.rows.jsonl[.gz]

Rows are `tests/milestone_mechanisms.rs` output (full, or slimmed to the Druid
as committed here). The panel is the AS-164 Nagrand Druid panel
(`2026-10-10-as164-mech_nagrand.jsonl`, all `Legacy`) plus a copy of every
config with `"ai_profile": "TeamPlan"` and `@TeamPlan` appended to its label
(`2026-10-31-as241-panel.py`). Matches pair by (label, seed, map); a profile is
read off the label.

Per profile and arm:
- starting rooms: matches in which the shifted Druid walked into one, entries,
  shifted seconds inside one, and deaths inside one;
- deaths, and the share of them in Travel Form;
- escape windows (the first 1.5s of a shift with a melee or pet enemy within
  12yd): how many gained ground on that chaser, and of those that did not, how
  many were within 6yd of a wall or a pillar on the escape side at any frame;
- the same for the windows whose chaser was within 6yd at the shift — the
  escapes the run decides, as opposed to a chaser still closing (a Warrior's
  Charge) — with the number of distinct situations (comp, start time) behind
  them, because the panel's seeds repeat many deterministic situations;
- McNemar on the Druid side's wins.
"""
import gzip
import json
import math
import sys
from collections import defaultdict

PIN_YARDS = 6.0
CLOSE_YARDS = 6.0
FULL_SECS = 1.45


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
    close_situations = set()
    for r, d, team in rows.values():
        s['n'] += 1
        s['win'] += r['winner'] == f'team{team}'
        s['room_match'] += d['shifted_room_entries'] > 0
        s['room_secs'] += d['shifted_room_secs']
        s['room_entries'] += d['shifted_room_entries']
        s['deaths'] += d['death'] is not None
        s['died_shifted'] += d['died_shifted']
        s['died_in_room'] += d['died_in_room']
        for w in d['escapes']:
            if w['secs'] < FULL_SECS:
                continue
            s['full'] += 1
            gained = w['gap_end'] > w['gap_start']
            s['gain_yd'] += w['gap_end'] - w['gap_start']
            if not gained:
                s['nogain'] += 1
                wall = min(w['wall_start'], w['wall_min'])
                pillar = min(w['pillar_start'], w['pillar_min'])
                if min(wall, pillar) <= PIN_YARDS:
                    s['pin_wall' if wall <= pillar else 'pin_pillar'] += 1
            if w['gap_start'] < CLOSE_YARDS:
                s['close_full'] += 1
                if not gained:
                    s['close_nogain'] += 1
                    close_situations.add((r['label'].split('|')[0], round(w['start'], 1)))
    s['close_situations'] = len(close_situations)
    return s


def report(name, s):
    n = s['n']
    full = max(s['full'], 1)
    print(f"  {name}: {int(n)} matches, Druid side won {s['win'] / n:.1%}")
    print(f"    starting rooms: shifted entry in {int(s['room_match'])} matches "
          f"({s['room_match'] / n:.1%}), {int(s['room_entries'])} entries, "
          f"{s['room_secs'] / n:.2f} shifted s/match inside; deaths inside {int(s['died_in_room'])}")
    print(f"    deaths {int(s['deaths'])}, shifted {int(s['died_shifted'])} "
          f"({s['died_shifted'] / max(s['deaths'], 1):.1%})")
    print(f"    escape windows {int(s['full'])}: no ground gained in {int(s['nogain'])} "
          f"({s['nogain'] / full:.1%}; within {PIN_YARDS:.0f}yd of a wall {int(s['pin_wall'])}, "
          f"of a pillar {int(s['pin_pillar'])}); mean gain {s['gain_yd'] / full:.2f}yd")
    print(f"    chaser within {CLOSE_YARDS:.0f}yd at the shift: {int(s['close_full'])} windows, "
          f"no ground gained in {int(s['close_nogain'])} "
          f"({s['close_nogain'] / max(s['close_full'], 1):.1%}), "
          f"{s['close_situations']} distinct situations")


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
    for i, arm in enumerate((before, after)):
        for k, v in arm.items():
            groups[profile(v[0])][i][k] = v
    for name, (a, b) in sorted(groups.items()):
        print(f"== {name}")
        report('before', summarize(a))
        report('after', summarize(b))
        n, up, down, z = mcnemar(a, b)
        print(f"  paired n={n}: wins gained {up}, lost {down}, McNemar z={z:.2f}")


main()
