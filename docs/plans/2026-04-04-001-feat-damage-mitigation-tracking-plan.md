---
title: "feat: Track damage mitigated by armor and spell resistance in combat logs"
type: feat
status: completed
date: 2026-04-04
deepened: 2026-04-11
---

# feat: Track Damage Mitigated by Armor and Spell Resistance

## Overview

The armor/spell resistance system already reduces incoming damage inside `apply_damage_with_absorb`, but the mitigated amounts are thrown away. Players cannot see how much damage their armor or resistances actually prevented in combat logs or post-match reports. This plan adds per-school mitigation accumulation to the `Combatant` struct (no signature changes), then surfaces those totals in the match report.

## Problem Frame

Armor and spell resistance shipped in `2026-04-03-003-feat-armor-spell-resistance-plan.md` and are working correctly — physical hits get reduced by `armor / (armor + 5500)`, magical hits by the standard WoW resistance curve. But:

- Match reports only show `damage_dealt` and `damage_taken` (already-mitigated values).
- Players cannot evaluate whether stacking Frost Resistance against a Mage actually paid off.
- Balance tuning of resistance values has no observable signal in match logs.
- Post-match analysis cannot answer "how much did the Plate set save the Warrior?" or "did the Shadow Resistance Aura matter?".

## Requirements Trace

- **R1.** Track total physical damage mitigated by armor per combatant, accumulated over the match.
- **R2.** Track total magical damage mitigated by spell resistance per combatant, broken down by spell school.
- **R3.** Render mitigation totals in the match report file (`combat/log.rs::save_to_file`) per combatant.
- **R4.** *(Deferred)* Per-hit mitigation in combat log lines (e.g., `"Frostbolt hits Warrior for 75 damage (32 resisted)"`). Requires plumbing pre-mitigation damage to log call sites — explicitly out of scope.

## Scope Boundaries

- **Not** changing the signature of `apply_damage_with_absorb` or any of its 7 call sites.
- **Not** adding per-hit mitigation strings to the live combat log lines (R4 deferred).
- **Not** tracking mitigation from absorb shields, `DamageTakenReduction` auras, or `DamageImmunity` (Divine Shield) — those are already visible elsewhere (absorb auras log their consumption; Devotion Aura is well-known by tooltip; Divine Shield is binary).
- **Not** exposing mitigation in the in-game graphical UI (floating combat text, scoreboard). Match report only.
- **Not** changing any existing damage math.

## Context & Research

### Relevant Code and Patterns

- `src/states/play_match/combat_core/damage.rs::apply_damage_with_absorb` (lines 24–124) — single chokepoint where armor and resistance reductions happen. Already takes `&mut Combatant`, so accumulating into a new field requires no signature change.
- `src/states/play_match/components/combatant.rs::Combatant` (lines 99–183) — already accumulates `damage_dealt`, `damage_taken`, `healing_done` as `f32` fields. New mitigation fields follow the same convention.
- `src/states/play_match/abilities.rs::SpellSchool` enum — 8 variants (`Physical`, `Frost`, `Holy`, `Shadow`, `Arcane`, `Fire`, `Nature`, `None`). The 6 magical schools that take resistance are `Frost`, `Holy`, `Shadow`, `Arcane`, `Fire`, `Nature`.
- `src/combat/log.rs::save_to_file` (lines 505–644) — owns the match report format. Currently writes per-combatant lines like `Damage Dealt: X, Damage Taken: Y`. The mitigation summary slots in alongside.
- `src/combat/log.rs::CombatantMetadata` (lines 657–667) — the struct that carries per-combatant stats from the runner into the report. Needs new fields for mitigation totals.
- `src/headless/runner.rs` and `src/states/play_match/match_flow.rs` — populate `CombatantMetadata` from live `Combatant` components at match end. Both need to copy the new fields.

**Call sites of `apply_damage_with_absorb` (7 total — verified):**
1. `src/states/play_match/combat_core/damage.rs` — definition
2. `src/states/play_match/auras.rs:771` (DoT ticks)
3. `src/states/play_match/combat_ai.rs:603` (one ability path)
4. `src/states/play_match/combat_ai.rs:757` (Frost Nova damage)
5. `src/states/play_match/effects/holy_shock.rs:150`
6. `src/states/play_match/projectiles.rs:238`
7. `src/states/play_match/combat_core/auto_attack.rs:251`

None of these need to change because the accumulation happens inside the helper.

### Institutional Learnings

- The armor/resistance system was added in `docs/plans/2026-04-03-003-feat-armor-spell-resistance-plan.md` — the formulas in that plan match what's in `damage.rs` today, so this plan can rely on them as stable.
- The Combatant struct is `Clone` and is checked by `debug_validate()` for invariants. New stat fields don't need invariant entries (they're just running totals, no upper bound).
- Feedback from the dynamic stat aura refactor: avoid signature cascades when the same data can be threaded through `&mut Combatant` already in scope (see `2026-04-05-004-refactor-dynamic-stat-auras-plan.md`).

### External References

None needed — this is a localized accounting change against patterns already established in the codebase.

## Key Technical Decisions

- **Store mitigation on `Combatant`, not in a parallel resource.** Rationale: matches the existing accumulation pattern (`damage_dealt`/`damage_taken`/`healing_done`). It's already `&mut` inside `apply_damage_with_absorb`, so no plumbing needed. A separate resource would force a `Commands`/`ResMut` parameter through 7 call sites.

- **Track magical mitigation as a fixed-size `[f32; 6]` indexed by school.** Rationale: only 6 schools have resistance. A `HashMap<SpellSchool, f32>` would allocate per-combatant for no real benefit. Use a small `school_index(school) -> usize` helper to map `Frost/Holy/Shadow/Arcane/Fire/Nature` to `0..6`. Document the mapping in a doc comment so the array is self-explanatory.

- **Compute mitigation as `damage_before - damage_after` inside the existing reduction blocks.** Rationale: keeps the diff small and the bookkeeping next to the math it describes. No new branches in the hot path beyond a single subtraction and field write per damage event.

- **Clone-friendly default.** Rationale: `Combatant` is `Clone` and is constructed in `Combatant::new`. The new fields default to `0.0` / `[0.0; 6]` and need to be initialized in every constructor (`new`, `new_with_curse_prefs`, `new_pet`).

- **Match report format: one new line per combatant.** Rationale: keeps the report parseable. Single line lists totals: `Mitigated: armor=X, frost=Y, fire=Z, ...` with zero-valued schools omitted to avoid noise.

- **Exclude armor mitigation from spell-school array.** Rationale: armor only mitigates `Physical`, and `Physical` does not have resistance. Storing them separately keeps the model honest and avoids ambiguity in the report.

## Open Questions

### Resolved During Planning

- **Which fields to add?** → `damage_mitigated_by_armor: f32` and `damage_mitigated_by_resistance: [f32; 6]`.
- **Should mitigation count blocked-by-immunity damage?** → No. `DamageImmunity` returns early (line 47–49) and is not "mitigation", it's full negation. Match reports already make Divine Shield obvious from the existing damage_taken delta.
- **Should `DamageTakenReduction` (Devotion Aura) count as mitigation?** → No. That's a generic damage reduction, not armor/resistance. Including it would conflate two systems and break the trace back to the Combatant's armor/resistance stats. Out of scope per R1/R2 wording.
- **What index does each school map to?** → `Frost=0, Holy=1, Shadow=2, Arcane=3, Fire=4, Nature=5`. Documented as a doc comment on the field.
- **Where does the report get its data?** → Both `headless/runner.rs` and the graphical match-end path populate `CombatantMetadata` from live `Combatant` components. Both must copy the new fields.

### Deferred to Implementation

- **Final exact wording / column layout in the match report.** The data is fixed; the cosmetic format can be adjusted while writing the file.
- **Whether to also expose `total_mitigated()` and `total_resistance_mitigated()` helper methods on Combatant.** Add only if used by the report format chosen — don't speculate.
- **Whether to extend `CombatLog` query methods (e.g., a `total_mitigated_by(combatant_id)`).** The Results scene is out of scope; revisit only if a future plan needs structured mitigation events from `CombatLog`.

## Implementation Units

- [x] **Unit 1: Add mitigation fields to `Combatant`**

**Goal:** Add the two accumulation fields and initialize them in every constructor path.

**Requirements:** R1, R2

**Dependencies:** None

**Files:**
- Modify: `src/states/play_match/components/combatant.rs`

**Approach:**
- Add `damage_mitigated_by_armor: f32` and `damage_mitigated_by_resistance: [f32; 6]` to the `Combatant` struct.
- Document the resistance index mapping in a doc comment directly above the field: `Frost=0, Holy=1, Shadow=2, Arcane=3, Fire=4, Nature=5`.
- Initialize both to zero in `Combatant::new` (the only direct constructor path — `new_with_curse_prefs` and `new_pet` both call `Self::new` first, so they're covered automatically).
- Do not add anything to `debug_validate` — these are unbounded running totals, no invariants apply.

**Patterns to follow:**
- Existing accumulators: `damage_dealt`, `damage_taken`, `healing_done` (lines 153–158).

**Test scenarios:**
- *Happy path:* A freshly-constructed `Combatant` from `Combatant::new(1, 0, CharacterClass::Warrior)` reports `damage_mitigated_by_armor == 0.0` and `damage_mitigated_by_resistance == [0.0; 6]`.
- *Happy path:* A pet built via `Combatant::new_pet` inherits the same zero defaults.

**Verification:**
- `cargo build --release` succeeds.
- `cargo test` passes existing tests with no modifications needed.

---

- [x] **Unit 2: Accumulate mitigation inside `apply_damage_with_absorb`**

**Goal:** Compute and store the mitigated amount at the exact site of each reduction.

**Requirements:** R1, R2

**Dependencies:** Unit 1

**Files:**
- Modify: `src/states/play_match/combat_core/damage.rs`

**Approach:**
- Inside the armor reduction block (currently lines 55–58), capture `pre_armor = remaining_damage`, apply reduction, then `target.damage_mitigated_by_armor += pre_armor - remaining_damage`.
- Inside the spell resistance block (currently lines 61–79), do the same: capture `pre_resist`, apply reduction, then add `pre_resist - remaining_damage` into the appropriate `[f32; 6]` slot.
- Add a small private helper `fn resistance_school_index(school: SpellSchool) -> Option<usize>` returning `Some(0..6)` for the six magical schools and `None` for `Physical`/`None`. Use it to drive the array index.
- Do **not** track the `DamageTakenReduction` aura branch (lines 83–89) as mitigation — that's outside the armor/resistance contract per Key Technical Decisions.
- Skip the absorb branch — absorb is not "mitigation by armor or resistance".
- The function signature and return type stay exactly the same. No call sites change.

**Patterns to follow:**
- Inline accumulation pattern already used in this function: `target.damage_taken += actual_damage` (line 115).

**Test scenarios:**
- *Happy path:* A combatant with `armor = 5500.0` taking 100 physical damage records `damage_mitigated_by_armor == 50.0` and takes 50 to health (50% reduction).
- *Happy path:* A combatant with `frost_resistance = 60.0` taking 100 Frost damage records exactly `100 * (60 / (60 * 5/3 + 300)) = 100 * 0.15 = 15.0` mitigated into the `Frost` slot of the array.
- *Edge case:* A combatant with `armor = 0.0` taking 100 physical damage records zero mitigation but still passes the damage through.
- *Edge case:* A combatant with no resistance taking 100 Holy damage records zero across the resistance array.
- *Edge case:* `SpellSchool::None` damage records nothing in the resistance array (no out-of-bounds access).
- *Edge case:* Physical damage with `armor > 0` does not write to any slot of the resistance array.
- *Edge case:* Damage that gets fully absorbed by an Absorb shield still records the armor/resistance mitigation that happened first (mitigation ordering: armor → resistance → reduction → absorb).
- *Edge case:* `DamageImmunity` early-returns before any mitigation runs, so neither field is incremented (consistent with the "immunity is not mitigation" decision).
- *Integration:* DoT ticks routed through this helper from `auras.rs:771` correctly accumulate mitigation across multiple ticks.

**Verification:**
- `cargo test` passes.
- Headless arena match (Mage vs. Warrior, Warrior wearing armor) shows non-zero `damage_mitigated_by_armor` on the Warrior at match end (verified via Unit 4).
- Existing damage numbers in match logs are unchanged (mitigation is bookkeeping, not gameplay).

---

- [x] **Unit 3: Carry mitigation into `CombatantMetadata`**

**Goal:** Make the mitigation totals available to the match report by extending the metadata struct and the two places that build it.

**Requirements:** R3

**Dependencies:** Unit 1

**Files:**
- Modify: `src/combat/log.rs` (`CombatantMetadata` struct, lines 657–667)
- Modify: `src/headless/runner.rs` (find `CombatantMetadata { ... }` construction and copy the new fields)
- Modify: `src/states/play_match/match_flow.rs` *(or wherever the graphical-mode match-end builds `CombatantMetadata`)*

**Approach:**
- Add `damage_mitigated_by_armor: f32` and `damage_mitigated_by_resistance: [f32; 6]` to `CombatantMetadata`.
- Update both construction sites to copy from the live `Combatant` component. Use `grep` for `CombatantMetadata {` to locate every site.
- Keep the field names identical to those on `Combatant` for traceability.

**Patterns to follow:**
- The existing `damage_dealt` / `damage_taken` flow through `CombatantMetadata` is the template.

**Test scenarios:**
- *Happy path:* After a headless Mage-vs-Warrior match where the Warrior has armor, the metadata for the Warrior reports nonzero `damage_mitigated_by_armor`.
- *Edge case:* Combatants with no armor/resistance still produce well-formed metadata (all zeros, no panics).
- *Integration:* Both headless and graphical match-end paths produce identical metadata for the same seeded match (verifies both call sites were updated).

**Verification:**
- `cargo build --release` succeeds in both `--features headless` and default modes.
- A headless run completes without panics.

---

- [x] **Unit 4: Render mitigation in the match report**

**Goal:** Surface the new totals in `save_to_file` so a human reading the report can see them.

**Requirements:** R3

**Dependencies:** Unit 3

**Files:**
- Modify: `src/combat/log.rs` (`save_to_file`, around lines 553–595 where each combatant's stats are written)

**Approach:**
- For each combatant, after the existing `Damage Dealt: X, Damage Taken: Y` line, write a `Mitigated:` line.
- Format: `    Mitigated: armor=N` followed by ` frost=N fire=N` etc. for each non-zero school. Omit zero-valued schools to avoid clutter.
- If both armor and all six schools are zero, skip the line entirely (a Mage with no armor and no resistances should not have a noisy "Mitigated: " line).
- Use the same `{:.0}` formatting as the existing damage lines for consistency.
- Add the line in **both** the Team 1 and Team 2 composition loops.

**Patterns to follow:**
- Existing per-combatant `writeln!` calls in the team composition section (lines 554–572 for Team 1, 577–595 for Team 2).

**Test scenarios:**
- *Happy path:* Match report for a Plate-wearing Warrior taking physical hits shows a non-zero `armor=` value.
- *Happy path:* Match report for a target taking mixed Frost + Fire damage shows both `frost=` and `fire=` entries.
- *Edge case:* A Mage with no armor and no resistances fighting another Mage produces no `Mitigated:` line at all (instead of a misleading `Mitigated:` with empty values).
- *Edge case:* The report still parses cleanly to the existing `END OF REPORT` footer — no truncation, no missing newlines.

**Verification:**
- Run a headless match: `echo '{"team1":["Warrior"],"team2":["Mage"]}' > /tmp/test.json && cargo run --release -- --headless /tmp/test.json`
- `cat match_logs/$(ls -t match_logs | head -1)` shows the new `Mitigated:` line for the Warrior with realistic values.
- A Mage-vs-Mage headless run produces a report with no `Mitigated:` line on either side.
- All existing assertions/snapshots in `cargo test` still pass (the report change is additive).

## System-Wide Impact

- **Interaction graph:** The change is contained inside `apply_damage_with_absorb`. All 7 call sites are unaffected. No new systems registered, so the dual `states/mod.rs` + `systems.rs` registration trap from CLAUDE.md does not apply here.
- **Error propagation:** None. Field writes are infallible and arithmetic cannot underflow (we accumulate non-negative differences).
- **State lifecycle risks:** Mitigation totals reset implicitly because `Combatant::new` is called per match. There's no cross-match state.
- **API surface parity:** The match report format gains a new optional line. Any external tool parsing match reports must tolerate the new line — but per `combat/log.rs` doc comment, the report is for human reading and the structured query API on `CombatLog` is the machine-readable surface. The structured API is unchanged.
- **Integration coverage:** The DoT path (`auras.rs:771`) is the most subtle integration — verify in unit 2's test scenarios that DoT ticks accumulate mitigation correctly across ticks, not just on the first hit.
- **Unchanged invariants:** `apply_damage_with_absorb` signature, return type, all 7 call sites, all existing damage math, the structured `CombatLog` query API, and floating combat text behavior. None of these change.

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| Forgetting one of the two `CombatantMetadata` construction sites (headless vs graphical) | Unit 3 explicitly directs implementer to grep for `CombatantMetadata {` and update every site. Test scenario verifies both paths produce identical metadata for a seeded match. |
| Index mismatch in the `[f32; 6]` array (e.g., writing `Fire` damage to the `Frost` slot) | Centralize the mapping in `resistance_school_index` so there's exactly one place to get it wrong. Document the mapping on the field. Unit tests assert per-school slot values. |
| New field bloats `Combatant` clones in hot paths | `[f32; 6]` is 24 bytes; total addition is 28 bytes. `Combatant` is already large and rarely cloned in hot loops. Negligible. |
| Match report format change breaks downstream tooling | The structured `CombatLog` API is unchanged. Only the human-readable text file gains an optional line, which any line-based parser can ignore. |
| Devs assume mitigation includes Devotion Aura / absorbs | Comment on the field clarifying scope: "Tracks reduction from `target.armor` only — does not include `DamageTakenReduction` auras or absorb shields." |

## Documentation / Operational Notes

- No user-facing docs to update — match reports are an internal debugging tool.
- No rollout/migration concerns — purely additive.
- If a future plan adds a Results scene UI for mitigation, it should read from `CombatLog`'s structured events rather than from `CombatantMetadata` (which is a snapshot, not a stream). That work is out of scope here.

## Sources & References

- **Origin code:** `src/states/play_match/combat_core/damage.rs::apply_damage_with_absorb`
- **Related plan:** [`docs/plans/2026-04-03-003-feat-armor-spell-resistance-plan.md`](2026-04-03-003-feat-armor-spell-resistance-plan.md) — established the armor/resistance formulas this plan instruments.
- **Related plan:** [`docs/plans/2026-04-05-004-refactor-dynamic-stat-auras-plan.md`](2026-04-05-004-refactor-dynamic-stat-auras-plan.md) — established the "avoid signature cascades when `&mut Combatant` is already in scope" pattern.
- **Combatant struct:** `src/states/play_match/components/combatant.rs:99`
- **Match report writer:** `src/combat/log.rs:505`
- **Spell schools:** `src/states/play_match/abilities.rs:13`
