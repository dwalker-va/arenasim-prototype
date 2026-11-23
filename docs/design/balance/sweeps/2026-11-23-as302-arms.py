#!/usr/bin/env python3
"""AS-302: the Paladin reference builds and node arms, generated and compared.

Every arm is the same 2v2 directional sweep -- every cell a Paladin change can
reach plus 8 control cells, on all three maps, 4 seeds a cell a map, under
`TeamPlan` -- with every Paladin, on either side, playing that arm's build. The
arms differ ONLY in that build (and, for the Retribution Aura arm, the aura its
node opens), so any two pair at identical (label, seed).

The stat rank is Sacred Duty (+3% maximum health a rank), the Paladin's one
computed `StatPercent` node on a stat the item budget weighs. Every node arm is
"the node" against "Sacred Duty in its place":

- a node the stem lacks: the stem plus the node, against the stem plus a
  Sacred Duty rank;
- a node the stem holds: the stem, against the stem with that node's point on
  a second Sacred Duty rank;
- a capstone: the reference, against the stem plus a Sacred Duty rank.

    # 1. write one JSONL per arm (calls scripts/gen_sweep.py) on a seed block
    2026-11-23-as302-arms.py gen OUTDIR SEED_BASE [ARM ...]
    # 2. run each arm (any binary built from the PR head)
    for f in OUTDIR/*.jsonl; do
      arenasim --batch "$f" --out "${f%.jsonl}.csv" --jobs 5 --trace-mode off
    done
    # 3. the tables in the findings doc; OUTDIR may be several directories
    #    joined by ':' (the seed blocks, pooled)
    2026-11-23-as302-arms.py table OUTDIR
    # 4. every node comparison and stat rank per block, and pooled
    2026-11-23-as302-arms.py replicate OUTDIR1 OUTDIR2
    # 5. non-vacuity: replay K (default 60) Paladin matches of each arm (or of
    #    the arms named) with full logs, and count what each node did
    2026-11-23-as302-arms.py sample OUTDIR <path to arenasim> [K [ARM ...]]

`table` reads `<arm>.csv` and prints, for each comparison, the Paladin side's
paired win-rate delta (CLEAN and AGAINST cells, scored for the side fielding
the Paladin -- `paired_sweep.py --headline affected`), its flips, McNemar z
and the resolution floor that pair bought.
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
from statistics import NormalDist

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPTS = os.path.normpath(os.path.join(HERE, "..", "..", "..", "..", "scripts"))
sys.path.insert(0, SCRIPTS)

CLASS = "Paladin"
MAPS = ("BasicArena", "PillaredArena", "TwinPillars")
SEEDS_PER_MAP = 4
HEALERS = {"Priest", "Paladin", "Shaman", "Druid"}
# Sacred Duty's computed price a rank at the reference Paladin
# (`talent_price_report` in tests/talents.rs), and the declared price of every
# other non-capstone rank the sweep measures (a capstone is two).
YARDSTICK_PRICE = 13.44
RANK_PRICE = 11.0
# The Retribution Aura arm wears the aura its node opens (Devotion otherwise).
RET_AURA = [("paladin_auras", "Retribution")]


def moved(reference, remove=None, add=None):
    """A reference build with ranks moved, as a headless config names it."""
    spec = {"reference": reference}
    if remove:
        spec["remove"] = remove
    if add:
        spec["add"] = add
    return spec


Y = {"SacredDuty": 1}
Y2 = {"SacredDuty": 2}
HOLY_CAP = {"AuraMastery": 1}
PROT_CAP = {"SacredShield": 1}
RET_CAP = {"CrusaderStrike": 1}


def without(cap, node, rank=1):
    """`cap` and `rank` ranks of `node` taken back."""
    out = dict(cap)
    out[node] = rank
    return out


# name -> (build spec or None for no talents, [(field, value)] per-slot choices)
ARMS = {
    "zero": (None, []),
    # Holy 6/2/0; its last point is Aura Mastery.
    "holy": ("Holy", []),
    "holy_stem": (moved("Holy", HOLY_CAP), []),
    "holy_y": (moved("Holy", HOLY_CAP, Y), []),
    "holy_noiol_y2": (moved("Holy", without(HOLY_CAP, "InfusionOfLight"), Y2), []),
    "holy_nodf_y2": (moved("Holy", without(HOLY_CAP, "DivineFavor"), Y2), []),
    "holy_nolg_y2": (moved("Holy", without(HOLY_CAP, "LightsGrace"), Y2), []),
    # Protection 2/6/0; its last point is Sacred Shield.
    "prot": ("Protection", []),
    "prot_stem": (moved("Protection", PROT_CAP), []),
    "prot_y": (moved("Protection", PROT_CAP, Y), []),
    "prot_ra": (moved("Protection", PROT_CAP, {"RetributionAura": 1}), RET_AURA),
    # The same build keeping Devotion Aura: what choosing the aura costs.
    "prot_ra_dev": (moved("Protection", PROT_CAP, {"RetributionAura": 1}), []),
    "prot_nodp_y2": (moved("Protection", without(PROT_CAP, "DivineProtection"), Y2), []),
    "prot_nobos_y2": (moved("Protection", without(PROT_CAP, "BlessingOfSacrifice"), Y2), []),
    # Retribution 0/2/6; its last point is Crusader Strike.
    "ret": ("Retribution", []),
    "ret_stem": (moved("Retribution", RET_CAP), []),
    "ret_y": (moved("Retribution", RET_CAP, Y), []),
    "ret_rep": (moved("Retribution", RET_CAP, {"Repentance": 1}), []),
    "ret_noveng_y2": (moved("Retribution", without(RET_CAP, "Vengeance"), Y2), []),
    "ret_noaow_y2": (moved("Retribution", without(RET_CAP, "TheArtOfWar"), Y2), []),
}

# The success condition's comparisons: (node, its price, arm with the node,
# arm with Sacred Duty in its place, the reference whose stem's Sacred Duty
# rank sets the allowance).
NODE_ARMS = [
    ("Infusion of Light (rank 1)", RANK_PRICE, "holy_y", "holy_noiol_y2", "holy"),
    ("Divine Favor", RANK_PRICE, "holy_y", "holy_nodf_y2", "holy"),
    ("Light's Grace", RANK_PRICE, "holy_y", "holy_nolg_y2", "holy"),
    ("Aura Mastery (capstone)", 2 * RANK_PRICE, "holy", "holy_y", "holy"),
    ("Divine Protection", RANK_PRICE, "prot_y", "prot_nodp_y2", "prot"),
    ("Retribution Aura (rank 1, chosen)", RANK_PRICE, "prot_ra", "prot_y", "prot"),
    ("Blessing of Sacrifice", RANK_PRICE, "prot_y", "prot_nobos_y2", "prot"),
    ("Sacred Shield (capstone)", 2 * RANK_PRICE, "prot", "prot_y", "prot"),
    ("Vengeance (rank 2)", RANK_PRICE, "ret_y", "ret_noveng_y2", "ret"),
    ("The Art of War", RANK_PRICE, "ret_y", "ret_noaow_y2", "ret"),
    ("Repentance", RANK_PRICE, "ret_rep", "ret_y", "ret"),
    ("Crusader Strike (capstone)", 2 * RANK_PRICE, "ret", "ret_y", "ret"),
]
# Two-sided Bonferroni threshold over the node comparisons.
BONFERRONI_Z = NormalDist().inv_cdf(1.0 - 0.05 / (2 * len(NODE_ARMS)))

# What a Sacred Duty rank buys on each stem: (stem name, key, +rank arm, stem).
STAT_RANK = [
    ("Holy stem", "holy", "holy_y", "holy_stem"),
    ("Protection stem", "prot", "prot_y", "prot_stem"),
    ("Retribution stem", "ret", "ret_y", "ret_stem"),
]

# What the Retribution Aura arm is made of: the aura chosen over Devotion on
# one build, and the rank held with Devotion kept, against Sacred Duty.
AURA_PARTS = [
    ("Retribution Aura chosen over Devotion (same build)", "prot_ra", "prot_ra_dev"),
    ("Retribution Aura rank, Devotion kept, vs Sacred Duty", "prot_ra_dev", "prot_y"),
]

REFERENCES = [("Holy 6/2/0", "holy"), ("Protection 2/6/0", "prot"),
              ("Retribution 0/2/6", "ret")]


def gen_args(arm, game_map, seed_base=0):
    spec, choices = ARMS[arm]
    argv = [sys.executable, os.path.join(SCRIPTS, "gen_sweep.py"),
            "--full", "2", "--exclude-double-healer",
            "--affects", CLASS, "--control-cells", "8",
            "--n", str(SEEDS_PER_MAP), "--seed-base", str(seed_base),
            "--extra", json.dumps({"ai_profile": "TeamPlan", "map": game_map}),
            "--label-suffix", game_map]
    if spec is not None:
        argv += ["--build", CLASS + "=" + (spec if isinstance(spec, str) else json.dumps(spec))]
    for field, value in choices:
        argv += ["--for-class", CLASS, field, json.dumps(value)]
    return argv


def gen(outdir, seed_base=0, arms=None):
    """One JSONL per arm on the seed block starting at `seed_base`; `arms`
    limits it."""
    os.makedirs(outdir, exist_ok=True)
    for arm in arms or ARMS:
        with open(os.path.join(outdir, arm + ".jsonl"), "w", encoding="utf-8") as out:
            for game_map in MAPS:
                subprocess.run(gen_args(arm, game_map, seed_base), check=True, stdout=out,
                               stderr=subprocess.DEVNULL)
        print("wrote", arm)


def read_csv(outdirs, arm):
    """An arm's rows, keyed (label, seed), pooled over every directory given
    (seed blocks do not overlap, so neither do their keys)."""
    import paired_sweep as ps
    rows = {}
    for outdir in outdirs.split(os.pathsep):
        rows.update(ps.load(os.path.join(outdir, arm + ".csv"))[0])
    return rows


def scored(cache, outdir, after_arm, before_arm):
    import paired_sweep as ps
    for arm in (after_arm, before_arm):
        if arm not in cache:
            cache[arm] = read_csv(outdir, arm)
    after, before = cache[after_arm], cache[before_arm]
    keys = [k for k in before if k in after]
    return before, after, keys, ps.build_slices(keys, before, {CLASS})


def compare(outdir, cache, after_arm, before_arm, only=None):
    """The Paladin side's paired delta, after_arm against before_arm, over the
    one-sided matches (`only(key, enemy team)` narrows them)."""
    import paired_sweep as ps
    before, after, keys, buckets = scored(cache, outdir, after_arm, before_arm)
    clean = set(buckets["clean"])
    one_sided = buckets["clean"] + buckets["against"]
    if only is not None:
        one_sided = [k for k in one_sided
                     if only(ps.classes(before[k][2] if k in clean else before[k][1]))]
    s = ps.Slice(CLASS.lower(), one_sided, before, after,
                 side=lambda k: "team1" if k in clean else "team2")
    control_same = sum(1 for k in buckets["control"] if before[k] == after[k])
    return s, len(buckets["control"]), control_same


def row(label, s, extra=""):
    floor = "%.1f" % s.floor if s.floor is not None else "-"
    mark = " B" if s.z >= BONFERRONI_Z else (" SIG" if s.significant else "")
    return ("| %s | %d | %.1f%% | %.1f%% | %+.1f | %d (+%d/-%d) | %.2f%s | %s |%s"
            % (label, s.n, 100.0 * s.before_wins / s.n, 100.0 * s.after_wins / s.n,
               s.delta, s.moved, s.to_win, s.to_loss, s.z, mark, floor, extra))


HEADER = ("| %s | n | before | after | delta (pt) | flips | z | floor (pt) |%s\n"
          "|---|---|---|---|---|---|---|---|%s")


def allowance(price, rank_value):
    """What a stat rank of `price` may be worth over a Sacred Duty rank,
    scaling that rank's measured worth on the same stem linearly in price:
    slightly negative for a one-rank node (11 against 13.44), and about
    two-thirds of a rank for a capstone (22)."""
    return rank_value * (price / YARDSTICK_PRICE - 1.0)


def verdict(s, allow):
    """DOMINATES when the node beats Sacred Duty by more than the allowance
    plus the floor; below when it loses to a same-price stat rank by more than
    the floor; otherwise within."""
    if s.floor is None:
        return "unresolvable"
    margin = s.delta - allow
    if margin > s.floor:
        return "DOMINATES"
    if margin < -s.floor:
        return "below"
    return "within"


def stat_values(outdir):
    return {key: compare(outdir, {}, plus, stem)[0].delta
            for _, key, plus, stem in STAT_RANK}


def table(outdir):
    cache = {}
    print("Bonferroni over %d node comparisons: z >= %.2f (marked B)\n"
          % (len(NODE_ARMS), BONFERRONI_Z))
    print("## References against 0 points (Paladin side's win rate)\n")
    print(HEADER % ("reference", "", ""))
    for label, arm in REFERENCES:
        s, _, _ = compare(outdir, cache, arm, "zero")
        print(row(label, s))
    print("\n## One Sacred Duty rank on each stem (7 -> 8 points)\n")
    print(HEADER % ("stem", "", ""))
    stat = {}
    for label, key, plus, stem in STAT_RANK:
        s, _, _ = compare(outdir, cache, plus, stem)
        stat[key] = s.delta
        print(row(label, s))
    print("\n## Each node against Sacred Duty in its place\n")
    print(HEADER % ("node", " allowance | margin | verdict |", "---|---|---|"))
    for label, price, arm, base, stem in NODE_ARMS:
        s, _, _ = compare(outdir, cache, arm, base)
        allow = allowance(price, stat[stem])
        print(row(label, s, " %+.1f | %+.1f | %s |" % (allow, s.delta - allow, verdict(s, allow))))
    print("\n## The Retribution Aura arm, taken apart\n")
    print(HEADER % ("comparison", "", ""))
    for label, arm, base in AURA_PARTS:
        s, _, _ = compare(outdir, cache, arm, base)
        print(row(label, s))
    print("\n## Each node by the enemy team: a healer, or none\n")
    print("| node | vs a healer | vs no healer |\n|---|---|---|")
    for label, _, arm, base, _ in NODE_ARMS:
        cells = []
        for only in (lambda t: bool(t & HEALERS), lambda t: not t & HEALERS):
            s, _, _ = compare(outdir, cache, arm, base, only)
            cells.append("%+.1f (%.2f) n=%d" % (s.delta, s.z, s.n))
        print("| %s | %s |" % (label, " | ".join(cells)))
    print("\n## Each arm against its reference\n")
    print(HEADER % ("arm", "", ""))
    refs = {"holy", "prot", "ret"}
    for arm in ARMS:
        if arm == "zero" or arm in refs:
            continue
        ref = arm.split("_")[0]
        s, _, _ = compare(outdir, cache, arm, ref)
        print(row("%s vs %s" % (arm, ref), s))
    print("\n## Control and non-vacuity\n")
    import paired_sweep as ps
    before, _, keys, buckets = scored(cache, outdir, "holy", "zero")
    for side, (owed, fielded) in zip((1, 2), ps.control_coverage(
            buckets["control"], keys, before, {CLASS})):
        print("- control team%d fields %s; owes %s; blind spots: %s"
              % (side, ",".join(sorted(fielded)), ",".join(sorted(owed)),
                 ",".join(sorted(owed - fielded)) or "none"))
    for arm in ARMS:
        if arm == "zero":
            continue
        _, nc, same = compare(outdir, cache, arm, "zero")
        print("- %s: control %d/%d identical to `zero`; %d decisive of %d"
              % (arm, same, nc,
                 sum(1 for v in cache[arm].values() if v[0] in ("team1", "team2")),
                 len(cache[arm])))
    print("\nmatches per arm: %d" % len(cache["zero"]))


def replicate(block1, block2):
    """Every node comparison and stat rank, per block and pooled; a node row's
    verdict reads its allowance from the Sacred Duty rank on the same stem in
    the same block(s)."""
    pooled = block1 + os.pathsep + block2
    blocks = (("seeds 0-3", block1), ("seeds 4-7", block2), ("pooled", pooled))
    stat = {name: stat_values(dirs) for name, dirs in blocks}
    print(HEADER % ("comparison", " block | allowance | margin | verdict |", "---|---|---|---|"))
    for label, price, arm, base, stem in NODE_ARMS:
        for name, dirs in blocks:
            s, _, _ = compare(dirs, {}, arm, base)
            allow = allowance(price, stat[name][stem])
            print(row(label, s, " %s | %+.1f | %+.1f | %s |"
                      % (name, allow, s.delta - allow, verdict(s, allow))))
    for label, _, plus, stem in STAT_RANK:
        for name, dirs in blocks:
            s, _, _ = compare(dirs, {}, plus, stem)
            print(row("Sacred Duty rank, " + label, s, " %s | | | |" % name))


# Non-vacuity: what each node does in a match, counted off full match logs.
# The batch CSV carries outcomes only, so `sample` replays a fixed slice of
# each arm's Paladin matches through `--headless` and counts these lines.
P = r"Paladin #\d"
EVENTS = [
    ("Paladins", r"\[EQUIPMENT\] Team \d Paladin"),
    ("Holy Shock casts", P + r" casts Holy Shock"),
    ("Flash of Light heals", P + r"'s Flash of Light (heals|CRITICALLY heals)"),
    ("Holy Light heals", P + r"'s Holy Light (heals|CRITICALLY heals)"),
    ("Paladin melee swings landed", P + r"'s Auto Attack (hits|CRITS)"),
    ("Infusion of Light procs", P + r"'s Infusion of Light procs"),
    ("Infusion of Light spent", P + r"'s Infusion of Light is spent on"),
    ("Light's Grace procs", P + r"'s Light's Grace procs"),
    ("Light's Grace spent", P + r"'s Light's Grace is spent on"),
    ("Divine Favor casts", P + r" casts Divine Favor"),
    ("Divine Favor spent", P + r"'s Divine Favor is spent on"),
    ("Aura Mastery casts", P + r" casts Aura Mastery"),
    ("Divine Protection casts", P + r" casts Divine Protection"),
    ("Blessing of Sacrifice casts", P + r" casts Blessing of Sacrifice"),
    ("Sacrifice transfers", r"\[TRANSFER\] Team \d Paladin #\d takes \d+ of .*\(Blessing of Sacrifice\)"),
    ("Sacred Shield casts", P + r" casts Sacred Shield"),
    ("Sacred Shield shields", r"gains Sacred Shield \(absorbs"),
    ("Retribution Aura hits", r"Holy damage from .*'s Retribution Aura"),
    ("Vengeance procs", P + r"'s Vengeance procs"),
    ("Art of War procs", P + r"'s The Art of War procs"),
    ("Art of War spent", P + r"'s The Art of War is spent on"),
    ("Repentance casts", P + r" casts Repentance"),
    ("Crusader Strikes", P + r" uses Crusader Strike"),
]
SAMPLE_PER_ARM = 60


def paladin_label(line):
    return CLASS in line.split('"label"')[1].split(",")[0]


def sample_configs(path, k=SAMPLE_PER_ARM):
    """Every n-th config of an arm that fields a Paladin: a fixed slice, the
    same configs (up to the build) in every arm."""
    with open(path, encoding="utf-8") as f:
        lines = [ln for ln in f if paladin_label(ln)]
    step = max(1, len(lines) // k)
    return lines[::step][:k]


def run_one(binary, config_line, workdir, index):
    cfg = os.path.join(workdir, "%d.json" % index)
    log = os.path.join(workdir, "%d.txt" % index)
    with open(cfg, "w", encoding="utf-8") as f:
        f.write(config_line)
    subprocess.run([binary, "--headless", cfg, "--output", log], check=True,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    with open(log, encoding="utf-8") as f:
        text = f.read()
    os.remove(cfg)
    os.remove(log)
    return [len(re.findall(pattern, text)) for _, pattern in EVENTS]


def sample(outdir, binary, k=SAMPLE_PER_ARM, arms=None, jobs=4):
    from concurrent.futures import ThreadPoolExecutor
    print("| arm | " + " | ".join(name for name, _ in EVENTS) + " |")
    print("|---|" + "---|" * len(EVENTS))
    for arm in arms or ARMS:
        work = os.path.join(outdir, ".sample-" + arm)
        os.makedirs(work, exist_ok=True)
        configs = sample_configs(os.path.join(outdir, arm + ".jsonl"), k)
        with ThreadPoolExecutor(max_workers=jobs) as pool:
            counts = list(pool.map(lambda a: run_one(binary, a[1], work, a[0]),
                                   enumerate(configs)))
        os.rmdir(work)
        totals = [sum(c[i] for c in counts) for i in range(len(EVENTS))]
        print("| %s (%d) | %s |" % (arm, len(configs), " | ".join(str(t) for t in totals)))


def main(argv=None):
    argv = sys.argv[1:] if argv is None else argv
    if len(argv) >= 3 and argv[0] == "sample":
        k = int(argv[3]) if len(argv) >= 4 else SAMPLE_PER_ARM
        sample(argv[1], argv[2], k, argv[4:] or None)
        return 0
    if len(argv) == 3 and argv[0] == "replicate":
        replicate(argv[1], argv[2])
        return 0
    if len(argv) >= 3 and argv[0] == "gen":
        gen(argv[1], int(argv[2]), argv[3:] or None)
        return 0
    if len(argv) == 2 and argv[0] == "table":
        table(argv[1])
        return 0
    sys.exit(__doc__)


if __name__ == "__main__":
    sys.exit(main())
