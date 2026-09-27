# AS-130: Shaman totems, Purge and Frost Shock — Client-Data Research

All findings are from WoW Classic Era build **1.15.9.69547** client data (wago.tools DB2
CSVs + CASC M2 fetches), except one join that is server data: the creature -> display
link, read from Wowhead's model viewer (see Method). Scope after AS-128's reconciliation:
Air, Earth and Fire totems, Purge, and Frost Shock's own identity. Wind Shear went to
AS-137, and Frost Shock's slow state to AS-133.

## Method

- **A totem's summon spell draws nothing** (established for Healing Stream in
  `2026-09-07-healing-stream-totem-client-data.md`: visual 319 has a sound and no model).
  A totem's look is the summoned CREATURE. So the chain is:
  `SpellEffect` (Effect 28 SUMMON, `EffectMiscValue_0` = creature id, `EffectMiscValue_1`
  = SummonProperties slot) -> creature -> `CreatureDisplayInfo` -> `CreatureModelData.FileDataID`.
- **The creature -> display link is server data.** This build's `Creature.db2` is empty.
  It was read from the `displayId` in Wowhead's Classic model viewer
  (`wowhead.com/classic/npc=<id>`), and cross-checked against the client: every display
  it names exists in `CreatureDisplayInfo` and resolves to a model in this build.
- M2s parsed at the v274 layout: bones at stride 88 from offset 44, particles at 492 from
  296, ribbons at 176 from 288, sequences at 64 from 28. The ribbons' orbit is read from
  their parent bones' rotation tracks.

## Result 1: one model per ELEMENT, shared by every totem of that element

| Element (slot) | Client totems -> creature | Display | Model (fdid) | Ours |
|---|---|---|---|---|
| Air (83) | Windfury 8512 -> 6112, Grace of Air 8835 -> 7486 | **4590** | `creature/spells/airelementaltotem.m2` (125990) | Windfury (AirTotem) |
| Earth (81) | Strength of Earth 8075 -> 5874, Stoneskin 8071 -> 5873 | **4588** | `creature/spells/earthelementaltotem.m2` (126009) | Strength of Earth (EarthTotem) |
| Fire (63) | Flametongue 8227 -> 5950 | **4589** | `creature/spells/fireelementaltotem.m2` (126012) | Flametongue (FireTotem) |
| Water (82) | Healing Stream 5394 -> 3527 | **4587** | `creature/spells/waterelementaltotem.m2` (126041) | Healing Stream (WaterTotem) |

Displays 4587-4590 are consecutive and all at scale 1.0: a set built together. The
build's other totem models (`healingtotem`, `manatotem`, `sentrytotem`, `stasistotem`,
`serpent_totem`, `firelighttotem`) are one-offs, and the race-specific totems in the
listfile (Draenei, Dwarven, Orc, Troll) are post-Classic and absent from this build's
`CreatureModelData`.

**Zero marginal cost holds.** A fifth totem in an element costs nothing new; it reuses
that element's model.

## Result 2: the four element models are ONE template

All four share:
- **the identical 267-vertex mesh**: a carved post, x -0.31..0.33, y -0.62..0.67,
  z 0.16..**3.41**, so about 3.25 yd tall and 1.3 yd across at its widest (bones 11-15
  put a crown or wings at z 2.5-2.84, spread y +/-0.3..0.4);
- the same five materials: one opaque textured body over `fireplacetex01`, plus
  additive glow layers over `demonrune1` and glow textures;
- the same bone rig and the same emitter structure.

They differ by texture and colour only:

| | Ribbon textures, colour | Top flame (e0) ramp | Glow textures |
|---|---|---|---|
| Air | `ribbonmagic2`, **white**, 2 ribbons | (49,244,237) -> (116,37,235) -> (7,250,219) | blue_glow2, genericglow2_32 |
| Earth | `ribbonne1`, white, 2 | (237,244,49) -> (154,244,37) -> (16,202,7) | green_glow2/3 |
| Fire | `ribbonblur1d`, **red-orange (0.97,0.24,0.02)**, **3 ribbons** | (233,53,5) -> (247,117,12) -> white; flamelick is larger (0.16 -> 0.30) | red_glow2, yellow_glow3 |
| Water | `ribbonmagic2`, white, 2 | cyan, plus a `waterblobs1` emitter | genericglow64 |

The display scale is 1.0. A creature's own scale is server-side and was not recovered,
so the in-world height is **not** established by this data. Choose it at the bench.

## Result 3: what a totem DOES (the lifecycle)

Sequences: **127 = birth, 1867 ms**; **0 = stand, 1500 ms, looping**; **1 = death,
1200 ms**. Fire and Water add three 734 ms wound/idle variants.

- **Birth (1.87 s):** the root bones (0 and 4) carry 3 translation and 8 rotation keys:
  the totem rises and settles.
- **Standing:**
  - **the ribbons ORBIT the post.** They hang off bones 5 and 6 (pivot z 1.61), whose
    rotation runs on a 1000 ms global loop (9 keys), so they circle at mid-height,
    radius about 0.29, **one revolution a second**. Ribbon half-height is 0.14 (Air,
    Water), 0.19 (Earth) or 0.22 (Fire), with an edge life of 0.51-0.60 s: a swirling
    ring of trails.
  - **A small flame burns at the top** (e0 at z 3.0-3.08, `flamelicksmall(magicblue)`,
    35/s, life 0.62 s, speed 0.91, area 0.14, size 0.08 -> 0.14 -> 0).
- **Death (1.2 s):** the top flame goes out (enabled 0-100 ms only). The root bone tilts
  (9 rotation keys) and the base bone rotates.
  - A **dust burst** at the base plays 33-433 ms: `dust5a`, 94.6/s, speed 2.78, life 0.5,
    area 0.22, size 0.26 -> 0.13 -> 0.06, in the element's colour.
  - An **orange-to-grey smoke puff** plays 266-1166 ms: `toonsmoke16_2`,
    **alpha-blended**, 32/s, speed 1.67, life 2.0, (255,104,22) -> (126,126,126) ->
    (174,174,174), growing 0.33 -> 0.45 -> 0.69. It is identical across the elements.

Today's totem shrinks over its final 1.2 s, the same length as the client's death
sequence.

## Purge

Spell 370 / 8012 -> visual 214:
- caster side: the Nature precast/cast hands (kits 345/181);
- impact: kit 357, `spells/purge_new_impact_chest.m2` (166666) at the Chest, 1600 ms.

The impact is **the same template as `magic_impact_chest`** (the Hunter shot landing,
AS-136): streaks, a shockwave ring, smoke, stars, a rune. It is recoloured violet and
green, plus `tail_dust3` motes falling (gravity 3.3) at 400-1300 ms. Dispel Magic, for
comparison, lands `dispel_low_base.m2` at the feet.

## Frost Shock

Re-confirmed from `2026-09-06-frost-shock-client-data.md`: visual 144's impact kit 214 is
`ice_impactdd_med_chest.m2`, **the same model as Frostbolt's landing**. Precast and cast
are the ice hand kits (196/204). It has no state kit.

## Decisions (2026-09-27, the user)

1. **Budget ruling: a standing totem is a WORLD OBJECT with its own tier.** The sustained
   on-victim channel budget (`sustained-visual-channel-budget`) does not bind it, because
   it stands apart from the bodies that budget protects. So it MAY carry standing motion
   (the orbiting ribbons, the top flame). It stays bound by the colour budget, and the
   bench judges it with several totems up at once.
2. **The totem look is the client template:**
   - a carved post, the orbiting ribbons, the top flame;
   - a birth rise;
   - a death tilt with the dust burst and the smoke puff, replacing the shrink.

   One template for all four elements, tinted per element. **The floating orb goes.**
   The faint radius disc STAYS: it is gameplay information the client does not draw.
3. **Colours: our element authorities stay** (`TotemElement::color`: brown, pale sky,
   orange-red, blue). The client's yellow-green Earth would alias Nature healing, and its
   violet-blue Air would alias Arcane and Shadow.
4. **Purge keeps the shared dispel ribbon: looked, and the house vocabulary wins.** The
   client's landing is a hit-like chest burst. The dispel ribbon was deliberately built
   without one, because a flash-and-band at the chest made a dispel read as a hit
   (`rendering/effects/dispel_ribbon.rs` header).
5. **Frost Shock: looked, generic is fine.** The stock Frost row IS the client's landing.
   The 0-cast orb stays suppressed on purpose. **No change.**

## Bench sign-off (2026-09-27)

In the Totem Bench, the user signed off the shape and effects at the bench defaults and
set the SCALE from an in-game reference screenshot: a Troll Shaman with its four totems.

**Height is 1.0 yd, about waist-high on a player.** In the reference, each totem stands
at roughly half the Shaman's height, and our players are 1.9 yd capsules. This also
reconciles with the data: the 3.25 yd mesh times a creature scale of about 0.3 (the
server-side value the client data could not give us) is about 1.0 yd.

The reference also shows **the front rune is a CIRCULAR glyph plate**, glowing in the
element colour, with a matching element flame on top and the winged crown. The bench
drew a diamond as a placeholder.

Signed-off values (the bench's Rust block):
- **Post:** height 1.0 yd; rune glow 0.8; radius disc alpha 0.08 at the real
  `TOTEM_RADIUS` (20 yd).
- **Birth and death:** birth 1.867 s (rise with a settling wobble); death 1.2 s (a tilt
  of 18 degrees).
- **Ribbons:**
  - orbit radius 0.29, at 47% of the post's height, 1.0 rev/s;
  - per element [Air, Earth, Fire, Water]: count [2, 2, 3, 2], half-height
    [0.139, 0.194, 0.222, 0.139], edge life [0.51, 0.58, 0.53, 0.51];
  - alpha 0.8, tinted by the element colour (lightened 35% toward white).
- **Top flame:** 35/s, life 0.62 s, size scale 1.0; Fire's flame is the larger client
  size (0.16 -> 0.30 -> 0.12).
- **Death FX:** dust at 94.6/s for 33-433 ms; smoke at 32/s for 266-1166 ms, life 2.0 s,
  alpha-blended, orange to grey.

Effect sizes are ABSOLUTE yards, as benched. They are not rescaled by the 0.3 creature
scale: the user approved them at these sizes around a 1.0 yd post.
