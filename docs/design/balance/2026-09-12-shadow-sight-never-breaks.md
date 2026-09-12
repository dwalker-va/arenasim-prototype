# Shadow Sight runs its full 15 seconds — measurement

**Date:** 2026-09-12
**Card:** AS-52
**Baseline:** `65cf204` (origin/main)

`SHADOW_SIGHT_BREAK_ON_DAMAGE` goes from `0.0` (break on ANY damage — the
Polymorph convention) to `-1.0` (never breaks). The orb's 15s anti-stealth buff
used to end on the first hit its holder took, which in a contested mid-fight
pickup was almost immediately. The apply site's own comment always claimed the
aura did not break; the value was the typo.

This is a sim change, so it is not byte-identical by construction and the
question is what it moves. Answer: **the break events disappear and nothing
else does** — 1200/1200 sweep rows bit-identical, and the only line that differs
in any of 72 long-fight logs is the removed `broke from damage` event. The
mechanism behind that null is structural, not statistical (see Findings).

## Method

Same-binary control: `origin/main` built and stashed as `arenasim_before`, the
branch built as `arenasim`, and every config below replayed through both from
the same JSONL / JSON files. 300s cap, `Legacy` AI. Raw per-match rows in
`2026-09-12-as52_{before,after}.csv` beside this doc.

Cells were chosen by a pickup scout rather than by comp popularity: the orbs
spawn 90s after gates, and most competitive 2v2 cells end before that (a first
36-match set of Rogue/Warrior/Hunter+healer comps reached the spawn once and
picked up nothing). Paladin-anchored and double-healer fights reliably run past
90s, so the sweep is built from those, plus two Rogue comps because stealth is
what the buff counters.

## Non-vacuity (72 matches, logs kept)

Twelve long-fight comps x 2 maps (BasicArena, PillaredArena) x 3 seeds, run
through both binaries:

| | before | after |
|---|---|---|
| matches reaching the orb spawn | 57/72 | 57/72 |
| `picks up Shadow Sight` events | 71 (Team 1: 36, Team 2: 35) | 71 (36 / 35) |
| `Shadow Sight broke from damage` events | **45** | **0** |
| logs differing between arms | — | 30/72 |
| non-break lines differing | — | **0** |
| winner flips | — | 0 |

The change is exercised: 40 matches had a pickup, 45 buffs were broken by
damage at baseline, and none are after. The 30 logs that differ differ ONLY by
the missing break line.

## Balance sweep (n=100 per cell)

| cell | n | reached orb | T1 before | T1 after | delta | z | bit-identical | winner flips |
|---|---|---|---|---|---|---|---|---|
| **2v2** Warlock+Paladin vs Mage+Paladin | 100 | 81 | 67% [57.3-75.4] | 67% [57.3-75.4] | 0 | 0.00 | 100/100 | 0 |
| **2v2** Warrior+Paladin vs Mage+Paladin | 100 | 23 | 28% [20.1-37.5] | 28% [20.1-37.5] | 0 | 0.00 | 100/100 | 0 |
| **2v2** Hunter+Paladin vs Warlock+Paladin | 100 | 25 | 11% [6.3-18.6] | 11% [6.3-18.6] | 0 | 0.00 | 100/100 | 0 |
| **2v2** Priest+Paladin vs Warlock+Paladin | 100 | 98 | 0% [0.0-3.7] | 0% [0.0-3.7] | 0 | 0.00 | 100/100 | 0 |
| **2v2** Priest+Paladin vs Priest+Shaman | 100 | 100 | 34% [25.5-43.7] | 34% [25.5-43.7] | 0 | 0.00 | 100/100 | 0 |
| **2v2** Rogue+Paladin vs Mage+Paladin | 100 | 37 | 93% [86.3-96.6] | 93% [86.3-96.6] | 0 | 0.00 | 100/100 | 0 |
| **2v2** Rogue+Priest vs Priest+Paladin | 100 | 0 | 100% [96.3-100.0] | 100% [96.3-100.0] | 0 | 0.00 | 100/100 | 0 |
| **2v2** Mage+Priest vs Priest+Paladin | 100 | 89 | 91% [83.8-95.2] | 91% [83.8-95.2] | 0 | 0.00 | 100/100 | 0 |
| **3v3** Priest+Warlock+Paladin vs Mage+Priest+Paladin | 100 | 100 | 85% [76.7-90.7] | 85% [76.7-90.7] | 0 | 0.00 | 100/100 | 0 |
| **3v3** Priest+Paladin+Hunter vs Priest+Paladin+Shaman | 100 | 100 | 100% [96.3-100.0] | 100% [96.3-100.0] | 0 | 0.00 | 100/100 | 0 |
| **3v3** Rogue+Priest+Paladin vs Warlock+Paladin+Shaman | 100 | 4 | 100% [96.3-100.0] | 100% [96.3-100.0] | 0 | 0.00 | 100/100 | 0 |
| **3v3** Warrior+Paladin+Priest vs Mage+Warlock+Paladin | 100 | 9 | 13% [7.8-21.0] | 13% [7.8-21.0] | 0 | 0.00 | 100/100 | 0 |

Pooled T1: 722/1200 = 60.2% [57.4-62.9] in both arms. Draws 7/7. No cell
MOVED; no cell moved at all. Two cells are exposure-vacuous for the orb
(Rogue+Priest vs Priest+Paladin never reaches 90s; the 3v3 Rogue cell 4/100)
and are reported rather than dropped, so the Rogue coverage that matters is
Rogue+Paladin vs Mage+Paladin (37/100 reach the orb; the scout logged Rogue
pickups in that cell).

## Mechanism diagnostic — where the buff has something to reveal

The buff can only act on a stealthed enemy, and (Findings 1-2) the only time
one exists at 90s is the all-stealth stalemate the orbs were designed for. So
the stalemate was measured directly. The 1v1 cell is a **mechanism
diagnostic**, not a balance cell; the two Rogue-mirror 2v2s were added for the
same reason and turned out never to reach the orb. Rows in
`2026-09-12-as52_diag_{before,after}.csv`.

| cell | n | reached orb | T1 before | T1 after | delta | bit-identical | winner flips |
|---|---|---|---|---|---|---|---|
| **diag 1v1** Rogue vs Rogue | 100 | 100/100 | 57% [47.2-66.3] | 57% [47.2-66.3] | 0 | 100/100 | 0 |
| **diag 2v2** Rogue+Priest vs Rogue+Priest | 100 | 0 | 51% [41.3-60.6] | 51% [41.3-60.6] | 0 | 100/100 | 0 |
| **diag 2v2** Rogue+Priest vs Rogue+Paladin | 100 | 0 | 1% [0.2-5.4] | 1% [0.2-5.4] | 0 | 100/100 | 0 |

The 1v1 stalemate resolves the same way in both arms, and one logged seed
shows why (seed 0, log diff is exactly one line):

```
[ 99.97s] [EVENT] Shadow Sight orbs have spawned!
[102.07s] [BUFF]  Team 1 Rogue #1 picks up Shadow Sight!
[112.31s] [EVENT] Team 1 Rogue #1's Shadow Sight broke from damage (9/0)   <- before only
Duration: 121.21s
```

The pickup reveals the other Rogue, the opener lands, and both are out of
stealth from that moment on. The break — ten seconds later — removes a buff
whose only job was already done; the fight ends inside the 15s window
regardless. Whether the buff survives its holder being hit changes nothing
because it never had a second target to reveal.

## Findings

1. **The buff's only consumer is stealth visibility.** `acquire_targets`
   (`combat_ai.rs`) and the class-AI visibility predicate (`class_ai/mod.rs`)
   read `ShadowSight` solely to decide whether a STEALTHED enemy can be seen or
   targeted. Nothing else reads it.

2. **At 90s nobody in a mixed comp is stealthed.** There is no Vanish and no
   in-combat re-stealth: a Rogue leaves stealth at its opener and never
   returns. So in every comp with a non-Rogue, by the time the orbs spawn there
   is nothing for a 15s buff to reveal, whether it lasts 0.1s or 15s. That is
   why 1272 matches with 71 pickups produce zero downstream divergence — not a
   small effect hiding under noise, but no effect to have.

3. **The card's expected buff to the orb-holder does not materialise.** The
   "straight buff to whichever side picks up the orb" premise assumed a
   revealed target to act on. The fix is correct on intent (the mechanic's
   documented purpose is a 15s window, and the encyclopedia now says so) and
   carries no balance cost in any measured cell.

4. **The one case with a stealthed target at 90s — the all-stealth stalemate
   the orb-seeking branch in `move_to_target` exists for — is ALSO null**, for
   the reason the diagnostic log shows: the reveal is consumed by the opener,
   after which the buff has nothing left to do. The change would only move an
   outcome if a second stealth event happened inside the 15s window, and the
   game has no re-stealth. If Vanish is ever added, this is the measurement
   to rerun.
