# AS-15: Warlock DoT Aura Visuals (Corruption / Curse of Agony) — Client-Data Research

All findings are from WoW Classic Era build **1.15.9.69547** client data (wago.tools DB2
CSVs + CASC file fetches), parsed directly from the DB2 tables and M2 model binaries.
No prose/wiki sources were used as evidence.

**Amended 2026-09-11 (AS-37).** §5's caster-side table has been re-derived row by
row; one row (Curse of Weakness) was wrong, and the generalisation it was inferred
from was wrong with it. Read the erratum box at the top of §5 before transcribing
anything from this doc. §1–§4 were re-joined at the same time and held unchanged.

## Method notes (what was actually done)

- Same toolchain as AS-9/AS-11: `SpellName → SpellXSpellVisual → SpellVisual →
  SpellVisualEvent → SpellVisualKit → SpellVisualKitEffect (EffectType 2) →
  SpellVisualKitModelAttach → SpellVisualEffectName.ModelFileDataID`; EffectType 6 →
  SpellVisualAnim (caster body animation); EffectType 5 = non-model (sound-adjacent).
  New CASC fetches via `wago.tools/api/casc/<fdid>?version=1.15.9.69547`, UA
  `curl/8.7.1`. Scripts: `as15_chain.py`, `as15_events.py`, `as15_kits.py`,
  `as15_deep.py`, `as15_kits2.py` (this session) plus the existing `m2parse.py`,
  `particles.py`, `skinparse.py`.
- **New event-row vocabulary established this session — the AURA STATE pair.**
  Table-wide histogram of SpellVisualEvent `(StartEvent, EndEvent)`:
  `(1,2)` precast loop, `(3,13)` cast one-shot, `(6,13)` impact (dup'd TargetType 1+2)
  — all as in AS-9 — plus **`(7,8)`, 2622 rows table-wide, dup'd TargetType 1+2: the
  persistent on-victim aura state**, a kit that stays alive from aura-apply (7) to
  aura-remove (8). Cross-validation: Immolate carries a `(7,8)` kit whose model is
  literally `spells/immolate_state_base.m2` (the burning-victim flames); Fear's `(7,8)`
  kit 4031 attaches `fear_state_base.m2` + `fear_state_head.m2`; Renew (a HoT with a
  visible persistent glow) has one; Rejuvenation and SW:Pain — famously *invisible*
  while applied in the era client — have **none**. `(11,12)` is the channel-target pair
  (seen on Drain Life), `(9,10)`/`(4,5)` are area/positioner shapes; **no event pair in
  the schema fires per periodic tick** (negative finding, see below).
- New deep-parse this session (`as15_deep.py`): per-particle **colorTrack /
  alphaTrack / scaleTrack** (M2PartTrack fixed16-keyed, record offsets 260/276/292 of
  the 492-byte particle record), M2Color mesh tint tracks (header offset 72),
  texture-weight transparency tracks (offset 88), sequence durations (offset 28,
  64-byte records), global sequences (offset 20), SFID skin fdids. colorTrack values
  are RGB 0–255 floats over normalized particle life.
- Attachment IDs (AS-9 mapping, reused): **19 = Base** (feet), **20 = Head**,
  **21/22 = spell left/right hand**, **34 = Chest**. Units: model units ≈ yards,
  a character is ~2 units tall.

---

## 1. Rank resolution — do ranks share visuals?

| Spell | Player-rank SpellIDs | SpellVisual | Collapse? |
|---|---|---|---|
| Corruption | 172, 6222, 6223, 7648, 11671, 11672, 25311 | **381** (all) | yes |
| Curse of Agony | 980, 1014, 6217, 11711, 11712, 11713 | **824** (all) | yes |
| Unstable Affliction (SoD) | 427717 | **381** (single row, prob 1) | — shares Corruption's |

Neither visual has a missile: SpellVisual rows 381 and 824 have `HasMissile=0` and an
empty missile set — both spells are instant-apply; the impact kit fires at cast
completion with no travel phase (unlike Shadow Bolt).

**Siphon Life (18265) also sits on visual 381** — the whole Corruption look (impact +
state) is shared three ways in the client: Corruption, Siphon Life, SoD Unstable
Affliction.

## 2. Event-row shapes (the structural finding)

**Corruption — visual 381** (6 rows):

| StartEvent | EndEvent | TargetType | Kit | Role |
|---|---|---|---|---|
| 1 | 2 | 1 | 114 | precast loop (caster) |
| 3 | 13 | 1 | 118 | cast-launch (caster) |
| 6 | 13 | 1 + 2 | **117** | **impact** (on victim at apply) |
| 7 | 8 | 1 + 2 | **535** | **PERSISTENT AURA STATE** (on victim, apply→expire) |

**Curse of Agony — visual 824** (4 rows):

| StartEvent | EndEvent | TargetType | Kit | Role |
|---|---|---|---|---|
| 1 | 2 | 1 | 114 | precast loop (caster) — same kit as Corruption |
| 3 | 13 | 1 | 118 | cast-launch (caster) — same kit as Corruption |
| 6 | 13 | 1 + 2 | **884** | **impact** (on victim at apply) |
| — | — | — | — | **NO (7,8) row — CoA has no persistent aura visual** |

Two negative findings, both design constraints:

1. **CoA is apply-moment-only.** A victim carrying Curse of Agony looks completely
   normal for the whole 24 s after the ~3 s apply flourish ends. The era client sold
   "cursed" entirely through the one-shot skull apparition + the debuff icon.
2. **Neither spell (nor any spell in this schema) has a periodic-tick visual.** The
   SpellVisualEvent vocabulary has no tick trigger; ticks exist only as combat-log
   damage while the state kit (if any) loops on. Corruption's "throb" comes from the
   state model's own animation cycle (below), not from tick events.
3. Neither on-victim kit (117, 535, 884) carries an EffectType-6 anim — **no forced
   victim animation**; the victim only plays its normal combat hit reactions.

## 3. On-victim models — full analysis

### Kit 117 — Corruption/UA/Siphon Life IMPACT (apply moment)

- **MODEL**: fdid **166796** → `spells/shadow_impactdd_low_chest.m2`, attach **34 =
  Chest**, offset (0,0,0), scale 1. (The generic shadow direct-damage impact — note
  Shadow Bolt's impact kit 219 does NOT use this; it uses fdid 165890
  `deathcoil_impact_chest.m2`. This "low" impact is the budget shadow hit.)
- **TEXTURES (TXID order)**: `spells/shadowring.blp`,
  `item/objectcomponents/weapon/flare.blp`, `spells/gradient64flipa.blp`,
  `spells/tooncloud64a.blp`.
- **SHAPE**: **zero mesh vertices — pure particle model.** 0 materials, 0 ribbons,
  7 emitters. One sequence, 1000 ms one-shot.
  Bbox min(−1.02,−1.71,−1.94) max(1.12,2.34,1.77) — a ~2-unit-radius envelope
  centred on the chest.
- **EMITTERS** (all life ≤ 1 s, zero gravity; the whole thing is over in ~1.75 s):
  - [0] `shadowring`, Plane, blend **Add**, speed 0, life 0.75 s, rate 20/s,
    scale 0.03→0.83→**1.25** over life, color (51,115,0)→(63,22,194)→(33,13,105),
    alpha 0→0.59→0 — an **expanding shadow ring** at the chest, green flashing to
    violet then fading dark.
  - [1] `flare`, Sphere, **Add**, speed 2.22, vrange π (all directions), life 1.0 s,
    rate 100/s, tiny (0.03→0.07→0.03), color (0,209,0)→(102,35,225)→(4,0,84) —
    a dense sphere of **green→violet spark motes** blowing outward.
  - [2] `gradient64flipa`, Sphere, **Alpha**, speed 2.22, life 0.8 s, rate 50/s,
    scale 0.003→0.14→0.03, color black — **darkening streaks** (alpha-blend, so they
    dim what's behind them).
  - [3],[4] `gradient64flipa`, Sphere, **Add**, speed 2.22, life 0.8 s, rate 30/s
    each, colors violet→green and green→violet→dark (mirrored ramps).
  - [5] `flare`, Sphere, **Alpha**, speed 2.22, life 0.5 s, rate 80/s,
    scale 0.08→0.33→0.42, color (4,0,84)→(33,16,88)→(0,209,0) — dark-violet puffs
    ending on a green pop.
  - [6] `tooncloud64a`, Sphere, **Alpha**, slow (0.28), life 0.8 s, rate 75/s,
    scale 0.03→0.17→0.28, color (73,0,119)→(160,120,191)→(148,148,185),
    alpha 0→0.39→0 — **lingering grey-violet smoke**.
- **READ**: at apply, a shadow ring snaps outward from the victim's chest while a
  burst of green-and-violet sparks and dark alpha-blend smoke churns for under a
  second. Palette is the classic Warlock two-tone: **sickly green flashing into
  violet, sinking to near-black**.

### Kit 535 — Corruption/UA/Siphon Life AURA STATE (the persistent look)

- **MODEL**: fdid **166637** → `spells/pestilence_impact_chest.m2` (name is legacy —
  despite "impact_chest" it is attached here at **20 = Head**, scale 1, and used as a
  looping state). Single model; nothing at feet or chest.
- **TEXTURES**: `spells/clouds8x8fade.blp`, `item/objectcomponents/weapon/flare.blp`,
  `spells/black_glow2.blp`.
- **SHAPE**: 15 verts, 1 submesh, 1 batch → texture `black_glow2`, material blend
  **Alpha(blend) — the only non-additive material seen in this entire research series
  (AS-9/AS-11/AS-15)**. Vert z-buckets: 3 verts near z≈0, 6 at mid, 6 at z≈1.0, max
  radius 1.02 — a stack of dark glow planes ~1 unit across. Bbox
  min(−0.99,−1.98,−1.97) max(1.16,1.99,1.97).
  - Mesh tint track color[0]: white RGB, **alpha keyed 1.0→0.0 over 0–3000 ms** of
    the 3000 ms anim-0 sequence — the dark shroud re-blooms and fades **every 3 s
    loop**. This is the "pulse" of Corruption: a slow 3 s throb, not a tick flash.
  - Sequences: anim 0 = 3000 ms (the loop), plus a second sequence animID 158,
    8600 ms (unused-alias shape; flags 0x820). One **global sequence of 3300 ms**
    drives emitter cycling independent of the anim loop.
- **EMITTERS** (2, both blend **Alpha** — again darkening, not additive):
  - [0] `clouds8x8fade`, Sphere, speed 0.28 (very slow drift), vrange π, life
    **2.5 s**, rate 20/s, emission area 0×0.28, scale **0.22→0.31→0.69** growing over
    life, color **(18,55,0)→(106,146,0)→(163,196,64)** — near-black green rising to
    sickly yellow-green — alpha 0.39→1.0→0. Slow, swelling **murk-green cloud wisps**
    that hang around the victim's upper body.
  - [1] `flare`, Sphere, speed 3.33, life 0.6 s, rate **150/s**, area 0.28×0.28,
    shrinking 0.11→0.06→0.03, color (49,102,1)→(99,136,20)→(115,152,1), alpha
    1→1→0 — a constant fizz of **small green motes** streaming off the victim.
- **READ (the persistent Corruption victim)**: a dark alpha-blended shroud sits over
  the victim's head/torso, re-blooming on a 3-second cycle, wrapped in slowly
  swelling black-green clouds and a continuous spray of small green motes. Because
  every layer is alpha-blend, **Corruption visibly DARKENS its victim** — the one
  effect in the researched set that dims instead of glows. Hue is green-black, not
  violet: in the era client the violet belongs to the apply/impact moment, the
  *carried* state reads as sickly green murk.

### Kit 884 — Curse of Agony IMPACT (the only CoA victim visual)

- **MODEL**: fdid **165852** → `spells/curseofagony_head.m2` — a dedicated model —
  attach **20 = Head**, scale 1. (The curse family shuffles names: Curse of
  Weakness's kit 719 uses `curseofmannoroth_head.m2` 165854, Curse of Doom's kit 311
  uses `curseofweakness_head.m2` 165858. Both of those are one-shot head
  apparitions like CoA — but **that is not a family property**: Curse of Tongues
  attaches at the CHEST and carries a persistent `(7,8)` state kit, measured in
  AS-19 `2026-09-10-warlock-curse-client-data.md` §0. Join the curse you mean.)
- **TEXTURES (TXID order)**: `spells/skull.blp`, `spells/genericglow64.blp`,
  `spells/skull_purple.blp`, `spells/red_star2.blp`, `spells/genericglow2b.blp`,
  `creature/golemharvest/red_glow3.blp`.
- **SHAPE**: 286 verts, 7 submeshes / 7 batches, **all additive**; one 3000 ms
  one-shot sequence; global sequences 433 ms and 267 ms (fast glow flicker).
  Bbox min(−0.19,−0.87,−0.02) max(0.32,0.90,1.72) above the head attach.
  Skin-verified batch→texture→tint map:
  - batches 0,1,3: `skull.blp` meshes (33+52+136 verts — the 136-vert submesh 3 is
    the main skull), mesh tints **red** (0.95,0.02,0.02) / (0.97,0.03,0.03) /
    (0.96,0,0).
  - batch 4: `skull_purple.blp` duplicate skull shell (33 verts), tint (0.99,0,0).
  - batches 2,5,6: `genericglow64` glow quads, tints **yellow** (1.0,0.96,0) /
    (1.0,0.92,0) and red (1.0,0,0).
  - Transparency tracks (the apparition's envelope over the 3000 ms): fade in over
    **134 ms**, hold, fade out between ~2270–2800 ms → **the skull is on screen
    ~2.8 s** and the first track (a flash layer) spikes 0→1→0 within 334 ms.
- **EMITTERS** (3, all **Add**):
  - [0] `red_star2`, Sphere at (0,0,0.83) (above the head), speed 0.83, life 0.75 s,
    rate **180/s**, area 0.28×0.33, color **white→(239,83,35)→(201,0,0)**
    (white→orange→red), shrinking 0.13→0.11→0.025 — a dense crackle of red-orange
    star sparks around the skull.
  - [1] `genericglow2b`, Plane at z=0.80, near-static (speed 0.06), life 0.55 s,
    rate 10/s, color (255,25,25)→(253,3,3)→(248,163,15), **growing 0.12→0.66→0.87**,
    alpha 0→0.72→0 — pulsing red glow blooms behind the skull.
  - [2] `red_glow3`, Plane at z=0.25, speed **−1.11 (downward)**, life 1.0 s,
    rate 20.7/s, area 0.56×0.56, color constant white, alpha 0.18→1→1, shrinking
    0.13→0.008 — glow motes sinking down over the victim's face/chest.
- **READ**: at apply, a **skull apparition materializes above the victim's head in
  ~0.13 s** — a purple skull texture shell tinted deep red with yellow-white core
  glows — crackles with red-orange star sparks and red glow pulses (flickering on
  433/267 ms global cycles), sheds glow downward over the face, and dissolves by
  ~2.8 s. Then nothing for the remaining ~21 s of the curse. Palette is
  **red-violet + yellow core**, clearly warmer than any other shadow visual.

## 4. Design contrast — Corruption vs CoA vs Unstable Affliction

What the client data actually distinguishes:

| Axis | Corruption | Curse of Agony | Unstable Affliction (SoD) |
|---|---|---|---|
| Apply moment | green→violet shadow ring + sparks at **chest** (~1 s) | red skull apparition above **head** (~2.8 s) | identical to Corruption (same visual 381) |
| Persistent state | **yes** — kit 535, dark green-black shroud + motes, 3 s pulse, **alpha-blend (darkens)** | **none** | identical to Corruption |
| Tick visual | none (schema has no tick events) | none | none |
| Expire moment | state kit ends at event 8 — no separate expire flourish | nothing (nothing was showing) | as Corruption |
| Blend character | darkening (Alpha) | glowing (all Add) | as Corruption |
| Palette | green-black (state), green+violet (apply) | red-violet skull, yellow core, red-orange sparks | as Corruption |
| Attach | impact 34 Chest, state 20 Head | 20 Head | as Corruption |

**The UA finding is the important one for AS-15**: SpellXSpellVisual has exactly one
row for 427717 → visual 381, probability 1 — **the client gives Unstable Affliction
zero visual identity of its own**; a UA victim is pixel-identical to a Corruption
victim. ArenaSim's implemented deep-violet pulsing glow for UA is therefore an
authored invention with no client-data counterpart — and it happens to occupy the
violet channel that the *client* spends on Corruption's apply moment, not its state.

**Three-DoTs-on-one-victim readability (the ArenaSim constraint).** The client data
itself demonstrates the differentiation axes that don't spend new hues:

1. **Blend mode**: Corruption is the era's only darkening aura — an alpha-blend
   shroud that dims the victim. That axis (darken vs glow) is free: no hue budget
   consumed, and it survives stacking because darkening composes under additive
   layers rather than competing with them.
2. **Silhouette/moment**: CoA's identity is a *shape at a moment* (skull at apply,
   nothing sustained). If ArenaSim wants sustained CoA readability the client offers
   a faithful extension: re-fire a reduced skull/glow at the curse's ramp
   breakpoints — but as-shipped-in-1.15, apply-only is the authentic behaviour.
3. **Pulse period**: Corruption's state throbs at 3.0 s (mesh alpha loop) with a
   3.3 s particle cycle — slow murk. UA's authored violet pulse can key off a
   different period; period is a hue-free channel.
4. **Palette within the already-spent Warlock range**: the data's Warlock shadow
   space spans green-black (Corruption state) → violet (shadow impacts, UA's
   authored glow) → red-violet/yellow (CoA). All three corners already exist in the
   client's own palette; green FCT/gameplay reservations are avoided because
   Corruption's state green is near-black murk (18–163 luminance ramp), not signal
   green.

## 5. Caster-side — precast/cast kits and body anims

> **⚠ ERRATUM, AND HOW IT HAPPENED: ONE ROW OF THIS TABLE WAS NEVER JOINED.**
>
> The 2026-09-06 version of this table put **Curse of Weakness on the Directed
> pair** (114 → 51, 118 → 53). It is on the **OMNI** pair: visual 346's caster rows
> are `(1,2)` kit **217** → LoopAnim **52 ReadySpellOmni** and `(3,13)` kit **218**
> → LoopAnim **54 SpellCastOmni**. Found by the AS-19 measure pass, corrected and
> audited here (AS-37, 2026-09-11).
>
> **Root cause is NOT the curse family's filename shuffle** — that trap is real,
> separate, and still stands (kit 719 loads `curseofmannoroth_head.m2`, kit 311
> loads `curseofweakness_head.m2`; §3). The cause here is that the row was never
> derived at all. The AS-15 session ran the per-spell `SpellVisualEvent` join for
> exactly three spells — `as15_chain.py` hardcodes the Corruption, CoA and UA rank
> lists — and inspected kits 217/218 only as a Fear / SW:Pain *comparison*
> (`as15_kits2.py`). CoW's visual ID (346) came in from the impact-kit comparison
> pass; its caster pair was then filled in from the belief that Directed is the
> shadow-school default with Fear/SW:Pain as the lone exception.
>
> **That belief is false.** The book sweep below shows the era Warlock book split
> roughly half and half, with the curse family itself split down the middle.
> **These rows are per-visual DB2 data and no school-level rule generates them:
> join each spell, or do not list it.**
>
> Every row below was re-derived on 2026-09-11 from
> `SpellName → SpellXSpellVisual → SpellVisual → SpellVisualEvent →
> SpellVisualKit → SpellVisualKitEffect (EffectType 6) → SpellVisualAnim`, against
> CSVs re-fetched from wago.tools at build 1.15.9.69547 and verified byte-identical
> to the local copies. **Status** is the verdict against the 2026-09-06 table.

| Spell | Visual | Precast kit → body anim | Cast kit → body anim | Status |
|---|---|---|---|---|
| Corruption / Siphon Life / UA | 381 | 114 → **51 ReadySpellDirected** | 118 → **53 SpellCastDirected** | held |
| Curse of Agony | 824 | 114 → 51 | 118 → 53 | held |
| Shadow Bolt | 64 | 114 → 51 | 118 → 53 | held |
| **Curse of Weakness** | 346 | **217 → 52 ReadySpellOmni** | **218 → 54 SpellCastOmni** | **MOVED** (was 114/118 → 51/53) |
| Curse of Doom | 5019 | 114 → 51 | 118 → 53 | held |
| Fear | 336 | 217 → **52 ReadySpellOmni** | 218 → **54 SpellCastOmni** | held |
| Shadow Word: Pain (Priest) | 71 | 217 → 52 | 218 → 54 | held |

Both pairs attach the same hand model to **both** spell hands (21+22) — verified on
all four kits 114/118/217/218: fdid **166807** → `spells/shadow_precast_low_hand.m2`
— 0 verts, 3 emitters: green flame licks (`fire1a2` tinted (32,255,2)→black, Alpha
blend, life 1.2 s) plus two constantly pulsing `shockwave10` rings, one **bright
green** (18,255,0) and one **violet** (114,0,255), on 0.5–1.2 s global cycles. So a
casting Warlock's hands burn green-and-violet regardless of spell; the pair choice
changes only the **body** anim — Directed reaches one hand toward the target, Omni
presents both hands forward (AS-11's naming).

### The book sweep (why there is no school rule)

Every era Warlock player spell, rank 1, resolved the same way. Grouped by what the
caster rows actually say:

| Body-anim pair | Spells (visual) |
|---|---|
| **Directed** — 114 → 51 / 118 → 53 | Shadow Bolt (64), Death Coil (**64 — shares Shadow Bolt's visual**), Corruption / Siphon Life (381), Curse of Agony (824), Curse of Recklessness (1265), Curse of Tongues (339), Curse of the Elements (785), Curse of Doom (5019), Banish (1305) |
| **Omni** — 217 → 52 / 218 → 54 | **Curse of Weakness AND Curse of Shadow (both 346)**, Fear (336), Life Tap (1225), Shadow Ward (343), Demon Skin / Demon Armor (130), Unending Breath (352), Enslave Demon (1266), Inferno (4859) |
| Mixed / other | **Shadowburn (3057): precast 114 → 51 Directed, cast 218 → 54 Omni** — the pair is not atomic (Priest Mind Blast shares visual 3057). Howl of Terror (4801): precast 217 → 52, cast kit 389 → LoopAnim 55 (unnamed in this series). Immolate (46) and Rain of Fire (329): precast kit 60 → 52 Omni. Searing Pain / Conflagrate / Soul Fire: precast 30 → 51, cast 38 → 53 (the fire hand kits, Directed). Summons: precast 137 → 52 Omni. Drain Life / Drain Mana / Drain Soul / Health Funnel: **no `(1,2)` row at all**, cast kit → LoopAnim 124 (the channel anim) |

The curse family alone spans all three groups, so "it is a curse" predicts nothing.

**Visual 346 is a shared *weakening-curse* visual, not a Curse of Weakness visual.**
Its spell set includes Curse of Shadow, Priest **Devouring Plague**, Hex of Weakness,
Voodoo Hex, Curse of Mending and Enfeeble. Anything transcribed from kit 719 (§3 of
the AS-19 doc — the violet skull-and-bone) is therefore the look of that whole
family, which is a reason to keep it generic rather than read it as CoW-specific.

### Re-derivation audit of the rest of the doc (AS-37)

Re-joined from the same CSVs, all **held**, no changes:

- **§1 ranks** — Corruption's seven player ranks and UA 427717 → visual 381
  (probability 1, single row for 427717); CoA's six ranks → 824; Siphon Life 18265
  → 381; `HasMissile = 0` on both 381 and 824.
- **§2 event rows** — visual 381 has exactly the six rows listed (114 / 118 /
  117 ×2 TargetType / 535 ×2); visual 824 has exactly four and **no `(7,8)` row**.
- **§3 models** — kit 117 → fdid 166796 `shadow_impactdd_low_chest.m2` attach 34;
  kit 535 → 166637 `pestilence_impact_chest.m2` attach 20; kit 884 → 165852
  `curseofagony_head.m2` attach 20; all scale 1. The filename-shuffle cross-checks
  resolve as documented (719 → 165854 `curseofmannoroth_head.m2`, 311 → 165858
  `curseofweakness_head.m2`, Shadow Bolt's 219 → 165890
  `deathcoil_impact_chest.m2`).
- **§3/§4 "no forced victim animation"** — kits 117, 535 and 884 carry zero
  EffectType-6 entries.
- Nothing in this audit was unreachable: every join resolved, so there is no row
  left silently unverified.

No shipped code was built from the wrong row — the Warlock effects in
`rendering/effects/warlock_dots.rs` and `affliction.rs` are victim-side only, and the
one caster-side body-posture implementation in the repo (`heal_cast.rs`) is the AS-11
Holy Omni pair, which is unaffected.

## Negative findings (exact)

- **No table 404s**; every join resolved. All needed DB2 CSVs were already local;
  five new M2s + two skins fetched successfully from CASC at 1.15.9.69547.
- **Curse of Agony has no aura-state kit** — no `(7,8)` SpellVisualEvent row on
  visual 824. A CoA victim shows nothing after the ~2.8 s apply skull.
- **No periodic-tick visual mechanism exists in the schema** for either spell (or
  any spell) — no per-tick StartEvent value; DoT throb, where present, is the state
  model's own animation loop.
- **Unstable Affliction (427717) has no distinct visual** — single SpellXSpellVisual
  row pointing at Corruption's visual 381.
- **No forced victim animation** — kits 117/535/884 contain no EffectType-6 entries.
- **(AS-37) No school-level rule generates the caster body-anim pair.** The era
  Warlock book splits roughly evenly between the Directed and Omni pairs, the curse
  family is split across both, and Shadowburn mixes one of each — so the pair cannot
  be inferred from spell school or spell family and must be joined per visual (§5).
- **(AS-37) No extra visual to find for CoW.** Visual 346 carries no `(7,8)` row;
  the audit turned up no unreached or ambiguous join anywhere in this doc.

## Artifacts (scratchpad)

- Models: `m2_166796.bin` (shadow_impactdd_low_chest), `m2_166637.bin`
  (pestilence_impact_chest = Corruption state), `m2_165852.bin` (curseofagony_head),
  `m2_166807.bin` (shadow_precast_low_hand), `m2_166403.bin` (immolate_state_base,
  cross-check), `m2_165890.bin` (deathcoil_impact_chest = Shadow Bolt impact,
  cross-check); skins `skin_493657.bin`, `skin_492330.bin`.
- Scripts: `as15_chain.py`, `as15_events.py`, `as15_kits.py`, `as15_deep.py`
  (adds colorTrack/alphaTrack/scaleTrack, mesh tints, transparency tracks,
  sequences, SFID), `as15_kits2.py`.
- AS-37 audit (2026-09-11): `join.py` — name-driven, no hardcoded spell IDs; walks
  every SpellXSpellVisual row of a spell name and prints each `(StartEvent,
  EndEvent, TargetType)` with its kit's EffectType-6 anim and EffectType-2 model
  attach, which is what makes an unjoined row impossible to fake. `sweep.py` — the
  era Warlock book at rank 1, printing each spell's name back from `SpellName` as a
  self-check on the ID list. Run against CSVs re-fetched from
  `wago.tools/db2/<table>/csv?build=1.15.9.69547` with a browser User-Agent
  (`urllib` gets a 403) and `cmp`-verified byte-identical to the local copies.
