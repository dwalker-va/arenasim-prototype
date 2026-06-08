# Rogue Energy Pooling — Fix Assessment (2026-06-07)

**Commit:** `7adf7f3` · **Sweeps:** batch harness, 300s cap, before/after on identical match sets (before = the parent-commit binary `7adf7f3^`). Per-match CSVs not retained — reproducible via `gen_sweep.py --t1 Rogue` / `--t1 'Rogue+{p}'` against the two binaries; the aggregated deltas below are the record.

## The bug

The U4.1 snapshot casting-visibility fix removed the Rogue's accidental idle
ticks (its target was invisible mid-cast pre-fix). Kidney Shot usage vs
casters collapsed 86/100 → 0/100: Sinister Strike (40 energy) re-drained the
pool every tick, so Kidney Shot (60) was never affordable. Reported from
manual play ("the rogue not being able to kidney shot the priest seems like
a bug") — confirmed via decision-trace diff (45,803 InsufficientResource
rejections in one matrix cell).

## The fix

`decide_rogue_action` pools energy when Kidney Shot's only blocker is energy
(cooldown classified before resource ⇒ CD ready; Kidney/SS share melee range
⇒ suppressing SS out of range is free). Held ticks are trace-visible as
SinisterStrike rejections noted "pooling energy for Kidney Shot".
Regression test: `rogue_pools_energy_and_lands_kidney_shot`.

## Measured impact (before → after, identical seeds)

**1v1 (N=100/cell):** surgical — Rogue v Priest 45% → **97%** (+52, the stun
restored; pre-U4 era was 100%). Every other cell **+0.0** (byte-identical).

**2v2 (N=20/cell, Rogue+ally vs all non-double-healer pairs):**
- vs enemy-has-Priest slice: 39.3% ±3.9 → **52.3% ±4.0** (CIs separate)
- vs enemy-no-healer slice: 41.8% → 39.2% (CIs overlap — noise)
- Overall: 40.8% → 42.1% (wash; effect is slice-concentrated, per methodology)
- Largest MOVED cells: Rogue+Paladin vs Warrior+Priest 5→95, Rogue+Warrior vs
  Priest+Warlock 0→90, Rogue+Priest vs Priest+Warlock 25→100. The two
  negative MOVED cells (Rogue+Priest vs Mage+Rogue / vs Rogue+Paladin,
  65→0) are mirrors of the same effect: the enemy Rogue now stuns our Priest.

## Probe adjustment

The working stun invalidated the statue probe's absolute path threshold (a
stunned Priest can't move, and it dies faster). The probe now (a) excludes
hard-CC windows from the measurement and (b) asserts mobility as units per
un-CC'd second (statue band ~0.65, healthy ~2.8–3.3, threshold 1.5).

## Follow-up

Healer 1v1 helplessness vs stunlock (Rogue v Priest 97%) is a class-kit gap
(no anti-stun tool), not a movement or pooling issue — candidate future kit
work (PvP trinket / freedom analogs). Canonical baselines
(`design-docs/balance/canonical_*`) should be regenerated when this branch
merges — they predate the healer-movement slice entirely.
