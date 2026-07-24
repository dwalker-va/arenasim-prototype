# Line-of-Sight Branch — Balance Assessment

**Sweeps run 2026-07-23** on branch `feat/line-of-sight-mechanics` (PR #86),
before merge. This documents the balance impact of the line-of-sight feature
and its most balance-relevant code change, and records the pre-merge
verdict. **Not a canonical baseline** — see the caveat below.

**Headline: balance-safe to merge as-is.** On the default map (BasicArena) the
branch is statistically indistinguishable from the pre-branch canonical — the
mana-timing change is neutral there. On the new LoS map (PillaredArena) the
meta shifts exactly as the plan predicted: cast-dependent ranged down, stealth
mobility up, and the Warrior *dips* (its Charge opener is line-of-sight-gated).
No oppressive or unplayable competitive comps, canaries clean, draws 0.2–0.8%.

---

## What changed on this branch (balance-relevant)

1. **Line-of-sight mechanics** — casts and ranged auto-attacks now require an
   unobstructed segment to target; LoS is checked at cast start *and*
   completion. Only matters on maps with obstacles (PillaredArena).
2. **Mana charged only on successful cast completion** (WoW-faithful). A cast
   that fizzles at completion — target juked out of LoS, or died — costs no
   mana; an interrupted cast never completes, so it costs nothing either. This
   makes every hardcaster more mana-efficient against interrupt/LoS pressure.
   It is **map-independent**, so its balance effect shows up on BasicArena.

The question for each: (1) did the mana change move class balance on the
default map, and (2) how does the meta land on the new LoS map.

## Method

Four batch sweeps on the branch binary, 300s cap, side-symmetrized cells,
analyzed with `scripts/comp_tiers.py`:

| Sweep | Map | Coverage | N/cell | Matches | Draws |
|---|---|---|---|---|---|
| 2v2 | BasicArena | competitive pairs (double-healer excluded) | 50 | 31,250 | 0.3% |
| 2v2 | PillaredArena | competitive pairs (double-healer excluded) | 50 | 31,250 | 0.8% |
| 3v3 | BasicArena | all distinct-class triples | 30 | 94,080 | 0.2% |
| 3v3 | PillaredArena | all distinct-class triples | 30 | 94,080 | 0.4% |

Compared against the pre-branch canonical baselines (`canonical_*_300s.csv`,
generated 2026-07-09, Priest sustain cycle). All figures are **competitive**
class tiers (2v2 = at most one healer; 3v3 = one or two healers).

> **Caveat — this is not a canonical baseline.** The canonical summary is the
> *shipped-state, full-matrix, default-map* reference. This data is (a) from an
> unmerged branch, (b) the competitive subset at reduced N (2v2 excludes the
> three double-healer pairs; N=50/30 vs canonical 100/50), and (c) spans two
> maps, whereas canonical is default-map only. **Regenerate the canonical
> baselines from the merged `main` binary after PR #86 lands** — a near-
> formality for BasicArena given the neutrality shown below.

---

## (1) BasicArena — the mana change is balance-neutral

Branch BasicArena tiers sit on top of the pre-branch canonical within CI noise:

| Class | 2v2 branch | 2v2 canonical | 3v3 branch | 3v3 canonical |
|---|---|---|---|---|
| Mage | 65.9 | 66.0 | 67.0 | 66.2 |
| Shaman | 59.4 | — | 49.7 | — |
| Paladin | 59.1 | — | 49.8 | — |
| Priest | 51.3 | — | 50.0 | — |
| Rogue | 41.6 | 41.6 | 52.4 | 52.1 |
| Warlock | 42.8 | — | 46.7 | — |
| Hunter | 45.1 | — | 39.0 | — |
| Warrior | 39.3 | 39.2 | 45.0 | 45.9 |

Mage, Rogue, and Warrior — the three classes most exposed to the mana-timing
change or most likely to reveal a regression — are flat to within a point at
N=50/30. **The hypothesis that the mana change over-buffs casters on open maps
is refuted.** Only Paladin separates from noise elsewhere in the branch data
(+~1.4), which does not threaten balance.

## (2) PillaredArena — LoS reshapes the meta as intended

| Class | 2v2 Basic | 2v2 Pillar | Δ | 3v3 Basic | 3v3 Pillar | Δ |
|---|---|---|---|---|---|---|
| **Mage** | 65.9 | 56.9 | **−9.0** | 67.0 | 62.5 | **−4.5** |
| **Rogue** | 41.6 | 51.8 | **+10.2** | 52.4 | 56.9 | **+4.5** |
| **Warrior** | 39.3 | 34.5 | **−4.8** | 45.0 | 42.5 | **−2.5** |
| Shaman | 59.4 | 60.6 | +1.2 | 49.7 | 52.5 | +2.8 |
| Paladin | 59.1 | 58.6 | −0.5 | 49.8 | 49.5 | −0.3 |
| Priest | 51.3 | 53.2 | +1.9 | 50.0 | 47.8 | −2.2 |
| Warlock | 42.8 | 43.9 | +1.1 | 46.7 | 46.5 | −0.2 |
| Hunter | 45.1 | 44.1 | −1.0 | 39.0 | 40.1 | +1.1 |

The three big movers tell a coherent story:

- **Mage down (−9.0 / −4.5).** Hardcast-ranged loses casts to pillar occlusion;
  a target that breaks LoS mid-Frostbolt fizzles it. Mage remains the top class
  on both maps — this trims dominance, it doesn't dethrone.
- **Rogue up (+10.2 / +4.5).** Stealth-mobility beats LoS: the Rogue closes
  under cover and doesn't depend on an unobstructed segment to open. This is
  *stealth*-melee up, not melee broadly — the Warrior, the other melee, moves
  the opposite direction (below).
- **Warrior down (−4.8 / −2.5).** The Warrior's Charge opener is LoS-gated at
  cast start, so a pillar between it and its target denies the gap-closer.
  Note this is **not** a pathfinding failure: the Warrior rounds pillars at
  ~99% of full speed with zero stalls (see the `warrior_pillar_pathing` probe
  in `tests/movement_probes.rs`). Good pathing, LoS-gated opener — the winrate
  dip is the opener, not the movement.

Top 2v2 pillar comps: Mage+Shaman (80.1), Rogue+Shaman (72.6), Paladin+Rogue
(70.0) — the Rogue's rise puts it into three of the top comps it was absent
from on BasicArena.

## Canaries

- **Anomaly (non-competitive comp ≥ 50% vs the field):** none. The only
  non-competitive comp to surface is triple-DPS Mage+Rogue+Warlock, at 48.9%
  (basic) / 44.1% (pillar) vs the competitive field — below the 50% line. No
  burst comp substitutes for a healer.
- **Draw rate:** 0.2–0.8% across all four sweeps — no cap artifacts; the 300s
  cap and arena dampening resolve endgames cleanly.
- **3v3 dominant-shape watch:** single-healer stays the dominant top-comp shape
  on both maps. Double-healer presence in the top comps ticks up on pillars
  (3/8 vs 2/8 on basic) — elevated, not dominant. Watch, not a blocker.

## Verdict

**Merge-safe, no tuning pass required.** BasicArena is neutral; PillaredArena
delivers the intended LoS variance (cast-ranged down, stealth up, LoS-gated
openers punished) without producing an oppressive or unplayable competitive
comp. The map is a genuine strategic differentiator, which was the goal.

## Deferred follow-up levers (below must-fix)

Neither is at a threshold that blocks merge; both are candidates if PillaredArena
becomes a heavily-played map:

1. **Rogue-on-pillars (+10.2 2v2).** The largest single-class swing. Healthy as
   a map-identity effect, but worth watching if PillaredArena weighting rises —
   a +10pt swing is large enough to distort the aggregate meta if the map is
   common.
2. **Warrior pillar-charge reliability (−4.8 2v2).** The Warrior is the biggest
   loser purely because its opener is LoS-gated. If the pillar-map Warrior needs
   help, the lever is Charge reliability (e.g. a short LoS-reacquire path or a
   fallback engage), *not* movement — the pathfinding is already clean.

## Post-merge chore

Regenerate the canonical baselines (`canonical_{1v1,2v2,3v3}_*_300s.csv` +
`canonical_baselines_summary.md`) from the merged `main` binary per the
`balance-sweep` skill. BasicArena will be within noise of the current
canonical; the real change is that PillaredArena now exists as a distinct
balance surface, which the team may choose to track with its own baseline.
