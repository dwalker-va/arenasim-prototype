#!/usr/bin/env python3
"""AS-299: the Warlock reference builds and node arms, generated and compared.

Every arm is the same 2v2 directional sweep -- every cell a Warlock change can
reach plus 8 control cells, on all three maps, 4 seeds a cell a map, under
`TeamPlan` -- with every Warlock, on either side, playing that arm's build. The
arms differ ONLY in that build (and, for two of them, the per-match choice its
node opens), so any two pair at identical (label, seed).

    # 1. write one JSONL per arm (calls scripts/gen_sweep.py); optionally on
    #    another seed block, for only some arms (the replication)
    2026-11-22-as299-arms.py gen OUTDIR [SEED_BASE [ARM ...]]
    # 2. run each arm (any binary built from the PR head)
    for f in OUTDIR/*.jsonl; do
      arenasim --batch "$f" --out "${f%.jsonl}.csv" --jobs 5 --trace-mode off
    done
    # 3. the tables in the findings doc
    2026-11-22-as299-arms.py table OUTDIR
    # 4. non-vacuity: replay 60 Warlock matches of each arm with full logs,
    #    and count what each node did
    2026-11-22-as299-arms.py sample OUTDIR <path to arenasim>
    # 5. the replication on a second seed block (`gen OUTDIR2 4 demo demo_de
    #    demo_bane demo_stem`, run as in 2), per block and pooled
    2026-11-22-as299-arms.py replicate OUTDIR OUTDIR2

`table` reads `<arm>.csv` from OUTDIR and prints, for each
comparison, the Warlock side's paired win-rate delta (CLEAN and AGAINST cells,
scored for the side fielding the Warlock -- `paired_sweep.py --headline
affected`), its flips, McNemar z and the resolution floor that pair bought.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPTS = os.path.normpath(os.path.join(HERE, "..", "..", "..", "..", "scripts"))
sys.path.insert(0, SCRIPTS)

MAPS = ("BasicArena", "PillaredArena", "TwinPillars")
SEEDS_PER_MAP = 4
EXHAUSTION = ["Exhaustion", "Exhaustion"]  # one curse per enemy, 2v2


def moved(reference, remove=None, add=None):
    """A reference build with ranks moved, as a headless config names it."""
    spec = {"reference": reference}
    if remove:
        spec["remove"] = remove
    if add:
        spec["add"] = add
    return spec


# name -> (build spec or None for no talents, [(field, value)] per-slot choices)
ARMS = {
    "zero": (None, []),
    # Affliction 6/0/2; its last point is Shadow Mastery.
    "aff": ("Affliction", []),
    "aff_stem": (moved("Affliction", {"ShadowMastery": 1}), []),
    "aff_de": (moved("Affliction", {"ShadowMastery": 1}, {"DemonicEmbrace": 1}), []),
    "aff_ac": (moved("Affliction", {"ShadowMastery": 1}, {"AmplifyCurse": 1}), []),
    "aff_coe": (moved("Affliction", {"ShadowMastery": 1}, {"CurseOfExhaustion": 1}),
                [("warlock_curse_prefs", EXHAUSTION)]),
    # aff_de with Siphon Life's point moved onto Demonic Embrace too.
    "aff_siphon_de": (moved("Affliction", {"ShadowMastery": 1, "SiphonLife": 1},
                            {"DemonicEmbrace": 2}), []),
    # Demonology 2/6/0; its last point is Soul Link.
    "demo": ("Demonology", []),
    "demo_stem": (moved("Demonology", {"SoulLink": 1}), []),
    "demo_de": (moved("Demonology", {"SoulLink": 1}, {"DemonicEmbrace": 1}), []),
    "demo_ds": (moved("Demonology", {"SoulLink": 1}, {"DemonicSacrifice": 1}),
                [("warlock_demons", "Sacrifice")]),
    "demo_nf": (moved("Demonology", {"SoulLink": 1}, {"Nightfall": 1}), []),
    "demo_isb": (moved("Demonology", {"SoulLink": 1}, {"ImprovedShadowBolt": 1}), []),
    "demo_bane": (moved("Demonology", {"SoulLink": 1}, {"Bane": 1}), []),
    # Destruction 0/2/6; its last point is Conflagrate.
    "destr": ("Destruction", []),
    "destr_stem": (moved("Destruction", {"Conflagrate": 1}), []),
    "destr_de": (moved("Destruction", {"Conflagrate": 1}, {"DemonicEmbrace": 1}), []),
    "destr_sb": (moved("Destruction", {"Conflagrate": 1}, {"Shadowburn": 1}), []),
    "destr_am": (moved("Destruction", {"Conflagrate": 1}, {"Aftermath": 1}), []),
    "destr_md": (moved("Destruction", {"Conflagrate": 1}, {"MasterDemonologist": 1}), []),
}

# The success condition's comparisons: (node, its price in ranks, arm with the
# node, arm with that point on Demonic Embrace instead).
NODE_ARMS = [
    ("Amplify Curse", 1, "aff_ac", "aff_de"),
    ("Nightfall (rank 1)", 1, "demo_nf", "demo_de"),
    ("Siphon Life", 1, "aff_de", "aff_siphon_de"),
    ("Curse of Exhaustion", 1, "aff_coe", "aff_de"),
    ("Master Demonologist", 1, "destr_md", "destr_de"),
    ("Demonic Sacrifice (Sacrifice chosen)", 1, "demo_ds", "demo_de"),
    ("Soul Link (capstone)", 2, "demo", "demo_de"),
    ("Improved Shadow Bolt (rank 1)", 1, "demo_isb", "demo_de"),
    ("Bane (rank 1)", 1, "demo_bane", "demo_de"),
    ("Aftermath (rank 1)", 1, "destr_am", "destr_de"),
    ("Shadowburn", 1, "destr_sb", "destr_de"),
    ("Conflagrate (capstone)", 2, "destr", "destr_de"),
    ("Shadow Mastery (capstone; a Patch, for scale)", 2, "aff", "aff_de"),
]

# What one Demonic Embrace rank buys on each stem: (stem name, +DE arm, stem).
STAT_RANK = [
    ("Affliction stem", "aff_de", "aff_stem"),
    ("Demonology stem", "demo_de", "demo_stem"),
    ("Destruction stem", "destr_de", "destr_stem"),
]

REFERENCES = [("Affliction 6/0/2", "aff"), ("Demonology 2/6/0", "demo"),
              ("Destruction 0/2/6", "destr")]


def gen_args(arm, game_map, seed_base=0):
    spec, choices = ARMS[arm]
    argv = [sys.executable, os.path.join(SCRIPTS, "gen_sweep.py"),
            "--full", "2", "--exclude-double-healer",
            "--affects", "Warlock", "--control-cells", "8",
            "--n", str(SEEDS_PER_MAP), "--seed-base", str(seed_base),
            "--extra", json.dumps({"ai_profile": "TeamPlan", "map": game_map}),
            "--label-suffix", game_map]
    if spec is not None:
        argv += ["--build", "Warlock=" + (spec if isinstance(spec, str) else json.dumps(spec))]
    for field, value in choices:
        argv += ["--for-class", "Warlock", field, json.dumps(value)]
    return argv


def gen(outdir, seed_base=0, arms=None):
    """One JSONL per arm. `seed_base` moves the whole sweep onto another seed
    block (the replication of a borderline comparison); `arms` limits it."""
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
    """The Warlock side's paired delta, after_arm against before_arm."""
    import paired_sweep as ps
    for arm in (after_arm, before_arm):
        if arm not in cache:
            cache[arm] = read_csv(outdir, arm)
    after, before = cache[after_arm], cache[before_arm]
    keys = [k for k in before if k in after]
    buckets = ps.build_slices(keys, before, {"Warlock"})
    clean = set(buckets["clean"])
    s = ps.Slice("warlock", buckets["clean"] + buckets["against"], before, after,
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


def table(outdir):
    cache = {}
    print("## References against 0 points (Warlock side's win rate)\n")
    print(HEADER % ("reference", "", ""))
    for label, arm in REFERENCES:
        s, nc, same = compare(outdir, cache, arm, "zero")
        print(row(label, s))
    print("\n## One Demonic Embrace rank on each stem (7 -> 8 points)\n")
    print(HEADER % ("stem", "", ""))
    stat = {}
    for label, plus, stem in STAT_RANK:
        s, _, _ = compare(outdir, cache, plus, stem)
        stat[plus] = s.delta
        print(row(label, s))
    print("\n## Each node against a Demonic Embrace rank in its place\n")
    print(HEADER % ("node", " verdict |", "---|"))
    for label, ranks, arm, base in NODE_ARMS:
        s, nc, same = compare(outdir, cache, arm, base)
        # A capstone is priced at two ranks: it may beat one stat rank by
        # what a second one buys on the same stem.
        allowance = stat.get(base, 0.0) if ranks == 2 else 0.0
        margin = s.delta - max(allowance, 0.0)
        floor = s.floor if s.floor is not None else 0.0
        dominates = s.floor is not None and margin > floor
        verdict = ("DOMINATES" if dominates else "within") + (
            " (allowance %+.1f)" % allowance if ranks == 2 else "")
        print(row(label, s, " %s |" % verdict))
    print("\n## Each arm against its reference\n")
    print(HEADER % ("arm", "", ""))
    ref_of = {"aff": "aff", "demo": "demo", "destr": "destr"}
    for arm in ARMS:
        if arm == "zero" or arm in ref_of:
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
# each arm's Warlock matches through `--headless` and counts these lines.
EVENTS = [
    ("Warlocks", r"\[EQUIPMENT\] Team \d Warlock"),
    ("Shadow Bolt hits", r"Warlock #\d's Shadow Bolt hits"),
    ("Immolate hits", r"Warlock #\d's Immolate hits"),
    ("Siphon Life casts", r"Warlock #\d casts Siphon Life"),
    ("Amplify Curse casts", r"Warlock #\d casts Amplify Curse"),
    ("Curse of Exhaustion casts", r"Warlock #\d casts Curse of Exhaustion"),
    ("Shadowburn casts", r"Warlock #\d casts Shadowburn"),
    ("Conflagrate casts", r"Warlock #\d casts Conflagrate"),
    ("Shadow Trance procs", r"\[PROC\] .*Warlock.* procs Shadow Trance"),
    ("Shadow Trances spent", r"Shadow Trance is spent on"),
    ("Shadow Vulnerability procs", r"\[PROC\] .*Warlock.* procs Shadow Vulnerability"),
    ("Aftermath procs", r"\[PROC\] .*Warlock.* procs Aftermath"),
    ("Soul Link transfers", r"\(Soul Link\)"),
    ("Master Demonologist auras", r"gains Master Demonologist"),
    ("Felhunters sacrificed", r"sacrifices its Felhunter"),
]
SAMPLE_PER_ARM = 60


def sample_configs(path, k=SAMPLE_PER_ARM):
    """Every n-th config of an arm that fields a Warlock: a fixed slice, the
    same configs (up to the build) in every arm."""
    with open(path, encoding="utf-8") as f:
        lines = [ln for ln in f if "Warlock" in ln.split('"label"')[1].split(",")[0]]
    step = max(1, len(lines) // k)
    return lines[::step][:k]


def run_one(binary, config_line, workdir, index):
    import re
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


def sample(outdir, binary, jobs=5):
    from concurrent.futures import ThreadPoolExecutor
    print("| arm | " + " | ".join(name for name, _ in EVENTS) + " |")
    print("|---|" + "---|" * len(EVENTS))
    for arm in ARMS:
        work = os.path.join(outdir, ".sample-" + arm)
        os.makedirs(work, exist_ok=True)
        configs = sample_configs(os.path.join(outdir, arm + ".jsonl"))
        with ThreadPoolExecutor(max_workers=jobs) as pool:
            counts = list(pool.map(lambda a: run_one(binary, a[1], work, a[0]),
                                   enumerate(configs)))
        os.rmdir(work)
        totals = [sum(c[i] for c in counts) for i in range(len(EVENTS))]
        print("| %s (%d) | %s |" % (arm, len(configs), " | ".join(str(t) for t in totals)))


# The comparisons a second seed block replicates: the two nodes the first
# block put over the line, and the stat rank Soul Link's allowance reads.
REPLICATED = [
    ("Soul Link (capstone)", "demo", "demo_de"),
    ("Bane (rank 1)", "demo_bane", "demo_de"),
    ("Demonic Embrace rank, Demonology stem", "demo_de", "demo_stem"),
]


def replicate(block1, block2):
    print(HEADER % ("comparison", " block |", "---|"))
    for label, arm, base in REPLICATED:
        for name, dirs in (("seeds 0-3", block1), ("seeds 4-7", block2),
                           ("pooled", block1 + os.pathsep + block2)):
            s, _, _ = compare(dirs, {}, arm, base)
            print(row(label, s, " %s |" % name))


def main(argv=None):
    argv = sys.argv[1:] if argv is None else argv
    if len(argv) == 3 and argv[0] == "sample":
        sample(argv[1], argv[2])
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
