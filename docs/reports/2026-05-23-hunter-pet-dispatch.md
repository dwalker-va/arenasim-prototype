# Hunter Pet Dispatch — Iteration 2b Report

**Date:** 2026-05-23
**Plan:** `docs/plans/2026-05-22-002-feat-hunter-pet-engagement-plan.md` (U4)
**Prior iteration:** `docs/reports/2026-05-22-hunter-pet-engagement.md` (iteration 2a)

## TL;DR

U4 lands: Hunter AI now owns the strategic decision to dispatch Spider Web, Boar Charge, and Master's Call via the `PetCommand` component, and the pet AI re-validates conditions at execution time. Both layers emit `pet_decision` trace events; the Hunter-dispatched ones carry `dispatched_by: Some(hunter_entity)` for audit attribution. The PetCommand infrastructure from iteration 2a (flagged as 90% dead by the prior code review) is now actively consumed.

**Validation:** Hunter 1v1 winrates at N=20 are byte-identical to the iteration 2a baseline — 4/20 vs Warrior, 1/20 vs Paladin, 4-6-10 mirror — confirming the architectural change ships with zero behavioral regression. Trace audit on Hunter v Warlock shows 405 `dispatched_by` events in a single match with the dispatch lifecycle (Hunter-side `chose` + pet-side `chose`) visible on every successful cast.

## Change Summary

| Change | File(s) |
|---|---|
| Hunter `try_dispatch_spider_web`, `try_dispatch_boar_charge`, `try_dispatch_masters_call` helpers | `src/states/play_match/class_ai/hunter.rs` |
| `dispatch_pet_ability` router (selects helper by `PetType`) | `src/states/play_match/class_ai/hunter.rs` |
| `dispatch_predicates_for_damaging` snapshot heuristic helper | `src/states/play_match/class_ai/hunter.rs` |
| Hunter dispatch call site in `decide_hunter_action` (runs before Hunter GCD gate) | `src/states/play_match/class_ai/hunter.rs` |
| `pet_command_rejection` authoritative re-check for queued PetCommands | `src/states/play_match/class_ai/pet_ai.rs` |
| `execute_spider_web`, `execute_boar_charge`, `execute_masters_call` execution helpers (shared by both dispatch paths) | `src/states/play_match/class_ai/pet_ai.rs` |
| PetCommand consumption replaces iteration 2a placeholder (despawn-without-execution) | `src/states/play_match/class_ai/pet_ai.rs` |
| Autonomous fallback `spider_autonomous_dispatch` / `boar_autonomous_dispatch` / `bird_autonomous_dispatch` (covers Hunter CastingState windows) | `src/states/play_match/class_ai/pet_ai.rs` |
| `CLAUDE.md` updated — Hunter pet engagement section reflects 2b's hybrid model + fallback rationale | `CLAUDE.md` |

## Validation Results

### 1v1 Matrix Sweep (N=20, seed_base=0)

Compared against iteration 2a (`design-docs/balance/matrix_baseline_2026-05-22_pet_engage_1v1_post.csv`). Hunter rows shown; non-Hunter matchups are unchanged because they don't run any code on this path.

| Matchup | 2a wins/N | 2b wins/N | Delta |
|---|---:|---:|---:|
| Hunter v Warrior | 4/20 | 4/20 | 0 |
| Hunter v Mage | 0/20 | 0/20 | 0 |
| Hunter v Rogue | 1/20 | 1/20 | 0 |
| Hunter v Priest | 0/20 | 0/20 | 0 |
| Hunter v Warlock | 1/20 | 1/20 | 0 |
| Hunter v Paladin | 1/20 (+1 draw) | 1/20 (+1 draw) | 0 |
| Hunter v Hunter | 4-6-10 | 4-6-10 | 0 |
| Warrior v Hunter | 15/20 | 15/20 | 0 |
| Mage v Hunter | 20/20 | 20/20 | 0 |
| Rogue v Hunter | 19/20 | 19/20 | 0 |
| Priest v Hunter | 20/20 | 20/20 | 0 |
| Warlock v Hunter | 20/20 | 20/20 | 0 |
| Paladin v Hunter | 19/20 (+1 draw) | 19/20 (+1 draw) | 0 |

Sub-second floating-point variation appears in two `avg_duration_secs` values (Hunter v Rogue: 24.20s → 24.19s, Rogue v Hunter: 23.89s → 23.88s). All winrates byte-identical.

### N=100 Sweep Addendum

Completed 2026-05-23 (`design-docs/balance/matrix_baseline_2026-05-23_pet_dispatch_1v1_post_n100.csv`). No iteration 2a N=100 baseline exists for direct comparison; the N=20 byte-identical result above is the strongest signal of zero regression. N=100 numbers below characterize iteration 2b's behavior at higher sample size.

| Matchup | N=20 | N=100 |
|---|---:|---:|
| Hunter v Warrior | 4/20 = 20% | 25/100 = 25% |
| Hunter v Mage | 0/20 = 0% | 0/100 = 0% |
| Hunter v Rogue | 1/20 = 5% | 3/100 = 3% |
| Hunter v Priest | 0/20 = 0% | 0/100 = 0% |
| Hunter v Warlock | 1/20 = 5% | 4/100 = 4% |
| Hunter v Paladin | 1/20 = 5% | 1/100 = 1% |
| Hunter v Hunter | 4-6-10 (20/30/50%) | 43-28-29 (43/28/29%) |
| Warrior v Hunter | 15/20 = 75% | 71/100 = 71% |
| Mage v Hunter | 20/20 = 100% | 100/100 = 100% |
| Rogue v Hunter | 19/20 = 95% | 99/100 = 99% |
| Priest v Hunter | 20/20 = 100% | 100/100 = 100% |
| Warlock v Hunter | 20/20 = 100% | 99/100 = 99% |
| Paladin v Hunter | 19/20+1 draw = 95% | 100/100 = 100% |

Most matchups are within ±5pp at the two sample sizes (normal binomial variance). The Hunter v Hunter mirror shifted from 30%/50% draws at N=20 to 28%/29% draws at N=100 — the additional 80 seeds simply produce fewer draws than the first 20 sampled. The Hunter v Paladin matchup dropping from 5% to 1% is the largest single-matchup delta; given the underlying matchup is severely Hunter-disadvantaged (no healer-CC tools, Paladin sustains indefinitely), this is variance around a near-zero true winrate rather than a regression introduced by iteration 2b.

### Decision Trace Audit (Hunter v Warlock, default seed, 19s match)

Run with `--trace-mode on`:

| Signal | Value | Notes |
|---|---:|---|
| Hunter-dispatched `pet_decision` events (`dispatched_by != null`) | 405 | Spider's lifecycle per Hunter tick |
| SpiderWeb dispatch `chosen` events | 2 | 1 from Hunter dispatch trace + 1 from pet execution trace (two-event lifecycle is by design) |
| SpiderWeb dispatch `rejected` events | 403 | 94 OnCooldown, 309 OutOfRange — healthy in-band rejections |
| `SpiderWeb:NoValidTarget` rejections | 0 | Iteration 2a's primary failure mode is gone |
| Felhunter autonomous `pet_decision` events (unchanged path) | 1169 | Spell Lock + Devour Magic still fire |
| Felhunter Devour Magic `chosen` events | 2 | Behavior unchanged from iteration 2a |

Sample `dispatched_by` event (formatted):

```json
{
  "kind": "pet_decision",
  "actor": { "entity_id": 12, "class": "Spider", "team": 1, ... },
  "owner": 4,
  "pet_type": "Spider",
  "candidates": [{ "ability": "SpiderWeb", "status": "chosen" }],
  "outcome": { "type": "action_taken", "ability": "SpiderWeb", "target_id": 7, "was_instant": true },
  "dispatched_by": 4
}
```

### Pre-Merge Checklist (from iteration 2a handoff)

- [x] Confirm `pet_decision` trace events include `dispatched_by: Some(hunter_id)` for Hunter-dispatched casts
- [x] Re-run trace audit on Hunter v Warlock — Spider Web fires via PetCommand path
- [x] Decide whether to keep the autonomous `spider_ai` Spider Web path as a fallback — **kept**, with rationale below
- [x] Re-run 1v1 matrix at N=20 to confirm Hunter v Warrior 20% holds
- [ ] N=100 rerun (1v1) before tagging the PR ready — queued, separate commit

## Architectural Decisions

### Autonomous fallback retained (U5 outstanding question resolved)

The plan's U5 outstanding question asked whether to keep `spider_ai`/`boar_ai`/`bird_ai` autonomous decide paths as a fallback once Hunter dispatch lands. The plan's default was "delete (Hunter owns dispatch); revisit if Hunter incapacitation scenarios need pet autonomy."

We initially defaulted to delete. The resulting N=20 sweep showed Hunter v Warrior dropping from 20% → 10%. Root cause: `decide_abilities` filters combatants `Without<CastingState>`, so Hunter is excluded from the dispatch loop during the 2.5s Aimed Shot cast. The autonomous spider_ai was previously covering those windows.

**Resolution:** Restored autonomous dispatch as a fallback. Both paths share the same `execute_*` helpers; the only trace difference is `dispatched_by` (set by Hunter, omitted by autonomous). Trace audits can distinguish active dispatch from fallback firing via that field. Hunter still owns the strategic dispatch decision when eligible; the fallback exists strictly to cover the casting-state gap.

### Two-event dispatch lifecycle

For every successful Hunter dispatch, two `pet_decision` events emit on the same `sim_time`:
1. Hunter-side: `chose(SpiderWeb)` with `dispatched_by: hunter_entity` (the decision)
2. Pet-side: `chose(SpiderWeb)` with `dispatched_by: hunter_entity` from the queued PetCommand (the execution)

This is by design per the plan's U3/U4 narratives — both layers record the same lifecycle event. Audit recipes that count "successful casts" should de-duplicate (e.g., filter on `pet-side trace only` or count via match-log `Spider uses Web` entries) to avoid double-counting.

### Friendly-CC guard scoped to Boar Charge only

Spider Web applies a 0-damage Root aura; its existence on a target cannot break a threshold-0 friendly CC. Boar Charge applies impact damage that would. The `has_friendly_breakable_cc` guard is therefore conditioned on `ability == BoarCharge` in both `dispatch_predicates_for_damaging` (hunter.rs) and `pet_command_rejection` (pet_ai.rs). Initial unconditional application caused over-rejection (most visible: Spider Web rejected while target was in a Hunter's own Freezing Trap, which can't be broken by a 0-damage aura anyway).

## Code Quality Notes

- All existing tests pass (`cargo test --release`: 270+ tests, 0 failures)
- `tests/decision_trace_audit.rs::expected_reasons` already includes every rejection variant emitted by the new dispatch paths (no additions needed for U4)
- `tests/registration_audit.rs` passes — no new systems added
- Determinism: BTreeMap iteration order preserved; `tests/headless_tests.rs::trace_file_is_deterministic_at_same_seed` continues to gate
- Pet-side `execute_*` helpers extracted from the iteration 2a `spider_ai`/`boar_ai`/`bird_ai` bodies; both dispatch paths (Hunter active, autonomous fallback) call them so the trajectory of a fired ability is identical regardless of which path triggered it

## Deferred to Follow-Up

- **AE3 (Hunter Aimed Shot crit breaking Spider Web's 80-damage Root threshold).** Still out of scope; would require `has_friendly_breakable_cc` to predict crit damage against numeric thresholds. Observable in the trace via `aura_removed` with `removal_cause == FriendlyDamage` — recipe in the trace-audit section of CLAUDE.md.
- **Per-pet headline ability CD tune beyond Spider Web's 45s→20s.** Boar Charge and Master's Call remain at 45s, which limits dispatch frequency in short matches. The plan's brainstorm Approach B is the natural follow-up if validation shows the dispatch infrastructure is starved of opportunities.
- **Pet HP recovery / Mend Pet.** Heel exit (HP rising back above 25%) is implemented but dead code today since no party heals exist. Forward-looking.
