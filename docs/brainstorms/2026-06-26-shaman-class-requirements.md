---
date: 2026-06-26
topic: shaman-class
---

# Shaman Class — Requirements

## Summary

Add the Shaman as a playable class: a mana ranged caster-healer that plays
offensively rather than out-healing the enemy. Its signature mechanic is four
indestructible element totems (one slot each for air / water / earth / fire)
that pulse an ally buff in a radius and can be swapped by re-casting that
element. Around the totems sits a kit of Lightning Bolt, Frost Shock, Lesser
Healing Wave, Purge (offensive dispel), and Wind Shear (ranged interrupt),
backed by a new caster-mail gear loadout. The class is wired into all three
match scenes (configure match, view combatant, play match).

## Problem Frame

The roster has two healers — Priest and Paladin — and both are built to keep
allies alive: each carries a panic button (Psychic Scream, Hammer of Justice)
and wins by out-sustaining pressure. There is no healer that contributes to
*winning the damage race* instead of stalling it. The Shaman fills that gap: a
support class whose value is offensive tempo — stripping enemy defensives,
locking enemy casts, and adding direct damage — while healing only enough to
stay in the fight. It also introduces a mechanic the game has never had: a
persistent, positioned, team-wide buff source (totems) that rewards the team
for fighting near it.

## Key Decisions

- **Indestructible totems for v1.** Totems are placed radius objects that
  pulse a buff to allies in range, but enemies cannot target or destroy them.
  This delivers the positioning identity (stay near totems; manage four element
  slots) without new enemy target-selection AI or the balance risk of cleave/AoE
  deleting totems instantly. Destructible totems are a deliberate future layer.

- **Offense lives in the kit, not the totems.** All four totems are pure
  ally-buff pulses. The offensive slant comes from Lightning Bolt pressure,
  Purge stripping enemy defensives, Wind Shear locking heals, and Frost Shock
  chip damage — not from damage/debuff totems (Searing, Earthbind). This keeps a
  single totem behavior to build and tune.

- **No hard CC; Frost Shock's slow is the only peel.** Unlike the other two
  healers, the Shaman has no stun / fear / root. Its control is Wind Shear
  (interrupt) and Purge (strip), with Frost Shock's single-target slow as a soft
  peel. This is a distinctive glass-cannon-healer identity; melee comps are a
  deliberately hard matchup, tuned via throughput and gear rather than a panic
  button.

- **Reuse the existing healer posture machine.** The Shaman plugs into the
  Priest/Paladin FREE / PRESSURED / ESCAPE / DIP movement state machine driven
  by `assets/config/movement.ron`, with its own weights block tuned toward
  offensive participation (spending GCDs on Lightning Bolt / Purge / Wind Shear)
  rather than maximal healing uptime.

- **Totems spawn at the Shaman's location, visually spaced.** Like every WoW
  totem and like the existing Frost Trap slow zone, a totem drops where the
  caster stands when it is summoned — but multiple totems are fanned out with a
  small positional offset so they are visually distinguishable rather than
  stacked on one point. The offset is small relative to the totem radius, so the
  difference in coverage between totems is negligible (an ally near the cluster
  is inside all of them). This creates an intentional tension between anchoring
  near the team (so totem buffs land) and roaming to apply pressure — the central
  tuning problem of the class (see Success Criteria).

- **New caster-mail gear is required, not optional.** Existing mail items are
  statted for physical Hunters (tiny mana, no spell power). A caster-mail
  loadout with spell power, intellect, and a workable mana pool does not exist
  and must be added within the item-level budget.

## Requirements

**Class integration**

- R1. The Shaman is a selectable class in the configure-match scene, with a
  class icon, color, and description, on both teams and at all team sizes.
- R2. The Shaman appears in the view-combatant scene with its stats, resource
  (Mana), and full ability list rendered.
- R3. The Shaman is playable in headless and graphical play-match modes, driven
  by class AI, with identical behavior across both modes.
- R4. The Shaman uses Mana as its resource, is treated as a ranged class for
  range/positioning purposes, and is classified as a healer for AI target and
  role logic.

**Abilities**

- R5. Lightning Bolt — a ranged Nature-school nuke with a cast time. Primary
  offensive damage and the class's main source of sustained pressure.
- R6. Frost Shock — an instant Frost-school nuke that also applies a
  single-target movement slow. Doubles as a soft peel and as damage usable while
  repositioning.
- R7. Lesser Healing Wave — a fast direct heal. The Shaman's only healing spell;
  sized to keep an ally alive opportunistically, not to out-heal sustained
  pressure.
- R8. Purge — an offensive dispel that removes one beneficial aura from an enemy.
  Targets high-value enemy buffs (absorbs/shields, attack-power and
  damage-reduction buffs) and prioritizes the enemy healer's defensives.
- R9. Wind Shear — a ranged, instant interrupt that locks the target out of the
  interrupted spell's school for a short duration. Short cooldown; the class's
  primary tool against enemy casts and heals.

**Totems**

- R10. The Shaman can have at most one totem active per element (air, water,
  earth, fire) — up to four simultaneously.
- R11. A totem is a stationary entity placed at the Shaman's position that, each
  tick, applies or refreshes its buff aura on allied combatants within its
  radius. Allies outside the radius receive nothing.
- R11a. When multiple totems are active, they are placed with a small visual
  offset around the drop point so they are individually distinguishable on the
  field rather than overlapping. The offset is small relative to the totem
  radius, so coverage differences between co-located totems are negligible.
- R12. Casting a totem of an element that already has an active totem replaces
  the old one immediately (the previous totem of that element is expired/removed),
  even before its duration ends.
- R13. Totems expire on a duration timer and cannot be targeted or destroyed by
  enemies in v1.
- R14. The four totems and their buffs:
  - Fire — Flametongue Totem: increases spell power of allies in radius.
  - Air — Windfury Totem: empowers melee allies' auto-attacks (proc-style bonus
    attack). Benefits only melee teammates; inert for ranged/caster allies.
  - Earth — Strength of Earth Totem: increases attack power of allies in radius.
  - Water — Healing Stream Totem: applies a periodic heal to allies in radius.

**Gear**

- R15. Add a caster-mail loadout for the Shaman carrying spell power, intellect,
  and a mana pool sufficient to support a mana-healer, distributed across
  equipment slots within the enforced item-level budget.
- R16. New mail items required by R15 pass the existing item-level budget
  validation (`cargo test`) with no budget violations.

**AI & movement**

- R17. The Shaman uses the shared healer posture state machine with its own
  `movement.ron` weights block, tuned so it spends meaningful time applying
  offensive pressure (Lightning Bolt / Purge / Wind Shear) rather than only
  healing and repositioning.
- R18. The Shaman's AI manages its totem slots — placing the most valuable
  totems for the current ally composition and re-placing them as needed — and
  decides Purge / Wind Shear / Frost Shock usage against enemy buffs and casts.

## Key Flows

- F1. Totem placement and swap
  - **Trigger:** Shaman AI decides a totem of element E provides value for the
    current ally composition.
  - **Steps:** Shaman casts the totem; a totem entity spawns at its position; if
    a totem of element E was already active it is removed; each tick the new
    totem refreshes its buff on allies within radius.
  - **Outcome:** Allies near the totem carry its buff; the Shaman holds at most
    one totem per element.
  - **Covered by:** R10, R11, R12, R14.

- F2. Offensive dispel (Purge)
  - **Trigger:** An enemy carries a beneficial aura worth removing (e.g. the
    enemy healer's shield/absorb, an enemy's attack-power or damage-reduction
    buff).
  - **Steps:** Shaman AI selects the highest-value beneficial aura on an enemy in
    range; Purge removes that one aura.
  - **Outcome:** The enemy loses the buff; the Shaman has spent a GCD on offense
    rather than healing.
  - **Covered by:** R8.

- F3. Cast denial (Wind Shear)
  - **Trigger:** An enemy is casting an interruptible spell (notably an enemy
    heal).
  - **Steps:** Shaman AI fires Wind Shear at the caster; the target is locked out
    of that spell school briefly.
  - **Outcome:** The enemy cast is interrupted and that school is denied for the
    lockout window.
  - **Covered by:** R9.

## Acceptance Examples

- AE1. **Covers R11.** An ally standing inside a Healing Stream Totem's radius
  receives the periodic heal each tick; the same ally, after moving out of
  radius, stops receiving it.
- AE2. **Covers R12.** With a Fire totem already active, casting another Fire
  totem removes the first immediately and starts the second's full duration;
  casting a Water totem in the same situation leaves the Fire totem untouched
  (different element slot).
- AE3. **Covers R14 (Windfury).** A Windfury Totem buffs a melee ally's
  auto-attacks but produces no benefit for a caster ally in the same radius.
- AE4. **Covers R8.** Purge cast on an enemy carrying both a shield/absorb and a
  minor buff removes the higher-value defensive first; Purge cast on an enemy
  with no beneficial auras is not used (no valid target).
- AE5. **Covers R13.** An enemy combatant cannot target, damage, or destroy an
  active totem; the totem persists until its duration expires or the Shaman
  replaces it.

## Scope Boundaries

**Deferred for later**

- Destructible totems (totem HP, enemy target-selection to kill totems, Shaman
  replacement behavior). A future layer once the class is balanced.
- Offensive totems — Searing Totem (a totem that auto-attacks an enemy) and
  Earthbind Totem (an enemy-slowing zone). Reconsidered only if the kit's
  offense proves insufficient.

**Outside this class's identity**

- Hard crowd control (stun / fear / root / polymorph). The Shaman is
  deliberately the CC-less healer; adding a panic button would erase the
  glass-cannon identity that distinguishes it from Priest and Paladin.
- A pet. The Shaman is not a pet class.

## Dependencies / Assumptions

- New aura types are needed to back the totem buffs and may not exist today:
  a spell-power buff, a heal-over-time buff, and the Windfury melee proc.
  Attack-power and crit buffs already exist. (Planning verifies exact gaps
  against `components/auras.rs`.)
- Offensive dispel is new behavior. Today's dispel removes debuffs from allies
  only, and beneficial auras are flagged not magic-dispellable; Purge needs new
  enemy-buff-removal logic and a way to mark which beneficial auras are
  purgeable.
- The totem radius-pulse mechanic can reuse the existing positioned-aura-zone
  pattern (the Frost Trap slow zone), aimed at allies instead of enemies.
- Wind Shear reuses the existing interrupt/lockout mechanic shared by Pummel and
  Kick.
- The new caster-mail gear must fit the item-level budget; spell power and mana
  are budgeted stats, so the loadout is a budgeting exercise, not a free grant.

## Success Criteria

- The Shaman is viable but not dominant in 2v2/3v3 — measured by balance sweeps
  per the project's methodology (clean slices, 2v2/3v3 over 1v1), not auto-win or
  auto-lose with a competent partner.
- The class reads as *offensive*: in traces and logs it spends real time on
  Lightning Bolt / Purge / Wind Shear, not purely healing and kiting.
- Totem positioning is a live decision, not dead weight: allies spend meaningful
  time inside totem radii, and the Shaman's roam-vs-anchor behavior keeps totems
  useful rather than dropping them away from the team. This is the central
  tuning risk created by spawning totems at the Shaman's feet.
- Melee comps are a hard but not unwinnable matchup, reflecting the no-hard-CC
  trade-off.

## Sources / Research

- Positioned-aura-zone analog for totems: the Frost Trap slow zone —
  `src/states/play_match/components/pets.rs` (`SlowZone`) and
  `src/states/play_match/traps.rs` (`slow_zone_system`). Owned secondary-entity
  model (HP, ownership) for a future destructible version: the Hunter pet —
  `src/states/play_match/components/pets.rs`, `src/states/play_match/class_ai/pet_ai.rs`.
- Dispel system (defensive today): `src/states/play_match/effects/dispels.rs`,
  `DispelPending` in `src/states/play_match/components/combatant.rs`,
  `try_dispel_ally` in `src/states/play_match/class_ai/mod.rs`.
- Aura types and dispel classification:
  `src/states/play_match/components/auras.rs`.
- Class wiring checklist: `src/states/match_config.rs` (`CharacterClass` enum and
  trait methods), `src/states/play_match/components/combatant.rs` (base stats),
  `src/states/view_combatant_ui.rs`, `src/states/configure_match_ui.rs`,
  `src/states/play_match/combat_ai.rs` (class AI dispatch),
  `src/states/play_match/class_ai/mod.rs`, plus a new
  `src/states/play_match/class_ai/shaman.rs`.
- Healer posture machine and tuning surface:
  `src/states/play_match/class_ai/priest.rs`, `paladin.rs`,
  `src/states/play_match/movement_config.rs`, `assets/config/movement.ron`.
- Ability and gear data: `assets/config/abilities.ron`,
  `assets/config/items.ron`, `assets/config/loadouts.ron`. Existing mail is
  Hunter-physical (e.g. the Beaststalker set) — small mana, no spell power.
- Reference values when implementing abilities/items: Wowhead Classic MCP
  (`lookup_spell`, `lookup_item`) per project conventions.
