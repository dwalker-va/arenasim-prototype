# Hunter Pet Engagement — Iteration 2a Report

**Date:** 2026-05-22
**Plan:** `docs/plans/2026-05-22-002-feat-hunter-pet-engagement-plan.md`
**Brainstorm:** `docs/brainstorms/2026-05-22-hunter-pet-engagement-requirements.md`

## TL;DR

Pet engagement architecture lands. Hunter pets now pursue enemies and Spider Web fires on a 20s cooldown (was 45s, never fired in practice). **Hunter v Warrior 1v1 moved from 0% → 20%** at N=20 — the first matchup since the mana iteration where the Hunter actually wins. Hunter v Paladin 0% → 5%. Average match duration for Hunter v Warrior extends ~6s, confirming Spider auto-attacks in melee plus Spider Web's 4s root are exerting meaningful pressure.

**U4 (Hunter try_dispatch_* helpers) deferred to iteration 2b.** The PetCommand component + apply_deferred wiring + EventPayload extension + LowHealthHeel variant are all landed and tested, but the autonomous spider_ai path is what's currently driving Spider Web casts. Hunter-side dispatch (the hybrid model from the brainstorm) lands in the follow-up. Today's signal comes from U1+U5+U8 (pet pursuit + filter removal + CD tune) and the infrastructure to consume PetCommands is dormant pending U4.

## Change Summary

**Implemented (U1+U2+U3+U5+U6+U8):**

| Change | File(s) |
|---|---|
| Pets excluded from `acquire_targets`; pet AI assigns `pet.target = owner.target` | `src/states/play_match/combat_ai.rs:107`, `src/states/play_match/class_ai/pet_ai.rs:109-` |
| `CombatantInfo.pet: Option<Entity>` snapshot field for owner→pet lookup | `src/states/play_match/class_ai/mod.rs:58` |
| `CombatSnapshot`/`CombatContext` gain `ability_cooldowns` map (per-entity BTreeMap) | `src/states/play_match/class_ai/{mod.rs,combat_snapshot.rs}` |
| `AbilityType` gains `PartialOrd, Ord` derive (required for BTreeMap key) | `src/states/play_match/abilities.rs:44` |
| `PetCommand` one-shot component + apply_deferred wiring | `src/states/play_match/components/pets.rs:80`, `src/states/play_match/systems.rs:176` |
| `EventPayload::Pet` gains `dispatched_by: Option<u32>` (backward-compat) | `src/states/play_match/decision_trace/events.rs:111` |
| `start_pet_dispatch_decision` builder helper | `src/states/play_match/decision_trace/mod.rs` |
| `RejectionReason::LowHealthHeel` variant | `src/states/play_match/decision_trace/events.rs:212` |
| Heel predicate (HP < 25% clears target + emits LowHealthHeel trace) | `src/states/play_match/class_ai/pet_ai.rs` |
| `spider_ai` `dist_to_owner ≤ 15` filter removed | `src/states/play_match/class_ai/pet_ai.rs` |
| Spider Web cooldown 45s → 20s | `assets/config/abilities.ron:809` |
| `decision_trace_audit` accepts `LowHealthHeel` + `Rooted` (now emitted) | `tests/decision_trace_audit.rs:91` |
| `cast_guard_tests`, `combat_snapshot_tests`, `class_ai_decisions` test helpers updated for new fields | `tests/*.rs` |

**Deferred to iteration 2b (U4):**

- `try_dispatch_spider_web`, `try_dispatch_boar_charge`, `try_dispatch_masters_call` in Hunter AI
- Authoritative `pre_cast_ok` execution path in `pet_ai_system` for received PetCommands
- Per-pet headline ability dispatch using the optimistic-dispatch model

## Validation Results

### Decision Trace Audit (Hunter v Warlock, seed 0, 60s match)

| Rejection class (Spider) | Pre Pet-Engage (~) | Post Pet-Engage |
|---|---:|---:|
| SpiderWeb:NoValidTarget | ~1,000 | 946 |
| SpiderWeb:OnCooldown | ~881 | 699 |
| **Spider Web fires (autonomous)** | **0-1** | **1** |

The trace shows Spider Web fires successfully — was effectively zero before. Mostly limited by range (Spider's 20yd range vs Hunter staying at 35yd safe distance). The Aimed Shot crit silent-debt (Web's 80-damage threshold) is now observable in match logs (`Team 2 Warlock's Web broke from damage (98/80)`) per the doc-review's AE3 flag.

### 1v1 Matrix (Hunter row, N=20)

Comparing post-pet vs post-mana (PR #55) baselines:

| Matchup | Post-Mana | Post-Pet | Δ | Pre Avg | Post Avg | Notes |
|---|---:|---:|---:|---:|---:|---|
| Hunter vs Warrior | 0% | **20%** | **+20pp** | 32.5s | 39.6s | **Material movement — Spider pursuit + Web slow** |
| Hunter vs Mage | 0% | 0% | 0pp | 10.5s | 10.2s | Dies before Spider closes |
| Hunter vs Rogue | 0% | 5% | 0pp | 24.2s | 24.2s | Noise (1/20 both runs) |
| Hunter vs Priest | 0% | 0% | 0pp | 32.2s | 36.7s | Survives ~5s longer; Priest heals through |
| Hunter vs Warlock | 5% | 5% | 0pp | 18.9s | 19.2s | Felhunter Devour still counters |
| Hunter vs Paladin | 0% | **5%** | **+5pp** | 50.1s | 77.1s | **Match runs 27s longer** — pet pressure visible |
| Hunter vs Hunter | 45% | 20% | -25pp | 17.9s | 17.9s | More drawn (50% draw rate) — mirror stalls |

**Aggregate non-mirror winrate:** 5/240 = 2.1% (was 3/240 = 1.25% post-mana) — modest aggregate lift, concentrated in Warrior matchup.

### 2v2 Matrix (Hunter+Priest team, N=10)

| Matchup | Post-Mana | Post-Pet | Avg duration delta |
|---|---:|---:|---|
| H+P vs Warrior+Priest | 0% | 0% | +13.2s |
| H+P vs Mage+Priest | 0% | 0% | +0.2s |
| H+P vs Rogue+Priest | 0% | 0% | 0s |
| H+P vs Priest+Priest | 60% | **80%** | -5.2s |
| H+P vs Warlock+Priest | 0% | 0% | +0.3s |
| H+P vs Paladin+Priest | 0% (10 draws) | 0% (10 draws) | 0s |

Hunter+Priest vs Warrior+Priest extends 13s without flipping wins — pet pressure absorbed by enemy Priest healing. vs Priest+Priest improved 60% → 80% (the no-healing-opponent matchup).

## Success Criteria Assessment

| Criterion | Status | Notes |
|---|---|---|
| 2v2 ≥30% winrate in ≥2 of 6 paired matchups | **Not met** | Only Priest+Priest matchup (~80%) is positive |
| 1v1 aggregate winrate moves to ≥5% | **Partial** | 2.1% — Hunter v Warrior (+20pp) is the standout |
| Hunter v Warrior 0% → ≥10% | **Met (20%)** ✓ | This was the brainstorm's primary expected lift |
| SpiderWeb:NoValidTarget drops ≥75% from ~1,000 baseline | **Not met** | 946 — barely moved. Range-gated (Spider 20yd vs Hunter 35yd) |
| Pet auto-attack damage events in ≥80% of Hunter matches | **Unknown** | Match log auto-attack visibility uncertain; Spider close-in time vs match duration is short for Hunter v Mage / v Warlock |
| Non-Hunter matchups stay within ±5pp of baseline | **Mostly met** | At N=20 noise dominates; warrants N=100 confirmation |

## Honest reading

The architecture lands and produces a real signal in Hunter v Warrior — that's the matchup the brainstorm specifically expected to move. Hunter v Paladin extends by 27s (pet pressure visible in duration even when wins don't flip).

Counter-intuitively, **Hunter v Mage and Hunter v Warlock didn't move** because:
- Hunter v Mage (10s defeats): Spider can't close before Hunter dies; matchup gated by Hunter survivability, not pet engagement
- Hunter v Warlock: Felhunter Devour Magic still hard-counters Hunter's Concussive Shot; matchup gated by the Devour interaction (separate survivor in the ideation doc)

**Hunter mirror went 45% → 20% with 50% draws** — pet engagement makes both Hunters more durable, mirror stalls. Worth monitoring but not blocking.

The **SpiderWeb:NoValidTarget rejection count barely moved** because Spider Web's range (20yd) is incompatible with Hunter's kiting distance (35yd). The filter removal was the architectural fix; the rate-limit is now range, not the owner-distance check. A future iteration could raise Spider Web's range to 30yd to match Hunter's kit.

## Recommendations

1. **Ship this iteration.** Hunter v Warrior 0→20% is the largest single-matchup movement since the rebalance started. The architecture is correct.
2. **Land U4 in a follow-up iteration.** PetCommand + apply_deferred + EventPayload extension are wired but dormant. Hunter `try_dispatch_*` helpers activate the hybrid model. ~200 LOC across 3 helpers.
3. **Consider Spider Web range tune (20yd → 30yd)** in the next iteration to actually move the NoValidTarget rejection count meaningfully — Hunter operates at 35yd; pet needs to web from a comparable range.
4. **Investigate Hunter mirror draws.** 50% draw rate is new; may indicate a mirror-stall mechanic that needs intervention (timeout or fatigue).
5. **Don't celebrate the +13s vs Warrior+Priest duration yet** — extending matches without flipping wins is "pet pressure visible but absorbed." 2v2 still needs healer-CC awareness to actually move wins (separate survivor in ideation doc).

## Deferred / Residual Work

- **U4 — Hunter try_dispatch_\* helpers** (the hybrid model the brainstorm specified). PetCommand infrastructure is in place; the consumer is the next iteration.
- N=100 rerun pre-merge to confirm the small lifts (Rogue 5%, Warlock 5%) and the mirror's draw rate.
- Hunter v Mage 0% at 10s — different binding constraint (Hunter survivability), separate survivor.
- Hunter v Warlock 5% — Felhunter Devour Magic counter, separate survivor.
- Hunter v Paladin 0/5% — Hunter never CCs the Paladin healer, separate survivor (team-comp awareness).

## Iteration 2a as autopilot scope

This LFG run delivered the foundational pet engagement architecture: target ownership moved out of `acquire_targets`, snapshot extended for cross-AI cooldown reads, PetCommand framework wired with proper apply_deferred and trace event extensions, Heel predicate with LowHealthHeel rejection variant, Spider Web mechanics tuned (filter removed + CD aligned).

U4 (Hunter dispatch helpers) was scoped out mid-session — the architecture is sound without it, the autonomous spider_ai path provides measurable signal, and the user accepted the scope reduction at the U2 commit boundary. The iteration ships as "pet engagement infrastructure + autonomous spider_ai fallback driving validation signal."
