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
    #    (and the Kidney Shots of its stem, the baseline) with full logs
    2026-11-22-as301-arms.py dance OUTDIR <path to arenasim>
    # 7. why the Dance did not go: its rejection reasons over the sample
    #    slice of the Subtlety reference, traced
    2026-11-22-as301-arms.py gates OUTDIR <path to arenasim>

`table` reads `<arm>.csv` from OUTDIR and prints, for each comparison, the
Rogue side's paired win-rate delta (CLEAN and AGAINST cells, scored for the
side fielding the Rogue -- `paired_sweep.py --headline affected`), its flips,
McNemar z and the resolution floor that pair bought.
"""

from __future__ import annotations

import json
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


def allowance(price, vitality_value):
    """What a stat rank of `price` may be worth over Vitality, scaling
    Vitality's measured worth on the stem linearly in price: negative for a
    one-rank node (11 against 20.4), slightly positive for a capstone (22)."""
    return vitality_value * (price / VITALITY_PRICE - 1.0)


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
    stat = {}
    for label, key, plus, stem in STAT_RANK:
        s, _, _ = compare(outdir, cache, plus, stem)
        stat[key] = s.delta
        print(row(label, s))
    print("\n## Each node against Vitality in its place\n")
    print(HEADER % ("node", " allowance | margin | verdict |", "---|---|---|"))
    for label, price, arm, base, stem in NODE_ARMS:
        s, nc, same = compare(outdir, cache, arm, base)
        allow = allowance(price, stat[stem])
        print(row(label, s, " %+.1f | %+.1f | %s |" % (allow, s.delta - allow, verdict(s, allow))))
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


def replicate(block1, block2):
    """Each replicated comparison per block and pooled; a node row's pooled
    verdict reads the pooled Vitality rank on its stem for its allowance."""
    pooled = block1 + os.pathsep + block2
    vitality = {}
    for label, key, plus, stem in STAT_RANK:
        vitality[key] = compare(pooled, {}, plus, stem)[0].delta
    node = {(arm, base): (price, stem) for _, price, arm, base, stem in NODE_ARMS}
    print(HEADER % ("comparison", " block | allowance | margin | verdict |", "---|---|---|---|"))
    for label, arm, base in REPLICATED:
        for name, dirs in (("block 1", block1), ("block 2", block2), ("pooled", pooled)):
            s, _, _ = compare(dirs, {}, arm, base)
            extra = " %s |" % name
            if (arm, base) in node and name == "pooled":
                price, stem = node[(arm, base)]
                allow = allowance(price, vitality[stem])
                extra += " %+.1f | %+.1f | %s |" % (allow, s.delta - allow, verdict(s, allow))
            else:
                extra += " | | |"
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
# members, so never count) between the Dance's cast and the end of its
# chain's last stun plus this follow-through: one global cooldown and a half,
# the time a burst on a target coming out of a stun still lands.
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


def analyse_dances(text):
    """One record per Dance: its chain, stun-seconds and conversion."""
    deaths = [(float(m.group(1)), m.group(2), m.group(3)) for m in map(DEATH.match, text.splitlines()) if m]
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
            "window": window_end - t0,
            # The class of the chain's first victim ("Team 2 Paladin #2").
            "victim": chain[0][4].split()[2] if chain else "(none)",
        })
    return records


def analyse_kidneys(text):
    """The baseline: every Kidney Shot a Rogue lands with no Dance, and
    whether its team kills an enemy by the stun's end plus the same grace."""
    deaths = [(float(m.group(1)), m.group(2), m.group(3)) for m in map(DEATH.match, text.splitlines()) if m]
    out = []
    for t, rogue, team, ability, target, applied, dr in stun_events(text):
        if ability != "Kidney Shot" or not applied:
            continue
        enemy = "2" if team == "1" else "1"
        window_end = t + applied + CONVERT_GRACE
        kills = [d for d in deaths if d[2] == enemy and t <= d[0] <= window_end]
        out.append({"converts": bool(kills), "converts_victim": any(k[1] == target for k in kills),
                    "applied": applied})
    return out


def dance_one(binary, config_line, workdir, index):
    text = run_log(binary, config_line, workdir, index)
    cfg = json.loads(config_line)
    key = (cfg["label"], str(cfg["random_seed"]))
    return (analyse_dances(text), analyse_kidneys(text),
            len(re.findall(r"\[EQUIPMENT\] Team \d Rogue", text)), key,
            len(re.findall(r"Rogue #\d's Initiative is spent on Shadow Dance", text)))


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
    if n:
        def mean(key):
            return sum(d[key] for d in dances) / n
        print("stuns a Dance: %.2f; stun-seconds applied a Dance: %.2f (held, cut at the "
              "victim's death: %.2f); DR-immune stuns a Dance: %.2f"
              % (mean("stuns"), mean("applied"), mean("held"), mean("immune")))
        conv = sum(d["converts"] for d in dances)
        convv = sum(d["converts_victim"] for d in dances)
        print("converts (an enemy dies by the chain's end + %.0fs): %d/%d = %.1f%%; the stunned "
              "enemy dies: %d/%d = %.1f%%; mean window %.1fs"
              % (CONVERT_GRACE, conv, n, 100.0 * conv / n, convv, n, 100.0 * convv / n,
                 mean("window")))
        print("\nchain shapes:")
        for shape, count in Counter(d["shape"] for d in dances).most_common(12):
            print("  %5d  %s" % (count, shape))
        healers = {"Priest", "Paladin", "Shaman", "Druid"}
        victims = Counter(d["victim"] for d in dances)
        print("\nthe chain's first victim: " + ", ".join(
            "%s %d" % (k, v) for k, v in victims.most_common()) +
            "; a healer in %.1f%%" % (100.0 * sum(v for k, v in victims.items() if k in healers) / n))
        applied = Counter(round(d["applied"]) for d in dances)
        print("\nstun-seconds applied a Dance (rounded): " +
              ", ".join("%ds x%d" % (k, applied[k]) for k in sorted(applied)))
    # The matches a Dance was cast in, paired against the same match with
    # Vitality in the Dance's place: does a Dance change who wins the match
    # it is cast in?
    danced = {r[3] for r in sub if r[0]}
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
    kid = [k for _, ks, _, _, _ in replay(outdir, "sub_stem", binary, configs("sub_stem"),
                                          dance_one)
           for k in ks]
    if kid:
        conv = sum(k["converts"] for k in kid)
        convv = sum(k["converts_victim"] for k in kid)
        print("\nbaseline, Subtlety stem (no Dance): %d Kidney Shots landed; an enemy dies by "
              "the stun's end + %.0fs: %.1f%%; the stunned enemy: %.1f%%"
              % (len(kid), CONVERT_GRACE, 100.0 * conv / len(kid), 100.0 * convv / len(kid)))


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
