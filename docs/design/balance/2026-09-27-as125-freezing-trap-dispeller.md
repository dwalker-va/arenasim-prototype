# Freezing Trap: throw it at whoever it would actually catch — measurement

**Date:** 2026-09-27
**Card:** AS-125
**Tier:** DIRECTIONAL (`sweep-tiers.md`), 1v1 and 2v2. Success condition: no
opening trap is thrown where its victim is just dispelled; a lane trap springs
on the enemy it was decided on; the trap is used wherever the enemy it would
catch cannot be freed (a trapped Felhunter included); the Warlock cells and the
Rogue + dispeller cells are reported whichever way they move. Win rate was
expected to be neutral at best (`hunter-trap-targeting`); it is reported, not
tuned.
**Arms:** before = `061dfa5` (main, binary sha1 `4c3e5cbb`); after = this
branch at `15f636e` (binary sha1 `153cffa2`). Same JSONL both arms.
**Raw rows:** `2026-09-27-as125_{1v1,2v2}_base_061dfa5.csv` and
`..._after.csv` (one row per match, from
`sweeps/2026-09-27-as125-hunter-{1v1,directional}.jsonl`);
`2026-09-27-as125_trap_events.csv` (one row per Freezing Trap thrown) and
`2026-09-27-as125_trap_matches.csv` (one row per match), both arms, from the
17-comp mechanism set.

## What the Hunter does now

Freezing Trap asks two questions before it is thrown: **who will spring it,
and would that enemy just be freed?**

Who would free a victim comes from the engine's own removal rules
(`ally_removal` / `can_free_ally` / `ally_freers`, asked of the aura the trap
actually lands, `traps::freezing_trap_aura`), never from a class list. Today the
Priest, the Paladin and the Felhunter free a trapped teammate; only the
Felhunter frees a trapped pet, and nobody frees a trapped Felhunter. The
healers' dispel scan and Devour Magic read their scope and pet reach from the
same `ally_removal`, so the cast and the question cannot drift.

A victim is **worth a trap** (`trap_victim_worth_it`) when:

- it is a **healer** — the dispeller the trap is for; a second dispeller behind
  it (a Felhunter, a second healer) is left to spend itself undoing it; or
- **no living teammate of its could free it** — a Felhunter always qualifies;
  a non-healer in a comp with no dispeller always qualifies; or
- **every teammate that could free it is one the Hunter's team is killing.**
  When the enemy healer is the kill target, trapping its partner forces the
  focused healer to spend a GCD on the dispel instead of on itself.

Where the trap goes:

- **Off-target (aimed):** the best eligible enemy the team is not killing —
  healer first — thrown at its position (led if moving), and only when no other
  enemy is near the landing.
- **Fallback (the lane throw, when there is no off-target):** the trap goes to
  the lane midpoint toward the enemy healer (or the kill target), and is
  decided on the enemy that would **spring** it — the first inside the trigger
  radius once the trap has armed (`predicted_trap_springer`). Each visible
  enemy, pets included, is extrapolated along its heading, stopping at its
  preferred range of its own target, as pursuit does. The throw is held when:
  - nobody is predicted to reach the landing within 3s of it arming
    (`TRAP_SPRING_WINDOW`): the trap would catch whoever wandered in later;
  - a teammate of the springer would free it;
  - the Hunter's own team is attacking the springer — a teammate's target, or
    the kill target once the Hunter has a teammate — because the trap breaks
    on any damage (`teammate_would_break_it`). Alone, the Hunter and its pet
    hold fire on a trapped target, so the 1v1 peel stands.

  The trace records the springer as the throw's aim.
- **An enemy in stealth.** While an enemy is hidden, the lane's springer is
  that unseen enemy: at `061dfa5` it sprang all 140 opening lane traps thrown
  with a Rogue in stealth, across seven comps. The throw is held when a visible
  enemy would free it (`unseen_victim_would_be_freed`), and otherwise goes with
  no aim recorded, since the Hunter cannot name an enemy it cannot see.
- **The guards ask about the springer.** The friendly-DoT guard (a friendly
  DoT's first tick breaks the trap) and Serpent Sting's reservation (don't
  sting the enemy the trap is about to catch) both check the decided victim,
  not the unit the lane points at.
- **Range:** every trap placement refuses a landing beyond the ability's
  configured `range` (30yd, from `abilities.ron`), measured on the clamped
  landing. Refused rather than pulled in, because a landing is chosen for where
  it lands; the Hunter asks again next tick, closer. Ranged placement itself is
  intended (the Trap Launcher model, not Classic's feet-drop). The opening lane
  throw lands about one second after the gates, once the lane is within 30yd.

## The mechanism: where the traps went

17 trap-relevant comps (2v2, 3v3, 1v1), seeds 0-19, traced (340 matches per
arm). Times are seconds after the gates.

| | before | after |
|---|---|---|
| traps thrown | 265 | 161 |
| sprung | 243 | 139 |
| **sprung on the enemy the throw was decided on** | **82 of 243** | **95 of 99 named, + 40 of 40 unseen** |
| **removed by an enemy dispel** | **120** | **17** |
| broken by damage | 3 | 2 |
| ran the full duration | 120 | 120 |
| thrown in the first 6s into a comp that freed the victim | 100 | **0** |
| matches with no Freezing Trap at all | 95 / 340 | 193 / 340 |

At `061dfa5` the "decided on" column is the enemy the placement was aimed at
(for the lane throw, the lane's candidate), and 161 of the 243 sprung throws
caught someone else. After, "named" throws carry the predicted springer, and "unseen"
throws were decided on a stealthed enemy. The four named misses: three
off-target throws in `Priest+Paladin` aimed at the Paladin that sprang on the
Priest, and one `Shaman+Rogue` lane throw decided on the Rogue that sprang on
the Shaman.

The 17 removals are all `Priest+Paladin`: the off-target trap on one healer,
freed by the other — the healer rule, by design. The two breaks: one
`Priest+Paladin` trap, and one `Warlock+Rogue` lane trap on the Rogue (the
Hunter was alone by then) broken by the Hunter's own Aimed Shot, begun on the
Rogue before the trap sprang on it.

Per comp (traps per match, zero-trap matches out of 20, median first throw,
decided-victim vs springer, fate as removed/broken/full/unsprung; Hunter-side
wins out of 20). `+Nu` counts sprung throws decided on an unseen enemy:

| comp | before: traps/m | zero | first | decided = springer | fate | after: traps/m | zero | first | decided = springer | fate | Hunter wins |
|---|---|---|---|---|---|---|---|---|---|---|---|
| H+Pri vs Rogue+Priest | 1.00 | 0 | 0.0s | 0/20 | 20/0/0/0 | 0.00 | 20 | - | - | - | 1 → 6 |
| H+War vs Rogue+Priest | 1.00 | 0 | 0.0s | 0/20 | 20/0/0/0 | 0.00 | 20 | - | - | - | 0 → 1 |
| H+Pri+War vs Mage+Priest+Rogue | 1.00 | 0 | 0.0s | 0/20 | 20/0/0/0 | 0.00 | 20 | - | - | - | 0 → 0 |
| H+Pri+War vs Warlock+Priest+Rogue | 1.00 | 0 | 0.0s | 0/20 | 20/0/0/0 | 0.00 | 20 | - | - | - | 2 → 4 |
| H+Pri vs Warlock+Rogue | 1.90 | 0 | 0.0s | 18/38 | 20/3/15/0 | 0.15 | 17 | 18.6s | 3/3 | 0/1/2/0 | 20 → 19 |
| H+Pri vs Rogue+Warrior | 1.00 | 0 | 0.0s | 0/20 | 0/0/20/0 | 1.00 | 0 | 1.0s | +20u | 0/0/20/0 | 20 → 20 |
| H+Pri vs Shaman+Rogue | 1.10 | 0 | 0.0s | 1/22 | 0/0/22/0 | 1.15 | 0 | 1.0s | 2/3 +20u | 0/0/23/0 | 0 → 0 |
| H+Pri vs Paladin+Warrior | 1.00 | 0 | 6.5s | 20/20 | 0/0/20/0 | 1.00 | 0 | 6.5s | 20/20 | 0/0/20/0 | 1 → 1 |
| H+Pri vs Priest+Paladin | 1.00 | 0 | 17.4s | 20/20 | 20/0/0/0 | 1.55 | 0 | 12.5s | 27/30 | 17/1/12/1 | 20 → 20 |
| H+Pri vs Warrior+Priest | 0.15 | 17 | 44.9s | 2/2 | 0/0/2/1 | 0.10 | 18 | 27.2s | 2/2 | 0/0/2/0 | 19 → 19 |
| H+Pri vs Warlock+Priest | 0.00 | 20 | - | - | - | 0.00 | 20 | - | - | - | 3 → 3 |
| H+Pri vs Warlock+Paladin | 0.00 | 20 | - | - | - | 0.00 | 20 | - | - | - | 1 → 1 |
| H+Pri vs Mage+Priest | 0.00 | 20 | - | - | - | 0.00 | 20 | - | - | - | 0 → 0 |
| H vs Warrior | 1.00 | 0 | 0.0s | 20/20 | 0/0/20/0 | 1.00 | 0 | 1.0s | 20/20 | 0/0/20/0 | 20 → 20 |
| H vs Warlock | 1.00 | 0 | 0.0s | 0/20 | 0/0/20/0 | 1.00 | 0 | 1.0s | 20/20 | 0/0/20/0 | 16 → 15 |
| H vs Priest | 1.00 | 0 | 0.0s | 1/1 | 0/0/1/19 | 1.00 | 0 | 1.0s | 1/1 | 0/0/1/19 | 17 → 17 |
| H vs Rogue | 0.10 | 18 | 30.7s | - | 0/0/0/2 | 0.10 | 18 | 30.7s | - | 0/0/0/2 | 3 → 3 |

**Warlock + Rogue.** At `061dfa5` the gates-open lane trap sprang on the
stealthed Rogue and the Felhunter devoured it every match (20 removals). A
second throw came as the Warlock died, and 3 of those 18 broke on damage. Now the opening throw is held (the Felhunter would free the
unseen Rogue), and the three later throws come once the Hunter is alone: each
is decided on the Rogue running the lane at it, and each springs on the Rogue.

**`Priest+Paladin` throws more** (1.55 per match, a second trap in 11 of 20
matches). Serpent Sting's reservation now checks the off-target Paladin the
trap is aimed at rather than the enemy healer the lane points at, so the
Hunter stings its kill target (20 of 20 matches, against 5 of 20 at
`061dfa5`), and the match runs differently from there. 12 of the traps run
their full duration.

**`Hunter vs Priest`** throws at 1.0s every match and 19 of 20 never spring:
the throw is decided on the Priest closing on the Hunter, and it stops short
of the landing. The trap costs a GCD; the win rate is unchanged, and was the
same at `061dfa5`.

**The Rogue + dispeller comps throw no trap, at either end of the match.**
`Rogue+Priest` (both partners), `Mage+Priest+Rogue` and `Warlock+Priest+Rogue`
throw none in 20 matches each. The opening throw is held in every match
(trace note: "an unseen enemy could spring it and be freed"). After the
opener, Freezing Trap is only considered while the nearest enemy is 20yd or
more away, and the Rogue rarely leaves that band:

- `H+Pri vs Rogue+Priest`: the trap comes up in 1 of 20 matches (seed 12,
  168 ticks), held because the enemy it would catch is one the team is
  attacking.
- `H+War vs Rogue+Priest`: it never comes up after the opener.
- `H+Pri+War vs Mage+Priest+Rogue`: it comes up in 2 of 20 (seeds 2 and 16,
  195 ticks each), held because no enemy is heading into the lane.
- `H+Pri+War vs Warlock+Priest+Rogue`: it comes up in every match, held
  because no enemy is heading into the lane (16 matches) or the enemy that
  would spring it has a teammate to free it (14).

## Win rate (directional)

### 1v1 — 1,150 matches, 15 reachable + 8 control cells, n=50 per cell

```
CONTROL: 400/400 matches with no affected class on either side are IDENTICAL
  ALL reachable  n=750   49.9% -> 49.7%  -0.1pt  flips 1 (+0/-1)  ns  resolves >=0.4pt
NON-VACUITY: 1136/1150 ended by elimination; 5 moved in winner or duration
```

| enemy (1v1) | n | Hunter before | after | flips |
|---|---|---|---|---|
| Warlock | 100 | 74.0% | 73.0% | +0/-1 |
| Priest | 100 | 94.0% | 94.0% | 0 |
| Paladin | 100 | 0.0% | 0.0% | 0 |
| Warrior | 100 | 100.0% | 100.0% | 0 |
| Rogue | 100 | 12.0% | 12.0% | 0 |
| Mage | 100 | 0.0% | 0.0% | 0 |
| Shaman | 100 | 0.0% | 0.0% | 0 |

1v1 is unchanged. Against the Warlock the lane trap springs on the Felhunter,
as it did at `061dfa5`; the throw is now decided on it.

### 2v2 — 3,090 matches, 301 reachable + 8 control cells, n=10 per cell

```
CONTROL: 80/80 matches with no affected class on either side are IDENTICAL
  ALL reachable  n=3010  49.3% -> 49.5%  +0.2pt  flips 353 (+179/-172)  z=0.32 ns   resolves >=1.3pt
  CLEAN          n=1260  38.2% -> 35.3%  -2.9pt  flips 142 (+52/-88)    z=2.96 SIG resolves >=1.9pt
  AGAINST        n=1260  60.8% -> 64.0%  +3.2pt  flips 148 (+94/-54)    z=3.21 SIG resolves >=2.0pt
  MIRRORED       n=490   48.4% -> 49.0%  +0.6pt  flips 63 (+33/-30)     z=0.25 ns  resolves >=3.4pt
NON-VACUITY: 3082/3090 ended by elimination; 1471 moved in winner or duration
SLICES TESTED: 3. Significant: 2 of 3 (CLEAN, AGAINST).
```

Per enemy comp, oriented to the Hunter's side (Hunter on exactly one side;
each row is 7 Hunter partners x 2 sides x 10 seeds). A row is a direction at
this n, not a value; the flips carry the significance.

| enemy comp | dispeller | Rogue | n | Hunter before | after | flips +/- | z |
|---|---|---|---|---|---|---|---|
| **Priest+Rogue** | Priest | yes | 140 | 42.1% | **6.4%** | +5/-55 | -6.5 |
| **Mage+Warrior** | - |  | 140 | 40.0% | **22.9%** | +2/-26 | -4.5 |
| **Rogue+Warlock** | Warlock | yes | 140 | 91.4% | **77.9%** | +10/-29 | -3.0 |
| Priest+Warlock | Priest+Warlock |  | 140 | 29.3% | 24.3% | +2/-9 | -2.1 |
| Warlock+Warrior | Warlock |  | 140 | 83.6% | 79.3% | +3/-9 | -1.7 |
| Mage+Priest | Priest |  | 140 | 12.1% | 13.6% | +8/-6 | +0.5 |
| Shaman+Warlock | Warlock |  | 140 | 11.4% | 12.9% | +7/-5 | +0.6 |
| Priest+Warrior | Priest |  | 140 | 55.0% | 57.1% | +10/-7 | +0.7 |
| Mage+Warlock | Warlock |  | 140 | 17.9% | 20.0% | +9/-6 | +0.8 |
| Mage+Paladin | Paladin |  | 140 | 5.0% | 7.1% | +7/-4 | +0.9 |
| Paladin+Warlock | Paladin+Warlock |  | 140 | 4.3% | 7.9% | +8/-3 | +1.5 |
| Paladin+Rogue | Paladin | yes | 140 | 6.4% | 12.1% | +17/-9 | +1.6 |
| Mage+Rogue | - | yes | 140 | 77.9% | 80.0% | +6/-3 | +1.0 |
| Rogue+Shaman | - | yes | 140 | 48.6% | 48.6% | +4/-4 | 0 |
| Shaman+Warrior | - |  | 140 | 46.4% | 46.4% | +6/-6 | 0 |
| Paladin+Warrior | Paladin |  | 140 | 20.7% | 20.7% | 0 | 0 |
| Rogue+Warrior | - | yes | 140 | 100.0% | 100.0% | 0 | 0 |
| Mage+Shaman | - |  | 140 | 2.9% | 2.9% | 0 | 0 |
| **all 18** | | | 2520 | 38.6% | 35.6% | +104/-181 | -4.6 |

**The Hunter is worse by 3pt, and the loss is three rows.** Every other row is
within noise.

**`Priest+Rogue` and `Rogue+Warlock`: the stealth reveal.** At `061dfa5` the
gates-open lane trap springs on the stealthed Rogue in every one of these
matches, **revealing it**; the Priest or Felhunter frees it a fraction of a
second later, but the Rogue has lost its opener. With the trap held, the Rogue
opens from stealth. The trap was a spent GCD as crowd control and a strong
trade as a stealth reveal. Holding it is what the ruling asks for (no
gates-open throw whose victim would just be dispelled), and the price is these
two rows. Against a Rogue with no dispeller the opening throw still goes: in
the mechanism set all 40 against `Rogue+Warrior` and `Shaman+Rogue` spring on
the Rogue, and the three no-dispeller Rogue rows are within noise.

**`Mage+Warrior`: Serpent Sting.** 26 of the 28 flips are Hunter+Priest and
Hunter+Paladin losing. At `061dfa5` Serpent Sting's reservation held the sting
off the kill target whenever the trap was ready, because with no enemy healer
the kill target *was* the lane's candidate — whether or not any trap was
headed for it. In these two cells the Hunter never stung at all (0 of 20
matches each). The reservation now asks whether the kill target is the trap's
decided victim, which it is not, so the Hunter stings it (20 of 20, the GCD
after its opening Concussive Shot), and in the matches traced the Aimed Shot
that GCD used to start comes one GCD later. No Freezing Trap is involved on either arm. This is
the price of keying the reservation on the right unit; whether the sting
belongs in that opening is a separate question.

### Warlock cells, explicitly

- **1v1 Hunter vs Warlock: 74.0% → 73.0%** (n=100, 1 flip). The trap is thrown
  in every match and springs on the Felhunter, which nobody frees.
- **Felhunter comps did not get easier for the Warlock side except through the
  reveal.** Across the six Warlock rows (n=840) the Hunter side goes 39.6% →
  37.1%, most of it the `Rogue+Warlock` reveal. The other five go 29.3% →
  28.9%.
- The Felhunter's devours of a trapped teammate go **20 → 0** in the mechanism
  set (`Warlock+Rogue`): the Hunter no longer throws where the Felhunter would
  free the victim.

## Baselines this invalidates

The Hunter cells of `canonical_1v1_n100_300s.csv` (1v1 is measured unchanged
here, but at n=50 per cell and the range limit touches every Hunter match),
`canonical_2v2_full_n100_300s.csv` (the Rogue + dispeller and `Mage+Warrior`
cells above most of all), and `canonical_3v3_full_n50_300s.csv` (3v3 was not
swept; the two 3v3 comps in the mechanism set moved). Every cell without a
Hunter is unchanged — the controls are byte-identical in winner and duration.

## Follow-ups

- **Serpent Sting in the opening.** Against `Mage+Warrior` the sting's GCD
  after the opening Concussive Shot costs Hunter+Priest and Hunter+Paladin 13
  matches in 20 each. The sting's place in the rotation is its own question.
- **An opener counter to stealth, on purpose.** The measured value of the old
  opening throw was the reveal (~36pt in `Priest+Rogue`, ~13pt in
  `Rogue+Warlock`). AS-166 (Hunter Flare) is that decision.
- **A Hunter's own cast in flight breaks its trap.** An Aimed Shot begun on an
  enemy before the lane trap springs on it lands and breaks the trap (one case
  in the mechanism set).
- **AS-129** (healers dispel pets) moves the pet row of
  `who_can_free_a_freezing_trap` and nothing else; the Hunter AI follows it.
