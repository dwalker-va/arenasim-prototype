# Warlock Pet System Brainstorm

**Date:** 2026-02-15
**Status:** Ready for planning

## What We're Building

A pet system for Warlocks, starting with the Felhunter demon. The pet is a full combat participant — targetable, killable, with its own health pool and abilities. It follows the Warlock's target but makes its own ability decisions (when to interrupt, when to dispel).

This system is designed to be reusable for Hunter pets in the future.

### Felhunter Abilities
- **Devour Magic** — Offensive dispel (removes a beneficial buff from an enemy) or defensive dispel (removes a harmful debuff from the Warlock/ally)
- **Spell Lock** — Interrupt + spell school lockout (like Pummel/Kick but ranged)
- **Melee attacks** — Basic auto-attack damage

## Why This Approach

**Architecture: PetAI trait with dedicated pet_ai_system**

We chose a separate `PetAI` trait (parallel to `ClassAI`) rather than reusing `ClassAI` or having the Warlock control pet abilities directly. Reasons:

- **Clean separation** — Pet AI needs different context than player AI (owner's target, owner's state). A dedicated trait provides the right abstraction.
- **Scales to Hunters** — Hunter pets are more autonomous than Warlock demons. A `PetAI` trait supports both easily.
- **Keeps Warlock AI simple** — The Warlock's `decide_action()` stays focused on Warlock decisions. Pet decisions happen in a parallel system.

**Entity structure: Pet as a full Combatant entity**

The pet spawns as a regular ECS entity with all standard combat components (`Health`, `Team`, `Combatant`, etc.) plus a `Pet { owner: Entity }` marker component. This reuses all existing combat infrastructure — damage application, aura processing, targeting, projectiles — without modification.

## Key Decisions

1. **Pets are full combat participants** — They have health, can be targeted and killed, deal real damage and use abilities.

2. **WoW-style victory conditions** — Killing a pet does NOT win the match. Only "primary combatants" (entities without the `Pet` component) count toward the team wipe condition.

3. **Start with Felhunter only** — Proves out the system with one well-defined pet. Felhunter has clear, utility-focused AI decisions (interrupt casters, dispel buffs).

4. **Owner-guided AI** — Pet follows the Warlock's current target but makes its own ability decisions via a `PetAI` trait. The Warlock AI can issue high-level guidance (e.g., "focus this target") through a shared component.

5. **Pet exists at match start** — No summoning mechanic. Pet spawns automatically with the Warlock. Keeps things simple.

6. **Separate PetAI trait and system** — A `pet_ai_system` runs alongside `class_ai_system`. `PetAI` implementations (FelhunterAI, future SuccubusAI, HunterPetAI) each define their own `decide_action()`.

## Integration Points

These are the key areas of the codebase the pet system will touch:

- **Entity spawning** (`play_match/mod.rs`) — Spawn pet entity alongside Warlock during match setup
- **Victory conditions** (`match_flow.rs`) — Filter out `Pet` entities from team-alive checks
- **Target selection** (`combat_ai.rs`) — Enemies can choose to target pets; pets need to be valid targets
- **AI system** — New `pet_ai/` module with `PetAI` trait and `FelhunterAI` implementation
- **Warlock AI** (`class_ai/warlock.rs`) — May need to communicate target preference to pet
- **Headless config** — Consider whether pet type is configurable per-match or always Felhunter
- **Combat components** (`components/mod.rs`) — New `Pet` component, possibly `PetOwner` on the Warlock
- **Abilities** (`abilities.rs`, `abilities.ron`) — New pet ability types (DevourMagic, SpellLock, PetMelee)

## Refined Decisions (from follow-up discussion)

7. **Pets are fully targetable** — Enemies can explicitly target and kill the pet. The `kill_target` config supports specifying a pet as a priority target. This enables the strategic choice of "kill the Felhunter to free up casting."

8. **Follow-owner + engage movement** — Pet follows the Warlock at a short distance when idle, then runs to engagement range when it has a target. Engagement range is parameterized per pet type (melee range for Felhunter, ranged for a future Imp). This keeps the movement system generic and reusable.

9. **Pet stats scale with owner** — Pet health and damage scale with the Warlock's stats (stamina for health, spell power for damage). Pets have significantly less health than player combatants (~40-50% of a player).

10. **Context-dependent Devour Magic AI** — Smart prioritization: (1) Remove CC from allies (highest priority — Polymorph, Fear), (2) Eat key enemy defensive cooldowns (Ice Barrier, Power Word: Shield, Divine Shield), (3) Clean up lesser debuffs on allies. This makes the Felhunter feel intelligent and impactful.

11. **Pet type configurable in match config** — Extend the JSON config to support pet selection now (e.g., `{"class": "Warlock", "pet": "Felhunter"}`). Future-proofs the config for multiple demon types and Hunter pets. Default to Felhunter if not specified.

## Open Questions

None — all design questions have been resolved. Ready for `/workflows:plan`.
