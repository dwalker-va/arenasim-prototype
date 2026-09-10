# AS-19: Curse of Weakness / Curse of Tongues victim visuals — client-data research

Follow-up measure pass to AS-15 (`2026-09-06-warlock-dot-client-data.md`), same
toolchain and same client build: WoW Classic Era **1.15.9.69547**, wago.tools DB2 CSVs
plus CASC file fetches, parsed out of the DB2 tables and the M2 binaries. No prose or
wiki sources were used as evidence.

Chain: `SpellName → SpellXSpellVisual → SpellVisual → SpellVisualEvent →
SpellVisualKit → SpellVisualKitEffect (EffectType 2) → SpellVisualKitModelAttach →
SpellVisualEffectName.ModelFileDataID`, then CASC fetch + M2/skin parse. Attachment
IDs as in AS-9: **20 = Head**, **34 = Chest**. Units: model units ≈ yards.

---

## 0. The headline: the card's premise was half wrong

AS-19 was written on the AS-15 aside that the curse family is uniformly *one-shot head
apparitions like CoA*. The joins say otherwise, in two places:

1. **Curse of Tongues is not a head apparition and is not apply-only.** It resolves to
   SpellVisual **339**, whose on-victim kits attach at the **CHEST (34)**, and it
   carries a `(7,8)` **persistent aura-state** row — kit 502 — the same event pair
   Corruption has and Curse of Agony lacks. CoT is a rune sigil at the chest with a
   sustained state, not a skull over the head.
2. **Curse of Weakness's caster-side kits are 217/218, not 114/118.** The AS-15 §5
   caster table lists CoW under the Directed pair (114 → anim 51, 118 → anim 53). The
   actual rows on visual 346 are the **Omni** pair, 217 → LoopAnim **52
   ReadySpellOmni** and 218 → LoopAnim **54 SpellCastOmni** — the Fear / SW:Pain pair.
   That is an erratum in AS-15 §5; caster-side is out of AS-19's scope, but it should
   be corrected before anything is built on that table.

The name-shuffle trap the card flagged is real and confirmed: **Curse of Weakness's
kit 719 loads `curseofmannoroth_head.m2`** (fdid 165854) and **Curse of Doom's kit 311
loads `curseofweakness_head.m2`** (fdid 165858). Trust the kit joins, never the
filenames.

## 1. Rank resolution and visual chain

| Spell | Player-rank SpellIDs | SpellVisual | Collapse? |
|---|---|---|---|
| Curse of Weakness | 702, 1108, 6205, 7646, 11707, 11708 | **346** (all, probability 1) | yes |
| Curse of Tongues | 1714, 11719 | **339** (all, probability 1) | yes |
| Curse of Agony (AS-15 baseline) | 980, 1014, 6217, 11711, 11712, 11713 | 824 | yes |

Neither visual has a missile (`HasMissile = 0`) — instant-apply, the on-victim kit
fires at cast completion with no travel phase, as with CoA.

## 2. Event rows

**Curse of Weakness — visual 346** (4 rows):

| StartEvent | EndEvent | TargetType | Kit | Role |
|---|---|---|---|---|
| 1 | 2 | 1 | 217 | precast loop (caster, **Omni**) |
| 3 | 13 | 1 | 218 | cast-launch (caster, **Omni**) |
| 6 | 13 | 1 + 2 | **719** | **impact** (on victim at apply) |
| — | — | — | — | **no (7,8) row — apply-only, like CoA** |

**Curse of Tongues — visual 339** (6 rows):

| StartEvent | EndEvent | TargetType | Kit | Role |
|---|---|---|---|---|
| 1 | 2 | 1 | 114 | precast loop (caster, Directed) |
| 3 | 13 | 1 | 118 | cast-launch (caster, Directed) |
| 6 | 13 | 1 + 2 | **503** | **impact** (on victim at apply) |
| 7 | 8 | 1 + 2 | **502** | **PERSISTENT AURA STATE** (apply→expire) |

## 3. On-victim models

### Kit 719 — Curse of Weakness IMPACT (the only CoW victim visual)

- **MODEL**: fdid **165854** → `spells/curseofmannoroth_head.m2`, name field
  `CurseofMannoroth_Head`, attach **20 = Head**, offset (0,0,0), scale 1.
- **TEXTURES (TXID order)**: `spells/skull_purple.blp`, `spells/genericglow64.blp`,
  **`spells/bone_purple.blp`**, `spells/starflash_grey.blp`, `spells/star5a.blp`,
  `spells/toonsmoke16.blp`.
- **SHAPE**: 310 verts, 9 submeshes / 9 batches, **all 8 materials additive**; one
  3000 ms one-shot sequence; **global sequences 433 ms and 267 ms — the same two
  flicker cycles as CoA**. Bbox min(−1.10,−1.21,−0.31) max(0.92,1.31,2.05); the mesh
  proper is a narrow cluster, vert extents x 0.42 / y 0.28 / z 0.82, max radius 0.21,
  spanning z 0.34–1.17 above the head attach.
  Skin-verified batch → texture → tint map:
  - submeshes 1/4 (52 verts, centre (0.08, 0, **0.81**)) `skull_purple` — one untinted
    shell + one tinted **violet (0.329, 0, 1.0)** → the cranium.
  - submeshes 0/3 (33 verts, centre (0.13, 0, **0.67**)) `skull_purple` — untinted +
    violet → the jaw, below and slightly forward.
  - submeshes 7/8 (54 verts, centre (**−0.14**, −0.01, 0.76)) **`bone_purple`** —
    untinted + violet → a bone element beside the skull, on the opposite side from the
    skull's own +0.13 offset.
  - submeshes 2/5/6 (16/8/8 verts, centre (0.18, 0, 0.81)) `genericglow64` glow quads,
    tints **green (0,1,0)**, **green (0,1,0)**, red (1,0,0).
  - Transparency tracks over the 3000 ms: `[0]` a flash layer spiking 0→1→0 within
    334 ms; `[1]` fade in over **134 ms**, hold, fade out 2267→2667; `[2]` hold, fade
    out 2400→2800. **On screen ~2.8 s** — identical envelope to CoA.
- **EMITTERS** (6, all Sphere at (0, 0, 0.83) above the head attach, vrange π, zero
  gravity):
  - [0] `starflash_grey`, **Add**, speed 0.83, life 0.75 s, rate **150/s**, area
    0.28×0.33, colour **white → (18,255,0) bright green → (186,255,181) pale green**,
    alpha 1 → 0.69 → 0, size 0.111 held then collapsing to 0.
  - [1] `star5a`, **Add**, same speed / life / rate / area, colour white →
    (161,255,154) → (12,255,0) — the mirrored green ramp.
  - [2] `toonsmoke16`, **Alpha**, speed 1.11, life 0.6 s, rate 50/s, colour black →
    (84,84,83) → (192,192,190), alpha 0 → 0.47 → 0, **growing** 0.139 → 0.25 → 0.556.
  - [3] `toonsmoke16`, **Alpha**, speed 1.22, life 1.0 s, rate 50/s, same colours,
    growing 0.194 → 0.306 → 0.611.
  - [4] `toonsmoke16`, **Add**, speed 1.17, life 0.8 s, rate 50/s, colour **violet**
    (121,0,198) → (162,140,245) → (114,0,255), alpha 0 → 0.47 → 0, growing 0.167 →
    0.278 → 0.583.
  - [5] `toonsmoke16`, **Add**, speed 1.28, life 1.2 s, rate 50/s, violet (110,0,234)
    → (146,116,209) → (156,0,255), alpha 0 → 0.47 → 0, growing 0.222 → 0.333 → 0.639.
- **READ**: at apply, a **violet skull-and-bone** materializes over the victim's head
  on CoA's exact envelope (134 ms in, gone by 2.8 s) and flickers on the same 433/267
  ms cycles — but the palette is inverted from CoA's: the shells are **violet**, the
  core glows are **green**, the sparks are **green** star motes, and the apparition
  swells inside violet blooms and grey alpha smoke. Then nothing for the rest of the
  two-minute curse.

### Kit 503 — Curse of Tongues IMPACT

- **MODEL**: fdid **165856** → `spells/curseoftongues_impact.m2`, attach **34 =
  Chest**, offset (0,0,0), scale 1.
- **TEXTURES**: `spells/aurarune_a.blp`, `spells/genericglow_black.blp`,
  `spells/aurarune8_mip1.blp`.
- **SHAPE**: 28 verts, 1 submesh / 1 batch → `aurarune8_mip1`, material **Add**; one
  1666 ms one-shot sequence; one **global sequence of 2000 ms** (the slow rune cycle).
  Bbox min(−1.22,−1.25,−0.29) max(1.27,1.24,1.06). The geometry is exactly:
  - **two horizontal rune discs**, ~1.77 units across (x −0.858…0.912, y −0.892…0.878)
    stacked at z = 0.225 and z = 0.044 — flat plates lying around the chest;
  - **five upright glyph cards**, each 0.424 × 0.424, on a ring of radius ≈ 0.70 about
    the chest axis at z −0.075…0.349. All five share one plane normal (+x) — they are
    translated copies, not rotated per position.
  - Mesh tint colour[0] = **(0.592, 0, 0.933) magenta-violet**, one tint for the whole
    mesh. Transparency track: 0 → **0.6** by 333 ms, hold to 1266 ms, out by 1533 ms,
    dead at 1666 ms. **Peak alpha is 0.6, not 1.0.**
- **EMITTERS** (2, both Plane, near-static, at (0.55, 0.01, 0.12) — offset ~0.55 units
  in FRONT of the chest, rate 6/s each):
  - [0] `aurarune_a`, **Add**, speed 0.056, life 1.2 s, colour (240,30,65) pink-red →
    (166,52,232) → (170,30,236) violet, alpha 0 → 0.72 → 0.05, **shrinking** 0.708 →
    0.339 → 0.097.
  - [1] `genericglow_black`, **Alpha**, speed 0.033, life 0.63 s, colour (30,30,30) →
    (33,8,41) → (170,30,236), alpha 0 → 0.70 → 0.05, **growing** 0.303 → 0.567 → 0.781.

### Kit 502 — Curse of Tongues AURA STATE (the persistent look)

- **MODEL**: fdid **165857** → `spells/curseoftongues_state_chest.m2`, attach **34 =
  Chest**, scale 1.
- **TEXTURES**: `spells/aurarune_a.blp`, `spells/genericglow_black.blp`,
  `spells/demonrune5backup.blp`.
- **SHAPE**: **4 verts — one quad**, 1 batch → `demonrune5backup`, material **Add**.
  The quad stands in the YZ plane at constant **x = 0.590** (0.31 × 0.31), i.e. a
  single small rune plate hovering ~0.6 units in front of the victim's chest. Mesh
  tint colour[0] = the same **(0.592, 0, 0.933)** magenta-violet.
  - Two sequences, both **8166 ms**. Transparency track: 0 → 1.0 by 233 ms, hold to
    1233 ms, out by 1866 ms, then **zero for the remaining ~6.3 s of the loop** — the
    state does not glow continuously, it **re-flashes for ~1.9 s once every 8.166 s**.
  - The same two emitters as kit 503, at the same offsets and rates.
- **READ (the persistent CoT victim)**: a magenta-violet rune plate blinks into
  existence in front of the victim's chest for about two seconds, then goes dark for
  six, then blinks again — for the whole 30 s curse.

## 4. Distinctness — what the client itself uses to tell the three curses apart

| Axis | Curse of Agony (AS-15) | Curse of Weakness | Curse of Tongues |
|---|---|---|---|
| Attach | Head (20) | Head (20) | **Chest (34)** |
| Silhouette | skull (136-vert main shell) | skull **+ bone** | **rune circle** (2 discs + 5 glyph cards) |
| Shell tint | **red** (0.96, 0, 0) | **violet** (0.329, 0, 1.0) | **magenta-violet** (0.592, 0, 0.933) |
| Core glow tint | **yellow** (1.0, 0.96, 0) | **green** (0, 1, 0) ×2, red ×1 | (no separate glow mesh) |
| Sparks | red-orange `red_star2`, 180/s | **green** `starflash_grey`/`star5a`, 150/s ×2 | slow violet rune motes, 6/s ×2 |
| Envelope | 134 ms in, gone by 2.8 s | **identical**: 134 ms in, gone by 2.8 s | 333 ms in to **α 0.6**, gone by 1.67 s |
| Rhythm | flicker 433 / 267 ms | flicker **433 / 267 ms** (same) | **2000 ms** rune cycle, no flicker |
| Persistent state | **none** | **none** | **yes** — kit 502, re-flash 1.9 s per 8.166 s |

So the client differentiates by **palette** (red+yellow / violet+green / magenta-violet)
and **attach + silhouette** (skull / skull+bone at the head / rune circle at the chest),
not by envelope: CoA and CoW deliberately share their timing and flicker down to the
millisecond.

## Negative findings (exact)

- **No table 404s**; every join resolved. Three new M2s (165854, 165856, 165857) and
  their three skins (493034, 493385, 493016) fetched successfully from CASC at
  1.15.9.69547.
- **Curse of Weakness has no aura-state kit** — no `(7,8)` row on visual 346. A CoW
  victim shows nothing after the ~2.8 s apply apparition, for the whole 2-minute curse.
  Apply-only, exactly like CoA.
- **Curse of Tongues DOES have one** — kit 502 on the `(7,8)` pair. This contradicts
  the AS-19 card's premise. See "shipped vs measured" below.
- **No forced victim animation** — kits 719, 503 and 502 contain no EffectType-6
  entries; the caster-side kits do (CoW 217/218 carry LoopAnim 52/54).
- **Curse of Doom** (visual 5019, kit 311, `curseofweakness_head.m2`) is also a
  one-shot head apparition on the Directed caster pair. ArenaSim has no Curse of Doom,
  so it was only pulled as the name-shuffle cross-check.

## 5. Shipped vs measured (what AS-19 implemented, and what it did not)

AS-19 shipped both **apply apparitions**, which is the card's stated deliverable:

- **Curse of Weakness** — the violet skull-and-bone above the head, on CoA's shared
  envelope and flicker, with green core glow, green star sparks and swelling violet
  blooms.
- **Curse of Tongues** — the magenta-violet rune circle at the chest: two flat discs
  plus five upright glyph tablets on a slowly turning ring, peaking at α 0.6 and gone
  by 1.67 s, with its two slow rune-mote emitters.

Deliberate transcription deviations, all for reasons the codebase already documents:

- **CoW's two grey `toonsmoke16` ALPHA emitters are omitted.** They are alpha-blend
  smoke: in the client they *remove* light from the apparition's interior. Reproducing
  them additively — the house convention outside Corruption's single blessed Blend
  exception — would invert their sign and make the apparition brighter, which is the
  opposite of what they do. The two additive violet emitters are kept.
- **The paired emitters are merged.** CoW's two green star emitters are identical in
  speed / life / rate and differ only in the direction of their white↔green ramp;
  they become one stream. Its two additive violet bloom emitters differ by ~10 % in
  speed, life and size; they become one at the pair's mid values.
- **CoW's single red glow quad is dropped** (the other two glow quads are green). A red
  accent on the head apparition is precisely the channel Curse of Agony owns, so
  keeping it works against the distinctness the rest of the data is spending.
- **CoT's five glyph cards face outward** instead of sharing one plane normal, so the
  ring reads as a ring from the arena camera's bearing rather than edge-on from half of
  it.
- **CoT's persistent state (kit 502) is NOT implemented.** The card scoped "apply
  apparitions", and adding a fourth sustained on-victim channel is a stacking-budget
  decision, not a transcription. The measured constants are in §3 above and the work is
  a small, well-specified follow-up card.
