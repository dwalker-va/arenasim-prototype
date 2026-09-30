# Hunter Flare: finding the Rogue on purpose — measurement

**Date:** 2026-09-29
**Card:** AS-166
**Tier:** DIRECTIONAL (`sweep-tiers.md`), 1v1 and 2v2, each 2v2 run with no
kill target and again with both teams' kill target at slot 0 (the graphical
client's default). The cells are AS-125's directional set cut to what Flare
can reach — every cell with a Hunter facing a Rogue — plus every seventh
Hunter cell with no Rogue against it (Flare's "never wasted" control) and the
sweep's own no-Hunter control (`sweeps/2026-09-29-as166-gensweep.py`). Success
condition: the Rogue's stealth opener is denied by a Flare the Hunter placed
without seeing the Rogue, no Flare is cast where nothing is hidden, and with
the Rogue the kill target the Freezing Trap goes on its partner. Win rate is
reported per enemy comp, sized against AS-125's accepted kill-target losses,
not tuned.
**Arms:** before = `main` at `020abda`; after = this branch on it. Same JSONL
both arms. One attribution arm, the after binary with Flare costing no mana
(`sweeps/2026-09-29-as166-arm-free-flare.patch`, a config-only change).
**Raw rows:** `2026-09-29-as166_{1v1,2v2,2v2_kt0}_{base_020abda,after}.csv`
(one row per match, from `sweeps/2026-09-29-as166-{1v1,directional,directional-kt0}.jsonl`);
`2026-09-29-as166_2v2{,_kt0}_arm_free_flare.csv` (the Rogue cells only);
`2026-09-29-as166_mech{,_sweepmix}_{base_020abda,after}.csv` (one row per
match: Flares, reveals, the Rogue's opener, trap springs — `sweeps/2026-09-29-as166-flaremech.py`,
summarised by `…-flaresumm.py`).

## What the Hunter does now

**Flare** (Classic spell 1543, from the Wowhead data): instant, 50 mana, 30yd,
15s cooldown. It burns on the ground for 30s, and every stealthed enemy inside
its 10yd radius is exposed through the one reveal funnel
(`combat_core::reveal_stealthed`, logged `[STEALTH] X is revealed by Flare`).
An area does not aim, so the light finds a Rogue the Hunter cannot see.

**No roll.** Per the user's ruling, the uncertainty is geometric: inside the
light the reveal is certain, outside it nothing happens. Whether a Flare finds
the Rogue is decided by where the Hunter put it and where the Rogue walked
(`a_flare_finds_exactly_what_stands_in_its_light` pins the edge).

**The guess** (`hunter::flare_plan`). The Hunter knows the enemy team has an
enemy it cannot see, where the enemy gate is, where its own team is, and how
long the gates have been open. From that:

- **Where:** the Rogue comes from its gate and reaches first the ally nearest
  that gate, so the light is centred 5yd (`FLARE_LEAD`) ahead of that ally,
  toward the gate — the ally well inside it, the approach lit. The centre must
  be within Flare's range of the Hunter; the Hunter itself always qualifies.
- **When:** not before a Rogue that left its gate when the gates opened and
  ran straight at base speed could reach the light's edge. Earlier, the ally
  is still moving and the light would be left behind. Within one GCD of that
  moment the Hunter begins nothing else, so the GCD is free when the light is
  due (without that, a shot begun just before put the light down after the
  Rogue had opened — measured on the first build).
- It is not cast while nothing is hidden: `flare_plan` answers `None` then,
  and every decision it would change is gated on it.

**The trap.** While an enemy is hidden, the opening lane trap is **held when
that enemy is the team's configured kill target**
(`CombatContext::kill_target_hidden`, traced `trap held: the unseen enemy is
the kill target, Flare finds it`): the team converges on it the moment it is
seen and breaks the trap, so the trap waits for everyone to be in view and goes
on the Rogue's partner through AS-125's off-target and pressure traps. With no
kill target on the Rogue the lane throw keeps AS-125's rule — held when a
visible enemy could free it, thrown otherwise — because there the trap is
worth its CC as well as its reveal (measured below).

**The visual** is a placeholder: a lit disc exactly the size of the radius
and a flare hanging over it (`rendering/effects/flare.rs`). The disc is the
gameplay radius so a Rogue walking past its edge unrevealed reads as a missed
guess. The bespoke Classic look is a follow-up.

## The mechanism: Flares, reveals, openers

Two sets, traced from the logs: the **mechanism set** — Hunter + each of 7
partners vs Rogue + each of 6 partners, Hunter and Rogue in slot 0, seeds 0-9,
both kill-target settings, plus 1v1 in both slot orders, 880 matches — and a
**sweep-mix sample**, seeds 0-4 of the directional sweep's Rogue cells (the
Hunter mostly in slot 1, both slot orders of the Rogue pair), 840 matches.

| | mechanism set | sweep-mix sample |
|---|---|---|
| Rogue stealth openers landed, before → after | **460 → 0** | **430 → 0** |
| Flares cast | 700 | 610 |
| Flares that revealed the Rogue | **700 (100%)** | **610 (100%)** |
| matches with a Rogue and no Flare | 180 (the lane trap revealed it first) | 230 (the same) |

**Every Flare cast found the Rogue, and no Rogue opened from stealth.** The
reveal rate is the direct read of the guess, and in this game the guess is
never wrong: a Rogue walks straight from its gate to the unit it has picked,
and that unit is on the Hunter's side of the arena with the light in front of
it. The model allows a miss — a Rogue that routes around the light is not
found — but no Rogue AI routes around anything yet (follow-ups).

Timing, from the mechanism set: the first Flare lands a median 4.98s after the
gates (4.52-8.02s), and the reveal comes a median **1.77s** before the opener
would have landed at `020abda` (0.41-1.93s).

Per enemy comp, mechanism set (Hunter wins of the n; openers and trap springs
before → after):

| enemy | kill targets | n | openers | Flares/m | reveal rate | traps on the Rogue | traps on its partner | Hunter wins |
|---|---|---|---|---|---|---|---|---|
| Priest+Rogue | at 0 | 70 | 70 → 0 | 1.00 | 70/70 | 0 → 0 | 12 → 22 | 30 → 65 |
| Paladin+Rogue | at 0 | 70 | 70 → 0 | 1.00 | 70/70 | 4 → 2 | 33 → 39 | 49 → 41 |
| Rogue+Warlock | at 0 | 70 | 70 → 0 | 1.00 | 70/70 | 0 → 0 | 0 → 0 | 48 → 65 |
| Rogue+Shaman | at 0 | 70 | 0 → 0 | 1.00 | 70/70 | **70 → 0** | **2 → 20** | 42 → 63 |
| Mage+Rogue | at 0 | 70 | 0 → 0 | 1.00 | 70/70 | **70 → 0** | 0 → 0 | 26 → 45 |
| Rogue+Warrior | at 0 | 70 | 0 → 0 | 1.00 | 70/70 | 0 → 0 | 70 → 0 | 59 → 70 |
| Priest+Rogue | none | 70 | 70 → 0 | 1.00 | 70/70 | 2 → 4 | 19 → 27 | 30 → 24 |
| Paladin+Rogue | none | 70 | 70 → 0 | 1.00 | 70/70 | 10 → 16 | 37 → 43 | 36 → 21 |
| Rogue+Warlock | none | 70 | 70 → 0 | 1.00 | 70/70 | 2 → 4 | 0 → 3 | 58 → 61 |
| Rogue+Shaman | none | 70 | 0 → 0 | 0.00 | - | 75 → 75 | 14 → 14 | 44 → 44 |
| Mage+Rogue | none | 70 | 0 → 0 | 0.14 | 10/10 | 73 → 73 | 0 → 0 | 60 → 60 |
| Rogue+Warrior | none | 70 | 0 → 0 | 0.29 | 20/20 | 50 → 50 | 20 → 20 | 69 → 69 |
| Rogue (1v1) | - | 40 | 40 → 0 | 1.00 | 40/40 | 0 → 40 | - | 14 → 40 |

**The trap goes on the partner.** With the Rogue the kill target and no
dispeller behind it (Rogue+Shaman, Mage+Rogue), the opening lane trap caught
the stealthed Rogue every match at `020abda` and the team broke it as it
converged; now none springs on the Rogue, and against Rogue+Shaman 20 spring on
the Shaman (`flare_reveals_the_kill_target_rogue_and_the_trap_takes_its_partner`
pins three seeds of the user's Warrior+Hunter vs Rogue+Shaman case). Against
Rogue+Warrior the base lane trap sprang on the Warrior running the lane
(70 → 0): held now for the unseen kill target, and the Hunter still wins more.
With no kill target nothing about the trap changes where the Rogue has no
dispeller, and there the lane trap usually reveals the Rogue before a Flare
comes due (0.00-0.29 Flares a match).

## Win rate (directional)

Hunter-side win rate per enemy comp, paired at identical seeds, from the
directional sweep (each Rogue row: 7 Hunter partners × 2 sides × 10 seeds;
side-swapped pairs replay the same match mirrored, so a row's z overstates —
the slices carry the significance). Next to it, AS-125's before → after at
`c79d8cc`, the losses this card is sized against.

| enemy comp | no kill target: before | after | flips | kill targets at 0: before | after | flips | AS-125 at 0 |
|---|---|---|---|---|---|---|---|
| **Priest+Rogue** | 30.0% | 35.7% | +44/-36 | 70.0% | **92.1%** | +42/-11 | 100 → 70.0 |
| **Rogue+Warlock** | 68.6% | **95.7%** | +41/-3 | 67.1% | **91.4%** | +42/-8 | 95.7 → 66.4 |
| **Paladin+Rogue** | 26.4% | **12.9%** | +17/-36 | 20.7% | 26.4% | +16/-8 | 55.0 → 20.7 |
| Rogue+Shaman | 50.7% | 48.6% | +12/-15 | 100.0% | 98.6% | +0/-2 | 98.6 → 100 |
| Mage+Rogue | 75.0% | 75.0% | 0 | 80.7% | 79.3% | +0/-2 | 78.6 → 80.7 |
| **Rogue+Warrior** | 100.0% | **86.4%** | +0/-19 | 97.1% | 97.1% | +4/-4 | 97.1 → 97.1 |
| all Hunter cells (non-mirror) | 51.1% | 51.6% | +114/-109 | 66.5% | **72.9%** | +104/-35 | |

```
no kill target
CONTROL: 80/80 matches with no affected class on either side are IDENTICAL
  ALL reachable  n=1270  50.6% -> 50.9%  +0.3pt  flips 227 (+115/-111)  z=0.20 ns  resolves >=2.4pt
  CLEAN          n=540   51.3% -> 52.2%  +0.9pt  flips 112 (+58/-53)    z=0.38 ns
  AGAINST        n=540   49.1% -> 48.7%  -0.4pt  flips 114 (+56/-58)    z=0.09 ns
NON-VACUITY: 1346/1350 ended by elimination; 676 moved in winner or duration

kill targets at slot 0
CONTROL: 80/80 matches with no affected class on either side are IDENTICAL
  ALL reachable  n=1270  51.3% -> 49.5%  -1.7pt  flips 159 (+66/-88)    z=1.69 ns  resolves >=2.0pt
  CLEAN          n=540   69.3% -> 74.1%  +4.8pt  flips 73 (+47/-21)     z=3.03 SIG
  AGAINST        n=540   35.9% -> 28.0%  -8.0pt  flips 75 (+16/-59)     z=4.85 SIG
NON-VACUITY: 1331/1350 ended by elimination; 770 moved in winner or duration
```

(`paired_sweep.py` reports team 1's win rate; CLEAN is the Hunter on team 1
and AGAINST the Hunter on team 2, so both slices say the same thing: the Hunter
side gains, +4.8 and +8.0pt.)

**With kill targets at 0 — the client's default — Flare wins back most of what
AS-125's held trap cost:** Priest+Rogue 70.0% → 92.1% and Rogue+Warlock 67.1% →
91.4%, against AS-125's 100 → 70.0 and 95.7 → 66.4. **Paladin+Rogue does not
come back** (20.7% → 26.4%, against AS-125's 55.0 → 20.7): the reveal is not
what that comp lost.

**With no kill target the Hunter side is flat overall, and two Rogue rows
fall:**

- **Rogue+Warrior 100% → 86.4%**, eighteen of the nineteen flips in
  Warrior+Hunter against it, both sides (Hunter+Shaman the other). Seed 0 of
  Warrior+Hunter vs Warrior+Rogue, read from both logs: at `020abda` the
  Hunter's Frost Trap revealed the Rogue at 16.47s; now the Flare reveals it
  at 14.97s, the Rogue's next act is a Kidney Shot on the Hunter's Warrior
  (6s, 15.63s) while the enemy Warrior works on it, and the Frost Trap goes out
  1.5s later, behind the Flare's GCD. One traced seed, so this is the shape of
  the loss, not its attribution; the mana arm below rules the cost out.
- **Paladin+Rogue 26.4% → 12.9%.** Not attributed. In the one seed read (the mechanism
  set's Hunter+Mage vs Rogue+Paladin, seed 1, kill targets at 0) the Rogue is revealed at 15.00s, the
  Spider's Web lands on it at 15.23s and the Paladin cleanses it at once; the
  Rogue dies at 34.85s against 24.03s at `020abda`. The mana arm moves this row
  +6.4pt with no kill target, so the cost is part of it and not all.

Every other Hunter cell is untouched: the no-Rogue sample (300 matches per
kill-target setting) and the controls are byte-identical in winner and
duration, 380/380 each — nothing is hidden there, so no Flare is ever cast.

### 1v1 — 750 matches, n=50 per cell

```
CONTROL: 400/400 matches with no affected class on either side are IDENTICAL
  CLEAN    n=150  78.0% -> 100.0%  +22.0pt  flips 33 (+33/-0)  z=5.57 SIG
  AGAINST  n=150  22.0% ->   0.0%  -22.0pt  flips 33 (+0/-33)  z=5.57 SIG
```

**Hunter vs Rogue goes 34% → 100%** (17 → 50 of 50 in each slot order). The
Flare finds the Rogue on its way in every match, and a Rogue without its
opener cannot beat a Hunter that sees it coming. Hunter vs Priest, Warrior and
the mirror are identical, seed for seed.

### What the mana is worth

Flare's 50 mana is a fifth of a Hunter's pool, and the Hunter regenerates none.
The attribution arm is the after binary with Flare free, on the Rogue cells:

| | no kill target | kill targets at 0 |
|---|---|---|
| Hunter side, Flare at 50 mana → free | 59.0% → 63.0% (+33/-0) | 80.8% → 81.8% (+11/-3) |
| Priest+Rogue | 35.7% → 50.0% (+20/-0) | 92.1% → 95.7% (+5/-0) |
| Paladin+Rogue | 12.9% → 19.3% (+9/-0) | 26.4% → 26.4% |
| Rogue+Warrior | 86.4% → 86.4% | 97.1% → 98.6% (+2/-0) |

The cost is real — four points with no kill target, most of it against
Priest+Rogue — but it is not what sinks Rogue+Warrior or Paladin+Rogue. Flare
keeps its Classic cost here; the milestone sweep is where to weigh it.

## Tests this changed

- `stealth_enforcement.rs`: Flare joins the enemy-area set, driven through the
  real `flare_system`; `a_flare_finds_exactly_what_stands_in_its_light` pins
  the radius edge (in at 9.9yd, out at 10.1yd, planar).
- `class_ai_decisions.rs`: `flare_plan`'s where and when, the GCD reservation,
  cooldown and mana; `kill_target_hidden` by slot, never a pet, alive and out
  of view only.
- `hunter_trap_trace.rs`: four seed-pinned probes re-pinned to seeds that still
  reach their scenario now that the Rogue is revealed before it opens. One
  changed its non-vacuity claim: with the Hunter's team told to kill the enemy
  Priest, the healer trap still turns the Hunter onto the Rogue, but the Rogue
  now reaches it unstunned and pins it inside its dead zone for the freeze in
  all but one of 320 seeds scanned, so the probe asserts the Hunter DECIDES
  on the Rogue while the Priest is frozen rather than that a shot lands.
- The encyclopedia's two ability-index snapshots re-blessed: "70 of 70
  abilities" → "71 of 71", nothing else.

## Follow-ups

- **A Rogue that plays around the light.** Every Flare in 1,310 found its
  Rogue, because no Rogue AI routes around anything: the model's miss never
  happens, and Hunter vs Rogue 1v1 is now 100%. A Rogue that sees a burning
  Flare and walks around it (or waits it out — 30s) is what makes the
  Hunter's placement a guess in practice.
- **The bespoke Flare visual.** Classic's flare missile and its lingering
  light, researched from the client data first (the placeholder is a disc and
  a mote).
- **Paladin+Rogue.** AS-125's kill-target loss there (55.0 → 20.7) is not the
  reveal; with Flare it is 26.4%, and 12.9% with no kill target. What the
  Paladin's cleanses do to the Hunter's control against a visible Rogue is its
  own question.
- **The pinned Hunter.** With Flare the Rogue arrives unstunned; under a
  pressure trap on a kill-target healer the Hunter now almost never gets a shot
  off during the freeze (AS-179's pinned-Hunter case, now the common one).
