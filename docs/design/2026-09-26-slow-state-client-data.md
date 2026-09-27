# AS-133: MovementSpeedSlow victim state — Client-Data Research

All findings are from WoW Classic Era build **1.15.9.69547** client data (wago.tools DB2
CSVs), joined directly. No prose/wiki sources were used as evidence.

## Method notes

Same chain as `2026-09-06-frost-shock-client-data.md`: `SpellXSpellVisual[SpellID] →
SpellVisualEvent[SpellVisualID] → SpellVisualKit → SpellVisualKitEffect`, with
`EffectType == 2` → `SpellVisualKitModelAttach` → `SpellVisualEffectName.ModelFileDataID`
(filenames from the community listfile), and `EffectType == 1` → `SpellProceduralEffect`.
An aura STATE is a `(StartEvent, EndEvent) = (7, 8)` event row; `(6, 13)` is a one-shot
impact.

The skill-line sweep (`scripts/db2_spell_sweep.py --skill-line 6 40 51 163 375`) reaches
only the player-facing spells. Three of the six slows are applied by HELPER spells that
sit on no skill line (the Crippling Poison proc debuff, Frost Armor's Chilled, Frost Trap
Aura), so those were joined by SpellID. The mechanic of each was confirmed from
`SpellEffect` (`EffectAura 33` = mod decrease speed) rather than assumed from the name.

## The six sources

The card named five. Frost Armor's chill (`frost_armor_movement_slow_aura`,
`combat_core/auto_attack.rs`) is a sixth `MovementSpeedSlow`, applied to melee
attackers.

| Source (ours) | Client spell | Aura-state `(7,8)` kit | Identity |
|---|---|---|---|
| Frostbolt | 116 → visual 13 | **3531** | Chill |
| Frost Armor chill | Chilled 6136 / 7321 → visual 675 | **3531** (+ one-shot `frostarmoreffect_impact_chest.m2`, kit 779) | Chill |
| Concussive Shot | 5116 → visual 3180 | **1564** | Daze |
| Frost Shock | 8056 → visual 144 | none | — |
| Crippling Poison proc | 3409 / 11201 → visual 19 | none (one-shot `poison_impact_chest.m2`, kit 3031) | — |
| Frost Trap zone | Frost Trap Aura 13810 → visual 3759 | none on bodies | the zone |

### Kit 3531 — the chill (Frostbolt, Frost Armor)

- Model `spells/ice_precast_uber_head.m2` at attachment 20 (Head), EffectName 54.
- `EffectType 1` → `SpellProceduralEffect` 356935: `Type 1`, `Value_0 = 3947775` =
  `0x3C3CFF`, read as RGB **(60, 60, 255)**, a saturated blue body tint. `Value_1/2 = 0`,
  `Value_3 = 3`. (The Type-1-is-tint reading is inferred from the value's shape. The
  enum is not in the table.)
- Improved Blizzard's Chilled (12484, not in our kit) uses a SIBLING kit, 2469: the same
  head model, with procedural 356903 = `0x6810FD` and 0.1 s fades. So the chill is one
  head model with per-source tints, not one fixed kit.

### Kit 1564 — the daze (Concussive Shot)

- Model `spells/sap_state_head.m2` at attachment 20 (Head), EffectName 542, plus a sound.
- Shared with the generic **Dazed** (1604). It is Sap's state model, the same over-head
  whirl family our Stun treatment draws (`hard_cc.rs`: "OVER THE HEAD… a hueless
  whirl").
- **Correction:** `2026-09-06-frost-shock-client-data.md` says "Concussive Shot has no
  state kit either". It has one: visual 3180 carries a `(7,8)` row to kit 1564.

### Kit 360 — the client's GENERIC slow state (found in round 2)

None of our six sources uses it, but it is the client's default "slowed" look. Kit 360
(`(7,8)` state) and its impact sibling kit 126 attach `spells/slow_impact_base.m2` (fdid
166898, EffectName 158) at **attachment 19 (Base, the feet)**. It is shared by Earthbind,
Piercing Howl, Improved Hamstring, Improved Wing Clip, Thunder Clap / Stomp's slow,
Entrapment, Counterattack, and a spell literally named "Slow".

It is a pure particle model (no vertices) with two emitters, both additive (blend 4),
looping the 1000 ms Stand sequence (the attach row sets no anim IDs):

| | Emitter 1: the ring | Emitter 0: motes |
|---|---|---|
| texture | `spells/shockwave1bgrey.blp` | `particles/tail_dust3.blp` |
| orientation | flag `0x20000` (read as XY-quad: flat on the ground) | billboard |
| live in each 1 s loop | 0-333 ms | 833-1000 ms |
| rate / life | 14 /s (~5 rings per pulse) / 0.395 s | 65.1 /s / 0.70 s |
| size over life | 0.26 -> 0.87 @0.65 -> **1.53** | 0.36 -> 0.31 -> 0.12 |
| alpha over life | 1 -> 1 @0.65 -> 0 | 0.09 -> 1 @0.65 -> 1 |
| speed | 0.056 | -0.95 (spread pi, from 0.39 above the base) |

Both colour tracks run yellow `(255, 222, 0)` -> violet `(189, 59, 249)` @0.65 ->
`(174, 89, 253)`. Both emitters carry flag `0x8`, read as world space, so a victim who
keeps moving leaves the ripples behind where they spawned. (The flag meanings are from
the community M2 docs, not the data itself.)

**What it means for the design.** The client already answers "a slow is a feet-level
state", with a pulsing ground ripple, about 1 Hz, that the moving victim walks out of.
It is the client's own vocabulary for exactly the generic case our one shared treatment
covers. Its hue is not usable as is: violet sits on top of our Shadow authority
`(148, 130, 201)`, so the school tint stays.

**Deliberate departure (2026-09-26, bench review):** our ring FOLLOWS the victim instead
of staying in world space. Previewed at the client's values, a world-space ring spawns
at the feet and is left behind within a fraction of its 0.4 s life at running speed, so
it read as detached from the unit rather than as a binding on it.

### No state: Frost Shock, Crippling Poison, Frost Trap

- **Frost Shock:** visual 144 has cast and impact kits only (confirms the 2026-09-06 doc).
- **Crippling Poison:** the proc debuff (3409 rank 1, 11201 rank 2) has a one-shot chest
  impact and no state. The skill-line spells 3420/3421 are the weapon-coating
  application (a precast kit only), not the debuff.
- **Frost Trap:** 13810 is a persistent area aura (`Effect 27`) applying `EffectAura 33`
  at −60 % directly. There is no separate slow spell and so no per-body visual. The only
  model is `spells/frosttrap_aura.m2` on the ground (attachment −1), which our zone decal
  already stands in for.

## Our slows, for scale

`magnitude` is the speed MULTIPLIER (0.3 = 70 % slow).

| Source | magnitude | duration |
|---|---|---|
| Frostbolt | 0.7 | 5 s |
| Frost Armor chill | 0.7 | 5 s |
| Frost Shock | 0.5 | 8 s |
| Concussive Shot | 0.5 | 4 s |
| Frost Trap zone | 0.4 (`FROST_TRAP_SLOW_MAGNITUDE`) | while inside |
| Crippling Poison | 0.3 | 8 s |

## Decision (2026-09-26): movement-layer treatment, off the channel budget

The client has two sustained identities (chill, daze) and leaves three slows invisible.
Copying it faithfully fails on two counts:

1. **The daze would read as a stun.** Our spatial grammar puts Stun OVER THE HEAD as a
   whirl, and `sap_state_head` is exactly that. A dazed target that is still running
   would look incapacitated, which misreads the matchup this card exists to make legible.
2. **The chill tint swaps the body material.** That puts it in contention for
   `OriginalBodyMaterial` with Fear and Polymorph (`shared-restore-slot-mutual-exclusion.md`),
   and Frostbolt → Fear is a routine sequence. It is also a sustained on-victim channel,
   which `sustained-visual-channel-budget` weighs against.

Chosen instead: **one treatment keyed on `AuraType::MovementSpeedSlow`, drawn at the
feet, only while the victim moves**, grounded in the client's generic slow state (kit 360
above), and tinted by the slow's school (Frost ice-blue,
Physical hueless, Nature/poison green). It extends the existing grammar (Root at the
feet, Stun over the head, so a slow reads as a partial root at the feet). It exists
only while the unit is moving, which is the only time a slow matters. It never touches
the body material. And any future slow gets it for free. It does NOT claim a sustained
channel. A stationary slowed unit shows nothing, and its aura icon carries the debuff,
the same as a stationary Root-free unit.

What the client data still contributes: the chill is **blue** (`0x3C3CFF`) and shared by
Frostbolt and Frost Armor; the daze is **hueless**; the poison has no state colour of its
own, so green comes from our Nature school tint.

Already free: `advance_gait` (`rendering/effects/gait.rs`) paces the walk bob by
DISTANCE, so a slowed unit already bobs at a proportionally lower cadence. That signal
is too weak to carry the state on its own, but the treatment must not fight it.

## Build notes

- **The tint cannot be read off `Aura::spell_school`.** That field is the school "for
  DAMAGE purposes", and `None` covers both physical and schoolless, so the Frost Trap
  zone slow would come out hueless. Route the tint through one function keyed by the
  aura's `ability_name`, the way `DotStateVisual::for_dot` routes DoTs, so that every
  source is an explicit match arm and a new slow is a compile-time question.
- `tests/lands_silently_audit.rs:465-469` names Frostbolt, Concussive Shot, Frost Shock
  and Crippling Poison as known-silent against "AS-133 (slows)". The build clears those
  entries, and the Frost Trap row's "slowed bodies unmarked" note with them.
- `aura_band.rs` routes `MovementSpeedSlow` / `AttackSpeedSlow` here as named deferrals
  (AS-134). The Crippling Poison PROC moment (the roll landing) is also this card's, per
  AS-134's review. The client gives it a one-shot `poison_impact_chest.m2`, an apply-tier
  flash that is cheap and off budget.
- Graphical-only, like `hard_cc.rs`: no `game_rng` draw and no sim write, so headless
  stays byte-identical by construction.
