# AS-136: Hunter ranged shots — Client-Data Research

All findings are from WoW Classic Era build **1.15.9.69547** client data (wago.tools DB2
CSVs + CASC M2 fetches). No prose/wiki sources were used as evidence.

## Method notes

Skill-line inventory via `scripts/db2_spell_sweep.py --skill-line 163 51 50 --events`
(Marksmanship, Survival, Beast Mastery). Missiles: `SpellVisual.SpellVisualMissileSetID →
SpellVisualMissile → SpellVisualEffectName.ModelFileDataID`. Impacts: `(6,13)` event kits,
`SpellVisualKitEffect.EffectType == 2 → SpellVisualKitModelAttach → SpellVisualEffectName`.
M2s parsed at the v274 layout (particle stride 492 at offset 296, ribbon stride 176 at 288,
sequence stride 64 at 28), textures from the `TXID` chunk, names from the community listfile.
Auto Shot's chain is AS-132's (`2026-09-20-as132-hit-reaction-client-data.md` §2.2).

## The join

| Shot (ours) | SpellVisual | Missile model | Impact kit → model | Ours today |
|---|---|---|---|---|
| Aimed Shot (all 6 ranks) | **3180** | `spells/arcaneshot_missile.m2` (165592), sound 3014 | 419 → `spells/magic_impact_chest.m2` (166525), Chest | arrow cuboid, gold tint |
| Concussive Shot | **3180** (the same row) | the same | 419 | arrow cuboid, tan |
| Arcane Shot (all 8 ranks) | 3299 | `spells/arcaneshot_missile.m2`, sound 3013 | 419 | arrow cuboid, gold |
| Serpent Sting (all 9 ranks) | 3179 | `spells/poisonshot_missile.m2` (166648) | 276 → `spells/bestowdisease_impact_chest.m2` (165679), Chest | arrow cuboid, green |
| Auto Shot (bow, IRDI 367) | 5 | none: the missile set is sound-only, and the arrow comes from the ammo item, `item/objectcomponents/ammo/arrowflight_01.m2` (137240) | 1947: wound anim + sound, no model | cosmetic arrow that times out in the air |

All missiles fly from attachment −1 to **34 (Chest)**, with `BaseMissileSpeed 0` (a client
constant, so our RON `projectile_speed` stays the sim's authority). None of the four
shots has a caster kit.

Negative findings:
- **There is no distinctive Aimed Shot missile.** Aimed, Arcane and Concussive fire the
  identical missile and land with the identical impact; only the sound differs.
- **Arcane Shot is not green.** It is blue-violet. The green shot is Serpent Sting.
- **None of the special shots is an arrow.** Only Auto Shot is, so the client's own
  grammar is: an arrow is an auto attack, and a glowing shot is an ability.

## MODEL: `arcaneshot_missile.m2` — the shared Hunter shot

Seven vertices in billboard glow planes, extent x −0.11..0.21, y ±0.31, z ±0.27 (a core
**about 0.6 yd across**). Two additive materials (blend 4) over `particles/genericglow5.blp`
and `item/objectcomponents/weapon/flare.blp`, plus `spells/purple_glow.blp`.

- **Ribbon:** white, alpha 0.25 (8192/32767), 0.056 above and below (0.11 yd tall), 50
  edges/s, 0.2 s edge life. The same ribbon as the plain arrow.
- **Particles** (all additive, looping a 167 ms Stand, always on in flight):

| | e0 glow puffs | e1 / e2 sparks | e3 core stream |
|---|---|---|---|
| texture | genericglow5 | flare | flare |
| rate / life | 6.7/s / 0.75 s | 20/s + 10/s / 1.1 s | 100/s / 0.5 s |
| speed | 0.056 | 0.33 | 1.11 |
| colour 0 → 0.5 → 1 | (22,34,168) → (78,33,201) → (203,103,255) | (77,15,207) → (114,59,190) → (165,222,255) | (0,0,154) → (97,67,207) → (245,245,236) |
| alpha | 0.59 → 0.98 → 0 | 1 → 1 → 0 | 0.39 → 1 → 0.2 |
| size | 0.14 → 0.26 → 0.14 | 0.083 → 0.028 → 0.014 | 0.19 → 0.14 → 0.03 |

Read: a small, bright indigo-violet core whose sparks cool to lilac and ice-white as they
fall behind, with a thin white streak.

## MODEL: `poisonshot_missile.m2` — Serpent Sting

**The same template as the Arcane Shot missile**: the same ribbon, the same four emitters,
the same rates, lives and speeds. It differs in three ways:
- recoloured yellow-green, with a model colour of (0.63, 1.0, 0.11);
- the sparks (e1/e2) have **gravity 1.39**, so they drip;
- the sparks are smaller (0.056 at birth instead of 0.083).

Colours: puffs (119,201,2) → (141,207,12) → (209,234,92); sparks green → (230,255,92);
core stream (158,255,64) → (109,201,0) → (91,169,0).

## MODEL: `magic_impact_chest.m2` — the shared landing

One 1600 ms sequence, six additive emitters, almost all of it in the first 433 ms:

| emitter | texture | window | what |
|---|---|---|---|
| e0 | cyanstarflash | from 133 ms, 60/s | fast streaks (speed 6.1, tail 6.55), violet → white |
| e1 | shockwave10d | 0–300 ms | expanding ring 0.22 → 0.84 → 1.37 |
| e2 | blue_glow2 | 0–300 ms | sparks, speed 5.3, **gravity 6.67** (they fall) |
| e3 | toonsmoke16 | 0–200 ms | puff 0.4 → 0.83, blue → grey → violet |
| e4 | star5a | 0–433 ms | stars from a sphere emitter |
| e5 | aurarune7 | 0–200 ms | expanding rune 0.2 → 0.93 → 1.76 |

The ring and rune colour tracks start green (37,249,82) and pass through violet
(117,0,226) to white. The green holds only for the first instant; the landing reads
violet-white.

## MODEL: `bestowdisease_impact_chest.m2` — Serpent Sting's landing

A 3000 ms sequence that **lingers**, unlike the magic impact:
- e0 `stargradient64`, additive, 3 s at 10/s, life 1.5 s, a 0.28 × 1.1 yd column: glints
  dark green → yellow-green → yellow, swelling to 0.56 and back.
- e1 `clouds8x8fade`, **alpha-blended** (blend 2), 3 s at 20/s, life 2.5 s, rising at
  0.28: sickly smoke (18,55,0) → (106,146,0) → (163,196,64), growing 0.22 → 0.69.
- e2 `flare`, alpha-blended, 0–167 ms at 150/s, speed 3.3: a dark-green spray.

## MODEL: `arrowflight_01.m2` — Auto Shot's arrow

38 vertices, **1.52 yd long** (x −0.03..1.49), 0.14 yd across: a thin opaque textured
shaft (material blend 0) with the same thin white ribbon. No particles, and no impact
model (the bow's impact kit 1947 is a wound animation and a sound).

## Decision (2026-09-26): client-faithful

The user chose the client's grammar as is:
- Aimed, Arcane and Concussive share one glowing violet shot and one violet-white landing.
- Serpent Sting gets the green dripping variant and the lingering green cloud.
- Auto Shot stays the only arrow.

The three shared shots stay distinguishable by other channels. Aimed Shot has its 2.5 s
cast and the heal-refused tell (`mortal_wounds.rs`), and Concussive Shot has the slow ring
(AS-155).

**Colour-budget check.** The shot's core (78,33,201) is close in hue to our Shadow Bolt
halo (98,63,188, `spell_bolts.rs`). They separate on MATERIAL, not hue: Shadow Bolt is an
opaque dark core in a violet halo, and this shot is an additive bright core that cools to
lilac and white. The bench shows them side by side so that separation is judged by eye,
not asserted.

Consequences for the build:
- The four shots stop routing through the stock school-impact rows. Aimed Shot and
  Concussive Shot are Physical and Arcane Shot is Arcane in our RON, so the stock rows
  would give them three different landings where the client has one. They need a
  `landing_style` override, like Mana Burn's.
- The RON `projectile_visuals` tints on these four stop being the colour authority.

## Bench sign-off (2026-09-26)

Tuned in the Hunter Shot Bench and signed off by the user at real-time speed.

**Missiles use the client's values:** core 0.6 yd, rate and size scale 1.0, and the white
ribbon at alpha 0.25, 0.11 yd tall, with a 0.2 s life.

**The shared landing (`magic_impact_chest`) is cut to three of its six layers:**
- **smoke** (e3);
- **stars** (e4);
- **rune** (e5).

The streaks (e0), the ring (e1) and the falling sparks (e2) are dropped. At the client's
raw values the user judged the landing "way too exaggerated", too long, and "a lot of
layering". The remaining three run at:
- size ×0.8;
- spread ×0.6 (particle speed, and therefore how far the stars throw);
- duration ×1.0 (emission windows 0–200 / 0–433 / 0–200 ms).

(The streaks were also the layer with the least certain window. They have no `enabledIn`
track, and a literal reading ran them to the 1.6 s sequence end. Dropping them makes that
moot.)

**Serpent Sting's cloud (`bestowdisease_impact_chest`) is kept at the client's values**,
lingering 3.0 s.
