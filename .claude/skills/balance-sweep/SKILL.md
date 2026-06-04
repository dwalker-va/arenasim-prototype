---
name: balance-sweep
description: Measure class/comp balance and assess balance changes via fast parallel match sweeps. Use when asked to evaluate how a class performs, whether a change helped, or to compare team compositions across 1v1/2v2/3v3 — especially before/after an ability or item tune. Encodes the methodology (300s cap, clean slices, confidence intervals, 2v2/3v3 over 1v1).
---

# Balance Sweep

Measure class balance and assess balance changes by running many seeded matches
in parallel and aggregating winrates. This is recurring work: pick a focus
class/comp, sweep it against opponents, and read the result with the right
statistical and methodological guardrails.

## The harness: `--batch`

`arenasim --batch <jsonl> --out <csv> [--jobs N]` runs one match per line of a
JSONL file (each line a `HeadlessMatchConfig`) across all cores, writing one CSV
row per match: `label,team1,team2,seed,winner,end_reason,duration_secs`.

It is **fast** (~100 matches/sec; a 2400-match 2v2 sweep takes ~25s) and
**internally deterministic** (same input → same output, regardless of `--jobs`).
Both `abilities.ron` and `items.ron` are runtime assets, so a balance change is
just an edit to those files — **no rebuild** between baseline and variant runs.

## Three-step workflow

```bash
# 1. Generate the matchup set
python3 scripts/gen_sweep.py --t1 'Hunter+{p}' --t2-size 2 --n 20 \
  --exclude-double-healer > /tmp/sweep.jsonl

# 2. Run it
cargo build --release            # only if code changed; not for .ron edits
target/release/arenasim --batch /tmp/sweep.jsonl --out /tmp/sweep.csv --jobs 16

# 3. Aggregate (winrates + 95% confidence intervals)
python3 scripts/agg_sweep.py /tmp/sweep.csv
```

## Assessing a change (before/after)

```bash
python3 scripts/gen_sweep.py --t1 'Hunter+{p}' --t2-size 2 --n 20 \
  --exclude-double-healer > /tmp/sweep.jsonl

# Baseline (current assets)
target/release/arenasim --batch /tmp/sweep.jsonl --out /tmp/before.csv --jobs 16

# Make the change — edit assets/config/abilities.ron or items.ron (no rebuild)
# ... e.g. lower Frostbolt damage_coefficient 0.8 -> 0.6 ...

# After
target/release/arenasim --batch /tmp/sweep.jsonl --out /tmp/after.csv --jobs 16

# Compare; --compare flags only matchups whose CIs do not overlap
python3 scripts/agg_sweep.py /tmp/after.csv --compare /tmp/before.csv
```

Revert the `.ron` edit when done (`git checkout -- assets/config/...`) unless you
intend to ship it.

## Methodology — read results with these guardrails

These are hard-won; ignoring them produces confident-but-wrong conclusions.

1. **Balance around 2v2/3v3, not 1v1.** 1v1 has kiting asymmetries and no team
   dynamics (a lone melee gets perma-kited by a ranged class — that is fine, not
   a balance bug). Use 1v1 only as a diagnostic signal. Real balance lives in
   2v2 and 3v3.

2. **Keep the cap at 300s.** Healer attrition resolves around ~200-240s; a
   shorter cap silently turns healer wins into draws and makes healers look
   weak. At 300s only ~2/4900 1v1 matches fail to resolve. `gen_sweep.py`
   defaults to 300 — do not lower it without a reason.

3. **Always compute clean slices, not just the aggregate.** An aggregate can
   hide the signal. A global Frostbolt nerf once looked like a 2v2 "wash"
   (37.7% → 37.0% overall) until the slice "enemy has a Mage, ally does not"
   revealed it doubled the target matchup (6.5% → 15.2%). Use
   `agg_sweep.py --include/--exclude <regex>` on labels to carve slices.

4. **A global single-class change is symmetric.** Nerfing Mage also weakens the
   *allied* Mage when the focus team includes one, washing out the net effect.
   Treat "buff class X" and "nerf class Y" as independent levers; never use a
   global Y-nerf to raise X's overall winrate and expect the aggregate to move.

5. **Respect confidence intervals.** `agg_sweep.py` prints a Wilson 95%
   interval. A 37.0% +/-3 vs 37.7% +/-3 is *noise*, not a finding. N=5 is a
   scout; use N>=20 (2v2/3v3) or N=100 (1v1) for conclusions, and bump N on
   close matchups until the intervals separate. `--compare` only flags a
   matchup as MOVED when the before/after intervals do not overlap.

6. **The batch harness is the canonical engine.** It is internally
   deterministic, but its absolute winrates differ by a few points from the
   older multithreaded `--matrix` numbers (a known small execution-order
   sensitivity). Always compare batch-vs-batch. Do not mix batch results with
   pre-2026-06-04 `--matrix` baselines.

## Strategy variables (pets, openers, curses, shouts, auras, armors)

The matchup space is not just classes — it includes per-class strategy choices.
Sweep one variable at a time, holding others at default, and keep variants
distinct with `--label-suffix`:

```bash
# Does pet choice change Hunter's 2v2 outcomes?
for pet in Spider Boar Bird; do
  python3 scripts/gen_sweep.py --t1 'Hunter+{p}' --t2-size 2 --n 20 \
    --exclude-double-healer \
    --extra "{\"team1_hunter_pet_types\":[\"$pet\"]}" --label-suffix "$pet"
done > /tmp/pet_sweep.jsonl
target/release/arenasim --batch /tmp/pet_sweep.jsonl --out /tmp/pet.csv --jobs 16
# Group by the pet suffix to compare tiers
python3 scripts/agg_sweep.py /tmp/pet.csv --group '#([A-Za-z]+)$' --overall-only
```

`--extra` accepts any `HeadlessMatchConfig` field: `team1_hunter_pet_types`,
`team1_rogue_openers`, `team1_warlock_curse_prefs`, `team1_warrior_shouts`,
`team1_mage_armors`, `team1_paladin_auras`, `team1_equipment`, etc.

**Scope, don't brute-force.** The full cross product (every strategy var on both
teams) is astronomical. Sweep only the focus class's variables; hold opponents
at default. Use a cheap low-N pass to find close/interesting cells, then re-run
just those at high N.

## Scripts

- `scripts/gen_sweep.py` — emit batch JSONL. `--t1` template (`{p}` expands over
  all classes), `--t2-size`, `--n`, `--cap` (default 300), `--exclude-double-healer`,
  `--extra` (strategy vars), `--label-suffix`.
- `scripts/agg_sweep.py` — aggregate a batch CSV: overall + per-matchup winrate
  with Wilson 95% CIs. `--include`/`--exclude` (clean slices by label regex),
  `--group <regex>` (sub-aggregate by a captured key), `--compare <baseline.csv>`
  (before/after deltas, flags non-overlapping CIs), `--overall-only`.

## Canonical baselines

Current-state references (batch harness, 300s cap), regenerate after any change
that ships:

- `design-docs/balance/2026-06-04_canonical_1v1_n100_300s.csv`
- `design-docs/balance/2026-06-04_canonical_2v2_n20_300s.csv`

For deeper context on a worked investigation (Hunter/Mage), see
`design-docs/balance/2026-06-04-hunter-mage-balance-findings.md`.
