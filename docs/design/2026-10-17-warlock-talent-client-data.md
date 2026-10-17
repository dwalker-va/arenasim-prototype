# AS-334: Warlock talent grants and procs — Client-Data Research

This doc covers the ten Warlock talent effects in the talents proposal §5.1
(`docs/design/proposals/2026-10-11-talents-and-gauntlet.md`): Amplify Curse, Shadow
Trance (Nightfall), Siphon Life, Shadowburn, Conflagrate, Shadow Vulnerability (Improved
Shadow Bolt), Aftermath, Master Demonologist (Felhunter), Soul Link, and Demonic
Sacrifice with its sacrifice buff. Findings come from two builds:

- Classic Era **1.15.9.69547**;
- TBC Classic **2.5.6.69795**.

The sources are the wago.tools DB2 CSVs, joined directly, and M2 files fetched from CASC
and parsed. No wiki or prose source was used as evidence. Every row below comes from a
join that was run. Where a join could not be made, or a reading rests on inference rather
than a table, the row says so (**UNRESOLVED** or "inferred").

## Method

- **Inventory (skill-line spells).** `scripts/db2_spell_sweep.py --build <b> --skill-line
  354 355 593 --name "Amplify Curse" Nightfall "Shadow Trance" "Siphon Life" Shadowburn
  Conflagrate "Improved Shadow Bolt" Aftermath "Master Demonologist" "Soul Link"
  "Demonic Sacrifice" "Touch of Shadow" "Shadow Vulnerability" Immolate "Shadow Bolt"
  --events --caster-anims` was run at both builds. Skill lines: 354 Demonology, 355
  Affliction, 593 Destruction.
  - Counts, both builds: `13 = 5 + 8` (13 inventory names matched; 5 with no visual:
    Aftermath, Improved Immolate, Improved Shadow Bolt, Master Demonologist, Nightfall;
    8 with one).
  - Property 4 **HELD** at both builds: every name with a visual resolves to exactly one
    `SpellVisual` at Probability 1.
  - The run is narrowed by `--name` (and, in Era, by the default `--era-cut 400000`), so
    these are claims about the named spells only, not about the whole skill lines.
  - **Shadow Trance, Shadow Vulnerability and Touch of Shadow are not on any of the three
    skill lines.** They are helper or proc spells and are not in the inventory, so the
    sweep cannot see them. They were joined by hand (next point).
- **Helper and proc spells.** These were found from `SpellName` (exact name, SpellID <
  400000) and confirmed from `SpellEffect`, never from the brief's hints:
  - the talent ranks name their proc in `EffectTriggerSpell` (Improved Shadow Bolt
    17793/17796/17801/17802/17803 → Shadow Vulnerability 17794/17798/17797/17799/17800;
    Aftermath 18119–18123 → 18118);
  - Nightfall 18094/18095 carry no trigger (Era: aura 112 override-class-script, misc
    4309; TBC: aura 4 dummy), so Shadow Trance 17941 is identified by name and its effect
    (aura 108, misc 10, −101 %: cast time);
  - the Felhunter's Master Demonologist auras are the rows with aura 22 (resistance),
    misc 126 (all six magic schools): 23762 and 23837–23840. The talent ranks themselves
    (23785, 23822–23825) are aura 4 dummies.
  - Soul Link's aura is 25228 (Era: `Effect 119` apply-area-aura-pet; TBC: `Effect 143`
    apply-area-aura-owner), auras 79 (damage done %) and 81 (split damage %).
  - The sacrifice buffs are 18789 Burning Wish, 18790 Fel Stamina, 18791 Touch of Shadow,
    18792 Fel Energy, plus TBC's 35701 Touch of Shadow (the Felguard's).
- **Joins.** `SpellXSpellVisual → SpellVisual → SpellVisualEvent → SpellVisualKitEffect`,
  joined on column `Effect`. Effect types: 2 model
  (`SpellVisualKitModelAttach → SpellVisualEffectName.ModelFileDataID`), 6 body anim
  (`SpellVisualAnim`), 1 procedural (`SpellProceduralEffect`), 5 sound (ignored). Event
  pairs: `(1,2)` precast loop, `(3,13)` cast, `(6,13)` one-shot impact, `(7,8)` sustained
  aura state. As `2026-09-06-warlock-dot-client-data.md` established, no event pair fires
  per periodic tick.
- **Instants never show a precast.** Every spell in this doc has cast time 0
  (`SpellMisc.CastingTimeIndex → SpellCastTimes.Base = 0`, both builds), so a `(1,2)` row
  on its visual has no casting phase to play in. Only the `(3,13)` cast row draws on the
  caster. (Inferred from the event vocabulary, not from a table.)
- **M2 parser** (v274): the AS-333 `m2.py`, which re-derives `slow_impact_base.m2`
  exactly as `2026-09-26-slow-state-client-data.md` publishes it. Era copies of most
  models carry an empty name string; TBC copies carry names. **None of the models in this
  doc has a `GPID` chunk**: every emitter is a sprite.
- **Units and conventions** (as AS-333): model units ≈ yards; particle colours RGB 0–255
  over normalised particle life, alpha 0–1; emitter blend 2 = alpha, 4 = additive;
  attachments 19 Base (feet), 20 Head, 21/22 spell hands, 34 Chest. Anim 9 is
  `CombatWound` (`2026-09-20-as132-hit-reaction-client-data.md`; `AnimationData` 9 falls
  back to 8, StandWound). `SpellProceduralEffect` type 1 is read as a body tint with
  packed RGB in `Value_0`, type 14 as transparency (AS-333 §4).
- **Scripts** are under the AS-334 scratchpad (`as334/`): `db.py`, `chain.py`,
  `reverse.py`, `proctypes.py`, `m2.py` (copied from AS-333), plus `names.py` (exact-name
  `SpellName` lookup with effects) and `users.py` (who shares a visual or kit). Outputs:
  `sweep_<build>.txt`, `chain_<build>.txt`, `names.txt`, `models/<build>_<fdid>.txt`.

## Summary table

| Effect | Client spell → visual | Cast side (instant: `(3,13)` only) | Landing (one-shot) | Sustained `(7,8)` | TBC |
|---|---|---|---|---|---|
| Amplify Curse | 18288 → **4600** (shared: Presence of Mind, Fel Domination) | kit 4012: `presenceofmind_cast_base.m2` @Base, **no body anim** | — | **kit 4013: the same model @Base, on the Warlock** | (7,8) TargetType 2 only; model holds its peak instead of shrinking back |
| Shadow Trance (Nightfall proc) | 17941 → **5219** | — (a proc) | kit 4569: `presenceofmind_cast_base.m2` @Base + `shadow_impactdot_med_head.m2` @Head | **none** | same; kit shared with TBC Backlash |
| Instant Shadow Bolt | 686… → **64** (unchanged) | kit 118 only | kit 219 (shipped) | none | missile and impact replaced |
| Siphon Life | Era 18265, 18879–81 → **381** (Corruption's) / TBC + 27264, 30911 → **8687** | kit 118 (shadow hands) | kit 117 `shadow_impactdd_low_chest.m2` (shipped, Corruption apply) | Era **kit 535** (Corruption's state, shipped); TBC **kit 234** `corruption_impactdot_med_base.m2` @Chest (UA's) | different state kit |
| Siphon Life heal to the Warlock | aura 53 (periodic leech): no separate spell | — | — | — | **NO-VISUAL**: no missile, beam or caster row |
| Shadowburn | 17877, 18867–71 (+27263, 30546 TBC) → **3057** (Mind Blast's) | kit 218 (shadow hands, anim 54 Omni) | kit 2709 `mindblast_head.m2` @Head (**shipped**) | none | same |
| Conflagrate | 17962, 18930–32 (+27266, 30912 TBC) → **5199** | kit 38: `fire_cast_hand.m2` both hands, anim 53 | kit 4551: `fire_impactdd_high_chest.m2` @Chest + victim anim 9 | none | same |
| Shadow Vulnerability | 17794, 17797–17800 | — | — | — | **NO-VISUAL** (no `SpellXSpellVisual` row, both builds) |
| Aftermath | 18118 → **4826** | — (a proc) | kit 728: `fire_impactdd_chest.m2` @Chest + victim anim 9 | **kit 985: `firenova_state.m2` @Base + tint `#FF5155`** | (7,8) TargetType 2 only; tint `Value_1/2` 0 → 0.1 |
| Master Demonologist (Felhunter) | 23762, 23837–40 (talent 23785, 23822–25) | — | — | — | **NO-VISUAL** (every rank, every demon, both builds) |
| Soul Link | 19028 (cast) and 25228 (aura) → **969** (shared with Amplify Magic and 28 others) | kit 267: `magic_cast_hand.m2` both hands (**shipped as Moonfire's**), anim 54 | kit 991: `amplifymagic_impact_base.m2` @Base | **none**; no beam | same |
| Demonic Sacrifice | 18788 → **1305** (Banish's) | kit 118 (shadow hands, anim 53) | — | kit 1286 (Banish's state) on a spell with **no aura** (instakill on the pet) | same |
| Touch of Shadow (Succubus buff; our design gives it to the Felhunter) | 18791 (+35701 TBC) → **5791** | — (applied by the sacrifice) | kit 5134: `demonicsacrifice_succubus_chest.m2` @Chest | **none** | same |
| Fel Energy (the Felhunter's own client buff) | 18792 → **5789** | — | kit 5132: `demonicsacrifice_felhunter_chest.m2` @Chest | **none** | same |

### Five findings that shape the work

1. **Only two of the ten draw a sustained state, and one of those is on the caster.**
   Amplify Curse holds a ground flash at the Warlock's feet for its buff (kit 4013), and
   Aftermath holds a fire-nova state with a red body tint on its victim (kit 985).
   Siphon Life's state is not new: in Era it is Corruption's kit 535, which we already
   draw. Shadow Trance, Soul Link, Touch of Shadow and Fel Energy are one-shot only.
2. **Four effects have no client visual at all.** Shadow Vulnerability, Master
   Demonologist, the Siphon Life heal back to the Warlock, and the Nightfall /
   Improved Shadow Bolt / Aftermath talent ranks themselves. Soul Link has no link, beam
   or persistent state on either body.
3. **Three effects reuse visuals we already ship.** Siphon Life is Corruption's whole
   visual in Era (apply kit 117 and state kit 535, `warlock_dots.rs`). Shadowburn is Mind
   Blast's whole visual (`mindblast_head.m2`, kit 2709, `client_landings.rs`). Soul Link's
   cast hands are `magic_cast_hand.m2`, Moonfire's kit 730 model (`heal_cast.rs`). Shadow
   Trance changes nothing about the Shadow Bolt it makes instant (visual 64 is shared by
   every rank; the shipped missile and impact stand).
4. **The client gives each demon's sacrifice its own colour, and Touch of Shadow is the
   Succubus's.** `demonicsacrifice_felhunter_chest.m2` is blue, the Succubus model violet
   with green-tipped motes; they share one rig (a spinning three-fold triad of glows and
   ribbons, about 2.4 s). The design's "Touch of Shadow" for the Felhunter borrows the
   Succubus's name; the client's Felhunter buff is Fel Energy (18792), with the blue
   model.
5. **Demonic Sacrifice draws nothing on the demon.** Its only rows are Banish's: the
   shadow cast hands and a `(7,8)` Banish state, and the spell carries no aura (`Effect 1`
   instakill on the pet), so the state has no aura to live on (inferred; see Unresolved).
   The demon simply dies; the visible moment is the buff's one-shot on the Warlock.

---

## 1. Amplify Curse

**Chain:** Amplify Curse 18288 (one rank; aura 108 +50 % misc 8 and aura 107 −20 % misc
3, 30 s) → visual **4600**. No missile. Visual 4600 is shared with Presence of Mind
(12043), Fel Domination (18708) and a QA spell in Era; TBC adds Presence of Mind 29976
and Sadist (37604).

| Event | TargetType | Kit | Contents |
|---|---|---|---|
| (3,13) cast | 1 | 4012 | `spells/presenceofmind_cast_base.m2` (fdid 166658, SVEN 2203) @**19 Base**, scale 1; sound. **No EffectType 6: no body anim.** |
| (7,8) state | 1 and 2 (Era) / **2 only** (TBC) | 4013 | the same model @19 Base; sound |

There is no `(1,2)` row. AS-15 §5 already listed Amplify Curse among the cast kits with
no body anim (visual 4600, with Fel Domination).

### `presenceofmind_cast_base.m2` (cast and state, @feet)

- **Sequence.** One Stand sequence of **1500 ms** (plus id 158, 1500 ms). Texture
  `spells/genericglow2b.blp`, one material, blend 4 (additive).
- **Mesh.** Three concentric flat quads on the ground (z = 0.03), half-widths **0.63 /
  0.51 / 0.41 yd**, all on bones whose scale runs **0.613 → 3.243 at 467 ms → 0.613 at
  1500 ms** (Era). So the outer quad is about 0.8 yd across at rest and **4.1 yd across at
  467 ms**.
- **Colour.** All three colour tracks `(0.15, 0.38, 0.355)` → `(−0.32, 0.38, 1.355)` at
  467 ms. Clamped to 0–255: `(38, 97, 91)` → `(0, 97, 255)`, a teal that pushes to blue.
- **Opacity.** Texture weight 0 → **0.6 at 133 ms** → 0 at 467 ms, then 0 to 1500 ms.
- **Reading:** a soft additive ground flash at the feet that blooms outward over 0.47 s,
  brightest at 0.13 s, and is gone by half a second. It is the client's generic
  "empowered next cast" flash (Presence of Mind, Fel Domination, Trueshot Aura,
  Perception all use it).
- **Era vs TBC.** The TBC copy ends the sequence at the peak instead of returning: bone
  scale holds 3.243 and colour holds `(−0.32, 0.38, 1.355)` at 1500 ms. Texture weight is
  unchanged, so the flash is still invisible after 467 ms.

### What this means for design (Amplify Curse)

- The cast is a one-shot ground flash with no hands and no body anim.
- **The buff is a sustained state on the Warlock** (kit 4013) for as long as the aura is
  up: a **caster sustained-channel cost** at the feet. If the client loops the 1500 ms
  Stand sequence for a `(7,8)` model, it is a 0.47 s flash repeating every 1.5 s; whether
  it loops is UNRESOLVED (see Unresolved 1).
- It shares its look with Fel Domination and Presence of Mind. Nothing in our kit uses it
  yet.

---

## 2. Shadow Trance (the Nightfall proc), and the instant Shadow Bolt

**Chain:** Nightfall talents 18094 / 18095 are NO-VISUAL (sweep) and carry no trigger
spell. The proc is Shadow Trance **17941** (aura 108 −101 % on misc 10 = cast time,
10 s; Era also carries an aura 108 +100 % misc 0 row) → visual **5219**. In Era, 5219 is
Shadow Trance's alone; TBC shares its kit 4569 with Backlash, Consume Essence, Consume
Life and Siphon Essence.

| Event | TargetType | Kit | Contents |
|---|---|---|---|
| (6,13) impact | 1 and 2 | 4569 | `presenceofmind_cast_base.m2` (166658) @**19 Base**; `spells/shadow_impactdot_med_head.m2` (fdid 166801, SVEN 1262) @**20 Head**; sound |

**No `(7,8)` row: the client draws no persistent Shadow Trance buff.** The proc plays once
on the Warlock and leaves only the buff icon.

### `shadow_impactdot_med_head.m2` (@Head)

- One sequence of **8000 ms**. No mesh. Two sprite emitters, both `flare.blp`, blend 2
  (alpha), plane emitters (horizontal range 2π, vertical 0):

| # | Rate /s | Life s | Speed yd/s | Spawn area | Gravity | Size (yd) | Colour | Alpha |
|---|---|---|---|---|---|---|---|---|
| P0 | 6 | **4.0** | **−0.28 (inward)** | 1.39 × 1.39 | 0.139 | 0.003 constant | `(2,2,2)` near-black | 0 → 1 → 0 |
| P1 | 6 | 4.0 | −0.28 | 1.39 × 1.39 | 0.139 | 0.028 constant | `(2,2,2)` | 0 → 1 → 0 |

- **Reading:** a sparse ring of tiny dark specks around the head that drift slowly inward
  and settle, over 4 s each. Very low energy: twelve specks a second. Paired with the
  teal ground flash at the feet (§1), the proc's identity is "the Amplify flash, plus a
  dark mote halo".
- Era vs TBC: byte-identical (same md5).

### The instant Shadow Bolt

- Shadow Bolt's visual is **64** for every rank, both builds; Shadow Trance adds no row to
  it and no other visual points at it. The client's instant Shadow Bolt is the ordinary
  one with no casting phase: the `(1,2)` precast hands (kit 114) have nothing to play
  in, and the cast hands (kit 118, anim 53), the missile and the impact play as usual
  (inferred; §Method).
- Era: missile `deathcoil_missile.m2` (165891), impact kit 219 `deathcoil_impact_chest.m2`
  (165890), both **shipped** (`spell_bolts.rs`). TBC replaces both: missile
  `shadowbolt_missile.m2` (166815, SVEN 3493) and impact kit 7775
  `shadowbolt_chest_impact.m2` (166814).

### What this means for design (Shadow Trance)

- The proc moment is one-shot on the Warlock: the Amplify ground flash (§1, shared) plus a
  dark speck halo at the head. **No sustained channel.**
- "The next Shadow Bolt is instant" has no client tell beyond the buff icon; the bolt
  itself reuses everything we ship. The only visible difference is that the precast
  phase never happens.

---

## 3. Siphon Life

**Chain:** Siphon Life ranks 18265, 18879, 18880, 18881 (TBC adds 27264, 30911): aura
**53** (periodic leech), period 3000 ms, 30 s.

- **Era → visual 381**, Corruption's visual (22 era spells: Corruption ranks, Siphon Life
  ranks, NPC Corruptions). The warlock DoT doc §1 recorded this.
- **TBC → visual 8687**, Siphon Life's alone (its six ranks). Visual 381 in TBC keeps only
  NPC spells and two NPC Siphon Lifes (35195, 41597).
- 18927–18929 are also named Siphon Life: `Effect 36` (learn spell) rows on visual 107,
  the talent's spellbook-learn helpers. Not part of the effect.

| Event | TargetType | Kit (Era 381) | Kit (TBC 8687) | Contents |
|---|---|---|---|---|
| (1,2) precast | 1 | 114 | 114 | shadow precast hands, anim 51 (never plays: instant) |
| (3,13) cast | 1 | 118 | 118 | `shadow_precast_low_hand.m2` (166807) @21/@22, anim **53** SpellCastDirected |
| (6,13) impact | 1 and 2 | 117 | 117 | `shadow_impactdd_low_chest.m2` (166796) @34 Chest |
| (7,8) state | 1 and 2 | **535** | — | `pestilence_impact_chest.m2` (166637) @**20 Head** |
| (7,8) state | 2 | — | **234** | `corruption_impactdot_med_base.m2` (165821, SVEN 251) @**34 Chest** |

Kits 117 and 535 are fully parsed in `2026-09-06-warlock-dot-client-data.md` §3 and are
**shipped** (`warlock_dots.rs`: Corruption apply and state). This parse did not
re-derive them.

### TBC state: `corruption_impactdot_med_base.m2` (kit 234, @Chest)

Kit 234 is shared in TBC by Siphon Life, **Unstable Affliction** (30108 → visual 8141),
and NPC spells (Seething Plague, Void Zone, Sul'thraze …). TBC's player Corruption (172 →
visual 8629) has **no `(7,8)` row at all**. In Era, kit 234 belongs only to Seething
Plague and Sul'thraze.

- One Stand sequence of **3000 ms** (plus id 158, 3000 ms). No mesh. Three sprite emitters,
  all sphere emitters:

| # | Texture | Blend | Rate /s | Life s | Speed yd/s | Spawn | Size (yd, birth → mid → death) | Colour | Alpha |
|---|---|---|---|---|---|---|---|---|---|
| P0 | `shockwave4white` | 4 | 30 | 1.5 | 1.67, full sphere | point | **1.39 → 0.56 → 0.28** | `(22,0,133)` → `(83,0,186)` → `(112,0,183)` | 0.78 → 1 → 0.78 |
| P1 | `flare` | 4 | 30 | 1.5 | 1.67, full sphere | point | 1.39 → 0.56 → 0.28 | as P0 | as P0 |
| P2 | `flare` | 2 | 25 | 0.8 | **−3.89 (inward)**, tail 1.5 | radius 2.22 | 0.139 | black → `(37,0,82)` → `(112,0,183)` | 0 → 1 → 0 |

- **Reading:** violet rings and flares that burst out from the chest and shrink as they
  fly, while dark streaks are pulled in from a 2.2 yd shell. Violet, additive, constant
  (no pulse key in the model). Unlike Corruption's Era state (kit 535), which darkens
  with alpha-blend, this one glows.
- Era vs TBC: identical apart from the name string and bounding box.

### The heal back to the Warlock

- Aura 53 is a periodic leech: the heal is part of the same aura, with **no separate
  spell, no missile set (`SpellVisual.HasMissile 0`), no `(11,12)` channel row and no
  caster-side row** on either visual. **NO-VISUAL**: the client draws no transfer from
  victim to Warlock, unlike Drain Life's channel (visual 177, `(11,12)`).

### What this means for design (Siphon Life)

- In Era, Siphon Life **looks exactly like Corruption**, cast to state. A Warlock running
  both puts the same state on the victim twice; whether two copies of one state model
  read differently is not in the data.
- The victim state is a **victim sustained-channel cost**, but in Era it is the channel
  Corruption already pays (kit 535). TBC gives Siphon Life its own state (kit 234,
  violet glow at the chest), shared with Unstable Affliction.
- The heal ticks have no client visual. Our heal-tick feedback (floating text) is the only
  carrier unless a new treatment is designed.

---

## 4. Shadowburn

**Chain:** Shadowburn ranks 17877, 18867–18871 (TBC adds 27263, 30546) → visual **3057**,
Mind Blast's visual (13 Mind Blast rank and NPC ids share it in Era; 34 spells in TBC).
Era effect 0 is aura 86 (the 5 s shard-on-death debuff); TBC replaces it with `Effect
64` triggering 29341. Effect 1 is direct damage (`Effect 2`).

| Event | TargetType | Kit | Contents |
|---|---|---|---|
| (1,2) precast | 1 | 114 | shadow hands, anim 51 (never plays: instant) |
| (3,13) cast | 1 | **218** | `shadow_precast_low_hand.m2` @21/@22, anim **54** SpellCastOmni |
| (6,13) impact | 1 and 2 | **2709** | `spells/mindblast_head.m2` (166558, SVEN 1442) @**20 Head**; sound |

No `(7,8)` row: the Era 5 s debuff draws nothing.

### `mindblast_head.m2` (kit 2709 — already shipped)

Shipped as Mind Blast's landing (`client_landings.rs`, `MINDBLAST_HEAD`). This parse
(TBC copy) re-derives it:

- One sequence of 2000 ms. No mesh. Three sphere emitters, gravity −1.25 (rising):
  - P0 `flamelick_purple` (4×4), blend 4, rate 0 → **75/s from 200 ms to 1400 ms** → 0 at
    2000, life 1.0 s, size 0.11 → 0.28 → 0.03, colour `(79,55,255)` → `(126,55,255)` →
    `(210,0,255)`.
  - P1 `lavalump2`, blend 4, 2.5/s, life 2.25 s, spin 3, size 0.03 → 0.06, violet → blue.
  - P2 `toonsmoke16` (8×8), blend 2, 5/s, life 2.25 s, size 0.17 → 0.35 → 0.69, colour
    `(60,0,255)` → `(58,35,84)` → grey.
- Reading: violet flame licks rising off the head for about 1.2 s, with a few violet lumps
  and smoke.

### What this means for design (Shadowburn)

- **Shadowburn reuses the shipped Mind Blast landing verbatim**, on the head, and the
  shadow cast hands (cast kit 218 with the Omni anim, the Life Tap / Curse of Weakness
  pair — AS-15 §5 records Shadowburn as the one "mixed" spell). No new asset is needed to
  be client-faithful. No sustained channel.

---

## 5. Conflagrate

**Chain:** Conflagrate ranks 17962, 18930–18932 (TBC adds 27266, 30912) → visual
**5199** (Conflagrate's alone, plus one TBC quest spell). School 4 (Fire).

| Event | TargetType | Kit | Contents |
|---|---|---|---|
| (1,2) precast | 1 | 30 | `fire_precast_hand.m2` (166120) @21/@22, anim 51 (never plays: instant) |
| (3,13) cast | 1 | **38** | `spells/fire_cast_hand.m2` (166110, SVEN 288) @21/@22, anim **53** SpellCastDirected |
| (6,13) impact | 1 and 2 | **4551** | `spells/fire_impactdd_high_chest.m2` (166115, SVEN 212) @**34 Chest**; **victim anim 9 (`CombatWound`)**; sound |

No `(7,8)` row.

### Relation to Immolate

- **Immolate** is visual **46**: precast kit 60 / cast kit 61, which attach **the same two
  hand models** (`fire_precast_hand.m2`, `fire_cast_hand.m2`) but with the **Omni** anims
  52 / 54; impact kit 76 `immolate_impact_chest.m2` (166401) @Chest; state kit 235
  `immolate_state_base.m2` (166403) @Base with procedural tint type 1 `#E75641`, values
  0 / 0 / 0.2 (TBC: (7,8) TargetType 2 only). The state is **shipped** (`immolate.rs`);
  the apply burst we ship (`flame.rs`) is not the client model.
- **Conflagrate references nothing of Immolate's.** No row in visual 5199 names an
  Immolate kit or model, and no Immolate row fires on removal: the `(7,8)` state ends at
  aura-remove (event 8), which is how "Immolate is consumed" shows in the client. The
  consumption is the burn state switching off as Conflagrate's fire burst lands.
- Conflagrate's impact model is **not** Immolate's: `fire_impactdd_high_chest.m2` is the
  client's big generic fire impact (also NPC dragon Breath, Conflagration, Pyroblast's
  visual 149239, Cleansing Flames …).

### `fire_impactdd_high_chest.m2` (kit 4551, @Chest)

One sequence of **1000 ms**. No mesh. Eight sprite emitters, all with time windows inside
the first 0.83 s:

| # | Texture | Blend | Window (ms) | Rate /s | Life s | Speed yd/s | Size (yd, birth → mid → death) | Colour over life | Reading |
|---|---|---|---|---|---|---|---|---|---|
| P0 | `tooncloud64_soft` | 4 | 0–833 | 50 | 1.2 | 0.33, sphere, gravity −0.56 | 0.22 → 0.22 → 0.03 | `(255,222,0)` → `(206,129,47)` → grey `(91,91,91)` | rising yellow-orange glow puffs |
| P1 | `firering128` | 4 | **167–233** | 20 | 0.75 | 0, plane | **0.03 → 0.83 → 0.97** | `(245,203,0)` → `(155,88,0)` → `(100,0,0)` | one expanding fire ring |
| P2 | `tooncloud64` | 2 | 0–667 | 30 | 1.25 | 0.28, gravity −0.56 | 0.28 → 0.28 → 0.03 | `(255,222,0)` → `(236,148,54)` → grey | opaque flame puffs |
| P3 | `flare` | 4 | 0–333 | **125** | 1.0 | **2.78**, sphere, spin −2 | 0.03 → 0.07 → 0.03 | `(255,209,0)` → `(255,159,35)` → `(101,0,0)` | spark spray, ~42 sparks |
| P4 | `fireplume64` | 4 | 0–167 | 125 | 0.7 | **3.89** | 0.03 → 0.07 → 0.03 | `(78,0,0)` → `(221,221,221)` → `(213,95,0)` | white-hot plume jets |
| P5 | `fireplume64` | 4 | 0–167 | 75 | 0.7 | **4.44** | 0.03 → 0.14 → 0.03 | `(59,2,2)` → `(205,106,0)` → `(198,0,0)` | orange plume jets |
| P6 | `shockwave10` | 4 | **133–233** | 40 | 1.0 | 0, plane | **0.03 → 1.39 → 2.22** | grey → `(255,162,0)` → `(255,0,0)` | the big shock ring |
| P7 | `toonsmoke16` (8×8) | 2 | 0–667 | 20 | 2.0 | 0.56, plane, area 0.56 | 0.33 → 0.44 → 0.03 | `(235,220,174)` → near-white | pale smoke |

- **Reading:** a hard fire detonation at the chest. Plumes and sparks jet out at 3–4.4
  yd/s in the first 0.17–0.33 s, a fire ring and a shock ring (growing to 2.2 yd) flash
  at 0.13–0.23 s, and yellow-orange puffs and pale smoke linger to about 2 s. The victim
  flinches (anim 9).
- Era vs TBC: identical apart from the name string, emitter-flag bits and bounding box.

### `fire_cast_hand.m2` (kit 38)

Not parsed in detail here: it is the shared fire launch model, also Immolate's cast kit
61. **Not shipped.**

### What this means for design (Conflagrate)

- One-shot on the victim: a big fire burst at the chest plus the CombatWound flinch we
  already draw (`hit_reaction.rs`). **No sustained channel.**
- "Immolate ends" is the shipped Immolate state turning off. The client adds no
  consumption visual.
- The cast hands are the fire hands Immolate also uses (Directed, not Omni). We ship
  neither.

---

## 6. Shadow Vulnerability (Improved Shadow Bolt's proc)

**Chain:** Improved Shadow Bolt ranks 17793, 17796, 17801, 17802, 17803 (NO-VISUAL, sweep)
each trigger one Shadow Vulnerability: 17794 (+4 %), 17798 (+8 %), 17797 (+12 %), 17799
(+16 %), 17800 (+20 %). All are aura 87 (damage taken %), misc 32 (Shadow), on the
target.

- **None of the five has a `SpellXSpellVisual` row, in either build. NO-VISUAL.** The
  client draws nothing on the victim when it lands, while it holds, or when it ends.
- (15258, also named Shadow Vulnerability, is the Priest's Shadow Weaving debuff; also
  NO-VISUAL. Not part of this effect.)

### What this means for design (Shadow Vulnerability)

- The client sells it only through the debuff icon. Any victim treatment is authored, not
  transcribed, and would be a **victim sustained-channel cost** for its 12 s.

---

## 7. Aftermath (the daze proc)

**Chain:** Aftermath talent ranks 18119–18123 (NO-VISUAL, sweep) each trigger **18118**:
aura 33 (movement speed) −50 %, 5 s, school Fire → visual **4826** (Aftermath's alone).

| Event | TargetType | Kit | Contents |
|---|---|---|---|
| (6,13) impact | 1 and 2 | 728 | `spells/fire_impactdd_chest.m2` (166113, SVEN 588) @**34 Chest**; **victim anim 9 (`CombatWound`)**; sound |
| (7,8) state | 1 and 2 (Era) / **2 only** (TBC) | **985** | `spells/firenova_state.m2` (166147, SVEN 715) @**19 Base**; procedural type 1, `Value_0` 16732501 = **`#FF5155`** (255, 81, 85), values Era 0 / 0 / 1, TBC 0.1 / 0.1 / 1 |

- Kit 728 is the generic small fire impact (31 Era names: Fire Blast, Firebolt, Flame Lash,
  Fiery Weapon …).
- **Kit 985 is the client's fire-slow state**, shared by Aftermath, **Blast Wave** (11113,
  a fire slow), Fire Nova, Flamebreak, Flames of Chaos and others (13 Era names, 19 TBC).

### `fire_impactdd_chest.m2` (kit 728, @Chest)

One sequence of **1900 ms**; four sphere emitters, all switched on at **67 ms** and held
for the rest of the sequence:

| # | Texture | Blend | Rate /s | Life s | Speed yd/s | Size (yd) | Colour | Alpha |
|---|---|---|---|---|---|---|---|---|
| P0 | `alphacloud` | 2 | 50 | 1.0 | 0.78 | 0.03 → 0.33 → 0.33 | grey `(64,64,64)` → `(185,185,185)` | 0 → 0.39 → 0 |
| P1 | `flare` | 4 | 30 | 1.2 | 1.67, gravity 1.39 | 0.22 → 0.08 → 0.03 | `(214,0,0)` → `(250,149,18)` → `(214,0,0)` | 1 |
| P2 | `flare` | 4 | 30 | 0.8 | 1.67, gravity 1.39, tail 0.3 | 0.056 → 0.03 → 0.003 | `(185,185,185)` → `(247,151,0)` → `(209,0,0)` | 1 |
| P3 | `alphacloud` | 4 | 50 | 0.8 | 0.28 | 0.03 → 0.42 → 0.14 | `(249,234,64)` → `(248,128,30)` → `(255,25,25)` | 0 → 0.78 → 0 |

Reading: a small red-orange burst with falling embers and grey smoke at the chest.

### `firenova_state.m2` (kit 985, @feet)

Three sequences of 1000 ms each: id 0 (Stand), id 158 and id 159. No mesh. Four sprite
emitters, all blend 4. Values from the TBC copy (see the Era note below):

| # | Texture | Rate /s by sequence | Life s | Speed yd/s | Gravity | Spawn | Size (yd) | Colour | Alpha |
|---|---|---|---|---|---|---|---|---|---|
| P0 | `ember` | Stand: **50 for 34–67 ms** (≈ 1.7 per loop) | 0.8 | **4.44**, cone 0.175 rad | **13.9** | 0.69 × 0.69 plane | 0.028 → 0.042 → 0.028 | `(255,37,37)` → `(255,164,45)` → `(255,219,109)` | 0.39 → 0.71 → 0 |
| P1 | `dust1_a` | Stand **0**; id 158 **20**; id 159 50 → 0 by 200 ms | 1.5 | 0.28, sphere, spin 0.5 | −0.69 (rising) | 0.14 × 0.44 | 0 → 0.22 → 0.28 | `(255,15,15)` → `(255,144,0)` → `(255,205,54)` | 0 → 0.39 → 0 |
| P2 | `dust1_a` | as P1 | 1.5 | 0.28, spin −0.5 | −0.69 | 0.14 × 0.44 | as P1 | `(255,25,25)` → `(239,148,30)` → `(254,216,77)` | as P1 |
| P3 | `ember` | id 159 only: 150 for 33–67 ms | 0.5 | 2.5, sphere | 6.94 | radius 0.56, 1.2 above base | 0.028 → 0.042 | `(255,0,0)` → `(255,95,0)` → `(255,174,0)` | 0 → 0.71 → 0 |

- **Reading:** at the feet, a red-to-gold heat haze (two counter-spinning dust emitters,
  20/s each in sequence 158) with an occasional ember tossed up and falling back, plus
  the whole body tinted red (`#FF5155`). Sequence 159 reads as an end burst (a puff of
  dust and a spray of embers). In the Stand sequence alone the dust is off and only
  about 1.7 embers a second pop. **Which sequence the client loops for a `(7,8)` kit is
  UNRESOLVED** (Unresolved 2), so the state's density is too.
- **Era note:** the Era copy sets emitter flag `0x800000` on P0 and P1, which the parser
  reads as a gravity value of −1.2e38; the flag is read as "compressed gravity" (community
  M2 docs). Rates, lives, sizes and colours match TBC. Use the TBC gravity values. The
  Era P0 texture is `spells/ember.blp`, TBC `creature/fireelemental/ember.blp`.

### Against the shipped slow treatment

- Our slow ring (`slow_ring.rs`) is keyed on `AuraType::MovementSpeedSlow`, so an
  Aftermath slow would already draw it. It is grounded in kit 360
  (`slow_impact_base.m2`, a 1 Hz yellow-to-violet ground ripple at the feet,
  `2026-09-26-slow-state-client-data.md`).
- **The client does not give Aftermath kit 360.** It gives the fire-slow kit 985: same
  anchor (feet), but a continuous heat haze, not a ripple, plus a body tint. The slow doc
  found the same pattern for every source it checked: each school or source has its own
  state (Frost: kit 3531 chill tint `#3C3CFF` on the head; Concussive: kit 1564 daze on
  the head), and kit 360 is the generic case.

### What this means for design (Aftermath)

- A one-shot fire hit with a flinch, then a **victim sustained-channel cost** for 5 s:
  feet haze and a red body tint. Both overlap channels the shipped slow ring already uses
  (feet) and the body-tint channel.
- The client's tint `#FF5155` is a strong red; the slow doc's hue argument (school tint vs
  client hue) applies here too.

---

## 8. Master Demonologist (Felhunter)

**Chain:** the talent ranks 23785, 23822, 23823, 23824, 23825 are aura 4 (dummy). The
per-demon auras are applied with `Effect 119` (apply area aura: pet) and `Effect 77`
(script, ranks 23784, 23830–23832). The Felhunter's are **23762, 23837, 23838, 23839,
23840**: aura 22 (resistance), misc 126 (all six magic schools). The other rows are aura 10
(23759, 23826–29), aura 87 (23760, 23841–44) and aura 79 (23761, 23833–36), read as the
Imp's, Voidwalker's and Succubus's; TBC adds 35702–35706 (aura 79 + aura 22, read as the
Felguard's). The demon attribution is read from the aura type against the talent's
per-demon text; the tables do not name the demon.

- **Every one of these ids has no `SpellXSpellVisual` row in either build.** The sweep
  marks the name NO-VISUAL (25 Era ids, 30 TBC ids on the skill lines), and the
  name lookup confirms it for each id. **NO-VISUAL** on the Warlock and on the demon.

### What this means for design (Master Demonologist)

- The client sells it through aura icons only, on both bodies. Any visual is authored.

---

## 9. Soul Link

**Chain:**

- **19028** Soul Link, the cast: `Effect 3` (dummy) on the pet (target 5). → visual
  **969**.
- **25228** Soul Link, the aura: Era `Effect 119` (area aura: pet), aura 79 +3 % and aura
  81 split 30 %; TBC `Effect 143` (area aura: owner), aura 79 +5 %, aura 81 split 20 %;
  duration −1 (permanent). → **the same visual 969**.
- Visual 969 is the arcane buff visual shared with Amplify Magic, Amplify Damage, Arcane
  Might, Ephemeral Power and 25 more Era spells (79 in TBC).

| Event | TargetType | Kit | Contents |
|---|---|---|---|
| (1,2) precast | 1 | 266 | `spells/magic_precast_hand.m2` (166526, SVEN 272) @21/@22, anim 52 (never plays: instant) |
| (3,13) cast | 1 | **267** | `spells/magic_cast_hand.m2` (166524, SVEN 392) @21/@22, anim **54** SpellCastOmni |
| (6,13) impact | 1 and 2 | **991** | `spells/amplifymagic_impact_base.m2` (165562, SVEN 720) @**19 Base**; sound |

**No `(7,8)` row on visual 969, no missile, no channel pair.** The client draws no link,
no beam and no persistent state on the Warlock or the demon. Kit 991 also appears on an
Era spell named "OLDSoul Link Effect".

### `magic_cast_hand.m2` (kit 267 — shipped as Moonfire's kit 730)

Already shipped (`heal_cast.rs`, Moonfire's arcane hands). This parse: 1000 ms, a
starflash quad pulsing 0.82 → 1.55 at 300 ms → 0.42, plus three emitters (cyan glow
8.3/s, star 11.9/s, `teleporttarget` ring 13.2/s for 0–300 ms) running violet → cyan →
gold. The precast model (kit 266, `magic_precast_hand.m2`) is not shipped and never plays
for an instant.

### `amplifymagic_impact_base.m2` (kit 991, @feet)

- **Sequence.** One sequence of **2500 ms**; global loops 1733 / 1333 / 5500 / 400 ms.
- **Mesh: a stacked column of red rune discs.** Eleven flat `aurarune_a` quads
  (half-widths 1.25–1.76 yd) stacked from the base down to −1.98, and two `aurarune_b`
  pieces (0.73 yd), all blend 4. Colour tracks are **pure red `(255,0,0)`** (one
  `(255,105,105)`, two flicker `(255,85,85)` ↔ `(221,159,159)` on a 400 ms loop). The
  stack's bones translate upward: bone 0 rises 0 → **5.06 yd** over 2.5 s while others
  sink up to −2.64.
- **Fade.** The discs fade in one after another: disc 1 at 334 ms (alpha 1.0), then each
  next disc 100 ms later at a lower peak (0.45, 0.40 … 0.05), holding about 0.77 s and
  fading by 1.6–2.5 s. The rune ring scale snaps 2.48 → 0.91 at 200 ms → 0.98.
- **Three emitters** (`ribbonne1_red` texture, all blend 4):
  - P0: 20/s falling to 5/s by 534 ms, then off; speed 2.22 outward, life 0.9 s, red →
    pink, 0.08 → 0.01 yd.
  - P1: 20/s for 0–1000 ms, speed 3.33, life 1.0 s, spin 3, peach → red.
  - P2: 16/s for 0–1000 ms, speed 1.67, life 1.25 s, spin −3, pink → red → peach.
- **Reading:** a column of red rune circles that rises off the ground and fans upward,
  each fainter than the last, with red sparks spiralling out; gone by 2.5 s.
- **Era vs TBC:** the Era copy re-enables P0 and P1 at 2500 ms (a loop restart key); TBC
  does not. Otherwise identical.

### What this means for design (Soul Link)

- The client marks Soul Link only at the moment it lands, with a red rune column at the
  feet of whoever the event targets. Kit 991 sits on both TargetType 1 and 2 rows, and
  both the cast (on the pet) and the aura share it, but which body draws it is not in the
  tables (Unresolved 3).
- **No sustained channel, and no beam.** A "link" visual between Warlock and demon has no
  client counterpart.
- The cast hands are a model we already ship (Moonfire's).

---

## 10. Demonic Sacrifice, and the sacrifice buffs

### The sacrifice (18788)

**Chain:** Demonic Sacrifice 18788: `Effect 1` (instakill) on target 5 (the pet), cast
time 0, no duration → visual **1305**, **Banish's visual** (Banish ranks, Shackle Shatter,
Spirit Shock, Draw Spirit; 20 spells in TBC).

| Event | TargetType | Kit | Contents |
|---|---|---|---|
| (1,2) precast | 1 | 114 | shadow precast hands, anim 51 (never plays: instant) |
| (3,13) cast | 1 | 118 | `shadow_precast_low_hand.m2` @21/@22, anim 53 |
| (7,8) state | 1 and 2 (Era) / 2 only (TBC) | 1286 | `spells/banish_chest.m2` (165648, SVEN 855) @34 Chest; tint type 1 `#350035` (53, 0, 53), values 0 / 1 / 0; transparency type 14 = **0.30**, values 2 / 1000 / 0 |

- **There is no `(6,13)` row: nothing lands on the demon.** The only victim-side row is a
  `(7,8)` aura state, and Demonic Sacrifice applies no aura, so it is expected never to
  play (inferred; Unresolved 4). If it did, it would be Banish's look: `banish_chest.m2`
  (a 3 yd green / magenta disc pair, sequences 700 / 633 / 1100 ms) plus a dark purple
  tint and 30 % transparency.
- **What happens to the demon is the instakill.** The client draws the death, not a
  sacrifice.

### The buffs (one-shot on the Warlock)

| Buff | Client demon | Effects | Duration | Visual | Kits | Model |
|---|---|---|---|---|---|---|
| Burning Wish 18789 | Imp | aura 79 +15 % misc 4 (Fire); aura 112 | 30 min | 5790 | 217 / 218 / **(6,13) 5133** | `demonicsacrifice_imp_chest.m2` (165924) @Chest |
| Fel Stamina 18790 | Voidwalker | aura 20, period 4000 ms (health regen) | 30 min | 5792 | 217 / 218 / **5135** | `demonicsacrifice_voidwalker_chest.m2` (165926) |
| **Touch of Shadow 18791** | **Succubus** | aura 79 **+15 %** misc 32 (Shadow); aura 112 | 30 min | **5791** | 217 / 218 / **5134** | **`demonicsacrifice_succubus_chest.m2` (165925)** |
| Fel Energy 18792 | **Felhunter** | aura 21, period 4000 ms (mana regen) | 30 min | **5789** | 217 / 218 / **5132** | **`demonicsacrifice_felhunter_chest.m2` (165923)** |
| (TBC) Touch of Shadow 35701 | Felguard | aura 79 +10 % Shadow; aura 112; aura 21 | 30 min | 5791 | as 18791 | as 18791 |

- **None of the five buffs has a `(7,8)` row.** The buff draws once, at the chest, and then
  only the icon remains for 30 minutes.
- Each visual carries the shadow hand kits 217/218 (Omni). The buff is applied by the
  sacrifice, not cast, so those rows are expected not to play (inferred, as for the
  AS-333 poison procs).
- Kit 5132 is shared in TBC with one quest spell; the other three kits are each their
  buff's alone.

### `demonicsacrifice_felhunter_chest.m2` / `_succubus_chest.m2` (@Chest)

The two models share one rig and differ only in texture and colour (diffed line by line;
Era and TBC copies identical).

- **Sequence.** One sequence of **2367 ms**; global loops 1067 / 0 ms. Bone 2 rotates on
  the 1067 ms global loop (5 keys; read as one turn a loop, inferred). All blend 4.
- **Layout:** a horizontal three-fold triad around the chest. Three inner glow sources at
  **0.83 yd** from centre (0°, ±120°), and three outer ones at **1.75 yd**. The inner
  bones drift ±0.28 yd radially over 2.33 s.
- **Six ribbons** (two per inner source): `ribbonblur1bea_blue` (Felhunter) /
  `_purple` (Succubus), 32 edges/s, edge life **0.65 s** (width 0.33 yd, gravity −0.5)
  and **0.75 s** (width 0.17 yd, gravity +0.5); white colour, alpha 0 → **0.8 at 334
  ms**, hold to 1000 ms, → 0 at 2367 ms.
- **Inner emitters** (3): **100/s from 334 ms to 1000 ms**, ramping 0 → 100 over 0–334
  ms and back to 0 by 2367 ms; life 0.6 s, speed 0.83, size 0 → 0.07 → 0.
  - Felhunter: `greyglowball64`, `(0,30,255)` → `(107,142,255)` → `(126,172,255)`.
  - Succubus: `green_glow3`, `(60,0,255)` → `(124,107,255)` → `(126,126,255)`.
- **Outer emitters** (3): **50/s from 1000 ms to 1667 ms**, off by 2367; speed 0, gravity
  **−6.94** (read as rising), life 0.8 s, spawn area 0.14 / 0.42 / 0.69 yd, size 0.07 →
  0.03 → 0.01.
  - Felhunter: `(220,229,246)` → `(105,140,255)` → `(0,25,213)` (white to deep blue).
  - Succubus: `(226,220,246)` → `(123,105,255)` → **`(25,213,0)`** (white to violet to
    green).
- **Reading:** three glowing orbs spin around the Warlock's chest trailing ribbons, then
  motes stream up off the outer points, all in about 2.4 s. Blue for the Felhunter;
  violet with green-tipped motes for the Succubus.

### What this means for design (Demonic Sacrifice)

- **The sacrifice is a one-shot on the Warlock**, not a moment on the demon: shadow cast
  hands, then (when the buff applies) the triad at the chest. **No sustained channel** on
  either body; the 30-minute buff has no state.
- **Name vs colour:** our "Touch of Shadow from the Felhunter" pairs the Succubus's buff
  name and effect (+15 % Shadow) with the Felhunter. The client's Felhunter model is the
  blue one (Fel Energy); the Succubus's violet one is Touch of Shadow's. Both are one
  rig, so the choice is a colour choice.
- The demon's disappearance has no client visual beyond death.

---

## Sustained channels per body (budget count)

Counted from `(7,8)` rows only, for the effects this doc covers.

- **On a victim:**
  - **Aftermath** adds one: `firenova_state.m2` at the feet plus a red body tint, 5 s.
    It overlaps the feet channel the shipped slow ring uses and the body-tint channel.
  - **Siphon Life** adds one in name, but in Era it is Corruption's kit 535, already
    shipped, so a victim with both carries the same state twice. In TBC it is kit 234
    (violet chest glow, shared with Unstable Affliction).
  - **Shadow Vulnerability** adds none in the client (NO-VISUAL).
  - Shadowburn, Conflagrate: none (one-shot).
- **On the caster:**
  - **Amplify Curse** adds one at the Warlock's feet (kit 4013) for the buff's duration.
  - Shadow Trance, Touch of Shadow / Fel Energy, Soul Link, Master Demonologist: none.
- **On the demon:** none. Soul Link and Master Demonologist draw no state; Demonic
  Sacrifice's only state row is Banish's, on a spell with no aura.
- **Between bodies:** none. No effect here has a beam, missile or channel row (Soul Link,
  the Siphon Life heal).

## Unresolved

1. **Whether a `(7,8)` kit loops its model's Stand sequence** (Amplify Curse's 1500 ms
   flash, which is invisible after 467 ms of each pass), or holds it. The warlock DoT doc
   read Corruption's state as re-blooming on its 3000 ms loop; that reading was never
   checked against the client either.
2. **Which sequence a `(7,8)` model plays**: id 0 (Stand) or id 158. `firenova_state.m2`
   has its dust emitters off in Stand and at 20/s in 158, with 159 reading as an end
   burst; `banish_chest.m2` has the same 0 / 158 / 159 shape (700 / 633 / 1100 ms). The
   names of `AnimationData` 158 and 159 are not in the table (it has no name column).
3. **Who TargetType 1 and 2 denote** (carried from AS-333). It decides which body draws
   Soul Link's kit 991 (cast on the pet; aura area-applied) and matters because TBC drops
   the TargetType 1 `(7,8)` rows of Amplify Curse, Siphon Life, Aftermath, Immolate and
   Demonic Sacrifice.
4. **Whether a `(7,8)` row can play on a spell that applies no aura** (Demonic Sacrifice's
   Banish state on an instakill). Read here as never; not confirmed.
5. **Whether the precast and cast rows of a triggered spell play** (Touch of Shadow and
   the other sacrifice buffs carry kits 217/218; Shadow Trance and Aftermath carry none).
   Read here as not playing, as AS-333 read the poison procs.
6. **How long the client keeps a `(6,13)` impact kit alive** against models whose
   emitters run to the end of a long sequence: `fire_impactdd_chest.m2` (1900 ms, emitters
   held from 67 ms), `shadow_impactdot_med_head.m2` (8000 ms, 4 s particle life).
7. **`SpellProceduralEffect` `Value_1` to `Value_3`** (carried from AS-333): Aftermath's
   tint 0 / 0 / 1 (Era) vs 0.1 / 0.1 / 1 (TBC), Immolate's 0 / 0 / 0.2, Banish's tint 0 /
   1 / 0 and transparency 2 / 1000 / 0.
8. **The M2 particle gravity sign** (read here as positive = down, from Fire Nova's
   embers thrown up at 4.4 yd/s against 13.9) and the Era `0x800000` compressed-gravity
   encoding, which the parser does not decode.
9. **Rotation of the sacrifice triad**: bone 2's 5-key rotation on the 1067 ms global
   loop was counted, not decoded (quaternions not dumped).
10. **Models not parsed:** `fire_cast_hand.m2` / `fire_precast_hand.m2` (Conflagrate and
    Immolate hands), `magic_precast_hand.m2` beyond its emitter list, `banish_chest.m2`
    beyond its sequences and materials, `demonicsacrifice_imp_chest.m2` /
    `_voidwalker_chest.m2` (fetched, not read; same file size class as the two read).
11. Texture BLPs were not decoded. Colours above are colour tracks, not texture pixels.
