# AS-132: Victim Hit-Reactions + Wand Attack Visuals — Client-Data Research

> Phase-1 research for AS-132, implemented by AS-144. Promoted verbatim from the
> PM session's scratchpad; the only edits are this note and the provenance
> pointers at the end. The three recipe corrections in the next section are the
> ones folded into the `wow-client-data-recipe` project memory.

All findings are from WoW Classic Era build **1.15.9.69547** (same build as the AS-9/AS-15
corpus) via wago.tools DB2 CSVs and CASC file fetches, cached under
`.db2-cache/1.15.9.69547/`. Every claim cites a table+row or file+offset. One external
non-Blizzard source is used for one purpose only — anim ID→name labels — and it is
corroborated in-build (§1.1).

## Method corrections discovered en route (read before reusing the recipe)

1. **M2 sequence records are 64 bytes, not 32** (id u16, variationIndex u16, duration_ms
   u32, movespeed f32, flags u32, frequency i16, pad u16, replay 2×u32, blendTimeIn u16,
   blendTimeOut u16, bounds 28B, variationNext i16, aliasNext u16). Verified: stride 64
   parses all 143 humanmale sequences with sane ids/durations; stride 32 fails on record 2.
2. **`SpellVisualKitEffect.EffectType == 6` IS the animation join** (`Effect` →
   `SpellVisualAnim.ID`), as the AS-132 brief said. AS-9's method note ("EffectType 6 =
   sound-like IDs") is wrong on this point: SpellVisualAnim IDs run 6510–426772 (1792
   rows), so values like 359885 that AS-9 read as sound-like are real SpellVisualAnim rows
   (e.g. SVA 359885 → LoopAnimID 46 = AttackBow). EffectType 5 is the sound. EffectType 2
   → SpellVisualKitModelAttach → SpellVisualEffectName remains the model join (AS-9's
   correction holds).
3. **`UnitBlood`'s spurt columns are SpellVisualEffectName IDs, NOT SpellVisualKit IDs.**
   Joining them as kits resolves to nonsense (meteor balls, precast hands); joining as
   SVEN IDs resolves every one of the 12 values to a `particles/bloodspurts/*.m2` model.
4. **AnimationData in this build has no Name column** (schema:
   `ID,Fallback,BehaviorTier,BehaviorID,Flags_0,Flags_1`). Names come from the community
   enum (below) and are corroborated by in-build fallback structure.

---

## 1. Victim melee hit reaction

### 1.1 The wound animation family

`AnimationData.csv` carries no names (finding, §Method 4). ID→name labels are from the
community M2 sequence enum (wowdev.wiki `M2/AnimationList`, saved at
`scratchpad/anim_names.tsv`). Two in-build corroborations that these labels are right:

- **Fallback chain**: AnimationData row 10 falls back to 9, row 9 falls back to 8, row 8
  to 0 — exactly the CombatCritical → CombatWound → StandWound → Stand ladder the enum
  names. (Rows: `8,0,0,8,…` / `9,8,0,9,…` / `10,9,0,10,…`.)
- **SpellVisualAnim rows command anim 9 at ranged-weapon impact events** (§2.5) — an
  anim id used precisely where a hit-reaction belongs.

The complete wound-related name set in the enum (searched for `wound|critical`):

| ID | Name | Relevant to era? |
|---|---|---|
| 8 | StandWound (out-of-combat flinch) | yes |
| 9 | CombatWound (in-combat flinch) | yes |
| 10 | CombatCritical (big-hit flinch) | yes |
| 237–239 | Fly* variants | flying rigs only |
| 748/749, 1164/1165, 1688–1691 | PetBattle/WADrunk/Drac variants | post-era features |

**There are exactly three victim hit-react anims in the era set. No per-weapon-type wound
anim exists in the enum** (no "Wound1H", "WoundPierce", etc.).

### 1.2 The wound anims on the character rig (humanmale.m2, fdid 119940)

MD21-chunked, m2version 274, name string `HumanMale` (sanity check passed), 143 sequences.

| Seq | Name | Duration | movespeed | Flags | Blend in/out | Variations |
|---|---|---|---|---|---|---|
| 8 | StandWound | **1000 ms** | 0 | 0x8a1 | 150/0 ms | 1 |
| 9 | CombatWound | **1000 ms** | 0 | 0x8a1 | 150/0 ms | 1 |
| 10 | CombatCritical | **1000 ms** | 0 | 0x8a1 | 150/0 ms | 1 |

Movement character: **zero root motion** (movespeed 0.0 — flinches happen in place; only
locomotion anims carry movespeed: Walk 2.5, Run 6.944, Sprint 11.667). Flag pattern on
this rig: every one-shot action (attacks, parries, wounds) is 0x8a1; every loop (Stand,
Ready*, Hold*) is 0x820 — the wound anims are one-shots.

**The 1000 ms is authored, not a normalization artifact.** Two proofs:
- The ATTACK family on the same rig is NOT uniformly 1000 ms: Attack1H/AttackUnarmed =
  1000 ms but Attack2H = 1333 ms, Attack2HL = 1333/1334/1500 ms (3 variations). The
  recipe's "attacks normalized to 1000 ms" is false as a family-wide claim on this rig.
- The wolf rig (§3) has CombatWound = **667 ms** — wound durations are per-rig.

Context rows for the bench (same rig): parries 20–23 all 1000 ms; Attack1HPierce (85)
1000 ms; AttackOff (87) 1000 ms; Stun (14) 2000 ms loop; Knockdown (121) 2000 ms with a
50 ms blend-in (the fastest blend on the rig besides SpellCastDirected's 50 ms).

### 1.3 Blood: UnitBlood → SpellVisualEffectName → particles/bloodspurts/*

`UnitBlood.csv` — **3 rows total** in the build:

| UnitBlood | Front_0 → model | Front_1 → model | Back_0 | Back_1 |
|---|---|---|---|---|
| 1 (red) | SVEN 109 → `particles/bloodspurts/bloodspurt.m2` (fdid 165398) | SVEN 164 → `bloodspurtlarge.m2` (165407) | SVEN 534 → bloodspurt | SVEN 55 → bloodspurtlarge |
| 2 (green) | SVEN 183 → `bloodspurtgreen.m2` (165404) | SVEN 184 → `bloodspurtgreenlarge.m2` (165405) | SVEN 535 → green | SVEN 537 → greenlarge |
| 3 (black) | SVEN 532 → `bloodspurtblack.m2` (165399) | SVEN 533 → `bloodspurtblacklarge.m2` (165400) | SVEN 536 → black | SVEN 538 → blacklarge |

Every SVEN row above has Scale 1, MinAllowedScale 0.01, MaxAllowedScale 100 (e.g. SVEN
row 109: `109,165398,0,1,0.00999999978,100,…`). The column structure is
[normal, large] × [front, back]; which of normal/large plays on a given hit (crit?
damage fraction?) is client logic, not expressed in these tables — stated as unknown,
not guessed. `bloodspurtblue.m2`/`bloodspurtbluelarge.m2` exist on disk (fdids
165401/165402) but **no SpellVisualEffectName row references them** — shipped but unused.

`UnitBloodLevels.csv` — 3 rows, `Violencelevel_0..2`: row 1 = (0,2,1), row 2 = (0,2,2),
row 3 = (0,3,3). These are the per-violence-preference gates; the exact semantics of the
three slots are client logic (reported raw, no interpretation).

**Who gets which blood**: `CreatureModelData.BloodID` → UnitBlood. Distribution across
all 663 model rows: BloodID 1 = 570 models, 2 = 37, 3 = 56. HumanMale (FileDataID
119940; CMD rows 49/14837/15894) → **BloodID 1** (red). Wolf (fdid 126487, CMD row 43)
→ **BloodID 1** as well.

**BloodSpurt M2 anatomy** (fdids 165398 / 165407, both 6,616 bytes, names `BloodSpurt` /
`BloodSpurtLarge`): **zero mesh vertices — pure particle models.** 4 particle emitters,
0 ribbons, 1 sequence, 3 textures (TXID order):
`spells/bloodspurtsmall01.blp`, `creature/fireelemental/glowball.blp`,
`spells/starflash_grey.blp`. Emitters: [0] the blood texture at **blendingType 2
(alpha-blend)**, plane emitter — blood is the one non-additive element (an additive red
spurt would wash out); [1],[2] glowball at blend 4 (additive), sphere emitters; [3]
starflash at blend 4, plane. Envelope: small ≈ 2.0×2.7×2.3 model units, large ≈
3.0×2.7×3.1 (bbox min/max from the MD21 header at offset 160) — a burst about
half-character height. Large differs from small only in bbox and emitter offsets; same
textures, same emitter set.

### 1.4 Does the victim visual differ by weapon type? — NO (negative finding)

What was checked, all in this build:

- **The anim set** (§1.1): one CombatWound, one CombatCritical, no weapon-typed variants
  in the whole 1778-entry enum, and only ids 8/9/10 exist on the humanmale rig.
- **`WeaponImpactSounds`** (30 rows): keyed by `WeaponSubClassID`, carries ONLY sound
  arrays — `ImpactSoundID_0..10`, `CritImpactSoundID_0..10`, `PierceImpactSoundID_0..10`,
  `PierceCritImpactSoundID_0..10`. Per-weapon differentiation at impact is entirely
  audio (including a separate crit bank), zero visual columns.
- **`UnitBlood`** is keyed by the VICTIM's model (CreatureModelData.BloodID), with no
  weapon or attacker dimension.
- **The ranged impact kits** (§2.5) differ per weapon family only in their EffectType 5
  sound (bow kit 1947 → sound 4296, thrown-axe kit 1945 → 4294, thrown-dagger kit 1946
  → 4295) while all three command the same anim 9 (CombatWound) and carry no model.

So the client's victim-side hit visual = wound anim + blood spurt chosen by the victim's
body, identical for every weapon; the weapon contributes only sound.

Note the melee trigger itself is client code: melee auto (6603) has no SpellXSpellVisual
row (§2.1), so no DB2 row "commands" the melee flinch — the data evidence that
CombatWound is the flinch anim is the fallback family plus the ranged impact kits that
do command it explicitly.

---

## 2. Wand Shoot visuals

### 2.1 Spell 5019 has NO spell-visual chain — proved, with control group

`scripts/db2_spell_sweep.py --skill-line 228` (228 = Wands, found via
`--list-skill-lines`): inventory is exactly 2 names — Shoot [5019], Wands [5009] — both
**NO-VISUAL** (`2 = 2 + 0`; property-4 verdict VACUOUS since nothing joined). Raw-CSV
confirmation: zero `SpellXSpellVisual` rows with SpellID 5019 or 5009.

This is a pattern, not a gap: **every auto-attack spell lacks a visual row** — melee auto
6603, Auto Shot 75, bow Shoot 2480, Throw 2764, wand Shoot 5019 all absent, while real
abilities have rows (control: Arcane Shot 3044 → `238055,0,3299,1,…`). Auto-attack
visuals are item-driven, below.

### 2.2 The actual chain is item-side: ItemDisplayInfo → ItemRangedDisplayInfo

`ItemDisplayInfo.ItemRangedDisplayInfoID` → `ItemRangedDisplayInfo` (23 rows; columns
`ID, CastSpellVisualID, AutoAttackSpellVisualID, QuiverFileDataID,
MissileSpellVisualEffectNameID`) → `CastSpellVisualID` is a **SpellVisual** row whose
`SpellVisualMissileSetID` → `SpellVisualMissile` → `SpellVisualEffectName` →
`ModelFileDataID` is the missile, and whose `SpellVisualEvent` rows are the caster/impact
kits. (`AutoAttackSpellVisualID`, `QuiverFileDataID` and `MissileSpellVisualEffectNameID`
are 0 on every era row.)

Weapon subclass attribution (join: ItemModifiedAppearance → ItemAppearance →
ItemDisplayInfo → IRDI, subclass from `Item.csv`, names from `ItemSparse.csv`):

| IRDI | Items | Cast SV | Missile model | Caster anim (precast → cast, via SVA) | Impact kit |
|---|---|---|---|---|---|
| 367 | **Bow** ×147 (Worn Shortbow…) | 5 | none — missile set 10633 is **sound-only** (SoundEntriesID 4222; arrow comes from ammo) | LoadBow(105)/HoldBow(109) → AttackBow(46) | **1947: anim 9 CombatWound** + sound 4296 |
| 366 | **Gun** ×125 (Dwarven Hand Cannon…) | 224 | sound-only (set 10636) | LoadRifle(106)/HoldRifle(110) → AttackRifle(49) | none |
| 373 | **Crossbow** ×38 | 743 | sound-only (set 10638, sound 4222) | LoadRifle/HoldRifle → AttackRifle | none |
| 368/375/376/380 | **Thrown** ×28 | 98/2027/2026/4081 | sound-only (3318/4222) | ReadyThrown(108) → AttackThrown(107) | 375→**1945: anim 9** + snd 4294; 376→**1946: anim 9** + snd 4295 |
| 370 | **Wand** ×49 (Fire Wand, Greater Magic Wand family…) | 225 | **`spells/firebolt_missile_low.m2`** (166135) | HoldThrown(111) → AttackThrown(107) | **none** |
| 371 | **Wand** ×40 (Shadow Wand, Gloom Wand…) | 226 | **`spells/deathcoil_missile.m2`** (165891) | HoldThrown → AttackThrown | none |
| 378 | **Wand** ×33 (Gnomish Zapper, Cookie's Stirring Rod…) | 2799 | **`spells/arcane_missile_lvl1.m2`** (165570) | HoldThrown → AttackThrown | none |
| 377 | **Wand** ×13 (Pestilent Wand, Rod of Corrosion…) | 2798 | **`spells/goobolt_missile_low.m2`** (166254) | HoldThrown → AttackThrown | none |
| 374 | **Wand** ×11 (Icefury Wand, Freezing Shard…) | 229 | **`spells/frostbolt.m2`** (166214) | HoldThrown → AttackThrown | none |
| 379 | **Wand** ×9 (Consecrated Wand, Moonbeam Wand…) | 2797 | **`spells/lightningbolt_missile.m2`** (166497) | HoldThrown → AttackThrown | none |
| 372 | **Wand** ×3 (Wand of Eternal Light…) | 228 | **`spells/holy_missile_low.m2`** (166331) | HoldThrown → AttackThrown | none |

(IRDI 369/381/382/383 map to no era item via ItemModifiedAppearance — test/NPC displays;
549 is a single fire-arrow display `item/objectcomponents/ammo/arrowfireflight_01.m2`;
587–590 are single post-era gun rows. All noted, none era-player-relevant.)

### 2.3 Answer to the design question: per-wand-family, not generic — and not sound-only

**Every era wand shoots a visible missile, and it varies by the wand's item art family:
seven school-flavored variants** (fire / shadow / arcane / nature / frost / lightning /
holy), all **reusing the standard school bolt models** at SpellVisualEffectName Scale 1,
`DestinationAttachment 34` (= Chest per AS-9's attachment map), `Attachment -1`,
`BaseMissileSpeed 0` (missile speed is not data-driven on any of these — client
constant). The variation axis is the ITEM DISPLAY (a Frost wand's display row points at
the frost visual), not the spell — the Shoot spell itself owns no visual (§2.1). The
sound-only "missiles" belong to bows/guns/crossbows/thrown, whose projectile comes from
the ammo item or is handled elsewhere — **not** to wands.

### 2.4 Missile M2 anatomy (two biggest families)

- **`firebolt_missile_low.m2`** (fdid 166135, name `FireBolt_Missile_Low`): 27 verts,
  vertex extent ≈ 1.4×1.6×1.6 units, materials blend [4,0,4] (add/opaque/add), **2
  ribbon emitters + 5 particle emitters** (the fire trail). Textures (TXID):
  `spells/clouds8x8.blp`, `creature/golemharvest/red_glow3.blp`, `spells/moltenrock.blp`,
  `spells/gradientgrey64.blp`, `spells/lavalump2.blp`.
- **`deathcoil_missile.m2`** (165891, `DeathCoil_Missile`): 98 verts, extent ≈
  0.4×0.9×0.9, materials [0,4,2,4], 2 ribbons + 1 particle emitter. Textures:
  `spells/skull.blp`, `item/objectcomponents/ammo/blue_glow2.blp`,
  `spells/purple_glow2.blp`, `particles/gradient64ba.blp`, `spells/purple_glow.blp` —
  a purple glow ball with trailing ribbons (yes, a tiny skull texture).

Both are small (≤ ~1.5 unit) glow-cores with ribbon trails — read at gameplay range as
a colored streak.

### 2.5 Impact: wands trigger NO data-driven victim reaction

The wand SpellVisuals (225/226/228/229/2797/2798/2799) have **only** precast
(StartEvent 1→2, TargetType 1) and cast (3→13, TargetType 1) event rows — **zero impact
(StartEvent 6) rows**, so no impact model, no impact sound, and no commanded victim anim
in the data. Contrast: bow SV 5 and thrown SVs 2026/2027 each carry impact rows
(StartEvent 6, duplicated TargetType 1 AND 2 — AS-9's impact-slot signature) whose kits
1945/1946/1947 hold exactly two effects: EffectType 6 → SpellVisualAnim →
**LoopAnimID 9 = CombatWound**, and EffectType 5 (sound). Whatever flinch a wand victim
shows comes from the client's generic hit-react path, same as melee.

---

## 3. Pet melee hits (wolf.m2, fdid 126487)

Same system as players, cheaper rig:

- **Blood**: CreatureModelData row 43 (wolf) → BloodID **1** — the same red
  bloodspurt/bloodspurtlarge models as humanmale. Nothing pet-specific.
- **Anims** (name string `Wolf`, 32 sequences): **CombatWound present, 667 ms**
  (one variation, flags 0x8a1); **CombatCritical present, 1000 ms**; **no StandWound**
  (id 8 absent from the rig — AnimationData's Fallback for 9 is 8 and for 8 is 0, so a
  missing 8 falls through to Stand). AttackUnarmed 1500 ms ×2 variations; Death 1833 ms.

So pets/beasts: same wound family, same blood family, per-rig durations.

---

## 4. Caster-side wand attack (what the SHOOTER shows)

Fully data-driven and currently unrepresented in ArenaSim:

- **Between shots / while auto-attacking**: precast kit 372 = SVA 359993, LoopAnimID
  **111 HoldThrown** (humanmale: 1000 ms, looped 0x820) + sound 745. The precast loop is
  the "aim/hold" — the character holds the wand up, thrown-weapon style. (Bows show
  LoadBow→HoldBow, guns LoadRifle→HoldRifle, same mechanism.)
- **On each shot**: cast kits 2970/2971/2973/2974/2975/2976 (one per school family) =
  SVA → LoopAnimID **107 AttackThrown** (humanmale: 1000 ms one-shot, blend 150 ms) +
  a per-family fire sound (EffectType 5 ids 5414/5415/5417/5419/5434/5435).
- **No hand glow**: none of the wand cast/precast kits contains an EffectType 2
  (model-attach) entry — the caster-side visual is anim + sound + the missile leaving
  from the weapon, nothing attached to hands. (Checked all kits on all 7 wand SVs; the
  only model-bearing kits in the whole §2.2 table are on the unused NPC/test rows,
  e.g. SV 313's holy hand glow.)

---

## DESIGN CONSTRAINTS (what the client actually does / does not do)

The bench must honor:

1. **One flinch family, victim-owned**: hit reaction = CombatWound (in combat), with a
   distinct CombatCritical for big hits; chosen per victim rig, played in place (zero
   root motion), one-shot with ~150 ms blend-in, duration authored per rig (human 1000 ms,
   wolf 667 ms — do not hardcode one duration for all rigs).
2. **Blood is the victim's property, not the weapon's**: 3 blood families game-wide
   (red/green/black), keyed by the victim's model; players, humanoids and beast pets are
   all red. Spurt = pure particle burst (no mesh), alpha-blended red core + additive
   glow/flash accents, ~half-character-height envelope, with a normal and a large
   variant.
3. **No per-weapon victim visual** — weapon type differentiates impact SOUND only
   (WeaponImpactSounds, incl. separate crit banks). A design that varies the victim
   visual by attacker weapon would be un-Classic.
4. **Wand missiles are school-flavored, not generic and not invisible**: 7 families
   reusing the standard fire/shadow/arcane/nature/frost/lightning/holy bolt models,
   scale 1, aimed at the chest. ArenaSim already has school-colored projectile visuals —
   the client precedent is literally "reuse the school bolt".
5. **Wand impacts carry NO impact flash and no data-driven victim anim** (unlike bow and
   thrown autos, which explicitly command CombatWound + an impact sound). A wand hit
   showing only the generic flinch is faithful; a bespoke wand impact burst is not.
6. **The wand shooter is not idle**: hold loop (HoldThrown-style) between shots, a
   thrown-style flick (AttackThrown) + fire sound per shot. Mage/Priest/Warlock/Shaman
   showing nothing while wanding is the gap; the client shows a full hold/shoot cycle.
7. **Missile speed for autos is not in the data** (BaseMissileSpeed 0 everywhere here) —
   the bench is free to pick a speed; there is no client constant to copy from DB2.

## Files / provenance

- DB2 CSVs + M2 binaries: `.db2-cache/1.15.9.69547/` (AnimationData, UnitBlood,
  UnitBloodLevels, CreatureModelData, SpellXSpellVisual, SpellVisual, SpellVisualMissile,
  SpellVisualEffectName, SpellVisualEvent, SpellVisualKit, SpellVisualKitEffect,
  SpellVisualKitModelAttach, SpellVisualAnim, WeaponImpactSounds, ItemDisplayInfo,
  ItemRangedDisplayInfo, Item, ItemAppearance, ItemModifiedAppearance, ItemSparse;
  `m2_119940.bin` humanmale, `m2_126487.bin` wolf, `m2_165398/165407.bin` bloodspurts,
  `m2_166135/165891.bin` missiles). Listfile: `.db2-cache/community-listfile.csv`.
- Anim id→name labels: the wowdev.wiki `M2/AnimationList` enum, fetched via the
  MediaWiki API (the ONLY non-Blizzard-data source in this doc; used solely for
  names, corroborated in-build per §1.1). The fetched table lived at
  `scratchpad/anim_names.tsv` in the research session and is not tracked here —
  it is a verbatim copy of that wiki page and regenerable from it.
- Sweep run: `scripts/db2_spell_sweep.py --skill-line 228` (Wands), NO-VISUAL for both
  inventory names, count identity `2 = 2 + 0`.
