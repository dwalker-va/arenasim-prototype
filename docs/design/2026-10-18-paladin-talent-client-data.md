# AS-337: Paladin talent grants and procs — Client-Data Research

This doc covers the Paladin talent effects on AS-302 (the talents proposal's Paladin trees)
that need a visual:

- the grants: Divine Favor, Aura Mastery, Divine Protection, Blessing of Sacrifice, Sacred
  Shield, Repentance and Crusader Strike;
- Retribution Aura, a new aura option, and the Holy damage it deals;
- the procs: Infusion of Light, Light's Grace, Vengeance and The Art of War.

The stat and patch nodes leave no footprint of their own and are skipped.

Findings come from three builds:

- Classic Era **1.15.9.69547**;
- TBC Classic **2.5.6.69795**, the reference unless a row says otherwise;
- Wrath Classic **3.4.5.63697**, for the spells whose form on AS-302 is Wrath's: Aura
  Mastery (an activated 6 s immunity only from Wrath), Divine Protection (damage reduction
  only from Wrath), Sacred Shield, Infusion of Light, The Art of War and Vengeance's state.

The Era build carries Sacred Shield, Infusion of Light and The Art of War too, under their
Wrath spell IDs (Season of Discovery). Their visual chains match Wrath's, with two
differences noted below.

The sources are the wago.tools DB2 CSVs, joined directly, and M2 files fetched from CASC and
parsed. No wiki or prose source was used as evidence. Where a reading rests on inference
rather than a table, the row says so ("inferred").

## Method

- **Inventory.** `scripts/db2_spell_sweep.py --build <b> --skill-line 594 267 184 --name
  "Divine Favor" "Aura Mastery" "Divine Protection" "Blessing of Sacrifice" "Hand of
  Sacrifice" "Sacred Shield" Repentance "Crusader Strike" "Retribution Aura" "Infusion of
  Light" "Light's Grace" Vengeance "The Art of War" --allow-split` at all three builds. Skill
  lines: 594 Holy, 267 Protection, 184 Retribution.
  - Counts: Era `7 = 2 + 5`, TBC `13 = 5 + 8`, Wrath `16 = 3 + 13`.
  - TBC has no visual for Aura Mastery (a passive there), Light's Grace, Vengeance's
    talent ranks, Improved Retribution Aura or Judgement of Vengeance.
  - Wrath has no visual only for Light's Grace, Judgement of Vengeance and Righteous
    Vengeance.
  - Property 4 **HELD** at every build: each named spell with a visual resolves to exactly
    one `SpellVisual` at Probability 1. `--allow-split` was passed but not needed.
  - The run is narrowed by `--name`, and at Era the default `--era-cut 400000` excluded 23
    names. These are claims about the named spells only.
- **Proc and helper spells**, from `SpellName` (exact name, SpellID < 400000) and confirmed
  from `SpellEffect`:
  - Vengeance: the talent ranks (20049, 20056–20059) are aura 42 proc triggers with no
    visual. The **buff is 20050–20055** (aura 79, damage done %), visual **6597**.
  - Light's Grace: talent 31833 / 31835 / 31836 trigger buff **31834** (aura 107, cast-time
    modifier). Neither has a visual at any build.
  - Infusion of Light: talent 53569 / 53576 (no visual) trigger buffs **53672 / 54149**,
    visual **12008**.
  - The Art of War: talent 53486 / 53488 (no visual) trigger buffs **53489 / 59578**, visual
    **11955**.
  - Sacred Shield: the 30 s buff is **53601** (visual 11956). It triggers **58597**, the
    absorb (aura 69, school absorb, all schools), visual **12581**.
  - Retribution Aura 7294 is an area aura (effect 35) of aura 15, a damage shield. Its damage
    is dealt by the aura itself: there is **no separate damage spell**, and so no visual row
    for the hit.
  - Blessing of Sacrifice is named Hand of Sacrifice at Wrath. TBC 6940 is aura 562 (the
    damage split) on the target.
- **Joins.** `SpellXSpellVisual → SpellVisual → SpellVisualEvent → SpellVisualKitEffect`
  (effect types 2 model attach, 6 body anim, 1 procedural, 5 sound — ignored). Event pairs:
  `(1,2)` precast, `(3,13)` cast, `(6,13)` impact, `(7,8)` sustained aura state. Target 1
  is the caster and target 2 the target. A self-cast spell lists both, which is the same
  unit.
- **Model names** from the community listfile (`as333/dl/listfile.csv`). Most of these
  models carry an empty M2 `NAME`; the listfile names every one.
- **M2 parser** (v274): the AS-333..336 `m2.py`. **None of these models has a `GPID`
  chunk**: every emitter is a sprite.
- **Units and conventions** (as AS-335/336): model units ≈ yards; particle colours RGB 0–255
  over normalised particle life, alpha 0–1; blend 4 = additive; attachments 19 Base (feet),
  20 Head, 21/22 hands, 34 Chest. Sequences 0 Stand (the onset), 158 the loop, 159 the end.
  Body anims: 9 CombatWound (a flinch), 14 Stun, 52–54 the precast and cast poses, 57
  Special1H (the anim Mortal Strike plays).

## Summary

| Effect | Client visual | Sustained? | What the engine draws today |
|---|---|---|---|
| Divine Favor | hand cast flash + `innerfocus_impact_chest.m2` | no | nothing (new) |
| Aura Mastery (Wrath) | hand cast flash + `holyward_impact_chest.m2`, 3 s, on the Paladin | no | nothing (new) |
| Divine Protection (Wrath) | `wrongfully_accused_impact.m2` at the chest **for the whole buff** | **yes, 8 s** | the band on `DamageTakenReduction` |
| Blessing of Sacrifice | `blessingofsacrifice_impact.m2` at the feet of **both** units, 1.9 s | no (**no link**) | nothing (new mechanic) |
| Sacred Shield, buff | `blessed_mending_impact.m2`, 2 s, on the ally | no | nothing (new) |
| Sacred Shield, absorb | `blessed_mending_proc_impact.m2`: a gold card shield **while the absorb holds** | yes, short | an unnamed `Absorb` draws nothing |
| Repentance | gold flash + flinch; then `shadowwordbefuddle_head.m2` + **Stun pose** for the duration | yes, 6 s | `Incapacitate` routes to **Freezing Trap's ice block** |
| Crusader Strike (TBC) | Special1H + red weapon trail; `strike_impact_chest.m2` | no | nothing (new) |
| Retribution Aura | `retributionaurared_base.m2`: a red rune ring at the feet, 1.4 s, **on cast only** | no | the aura band; the hit is new |
| Infusion of Light (Wrath) | `infusion_of_light_impact.m2` above the head, ~1.8 s | no | nothing (new) |
| Light's Grace | **none at any build** | — | — |
| Vengeance | `vengeance_state_hand.m2` on **both hands for the whole buff** | **yes, 30 s** | nothing (new) |
| The Art of War (Wrath) | `the_art_of_war_impact.m2` at the chest, 2 s | no | nothing (new) |

## Findings

1. **The client is lighter on sustained channels than the card feared.** Only three of these
   hold a model for an aura's life: Divine Protection (8 s, the Paladin), Vengeance (30 s, the
   Paladin's hands) and Repentance (6 s, the enemy). Sacred Shield's absorb card is short.
   Sacred Shield's 30 s buff, Blessing of Sacrifice, Aura Mastery and Retribution Aura draw
   **no state at all**: each is a one-shot on cast, and the icon carries it after.
2. **Retribution Aura has no standing visual and no hit visual.** The client draws a red rune
   ring at the Paladin's feet as the aura goes up, as it does for every Paladin aura, and
   nothing after. The damage-shield hit has no spell of its own, so it draws only combat
   text. A standing on-body aura, or a per-hit flash, would be authored.
3. **Blessing of Sacrifice has no link.** The client marks both ends at the cast (the same
   impact at the Paladin's feet and the ally's) and draws nothing while damage is
   transferred. A tether or a per-transfer pulse would be authored.
4. **Repentance looks like a daze, not an enclosure.** The client plays a gold holy flash
   and a flinch, then holds the victim in the **Stun pose** with a violet-blue sparkle swirl
   around the head. Our `Incapacitate` type routes to Freezing Trap's ice block today, so
   Repentance needs its own route by name, whatever it is drawn as.
5. **Sacred Shield's absorb is a shield card, from the same family as Power Word: Shield.**
   `blessed_mending_proc_impact.m2` uses PW:S's own textures (`shockwave10d`,
   `genericglow2b`) on a tall flat card at the feet, with birth, loop and death sequences.
   It fits `pw_shield.rs` as a fourth `ShieldKind`. That is a better fit than the "existing
   absorb treatment", which draws nothing for an unnamed absorb.

## 1. Divine Favor (20216, visual 7424)

The same chain at all three builds.

- `(1,2)` precast, **kit 266**: `magic_precast_hand.m2` on both hands. Divine Favor is
  instant, so the precast never plays.
- `(3,13)` cast, **kit 425**: `magic_cast_hand.m2` on both hands, body anim 54.
  - 1.0 s, three emitters on a 0.96 yd span:
  - `cyan_glow3`, 8.3/s, 0.8 s life: violet (167,84,244) → cyan (62,227,229) → yellow;
  - `star5a`, 11.9/s: green (0,221,62) → pale cyan → gold;
  - a `teleporttarget` ring, 13.2/s: yellow → violet → green.
- `(6,13)` impact on the Paladin, **kit 3772**: `innerfocus_impact_chest.m2` at the chest.
  - A 0.4 s burst (Priest Inner Focus's model):
  - `flare` 100/s for 60 ms at 2.2 yd/s, blue (64,121,251) → yellow → red;
  - a `gradient64flipa` streak, 60/s, 0.8 s, violet (88,0,248) → red → sky blue;
  - four `flare` points 70/s on a 0.36 yd box, deep blue → cyan → lavender.
- **The buff has no state.** The icon carries "next heal crits" until it is spent.

## 2. Aura Mastery (Wrath 31821, visual 13619)

At TBC 31821 is a passive with no visual. Wrath makes it an activated ability, which is the
form on AS-302.

- `(3,13)` cast, **kit 191**: `holy_precast_high_hand.m2` on both hands, body anim 54.
- `(6,13)` impact on the Paladin (targets 1 and 2), **kit 489**: `holyward_impact_chest.m2`
  at the chest, **3.0 s**, played once.
  - A camera-facing 4.4 yd `yellow_glow` quad: weight 0 → 0.6 by 0.67 s, held to 2.33 s,
    out by 3.0 s.
  - `shockwavewater1` ripples, 10.8/s, 1 s life, gold (253,221,94), alpha 0.4.
  - A `star8b` burst, 45/s from 67 to 600 ms, 2.56 yd/s with heavy drag (3.35), white → gold.
  - `genericglow2c` amber glows, 7.5/s, 1.2 s.
  - A `star8b` ring emitter (type 3), 61/s, radius 1.0 yd, gold → white.
- The team-wide effect (Wrath 64364, on the aura's targets) **has no visual**. The client
  draws the ward on the Paladin only.

## 3. Divine Protection (Wrath 498, visual 11817)

At Era and TBC, 498 is the old immunity, drawn exactly as Divine Shield (visual 154, the
`divineshield_low_base.m2` bubble). Wrath's 498 is the damage reduction on AS-302 and has its
own visual.

- `(1,2)` / `(3,13)`: Divine Shield's hand kits (99, 417). Body anim 54.
- `(6,13)`: kit 10909, **empty**.
- `(7,8)` state on the Paladin, **kit 11101**: `wrongfully_accused_impact.m2` at the chest
  for the buff's life.
  - One 2.63 s sequence, looped by the state (inferred: a state kit holds its model and the
    model has no loop sequence of its own).
  - A **1020-vertex globe, radius ≈ 0.75 yd**, round the torso. Four textures fade in turn
    across each cycle:
    - `aura_02_h` 0.17–0.83 s, weight 0.8;
    - `flare1_tc` 0.6–1.2 s;
    - `lavalump2` 1.03–1.63 s;
    - `fire1white_invert` 1.27–1.83 s, weight 1.
  - Colour 0 is an over-bright pale gold (0.99, 1.59, 1.0) dipping to amber at 0.9 s. Colour
    1 is red (0.97, 0.14, 0.14), flashing white at 1.27 s.
  - Emitters:
    - a `fire1white_invert` burst at 1.3 s, 2.8 yd/s, white → orange (233,98,32);
    - a `flare07` flash at 1.27–1.47 s, white → orange;
    - `crystalball_large` glints, 11/s from 0.2 to 1.4 s, white → red (223,32,32).
  - Read: a red-gold globe that pulses once every 2.6 s, flaring at the pulse.
- It differs from Divine Shield at a glance: Divine Shield is gold cards at the feet, and
  Divine Protection is a red-gold globe at the chest. AS-302 gives the two no shared lockout,
  so both can be live at once.

## 4. Blessing of Sacrifice (6940, visual 299)

- `(1,2)` / `(3,13)`: `holy_precast_med_hand.m2` on both hands (kits 186, 189). Body anims
  52 and 54.
- `(6,13)` impact on **both** the Paladin and the ally (kit 7815, TBC and Wrath):
  `blessingofsacrifice_impact.m2` at the feet, **1.9 s**.
  - A 3.6 yd-wide, 3.3 yd-tall shell of red layers (colours (1.0,0.21,0), (0.86,0.04,0)):
  - `blessingofsacrifice` / `aurarune8` / glow textures fade in to 0.57 s, hold to 1.5 s,
    out by 1.73 s;
  - `beerbubble` motes, 75/s over a 1.78 yd box, **falling** at 1.67 yd/s, dark red (42,0,0)
    → red → orange (255,60,0);
  - `gradient64flipa` streaks, 33/s at 6.1 yd/s, red → orange, alpha ≤ 0.25.
- At Era the impact is `sealoffury_impact_base.m2` instead: a violet ribbon and rune on the
  ground. TBC and Wrath agree on the red one.
- **No state and no link** (finding 3).

## 5. Sacred Shield (Wrath 53601 / 58597)

The buff (53601, visual 11956):

- `(1,2)`: `holy_precast_high_base.m2` at the feet and `holy_precast_high_hand.m2` on both
  hands (kit 193). `(3,13)`: kit 191, as Aura Mastery.
- `(6,13)` impact on the ally (and the Paladin), **kit 11039**: `blessed_mending_impact.m2`
  at the chest, **1.97 s**.
  - A flat 1.6 yd gold rune disc (`rune1d`, `aurarune_b`), gold (0.95,0.83,0.32).
  - Gradient streaks out from the centre, 66/s from 0.1 to 0.43 s, at 6.7 yd/s.
  - Two counter-rotating `aurarune_b` rune sprites, 7/s.
- **The 30 s buff has no state.**

The absorb (58597, visual 12581):

- `(7,8)` state on the ally, **kit 11040**: `blessed_mending_proc_impact.m2` at the feet, for
  as long as the absorb holds.
- **Birth 0.7 s, loop 1.47 s, death 1.1 s.** A flat card 3.2 yd wide and 4.1 yd tall, from
  0.84 yd below the feet to 3.25 above.
  - Its layers are `shockwave10d` and `genericglow2b`, Power Word: Shield's textures, tinted
    pale gold (0.97,0.99,0.32) and (1.0,0.91,0.32).
  - The weights flicker (0.26 → 1.0 at 300 ms in birth; 0.16–0.49 through the loop), as PW:S
    shimmers.
  - `flare1_tc` sparks, 111/s for the first 33 ms of death only, gold.
  - `t_vfx_flare07` flares, 44/s through the loop, white → yellow (241,234,44).
  - A `flare07` burst inward, 111/s at 34–67 ms.
- In Wrath the absorb lasts 6 s or until it is used up. On AS-302 it absorbs about 40 at most
  once every 6 s. How long the card holds depends on whether the build card makes the absorb
  an aura that waits for damage, or eats the hit at once.

## 6. Repentance (20066, visual 5539)

The same chain at all three builds.

- `(3,13)` cast, **kit 190**: `holy_precast_high_hand.m2` on both hands, body anim 53.
- `(6,13)` impact on the victim, **kit 6499**: body anim **9 (a flinch)** and
  `holy_impactdd_low_chest.m2` at the chest, **0.93 s**.
  - `starflashyellowa` stars, 75/s from 67 to 400 ms, yellow → white.
  - `ribbonblur1bea_gold` streaks, peaking at 250/s at 134 ms, white.
- `(7,8)` state on the victim, **kit 4949**: body anim **14 (Stun)** and
  `shadowwordbefuddle_head.m2` at the head, for the duration.
  - A 2 s loop. Two `sparkle` emitters, 111/s each, on a 1.5 yd box round the head, falling at
    3.3 yd/s, 0.5 s life:
    - one sky blue (104,182,242) → violet (184,0,245) → white;
    - one indigo (102,35,225) → lavender → white.
  - Three `star8b` points at about 0.7–1.2 yd from the head (bones 14–16), 40/s, violet
    (180,0,247) → white. The bones animate (not parsed): inferred to circle.
- **Our routing.** `AuraApplyRoute::for_aura(Incapacitate)` is
  `Owned(IceBlock)` (`components/visual.rs`), Freezing Trap's ice block. Repentance must be
  routed by name, as `AuraLandingKind` does for Rejuvenation.
- **Our grammar.** `hard_cc.rs` reserves "over the head" for Stun (a hueless whirl of beads)
  and "enclosure" for Incapacitate (the ice block). The client's Repentance is over the head,
  and in colour.

## 7. Crusader Strike (TBC 35395, visual 8316)

- `(3,13)` cast, **kit 324**: body anim **57 (Special1H)**, and procedural type 8 with values
  `16263465, 20, 600, 100` = colour **0xF82929**, width 20, **600 ms**, alpha 100. This is
  the same red weapon trail as Mortal Strike and Heroic Strike (visual 39), which
  `heroic_strike.rs` already draws (`HEROIC_STRIKE_TRAIL_COLOR` 0xF82A29).
- `(6,13)` impact on the victim:
  - **TBC, kit 437 → `strike_impact_chest.m2`**, 0.67 s:
    - `flare` sparks, 50/s from 67 ms at 2.8 yd/s, gold (255,209,0) → orange → dark red;
    - a `fireplume64` puff, 30/s at 34–67 ms at 5.6 yd/s, red (174,0,0) → grey → orange.
  - **Wrath, the same kit 437 → `decisivestrike_impact_chest.m2`**: the landing Heroic Strike
    already ships (`client_landings.rs`).

## 8. Retribution Aura (7294, visual 682)

The same at all three builds.

- `(3,13)` and `(6,13)`, **kits 753 / 810**: `retributionaurared_base.m2` at the feet,
  **1.43 s**, played once.
  - A flat 2.9 yd rune disc on the floor: `aurarune10` glyphs and four `aurarunearrow1` arrows
    at 0.91 yd from the centre, pointing out.
  - Colours red (0.82,0,0), orange (0.99,0.37,0.05), red (1,0,0), amber (1,0.64,0), and a
    pale-gold `blue_glow2` underlay (1,0.78,0.11).
  - The arrows scale 1.0 → 1.62 by 0.8 s, and the glyph rings 1.0 → 1.31 by 0.4 s. Each layer
    fades in and out across 0.1–1.43 s.
  - The model also has a 9.2 s loop (seq 158) that replays the same disc once near its end.
    No kit holds this model as a state, so the loop never plays (inferred).
- **Nothing on the aura's bearers, and nothing on the hit** (finding 2).
- Our Paladin auras are the Paladin AI's first priority, so they go up in the pre-match
  countdown and land on every ally in range. Devotion and
  Concentration are on `lands_silently_audit`'s known-silent list under AS-134 (aura
  application). A rune disc on cast would join that card's scope.

## 9. Infusion of Light (Wrath 53672 / 54149, visual 12008)

- `(6,13)` impact on the Paladin, **kit 11091**: `infusion_of_light_impact.m2` at the feet,
  **3.83 s**, played once. Every emitter has stopped by 1.4 s, so it reads for about 1.8 s.
  - A **0.3 yd ring 2.5 yd up**, just above the head (`timebar2`, 559 vertices), weight 0.55
    from 0.57 to 1.43 s.
  - Above-the-head glows at 2.5 yd:
    - a `crystalball_large` glow, 3/s to 1.17 s;
    - a `shockwave_hip1` pulse, 15/s to 1.37 s, pale gold → amber (236,162,44);
    - a `flare1_tc` and a `flare05` flash at 0–67 ms.
  - Four `rune1d` runes at 1.3–1.5 yd (the chest), 11/s to 0.5 s, rising and falling at
    1.1 yd/s, white → gold.
  - `flare09` bursts at 3.8 yd (0–0.23 s) and at 2.2 yd (0.27–0.87 s), white → gold
    (233,193,32).
- Read: a gold halo pops above the head while gold runes rise at the chest.
- **The buff has no state.** The icon carries it until Flash of Light spends it.

## 10. Light's Grace (TBC 31834)

**No visual at any build.** The icon alone carries it.

## 11. Vengeance (20050–20055, visual 6597)

The same chain at all three builds.

- `(7,8)` state on the Paladin, **kit 5898**: `vengeance_state_hand.m2` on **both hands**,
  for the buff's life (30 s on AS-302). A 1.0 s loop.
  - A 0.37 yd quad, blue (0.37,0.74,1.0), weight 0.8. Its texture is `whiteringfat128`.
  - `shockwave10b` glows, 15/s, 0.8 s life, white → pale gold (255,244,180), alpha peaking
    at 0.65 of life.
  - `flare` sparks, 50/s, 0.4 s, at 0.44 yd/s over a 0.14 × 0.28 yd box, white → sky blue
    (75,177,255).
- Read: both hands glow pale blue and white, softly sparking.
- **A melee crit refreshes it**, so a Retribution Paladin in melee wears it nearly all match.

## 12. The Art of War (Wrath 53489 / 59578, visual 11955)

- `(6,13)` impact on the Paladin, **kit 11038**: `the_art_of_war_impact.m2` at the chest,
  **2.0 s**.
  - A small flat gold disc (`shockwave_fullrays1`, 0.35 yd), weight 1.0 at 100 ms, out by
    634 ms.
  - `flare1_tc` sparks on a ring that **grows from 0.1 to 1.0 yd** over 0.9 s, 333/s to
    0.8 s, pale gold → orange (229,148,62) → yellow.
  - `flare09` glows, 11/s to 0.87 s, gold.
  - `rune1d` runes, 6/s to 0.5 s.
  - A `flare09` burst 1.33 yd above the chest, 77/s to 0.2 s.
- Read: a gold ring of sparks opens out round the chest.
- At Era the kit has no model attached (the SoD row is empty). The Wrath one is used.
- **The buff has no state.** The icon carries "next Flash of Light instant and free".

## Sustained channels

Counted per unit, with every option at its client form:

- **The Paladin**: Divine Protection (8 s), Vengeance (30 s, near-permanent in Retribution),
  and the Divine Shield cards already shipped. Divine Protection and Divine Shield have no
  shared lockout on AS-302, so all three can stack. Vengeance and Divine Protection are both
  tier 2, in different trees, so a hybrid build can take both.
- **An ally**: Sacred Shield's absorb card (short), alongside a Power Word: Shield card or
  Ice Barrier.
- **An enemy**: Repentance's head swirl, 6 s. It replaces nothing: a Repentance victim is in
  no other crowd control (shared diminishing returns with Polymorph).

## What the engine doesn't draw today

- **`Incapacitate` has one owner**, Freezing Trap's ice block. Repentance needs a route by
  name.
- **An unnamed `Absorb` draws nothing.** `pw_shield.rs` draws three named shields.
- **The damage-shield hit** is a new damage source. `lands_silently_audit`'s damage family
  will ask for a visual, or for a `SilentByDesign` entry citing this doc.
- **The `Special1H` swing** is drawn for Mortal Strike (`SwingStyle::MortalStrike`), and the
  red trail by Heroic Strike's `heroic_strike_trail()`. Crusader Strike can use both.

## Rulings (user, 2026-10-18)

The user signed off the bench at its defaults, version **1792379243-e3d6**
(`benches/2026-10-18-paladin-talent-bench.html`): every client layer kept at client values,
every authored layer off.

- **Retribution Aura**: the rune disc on cast; nothing standing; the hit is silent by design.
- **Blessing of Sacrifice**: the impact at both units' feet; no link.
- **Aura Mastery**: the ward on the Paladin only.
- **Sacred Shield**: the disc on cast; the absorb card held while the absorb lasts.
- **Repentance**: the gold impact, then the violet head swirl, routed by name.
- **Crusader Strike**: Mortal Strike's stroke, Heroic Strike's trail, TBC's landing.
- **Vengeance** and **Divine Protection**: held for the whole buff.
- **Divine Favor, Infusion of Light, The Art of War**: the client one-shots. **Light's Grace**:
  icon only.

Build cards: AS-374 (the one-shots and the instant hand flash), AS-375 (Sacred Shield), AS-376
(Divine Protection), AS-377 (Vengeance), AS-378 (Repentance), AS-379 (Crusader Strike).
