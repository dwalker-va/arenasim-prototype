#!/usr/bin/env python3
"""AS-301: the Rogue reference builds and node arms, generated and compared,
and Shadow Dance's stun-seconds and kill conversion.

Every arm is the same 2v2 directional sweep -- every cell a Rogue change can
reach plus 8 control cells, on all three maps, 4 seeds a cell a map, under
`TeamPlan` -- with every Rogue, on either side, playing that arm's build. The
arms differ ONLY in that build (and the off-hand poison its Master Poisoner
opens), so any two pair at identical (label, seed).

The comparison is Vitality (+5% maximum health), the Rogue's one computed
`StatPercent` node (20.4 at the reference Rogue: 1.85 ranks). Every node arm
is "the node" against "Vitality in its place": a stem (a reference without
its capstone) plus the node against the stem plus Vitality, or, for a node
the stem holds, the stem against the stem with that node's point on
Vitality.

    # 1. write one JSONL per arm (calls scripts/gen_sweep.py); optionally on
    #    another seed block, for only some arms (the replication)
    2026-11-22-as301-arms.py gen OUTDIR [SEED_BASE [ARM ...]]
    # 2. run each arm (any binary built from the PR head)
    for f in OUTDIR/*.jsonl; do
      arenasim --batch "$f" --out "${f%.jsonl}.csv" --jobs 4 --trace-mode off
    done
    # 3. the tables in the findings doc
    2026-11-22-as301-arms.py table OUTDIR
    # 4. non-vacuity: replay 60 Rogue matches of each arm with full logs,
    #    and count what each node did (or K matches of the arms named)
    2026-11-22-as301-arms.py sample OUTDIR <path to arenasim> [K [ARM ...]]
    # 5. the replication on a second seed block (seeds 4-7), per block and
    #    pooled: `gen OUTDIR2 4 <REPLICATED_ARMS>`, run as in 2, then
    2026-11-22-as301-arms.py replicate OUTDIR OUTDIR2
    # 6. Shadow Dance: replay every Rogue match of the Subtlety reference
    #    with full logs, and the Vitality twin of every match with a Dance
    #    (the counterfactual)
    2026-11-22-as301-arms.py dance OUTDIR <path to arenasim>
    # 7. why the Dance did not go: its rejection reasons over the sample
    #    slice of the Subtlety reference, traced. The gates are checked in
    #    order, so each share is of the decisions that reached that gate
    2026-11-22-as301-arms.py gates OUTDIR <path to arenasim>

`table` reads `<arm>.csv` from OUTDIR and prints, for each comparison, the
Rogue side's paired win-rate delta (CLEAN and AGAINST cells, scored for the
side fielding the Rogue -- `paired_sweep.py --headline affected`), its flips,
McNemar z and the resolution floor that pair bought.
"""

from __future__ import annotations

import json
import math
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPTS = os.path.normpath(os.path.join(HERE, "..", "..", "..", "..", "scripts"))
sys.path.insert(0, SCRIPTS)

MAPS = ("BasicArena", "PillaredArena", "TwinPillars")
SEEDS_PER_MAP = 4
# Master Poisoner opens an off-hand poison slot; an arm holding it wears
# Deadly Poison there (Crippling, the default, stays on the main hand).
OFFHAND = [("rogue_offhand_poisons", "Deadly")]
VITALITY_PRICE = 20.4  # its computed price at the reference Rogue
RANK_PRICE = 11.0
# Bonferroni at alpha 0.05 two-sided over the 12 rows of NODE_ARMS (10 nodes
# under test, Blood Spatter shown for scale, and Deflection, which plays the
# same matches as Lightning Reflexes): Phi^-1(1 - 0.025/12) = 2.865. Applied to
# each row's MARGIN z, the test of the success condition's null ("worth a stat
# rank of its price"), never to the raw delta's.
BONFERRONI_Z = 2.87
HEALERS = {"Priest", "Paladin", "Shaman", "Druid"}


def moved(reference, remove=None, add=None):
    """A reference build with ranks moved, as a headless config names it."""
    spec = {"reference": reference}
    if remove:
        spec["remove"] = remove
    if add:
        spec["add"] = add
    return spec


VIT = {"Vitality": 1}
# name -> (build spec or None for no talents, [(field, value)] per-slot choices)
ARMS = {
    "zero": (None, []),
    # Assassination 6/2/0; its last point is Blood Spatter.
    "assn": ("Assassination", OFFHAND),
    "assn_stem": (moved("Assassination", {"BloodSpatter": 1}), OFFHAND),
    "assn_vit": (moved("Assassination", {"BloodSpatter": 1}, VIT), OFFHAND),
    "assn_lr": (moved("Assassination", {"BloodSpatter": 1}, {"LightningReflexes": 1}), OFFHAND),
    "assn_defl": (moved("Assassination", {"BloodSpatter": 1}, {"Deflection": 1}), OFFHAND),
    "assn_mos": (moved("Assassination", {"BloodSpatter": 1}, {"MasterOfSubtlety": 1}), OFFHAND),
    # The stem with Master Poisoner's point on Vitality: one poison.
    "assn_nomp_vit": (moved("Assassination", {"BloodSpatter": 1, "MasterPoisoner": 1}, VIT), []),
    # The stem with Rupture's point on Vitality.
    "assn_norup_vit": (moved("Assassination", {"BloodSpatter": 1, "Rupture": 1}, VIT), OFFHAND),
    # Combat 2/6/0; its last point is Expose Armor.
    "combat": ("Combat", []),
    "combat_stem": (moved("Combat", {"ExposeArmor": 1}), []),
    "combat_vit": (moved("Combat", {"ExposeArmor": 1}, VIT), []),
    # The stem with Swift Blades' point on Vitality.
    "combat_nosb_vit": (moved("Combat", {"ExposeArmor": 1, "SwiftBlades": 1}, VIT), []),
    # Subtlety 0/2/6; its last point is Shadow Dance.
    "sub": ("Subtlety", []),
    "sub_stem": (moved("Subtlety", {"ShadowDance": 1}), []),
    "sub_vit": (moved("Subtlety", {"ShadowDance": 1}, VIT), []),
    "sub_cd": (moved("Subtlety", {"ShadowDance": 1}, {"CheatDeath": 1}), []),
    # The stem with Initiative's second rank on Vitality.
    "sub_noinit_vit": (moved("Subtlety", {"ShadowDance": 1, "Initiative": 1}, VIT), []),
    # The stem with Hemorrhage's point on Vitality.
    "sub_nohemo_vit": (moved("Subtlety", {"ShadowDance": 1, "Hemorrhage": 1}, VIT), []),
}

# The success condition's comparisons: (node, its price, arm with the node,
# arm with Vitality in its place, the stem whose Vitality rank sets the
# allowance).
NODE_ARMS = [
    ("Lightning Reflexes (rank 1)", RANK_PRICE, "assn_lr", "assn_vit", "assn"),
    ("Deflection (rank 1)", RANK_PRICE, "assn_defl", "assn_vit", "assn"),
    ("Master of Subtlety (rank 1)", RANK_PRICE, "assn_mos", "assn_vit", "assn"),
    ("Master Poisoner (Deadly off hand)", RANK_PRICE, "assn_stem", "assn_nomp_vit", "assn"),
    ("Rupture", RANK_PRICE, "assn_stem", "assn_norup_vit", "assn"),
    ("Swift Blades", RANK_PRICE, "combat_stem", "combat_nosb_vit", "combat"),
    ("Initiative (rank 2)", RANK_PRICE, "sub_stem", "sub_noinit_vit", "sub"),
    ("Hemorrhage", RANK_PRICE, "sub_stem", "sub_nohemo_vit", "sub"),
    ("Cheat Death", RANK_PRICE, "sub_cd", "sub_vit", "sub"),
    ("Shadow Dance (capstone)", 2 * RANK_PRICE, "sub", "sub_vit", "sub"),
    ("Expose Armor (capstone)", 2 * RANK_PRICE, "combat", "combat_vit", "combat"),
    ("Blood Spatter (capstone; a Patch, for scale)", 2 * RANK_PRICE, "assn", "assn_vit", "assn"),
]

# What Vitality buys on each stem: (stem name, key, +Vitality arm, stem).
STAT_RANK = [
    ("Assassination stem", "assn", "assn_vit", "assn_stem"),
    ("Combat stem", "combat", "combat_vit", "combat_stem"),
    ("Subtlety stem", "sub", "sub_vit", "sub_stem"),
]

REFERENCES = [("Assassination 6/2/0", "assn"), ("Combat 2/6/0", "combat"),
              ("Subtlety 0/2/6", "sub")]


def gen_args(arm, game_map, seed_base=0):
    spec, choices = ARMS[arm]
    argv = [sys.executable, os.path.join(SCRIPTS, "gen_sweep.py"),
            "--full", "2", "--exclude-double-healer",
            "--affects", "Rogue", "--control-cells", "8",
            "--n", str(SEEDS_PER_MAP), "--seed-base", str(seed_base),
            "--extra", json.dumps({"ai_profile": "TeamPlan", "map": game_map}),
            "--label-suffix", game_map]
    if spec is not None:
        argv += ["--build", "Rogue=" + (spec if isinstance(spec, str) else json.dumps(spec))]
    for field, value in choices:
        argv += ["--for-class", "Rogue", field, json.dumps(value)]
    return argv


def gen(outdir, seed_base=0, arms=None):
    """One JSONL per arm. `seed_base` moves the whole sweep onto another seed
    block (the replication); `arms` limits it."""
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


def compare(outdir, cache, after_arm, before_arm):
    """The Rogue side's paired delta, after_arm against before_arm."""
    import paired_sweep as ps
    for arm in (after_arm, before_arm):
        if arm not in cache:
            cache[arm] = read_csv(outdir, arm)
    after, before = cache[after_arm], cache[before_arm]
    keys = [k for k in before if k in after]
    buckets = ps.build_slices(keys, before, {"Rogue"})
    clean = set(buckets["clean"])
    s = ps.Slice("rogue", buckets["clean"] + buckets["against"], before, after,
                 side=lambda k: "team1" if k in clean else "team2")
    control_same = sum(1 for k in buckets["control"] if before[k] == after[k])
    return s, len(buckets["control"]), control_same


def row(label, s, extra=""):
    floor = "%.1f" % s.floor if s.floor is not None else "-"
    return ("| %s | %d | %.1f%% | %.1f%% | %+.1f | %d (+%d/-%d) | %.2f%s | %s |%s"
            % (label, s.n, 100.0 * s.before_wins / s.n, 100.0 * s.after_wins / s.n,
               s.delta, s.moved, s.to_win, s.to_loss, s.z,
               " SIG" if s.significant else "", floor, extra))


HEADER = ("| %s | n | before | after | delta (pt) | flips | z | floor (pt) |%s\n"
          "|---|---|---|---|---|---|---|---|%s")


def per_match_margin(after, before, vplus, v0, k):
    """(margin, margin z) from per-match outcomes: four equal-length lists of
    1 (the Rogue side won) or 0, one entry per paired match, for the node's
    arm, its comparison, the stem + Vitality and the stem. Each match gives
    x = 100 * [(after - before) - k * (vplus - v0)]; the margin is mean(x)
    and its standard error sd(x) / sqrt(n).

    The arms play the same matches, and a node row and its Vitality row share
    an arm or a stem, so their two deltas are correlated: only the per-match
    difference carries that covariance (user ruling, AS-301 r3, for every
    talent sweep)."""
    x = [100.0 * ((a - b) - k * (p - z)) for a, b, p, z in zip(after, before, vplus, v0)]
    n = len(x)
    if n < 2:
        return (x[0] if x else 0.0), 0.0
    mean = sum(x) / n
    sd = math.sqrt(sum((xi - mean) ** 2 for xi in x) / (n - 1))
    se = sd / math.sqrt(n)
    return mean, (mean / se if se else 0.0)


VITALITY_ARMS = {key: (plus, stem) for _, key, plus, stem in STAT_RANK}


def node_stats(dirs, cache, arm, base, price, stem_key):
    """One node row measured over `dirs`: (its slice, Vitality's slice on its
    stem, allowance, margin, margin z). Everything, the allowance included,
    is read from the same directories, so a block's row uses that block's
    own Vitality rank."""
    import paired_sweep as ps
    plus, stem = VITALITY_ARMS[stem_key]
    s, _, _ = compare(dirs, cache, arm, base)
    vit, _, _ = compare(dirs, cache, plus, stem)
    k = price / VITALITY_PRICE - 1.0
    allow = k * vit.delta
    rows = [cache[a] for a in (arm, base, plus, stem)]
    keys = sorted(set(rows[1]).intersection(*rows))
    buckets = ps.build_slices(keys, rows[1], {"Rogue"})
    clean = set(buckets["clean"])
    scored = buckets["clean"] + buckets["against"]

    def wins(r):
        return [1 if r[key][0] == ("team1" if key in clean else "team2") else 0
                for key in scored]
    margin, mz = per_match_margin(*(wins(r) for r in rows), k)
    return s, vit, allow, margin, mz


def node_cells(s, allow, margin, mz):
    """The node columns a table prints: allowance, margin, margin z (with
    BONF when it clears the Bonferroni threshold, in either direction),
    verdict."""
    return " %+.1f | %+.1f | %+.2f%s | %s |" % (
        allow, margin, mz, " BONF" if abs(mz) >= BONFERRONI_Z else "", verdict(s, allow))


NODE_HEADER = " allowance | margin | margin z | verdict |"
NODE_RULE = "---|---|---|---|"


def changes_at_allowance_zero(rows):
    """The node rows whose verdict changes when the allowance is dropped, so
    the node faces a full Vitality: [(label, verdict, verdict at 0)] from
    [(label, slice, allowance)]."""
    out = []
    for label, s, allow in rows:
        modelled = verdict(s, allow)
        flat = verdict(s, 0.0)
        if flat != modelled:
            out.append((label, modelled, flat))
    return out


def verdict(s, allow):
    """DOMINATES when the node beats Vitality by more than the allowance plus
    the floor; BELOW when it loses to a same-price stat rank by more than the
    floor; otherwise within."""
    if s.floor is None:
        return "unresolvable"
    margin = s.delta - allow
    if margin > s.floor:
        return "DOMINATES"
    if margin < -s.floor:
        return "below"
    return "within"


def table(outdir):
    cache = {}
    print("## References against 0 points (Rogue side's win rate)\n")
    print(HEADER % ("reference", "", ""))
    for label, arm in REFERENCES:
        s, nc, same = compare(outdir, cache, arm, "zero")
        print(row(label, s))
    print("\n## Vitality on each stem (7 -> 8 points)\n")
    print(HEADER % ("stem", "", ""))
    for label, key, plus, stem in STAT_RANK:
        s, _, _ = compare(outdir, cache, plus, stem)
        print(row(label, s))
    print("\n## Each node against Vitality in its place\n")
    print("BONF: |margin z| >= %.2f, Bonferroni over the %d rows.\n"
          % (BONFERRONI_Z, len(NODE_ARMS)))
    print(HEADER % ("node", NODE_HEADER, NODE_RULE))
    measured = []
    for label, price, arm, base, stem in NODE_ARMS:
        s, _, allow, margin, mz = node_stats(outdir, cache, arm, base, price, stem)
        print(row(label, s, node_cells(s, allow, margin, mz)))
        measured.append((label, s, allow))
    changed = changes_at_allowance_zero(measured)
    print("\n## Verdicts that change at allowance 0 (the node against a full Vitality)\n")
    for label, modelled, flat in changed:
        print("- %s: %s -> %s" % (label, modelled, flat))
    if not changed:
        print("- (none)")
    print("\n## Each arm against its reference\n")
    print(HEADER % ("arm", "", ""))
    refs = {"assn", "combat", "sub"}
    for arm in ARMS:
        if arm == "zero" or arm in refs:
            continue
        ref = arm.split("_")[0]
        s, _, _ = compare(outdir, cache, arm, ref)
        print(row("%s vs %s" % (arm, ref), s))
    print("\n## Control and non-vacuity\n")
    n_total = 0
    for arm in ARMS:
        if arm == "zero":
            continue
        s, nc, same = compare(outdir, cache, arm, "zero")
        n_total = len(cache[arm])
        print("- %s: control %d/%d identical to `zero`; %d decisive of %d"
              % (arm, same, nc,
                 sum(1 for v in cache[arm].values() if v[0] in ("team1", "team2")),
                 len(cache[arm])))
    print("\nmatches per arm: %d" % n_total)


# Non-vacuity: what each node does in a match, counted off full match logs.
# The batch CSV carries outcomes only, so `sample` replays a fixed slice of
# each arm's Rogue matches through `--headless` and counts these lines.
EVENTS = [
    ("Rogues", r"\[EQUIPMENT\] Team \d Rogue"),
    ("Crippling applied", r"\[CC\] Crippling Poison applied to"),
    ("Deadly procs", r"\[CC\] Deadly Poison procs on"),
    ("Rupture casts", r"Rogue #\d uses Rupture on"),
    ("Hemorrhage casts", r"Rogue #\d uses Hemorrhage on"),
    ("Expose Armor casts", r"Rogue #\d uses Expose Armor on"),
    ("Rogue dodges", r"\[AVOID\] Team \d Rogue #\d dodges"),
    ("Rogue parries", r"\[AVOID\] Team \d Rogue #\d parries"),
    ("Master of Subtlety procs", r"\[PROC\] .*Rogue.* procs Master of Subtlety"),
    ("Initiative procs", r"\[PROC\] .*Rogue.* procs Initiative"),
    ("Initiative spent", r"Rogue #\d's Initiative is spent on"),
    ("Cheat Death saves", r"\[SAVED\] Cheat Death prevents a killing blow on Team \d Rogue #\d"),
    ("of them killed in the same frame",
     r"\[SAVED\] Cheat Death prevents a killing blow on Team \d Rogue #\d, who is killed"),
    ("Shadow Dances", r"Rogue #\d uses Shadow Dance"),
    ("Rogue stuns", r"Rogue #\d uses (Cheap Shot|Kidney Shot) on"),
    ("crowd control broken by damage", r"broke from damage"),
    # Melee strikes that land on a Rogue (an auto-attack is a melee weapon's
    # or a pet's; Auto Shot and Wand Shot are named apart). With the
    # avoidances above, the melee attacks a Rogue's dodge or parry rolls on.
    ("melee hits on a Rogue",
     r"'s (Auto Attack|Mortal Strike|Sinister Strike|Hemorrhage|Ambush|Crusader Strike"
     r"|Heroic Strike) (hits|CRITS) Team \d Rogue #\d"),
]
SAMPLE_PER_ARM = 60


def rogue_label(line):
    return "Rogue" in line.split('"label"')[1].split(",")[0]


def sample_configs(path, k=SAMPLE_PER_ARM):
    """Every n-th config of an arm that fields a Rogue: a fixed slice, the
    same configs (up to the build) in every arm."""
    with open(path, encoding="utf-8") as f:
        lines = [ln for ln in f if rogue_label(ln)]
    step = max(1, len(lines) // k)
    return lines[::step][:k]


def run_log(binary, config_line, workdir, index):
    """The full match log of one config line."""
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
    return text


def run_one(binary, config_line, workdir, index):
    text = run_log(binary, config_line, workdir, index)
    return [len(re.findall(pattern, text)) for _, pattern in EVENTS]


def replay(outdir, arm, binary, configs, fn, jobs=4):
    from concurrent.futures import ThreadPoolExecutor
    work = os.path.join(outdir, ".replay-" + arm)
    os.makedirs(work, exist_ok=True)
    with ThreadPoolExecutor(max_workers=jobs) as pool:
        out = list(pool.map(lambda a: fn(binary, a[1], work, a[0]), enumerate(configs)))
    os.rmdir(work)
    return out


def sample(outdir, binary, k=SAMPLE_PER_ARM, arms=None):
    print("| arm | " + " | ".join(name for name, _ in EVENTS) + " |")
    print("|---|" + "---|" * len(EVENTS))
    for arm in arms or ARMS:
        configs = sample_configs(os.path.join(outdir, arm + ".jsonl"), k)
        counts = replay(outdir, arm, binary, configs, run_one)
        totals = [sum(c[i] for c in counts) for i in range(len(EVENTS))]
        print("| %s (%d) | %s |" % (arm, len(configs), " | ".join(str(t) for t in totals)))


# The comparisons a second seed block (seeds 4-7) replicates: the three nodes
# the first block put over the line, Shadow Dance, the two significant
# "below" capstone and grant rows that block 2's arms reach, and the Vitality
# ranks their allowances read. (`gen OUTDIR2 4 <the arms below>`.)
REPLICATED = [
    ("Cheat Death", "sub_cd", "sub_vit"),
    ("Swift Blades", "combat_stem", "combat_nosb_vit"),
    ("Hemorrhage", "sub_stem", "sub_nohemo_vit"),
    ("Shadow Dance (capstone)", "sub", "sub_vit"),
    ("Expose Armor (capstone)", "combat", "combat_vit"),
    ("Rupture", "assn_stem", "assn_norup_vit"),
    ("Vitality, Subtlety stem", "sub_vit", "sub_stem"),
    ("Vitality, Combat stem", "combat_vit", "combat_stem"),
    ("Vitality, Assassination stem", "assn_vit", "assn_stem"),
]
REPLICATED_ARMS = sorted({a for _, x, y in REPLICATED for a in (x, y)})


def replicate_rows(block1, block2, pairs=None):
    """Each replicated comparison per block and pooled, as data: (label,
    block name, slice, node stats or None). A node row's stats are
    (allowance, margin, margin z) read from that block's own directories."""
    pooled = block1 + os.pathsep + block2
    blocks = (("block 1", block1), ("block 2", block2), ("pooled", pooled))
    node = {(arm, base): (price, stem) for _, price, arm, base, stem in NODE_ARMS}
    out = []
    for label, arm, base in pairs or REPLICATED:
        for name, dirs in blocks:
            cache = {}
            if (arm, base) in node:
                price, stem = node[(arm, base)]
                s, _, allow, margin, mz = node_stats(dirs, cache, arm, base, price, stem)
                out.append((label, name, s, (allow, margin, mz)))
            else:
                s, _, _ = compare(dirs, cache, arm, base)
                out.append((label, name, s, None))
    return out


def replicate(block1, block2):
    """Print `replicate_rows`: each node row with its margin z against the
    Bonferroni threshold."""
    print(HEADER % ("comparison", " block |" + NODE_HEADER, "---|" + NODE_RULE))
    for label, name, s, stats in replicate_rows(block1, block2):
        extra = " %s |" % name
        extra += node_cells(s, *stats) if stats else " | | | |"
        print(row(label, s, extra))


# ---------------------------------------------------------------------------
# Shadow Dance
# ---------------------------------------------------------------------------

T = r"\[\s*([0-9.]+)s\]"
DANCE = re.compile(T + r" \[CAST\] (Team (\d) Rogue #\d) uses Shadow Dance$")
STUN_USE = re.compile(T + r" \[CC\] (Team (\d) Rogue #\d) uses (Cheap Shot|Kidney Shot) on (.+)$")
STUN_LAND = re.compile(T + r" \[CC\] (Cheap Shot|Kidney Shot) on (.+) \(([0-9.]+)s, DR: (\d+)%\)$")
STUN_IMMUNE = re.compile(T + r" \[CC\] (Cheap Shot|Kidney Shot) IMMUNE on (.+) \(DR immune\)$")
DEATH = re.compile(T + r" \[DEATH\] (Team (\d) .+) has been eliminated$")
DURATION = re.compile(r"^Duration: ([0-9.]+)s", re.M)
DANCE_SECS = 8.0
# The chain a Dance starts: every stun its Rogue begins inside the Dance, and
# the Kidney Shot that the Rogue's planner hands off to as the Dance's last
# Cheap Shot ends, which may begin up to this long after the Dance's 8s.
HANDOFF_GRACE = 1.0
# A Dance converts when the Rogue's team kills an enemy (pets are not team
# members, so never count) between the Dance's cast and this follow-through
# past the LATER of the Dance's own end and its chain's last stun's end: one
# global cooldown and a half, the time a burst on a target coming out of a
# stun still lands.
CONVERT_GRACE = 2.0


def stun_events(text):
    """Every stun a Rogue began, in order: (time, rogue, rogue team, ability,
    target, applied seconds or None when DR-immune, DR percent or None)."""
    lines = text.splitlines()
    out = []
    for i, line in enumerate(lines):
        m = STUN_USE.match(line)
        if not m:
            continue
        t, rogue, team, ability, target = float(m.group(1)), m.group(2), m.group(3), m.group(4), m.group(5)
        applied, dr = None, None
        # The landing line follows within a few lines (the aura lands the
        # next tick); take the first for this ability and target.
        for later in lines[i + 1:i + 12]:
            land = STUN_LAND.match(later)
            if land and land.group(2) == ability and land.group(3) == target:
                applied, dr = float(land.group(4)), int(land.group(5))
                break
            immune = STUN_IMMUNE.match(later)
            if immune and immune.group(2) == ability and immune.group(3) == target:
                applied, dr = 0.0, 0
                break
        out.append((t, rogue, team, ability, target, applied, dr))
    return out


CLASSES = ("Warrior", "Mage", "Rogue", "Priest", "Warlock", "Paladin", "Hunter", "Shaman",
           "Druid")


def member_deaths(text):
    """Every team member's death, in order: (time, name, team). A pet's death
    ("Team 2 Felhunter #1") is not a team member's and is left out."""
    out = []
    for m in map(DEATH.match, text.splitlines()):
        if m and m.group(2).split()[2] in CLASSES:
            out.append((float(m.group(1)), m.group(2), m.group(3)))
    return out


# Why `enemy_healer_answers` (class_ai/rogue.rs) let a Dance go, read off the
# match: it returns false -- admitting the Dance -- when the target is the
# team's kill window, or when no living enemy healer other than the target
# stands outside the one the team holds locked. 2v2 with double healers
# excluded fields at most one healer a team, so each Dance is one of these.
ADMISSIONS = (
    "the target is the enemy healer",
    "no healer in the enemy team",
    "the enemy healer is dead",
    "a healer stands: its lock or the kill window",
    "no stun landed (target unknown)",
)


def admission(config, team, victim, t0, deaths):
    """Which branch of the gate admitted a Dance by a Rogue of `team` whose
    chain's first victim is `victim` (None when no stun landed)."""
    if victim is None:
        return ADMISSIONS[4]
    if victim.split()[2] in HEALERS:
        return ADMISSIONS[0]
    enemy = "2" if team == "1" else "1"
    healers = ["Team %s %s #%d" % (enemy, cls, slot + 1)
               for slot, cls in enumerate(config["team" + enemy]) if cls in HEALERS]
    if not healers:
        return ADMISSIONS[1]
    dead = {who for when, who, _ in deaths if when <= t0}
    if all(h in dead for h in healers):
        return ADMISSIONS[2]
    return ADMISSIONS[3]


def analyse_dances(text, config):
    """One record per Dance: its chain, stun-seconds, conversion and the gate
    branch that admitted it."""
    deaths = member_deaths(text)
    end = DURATION.search(text)
    match_end = float(end.group(1)) if end else None  # the log's own clock
    stuns = stun_events(text)
    records = []
    for line in text.splitlines():
        m = DANCE.match(line)
        if not m:
            continue
        t0, rogue, team = float(m.group(1)), m.group(2), m.group(3)
        chain = [s for s in stuns if s[1] == rogue and t0 <= s[0] <= t0 + DANCE_SECS + HANDOFF_GRACE
                 and (s[0] <= t0 + DANCE_SECS or s[3] == "Kidney Shot")]
        applied = sum(s[5] or 0.0 for s in chain)
        # Held: each stun cut at its victim's death (or the match's end).
        held = 0.0
        last_end = t0 + DANCE_SECS
        for s in chain:
            if not s[5]:
                continue
            stop = s[0] + s[5]
            for dt, who, _ in deaths:
                if who == s[4] and s[0] <= dt < stop:
                    stop = dt
            if match_end is not None:
                stop = min(stop, match_end)
            held += max(0.0, stop - s[0])
            last_end = max(last_end, s[0] + s[5])
        window_end = last_end + CONVERT_GRACE
        enemy = "2" if team == "1" else "1"
        kills = [d for d in deaths if d[2] == enemy and t0 <= d[0] <= window_end]
        victims = {s[4] for s in chain}
        victim = chain[0][4] if chain else None
        records.append({
            "shape": " ".join("%s%s" % ("CS" if s[3] == "Cheap Shot" else "KS",
                                        "imm" if s[5] == 0.0 else ("%g" % s[5] if s[5] else "?"))
                              for s in chain) or "(none)",
            "stuns": len(chain),
            "applied": applied,
            "held": held,
            "immune": sum(1 for s in chain if s[5] == 0.0),
            "converts": bool(kills),
            "converts_victim": any(k[1] in victims for k in kills),
            # The first victim dead by the window's end: the same reading
            # `fates` takes of the Vitality twin's log.
            "victim_dead": victim is not None and any(
                who == victim and when <= window_end for when, who, _ in deaths),
            "window": window_end - t0,
            "window_end": window_end,
            "team": team,
            "victim_name": victim,
            # The class of the chain's first victim ("Team 2 Paladin #2").
            "victim": victim.split()[2] if victim else "(none)",
            "admission": admission(config, team, victim, t0, deaths),
        })
    return records


def fates(text, record):
    """In another arm's log of the same match: (the Dance's first victim is
    dead by the Dance's window end, some enemy team member is), for the
    counterfactual of a Dance."""
    deaths = member_deaths(text)
    enemy = "2" if record["team"] == "1" else "1"
    by_end = [d for d in deaths if d[0] <= record["window_end"]]
    return (any(d[1] == record["victim_name"] for d in by_end),
            any(d[2] == enemy for d in by_end))


def dance_one(binary, config_line, workdir, index):
    text = run_log(binary, config_line, workdir, index)
    cfg = json.loads(config_line)
    key = (cfg["label"], str(cfg["random_seed"]))
    return (analyse_dances(text, cfg), None,
            len(re.findall(r"\[EQUIPMENT\] Team \d Rogue", text)), key,
            len(re.findall(r"Rogue #\d's Initiative is spent on Shadow Dance", text)))


def twin_one(binary, config_line, workdir, index):
    cfg = json.loads(config_line)
    return (cfg["label"], str(cfg["random_seed"])), run_log(binary, config_line, workdir, index)


def dance(outdir, binary):
    from collections import Counter

    def configs(arm):
        with open(os.path.join(outdir, arm + ".jsonl"), encoding="utf-8") as f:
            return [ln for ln in f if rogue_label(ln)]

    sub = replay(outdir, "sub", binary, configs("sub"), dance_one)
    dances = [d for recs, _, _, _, _ in sub for d in recs]
    rogues = sum(r[2] for r in sub)
    n = len(dances)
    print("Subtlety reference: %d Rogue matches, %d Rogues, %d Dances (%.2f a Rogue a match); "
          "%d Dances spent an Initiative discount on the free Dance itself"
          % (len(sub), rogues, n, n / max(1, rogues), sum(r[4] for r in sub)))
    if not n:
        return

    def mean(key):
        return sum(d[key] for d in dances) / n
    print("stuns a Dance: %.2f; stun-seconds applied a Dance: %.2f (held, cut at the "
          "victim's death: %.2f); DR-immune stuns a Dance: %.2f"
          % (mean("stuns"), mean("applied"), mean("held"), mean("immune")))
    conv = sum(d["converts"] for d in dances)
    convv = sum(d["converts_victim"] for d in dances)
    print("converts (an enemy team member dies by %.0fs past the later of the Dance's end and "
          "its chain's last stun's end): %d/%d = %.1f%%; the stunned enemy dies: %d/%d = %.1f%%; "
          "mean window %.1fs"
          % (CONVERT_GRACE, conv, n, 100.0 * conv / n, convv, n, 100.0 * convv / n,
             mean("window")))
    print("\nchain shapes:")
    for shape, count in Counter(d["shape"] for d in dances).most_common(12):
        print("  %5d  %s" % (count, shape))
    victims = Counter(d["victim"] for d in dances)
    print("\nthe chain's first victim: " + ", ".join(
        "%s %d" % (k, v) for k, v in victims.most_common()) +
        "; a healer in %.1f%%" % (100.0 * sum(v for k, v in victims.items() if k in HEALERS) / n))
    applied = Counter(round(d["applied"]) for d in dances)
    print("\nstun-seconds applied a Dance (rounded): " +
          ", ".join("%ds x%d" % (k, applied[k]) for k in sorted(applied)))

    # The counterfactual: the same matches in the Vitality twin (the Subtlety
    # stem with Vitality in the Dance's place), at the same window end.
    danced = {r[3] for r in sub if r[0]}
    twin_lines = [ln for ln in configs("sub_vit")
                  if (json.loads(ln)["label"], str(json.loads(ln)["random_seed"])) in danced]
    twin = dict(replay(outdir, "sub_vit", binary, twin_lines, twin_one))
    per_key = {r[3]: r[0] for r in sub if r[0]}
    rows = []  # (admission, dance victim dead, twin victim dead, twin enemy dead)
    for key, recs in per_key.items():
        for d in recs:
            if d["victim_name"] is None:
                continue
            tv, te = fates(twin[key], d)
            rows.append((d["admission"], d["victim_dead"], tv, te))
    print("\nadmission (which branch of `enemy_healer_answers` let the Dance go), and the "
          "Dance's first victim against the Vitality twin of the same match, both read at the "
          "Dance's window end:\n")
    print("| admission | Dances | share | victim dead, Dance | victim dead, twin | "
          "any enemy dead, twin |")
    print("|---|---|---|---|---|---|")
    groups = [(a, [r for r in rows if r[0] == a]) for a in ADMISSIONS[:4]] + [("all", rows)]
    for name, rs in groups:
        if not rs:
            continue
        k = len(rs)
        print("| %s | %d | %.1f%% | %.1f%% | %.1f%% | %.1f%% |"
              % (name, k, 100.0 * k / len(rows), 100.0 * sum(r[1] for r in rs) / k,
                 100.0 * sum(r[2] for r in rs) / k, 100.0 * sum(r[3] for r in rs) / k))
    print("\n(%d Dances landed no stun and are left out)" % (n - len(rows)))

    # The matches a Dance was cast in, paired against the same match with
    # Vitality in the Dance's place: does a Dance change who wins the match
    # it is cast in? Selected on the reference arm, which favours the Dance.
    ref, vit = read_csv(outdir, "sub"), read_csv(outdir, "sub_vit")
    import paired_sweep as ps
    buckets = ps.build_slices(sorted(danced), vit, {"Rogue"})
    clean = set(buckets["clean"])
    if buckets["clean"] + buckets["against"]:
        s = ps.Slice("danced", buckets["clean"] + buckets["against"], vit, ref,
                     side=lambda k: "team1" if k in clean else "team2")
        print("\nthe %d one-sided Rogue matches with a Dance (of %d Rogue matches), Rogue side's "
              "win rate, Vitality in the Dance's place -> the reference:" % (s.n, len(sub)))
        print(HEADER % ("slice", "", ""))
        print(row("matches with a Dance", s))


def gates_one(binary, config_line, workdir, index):
    """The Shadow Dance candidate's outcome in every Rogue decision of one
    traced match: a Counter of rejection notes (or reasons), plus "chosen"."""
    from collections import Counter
    cfg = os.path.join(workdir, "%d.json" % index)
    log = os.path.join(workdir, "%d.txt" % index)
    trace = os.path.join(workdir, "%d_trace.jsonl" % index)
    with open(cfg, "w", encoding="utf-8") as f:
        f.write(config_line)
    subprocess.run([binary, "--headless", cfg, "--output", log, "--trace-mode", "on"],
                   check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    out = Counter()
    with open(trace, encoding="utf-8") as f:
        for line in f:
            try:
                ev = json.loads(line)
            except ValueError:
                continue
            if ev.get("kind") != "ability_decision" or ev.get("actor", {}).get("class") != "Rogue":
                continue
            for c in ev.get("candidates") or []:
                if c.get("ability") != "ShadowDance":
                    continue
                if c.get("status") != "rejected":
                    out["chosen"] += 1
                    continue
                reason = c.get("reason")
                if isinstance(reason, dict):
                    kind = next(iter(reason))
                    note = reason[kind].get("note") if isinstance(reason[kind], dict) else None
                    out[note or kind] += 1
                else:
                    out[str(reason)] += 1
    for path in (cfg, log, trace):
        os.remove(path)
    return out


def gates(outdir, binary):
    """Why the Subtlety reference's Rogues did not Dance: the Shadow Dance
    candidate's rejection reasons over the `sample` slice, traced. Counts are
    decision events, not seconds."""
    from collections import Counter
    total = Counter()
    for c in replay(outdir, "sub-gates", binary,
                    sample_configs(os.path.join(outdir, "sub.jsonl")), gates_one):
        total.update(c)
    n = sum(total.values())
    print("Shadow Dance candidate over %d Rogue decisions (sample slice of `sub`):" % n)
    for reason, count in total.most_common():
        print("  %6d  %5.1f%%  %s" % (count, 100.0 * count / n, reason))


def main(argv=None):
    argv = sys.argv[1:] if argv is None else argv
    if len(argv) >= 4 and argv[0] == "sample":
        sample(argv[1], argv[2], int(argv[3]), argv[4:] or None)
        return 0
    if len(argv) == 3 and argv[0] in ("sample", "dance", "gates"):
        {"sample": sample, "dance": dance, "gates": gates}[argv[0]](argv[1], argv[2])
        return 0
    if len(argv) == 3 and argv[0] == "replicate":
        replicate(argv[1], argv[2])
        return 0
    if len(argv) >= 3 and argv[0] == "gen":
        gen(argv[1], int(argv[2]), argv[3:] or None)
        return 0
    if len(argv) != 2 or argv[0] not in ("gen", "table"):
        sys.exit(__doc__)
    {"gen": gen, "table": table}[argv[0]](argv[1])
    return 0


if __name__ == "__main__":
    sys.exit(main())
