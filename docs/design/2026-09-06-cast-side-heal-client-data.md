# AS-11: Classic Era Heal Cast-Side Visuals — Client-Data Research

What the CASTER shows while casting and launching a heal: the precast hand-glow
kits, the cast-launch kits, and the caster body animation. All findings are from
WoW Classic Era build **1.15.9.69547** client data (wago.tools DB2 CSVs + CASC
fetches), parsed directly from DB2 tables and M2 binaries. No prose/wiki
sources. Companion to `2026-09-06-heal-impact-client-data.md` (the impact side,
AS-9), same parsers.

## Method notes

- Kit → model join as AS-9 documented: `SpellVisualKitEffect.EffectType == 2`,
  Effect = SpellVisualKitModelAttach ID → `SpellVisualEffectNameID` →
  ModelFileDataID. New parsing written for this pass: M2 **ribbon emitters**
  (176-byte records: texture/material indices, color and alpha tracks, edge
  rate/lifetime, half-widths) — the cast-side models turn out to be
  ribbon-driven, which the AS-9 particle parsers could not see.
- **Recipe correction to AS-9's EffectType-6 note**: ET6's Effect column is a
  **SpellVisualAnim ID**, not a sound — all four cast-side kits' ET6 IDs
  resolve in `SpellVisualAnim.csv` (359853/359854/359904/359952). This is the
  caster body-animation join. ET5 remains non-visual (sound-kit reference).
- Attach IDs: **21/22 = spell left/right hand**. Every kit here carries exactly
  two KMA rows (attach 21 AND 22), offsets (0,0,0), scale 1.0, no positioner —
  pure symmetric both-hands attachment in every case.
- **Every material and every particle/ribbon emitter in every model below is
  additive (M2 blend 4).** Units: model units ≈ yards; a character is ~2 units
  tall.

## The kit → model resolution

| Kit | Role | Model fdid | Path | Attach | Anim (ET6) |
|---|---|---|---|---|---|
| **99** | Holy precast loop | 166336 | `spells/holy_precast_low_hand.m2` | 21+22 | loop **52 ReadySpellOmni** |
| **270** | Holy cast-launch | 166336 | **SAME model as kit 99** | 21+22 | **54 SpellCastOmni** |
| **100** | Nature precast loop | 166605 | `spells/nature_precast_low_hand.m2` | 21+22 | loop 52 |
| **183** | Nature cast-launch | 166602 | `spells/nature_cast_hand.m2` | 21+22 | 54 |

**Kit 270 vs 99**: both resolve to SpellVisualEffectName 135 → the same M2. The
Holy "launch flash" is literally the precast hand-glow model replayed as a
one-shot; only the body animation (52 vs 54) and the event window differ. Holy
has no dedicated launch vocabulary. Nature DOES: kit 183 is a distinct
particles-only burst model.

## `holy_precast_low_hand.m2` (kits 99/270, per hand)

- **Mesh**: two camera-facing quads on spherical-billboard bones — a
  **0.88 × 0.88 u** outer soft gold glow (`yellow_glow3a`) and a **0.54 u**
  brighter core (`genericglow2c`). ~0.9 u glow ball around the hand.
- **Ribbons: 3** (the model's whole motion — ZERO particle emitters). All
  `genericglow2b`, additive, 50 edges/s, **half-widths 0.167 → 0.33 u wide**,
  alpha 0.80, gold colors (0.988,0.863,0.165) / (0.992,0.804,0.290) /
  (0.984,0.855,0.145), edge lifetimes **0.40 / 0.60 / 0.50 s**.
- **Rig**: the three ribbon heads hang off three independent animated bone
  chains, pivots ~0.2 u from the attach in three different directions, orbiting
  the hand over the model's single **1000 ms loop**, dragging 0.4–0.6 s trails.
- **Read**: a gold glow ball per hand with three thin gold ribbon wisps
  swirling on a 1 s cycle. As kit 270 the same model fires once at launch — a
  brief re-flare of the identical glow-and-wisps, killed by the kit-end edge
  rather than a fade track.

## `nature_precast_low_hand.m2` (kit 100, per hand)

- **Mesh**: two concentric green quads (`green_glow3`), **0.76 and 0.36 u**, on
  the same billboard bones. Slightly smaller than Holy's.
- **Ribbons: 3** — the same rig template (identical head offsets and chain
  structure), same 50 edges/s and 0.40/0.60/0.50 s lifetimes, but texture
  `star11b` (a star sheet), **bright green** (0.235,1.0,0.0) family, alpha
  0.70, and much thinner: **0.08–0.11 u wide** — star-threads, not streamers.
- **Particles: 1** — the leaves: plane emitter near the hand, `leafbrown`,
  additive, **speed 0.086 u/s** (near-motionless drift), **spread π** (full
  hemisphere), **life 0.8 s, rate 11/s**, point source, gravity 0.
- **Read**: the same skeleton as Holy re-dressed — smaller, greener, thinner
  wisps, plus a slow ambient shed of leaves off the glowing hand.

## `nature_cast_hand.m2` (kit 183, per hand)

- **Mesh: none** — a pure particle model, the inverse of the precast models.
- **Particles: 3**, all at the hand origin, additive, gravity 0, point sources,
  **spread 0.035 rad (~2°, a tight directional jet)**, **life 0.30 s**:
  - `shockwavewater1`, speed 0.261 u/s, rate 21.4/s — expanding water-rings
  - `yellow_glow2` + `yellow_glow3`, speed 0.278 u/s, rate 15/s each — gold
    glow sparks
- The only one-shot-flagged sequence in the set; visible content is over in
  ~0.3 s once emission stops.

## Caster body animation

`SpellVisualEvent → kit → EffectType 6 → SpellVisualAnim`; there is no
animation column on SpellVisual itself, so this is the complete story:

| Spell | Precast anim | Cast anim |
|---|---|---|
| Flash Heal / Heal / Lesser Heal (Priest) | 52 ReadySpellOmni (loop) | 54 SpellCastOmni |
| Holy Light (Paladin) | 52 | 54 |
| Flash of Light (Paladin) | 52 | **none — no cast kit exists** |
| LHW / Healing Wave (Shaman) | 52 | 54 |

- Every SpellVisualAnim row has InitialAnimID = −1: no intro, the loop starts
  at the cast-begin edge.
- **All heals use the OMNI pair** (both hands raised, then the both-hands
  upward/outward release) — never the directed one-hand point of offensive
  bolts. The 52→54 handoff IS the cast body language of a Classic heal.
- **The target never animates**: no impact kit carries an ET6 row.

## Timing structure (SpellVisualEvent)

Every ms-offset column is 0 on every row — timing is purely event-edge-driven:

| Row | Start → End | Meaning |
|---|---|---|
| Precast loop | 1 → 2 | alive from cast-start to cast-end; **duration = the cast time, decided by gameplay**, the 1000 ms model loops under it |
| Cast-launch one-shot | 3 → 13 | fires exactly AT the completion edge, runs to its own kit-end |
| Impact one-shot | 6 → 13 | the AS-9 territory; data-simultaneous with launch for heals (`HasMissile = 0`) |

- The precast loop ends at event 2 whether the cast completes or is
  interrupted — the same edge covers both. **An interrupted Classic heal's
  hand-glow just vanishes; no interrupt-specific kit exists on these visuals.**
- **Flash of Light**: visual 6623's only event row is the kit-99 precast loop.
  No cast kit, no launch flash, and (per AS-9) no impact — in data, an FoL
  cast is a gold hand glow + ReadySpellOmni loop that simply stops.

## What a watching player sees

1. **Cast begins**: the caster snaps into ReadySpellOmni (no intro) and both
   hands light up — glow ball + three orbiting wisps (gold streamers for Holy;
   green star-threads plus drifting leaves for Shaman) — looping for the
   entire cast bar.
2. **Cast completes**: the loop kit dies, the caster plays SpellCastOmni, and
   the launch fires on both hands — Holy a one-shot re-flare of the same
   glow-and-wisps; Nature a ~0.3 s jet of water-rings and gold sparks. The
   impact blooms on the target the same instant.
3. **If interrupted**: the hand glow and body loop stop dead. There is no
   interrupt visual.

## In-repo implementation notes (AS-13)

`rendering/effects/heal_cast.rs` implements the above at the blessed workshop
defaults, replacing the generic casting orb for hard-cast heals only. One
deliberate divergence: `FLASH_OF_LIGHT_HAS_LAUNCH_FLASH = true` — the blessed
call gives FoL the standard Holy re-flare instead of the source's silence.
Probes: `tests/heal_cast_visual_probes.rs`.
