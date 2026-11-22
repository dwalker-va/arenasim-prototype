# AS-336: Rogue talent grants and procs — Client-Data Research

This doc covers the Rogue talent effects on AS-301 (the talents proposal's Rogue trees) that
need a visual: Rupture, Hemorrhage, Expose Armor and Shadow Dance (the grants), Master of
Subtlety (the proc) and Cheat Death (the mechanic). Improved Kidney Shot, Blood Spatter and
the stat nodes leave no footprint of their own and are skipped (the sweep reports Improved
Kidney Shot NO-VISUAL at both builds; the others are not spells). The poisons and the
Dodge/Parry text are in the AS-333 doc (`2026-10-17-baseline-kit-client-data.md`).

Findings come from three builds:

- Classic Era **1.15.9.69547**;
- TBC Classic **2.5.6.69795**, the reference for everything below unless a row says
  otherwise (Hemorrhage's rank 4, Cheat Death and Master of Subtlety are TBC spells);
- Wrath Classic **3.4.5.63697**, for Shadow Dance only (51713 is in neither other client).

The sources are the wago.tools DB2 CSVs, joined directly, and M2 files fetched from CASC and
parsed. No wiki or prose source was used as evidence. Where a join could not be made, or a
reading rests on inference rather than a table, the row says so (**UNRESOLVED** or
"inferred").

## Method

- **Inventory.** `scripts/db2_spell_sweep.py --build <b> --skill-line 38 39 253 --name
  Rupture Hemorrhage "Expose Armor" "Shadow Dance" "Master of Subtlety" "Cheat Death"
  "Improved Kidney Shot" "Blood Spatter" --events --caster-anims --allow-split` at Era and
  TBC. Skill lines: 38 Combat, 39 Subtlety, 253 Assassination.
  - Counts. Era: `5 = 2 + 3`; TBC: `7 = 4 + 3`. Visual: Rupture, Hemorrhage, Expose Armor.
    No visual: Improved Expose Armor, Improved Kidney Shot, and at TBC the passives Cheat
    Death (31228–31230) and Master of Subtlety (31221–31223).
  - Property 4 **HELD** at both builds: every named spell with a visual resolves to exactly
    one `SpellVisual` at Probability 1. (`--allow-split` was passed but not needed.)
  - The run is narrowed by `--name` (and at Era the default `--era-cut 400000` excluded 9
    Season of Discovery names). These are claims about the named spells only.
- **Helper and proc spells**, from `SpellName` (exact name, SpellID < 400000) and confirmed
  from `SpellEffect`:
  - Cheat Death: the talent ranks 31228–31230 are aura 69 (school absorb) passives; 31231 is
    a dummy. The **3 s buff is 45182 "Cheating Death"** (aura 4, base points −91, i.e. −90 %
    damage taken; misc 127 all schools; duration index 27 = **3000 ms**). It carries visual
    **10423**. (28845/28846 "Cheat Death" are an NPC version, aura 42/115, and are ignored.)
  - Master of Subtlety: the talent ranks 31221–31223 are aura 4 dummies; the proc is
    **31665** (aura 79, damage done %, all schools) with **31666** (aura 226 periodic dummy,
    6000 ms). Neither has a `SpellXSpellVisual` row at TBC or Wrath.
  - Shadow Dance 51713 (Wrath): aura 275 (ignore shapeshift restrictions) plus aura 36
    shapeshift into **form 13 "Shadow Dance"**. Duration index 32 = **6000 ms** in Wrath (our
    design uses 8 s). `SpellShapeshiftForm` 13 has no creature display and no preset spells,
    so the form adds no visual of its own.
- **Joins.** `SpellXSpellVisual → SpellVisual → SpellVisualEvent → SpellVisualKitEffect`
  (effect types 2 model attach, 6 body anim, 1 procedural, 5 sound — ignored). Event pairs:
  `(3,13)` cast, `(6,13)` impact, `(7,8)` sustained aura state.
- **Model names** from the community listfile (`as333/dl/listfile.csv`), confirmed by the
  M2 `NAME` field where present (the Wrath Shadow Dance model and the Era copy of
  `bloodyexplosion.m2` carry an empty name; the listfile names both).
- **M2 parser** (v274): the AS-333..335 `m2.py`. **None of these models has a `GPID`
  chunk**: every emitter is a sprite.
- **Units and conventions** (as AS-335): model units ≈ yards; particle colours RGB 0–255
  over normalised particle life, alpha 0–1; blend 2 = alpha, 4 = additive; attachments 19
  Base (feet), 20 Head, 21/22 hands, 34 Chest. Sequences: 0 Stand is the onset, 158 the
  loop, 159 the end; seq flags `0x820` loop, `0x8a1` play once. Body anim 57 is
  Special1H, the anim Mortal Strike plays.
- **Scripts** are under the AS-336 scratchpad (`as336/`): `db.py`, `chain.py`, `names.py`,
  `reverse.py`, `proctypes.py`, `m2.py` (copied from AS-335). Outputs:
  `sweep_<build>.txt`, `chain_<build>.txt` (three builds), `models/<build>_<fdid>.txt`.

## Summary table

| Effect | Client visual | Where | Sustained state? |
|---|---|---|---|
| Rupture | 250: hand flash (cast) + **`bloodyexplosion.m2`** (impact) | Rogue's hands; victim chest | **No** (12 s DoT draws nothing) |
| Hemorrhage | 5119: Special1H swing + deep-red weapon trail (cast) + `bloodyexplosion.m2` (impact) | Victim chest | **No** (stacking debuff draws nothing) |
| Expose Armor | 3441: hand flash (cast) + **`exposearmor_head.m2`** (impact, 3 s) | Rogue's hands; victim head | **No** (15 s debuff draws nothing after the 3 s) |
| Shadow Dance | 11827 (Wrath): **`shadowdance_state.m2`**, a looping model, on an impact event | Rogue's chest | **UNRESOLVED** (see §4) |
| Master of Subtlety | none | — | — |
| Cheat Death | 10423 on the 3 s "Cheating Death" buff: **`cheat_death.m2`** | Rogue's feet | **Yes**, `(7,8)`: onset 1.9 s, then a loop for the 3 s |

### Five findings that shape the work

1. **Rupture, Hemorrhage and our shipped Rend share one client impact,
   `bloodyexplosion.m2`** (kit 220 / 4490): twelve blood-spurt emitters spraying from the
   chest for 0.5 s under gravity. Kit 220 is Rend's impact too, so the bleed family has a
   common landing the engine does not draw today (Rend ships drips only).
2. **No bleed or armor debuff has a client state.** The victim's 12 s Rupture, 15 s
   Hemorrhage stacks and 15 s Expose Armor are all icon-only after their landing. So the
   kill target's sustained channels do not grow: Rupture can join Rend's shipped red drips
   (`DripKind::Bleed`), and the other two need no state.
3. **Rupture and Expose Armor share the "finisher" cast kit 394**: a red-and-gold glow on
   both of the Rogue's hands (`bloodlust_state_hand.m2`). Warrior Execute uses the same
   visual. Hemorrhage instead carries the melee-special weapon trail (procedural type 8,
   deep red 0xBE0307), the same effect class as Sinister Strike's and Kidney Shot's, which
   the engine records but does not draw.
4. **Shadow Dance's client visual is a dark-red, black-wisped chest state, not a
   transparency.** It cannot be confused with our stealth (40 % alpha, darkened body). Its
   persistence for the window is UNRESOLVED from the tables.
5. **Cheat Death has its own client moment and state**: a lavender skull flash above the
   head with ring bursts, then a lavender ghost-fire column round the body for the 3 s
   buff. AS-335 ruled that Cheat Death shares the authored "Saved" cue (gold text and gold
   flash); the client's own purple moment is a separate question for this card.

## 1. Rupture

- TBC 1943 (and ranks 8639–11275, 26867): aura 3 periodic damage, period 2000 ms, on the
  enemy. Visual **250**, identical at all three builds apart from the differences below.
- `(3,13)` caster, **kit 394**: body anim loop 57 (Special1H); `bloodlust_state_hand.m2` at
  attachments 21 and 22 (both hands), scale 1.
- `(6,13)` target, **kit 220**: `bloodyexplosion.m2` at attachment 34 (chest), scale 1.
- Era and Wrath also carry a `SpellProceduralEffect` **type 13** on kit 220 (value
  0xEC3200, 0, 6, 0) that TBC lacks, and Era adds a `(6,13)` caster row playing kit 220 on
  the Rogue. Type 13 is otherwise used on impact kits such as Chest Pains; its meaning is
  **UNRESOLVED**. TBC, the reference, has neither.

### `bloodlust_state_hand.m2` (@each hand, cast)

- Sequence 0, 1000 ms, looping (`0x820`); the cast kit holds it for the cast moment only.
- A small quad (`red_glow3`, about 0.9 yd across, additive) on each hand.
- **P0** shockwave rings: 10/s, life 1.2 s, speed 0.28; yellow (255,216,0) → red (250,5,0)
  at 0.65; alpha 1 → 0.13; 0.22 → 0.41 yd.
- **P1** cloud puffs: 40/s, life 0.7 s, speed 0.56; red (255,0,0) → orange-red (255,54,0);
  alpha 1 → 0; 0.28 → 0.03 yd.

### `bloodyexplosion.m2` (@Chest, one-shot)

- Sequence 0, **667 ms**, play once (`0x8a1`). No mesh.
- **Twelve** identical blood-spurt emitters (`bloodspurtsmall01.blp`, alpha-blended — the
  texture carries the red; the tint is white), on bones fanned round the chest. Each: burst
  rate 150/s 0–167 ms, 50/s at 333 ms, off at 500 ms; life **0.5 s**; speed **2.2 yd/s**
  (the two lowest at 4.4); vRange 0.175 rad (a narrow cone); **gravity 6.9**; tail 0.05;
  alpha 1 → 0.69 → 0; scale 0.014 → **0.089** → 0.003 yd.
- Read together: a short spray of red droplets out of the chest that arcs down within
  half a second.

### What this means for design (Rupture)

- Landing: `bloodyexplosion.m2` on the victim, plus the finisher hand flash on the Rogue.
- The motion (user, 2026-10-17): an authored **apply stroke**, shared with Expose Armor: a
  **backhand sweep**. The weapon hand winds up across the body, sweeps outward across the
  victim's chest (contact mid-arc) and follows through wide, so it reads apart from the Rogue's
  thrusts and Sinister Strike's rising slash; the hand flash rides the striking hand and the
  landing fires at contact. Reusing a generic cast pose does not read
  as putting the debuff on the target. The client's Special1H is not drawn.
- State: none in the client. **Route Rupture to `DripKind::Bleed`**, Rend's shipped drips:
  one line in `DotStateVisual::for_dot`, and the drip is already this game's "bleeding"
  read. No new channel.

## 2. Hemorrhage

- TBC 16511 (and 17347, 17348, 26864): weapon damage (Eff 121, 31) plus aura 14 (mod
  damage taken, Physical) on the enemy. Visual **5119**.
- `(3,13)` caster, **kit 4491**: body anim loop **57 (Special1H)**, Mortal Strike's anim;
  `SpellProceduralEffect` **type 8**, values (0xBE0307, 15, 400, 96). Type 8 sits on the
  cast kits of melee specials — Charge, Mortal Strike (0xF82929, 20, 600, 100), Cleave,
  Sunder Armor, Hamstring, Eviscerate, Kidney Shot — so it reads as a coloured weapon
  trail (colour, width, duration ms, alpha %; inferred). The engine records Sinister
  Strike's (0xBD55C6) and draws none.
- `(6,13)` **target 1** and **target 2**, kit 4490: `bloodyexplosion.m2` at the chest.
  Rend has the same pair. Target 2 is the victim. A target-1 row on a hostile strike would
  spray blood on the Rogue too; we read it as data carried over with the kit (inferred)
  and the bench leaves it off by default.

### What this means for design (Hemorrhage)

- The strike: our shipped Sinister Strike stroke (Hemorrhage replaces it in the rotation),
  or a Mortal Strike-family stroke to match the client's Special1H. A question for the
  user.
- Landing: `bloodyexplosion.m2` on the victim, every hit. Stacks show on the icon.
- No state.

## 3. Expose Armor

- TBC 8647 (and ranks to 26866): aura 22 (mod resistance, armor) on the enemy. Visual
  **3441**. (Wrath 8647 has moved to aura 101, armor %, with the same visual.)
- `(3,13)` caster: **kit 394**, Rupture's hand flash.
- `(6,13)` target, **kit 3069**: **`exposearmor_head.m2`** at attachment 20 (head). The
  model is also used by the NPC spell Armor Disruption.

### `exposearmor_head.m2` (@Head, one-shot, 3 s)

- Sequence 0, **3000 ms**, play once. Mesh: three quads, 0.5–1.5 yd above the head
  attachment.
  - Batches 0 and 2: a **shield crest** (`shield_crest_a_01silver.blp`, alpha and
    additive), about 0.5 yd tall. Its colour is **gold** (0.94, 0.83, 0.32) to 667 ms,
    then turns **blue** (0.32, 0.54, 0.94) by 1000 ms and holds. Fades in by 134 ms and out
    from 2800 ms.
  - Batch 1: a blue glow quad (`blue_glowd`, additive), ~0.9 yd, gold → blue on the same
    keys, weight 0.83 from 400 to 2600 ms.
  - Bone 1 (the crest) **squashes** between 700 and 1000 ms: scale 1.5 → (1.5, 1.3, 1.7)
    → (1.0, 1.2, 0.8) → 1.0. The crest jolts as it turns blue: the armor breaking.
- Particles:
  - **P0, P1, P2, P4** sparkles (`star4`, additive, spin): 7.5–15/s, life 0.2 s; on
    134–300 ms and 500–667 ms (P2/P4 300–500 ms); gold (255,210,0) → white → cyan
    (0,246,255); 0.02 → 0.21 → 0.02 yd. Glints across the crest in its gold phase.
  - **P3** smoke (`toonsmoke16`, additive): 25/s, 500–2600 ms, life 1.0 s, speed 0.56,
    drag 2.5; pale blue (195,207,225) → (186,210,255) → deep blue (12,0,255); alpha 0 →
    0.2 → 0; 0.24 → 0.57 yd. A faint blue haze about the crest.
  - **P5** shatter streaks (`grad2db`, additive): **150/s, 400–800 ms**, speed −0.42
    (inward/down), life 0.5 s, tail 0.4; magenta (255,18,115) → blue (0,162,255) → violet
    (120,0,255); 0.03 → 0.17 → 0 yd. The burst that sells the break.

### What this means for design (Expose Armor)

- A 3 s apply: Rupture's apply stroke and hand flash on the Rogue, then a crest above the victim's head
  that glints gold, cracks blue with a streak burst, and fades. The crest is an item
  texture; ours would be an authored crest shape.
- No state for the 15 s after; the icon carries it.

## 4. Shadow Dance (Wrath Classic 3.4.5.63697)

- 51713: aura 275 + shapeshift to form 13, 6000 ms. Visual **11827**.
- **No cast kit, no `(7,8)` state.** One kit, **10921**, on `(6,13)` for **both target 1
  and target 2** (the Rogue is both): `shadowdance_state.m2` at attachment 34 (chest),
  scale 1. Kit flags 0, unlike most impact kits (0x100000).
- The same kit shape (`(6,13)`, the same model) is used by ICC's **Essence of the Blood
  Queen** (70871, a permanent buff, duration −1) and Pact of the Darkfallen. A permanent
  buff whose only visual is a one-shot impact would show nothing, so the client most likely
  keeps a looping model attached for the aura when its impact kit has no end (inferred).
  The tables do not say so: **whether the state persists for the window is UNRESOLVED.**

### `shadowdance_state.m2` (@Chest, looping)

- Sequence 0, **1967 ms**, **looping** (`0x820`). No mesh, no animated rates: every emitter
  runs at a constant rate, which is the shape of a state, not a one-shot.
- **P0** glow (`genericglow2c`, alpha-blended): 3/s, life 1.5 s, still; white → dark red
  (144,35,35) → near black (43,5,5); alpha 0 → **0.78** → 0; **0.04 → 0.43 → 1.74 yd**. A
  slow dark-red bloom that swells from the chest.
- **P1** wisps (`shoulder_robe_raidpriest` cloth texture, **additive**): 33/s, life 1.2 s,
  speed 0.56 outward; grey-brown (55,47,47) → dark red (126,44,44) → (92,0,0); alpha 0 → 1
  → 0; 0.18 → 0.24 yd. Dark-red ribbons of cloth-shadow drifting off the body.
- **P2** black wisps (same texture, **alpha-blended**): 33/s, life 1.0 s, speed **−1.39**
  (drawn inward); (117,47,47) → grey (44,44,44) → black; alpha 0 → 1 → 0; 0.16 → 0.48 yd.
  Black shadow pulled into the chest.
- **P3** faint cloud (`alphacloud`, alpha), 1.16 yd up: 11/s, life 0.5 s; near-black →
  dark red; alpha ≤ 0.25; 0.01–0.03 yd. Negligible.

### What this means for design (Shadow Dance)

- The window must read for its whole 8 s (AS-301 ruling). The faithful reading is the
  client model held as a looping state on the Rogue for the window, its own shape (a
  dark-red bloom with black shadow drawn in and red shadow drifting out), never a fade of
  the body. A quieter alternative is the onset only plus the icon, which fails the
  "window must read" constraint. A question for the user, with the persistence
  UNRESOLVED.
- The end of the window has no client cue. Answers are timed against the Dance, so its
  end matters as much as its start; the bench offers an authored fade-out.

## 5. Master of Subtlety

- 31665 (aura 79, +damage done) and 31666 (aura 226, 6000 ms): **no visual** at TBC or
  Wrath. The talent passives have none either.

### What this means for design (Master of Subtlety)

- Nothing to draw from the client. Our engine already gives any new buff the generic
  rising `AuraBand` ring (as Inspiration got on AS-335); that, plus the icon, is the free
  answer. It fires as the Rogue leaves stealth, where the unstealth already draws the eye.

## 6. Cheat Death (and the 3 s "Cheating Death" buff)

- 45182 Cheating Death: visual **10423**, one event, **`(7,8)` target 2** (the buff's
  bearer), **kit 9543**: `cheat_death.m2` at attachment **19 (feet)**, scale 1. A sound.
- The same model is on kit 8018 (attachment 34, `(3,13)`, visual 9443) with no spell.

### `cheat_death.m2` (@feet, onset then loop)

- Sequences: **0 Stand 1900 ms** (onset, play once), **158 566 ms** (loop), 159 633 ms
  (end). Global loops 433 and 4100 ms.
- Mesh:
  - Batch 0: a **skull** (`skull.blp`, alpha), 2.2–3.6 yd above the feet, i.e. floating
    over the head; colour lavender **(0.58, 0.47, 1.0)**; alpha 1 to 567 ms, gone by
    867 ms; texture weight fades in over 567 ms. Only in the onset.
  - Batch 1: a small white glow (`blue_glow2`) at 2.8–3.5 yd — the skull's eyes; same
    alpha keys.
  - Batch 2: a tall flare quad (`t_vfx_flare05`) 3.2 yd wide and running from below the
    floor to 12 yd up, alpha 0 → **1 at 767 ms** → 0 at 1100 ms: a vertical flash of
    light through the body.
  - Batch 3: a **cylinder** about 0.8 yd across and 2.5 yd tall about the body, with a
    desaturated animated fire texture tinted lavender **(0.71, 0.53, 0.96)**; alpha 0 to
    800 ms, **1 from 1367 ms** through the onset and the loop; in the end sequence 1 → 0
    over 600 ms. **The ghost-fire column: the sustained state.**
- Particles (all onset only):
  - **P0** smoke (`toonsmoke16`, additive) at 3.3 yd: 50 → 100/s to 1100 ms, life 2.5 s,
    speed −0.56; indigo (42,12,157) → (60,15,203) → (97,110,203); 0.83 → 0.26 → 0.42 yd.
  - **P1–P3** rings at 3.0 yd (over the head): a dark ring (alpha), 15/s at 567–800 ms,
    white → brown-orange → tan, 1.1 → 1.4 yd; a white ring 25/s at 667–900 ms, 0.56 →
    0.97 yd; a shockwave 15/s at 700–800 ms, 0.56 → 0.83 yd.
  - **P4** flare 20/s at 567–733 ms, 1.1 → 1.4 yd; **P5** circle 15/s at 700–833 ms,
    slate → white → mauve, 0.56 → 1.67 yd.
  - **P6** streaks (`t_vfx_flar_blur_64`): **300/s at 567–833 ms**, speed **8.3 yd/s**,
    life 2 s, white → lavender (183,183,255); a spray of light streaks out of the skull.
- Read together: a lavender skull appears over the head, bursts in rings and streaks at
  about 0.6 s, a column of light flashes through the body at 0.77 s, and the body is left
  wrapped in lavender ghost-fire until the buff ends.

### What this means for design (Cheat Death)

- AS-335's ruling: a prevented death plays the shared authored "Saved" cue (gold "Saved"
  text, `fct::INVULNERABLE`, and a 0.6 s gold body flash; built by AS-354).
- The client adds two things the "Saved" cue does not cover: the **skull moment** (the
  save itself, 1.9 s) and the **ghost-fire column** (the 3 s damage reduction). Options:
  the Saved cue alone (DR on the icon); the Saved cue plus the client's skull and
  ghost-fire; or the client model in place of the Saved cue's gold flash, keeping the
  shared "Saved" text. A question for the user.
- The ghost-fire is lavender; the "Saved" flash is gold. Played together, the gold flash
  comes first (0.6 s) and the skull peaks at 0.6–0.8 s, so they overlap.

## Channel count on the kill target (`sustained-visual-channel-budget`)

A Combat/Assassination Rogue's kill target can carry Rupture, Hemorrhage stacks and Expose
Armor at once. Their sustained client channels: **one** (Rupture, through the existing
bleed drips, shared with Rend). Hemorrhage and Expose Armor are landing-only. Shadow
Dance and Cheat Death are on the Rogue, not the victim. No new victim channel is needed.

## Not drawn by the engine today (what a build would add)

- `bloodyexplosion.m2` (the bleed landing): new; could also serve Rend's landing, outside
  this card's scope.
- The finisher hand flash: new (Rupture, Expose Armor).
- The crest: new (Expose Armor).
- The Shadow Dance state: new; on the Rogue's body as an emitter set, never through the
  body material (stealth owns that).
- `cheat_death.m2`: new, if kept.
- Type-8 weapon trails stay undrawn, as for Sinister Strike and Kidney Shot.
