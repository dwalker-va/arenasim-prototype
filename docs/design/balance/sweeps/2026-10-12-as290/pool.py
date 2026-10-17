"""Pool the per-(peeler@ally) peel effect: seeds 1-4 (per-class ablations) + 5-16 (confirmation).

usage: pool.py [dir of the CSVs; default: this script's]

The peel's effect in a context = (ungated) - (that context gated), scored from the peeling side.
"""
import csv, gzip, math, os, sys
from collections import defaultdict

D = (sys.argv[1] if len(sys.argv) > 1 else os.path.dirname(os.path.abspath(__file__))) + '/'


def read(p):
    with gzip.open(D + p + '.gz', 'rt') as f:
        return {(r['label'], r['seed']): r for r in csv.DictReader(f)}


def sc(r, s):
    return 1 if r['winner'] == s else (0 if r['winner'] == 'draw' else -1)


def z(u, d):
    return 0 if u + d == 0 else max(0, abs(u - d) - 1) / math.sqrt(u + d) * (1 if u >= d else -1)


acc = defaultdict(lambda: {'early': [0, 0, 0], 'fresh': [0, 0, 0]})


def add(with_rows, without_rows, peeler, half, only_allies=None):
    for k, r in without_rows.items():
        if k not in with_rows:
            continue
        base, arm = k[0].rsplit('#', 1)
        side = 'team1' if arm == 'PN' else 'team2'
        t1, t2 = base.rsplit('@', 1)[0].split('_vs_')
        team = (t1 if side == 'team1' else t2).split('+')
        if peeler not in team:
            continue
        ally = [c for c in team if c != peeler][0]
        if only_allies is not None and ally not in only_allies:
            continue
        b, p = sc(r, side), sc(with_rows[k], side)
        a = acc[f"{peeler}@{ally}"][half]
        a[0] += 1
        a[1] += b != 1 and p == 1
        a[2] += b == 1 and p != 1


main = read('h2h-main.csv')
for x in ["Mage", "Warlock", "Paladin", "Hunter", "Rogue", "Druid", "Priest", "Shaman"]:
    add(main, read(f'abl-{x}.csv'), x, 'early')
cu, cg = read('conf-ungated.csv'), read('conf-gated.csv')
# conf gated arm: every peeler @Hunter / @Warlock, plus Druid@Warrior.
for x in ["Mage", "Paladin", "Rogue", "Druid", "Priest", "Shaman", "Warlock"]:
    add(cu, cg, x, 'fresh', {"Hunter"})
for x in ["Mage", "Paladin", "Rogue", "Druid", "Priest", "Shaman", "Hunter"]:
    add(cu, cg, x, 'fresh', {"Warlock"})
add(cu, cg, "Druid", 'fresh', {"Warrior"})
print(f"{'context':<18} {'seeds 1-4':>26} {'seeds 5-16':>26} {'pooled':>30}")
for k in sorted(acc):
    e, f = acc[k]['early'], acc[k]['fresh']
    if f[0] == 0:
        continue
    p = [e[i] + f[i] for i in range(3)]
    fmt = lambda a: f"{100*(a[1]-a[2])/a[0]:+5.2f}pt +{a[1]}/-{a[2]} z={z(a[1], a[2]):+.2f}"
    print(f"{k:<18} {fmt(e):>26} {fmt(f):>26} {fmt(p):>30}  n={p[0]}")
