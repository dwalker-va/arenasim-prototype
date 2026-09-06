# AS-14 round 2: Warrior Charge — Client-Data Research

All findings are from WoW Classic Era build **1.15.9.69547** client data (wago.tools DB2
CSVs + CASC file fetches), parsed directly from the DB2 tables and M2 model binaries.
No prose/wiki sources were used as evidence.

## Method notes

Same recipe as the heal-impact and Frost Shock research
(`2026-09-06-heal-impact-client-data.md`, `2026-09-06-frost-shock-client-data.md`):
`SpellName → SpellXSpellVisual[SpellID] → SpellVisual → SpellVisualEvent →
SpellVisualKit`, kit→model via `SpellVisualKitEffect.EffectType == 2` →
`SpellVisualKitModelAttach` → `SpellVisualEffectName.ModelFileDataID`; CASC fetch
`wago.tools/api/casc/<fdid>?version=1.15.9.69547` with a curl User-Agent; filenames
from the community listfile. M2 ribbon records parsed at the v274 layout (176-byte
stride at offset 288), particles at stride 492.

## SPELL: Charge (Warrior)

- **Spell IDs / ranks**: player ranks **100 (r1), 6178 (r2), 11578 (r3)** — all three
  resolve to **SpellVisual 867** (ranks collapse: yes).
- **SpellVisual 867**: `HasMissile = 0`, no positioners, no area model — a dash is
  NOT a missile in the client's terms; there are no `(9,10)`/`(4,5)` area rows.
- **One event row**: `(StartEvent=3, EndEvent=13)`, TargetType 1 — a single kit
  played on the CASTER for the duration of the cast/dash: **kit 44**.
- **Kit 44 effects**: two `EffectType == 2` model attaches (plus a sound and one
  unresolvable row):
  - KMA 367044 → effname **634** → fdid **165784 `spells/chargetrail.m2`**,
    **attachment 34 (Chest)**, scale 1.0.
  - KMA 367043 → effname **1303** → fdid **165976 `spells/dustcloud_land.m2`**,
    **attachment 19 (Base)** — the ground point at the feet, scale 1.0.

## MODEL: `spells/chargetrail.m2` (fdid 165784)

- **ONE RIBBON** (bone 1, i.e. riding the attachment as the model moves):
  - color **(0.81, 0.0, 0.0) — red**; alpha fixed16 26214/65535 ≈ **0.40**
  - `heightAbove = heightBelow = 0.472` → a **~0.94-unit-tall band centred on the
    chest** (a character is ~2 units tall)
  - `edgesPerSec = 50`, **edgeLifetime = 1.0 s**, gravity 0
  - texture `spells/grad3a.blp` — a soft gradient
- **One particle emitter**: Add-blended `flare.blp` sprites, **zero speed, zero
  gravity**, life 1.0 s, rate 50/s, plane area 0.56 × 0.97 — glowing flecks dropped
  in place along the path as the chest moves through space.
- Sequences: a 334 ms loop (animID 0) — the effect loops for as long as the kit is
  active, i.e. the dash.

**Read**: a translucent **red streamer painted along the actual dash path**, each
trailing edge fading over one second, sprinkled with stationary additive flecks.
A ribbon is a camera-facing strip extruded behind a moving attachment — the exact
opposite of a solid volume parked at the dash origin.

## MODEL: `spells/dustcloud_land.m2` (fdid 165976)

- **Zero ribbons, two Alpha-blended sphere emitters** (texture
  `spells/smoke_loose_02_256_blend.blp` — loose smoke):
  - speed 4.44 u/s, life 0.9 s, rate profile 50 → 50 → 0 (front-loaded)
  - speed 1.11 u/s, life 1.0 s, rate 5/s
- Attached at the Base: **dust kicked up at the feet** for the dash's duration,
  heaviest at launch.

## Negative findings

- No missile, no positioner, no area-shape rows: nothing about Classic Charge is a
  projectile or a zone.
- Nothing attaches at the dash ORIGIN. Both models ride the moving Warrior; every
  visible element is a trail artifact left behind the live position.

## What the repo builds from this

`rendering/effects/movement_trails.rs`: distance-paced emission along the mover's
live transform while `ChargingState` is present — thin vertical red streak segments
at chest height (ribbon analog: red 0.81/0/0, ~0.94-unit band, sub-second fade)
plus ground-level dusty puffs that expand and fade (dustcloud analog, ~0.9 s,
launch burst included). Shared verbatim by the Boar's charge at a body-size scale.
The client's 1.0 s edge life is trimmed to 0.7 s because the repo renders the
streamer additively (repo Z-fighting idiom) rather than at true alpha 0.40.
