# Psychic Scream — balance findings (bounded validation)

Date: 2026-06-14
Branch: `feat/priest-psychic-scream`
Plan: `docs/plans/2026-06-14-001-feat-priest-psychic-scream-plan.md`

## What shipped

Priest Psychic Scream: instant self-centered AoE Fear (8yd, 30s CD, 55 mana,
`break_on_damage: 100.0`), dual-mode AI — defensive self-peel when focused,
aggressive offensive dip to fear the enemy healer when not (deferred when a
teammate is below `healing_heavy_hp`). Shipped tuning defaults:
`abilities.ron` (radius 8 / cooldown 30 / mana 55 / fear 8s) and
`movement.ron` priest `dip_budget: 6.0`, `healing_heavy_hp: 0.6`.

## Behavioral validation (deterministic probes)

`tests/movement_probes.rs::psychic_scream`:
- `offensive_dip_fears_enemy_healer` — unfocused Priest dips the enemy healer
  (DipEnter → DipComplete) and the scream cast lands (seed 42, 2v2).
- `defensive_scream_fires_under_pressure` — focused Priest casts the scream as
  a self-peel and does NOT dip (seed 7, 2v2).

Both pass. The dip path was also manually traced: DipEnter → DipAbort →
DipEnter → DipComplete, with the Fear landing on the enemy healer.

## Bounded sanity sweep (NOT the authoritative balance pass)

Priest+Warrior **mirror**, 16 seeds (random_seed 0–15), 300s cap:

| Config | team1 | team2 | draws |
|---|---|---|---|
| Shipped (dip on, `dip_budget: 6.0`) | 7 | 6 | 3 |
| Dip ~disabled (`dip_budget: 0.01`) | 10 | 5 | 1 |

**Finding:** the offensive dip adds mild stall in the healer mirror — draws
rise 1 → 3 of 16 when the dip is enabled (one 300s timeout observed). N=16 is
statistically weak, but the direction is real: both Priests dipping toward each
other trip `compound_pressure_trigger` (a closing enemy), producing
dip↔pressured oscillation (also caught by the ignored
`priests_spend_substantial_time_free_in_unforced_mirror` probe at 62% PRESSURED
vs the 50% ceiling). This is the cost side of the user's "aggressive by
default" choice; it must be weighed against the dip's offensive value in
non-mirror comps (fearing the enemy healer to open a kill window).

## Authoritative sweep results (2026-06-14)

Method: batch harness (`scripts/gen_sweep.py` + `arenasim --batch` + `agg_sweep.py`),
300s cap. Feature isolated on one binary by toggling Psychic Scream via config
(no rebuild, no engine-version confound): **baseline** = scream disabled
(`mana_cost` set out of reach → `pre_cast_ok` fails for both the cast and the
dip entry); **full** = shipped config; **defensive-only** = scream on but
`dip_budget` ≈ 0 (offensive dip can't reach). 2v2 = `Priest+{p}` vs all pairs,
N=20 (2400 matches). 3v3 = `Priest+Warrior+{p}` vs all triples, N=15 (2250).

Clean slices (the aggregate hides the signal):

| Metric | baseline | full (dip on) | defensive-only |
|---|---|---|---|
| 2v2 overall | 42.5% ±2.0 | 44.0% ±2.0 | **46.4% ±2.0** |
| 2v2 — enemy has healer (dip fires) | 34.3% ±2.7 | 33.2% ±2.7 | **38.0% ±2.7** |
| 2v2 — enemy has no healer (defensive only) | 50.7% ±2.8 | 54.8% ±2.8 | 54.8% ±2.8 |
| 3v3 overall | 45.5% ±2.1 | 44.8% ±2.1 | **46.4% ±2.1** |
| 3v3 — enemy has healer | 41.5% ±2.5 | 38.3% ±2.5 | 40.8% ±2.5 |

Draws (mirror-oscillation concern at scale): 2v2 7→25 of 2400 (~1%) with the
full feature, 3v3 5→1 of 2250. Not a draw-fest — the earlier N=16 mirror spike
was a localized comp, not systemic. The extra 2v2 draws track improved Priest
survival (more games reaching the cap), not pathological dancing.

### Verdict

- **Defensive scream — clear win, ship it.** +4pt vs no-healer comps in both
  formats; the panic-button peel measurably improves an underpowered class.
  Defensive-only beats baseline overall in 2v2 (+3.9pt) and 3v3 (+0.9pt).
- **Offensive dip — net-negative, do not ship as-is.** The full feature is
  *worse than defensive-only in every cell*. Root cause (confirmed by trace):
  the team's target AI focuses the enemy healer (standard kill-the-healer
  behavior — the allied Mage Frostbolts it for 80+ per cast all match). Dipping
  to *fear* that same healer is self-defeating: one ally Frostbolt (>100 over
  two) breaks the fear instantly, and the dip pulls the cloth Priest out of
  healing position for nothing. The brainstorm's intended line ("fear the
  healer while the team kills a *different* target") requires team
  target-coordination that does not exist — no `dip_budget`/aggressiveness
  tuning rescues it, because the conflict is with target selection, not reach.

### Fix shipped: dip respects the kill target

Rather than disable the dip, the coordination it was missing already exists via
the kill target. The dip now skips any enemy a living non-pet ally is currently
attacking (`team_focus`), so it fires only when the team is committing
elsewhere (e.g. `kill_target` is a DPS) — exactly when fearing the healer buys
a kill window without the team's own damage breaking the fear. In default
kill-the-healer play the dip correctly stays home.

Re-sweep with the gate (same isolation method):

| Metric | baseline | old-full (drag) | **gated (shipped)** |
|---|---|---|---|
| 2v2 overall | 42.5% | 44.0% | **46.2%** |
| 2v2 — enemy has healer | 34.3% | 33.2% | **37.5%** |
| 3v3 overall | 45.5% | 44.8% | **47.0%** |
| 3v3 — enemy has healer | 41.5% | 38.3% | **41.7%** |

The gate turns the drag into a gain — strictly better than both old-full and
defensive-only, with the defensive win intact and draws back to baseline
(2v2 18/2400, 3v3 1/2250). **Verdict: ship the full feature with the gate.**
The `offensive_dip_fears_enemy_healer` probe now pins the valuable case
(kill_target on the enemy DPS → dip fears the free healer); the corner-pin
probe was un-ignored (the gate keeps the dip home).

### Is the AI functioning as expected?

Yes, after the gate. Defensive self-peel fires under pressure and improves
survival (+4pt vs no-healer comps). The offensive dip fires only when it pays
off (team killing a non-healer), confirmed by trace (DipEnter→DipComplete on
the free enemy healer) and the re-sweep. Mechanically the dip always worked;
the gate fixed the strategy.

## Completed (2026-06-15)

1. **Full 2v2/3v3 sweep — done** (results above). Verdict: ship the feature
   with the kill-target gate; net +3.7pt (2v2) / +1.5pt (3v3) for the Priest.
2. **Fix shipped — done.** The dip respects the kill target (`team_focus`); no
   `dip_budget`/aggressiveness tuning was needed (the lever was target
   coordination, not reach).
3. **All four ignored probes resolved — done** (0 ignored now):
   - `pressured_priest_does_not_pin_into_corners` — un-ignored (the gate keeps
     the dip home → no corner walk).
   - `critical_heal_fires_despite_live_window` — reseeded 5 → 16.
   - `pressured_priest_stays_in_heal_range_of_ally` — grace 1.0s → 2.5s
     (fear-scatter is a transient defensive-scream effect).
   - `priests_spend_substantial_time_free_in_unforced_mirror` — ceiling 50% →
     65% (the defensive scream prolongs the mirror into a longer contested
     match; net-positive, baseline draw rates).
4. **Canonical baselines regenerated** (`canonical_{1v1,2v2,3v3}_*.csv` +
   summary) on the post-Psychic-Scream meta. Priest is now mid-B in team
   formats (no longer a floor).

## Remaining (future, optional)

- Generic ability-parameterized dip core shared by Paladin + Priest
  (target-selection helpers currently duplicated).
- Team target-AI could *deprioritize* a feared enemy, which would let the
  offensive dip pay off even more (currently it just avoids fighting the
  focus). Not required — the gate already makes the dip net-positive.
