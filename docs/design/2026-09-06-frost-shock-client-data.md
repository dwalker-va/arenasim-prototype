# AS-16: Frost Shock Landing — Client-Data Research

All findings are from WoW Classic Era build **1.15.9.69547** client data (wago.tools DB2
CSVs + CASC file fetches), parsed directly from the DB2 tables and M2 model binaries.
No prose/wiki sources were used as evidence.

## Method notes (what was actually done)

- Same recipe as the heal-impact research (`2026-09-06-heal-impact-client-data.md`):
  `SpellName → SpellXSpellVisual[SpellID] → SpellVisual → SpellVisualEvent →
  SpellVisualKit`, kit→model via `SpellVisualKitEffect.EffectType == 2` →
  `SpellVisualKitModelAttach` → `SpellVisualEffectName.ModelFileDataID`; CASC fetch
  `wago.tools/api/casc/<fdid>?version=1.15.9.69547` with `User-Agent: curl/8.7.1`;
  filenames from the community listfile.
- Impact slot: the `StartEvent=6, EndEvent=13` rows, duplicated as `TargetType=1` and
  `TargetType=2`. Aura state would be a `(7,8)` row.
- M2: MD21-chunked, m2version=274, particle stride 492. Skin parsed to map mesh
  batches → textures. Units: model units ≈ yards; a character is ~2 units tall.

## SPELL: Frost Shock (Shaman)

- **Spell IDs / ranks**: player ranks **8056 (r1), 8058, 10472, 10473** — **all share
  SpellVisual 144** (ranks collapse: yes). 8057/8059/10474/10475 are non-player copies
  on the generic NPC visual 107 (impact kit 362 → `learn_impact_base.m2`, the "skill
  learned" sparkle — same NPC fallback the heal research saw). The remaining
  same-name IDs (12548, 15089, 15499, 19133, 21030, 21401, 22582, 23115) are NPC
  spells also on visual 144.
- **Event rows on visual 144** (all four of them):
  - precast loop `(1,2)` T1: kit 196 — `spells/ice_precast_med_hand.m2` (fdid 166382)
    on BOTH hands (attachments 21 and 22).
  - cast one-shot `(3,13)` T1: kit 204 — same hand model, both hands.
  - **impact `(6,13)` T1 AND T2: kit 214** — KMA 367321, **attachment 34 = Chest**,
    offsets (0,0,0), effname 214 → **fdid 166370 `spells/ice_impactdd_med_chest.m2`**,
    scale 1.0.
  - **NO `(7,8)` row.** The 8s MovementSpeedSlow has no aura-state visual in the
    client data — see the slow-state section below.

## THE HEADLINE: kit 214 is Frostbolt's landing model

Frostbolt (all player ranks 116…25304 → SpellVisual 13) lands with impact kit
**4991**, and kit 4991's model attach resolves to **the same effname 214 → the same
fdid 166370 → the same attachment 34** as Frost Shock's kit 214. Frost Shock's landing
IS the mid-tier generic frost direct-damage hit; nothing about the model is Frost
Shock-specific. (The listfile also carries `ice_impactdd_uber_chest.m2` fdid 166371 —
the tier above — which neither spell uses.)

Per the card's decision rule, this settles stock-vs-bespoke: **stock**. The one
bespoke-arm precedent (Mana Burn) earned its arm by having its own model
(`manaburn_chest.m2`); Frost Shock has the opposite evidence.

## MODEL: `spells/ice_impactdd_med_chest.m2` (fdid 166370, skin 493702)

- **MESH**: 8 verts = **two 4-vert `shockwave10.blp` ring quads** (skin-verified:
  2 submeshes, 2 batches, both additive, each with its own color/fade track), at
  z = −0.24 and +0.24, radius 0.24 at spawn. The expanding-band language the stock
  Frost row's `ring` already speaks.
- **TEXTURES (TXID order)**: `spells/cyanstarflash.blp`, `spells/greyglowball64.blp`,
  `spells/ribbonblur1bc.blp`, `spells/dust1_a.blp`, `spells/snowflake3.blp`,
  `spells/snowflake2.blp`, `spells/shockwave10.blp`, `particles/lightning_side.blp`.
  Cyan/white/grey palette.
- **EMITTERS**: 28, **all additive, all one-shot burst-profiled** (every rate track
  keyed `0 → N → 0` at the impact instant — nothing sustains):
  - Two omnidirectional sphere shells (full-π spread, zero gravity): 8 emitters at
    speed 2.22 u/s + 7 at 1.67 u/s, lives 0.5–0.6 s, mixing `cyanstarflash`,
    `greyglowball64`, `ribbonblur1bc`, `dust1_a`, and the two `snowflake` textures —
    star flashes, glow motes, ribbon blurs, dust, and snowflakes thrown outward.
  - Three point-source sphere bursts (`cyanstarflash`, area 0, speed 0.56, lives
    0.25–0.75 s, rates 75–250/s) — the central flash pop.
  - **9 `lightning_side` crackle emitters**: 8 plane emitters at NEGATIVE speeds
    (−2.78 to −5.56 u/s, i.e. drawn inward/downward through the impact point), tight
    cones (0.35–1.31 rad), lives 0.22–0.25 s, rates 150–200/s, plus one sphere at
    −1.67. Short-lived electric slivers snapping around the hit — this is what makes
    the frost DD family read as a "shock", and Frostbolt's landing has it too.
  - Plus two slow `greyglowball64` drifters (speed −0.11, lives 0.3–0.4 s).
- **Zero gravity on every emitter. Zero ribbons.**
- **ENVELOPE**: bbox min(−1.64,−1.64,−1.64) max(1.64,1.64,1.64) — a ~1.6 u-radius
  sphere centred on the chest. Longest particle life 0.75 s; with the burst-profiled
  rates everything is over in well under a second.
- **READ**: a cyan-white **omnidirectional pop on the chest** — star flash, twin
  expanding shockwave rings, a shell of glow motes, dust and snowflakes flying
  outward with no gravity, laced with sub-quarter-second lightning crackles. Brief,
  symmetric, additive.

### Stock Frost row vs the source

The shipped Frost `impact_style` row (flash 0.65/0.14 s + ring 1.05/0.32 s + 10 chips)
already matches the source's grammar: central flash, expanding ring, outward debris,
sub-second life, chest anchor. Two knowing divergences, both established module-wide
precedents left as-is: the row's chips fall under gravity 6.5 (source debris is
zero-gravity — the module compresses and grounds every school's debris), and no
lightning-crackle piece (a frost-FAMILY trait shared with Frostbolt's bespoke
`BoltImpact`, not a Frost Shock signature — adding it only here would make Frost
Shock read as MORE electric than Frostbolt, inverting the source).

## SLOW STATE: none in the data

SpellVisual 144 carries no `(7,8)` aura-state row, so Frost Shock's slow is visually
silent on the receiver in the Classic client. For contrast, Frostbolt's visual 13 DOES
carry one — `(7,8)` T1+T2, kit 3531 → fdid 166385 `spells/ice_precast_uber_head.m2`
(a 2-emitter snowflake drip, attachment 20 = Head) — which is the chill state a
MovementSpeedSlow receiver treatment would source. It is Frostbolt's, not Frost
Shock's, and building it would implicate every MovementSpeedSlow applier (Concussive
Shot has no state kit either). **Per the card: out of scope, nothing built.**

## What shipped

- `AbilityType::FrostShock` added to `SchoolImpact::anchor_for` →
  `ImpactAnchor::Chest` (attachment 34, per kit 214). It resolves through
  `process_casting`'s direct-effect landing (0-cast `CastingState` ability), the
  existing `anchor_for` spawn site — headless byte-identical, graphical-only render.
- No `landing_style` override (the measured kit is the shared generic frost hit).
- No slow receiver treatment (no aura-state kit in the data).

## Working artifacts

All in the session scratchpad
(`/private/tmp/claude-501/-Users-dwalker-Projects-arenasim-prototype/9e56438a-688e-4700-ae79-5f012de98eea/scratchpad/`):
`as16_chain.py`, `as16_fetch.py`, `m2_166370.bin`, `skin_493702.bin`, `m2_166385.bin`,
the shared DB2 CSVs, `listfile.csv`, and parsers `m2parse.py`, `particles.py`,
`skinparse.py`.
