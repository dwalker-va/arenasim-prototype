# Freezing Trap: throw it at whoever it would actually catch — measurement

**Date:** 2026-09-27
**Card:** AS-125
**Tier:** DIRECTIONAL (`sweep-tiers.md`), 1v1 and 2v2. Success condition: no
opening trap is thrown where its victim is just dispelled; the trap is used
wherever the enemy it would catch cannot be freed (a trapped Felhunter
included); no-dispeller comps move only by the range limit; the Warlock cells
and the Rogue + dispeller cells are reported whichever way they move. Win rate
was expected to be neutral at best (`hunter-trap-targeting`); it is reported,
not tuned.
**Arms:** before = `061dfa5` (main, binary sha1 `4c3e5cbb`); after = this
branch at `40fb7d2` (binary sha1 `31185cb5`). Same JSONL both arms.
**Raw rows:** `2026-09-27-as125_{1v1,2v2}_base_061dfa5.csv` and
`..._after.csv` (one row per match, from
`sweeps/2026-09-27-as125-hunter-{1v1,directional}.jsonl`);
`2026-09-27-as125_trap_events.csv` (one row per Freezing Trap thrown) and
`2026-09-27-as125_trap_matches.csv` (one row per match), both arms, from the
17-comp mechanism set.

## What the Hunter does now

Freezing Trap asks one question before it is thrown: **would the enemy it
catches just be freed?** The answer comes from the engine's own removal rules
(`ally_removal` / `can_free_ally` / `ally_freers`, asked of the aura the trap
actually lands, `traps::freezing_trap_aura`), never from a class list. Today
the Priest, the Paladin and the Felhunter free a trapped teammate; only the
Felhunter frees a trapped pet, and nobody frees a trapped Felhunter. The healers'
dispel scan and Devour Magic read their scope and pet reach from the same
`ally_removal`, so the cast and the question cannot drift.

A victim is **worth a trap** (`trap_victim_worth_it`) when:

- it is a **healer** — the dispeller the trap is for; a second dispeller behind
  it (a Felhunter, a second healer) is left to spend itself undoing it; or
- **no living teammate of its could free it** — a Felhunter always qualifies;
  a non-healer in a comp with no dispeller always qualifies; or
- **every teammate that could free it is one the Hunter's team is killing.**
  This is the "costs the dispeller its GCD at a useful moment" rule: when the
  enemy healer is the kill target, trapping its partner forces the focused
  healer to spend a GCD on the dispel instead of on itself.

Where the trap goes:

- **Off-target (aimed):** the best eligible enemy the team is not killing —
  healer first — thrown at its position (led if moving), and only when it is
  the only enemy near the landing. Unchanged in shape; it now filters by the
  rule above.
- **Fallback (the lane throw, when there is no off-target):** the trap still
  goes to the lane midpoint toward the enemy healer (or the kill target), but
  it is decided on the enemy **expected to spring it** — the visible enemy,
  pets included, nearest the landing (`expected_trap_victim`). Thrown when
  that enemy is worth a trap, held when a teammate of its would free it. While
  an enemy is **hidden** (a stealthed Rogue) and a visible enemy could free a
  teammate, the lane throw is held (`unseen_victim_would_be_freed`): the lane's
  victim at gates-open is that unseen Rogue. The trace records the expected
  victim as the throw's aim.
- **Range:** every trap placement refuses a landing beyond the ability's
  configured `range` (30yd, from `abilities.ron`), measured on the clamped
  landing. Refused rather than pulled in, because a landing is chosen for where
  it lands; the Hunter asks again next tick, closer. Ranged placement itself is
  intended (the Trap Launcher model, not Classic's feet-drop). The old lane
  throw landed 35yd out; it now waits roughly one second for the lane to come
  within 30yd.

## The mechanism: where the traps went

17 trap-relevant comps (2v2, 3v3, 1v1), seeds 0-19, traced (340 matches per
arm). Times are seconds after the gates.

| | before | after |
|---|---|---|
| traps thrown | 265 | 174 |
| sprung | 243 | 149 |
| **removed by an enemy dispel** | **120 of 243 sprung** | **23 of 149 sprung** |
| broken by damage | 3 | 18 |
| ran the full duration | 120 | 108 |
| thrown in the first 6s into a comp that freed the victim | 100 | **0** |
| matches with no Freezing Trap at all | 95 / 340 | 169 / 340 |

The 23 remaining removals: 20 in `Priest+Paladin` (aimed at the off-target
healer, freed by the other healer — the healer rule, by design) and 3 in
`Warlock+Priest+Rogue` (a Rogue whose freers were the kill target). 17 of the 18
breaks are `Warlock+Rogue`: the lane trap is thrown as the Warlock dies —
decided on the Felhunter (13) or, once the Felhunter has despawned with its
Warlock, on the Rogue it no longer protects (7) — and the Rogue, now the kill
target, walks into it and is broken free by the Hunter's own team.

Per comp (traps per match, zero-trap matches out of 20, median first throw,
removals; Hunter-side wins out of 20):

| comp | before: traps/m | zero | first | removed | after: traps/m | zero | first | removed | Hunter wins |
|---|---|---|---|---|---|---|---|---|---|
| H+Pri vs Rogue+Priest | 1.00 | 0 | 0.0s | 20 | 0.00 | 20 | - | 0 | 1 → 9 |
| H+War vs Rogue+Priest | 1.00 | 0 | 0.0s | 20 | 0.00 | 20 | - | 0 | 0 → 1 |
| H+Pri+War vs Mage+Priest+Rogue | 1.00 | 0 | 0.0s | 20 | 0.00 | 20 | - | 0 | 0 → 0 |
| H+Pri+War vs Warlock+Priest+Rogue | 1.00 | 0 | 0.0s | 20 | 0.30 | 14 | 22.2s | 3 | 2 → 2 |
| H+Pri vs Warlock+Rogue | 1.90 | 0 | 0.0s | 20 | 1.00 | 0 | 13.7s | 0 | 20 → 19 |
| H+Pri vs Warlock+Priest | 0.00 | 20 | - | 0 | 0.00 | 20 | - | 0 | 3 → 3 |
| H+Pri vs Warlock+Paladin | 0.00 | 20 | - | 0 | 0.00 | 20 | - | 0 | 1 → 1 |
| H+Pri vs Paladin+Warrior | 1.00 | 0 | 6.5s | 0 | 1.00 | 0 | 6.5s | 0 | 1 → 1 |
| H+Pri vs Priest+Paladin | 1.00 | 0 | 17.4s | 20 | 1.00 | 0 | 17.4s | 20 | 20 → 20 |
| H+Pri vs Warrior+Priest | 0.15 | 17 | 44.9s | 0 | 0.15 | 17 | 44.9s | 0 | 19 → 19 |
| H+Pri vs Mage+Priest | 0.00 | 20 | - | 0 | 0.00 | 20 | - | 0 | 0 → 0 |
| H+Pri vs Rogue+Warrior | 1.00 | 0 | 0.0s | 0 | 1.00 | 0 | 1.0s | 0 | 20 → 20 |
| H+Pri vs Shaman+Rogue | 1.10 | 0 | 0.0s | 0 | 1.15 | 0 | 1.0s | 0 | 0 → 0 |
| H vs Priest | 1.00 | 0 | 0.0s | 0 | 1.00 | 0 | 1.0s | 0 | 17 → 17 |
| H vs Warlock | 1.00 | 0 | 0.0s | 0 | 1.00 | 0 | 1.8s | 0 | 16 → 15 |
| H vs Warrior | 1.00 | 0 | 0.0s | 0 | 1.00 | 0 | 1.0s | 0 | 20 → 20 |
| H vs Rogue | 0.10 | 18 | 30.7s | 0 | 0.10 | 18 | 30.7s | 0 | 3 → 3 |

**Where the trap is used again.** `Hunter vs Priest` and `Hunter vs Warlock`
throw in every match again; against the Warlock all 20 traps spring on the
Felhunter (the enemy the throw was decided on) and run the full 8s — nobody
can free it. `Paladin+Warrior` throws in every match (on the Warrior, whose
only freer is the focused Paladin). `Warlock+Priest+Rogue` throws again in 6
of 20. `Warlock+Priest` and `Warlock+Paladin` threw at most once in 20 at
`061dfa5` and throw none now: the Hunter's team kills the Warlock, the healer is
the off-target and never reached point-blank, so neither the aim nor the
fallback comes up.

**Where it is not: the Rogue + dispeller comps.** In `Rogue+Priest` (both
partners) and `Mage+Priest+Rogue` the only trap the Hunter ever threw was the
gates-open lane throw, sprung by the stealthed Rogue and dispelled in a median
0.28s. That throw is the one the ruling keeps held. After the opener the Rogue
stays on the Hunter, so the Hunter never reaches the safe band (20yd+ from the
nearest enemy) where the rotation considers Freezing Trap at all; the trap has
no later window in these comps at either arm. A point-blank Freezing Trap peel
would give it one; that is a new behaviour for every melee matchup and is left
as a follow-up.

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

1v1 is unchanged, **Hunter vs Warlock included**: the trap springs on the
Felhunter, as it always did, and the Hunter wins as it always did.

### 2v2 — 3,090 matches, 301 reachable + 8 control cells, n=10 per cell

```
CONTROL: 80/80 matches with no affected class on either side are IDENTICAL
  ALL reachable  n=3010  49.3% -> 50.4%  +1.1pt  flips 254 (+142/-110)  z=1.95 ns  resolves >=1.1pt
  CLEAN          n=1260  38.2% -> 37.1%  -1.1pt  flips 104 (+44/-58)    z=1.29 ns  resolves >=1.7pt
  AGAINST        n=1260  60.8% -> 64.1%  +3.3pt  flips 122 (+82/-40)    z=3.71 SIG resolves >=1.8pt
  MIRRORED       n=490   48.4% -> 49.2%  +0.8pt  flips 28 (+16/-12)     z=0.57 ns  resolves >=2.3pt
NON-VACUITY: 3082/3090 ended by elimination; 1104 moved in winner or duration
SLICES TESTED: 3. Significant: 1 of 3 (AGAINST).
```

Per enemy comp, oriented to the Hunter's side (Hunter on exactly one side;
each row is 7 Hunter partners x 2 sides x 10 seeds). A row is a direction at
this n, not a value; the flips carry the significance.

| enemy comp | dispeller | Rogue | n | Hunter before | after | flips +/- | z |
|---|---|---|---|---|---|---|---|
| **Priest+Rogue** | Priest | yes | 140 | 42.1% | **6.4%** | +5/-55 | -6.5 |
| **Rogue+Warlock** | Warlock | yes | 140 | 91.4% | **72.9%** | +10/-36 | -3.8 |
| Priest+Warlock | Priest+Warlock |  | 140 | 29.3% | 25.7% | +3/-8 | -1.5 |
| Warlock+Warrior | Warlock |  | 140 | 83.6% | 82.9% | +10/-11 | -0.2 |
| Mage+Warlock | Warlock |  | 140 | 17.9% | 20.0% | +9/-6 | +0.8 |
| Shaman+Warlock | Warlock |  | 140 | 11.4% | 13.6% | +8/-5 | +0.8 |
| Paladin+Warlock | Paladin+Warlock |  | 140 | 4.3% | 8.6% | +8/-2 | +1.9 |
| Paladin+Rogue | Paladin | yes | 140 | 6.4% | 12.1% | +17/-9 | +1.6 |
| Mage+Paladin | Paladin |  | 140 | 5.0% | 6.4% | +2/-0 | +1.4 |
| Paladin+Warrior | Paladin |  | 140 | 20.7% | 20.7% | 0 | 0 |
| Priest+Warrior | Priest |  | 140 | 55.0% | 55.0% | 0 | 0 |
| Mage+Priest | Priest |  | 140 | 12.1% | 12.1% | 0 | 0 |
| Mage+Rogue | - | yes | 140 | 77.9% | 80.0% | +6/-3 | +1.0 |
| Rogue+Shaman | - | yes | 140 | 48.6% | 48.6% | +4/-4 | 0 |
| Rogue+Warrior | - | yes | 140 | 100.0% | 100.0% | 0 | 0 |
| Mage+Warrior | - |  | 140 | 40.0% | 40.0% | 0 | 0 |
| Mage+Shaman | - |  | 140 | 2.9% | 2.9% | 0 | 0 |
| Shaman+Warrior | - |  | 140 | 46.4% | 46.4% | 0 | 0 |
| **all 18** | | | 2520 | 38.6% | 36.3% | +82/-139 | -3.8 |

**The Hunter is worse by about 2pt, and the loss is two rows: the Rogue +
dispeller comps.** `Priest+Rogue` goes 42.1% → 6.4% and `Rogue+Warlock` 91.4%
→ 72.9%; every other row is within noise, and the no-dispeller rows move only
by the range limit's one-second delay (17 flips across six comps, +10/-7).

**This is the stealth reveal, and it is the thing to weigh.** At `061dfa5` the
gates-open lane trap springs on the stealthed Rogue in every one of these
matches, **revealing it**; the Priest or Felhunter frees it a fraction of a
second later, but the Rogue has lost its opener. With the trap held, the Rogue
opens from stealth — Cheap Shot on the Hunter or its partner — in every match
spot-checked (8 of 8, `Warrior+Hunter vs Rogue+Priest` and
`Rogue+Hunter vs Rogue+Warlock`, seeds 0-3, both arms). The trap was a spent
GCD as crowd control and a strong trade as a stealth reveal. Holding it is
what the ruling asks for (no gates-open throw into a crowd whose victim would
just be dispelled), and the price is these two rows.

### Warlock cells, explicitly

- **1v1 Hunter vs Warlock: 74.0% → 73.0%** (n=100, 1 flip). The trap is thrown
  in every match and springs on the Felhunter, which nobody frees.
- **Felhunter comps did not get easier for the Warlock side overall.** Across
  the six Warlock rows (n=840) the Hunter side goes 39.6% → 37.3%, all of it
  the `Rogue+Warlock` reveal. The other five go 29.3% → 30.2%: within noise,
  slightly toward the Hunter (+4.3pt vs `Paladin+Warlock`, the largest,
  z=1.9).
- The Felhunter's devours of a trapped teammate go **20 → 0** in the mechanism
  set (`Warlock+Rogue`): the Hunter no longer throws where the Felhunter would
  free the victim, and throws at the Felhunter itself when it is the one that
  would spring the trap.

## Baselines this invalidates

The Hunter cells of `canonical_1v1_n100_300s.csv` (1v1 is measured unchanged
here, but at n=50 per cell and the range limit touches every Hunter match),
`canonical_2v2_full_n100_300s.csv` (the Rogue + dispeller cells above most of
all), and `canonical_3v3_full_n50_300s.csv` (3v3 was not swept; the two 3v3
comps in the mechanism set moved). Every cell without a Hunter is unchanged —
the controls are byte-identical in winner and duration.

## Follow-ups

- **A point-blank Freezing Trap peel.** The Rogue + dispeller comps have no
  trap window after the opener because Freezing Trap is a safe-band ability.
  Trapping the melee on the Hunter would give the held trap a use, and would
  change every melee matchup — its own card and its own sweep.
- **An opener counter to stealth, on purpose.** The measured value of the old
  opening throw was the reveal (~36pt in `Priest+Rogue`, ~19pt in
  `Rogue+Warlock`). If that is wanted, it wants to be a decision, not a lane
  trap that happened to catch a Rogue.
- **AS-129** (healers dispel pets) moves the pet row of
  `who_can_free_a_freezing_trap` and nothing else; the Hunter AI follows it.
