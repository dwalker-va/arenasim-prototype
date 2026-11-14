# AS-335: Priest talent grants and procs — Client-Data Research

This doc covers the Priest talent effects in the talents proposal §5.2
(`docs/design/proposals/2026-10-11-talents-and-gauntlet.md`) that need a visual: Inner
Focus, Power Infusion, Pain Suppression, Holy Nova, Guardian Spirit, Mind Flay, Vampiric
Embrace, Silence, Inspiration, Blessed Recovery, Blackout and Reflective Shield. Unbreakable
Will needs no visual and is skipped (the sweep reports it NO-VISUAL at both builds).
Shadow Word: Pain and Shadowform are in the AS-333 doc
(`2026-10-17-baseline-kit-client-data.md`) and are not repeated here.

Findings come from three builds:

- Classic Era **1.15.9.69547**;
- TBC Classic **2.5.6.69795**;
- Wrath Classic **3.4.5.63697**, for Guardian Spirit only (it is in neither of the other two
  clients).

The sources are the wago.tools DB2 CSVs, joined directly, and M2 files fetched from CASC and
parsed. No wiki or prose source was used as evidence. Every row below comes from a join that
was run. Where a join could not be made, or a reading rests on inference rather than a
table, the row says so (**UNRESOLVED** or "inferred").

## Method

- **Inventory (skill-line spells).** `scripts/db2_spell_sweep.py --build <b> --skill-line 56
  78 613 --name "Inner Focus" "Power Infusion" "Pain Suppression" "Holy Nova" "Guardian
  Spirit" "Mind Flay" "Vampiric Embrace" Silence Inspiration "Blessed Recovery" Blackout
  "Reflective Shield" "Unbreakable Will" --events --caster-anims --allow-split` was run at
  both builds. Skill lines: 56 Holy, 78 Shadow Magic, 613 Discipline (594 is the Paladin's
  Holy).
  - Counts. Era: `11 = 4 + 7` (no visual: Blackout, Improved Vampiric Embrace, Inspiration,
    Unbreakable Will). TBC: `13 = 5 + 8` (adds Pain Suppression with a visual and
    Reflective Shield without one).
  - **Property 4 SPLIT on Holy Nova, both builds**, hence `--allow-split`. The split is
    real and explained: each Holy Nova rank is two spells of the same name, a damage spell
    (15237, 15430, 15431, 27799–27801, + 25331 TBC; `Effect 2` school damage, target 22) on
    visual **3643**, and a heal spell (23455, 23458, 23459, 27803–27805, + 25329 TBC;
    `Effect 10` heal, target 20 party) on visual **6882**. No `SpellEffect` row on the
    damage spell names the heal; they are paired by name and rank order (inferred). Both
    halves are joined below.
  - The run is narrowed by `--name` (and, in Era, the default `--era-cut 400000` excluded
    17 names, Pain Suppression among them: Era's only Pain Suppression is the Season of
    Discovery rune 402004). These are claims about the named spells only.
- **Helper and proc spells.** Found from `SpellName` (exact name, SpellID < 400000) and
  confirmed from `SpellEffect`, never from the brief's hints:
  - Inspiration talent ranks 14892 / 15362 / 15363 name their procs in `EffectTriggerSpell`:
    **14893 / 15357 / 15359** (aura 101 armor %, +8 / +16 / +25, target 21 ally, 15 s).
  - Blackout talent ranks 15268, 15323–15326 all trigger **15269** (aura 12 stun, 3 s).
  - Blessed Recovery talent ranks 27811 / 27815 / 27816 trigger 18350, which is "Dummy
    Trigger", so the procs **27813 / 27817 / 27818** are identified by name and effect
    (aura 8 periodic heal, period 2000 ms, 6 s, on self).
  - Vampiric Embrace 15286 (aura 4 dummy, 60 s) is paired with its party heal **15290**
    (`Effect 10` heal, target 20) by name.
  - Reflective Shield (TBC) ranks 33201–33205 are aura 4 dummies; the reflected damage is
    **33619** (`Effect 2` school damage, target 6 enemy).
  - Guardian Spirit (Wrath) 47788 (aura 118 healing received +40 %, aura 69 school absorb,
    10 s) and its save **48153** (`Effect 10` heal, self).
- **Wrath build.** `https://wago.tools/api/builds` lists products `wow_classic_era`,
  `wow_classic` (now 5.5.4, Mists), `wow_classic_titan` (3.80.x) and others; `wow_classic`
  still carries 106 3.x builds, newest **3.4.5.63697**, which was used. 47788 and 48153 are
  absent from `SpellName` at both other builds.
- **Joins.** `SpellXSpellVisual → SpellVisual → SpellVisualEvent → SpellVisualKitEffect`,
  joined on column `Effect`. Effect types: 2 model
  (`SpellVisualKitModelAttach → SpellVisualEffectName.ModelFileDataID`), 6 body anim
  (`SpellVisualAnim`), 1 procedural (`SpellProceduralEffect`), 5 sound (ignored). Event
  pairs: `(1,2)` precast loop, `(3,13)` cast, `(6,13)` one-shot impact, `(7,8)` sustained
  aura state, `(11,12)` channel.
- **Beams.** The channel kits carry `SpellProceduralEffect` **type 0**. Its `Value_0` is read
  as a `SpellChainEffects` ID (inferred): across the Era table every type-0 row sits on an
  `(11,12)` channel kit, and the IDs resolve to textures under
  `textures/spellchaineffects/` that match their spells (Mind Flay 4 → `manabeam.blp`,
  Drain Life 8 → `soulbeam.blp`, Spider Web 10 → `beam_purple.blp`).
- **Every spell here is cast time 0** (`SpellMisc.CastingTimeIndex → SpellCastTimes.Base =
  0`, all builds; Mind Flay is a 3000 ms channel). So a `(1,2)` precast row has no casting
  phase to play in, and only `(3,13)` draws on the caster (inferred, as AS-334).
- **M2 parser** (v274): the AS-333 / AS-334 `m2.py`. **None of the models in this doc has a
  `GPID` chunk**: every emitter is a sprite. Holy Nova's dome orientation needed one bone
  quaternion decoded (`quat.py`).
- **Units and conventions** (as AS-334): model units ≈ yards; particle colours RGB 0–255
  over normalised particle life, alpha 0–1; blend 2 = alpha, 4 = additive; attachments 17
  Breath (mouth; community list, matching the model name), 19 Base (feet), 20 Head, 21/22
  spell hands, 34 Chest. Body anims by the standard `AnimationData` IDs (the table has no
  name column): 9 CombatWound, 14 Stun, 52/54 ReadySpellOmni/SpellCastOmni, 53
  SpellCastDirected, 124 ChannelCastDirected.
- **Scripts** are under the AS-335 scratchpad (`as335/`): `db.py`, `chain.py`, `reverse.py`,
  `proctypes.py`, `m2.py`, `names.py`, `users.py` (copied from AS-334), plus `misc.py`
  (cast time and duration) and `quat.py` (bone rotations). Outputs: `sweep_<build>.txt`,
  `chain_<build>.txt` (three builds), `names.txt`, `users.txt`,
  `models/<build>_<fdid>.txt`.

## Summary table

| Effect | Client spell → visual | Cast side (instant: `(3,13)` only) | Landing (one-shot) | Sustained `(7,8)` | Era vs TBC |
|---|---|---|---|---|---|
| Inner Focus | 14751 → **4372** | kit 425: `magic_cast_hand.m2` both hands (**shipped, Moonfire's**), anim 54 | kit 3772: `innerfocus_impact_chest.m2` @Chest (shared with Divine Favor), 400 ms | **kit 3773: `magic_precast_hand.m2` on both hands, on the Priest, until the buff is spent** | (7,8) TargetType 2 only |
| Power Infusion | 10060 → **7553** | kit 191: `holy_precast_high_hand.m2` both hands, anim 54 | — | **kit 3769: `arcanepower_state_chest.m2` @Chest on the ally, 15 s (Arcane Power's state)** | (7,8) TargetType 2 only |
| Pain Suppression | TBC 33206 → **8039**; Era: none (SoD rune 402004 only) | kit 165: `holy_precast_uber_base.m2` @Base + high hands, anim 54 (Consecration's cast kit) | — | **kit 6915: `painsuppression_state.m2` @Base on the ally, 8 s** | TBC only |
| Holy Nova (damage) | 15237… → **3643** | kit 3154: `holynova_impact_base.m2` @Base (a gold dome, 0 → 6.2 yd radius in 767 ms) + high hands, anim 54 | kit 3153: `holy_impactdd_high_chest.m2` @Chest on each enemy (Holy Wrath's model) | none | same |
| Holy Nova (heal) | 23455… → **6882** | — | Era kit 232: `heal_low_base.m2` @Base (**shipped: Heal / Holy Shock heal**); TBC kit 431: `holylight_impact_head.m2` @Head (Prayer of Healing's) | none | **different landing** |
| Guardian Spirit (Wrath) | 47788 → **12114** | kit 270: `holy_precast_low_hand.m2` (**shipped**), anim 54 | — | **kit 11186: `priest_guardianspirit.m2` @Base on the ally, 10 s (gold halo and wings)** | Wrath only |
| Guardian Spirit save | 48153 → **12115** | — | kit 232: `heal_low_base.m2` @Base (**shipped**) | none | Wrath only |
| Mind Flay | 15407… → **277** (Drain Mana's) | kit 429: `manafunnel_impact_chest.m2` @right hand, anim 124 | — | **kit 814: same model @Chest on the victim; kit 430 `(11,12)`: same model @hand, anim 124, plus a beam (`SpellChainEffects` 4, `manabeam.blp`)** | (7,8) TargetType 2 only |
| Mind Flay slow | same spell (aura 33) | — | — | no separate visual | — |
| Vampiric Embrace | 15286 → **3582** | kit 218: shadow hands (**shipped**), anim 54 | kit 3109: `soothingkiss_impact_head.m2` @Chest (Death Pact's) | **none** | same |
| VE party heal | 15290 → **3542** | — | kit 3072: `vampiricembrace_state_base.m2` @Base on each healed ally (a violet recolour of `heal_low_base.m2`) | none | same |
| Silence | 15487 → **179** (shared by 11 silences) | kit 418: shadow hands (**shipped model**), anim 53 | — | **kit 297: `shadowwordsilence_breath.m2` @Breath on the victim, 5 s** | (7,8) TargetType 2 only |
| Inspiration | 14893 / 15357 / 15359 | — | — | — | **NO-VISUAL** (no `SpellXSpellVisual` row, both builds) |
| Blessed Recovery | 27813 / 27817 / 27818 → **7548** | — | kit 435: `renew_chest.m2` @Chest (Renew's and Greater Heal's) | **none** | same |
| Blackout | 15269 → **2816** (the generic "Stun") | — | — | **kit 349: `stunswirl_state_head.m2` @Head + victim anim 14 (Stun), 3 s: the same kit as Hammer of Justice and Kidney Shot** | (7,8) TargetType 2 only |
| Reflective Shield | TBC 33619 → **8383**; talent ranks NO-VISUAL | — | kit 121: `holy_impactdd_low_chest.m2` @Chest + anim 9 on the attacker (Consecration's, Judgement's) | none | TBC only |

### Six findings that shape the work

1. **Seven of these effects hold a sustained state, more than any class card so far.** On
   allies: Power Infusion (15 s), Pain Suppression (8 s) and Guardian Spirit (10 s). On the
   caster: Inner Focus (glowing hands until spent). On victims: Mind Flay (3 s, with a
   beam), Silence (5 s) and Blackout (3 s). Blackout's state is the generic stun kit we
   already answer for HoJ and Kidney Shot, so it costs nothing new.
2. **Five effects draw nothing, or nothing that lasts.** Inspiration has no visual at all.
   Vampiric Embrace's 60-second debuff has no state, only a one-shot on cast. Blessed
   Recovery's HoT, Holy Nova and Reflective Shield are one-shots. All talent ranks
   themselves are NO-VISUAL.
3. **Four effects land with visuals we already ship.** Guardian Spirit's save heal and Era's
   Holy Nova heal are kit 232, `heal_low_base.m2`, the shipped Heal / Holy Shock heal
   landing (`heal_impact.rs`). Guardian Spirit's cast is kit 270, the shipped holy cast
   hands (`heal_cast.rs`). Inner Focus's cast hands are `magic_cast_hand.m2`, Moonfire's
   model (`moonfire.rs`). Vampiric Embrace and Silence cast with the shipped shadow hands.
4. **Mind Flay is the only beam, and the client has one.** Its `(11,12)` channel kit
   carries a chain effect (`manabeam.blp`, 0.25 yd wide) between the bodies, plus the same
   flickering blue star at the Priest's hand and the victim's chest. The whole visual is
   Drain Mana's. Our Drain Life beam is authored, not client-derived.
5. **The Discipline and Holy grants share one cast vocabulary.** Power Infusion, Holy Nova
   and Pain Suppression all cast with `holy_precast_high_hand.m2`, a bigger variant of the
   shipped low hand (0.88 yd glow, three gold ribbons). Pain Suppression adds a gold ground
   swirl (Consecration's cast kit).
6. **Holy Nova changes landing between builds.** Era heals with the shipped
   `heal_low_base.m2` at the feet; TBC switches to Prayer of Healing's
   `holylight_impact_head.m2`, a 2.5 s fall of gold stars and cloud puffs from the head.

---

## 1. Inner Focus

**Chain:** Inner Focus 14751 (aura 108 −100 % misc 14 = mana cost; aura 107 +25 misc 7 =
crit; duration −1, until spent) → visual **4372**. In TBC the visual is shared with
Relentlessness (37601).

| Event | TargetType | Kit | Contents |
|---|---|---|---|
| (1,2) precast | 1 | 266 | `magic_precast_hand.m2` @21/@22, anim 52 (never plays: instant) |
| (3,13) cast | 1 | 425 | `spells/magic_cast_hand.m2` (fdid 166524, SVEN 392) @21/@22, anim 54 |
| (6,13) impact | 1 and 2 | 3772 | `spells/innerfocus_impact_chest.m2` (166419, SVEN 2123) @34 Chest |
| (7,8) state | 1 and 2 (Era) / **2 only** (TBC) | 3773 | `spells/magic_precast_hand.m2` (166526, SVEN 272) @21 **and** @22 |

- Kit 425 is shared with 27 spells (Evasion, Fade, Invisibility, Divine Favor…); kit 3772
  only with Divine Favor; kit 3773 is Inner Focus's alone in Era (Relentlessness in TBC).
- `magic_cast_hand.m2` is the model of Moonfire's cast kit 730, already shipped
  (`heal_cast.rs`, `moonfire.rs`). Era and TBC copies are identical.

### `innerfocus_impact_chest.m2` (@Chest, one-shot)

- **Sequence** 400 ms. No mesh. All blend 4.
- **Four converging points.** Bones start at (±0.74, ±0.74) yd in the vertical plane and
  cross the centre to (∓0.37, ∓0.37) at 400 ms. Each trails a **ribbon** (`gradient128`,
  50 edges/s, edge life 0.5 s, width 0.17 yd, colour `(0, 121, 198)`, alpha 1) and a
  **flare emitter** (70/s, life 0.8 s, speed 0.56, size **0.33 → 0.08 → 0.03 yd**, colour
  `(16, 29, 126)` → `(44, 155, 225)` → `(185, 172, 223)`).
- **Centre.** P1 `gradient64flipa` 60/s from 200 to 400 ms, speed 3.3, life 0.8 s, size 0
  → 0.14 → 0.03 yd, `(88, 0, 248)` → `(255, 0, 0)` → `(98, 181, 239)`. P0 `flare` bursts at
  340 ms (100/s for 60 ms), speed 2.2, size 0.03 → 0.06 yd, blue → yellow → red.
- **Reading:** four blue comets streak in from the corners and cross at the chest in 0.4 s,
  with a small spark pop at the end.
- **Era vs TBC:** the TBC copy does not re-enable the corner emitters at 400 ms. Otherwise
  identical.

### `magic_precast_hand.m2` (@each hand, the sustained state)

- **Sequence** 1000 ms. Two flat quads (`cyan_glow3`, blend 4), half-widths 0.37 and 0.20
  yd, so a glow about **0.74 yd across**, pulsing in scale 1.15 → 0.95 → 1.15 on a 533 ms
  loop.
- **P0** `cyan_glow3` motes: 11/s, life 0.8 s, speed 0.19, size 0.03 → 0.08 → 0.03 yd,
  white → white → violet `(162, 42, 231)`, alpha 0.25 → 1 → 0.04.
- **P1** `shockwave4white` rings: 10/s (off 667–1000 ms), life 0.4 s, size **0.14 → 0.28
  → 0.56 yd**, `(140, 60, 231)` → `(170, 84, 237)` → white.
- **Reading:** a violet-cyan glow in each palm, pulsing twice a second, shedding motes and
  small expanding rings. It is the arcane precast hand (kit 266 has 257 users), held as a
  state.
- Era vs TBC: identical but the bounding box.

### What this means for design (Inner Focus)

- Cast with Moonfire's shipped arcane hands, then a 0.4 s blue converging flash at the
  chest.
- **The buff is a caster sustained state on both hands** for as long as it waits to be
  spent. In our design it is spent by the next cast, so it lasts from cast to cast: often
  under 2 s, at most the cooldown.
- The hand glow sits on the same hands every cast already uses, so it competes with the
  cast hands rather than the body channels.

---

## 2. Power Infusion

**Chain:** Power Infusion 10060 (Era: aura 136 +20 % healing done, aura 79 +20 % damage
done, misc 126; TBC: aura 65 +20 % spell haste, aura 72 −20 % mana cost; target 21 ally;
15 s) → visual **7553**, its own in Era (TBC adds an NPC copy, 37274).

| Event | TargetType | Kit | Contents |
|---|---|---|---|
| (1,2) precast | 1 | 193 | `holy_precast_high_base.m2` @19 + `holy_precast_high_hand.m2` @21/@22, anim 52 (never plays) |
| (3,13) cast | 1 | 191 | `spells/holy_precast_high_hand.m2` (166335, SVEN 131) @21/@22, anim 54 |
| (7,8) state | 1 and 2 (Era) / **2 only** (TBC) | 3769 | `spells/arcanepower_state_chest.m2` (165589, SVEN 2122) @34 Chest |

- **No `(6,13)` row: nothing lands.** The ally shows only the state.
- Kit 3769 is **Arcane Power's state** (and Mind Quickening, and AV insignia spells); in TBC
  also Arcane Power State and others. ArenaSim ships neither.

### `holy_precast_high_hand.m2` (@each hand)

- **Sequence** 1000 ms. Two quads, `yellow_glow3a` (half-width **0.44 yd**, weight 0.7) and
  `genericglow2c` (0.27 yd), both pulsing 1.15 → 0.95 on a 533 ms loop.
- **Three ribbons** (texture `genericglow2b`, blend 4) orbiting the hand on short global
  loops (367–500 ms bone offsets): 50 edges/s, edge life 0.4 / 0.6 / 0.5 s, width **0.33 yd**, gold `(252, 220, 42)`,
  `(253, 205, 74)`, `(251, 218, 37)`, alpha 0.8.
- **P0** `grad2db` sparks: 30/s fading to 0 over an 833 ms loop, speed 0.67, life 1 s,
  size 0.007 → 0.011 yd, yellow → white → cyan.
- **Reading:** a gold palm glow with three short gold ribbons circling it. The shipped
  `holy_precast_low_hand.m2` is the smaller sibling.
- Era vs TBC: identical but the bounding box.

### `arcanepower_state_chest.m2` (@Chest on the ally)

- **Sequences:** 0 Stand **1667 ms**, 158 1000 ms, 159 1000 ms (end: the ring layers' alpha
  fades 1 → 0). Global loops 1067 / 1100 / 100 / 67 / 133 / 1000 / 433 / 567 ms.
- **Mesh**, all blend 4:
  - an electric `zap1` lattice (156 verts, about 1.7 × 1.5 × 1.6 yd) whose texture weight
    flickers 0 → 0.73 → 0 three times in an 1100 ms loop;
  - two `blue_glow2` discs about **3.2 yd** and **4.6 yd** across (weight 0.1 ↔ 0.5 on a
    1000 ms loop) and a 2.2 yd disc;
  - a `star7b` disc 3.4 yd across and a horizontal `star6` streak **6.1 × 0.3 yd**, both
    visible only in Stand until 1334 ms.
  - Bones pulse in scale 1 → 0.62 → 1.16 → 0.5 → 1 (433 ms loop) and 1 → 0.69 → 1.29 →
    0.69 (567 ms loop).
- **P0** `zap1` sparks: 60/s (off 1267–1667 ms in Stand, off in 158/159), life 0.2 s, speed
  3.3, size **0.42 → 0.28 → 0.03 yd**, white.
- **Reading:** a crackling blue-white electric aura at the chest, with wide pale-blue glow
  discs that throb. It is loud: the discs alone span 4.6 yd.
- Era vs TBC: the TBC copy does not re-enable the spark emitter at 1667 ms.

### What this means for design (Power Infusion)

- Gold hands on the Priest; nothing lands; **the ally carries a sustained chest state for
  15 s** (an ally channel cost).
- The state is borrowed from Arcane Power and is blue and electric, not holy. If the client's
  size (4.6 yd discs) is kept, it is the biggest state on a body in this doc.

---

## 3. Pain Suppression (TBC)

**Era has no Pain Suppression.** The Era client's only spell of that name is the Season of
Discovery rune 402004 (above the era cut), which uses the same kit 6915. The model file
exists in the Era client for that reason.

**Chain (TBC):** Pain Suppression 33206 (aura 87 −40 % damage taken, misc 127; aura 588; 8
s; target 21 ally) → visual **8039**, its own. 44416 (`Effect 125`) has no visual.

| Event | TargetType | Kit | Contents |
|---|---|---|---|
| (3,13) cast | 1 | 165 | `spells/holy_precast_uber_base.m2` (166338, SVEN 211) @19 Base + `holy_precast_high_hand.m2` @21/@22, anim 54 |
| (7,8) state | **2** | 6915 | `spells/painsuppression_state.m2` (166631, SVEN 3210) @19 Base |

- No `(1,2)` row and no `(6,13)` row.
- Kit 165 is Consecration's cast kit (also Lightwell, Summon Warhorse, blessings in TBC).
  Kit 6915 is shared only with Ooze Channel (42483).

### `holy_precast_uber_base.m2` (@feet, cast)

- **Sequence** 1067 ms; global loops 833 / 733 / 4000 ms.
- **Mesh**, blend 4: a `shockwave10c` ground ring 1.38 yd across, scaling **1.75 → 1** over
  833 ms (2.4 → 1.4 yd), colour orange `(255, 132, 0)` → gold `(255, 216, 0)` at 333 ms →
  violet at 833 ms, weight 0 → 1 → 0; a `yellow_glowdgold` disc 2.1 yd; and two swirl meshes
  (`ribbonblur1bd_gold` 1.2 yd wide, 0.45 yd tall; `ribbonblur1bea` **4 × 3.4 yd, 2 yd
  tall**), scaling 0.62 → 1.46 on a 733 ms loop.
- **Emitters:** gold sparks drawn inward (P0 300/s, P4 400/s, speed −1.7, life 0.4–0.7 s),
  sparks thrown out (P2 150/s, P3 125/s, speed 1.4–1.7), and spinning `star5a` stars (P1,
  P5 15/s, speed −2.2, spin ±11). Colours gold → white → cyan.
- **Reading:** a gold ground ring that contracts while a tall gold swirl rises around the
  caster, sparks pouring in.
- Era vs TBC: identical but the bounding box.

### `painsuppression_state.m2` (@feet on the ally, sustained)

- **Sequences:** 127 (867 ms) and 0 Stand (867 ms), 158 (**867 ms**, the loop), 1 and 159
  (600 ms, end). Read as start / loop / end (inferred; see Unresolved 2).
- **Mesh:** three upright quads in front of the body (offset 0.22 / 0.75 yd), all blend 4:
  - `shockwave10d`, gold `(212, 176, 34)`, 2.3 × 2.9 yd;
  - `shockwave10b`, pale yellow `(254, 240, 81)`, 2.4 × 3.1 yd;
  - `genericglow2b1`, teal `(34, 236, 219)`, **3.4 × 4.4 yd**.
  - In the start sequences each flashes to weight 1 at 467 ms and settles at 0.12–0.15. In
    158 they flicker between 0 and 0.4. The quad bone squashes and stretches (y 0.52 → 1.09
    → 0.9).
- **Emitters**, all blend 4, centred about 1.3 yd up: `beerbubble` bubbles 10/s growing to
  **2.2 yd**, alpha 0.61 → 0; faint `clouds8x8` 300/s, alpha ≤ 0.12, size to 1.67 yd;
  `glow` streaks pulled inward (50/s, speed −3.9 from a 2.8 yd line; 50/s, speed −5.6 from
  1.4 yd), yellow → cyan → blue.
- **Reading:** a tall gold-and-teal shell standing over the ally, with bubbles swelling and
  light streaks drawn into it.

### What this means for design (Pain Suppression)

- Cast: gold high hands plus a gold ground swirl at the Priest's feet. Nothing lands.
- **The ally carries a sustained state for 8 s**: a body-height shell in front of the body,
  about 4.4 yd tall at its largest layer. It encloses the body more than any other state
  here; enclosure is this game's Incapacitate language (`hard_cc.rs`), so that reading
  needs a design decision.
- Pain Suppression is TBC-only; the design already takes it from TBC.

---

## 4. Holy Nova

### The damage half (15237 and ranks)

**Chain:** Holy Nova 15237, 15430, 15431, 27799–27801 (+ 25331 TBC) → visual **3643**
(shared with Cleanse Nova and NPC Holy Novas). The ranks named 15434 / 27821–27823 and 3046
are rank-learning spells (`Effect 36`) on visual 107.

| Event | TargetType | Kit | Contents |
|---|---|---|---|
| (1,2) precast | 1 | 193 | holy precast base + high hands, anim 52 (never plays) |
| (3,13) cast | 1 | 3154 | `spells/holynova_impact_base.m2` (166350, SVEN 1722) @19 Base + `holy_precast_high_hand.m2` @21/@22, anim 54 |
| (6,13) impact | 1 and 2 | 3153 | `spells/holy_impactdd_high_chest.m2` (166325, SVEN 125) @34 Chest |

- **The nova is the cast kit**, at the Priest's feet. Each struck enemy then shows the
  impact kit. Kit 3153 is shared with Cleanse Nova; the model is **Holy Wrath's** impact
  (kit 211, visual 126). ArenaSim ships neither.

### `holynova_impact_base.m2` (@Priest's feet)

- **Sequence** 767 ms. Mesh: a **hemisphere shell of radius 6.22 yd** (487 verts), on a bone
  whose scale runs **0 → 1 over 767 ms**. Colour gold `(255, 216, 0)`; two batches, a
  `white8x8` layer (weight 0 → **0.2 at 134 ms** → 0) and an `armorreflect3` layer (0 →
  **0.5** → 0), blend 4.
- **P0** `glowstar` sparks: rate 0 → **500/s at 300 ms** → 0 at 767 ms, from a square spawn
  area growing **0 → 7.2 yd**, speed 1.67, life 1 s, spin 4, size **0.56 → 0.28 → 0.03
  yd**, `(255, 252, 222)` → `(255, 249, 98)` → `(255, 222, 39)`, alpha 0.39 → 1 → 0.
- **Reading:** a gold shell bursts outward from the Priest to 12.4 yd across in 0.77 s,
  brightest in its first 0.13 s, trailed by a ring of gold stars. The game's range is 10
  yd; the shell's radius is 6.2.
- **Orientation:** the hemisphere's open face is the model's x = 0 plane, not z = 0. Bone 1
  carries a constant 90° rotation about x (quaternion (0.707, 0, 0, 0.707)), which does not
  move that face. Whether the client stands the dome up is UNRESOLVED (Unresolved 4).
- Era vs TBC: identical but the bounding radius.

### `holy_impactdd_high_chest.m2` (@Chest on each enemy)

- **Sequence** 934 ms. A flat `lensflare`-style quad 2.6 yd across, colour `(255, 241,
  200)`, weight 0 → **1 at 134–267 ms** → 0 at 800 ms.
- **Emitters** (14, all blend 4):
  - two `starflashyellow` showers: up to **250/s** at 200–267 ms, speed up to 4.4,
    gravity 6.9, life 0.75 s, size 0.11 → 0.03 yd, white;
  - `glowstar_yellow` flashes: P5 40/s to 500 ms, size 0.28 → **1.25** → 0.42 yd; **P6
    15/s for 200 ms, size 0.83 → 3.33 → 0.03 yd** (the big flash);
  - `starflashyellowa` 75/s drawn inward, size to 2.2 yd;
  - gold ribbon-texture streaks (P3, P4 up to 250/s, tail 5; P8, P9 20/s, speed 3.3);
  - four `clouds8x8` puff layers, 40/s from 200 to 300 ms, brown-gold `(150, 145, 0)` →
    `(139, 88, 16)` → `(212, 131, 58)`, alpha ≤ 0.29, size up to 0.75 yd.
- **Reading:** a big gold starburst on the chest, about 3.3 yd at its peak, with sparks
  falling and a little dust, gone by 0.93 s.
- Era vs TBC: one speed key differs.

### The heal half (23455 and ranks)

**Chain:** Holy Nova 23455, 23458, 23459, 27803–27805 (+ 25329 TBC) → visual **6882**, Holy
Nova's own.

| Build | Event | TargetType | Kit | Contents |
|---|---|---|---|---|
| Era | (6,13) | 1 and 2 | **232** | `spells/heal_low_base.m2` (166292, SVEN 249) @19 Base |
| TBC | (6,13) | 1 and 2 | **431** | `spells/holylight_impact_head.m2` (166348, SVEN 410) @**20 Head** |

- **Era's heal landing is the shipped Heal / Holy Shock heal** (kit 232,
  `heal_impact.rs`; the model is described in `2026-09-06-heal-impact-client-data.md`).
- **TBC's** kit 431 is Prayer of Healing's (also Healing Circle, Mass Healing, Divine
  Light).

### `holylight_impact_head.m2` (@Head, TBC heal)

- **Sequence** 2500 ms. One horizontal `yellow_glow3a` quad about 1.4 yd across, 2.17 yd
  below the head attachment (around the feet), weight 0 → 1 at 300 ms, held to 1234 ms, 0
  by 1667 ms.
- **Emitters**, blend 4: `star5a` stars (P0 8.3/s, P1 10/s, falling: speed −0.6 to −0.8,
  life 0.8–1.1 s, size to 0.07–0.08 yd, `(255, 186, 77)` → `(253, 255, 200)` →
  `(246, 255, 0)`); three `clouds8x8fade` puff layers (7–10/s, speed −0.56, life 1.5 s,
  size **0.33 → 0.56 → 1.11 yd**, alpha ≤ 0.13); and `yellow_glow3` sparks thrown up (13.2/s
  until 767 ms, speed 1.67). All rates fall to 0 by about 1500 ms.
- **Reading:** a soft fall of gold stars and faint cloud from the head down, with a gold
  glow at the feet, over about 2.5 s. Gentler than the shipped Holy Light landing
  (`holylight_low_head.m2`, a different model).
- Era copy identical (the model exists in both builds).

### What this means for design (Holy Nova)

- **One cast, three looks:** gold high hands; a gold shell bursting out from the Priest's
  feet (0.77 s); a gold starburst on each enemy hit (0.93 s); a heal landing on each
  healed ally.
- **Pick a build for the heal.** Era's is the shipped `heal_low_base.m2` landing, free.
  TBC's is a new 2.5 s head-down fall.
- No sustained state on anyone.
- The shell is 12.4 yd across at its end against our 10 yd radius; scale it to the radius
  if the shell is meant to show the reach.

---

## 5. Guardian Spirit (Wrath Classic 3.4.5.63697)

**Not in Era or TBC.** Neither 47788 nor 48153 is in `SpellName` at 1.15.9.69547 or
2.5.6.69795.

**Chain:** Guardian Spirit 47788 (aura 118 +40 % healing received; aura 69 absorb; 10 s;
target 21 ally) → visual **12114**. The save, Guardian Spirit 48153 (`Effect 10` heal, 50
%, self) → visual **12115**.

| Spell | Event | TargetType | Kit | Contents |
|---|---|---|---|---|
| 47788 | (1,2) precast | 1 | 11405 | `holy_precast_med_hand.m2` @21/@22, anim 52 (never plays) |
| 47788 | (3,13) cast | 1 | **270** | `spells/holy_precast_low_hand.m2` (166336, SVEN 135) @21/@22, anim 54 |
| 47788 | (7,8) state | **2** | 11186 | `spells/priest_guardianspirit.m2` (240994, SVEN 4726) @19 Base |
| 48153 | (6,13) impact | 1 and 2 | **232** | `spells/heal_low_base.m2` (166292, SVEN 249) @19 Base |

- **The cast is kit 270, the shipped holy cast hands** (the Flash Heal family,
  `heal_cast.rs`).
- **The save is kit 232, the shipped Heal / Holy Shock heal landing.** The client marks the
  moment a death is prevented with nothing beyond that ordinary heal; the state model has
  no end-burst sequence keyed to the save (inferred: the save is a separate spell, and only
  48153 carries the impact).
- The Wrath copy of `heal_low_base.m2` differs from TBC's only in its emitter-enabled keys
  and one rate key.

### `priest_guardianspirit.m2` (@feet on the ally, sustained)

- **Sequences:** 0 Stand **1800 ms** (onset), 158 **466 ms** (the loop), 159 433 ms (end:
  rates fall to 0). No mesh; every emitter sits **1.71–1.98 yd above the feet** (behind and
  above the shoulders). Eleven emitters, all sprites:

| # | Texture | Blend | Onset (Stand) | Loop (158) | Life s | Size (yd) | Colour |
|---|---|---|---|---|---|---|---|
| P0 | `t_vfx_flar_blur_64` sparks | 4 | 0 → 200/s at 200 ms → 100/s | 100/s | 0.5 | 0.06 → 0.03, speed 2.8 | white → `(235, 210, 0)` → `(183, 183, 0)` |
| P1 | `t_vfx_flare05_dark_64` halo | **2** | 20/s burst at 33 ms → 2/s | 2/s | 1.0 | **1.65–1.71** | grey `(172…212)` |
| P2 | `t_vfx_smoke_cigarette2` | 2 | 20/s 200–1767 ms | — | 0.5 | 0.96 → 1.53 | white → `(183, 175, 10)` |
| P3, P4 | `t_vfx_flare_long` flash | 4 | 20/s at 567 ms only | — | 0.5 | 0.4 × 0.14 → **1.3–1.4** | white → `(255, 211, 107)` |
| P5 | `t_vfx_ring_bow` rings | 4 | 50/s at 533 ms | 3/s | 1.0 | 0.28 → **1.04** | white → `(255, 255, 127)` |
| P6 | smoke | 4 | 50/s → 20/s | 20/s | 0.3 | 0.84 → 1.41 | white → `(252, 251, 92)` → `(247, 192, 107)` |
| P7 | `t_vfx_hero_circle` | 4 | 5/s 333–433 ms | — | 0.5 | 0.56 | white → `(253, 221, 8)` |
| P8 | `t_vfx_hero_aura_02_h` | 4 | 0 → 100/s at 367 ms → 20/s | 20/s | 0.5 | **0.96 × 0.83 → 3.9 × 0.78** (widening) | white → `(249, 240, 170)` |
| P9 | `t_vfx_flare_long` streak | 4 | 10/s → 5/s | 5/s | 0.5 | **1.66 × 0.52 → 2.6 × 0.11** | white → `(255, 211, 107)` |
| P10 | **`wings3`** | 4 | 5/s → 3/s | 3/s | 1.0 | **2.2 × 0.48 → 2.8 × 0.53** | white → `(255, 170, 2)` |

- **Reading:** a gold halo above the ally's shoulders with a wide pair of wings, a dark
  circle behind it, rings and a widening horizontal aura band, all sparking gold. The
  onset (1.8 s) flashes rings and long flares at about 0.55 s; the loop is a calmer
  re-emission every 0.47 s.

### What this means for design (Guardian Spirit)

- **Cast and save both reuse shipped visuals** (kit 270 hands; kit 232 heal landing).
- **The ally carries a sustained state for 10 s**: the halo-and-wings over the shoulders.
  This is the only new art the effect needs, and it is a Wrath asset (its textures are
  the `t_vfx_*` family, newer than anything else in our set).
- **The client draws no distinct "saved" moment.** The design's "'saved' logged" footprint
  has no client counterpart beyond the ordinary heal; anything louder would be authored.

---

## 6. Mind Flay

**Chain:** Mind Flay 15407, 17311–17314, 18807 (+ 25387 TBC): aura 3 periodic damage
(period 1000 ms) and **aura 33 slow −50 %**, 3000 ms channel → visual **277**. Visual 277
is **Drain Mana's** (5138, 6226, 11703…), shared with Curse of Mending and NPC drains; TBC
has 49 users.

| Event | TargetType | Kit | Contents |
|---|---|---|---|
| (3,13) cast | 1 | 429 | `spells/manafunnel_impact_chest.m2` (166538, SVEN 271) @**21 only**, anim **124** |
| (7,8) state | 1 and 2 (Era) / **2 only** (TBC) | 814 | the same model @34 Chest |
| (11,12) channel | 2 | 430 | the same model @21, anim 124; **procedural type 0, values 4 / 1 / 1 / 0** (the beam) |

- No `(1,2)` row. The slow is the same spell's aura: **no separate slow visual**. The victim
  shows only the `(7,8)` state for the channel.
- TBC kit 814 carries `Flags_0` 1048579 against Era's 1048576 (Unresolved 6).
- Wrath 3.4.5 swaps the beam to chain 750: a composite of five sub-chains, still
  `manabeam.blp`, 0.15 yd wide, alpha 51/255, flickering.

### The beam (`SpellChainEffects` 4, inferred link)

| Field | Era | TBC |
|---|---|---|
| Texture | `textures/spellchaineffects/manabeam.blp` (186202) | same |
| StartWidth / EndWidth | **0.25 / 0.25 yd** | same |
| AvgSegLen | 2.78 yd | same |
| SegDuration / SegDelay | 1000 / 200 ms | same |
| NoiseScale | 0.01 | same |
| TexCoordScale | −0.5 | **−2** |
| JointSmoothness | 0 | **0.75** |
| TimePerFlipFrame | 0.066 s (variance 0.01) | same |
| Colour, alpha | white, 255 | same |
| BlendMode | 3 | same |

Drain Life's chain 8 has the same geometry with `soulbeam.blp` (and, in TBC, a pulse:
`PulseSpeed` −24, on 4, fade 1). Texture pixels were not decoded.

### `manafunnel_impact_chest.m2` (hand, channel and victim)

- **Sequences:** 0 Stand **1067 ms**, 158 1066 ms, 159 1067 ms (end: everything fades in
  333 ms). Global loop 267 ms.
- **Mesh:** two `starflash_grey` quads in the vertical plane, **2.38 yd** and **1.1 yd**
  across, tinted light blue `(49, 180, 255)` (outer) and white (inner), blend 4. Their bone
  scale flickers 0.99 ↔ **1.25** in eight steps every **267 ms**.
- **P0** `flare` burst: 20/s, sphere, speed 1.67, life 0.7 s, size 0.08 → 0.06 → 0.003 yd,
  `(45, 20, 139)` → `(0, 42, 255)` → black.
- **P1** `gradient64flipa` wisps: 30/s, **blend 2 (alpha)**, speed 2.2, life 0.5 s, spin
  −2, size 0 → 0.14 → 0.03 yd, **black**.
- **Reading:** a flickering pale-blue star, 2.4 yd across, throwing out dark specks and a
  few blue sparks. The same model sits at the Priest's casting hand and on the victim's
  chest, joined by the beam.
- Era vs TBC: identical but the bounding box.

### What this means for design (Mind Flay)

- **Three sustained pieces for the channel's 3 s:** the star at the Priest's right hand
  (a caster channel), the star on the victim's chest (a victim channel), and the beam
  between them.
- The slow has no visual of its own in the client. If our shipped slow ring
  (`slow_ring.rs`) also shows, the victim carries both.
- Our Drain Life beam is an authored purple cylinder. The client's channel beams are chain
  effects of one shape (0.25 yd, 2.78 yd segments); Mind Flay and Drain Life differ only in
  texture.

---

## 7. Vampiric Embrace

### The debuff (15286)

**Chain:** Vampiric Embrace 15286 (aura 4 dummy; 20 % Era, 15 % TBC; 60 s; target 6 enemy)
→ visual **3582**, shared with Death Pact (and Vampiric Touch in TBC).

| Event | TargetType | Kit | Contents |
|---|---|---|---|
| (1,2) precast | 1 | 217 | shadow precast hands, anim 52 (never plays) |
| (3,13) cast | 1 | 218 | `spells/shadow_precast_low_hand.m2` (166807) @21/@22, anim 54 |
| (6,13) impact | 1 and 2 | 3109 | `spells/soothingkiss_impact_head.m2` (166922, SVEN 1402) @34 Chest |

- **No `(7,8)` row: the 60-second debuff draws nothing after it lands.** Only the icon
  remains.
- Kits 217/218 are the shadow hands AS-333 records as shipped (the Warlock's).

### `soothingkiss_impact_head.m2` (@Chest, one-shot)

- **Sequences** 2000 ms (Stand), 158 5000 ms. No mesh. One emitter: `tail_dust3`, blend 4,
  sphere, **80/s** for 1967 ms, speed **−1.67 (inward)**, life 0.5 s, size **0.83 → 0.42 →
  0.14 yd**, `(0, 0, 196)` → `(101, 22, 198)` → `(212, 5, 195)`, alpha 0 → 1 → 0.78.
- **Reading:** a 2 s shrinking ball of blue-to-magenta dust collapsing into the chest.
- Era vs TBC: identical.

### The party heal (15290)

**Chain:** Vampiric Embrace 15290 (`Effect 10` heal, target 20 party; fires per damage
event) → visual **3542**, shared with Vampiric Aura and Ritual Bell Aura (TBC adds Vampiric
Touch's 34919, Shadowmend, Triune Power).

| Event | TargetType | Kit | Contents |
|---|---|---|---|
| (6,13) impact | 1 and 2 | 3072 | `spells/vampiricembrace_state_base.m2` (167150, SVEN 2242) @19 Base |

### `vampiricembrace_state_base.m2` (@feet of each healed ally, one-shot)

- **It is `heal_low_base.m2`'s rig recoloured.** Same textures (`ribbonblur1bd_gold_side`,
  `star5a`, `yellow_star_dim`), same emitter positions (ribbon emitter at z 1.35, stars at
  x −0.28, z 0.47), same 1867 ms sequence; diffed against the Era `heal_low_base.m2` dump.
- **Changes:** colours violet (`(47, 0, 179)` → `(74, 0, 225)` → `(138, 90, 238)` on the
  ribbons; `(40, 0, 155)` → `(66, 0, 255)` on the dim stars); one `star5a` emitter goes red
  → gold `(255, 0, 0)` → `(255, 192, 0)`; ribbon rate **20 → 40/s at 100 ms → 0 at 633 ms**
  (against 15 → 20 → 5) and spawn area 0.14 → **0.69 yd**; three ribbon emitters instead of
  one (life 0.9 / 1.1 / 1.3 s); a 158 sequence of 4834 ms.
- **Reading:** the shipped Heal landing, in violet: a narrow stream of motes rising through
  the body, slightly busier.

### What this means for design (Vampiric Embrace)

- Cast with the shipped shadow hands; a 2 s violet dust implosion on the target; **no
  sustained debuff state**.
- **The party heals do have a client visual**, one per heal event, and it is a recolour of
  a landing we already ship. It fires once per Priest damage event on every healed ally,
  so its cadence is the Priest's hit rate (Mind Flay ticks once a second).

---

## 8. Silence

**Chain:** Silence 15487 (aura 27 silence, 5 s, target 6) → visual **179**, shared by 11
Era spells (6726, 8988, 12528, 18278… all named Silence, plus Manifest Spirit); TBC adds
Chilling Words, Feeble Weapons, Doubting Mind, Reverberation and two more.

| Event | TargetType | Kit | Contents |
|---|---|---|---|
| (1,2) precast | 1 | 114 | shadow precast hands, anim 51 (never plays) |
| (3,13) cast | 1 | 418 | `shadow_precast_low_hand.m2` @21/@22, anim **53** (SpellCastDirected) |
| (7,8) state | 1 and 2 (Era) / **2 only** (TBC) | 297 | `spells/shadowwordsilence_breath.m2` (166836, SVEN 301) @**17 Breath** |

- No `(6,13)` row. Kit 418 holds the same model as Spell Lock's kit 118.
- Kit 297 is shared with Piercing Screech, Sonic Burst, Aspect of Jeklik, The Black Sleep
  (TBC: 14 spells).

### Compared with the other silences

| Silence | Spell | Visual |
|---|---|---|
| Counterspell - Silenced | 18469 | **none** (no `SpellXSpellVisual` row) |
| Spell Lock (silence aura) | 24259 | **none**; Spell Lock's only visual is 19244 → 5282, a one-shot `counterspell_impact_chest.m2` (shipped, `client_landings.rs`) |
| Priest Silence | 15487 | **kit 297: a sustained state at the mouth** |

As `2026-09-26-interrupts-and-one-offs-client-data.md` found, interrupt silences have no
state. Priest Silence is the exception.

### `shadowwordsilence_breath.m2` (@mouth, sustained)

- **Sequences:** 0 Stand **1167 ms**, 158 **4167 ms**. No mesh. Thirteen emitters, read as
  two sets:
  - **Stand (inhale):** three `gradient64flipb` emitters, **80/s each for 0–167 ms**,
    speed **−3.3 (inward)** from a 1.5 yd area, life 0.5 s, size to 0.02–0.035 yd, violet,
    magenta and green; then from 233 to 867 ms `toonsmoke16` puffs pulled in (50/s × 2,
    speed −0.83, size **0.56 → 0.03 yd**, grey and magenta `(163, 0, 175)`), dark `flare`
    specks (100/s, blend 2, `(4, 0, 84)`) and `star5a` sparkles (30/s × 2, spin ±7).
  - **158 (the loop):** the same textures emitted outward from the mouth: `toonsmoke16`
    50/s × 2 from **233 to 800 ms** (speed 0.28, life 1.5 s, size **0.03 → 0.08 → 0.28
    yd**, grey → magenta → pale blue), `star5a` 30/s × 2 from 367 to 1167 ms (green →
    magenta → dark blue), dark specks 100/s from 733 to 967 ms. Then nothing until the
    loop repeats at 4167 ms.
- **Reading:** violet motes and smoke sucked into the mouth on landing; then, every 4.2 s, a
  small puff of grey-magenta smoke and sparkles breathed out.
- Era vs TBC: TBC does not re-enable the inhale emitters at 1167 ms.

### What this means for design (Silence)

- Cast with the shipped shadow hands (Directed anim). Nothing lands.
- **The victim carries a sustained state at the mouth for 5 s**: small (the smoke tops out
  at 0.28 yd) and mostly idle, a puff every 4.2 s. If the loop is played, a 5 s silence
  shows one exhale.
- No other silence in our kit has a client state, so this does not set a pattern to copy
  onto Kick or Spell Lock.

---

## 9. Inspiration

**Chain:** talent ranks 14892 / 15362 / 15363 trigger 14893 / 15357 / 15359 (aura 101
armor +8 / +16 / +25 %, 15 s, target 21 ally).

- **NO-VISUAL.** None of the six spells has a `SpellXSpellVisual` row, at either build. The
  sweep confirms the talent ranks.

### What this means for design (Inspiration)

- The client marks the buff with its icon only. Any visual would be authored.

---

## 10. Blessed Recovery

**Chain:** talent ranks 27811 / 27815 / 27816 trigger 18350 "Dummy Trigger"; the procs are
27813 / 27817 / 27818 (aura 8 periodic heal, period 2000 ms, 6 s, self) → visual **7548**
(shared in TBC with Blessed Resilience and Surge of Light).

| Event | TargetType | Kit | Contents |
|---|---|---|---|
| (6,13) impact | 1 and 2 | 435 | `spells/renew_chest.m2` (166697, SVEN 415) @34 Chest |

- **No `(7,8)` row: the 6 s HoT has no state.** Kit 435 is Renew's and Greater Heal's
  impact (TBC: 19 users).

### `renew_chest.m2` (@Chest, one-shot)

- **Sequences** 1867 ms, 158 1867 ms (emitters off). No mesh. Two emitters, blend 4, both
  9.2/s until **700 ms**, speed 0.18, life 0.76 s, drag 0.3:
  - `shockwavewater1` rings: size **0.28 → 0.49 → 1.09 yd**, `(255, 157, 3)` →
    `(250, 219, 8)` → `(253, 179, 20)`, alpha 0.15 → 0.57 → 0;
  - `yellow_glow3a` glows: size 0.22 → **0.7** → 0.56 yd, orange → white, alpha ≤ 0.45.
- **Reading:** a few soft amber rings swelling from the chest for 0.7 s.
- Era vs TBC: identical.

### What this means for design (Blessed Recovery)

- One small amber ring burst on the Priest when the proc fires; no state for the HoT. The
  ticks draw nothing (no event pair fires per periodic tick).

---

## 11. Blackout

**Chain:** talent ranks 15268, 15323–15326 (NO-VISUAL, sweep) trigger **15269** (aura 12
stun, 3 s, target 6) → visual **2816**, the generic "Stun" visual: 22 Era users (Stun,
Charge Stun, Intercept Stun, Revenge Stun, Mace Stun Effect, Intimidation, Improved
Concussive Shot…), 33 in TBC.

| Event | TargetType | Kit | Contents |
|---|---|---|---|
| (7,8) state | 1 and 2 (Era) / **2 only** (TBC) | 349 | `spells/stunswirl_state_head.m2` (166988, SVEN 50) @20 Head; victim anim **14** (Stun) |

- **Kit 349 is the same stun state Hammer of Justice and Kidney Shot use** (their `(7,8)`
  rows; Cheap Shot uses kit 33). It has 76 Era users, 129 TBC.
- ArenaSim already draws a stun state for HoJ and Kidney Shot (`hard_cc.rs`, authored: "a
  hueless whirl of beads turning once per second").

### `stunswirl_state_head.m2` (@Head)

- **Sequences** 0 / 158 / 159, all **1000 ms**; 159 fades the weight 0.45 → 0.
- **Mesh:** a ring of 400 verts, **1.04 yd across**, 0.22–0.51 yd above the head pivot,
  `genericglow64`, colour **`(0, 192, 255)`** (cyan-blue), weight 0.45, blend 4. Its bone
  rotates on a **1000 ms** global loop (nine keys): one turn a second.
- No emitters. Era vs TBC identical.

### What this means for design (Blackout)

- **Nothing new to draw.** Blackout's stun is the client's generic stun, the same kit as
  the HoJ and Kidney Shot stuns we already render. Route it through the existing Stun
  treatment.
- The client's ring is cyan, where our whirl is hueless; that difference is already ours
  across every stun, not Blackout's.

---

## 12. Reflective Shield (TBC)

**Chain:** talent ranks 33201–33205 (aura 4 dummy, 10–50 %) are **NO-VISUAL** (sweep). The
reflected damage is **33619** (`Effect 2` school damage, target 6 enemy) → visual **8383**,
its own. Era has neither.

| Event | TargetType | Kit | Contents |
|---|---|---|---|
| (6,13) impact | 1 and 2 | 121 | `spells/holy_impactdd_low_chest.m2` (166326, SVEN 129) @34 Chest; anim **9** (CombatWound) |

- **The visual is on the attacker** (the damage spell's target) as a one-shot impact and
  hit reaction. No missile, no beam, nothing on the Priest or the shield.
- Kit 121 is shared with Consecration, Judgement and Vindication (TBC: 14 users).

### `holy_impactdd_low_chest.m2` (@Chest of the attacker, TBC copy)

- **Sequence** 934 ms. No mesh. Ten emitters, blend 4: the vocabulary of Holy Nova's
  `_high_chest` impact (§4) at a smaller scale: a `glowstar_yellow` flash (P3 15/s for
  200 ms, size 0.83 → **2.5** → 0.03 yd); `glowstar_yellow` P0 40/s to 500 ms, size to 0.61
  yd; `starflashyellowa` 75/s inward, size to **1.39 yd**; gold ribbon-texture streaks (up
  to 150/s, tail 4–4.5); `starflashyellow` showers (up to 100/s, gravity 6.9); three
  `dust1_a` puff layers, brown-gold.
- **Reading:** a 2.5 yd gold starburst on the attacker's chest, under a second.
- **Era vs TBC:** the Era file is a two-emitter stub (`starflashyellowa` and the ribbon
  streaks only). Reflective Shield is TBC-only, so the TBC copy is the one that applies.

### What this means for design (Reflective Shield)

- One gold burst and a flinch on the attacker each time damage is reflected. Nothing ties
  it to the Priest's shield (our `shield_bubbles.rs`); the reflected damage number is the
  only link.
- It is the low variant of Holy Nova's enemy impact, so the two can share an implementation
  at two scales.

---

## Sustained channels per body (budget count)

Counted from `(7,8)` and `(11,12)` rows only, for the effects this doc covers.

- **On an ally:**
  - **Power Infusion** adds one at the chest, 15 s (Arcane Power's state, up to 4.6 yd).
  - **Pain Suppression** adds one at the feet, 8 s (a 4.4 yd shell in front of the body).
  - **Guardian Spirit** adds one over the shoulders, 10 s (halo and wings).
  - An ally given all three at once carries three states; no two share an attachment
    (Chest, Base-low, Base at shoulder height).
  - Inspiration, Blessed Recovery, the Holy Nova and Vampiric Embrace heals: none.
- **On the caster (the Priest):**
  - **Inner Focus** adds one on both hands until the next cast.
  - **Mind Flay** adds one on the casting hand for the 3 s channel (`(11,12)`).
  - Shadowform's self-state (AS-333) is the Priest's existing one; a Shadowform Priest
    channelling Mind Flay carries both.
- **On a victim:**
  - **Mind Flay** adds one at the chest for 3 s.
  - **Silence** adds one at the mouth for 5 s.
  - **Blackout** adds the generic stun state at the head for 3 s, **already shipped** for
    HoJ and Kidney Shot.
  - Vampiric Embrace: **none** (one-shot only). Holy Nova, Reflective Shield: none.
- **Between bodies:** **Mind Flay's beam**, the only one. Vampiric Embrace's heals, Holy
  Nova and Reflective Shield have no missile, beam or chain.

## Unresolved

1. **Whether a `(7,8)` kit loops its model's sequence or holds it** (carried from AS-334).
   It decides whether Silence's breath exhales once per 4.2 s and whether Inner Focus's
   hand glow pulses throughout.
2. **Which sequence a `(7,8)` model plays**: Stand (0), 158, or a start / loop / end chain
   of 0 → 158 → 159. Silence's breath, Mind Flay's star, Power Infusion's state and
   Guardian Spirit all have distinct Stand and 158 emitters; Pain Suppression adds id 127
   and id 1. `AnimationData` has no name column for 127, 158 and 159.
3. **Who TargetType 1 and 2 denote** (carried from AS-333). Mind Flay's `(11,12)` row is
   TargetType 2 and carries the caster's channel anim (124), while Blackout's `(7,8)`
   TargetType 2 row carries the victim's stun anim (14); so TargetType 2 is not simply
   "caster" or "target". TBC keeps only the TargetType 2 `(7,8)` rows for Inner Focus,
   Power Infusion, Mind Flay, Silence and Blackout.
4. **Holy Nova's dome orientation.** The hemisphere opens on the model's x = 0 plane and the
   only rotation (bone 1, 90° about x) does not move that face. Whether the vertices sit on
   bone 1, and whether the client presents the shell as an upright dome, was not decoded
   (vertex bone indices not dumped).
5. **`SpellProceduralEffect` type 0** read as a `SpellChainEffects` link: inferred from the
   texture names, not from a foreign key. `Value_1` / `Value_2` (1 / 1) and the meaning of
   `BlendMode` 3 and `Flags` 4 / 68 on the chain rows are not known.
6. **`SpellVisualKit.Flags_0`** 1048579 on TBC's Mind Flay kit 814 (Era 1048576): the two
   low bits are not known.
7. **Whether the precast and cast rows of a triggered spell play** (carried): the Holy Nova
   heal, the Vampiric Embrace heal, the Blessed Recovery and Blackout procs and Reflective
   Shield's 33619 carry no hand rows, so the question does not arise for them; Guardian
   Spirit's save carries none either.
8. **How long a `(6,13)` kit lives** against a model whose sequence outlasts the moment
   (carried): `holylight_impact_head.m2` 2500 ms, `soothingkiss_impact_head.m2` 2000 ms
   with a 5000 ms 158 sequence.
9. **The M2 particle gravity sign** (carried; read as positive = down) and the Era
   compressed-gravity encoding (flags `0x800000`, seen on `heal_low_base.m2` and the Era
   `holy_impactdd_low_chest.m2`), which the parser does not decode.
10. **Models not parsed in full:** `holy_precast_high_base.m2` (166334; precast only, never
    plays), `holy_precast_med_hand.m2` (166337; Guardian Spirit's precast, never plays),
    `shadow_precast_low_hand.m2` (documented in the warlock DoT doc), and the Era copy of
    `painsuppression_state.m2` (the SoD rune's; 30 lines differ from TBC's, not read).
11. Texture BLPs were not decoded. Colours above are colour tracks, not texture pixels.
