# AS-338: 0.9 trinket effects — Client-Data Research

This doc covers the trinket effects Gear E2 (AS-305) builds:

- **Aegis of the Last Stand**, the triggered absorb (below 35% health, once a match);
- the **on-use** trinkets: the Medallion (the crowd-control break), Talisman of Ephemeral
  Power, Earthstrike, Diamond Flask and Kiss of the Spider;
- the **procs**: Hand of Justice (the extra swing), the Dragonspine Trophy (the haste aura)
  and Darkmoon Card: Blue Dragon.

A proc buff already wears its trinket's icon in the buff bar (`Aura::source_item`). The
question is which effects need more than that, and what.

Findings come from two builds:

- Classic Era **1.15.9.69547**, for the Classic items;
- TBC Classic **2.5.6.69795**, for the Medallion and the Dragonspine Trophy (TBC items), and
  the reference for every spell whose Era row has been rewritten (Diamond Flask, below).

The sources are the wago.tools DB2 CSVs, joined directly, and M2 and SKIN files fetched from
CASC and parsed. No wiki or prose source was used as evidence. Where a reading rests on
inference rather than a table, the row says so ("inferred").

The bench is `benches/2026-10-29-trinket-effects-bench.html`, published at
<https://claude.ai/artifact/1FRTwgEeJZWYK1iFcHPkY2>, **version 1793329044-312e** (opens on
the Proposed preset). It is not signed off.

## Method

- **The join.** An item's effect is not on a skill line, so `db2_spell_sweep.py` cannot reach
  it. The chain is `ItemEffect[ParentItemID]` → `SpellID` → `SpellEffect` (and on through
  `EffectTriggerSpell`, for a proc aura 42 that fires another spell) → `SpellXSpellVisual` →
  `SpellVisual` → `SpellVisualEvent` → `SpellVisualKitEffect` (2 model attach, 6 body anim,
  1 procedural; 5 sound, ignored) → `SpellVisualKitModelAttach` → `SpellVisualEffectName` →
  `ModelFileDataID`, named from the community listfile. `ItemEffect.TriggerType`: 0 on use,
  1 on equip (a proc is an equip aura 42 that triggers the proc spell).
- **Every item was joined, none assumed.** The Medallion's ten TBC item rows (28234–28243,
  30343–30351, 37864/37865) were found by name in `ItemSparse` and all resolve to the same
  spell, 42292. Classic's Insignia (18854) resolves to 5579 at Era and 42292 at TBC; both use
  visual 86.
- **Rank collapse.** Every spell here has one `SpellXSpellVisual` row at Probability 1.
- **Diamond Flask at Era is rewritten.** Era's item 20130 points at 363880 (an effect-77
  script spell with no visual, a Season of Discovery rework). The original, 24427, survives
  in both builds with visual 101 and is the one read here.
- **M2 parser** (v274, MD21): the AS-333..337 layout, re-implemented for this card and
  validated by re-deriving `holydivineshield_state_base.m2` exactly as
  `2026-10-16-pws-client-data.md` records it (sequences 700/633/1100, the three cards, the
  colours and weight tracks). Skin batches were read for geometry layers. **No model here has
  a `GPID` chunk**: every emitter is a sprite.
- **Units.** Model units ≈ yards; the rig scale is `PWS_CLIENT_TO_RIG` (2.5 / 2.034).
  Attachments 19 Base (feet), 20 Head, 21/22 hands, 34 Chest. Blend 4 = additive.
- **Contrast.** Each effect is drawn alone over the calibrated sand and composited the way the
  client does it (linear HDR, then Bevy 0.16's TonyMcMapface LUT), with AS-401's scene and
  calibration. The number is the peak, over one cycle, of the 95th-percentile |ΔL*| of the
  effect's pixels over lit sand. 10 or more reads as clear (AS-401). The figures below were
  computed headlessly from the bench's own compositing code at 60 fps over 16 s, at the match
  camera (64 yd).

## Summary

| Effect | Client visual | Sustained? | Engine today | ΔL*, client layers | ΔL*, proposed |
|---|---|---|---|---|---|
| Aegis (borrow) | `holydivineshield_state_base.m2`, kit 847 — **PW:S's state**, the client's own item-absorb visual | yes, while the absorb holds | **nothing in the world** (an unnamed `Absorb` draws no rig and no band) | 12.7 (the shipped PW:S rig) | 12.7 |
| Medallion | PW:S's cast hands (kit 270) + `dispel_low_base.m2`, Dispel Magic's impact | no | the stun whirl retracts | ribbon 5.9, hands 0.7 | 12.8 |
| Talisman of Ephemeral Power | `amplifymagic_impact_base.m2`: a red rune disc and a rising rune column, 2.5 s | no | aura band | ≤ 1.2 per layer | 11.6 |
| Earthstrike | `aspectwolf_impact_head.m2`: a green wolf glyph over the head, 0.67 s | no | aura band | 0.9 | 10.9 |
| Diamond Flask | a drink pose, a tankard and `food_healeffect_base.m2` (8 sparkles/s) | no | aura band | 1.7 | 6.2 (the band) |
| Kiss of the Spider | `sprint_cast_base.m2` (Sprint's): a blue floor ring that contracts and re-opens, 1.0 s | no | aura band (once routed) | ≤ 1.7 | 13.4 |
| Hand of Justice | an `Attack1H` swing + `sap_cast_base.m2` (Cheap Shot's crescents); a 10-yd blue floor ripple under **both** units, 1.6 s | no | nothing (Windfury's tornado if the build reuses its spawn) | ≤ 1.3 | 10.9 |
| Dragonspine Trophy | `blessingofstrength_base.m2` (Blessing of Might's): fire ring, a 6.7-yd fire wall, a strength glyph, 1.9 s | no | aura band | ≤ 2.8 | 13.9 |
| Blue Dragon | **none** | — | aura band | — | — |

"Engine today" for the stat buffs is the aura band (`aura_band.rs`), which fires on any new
`AttackPowerIncrease`, `SpellPowerIncrease`, `HealingOverTime` or `ManaRegenIncrease` aura,
item auras included. It measures **6.2** on sand: faint.

## Findings

1. **No trinket buff has a sustained state in the client.** Every effect is a one-shot on the
   moment it fires, and the icon carries the buff after. The sustained-channel budget is
   therefore not spent by any of them unless we author one, and this doc recommends none.
2. **Every client layer is invisible on the sand at client weights.** Each layer alone
   measures under 3 L*, most under 1.5. The cause is AS-401's: the layers are additive light,
   and the sand sits on the tonemapper's shoulder (sRGB 241), so even 12× gain moves no
   effect past 4. The only lever that reaches 10 is AS-401's: draw the layer **alpha-blended in
   its own client colour** at a level below the sand, so it darkens. The proposed preset does
   exactly that for one or two layers per effect, and keeps every other kept layer additive.
3. **Aegis's faithful borrow is Power Word: Shield's rig.** The client's own trinket absorbs —
   Talisman of Arathor and Defiler's Talisman (the Arathi Basin absorb trinkets, 23991 /
   25746 / 25747 / 25750) and Runed Fungalcap (31771) — all wear visual **1726**, a single
   `(7,8)` state kit, **847**, which is PW:S's state kit: `holydivineshield_state_base.m2`.
   That model is `ShieldKind::PowerWordShield`, already shipped and already legible after
   AS-401 (12.7).
4. **The "existing absorb treatment" draws nothing in the world.** `AuraApplyRoute::for_aura`
   sends every `Absorb` to `Owned(Shields)`, which suppresses the aura band, and
   `shield.rs` draws only the three named shields (`is_live_shield`). An unnamed absorb gets
   the health-bar absorb overlay (`hud.rs`), the `N absorbed` combat text and its icon, and
   nothing on the body. See *The caveat* below.
5. **The Medallion's client visual is a self-cast Dispel Magic.** It is PW:S's cast kit (270,
   the Holy two-hand launch `heal_cast.rs` already draws) plus Dispel Magic's impact,
   `dispel_low_base.m2`, which the engine draws as the dispel ribbon (`dispel_ribbon.rs`
   cites it). The client's ribbons are **gold and blue** (`ribbonblur1bea_gold` / `_blue`);
   the engine's Priest ribbon is silver.
6. **Hand of Justice is not Windfury in the client.** Windfury Attack (25504, visual 8251) is
   `cyclone_state.m2`, which `windfury.rs` draws as its grey tornado. Hand of Justice's extra
   attack (15601, visual 6624) is **Sword Specialization's** caster kit 411 — an `Attack1H`
   swing and `sap_cast_base.m2`, the crescents Cheap Shot already draws
   (`rogue_crescents.rs`) — plus a large floor ripple. AS-305 builds the extra swing on
   Windfury's path; if it also reuses `WindfuryTornado`'s spawn, a Hand of Justice proc will
   read as a Windfury Totem proc, a Shaman-team signal. It must not.
7. **Two trinkets borrow class spells' visuals.** Kiss of the Spider is Sprint's
   (`sprint_cast_base.m2`, shared with Thrash 21919) and the Dragonspine Trophy's Haste is
   Blessing of Might's (`blessingofstrength_base.m2`). Neither collides with anything the
   engine draws today.

## 1. Aegis of the Last Stand (borrowed: visual 1726, kit 847)

No Classic or TBC original. The candidates, joined:

| Candidate | Spell → visual | What it draws | Fit |
|---|---|---|---|
| **Item absorb** (Talisman of Arathor, Defiler's Talisman, Runed Fungalcap) | 23991, 25746, 25747, 25750, 31771 → **1726** | `(7,8)` kit 847: `holydivineshield_state_base.m2` at the feet, held while the absorb lasts | **the client's own trinket absorb; = PW:S** |
| Power Word: Shield | 17 → 784 | the same kit 847, plus the Holy cast hands | identical state |
| Last Stand (the name's echo) | 12976 → 2758 | `bloodlust_cast_hand.m2` on hands, head and chest; then a procedural type-1 **red body tint** (0xFA0001, 0.7) for the 20 s | not an absorb; a sustained body tint |
| Spell Shield (Argussian Compass family) | 33054 → 8024 | `shieldwallwar_impact_base.m2` + `manashield_state_chest.m2` held | a Mage-shield look |
| Persistent Shield (Scarab Brooch) | 26470 → 7459 | `spiritarmor_impact_head.m2`, a one-shot | no state |

The PW:S model is fully characterised in `2026-10-16-pws-client-data.md`: three hollow
additive cards on a cylindrical billboard at the feet (rim, two streak rings), a 700 ms pop,
a 633 ms shimmer loop and a 1100 ms swell-and-fade. Its tuning is AS-401's
(`PWS_TUNING`: rim gain 2.0, the alpha-blended gold outline at 0.85 / level 0.6).

**On the bench**, at AS-401's shipped PW:S values: **12.7**, clear. Outline variants (authored):
silver 13.0, crimson 23.0.

### The caveat: is the existing absorb treatment enough for AS-305's review?

**Recommendation: yes, provided "the existing absorb treatment" means the PW:S rig, which AS-305
must wire by name.** The generic absorb treatment (health-bar overlay, `absorbed` text, icon)
is **not** enough: nothing appears on the body when Aegis fires, so "watch Aegis fire in a
match" would rest on reading a white sliver on a health bar.

Wiring the rig is small and faithful: route Aegis's aura name to `ShieldKind::PowerWordShield`
(or a `ShieldKind::Aegis` that reuses `PWS_TUNING` and the PW:S tracks), the same way
`is_live_pw_shield` keys on `PW_SHIELD_AURA_NAME`. The birth pop is the trigger moment (it
fires the instant health crosses 35%), the shimmer holds while the absorb lasts, and the
death swell plays when it is used up. That is the client's own look for a trinket absorb,
and it measures 12.7 on sand. **The proposal is to put that in AS-305 itself**, so its human
test has something to watch; the colour question below can trail.

The open question is whether Aegis should look **identical** to PW:S, as it does in the
client. On a team with a Priest the two cannot be told apart in the world; the buff bar's icon
can. An authored outline colour (silver, or Last Stand's crimson) separates them at the cost
of a hue.

## 2. Medallion of the Alliance / Horde (42292 TBC; 5579 Era; visual 86)

Both builds, both factions, every Medallion row: spell **42292 "PvP Trinket"** (TBC; aura 77
mechanic immunity, misc 1) or **5579 "Immune Root/Snare/Stun"** (Era Insignia; three aura-77
effects, misc 7 / 11 / 12). Both use visual 86:

- `(1,2)` precast, kit 99: `holy_precast_*_hand.m2` on both hands, body anim 52. The use is
  instant, so this never plays.
- `(3,13)` cast, **kit 270**: body anim 54 and `holy_precast_low_hand.m2` on both hands: a
  0.87 yd `yellow_glow3a` card at weight 0.7, a 0.54 yd `genericglow2c`, and three gold
  `genericglow2b` ribbons (0.99, 0.80–0.86, 0.15–0.29) trailing the hands, 0.4–0.6 s life.
  This is PW:S's own cast kit; `heal_cast.rs` draws it as the Holy launch (0.45 s flare).
- `(6,13)` impact on the wearer, **kit 226**: `dispel_low_base.m2` at the feet, **2.77 s**:
  - two ribbons (`ribbonblur1bea_gold`, `ribbonblur1bea_blue`) on bones that orbit (rotation
    on a global loop) while the root climbs **0 → 2.78 yd** over the life: a gold and a blue
    helix rising from the feet past the head;
  - `starflash_grey` sparkles along them, 35–57/s, blue → yellow → orange, 0.4 s;
  - a burst at the crown at 2.27–2.37 s (500/s, gold → yellow → blue, drag 3);
  - an inward implosion at 2.57–2.67 s (1000/s at −3.9 yd/s).
- **No state.** The break is the whole visual.

The engine already draws both halves: the Holy launch hands and the dispel ribbon
(1.15 s, climbing from the feet, playing out with sparks).

**Against the crowd control it removes.** The proposal's worked case is a Medallion breaking
Kidney Shot. The stun whirl (`hard_cc.rs`) itself measures **5.6** on sand, faint, and today
it ends by retracting over 0.18 s. On the bench:

| Medallion layers | ΔL* |
|---|---|
| hands (kit 270) alone | 0.7 |
| ribbon, engine silver, alone | 5.9 |
| ribbon sparks alone | 6.1 |
| authored scatter of the stun's beads alone | 6.4 |
| client defaults (stun + hands + silver ribbon + sparks) | 5.8 |
| ribbon silver at level 0.3, + scatter | 11.8 |
| **proposed: ribbon gold at level 0.3, + scatter** | **12.8** |

The ribbon is a lit, opaque surface facing the same sun as the sand, so at full level it is as
bright as the sand and reads only by hue. Darkening it (level 0.3) is the AS-401 lever.

The proposal's **authored scatter** makes the break read *against the CC*: the CC's own
visual flies apart outward (0.4 s) instead of retracting, so the eye sees the stun broken
rather than expired, and the gold ribbon climbs through it. For fear, polymorph and roots the
same rule would apply to each CC's own end (the shroud, the sheep's puff, the ice spikes);
the bench shows the stun only.

**Gold or silver.** The client's ribbon is gold and blue. The engine's dispel ribbon takes the
dispeller's class colour, so a Priest teammate's dispel is silver and a Paladin's Cleanse is
gold. Gold separates a Medallion from a Priest dispel and collides with a Paladin Cleanse;
silver does the reverse. The hand flash on the wearer (the self-cast) is the cue that tells a
Medallion from either, but at 0.7 L* it does not read on sand.

## 3. Talisman of Ephemeral Power (23271, visual 969)

Arcane school, +175 spell damage and healing for 15 s (here: +40 SP for 15 s, 45 s).

- `(1,2)` / `(3,13)`: `magic_precast_hand.m2` / `magic_cast_hand.m2` on both hands (kits
  266 / 267; body anims 52 / 54), as Divine Favor in AS-337 §1.
- `(6,13)` impact on the wearer, **kit 991**: `amplifymagic_impact_base.m2` at the feet,
  **2.5 s** (Amplify Magic's model):
  - **floor rune**: a 2.5 yd `aurarune_a` disc, red (1,0,0) and pink (1,0.41,0.41), that
    **slams down** (bone scale 2.48 → 0.91 at 200 ms), weight 0 → 1 by 334 ms, held to
    1.1 s, out by 1.6 s;
  - **rising rune column**: ten more `aurarune_a` discs (radii 1.39–1.76 yd) stacked 0.22 yd
    apart *below* the floor on a root bone that rises **0 → 5.06 yd** from 200 ms to 2.5 s,
    their weights staggered 100 ms apart (0.45 → 0.05): a column of fading red rune rings
    rising up through the body and past the head;
  - **small runes**: a 0.73 yd `aurarune_b` at the feet and two more that descend from 1.32
    and 2.64 yd to the floor, pulsing red ↔ pale pink on a 400 ms global loop, weight 0.75;
  - **motes**: three `ribbonne1_red` emitters, 16–20/s, red → pink, rising.

On the bench every layer is ≤ 1.2 L*. Proposed: the floor rune and the column alpha-blended
at 0.5 (level 0.6), the small runes and motes dropped: **11.6**.

## 4. Earthstrike (25891, visual 3160)

Physical, +280 AP for 20 s (here: +70 AP for 20 s, 60 s).

- `(3,13)` cast, kit 2810: body anim 53 and `nature_cast_hand.m2` on both hands.
- `(6,13)` impact on the wearer, **kit 2829**: `aspectwolf_impact_head.m2` at the **head**,
  **0.67 s** (Aspect of the Wolf's):
  - two spherical-billboard quads of `aspect_wolf.blp`, a wolf-head glyph, 0.35 and 0.7 yd,
    0.7–1.4 yd above the head attachment;
  - dark green (0.33, 0.56, 0.03) and yellow-green (0.78, 0.85, 0.0);
  - weight 0 → 0.5 at 167 ms → 0 by 667 (the second by 500); the first scales 1 → 3.56 at
    167 ms and shrinks to 0.58, the second grows 1 → 1.78 → 2.74 as it fades: the glyph pops
    to about 1.25 yd and dissolves outward.

Bench: glyph 0.9 alone. Proposed: the glyph alpha-blended at gain 1.0 (level 0.6): **10.9**.

## 5. Diamond Flask (24427, visual 101)

Nature, +75 Strength and 9 health every 5 s for 60 s (here: a heal over time and +AP for
30 s, once a match).

- `(3,13)` cast, **kit 438**: body anim **61** (inferred a drink or eat emote: it is paired with
  a tankard), `tankarda_spellobject.m2` in the right hand (attach 22), and
  `food_healeffect_base.m2` at the feet: one `star8b` emitter, **8/s**, 0.6 s life, white →
  yellow → white, 0.03–0.06 yd, rising at 1.67 yd/s, enabled from 1.0 to 2.0 s of its 2.83 s
  sequence (a 4.03 s loop exists for eating).
- **No impact, no state.** The 60 s heal over time ticks with no visual.

The client draws a drink. The bench measures its sparkles at 1.7. The aura band (today) is
6.2. Proposed: **keep the band, add nothing.** A drink pose is not in the engine's vocabulary,
and an authored layer on a once-a-match buff is not worth a build card.

## 6. Kiss of the Spider (28866, visual 1508)

On use, +20% attack speed for 15 s (Classic aura 138 / 140; TBC aura 189, 200 haste rating).

- `(3,13)` cast, **kit 395**: `sprint_cast_base.m2` at the feet, **1.0 s** (Sprint's, also
  Thrash 21919's):
  - two flat `shockwave_invertgrey` quads, 2.78 yd, blend additive, unlit;
  - **ring 1**, blue (0.16, 0.40, 1.0): bone scale **2.5 → 0 at 500 ms → 2.5 at 1000**, alpha
    1 → 0 → 1: a 7-yd ring that contracts onto the feet and opens out again;
  - **ring 2**, pale blue (0.55, 0.67, 1.0): scale 0 → 2.5 at 267 ms → 0 at 500, alpha
    0 → 1 → 0: a second ring that flares out and back between.
- No impact, no state.

Bench: ≤ 1.7 per ring. Proposed: both rings alpha-blended at 0.75 (level 0.6): **13.4**.
This is the haste aura's on-use, so the new `AttackSpeedIncrease` type would otherwise get the
band.

## 7. Hand of Justice (15600 → 15601, visual 6624)

The equip aura 15600 (aura 42) triggers 15601, effect 19 (an extra attack).

- `(3,13)` cast on the attacker, **kit 411** — Sword Specialization's (16459, visual 6560):
  - body anim **17, `Attack1H`**: a second swing;
  - `sap_cast_base.m2` at the feet, **634 ms**: two `gradientcrescent` batches, four
    crescents about 1.3 × 0.6 yd at 1.5–2.2 yd up, untinted white, weight 0.52, peaking at 167
    and 200 ms, gone by 267 / 300 ms. Bones translate and grow (0.49 → 1.12). **This is
    exactly Cheap Shot's crescent model** (`rogue_crescents.rs`, which shares it with Sap).
- `(6,13)` impact on **both** the attacker and the target, **kit 3415**:
  `slowingstrike_impact_base.m2` at the feet, **1.6 s**: three flat `shockwavewater1` rings of
  radius 2.9, 4.0 and 4.0 yd (3.6–4.9 in rig yards), blue (0, 0.24, 0.99) → violet (0.24, 0,
  0.99) by 1 s, weight 0 → 1 at 267 ms, held to 733, out by 1000; each collapses
  (scale 1 → 0.1) between 1.0 and 1.33 s.

**What the engine would draw.** `auto_attack.rs` spawns `WindfuryTornado` at the Windfury
extra-swing site. If Hand of Justice reuses that spawn, the bench's tornado layer measures
**31.8** — very legible, and the wrong identity (finding 6).

Bench: crescents 1.3, each ripple 1.1. Proposed: Cheap Shot's crescents, alpha-blended at
gain 2.0 (level 0.6): **10.9**; no ripple, no tornado. The ripple is a 10-yd floor decal under
two units, for a proc whose whole effect is one extra hit (5% a swing, so roughly once a minute
of melee); it competes with the floor's real information (traps, Frost Nova, the slow ring). Alpha-blended at 0.3 with the crescents it
dilutes to 5.8: a large faint shape.

## 8. Dragonspine Trophy (34774 → 34775 "Haste", visual 4345)

TBC: the equip aura 34774 triggers 34775, aura 189, 325 haste rating for 10 s. (Our
Dragonspine moves to the haste aura in AS-305.)

- `(6,13)` impact on the wearer, **kit 146**: `blessingofstrength_base.m2` at the feet,
  **1.9 s** (Blessing of Might's):
  - **fire ring**: a flat 2.7 yd `firering1a` ring, orange (1, 0.30, 0.02); weight 1 → 0 by
    67 ms, back to 0.26 at 167, 0.64 at 1033, 0 by 1800;
  - **fire wall**: from 1.07 s the same ring's bones scale **×44–62 in height** on a flicker
    (100 ms steps), turning it into a cylinder of flame about 5.5 yd tall (6.7 in rig yards)
    around the body, until the end;
  - **glow column**: a 2.4 × 2.85 yd `yellow_glow` card, (0.95, 0.45, 0), weight pulsing to
    0.41 at 567 ms and 0.81 at 1033–1233;
  - **strength glyph**: a 1 yd `strength_128` card 3.3–4.3 yd up, above the head, (0.99,
    0.44, 0), weight 0.6 → 1.0 (1033–1233) → 0 by 1633;
  - **starburst**: a small `grad6` / `lensflare1a` / `star2_32` cluster at the waist that rises
    0 → 2.56 yd and swells ×5.9 at 667 ms, orange, blue (0.25, 0.85, 1) and pale yellow;
  - **motes**: `ribbonmagic1`, 36/s, orange → white, rising.
- No state.

Bench: every layer ≤ 2.8. Proposed: the floor ring (gain 0.6) and the glyph (0.8), both
alpha-blended at level 0.6, the starburst and motes kept additive, the fire wall and glow
column dropped: **13.9**. The wall is the client's loudest layer: a 6.7 yd column of flame
that hides the wearer for most of a second, at most once every 45 s (the trinket's internal
cooldown). Alpha-blended, its large faint area dilutes the read to 4.5; additive, it does not
read at all. It is the one layer whose cut is a taste call; the bench has it.

## 9. Darkmoon Card: Blue Dragon (23688 → 23684)

The equip aura 23688 (aura 42) triggers 23684, aura 134 (mana regeneration while casting) for
15 s. **Neither has a `SpellXSpellVisual` row in either build.** Icon only; the aura band
fires for its `ManaRegenIncrease` today.

## Sustained channels

The client spends none. The proposal spends none:

- Aegis's PW:S rig is the one sustained shape, and it is an absorb (it ends when the shield is
  used up, as PW:S's does), on a once-a-match trigger. It shares the PW:S channel: one bearer
  can wear both, as a Priest-shielded bubbled Paladin wears both rigs today.
- The haste aura (Kiss, Dragonspine) and the on-use buffs (Talisman, Earthstrike, Flask) are
  one-shots on the moment they fire; the buff bar's icon carries the duration (the Moonfire
  precedent).

## What AS-305 must not do by default

- **Do not reuse `WindfuryTornado` for Hand of Justice** (finding 6).
- **Do not rely on the generic absorb route for Aegis** (finding 4).
- **The new `AttackSpeedIncrease` type** must answer `AuraApplyRoute::for_aura` (the match is
  exhaustive). `Band` keeps it consistent with every other stat buff until a build card routes
  Kiss and Dragonspine to their one-shots by name.

## Proposed build cards (not filed; awaiting sign-off)

1. **Aegis shield** (fold into AS-305): Aegis's aura routed to the PW:S rig by name.
2. **Medallion break**: the Holy launch hands on the wearer, the dispel ribbon (colour and
   level per ruling), and each CC's own visual scattering on a Medallion break (the stun
   first).
3. **Trinket one-shots**: Talisman, Earthstrike, Kiss of the Spider and Dragonspine, each
   routed by name in place of the band (`AuraApplyRoute::for_instance`), with the kept layers
   and the alpha-blended legibility layer.
4. **Hand of Justice crescents**: the extra swing fires Cheap Shot's crescent spec on the
   wearer.
5. Diamond Flask and Blue Dragon: no card (the band and the icon).
