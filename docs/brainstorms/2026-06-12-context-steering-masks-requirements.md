---
date: 2026-06-12
topic: context-steering-masks
---

# Context-Steering Masks, Then Universal Movement (Mage Pilot)

## Summary

Restructure the movement direction scorer into additive interest terms plus boolean danger masks (replacing the 1000.0 hard-penalty scheme), shipped as a near-identity refactor; then migrate Mage movement onto the posture architecture with a minimal ENGAGE/KITE posture pair, world-state entry predicates, and a new `range_band` scorer term — retiring the Mage's use of the hand-coded kiting path.

---

## Problem Frame

Three movement brains coexist today. Healers use the data-driven posture scorer (`src/states/play_match/combat_core/movement_scoring.rs`); Mage and Hunter use `find_best_kiting_direction` in `src/states/play_match/combat_core/movement.rs` — a hand-coded sibling with the same 16-compass argmax skeleton but hardcoded weights, no config, and no trace events; melee and pets use simple pursuit.

Two structural costs follow. The scorer encodes its hard constraints (ally-anchor, boundary) as -1000.0 penalties, and `MovementConfig::validate()` must police that they dominate every possible soft-term sum — a standing tax on every new term and a landmine for any future automated weight search. And kiting is triggered by `kiting_timer`, a raw float on `Combatant` that the ability AI writes directly (`src/states/play_match/class_ai/mage.rs` sets it to Nova's aura duration) — an untraced cross-system mutation channel of exactly the kind the posture system replaced for healers.

DPS movement is also a diagnosis blind spot: no `movement_decision` trace events, no KPI coverage, no probes — while healer movement has all three.

---

## Key Decisions

- **Masks, not penalties, for hard constraints.** Candidate directions violating a hard constraint are removed before scoring, not out-scored. This deletes the dominance invariant and its `validate()` checks entirely, and frees soft weights to take any magnitude. The kiting branch already half-uses this pattern (bounds violations are `continue`-skipped), so this unifies existing siblings rather than importing a foreign pattern.
- **Fixed fallback ladder, anchor drops first.** When masks eliminate all 16 candidates, drop the anchor (heal-range) mask and rescore; the boundary mask effectively never drops because the movement executor clamps to the arena anyway. This reproduces the current penalty semantics in all but exotic mixed-violation frames (a uniform penalty cancels in the argmax — equivalent to dropping the mask), and where it diverges, preferring in-bounds over in-heal-range is strictly saner than today's indifference. Posture-dependent ladders were rejected: they add a config surface and break the near-identity claim.
- **Minimal ENGAGE/KITE posture pair for the Mage.** Reuses the existing posture machinery — typed transitions, hysteresis, commit windows, `movement_decision` trace events — without the healer's four-state set. Healer postures earn their complexity from the heal-range anchor problem, which the Mage does not have. Dead states are config noise.
- **World-state predicates trigger KITE, not `kiting_timer`.** KITE entry derives from observable state (a melee-range threat carrying the Mage's root/slow aura — the aura is the timer; or a threat closing while the Mage is free to move). This kills the ability-AI→movement mutation channel for the Mage and gives transitions typed, traced triggers. Known accepted delta: KITE exits when the root/slow aura breaks early from damage (Frost Nova breaks at 80 cumulative damage), where today's timer runs its full duration regardless.
- **KPI acceptance bar for the pilot, not strict parity.** Part B is an intentional behavior change. Building timer-compatibility shims to prove byte-parity first would be throwaway work; instead the bar is behavioral KPIs plus a bounded matrix delta (see Success Criteria).
- **Part A ships alone, before Part B.** `range_band` as a soft term under the penalty scheme would re-litigate the dominance invariant; masks must land first. Architecture changes land isolated and matrix-measured — never bundle the near-identity refactor with the behavior change in one PR.

---

## Requirements

**Part A — mask refactor (near-identity)**

- R1. The direction scorer evaluates candidates in two passes: a mask pass (boolean elimination) and an interest pass (additive scoring over surviving candidates).
- R2. The boundary constraint (`is_in_arena_bounds`) and the ally-anchor heal-range constraint become masks; `threat_repulsion`, `formation_pull`, `corner_penalty`, `wand_pull`, and `commitment_bonus` remain soft interest terms unchanged.
- R3. When masks eliminate all candidates, a fixed fallback ladder applies: drop the anchor mask and rescore; if candidates remain fully boundary-masked, score them anyway (least-bad) and let the executor clamp.
- R4. The `ally_anchor` and `boundary_penalty` weight fields and the soft-vs-hard dominance checks in `MovementConfig::validate()` are removed; remaining validation (ranges, windows, TTLs) is unchanged.
- R5. `movement_decision` trace events gain a field recording which candidate directions were masked (and by which mask), present when the scorer ran. The trace remains non-perturbing (byte-equality tests stay green).
- R6. Healer behavior is preserved: all existing movement probes pass unmodified, and a full matrix run is byte-identical to baseline except in frames where masks eliminated all candidates — each divergent cell must be attributable to that case.

**Part B — Mage pilot (intentional behavior change)**

- R7. The Mage gets a two-posture state machine (ENGAGE, KITE) running on the shared posture machinery: typed transition triggers, hysteresis, commit windows, and `movement_decision` trace events.
- R8. KITE entry and exit derive from world-state predicates (threat proximity, the Mage's own root/slow auras on threats, casting state) — not from `kiting_timer`. The Mage's writes to `kiting_timer` are removed; the field and the legacy kiting branch remain for the Hunter.
- R9. A new `range_band` interest term rewards positions within a configured [min, max] distance ring of the kill target. The Mage uses it to hold cast range while kiting (replacing the legacy keep-in-shot-range penalty and 0.85x orbit logic); it is defined generically so later migrations (Hunter dead-zone, melee pursuit) reuse it.
- R10. Mage movement weights and posture parameters live in a `mage` block in `assets/config/movement.ron` with struct defaults and startup validation, following the existing per-class pattern.
- R11. Arc-kiting (circle-strafing the kill target when fleeing straight would leave shot range) emerges from `range_band` + `threat_repulsion`; the pilot must demonstrate it in probes rather than special-casing it.
- R12. A `mage_postures` probe module pins KITE entry on Nova-root, KITE exit on aura break/expiry, and arc-kiting geometry at fixed seeds, using the observed-run harness.

**Diagnosis parity**

- R13. Mage movement decisions are diagnosable with the same tooling as healers: `movement_decision` events with `scorer_terms` breakdowns and posture transitions appear in traces, and `scripts/movement_kpis.sh` output covers the Mage without modification.

---

## Acceptance Examples

- AE1. **Covers R3.** Given a healer cornered such that every in-heal-range direction is out of bounds, when the scorer runs, then the anchor mask is dropped, an in-bounds direction is chosen, and the trace records the fallback.
- AE2. **Covers R6.** Given the Part A build and the baseline build run on the full seeded matrix, when trace and result outputs are compared, then all divergent cells (if any) contain at least one all-masked frame, and no other cells differ.
- AE3. **Covers R8.** Given a Warrior in melee range of the Mage, when Frost Nova lands (root aura applied), then the Mage transitions ENGAGE→KITE with a typed trigger in the trace — and when the root breaks early from damage, KITE exits without waiting for the legacy timer duration.
- AE4. **Covers R9, R11.** Given the Mage fleeing a pursuer while its kill target is a different enemy near max cast range, when KITE movement is scored, then the chosen direction keeps the kill target inside the range band (arcing), not straight away from the pursuer.

---

## Success Criteria

**Part A:** full matrix byte-identical to baseline, excepting only attributable all-masked frames (AE2); all existing probes green; dominance-validation code deleted.

**Part B (side-symmetrized, vs pre-pilot baseline):**
- Mage time-within-melee-range of enemies: no worse than baseline.
- Mage kill-target-within-shot-range uptime while kiting: no worse than baseline.
- No matchup's side-symmetrized winrate shifts more than 5 points without an explained mechanism traceable in decision traces.
- The legacy kiting branch is no longer reachable from Mage entities (verified by trace: no Mage kiting decisions outside `movement_decision` events).

---

## Scope Boundaries

**Deferred for later**
- Hunter migration (dead-zone lower bound via `range_band`) and melee pursuit migration — follow the pilot as separate efforts; until then, two kiting mechanisms coexist by design (Mage on postures, Hunter on `kiting_timer` + legacy path), and the legacy code cannot be deleted in this work.
- Pets — stay on pursuit until the snapshot unification effort; they do not ride the new scorer here.
- Folding `wand_pull` into `range_band` for healers — `range_band` generalizes it, but healer configs stay untouched so the Mage PR carries no healer behavior risk.
- LoS/cover masks for pillar play — separate effort; Part A creates the danger-mask slot it plugs into.
- Posture-dependent fallback ladders — revisit only if the fixed ladder proves limiting in practice.

---

## Dependencies / Assumptions

- The near-identity claim for Part A rests on the dominance invariant currently holding (it is enforced by `validate()` at startup), which makes mask-argmax equivalent to penalty-argmax whenever at least one unmasked candidate exists. If shipped configs ever bypassed validation, the equivalence argument needs rechecking.
- The observed-run probe harness (`tests/movement_probes.rs`) and its non-perturbation self-test are the verification substrate for R6/R12; matrix tooling (`--matrix`, side-symmetrized comparison) is the substrate for AE2 and the Part B bar.
- Assumes Frost Nova and Mage slow effects are auras on the target with observable break/expiry — true today (`break_on_damage_threshold: 80.0` on Nova).

---

## Outstanding Questions

**Deferred to planning**
- The exact KITE entry/exit predicate set (which auras qualify, whether "threat closing while free to move" is in the pilot's entry set or only aura-based entry).
- Whether ENGAGE uses `range_band` toward the kill target continuously or only constrains movement when repositioning (current behavior: Mage mostly stands and casts).
- The trace-field shape for masked candidates (bitmask vs per-mask lists) within the closed-enum audit's constraints.

---

## Sources / Research

- Ideation: `docs/ideation/2026-06-12-combatant-ai-ideation.md` (idea 2, including the deep-dive findings on the kiting branch's proto-context-steering structure).
- Code: `src/states/play_match/combat_core/movement_scoring.rs` (scorer + tests), `src/states/play_match/combat_core/movement.rs` (`find_best_kiting_direction`, kiting branch), `src/states/play_match/movement_config.rs` (`validate()` dominance checks), `src/states/play_match/class_ai/mage.rs` (`kiting_timer` writes).
- External: context steering (Andrew Fray, *Game AI Pro 2*, ch. 18) — interest/danger map separation; the danger-map-as-mask semantics adopted here.
- Prior lesson: the U4 casting-visibility incident (±50 winrate points latent in an AI-visibility change) — the reason Part A and Part B never share a PR and each lands matrix-measured.
