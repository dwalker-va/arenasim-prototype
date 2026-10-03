# AS-160: Druid kit visuals — Client-Data Research

All findings come from WoW Classic Era build **1.15.9.69547** client data: wago.tools DB2
CSVs, joined directly, plus M2 files fetched from CASC and parsed. No prose or wiki
sources were used as evidence.

## Method

- **Inventory.** `scripts/db2_spell_sweep.py --skill-line 134 573 574 --caster-anims
  --group-by-anims` covers Feral Combat, Restoration and Balance. It prints `106 = 51 + 55`.
  Seven of our nine abilities are on these lines at era IDs.
  - Lifebloom is above the era cut. It exists in this build only as a Season of Discovery
    rune, 408124, plus its helper spells 408245 and 408246.
  - The TBC Cyclone (33786) is absent. The two spells named "Cyclone" in this build
    (5197, 5199) are NPC spells that play `learn_impact_base.m2`. That is not the Druid
    spell, and it was joined to prove it.
- **Joins** follow the usual chain:
  - `SpellXSpellVisual → SpellVisualEvent → SpellVisualKitEffect`, then
    `EffectType 2 → SpellVisualKitModelAttach → SpellVisualEffectName.ModelFileDataID`
    for models.
  - `EffectType 6 → SpellVisualAnim` for body animation.
  - Event `(1,2)` is precast, `(3,13)` is cast, `(6,13)` is a one-shot impact, and
    `(7,8)` is an aura STATE (sustained).
- **M2 parser.** Particle emitter stride is 492 and ribbon stride is 176 on version 274.
  Both were validated by re-deriving `slow_impact_base.m2` exactly as
  `2026-09-26-slow-state-client-data.md` publishes it (rates 14/65.1, lives 0.395/0.7,
  and the yellow → violet colour track).
- **Attachment IDs:**
  - 19 = Base (the feet);
  - 20 = Head;
  - 21/22 = the spell hands;
  - 34 is read as Chest, from the community enum (not data);
  - −1 = no attachment, placed at the unit's origin.

## Summary table

| Ability (ours) | Client spell → visual | Cast side | Landing (one-shot) | Sustained `(7,8)` state |
|---|---|---|---|---|
| Rejuvenation | 774 → 32 | kits 345/181 = nature hands (**shipped**, `HealCastKind::Nature`) | kit 56 `rejuvenation_impact_base.m2` @Base, 3.0 s | **none** |
| Lifebloom | SoD 408124 → 8145 | kits 345/181 = nature hands | — | kit 6966 `lifebloom_state.m2` @Head, 1834 ms loop |
| Lifebloom bloom | SoD helper 408245 → 8101 | — | kit 6965 `lifebloom_impact.m2` @34 (Chest), 1867 ms | — |
| (Lifebloom mana return) | SoD helper 408246 → 54 | — | kit 4990 `manainfuse_base.m2` | — (we have no mana return; not used) |
| Swiftmend | 18562 → 3884 | kits 100/183 = nature hands | kit 101 `restoration_impact_base.m2` @Base | none |
| Innervate | 29166 → **3884** (same visual as Swiftmend) | nature hands | kit 101 `restoration_impact_base.m2` @Base | **none** |
| Moonfire | 8921 → 1263 | kit 730 `magic_cast_hand.m2` (both hands) | kit 3293 `moonfire_impact_base.m2` @Base | **none** |
| Mark of the Wild | 1126 → 212 (shared with Gift of the Wild) | kit 181 nature hands | kit 542 `markofwild_impact_head.m2` @Head, 667 ms | none |
| Entangling Roots | 339 → 38 | kits 345/181 nature hands | — | kit 66 `entanglingroots_state.m2` @−1 (origin) |
| Cyclone (TBC) | **absent from build** | — | — | nearest kin: kit 861 `cyclone_state.m2` @Base + victim anim 41 (Enveloping Winds 6728/15535, Tornado 21990) |
| Travel Form | 783 → 4228 (shared with Cat Form) | kit 345 precast hands | kit 3610 `druidmorph_impact_base.m2` @Base, both shift in and shift out | body = display 918 → `creature/tiger/tiger.m2` (the cheetah skin), scale 0.8 |

### Three findings that change the work

1. **Swiftmend's borrowed landing is already client-faithful.** `restoration_impact_base.m2`
   is the model `HealImpactKind::HealingWave` was built from
   (`2026-09-06-heal-impact-client-data.md`). Healing Wave, Lesser Healing Wave, Swiftmend
   and Innervate all share that one kit (101). The "until the Druid's own visuals land"
   comment in `components/visual.rs` is wrong: Swiftmend needs no new visual.
2. **The client gives Rejuvenation, Moonfire and Innervate no sustained state.** Lifebloom
   and Entangling Roots are the only sustained Druid channels in the source. Our Moonfire
   DoT and the Rejuvenation HoT have no client identity to transcribe while they run.
3. **Every Druid cast hand except Moonfire's is the shipped Nature hand.** Kits 345/181 and
   100/183 are the same two models (`nature_precast_low_hand`, `nature_cast_hand`) the
   Shaman's Lesser Healing Wave already draws. That covers Rejuvenation, Lifebloom,
   Swiftmend, Innervate, Mark of the Wild, Entangling Roots and Travel Form's precast. Two
   things are needed:
   - routing those abilities to `HealCastKind::Nature`;
   - an instant-cast entry, because the casting orb and hand glows skip zero-length casts
     today.

## Per-model detail

### `rejuvenation_impact_base.m2` (Rejuvenation landing)

- Pure particle and ribbon model, one 3000 ms sequence.
- **Five ribbons**, each on a bone that orbits the body centre. The bones are rotation-keyed
  at 15–27 keys, with pivots about 0.75 yd off the axis at heights 0.7–2.0.
  - Texture `spells/crystalball.blp`; alpha 0.5.
  - Height above and below 0.139 each, so each band is 0.28 yd wide.
  - Five greens: `(0,172,49)`, `(70,91,13)`, `(156,228,54)`, `(93,196,12)`, `(24,128,0)`.
  - **Identity: a green ribbon swirl wrapping the target for 3 s.**
- Three emitters at the body core, all additive:
  - **P0 and P2:** `flare.blp` sparks, sizes 0.28 and 0.14, life 0.3 s, 20/s each. Colour
    runs dark green → white (P0) or yellow (P2) → light green. Alpha 0.39 → 1 → 0.39.
  - **P1:** `starbursts.blp` (2×2 atlas), size 0.97 → 1.39 → 0.97, life 0.2 s, 28.8/s. White,
    with alpha peaking at only 0.22. It is a faint pulsing starburst.

### `lifebloom_state.m2` (Lifebloom sustained, @Head)

- Particle-only, with a 1834 ms loop.
- **P0:** `ribbonblur1bea_green.blp` streaks, flag `0x840038`. They rise from 0.54 above
  the head anchor (speed 1.11, drag 0.5; `vRange` 0 reads as straight along the emitter
  axis).
  - Enabled only for **100–600 ms of each loop**, at a rate of 15 → 20 → 5.
  - Life 0.6 s; tail 1.85; size 0.083 → 0.014 → 0.
  - Colour green `(22,185,0)` → white → pale yellow. Alpha 0.13 → 1 → 0.
  - It reads as a short burst of green sparkle streaks once per 1.8 s.
- **P1:** `starflashyellow.blp` star flashes, enabled for **0–967 ms of each loop**.
  - Speed 3.33, life 0.5 s, 10/s; size 0 → 0.139 → 0; spin 0.05.
  - Colour green-teal `(0,213,85)` → pale yellow → white.
- **Duty cycle:** each 1.8 s loop is one pulse, with a quiet second half. That is a
  lower-duty sustained channel than a constant glow.

### `lifebloom_impact.m2` (the bloom, @Chest)

- 1867 ms, four emitters, all additive. The palette is **gold, not green**: `(255,156,0)` →
  pale yellow → white on every emitter.
- **P0:** `yellow_glow_dim.blp` glow, enabled 100–633 ms, rate 15 → 20 → 5. Size 0.23 → 0.88 →
  0.78 (a swelling bloom of light), life 0.9 s.
- **P1:** `shockwavewater1extreme.blp`, speed 3.33, full sphere, life 1.0 s, enabled for the
  first 1000 ms.
- **P2:** `star5a.blp` streaks, tail 3.4, rising along the emitter axis (`vRange` 0).
- **P3:** `yellow_star_dim.blp` drifting stars, speed 1.11, full sphere, life 1.25 s, 16/s.
- **Identity:** a golden flower-burst.

### `restoration_impact_base.m2` (Swiftmend, Innervate)

Already measured in `2026-09-06-heal-impact-client-data.md`, and already shipped as
`HealImpactKind::HealingWave`.

### `moonfire_impact_base.m2` (Moonfire landing, @Base)

- A **mesh** model: 342 vertices; extent x/y ±3.8, z −2.0 … **13.9**.
- It is a column of light descending from far above the target. It is not a particle
  burst.
- One sequence of 12000 ms, but every texture-weight track is dark after **2300 ms**:
  - `blue_glow2` 0 → peak at 667 → 0 at 1333 ms;
  - `red_glow3` 0 → 1133 → 0 at 2267 ms;
  - `ribbonblur1bea_gold` peaks at 600, gone by 1533 ms;
  - `gradient` held 700–1000 ms, gone by 1167 ms.
- One colour track is violet `(47,0,144)`. The visible event is about **1.3 s of
  blue-and-gold column**, with a red-violet afterglow out to 2.3 s.
- Not yet measured: the column's radius profile. Bucket the vertex cloud along z before
  benching, per the recipe's gotcha 4.

### `magic_cast_hand.m2` (Moonfire cast hand)

- 1000 ms, with three emitters per hand.
- **P0:** `cyan_glow3` motes; violet → cyan → gold; size about 0.08.
- **P1:** `star5a` twinkles; green → pale cyan → gold.
- **P2:** `teleporttarget` ring disc, enabled for the first 300 ms only; gold → violet →
  green.
- **Identity:** a multicolour arcane sparkle. It is not shipped anywhere yet.

### `markofwild_impact_head.m2` (Mark of the Wild, @Head)

- 8 vertices: **two flat quads** in the y/z plane, 0.7 × 0.7 yd, from 0.68 to 1.38 above the
  head anchor.
- Texture `spells/agility_128.blp`. Two colour tracks: red-orange `(223,52,0)` and gold
  `(255,201,0)`.
- Texture weight fades 0 → 0.5 @167 ms → 0 @667 ms.
- **Identity:** a brief (0.67 s) orange-gold glyph above the head.

### `entanglingroots_state.m2` (Entangling Roots sustained, @origin)

- A **mesh**: 300 vertices, textured `spells/kalidartreetrunk02.blp` (bark). The raw extent
  spans y −2.3 … 3.7 and z −0.5 … 4.6. The bones re-pose it, so the bind pose is not the
  in-game silhouette.
- Three sequences: 0 (1500 ms), 158 (1500 ms) and 159 (1300 ms). These are read as
  birth / stand / death, like the other state models.
- Four emitters:
  - **P0, P2:** `dust1` puffs in earth brown `(106,91,0)` → `(157,146,87)`, 30/s, stopping at
    1334–1500 ms, so the dust is the **birth** only.
  - **P1:** `partrock` pebbles, gravity 4.2, rate 0 → 20 → 10 over the first 1.5 s.
  - **P3:** a green `(98,126,0)` → `(155,199,0)` ground disc. Its rate is 0, so it is
    effectively off.
- **Identity:** bark-textured roots erupt around the feet in a puff of brown dust and
  pebbles, then hold.
- **Our current rendering is wrong for this.** `hard_cc::root_style` maps Nature to
  `RootStyle::Web`, so Entangling Roots wears Spider Web's silk sheet.

### `cyclone_state.m2` (Cyclone stand-in, @Base)

- Eleven emitters and no mesh. Loop 2066 ms, death sequence 158 (2200 ms).
- **Funnel, P0–P4:** `clouds.blp`, blend 1 (alpha-key, not additive), in grey `(153,153,153)`
  → white.
  - Speeds and rates are keyframed on a 2066 ms loop, which gives it a gusting funnel.
  - P4's colour ends violet-blue `(31,3,243)`.
  - Heights run −0.33 … 1.36.
- **P5:** `toonsmoke16` (8×8 atlas) white smoke, spin 2.0, 3 s life.
- **P6–P10:** brown `(98,85,77)` debris. Four of them sit at radius **2.54 yd** around the
  base with gravity −6.9 and life 0.7 s, so dust is kicked up at the funnel's rim.
- **Victim animation:** kit 861 carries `SpellVisualAnim` loop **41**. Its name is not in the
  table; in the community enum, 41 sits in the swim/fall block. Before building a
  "lifted" pose on it, check what TBC Cyclone actually did to the victim. Ask for a
  reference.
- This is the client's wind-vortex kit, shared with Windfury (kit 363), Tornado and
  Enveloping Winds. **Enveloping Winds (aura 12 = stun) is a wind CC that suspends its
  victim. It is the closest era analogue to Cyclone**, as Wind Shear borrowed in AS-137.

### `druidmorph_impact_base.m2` (Travel Form shift in and out, @Base)

- 2800 ms, five emitters. It is shared by every shapeshift: Bear, Cat, Travel, Moonkin and
  Tree.
- **P0:** `genericglow2c` swelling glow. Violet `(69,0,210)` → sky blue `(106,180,243)` →
  violet. Size 0.15 → 0.74 → 1.84; alpha ≤ 0.36; 0–1400 ms.
- **P1, P2:** `toonsmoke16` white smoke, blend 2, in a full sphere. 37/s each, sizes about
  0.6/0.4, for 0–1700 ms.
- **P3:** `gradient64flipa` violet-blue → white streaks with **tail 5.0** and spin −2.0.
  These fly out fast (3.9).
- **P4:** `stardust` violet → blue → white motes.
- **Identity:** a cartoon puff of white smoke with a violet-blue flash. It is the
  "poof" of a shapeshift.

### `tiger.m2` (Travel Form body)

- Display 918 (`CreatureModelScale` 0.8) → model 82 → `creature/tiger/tiger.m2`, with the
  cheetah skin (texture variation fdid 126187).
- The bind-pose extent at scale 1:
  - nose +1.30, rump −1.25, tail tip −2.12 (the tail is held **raised**, z 1.0–1.25 along
    its whole length);
  - width 0.96, height 1.42.
- At ×0.8 that is about 2.7 yd nose-to-tail, 1.14 yd tall and 0.77 yd wide.
- Sequences:
  - Run (5): 800 ms at move speed 6.94;
  - Walk (4): 1000 ms at 2.5;
  - Stand (0): 2000 ms.
- Our bodies are primitive capsules, so the build is a primitive cat assembled the way
  Polymorph builds its sheep: a horizontal torso, a head, four legs and a raised tail.
  It needs its OWN restore components; see the card's constraint about
  `OriginalBodyMaterial`.

## Sustained channels per victim (budget count)

- **On an ally:** Lifebloom state, plus Rejuvenation, which has no state in the client. Our
  HoT ticks currently borrow the Healing Stream Totem pulse, so a Druid's ally can carry
  two HoT tick visuals. Rejuvenation is 3 s and Lifebloom 1 s, both visually identical to
  a totem's.
- **On an enemy:**
  - Entangling Roots (state);
  - Cyclone (state);
  - Moonfire, which has no state in the client.

  Roots and Cyclone cannot coexist with each other in practice, because Cyclone is
  immune to everything.
- No new channel collides with the Warlock's three. Moonfire is the one DoT that the
  `lands_silently` DoT family requires to reach a visual, and the client gives it none.

## Rulings (user, 2026-10-03)

1. **HoT ticks: client-faithful.** Rejuvenation draws its 3 s ribbon swirl on landing and
   nothing on its ticks. Lifebloom draws its 1.8 s head pulse while it is up, and the gold
   bloom when it ends. `kind_for_hot_tick` gets a per-ability route by RON name, as
   `DotStateVisual::for_dot` has, and that route returns none for both Druid HoTs, so
   they stop borrowing the Healing Stream pulse.
2. **Moonfire's DoT: nothing sustained.** The landing column is the whole visual, and the
   aura icon carries the DoT, as with Curse of Tongues. Moonfire's DoT-family entry in
   `lands_silently_audit.rs` becomes a permanent entry whose reason cites this doc,
   instead of a card id.
3. **Cyclone: funnel plus a lifted victim.** It borrows `cyclone_state.m2` (kit 861). The
   victim is lifted and spun slowly for the duration through its own transform
   component, never the `OriginalBodyMaterial` / `OriginalMesh` slot.
4. **Innervate: faithful reuse.** It lands with the shipped `HealImpactKind::HealingWave`
   splash (kit 101), with zero new art. The rising aura band and the icon tell it apart
   from a heal.

Not put to the user because the data settles them, so they stand unless reversed:

- Swiftmend keeps the Healing Wave landing; it is already faithful.
- Entangling Roots gets a new bark-root style in place of the web.
- Mark of the Wild gets its 0.67 s glyph.
- Moonfire gets its arcane cast hands and its column.
- Travel Form gets a primitive cheetah body with its own restore component, plus the
  shift puff on entry and exit.
- Every other Druid cast routes to the shipped `HealCastKind::Nature` hands, including
  the instants.

## Bench

The Druid Visuals Bench is https://claude.ai/artifact/SF3kLUNLY5wVRjFFSzRXwn. Every
effect opens at the client's values (×1.0) at real time, with one toggle per emitter
layer. Its sign-off goes here.
