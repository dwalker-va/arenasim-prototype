---
title: "feat: Add Unstable Affliction ability to Warlock"
type: feat
status: active
date: 2026-04-18
origin: docs/brainstorms/2026-04-18-unstable-affliction-warlock-requirements.md
---

# feat: Add Unstable Affliction ability to Warlock

## Overview

Add Unstable Affliction (UA) as a new Warlock Shadow-school DoT whose dispel triggers a dual backlash on the enemy dispeller: a direct Shadow damage burst plus a 5-second blanket Silence that blocks all mana-cost abilities. Introduces `AuraType::Silence` as a new reusable primitive with its own diminishing-returns category, and plumbs a dispeller entity reference through the dispel pipeline so backlash can be attributed correctly.

UA coexists with Corruption, follows the existing dispel RNG (dispels pick a random magic debuff), and is bound into Warlock AI as part of the standard opener alongside Corruption. Scope covers the ability itself, the Silence primitive, all game-logic plumbing, visual effects (DoT indicator, backlash burst, silence status text), ability-list UI, and icon integration.

## Problem Frame

The 2026-04-12 bug-hunt report flagged Warlock as the worst-performing class (20% 2v2 win rate, 38% 3v3). Root cause identified in that report: DoT-dependent damage profile combined with Priests/Paladins instantly dispelling those DoTs the moment they land. In two matches, Warlock dealt fewer than 20 total damage. UA taxes that dispel counterplay twice — first with immediate HP loss, then with a 5s cast lockout — so healers face a real cost to stripping Warlock DoTs.

UA does not address the companion "Warlock dies first in every 2v2" failure mode. The risk that the 2v2 numeric target won't move has been accepted; the 3v3 target is the more meaningful success bar for the mechanic itself.

## Requirements Trace

From `docs/brainstorms/2026-04-18-unstable-affliction-warlock-requirements.md`:

- **R1** — `UnstableAffliction` as Warlock Shadow-school DoT; `break_on_damage_threshold: -1.0`
- **R2** — Coexists with Corruption on same target
- **R3** — Magic-dispellable; dispels pick a random magic debuff (unchanged)
- **R4 / R4a** — On enemy-team dispel: Silence aura + Shadow damage burst (SP-scaling, stored on aura at cast time)
- **R5 / R6** — Silence gates: `resource_type == Mana && mana_cost > 0`; no effect on rage, energy, or zero-cost abilities
- **R7** — Silence fires only on qualifying enemy dispel
- **R8** — Refresh rule: reset to 5s; DR category `DRCategory::Silence` handles chain dispels
- **R9** — Warlock AI applies UA as part of opener
- **R10** — Friendly-CC-break guard via explicit `has_friendly_breakable_cc` call (not automatic)
- **R11** — Backlash team-comparison via `removed_aura.caster`; mirror-match symmetric
- **R12** — Silence is magic-dispellable (ally can cleanse a silenced teammate)
- **R13** — Distinct on-target DoT visual (readable, distinct from Corruption)
- **R14** — Backlash animation on dispeller at silence application (distinct from `DispelBurst`)
- **R15** — Floating "Silenced" status text over head
- **R16** — UA icon in view-combatant screen, ability usage timeline, debuff display, loadout
- **R17** — Silence visible in view-combatant debuff list
- **R18** — Download authentic UA icon from Wowhead

Success criteria (from origin doc):
- **SC1** — Warlock 2v2 WR 20% → ≥35%, 3v3 38% → ≥50% on seeds 6001–6031 (2v2 risk accepted)
- **SC2** — At least 3/24 matches show a dispeller Silenced during a kill window with a heal cast denied
- ~~**SC3**~~ — *Removed 2026-04-18: the 40% uptime threshold did not correspond to any natural DR boundary and conflicted with the DR math (5s + 2.5s in a 15s window = 50% uptime by design). The DR cap at 3 applications via `DRCategory::Silence` already provides the hard ceiling on runaway silence. If Unit 9 tuning reveals genuine runaway patterns, adjust DR or Silence duration directly rather than defining a numeric success threshold.*
- **SC4** — Dispel pipeline still works correctly for all other dispellable auras

## Scope Boundaries

- UA does not address Warlock survivability ("dies first" problem). Separate workstream (Death Coil / Shadow Ward / Healthstone) — tracked as out of scope here.
- Silence primitive is introduced with only UA backlash as consumer; future Mage Counterspell and Priest Silence reuse are anticipated but not implemented.
- Existing dispel RNG behavior is unchanged — dispellers still remove a random magic debuff.
- No rework of Corruption or any existing Warlock ability.
- Exact tuning of the balance levers (UA DoT damage-per-tick, mana cost, cast time, backlash damage base/coefficient) will be converged via simulation during Unit 9; this plan starts from the brainstorm's conservative seed values.

### Deferred to Separate Tasks

- Warlock survivability primitive (new workstream): not in this PR.
- Generalizing Silence to Priest Silence / Mage Counterspell: future features will consume the `AuraType::Silence` primitive introduced here.

## Context & Research

### Relevant Code and Patterns

- **Aura system:** `src/states/play_match/components/auras.rs` — `AuraType` enum (~L12), `is_magic_dispellable()` (~L91), `Aura` struct with `caster: Option<Entity>` (~L125), `DRCategory` enum (~L307), `DRCategory::from_aura_type` (~L324), `DRStates` fixed-size array indexed by discriminant (~L349).
- **DR constants:** `src/states/play_match/constants.rs` — `DR_RESET_TIMER = 15.0`, `DR_IMMUNE_LEVEL = 3`, `DR_MULTIPLIERS = [1.0, 0.5, 0.25, 0.0]`.
- **Dispel pipeline:** `src/states/play_match/effects/dispels.rs` — `process_dispels` removes a random magic aura, logs, spawns visual `DispelBurst`, and optionally heals Felhunter caster.
- **Pending dispel struct:** `src/states/play_match/components/combatant.rs` (`DispelPending` around L603) — needs a new `dispeller: Entity` field.
- **Ability registration:** `src/states/play_match/abilities.rs` — `AbilityType` enum (L45), `can_cast_config` (L114) is the single gate for most instant-cast abilities. `is_spell_school_locked` helper (L151) is the template for a parallel `is_silenced` helper.
- **Config loading:** `src/states/play_match/ability_config.rs` — `validate()` has an `expected_abilities` array that must be updated when new `AbilityType` variants are added.
- **Warlock AI:** `src/states/play_match/class_ai/warlock.rs` — `try_corruption` is the pattern for instant-apply DoT with friendly-CC-break guard.
- **Dispel call sites that need the new `dispeller` field:**
  - `src/states/play_match/class_ai/mod.rs` ~L402 (Priest Dispel Magic, Paladin Cleanse)
  - `src/states/play_match/class_ai/pet_ai.rs` ~L364 (Felhunter Devour Magic)
  - `src/states/play_match/class_ai/pet_ai.rs` ~L602 (Hunter Bird Master's Call) — filters to Root/MovementSpeedSlow only, so UA (a DoT) can never be dispelled here; the `dispeller` field must still be populated for compilation, but the team-comparison check in Unit 4 will correctly never fire backlash from this path.
- **Cast-time mana deduction:** `src/states/play_match/combat_core/casting.rs` ~L189 — second enforcement site for Silence.
- **Dispel-helper bypass:** `src/states/play_match/class_ai/mod.rs` ~L394 — this path queues a dispel and deducts mana directly, bypassing `can_cast_config`; needs its own Silence check.
- **Friendly-CC-break guard:** `src/states/play_match/class_ai/mod.rs` ~L239 (`has_friendly_breakable_cc`). Per-ability opt-in; `try_corruption` is the reference call site.
- **Divine Shield clear list:** `src/states/play_match/effects/divine_shield.rs` ~L54 — add `AuraType::Silence` to the sweep so bubbling clears an in-progress silence.
- **View-combatant UI:** `src/states/view_combatant_ui.rs` — `get_class_abilities()` (~L210) hardcodes class ability lists; Warlock entry at ~L247 (`AbilityType::Corruption`). Ability display-name map at ~L296.
- **Icon paths:** `src/states/play_match/rendering/mod.rs` — `get_aura_icon_key` (~L51) resolves aura icons by matching ability definition; UA icon flows through the ability-config path automatically once the `icon:` field is set.
- **Visual effects pattern:** `src/states/play_match/rendering/effects.rs` — spawn/update/cleanup three-system pattern (see project memory).

### Institutional Learnings

- **`docs/solutions/implementation-patterns/graphical-mode-missing-system-registration.md`** — New systems must be registered in both `src/states/play_match/systems.rs` (headless) and `src/states/mod.rs` (graphical). Symptom of missing graphical registration is AI logs ability cast but `[BUFF]`/`[DMG]` absent. The core Silence-backlash logic (game state) must register in both; the visual-only systems register in `src/states/mod.rs` only.
- **`docs/solutions/implementation-patterns/adding-visual-effect-bevy.md`** — Spawn/update/cleanup three-system pattern. Use `AlphaMode::Add` not `Blend`. Use `Res<Time>` not `Time<Real>`. Use `try_insert()` not `insert()`. Separate `.add_systems()` groups per effect type.
- **`docs/solutions/implementation-patterns/adding-new-class-paladin.md`** — Precedent for the AI + abilities.ron + validation + view-combatant wiring checklist. The UA ability addition follows the same end-to-end steps as adding a class ability.

### External References

Not required — the feature extends established internal patterns (aura application, DR integration, visual effects). Wowhead MCP is consulted only for the UA spell icon asset.

## Key Technical Decisions

- **Silence as a new `AuraType`, not an extension of `SpellSchoolLockout`** (see origin). The predicate is cost-based, not school-based — two genuinely different CC semantics. Cost: one new variant and ~6 plumbing sites (DR table, dispellability, Divine Shield clear list, HUD color, icon map, is_silenced helper). Benefit: clean primitive reusable by Priest Silence / Mage Counterspell.
- **Backlash damage stored on the aura at cast time** (R4a). If the Warlock dies before the dispel, backlash damage and Silence still land at their snapshot values. Avoids the "caster is dead, what spell power do we use?" class of bugs caught in recent bug hunts.
- **Damage applies before Silence.** Ordering matters for log readability and for the edge case where the backlash damage kills the dispeller (dead combatants shouldn't receive new auras). Matches the established `[DMG]` → `[BUFF]` log ordering.
- **`DRCategory::Silence` as its own DR bucket** (see origin). Prevents runaway silence chains: first silence 5s, second 2.5s, third 0s (immune for `DR_RESET_TIMER = 15s`). `DRStates` uses a fixed-size `[DRState; N]` array — bumping `DRCategory::COUNT` from 5 to 6 is the entire data-structure change.
- **Silence gates only `resource_type == Mana && mana_cost > 0`** (see origin). A silenced Warrior can still Mortal Strike; a silenced Paladin cannot Flash of Light or Hammer of Justice. Divine Shield, if zero-cost in current abilities.ron, remains usable — matches the known-issue behavior already documented.
- **Two enforcement sites for Silence**, not one (see origin Dependencies). Instant abilities gate via `can_cast_config`; cast-time abilities gate via `combat_core/casting.rs`. Additionally, the dispel helper in `class_ai/mod.rs:394` bypasses `can_cast_config` entirely and needs its own check — otherwise a silenced healer would still successfully dispel.
- **Healer AI is unchanged** (see origin). Dispellers continue to dispel on any dispellable aura. The mechanic's tension emerges from the random-debuff RNG, not from any AI skill-check.
- **Download real icon from Wowhead** via MCP rather than placeholder. Follows the pattern used for the strategic-options work on 2026-04-05.

## Open Questions

### Resolved During Planning

- **Where does `process_dispels` get the dispeller entity?** Via new `DispelPending.dispeller: Entity` field, populated at all three call sites. Resolved in Unit 2.
- **Is UA's break_on_damage_threshold specified?** Yes — `-1.0` (never breaks on damage). Confirmed in origin R1.
- **Is `Aura.caster` already populated for magic DoTs?** Yes, confirmed via `AuraPending::from_ability` in the origin's verified-dependency section.
- **What icon key does UA use in the HUD?** The existing ability-icon pipeline in `rendering/mod.rs` derives the key from `ability_definitions`; once `icon:` is set on the `abilities.ron` entry, all HUD/UI paths pick it up automatically. No custom key needed.

### Deferred to Implementation

- **Exact balance numbers** (UA cast time, mana cost, DoT tick damage, DoT duration, backlash damage base + SP coefficient): starting values from origin — cast instant, mana 30, DoT 8 dmg/3s × 5 ticks = 40 base over 15s, backlash 40 + 0.3×SP. Final values converge via Unit 9 simulation loop.
- **Visual effect concrete design** (colors, particle behavior, scale for the DoT indicator, backlash burst, silence status text): the plan specifies pattern and registration; specific visual tuning lands during implementation with screenshot feedback.
- **Should the UA on-target visual effect stack visibly when both Corruption and UA are applied?** Prefer distinct stacking — Corruption has one visual, UA has another, and the two coexist. Concrete coordination deferred to Unit 7.

## Implementation Units

- [ ] **Unit 1: Extend aura system with Silence type and DR category**

**Goal:** Add the `AuraType::Silence` primitive, register it with the DR system, make it magic-dispellable, and update the Divine Shield sweep to clear it.

**Requirements:** R3, R5, R6, R8, R12

**Dependencies:** None (foundational).

**Files:**
- Modify: `src/states/play_match/components/auras.rs`
- Modify: `src/states/play_match/effects/divine_shield.rs`
- Modify: `src/states/play_match/abilities.rs` (add `is_silenced` helper alongside `is_spell_school_locked`)
- Modify: `src/states/play_match/rendering/mod.rs` (extend the exhaustive `AuraType` match in `get_aura_icon_key` — add a `Silence` arm mapping to `"aura_silence"` fallback key)
- Create: `assets/icons/auras/silence.jpg` (generic silence icon; used when an ability's own icon isn't available)
- Test: `src/states/play_match/components/auras.rs` (add unit tests in the existing test module if present; otherwise new `#[cfg(test)]` block)

**Approach:**
- Add `AuraType::Silence` variant with doc-comment describing the predicate (gates mana-cost abilities for combatants whose resource type is Mana).
- Extend `is_magic_dispellable()` to include `AuraType::Silence`.
- Add `DRCategory::Silence` variant; bump `DRCategory::COUNT` from 5 to 6. The `DRStates` array widens automatically via the constant.
- Extend `DRCategory::from_aura_type` match to map `AuraType::Silence` → `Some(DRCategory::Silence)`. Note: the origin brainstorm's Dependencies section originally said "Silence has no DR; freestanding" — that note is superseded by R8. `from_aura_type` MUST return `Some(DRCategory::Silence)`; this mapping is load-bearing for the chain-dispel immunity behavior.
- Add `is_silenced(caster: &Combatant, auras: Option<&ActiveAuras>) -> bool` in `src/states/play_match/abilities.rs`. Returns true only when `caster.resource_type == ResourceType::Mana` and any active aura has `effect_type == AuraType::Silence`.
- Extend Divine Shield's aura sweep list (`effects/divine_shield.rs` ~L54) to include `AuraType::Silence` so bubbling clears an in-progress silence.
- Extend the exhaustive `AuraType` match in `rendering/mod.rs::get_aura_icon_key` (it has no wildcard arm — every variant is enumerated; adding `AuraType::Silence` without updating this match will break the build). Map `AuraType::Silence` → `"aura_silence"` icon key with fallback asset at `assets/icons/auras/silence.jpg`. When UA-derived silence auras land, they inherit UA's own icon via the existing ability-name path; the `"aura_silence"` asset is the generic fallback for future non-UA Silence consumers.

**Patterns to follow:**
- `is_spell_school_locked` (`abilities.rs:151`) — exact shape for the new `is_silenced` helper (iteration over `ActiveAuras.auras` with a type match).
- Existing `DRCategory::from_aura_type` entries for `Fear`, `Polymorph`, `Stun` — mirror the same structure.

**Test scenarios:**
- Happy path: `is_silenced` returns true when a Mana-resource combatant has an active `AuraType::Silence` aura.
- Edge case: `is_silenced` returns false for a Rage combatant (Warrior) even when a Silence aura is present (defensive — shouldn't happen in normal flow, but the predicate is resource-gated).
- Edge case: `is_silenced` returns false for a Mana combatant with no Silence aura but other auras active.
- Happy path: `DRCategory::from_aura_type(&AuraType::Silence)` returns `Some(DRCategory::Silence)`.
- Integration: applying a `Silence` aura to a combatant with DR level 1 in `DRCategory::Silence` returns a half-duration aura (2.5s from a 5s request).
- Integration: third Silence application returns 0s duration (immune); state resets after `DR_RESET_TIMER` seconds.
- Happy path: `is_magic_dispellable()` returns true for `AuraType::Silence`.
- Integration: Divine Shield application clears an active `Silence` aura from the shielded combatant.

**Verification:**
- `cargo test` passes, including the new DR tests for Silence.
- A headless run with a manual Silence application via test harness shows the aura applied, then removed after 5s (or sooner under DR).

---

- [ ] **Unit 2: Plumb dispeller entity through `DispelPending`**

**Goal:** Give `process_dispels` access to the dispeller's `Entity` so downstream backlash logic (Unit 4) can apply Silence and damage to the correct combatant.

**Requirements:** R4, R11

**Dependencies:** None.

**Files:**
- Modify: `src/states/play_match/components/combatant.rs` (DispelPending struct ~L603)
- Modify: `src/states/play_match/class_ai/mod.rs` (Priest/Paladin dispel call site ~L402)
- Modify: `src/states/play_match/class_ai/pet_ai.rs` (Felhunter Devour Magic call site ~L364 AND Hunter Bird Master's Call ~L602)
- Test: `src/states/play_match/effects/dispels.rs` (or a colocated test module)

**Approach:**
- Add `pub dispeller: Entity` field to `DispelPending` (non-optional — every caller has the entity in scope at the spawn site).
- Update **all four** existing call sites to populate `dispeller`: Priest Dispel Magic, Paladin Cleanse, Felhunter Devour Magic, Hunter Bird Master's Call.
- `process_dispels` itself does not use the field in this unit — it just passes through. The consumer is Unit 4.
- Master's Call will never trigger backlash (filter restricts to Root/MovementSpeedSlow so UA cannot be removed via that path), but the field is still required for compilation and for consistent logging.

**Patterns to follow:**
- Existing `DispelPending` construction at the three call sites — match the field ordering convention.

**Test scenarios:**
- Happy path: After a Priest casts Dispel Magic, the spawned `DispelPending` entity has `dispeller` set to the Priest's entity.
- Happy path: After Felhunter Devour Magic, `DispelPending.dispeller` is the Felhunter entity.
- Integration: a headless match with Priest vs Warlock completes without panic (validates all `DispelPending` construction sites compile and run).

**Verification:**
- `cargo build` compiles cleanly — no `DispelPending` construction site left unmigrated.
- Existing dispel tests still pass (dispel of Corruption/Fear/Polymorph still works correctly — SC4).

---

- [ ] **Unit 3: Register `UnstableAffliction` ability (config + enum + icon)**

**Goal:** Make UA a real ability the ability system and AI can reference. No game logic yet — just data.

**Requirements:** R1, R2, R16, R18

**Dependencies:** None (can run parallel with Units 1 and 2).

**Files:**
- Modify: `src/states/play_match/abilities.rs` (`AbilityType` enum)
- Modify: `src/states/play_match/ability_config.rs` (`expected_abilities` array in `validate()`)
- Modify: `assets/config/abilities.ron` (new `UnstableAffliction:` entry)
- Create: `assets/icons/abilities/spell_shadow_unstableaffliction.jpg` (via Wowhead MCP)
- Modify: `src/states/view_combatant_ui.rs` (Warlock ability list + display-name map)

**Approach:**
- Add `UnstableAffliction` to `AbilityType` enum.
- Add `UnstableAffliction` entry to `abilities.ron` with initial values: instant cast, 30 mana, 30yd range, 18s DoT duration, 6s tick interval (3 ticks), ~8 damage per tick, shadow school, `break_on_damage: -1.0`.
- Define `DispelBacklashConfig` as a new struct in `ability_config.rs`: `pub struct DispelBacklashConfig { pub silence_duration: f32, pub damage_base: f32, pub damage_sp_coefficient: f32 }`. Derive `#[derive(Debug, Clone, Deserialize)]` and apply `#[serde(default)]` to each field to allow RON omission in every non-UA ability entry. Add `pub dispel_backlash: Option<DispelBacklashConfig>` to `AbilityConfig` with `#[serde(default)]`.
- UA's `abilities.ron` entry carries `dispel_backlash: Some((silence_duration: 5.0, damage_base: 40.0, damage_sp_coefficient: 0.3))`. Starting values; Unit 9 tunes them.
- Append `AbilityType::UnstableAffliction` to the `expected_abilities` array in `ability_config.rs::validate()`.
- Use Wowhead MCP `get_spell_icon("Unstable Affliction")` to download the authentic icon; save to `assets/icons/abilities/spell_shadow_unstableaffliction.jpg`. Reference this path in the `icon:` field of the abilities.ron entry.
- Add UA to `get_class_abilities(CharacterClass::Warlock)` in `view_combatant_ui.rs:210` and to the display-name match at ~L296.

**Patterns to follow:**
- `Corruption` entry in `abilities.ron` (instant shadow DoT, `applies_aura` with `DamageOverTime`) — exact template.
- Strategic-options class icon work (2026-04-05 session) — Wowhead MCP icon download pattern.

**Test scenarios:**
- Happy path: `cargo test` (including `validate()`) passes with UA registered in the expected list.
- Happy path: loading abilities.ron via `AbilityConfig::load()` returns a config whose `UnstableAffliction` entry has expected cast time (0.0), mana cost (30), and applies a DoT aura.
- Happy path: the view-combatant screen, given a Warlock combatant, lists UA in the ability roster (can be verified via a headless runner or spot-check).
- Test expectation: no behavioral logic yet — units below add that.

**Verification:**
- `cargo test` passes.
- `ls assets/icons/abilities/ | grep unstable` returns the downloaded icon file.

---

- [ ] **Unit 4: Implement UA dispel backlash in `process_dispels`**

**Goal:** When a qualifying enemy dispels a UA aura, apply the Shadow damage burst (using the snapshotted SP) and queue a Silence aura with DR applied.

**Requirements:** R4, R4a, R7, R8, R11, R12

**Dependencies:** Units 1, 2, 3.

**Files:**
- Modify: `src/states/play_match/effects/dispels.rs` (post-removal hook spawns `BacklashPending` entity)
- Create or modify: `src/states/play_match/effects/backlash.rs` (new file) — defines the `BacklashPending` component and the `process_backlash` system that runs after `process_dispels` each frame
- Modify: `src/states/play_match/components/combatant.rs` (add `BacklashPending` component struct alongside `DispelPending`)
- Modify: `src/states/play_match/components/auras.rs` (add `#[derive(Default)]` to `Aura` and `pub backlash_damage: Option<f32>` field)
- Modify: `src/states/play_match/class_ai/warlock.rs` (when casting UA, compute SP and populate `backlash_damage` on the AuraPending)
- Modify: `src/states/play_match/systems.rs` AND `src/states/mod.rs` (register `process_backlash` in both modes, running after `process_dispels` within the same `CombatSystemPhase`)
- Test: colocated tests in `effects/backlash.rs`

**Approach:**
- Add `#[derive(Default)]` to `Aura` so adding fields doesn't force changes at the ~15 existing struct-literal construction sites (auras.rs, traps.rs, combat_core/{auto_attack,damage,mod}.rs, effects/divine_shield.rs, class_ai/{priest,paladin}.rs, shadow_sight.rs).
- Add `pub backlash_damage: Option<f32>` to `Aura` (defaults to None via the new Default derive; populated only for UA via the Warlock AI at cast time).
- When UA is applied, Warlock AI computes `backlash = base + sp_coefficient * caster_spell_power` and sets it on the AuraPending/Aura. This is the R4a snapshot — caster death after this point does not change the backlash value.
- In `process_dispels`, after the `active_auras.auras.remove(idx_to_remove)` line, use **ability-name match** (consistent with `try_corruption`'s existing pattern at `warlock.rs:235`) to detect UA: `removed_aura.effect_type == AuraType::DamageOverTime && removed_aura.ability_name == "Unstable Affliction"`. Detection is by ability-name; `backlash_damage` is pure payload (never a sentinel). If `caster` is populated AND the dispeller and caster are on opposing teams, spawn a `BacklashPending` entity with `(dispeller, damage, silence_duration)` — do NOT apply damage or silence inline (avoids the mutable-query-borrow conflict since `process_dispels` holds `&mut (Combatant, ActiveAuras)` for the dispel target, not the dispeller).
- `process_backlash` (new system in `effects/backlash.rs`) runs after `process_dispels` within the same `CombatSystemPhase::ResourcesAndAuras` ordering. For each `BacklashPending`:
  1. Apply Shadow damage to the dispeller via the existing `apply_damage_with_absorb` helper (so armor/resistance/absorb apply correctly and log format is consistent). Log a `[BACKLASH]` line.
  2. If the dispeller survives the damage (`combatant.is_alive()` check), spawn an `AuraPending` for `AuraType::Silence` with `silence_duration` — the existing `apply_pending_auras` flow handles DR automatically.
  3. Despawn the `BacklashPending` entity.
- **Ordering invariant (revised):** Damage and Silence are applied in the SAME frame — both land within `process_backlash`, damage first. Silence does NOT wait for the next frame, because `process_backlash` queues the `AuraPending` and `apply_pending_auras` runs next frame. The key invariant is: a dispeller killed by backlash damage is dead before the Silence aura is queued, so `is_alive()` prevents attaching a Silence to a dead entity. This is the testable behavior — not "same-frame vs next-frame".
- **Note on crit:** `apply_damage_with_absorb` does NOT roll crit; existing callers (Shadow Bolt, Mind Blast) roll crit before calling. `process_backlash` should either (a) not crit (simplest, and backlash is a punishment not an offensive ability) OR (b) roll crit using the standard crit helper. Recommended: no crit on backlash — one less balance variable, and DR already caps uptime.
- **DR-immune signal for visuals (Unit 7 interaction):** `process_backlash` should check DR state BEFORE queueing the Silence aura (via `DRStates::is_immune(DRCategory::Silence)` on the dispeller). If immune, skip the `AuraPending` spawn AND emit a signal — either by spawning an `ImmuneFeedback { target: dispeller }` marker component that Unit 7's floating-text system renders as "Immune", or by logging the distinction in the `[BACKLASH]` line so visuals can subscribe. This disambiguates the DR-immune visual from the dispeller-died visual.

**Execution note:** Test-first for the damage-before-silence ordering invariant — write the test that a dispeller killed by backlash damage does NOT receive a Silence aura, then implement.

**Technical design:** *(directional)*

```
process_dispels (existing system, extended):
  for each DispelPending:
    removed_aura = active_auras.remove(random_dispellable_idx)
    log standard dispel
    spawn standard DispelBurst visual

    // Backlash hook (Unit 4) — queue, don't apply
    if removed_aura.ability_name == "Unstable Affliction"
       && removed_aura.caster is Some
       && teams_differ(removed_aura.caster, pending.dispeller):
        commands.spawn(BacklashPending {
            dispeller: pending.dispeller,
            damage: removed_aura.backlash_damage.unwrap_or(0.0),
            silence_duration: 5.0,
            caster: removed_aura.caster.unwrap(),
        })

process_backlash (new system in effects/backlash.rs):
  for each BacklashPending:
    dispeller_combatant = combatants.get_mut(pending.dispeller)
    apply_damage_with_absorb(dispeller_combatant, pending.damage, Shadow)
    log [BACKLASH] line
    if dispeller_combatant.is_alive():
        commands.spawn(AuraPending {
            target: pending.dispeller,
            aura: silence_aura(duration=pending.silence_duration, caster=pending.caster),
        })
        spawn BacklashBurst visual  // Unit 7
    commands.entity(pending_entity).despawn()
```

**Patterns to follow:**
- `process_dispels` existing structure — keep the hook as a small block *after* the standard dispel work, not interleaved.
- Damage queuing used by Shadow Bolt / Mind Blast in `combat_core/damage.rs`.
- `AuraPending::from_ability` for the Silence aura construction — goes through the standard DR path.

**Test scenarios:**
- Happy path: Priest dispels UA off own ally → dispeller (Priest) takes Shadow damage equal to the aura's `backlash_damage` and receives a 5s Silence aura.
- Happy path: Paladin dispels UA via Cleanse → same backlash outcome.
- Edge case: Felhunter Devour Magic dispels an enemy's UA off an ally (team comparison: dispeller team != aura caster team) → Felhunter takes backlash.
- Edge case: UA expires naturally (timer reaches 0) → no backlash damage, no Silence applied to anyone.
- Edge case: Dispelling Corruption (not UA) from the same target → standard dispel, no backlash. SC4 regression protection.
- Edge case: Warlock dies before UA is dispelled, then healer dispels UA → backlash damage still applied using snapshot SP; Silence still applied to dispeller (caster death does not void the snapshot).
- Integration: Backlash damage kills the dispeller → dispeller does not receive the Silence aura (ordering invariant). Use `Execution note` test as the seed.
- Integration: Dispelling UA on a target with DR level 1 in Silence → Silence aura duration is 2.5s (DR applied automatically by `apply_aura`).
- Integration: Dispelling UA three times against the same dispeller within 15s → third dispel applies 0s Silence (immune); Silence state resets after `DR_RESET_TIMER`.
- Integration: Mirror match (Team 1 Warlock UAs Team 2 target, Team 2 dispels) → Team 2 dispeller gets silenced (symmetric behavior).

**Verification:**
- `cargo test` passes, including the damage-before-silence ordering test.
- Headless run of `team1: [Warlock], team2: [Priest]` produces at least one `[BACKLASH]` combat-log line (once Priest AI dispels).

---

- [ ] **Unit 5: Enforce Silence at cast-time gates**

**Goal:** A silenced mana-resource combatant cannot initiate any mana-cost ability, including dispels, and in-flight casts are interrupted when silence lands.

**Requirements:** R5, R6

**Dependencies:** Unit 1 (`is_silenced` helper).

**Files:**
- Modify: `src/states/play_match/class_ai/priest.rs` (add `is_silenced` check to each mana-cost ability attempt — parallel the `is_spell_school_locked` pattern)
- Modify: `src/states/play_match/class_ai/paladin.rs` (same)
- Modify: `src/states/play_match/class_ai/mage.rs` (same)
- Modify: `src/states/play_match/class_ai/warlock.rs` (same; relevant if Warlock ever gets silenced by a future mirror match)
- Modify: `src/states/play_match/class_ai/hunter.rs` (same)
- Modify: `src/states/play_match/class_ai/mod.rs` (dispel helper ~L394 that bypasses per-class-AI checks)
- Modify: `src/states/play_match/combat_core/casting.rs` (`process_casting` — the `is_incapacitating` interrupt path at ~L147, NOT the mana-deduction site at L189)
- Modify: `src/states/play_match/utils.rs` (extend `is_incapacitating` to include Silence — simplest integration)
- Test: existing abilities-module tests plus new tests in `class_ai/mod.rs`

**Approach:** Silence has **two** enforcement sites via per-class-AI checks plus **one** mid-cast interrupt site — mirroring the existing `is_spell_school_locked` pattern for consistency:

1. **Per-class-AI call sites (mirrors `is_spell_school_locked` usage)**: Every class-AI file that currently checks `is_spell_school_locked(...)` before attempting a mana-cost ability must also check `!is_silenced(caster, auras)`. Expected call sites: `class_ai/priest.rs`, `class_ai/paladin.rs`, `class_ai/mage.rs`, `class_ai/warlock.rs`, `class_ai/hunter.rs` (plus Warrior and Rogue rely on `is_silenced` returning false by resource-type, so no changes needed there). Rough inventory: ~20-30 call sites to update, matching the `is_spell_school_locked` precedent. `can_cast_config`'s signature does NOT change.

2. **Dispel helper (bypasses the per-class-AI path)**: at `class_ai/mod.rs:394`, add an explicit `is_silenced` check before queueing the dispel. Without this, a silenced healer would still successfully cast Dispel Magic / Cleanse because the helper deducts mana directly.

3. **`process_casting` mid-cast interrupt**: extend the `is_incapacitating` check at `casting.rs:147` (currently interrupts on Stun/Fear/Poly/Incapacitate) to ALSO interrupt on Silence. This handles the case surfaced during review: because Silence is queued via `AuraPending` by `process_backlash` and applied by `apply_pending_auras`, there is a window (potentially one frame) where a healer's in-flight cast could complete even though Silence is inbound. Interrupting at `is_incapacitating`-time is the correct gate — NOT the mana-deduction site at L189 (which only fires on cast completion after the heal has already effectively landed).

**Rationale for per-call-site over centralizing in `can_cast_config`:** preserves consistency with `is_spell_school_locked` (the exact primitive Silence parallels). Both primitives are "caster has an aura that blocks specific abilities" — identical shape, same usage pattern. Centralizing one but not the other would create an inconsistency readers would have to explain.

**Patterns to follow:**
- `is_spell_school_locked` usage in existing callers (if any) — mirror the same pattern.
- The mana-check short-circuit in `can_cast_config` as the precedent for an early-return on silence.

**Test scenarios:**
- Happy path: a Silenced Priest cannot cast Flash Heal — `can_cast_config` returns false.
- Happy path: a Silenced Paladin cannot cast Hammer of Justice (even though HoJ is stun-CC, it costs mana) — `can_cast_config` returns false.
- Edge case: a Silenced Warrior can still use Mortal Strike — the helper's `resource_type == Mana` gate keeps rage users unaffected.
- Edge case: a Silenced Rogue can still use Ambush / Eviscerate — energy-users unaffected.
- Edge case: a non-silenced mana user can cast normally — no regression.
- Integration: a mid-cast Frostbolt is interrupted if the caster becomes Silenced mid-cast. (If the current code path doesn't support mid-cast interruption yet, document this as "mid-cast interruption is effectively N/A because Silence is applied only on dispel events, which are not concurrent with the casting combatant" — but still add the check defensively.)
- Integration: a Silenced Priest cannot cast Dispel Magic — the dispel helper at `class_ai/mod.rs:394` rejects. Regression check for the bypass gap.

**Verification:**
- `cargo test` passes.
- Headless run of Warlock vs Priest: at least one sequence shows the Priest dispelling UA, receiving Silence, and failing their next heal cast within the 5s window.

---

- [ ] **Unit 6: Warlock AI — `try_unstable_affliction`**

**Goal:** Warlock AI applies UA as part of its standard opener, alongside Corruption, with friendly-CC-break protection.

**Requirements:** R9, R10

**Dependencies:** Units 3 (ability config), 4 (aura with `backlash_damage` field).

**Files:**
- Modify: `src/states/play_match/class_ai/warlock.rs`

**Approach:**
- Mirror the `try_corruption` function: check target is alive, UA is off cooldown, mana sufficient, UA not already active on target, and `ctx.has_friendly_breakable_cc(target) == false`.
- UA freshness check: `a.effect_type == AuraType::DamageOverTime && a.ability_name == "Unstable Affliction"`. The ability name `"Unstable Affliction"` is the canonical string (matches the `name:` field in abilities.ron); this is also the sentinel `process_dispels` uses to identify UA for backlash (see Unit 4).
- Inject into the opener priority chain: after Corruption, before Shadow Bolt (the 2s cast nuke). Ordering rationale: both DoTs load, then Shadow Bolt for direct pressure.
- When casting UA, compute `caster_spell_power` at cast time and populate the AuraPending's snapshot field: `base + sp_coefficient * sp`.
- Honor the R10 friendly-CC-break guard explicitly via `ctx.has_friendly_breakable_cc` — the guard is not automatic.

**Patterns to follow:**
- `try_corruption` in `class_ai/warlock.rs` — copy structure, adjust for UA.
- Strategic-options additions from 2026-04-05 (ability dispatch in Warlock AI) for the priority placement.

**Test scenarios:**
- Happy path: Warlock's first non-movement tick against a target with no Corruption/UA applies Corruption first, then UA on the next GCD.
- Happy path: UA is not re-applied while already active on target (cooldown/duration gate works).
- Edge case: target is Polymorphed by an ally Mage → Warlock skips UA (friendly-CC-break guard).
- Edge case: target is already dead → Warlock skips UA (standard dead-target guard).
- Integration: a full headless match shows UA applied within the first 10 seconds of combat against at least one target.

**Verification:**
- `cargo test` passes.
- Headless `team1: [Warlock], team2: [Warrior]` run shows both Corruption and UA applied in combat logs.

---

- [ ] **Unit 7: Visual effects — UA DoT indicator, backlash burst, silence status text**

**Goal:** Make the mechanic readable at a glance in the graphical client.

**Requirements:** R13, R14, R15

**Dependencies:** Unit 1 (aura type), Unit 4 (backlash triggers).

**Files:**
- Create or modify: `src/states/play_match/rendering/effects.rs` (new effect structs + spawn/update/cleanup systems)
- Modify: `src/states/play_match/components/mod.rs` (new visual-effect components: `UnstableAfflictionGlow`, `BacklashBurst`, `SilenceStatusText`)
- Modify: `src/states/mod.rs` (register all three new system trios in graphical mode only)
- Modify: `src/states/view_combatant_ui.rs` (extend the debuff-list rendering so active Silence auras appear with their icon + remaining duration — this closes R17)

**Approach:**
- **UA DoT indicator (`UnstableAfflictionGlow`)**: a deep-violet outlined glow that pulses at ~0.5Hz (every 2s) around the afflicted combatant. Distinct from Corruption's faster green tendrils — different color family AND different rhythm so stacked Corruption+UA read independently. Spawned when UA aura is applied; despawned when UA aura is removed.
- **Backlash burst (`BacklashBurst`)**: a sharp dark-violet shadow explosion at the dispeller's position the frame backlash fires. Distinct from the existing `DispelBurst`: ~2x the particle count, dark-violet shadow color (vs DispelBurst's gentle sparkle), snappier 0.3s lifetime. Reads as "impact" not "sparkle".
- **Silence status text (`SilenceStatusText`)**: a floating "Silenced" text anchored above the silenced combatant for the duration of the aura. Reuses the floating-text system pattern.
- **DR-immune "Immune" text**: when `process_backlash` determines Silence will be rejected due to DR-immune, spawn a short-lived "Immune" floating text over the dispeller (distinct from the dispeller-died case, which has its own death animation). This disambiguates the visual so a viewer can tell "silence was resisted" from "dispeller died from backlash".
- Each visual follows the spawn/update/cleanup three-system pattern. `AlphaMode::Add`. `Res<Time>`. `Without<T>` for query conflicts. `try_insert` not `insert`.
- Register all visual system trios in `src/states/mod.rs` only (graphical). Do NOT register in `systems.rs` — these are visual-only and headless should ignore them. (Core game-logic systems from Units 1–6 DO register in both per the dual-registration learning.)

**Patterns to follow:**
- `DispelBurst` in `rendering/effects.rs` for the backlash-burst analogue.
- `HealingLightColumn` in `rendering/effects.rs` for the DoT glow analogue.
- Floating combat text system for the silence-status-text analogue.
- `docs/solutions/implementation-patterns/adding-visual-effect-bevy.md` — full pattern.

**Test scenarios:**
- Test expectation: none — visual effects lack behavior under test. Validation is screenshot/visual inspection in the graphical client.

**Verification:**
- `cargo run --release` shows: (a) a distinct visual on a UA'd target, (b) a distinct backlash burst on the dispeller when UA is dispelled, (c) floating "Silenced" text above a silenced combatant for ~5s.
- Visual regression spot-check: Corruption and Dispel Magic visuals are unchanged.

---

- [ ] **Unit 8: Simulation-driven balance tuning and success-criteria validation**

*(Unit 8 formerly covered dual-mode system registration. That work has been folded into Units 4 and 7 as part of each unit's definition-of-done. See below.)*

**Dual-registration now owned by producing units:**
- **Unit 4** owns registering `process_backlash` in BOTH `src/states/play_match/systems.rs::add_core_combat_systems` (headless) AND `src/states/mod.rs` (graphical). Verification per `docs/solutions/implementation-patterns/graphical-mode-missing-system-registration.md`: a headless run of `team1: [Warlock], team2: [Priest]` must produce `[BUFF] Unstable Affliction applied...`, DoT `[DMG]` ticks, and after a dispel: `[BACKLASH]` damage + `[BUFF] Silence applied`. The same match in graphical mode must produce the same combat log.
- **Unit 7** owns registering its three visual system trios in `src/states/mod.rs` ONLY (not `systems.rs`). Verification: visuals render in graphical client and headless remains unaffected.

**Goal:** Converge UA's damage/mana/duration values against the 24-seed bug-hunt configuration until success criteria are met (or the risk-accepted 2v2 gap is explicit and documented).

**Requirements:** SC1, SC2 (SC3 removed)

**Dependencies:** Units 1–7 (functional UA required).

**Files:**
- Modify: `assets/config/abilities.ron` (UA tuning values — iterative)
- Create: `docs/reports/2026-04-18-ua-simulation-tuning.md` (summary of tuning iterations + final numbers)

**Approach:**
- Run the 24-seed bug-hunt configuration used by `docs/reports/2026-04-12-bug-hunt-2v2-3v3.md` (seeds 6001–6031) with the current UA tuning.
- Compare Warlock 2v2 and 3v3 win rates to SC1 thresholds.
- Measure SC2: count matches where a dispeller was silenced during a kill window and a heal was denied.
- If SC1 or SC2 fall short, iterate: raise DoT damage slightly, lengthen duration, or raise backlash damage. If tuning reveals any match where Silence uptime feels oppressive (subjective log review), tighten DR directly rather than against a fixed uptime threshold.
- **Iteration cap: 3 full bug-hunt cycles.** After 3 iterations, stop tuning regardless of outcome and move to the exit criteria below.
- **Exit criteria (after \u22643 iterations):**
    - 3v3 WR \u2265 50% \u2192 success, ship.
    - 3v3 WR in [45%, 50%) \u2192 partial-pass, ship with a note in the tuning report describing the gap and what would close it (likely survivability work from the separate Warlock workstream).
    - 3v3 WR < 45% \u2192 tuning failure; do NOT keep raising damage numbers. Revisit the mechanism design \u2014 this is a signal that UA alone isn't the right buff, not that it needs more power. Escalate to a new brainstorm before continuing.
    - 2v2 WR: any value \u2265 20% is acceptable; 35% target is aspirational per the risk-accepted note in Problem Frame.
- Final numbers land in `abilities.ron`.

**Patterns to follow:**
- Bug-hunt workflow (`/bug-hunt` skill) for running 24-seed matrices in parallel.
- `docs/reports/2026-04-12-bug-hunt-2v2-3v3.md` — report template for the tuning summary.

**Test scenarios:**
- Integration: final-tuning 24-seed run produces Warlock 3v3 WR ≥ 50% (SC1 3v3 target).
- Integration: final-tuning run produces ≥ 3/24 matches with a heal denied by UA Silence during a kill window (SC2).
- Regression: dispel pipeline still removes Corruption/Fear/Polymorph correctly across all 24 matches (SC4).

**Verification:**
- `docs/reports/2026-04-18-ua-simulation-tuning.md` exists with final tuning values and before/after win-rate table.
- SC4 passes for all non-UA dispels in the 24-seed run.

## System-Wide Impact

- **Interaction graph:** The dispel pipeline gains a post-removal hook that queues two things (damage + aura). The cast-time mana-deduction path gains a Silence gate. Warlock AI gains one new ability-try function. Visual systems expand by three effect components. No callback or middleware outside these gains a new entry point.
- **Error propagation:** If UA is mis-cast (e.g. target dies mid-cast), existing cast-cancellation logic handles it; no new error path. If `DispelPending.dispeller` is somehow unpopulated (shouldn't be — non-optional field) the game logic will not compile — compile-time safety net.
- **State lifecycle risks:** Backlash damage is snapshotted on the aura at cast time, so caster death does not corrupt the damage amount. Silence aura is queued via the standard `AuraPending` flow so DR is applied automatically — no duplicate-aura risk.
- **API surface parity:** `is_silenced` is the parallel to `is_spell_school_locked`. If either surface adds a new caller in the future, both should be considered. The dispel helper's bypass of `can_cast_config` remains a known quirk — Unit 5 adds a Silence check there as a targeted fix rather than a refactor.
- **Integration coverage:** Unit 4's damage-before-silence ordering, Unit 5's dispel-helper gate, the dual registration owned by Units 4 and 7, and Unit 8's simulation pass are the cross-layer scenarios where mocks alone are insufficient.
- **Unchanged invariants:** Existing dispel RNG is unchanged (SC4). Corruption, Fear, Polymorph, and all other magic debuffs dispel exactly as before. No class-AI file other than `warlock.rs` and `class_ai/mod.rs` is modified. No existing ability's stats change.

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| Balance overshoots: Warlock jumps from 20% to 80% WR with UA landing backlash too often | Unit 9 simulation-driven tuning, with conservative starting values (40 base damage + 0.3 SP). First iteration likely needs damage down or duration down. |
| Silence chains feel oppressive despite DR | DR category already caps third silence at 0s duration. Backup: raise DR penalty (second silence 25% instead of 50%) — isolated one-line change. |
| Felhunter Devour Magic edge case misfires (silences own Felhunter when dispelling allied UA) | Unit 4's team-comparison check uses `aura.caster` (populated for UA) and `DispelPending.dispeller`. Mirror-match test case in Unit 4 validates. |
| Felhunter net-positive from Devour-Magic-heal + backlash (heal exceeds damage on a healthy Felhunter; Silence is cosmetic on pets) | Accepted. If Unit 8 tuning shows Felhunter-Warlock comps swinging over-buffed, address via Devour Magic heal-amount tuning in a separate workstream rather than complicating backlash logic. |
| Dual-registration gap — headless works, graphical silently fails | Unit 8 explicitly follows the institutional learning; graphical smoke test in verification. |
| `DRCategory::COUNT` bump breaks fixed-size array callers that hardcoded 5 | `DRStates` uses `DRCategory::COUNT`, not `5`, per its definition. Unit 1 test validates array indexing at the new size. |
| 2v2 target (35%) not reached because Warlock still dies first | Risk accepted per origin doc. Document in Unit 9 tuning report as expected outcome if observed. |
| Silence helper misfire: silences Warrior/Rogue abilities that happen to have non-zero `mana_cost` fields | `is_silenced` gates on `resource_type == Mana` first, so rage/energy users are always unaffected. Unit 1 test explicitly covers this. |

## Documentation / Operational Notes

- Update `CLAUDE.md` "Available aura types" list to include `Silence`. (One-line addition.)
- Update `docs/known-issues.md` if Unit 9 tuning reveals any intentional behavior worth documenting (e.g. "Silenced dispel attempts appear in logs as a rejected cast, not a wasted GCD — working as intended").
- After merge, run the bug-hunt skill on the 24-seed config and archive the new report as `docs/reports/2026-04-18-ua-simulation-tuning.md` (output of Unit 9) alongside the existing bug-hunt report.
- Update `docs/brainstorms/2026-04-18-unstable-affliction-warlock-requirements.md` `status: planned` → `status: implemented` when Unit 9 completes successfully.

## Sources & References

- **Origin document:** [docs/brainstorms/2026-04-18-unstable-affliction-warlock-requirements.md](../brainstorms/2026-04-18-unstable-affliction-warlock-requirements.md)
- **Bug-hunt source:** [docs/reports/2026-04-12-bug-hunt-2v2-3v3.md](../reports/2026-04-12-bug-hunt-2v2-3v3.md)
- **Institutional learnings:**
  - [docs/solutions/implementation-patterns/graphical-mode-missing-system-registration.md](../solutions/implementation-patterns/graphical-mode-missing-system-registration.md)
  - [docs/solutions/implementation-patterns/adding-visual-effect-bevy.md](../solutions/implementation-patterns/adding-visual-effect-bevy.md)
  - [docs/solutions/implementation-patterns/adding-new-class-paladin.md](../solutions/implementation-patterns/adding-new-class-paladin.md)
- **Related code:**
  - `src/states/play_match/components/auras.rs` (AuraType, DR system, Aura struct)
  - `src/states/play_match/effects/dispels.rs` (dispel pipeline)
  - `src/states/play_match/abilities.rs` (ability gating)
  - `src/states/play_match/class_ai/warlock.rs` (Warlock AI)
- **Related prior plans:**
  - [2026-04-12-002-fix-friendly-cc-break-missing-guards-plan.md](2026-04-12-002-fix-friendly-cc-break-missing-guards-plan.md) — the friendly-CC-break guard UA relies on (R10)
  - [2026-04-05-003-feat-class-strategic-options-plan.md](2026-04-05-003-feat-class-strategic-options-plan.md) — precedent for Wowhead icon downloads and view-combatant UI ability list additions
