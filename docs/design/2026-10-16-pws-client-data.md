# Power Word: Shield — client data and the gap (AS-344)

Research for AS-344: what the Classic and TBC clients draw for Power Word: Shield, and
how that differs from `ShieldBubble` (`rendering/effects/shield_bubbles.rs`). Phase 1 of
the card. The bench (phase 2) is
[`benches/2026-10-16-pws-bench.html`](benches/2026-10-16-pws-bench.html) (published at
https://claude.ai/artifact/1PPVBh7jQFVRj8SdcbQEfX). Nothing in Rust
changes until the user signs off on it.

Sources: wago.tools DB2 tables and CASC files for Classic Era `1.15.9.69547` and TBC
`2.5.6.69795`. The join is `scripts/db2_spell_sweep.py` over skill line **613 Discipline**,
followed by a per-kit walk and an M2 parse (the same chain and M2 strides as
`2026-10-03-druid-client-data.md`).

## 1. The join

`db2_spell_sweep.py --skill-line 613` holds on both builds:

| build | inventory | no visual | with visual | property 4 |
|---|---|---|---|---|
| Era 1.15.9 | 28 | 12 | 16 | HELD |
| TBC 2.5.6 | 39 | 17 | 22 | HELD |

Every rank of Power Word: Shield resolves to **SpellVisual 784**: ranks 1–10 on Era and
1–12 on TBC (spell 17 to 25218). Rank collapse is intact. Visual 784 has **no missile and
no impact kit**. Its events are these:

| event | kit | what it is | Era | TBC |
|---|---|---|---|---|
| (1,2) precast, caster | 99 | `ReadySpellOmni` loop + `holy_precast_*_hand.m2` on both spell hands (attach 21/22) | `low_hand` (166336) | `med_hand` (166337) |
| (3,13) cast, caster | 270 | `SpellCastOmni` one-shot + `holy_precast_low_hand.m2` (166336) on both hands | same | same |
| (7,8) aura state | 847 | `holydivineshield_state_base.m2` (166342) at **attach 19, Base (the feet)**, scale 1, plus sound 39 | target types 1 and 2 | target type 2 only |

Notes on the table:

- **The precast row never plays for PW:S.** The spell is an instant, so it has no cast
  bar to loop through. Era and TBC differ only on that row, so the shield the player sees
  is the same in both.
- **Era also fires the state kit on target type 1.** TBC dropped it. That reads as a
  self-cast duplicate and does not change what the shielded unit shows. I take the TBC
  reading: the bubble is on the shielded unit only.
- **Kit 270 is the Holy launch** that `heal_cast.rs` already draws for Flash Heal and Holy
  Light (`HealCastKind::Holy`, `HOLY_LAUNCH_FLARE_SECS` 0.45). There is no PW:S-specific
  cast art.
- **The name is a legacy name.** Divine Shield, Divine Protection and Blessing of
  Protection do **not** use this model. Divine Shield's state is `divineshield_low_base.m2`
  (165961). Ice Barrier's is 166392. Our renderer draws all three absorb and immunity
  shields with one sphere, but in the client they are three identities. This card covers
  PW:S only.

## 2. The state model: `holydivineshield_state_base.m2` (fdid 166342)

The model is byte-equivalent across the two builds. Only one sequence-flag bit differs.

**It has no particle emitters and no ribbons.** It is three flat 4-vertex quads in one
plane. Their parent bone (bone 0) has flags `0x240`, which is a **cylindrical billboard
locked to the vertical axis**. So the bubble is an upright card that turns to face the
camera, not a sphere. Every material is `flags 19, blend 4`: unlit, unfogged, no depth
write, **additive**.

| layer | bone | texture | quad (client yd) | quad (our rig, ×1.229) | colour |
|---|---|---|---|---|---|
| **Rim glow** | 2 | `genericglow2b1.blp`, a ring with a hard inner edge at r=0.50 and a soft outer falloff to r=0.70 | 3.18 × 4.08, feet −0.91 … +3.17 | 3.91 × 5.01 | `(1.0, 0.91, 0.318)` |
| **Streak ring A** | 1 | `shockwave10d.blp`, ~17 soft radial streaks, band r 0.55–0.90, peak 0.70 | 2.26 × 2.90, feet −0.12 … +2.78 | 2.78 × 3.56 | `(0.969, 0.992, 0.318)` |
| **Streak ring B** | 3 | `shockwave10b.blp`, ~15 streaks, the same band but fainter inside | as A | as A | `(1.0, 0.976, 0.318)` |

- **Rig scale.** `humanmale.m2` stands 2.034 client units tall and our capsule stands 2.5,
  so our rig is ×1.229 the client's. On our rig the rim's inner edge is an ellipse with
  semi-axes 0.98 × 1.25 yd, centred 0.14 yd above the capsule centre. The streak band
  peaks at 0.97 × 1.25 yd, centred 0.38 yd above it. Both clear the 0.5 capsule radius
  sideways. The lower arc crosses the legs.
- **Hollow.** All three textures are black inside r≈0.45. The body itself is never tinted.
  Only a gold outline sits around it.
- **Scale pivot.** Bones 1–3 scale about client z=1.11 (1.36 yd above the feet on our
  rig). The bone's y axis is the quad's WIDTH and its z axis is the HEIGHT.

### Sequences

The tracks give these readings (the sequence ids follow no consistent convention across
earlier docs, so the shape decides):

| seq | id | length | role | width × height scale | texture weights (rim / A / B) |
|---|---|---|---|---|---|
| 0 | 0 | 700 ms | **birth** | 0 until 200 ms, then **0.522 × 1.11** at 300, 1.09 × 1.0 at 500, 0.901 × 1.123 at 700 | all three spike **0 → 1.0 at 300 ms**, then settle to 0.12–0.15 |
| 1 | 158 | 633 ms | **loop** | 0.901 × 1.123 → 1.04 × 1.0 → back, a jelly wobble in anti-phase | rim 0.15 → 0.06 → 0.15; A and B flicker 0 ↔ 0.12–0.20 **in anti-phase** every ~100 ms |
| 2 | 159 | 1100 ms | **death** | swells to **1.271 × 1.158** by 1000 ms | rim fades 0.15 → 0 by 1000 ms; A and B keep flickering to ~900 ms |

All interpolation is linear. Read as a whole:

1. The shield appears **200 ms after the cast**, as a tall, narrow pop at full brightness
   (weight 1.0). It snaps wide and then settles within half a second.
2. It then holds as a **faint shimmer**: a gold rim at about 6–15% strength, with two
   streak rings alternating several times a second. The outline wobbles slowly.
3. When it ends, it **swells outward about 25% and fades over a second**.

The client has **one ending**. Sequence 159 plays when the aura goes, whether the shield
expired or was broken. Nothing marks an absorbed hit except the combat-text "Absorb".

## 3. What we draw today

`update_shield_bubbles` spawns a `Sphere::new(1.0)` scaled **0.9 × 1.4 × 0.9** at the
capsule centre whenever any `Absorb` aura is up. It has base colour `(1.0, 0.95, 0.7, 0.25)`
and emissive `(2.0, 1.8, 1.0)`, with `AlphaMode::Add`. It despawns on the frame the aura
is gone.

| | client | ours |
|---|---|---|
| form | upright **billboard card**: a hollow gold outline | **solid** additive ellipsoid that washes the whole body gold |
| size | rim inner edge 0.98 × 1.25 semi-axes | 0.9 × 1.4 semi-axes. **Size is about right.** |
| centre | +0.14 to +0.38 above capsule centre | capsule centre |
| sustained intensity | faint, 6–20% weight | constant and bright (emissive 2.0 at α 0.25, plus bloom) |
| motion | streak rings shimmer in anti-phase; slow jelly wobble | static |
| arrival | 200 ms delay, then a full-brightness tall pop that settles in 0.5 s | appears at full strength on the frame the aura lands |
| ending | swell 1.27× and fade over 1 s | vanishes on one frame. **Break and expiry look identical, and both look like nothing.** |
| caster side | kit 270, the Holy two-hand launch flare | **nothing**. PW:S is applied in `try_power_word_shield` without a `CastingState`, so `spawn_instant_cast_hands` never sees it, and it is not in `InstantAbilityFired::is_spawned_for` |
| colour | saturated pale gold `(1.0, 0.91–0.99, 0.32)` | desaturated gold-white `(1.0, 0.95, 0.7)` |

## 4. The gap

1. **Shape.** Ours is a filled egg and the client draws a hollow, shimmering outline. This
   is the identity gap. Like the Frostbolt dart and the Shadow Bolt mote, silhouette and
   density carry the recognition here, not hue.
2. **Lifecycle.** Ours has no arrival and no departure. A break, an expiry and "never
   shielded" all look like the frame the sphere disappears. The client gives an arrival
   pop and a swell-and-fade exit.
3. **Duty.** The client's sustained state is quiet, and its loud moment is the birth. Ours
   is loud for the whole 30 s. PW:S is a sustained on-ally channel
   (the same budget that declined Curse of Tongues' persistent state, AS-36). A faithful bubble *lowers* the sustained load
   while making the arrival legible.
4. **Caster.** The Priest's hands do nothing on the cast. The shipped Holy launch is the
   client's own answer, so this needs routing, not new art.

Not a gap: **colour.** Both are Holy gold, which is the existing Holy palette
(`holy_gold_pale`, the heal impacts), so the change spends no new hue.

## 5. Constraints for phase 3

These are open until the user signs off on the bench:

- **Break vs expiry.** The card requires both endings to read as distinct from "shield
  up", and the client's death sequence already does that for both. Whether a break should
  ALSO differ from an expiry is a design call the client does not make. The bench offers
  an authored "break burst" (our own, off by default).
- **Draw order.** The client's quad sits on the body's centre plane, with no depth write.
  Our capsule material is `AlphaMode::Blend` and is sorted by entity distance with no depth write (`play_match/mod.rs`; the trap
  the dispel ribbon hit). A card at the same
  origin as the body sorts ambiguously against it wherever the two overlap. Because the
  ring is hollow, they only overlap where the lower arc crosses the legs. The bench's
  `push` knob (yd toward the camera) is the lever: 0 is the client's placement, and ≥0.5
  puts the whole card in front of the body.
- **No textures from the client ship.** The three textures are to be generated
  procedurally from the measured radial profiles and streak counts, like
  `soft_dot_texture`. The bench draws exactly those procedural textures.
- **Scope.** Ice Barrier and Divine Shield share `ShieldBubble` today but have their own
  client identities (166392 and 165961). PW:S moving to its own card leaves them on the
  sphere until they get their own research.
