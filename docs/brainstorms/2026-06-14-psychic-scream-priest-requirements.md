---
date: 2026-06-14
topic: psychic-scream-priest
---

# Psychic Scream (Priest) — Requirements

## Summary

Add Psychic Scream to the Priest: an instant, self-centered AoE fear that
applies the existing Fear aura to every enemy within a short radius of the
caster. Ships with the full visual/UI scope of a new ability and dual-mode
movement AI — a defensive self-peel / escape opener when the Priest is being
focused, and an aggressive offensive dip toward the enemy healer to fear them
when it is not.

---

## Problem Frame

The Priest is currently underpowered (no hard CC, no panic button) and its
movement AI is built to flee from danger. That makes it easy to train: melee
sticks on the Priest, the Priest kites and heals but has no way to break the
pressure, and it has no offensive lever to disrupt an enemy healer in a
mirror-ish 2v2/3v3.

Psychic Scream is the canonical answer in WoW Classic — a short-range AoE
fear. Its limited radius is the design tension: the ability only matters when
enemies are close, which fights the healer's instinct to maximize distance.
The interesting property is that fear sends enemies running *away* from the
caster, so the separation the ability creates is the same separation a fleeing
healer already wants — and the same lever can be turned outward to shut off an
enemy healer.

---

## Key Decisions

- **Self-centered AoE fear, reusing existing mechanics.** Psychic Scream
  applies the existing `Fear` aura (currently used by the Warlock) to all
  enemies within its radius of the caster. The AoE collection reuses the Mage
  Frost Nova pattern (iterate enemies within range of the caster, apply the
  aura to each). No damage component — it is pure CC. This makes the core
  ability a composition of two already-built mechanics rather than new
  combat machinery.

- **Mode is decided by the Priest's targeting status.** If the Priest is
  currently being targeted by melee / has enemies inside its danger radius,
  the scream is *defensive* (self-peel / escape opener). If the Priest is not
  the target, the scream is *offensive* (dip to fear the enemy healer).
  Defensive use always takes priority — the cooldown is never spent on an
  offensive dip while the Priest itself is under pressure.

- **Aggressive offensive dip by default, mirroring the Paladin DIP.** The
  offensive dip walks the Priest into fear range of the enemy healer, lands
  the scream, and retreats — structurally the same behavior as the Paladin's
  existing dip-to-land-Hammer-of-Justice on the enemy healer (DipEnter /
  DipComplete / DipAbort, a cooldown reservation, a dip budget, abort guards).
  It fires whenever the enemy healer is reachable and the scream is off
  cooldown, and is deferred only when a teammate is in HP trouble (and the
  Priest itself is not being chased). Accepts that a cloth healer dipping into
  the enemy backline can backfire; the deferral guard and the
  targeting-status priority are the safety rails.

- **Fear fragility matches Warlock Fear.** Psychic Scream's fear uses the
  same break-on-damage threshold as the existing Warlock Fear (~100 cumulative
  damage). Consequence: a focused, feared enemy healer breaks free quickly, so
  the offensive payoff is "fear the healer while the team kills a *different*
  target," not "fear the healer and burst the healer."

---

## Requirements

### Ability

- R1. Psychic Scream is a Priest ability: instant cast (0.0 cast time),
  self-centered AoE, on a cooldown, with a mana cost. It applies the `Fear`
  aura to every living enemy within its radius of the caster at cast time.
- R2. The ability deals no damage — it is crowd control only.
- R3. The fear's duration and break-on-damage threshold are configured to
  match the existing Warlock Fear behavior (break threshold ~100 damage) as
  the starting point, tunable via `abilities.ron`.
- R4. The ability follows the data-driven ability pattern: an `AbilityType`
  variant, a validation-list entry, and an `abilities.ron` definition. Values
  (radius, cooldown, mana, duration) start from WoW Classic references and are
  finalized via balance sweeps.
- R5. Enemies already immune (e.g., under an immunity effect) are not feared,
  consistent with how existing aura application checks immunity.

### Visuals / UI

- R6. The ability has a spell icon wired into the ability timeline UI, sourced
  and saved under `assets/icons/abilities/`.
- R7. Casting Psychic Scream produces a self-centered AoE visual effect (an
  expanding burst/shockwave around the Priest), following the established
  spawn/update/cleanup three-system visual pattern and registered for
  graphical mode only.
- R8. Fear application reuses the existing crowd-control feedback (speech
  bubble + combat-log CC entry) already emitted when the Fear aura lands.

### AI — defensive mode

- R9. When the Priest is being targeted by melee / has enemies within its
  danger radius and the scream is off cooldown, the Priest casts Psychic
  Scream as a self-peel. This is a predicate in the Priest's `try_*` decision
  chain, prioritized appropriately against its heals.
- R10. The scream integrates with the existing ESCAPE posture as an escape
  opener: when the Priest is being chased and needs to flee, screaming first
  (fearing the chasers away) is preferred over fleeing without it, so the
  fear-driven separation compounds the escape.

### AI — offensive mode (dip)

- R11. When the Priest is *not* being targeted, an enemy healer is alive and
  reachable, and the scream is off cooldown, the Priest performs an offensive
  dip: move into fear range of the enemy healer, cast Psychic Scream, then
  retreat. This reuses the Paladin DIP movement machinery.
- R12. The offensive dip is deferred (the Priest holds position and heals
  instead) when a teammate is below the healing-need HP threshold and the
  Priest itself is not being chased/targeted.
- R13. The offensive dip aborts safely (matching Paladin DIP abort guards) if,
  mid-dip, the target dies/becomes immune, a teammate's HP dives, the Priest
  becomes targeted, or the dip budget expires.
- R14. The cooldown is reserved for the offensive dip while a living enemy
  healer exists and the Priest is not under pressure — the defensive self-peel
  predicate still preempts the reservation the moment the Priest becomes the
  target.

### Instrumentation

- R15. The ability emits AI decision-trace reject/choose events at each
  predicate gate (mirroring the existing per-ability tracing), and the
  offensive dip emits DipEnter / DipComplete / DipAbort movement-decision
  events carrying the enemy-healer goal, consistent with the Paladin dip.

---

## Acceptance Examples

- AE1. **Covers R9, R10.** Priest is being trained by a Warrior inside its
  danger radius, scream off cooldown. **Then** the Priest casts Psychic Scream,
  the Warrior is feared and runs away, and the Priest gains separation /
  continues its escape rather than face-tanking.

- AE2. **Covers R11, R1.** Priest is safe (not targeted), enemy team is a
  DPS + healer, the team is healthy, scream off cooldown. **Then** the Priest
  dips toward the enemy healer, casts Psychic Scream within radius, the enemy
  healer (and any enemy within radius) is feared, and the Priest retreats.

- AE3. **Covers R12.** Priest is safe and an enemy healer is reachable, but a
  teammate has dropped below the healing-need threshold. **Then** the Priest
  does *not* dip; it holds and heals the teammate, saving the scream.

- AE4. **Covers R13, R14.** Priest begins an offensive dip; mid-dip a melee
  switches onto the Priest. **Then** the dip aborts and the scream becomes
  available for an immediate defensive self-peel instead.

- AE5. **Covers R3 + the AoE-scatter consequence.** Priest dips and screams
  near the enemy healer while an enemy DPS is also within radius. **Then**
  both are feared (full peel); if the Priest's team then focuses the feared
  healer, the fear breaks after ~100 damage — so the intended line is the team
  killing the non-feared target during the fear window.

---

## Scope Boundaries

- Psychic Scream as a damage or rotational ability — out. It is CC-only.
- Diminishing-returns / CC-stacking system changes — out. The ability uses the
  existing Fear aura and break-on-damage economics as-is.
- Changes to the Warlock's existing single-target Fear — out.
- Constraining the offensive dip's AoE to never catch a friendly kill target —
  out for v1 (treated as an acceptable full-peel side effect; revisit if
  sweeps show it scattering kills net-negatively).
- A generalized AoE-ability framework — out. The Frost Nova pattern is reused
  directly, not abstracted.

---

## Dependencies / Assumptions

- Assumes the existing Fear aura's feared-movement behavior (run in a random
  direction at full speed, break after a damage threshold) is the desired
  feel for Psychic Scream. If a distinct feared behavior is wanted, that is
  additional scope.
- Assumes the Paladin DIP machinery (dip posture, reservation, budget, abort
  guards, trace events) is reusable/parameterizable for a Priest scream-dip
  rather than Paladin-specific. Planning should confirm how much is shared vs.
  needs generalizing.
- Final radius, cooldown, mana, and fear duration are tuning values to be
  finalized with the balance-sweep workflow (2v2/3v3), not pinned here. The
  Priest being underpowered is the balance backdrop; success is a measurable
  Priest win-rate improvement without regressing other matchups.

---

## Success Criteria

- Priest 2v2/3v3 win rate improves measurably versus the current baseline,
  with no significant unintended regressions in other matchups (validated via
  the balance-sweep workflow).
- Movement probes confirm both modes fire correctly at fixed seeds: the
  defensive self-peel/escape when the Priest is focused, and the offensive
  dip-to-enemy-healer (with correct deferral when a teammate needs healing).
- The ability works identically in headless and graphical modes (no
  dual-registration gap), with the visual effect present only in graphical
  mode.

---

## Sources / Research

- Existing Fear aura + feared-movement: `src/states/play_match/combat_core/movement.rs`
  (fear-direction handling), `src/states/play_match/combat_core/casting.rs`
  (Fear application + CC feedback), `assets/config/abilities.ron` (`Fear`).
- Self-centered AoE pattern to reuse: Mage Frost Nova in
  `src/states/play_match/class_ai/mage.rs` and `QueuedAoeDamage` in
  `src/states/play_match/class_ai/mod.rs`.
- Priest AI decision chain: `src/states/play_match/class_ai/priest.rs`
  (`decide_priest_action`, `try_*` predicates, `evaluate_priest_posture`).
- Paladin DIP precedent (offensive dip-to-CC-the-healer): Paladin block in
  `assets/config/movement.ron` and the Paladin AI / dip trace events
  documented in `CLAUDE.md`.
- WoW Classic spell reference via the Wowhead MCP
  (`lookup_spell("Psychic Scream")`) for radius / cooldown / duration values.
- Balance context: Priest underpowered (project memory + matrix CSVs under
  `design-docs/balance/`).
