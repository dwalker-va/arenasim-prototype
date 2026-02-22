# Hunter Class Brainstorm

**Date:** 2026-02-22
**Status:** Ready for planning

## What We're Building

A Hunter class — the 7th class in ArenaSim — centered around ranged auto-attacks, a dead zone mechanic, choosable pets with unique abilities, and ground-targeted traps with proximity triggers. The Hunter fills the "ranged physical DPS" niche that doesn't currently exist, distinct from caster ranged (Mage/Warlock) and melee physical (Warrior/Rogue).

## Why This Approach

### Core Identity: Ranged Physical + Pet + Area Denial

The Hunter's identity comes from three interacting systems:

1. **Dead zone** forces the Hunter to actively manage positioning — stay outside 8 yards but within 35-40 yards. This gives melee a clear counterplay window (close the gap into the dead zone) while rewarding the Hunter for maintaining distance.

2. **Choosable pets** let the Hunter adapt to team comps. Spider for CC chains, Boar for pressure, Bird for counter-kiting.

3. **Traps as targeted area denial** — a new mechanic in ArenaSim. Traps are placed at a target location, arm after a delay, and trigger on enemy proximity. Frost Trap creates a persistent slow zone that forces enemies to path around it.

## Key Decisions

### Resource: Mana
- Classic WoW authentic
- Reuses existing mana infrastructure (regen, costs, OOM pressure)
- No new resource type needed

### Dead Zone: Hard Minimum Range (~8 yards)
- Ranged abilities (Auto Shot, Aimed Shot, Arcane Shot, Concussive Shot) cannot be used within 8 yards
- Hunter has no melee kit — must escape the dead zone to deal damage
- Primary escape: **Disengage** (instant backward leap ~15 yards, 25s CD)
- Secondary escapes: Frost Trap at feet while fleeing, pet CC (Spider Web, Boar stun)
- AI must prioritize maintaining distance, similar to Mage kiting but with different tools (Disengage + Concussive Shot + traps instead of Frost Nova + Blink)

### Core Abilities (5 active + Auto Shot)
| Ability | Type | Cooldown | Key Mechanic |
|---------|------|----------|-------------|
| **Auto Shot** | Ranged auto-attack | None (timer-based) | Replaces wand attack. Continuous ranged DPS when not casting. Requires 8+ yard range. Pauses during casts. |
| **Aimed Shot** | Cast time (~2.5s) | 10s | Big physical damage + healing reduction (Mortal Strike effect). Primary burst ability. |
| **Arcane Shot** | Instant | 6s | Moderate Arcane damage. Bread-and-butter instant between Auto Shots. |
| **Concussive Shot** | Instant | 10s | Applies movement speed slow. Key kiting tool. Requires 8+ yard range. |
| **Disengage** | Instant | 25s | Backward leap ~15 yards. Emergency escape from dead zone. No range requirement (used in melee). |

Auto Shot is the primary sustained damage source. Abilities punctuate the rotation as periodic power spikes.

### Traps: Freezing Trap + Frost Trap
**Mechanic:** Targeted placement within range. 1-2 second arming delay. Proximity trigger (~5 yard radius). Independent cooldowns. One of each type max on the field (placing a second despawns the first).

| Trap | Cooldown | Trigger | Effect |
|------|----------|---------|--------|
| **Freezing Trap** | 25s | First enemy contact | Incapacitates target (breaks on damage, like Polymorph). Single target. Trap consumed. |
| **Frost Trap** | 20s | First enemy contact | Creates persistent slow zone entity (~10s duration, ~8 yard radius). Enemies inside receive a refreshing movement speed slow. Zone persists even after triggering enemy leaves. Trap consumed, zone remains. |

**Implementation:** Traps are ECS entities with a `Trap` component (position, arm timer, trigger radius, effect type, owner). A `trap_proximity_system` checks armed trap positions against enemy combatant positions each frame. Consistent with existing projectile entity pattern.

**Frost Trap zone** is a second ECS entity spawned on trigger — a `SlowZone` component with position, radius, duration, and slow magnitude. A `slow_zone_system` applies/refreshes slow auras on enemies within the zone each frame.

### Pets: 3 Choices via Config
Pet choice is configured in match config (headless JSON + UI), similar to Warlock curse selection or Rogue opener config.

| Pet | Role | Special Ability |
|-----|------|----------------|
| **Spider** | Offensive CC | **Web** (45s CD) — ranged root on target. Gives Hunter breathing room to re-establish range. |
| **Boar** | Aggressive pressure | **Charge** (45s CD) — gap closer + short stun on target. Disrupts enemy casters, creates burst windows. |
| **Bird** | Defensive utility | **Master's Call** (45s CD) — removes movement impairing effects from both the pet and a friendly target. Counter to slows/roots. |

All pets also auto-attack in melee (using existing pet auto-attack system from Felhunter).

**PetType extension:** Add `Spider`, `Boar`, `Bird` variants to existing `PetType` enum. Each gets stats (health, speed, preferred range) and AI behavior in `pet_ai.rs`.

### Pet AI Decision Logic

**Spider — Web (ranged root):**
- **When to use:** Enemy is closing on Hunter (within 15 yards and approaching). Defensive use — protect the Hunter's range.
- **Don't use on:** Already-rooted or slowed targets (waste of 45s CD). Targets that are moving away.
- **Philosophy:** Shield the Hunter's positioning. Spider is a bodyguard.

**Boar — Charge (gap closer + stun):**
- **Primary:** Charge enemy casters who are mid-cast (especially healers). The stun interrupts their cast.
- **Secondary:** If no active casts to interrupt, charge the primary kill target for burst window pressure.
- **Philosophy:** Interrupt-first. Boar is a disruptor.

**Bird — Master's Call (cleanse movement impairments):**
- **Primary:** Use when Hunter has a movement impairment (root, slow, snare). Frees the Hunter to kite.
- **Secondary:** If Hunter is already mobile, use on a teammate who has movement impairments.
- **Philosophy:** Hunter-first. Bird is a freedom enabler.

## Hunter AI Strategy

### Overall Philosophy: Control-First
The Hunter prioritizes maintaining distance and controlling space over raw damage output. Damage is woven in when the situation is safe.

### Priority by Range Zone

**Safe range (20-40 yards):**
1. Keep target slowed (Concussive Shot if not already slowed)
2. Place Frost Trap proactively between self and enemy (area denial)
3. Place Freezing Trap on healer/CC target if available
4. Aimed Shot if safe to cast (target slowed, not closing fast)
5. Arcane Shot as instant filler
6. Auto Shot runs continuously

**Closing range (8-20 yards):**
1. Concussive Shot (if target not slowed and in range)
2. Frost Trap between self and enemy
3. Kite away to re-establish safe range
4. Arcane Shot while moving (instant)

**Dead zone (<8 yards):**
1. Disengage (backward leap to escape, if available)
2. Drop Frost Trap at current position while fleeing
3. Sprint away to 8+ yard range
4. Once at range: Concussive Shot to slow pursuer

### Trap Placement: Predictive Pathing
The AI places traps based on enemy movement prediction:
1. Read target's current movement direction/velocity
2. Estimate where they'll be in ~2 seconds (arming delay)
3. Place trap at predicted intercept point
4. Frost Trap placed as "tripwires" between Hunter and approaching enemies
5. Freezing Trap placed on CC targets (healers) predicted position

### Kiting Pattern
Unlike Mage (which uses Frost Nova for instant distance), Hunter kiting relies on:
- Concussive Shot for ranged slowing
- Disengage for emergency repositioning
- Frost Trap zones as persistent barriers
- Pet CC (Spider Web / Boar stun) for breathing room
- Constant movement away from approaching enemies while maintaining 20-35 yard range

## New Systems Required

1. **Trap entity system** — spawn, arm timer countdown, proximity detection, trigger + despawn
2. **Slow zone system** — persistent AoE entity, applies refreshing slow aura to enemies inside, duration countdown + despawn
3. **Dead zone enforcement** — minimum range check on ranged abilities (new concept, doesn't exist yet)
4. **Hunter AI** — kiting logic (maintain 15-30 yard range), trap placement strategy, ability priority
5. **3 new pet AI behaviors** — Spider/Boar/Bird decision logic in `pet_ai.rs`

## Resolved Design Questions

- **Trap cooldowns:** Independent. Each trap has its own cooldown (not shared). Hunter can deploy Freezing Trap and Frost Trap in sequence without waiting.
- **Trap limit:** One of each type max. Can have one Freezing Trap and one Frost Trap deployed simultaneously. Placing a second of the same type despawns the first. Up to 2 traps on the field.
- **Auto Shot + casting:** Auto Shot pauses during casts and resumes after. No shot-weaving complexity. Clean and simple for AI.
- **Pet health:** Same as Felhunter (45% of owner HP) for all pet types. Consistent with existing framework.

## Remaining Open Questions

- **Frost Trap zone visuals?** Ground decal/particle effect for the slow zone. Needs to be readable at a glance.
- **Dead zone exact range?** ~8 yards assumed, may need tuning.
- **Trap arming delay exact value?** 1-2 seconds TBD during implementation.

## Existing Infrastructure to Reuse

- **Pet framework**: `Pet` component, `PetType` enum, `pet_ai.rs`, pet spawning in `Combatant::new()`
- **Auto-attack system**: `combat_auto_attack()` already handles ranged attacks for non-melee classes via `WAND_RANGE`
- **Aura system**: Slow auras (`MovementSpeedSlow`), incapacitate auras, healing reduction (`HealingReduction`)
- **Projectile system**: Trap entity lifecycle mirrors projectile spawn/travel/hit pattern
- **Kiting AI**: Mage AI in `mage.rs` has kiting logic that can inform Hunter AI (but Hunter kites differently — no instant CC like Frost Nova)
