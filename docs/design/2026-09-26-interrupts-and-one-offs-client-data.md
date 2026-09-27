# AS-137: Interrupts and the generic-tier one-offs — Client-Data Research

All findings are from WoW Classic Era build **1.15.9.69547** client data (wago.tools DB2
CSVs + CASC M2 fetches). No prose/wiki sources were used as evidence. Method as in
`2026-09-26-hunter-shot-client-data.md`: SpellName → SpellXSpellVisual → SpellVisual
(missile set) → SpellVisualEvent → kits → model attach / anim / procedural. M2s parsed at
the v274 layout; texture names come from `TXID` plus the community listfile.

Scope: Spell Lock, Wind Shear, Heroic Strike, Mind Blast, Holy Shock (damage leg) and Web.
Kick and Pummel are added because they share the interrupt question.

## The join

| Ours | Client spell | Caster side | Victim side |
|---|---|---|---|
| Kick | 1766 → visual 90 | anim 95 (Kick) | kit 133: `kick_chest_impact.m2` @Chest + victim anim 10 (CombatCritical) |
| Pummel | 6552 → visual 1023 | anim 118 (SpecialUnarmed) | **kit 133, the same as Kick** |
| Spell Lock | 19244 / 19647 → visual 5282 | shadow precast/cast hands (kits 114/118) | kit 381: `counterspell_impact_chest.m2` @Chest, **shared with Counterspell** (2139 → visual 239) |
| Wind Shear | **not in the Classic client** (it arrived in Wrath) | — | — |
| (Earth Shock, Classic's Shaman interrupt) | 8042 → visual 3444 | nature precast/cast hands | kit 3055: `earthshock_impact_chest.m2` |
| Heroic Strike | 78 → visual **39** | kit 324: anim 57 (special 1H swing) + procedural 356792 (**weapon trail**, see below) | kit 437: `decisivestrike_impact_chest.m2` @Chest + victim anim 9 (CombatWound) |
| Mortal Strike | 12294 → visual **39** (**identical to Heroic Strike**) | kit 324 | kit 437 |
| Mind Blast | 8092 → visual 3057 | shadow precast/cast hands (kits 114/218) | kit 2709: `mindblast_head.m2` @**Head** (20) |
| Holy Shock (damage) | 25912 → visual 128 | holy precast/cast hands | kit 291: `holysmite_low_chest.m2` @Chest (Holy Smite's impact) |
| Web | 4167 → visual 684 | anim 107 (AttackThrown) | missile `web_missile.m2`; STATE kit 746 `web_state.m2` (the root, already drawn); no impact |

**The "Silenced" debuffs have no visual:** Kick - Silenced 18425, Counterspell - Silenced
18469, Shield Bash - Silenced 18498. In the client, an interrupt is legible through its
LANDING on the victim, not through any lingering state.

**Procedural Type 8 is a weapon trail.** 81 Type-8 rows are used by Charge (1000 ms),
Cleave, Bloodthirst, Mortal Strike, Eviscerate, Garrote, Feint, Disengage, Bladestorm
(10000 ms) and others. Heroic Strike's and Mortal Strike's row 356792 is colour `0xF82A29`
= RGB (248, 42, 41), with Value_1..3 = 20 / **600** / 100. The 600 matches the swing's
duration in ms; the other two values are unidentified.

## Models

### `kick_chest_impact.m2` (Kick, Pummel) — 1233 ms
One emitter only. `shockwave8.blp`, additive, 10/s for 0–333 ms (about 3 rings), life
0.3 s, near-white (253,244,225) → white, alpha 0.7 → 0.53 → 0, size **0.28 → 0.56 → 0.97**.

### `counterspell_impact_chest.m2` (Spell Lock) — 1666 ms
- A 28-vertex rune mesh (~2.2 × 2.2 yd, additive, model colour (0, 0.92, 0.11)). Its
  animation is not parsed.
- e0 `aurarune256b`: 5.1/s for 0–1000 ms, life 1.24 s, (30,136,235) → (144,52,230) →
  (170,30,236), alpha 0 → 0.55 → 0.21, size **0.75 → 0.46 → 0.17 (shrinking)**.
- e1 `aurarune256`: 6/s for 0–1000 ms, life 1.2 s, (255,221,30) → (52,227,192) @0.45 →
  (170,30,236), alpha 0 → 0.65 → 0.36, size **0.91 → 0.61 → 0.42 (shrinking)**.
- e2 `star8c`: a sphere 0.42 × 0.83, 30/s for 0–1000 ms, life 1 s, speed 0.3, violet →
  white → green, size 0.03 → 0.14 → 0.

Read: rune discs that CLOSE IN on the chest, a seal.

### `earthshock_impact_chest.m2` (reference only) — 3734 ms
A 336-vertex mesh ~4 yd across, lightning zaps, and falling brown and green leaves
(gravity 6.9). An earth-and-leaves burst. Not used: see the decisions.

### `decisivestrike_impact_chest.m2` (Heroic Strike = Mortal Strike) — 667 ms
Everything happens in the first 67 ms:
- `whiteringthin128`: a thin ring 0.03 → 0.97 → **1.67**, red (208,0,0) → violet
  (129,35,189) → (63,0,134), alpha 0 → 1 → 0, life 0.5 s;
- `flare` sparks, violet → red → violet, speed 2.8;
- a violet `fireplume64`, speed 5.6.

The particle COUNTS are ambiguous. The rate tracks ramp across a 67 ms window, which
literally yields one or two particles. The bench uses a small burst and says so.

### `mindblast_head.m2` — 2000 ms, Head
- e0 `flamelick_purple`: rate 0 → 75/s at 200 ms, held to 1400 ms, 0 by 2000 ms; life
  1 s; gravity **−1.25 (rises)**; (79,55,255) → (126,55,255) → (210,0,255); size
  0.11 → 0.28 → 0.03.
- e1 `lavalump2` embers: 2.5/s for 200–1800 ms, life 2.25 s, rising.
- e2 `toonsmoke16` (**alpha-blended**): 5/s for 200–1600 ms, life 2.25 s, rising, violet →
  (58,35,84) → grey (180,162,192), growing 0.17 → 0.69.

### `holysmite_low_chest.m2` (Holy Shock damage) — 1133 ms
Thirteen emitters and two ribbons, nearly all starting at 133–200 ms and done by ~550 ms:
- e3 a big yellow-white flash growing to **2.78**;
- e4 stars converging (speed −0.42, growing to 1.39);
- e5 / e7 white sparks arcing under **gravity 6.94** (rates peaking at 100/s and 75/s);
- e1 / e2 gold ribbon-blur particles (peaking at 150/s);
- e6 / e8 / e9 olive-to-tan dust (75/s at 333 ms);
- e10 fine white-to-orange streaks;
- e0 a soft glow;
- e11 / e12 converging motes (speed −0.56, 0–333 ms);
- two white model ribbons, 0.17 yd tall at 200 ms, gone by 467 ms.

### `web_missile.m2` (Web)
26 vertices, a flat disc 1.2 yd across and 0.15 thick, one additive material over
`spiderwebs01.blp`. Sequences 144 (700 ms) and 0 (167 ms). A spinning web disc in flight,
not a cuboid.

## Decisions (2026-09-26, the user)

1. **All four interrupts get a victim landing**, so an interrupt stops looking like a
   fizzle (today both are only the casting-orb sputter):
   - Kick and Pummel: the client's white shockwave ring;
   - Spell Lock: Counterspell's shrinking runes;
   - Wind Shear: see 2.
2. **Wind Shear lands as the shockwave ring tinted a pale wind-blue.** The client has no
   Wind Shear. Earth Shock's earth-and-leaves burst is the wrong element, and runes would
   make it Spell Lock's sibling.
3. **Heroic Strike gets the red trail and the ring landing on the ORDINARY swing arc.**
   The client draws it identically to Mortal Strike, but ours keeps Mortal Strike as the
   signature: rising-diagonal stroke plus the heal-refused tell. The two share the trail
   colour and the landing, and differ by stroke.
4. **Mind Blast, the Holy Shock damage leg and Web are built client-faithful**, trimmed
   at the bench (see `client-values-are-a-ceiling-not-a-target`).

## Bench sign-off (2026-09-26)

In the Interrupt Landing Bench, the user took the defaults as **the starting point for
every effect**. Build to these values, then refine them in the Animation Sandbox rather
than re-researching:

- **Every effect at the client's values:** size, spread and duration ×1.0.
- **Every layer is kept, with one exception:** Spell Lock's rune PLATE (the 28-vertex
  mesh) is left out, because its animation is not parsed. Spell Lock is the two
  shrinking rune emitters plus the sparkle column.
- **Holy Shock damage keeps all seven layer groups** (flash, converge, sparks, ribbons,
  dust, streaks, glow). It is the most layered effect in the batch. If the sandbox shows
  it too busy, trim it there (see `client-values-are-a-ceiling-not-a-target`).
- **Wind Shear ring tint** is `#bee1ff`, i.e. `Color::srgb(0.75, 0.88, 1.00)`.
- **Heroic Strike trail:** (248, 42, 41), 0.6 s life, 0.12 yd wide, on the ordinary
  swing arc. The landing is `decisivestrike_impact_chest`'s ring plus a small spark and
  plume burst (about 6 and 3 particles; the counts are inferred, see above).
- **Web disc:** 1.2 yd across, spinning at 1.5 rev/s. The bench flew it at 30 yd/s.
  In the game, the sim's own projectile speed governs.
- **Rings face the camera.** Whether the client lays them flat rests on a particle flag
  documented only by the community.
