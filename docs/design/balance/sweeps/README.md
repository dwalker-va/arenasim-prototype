# Committed sweep inputs

One JSONL per balance sweep whose findings are published in
`docs/design/balance/`. These are the **exact** files fed to
`arenasim --batch`, so a published result can be re-run rather than
re-derived — including the map, which the batch runner's CSV does not carry.

A findings doc that cites a sweep should name its input file here.

## `2026-09-13-as54-frost-armor-chill.jsonl`

Behind `2026-09-13-frost-armor-one-debuff-findings.md` (card AS-54). 5,150
configs: a 2v2 arm and a 3v3 arm concatenated, every line pinned to
`BasicArena`. Regenerate with:

```bash
scripts/gen_sweep.py --t1 'Mage+{p}' --t2-size 2 --n 20 --seed-base 0 \
  --exclude-double-healer --extra '{"map":"BasicArena"}'          # 3,500
scripts/gen_sweep.py --t1 'Mage+Priest+Warrior' --t2-size 3 --n 30 \
  --seed-base 0 --extra '{"map":"BasicArena"}'                    # 1,650
```

Run each arm's binary over the whole file, then compare them:

```bash
<binary> --batch docs/design/balance/sweeps/2026-09-13-as54-frost-armor-chill.jsonl \
  --out <before|after>.csv --jobs 6 --trace-mode off

sweeps/2026-09-13-as54-paired.py before.csv after.csv
```

`2026-09-13-as54-paired.py` is the analysis that produced the published tables:
McNemar on the flip counts, Wilson intervals for the level, and the slices the
change can physically reach. It lives beside its input rather than in
`scripts/` because its slicing knows what a Frost Armor chill is.

Its generic half has since been promoted to `scripts/paired_sweep.py`, which
does the control / CLEAN / AGAINST / MIRRORED split off a `--affects` class
list and prints the resolution floor and the slice count — see
`docs/design/balance/sweep-tiers.md`. Reach for that first; write a script
here only when the slicing needs to know something a class name cannot say.

## `2026-09-20-as104-slot-diagonal.jsonl`, `2026-09-20-as104-slot-swap.jsonl`

Behind `2026-09-20-as104-slot-symmetry-findings.md` (card AS-104). A ONE-ARM
probe, so there is no before/after: it asks whether team slot 1 is advantaged
at all, which is the necessary condition behind the team-1 mirrored-slice
question. 5,640 configs, every line pinned to `BasicArena`. Regenerate with
`2026-09-20-as104-make-probe.py`, and analyse with:

```bash
<binary> --batch docs/design/balance/sweeps/2026-09-20-as104-slot-diagonal.jsonl \
  --out diag.csv --jobs 6 --trace-mode off       # and again for -slot-swap
# `--jobs` keys off how many agents are sweeping right now; 6 on a quiet box.
# See "Sizing --jobs" in ../sweep-tiers.md — 16 is too many.
docs/design/balance/sweeps/2026-09-20-as104-slot-symmetry.py diag.csv swap.csv
```

- `-slot-diagonal` — identical comps on both sides (25 2v2 teams + 8 1v1
  self-mirrors, 80 seeds). No comp-strength confound is possible.
- `-slot-swap` — every ordered pair of 25 distinct 2v2 teams, 5 seeds. Closed
  under swapping the sides, so comp strength cancels across each pair; the
  script checks that closure rather than assuming it.

## `2026-09-27-as115-warrior.jsonl`, `2026-09-27-as115-staff-arm.jsonl`

Behind `2026-09-27-as115-two-hander-budget.md` (card AS-115). Two directional
2v2 sweeps, 10 seeds per cell:

- `-warrior` — 3,090 configs, run once on `main` and once on the change. It was
  generated with
  `scripts/gen_sweep.py --full 2 --exclude-double-healer --affects Warrior --control-cells 8 --n 10`.
- `-staff-arm` — 5,520 configs, both arms on ONE binary. This is the staff
  arm: every Mage, Priest and Warlock carries a `team{1,2}_equipment` override
  of `MainHand: CrescentStaff`. The pair arm is the same file without the two
  `*_equipment` keys. It is also exactly the output of
  `scripts/gen_sweep.py --full 2 --exclude-double-healer --affects Mage,Priest,Warlock --control-cells 8 --n 10`.

Analyse each pair of CSVs with `scripts/paired_sweep.py ... --tier directional`.
Get the per-class tables with `2026-09-27-as115-slices.py` (`--by partner` for
the Warrior sweep, `--by holder` for the staff sweep).

## `2026-09-28-as167-weapon-speed.jsonl`

Behind `2026-09-28-as167-weapon-speed.md` (card AS-167). 6,250 configs: the
whole 2v2 matrix without double healers, 10 seeds per cell, run once on
`main` @ `17cb9f0` and once on the change. It was generated with
`scripts/gen_sweep.py --full 2 --exclude-double-healer --n 10`, with no
`--affects`, because every class wields a weapon whose speed moved and no cell
is out of reach. For the same reason there is no control group, and
`scripts/paired_sweep.py` has nothing to put in one. Get the run-wide
non-vacuity and the per-class tables with `2026-09-28-as167-slices.py
before.csv after.csv`. `2026-09-28-as167-swings.py <logs>` counts swings
landed per hand from match logs (the doc's non-vacuity table, over every 60th
line of this file).

## `2026-09-28-as167-warrior-shaman-cells.jsonl`

Behind the Windfury knockout in `2026-09-28-as167-weapon-speed.md`: the 36
Warrior+Shaman cells of `2026-09-28-as167-weapon-speed.jsonl`, 10 seeds each
(360 configs). Each arm's binary was built with Windfury Totem's magnitude set
to `0.0` in `class_ai/shaman.rs` (`TotemElement::Air`, `0.12` -> `0.0`). The
roll still happens, so the RNG draw order is unchanged. The results are
`2026-09-28-as167_windfury_off_{before_17cb9f0,after}.csv`.

## `2026-09-27-as125-hunter-directional.jsonl`, `-directional-kt0.jsonl` and `-hunter-1v1.jsonl`

Behind `2026-09-27-as125-freezing-trap-dispeller.md` (card AS-125). The
DIRECTIONAL tier for a Hunter change, on the default map (`BasicArena`; the
lines carry no `map` field): 2v2 is 301 reachable cells plus 8 control cells,
10 seeds, 3,090 configs; 1v1 is 15 reachable plus 8 control cells, 50 seeds,
1,150 configs. Regenerate with:

```bash
scripts/gen_sweep.py --full 2 --exclude-double-healer \
  --affects Hunter --control-cells 8 --n 10
scripts/gen_sweep.py --full 1 --affects Hunter --control-cells 8 --n 50
```

`-directional-kt0.jsonl` is the 2v2 file with `team1_kill_target` and
`team2_kill_target` set to 0 on every line — the graphical client's default,
under which the Hunter's healer trap has to survive a kill-target re-force.

The three attribution arms are patches against the branch that
`git apply` cleanly: `2026-09-27-as125-arm-trade-trap.patch` (the trade trap
back), `-arm-trap-setup-off.patch` (`movement.ron`'s Hunter `trap_setup` at
0.0) and `-arm-opener-ignores-hidden.patch` (Aimed Shot may begin while an
enemy is hidden).

`2026-09-27-as125-by-enemy.py <before.csv> <after.csv>` turns either pair of
result CSVs into the per-enemy-comp table (Hunter-side win rate, flips, McNemar
z per enemy comp), because a pooled bucket hides comps that move in opposite
directions.

Beside them, the trap-mechanism instrument the findings doc's per-trap tables
come from: `2026-09-27-as125-trapmech.py [--kill-target N[,M]] <binary> <outdir> <jobs>
[seeds] [comps]` runs 27 trap-relevant comps traced — every melee + healer shape the
healer trap reaches, in both slot orders the directional sweep fields for
Rogue + Priest — pairs every Freezing Trap's intended victim (trace
`target_id`, empty for a throw decided on an enemy the Hunter could not see)
with whom it sprang on, how it ended (a Divine Shield that lifts it counts as a
removal) and how long its victim was held (match log), how many decisions the
Hunter still aimed at a victim it held fire on, and records each match's
winner and first throw;
`2026-09-27-as125-trapsumm.py <before_outdir> [<after_outdir>]` tabulates them
per comp, including the decided-victim vs springer match rate and the traps
that caught a healer. The committed `2026-09-27-as125_trap_{events,matches}.csv`
are the four runs' `traps.csv` / `matches.csv` stacked, with `arm` and
`kill_target` columns in front. Each match runs in its own directory
with a link to the measured binary's own `assets/`, because single-match
traces are stamped to the second and would collide in parallel. The binary
must sit in a checkout (a `Cargo.toml` above it): a binary that classifies as
installed writes its traces to the per-user data directory instead, where
they collide and the victim column comes back empty.

## `2026-10-10-as164-*` — the 0.7 milestone sweep

Behind `2026-10-10-as164-milestone-sweep.md` (card AS-164), measured at `ea63f4f`.
The three canonical matrices are plain `gen_sweep.py --full {1,2,3} --n {100,100,50}`
output, so their JSONL is not committed; the results are committed gzipped as
`../canonical_*.csv.gz`. `2026-10-10-as164-gen.py <outdir>` writes the rest; the
committed copies carry the prefix:

- `h2h.jsonl` — TeamPlan head-to-head, PillaredArena, seeds 1-300 (CSV:
  `../2026-10-10-as164-teamplan-headtohead-n300.csv`).
- `bisect.jsonl` (its `hunter` and `wmp` cells, seeds 1-200) and `repro0806.jsonl`
  (the 2026-08-06 cells, seeds 1-100) — the TeamPlan bisect, run on each of
  `1ade748 1323a25 b175653 e6c0e93 147e95b` built with
  `2026-10-10-as164-as235-backport.patch` applied (the AS-235 headless-executor
  fix; it changes speed, not output — `147e95b` built without it reproduces the
  committed bisect cells 400/400 on side_won).
  `2026-10-10-as164-bisect-table.py` folds those runs and the head-to-head CSV
  into `../2026-10-10-as164-teamplan-bisect.csv`; the `1ade748` reproduction is
  `../2026-10-10-as164-teamplan-repro-1ade748.csv`.
- `mech_basic.jsonl`, `mech_nagrand.jsonl`, `mech_hunter.jsonl`,
  `mech_duels.jsonl`, `mech_h2h_hunter.jsonl` — the observed-run panels, run
  through `tests/milestone_mechanisms.rs`
  (`MECH_IN=… MECH_OUT=… MECH_LOGS=… cargo test --release --test milestone_mechanisms -- --ignored`);
  their per-match rows are the `.rows.jsonl.gz` files.
- `as234-paired.jsonl` — `gen_sweep.py --full 2 --exclude-double-healer --affects Paladin
  --control-cells 8 --n 50`, run on `e4c01dc` and `ea63f4f`
  (CSVs: `../2026-10-10-as164-as234-{before-e4c01dc,after-ea63f4f}.csv`).

Analysis: `2026-10-10-as164-analyze.py` (standings, healer grid, interrupts,
draws, duels, mirrors, head-to-head, bisect, comps, Hunter),
`2026-10-10-as164-mechanisms.py` (economy, Druid, Hunter traps, duels, the
head-to-head Hunter cell) and `2026-10-10-as164-as234-slices.py`. Each reads the
committed `.gz` files directly; the findings doc maps every figure to its command.

## `2026-10-12-as285-directional.jsonl.gz`

Behind `2026-10-12-as285-held-cc.md` (card AS-285). 18,000 configs: two
`gen_sweep.py --full 2 --exclude-double-healer --affects
Warlock,Rogue,Paladin,Hunter,Mage,Druid --control-cells 8 --n 10` runs, one per
map (`--extra '{"map":"BasicArena"}' --label-suffix BasicArena`, then
PillaredArena), concatenated. Gunzip it, run each arm's binary over it with
`--jobs 4 --trace-mode off`, then `scripts/paired_sweep.py` and
`2026-10-12-as285-oriented.py` on the two CSVs. `2026-10-12-as285-attribute.py`
replays the moved matches; run each arm's binary in place, in its own checkout.

## `2026-10-12-as279-{basicarena,pillared,twinpillars}.jsonl.gz`

Behind `../2026-10-12-as279-hoj-sight.md` (card AS-279). One
`gen_sweep.py --full 2 --exclude-double-healer --affects Paladin
--control-cells 8` run per map (`--extra '{"map":"<Map>"}'`): `--n 10` on
PillaredArena and TwinPillars (2,830 configs each), `--n 5` on BasicArena
(1,415, the byte-identity check). Gunzip, run each arm's binary over each with
`--jobs 4 --trace-mode off`; the CSVs are `2026-10-12-as279-<map>-<arm>.csv.gz`.
Analysis: `scripts/paired_sweep.py --affects Paladin --tier directional` and
`2026-10-12-as279-oriented.py` (scored from the Paladin's side). The
casts-through-a-pillar counts come from arms instrumented by
`2026-10-12-as279-hoj-sight-counter.py`.
