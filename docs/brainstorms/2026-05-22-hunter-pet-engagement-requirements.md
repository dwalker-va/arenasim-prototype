---
date: 2026-05-22
topic: hunter-pet-engagement
---

# Hunter Pet Engagement (AI-Only)

## Summary

Wire up Hunter pet engagement via a Send/Heel command model: pet target acquisition (`pet.target = owner.target`), Hunter AI dispatches pet abilities (hybrid — Hunter owns the headline Spider Web / Boar Charge / Master's Call calls, pet handles auto-attacks and pursuit between commands), Spider Web's `dist_to_owner ≤ 15` filter removed, and low-HP Heel at 25% pet HP. Pet ability cooldowns and ranges are deliberately **not** tuned in this iteration — the goal is to isolate the AI architecture as the only variable.

## Problem Frame

After the mana economy fix (PR #55), the decision-trace audit on Hunter v Warrior shifted from `InsufficientMana` (now <60) to pet-AI failures: **1,000+ `SpiderWeb:NoValidTarget` rejections** in a Hunter v Warlock match plus zero Spider auto-attack entries despite a 16-second fight. Pets sit at the Hunter's flank, 3 yards behind. They never close on enemies because no pet AI ever assigns a `combatant.target`, so `combat_core/movement.rs:311+` routes pets to the "follow owner at 3yd" branch instead of the target-pursuit branch.

Two compounding issues make Spider Web fire never:
1. `spider_ai` in `src/states/play_match/class_ai/pet_ai.rs:374` rejects targets whose `dist_to_owner > 15.0` AND whose `dist_to_spider > 20.0`. With the Spider physically at the Hunter's feet (the 3yd-follow default), both checks reduce to "enemy within 15yd of Hunter" — which is incompatible with Hunter's 35yd range identity.
2. Hunter v Mage was the canonical case: Mage stays at 30yd, so the 15yd-from-owner check is never true, so Spider Web never has a valid target, regardless of pet positioning.

The pet auto-attack damage assumed by the design comment at `combatant.rs:215` ("Auto Shot is the primary sustained damage") is theoretically real (7 base + 0.5× owner AP at 1.3s swing speed) but never fires because the pet is never in melee range of an enemy. The Hunter pet's actual damage contribution is ~0% of expected.

## Topic context

- Pet AI is wired but inert. `class_ai/pet_ai.rs` is 635 lines with `felhunter_ai`, `spider_ai`, `boar_ai`, `bird_ai`. Each pet has a decide function and emits to a `pet_decision` trace event. The infrastructure exists; the decisions just don't lead to action.
- Hunter pets are melee (`PetType::Spider/Boar/Bird` all set `is_melee: true` at `components/pets.rs:55-58`). Their ability ranges (Spider Web 20yd, Boar Charge 25yd, Master's Call 40yd) suggest pursuit was the intended model.
- Warlock Felhunter is a different shape — both its abilities (Spell Lock 30yd, Devour Magic 30yd) are ranged utility, so it functions adequately at owner range without pursuit. Felhunter is **not** in scope for this brainstorm.
- The friendly-CC guard in `class_ai/cast_guard.rs::pre_cast_ok` already centralizes CC-break prevention. New pet-applied roots (Spider Web has `break_on_damage_threshold: 80`) should integrate with the existing guard.

## Topic Axes

1. **Target acquisition** — assigning `combatant.target` for pets
2. **Pet ability dispatch** — Hunter AI decides when pet abilities fire (hybrid model)
3. **Pursuit movement** — pet closes on its target via existing movement code
4. **Heel behavior** — pet returns to Hunter on low HP

## Key Flows

- **F1. Pet engages enemy on Hunter's behalf**
  - **Trigger:** Hunter has an active target and the pet's target field is `None` (or stale)
  - **Actors:** Hunter AI, pet entity, movement system
  - **Steps:**
    1. Hunter AI sets `pet.target = owner.target` at the start of each tick when the Hunter has a target
    2. Movement system sees pet has a non-`None` target and routes to the target-pursuit branch (existing behavior at `movement.rs:296+`)
    3. Pet closes on target at `pet.base_movement_speed`
    4. Once in melee range (2.5yd), pet auto-attacks via the existing `combat_auto_attack` loop
  - **Outcome:** Pet auto-attack damage starts contributing; SpiderWeb / BoarCharge become reachable when the Hunter dispatches them
  - **Covered by:** R1, R3, R4

- **F2. Hunter commands a headline pet ability**
  - **Trigger:** Hunter AI per-tick evaluates pet abilities alongside its own abilities; pet ability's cooldown is ready and conditions are met
  - **Actors:** Hunter AI, pet entity
  - **Steps:**
    1. Hunter AI decides to dispatch a pet ability (e.g., Spider Web on a slowed gap-closer)
    2. Hunter AI marks the pet ability for execution (mechanism TBD by ce-plan — could be a `PetCommand` component or direct cooldown setting on the pet)
    3. Pet executes the ability on its next tick within the pet AI loop
    4. `pet_decision` trace event records `choose(ability, target, was_instant=true)` with the dispatch source attributed to Hunter
  - **Outcome:** Pet abilities fire more reliably because Hunter brain orchestrates timing, not pet brain trying to figure out positioning
  - **Covered by:** R2, R5

- **F3. Pet heels on low HP**
  - **Trigger:** Pet's `current_health / max_health < 0.25`
  - **Actors:** Hunter AI (or pet AI — site TBD by ce-plan), pet entity, movement system
  - **Steps:**
    1. Pet's target is cleared (`pet.target = None`)
    2. Movement system routes pet back to "follow owner at 3yd" branch
    3. Hunter AI suppresses pet ability dispatch while pet HP < 25% (so it doesn't get re-sent immediately)
    4. When pet HP recovers above 25% (heal events, regen, or pet death-and-respawn — currently pets don't respawn so this is one-shot), Hunter AI resumes assigning target
  - **Outcome:** Pets don't die mid-fight as often; Hunter retains the pet body for the remainder of the match
  - **Covered by:** R7, R8

## Requirements

**Target acquisition**
- R1. Pets receive a `combatant.target` assignment at the start of each AI tick when the pet's owner has a valid target. The default rule: `pet.target = owner.target` when present.
- R2. Pet target assignment respects the Heel state — when the pet is in Heel mode (R7), no target is assigned regardless of the owner's target.

**Pet ability dispatch (hybrid model)**
- R3. Hunter AI is responsible for deciding when to dispatch the pet's headline ability (Spider Web for Spider, Boar Charge for Boar, Master's Call for Bird). The exact dispatch mechanism — component vs cooldown signal vs direct invocation — is deferred to planning.
- R4. Pet AI continues to own pet auto-attack execution and pursuit movement. The existing `combat_auto_attack` and `move_to_target` paths are unchanged.
- R5. Hunter AI does not dispatch a pet ability when the pet is in Heel mode (R7) or when standard ability gates (cooldown, range, friendly-CC) reject the cast. The friendly-CC guard at `class_ai/cast_guard.rs::pre_cast_ok` must continue to apply to pet-applied roots.

**Spider Web filter**
- R6. The `dist_to_owner ≤ 15.0` filter in `spider_ai` (`class_ai/pet_ai.rs:374`+) is removed. Spider Web's only spatial constraint becomes its own `range` field in `abilities.ron` (currently 20yd, unchanged in this iteration).

**Heel behavior**
- R7. When a pet's `current_health / max_health < 0.25`, the pet enters Heel mode: target is cleared, ability dispatch is suppressed, pet returns to owner's flank.
- R8. Heel mode persists until pet HP recovers above 25%, OR until match end. (Pets currently do not respawn or self-heal in normal play, so Heel mode is typically one-shot per match per pet — confirm at planning.)

**Validation**
- R9. 2v2-with-healer matrix sweep at N=10 (autopilot) and N=100 (pre-merge): Hunter+Priest vs each-class+Priest. Compared against `design-docs/balance/matrix_baseline_2026-05-22_2v2_post.csv` (post-mana baseline).
- R10. 1v1 matrix sweep at N=20 (autopilot) and N=100 (pre-merge). Compared against `design-docs/balance/matrix_baseline_2026-05-22.csv`.
- R11. Decision-trace audit on Hunter v Warlock: confirm `SpiderWeb:NoValidTarget` rejection count drops by ≥75% from the ~1,000 pre-change baseline; confirm pet auto-attack damage events appear in the match log (currently zero).

## Acceptance Examples

- **AE1. Covers R1, F1.** Given a Hunter+Spider team and an enemy Warrior at 25yd from Hunter, when combat begins, then the Spider's target is set to the Warrior (or whatever the Hunter is currently targeting) within one AI tick.
- **AE2. Covers R3, F2.** Given a Spider with target set to an enemy within Spider Web range, when Hunter AI evaluates the next tick and Spider Web is off cooldown, then the Hunter AI dispatches Spider Web and a `pet_decision` trace event records a `choose(SpiderWeb, target, was_instant=true)` outcome.
- **AE3. Covers R5.** Given the Hunter is firing Aimed Shot at a target that is currently rooted by the Spider's Spider Web (80-damage break threshold), when the Aimed Shot would crit for damage exceeding 80, then `pre_cast_ok` rejects the Aimed Shot for friendly-CC-break and the trace records the rejection.
- **AE4. Covers R6.** Given the Spider has closed to within Spider Web range of an enemy 30yd from the Hunter, when Hunter dispatches Spider Web, then the ability fires (no `NoValidTarget` rejection from the removed owner-distance filter).
- **AE5. Covers R7, F3.** Given the Spider's current_health drops below 25% of max_health mid-match, when the next pet AI tick fires, then the Spider's target is cleared and the Spider returns to within 3yd of the Hunter.
- **AE6. Covers R7.** Given the Spider is in Heel mode (HP < 25%), when Hunter AI evaluates pet ability dispatch, then no pet ability is dispatched on this tick.

## Success Criteria

- 2v2 Hunter+Priest team winrate moves measurably against at least 2 of 6 paired matchups (target: at least 2 matchups move from 0% to ≥10%). Confirms pet engagement contributes meaningful pressure.
- 1v1 Hunter aggregate non-mirror winrate moves from ~1.4% (post-mana baseline) to ≥5% as a soft signal; matchups with shorter durations (Hunter v Mage, Hunter v Warlock) where the pet has time to engage should move more than matchups gated by other issues.
- Decision-trace audit on Hunter v Warlock confirms `SpiderWeb:NoValidTarget` rejection count drops ≥75% from ~1,000 baseline.
- Match log shows pet auto-attack damage events in ≥80% of Hunter matches (currently 0%).
- Downstream agent (ce-plan) can execute this brainstorm without inventing pet AI behavior, target-assignment rules, or Heel semantics.

## Scope Boundaries

- **Pet ability cooldown tuning** (Spider Web 45s → ~15s, Boar Charge 45s → ~18s, Master's Call 45s → ~25s) — deferred. Approach A intentionally isolates AI changes as the only variable; CD tuning becomes the immediate follow-up if A's matrix signal is muted by 45s CDs firing 0-1 times per match.
- **Spider Web range tuning** (20yd → 30yd to align with Hunter kit range) — deferred to the CD tuning follow-up.
- **Minion framework generalization** (lift Send/Heel architecture into a shared `Minion` subsystem usable by Warlock Felhunter and future pets) — deferred. Felhunter doesn't pursue today, so it doesn't exercise the pursuit subsystem we'd be generalizing.
- **Pet HP / damage stat tuning** — out of scope. Pets get 45% of owner's max_health; staying with current values to keep variables minimal.
- **Pet AI changes for Warlock Felhunter** — out of scope. Felhunter operates as ranged utility from owner range and does not need the pursuit changes.
- **Other Hunter rebalance survivors** — Devour Magic counter (`docs/ideation/2026-05-22-hunter-rebalance-ideation.md` #4), predictive trap placement (#5), Disengage follow-through (#6), team-comp awareness for trap targeting — separately tracked, all out of scope here.
- **Trace audit pattern codification** (#7) — out of scope.

### Deferred to Follow-Up Work

- If matrix signal at A is muted (pet abilities firing 0-1 times per match), the immediate follow-up is the cooldown tuning + Spider Web range alignment that was deferred from this iteration (Approach B in the brainstorm). Re-runs through the same brainstorm → plan → work cycle.
- If pet survivability becomes a problem (pets die before reaching engaging-mid-fight in most matches), revisit Heel threshold (25% may need to be 30-40%) and/or pet HP scaling.

## Key Decisions

- **Hunter-controlled command model over autonomous attacker.** Rationale: pet feels like a tool the Hunter wields rather than a second autonomous combatant. Hunter AI gets a meaningful new decision layer (pet ability dispatch) and pet identity becomes more distinct between Spider/Boar/Bird variants. Tradeoff: more AI surface to maintain in `class_ai/hunter.rs` vs simpler autonomous logic in `class_ai/pet_ai.rs`.
- **Hybrid ability control over Hunter-commands-everything.** Rationale: pet auto-attacks and pursuit don't need centralized scheduling — those are mechanical pet behaviors. Headline abilities (Spider Web, Boar Charge, Master's Call) are tactical moments worth Hunter brain attention. Pet AI stays the home for execution; Hunter AI owns the choreography.
- **Minimal Send/Heel triggers (always engage unless emergency) over reactive or tactical triggers.** Rationale: the Send/Heel framing's value comes from being able to recall the pet, not from sophisticated send logic. "Always engage" is the simplest viable default; "Heel on low HP" prevents pet death from being a one-time-per-match event. Tactical send-on-burst-window logic can be layered later if needed.
- **Approach A (AI-only) over Approach B (AI + CD/range tuning).** Rationale: shipping only AI changes isolates the variable for matrix validation. If A moves outcomes meaningfully, AI architecture was sufficient. If A doesn't move outcomes, we know CD tuning is required and ship B as fast follow. Avoids confounded matrix readings.

## Dependencies / Assumptions

- **Assumes the friendly-CC guard at `class_ai/cast_guard.rs::pre_cast_ok` already covers pet-applied roots.** Spider Web's `break_on_damage_threshold: 80` means a single Aimed Shot crit (~40-50 damage post-buffs) won't exceed it, but a combined Aimed Shot + Auto Shot in the same frame could. If `pre_cast_ok` doesn't already integrate pet-applied roots into its friendly-CC check, that integration becomes a blocker for this iteration. Verify at planning.
- **Assumes existing target-pursuit movement at `movement.rs:296+` is sufficient for pets.** Pet movement uses `base_movement_speed` from `components/pets.rs:37-49` (currently 5.5 for Felhunter/Spider/Bird, 6.0 for Boar). If pet movement turns out to be too slow to close on kiting enemies, pet speed tuning becomes an in-scope follow-up.
- **Assumes the AI decision trace's `pet_decision` event kind continues to work after the Hunter-dispatches-pet-ability change.** The hybrid model may shift the source of `builder.choose(ability, ...)` calls from `pet_ai.rs` to `hunter.rs`. The trace builder API needs to accept "Hunter dispatched this pet ability" semantics; otherwise the trace audit (R11) loses fidelity.
- **Assumes Hunter v Mage's 10.5s defeats won't move from this change.** Mage Frostbolt + Frost Nova kills the Hunter before the pet has time to close on the Mage. This matchup is gated by Hunter survivability, not pet engagement. Validation should expect no movement here and not treat that as a failure.
- **Assumes 25% as the Heel threshold is reasonable.** Could be 20% (last-second emergency) or 30% (more conservative). Tune at validation time based on observed pet death rates.

## Outstanding Questions

### Deferred to Planning

- [Affects R3][Technical] **Mechanism for Hunter-dispatched pet abilities:** does the Hunter AI directly cast on the pet (writing to `pet.casting_state` or similar), introduce a `PetCommand` component the pet AI reads on next tick, or signal via a queue? Planner picks based on existing component patterns and what minimizes friendly-CC guard complexity.
- [Affects R7][Technical] **Where does the Heel logic live:** Hunter AI (sets pet.target = None) or pet AI (checks own HP and clears its target)? Both work; planner picks based on which keeps `pet_decision` traces cleaner.
- [Affects R11][Technical] **Trace event attribution for Hunter-dispatched pet abilities:** does the `pet_decision` event mark the dispatch source (`dispatched_by: owner` vs `autonomous`)? If yes, this affects audit recipes for R11 verification. Resolve at planning.
- [Affects R8][Needs research] **Pet HP recovery during a match:** do pets receive party heals (e.g., from a healer ally)? If yes, Heel mode can exit mid-match when an ally heals the pet above 25%. If no, Heel is one-shot per pet per match. Confirm by reading current pet damage application code.
