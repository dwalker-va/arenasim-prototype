# Freezing Trap: hold it for the dispeller — measurement

**Date:** 2026-09-27
**Card:** AS-125
**Tier:** DIRECTIONAL (`sweep-tiers.md`). Success condition: the opener no
longer throws the trap where a dispeller undoes it, the aim lands on the
dispeller, and the Warlock cells are reported whichever way they move. Win rate
was expected to be neutral at best (`hunter-trap-targeting`); it is reported,
not tuned.
**Arms:** before = `a2f483a` (main); after = this branch. Same JSONL both arms
(`sweeps/2026-09-27-as125-hunter-directional.jsonl`).
**Raw rows:** `2026-09-27-as125_sweep_base_a2f483a.csv`,
`2026-09-27-as125_sweep_after.csv` (one row per match),
`2026-09-27-as125_trap_events.csv` (one row per Freezing Trap thrown, per arm).

## What changed

Freezing Trap now answers one question before it is thrown: **can the enemy
undo it?** The answer comes from the engine's own removal rules
(`ally_removal` / `can_free_ally` / `frees_teammates`, asked of the aura the
trap actually lands, `traps::freezing_trap_aura`), never from a class list.
Today that makes the Priest, the Paladin and the Felhunter dispellers of a
trapped teammate, and only the Felhunter a dispeller of a trapped pet.

- **Off-target (the aimed trap).** A non-healer an enemy could free is no
  longer a trap candidate. A healer always is — it is the dispeller the trap is
  FOR. The dip cast gains the clean-landing guard the opportunistic drop
  already had.
- **Fallback (the lane throw, when there is no off-target).** Unchanged where
  nothing the enemy fields could free whoever springs it; **held** otherwise.
  The question is asked of the dispeller's kit, not of the teammates the Hunter
  can see — the teammate that walks into a lane trap at gates-open is a Rogue
  the Hunter cannot see.

**In a Warlock + healer team** the Hunter keeps aiming at the healer, and the
Felhunter is left free to devour it: the intended counter plays out. The
Felhunter is never aimed at (pets are not trap candidates), and a Warlock the
Felhunter or the healer would free is passed over.

## The mechanism: where the traps went

17 trap-relevant comps (2v2, 3v3, 1v1), seeds 0-9, traced. `hit` = sprang on
the victim it was aimed at (trace `target_id` against the `[TRAP] ... triggers
on` line).

| | before | after |
|---|---|---|
| traps thrown | 132 | 56 |
| sprung | 121 | 56 |
| sprung on the intended victim | 41 | 35 |
| sprung on someone else | 80 | 21 |
| **removed by an enemy dispel** | **60** | **8** |
| ran the full duration | 60 | 41 |
| thrown within 6s of the gates into a comp with a dispeller | 70 | **0** |

The 21 remaining "someone else" rows are all comps with no dispeller: a lane
throw aimed at the Warrior (or the Shaman) that the stealthed Rogue ran into,
then ran its full 8s (20 rows), and one double-healer trap. That is the
fallback working as specified: in those comps whoever springs it stays caught.

The 8 remaining removals are all `Hunter+Priest vs Priest+Paladin`: 7 traps
land on the Paladin they were aimed at and the Priest dispels them, 1 lands on
the Priest and the Paladin cleanses it — the healer exception, by design.

### Warlock comps, explicitly

| comp | before | after |
|---|---|---|
| `Hunter+Priest vs Warlock+Rogue` | 19 thrown; 10 devoured off the Rogue | 10 thrown, all after the Warlock died (its Felhunter despawns with it); 0 devoured |
| `Hunter+Priest+Warrior vs Warlock+Priest+Rogue` | 10 thrown, 10 devoured off the Rogue | 0 thrown |
| `Hunter vs Warlock` (1v1) | 10 thrown, 10 on the Felhunter, full 8s | 0 thrown |
| `Hunter+Priest vs Warlock+Priest` | 1 thrown, unsprung | 0 thrown |

The Felhunter's devours of a trap go 20 → 0 because the Hunter no longer
throws where it would devour, and the accidental full-duration Felhunter traps
go 10 → 0. Nothing makes Felhunter comps easier: in the sweep the Hunter side's
win rate against a Warlock goes **38.1% → 33.6%** (n=840, 138 flips) — the
direction is that Warlock comps got harder for the Hunter, not easier.

## Win rate (directional, 2v2)

```
CONTROL: 80/80 matches with no affected class on either side are IDENTICAL
  ALL reachable  n=3010  49.3% -> 49.6%  +0.3pt  flips 348  z=0.43 ns  resolves >=1.2pt
  CLEAN          n=1260  37.9% -> 35.8%  -2.1pt  flips 146  z=2.08 SIG resolves >=1.9pt
  AGAINST        n=1260  61.1% -> 63.8%  +2.7pt  flips 153  z=2.69 SIG resolves >=2.0pt
  MIRRORED       n=490   48.4% -> 48.6%  +0.2pt  flips 49   z=0.00 ns  resolves >=2.9pt
NON-VACUITY: 3080/3090 ended by elimination; 1402 moved in winner or duration
SLICES TESTED: 3
```

**The Hunter is worse by roughly 2-3pt**, and the loss sits in one place.
Splitting the Hunter-on-one-side matches by the enemy comp:

| enemy comp | n | Hunter before | after | flips |
|---|---|---|---|---|
| a Rogue **and** a dispeller | 420 | 46.0% | 35.2% | +44 / -89 (z=-3.9) |
| a dispeller, no Rogue | 1260 | 26.0% | 25.1% | +75 / -86 (ns) |
| a Rogue, no dispeller | 420 | 76.0% | 76.0% | 0 |
| neither | 420 | 29.0% | 29.0% | 0 |

**The lane throw the card set out to remove was worth something, and it was
not the CC.** In every one of the 50 opening throws into a Rogue + dispeller
comp, the trap sprang on the stealthed Rogue, **revealed it**, and the dispel
0.3s later freed a Rogue that had already lost its stealth opener: 0 of those
50 matches saw a Rogue opener. With the trap held, the Rogue opens from stealth
in 50 of 50 (Cheap Shot on the Hunter, typically). The trap was a spent GCD as
crowd control and a good trade as a stealth reveal. That is an outcome of the
ruling rather than a defect in the change, and it is the thing to decide next
(see *Follow-ups*).

## The three candidates

- **A — guard the fallback** (`sweeps/2026-09-27-as125-variant-a.patch`: the
  victim must be the only enemy near the midpoint). Measured on its own over
  the 17-comp mechanism set: **132 of 132 traps identical to before**, every
  row. At gates-open the midpoint is empty of every enemy the Hunter can see —
  the Rogue that runs into it is stealthed — so the guard passes and the throw
  happens exactly as before. A alone cannot fix the opener.
- **B — lead the victim instead of the midpoint.** Tried two ways on the
  branch. Leading the old fallback candidate (usually the Hunter's own kill
  target) made the trap land on it — and 40 of 96 traps then broke on the
  Hunter team's own damage (the Hunter's Serpent Sting applied after the
  throw, a teammate's Rend). Pulling the lane throw back inside the trap's
  30yd placement range (the midpoint lands at 35yd at gates-open) moved a
  still-successful opener catch on the stealthed Rogue by about a second, and
  four no-dispeller 2v2 cells each lost 16-20 of their 20 matches
  (`Hunter+Warrior vs Mage+Rogue` 20 -> 0).
  Shipped: the aimed placements lead (they already did); the lane throw keeps
  its midpoint, because its victim is running down that line.
- **C — make stealthed-but-known enemies trap-eligible.** Not buildable as
  stated since AS-147: a stealthed enemy is absent from the class AI's view
  entirely, so "eligible" would mean the AI perceiving it, which is the leak
  AS-147 closed. The ruling's dispeller-first targeting supersedes it: the
  question is no longer who to aim at in the opener, but whether to throw.

## Follow-ups

- **An opener counter to stealth, on purpose.** The measured value of the old
  opening throw was the reveal. If that is wanted, it wants to be a decision
  (a deliberate lane trap against a comp the Hunter knows fields a Rogue, or
  another reveal), sized against the ~11pt the Rogue + dispeller comps lost.
- **Freezing Trap's placement range is not enforced.** `try_place_trap_at`
  throws to any distance; the lane throw lands 35yd out against a 30yd range.
  Unchanged here on purpose (see B).
- **AS-129** (healers dispel pets) moves the "who frees a pet" row of
  `who_can_free_a_freezing_trap` and nothing else; the Hunter AI follows it.
