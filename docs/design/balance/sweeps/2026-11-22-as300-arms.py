#!/usr/bin/env python3
"""AS-300: the Priest reference builds and node arms, generated and compared.

Every arm is the same 2v2 directional sweep -- every cell a Priest change can
reach plus 8 control cells, on all three maps, 4 seeds a cell a map, under
`TeamPlan` -- with every Priest, on either side, playing that arm's build (and,
for the Shadowform arms, choosing the form). The arms differ ONLY in that
build and form, so any two pair at identical (label, seed).

    # 1. write one JSONL per arm (calls scripts/gen_sweep.py), on a seed block
    2026-11-22-as300-arms.py gen OUTDIR [SEED_BASE [ARM ...]]
    # 2. run each arm (any binary built from the PR head)
    for f in OUTDIR/*.jsonl; do
      arenasim --batch "$f" --out "${f%.jsonl}.csv" --jobs 5 --trace-mode off
    done
    # 3. the findings doc's tables, over one block or several pooled
    #    (directories joined by the path separator, ':' here)
    2026-11-22-as300-arms.py table OUTDIR[:OUTDIR2]
    # 4. every node comparison per seed block and pooled
    2026-11-22-as300-arms.py replicate OUTDIR OUTDIR2
    # 5. non-vacuity: replay a fixed slice of each arm's Priest matches with
    #    full logs, and count what each node did
    2026-11-22-as300-arms.py sample OUTDIR <path to arenasim>
    # 6. the Mind Flay mechanism counts: replay 120 Priest matches of each
    #    Mind Flay pair with full logs, and sum the Priest's channels, heals,
    #    damage and damage taken
    2026-11-22-as300-arms.py mechanism OUTDIR <path to arenasim>

**The yardstick.** The Priest has no `StatPercent` node near a rank's price:
its one, Holy Concentration, prices at 3.5. A node is therefore measured
against one rank of **Spiritual Healing** (+5% healing, a plain Patch at the
declared 11), which on today's kit is about what +7.3 spell power -- 11 item
budget points at its 1.5 weight -- adds to Flash Heal. In Shadowform no heal
lands, so there the yardstick is one rank of **Improved Power Word: Shield**
(+5% absorb, the one plain rank still at work in the form).

Each node comparison is two 8-point builds that differ in ONE point: on the
node, or on the yardstick. A node the reference's stem lacks is added to the
stem (`stem + node` against `stem + Y`); a node the stem holds is measured by
its removal (`stem + Y` against `stem - node + 2Y`), as AS-299 measured Siphon
Life. A capstone may beat one yardstick rank by what a second buys on the same
stem (`stem + Y` against `stem`), its two-rank price.
"""

from __future__ import annotations

import json
import os
import re
import statistics
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPTS = os.path.normpath(os.path.join(HERE, "..", "..", "..", "..", "scripts"))
sys.path.insert(0, SCRIPTS)

MAPS = ("BasicArena", "PillaredArena", "TwinPillars")
SEEDS_PER_MAP = 4
CLASS = "Priest"
HEALERS = {"Priest", "Paladin", "Shaman", "Druid"}

Y = "SpiritualHealing"            # the healing role's yardstick rank
YS = "ImprovedPowerWordShield"    # Shadowform's

# The Discipline-Shadow hybrid of design §5.2 (5/0/3). It takes no capstone,
# so it cannot be a reference build; it is an arm, written as its ranks.
HYBRID = {"UnbreakableWill": 2, "InnerFocus": 1, "PowerInfusion": 1, "ReflectiveShield": 1,
          "ImprovedMindBlast": 2, "Shadowform": 1}


def moved(reference, remove=None, add=None):
    """A reference build with ranks moved, as a headless config names it."""
    spec = {"reference": reference}
    if remove:
        spec["remove"] = remove
    if add:
        spec["add"] = add
    return spec


def stem(reference, capstone, remove=None, add=None):
    """The reference without its capstone, with further ranks moved."""
    out = dict(remove or {})
    out[capstone] = 1
    return moved(reference, out, add)


SF = "Shadowform"
# name -> (build spec or None for no talents, the form, or None for no form)
ARMS = {
    "zero": (None, None),
    # Discipline 6/2/0; its last point is Pain Suppression.
    "disc": ("Discipline", None),
    "disc_stem": (stem("Discipline", "PainSuppression"), None),
    "disc_y": (stem("Discipline", "PainSuppression", add={Y: 1}), None),
    "disc_pi": (stem("Discipline", "PainSuppression", add={"PowerInfusion": 1}), None),
    "disc_uw_y": (stem("Discipline", "PainSuppression", {"UnbreakableWill": 1}, {Y: 2}), None),
    "disc_if_y": (stem("Discipline", "PainSuppression", {"InnerFocus": 1}, {Y: 2}), None),
    "disc_rs_y": (stem("Discipline", "PainSuppression", {"ReflectiveShield": 1}, {Y: 2}), None),
    # Holy 2/6/0; its last point is Guardian Spirit.
    "holy": ("Holy", None),
    "holy_stem": (stem("Holy", "GuardianSpirit"), None),
    "holy_y": (stem("Holy", "GuardianSpirit", add={Y: 1}), None),
    "holy_hn": (stem("Holy", "GuardianSpirit", add={"HolyNova": 1}), None),
    "holy_br": (stem("Holy", "GuardianSpirit", add={"BlessedRecovery": 1}), None),
    "holy_insp_y": (stem("Holy", "GuardianSpirit", {"Inspiration": 1}, {Y: 2}), None),
    "holy_hc_y": (stem("Holy", "GuardianSpirit", {"HolyConcentration": 1}, {Y: 2}), None),
    # Shadow 2/0/6 in Shadowform; its last point is Silence.
    "shadow": ("Shadow", SF),
    "shadow_stem": (stem("Shadow", "Silence"), SF),
    "shadow_y": (stem("Shadow", "Silence", add={YS: 1}), SF),
    "shadow_ve": (stem("Shadow", "Silence", add={"VampiricEmbrace": 1}), SF),
    "shadow_bl_y": (stem("Shadow", "Silence", {"Blackout": 1}, {YS: 2}), SF),
    "shadow_mf_y": (stem("Shadow", "Silence", {"MindFlay": 1}, {YS: 2}), SF),
    # The same builds healing (no form): the Shadow tree serves a healer too.
    "shadow_heal": ("Shadow", None),
    "shadow_stem_heal": (stem("Shadow", "Silence"), None),
    "shadow_y_heal": (stem("Shadow", "Silence", add={Y: 1}), None),
    "shadow_ve_heal": (stem("Shadow", "Silence", add={"VampiricEmbrace": 1}), None),
    "shadow_bl_y_heal": (stem("Shadow", "Silence", {"Blackout": 1}, {Y: 2}), None),
    "shadow_mf_y_heal": (stem("Shadow", "Silence", {"MindFlay": 1}, {Y: 2}), None),
    # The hybrid, healing and in Shadowform.
    "hybrid": (HYBRID, None),
    "hybrid_sf": (HYBRID, SF),
}

# The success condition's comparisons: (node, its price in ranks, arm with the
# node's point, arm with that point on the yardstick, the stem's "+Y" arm whose
# worth over the bare stem is a capstone's allowance).
NODE_ARMS = [
    ("Unbreakable Will (rank 2)", 1, "disc_y", "disc_uw_y", None),
    ("Inner Focus", 1, "disc_y", "disc_if_y", None),
    ("Reflective Shield", 1, "disc_y", "disc_rs_y", None),
    ("Power Infusion", 1, "disc_pi", "disc_y", None),
    ("Pain Suppression (capstone)", 2, "disc", "disc_y", "disc_y"),
    ("Inspiration (rank 2)", 1, "holy_y", "holy_insp_y", None),
    ("Blessed Recovery", 1, "holy_br", "holy_y", None),
    ("Holy Nova", 1, "holy_hn", "holy_y", None),
    ("Guardian Spirit (capstone)", 2, "holy", "holy_y", "holy_y"),
    ("Blackout (rank 1), Shadowform", 1, "shadow_y", "shadow_bl_y", None),
    ("Mind Flay, Shadowform", 1, "shadow_y", "shadow_mf_y", None),
    ("Vampiric Embrace, Shadowform", 1, "shadow_ve", "shadow_y", None),
    ("Silence (capstone), Shadowform", 2, "shadow", "shadow_y", "shadow_y"),
    ("Blackout (rank 1), healing", 1, "shadow_y_heal", "shadow_bl_y_heal", None),
    ("Mind Flay, healing", 1, "shadow_y_heal", "shadow_mf_y_heal", None),
    ("Vampiric Embrace, healing", 1, "shadow_ve_heal", "shadow_y_heal", None),
]

# For scale, not verdicts: the one computed StatPercent node (priced 3.5)
# against a second yardstick rank, and Silence where it cannot be cast.
SCALE_ARMS = [
    ("Holy Concentration (StatPercent, priced 3.5)", "holy_y", "holy_hc_y"),
    ("Silence, healing (locked without the form)", "shadow_heal", "shadow_y_heal"),
]

# What one yardstick rank buys on each stem: (stem, +Y arm, stem arm).
STAT_RANK = [
    ("Discipline stem + Spiritual Healing", "disc_y", "disc_stem"),
    ("Holy stem + Spiritual Healing", "holy_y", "holy_stem"),
    ("Shadow stem + Imp. PW:Shield (Shadowform)", "shadow_y", "shadow_stem"),
    ("Shadow stem + Spiritual Healing (healing)", "shadow_y_heal", "shadow_stem_heal"),
]

# A capstone's allowance: its stem with one yardstick rank (V+) against the
# bare stem (V0).
ALLOWANCE_STEM = {plus: bare for _, plus, bare in STAT_RANK}

REFERENCES = [
    ("Discipline 6/2/0", "disc"),
    ("Holy 2/6/0", "holy"),
    ("Shadow 2/0/6, Shadowform", "shadow"),
    ("Shadow 2/0/6, healing", "shadow_heal"),
    ("Hybrid 5/0/3, healing", "hybrid"),
    ("Hybrid 5/0/3, Shadowform", "hybrid_sf"),
]

# Each arm a node is compared against, beside its reference: how far the
# yardstick itself sits from the build it stands in for.
YARDSTICK_ARMS = [
    ("Discipline stem + Spiritual Healing", "disc_y", "disc"),
    ("Discipline stem - Unbreakable Will rank + 2 Spiritual Healing", "disc_uw_y", "disc"),
    ("Discipline stem - Inner Focus + 2 Spiritual Healing", "disc_if_y", "disc"),
    ("Discipline stem - Reflective Shield + 2 Spiritual Healing", "disc_rs_y", "disc"),
    ("Holy stem + Spiritual Healing", "holy_y", "holy"),
    ("Holy stem - Inspiration rank + 2 Spiritual Healing", "holy_insp_y", "holy"),
    ("Shadow stem + Imp. PW:Shield (Shadowform)", "shadow_y", "shadow"),
    ("Shadow stem - Blackout + 2 Imp. PW:Shield (Shadowform)", "shadow_bl_y", "shadow"),
    ("Shadow stem - Mind Flay + 2 Imp. PW:Shield (Shadowform)", "shadow_mf_y", "shadow"),
    ("Shadow stem + Spiritual Healing (healing)", "shadow_y_heal", "shadow_heal"),
    ("Shadow stem - Blackout + 2 Spiritual Healing (healing)", "shadow_bl_y_heal", "shadow_heal"),
    ("Shadow stem - Mind Flay + 2 Spiritual Healing (healing)", "shadow_mf_y_heal", "shadow_heal"),
]

# Shadowform as an arm: the same build, the form chosen against not.
FORM_ARMS = [
    ("Hybrid 5/0/3: Shadowform vs healing", "hybrid_sf", "hybrid"),
    ("Shadow 2/0/6: Shadowform vs healing", "shadow", "shadow_heal"),
]


def gen_args(arm, game_map, seed_base=0):
    spec, form = ARMS[arm]
    argv = [sys.executable, os.path.join(SCRIPTS, "gen_sweep.py"),
            "--full", "2", "--exclude-double-healer",
            "--affects", CLASS, "--control-cells", "8",
            "--n", str(SEEDS_PER_MAP), "--seed-base", str(seed_base),
            "--extra", json.dumps({"ai_profile": "TeamPlan", "map": game_map}),
            "--label-suffix", game_map]
    if spec is not None:
        argv += ["--build", CLASS + "=" + json.dumps(spec)]
    if form is not None:
        argv += ["--for-class", CLASS, "priest_forms", json.dumps(form)]
    return argv


def gen(outdir, seed_base=0, arms=None):
    """One JSONL per arm. `seed_base` moves the whole sweep onto another seed
    block; `arms` limits it."""
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


def priest_side(buckets):
    clean = set(buckets["clean"])
    return lambda k: "team1" if k in clean else "team2"


def compare(outdir, cache, after_arm, before_arm, only=None):
    """The Priest side's paired delta, after_arm against before_arm, over the
    matches where exactly one side fields a Priest (`only` narrows them)."""
    import paired_sweep as ps
    for arm in (after_arm, before_arm):
        if arm not in cache:
            cache[arm] = read_csv(outdir, arm)
    after, before = cache[after_arm], cache[before_arm]
    keys = [k for k in before if k in after]
    buckets = ps.build_slices(keys, before, {CLASS})
    side = priest_side(buckets)
    scored = buckets["clean"] + buckets["against"]
    if only is not None:
        scored = [k for k in scored if only(k, before, side)]
    s = ps.Slice("priest", scored, before, after, side=side)
    control_same = sum(1 for k in buckets["control"] if before[k] == after[k])
    return s, len(buckets["control"]), control_same


def enemy_has_healer(key, rows, side):
    import paired_sweep as ps
    enemy = rows[key][2] if side(key) == "team1" else rows[key][1]
    return bool(ps.classes(enemy) & HEALERS)


def bonferroni_z(k, alpha=0.05):
    return statistics.NormalDist().inv_cdf(1 - alpha / (2 * k))


def row(label, s, extra=""):
    floor = "%.1f" % s.floor if s.floor is not None else "-"
    return ("| %s | %d | %.1f%% | %.1f%% | %+.1f | %d (+%d/-%d) | %.2f%s | %s |%s"
            % (label, s.n, 100.0 * s.before_wins / s.n, 100.0 * s.after_wins / s.n,
               s.delta, s.to_win + s.to_loss, s.to_win, s.to_loss, s.z,
               " SIG" if s.significant else "", floor, extra))


HEADER = ("| %s | n | before | after | delta (pt) | flips | z | floor (pt) |%s\n"
          "|---|---|---|---|---|---|---|---|%s")


def margin_z(keys, side, a, b, v_plus=None, v0=None, k=0):
    """The node's margin over the yardstick, per match, and its z.

    For each paired match i, scored for `side(i)`:
        x_i = 100 * [(A_i - B_i) - k * (V+_i - V0_i)]
    where A, B, V+ and V0 are 1 when that side won in the arm's row and 0
    otherwise. A is the node's arm, B its yardstick arm, and (V+, V0) the
    stem with and without one yardstick rank: a capstone's second-rank
    allowance (k = 1; k = 0 for a one-rank node). The margin is mean(x) and
    se = sd(x) / sqrt(n). The arms share every match and a stem, so their
    deltas correlate: a quadrature sum of the pair z-scores would be wrong.
    Returns (margin, se, z, n).
    """
    def won(rows, key):
        return 1.0 if rows[key][0] == side(key) else 0.0

    xs = []
    for key in keys:
        x = won(a, key) - won(b, key)
        if k:
            x -= k * (won(v_plus, key) - won(v0, key))
        xs.append(100.0 * x)
    n = len(xs)
    if n < 2:
        return (xs[0] if xs else 0.0), None, 0.0, n
    mean = sum(xs) / n
    se = statistics.stdev(xs) / n ** 0.5
    return mean, se, (abs(mean) / se if se else 0.0), n


def verdict(margin, z, floor, z_bonf):
    """AS-299's line, with the per-match margin z (AS-301 ruling): a node
    dominates when its margin over the yardstick (less a capstone's
    allowance) is past the pair's floor and its margin z survives the
    Bonferroni correction."""
    if floor is not None and margin > floor:
        return "DOMINATES" if z >= z_bonf else "over the line (not after correction)"
    if margin < 0 and z >= 1.96:
        return "below" if z >= z_bonf else "below (not after correction)"
    return "within"


def node_margin(outdir, cache, arm, base, allow_arm, ranks):
    """A node row's margin, its se and z (`margin_z`), and its n."""
    import paired_sweep as ps
    s, _, _ = compare(outdir, cache, arm, base)
    for name in (arm, base, allow_arm, ALLOWANCE_STEM.get(allow_arm)):
        if name and name not in cache:
            cache[name] = read_csv(outdir, name)
    before = cache[base]
    keys = [kk for kk in before if all(kk in cache[x] for x in (arm, base))]
    buckets = ps.build_slices(keys, before, {CLASS})
    side = priest_side(buckets)
    scored = buckets["clean"] + buckets["against"]
    if ranks == 2:
        v_plus, v0 = cache[allow_arm], cache[ALLOWANCE_STEM[allow_arm]]
        scored = [kk for kk in scored if kk in v_plus and kk in v0]
        return s, margin_z(scored, side, cache[arm], before, v_plus, v0, k=1)
    return s, margin_z(scored, side, cache[arm], before)


def node_rows(outdir, cache, z_bonf):
    """Each node comparison as the table prints it: (label, ranks, allow_arm,
    the pair's Slice, margin, margin z, verdict). The verdict reads the
    per-match margin z (`node_margin`), never the pair's McNemar z."""
    out = []
    for label, ranks, arm, base, allow_arm in NODE_ARMS:
        s, (margin, _, z, _) = node_margin(outdir, cache, arm, base, allow_arm, ranks)
        out.append((label, ranks, allow_arm, s, margin, z,
                    verdict(margin, z, s.floor, z_bonf)))
    return out


def table(outdir):
    cache = {}
    z_bonf = bonferroni_z(len(NODE_ARMS))
    print("## References against 0 points (Priest side's win rate)\n")
    print(HEADER % ("reference", "", ""))
    for label, arm in REFERENCES:
        s, _, _ = compare(outdir, cache, arm, "zero")
        print(row(label, s))
    print("\n## Shadowform as an arm (the same build; the form chosen vs not)\n")
    print(HEADER % ("arm", "", ""))
    for label, arm, base in FORM_ARMS:
        s, _, _ = compare(outdir, cache, arm, base)
        print(row(label, s))
    print("\n## One yardstick rank on each stem (7 -> 8 points)\n")
    print(HEADER % ("stem", "", ""))
    stat = {}
    for label, plus, bare in STAT_RANK:
        s, _, _ = compare(outdir, cache, plus, bare)
        stat[plus] = s.delta
        print(row(label, s))
    print("\n## Each node against a yardstick rank in its place "
          "(Bonferroni over %d: z >= %.2f)\n" % (len(NODE_ARMS), z_bonf))
    print(HEADER % ("node", " margin | margin z | verdict |", "---|---|---|"))
    for label, ranks, allow_arm, s, margin, z, word in node_rows(outdir, cache, z_bonf):
        note = " (allowance %+.1f)" % stat[allow_arm] if ranks == 2 else ""
        print(row(label, s, " %+.1f | %.2f | %s%s |" % (margin, z, word, note)))
    print("\n## Each comparison arm against its reference\n")
    print(HEADER % ("comparison arm", "", ""))
    for label, arm, ref in YARDSTICK_ARMS:
        s, _, _ = compare(outdir, cache, arm, ref)
        print(row(label, s))
    print("\n## For scale (no verdict)\n")
    print(HEADER % ("comparison", "", ""))
    for label, arm, base in SCALE_ARMS:
        s, _, _ = compare(outdir, cache, arm, base)
        print(row(label, s))
    print("\n## Each node comparison by the enemy team: a healer, or none\n")
    print("| node | vs a healer: n, delta (pt), z | vs no healer: n, delta (pt), z |")
    print("|---|---|---|")
    for label, _, arm, base, _ in NODE_ARMS:
        cells = []
        for want in (True, False):
            s, _, _ = compare(outdir, cache, arm, base,
                              only=lambda k, rows, side, w=want:
                              enemy_has_healer(k, rows, side) == w)
            cells.append("%d, %+.1f, %.2f" % (s.n, s.delta, s.z))
        print("| %s | %s |" % (label, " | ".join(cells)))
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


def replicate(block1, block2):
    """Every node comparison and every yardstick rank, per block and pooled.
    A node row carries its margin and the margin z its verdict reads."""
    z_bonf = bonferroni_z(len(NODE_ARMS))
    print(HEADER % ("node", " block | margin | margin z | verdict |", "---|---|---|---|"))
    for label, ranks, arm, base, allow_arm in NODE_ARMS:
        for name, dirs in (("block 1", block1), ("block 2", block2),
                           ("pooled", block1 + os.pathsep + block2)):
            s, (margin, _, z, _) = node_margin(dirs, {}, arm, base, allow_arm, ranks)
            print(row(label, s, " %s | %+.1f | %.2f | %s |"
                      % (name, margin, z, verdict(margin, z, s.floor, z_bonf))))
    print()
    print(HEADER % ("comparison", " block |", "---|"))
    rows = [(label, plus, bare) for label, plus, bare in STAT_RANK]
    rows += [(label, arm, base) for label, arm, base in FORM_ARMS]
    for label, arm, base in rows:
        for name, dirs in (("block 1", block1), ("block 2", block2),
                           ("pooled", block1 + os.pathsep + block2)):
            s, _, _ = compare(dirs, {}, arm, base)
            print(row(label, s, " %s |" % name))


# Non-vacuity: what each node does in a match, counted off full match logs.
# The batch CSV carries outcomes only, so `sample` replays a fixed slice of
# each arm's Priest matches through `--headless` and counts these lines.
EVENTS = [
    ("Priests", r"\[EQUIPMENT\] Team \d Priest"),
    ("Flash Heals landed", r"Priest #\d's Flash Heal (CRITICALLY )?heals"),
    ("Flash Heal crits", r"Priest #\d's Flash Heal CRITICALLY heals"),
    ("Mind Blasts begun", r"Priest #\d begins casting Mind Blast"),
    ("Mind Blast hits", r"Priest #\d's Mind Blast (hits|CRITS)"),
    ("CC shortened (UW)", r"Unbreakable Will: \d+% shorter"),
    ("Inner Focus casts", r"Priest #\d casts Inner Focus"),
    ("shield reflects (RS)", r"\[REFLECT\].*Power Word: Shield"),
    ("Power Infusion casts", r"Priest #\d casts Power Infusion"),
    ("Pain Suppression casts", r"Priest #\d casts Pain Suppression"),
    ("Inspiration procs", r"procs Inspiration"),
    ("Blessed Recovery procs", r"procs Blessed Recovery"),
    ("Holy Nova casts", r"Priest #\d casts Holy Nova"),
    ("Holy Nova hits", r"Priest #\d's Holy Nova (hits|CRITS)"),
    ("Holy Nova heals", r"Priest #\d's Holy Nova (CRITICALLY )?heals"),
    ("Guardian Spirit casts", r"Priest #\d casts Guardian Spirit"),
    ("Guardian Spirit saves", r"\[SAVED\] Guardian Spirit prevents a killing blow"),
    ("Blackout stuns", r"procs Blackout"),
    ("Mind Flay channels", r"Priest #\d begins channeling Mind Flay"),
    ("VE casts", r"Priest #\d casts Vampiric Embrace"),
    ("VE heals", r"Priest #\d's Vampiric Embrace heals"),
    ("Silence casts", r"Priest #\d casts Silence"),
    ("Shadowform casts", r"Priest #\d casts Shadowform"),
]
SAMPLE_PER_ARM = 60
# Guardian Spirit's save rate is a card deliverable, so its arm is sampled
# deeper.
SAMPLE_DEEP = {"holy": 600}


def sample_configs(path, k=SAMPLE_PER_ARM):
    """Every n-th config of an arm that fields a Priest: a fixed slice, the
    same configs (up to the build) in every arm."""
    with open(path, encoding="utf-8") as f:
        lines = [ln for ln in f if CLASS in json.loads(ln)["label"]]
    step = max(1, len(lines) // k)
    return lines[::step][:k]


def replay_log(binary, config_line, workdir, index):
    """One config replayed through `--headless`; its full match log."""
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
    text = replay_log(binary, config_line, workdir, index)
    return [len(re.findall(pattern, text)) for _, pattern in EVENTS]


def sample(outdir, binary, jobs=5, arms=None):
    from concurrent.futures import ThreadPoolExecutor
    print("| arm | " + " | ".join(name for name, _ in EVENTS) + " |")
    print("|---|" + "---|" * len(EVENTS))
    for arm in arms or ARMS:
        work = os.path.join(outdir, ".sample-" + arm)
        os.makedirs(work, exist_ok=True)
        configs = sample_configs(os.path.join(outdir, arm + ".jsonl"),
                                 SAMPLE_DEEP.get(arm, SAMPLE_PER_ARM))
        with ThreadPoolExecutor(max_workers=jobs) as pool:
            counts = list(pool.map(lambda a: run_one(binary, a[1], work, a[0]),
                                   enumerate(configs)))
        os.rmdir(work)
        totals = [sum(c[i] for c in counts) for i in range(len(EVENTS))]
        print("| %s (%d) | %s |" % (arm, len(configs), " | ".join(str(t) for t in totals)),
              flush=True)


# The Mind Flay mechanism: each pair is an arm holding Mind Flay and the arm
# with its point moved onto the yardstick, in one role.
MECHANISM_PAIRS = [
    ("healing", "shadow_y_heal", "shadow_mf_y_heal"),
    ("Shadowform", "shadow_y", "shadow_mf_y"),
]
MECHANISM_PER_ARM = 120
MF_CHANNEL_SECS = 3.0

_TIME = r"\[ *([0-9.]+)s\] "
_PRIEST = r"Team \d Priest #\d"


CHANNEL_ENDS = ("full", "interrupt", "crowd control or silence", "death", "own cast",
                "match end")
_EMPTY = {"channels": 0, "channel_secs": 0.0, "ended": {}, "with_next_cast": 0,
          "full_with_next_cast": 0, "mind_blast_hits": 0, "damage": {}, "healing": 0,
          "damage_taken": 0}


def channel_end(lines, start, who, target):
    """How a Mind Flay channel begun at `start` by `who` on `target` ended,
    and when: at the first of an interrupt of it ("interrupts <who>'s Mind
    Flay"), crowd control or a silence cutting it ("<who>'s Mind Flay
    interrupted by"), the death of its target or its caster, its caster's
    next cast, or its full 3s. A log that ends first ends it at its last
    line ("match end"). Returns (reason, seconds it ran)."""
    who_re, target_re = re.escape(who), re.escape(target)
    full = start + MF_CHANNEL_SECS
    last = start
    for line in lines:
        m = re.match(_TIME, line)
        if not m:
            continue
        t = float(m.group(1))
        last = t
        if re.search(r"interrupts " + who_re + r"'s Mind Flay", line):
            reason = "interrupt"
        elif re.search(who_re + r"'s Mind Flay interrupted by", line):
            reason = "crowd control or silence"
        elif re.search(r"\[DEATH\] (?:" + target_re + "|" + who_re + r") has been eliminated",
                       line):
            reason = "death"
        elif re.match(_TIME + r"\[CAST\] " + who_re + r" (?:casts|begins)", line):
            # its next cast, a frame after a full channel, is the full channel
            if t >= full - 0.1:
                return "full", MF_CHANNEL_SECS
            reason = "own cast"
        elif t >= full:
            return "full", MF_CHANNEL_SECS
        else:
            continue
        if t >= full:
            return "full", MF_CHANNEL_SECS
        return reason, t - start
    return "match end", min(last, full) - start


def mechanism_counts(text):
    """What a Priest's Mind Flay did in one match log, and what it cost.

    - `channels`: Mind Flay channels begun by a Priest;
    - `ended`: how many ended each way (`channel_end`): ran its `full` 3s, or
      was cut by an interrupt, crowd control or a silence, a death, its
      caster's own cast, or the match's end;
    - `channel_secs`: the seconds the channels ran, each to its real end;
    - `with_next_cast` / `full_with_next_cast`: the channels its Priest cast
      again after, and how many of those ran their full 3s;
    - `mind_blast_hits`; `damage`: a Priest's damage by spell (Mind Blast,
      Mind Flay, Shadow Word: Pain);
    - `healing`: a Priest's heals, by any spell;
    - `damage_taken`: direct damage on a Priest -- hits, crits and channelled
      ticks ("ticks on"). Periodic ticks ("ticks for N damage on") are not
      counted.
    """
    out = {k: (dict(v) if isinstance(v, dict) else v) for k, v in _EMPTY.items()}
    lines = text.splitlines()
    for i, line in enumerate(lines):
        m = re.search(r"\[HEAL\] (?:\[[A-Z]+\] )?" + _PRIEST
                      + r"'s [A-Za-z :]+ (?:CRITICALLY )?heals Team \d \w+ #\d for (\d+)", line)
        if m:
            out["healing"] += int(m.group(1))
        if "[DMG]" in line:
            m = re.search(_PRIEST + r"'s (Mind Blast|Mind Flay|Shadow Word: Pain) "
                          r"(?:hits|CRITS|ticks)\b.*? for (\d+) damage", line)
            if m:
                out["damage"][m.group(1)] = out["damage"].get(m.group(1), 0) + int(m.group(2))
                if m.group(1) == "Mind Blast":
                    out["mind_blast_hits"] += 1
            m = re.search(r" (?:hits|CRITS|ticks on) " + _PRIEST + r" for (\d+) damage", line)
            if m:
                out["damage_taken"] += int(m.group(1))
        m = re.match(_TIME + r"\[CAST\] (" + _PRIEST + r") begins channeling Mind Flay on "
                     r"(Team \d \w+ #\d)", line)
        if not m:
            continue
        start, who, target = float(m.group(1)), m.group(2), m.group(3)
        out["channels"] += 1
        reason, secs = channel_end(lines[i + 1:], start, who, target)
        out["ended"][reason] = out["ended"].get(reason, 0) + 1
        out["channel_secs"] += secs
        later = lines[i + 1:]
        if any(re.match(_TIME + r"\[CAST\] " + re.escape(who) + r" (?:casts|begins)", x)
               for x in later):
            out["with_next_cast"] += 1
            if reason == "full":
                out["full_with_next_cast"] += 1
    return out


def merge_counts(counts):
    total = {k: (dict(v) if isinstance(v, dict) else v) for k, v in _EMPTY.items()}
    for c in counts:
        for key in ("channels", "channel_secs", "with_next_cast", "full_with_next_cast",
                    "mind_blast_hits", "healing", "damage_taken"):
            total[key] += c[key]
        for key in ("damage", "ended"):
            for name, amount in c[key].items():
                total[key][name] = total[key].get(name, 0) + amount
    return total


def mechanism_row(role, arm, n, c):
    dmg, ended = c["damage"], c["ended"]
    return ("| %s | %s (%d) | %d | %.1f | %s | %d of %d | %d | %.1fk | %.1fk | %.1fk | %.1fk | %.1fk |"
            % (role, arm, n, c["channels"], c["channel_secs"] / n,
               " / ".join(str(ended.get(r, 0)) for r in CHANNEL_ENDS),
               c["full_with_next_cast"], c["with_next_cast"], c["mind_blast_hits"],
               dmg.get("Mind Blast", 0) / 1000, dmg.get("Mind Flay", 0) / 1000,
               dmg.get("Shadow Word: Pain", 0) / 1000, c["healing"] / 1000,
               c["damage_taken"] / 1000))


def mechanism(outdir, binary, jobs=5):
    from concurrent.futures import ThreadPoolExecutor
    print("| role | arm (matches) | Mind Flay channels | channelling s a match | ended: "
          + " / ".join(CHANNEL_ENDS) + " | full, of those cast again after | Mind Blast hits "
          "| Mind Blast dmg | Mind Flay dmg | SW:P dmg | Priest healing | Priest direct "
          "damage taken |")
    print("|---|---|---|---|---|---|---|---|---|---|---|---|---|")
    for role, with_mf, without in MECHANISM_PAIRS:
        for arm in (with_mf, without):
            work = os.path.join(outdir, ".mechanism-" + arm)
            os.makedirs(work, exist_ok=True)
            configs = sample_configs(os.path.join(outdir, arm + ".jsonl"), MECHANISM_PER_ARM)
            with ThreadPoolExecutor(max_workers=jobs) as pool:
                counts = list(pool.map(lambda a: mechanism_counts(
                    replay_log(binary, a[1], work, a[0])), enumerate(configs)))
            os.rmdir(work)
            print(mechanism_row(role, arm, len(configs), merge_counts(counts)), flush=True)


def main(argv=None):
    argv = sys.argv[1:] if argv is None else argv
    if len(argv) >= 3 and argv[0] == "sample":
        sample(argv[1], argv[2], arms=argv[3:] or None)
        return 0
    if len(argv) == 3 and argv[0] == "mechanism":
        mechanism(argv[1], argv[2])
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
