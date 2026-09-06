# AS-20: Hunter Disengage — Client-Data Research

All findings are from WoW Classic Era build **1.15.9.69547** client data
(wago.tools DB2 CSVs + CASC file fetches), parsed from the DB2 tables and M2
model binaries. No prose/wiki sources were used as evidence.

## Method notes

Same recipe as the Charge research (`2026-09-06-charge-client-data.md`):
`SpellName → SpellXSpellVisual[SpellID] → SpellVisual → SpellVisualEvent →
SpellVisualKit`, kit→model via `SpellVisualKitEffect.EffectType == 2` →
`SpellVisualKitModelAttach` → `SpellVisualEffectName.ModelFileDataID`; CASC
fetch `wago.tools/api/casc/<fdid>?version=1.15.9.69547` with a curl
User-Agent. M2 particles parsed at the v274 layout (stride 492).

## SPELL: Disengage (Hunter)

- **Spell IDs / ranks**: player ranks **781 (r1), 14272 (r2), 14273 (r3)** —
  all three resolve to **SpellVisual 738** (ranks collapse: yes). (IDs
  6791/14344/14345 also named "Disengage" resolve to visual 107 — an NPC/proc
  variant with the same shape: one caster kit, nothing spatial.)
- **SpellVisual 738**: `HasMissile = 0`, no positioners, no area model — in
  this client Disengage is NOT a leap, a missile, or a zone. It is the
  VANILLA melee threat-drop.
- **Event rows**:
  - `(StartEvent=3, EndEvent=13)`, TargetType 1 (caster) → **kit 823**
  - `(StartEvent=6, EndEvent=13)`, TargetType 1 (caster) → **kit 3394**
  - `(StartEvent=6, EndEvent=13)`, TargetType 2 (target) → **kit 3394**
- **Kit 823** (cast): `EffectType 6` → SpellVisualAnim 360123 — loop anim
  **57 (Special1H)**, a one-handed melee special-attack gesture; a sound
  (48); one `EffectType 1` SpellProceduralEffect (356844, Type 8,
  Value_0 = 4235246 — not a fetchable fdid, unresolvable, same class of row
  the Charge research left unresolved).
- **Kit 3394** (both caster and target): a sound (3226) and one
  `EffectType 2` model attach — KMA 368207 → effname 1985 → fdid **165715
  `spells/blink_impact_chest.m2`**, **attachment 34 (Chest)**, scale 1.0.

## MODEL: `spells/blink_impact_chest.m2` (fdid 165715)

- **Zero ribbons. Nine particle emitters, ALL Add-blended** — a one-shot
  white-blue chest flash (single 1600 ms sequence, flags 0x8a1):
  - a fast flare burst: speed 3.98, vrange π, life **0.37 s**, rate 60/s
    (`item/objectcomponents/weapon/flare.blp`)
  - a star burst: speed 1.64, life 0.5 s, rate 40/s (`spells/star5a.blp`)
  - a sphere sparkle shell: speed 0.94, life 0.75 s, rate 24.3/s
    (`world/skillactivated/containers/sparkle.blp`)
  - four near-static glow points: speed 0.06, life 0.4 s, rate 11/s each
    (`spells/genericglow2c.blp`)
  - an INWARD-collapsing pixie shell: speed **-0.56**, sphere area 2.33,
    life **1.135 s**, rate 102.6/s (`world/generic/ogre/passive
    doodads/torches/pixies1.blp`)
  - a dust wisp emitter (`world/generic/passivedoodads/duelingflag/dust2b.blp`)
- Particle lives span **0.35–1.14 s**; every emitter is additive; the whole
  model is a chest-attached sparkle FLASH, not a trail.

## Negative findings

- **The Classic Era client has NO leap visual for Disengage.** No missile,
  no positioner, no area rows, no ribbon, no wind or feather model — the
  backward leap is a Wrath-era mechanic this sim adopted; nothing in this
  build's data depicts it.
- Nothing attaches at any origin point; the only model rides the chest of
  the caster (and target) as a one-shot flash.

## What the repo builds from this

A negative finding is a design constraint: the trail is AUTHORED, grounded in
the Charge trail's client-proven path-laid construction
(`rendering/effects/movement_trails.rs` — distance-paced elements along the
live transform, per-element fade), and says so. The palette and particle
vocabulary come from Disengage's OWN kit model: thin wind slivers
(speed-lines, deliberately unlike Charge's tall red ribbon band) plus tiny
additive white-blue spark motes (the flare/star/sparkle/pixie analog, mote
lives at the short end of the client's 0.35–1.14 s), and a one-shot spark
burst at the jump point (the `blink_impact_chest` flash analog). No ground
elements — the leap is an air move; nothing about the vanilla visual (or the
leap) touches the floor.

