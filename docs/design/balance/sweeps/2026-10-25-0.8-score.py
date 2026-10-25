#!/usr/bin/env python3
"""AS-395 scoring: the 0.8 milestone sweep, from the committed `--batch` CSVs.

    2026-10-25-0.8-score.py <command> [args] <csv[.gz]>...

Every CSV row's label is `bank|map|team1|team2|seed`
(`2026-10-25-0.8-gen.py`); the files may be passed in any order and mixed.
A side WINS when the match's winner is its team; a draw is a non-win for both.

Blocks. Every figure reported as an effect carries its value in each of the
three independent seed blocks (2v2: 801-810 / 811-820 / 821-830; 3v3: 801 /
811 / 821; AS-391's bank: 841-844 / 845-848 / 849-852; the Legacy-since-0.7
bank: 0-9 / 10-19 / 20-29, 3v3 0 / 1 / 2; the confirmation runs: 871-872 /
873-874 / 875-876 — score those files on their own). An effect REPLICATES when its
sign agrees in all three blocks and its pooled McNemar |z| >= 1.96.

Commands
--------
standings               class win rates per profile x map x format, all and
                        competitive, with per-block values; non-vacuity
draws                   draw rates per bank x map x format, competitive or not
pair <arm:base:seat>,.. any arm bank against any base bank (seat 1, 2, or 0
                        for both); the confirmation runs
comps <size> [k]        comp win rates against the competitive field; top k,
                        and every no-healer comp (the canaries)
h2h                     TeamPlan against Legacy: the TeamPlan side's result
                        against the same side in the all-Legacy twin (LL)
play <Play>             the play's head-to-head (PN/NP against NN, scored from
                        the side that runs it), its exceptions lifted (PNx/NPx
                        against PN/NP), and its uniform effect (TT against NN)
focus                   the AS-389 target focus, uniform: TT against focus-off
pillar-h2h              3v3: a TeamPlan side against Legacy, Pillar on (h2h)
                        against Pillar withheld (pillar-h2h-off)
as391 <on> <off>        AS-391's own bank (labels `map:t1:t2:seed:tN`)
legacy07 <0.7 csv>      the Legacy BasicArena matrix at the 0.7 canonical
                        seeds, paired with the 0.7 CSV row for row
"""
import collections
import csv
import gzip
import math
import sys

HEALERS = {"Priest", "Paladin", "Shaman", "Druid"}
MELEE = {"Warrior", "Rogue"}
RANGED = {"Mage", "Hunter", "Warlock"}
# `chain::chain_tools`: the classes with a hand-off.
CHAINERS = {"Paladin", "Rogue", "Warlock", "Mage", "Druid"}
CLASSES = ["Warrior", "Mage", "Rogue", "Priest", "Warlock", "Paladin", "Hunter", "Shaman", "Druid"]
MAPS = ["BasicArena", "PillaredArena", "TwinPillars"]
MAPNAME = {"BasicArena": "Basic", "PillaredArena": "Nagrand", "TwinPillars": "Twin"}


def opener(p):
    return gzip.open(p, "rt") if p.endswith(".gz") else open(p)


def load(paths):
    """{(bank, map, t1, t2, seed): (winner, reason, duration)}"""
    out = {}
    for p in paths:
        with opener(p) as f:
            for r in csv.DictReader(f):
                parts = r["label"].split("|")
                if len(parts) != 5:
                    continue
                bank, m, t1, t2, s = parts
                out[(bank, m, t1, t2, int(s))] = (r["winner"], r["end_reason"], float(r["duration_secs"]))
    return out


def block(seed, size):
    if seed >= 871:
        return (seed - 871) // 2
    if seed >= 841:
        return (seed - 841) // 4
    if seed >= 801:
        return (seed - 801) // 10
    if size == 3:
        return seed
    return seed // 10


def nheal(team):
    return sum(1 for c in team.split("+") if c in HEALERS)


def competitive(t1, t2):
    size = len(t1.split("+"))
    if size == 2:
        return nheal(t1) <= 1 and nheal(t2) <= 1
    return 1 <= nheal(t1) <= 2 and 1 <= nheal(t2) <= 2


def wilson(k, n, z=1.96):
    if n == 0:
        return (0, 0)
    p = k / n
    d = 1 + z * z / n
    c = (p + z * z / (2 * n)) / d
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return (c - h, c + h)


# ---------------------------------------------------------------- paired core

class Paired:
    """Paired side outcomes: base win (0/1) against arm win, per block."""

    def __init__(self):
        self.n = 0
        self.b = 0  # base win
        self.a = 0  # arm win
        self.up = 0
        self.down = 0
        self.blocks = collections.defaultdict(lambda: [0, 0, 0, 0, 0])  # n, b, a, up, down

    def add(self, bw, aw, blk):
        self.n += 1
        self.b += bw
        self.a += aw
        self.up += (aw and not bw)
        self.down += (bw and not aw)
        x = self.blocks[blk]
        x[0] += 1
        x[1] += bw
        x[2] += aw
        x[3] += (aw and not bw)
        x[4] += (bw and not aw)

    def delta(self):
        return 100.0 * (self.a - self.b) / self.n if self.n else 0.0

    def z(self):
        d = self.up + self.down
        return (self.up - self.down) / math.sqrt(d) if d else 0.0

    def floor(self):
        """95% half-width on the delta, from the discordance (McNemar)."""
        if not self.n:
            return 0.0
        d = self.up + self.down
        var = d - (self.up - self.down) ** 2 / self.n
        return 100.0 * 1.96 * math.sqrt(max(var, 0)) / self.n

    def block_deltas(self):
        return [100.0 * (x[2] - x[1]) / x[0] if x[0] else float("nan")
                for _, x in sorted(self.blocks.items())]

    def replicates(self):
        bd = self.block_deltas()
        if len(bd) < 3 or abs(self.z()) < 1.96:
            return False
        return all(v > 0 for v in bd) or all(v < 0 for v in bd)

    def row(self, name):
        bd = " / ".join(f"{v:+.1f}" for v in self.block_deltas())
        mark = " **R**" if self.replicates() else ""
        return (f"| {name} | {self.n} | {100.0 * self.b / self.n:.1f} | {100.0 * self.a / self.n:.1f} | "
                f"{self.delta():+.1f} | {bd} | {self.up}/{self.down} | {self.z():+.2f}{mark} | {self.floor():.1f} |")


HEAD = ("| slice | n | base | arm | delta | per block | +/- | z | floor |\n"
        "|---|---|---|---|---|---|---|---|---|")


def side_keys(m, team, opp, seat, size):
    """The slices a side belongs to."""
    cls = team.split("+")
    oc = opp.split("+")
    keys = ["all", f"map={MAPNAME[m]}", f"{size}v{size}", f"seat={seat}",
            f"map={MAPNAME[m]} seat={seat}", f"{size}v{size} map={MAPNAME[m]}"]
    for c in sorted(set(cls)):
        keys.append(f"has {c}")
        keys.append(f"{size}v{size} has {c}")
        keys.append(f"{MAPNAME[m]} has {c}")
    if competitive(team, opp):
        keys.append(f"{size}v{size} competitive")
        keys.append(f"{size}v{size} competitive map={MAPNAME[m]}")
        for c in sorted(set(cls)):
            keys.append(f"{size}v{size} comp has {c}")
            if m != "BasicArena":
                keys.append(f"{size}v{size} pillars comp has {c}")
    for c in sorted(set(oc)):
        keys.append(f"vs {c}")
    h = [c for c in cls if c in HEALERS]
    if not h:
        keys.append("no healer")
        keys.append(f"{size}v{size} no healer")
    if len(h) >= 2:
        keys.append(f"{size}v{size} double healer")
        keys.append(f"{size}v{size} double healer map={MAPNAME[m]}")
    keys.append(f"team {team}" if size == 2 else f"3v3 team {team}")
    if size == 2:
        dps = [c for c in cls if c not in HEALERS]
        if len(dps) == 1:
            keys.append(f"only DPS {dps[0]}")
        chainers = [c for c in cls if c in CHAINERS]
        if len(chainers) == 1:
            keys.append(f"only chainer {chainers[0]}")
        if len(h) == 1 and sum(1 for o in oc if o in HEALERS) == 1:
            for o in oc:
                if o in HEALERS:
                    keys.append(f"{h[0]} vs {o}")
                    keys.append(f"{MAPNAME[m]} {h[0]} vs {o}")
    return keys


def won(res, team_n):
    return 1 if res[0] == f"team{team_n}" else 0


def report(acc, order=None, minimum=0, only=None):
    print(HEAD)
    keys = order or sorted(acc)
    for k in keys:
        p = acc.get(k)
        if p is None or p.n < minimum:
            continue
        if only and not only(k, p):
            continue
        print(p.row(k))


def pair_sides(data, arm_bank, base_bank, seat_of=None, sizes=None):
    """Yield (map, size, seat, team, opp, seed, base_win, arm_win) for every
    arm row with a twin in the base bank. seat_of(bank) -> the seat scored, or
    None for both seats (a uniform arm)."""
    for (bank, m, t1, t2, s), res in data.items():
        if bank != arm_bank:
            continue
        twin = data.get((base_bank, m, t1, t2, s))
        if twin is None:
            continue
        size = len(t1.split("+"))
        if sizes and size not in sizes:
            continue
        seats = [seat_of] if seat_of else [1, 2]
        for seat in seats:
            team, opp = (t1, t2) if seat == 1 else (t2, t1)
            yield m, size, seat, team, opp, s, won(twin, seat), won(res, seat)


def accumulate(acc, it):
    n = 0
    for m, size, seat, team, opp, s, bw, aw in it:
        n += 1
        for k in side_keys(m, team, opp, seat, size):
            acc.setdefault(k, Paired()).add(bw, aw, block(s, size))
    return n


# ---------------------------------------------------------------- commands

def cmd_standings(data):
    groups = collections.defaultdict(list)
    for (bank, m, t1, t2, s), res in data.items():
        if bank.startswith("uni-"):
            size = len(t1.split("+"))
            tag = bank[4:]
            if s < 801:
                tag += "@0.7seeds"
            groups[(tag, size, m)].append((t1, t2, s, res))
    for (tag, size, m), rows in sorted(groups.items()):
        print(f"\n### {tag} {size}v{size} {MAPNAME[m]}  ({len(rows)} matches)")
        draws = sum(1 for r in rows if r[3][0] == "draw")
        caps = sum(1 for r in rows if r[3][0] == "draw" and r[3][2] >= 299.9)
        kills = sum(1 for r in rows if r[3][1] == "kill" and r[3][0] != "draw")
        durs = len(set(r[3][2] for r in rows))
        print(f"non-vacuity: decisive {len(rows) - draws}, draws {draws} ({100.0 * draws / len(rows):.2f}%, "
              f"{caps} at the cap), distinct durations {durs}; team1 wins "
              f"{sum(1 for r in rows if r[3][0] == 'team1')}, team2 {sum(1 for r in rows if r[3][0] == 'team2')}")
        acc = {}
        for t1, t2, s, res in rows:
            comp = competitive(t1, t2)
            for seat, team in ((1, t1), (2, t2)):
                w = won(res, seat)
                b = block(s, size)
                for c in set(team.split("+")):
                    for sl in (["all", "comp"] if comp else ["all"]):
                        x = acc.setdefault((c, sl), [0, 0, collections.defaultdict(lambda: [0, 0])])
                        x[0] += 1
                        x[1] += w
                        x[2][b][0] += 1
                        x[2][b][1] += w
        print("| class | all | competitive | competitive per block | comp n |")
        print("|---|---|---|---|---|")
        order = sorted(CLASSES, key=lambda c: -acc.get((c, "comp"), [1, 0])[1] / max(acc.get((c, "comp"), [1, 0])[0], 1))
        for c in order:
            a = acc.get((c, "all"))
            k = acc.get((c, "comp"))
            if not a:
                continue
            bl = " / ".join(f"{100.0 * v[1] / v[0]:.1f}" for _, v in sorted(k[2].items())) if k else "-"
            print(f"| {c} | {100.0 * a[1] / a[0]:.1f} | {100.0 * k[1] / k[0]:.1f} | {bl} | {k[0]} |")


def cmd_draws(data):
    """Draw rates by bank x map x format, split competitive / not, plus the
    cells that draw most."""
    groups = collections.defaultdict(list)
    for (bank, m, t1, t2, s), res in data.items():
        groups[(bank, len(t1.split("+")), m)].append((t1, t2, s, res))
    print("| bank | fmt | map | n | draws | competitive draws | other draws | cap draws | median dur |")
    print("|---|---|---|---|---|---|---|---|---|")
    worst = {}
    for (bank, size, m), rows in sorted(groups.items()):
        comp = [r for r in rows if competitive(r[0], r[1])]
        oth = [r for r in rows if not competitive(r[0], r[1])]
        d = lambda rs: sum(1 for r in rs if r[3][0] == "draw")
        cap = sum(1 for r in rows if r[3][0] == "draw" and r[3][2] >= 299.9)
        durs = sorted(r[3][2] for r in rows)
        pc = lambda a, b: f"{100.0 * a / b:.2f}%" if b else "-"
        print(f"| {bank} | {size}v{size} | {MAPNAME[m]} | {len(rows)} | {pc(d(rows), len(rows))} | "
              f"{pc(d(comp), len(comp))} | {pc(d(oth), len(oth))} | {cap} | {durs[len(durs) // 2]:.1f} |")
        cells = collections.Counter()
        tot = collections.Counter()
        for t1, t2, s, res in comp:
            key = "+".join(sorted([t1, t2]))
            tot[key] += 1
            cells[key] += res[0] == "draw"
        worst[(bank, size, m)] = sorted(((cells[k] / tot[k], k, tot[k]) for k in tot), reverse=True)[:5]
    print("\ncompetitive cells (either seat order) that draw most:")
    for key, w in sorted(worst.items()):
        print(f"- {key[0]} {key[1]}v{key[1]} {MAPNAME[key[2]]}: " +
              "; ".join(f"{c} {100 * r:.0f}% of {n}" for r, c, n in w if r > 0))


def cmd_comps(data, size, k):
    groups = collections.defaultdict(list)
    for (bank, m, t1, t2, s), res in data.items():
        if bank.startswith("uni-") and len(t1.split("+")) == size and s >= 801:
            groups[(bank[4:], m)].append((t1, t2, s, res))
    for (tag, m), rows in sorted(groups.items()):
        acc = collections.defaultdict(lambda: [0, 0, collections.defaultdict(lambda: [0, 0])])
        for t1, t2, s, res in rows:
            for seat, team, opp in ((1, t1, t2), (2, t2, t1)):
                # against the competitive field
                no = nheal(opp)
                if (size == 2 and no != 1) or (size == 3 and not 1 <= no <= 2):
                    continue
                x = acc[team]
                x[0] += 1
                x[1] += won(res, seat)
                x[2][block(s, size)][0] += 1
                x[2][block(s, size)][1] += won(res, seat)
        ranked = sorted(acc.items(), key=lambda kv: -kv[1][1] / kv[1][0])
        print(f"\n### {tag} {size}v{size} {MAPNAME[m]}: comp vs the competitive field")
        print("| rank | comp | win | per block | n |")
        print("|---|---|---|---|---|")
        for i, (team, x) in enumerate(ranked):
            if i < k or nheal(team) == 0:
                bl = " / ".join(f"{100.0 * v[1] / v[0]:.1f}" for _, v in sorted(x[2].items()))
                print(f"| {i + 1} | {team} | {100.0 * x[1] / x[0]:.1f} | {bl} | {x[0]} |")


def cmd_h2h(data):
    acc = {}
    n1 = accumulate(acc, pair_sides(data, "h2h-TL", "uni-LL", seat_of=1))
    n2 = accumulate(acc, pair_sides(data, "h2h-LT", "uni-LL", seat_of=2))
    print(f"paired TeamPlan sides: seat 1 {n1}, seat 2 {n2}")
    order = ["all", "seat=1", "seat=2", "2v2", "3v3"]
    for m in MAPS:
        order += [f"map={MAPNAME[m]}", f"2v2 map={MAPNAME[m]}", f"3v3 map={MAPNAME[m]}"]
        order += [f"map={MAPNAME[m]} seat=1", f"map={MAPNAME[m]} seat=2"]
    for sz in (2, 3):
        order += [f"{sz}v{sz} has {c}" for c in CLASSES]
        order += [f"{sz}v{sz} no healer", f"{sz}v{sz} competitive"]
        order += [f"{sz}v{sz} comp has {c}" for c in CLASSES]
    for mm in MAPS:
        order += [f"{MAPNAME[mm]} has {c}" for c in CLASSES]
    report(acc, order)
    print("\nhealer v healer (2v2, one-healer sides):")
    report(acc, only=lambda k, p: " vs " in k and k.split()[0] in HEALERS | {"Basic", "Nagrand", "Twin"}
           and not k.startswith("vs"))


def cmd_play(data, play):
    base = f"play-{play}-NN"
    print(f"## {play}: head-to-head (scored from the side that runs it)")
    acc = {}
    a1 = accumulate(acc, pair_sides(data, f"play-{play}-PN", base, seat_of=1))
    a2 = accumulate(acc, pair_sides(data, f"play-{play}-NP", base, seat_of=2))
    print(f"paired sides: {a1} + {a2}")
    order = ["all", "2v2", "3v3", "seat=1", "seat=2"]
    for m in MAPS:
        order += [f"map={MAPNAME[m]}", f"map={MAPNAME[m]} seat=1", f"map={MAPNAME[m]} seat=2",
                  f"2v2 map={MAPNAME[m]}", f"3v3 map={MAPNAME[m]}"]
    order += [f"2v2 has {c}" for c in CLASSES] + [f"3v3 has {c}" for c in CLASSES]
    order += [f"only DPS {c}" for c in CLASSES] + [f"only chainer {c}" for c in CLASSES]
    for m in MAPS:
        order += [f"{MAPNAME[m]} has {c}" for c in CLASSES]
    report(acc, order)
    print("\nteams with a significant replicated slice, or |z| >= 1.96:")
    report(acc, only=lambda k, p: k.startswith("team") or k.startswith("3v3 team"),
           minimum=1)
    if any(k[0] == f"play-{play}-PNx" for k in data):
        print(f"\n## {play}: exceptions lifted for the running side (arm) against shipped (base)")
        acc2 = {}
        accumulate(acc2, pair_sides(data, f"play-{play}-PNx", f"play-{play}-PN", seat_of=1))
        accumulate(acc2, pair_sides(data, f"play-{play}-NPx", f"play-{play}-NP", seat_of=2))
        report(acc2, ["all", "2v2", "3v3"] + [f"map={MAPNAME[m]}" for m in MAPS])
        print("\nper team (only teams holding an exception move):")
        report(acc2, only=lambda k, p: (k.startswith("team") or k.startswith("3v3 team")) and (p.up + p.down) > 0)
        print("\nper class:")
        report(acc2, [f"2v2 has {c}" for c in CLASSES] + [f"3v3 has {c}" for c in CLASSES])
    print(f"\n## {play}: uniform (both teams run it: TT against NN, every side)")
    acc3 = {}
    accumulate(acc3, pair_sides(data, "uni-TT", base))
    report(acc3, ["2v2", "3v3"] + [f"map={MAPNAME[m]}" for m in MAPS]
           + [f"2v2 has {c}" for c in CLASSES] + [f"3v3 has {c}" for c in CLASSES])


def cmd_focus(data):
    acc = {}
    accumulate(acc, pair_sides(data, "uni-TT", "focus-off"))
    order = ["2v2", "3v3"] + [f"2v2 map={MAPNAME[m]}" for m in MAPS]
    order += [f"2v2 has {c}" for c in CLASSES] + ["2v2 no healer"]
    order += ["2v2 competitive"] + [f"2v2 comp has {c}" for c in CLASSES]
    order += [f"2v2 pillars comp has {c}" for c in CLASSES]
    order += [f"3v3 pillars comp has {c}" for c in CLASSES]
    order += [f"Nagrand has {c}" for c in CLASSES]
    order += [f"Twin has {c}" for c in CLASSES]
    order += [f"3v3 has {c}" for c in CLASSES] + ["3v3 no healer"]
    for t in ("Mage+Hunter", "Mage+Warlock", "Warlock+Hunter"):
        order.append(f"team {t}")
    report(acc, order)


def cmd_pillar_h2h(data):
    acc = {}
    for seat in (1, 2):
        for (bank, m, t1, t2, s), res in data.items():
            if bank != f"pillar-h2h-off-s{seat}":
                continue
            on = data.get(("h2h-TL" if seat == 1 else "h2h-LT", m, t1, t2, s))
            if on is None:
                continue
            team, opp = (t1, t2) if seat == 1 else (t2, t1)
            for k in side_keys(m, team, opp, seat, 3):
                acc.setdefault(k, Paired()).add(won(res, seat), won(on, seat), block(s, 3))
    report(acc, ["all"] + [f"map={MAPNAME[m]}" for m in MAPS[1:]] + [f"3v3 has {c}" for c in CLASSES])


def cmd_as391(on_path, off_path):
    def rd(p):
        out = {}
        with opener(p) as f:
            for r in csv.DictReader(f):
                m, t1, t2, s, seat = r["label"].split(":")
                out[(m, t1, t2, int(s), int(seat[1]))] = r["winner"]
        return out
    on, off = rd(on_path), rd(off_path)
    acc = {}
    for key, w_on in on.items():
        w_off = off.get(key)
        if w_off is None:
            continue
        m, t1, t2, s, seat = key
        team, opp = (t1, t2) if seat == 1 else (t2, t1)
        size = len(t1.split("+"))
        for k in side_keys(m, team, opp, seat, size):
            acc.setdefault(k, Paired()).add(1 if w_off == f"team{seat}" else 0,
                                            1 if w_on == f"team{seat}" else 0, block(s, size))
    report(acc, ["all", "2v2", "3v3", "3v3 map=Nagrand", "3v3 map=Twin", "map=Nagrand", "map=Twin"]
           + [f"3v3 has {c}" for c in CLASSES])


def cmd_legacy07(data, old_path):
    old = {}
    with opener(old_path) as f:
        for r in csv.DictReader(f):
            old[(r["team1"], r["team2"], int(r["seed"]))] = (r["winner"], r["end_reason"], float(r["duration_secs"]))
    acc = {}
    same = diff_dur = diff_win = 0
    for (bank, m, t1, t2, s), res in data.items():
        if bank != "uni-LL" or m != "BasicArena" or s >= 801:
            continue
        o = old.get((t1, t2, s))
        if o is None:
            continue
        if o == res:
            same += 1
        elif o[0] == res[0]:
            diff_dur += 1
        else:
            diff_win += 1
        size = len(t1.split("+"))
        for seat in (1, 2):
            team, opp = (t1, t2) if seat == 1 else (t2, t1)
            for k in side_keys(m, team, opp, seat, size):
                acc.setdefault(k, Paired()).add(won(o, seat), won(res, seat), block(s, size))
    print(f"rows: identical {same}, duration only {diff_dur}, winner changed {diff_win}")
    order = ["2v2", "3v3"]
    for sz in (2, 3):
        order += [f"{sz}v{sz} has {c}" for c in CLASSES]
        order += [f"{sz}v{sz} competitive"] + [f"{sz}v{sz} comp has {c}" for c in CLASSES]
    report(acc, order)


def cmd_pair(data, pairs):
    """Generic: each `arm:base:seat` (seat 1, 2, or 0 for both) pairs the arm
    bank's rows with the base bank's twins; all pairs pool into one table."""
    acc = {}
    for spec in pairs:
        arm, base, seat = spec.split(":")
        accumulate(acc, pair_sides(data, arm, base, seat_of=int(seat) or None))
    report(acc, minimum=1, only=lambda k, p: (p.up + p.down) > 0 or k == "all")


def main(argv):
    cmd = argv[0]
    if cmd == "pair":
        # pair <arm:base:seat>[,<arm:base:seat>...] <csv>...
        cmd_pair(load(argv[2:]), argv[1].split(","))
        return
    if cmd == "as391":
        cmd_as391(argv[1], argv[2])
        return
    if cmd == "legacy07":
        cmd_legacy07(load(argv[2:]), argv[1])
        return
    if cmd == "comps":
        size, k = int(argv[1]), int(argv[2])
        cmd_comps(load(argv[3:]), size, k)
        return
    if cmd == "play":
        cmd_play(load(argv[2:]), argv[1])
        return
    data = load(argv[1:])
    {"standings": cmd_standings, "draws": cmd_draws, "h2h": cmd_h2h, "focus": cmd_focus,
     "pillar-h2h": cmd_pillar_h2h}[cmd](data)


if __name__ == "__main__":
    main(sys.argv[1:])
