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

## Remaining work (the authoritative U6 pass)

1. **Full side-symmetrized 2v2/3v3 sweep** (N≥100, 300s cap) per
   `docs/solutions/implementation-patterns/mirror-asymmetry-side-symmetrized-measurement.md`
   and the `balance-methodology` memory: measure Priest win-rate vs the `main`
   baseline across comps, confirm a real improvement with no unintended
   regressions, and decide whether the mirror draw cost is acceptable.
2. **Tune** `dip_budget` / `healing_heavy_hp` / dip aggressiveness from that
   data — e.g. gate the dip harder in healer mirrors if the stall is
   net-negative, or accept it if the offensive value dominates.
3. **Recalibrate the four probes ignored during U2/U4** to the tuned behavior
   (do NOT weaken their guards before the sweep decides the target behavior):
   - `pressured_priest_stays_in_heal_range_of_ally` (fear-scatter breaks the
     anchor window when the melee ally chases a feared enemy)
   - `critical_heal_fires_despite_live_window` (re-scan seeds — the scream
     peels attackers so seed 5 no longer hits the critical-heal moment)
   - `priests_spend_substantial_time_free_in_unforced_mirror` (mirror dip
     oscillation raises PRESSURED past the 50% ceiling)
   - `pressured_priest_does_not_pin_into_corners` (the dip Entity-goal walk
     bypasses the corner-penalty scorer; 5.47s vs the 5s ceiling)
