# AS-17: Healing Stream Totem — Client-Data Research

All findings are from WoW Classic Era build **1.15.9.69547** client data (wago.tools DB2
CSVs + CASC file fetches), parsed directly from the DB2 tables and M2 model binaries.
No prose/wiki sources were used as evidence. Method and event-slot evidence rules follow
`2026-09-06-heal-impact-client-data.md` (AS-9): chain
`SpellXSpellVisual[SpellID] → SpellVisual → SpellVisualEvent → SpellVisualKit →
SpellVisualKitEffect (EffectType==2 → SpellVisualKitModelAttach) →
SpellVisualEffectName.ModelFileDataID`; `User-Agent: curl/8.7.1`.

Event-slot key (AS-9/AS-15 established): `(1,2)` = precast loop; `(3,13)` = cast-launch
one-shot; `(6,13)` duplicated TargetType 1+2 = **impact**; `(7,8)` = **aura state**
(plays while the aura persists).

## The spell family

| Spell IDs | Role | SpellVisual |
|---|---|---|
| 5394, 6375, 6377, 10462, 10463 | Healing Stream Totem (the player's summon, all ranks) | **319** |
| 5672, 6371, 6372, 10460, 10461 | Healing Stream (the heal the totem provides, all ranks) | **366** |
| 5396, 6383, 6384, 10464, 10465 | non-player copies | 107 (the generic NPC heal visual) |

Ranks collapse completely: one visual for every summon rank, one for every heal rank.

## SUMMON: visual 319 — nothing to draw

SpellVisualEvent carries exactly one row for 319: `(3,13)` TargetType 1 → kit **352**,
whose SpellVisualKitEffect rows are EffectType 6 (sound 359984) and EffectType 5
(procedural 2552) only — **no EffectType 2, no model attach, no model**. The totem
placement has sound but no spell visual; the totem's look is the summoned totem object
itself. Nothing is ever attached to or drawn on the totem by the spell system.

## HEAL: visual 366 — no impact, no stream; a target-side aura-state borrow

SpellVisual row 366: `HasMissile = 0`, no missile fields set — **no projectile, no
beam, no arc from totem to target exists anywhere in the data**. The name "Healing
Stream" describes nothing the client draws.

SpellVisualEvent rows for 366 (complete):

| Events | TargetType | Kit | Resolves to |
|---|---|---|---|
| (1,2) precast | 1 | 99 | `spells/holy_precast_low_hand.m2` (fdid 166336), attaches 21+22 (spell hands) |
| (3,13) launch | 1 | 270 | same model, same hand attaches |
| (7,8) **aura state** | 1 AND 2 | **523** | `spells/lesserheal_base.m2` (fdid **166463**), attach **19 = Base** |

There is **no `(6,13)` row — Healing Stream has NO impact kit**. Individual heal
pulses land with no per-tick visual event at all. The heal's entire target-side
identity is kit 523: an **aura-state** effect that plays continuously at the bearer's
feet while the buff holds. Kits 99/270 are the standard Holy caster-side hand kits
(AS-11 vocabulary) — vestigial here, since the caster is a handless totem.

### The aura-state model: `lesserheal_base.m2`

fdid 166463 — **verbatim the model that is Lesser Heal's (Priest) impact** in AS-9's
table. `LesserHeal_Base`, m2version 274, 0 mesh verts, 0 ribbons, **5 plane particle
emitters**, every one additive (blend Add), zero gravity, rising straight up:

- [0],[3],[4]: tex `spells/ribbonblur1bd_gold_side.blp`, origin z=1.35, speed 2.22 u/s,
  lives 1.1/0.9/1.3 s, rate keyed 20→40→0 /s, area keyed 0.14→0.69→0.14.
- [1]: tex `spells/star5a.blp`, origin (−0.28, 0, 0), speed 3.33 u/s, life 1.0 s,
  rate 9/s, area 0.56×0.56.
- [2]: tex `world/.../yellow_star_dim.blp`, origin z=0.47, speed 1.67 u/s, life 1.25 s,
  rate 8/s, area 0.56×0.56.

Palette: pure gold. Textures, speeds, lives and areas are the Priest Holy family's.

## Answer to the card's key question

The effect lives on the **TARGET**, and only the target. Not on the totem (nothing is
ever drawn there), and there is no stream/arc (no missile, no ribbon, no beam). What
the target shows is not per-tick either: it is a **persistent gold sparkle at the feet
for as long as the buff holds — borrowed verbatim from Priest Lesser Heal's impact
model**.

## Design constraint (negative/partial finding)

The client identity is unusable as-is here, twice over, so the in-game visual is
**authored on the AS-10 Nature vocabulary rather than transcribed** — stated per the
card's rule that a negative finding is a design constraint:

1. **Wrong color, wrong class voice.** The borrowed `lesserheal_base` gold is the
   Priest Holy palette; rendering it under a Shaman totem would alias totem sustain
   with Priest heal landings. ArenaSim's Shaman heal vocabulary is Nature green
   (AS-10 impact side: `restoration_impact_base` green/gold; AS-11 cast side:
   `nature_precast_low_hand` green).
2. **Wrong lifecycle.** A persistent aura-state loop on every ally, refreshed every
   pulse for the totem's whole 45 s life, is constant screen noise in a 3v3 — and
   ArenaSim's totem heals as discrete 1 s ticks (`tick_interval` 1.0), so the honest
   moment to mark is the tick that actually heals.

What IS transcribed: the client's **shape**. A small one-shot of rising sparkle motes
from the **Base attach** (feet), narrow envelope (the source's 0.56 area), source
emitter speeds (3.33 / 2.22 u/s) and lives — recolored Nature green, cut down to a
per-tick blip far below Healing Wave's butterfly-swirl scale.
