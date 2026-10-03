# WoW Mechanics Reference

Implemented WoW Classic mechanics adapted for our autobattler. Reference this document when implementing new abilities or debugging combat behavior.

---

## Combat System

### Global Cooldown (GCD)
- 1.5 seconds between most abilities
- Prevents ability spam, creates tactical decisions

### Shared Cooldown Categories
- Some abilities share one cooldown: using any member puts every member on
  it. Declared per ability as `cooldown_category` in `abilities.ron`, after the
  Classic client's `SpellCategory`; applied by `Combatant::start_cooldown`,
  the one place a cooldown starts (`tests/cooldown_site_audit.rs`).
- **Traps.** Freezing Trap and Frost Trap are both `Trap`: throwing either
  locks both for 15s. Source: client 1.15.9.69547 (wago.tools DB2) —
  `SpellCategories` puts every rank of Freezing (1499/14310/14311), Frost
  (13809), Immolation and Explosive Trap in category 411 "Trap", and
  `SpellCooldowns` gives each a 15000ms `CategoryRecoveryTime` and a
  `RecoveryTime` of 0, so the category cooldown is the traps' only cooldown.
  It sits beside the one-active-trap rule (a new trap replaces the Hunter's
  live one): the cooldown decides when the next trap can go, the slot decides
  what happens to the last.

### Casting
- Movement stops while casting non-instant spells
- Caster faces target when beginning a cast
- Cast bars show spell name and progress
- Cast can be interrupted by enemy abilities

### Interrupts & Spell Lockout
- Interrupting a cast locks that spell school for X seconds
- Only the interrupted school is locked (e.g., interrupting Frostbolt locks Frost, not Arcane)
- Spell schools: Physical, Fire, Frost, Shadow, Arcane, Holy, Nature

### Auto-Attacks
- Disabled while casting
- Melee: Within MELEE_RANGE (2.5 units)
- Ranged: Mage/Priest use "Wand Shots" at 40 unit range
- **The Hunter has two auto-attacks on separate timers**, as in Classic: Auto
  Shot from its bow at `HUNTER_DEAD_ZONE`..`AUTO_SHOT_RANGE` (8-35yd), and a
  melee auto-attack from its main-hand weapon (and off-hand weapon, when it
  dual wields) within `MELEE_RANGE`. Between the two lies the dead zone, where
  it does neither. The bow keeps `attack_timer`; the melee main hand has its own
  `melee_timer` (`Combatant::melee_damage` / `melee_weapon_speed`), so closing to
  melee right after a shot swings at once rather than waiting out the bow's
  interval, and every timer keeps building out of range, like any other swing.
  A Hunter with no main-hand weapon does nothing in melee — the sim has no
  unarmed swing. The default Hunter carries a two-handed polearm, Peacemaker
  (AS-195), so a melee that pins it in melee range is struck back. The melee swing
  is a melee swing everywhere: it procs `MeleeHit` trinkets, draws Frost Armor's
  chill and can take a Windfury bonus swing; Auto Shot does none of those.
  The model shows one set at a time: the hand weapons once its target closes to
  melee reach, the bow once the target is back at the Auto Shot minimum, and in
  the dead zone between, whichever it had (`WeaponSetSwap`).
- **Every swing's speed comes from the weapon.** `ItemConfig::weapon_speed` is
  seconds per swing, the Classic tooltip's "Speed", copied from a named real
  Classic item for every weapon — main hand, off hand, two-hander, bow,
  crossbow and wand (AS-167; the stand-in table is
  `docs/design/balance/2026-09-28-as167-weapon-speed.md`). There is no
  per-class speed. A combatant built without equipment swings at
  `UNARMED_WEAPON_SPEED` (2.0s, Classic's unarmed speed); an equipped one whose
  live socket is empty does not auto-attack at all.
- **A two-hander carries Classic's weapon-DPS premium** over the one-hander it
  displaces (1.30-1.42x, per tier, from the Classic items' own DPS) — the
  damage that buys back the off hand it gives up.
- **Per-swing procs stay a flat chance per hit**, as Classic rolled them
  (Windfury Totem "each hit has a 20% chance", Crippling Poison "each strike
  has a 30% chance"): Crippling Poison, Windfury Totem and `MeleeHit` proc
  trinkets roll their chance on every landed swing, so a slower weapon procs
  less often per second but each proc is a slower weapon's bigger hit.

### Dual Wield
- **Who can**: Warrior, Rogue, Hunter — Classic's list, in
  `equipment::can_dual_wield`. A Shaman cannot: that is the Burning Crusade
  Enhancement talent, the same era line the weapon-proficiency table draws when
  it refuses a Rogue an axe.
- **What fits where**: a one-handed weapon fits EITHER hand; a two-hander only
  the main hand; a shield or held frill only the off hand. The item states this
  as `HeldSlot` (`ItemConfig::held()`), and `ItemSlot::accepts` is membership in
  that set rather than equality on a slot kind.
- **Two copies of one weapon is legal**, unlike two copies of one ring. The
  hands are a pair of sockets and deliberately NOT a unique-equipped pair.
- **The off hand swings on its own timer** at `OFFHAND_DAMAGE_MULTIPLIER` (50%)
  of its listed damage, so two weapons of different speeds drift apart over a
  match. It carries no Heroic Strike bonus — that buffs the main hand's next
  swing.
- **Dual wielding costs accuracy**: every swing, BOTH hands, rolls against
  `DUAL_WIELD_MISS_CHANCE` (19% — Classic's 24% dual-wield white-hit miss less
  its 5% baseline). This is the sim's only miss roll; a single-wielding
  attacker never rolls it, which is what keeps every pre-existing match
  byte-identical.
- **A Hunter's off hand swings beside its melee main hand** (see Auto-Attacks),
  in melee range only. With no main-hand weapon it arms no swing — the off hand
  accompanies the main hand — and may still hold a weapon for the stats. The
  miss roll is charged to the melee swings only, never to Auto Shot.
- **The Rogue dual wields by default**, and is the only class that does: a
  second Serpent Fang Dagger in its off hand, with the power it is worth
  measured rather than assumed (AS-122 —
  `docs/design/balance/2026-09-18-as122-rogue-offhand-findings.md`). The Warrior
  and the default Hunter each hold a two-hander, so for those two
  dual wield stays a build a player opts into.

### Windfury Totem procs on the main hand only

The Shaman's Air Totem grants `WindfuryBuff`, and a melee ally carrying it rolls
for one bonus swing per landed auto. **That roll happens on the main-hand swing
and nowhere else** — the off-hand branch in `combat_core/auto_attack.rs` has no
twin of it.

This is a deliberate simplification, and the reason is how the buff was played
rather than how it was coded:

- In Classic, Windfury Totem applies a temporary **weapon enchant** while it
  is active — it occupies a weapon's enchant slot.
- A Rogue keeps a **poison** in the off hand, which consumes exactly the slot
  the Windfury enchant would otherwise land in. That *forces* Windfury onto the
  main hand.
- Players wanted it there anyway. Each hit has a flat 20% chance of an extra
  attack (Wowhead spell 8512), and that extra attack is a swing of the weapon
  that procced, so a proc is worth more on the main hand's bigger hit. The
  single-poison arrangement got value out of the off hand at the same time.

So main-hand-only is the realistic *outcome* of how the buff was actually used.
We model that outcome directly rather than modelling the enchant slot that
produces it.

---

## Resource Systems

### Mana
| Class   | Max Mana | Regen/sec |
|---------|----------|-----------|
| Mage    | 200      | 10        |
| Priest  | 150      | 8         |
| Warlock | 200      | 8         |
| Paladin | 160      | 8         |

### Rage (Warrior)
- Max: 100
- Starts every match at 0: the pre-match countdown holds rage empty while it
  holds mana and energy full (`Combatant::pre_combat_resource`)
- Generates on damage dealt and received. A landed swing pays
  `RAGE_PER_WEAPON_SECOND` (9) per second of its weapon's speed — 34.2 for a
  3.8s two-hander — so a slow weapon earns the same rage per second as a fast
  one; an off-hand swing pays half its own weapon's share.
- Decays over time out of combat
- No passive regeneration

### Energy (Rogue)
- Max: 100
- Regenerates: 5/sec (constant rate)
- Instant regeneration tick model

---

## Crowd Control

### Root
- Prevents movement only
- Target can still attack and cast spells
- Example: Frost Nova (6s duration)
- Entangling Roots (Druid, 1.5s cast, 8s) is a root plus a small Nature DoT,
  bound into one compound debuff: whatever ends the root (80 damage, a dispel,
  a Travel Form shift, its duration) ends the DoT too. The DoT's own ticks
  count toward the break.

### Stun
- Prevents all actions (movement, attacking, casting)
- Examples: Kidney Shot, Charge stun component

### Fear
- Target runs in random directions
- Direction changes every 1-2 seconds
- Breaks on damage (threshold: 100 damage)
- Prevents intentional movement, attacking, casting
- Example: Warlock Fear (8-10s duration)

### Polymorph (Incapacitate)
- Target wanders at 50% speed
- Breaks on ANY damage (threshold: 0)
- Separate category from stuns for future diminishing returns
- Example: Mage Polymorph (10s duration)

### Cyclone (Druid, TBC)
- 1.5s cast, 20 yd, 6s: the target cannot move or act
- The target is immune to damage AND healing for the duration, and no new
  aura of any kind lands on it (friendly or hostile). The damage immunity is at
  the top of `apply_damage_with_absorb`, the healing immunity at the top of
  `apply_healing`, the aura immunity in `apply_pending_auras` — plus the three
  sites that push an aura directly (Frost Trap zone, totem pulse, Crippling
  Poison)
- Never breaks on damage, and no dispel takes it
- Diminishes on its own bucket (`DRCategory::Cyclone`): Polymorph and Fear do
  not shorten it, and it does not shorten them
- Nothing is used from inside it — not Divine Shield, not Berserker Rage
- The other classes' AIs play against it (AS-163): it counts as immunity
  through the same predicate as Divine Shield (`class_ai::grants_damage_immunity`),
  so attackers switch off a cycloned target, interrupts and Mana Burn are not
  spent on it, and a healer neither heals, buffs nor dispels a cycloned ally.
  Its CAST is every interrupter's first pick — Kick, Pummel, Wind Shear and
  Spell Lock take an enemy Druid's Cyclone over any other cast in reach
  (`class_ai::priority_interrupt_target`)
- Mana Burn is blocked by Cyclone and by Divine Shield alike: a burn that lands
  on either destroys nothing (`[MANA BURN] ... fails: ... is immune`)

### Purge priority (Shaman)
- Keyed by the aura INSTANCE, not its type (`class_ai::aura_purge_priority`):
  the type's default (`purge_priority`), unless the aura BLOOMS (Lifebloom —
  worth 0, since a purge that takes it heals its bearer) or its source ability
  sets `purge_priority` in its RON `applies_aura` (Innervate — 110, the urgent
  bar, which the Shaman purges ahead of its filler Lightning Bolt)
- Rejuvenation keeps the `HealingOverTime` default, 70 — exactly the floor: a
  legitimate purge, taken only in a GCD the Shaman has nothing better for
- A purge chosen for a source-keyed buff takes that buff only
  (`DispelScope::PurgeSource`), so a purge for a Rejuvenation never blooms the
  Lifebloom beside it

### Travel Form (Druid escape shift)
- Instant, 25 mana, on the global cooldown. Shifting removes every Root and
  every `MovementSpeedSlow` on the Druid, whatever its removal class (the
  physical snares too), each as a whole debuff
- While shifted: +40% movement speed, immune to Polymorph, no casting and no
  auto-attacks. Not purgeable or dispellable — a shapeshift is not a spell on
  the Druid
- **Leaving the form is free and costs no global cooldown** (WoW's cancel-aura),
  so the Druid can shift out and cast an instant heal on the same beat. The
  form has no timer that matters (600s ceiling); the Druid ends it
- No second resource pool and no kit swap — an escape shift only
- AI: the Druid shifts when PRESSURED and rooted, or snared with a melee or pet
  chasing it, or below 60% HP with a melee on it. A snare with no chaser is
  shrugged off — the form outruns legs, not spells. The posture machine then
  runs it in ESCAPE from its melee and pet chasers (`ShiftEscape`). It shifts
  out at once if it is rooted again, and re-shifts through the root. Otherwise
  it shifts out once it has been shifted 3s, no melee or pet is within
  striking reach (nor within 12 yd, for its first 8s in form), the shift rule
  would not fire straight back, and it has work — an ally in range below 80%
  HP, or an enemy within Moonfire's range. With nothing to heal and nothing in
  reach it stays shifted. After a leave of its own accord it holds the shift
  back for one global cooldown, as the cast it left to make would

### Future: Diminishing Returns
- Not yet implemented
- Same CC type on same target has reduced duration
- Categories: Stun, Fear, Incapacitate, Root

---

## Defensive Mechanics

### Absorb Shields
- Damage absorbed before health is reduced
- Multiple shields can coexist (Ice Barrier + Power Word: Shield)
- Each shield tracked by ability_name, not just AuraType
- Shield value depletes as damage is absorbed
- Visual: Bubble around shielded combatant

### Weakened Soul
- Applied by Power Word: Shield
- Prevents re-application of PW:S for 15 seconds
- Does NOT prevent other absorb effects (Ice Barrier)

### Stealth (Rogue)
- Invisible to enemies
- Breaks on damage or ability use
- Visual: 40% opacity, purple "STEALTH" label
- Shadow Sight orbs spawn after 90s to counter stealth stalemates
- A lit enemy Flare is visible to a stealthed Rogue, and it plays around the
  light (`class_ai/rogue_flare.rs`): it walks round the light when a way to its
  target stays dark, holds 4yd outside it when none does, and goes in anyway
  when a teammate (not a pet) falls below half health or the light over its
  approach is relit. Whether a Flare finds the Rogue is geometry only — where
  it was lit and which way the Rogue walked — never a roll. Traced as the
  `FlareSkirt` / `FlareWait` / `FlareCommit` movement triggers, each with a
  `[FLARE] <Rogue> skirts / holds outside / goes into the Flare` log line.

---

## Buffs

### Pre-Match Buffing Phase
- 10 second countdown before gates open
- Mana restored each frame during countdown
- Combatants can cast buffs on allies
- Examples: Power Word: Fortitude, Arcane Intellect

### Stat Buffs
| Buff               | Effect        | Duration |
|--------------------|---------------|----------|
| Fortitude          | +100 Max HP   | 300s     |
| Arcane Intellect   | +40 Max Mana  | 300s     |

---

## Spell Schools

| School   | Color       | Classes Using                    |
|----------|-------------|----------------------------------|
| Physical | White       | Warrior, Rogue                   |
| Fire     | Orange      | (Future: Mage Fire spec)         |
| Frost    | Blue        | Mage (Frostbolt, Frost Nova, Ice Barrier) |
| Shadow   | Purple      | Warlock, Priest (Mind Blast)     |
| Arcane   | Pink/Purple | Mage (Polymorph)                 |
| Holy     | Gold        | Priest, Paladin (heals, Holy Shock, HoJ) |
| Nature   | Green       | Shaman, Druid                    |

---

## Damage & Healing Formulas

### Stat Scaling
```
Damage = Base + (Stat × Coefficient)
Healing = Base + (Spell Power × Coefficient)
```

### Scaling Stats
- `AttackPower`: Warrior, Rogue physical abilities
- `SpellPower`: Mage, Priest, Warlock magical abilities
- `None`: Utility/CC abilities with no scaling

### Class Base Stats
| Class   | Attack Power | Spell Power |
|---------|--------------|-------------|
| Warrior | 30           | 0           |
| Rogue   | 35           | 0           |
| Mage    | 0            | 50          |
| Priest  | 0            | 40          |
| Warlock | 0            | 45          |
| Paladin | 20           | 35          |

### Example Coefficients
| Ability       | Base Damage | Coefficient | Scales With |
|---------------|-------------|-------------|-------------|
| Frostbolt     | 10-15       | 80%         | SpellPower  |
| Ambush        | 20-30       | 120%        | AttackPower |
| Flash Heal    | 15-20       | 75%         | SpellPower  |
| Mind Blast    | 15-20       | 85%         | SpellPower  |

---

## Break-on-Damage Semantics

For auras that can break on damage:

| Threshold Value | Behavior                          |
|-----------------|-----------------------------------|
| `-1.0`          | Never breaks on damage (buffs)    |
| `0.0`           | Breaks on ANY damage (Polymorph)  |
| `100.0`         | Breaks after 100 cumulative damage (Fear) |

---

## Aura Types Reference

| AuraType            | Effect                                    |
|---------------------|-------------------------------------------|
| `Absorb`            | Damage shield that depletes              |
| `Root`              | Prevents movement                        |
| `Stun`              | Prevents all actions                     |
| `Fear`              | Random movement, breaks on damage        |
| `Polymorph`         | Slow wander, breaks on any damage        |
| `MovementSpeedSlow` | Reduces movement speed by magnitude %    |
| `HealingReduction`  | Reduces healing received (Mortal Strike) |
| `DamageOverTime`    | Periodic damage ticks                    |
| `MaxHealthIncrease` | Temporary max HP buff                    |
| `MaxManaIncrease`   | Temporary max mana buff                  |
| `SpellLockout`      | Prevents casting school for duration     |
| `WeakenedSoul`      | Prevents PW:S reapplication              |
| `ShadowSight`       | Can see stealthed enemies                |
| `Cyclone`           | No acting; immune to damage, healing and new auras |
| `TravelForm`        | Druid shapeshift: speed, Polymorph immunity, no casting |

---

## Intended Behaviors (Not Bugs)

These behaviors may look like bugs but are intentional design decisions. Do not report these during bug hunts.

### Damage After Caster Death

| Scenario | Intended? | Reason |
|----------|-----------|--------|
| DoT damage continues after caster dies | ✅ Yes | Authentic WoW behavior - applied effects persist |
| Projectile in flight hits after caster dies | ✅ Yes | Projectile was already launched before death |
| Simultaneous ability kills (both combatants die) | ✅ Yes | Abilities resolving at same timestamp is valid; RNG/gear/talents will reduce frequency |

### What IS a Bug

| Scenario | Bug? | Reason |
|----------|------|--------|
| Dead unit initiates NEW attack after death | ❌ Bug | Fixed via `died_this_frame` tracking |
| Dead unit starts casting after death | ❌ Bug | Should be caught by alive checks |
| Queued melee attack lands after attacker dies in same frame | ❌ Bug | Fixed in combat_core.rs |

---

## Paladin Abilities

| Ability | Type | Cast Time | Effect |
|---------|------|-----------|--------|
| Devotion Aura | Buff | Instant | 10% damage reduction to all allies |
| Flash of Light | Heal | 1.5s | Fast, efficient single-target heal |
| Holy Light | Heal | 2.5s | Large heal for safe situations |
| Holy Shock | Instant | - | Heals ally OR damages enemy (20yd range for damage) |
| Cleanse | Utility | Instant | Removes 1 dispellable debuff (Poly/Fear/Root/DoT) |
| Hammer of Justice | CC | Instant | 10yd range, 6s stun, prioritizes healers |
