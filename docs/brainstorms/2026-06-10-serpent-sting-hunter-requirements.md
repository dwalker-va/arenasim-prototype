---
date: 2026-06-10
topic: serpent-sting-hunter
---

# Serpent Sting (Hunter) — Requirements

## Summary

Add Serpent Sting to the Hunter kit: an instant, cheap, no-cooldown Nature DoT modeled on the existing Corruption pattern, which the AI keeps ticking on its kill target while kiting. Tuned deliberately as a Hunter buff, with the full visual surface (icon, projectile, DoT body effect, HUD/combat-log presentation) shipped alongside the mechanics.

---

## Key Decisions

- **Role: mana-efficient kiting damage.** Serpent Sting is a cheap instant the Hunter applies while moving — damage continues during repositioning, complementing Concussive Shot kiting. It is not a healer-pressure spread tool; it lives on the kill target only.
- **Full two-way CC guard.** The sting is never cast on a target under friendly breakable CC, and Freezing Trap placement avoids targets carrying an active friendly DoT. Both checks already exist in the cast-guard infrastructure; this feature wires them, it does not build them.
- **Guards are reactive only.** No predictive "withhold sting because a trap play might come" logic in v1. In single-enemy situations this means the trap waits for the sting to expire — an accepted cost.
- **Intentional Hunter buff.** Hunter regressed in the recent balance patch; Serpent Sting is partly a recovery lever. The post-change sweep accepts a measurable Hunter winrate gain and gates only on runaway dominance.
- **Classic-anchored values, scaled to sim pace.** Damage/duration/mana follow WoW Classic Serpent Sting as the reference, scaled the way other abilities were (e.g., Freezing Trap's compressed durations). Exact numbers are a planning/tuning concern.

---

## Requirements

**Ability mechanics**

- R1. A new `SerpentSting` ability exists as a data-driven entry: instant cast, no cooldown, Nature school, standard Hunter shot range with the dead-zone minimum range shared by the other shots.
- R2. It applies a `DamageOverTime` aura following the Corruption pattern (duration + tick interval). DoT ticks count as damage for `break_on_damage` CC thresholds — existing engine behavior, unchanged.
- R3. Mana cost is cheap relative to Arcane Shot — mana efficiency is the role — and must fit within the constraints established by the Hunter mana economy work.
- R4. Damage scales with AttackPower, per Hunter ability convention.
- R5. The shot travels as a projectile (`projectile_speed`), consistent with every other Hunter shot.

**AI behavior**

- R6. Hunter AI maintains sting uptime on its kill target: apply when missing, reapply when expired. Because it is instant, it is usable mid-kite without interrupting movement.
- R7. The sting respects the friendly-CC pre-cast guard: never cast on a target under friendly breakable CC (Freezing Trap, Polymorph, etc.).
- R8. Freezing Trap placement avoids targets with an active friendly DoT, completing the two-way guard.
- R9. Guards are reactive only (per Key Decisions): no anticipatory sting suppression. When the only viable trap target carries a ticking sting, the trap is deferred until the sting expires.
- R10. Decision-trace instrumentation follows the established builder pattern: `reject` events with typed reasons at every predicate gate, `choose` on the success branch.

**Visual & UI surface**

- R11. Spell icon saved under `assets/icons/abilities/` and referenced from the ability entry, so the ability timeline UI and aura icons resolve it.
- R12. Projectile visuals (color/emissive) themed to the ability (poison-green), consistent in presentation with the other Hunter shot projectiles.
- R13. Target debuff presentation works end to end: aura icon over the afflicted target and HUD aura display render for the sting (generic `DamageOverTime` handling is expected to cover this — verify rather than assume).
- R14. A distinct DoT body effect on the afflicted target, visually distinguishable from Corruption and Unstable Affliction when stacked, following the established spawn/update/cleanup three-system pattern with graphical-only registration.
- R15. Combat log presentation: abbreviation and color entry for Serpent Sting alongside the existing per-ability mappings.

**Validation & balance**

- R16. `cargo test` passes: ability validation list updated, registration audit clean.
- R17. The 2v2 healer sweep (Hunter+Priest vs each-class+Priest) shows a Hunter gain without runaway dominance — no cell driven past roughly 65% winrate, judged against the most recent baseline CSV.

---

## Key Flows

- F1. Kiting application loop
  - **Trigger:** Gates open; Hunter's kill target closes distance.
  - **Steps:** Hunter applies Serpent Sting (instant, mid-movement) → kites with Concussive Shot/Disengage as usual → sting ticks while the Hunter repositions → on expiry, AI reapplies if guards permit.
  - **Outcome:** Sustained damage uptime that costs the Hunter no standing-still time.
  - **Covers:** R1, R2, R6.

---

## Acceptance Examples

- AE1. **Covers R7.** Given the kill target is held by a friendly Freezing Trap, when the Hunter evaluates Serpent Sting, then the cast is rejected with the friendly-CC rejection reason in the decision trace.
- AE2. **Covers R8, R9.** Given the sting is ticking on the only enemy and Freezing Trap is off cooldown, when the Hunter evaluates a trap play, then the trap is withheld (friendly-DoT rejection traced) and placed only after the sting expires.
- AE3. **Covers R6.** Given the Hunter is kiting a slowed melee target, when the sting is missing from the target, then it is applied without the Hunter stopping or losing separation.
- AE4. **Covers R13, R14.** Given a sting and a Corruption tick on the same target in graphical mode, then both the aura icons and the two DoT body effects render and read as independent effects.

---

## Scope Boundaries

- No multi-target DoT spreading or multi-dotting logic — that belongs to a healer-pressure role this brainstorm explicitly did not pick.
- No other Classic stings (Viper Sting, Scorpid Sting) — Serpent Sting only; the sting "family" (one-sting-per-hunter exclusivity rules) is out of scope until a second sting exists.
- No predictive trap/sting sequencing intelligence — reactive guards only (R9).
- No new dispel mechanics — the sting is removable by existing dispel systems like any magic DoT. An enemy Priest dispelling it is accepted counterplay, not a gap.

---

## Dependencies / Assumptions

- The Wowhead Classic MCP was not connected during this brainstorm. Classic reference values (damage over duration, mana cost) and the icon download (`get_spell_icon("Serpent Sting")`) need the MCP reconnected at planning/implementation time, or values entered from a manual reference check.
- Assumes DoT ticks already count toward `break_on_damage` thresholds (this is current engine behavior — Freezing Trap breaks on any damage) and that the friendly-DoT check used by the cast guard surfaces Hunter-owned stings the same as Warlock DoTs. Verify during planning.
- Hunter balance baseline: the most recent sweep CSVs under `design-docs/balance/` are the comparison point for R17.

---

## Outstanding Questions

**Deferred to Planning**

- Exact tuning numbers: total damage, duration, tick interval, mana cost, AP coefficient.
- AI priority slot: where sting application sits relative to Arcane Shot and Concussive Shot in the decision order (likely above Arcane Shot when missing from the target, below control abilities).
- Whether trap placement's friendly-DoT check should consider remaining sting duration (e.g., trap anyway if the sting expires within a second) or stay binary in v1.

---

## Sources

- Hunter ability block and Corruption DoT precedent: `assets/config/abilities.ron`
- Two-way guard infrastructure: `src/states/play_match/class_ai/cast_guard.rs` (`check_friendly_cc`, `has_friendly_dots_on_target`)
- Hunter AI structure (`try_*` helpers, kiting branch): `src/states/play_match/class_ai/hunter.rs`
- Visual surface precedents: DoT body effects in `src/states/play_match/rendering/effects.rs` (Corruption vs Unstable Affliction distinguishability), aura icon keys in `src/states/play_match/rendering/mod.rs`, HUD aura colors in `src/states/play_match/rendering/hud.rs`, per-ability combat-log entries in `src/states/play_match/rendering/combat_log.rs`
- Prior related brainstorms: `docs/brainstorms/2026-05-22-hunter-mana-economy-requirements.md`, `docs/brainstorms/2026-04-18-unstable-affliction-warlock-requirements.md` (the most recent add-a-DoT precedent)
- Balance context: `design-docs/balance/2026-06-04-hunter-mage-balance-findings.md`
