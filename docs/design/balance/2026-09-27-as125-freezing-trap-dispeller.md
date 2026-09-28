# Freezing Trap: the healer, the enemy it would catch, and the opener — measurement

**Date:** 2026-09-27
**Card:** AS-125
**Tier:** DIRECTIONAL (`sweep-tiers.md`), 1v1 and 2v2. Success condition: no
opening trap is thrown where its victim is just dispelled; a trap springs on
the enemy it was decided on; with a melee on the Hunter, the trap goes on the
enemy healer; the Hunter positions for that trap only while it is ready; in
the opener Aimed Shot goes before Serpent Sting when it has time to finish.
The Warlock cells, the Rogue + healer cells and the no-healer cells are
reported whichever way they move. Win rate is reported, not tuned.
**Arms:** before = `061dfa5` (main); after = this branch (release binary
sha1 `5629306a`). Same JSONL both arms.
**Raw rows:** `2026-09-27-as125_{1v1,2v2}_base_061dfa5.csv` and
`..._after.csv` (one row per match, from
`sweeps/2026-09-27-as125-hunter-{1v1,directional}.jsonl`);
`2026-09-27-as125_2v2_after_trap_setup_off.csv` (the after binary with
`movement.ron`'s Hunter `trap_setup: 0.0` — the movement attribution arm);
`2026-09-27-as125_trap_events.csv` (one row per Freezing Trap thrown) and
`2026-09-27-as125_trap_matches.csv` (one row per match), both arms, from the
27-comp mechanism set.

## What the Hunter does now

Freezing Trap asks two questions before it is thrown: **who will spring it,
and would that enemy just be freed?** Who would free a victim comes from the
engine's own removal rules (`ally_removal` / `can_free_ally` / `ally_freers`,
asked of the aura the trap actually lands, `traps::freezing_trap_aura`), never
from a class list: the Priest, the Paladin and the Felhunter free a trapped
teammate; only the Felhunter frees a trapped pet; nobody frees a trapped
Felhunter.

### With a melee on the Hunter: the trap goes on the healer

While a Warrior or Rogue is inside the Hunter's 20yd closing band and the trap
is ready, Freezing Trap is thrown at the **enemy healer**
(`hunter::try_pressure_trap`) — first in the closing band, and second to
Disengage in the dead zone. The healer must be (`pressure_trap_healer`):

- **trap-eligible** — alive, not immune, not incapacitate-DR-immune, and no
  friendly DoT on it that would break the trap on its first tick;
- **not freed** — no teammate of the healer's can lift it (a Rogue or Warrior
  partner cannot; a Felhunter or a second healer can);
- **not a teammate's target** — every damage source holds fire on a trapped
  enemy, so trapping the healer a partner is killing idles that partner for
  the trap's duration. The Hunter's **own** kill target does not hold it: on
  the throw, a Hunter that was killing the healer moves its target to the
  melee on it (`pressure_trap_retarget`) — trap the healer, kill the melee.

and it must land cleanly (`pressure_trap_landing`): the healer's led position
within the trap's configured 30yd range, the healer predicted to spring it
(`predicted_trap_springer`), and no other enemy near the landing. A held
throw is traced with why (`healer trap held: …`). Serpent Sting is held off a
healer the trap is reserved for while a melee is in view, since a sting would
make it untrappable.

### Positioning for it, only while the trap is ready

While the trap is ready and such a healer exists (`trap_setup_healer`), the
Hunter's repositioning under melee pressure takes the healer into account:

- **Kite** — a `trap_setup` scorer term (`movement.ron`, Hunter 3.0) pulls
  toward the healer once a step would leave it beyond throw range (the trap's
  range less its 5yd trigger radius); zero inside that range. It sits below
  `flee` (6.0), so the kite still escapes the melee, bent toward the healer.
- **Disengage** — the leap is bent by the same two weights when a straight
  leap would land beyond that range, never so far that it stops carrying the
  Hunter away from the melee (`trap_setup_disengage`).

On cooldown, or with no such healer, movement is exactly as before. The trace
carries a `trap_setup` scorer term on KITE decisions and names the healer as
the Disengage's `target_id` when the leap bent.

### The lane trap and the off-target trap

- **Off-target (aimed):** the best eligible enemy the team is not killing —
  healer first — at its position (led if moving), only when no other enemy is
  near the landing. A healer is always worth one; anyone else when nobody
  could free it, or when every teammate that could is one the Hunter's team is
  killing (`trap_victim_worth_it`).
- **Lane throw (no off-target):** to the lane midpoint toward the enemy
  healer (or the kill target), decided on the enemy predicted to **spring** it
  — the first inside the trigger radius once armed. Held when nobody is
  predicted to reach it within 3s of arming, when a teammate of the springer
  would free it, or when the Hunter's team is attacking the springer. While an
  enemy is in stealth the springer is that unseen enemy; the throw is held
  when a visible enemy would free it, and otherwise goes with no aim recorded.
- **Range:** every placement refuses a landing beyond the ability's configured
  `range` (30yd); ranged placement itself is intended (the Trap Launcher
  model).

### The opener: Aimed Shot before Serpent Sting when it has time

Until the Hunter's first Aimed Shot or Serpent Sting of the match, a due sting
waits behind Aimed Shot when the cast can finish (`aimed_shot_has_time`): no
visible enemy that could stop it reaches it first. Each such enemy has a
reach — its own kit's interrupt range (`interrupt_reach`, from
`abilities.ron`'s `is_interrupt`: Spell Lock and Wind Shear at 30yd, Kick and
Pummel in melee) or the Hunter's 8yd dead zone for a Warrior or Rogue — and
reaches it at its distance over its closing speed; one already within reach
leaves no time, one held in hard CC past the cast cannot act. The cast time is
read from `abilities.ron`. After the opener, a due sting keeps its place, so a
sting re-applied after a Devour Magic is not delayed.

## The mechanism: where the traps went

27 trap-relevant comps (2v2, 3v3, 1v1), seeds 0-19, traced (540 matches per
arm). The set fields every melee + healer shape the healer trap reaches, and
Rogue + Priest in **both slot orders** (Hunter first, and partner first as the
directional sweep fields it).

| | before | after |
|---|---|---|
| traps thrown | 506 | 339 |
| **sprung on the enemy the throw was decided on** | **103 of 451 named** | **222 of 232 named, + 40 of 40 unseen** |
| **sprung on a healer** | **31** (101s held in all) | **118** (730s held in all) |
| removed by an enemy dispel | 292 | 64 |
| broken by damage | 3 | 8 |
| ran the full duration | 156 | 200 |
| never sprung | 55 | 67 |

Per comp: traps per match, matches with none, decided-victim vs springer (`+Nu`
counts sprung throws decided on an unseen enemy), fate as
removed/broken/full/unsprung, traps that sprang on a healer with the mean
seconds it was held (8.0s is the full, undiminished trap). Hunter-side wins out
of 20.

| comp | before: traps/m | zero | decided = springer | fate | on a healer | after: traps/m | zero | decided = springer | fate | on a healer | Hunter wins |
|---|---|---|---|---|---|---|---|---|---|---|---|
| H+Pri vs Rogue+Priest | 1.00 | 0 | 0/20 | 20/0/0/0 | 0 | 0.90 | 3 | 13/13 | 0/1/12/5 | 12 (7.4s) | 1 → 11 |
| H+War vs Rogue+Priest | 1.00 | 0 | 0/20 | 20/0/0/0 | 0 | 0.00 | 20 | - | - | - | 0 → 11 |
| H+Pri+War vs Mage+Priest+Rogue | 1.00 | 0 | 0/20 | 20/0/0/0 | 0 | 0.00 | 20 | - | - | - | 0 → 1 |
| H+Pal vs Rogue+Priest | 1.00 | 0 | 0/20 | 20/0/0/0 | 0 | 1.20 | 0 | 17/19 | 2/0/17/5 | 17 (8.0s) | 2 → 5 |
| H+Mage vs Rogue+Priest | 1.00 | 0 | 0/20 | 20/0/0/0 | 0 | 0.00 | 20 | - | - | - | 3 → 15 |
| H+Wlk vs Rogue+Priest | 1.00 | 0 | 0/20 | 20/0/0/0 | 0 | 0.00 | 20 | - | - | - | 1 → 8 |
| H+Rogue vs Rogue+Priest | 1.00 | 0 | 0/20 | 20/0/0/0 | 0 | 0.00 | 20 | - | - | - | 13 → 0 |
| Pri+H vs Rogue+Priest | 1.85 | 0 | 7/27 | 20/0/7/10 | 0 | 1.10 | 0 | 20/20 | 19/0/1/2 | 0 | 8 → 0 |
| War+H vs Rogue+Priest | 1.00 | 0 | 0/20 | 20/0/0/0 | 0 | 1.20 | 0 | 19/20 | 18/1/1/4 | 2 (4.4s) | 20 → 1 |
| Pal+H vs Rogue+Priest | 2.05 | 0 | 14/41 | 32/0/9/0 | 7 (8.0s) | 1.00 | 0 | 4/6 | 3/0/3/14 | 2 (8.0s) | 11 → 3 |
| H+Pri vs Rogue+Paladin | 1.00 | 0 | 0/20 | 20/0/0/0 | 0 | 1.20 | 0 | 14/18 | 3/4/11/6 | 14 (5.4s) | 0 → 4 |
| H+Pri vs Warrior+Priest | 0.15 | 17 | 2/2 | 0/0/2/1 | 2 (8.0s) | 1.00 | 0 | 20/20 | 0/0/20/0 | 20 (8.0s) | 19 → 19 |
| H+War vs Warrior+Priest | 1.25 | 0 | 0/20 | 0/0/20/5 | 0 | 0.00 | 20 | - | - | - | 20 → 20 |
| H+Pri vs Warrior+Shaman | 0.90 | 2 | 0/0 | 0/0/0/18 | 0 | 1.20 | 0 | 16/16 | 0/1/15/8 | 16 (7.3s) | 20 → 19 |
| H+Pri vs Shaman+Rogue | 1.10 | 0 | 1/22 | 0/0/22/0 | 1 (8.0s) | 1.80 | 0 | 14/15 +20u | 0/0/35/1 | 13 (8.0s) | 0 → 0 |
| H+Pri vs Paladin+Warrior | 1.00 | 0 | 20/20 | 0/0/20/0 | 0 | 1.00 | 0 | 20/20 | 0/0/20/0 | 0 | 1 → 0 |
| H+Pri+War vs Warlock+Priest+Rogue | 1.00 | 0 | 0/20 | 20/0/0/0 | 0 | 0.00 | 20 | - | - | - | 2 → 4 |
| H+Pri vs Rogue+Warrior | 1.00 | 0 | 0/20 | 0/0/20/0 | 0 | 1.00 | 0 | 0/0 +20u | 0/0/20/0 | 0 | 20 → 20 |
| H+Pri vs Warlock+Rogue | 1.90 | 0 | 18/38 | 20/3/15/0 | 0 | 0.15 | 17 | 3/3 | 0/1/2/0 | 0 | 20 → 19 |
| H+Pri vs Priest+Paladin | 1.00 | 0 | 20/20 | 20/0/0/0 | 20 (0.6s) | 1.10 | 0 | 22/22 | 19/0/3/0 | 22 (1.2s) | 20 → 20 |
| H+Pri vs Mage+Priest | 0.00 | 20 | - | - | - | 0.00 | 20 | - | - | - | 0 → 0 |
| H+Pri vs Warlock+Priest | 0.00 | 20 | - | - | - | 0.00 | 20 | - | - | - | 3 → 3 |
| H+Pri vs Warlock+Paladin | 0.00 | 20 | - | - | - | 0.00 | 20 | - | - | - | 1 → 1 |
| H vs Warrior | 1.00 | 0 | 20/20 | 0/0/20/0 | 0 | 1.00 | 0 | 20/20 | 0/0/20/0 | 0 | 20 → 20 |
| H vs Rogue | 0.10 | 18 | 0/0 | 0/0/0/2 | 0 | 0.10 | 18 | 0/0 | 0/0/0/2 | 0 | 3 → 3 |
| H vs Priest | 1.00 | 0 | 1/1 | 0/0/1/19 | 1 (8.0s) | 1.00 | 0 | 0/0 | 0/0/0/20 | 0 | 17 → 18 |
| H vs Warlock | 1.00 | 0 | 0/20 | 0/0/20/0 | 0 | 1.00 | 0 | 20/20 | 0/0/20/0 | 0 | 16 → 15 |

**The healer trap in the Rogue + Priest comps.** Where the Rogue opens on the
Hunter and the Hunter's partner is not killing the Priest, the trap now goes
on the Priest: `H+Pri vs Rogue+Priest` throws 18 traps, 12 of them springing
on the Priest for 7.4s on average (one broken), and the Hunter turns on the
Rogue; `H+Pal vs Rogue+Priest` 17 on the Priest for the full 8.0s. Most
healer traps run their full 8.0s; the short ones broke on a teammate's cast
begun before the trap sprang (in `H+Pri vs Rogue+Paladin`, the Hunter's
Priest's Mind Blast on the Paladin, 4 of 14). The healer trap was held, in
`H+Pri vs Rogue+Priest`, out of throw range 1,949 ticks and not a clean
landing 328 ticks (trace notes). Two of the three comps the card names throw
no trap, for reasons the rule states:

- **`H+War vs Rogue+Priest`:** the Warrior is killing the Priest (455 held
  ticks, "a teammate is killing the healer") and its bleeds make the Priest
  untrappable (1,029, "cannot be trapped now"). The Hunter still goes 0 → 11:
  the opener now starts Aimed Shot on the Priest while the Rogue is still in
  stealth.
- **`H+Pri+War vs Mage+Priest+Rogue`:** the Rogue opens on the Warrior, then
  the Priest, never the Hunter — the pressure the rule waits for never comes.

**Partner-first slot order is where Rogue + Priest still loses.** With the
Hunter in slot 2 (`Pri+H`, `War+H`, `Pal+H`), and in `H+Rogue`, the Hunter
goes 52 → 4 wins of 80. At `061dfa5` the gates-open lane trap sprang on the
stealthed Rogue and **revealed** it; that throw is held now (its victim would
just be dispelled), and the Rogue opens from stealth. In `Pri+H` the Hunter's
own Priest keeps a DoT on the enemy Priest, so the healer trap cannot be
thrown, and the off-target trap goes on the Rogue instead — worth it only as
a trade against the focused Priest's GCD, and removed 19 times of 20. The
Hunter-first rows with any other partner, which the directional sweep does
not field, go 7 → 50 wins of 100.

**Warlock + Rogue** no longer throws the gates-open trap the Felhunter
devoured every match; the three later throws are decided on the Rogue
running the lane and spring on it. **`Priest+Paladin`**
keeps its off-target trap on one healer freed by the other, by design (19 of
the 64 removals). **`Hunter vs Priest`** throws at 1.0s every match and the
Priest, closing on the Hunter, stops short of the landing; the win rate is
unchanged.

## Win rate (directional)

### 1v1 — 1,150 matches, 15 reachable + 8 control cells, n=50 per cell

```
CONTROL: 400/400 matches with no affected class on either side are IDENTICAL
  ALL reachable  n=750  49.9% -> 47.1%  -2.8pt  flips 44 (+3/-24)  z=3.85 SIG  resolves >=1.5pt
  CLEAN          n=350  40.3% -> 40.0%  -0.3pt  flips 3 (+1/-2)    ns
  AGAINST        n=350  60.3% -> 60.3%  +0.0pt  flips 2 (+1/-1)    ns
  MIRRORED       n=50   44.0% ->  4.0%  -40.0pt flips 39 (+1/-21) z=4.05 SIG
NON-VACUITY: 1105/1150 ended by elimination; 271 moved in winner or duration
```

| enemy (1v1) | n | Hunter before | after | flips |
|---|---|---|---|---|
| Warlock | 100 | 74.0% | 73.0% | +0/-1 |
| Priest | 100 | 94.0% | 94.0% | +2/-2 |
| Paladin | 100 | 0.0% | 0.0% | 0 |
| Warrior | 100 | 100.0% | 100.0% | 0 |
| Rogue | 100 | 12.0% | 12.0% | 0 |
| Mage | 100 | 0.0% | 0.0% | 0 |
| Shaman | 100 | 0.0% | 0.0% | 0 |

Every non-mirror 1v1 cell moves by at most one net flip. The whole reachable delta is
the **Hunter mirror**: both Hunters now open with the same Aimed Shot, and the
mirror goes from 22/19/9 (team 1 / team 2 / draw) to 2/8/40 — a symmetric
race that ends in simultaneous deaths, which count as draws.

### 2v2 — 3,090 matches, 301 reachable + 8 control cells, n=10 per cell

```
CONTROL: 80/80 matches with no affected class on either side are IDENTICAL
  ALL reachable  n=3010  49.3% -> 49.4%  +0.1pt  flips 547 (+270/-268)  z=0.04 ns  resolves >=1.5pt
  CLEAN          n=1260  38.2% -> 38.0%  -0.2pt  flips 213 (+104/-106)  ns        resolves >=2.3pt
  AGAINST        n=1260  60.8% -> 61.1%  +0.3pt  flips 216 (+110/-106)  ns        resolves >=2.4pt
  MIRRORED       n=490   48.4% -> 48.4%  +0.0pt  flips 118 (+56/-56)    ns        resolves >=4.4pt
NON-VACUITY: 3080/3090 ended by elimination; 2146 moved in winner or duration
SLICES TESTED: 3. Significant: none.
```

Per enemy comp, oriented to the Hunter's side (each row is 7 Hunter partners
x 2 sides x 10 seeds). About 60% of a row's side-swapped pairs replay the same
match mirrored, so a row's flips are not all independent and its z
overstates; the slices above carry the significance. The sweep fields each
pairing in one slot order only — the partner first where it sorts before
"Hunter".

| enemy comp | healer | Rogue | n | Hunter before | after | flips +/- | z |
|---|---|---|---|---|---|---|---|
| **Priest+Rogue** | Priest | yes | 140 | 42.1% | **4.3%** | +2/-55 | -7.0 |
| **Rogue+Warlock** | - | yes | 140 | 91.4% | **77.9%** | +10/-29 | -3.0 |
| Priest+Warlock | Priest |  | 140 | 29.3% | 24.3% | +2/-9 | -2.1 |
| **Mage+Warrior** | - |  | 140 | 40.0% | **31.4%** | +14/-26 | -1.9 |
| Warlock+Warrior | - |  | 140 | 83.6% | 79.3% | +3/-9 | -1.7 |
| Mage+Paladin | Paladin |  | 140 | 5.0% | 2.9% | +4/-7 | -0.9 |
| Mage+Shaman | Shaman |  | 140 | 2.9% | 2.9% | 0 | 0 |
| Rogue+Warrior | - | yes | 140 | 100.0% | 100.0% | 0 | 0 |
| Shaman+Warlock | Shaman |  | 140 | 11.4% | 12.9% | +7/-5 | +0.6 |
| Rogue+Shaman | Shaman | yes | 140 | 48.6% | 50.0% | +6/-4 | +0.6 |
| Mage+Warlock | - |  | 140 | 17.9% | 20.0% | +9/-6 | +0.8 |
| Mage+Priest | Priest |  | 140 | 12.1% | 14.3% | +6/-3 | +1.0 |
| Paladin+Warlock | Paladin |  | 140 | 4.3% | 7.9% | +8/-3 | +1.5 |
| Paladin+Warrior | Paladin |  | 140 | 20.7% | 28.6% | +27/-16 | +1.7 |
| Mage+Rogue | - | yes | 140 | 77.9% | 82.9% | +11/-4 | +1.8 |
| **Paladin+Rogue** | Paladin | yes | 140 | 6.4% | **16.4%** | +23/-9 | +2.5 |
| **Shaman+Warrior** | Shaman |  | 140 | 46.4% | **63.6%** | +42/-18 | +3.1 |
| **Priest+Warrior** | Priest |  | 140 | 55.0% | **71.4%** | +34/-11 | +3.4 |
| **all 18** | | | 2520 | 38.6% | 38.4% | +208/-214 | -0.3 |

**The melee + healer comps gain.** `Priest+Warrior` +16pt, `Shaman+Warrior`
+17pt, `Paladin+Rogue` +10pt, `Paladin+Warrior` +8pt. In the mechanism set
the trap springs on the healer 20, 16 and 14 times in 20 matches of the first
three shapes. `H+Pri vs Paladin+Warrior` throws the lane trap on the Warrior
instead, as at `061dfa5`, so the `Paladin+Warrior` gain is not the healer
trap.

**`Priest+Rogue` and `Rogue+Warlock`: the stealth reveal.** Both losses are
the held gates-open trap that used to spring on the stealthed Rogue and reveal
it (see the partner-first rows above). Against a Rogue with no dispeller the
opening throw still goes, and those rows are within noise.

**The no-healer comps.** `Mage+Warrior` -8.6pt, `Warlock+Warrior` -4.3pt,
`Mage+Warlock` +2.1pt, `Mage+Rogue` +5.0pt, `Rogue+Warrior` unchanged,
`Rogue+Warlock` as above. `Mage+Warrior` by Hunter partner (wins of 20):

| partner | before | after |
|---|---|---|
| Priest | 12 | 14 |
| Paladin | 16 | 16 |
| Warlock | 8 | 10 |
| Rogue | 4 | 2 |
| **Mage** | **16** | **2** |
| Warrior, Shaman | 0 | 0 |

The opener recovers the healer partners: Serpent Sting's reservation now keys
on the trap's decided victim, so with no enemy healer the Hunter stings its
kill target, and Aimed Shot going first keeps that sting from taking the GCD
Aimed Shot started on. With a **Mage** partner the Aimed Shot now completes
where the Warrior used to Pummel it, and the Warrior's Pummel lands on the
partner Mage's Frostbolt instead.

**Warlock cells, explicitly.** 1v1 Hunter vs Warlock: 74.0% → 73.0% (one
flip). The six Warlock rows move with the reveal (`Rogue+Warlock`) and within
noise otherwise.

### The movement, attributed

The after binary with `trap_setup: 0.0` (no kite pull, no Disengage bend) is
the attribution arm: same binary, same JSONL, so the difference is the
positioning alone.

```
ALL reachable  n=3010  49.2% -> 49.4%  +0.1pt  flips 83 (+41/-37)  z=0.34 ns  resolves >=0.6pt
```

| enemy comp | setup off | setup on | flips +/- |
|---|---|---|---|
| Shaman+Warrior | 53.6% | 63.6% | +25/-11 |
| Priest+Warrior | 66.4% | 71.4% | +10/-3 |
| Priest+Rogue | 3.6% | 4.3% | +3/-2 |
| **Paladin+Warrior** | 32.9% | **28.6%** | +5/-11 |
| **Paladin+Rogue** | 22.1% | **16.4%** | +2/-10 |
| the other 13 | | | identical |

**Two comps regress from the positioning: the Paladin ones.** Split on the 840
melee + healer cells (Hunter-side wins of 140), the Disengage bend carries
both the Paladin cost and the Shaman gain; the kite pull alone is neutral on
the Paladins:

| enemy comp | setup off | kite pull only | kite + Disengage |
|---|---|---|---|
| Paladin+Rogue | 31 | 32 | 23 |
| Paladin+Warrior | 46 | 50 | 40 |
| Priest+Warrior | 93 | 96 | 100 |
| Shaman+Warrior | 75 | 68 | 89 |
| Priest+Rogue, Rogue+Shaman | 5, 70 | 5, 70 | 6, 70 |

What about a Paladin makes the bent leap cost is not isolated.

## Baselines this invalidates

The Hunter cells of `canonical_1v1_n100_300s.csv` (the Hunter mirror most of
all), `canonical_2v2_full_n100_300s.csv` (every row above that moved), and
`canonical_3v3_full_n50_300s.csv` (3v3 was not swept; the 3v3 comps in the
mechanism set moved). Every cell without a Hunter is unchanged — the controls
are byte-identical in winner and duration.

## Follow-ups

- **The Disengage bend toward a Paladin** costs `Paladin+Warrior` and
  `Paladin+Rogue` (the table above); the kite pull alone does not.
- **An opener counter to stealth, on purpose.** The measured value of the old
  gates-open throw was the reveal (`Priest+Rogue`, `Rogue+Warlock`, and the
  partner-first Rogue + Priest rows). AS-166 (Hunter Flare) is that decision.
- **The Hunter mirror is a draw.** Identical openers race to simultaneous
  deaths in 40 of 50 matches.
- **A cast in flight breaks the trap.** A damaging cast begun on an enemy
  before the trap springs on it — the Hunter's own Aimed Shot, a partner
  Priest's Mind Blast — lands and breaks it. The throw does not ask what is
  already on its way to the victim.
- **AS-129** (healers dispel pets) moves the pet row of
  `who_can_free_a_freezing_trap` and nothing else; the Hunter AI follows it.
