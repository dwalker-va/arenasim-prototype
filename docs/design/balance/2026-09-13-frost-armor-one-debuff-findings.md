# Frost Armor's chill as one debuff — what it cost the Mage (AS-54)

Frost Armor's proc used to hang two independent auras on a melee attacker: a
movement slow and an attack-speed slow, both called "Frost Armor". They are now
one debuff. It lands as a unit, diminishes as a unit, and comes off as a unit.

Three separate things changed for the Mage, and only the first is the one the
card set out to fix:

1. A removal that takes the chill now takes **both** effects. Previously only
   the movement slow was ever a dispel candidate, so the Felhunter's Devour
   Magic or the Hunter's Master's Call lifted half a debuff. (The Priest's and
   Paladin's dispels are not part of this: a `MovementSpeedSlow` is graded
   below every caller's priority bar in `dispel_priority`, so they never
   dispelled the chill and still do not.)
2. A **second slow replacing the chill** — most often the Mage's own Frostbolt,
   which shares the `Slows` diminishing-returns category — takes the
   attack-speed half with it. It used to leave it standing.
3. The attack-speed half no longer **outlives or outflanks** the chill. It was
   queued as its own pending, so it carried no DR category: it survived a
   DR-immune rejection its movement-slow half did not, and it kept its full 5
   seconds while a diminished chill expired after 1.2. Riders now land with
   their face, at the face's post-DR duration, or not at all.

## Method

A **paired** before/after sweep: one JSONL of 5,150 seeded matches run twice,
once through `main`'s binary and once through this branch's. Same seeds, same
comps, same map (`BasicArena`, pinned on every line of the input), so the
change is the only variable and outcomes compare match by match.

- input: `sweeps/2026-09-13-as54-frost-armor-chill.jsonl` (committed — the
  batch runner's CSV carries no map column, so the input is the only record of
  which map was played)
- analysis: `sweeps/2026-09-13-as54-paired.py`
- results: `2026-09-13-as54_chill_{before,after}.csv`
- arms: 2v2 `Mage+{partner}` vs every distinct opposing pair, no double-healer,
  20 seeds per cell (175 cells, 3,500 matches); 3v3 `Mage+Priest+Warrior` vs
  every distinct opposing triple, no all-healer, 30 seeds per cell (55 cells,
  1,650 matches).

Because the design is paired, **McNemar's test on the matches whose outcome
flipped** is the instrument, not a comparison of two independent Wilson
intervals — the latter is badly conservative here. Wilson 95% intervals are
reported for the level.

**Slicing follows what the change can physically reach.** A chill exists only
where one side fields a Mage and the other fields a melee attacker. Every pet
in the game melees, so a Hunter or a Warlock brings one even though the class
is ranged. team1 always has a Mage, but its partner may be a melee and the
enemy may field a Mage of its own, so the chill runs in both directions.

**Errors: 0 in each arm. 5,133 of 5,150 paired matches ended by elimination**
(the remainder are draws), so the slices below are not measuring a wall of
timeouts.

## Results

Winrate is team1's — the side that always fields the Mage.

### 2v2 (n=3,500)

| slice | before | after | delta | flips | McNemar |
|---|---|---|---|---|---|
| all opponents | 64.5% [62.9-66.0] | 62.5% [60.9-64.1] | **-2.0pt** | 172 (+50/-119) | z=5.23 **sig** |
| our chill can land | 67.2% [65.6-68.8] | 65.1% [63.4-66.7] | **-2.1pt** | 172 (+50/-119) | z=5.23 **sig** |
| …enemy can remove it | 67.9% [65.7-70.0] | 66.9% [64.7-69.0] | -1.0pt | 57 (+19/-38) | z=2.38 sig |
| …enemy cannot remove it | 66.3% [63.8-68.7] | 62.7% [60.1-65.2] | **-3.6pt** | 115 (+31/-81) | z=4.63 **sig** |
| MIRRORED (enemy Mage chills us too) | 43.6% [39.9-47.3] | 43.7% [40.1-47.4] | +0.1pt | 12 (+5/-4) | z=0.00 ns |
| CONTROL (no chill possible) | 55.0% [44.1-65.4] | 55.0% [44.1-65.4] | +0.0pt | 0 | — |

### 3v3 (n=1,650)

| slice | before | after | delta | flips | McNemar |
|---|---|---|---|---|---|
| all opponents | 69.5% [67.3-71.7] | 67.5% [65.2-69.7] | **-2.1pt** | 171 (+68/-102) | z=2.53 **sig** |
| our chill can land | 69.1% [66.8-71.3] | 67.0% [64.7-69.3] | -2.0pt | 170 (+68/-101) | z=2.46 sig |
| …enemy can remove it | 66.9% [64.1-69.7] | 65.2% [62.3-68.0] | -1.8pt | 98 (+39/-58) | z=1.83 ns |
| …enemy cannot remove it | 73.3% [69.4-76.9] | 70.7% [66.8-74.4] | -2.6pt | 72 (+29/-43) | z=1.53 ns |
| MIRRORED | 58.3% [54.4-62.0] | 59.2% [55.3-63.0] | +1.0pt | 23 (+14/-8) | z=1.07 ns |

No 3v3 cell qualifies as a control: team1 carries a Warrior, so any opposing
triple without a melee of its own still contains a Mage that chills it.

## Read

**It costs a Mage comp about two points, in both formats.** -2.0pt at 2v2 and
-2.1pt at 3v3, and the two arms agreeing to a tenth of a point on independent
comps is worth more than either number alone. The direction is the expected
one: the chill is a debuff the Mage puts on the ENEMY, and it now comes off
more completely, so the Mage keeps less of it.

**The change is inert where it cannot reach, exactly.** In the 80 matches where
neither side can produce a chill, **all 80 are identical in both winner and
duration** — bit-exact, not merely same-winner. That is the control that makes
the rest of this attributable: the change predicts which matches move, and
those are the matches that moved. The 27-cell determinism baseline says the
same thing independently — nine of its cells moved and they are precisely the
nine with a Mage in them
(`tests/baselines/legacy_behaviour_2026-09-13_frost_armor_chill.txt`).

**The symmetric case washes**, as a symmetric change must: where the enemy
fields a Mage too, 12 flips out of 700 in 2v2 (+0.1pt, z=0.00) and 23 out of
630 in 3v3 (+1.0pt, z=1.07). Both sides lose the same thing, and the result
does not move.

**The dispel is NOT the dominant mechanism.** This is the finding that will
surprise anyone reading the card: the loss is *larger* against enemies with no
way to remove the chill (-3.6pt) than against enemies who can (-1.0pt). So most
of the two points is not item 1 in the list above — it is items 2 and 3, the
Mage's own Frostbolt replacing its chill and taking the attack-speed half with
it, and the attack-speed half no longer accumulating on a target whose chill
was diminished away or rejected outright.

**Read those two sub-slices as suggestive, not as an isolation of the dispel.**
"Can remove it" means the enemy fields a Warlock or a Hunter, and those comps
differ from the rest in far more than dispel access — pet pressure, kiting,
mana. The comparison is between two different opponent sets, not between two
treatments of one set. What it does establish is that a large effect survives
in comps with no dispel at all, which is enough to rule the dispel out as the
whole story.

**No claim about where the Mage now sits.** The canonical baselines are stale
by standing decision and AS-96 regenerates them last, so the absolute levels
above are properties of *this* seed set and these comps, not a tier position.
The delta is the result; the level is scaffolding for it.
