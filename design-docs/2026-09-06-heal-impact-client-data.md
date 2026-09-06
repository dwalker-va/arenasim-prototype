# AS-9: Classic Era Heal Landing-Impact Visuals — Client-Data Research

All findings are from WoW Classic Era build **1.15.9.69547** client data (wago.tools DB2
CSVs + CASC file fetches), parsed directly from the DB2 tables and M2 model binaries.
No prose/wiki sources were used as evidence.

## Method notes (what was actually done)

- Build: `wago.tools/api/builds` → `wow_classic_era` → `1.15.9.69547`.
- Chain: `SpellXSpellVisual[SpellID] → SpellVisual[ID] → SpellVisualEvent[SpellVisualID]
  → SpellVisualKit → SpellVisualKitEffect → SpellVisualKitModelAttach →
  SpellVisualEffectName.ModelFileDataID`.
- **Recipe correction discovered en route**: in this build the model-carrying join is
  `SpellVisualKitEffect.EffectType == 2`, whose `Effect` column is a
  **SpellVisualKitModelAttach** ID; that row's `SpellVisualEffectNameID` yields the
  model fdid. (EffectType 6 = sound-like IDs, EffectType 5 = non-model; neither resolves
  in SpellVisualEffectName.)
- **Impact-slot evidence**: every heal visual carries three event-row shapes in
  SpellVisualEvent:
  - precast loop: `StartEvent=1, EndEvent=2`, `TargetType=1` only (caster, during cast)
  - cast-launch one-shot: `StartEvent=3, EndEvent=13`, `TargetType=1`
  - **impact: `StartEvent=6, EndEvent=13`, duplicated as `TargetType=1` AND
    `TargetType=2`** — the only row type that ever carries TargetType=2 (on-target).
    That duplication + StartEvent=6 is the column evidence used to name the impact kit
    throughout.
- M2s: MD21-chunked, m2version=274, particle record stride 492 (validated by
  blend/emitterType/texture-index sanity per emitter). Model names, vertex extents, and
  TXID chunks all sanity-checked. Skin files parsed to map mesh batches → textures.
- Attachment IDs seen: **19 = Base** (target origin/feet), **20 = Head**,
  **21/22 = spell left/right hand**, **34 = Chest**.
- Units: model units ≈ yards; a character is ~2 units tall.
- **Every material and every particle emitter in every model below is additive blend
  (M2 blend mode 4).** No alpha-blend, no opaque geometry anywhere in the set.
- All SpellVisualEffectName scales = 1.0, all KMA attach scales = 1.0, offsets (0,0,0).

---

## SPELL: Flash Heal (Priest)

- **Spell IDs / ranks**: player ranks 2061 (r1), 9472, 9473, 9474, 10915, 10916, 10917 —
  **all share SpellVisual 3077** (ranks collapse: yes). IDs 2066 and 9475–9477 are
  non-player copies on the generic NPC heal visual 107.
- **IMPACT KIT: 2730** (evidence: StartEvent=6/EndEvent=13 rows for TargetType 1 and 2
  on visual 3077). Precast kit 99, cast kit 270 (shared Holy family, below).
- **MODEL**: fdid **166197** → `spells/flashheal_base.m2`, attach **19 = Base**.
- **TEXTURES (TXID order)**: `spells/star5a.blp`, `spells/yellow_star_dim.blp`,
  `spells/ribbonblur1bd_gold_side.blp`, `spells/grad6.blp`, `spells/lensflare1a.blp`,
  `spells/star32.blp` — all gold/yellow.
- **SHAPE**: 60 mesh verts, 3 submeshes (skin-verified batch→texture map):
  - submesh 0: 48 verts of `grad6` = **8 radial ray-fans** (6 verts each, one per bone
    6–13; apex-fan UV layout verified per-vertex — apex verts at v=0, rim at v=1).
    An animated **starburst of gradient light rays**, NOT a tube/column.
  - submesh 1: 4 verts = one `lensflare1a` quad (the central flash).
  - submesh 2: 8 verts = two `yellow_star_dim` quads.
  - 3 color animation tracks (flash fade-in/out).
  - Bbox: min(−1.51,−1.68,−0.45) max(1.64,1.36,4.08) → ~3.2 wide × ~4.5 tall envelope.
- **EMITTERS**: 5 plane particle emitters, 0 ribbons. All additive, zero gravity, zero
  cone spread (rise straight up):
  - [0],[1]: tex `star5a`, speed 3.33 u/s, life 1.0 s, rate 5/s, area 0.14×0.14,
    offset positions (−0.62, 0.02, −0.69) and (0.27, 0.02, 0.20), bones 17/18.
  - [2]: tex `yellow_star_dim`, speed 1.67, life 1.25 s, rate 13/s, area 0.56×0.56,
    at z=0.47.
  - [3],[4]: tex `ribbonblur1bd_gold_side`, speed 2.22, life 0.9 s, rate keyed
    11→15→0/s, area keyed 0.14→0.69→0.14, at z=0.63 and z=1.35.
- **MATERIAL/COLOR**: 4 materials, all Add. Pure gold palette.
- **READ**: a bright golden **lens-flare flash with radiating gradient light rays** pops
  at the target's base, and a quick, sparse column of gold star/ribbon sparks streams
  straight up for about a second. The showiest Priest heal impact — flash first, motes
  second.

## SPELL: Heal (Priest) — also the base family for Lesser Heal

- **Spell IDs / ranks**: Heal 2054, 2055, 6063, 6064 → **all SpellVisual 135** (collapse:
  yes). Lesser Heal 2050, 2052, 2053 → **all SpellVisual 285** (2051 is a non-player
  copy on visual 107). Greater Heal 2060 → visual 57 (context only).
- **IMPACT KITS**: Heal = **232**; Lesser Heal = **442**; (Greater Heal = 98). Evidence:
  StartEvent=6 + TargetType 1&2 rows. Precast 99 / cast 270 in all three.
- **MODELS**:
  - Heal: fdid **166292** → `spells/heal_low_base.m2`, attach **19 = Base**.
  - Lesser Heal: fdid **166463** → `spells/lesserheal_base.m2`, attach **19 = Base**.
  - (Greater Heal: fdid 166273 → `spells/greaterheal_low_base.m2`, attach 19; adds
    `spells/aurarune1.blp` + `spells/aurarune3.blp` — a golden rune ring at the feet —
    plus the star/ribbon/flare set; 68 verts, 6 additive materials, 7 emitters.)
- **TEXTURES**: Heal: `ribbonblur1bd_gold_side`, `star5a`, `yellow_star_dim`.
  Lesser Heal: same three (yellow_star_dim via its gnome-machine duplicate fdid 197639).
- **SHAPE**: **zero mesh vertices — pure particle models.**
  - Heal bbox: min(−0.37,−0.45,0.25) max(0.64,0.47,3.68) — a narrow vertical envelope
    ~0.5 u radius from feet to above the head.
  - Lesser Heal bbox: min(−0.88,−0.88,−3.33) max(0.88,0.88,4.38).
- **EMITTERS** (all plane, additive, zero gravity, zero cone spread = straight up):
  - Heal (4): [0] tex ribbon-gold at z=1.35, speed 2.22, life 0.9 s, rate keyed
    15→20→5/s, area keyed 0.14→0.69→0.14. [1],[2] tex star5a at offset (−0.28,0,0) on
    bones 3/4, speed 3.33, life 1.0 s, rate 10/s, area 0.56. [3] tex yellow_star_dim at
    z=0.47, speed 1.67, life 1.25 s, rate 16/s, area 0.56.
  - Lesser Heal (5): three ribbon-gold emitters at z=1.35 (speeds 2.22, lives
    0.9/1.1/1.3 s, rates keyed 20→40→0/s, area keyed 0.14→0.69→0.14), one star5a
    (speed 3.33, life 1.0, rate 9/s), one yellow_star_dim (speed 1.67, life 1.25,
    rate 8/s). Burstier than Heal.
- **MATERIAL/COLOR**: particles only, additive, pure gold.
- **READ**: no flash at all — a narrow, quiet **stream of golden stars and ribbon-blur
  sparks rising straight up** through the target's body for ~1 s. Lesser Heal is the
  same recipe slightly denser and shorter-lived; Greater Heal adds a golden rune circle
  at the feet.

## SPELL: Flash of Light (Paladin)

- **Spell IDs / ranks**: player ranks 19750 (r1), 19939, 19940, 19941, 19942, 19943 —
  **all share SpellVisual 6623** (collapse: yes).
- **IMPACT KIT: NONE.** SpellVisual 6623 has **exactly one** SpellVisualEvent row:
  `StartEvent=1, EndEvent=2, TargetType=1, kit 99` — the caster hand-glow precast loop.
  There is no StartEvent=3 (cast) row and no StartEvent=6 (impact) row. Verified by
  direct grep of SpellVisualEvent for `,6623$`.
- **READ**: in Era client data, player Flash of Light **lands visually silent on the
  target** — the only visual anywhere is the caster's golden hand glow while casting.
- **Non-player variants** (design-relevant precedent): spell 19993 ("Flash of Light",
  non-player) → visual **6622** = cast kit 270 + **impact kit 154** (Holy Light's);
  spell 25514 → visual **7379** = precast 99 + cast 270 + **impact kit 154**. So when
  Blizzard's own data gives an FoL variant a landing, it **reuses Holy Light's
  head-shower impact** — the sanctioned borrow if the ArenaSim design wants FoL to land
  visibly (presumably at reduced intensity).

## SPELL: Holy Light (Paladin)

- **Spell IDs / ranks**: 635, 639, 647, 1026, 1042, 3472, 10328, 10329 — **all share
  SpellVisual 2936** (collapse: yes).
- **IMPACT KIT: 154** (StartEvent=6, TargetType 1&2). Precast 99 / cast 270.
- **MODEL**: fdid **166349** → `spells/holylight_low_head.m2`, attach **20 = Head** —
  the ONLY heal in the set that lands at the head instead of the base.
- **TEXTURES**: `spells/genericglow64.blp`, `spells/star5a.blp`,
  `spells/starflashyellow.blp`, `spells/clouds8x8fade.blp` (8×8 animated soft-cloud
  sheet).
- **SHAPE**: mesh = 30 verts in two 15-vert `genericglow64` glow fans near the head
  (1 additive material, 2 color tracks). Bbox min(−0.82,−1.57,−3.15)
  max(1.74,1.81,1.61) — note the bbox extends 3+ units BELOW the head attach: the
  effect envelops the whole body downward.
- **EMITTERS**: **12 plane emitters, all with NEGATIVE emission speed** (particles fall
  DOWN from the head):
  - speeds −0.56 to −0.83 u/s; life 1.5 s each; rate ~10/s each (some keyed 10→5..10→1);
    slight cone spread 0.26 rad; zero gravity.
  - Origin points staggered downward: z = +1.05, +0.97, +0.74, +0.38, −0.05, −0.53,
    −1.09 (several duplicated across texture layers).
  - Emission areas widen as the origins descend: 0.0 → 0.25 → 0.5 → 0.75 → 1.0 → 1.25 →
    1.44 (a widening curtain).
  - Texture mix: star5a + starflashyellow on the upper emitters, clouds8x8fade puffs on
    six of the lower ones.
- **MATERIAL/COLOR**: additive, gold; soft cloud puffs are tinted by particle color
  (gold family per kit palette).
- **READ**: a glow blooms at the target's **head** and a wide, slow **cascade of golden
  stars and soft light-puffs rains down** over the whole body for ~1.5 s — a descending
  shower, the exact inverse of the Priest's rising motes, and the grandest impact in the
  set (12 emitters vs the Priest's 4–5).

## SPELL: Holy Shock (Paladin) — heal component

- **Spell IDs**: base 20473, 20929, 20930 have **no SpellXSpellVisual rows at all**
  (pure trigger spells). Heal triggers **25914 (r1), 25913, 25903** → **SpellVisual
  135**; damage triggers 25912, 25911, 25902 → SpellVisual 128. (Collapse: yes, per
  component.)
- **HEAL IMPACT KIT: 232 — byte-identical to Priest Heal.** Same visual 135, same kit,
  same model `spells/heal_low_base.m2` (fdid 166292) at attach 19 = Base. All numbers as
  in the Heal section above.
- **DAMAGE half (contrast, from the same lookup)**: kit **291**, model fdid **166354** →
  `spells/holysmite_low_chest.m2` (reuses Holy Smite's impact), attach **34 = Chest**.
  13 particle emitters + **2 ribbon emitters**, burst-profiled rates keyed
  0→0→(40–100)/s at the impact instant, sphere emitters with full-π spread, dust
  particles with real gravity 6.94 (sparks arc up then fall), textures
  `glowstar_yellow`, `ribbonblur1bea_gold`, `starflashyellowa/b`, `dust1_a`, `grad2db`,
  `demon_rune_ribbonb`, `star5a`. Bbox ~6 u across.
- **READ**: the heal lands as the quiet Priest-style rising gold sparkle at the feet;
  the damage lands as a **violent golden spark-burst on the chest** with arcing embers
  and ribbon trails. Same school, opposite grammar (stream vs burst) — the two halves
  are trivially distinguishable.

## SPELL: Lesser Healing Wave / Healing Wave (Shaman)

- **Spell IDs / ranks**: LHW player ranks 8004 (r1), 8008, 8010, 10466 AND all Healing
  Wave ranks (331, 332, 547, 913, 939, 959, 8005, 10395, …) → **the SAME SpellVisual
  58**. (LHW 8007/8009 are non-player copies on visual 107.) Collapse: yes — and
  further: **LHW and Healing Wave are visually identical; there is no LHW/HW
  distinction in the client data.**
- **IMPACT KIT: 101** (StartEvent=6, TargetType 1&2). Precast kit **100**
  (`nature_precast_low_hand.m2` — textures `particles/green_glow3.blp`,
  `particles/star11b.blp`, `particles/leafbrown.blp`: green glow + LEAVES on the hands),
  cast kit **183** (`nature_cast_hand.m2` — `shockwavewater1`, `yellow_glow2/3`).
- **MODEL**: fdid **166703** → `spells/restoration_impact_base.m2`, attach **19 = Base**.
- **TEXTURES (TXID order)**: `spells/yellow_glow_dim2.blp`, `yellow_star_dim.blp`
  (gnome-machine duplicate fdid 197639), `particles/green_glow3.blp`,
  `spells/butterfly.blp`, `spells/star7b.blp`, `spells/lensflare1a.blp`.
- **SHAPE**: 116 mesh verts in 6 submeshes / 6 batches (skin-verified):
  - **8 butterfly quads** (32 verts, `butterfly.blp`, bone-animated flutter), centered
    z≈0.51.
  - `yellow_glow_dim2` planes (20 verts, z≈0.70) + a second glow batch (12 verts,
    z≈0.83).
  - `green_glow3` layer (32 verts, z≈0.49).
  - one `star7b` quad (4 verts, offset (0.29,−0.63,0.39)).
  - `lensflare1a` fan (16 verts, z≈0.49).
  - Vertex cloud: mostly z 0.3–1.3 at radial extent ~1.2–1.7 u → a **torso-wrapping
    swirl**. Bbox min(−1.63,−1.62,−2.90) max(1.63,1.63,4.39). 5 color tracks.
- **EMITTERS**: only 2 (plane, additive, zero gravity, zero spread) — the inverse of the
  Priest models (mesh carries the design; particles are garnish):
  - [0] tex `yellow_star_dim` at z=2.66 (above head), speed 2.78, life 0.7 s, rate keyed
    17.85→35.25→13.25/s, area keyed 0.20→1.03→0.20.
  - [1] same texture at z=0.60, speed 2.78, life 1.0 s, rate 23/s, area 0.67×0.67.
- **MATERIAL/COLOR**: 5 materials, all Add. Green + gold palette — the only non-pure-gold
  heal in the set.
- **READ**: a soft **green-and-gold glow swirls around the target's torso while small
  butterflies flutter outward**, with a light dusting of gold stars rising. Organic and
  warm where the Priest heals are astral and sharp.

---

## CROSS-SPELL

### Shared kits and models

| Sharing | Detail |
|---|---|
| LHW = Healing Wave | Identical: visual 58, kits 100/183/101, `restoration_impact_base.m2`. No distinction exists to replicate. |
| Holy Shock heal = Priest Heal | Identical: visual 135, kit 232, `heal_low_base.m2`. |
| FoL non-player variants → Holy Light | Visuals 6622/7379 borrow impact kit 154 (`holylight_low_head.m2`). Player FoL has no impact at all. |
| Holy precast/cast | Kits 99/270 (`holy_precast_low_hand.m2`, gold hand glow + ribbons) shared by Flash Heal, Heal family, Holy Light, FoL. |
| NPC generic heal | Visual 107 → impact kit 362 → `learn_impact_base.m2` (the "skill learned" sparkle) — used by the NPC copies of several ranks. |

### Holy vs Nature, concretely

- **Holy (Priest/Paladin)**: star vocabulary — `star5a`, `yellow_star_dim`,
  `starflashyellow`, `lensflare1a`, `ribbonblur*_gold`. Pure gold. Particle-emitter
  driven (4–12 emitters, 0–60 mesh verts). Motion is strictly **vertical**: rising for
  Priest heals (+1.7 to +3.3 u/s), falling for Holy Light (−0.56 to −0.83 u/s).
- **Nature (Shaman)**: biological vocabulary — `green_glow3`, `butterfly`, `leafbrown`
  (precast), `shockwavewater1` (cast). Green + gold. **Mesh-driven** (116 verts, only
  2 emitters). Motion is **orbital/swirling around the torso**, not vertical.
- **The design hinge (LHW vs Flash Heal)**: Flash Heal = instantaneous gold
  **ray-burst flash** (8 gradient ray-fans + lens flare) + rising star motes, at the
  feet, ~1 s. Lesser Healing Wave = sustained **green/gold torso swirl with 8
  fluttering butterflies** + minimal rising stars, ~1–1.5 s. They differ in color
  (gold vs green-gold), motion (vertical burst vs orbital swirl), medium (particles vs
  animated mesh), and iconography (stars/flare vs butterflies/leaves).

### Vertical-column verdict

**NO heal in this set lands as a vertical column/beam.** The one candidate was
explicitly checked: Flash Heal's 48-vert `grad6` gradient submesh is 8 radial ray-fans
(apex-fan UV layout, one fan per bone), not a tube. Every other model is pure particles
or torso/head glow quads. The closest thing to a column is the Heal/Lesser Heal impact:
a **narrow (~0.5 u radius) stream of individually rising gold motes** — a column-shaped
envelope with no solid geometry. A card needing a solid light column cannot source it
from any Classic heal impact.

---

## Working artifacts

All in `/private/tmp/claude-501/-Users-dwalker-Projects-arenasim-prototype/9e56438a-688e-4700-ae79-5f012de98eea/scratchpad/`:
DB2 CSVs (`SpellName`, `SpellXSpellVisual`, `SpellVisual`, `SpellVisualEvent`,
`SpellVisualKit`, `SpellVisualKitEffect`, `SpellVisualEffectName`,
`SpellVisualKitModelAttach`), `listfile.csv`, M2 binaries `m2_<fdid>.bin`, skins
`skin_<fdid>.bin`, and parsers `chain.py`, `resolve.py`, `m2parse.py`, `particles.py`,
`skinparse.py`.
