# Divine Shield — client data and the gap (AS-359)

Research for AS-359: what the Classic and TBC clients draw for Divine Shield, and how it
differs from the `ShieldBubble` sphere we draw for it today. The bench is
[`benches/2026-10-16-divine-shield-bench.html`](benches/2026-10-16-divine-shield-bench.html)
(published at https://claude.ai/artifact/AehhwbuxGwuaDhttmDYqS2). It awaits the user's sign-off, and no Rust has been written.

Sources: wago.tools DB2 tables and CASC files for Classic Era `1.15.9.69547` and TBC
`2.5.6.69795`. The join is `scripts/db2_spell_sweep.py` over the three Paladin skill lines,
**594 Holy, 267 Protection and 184 Retribution**, followed by a per-kit walk and an M2
parse. The chain, M2 strides and rig scale are the ones AS-344 used
(`2026-10-16-pws-client-data.md`). This note leans on that one, because the two models
turn out to share a skeleton.

## 1. The join

`db2_spell_sweep.py --skill-line 594 267 184 --allow-split` gives:

| build | inventory | no visual | with visual | property 4 |
|---|---|---|---|---|
| Era 1.15.9 | 94 | 41 | 53 | SPLIT on Flash of Light and Holy Shock |
| TBC 2.5.6 | 130 | 67 | 63 | SPLIT on Holy Shock |

The splits are not Divine Shield's. Holy Shock splits into its heal and damage spells (128
and 135), and Era's Flash of Light rank 19993 is an odd one out. `--allow-split` is used
only to read past them. Era's `--era-cut` also drops 23 Season of Discovery names; none of
them is Divine Shield.

Both ranks of Divine Shield (642, 1020) resolve to **SpellVisual 154** in both builds.
**Divine Protection (498, 5573) uses the same visual 154**, so the two are one identity in
the client. Blessing of Protection uses its own visual, 302, with
`blessingofprotection_base.m2` (165700) as its state. Visual 154 has no missile. Its
events are these:

| event | kit | what it is | Era | TBC |
|---|---|---|---|---|
| (1,2) precast, caster | 99 | `ReadySpellOmni` loop and `holy_precast_*_hand.m2` on both hands. **The same kit as PW:S** | `low_hand` (166336) | `med_hand` (166337) |
| (3,13) cast, caster | 417 | `SpellCastOmni` one-shot (anim 54), `holy_precast_low_hand.m2` on both hands (attach 21/22), sound 2562 | same | same |
| (7,8) aura state | 252 | `divineshield_low_base.m2` (165961) at **attach 19, Base (the feet)**, scale 1, sound 39 | target types 1 and 2 | target type 2 only |

Notes on the table:

- **The precast row never plays.** Divine Shield is instant. As with PW:S, Era and TBC
  differ only on that row and on Era's duplicate type-1 state row, which TBC dropped. The
  bubble the player sees is the same in both builds.
- **Kit 417 is kit 270 plus a sound.** It has the same animation and the same hand model
  on both hands, so the cast is the Holy two-hand launch that `heal_cast.rs` already draws
  for PW:S, Flash Heal and Holy Light. It needs routing, not new art.
- **The state kit shares PW:S's sound (39).** The two differ only in the model.

## 2. The state model: `divineshield_low_base.m2` (fdid 165961)

**The file is byte-identical in Era and TBC** (9762 bytes, no differing byte). The six
textures decode to the same images within compression noise. The mean texel difference is
0.2–0.8 out of 255; the BLPs were re-encoded between builds, not redrawn.

### It is PW:S's skeleton

Every structural number matches `holydivineshield_state_base.m2` (166342), the PW:S model:

- bone 0 is the same cylindrical billboard (flags `0x240`), with the same
  global-sequence translation of −0.208 that recentres the cards on the axis;
- the scale bones pivot at the same height, z=1.11;
- the rim quad is 3.18 × 4.08 (feet −0.91 … +3.17), and the streak quads are 2.26 × 2.90
  (−0.12 … +2.78);
- the three sequences have the same ids and lengths: birth 0 (700 ms), loop 158
  (633 ms), death 159 (1100 ms);
- every bone-scale key is the same: nothing for 200 ms, the tall 0.522 × 1.11 pop, the
  0.90/1.04 jelly wobble, and the 1.271 × 1.158 death swell;
- the three texture-weight tracks are the same, key for key;
- every material is the same: `flags 19, blend 4` (unlit, unfogged, no depth write,
  additive).

So Divine Shield **arrives, holds and leaves exactly as PW:S does.** Its timing, its
outline and its quiet duty cycle carry no difference between them.

### What it adds

| layer | bone(s) | texture | quad | colour | weight track |
|---|---|---|---|---|---|
| Rim glow | 1 | `genericglow2b1` (hard-edged ring) | rim | gold `(1.0, 0.91, 0.318)` | 0 |
| **Blue fill** (new) | 2 and 3 | `genericglow64`, a **filled** soft disc (rgb×alpha 0.90 at the centre, 0.24 at r=0.5, 0 by r=0.9) | **two cards**: streak-sized (bone 2) and rim-sized (bone 3) | **blue `(0.318, 0.396, 1.0)`** | 0 (the rim's) |
| Streak B | 4 | `shockwave10b` | streak | gold `(1.0, 0.976, 0.318)` | 1 |
| **Streak A** (recoloured) | 5 | `shockwave10d` | streak | **cyan `(0.318, 0.91, 1.0)`**; yellow on PW:S | 2 |

And three particle emitters, where PW:S has none:

| emitter | bone, position (client) | texture | behaviour |
|---|---|---|---|
| **P1, P2: star sparkles** | 7 and 8, (0.22, 0, 0.775): about 0.95 yd above the feet on our rig | `star5a`, an X-shaped glint with a bright core | sphere emitter (type 2), area 0.78 × 0.83, speed 1.667 ±30% outward in every direction (vRange π), life 0.4 s, **30/s each**, spin +3 and −3 rad/s. Rate 0 until 300 ms into the birth, 30 by 500 ms, 30 through the loop, ramping to 0 over the death. Colour orange `(255,186,77)` → pale `(253,255,200)` at 75% → yellow `(246,255,0)`; alpha 0 → 1 at 75% → 0; half-size 0.056 → 0.083 → 0 |
| **P0: halo fall** | 6, (0.195, 0, 2.641): the crown | `genericglow2c`, a soft ring (zero inside r=0.55, peak rgb×alpha 0.083 at r=0.65) | plane emitter of zero area, speed **−1.914** with vRange 0, so straight down; life 1.355 s, so it reaches the feet; 15.1/s. Colour white → `(255,226,112)` → white; alpha 0 → **0.37** → 0; half-size 0.139 → 1.389 at the chest → 0.139 |

Two readings in that table are uncertain, and the bench exposes both as knobs:

- **The halo's `enabled` track** has keys only in the death sequence: on at 0, off at
  300 ms. If an empty sequence means enabled (the rule that makes P1 and P2 emit at all,
  since their tracks are empty everywhere), the halo falls for the whole shield and stops
  300 ms into the death. Read literally, only 4–5 rings fall at the end. The bench defaults
  to the shorter reading, per the AS-136 precedent. Either way its texture peaks near 8%
  and its alpha at 0.37, so its contribution is under 3%, which is close to invisible.
- **Particle scale** is read as a half-size (the WMV convention), and the sphere emitter's
  spawn radius as up to the area value. Both affect size only.

### Read as a whole

1. The arrival is PW:S's: nothing for 200 ms, then a tall pop at full weight. Here the pop
   is a **blue-white flash filling the body**, because the fill carries weight 1.0 at
   300 ms on both of its cards.
2. The hold is PW:S's faint gold outline (6–15%), plus a faint blue glow over the body,
   a cyan-and-gold shimmer, and **a continuous spray of gold glints** (about 24 live at a
   time).
3. The departure is PW:S's swell-and-fade, with the sparkles thinning to nothing over the
   same 1.1 s.

## 3. How the client tells Divine Shield from Power Word: Shield

**The outline cannot tell them apart; the interior and the motion do.** They share the
gold rim card, its size, its timing and its brightness. Divine Shield is told apart by:

1. **a filled body**: blue-white glow inside the outline, where PW:S is hollow;
2. **the cyan streak**: PW:S's shimmer is all yellow;
3. **sparkles**: Divine Shield throws off gold glints for its whole duration, while PW:S
   has no particles.

**On one Paladin at once**, both models attach at the feet and add. The two rims coincide
exactly and sum, each on its own clock: about double brightness when their wobbles are in
phase, a slight doubling when they are not. Everything Divine Shield adds stays visible
through PW:S, because PW:S adds only gold to the outline. Bench this with the *PW:S
overlay* layer.

## 4. What we draw today

`update_shield_bubbles` (`rendering/effects/shield_bubbles.rs`) gives any unit with a
`DamageImmunity` aura a `Sphere::new(1.0)` scaled **1.17 × 1.82 × 1.17**. It is drawn
with base colour `(1.0, 0.85, 0.3, 0.4)` and emissive `(3.0, 2.5, 0.8)`, `AlphaMode::Add`,
at the capsule centre. `follow_shield_bubbles` pulses it ±5% at 3 rad/s (a 2.1 s period).
Immunity takes priority over absorb in that system, so Ice Barrier's sphere never doubles
it. PW:S is drawn separately (`pw_shield.rs`), so a shielded Paladin already shows both.
Divine Shield has no caster gesture: it is spawned as `DivineShieldPending` and is not in
`InstantAbilityFired::is_spawned_for`.

| | client | ours today |
|---|---|---|
| form | the PW:S outline cards plus a blue fill and particles | a solid gold egg |
| size | rim inner edge 0.98 × 1.25 semi-axes; the outer fill card is at a quarter strength there and dark by 1.76 × 2.26 | 1.17 × 1.82 semi-axes: 1.2× wider and 1.46× taller |
| centre | +0.14 to +0.38 above the capsule centre | the capsule centre |
| hue | gold rim, **blue-white body**, cyan and gold shimmer, gold glints | gold throughout |
| sustained intensity | faint cards (6–20%), sparkles at full alpha but tiny | bright and constant (emissive 3.0 at α 0.4, bloomed) |
| motion | jelly wobble, anti-phase shimmer, 60 glints/s | a slow ±5% pulse |
| arrival | 200 ms delay, then a full-strength blue-white pop | full strength on the frame the aura lands |
| ending | swell and fade over 1.1 s, sparkles thinning | gone on one frame |
| caster | kit 417, the Holy two-hand launch | nothing |

## 5. The gap

1. **Identity.** Today's Divine Shield is distinguished from the old PW:S sphere only by
   size, brightness and a pulse, all in gold. Now that PW:S wears the client's cards, ours
   is a gold egg beside a gold outline. The client draws them as the same outline,
   distinguished by a **blue-white fill, a cyan shimmer and sparkles.** That is the
   distinction to build.
2. **Lifecycle.** As for PW:S: no arrival, and a one-frame exit. In our sim Divine Shield
   only ever expires: it is immune to the dispels we have, and it prevents its own
   bearer's death. So one ending is all it needs, and the client has only one.
3. **Hardness.** The card asks for "a hard, full-duration bubble". The client's sustained
   cards are as faint as PW:S's. In the client, what reads as solid and continuous is the
   blue body fill together with the unbroken sparkle spray, not a bright shell. If the
   client values read too soft at our 60 yd camera, the levers are `DS_FILL_GAIN` and
   `DS_RIM_GAIN`. The bench's *Harder read* preset is one suggestion: rim ×2, fill ×2.5,
   streak ×1.5, sparkle size ×1.5.
4. **Caster.** Kit 417 is the Holy launch, so this needs routing (an `InstantAbilityFired`
   for `DivineShield`), as AS-344 did for PW:S.

**Budget.**

- *Hue.* Blue is new on a Paladin, but it is the client's own identity and is spent on a
  world-space effect, not on UI text. It sits well away from the FCT reservations
  (yellow damage, green heals, light-blue absorbs): the fill is a saturated periwinkle
  `(0.32, 0.40, 1.0)`, and over the Paladin's pink it adds to a lavender-white. The nearest
  existing blues are Mage frost (Ice Block, Frost Nova, and AS-358's Ice Barrier), which
  never land on a Paladin.
- *Sustained channels.* Divine Shield is self-only and lasts 12 s on a 5-minute cooldown.
  At worst it shares the Paladin with PW:S and an aura band.
