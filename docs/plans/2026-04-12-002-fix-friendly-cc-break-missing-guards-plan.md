---
title: "fix: Add missing friendly-CC-break guards to all damage abilities"
type: fix
status: completed
date: 2026-04-12
origin: docs/reports/2026-04-12-bug-hunt-2v2-3v3.md
---

# fix: Add missing friendly-CC-break guards to all damage abilities

## Overview

The `has_friendly_breakable_cc()` guard was added in March 2026 to prevent AI from breaking own team's Polymorph, but only 5 DoT/debuff abilities were guarded. Direct damage abilities (Mortal Strike, Sinister Strike, Ambush, Shadow Bolt, etc.) were missed, causing DPS to attack friendly-Polymorphed targets and break the CC.

## Problem Frame

The March fix (documented in `docs/solutions/ai-decision-patterns/friendly-cc-break-prevention.md`) established the correct pattern — a `has_friendly_breakable_cc()` helper on `CombatContext` — and applied it to Warlock DoTs (Corruption, Immolate, Curses), Warrior's Rend, and Mage's Frostbolt. But direct damage abilities from all classes were left unguarded.

Bug hunt evidence:
- **m06** (seed 6006): Rogue Sinister Strikes a Warrior Polymorphed by own team's Mage
- **m13** (seed 6020): Warrior Mortal Strikes a Paladin Polymorphed by own team's Mage

## Requirements Trace

- R1. All damage-dealing `try_*` functions must check `has_friendly_breakable_cc()` before committing to damage on the target
- R2. The helper should be data-driven (threshold-based, not hardcoded to specific AuraTypes) so future CCs like Blind, Sap, and Freezing Trap are automatically covered
- R3. When all valid targets have friendly breakable CC, the combatant should idle rather than break CC

## Scope Boundaries

- Does NOT address auto-attacks breaking friendly CC — auto-attacks are handled by `combat_auto_attack` in Phase 3, which has no access to `CombatContext`. That's a separate, larger change.
- Does NOT address `try_heroic_strike` — it sets `next_attack_bonus_damage` without specifying a target; the damage lands via the auto-attack system
- Does NOT address CC-on-CC waste (e.g., Kidney Shot on a Poly'd target) — that's a targeting optimization, not a CC break bug
- Does NOT change `process_aura_breaks` or the aura break-on-damage mechanics

## Context & Research

### Relevant Code and Patterns

- **Helper**: `has_friendly_breakable_cc()` on `CombatContext` at `class_ai/mod.rs:237` — checks `AuraType::Polymorph | AuraType::Incapacitate` with friendly caster
- **Existing guard pattern**: `try_rend` (warrior.rs:432), `try_frostbolt` (mage.rs:567), `try_corruption` (warlock.rs:227), `try_immolate` (warlock.rs:303), `try_cast_curse` (warlock.rs:658)
- **Guard placement**: At the top of each `try_*` function, before cooldown/range/resource checks: `if ctx.has_friendly_breakable_cc(target_entity) { return false; }`

### Institutional Learnings

- `docs/solutions/ai-decision-patterns/friendly-cc-break-prevention.md` — full pattern documentation, including checklist for new abilities
- `docs/plans/2026-03-22-fix-friendly-dot-breaks-polymorph-plan.md` — the original fix plan (completed)
- The learnings doc explicitly states: "Every `try_<damage_ability>()` function should ask 'Is a teammate's CC on this target?' before doing anything."

## Key Technical Decisions

- **Update helper to be threshold-based**: The current `has_friendly_breakable_cc()` matches `AuraType::Polymorph | AuraType::Incapacitate`. This requires updating the match arm every time a new break-on-any-damage CC is added. Change to check `a.break_on_damage_threshold == 0.0` instead, which is fully data-driven. Any aura configured with `break_on_damage_threshold: 0.0` in `abilities.ron` will automatically be respected. Non-breakable auras use negative thresholds (-1.0), so there's no false-positive risk.

- **Guard placement at `try_*` level, not target-selection level**: Putting the guard in each `try_*` function (not `acquire_targets`) ensures it works with the freshest aura snapshot data and matches the established pattern. The DPS idles when all targets are CC'd rather than switching targets — this is correct behavior since breaking CC is worse than waiting.

## Open Questions

### Resolved During Planning

- **Should the guard also cover threshold-based CCs like Frost Nova (35)?** No. The learnings doc explicitly categorizes Root (35.0 threshold) and Fear (100.0 threshold) as "OK to DoT" because they can absorb significant damage before breaking. Only `0.0` threshold (break on ANY damage) warrants skipping.

- **Which functions need `target_entity` resolution?** `try_mind_blast` and `try_holy_shock_damage` resolve target from `combatant.target` internally rather than receiving it as a parameter. The guard goes after that resolution, before any resource/cooldown checks.

## Implementation Units

- [ ] **Unit 1: Update `has_friendly_breakable_cc` to threshold-based check**

  **Goal:** Make the helper data-driven so future break-on-any-damage CCs are automatically covered.

  **Requirements:** R2

  **Dependencies:** None

  **Files:**
  - Modify: `src/states/play_match/class_ai/mod.rs` (`has_friendly_breakable_cc` method)

  **Approach:**
  - Replace the `matches!(a.effect_type, AuraType::Polymorph | AuraType::Incapacitate)` check with `a.break_on_damage_threshold == 0.0`
  - Keep the friendly-caster team check unchanged
  - Update the docstring to describe the threshold-based behavior

  **Patterns to follow:**
  - The existing helper at `class_ai/mod.rs:237`

  **Test scenarios:**
  - Happy path: Target has friendly Polymorph (threshold 0.0) -> returns true
  - Happy path: Target has enemy Polymorph (threshold 0.0) -> returns false (different team)
  - Edge case: Target has friendly Frost Nova root (threshold 35.0) -> returns false (non-zero threshold)
  - Edge case: Target has friendly Fear (threshold 100.0) -> returns false
  - Edge case: Target has no auras -> returns false
  - Edge case: Target has friendly DoT (threshold -1.0) -> returns false

  **Verification:**
  - All 5 existing callsites continue to work correctly (Rend, Frostbolt, Corruption, Immolate, Curses)
  - Existing headless matches produce identical outcomes (the helper's behavior is unchanged for Polymorph/Incapacitate)

- [ ] **Unit 2: Add guard to all missing damage `try_*` functions**

  **Goal:** Every damage-dealing ability checks for friendly breakable CC before proceeding.

  **Requirements:** R1, R3

  **Dependencies:** Unit 1

  **Files:**
  - Modify: `src/states/play_match/class_ai/warrior.rs` — `try_mortal_strike`, `try_charge`
  - Modify: `src/states/play_match/class_ai/rogue.rs` — `try_ambush`, `try_sinister_strike`
  - Modify: `src/states/play_match/class_ai/warlock.rs` — `try_shadowbolt`, `try_drain_life`
  - Modify: `src/states/play_match/class_ai/priest.rs` — `try_mind_blast`
  - Modify: `src/states/play_match/class_ai/paladin.rs` — `try_holy_shock_damage`
  - Modify: `src/states/play_match/class_ai/hunter.rs` — `try_aimed_shot`, `try_arcane_shot`, `try_concussive_shot`
  - Test: headless match reproduction with seeds 6006 (m06) and 6020 (m13)

  **Approach:**
  - Add `if ctx.has_friendly_breakable_cc(target_entity) { return false; }` near the top of each function, after target resolution but before cooldown/range/resource checks
  - For `try_mind_blast` and `try_holy_shock_damage` which resolve target from `combatant.target` internally, place the guard immediately after the `let Some(target_entity) = combatant.target` line
  - For `try_charge`, the guard prevents charging into a friendly-Poly'd target (which would break the Poly with charge damage)

  **Complete list of functions needing the guard (11 total):**

  | File | Function | Notes |
  |------|----------|-------|
  | warrior.rs | `try_mortal_strike` | Takes target_entity param |
  | warrior.rs | `try_charge` | Takes target_entity param |
  | rogue.rs | `try_ambush` | Takes target_entity param |
  | rogue.rs | `try_sinister_strike` | Takes target_entity param |
  | warlock.rs | `try_shadowbolt` | Takes target_entity param |
  | warlock.rs | `try_drain_life` | Takes target_entity param |
  | priest.rs | `try_mind_blast` | Resolves target from combatant.target |
  | paladin.rs | `try_holy_shock_damage` | Resolves target from combatant.target |
  | hunter.rs | `try_aimed_shot` | Takes target_entity param |
  | hunter.rs | `try_arcane_shot` | Takes target_entity param |
  | hunter.rs | `try_concussive_shot` | Takes target_entity param |

  **Patterns to follow:**
  - Existing guard in `try_rend` at `warrior.rs:432`:
    ```
    if ctx.has_friendly_breakable_cc(target_entity) { return false; }
    ```

  **Test scenarios:**
  - Happy path: Rogue uses Sinister Strike on target with NO friendly CC -> attack proceeds normally
  - Bug fix: Rogue targets enemy with friendly Polymorph -> Sinister Strike returns false, Poly is not broken
  - Bug fix: Warrior targets enemy with friendly Polymorph -> Mortal Strike returns false, Poly is not broken
  - Edge case: Only remaining enemy has friendly Polymorph -> all damage abilities return false, combatant idles until Poly expires, then resumes attacking
  - Edge case: Shadow Bolt cast-start on Poly'd target -> cast doesn't begin, mana not spent
  - Integration: m06 config (seed 6006, Rogue+Mage vs Warrior+Priest) -> after Priest dies and Mage Polys Warrior, Rogue should idle until Poly expires instead of attacking the Warrior
  - Integration: m13 config (seed 6020, WMP vs RWlPal) -> after Mage Polys Paladin, Warrior should attack Rogue/Warlock instead of Mortal Striking the Paladin

  **Verification:**
  - Run m06 (seed 6006): No `[DMG]` from Team 1 Rogue on Team 2 Warrior while Polymorph is active
  - Run m13 (seed 6020): No `[DMG]` from Team 1 Warrior on Team 2 Paladin while Polymorph is active
  - Run 2-3 additional diverse matches to confirm no regressions in normal combat damage

- [ ] **Unit 3: Update learnings doc coverage table**

  **Goal:** Keep the institutional knowledge current with the expanded coverage.

  **Requirements:** Documentation accuracy

  **Dependencies:** Unit 2

  **Files:**
  - Modify: `docs/solutions/ai-decision-patterns/friendly-cc-break-prevention.md`

  **Approach:**
  - Update the "Applied to" line (line 86) to include all covered abilities
  - Note the threshold-based helper change
  - Update the "Currently Covered CC Types" table if any new AuraTypes were added

  **Verification:**
  - Learnings doc accurately reflects all guarded functions

## System-Wide Impact

- **Interaction graph:** Only `try_*` functions in class AI modules are modified. No changes to system registration, combat core, or aura processing.
- **Unchanged invariants:** Aura break-on-damage mechanics unchanged. Auto-attack behavior unchanged. `process_aura_breaks` unchanged. Enemy damage still breaks CC as expected.
- **API surface parity:** The `has_friendly_breakable_cc()` helper signature is unchanged (only internal implementation moves to threshold-based).

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| DPS idles too long when only target is CC'd | This is correct behavior — breaking CC is worse than waiting. Poly duration is 10s max, shortened by DR. The DPS resumes immediately when Poly expires. |
| `break_on_damage_threshold == 0.0` matches a non-CC aura | No existing non-CC aura uses 0.0 threshold. Non-breakable auras use -1.0. This is a convention, not a hard constraint, but any aura with threshold 0.0 SHOULD be protected from friendly damage. |
| Auto-attacks still break Poly | Out of scope. Auto-attacks from melee in range will break Poly. In practice, Poly targets are usually ranged enemies away from melee. The ability-level guard prevents the most impactful breaks (burst abilities). |

## Sources & References

- **Origin:** [Bug Hunt Report](docs/reports/2026-04-12-bug-hunt-2v2-3v3.md) — BUG-1
- **Pattern doc:** [Friendly CC Break Prevention](docs/solutions/ai-decision-patterns/friendly-cc-break-prevention.md)
- **Original fix plan:** [docs/plans/2026-03-22-fix-friendly-dot-breaks-polymorph-plan.md](docs/plans/2026-03-22-fix-friendly-dot-breaks-polymorph-plan.md)
