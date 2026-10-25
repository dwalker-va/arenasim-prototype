# Ice Barrier — client data and the gap (AS-358)

Research for AS-358: what the Classic and TBC clients draw for Ice Barrier, and how
that differs from the `ShieldBubble` sphere we drew for it. The signed-off bench is
[`benches/2026-10-17-ice-barrier-bench.html`](benches/2026-10-17-ice-barrier-bench.html)
(published at https://claude.ai/artifact/6StjLsouEtbtmBHqjR2fHP, blessed version
`1792216804-7aa7`), and §8 records what shipped.
The method and the shape follow AS-344's
[`2026-10-16-pws-client-data.md`](2026-10-16-pws-client-data.md). The two models share a
skeleton, so this note leans on that one and records only what differs.

Sources: wago.tools DB2 tables and CASC files for Classic Era `1.15.9.69547` and TBC
`2.5.6.69795`. The join is `scripts/db2_spell_sweep.py` over skill line **6 Frost**,
followed by a per-kit walk and an M2 parse with the AS-344 parser (MD21 v274 strides from
project memory `wow-client-data-recipe`). The textures were decoded from BLP to measure
them. No client art is committed or shipped.

## 1. The join

`db2_spell_sweep.py --skill-line 6`:

| build | inventory | no visual | with visual | property 4 |
|---|---|---|---|---|
| Era 1.15.9 | 25 | 13 | 12 | HELD |
| TBC 2.5.6 | 32 | 17 | 15 | **SPLIT on Blizzard only** |

The TBC split is Blizzard: visual 9490 for the player ranks and 9487 for the 42198–42213
helper spells. It does not touch Ice Barrier. Run with `--name "Ice Barrier"`, TBC
holds. Every rank of Ice Barrier resolves to **SpellVisual 4302**: ranks 1–4 on Era
(11426, 13031–13033) and 1–6 on TBC (adding 27134 and 33405). Rank collapse is intact.
Visual 4302 has no missile and no impact kit:

| event | kit | what it is | Era | TBC |
|---|---|---|---|---|
| (1,2) precast, caster | 199 | anim 52 + `ice_precast_high_base.m2` (166378) at the feet + `ice_precast_uber_hand.m2` (166384) on both hands | same | same |
| (3,13) cast, caster | 207 | anim 54 (`SpellCastOmni`) + sound 2559 + `ice_precast_high_hand.m2` (**166379**) on both spell hands (attach 21/22) | same | same |
| (7,8) aura state | 3672 | `iceshield_state.m2` (**166392**) at **attach 19, Base (the feet)**, scale 1, plus sound 6616 | target types 1 and 2 | target type 2 only |

- **The precast row never plays.** Ice Barrier is an instant.
- **Era also fires the state kit on target type 1**, as with PW:S. TBC dropped it. The
  spell is self-only, so both rows land on the Mage either way.
- **Kit 207 is Ice Barrier's alone** (the reverse join finds only it and Season of
  Discovery's Greater Ice Barrier). Frostbolt, Frost Armor and Ice Block use
  `ice_precast_low_hand.m2` (166381) on one hand. Ice Barrier is the Mage's two-hand,
  "high" cast.
- **Kit 3672 is Ice Barrier's alone** in the era inventory. The model is also reused by
  SoD's Greater Ice Barrier and Freezing Field.

**TBC does not differ from Era** in anything the player sees. The state model's tracks,
geometry, textures and emitter are identical. Only one sequence-flag bit and the bounding
box's lower z differ (−1.29 vs −1.17). The cast-hand model differs only in its embedded
name string and bounding radius.

## 2. The state model: `iceshield_state.m2` (fdid 166392)

### The cards are PW:S's skeleton

Submeshes 0–2 are **exactly** `holydivineshield_state_base.m2`'s three quads:

- the rim, 3.18 × 4.08 with its feet at −0.91 … +3.17;
- the two streak rings, 2.26 × 2.90 at −0.12 … +2.78.

They sit on the same cylindrical-billboard bone 0 (flags `0x240`) and the same three
scale bones, pivoting at z = 1.11. The bone-scale tracks are identical key for key: the
birth pop, the jelly wobble and the death swell to 1.271 × 1.158. The textures are the
same three, `genericglow2b1`, `shockwave10b` and `shockwave10d`. Re-measured here, their
radial profiles match AS-344's. Every material is `flags 19, blend 4`: unlit, unfogged, no
depth write, additive.

So the geometry, rig scale (×1.229), clearance and draw-order findings in the PW:S note
§2 and §5 hold verbatim. On our rig the rim's inner edge is 0.98 × 1.25 yd from the
centre, and it clears the 0.5 body radius by 0.38 yd at the narrowest wobble.

### What differs: colour and duty

| layer | texture | colour | birth (700 ms) | loop (633 ms) | death (1100 ms) |
|---|---|---|---|---|---|
| **Rim** | genericglow2b1 | `(0.584, 0.82, 1.0)` | 0 → **1.0** at 300, 0.24, 0.30 | 0.30 → 0.12 → 0.30 | 0.30 → 0 by 1000 |
| **Streak A** | shockwave10b | `(0.514, 0.741, 1.0)` | 0 → 1.0 at 300, then 0.6 ↔ 1.0 | **0.6 ↔ 1.0** every ~100 ms | flickers to 900, 0 by 1000 |
| **Streak B** | shockwave10d | `(0.478, 0.733, 0.992)` | as A | 0 ↔ 0.24 ↔ 0.4, anti-phase | flickers to 900, 0 |
| **Icicles** | ice3b_b | white × texture | 0.6 (pops by scale) | 0.6 | 0.6 → 0 by 834 |
| **Icicle glint** | ice3b_b, sphere-mapped | white × texture | 0.1 | 0.1 | 0.1 → 0 by 167 |

PW:S holds its rim at 0.06–0.15 and both streaks at 0–0.2. **Ice Barrier holds its rim
twice as bright and streak A five times as bright (never below 0.6).** PW:S's loud moment
is its arrival, and its held state is a faint shimmer. Ice Barrier stays a bright,
flickering blue ring for its whole life.

### Icicles (submeshes 3–5, 497 vertices, bone 3)

The icicles are flat, like the cards: x spans only 0.11–0.27. They lie on the billboard
plane and scale with the cards. There are five clusters of 3–5 downward-pointing
icicles, sitting just inside the rim's inner edge:

| cluster | y (client) | crown z | tips (y, z) |
|---|---|---|---|
| upper left (two groups) | −0.69 … −0.02 | 1.88 – 2.12 | (−0.63, 1.52) (−0.42, 1.70) (−0.15, 1.71) |
| upper right | 0.38 … 0.71 | 1.84 – 1.95 | (0.52, 1.55) (0.67, 1.62) |
| lower left | −0.69 … −0.42 | 0.49 – 0.64 | (−0.48, 0.27) (−0.66, 0.37) |
| lower right | 0.14 … 0.61 | 0.27 – 0.54 | (0.21, 0.05) (0.375, 0.15) (0.50, 0.32) |

So the icicles frame the body at the shoulders and the knees, and their tips cross the
body's silhouette. Each submesh is drawn twice. Shader 16 is a plain UV pass at weight
0.6. Shader 144 (`0x80`: env-mapped texture coordinates) is a sphere-mapped glint at
weight 0.1. The `ice3b_b` texture is 64², pale-to-mid blue (mean `(95, 160, 245)`, max
`(208, 232, 248)`, mean alpha 0.62) with diagonal frost streaks.

### Frost mist (particle 0, bone 4)

| field | value |
|---|---|
| texture | `toonsmoke16.blp`, an 8 × 8 flipbook of a smoke puff (not random-frame) |
| emitter | plane, 1.389 × 0.075, at z = **2.541** (0.5 above the head), child of the billboard bone |
| motion | speed −1.667 (down) ± 20%, within a 0.227 rad (13°) cone, no gravity, drag 0.05 |
| life, rate | 1.8 s, 11.7 /s, so about 21 alive |
| colour | `(81, 172, 243)` → `(146, 208, 255)` → white |
| alpha | 0 → **0.39** at mid-life → 0 |
| size | 0.278 → 0.417 → 0.556 client units |
| enabled | from the cast (no 200 ms delay); off at 767 ms into death |

That is a soft frost veil that drifts down the body from above the head and is brightest
at chest height. When the barrier ends, its last puffs keep falling for up to 1.8 s after
the cards are gone. The mist is the only part of the model that is not on the billboard
plane, and the only part that outlives the aura.

### Sequences

They have the same three roles as PW:S: birth (id 0, 700 ms), loop (id 158, 633 ms) and
death (id 159, 1100 ms). The client has **one ending**. Sequence 159 plays whether the
barrier expired or was broken. The swell, the rim fade, the icicles fading in place and
the mist trailing off are the same in both cases.

## 3. The cast: `ice_precast_high_hand.m2` (fdid 166379)

It is a 1000 ms one-shot on both spell hands, drawn with the `SpellCastOmni` animation:

- a **deep-blue glow card**, 0.76 client units across (`blue_glow2`, colour
  `(0.106, 0.42, 1.0)`, weight 1.0);
- a **ring**, 0.6 across (`shockwave10`, `(0.157, 0.357, 1.0)`, weight 0.75), both
  pulsing 1.139 ↔ 0.957 on a 533 ms global loop;
- four tiny sphere emitters: cyan star flashes, a grey glow ball, lightning sparks and
  ribbon blur, 0.03–0.06 units each and 0.15–0.5 s lives.

No Mage spell draws hands today. A hard cast gets the generic casting orb, and an instant
gets nothing. `HealCastKind` routes only Holy, Nature and Arcane.

## 4. What we draw today

`update_shield_bubbles`, the Frost arm. It is a `Sphere::new(1.0)` scaled 0.9 × 1.4 × 0.9
at the capsule centre, with base `(0.4, 0.7, 1.0, 0.25)`, emissive `(0.4, 1.0, 2.0)` and
`AlphaMode::Add`. It spawns on the frame the aura lands and despawns on the frame it goes.
The caster side is nothing. `try_ice_barrier` applies the aura inline with no
`CastingState` and spawns no `InstantAbilityFired`.

| | client | ours |
|---|---|---|
| form | hollow billboard ring + icicles at shoulders and knees + falling mist | solid additive ellipsoid washing the whole body blue |
| size | rim inner edge 0.98 × 1.25 yd | 0.9 × 1.4 semi-axes. **About right.** |
| held intensity | bright: rim 0.12–0.30, streaks 0.6–1.0, flickering at ~10 Hz | constant, bright, static |
| motion | streak flicker, jelly wobble, falling mist | none |
| arrival | 200 ms delay, a tall full-strength pop that settles in 0.5 s; the mist starts at once | appears on one frame |
| ending | swell 1.27× and fade over 1.1 s, icicles fade by 0.83 s, mist trails 1.8 s | vanishes on one frame |
| caster | two-hand deep-blue flare, 1 s | nothing |
| colour | `(0.48–0.58, 0.73–0.82, 1.0)` | `(0.4, 0.7, 1.0)`: **the same hue** |

## 5. The gap

1. **Shape.** We draw a filled egg. The client draws a hollow ring with icicles: the
   icicles are what makes it read as *ice*, not just "blue shield". This is the identity
   gap, as with PW:S.
2. **Lifecycle.** Ours has no arrival and no departure. A break, an expiry and "never
   shielded" all look like the frame the sphere vanishes.
3. **Caster.** The Mage's hands do nothing. The client's answer is a dedicated two-hand
   flare (kit 207), which would be the Mage's first hand effect.
4. **Duty: here the client is louder than PW:S, not quieter.** Faithful Ice Barrier is a
   bright, flickering ring plus a mist column for up to 60 s (our config matches the
   client's 1 minute). It is the Mage's only sustained self-visual (Frost Armor draws
   nothing), so it does not crowd another channel on the same unit. But it is a strong
   channel. The bench's brightness and mist knobs exist so the user can trim the amount
   while keeping the identity (memory `client-values-are-a-ceiling-not-a-target`).

## 6. Colour budget

Project memory `color-budget-principle` reserves FCT yellow (damage), green (heals) and
**light blue (absorbs)**. `ui::theme::fct::ABSORB` is `(100, 180, 255)`, hue about 209°.
The client barrier is in the same hue: the rim `(149, 209, 255)` is about 206°, and the
streaks about 212°.

That match is congruent, not a collision:

- **It is not a new spend.** The Frost world palette already uses this exact colour. Frost
  Nova's edge is `(0.39, 0.71, 1.00)`, which equals `fct::ABSORB`, and today's Ice
  Barrier sphere is `(0.4, 0.7, 1.0)`. Replacing the sphere with the client's ring adds no
  hue to the screen.
- **The meaning agrees.** The reserved blue means "absorbed". The "N absorbed" numbers
  that rise off a Mage under Ice Barrier are that barrier's own, so a blue number off a
  blue barrier says the same thing twice. PW:S, the other absorb, is gold.
- **They barely overlap in space.** FCT spawns at `FCT_HEIGHT` 4.0 above the capsule
  centre, about 5.0 yd above the feet, and rises. The rim's top edge peaks at about
  4.2 yd at its tallest wobble, and the mist spawns at 3.1 yd. The text starts clear of
  the effect and is drawn in screen space with a black outline.

One readability risk is not about FCT: **the Mage's own body is light blue**
(`(0.41, 0.80, 0.94)`). Additive pale-blue icicles over a light-blue capsule lose contrast
where they cross the body edge, and the bench shows this. The rim and streaks clear the
body, so the ring itself stays legible. Pushing the icicles away from blue toward white is
a possible trim, but it would depart from the client. The bench leaves it as is.

## 7. Where the design pressed

These points were open while the bench was built. §8 records how each one was settled.

- **The mist is world-space trailing particles.** Emitter flag `0x80` (model space) is
  not set. The mist is also the one piece that outlives the aura.
- **Coexistence with PW:S.** A Mage can wear both absorbs (`auras.rs` allows different
  absorbs together). The client then draws two near-identical ring sets, gold and blue,
  on one unit.
- **The `ShieldBubble` sphere loses its last user.** Power Word: Shield (AS-344),
  Divine Shield (AS-359) and Ice Barrier each wear their own client model. They are the
  only absorbs and the only immunity in the book.

## 8. What shipped

The user signed off on the bench at its defaults: artifact
`6StjLsouEtbtmBHqjR2fHP`, version `1792216804-7aa7`.

- **Layers.** Every layer is kept at the client's values: the rim, both streak rings,
  the icicles, the glint, the frost mist, the birth pop and the death swell.
- **Ending.** A break, an expiry, a purge and the bearer's death all play the client's
  single swell-and-fade. The mist stops spawning 767 ms into it, and the puffs already
  falling outlive the cards and then clear.
- **One rig, three kinds.** Ice Barrier is the third `ShieldKind` on the shield rig in
  `rendering/effects/shield.rs`, beside Power Word: Shield and Divine Shield. It
  shares their lifecycle, billboard, `VisualBody` anchor and draw-order proof (re-run
  here for all five cards).
  - Its knobs are `IB_TUNING`, plus its own weight tracks and colours. The `IB_*`
    constants carry the bench's numeric knobs under the same names, every multiplier
    at 1.0.
  - The layer toggles have no constants, as on AS-344 and AS-359, because every layer
    is kept. The bench's break shatter was declined, so it was not built.
- **Ice Barrier's own pieces** are in `rendering/effects/ice_barrier.rs`.
  - The icicles and the glint are flat meshes on a card centred at the streak cards'
    centre. Their colour is in the vertices (`ice3b_b`'s measured light and deep blue).
  - The glint's sheen turns with the camera's yaw about the Mage. That is the bench's
    stand-in for the client's sphere map.
  - The mist is world-space `ClientParticle`s, rolling as they fall, on four
    procedural puff shapes.
- **Textures.** The streak and rim cards wear the same generated textures as PW:S. They
  are the same client files, measured once by AS-344. This card's re-measurement, which
  the bench drew, differs by at most 0.03 in alpha at any radius. Ice Barrier's streak
  A is the `shockwave10b` ring and its B the `10d`, the other way round from PW:S's.
- **Caster.** The Mage AI spawns `InstantAbilityFired` for Ice Barrier. A new
  launch-only `HealCastKind::Frost` plays kit 207 on both hands for 1 s:
  - the deep-blue glow card and ring, pulsing on the 533 ms loop;
  - four sparks per hand.

  This is the Mage's first hand effect. Its other casts are unchanged.
- **The sphere is retired.** `shield_bubbles.rs` and its `ShieldBubble` component are
  gone, and the aura router's owner for absorbs and immunities is now `Shields`. A
  future absorb has no fallback: `lands_silently_audit` fails until it is given a
  visual.
- **Colour.** Client blue as measured, and the icicles keep their client colours. §6
  explains why the floating-combat-text overlap is congruent rather than a new spend.
- **No client art ships.** Every texture is generated from the measured profiles and
  positions.
