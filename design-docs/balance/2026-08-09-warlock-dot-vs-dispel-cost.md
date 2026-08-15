# Warlock DoTs vs Dispel Magic: pricing the exchange

**Date:** 2026-08-09
**Changes:** `Corruption` **25 → 16**, `Fear` **30 → 22** (`assets/config/abilities.ron`).
**Status:** committed. Canonical baselines NOT yet regenerated — see *Follow-up*.

---

## The problem

The Warlock's game against a dispeller is an exchange: it applies a DoT, the
healer removes it. The costs were lopsided against the Warlock on both axes:

| | cost | cast | cooldown |
|---|---:|---|---|
| Corruption / Curse of Agony / Immolate | 25 mana | GCD | — |
| Unstable Affliction | 30 mana | 1.5s cast | — |
| **Priest Dispel Magic** | **18 mana** | **instant** | **none** |

Measured over 80 matches (`Warlock+Priest` vs `Priest+Rogue`, both maps):
**9.5 DoT applications per match, 46% of them dispelled, mean lifetime before
removal 2.1 seconds.** Curse of Agony applied at 19.25s was gone at 19.28s;
Corruption applied at 27.73s was gone at 27.75s.

So the Warlock paid 25 mana and a global to have a DoT removed roughly half the
time, within about two seconds, for 18 mana and no global — out of a pool of 296
that funds about **eleven casts in a match that runs 50-70 seconds**. Every
combatant in these matches finishes on ~10 of 296 mana; mana, not the GCD, is the
binding resource, and the Warlock was losing the resource race by construction.

## Why a cost change rather than a mechanic

In TBC the Warlock's edge in this exchange came from **dispel-resistance
talents**: a dispel would sometimes fail outright, wasting the healer's global
*and* its mana. That is the effect worth having.

Importing it literally would mean adding another random roll on top of one that
already exists — `process_dispels` picks which debuff instance to remove at
random, and that randomness is *intentional* (see
`docs/solutions/.../dispel-randomness-intentional`, and the
[`dispel-randomness-intentional`] note: it makes purge an investment rather than
a deterministic counter). Stacking a second RNG layer on the same interaction
would make the matchup swingier without making it deeper.

Pricing it into the spell instead gives the Warlock the same structural
advantage **plainly and every time**: it wins the exchange slightly on mana on
every trade, rather than winning it a lot at random. 16 is deliberately just
under Dispel Magic's 18.

## Measured effect

`Warlock+Priest` vs `Priest+Rogue`, free targeting, n=120 per cell, before/after
on identical seeds:

| map | before (25) | after (16) | effect |
|---|---:|---:|---:|
| BasicArena | 37% | **49%** | +12pt |
| PillaredArena | 34% | **43%** | +9pt |
| **pooled** | 36% | **46%** | **+10pt, z ~ 2.3** |

Individually each cell is underpowered (z ~ 1.8 and ~1.4); the pooled figure is
the one to quote.

**A prediction that did not hold:** the change was expected to matter more in
longer games. It does not — the effect is marginally *larger* on the short map,
and mean match length barely moves (51s → 51s, 66s → 64s). The likely reason is
that the exchange is lopsided from the first dispel rather than accumulating a
deficit over time, so it bites early too.

## The second change: Fear 30 → 22

Cutting Corruption exposed a ratio problem. Fear stayed at 30, so the
**CC-to-DoT price ratio moved from 1.2 to 1.9** — a Fear cost nearly two DoT
applications. That re-opened a map-dependent regression in the CC value model
(`cc_policy: Priced`), measured at n=300 per cell varying only Fear's cost:

| Fear mana | BasicArena | PillaredArena |
|---|---:|---:|
| 30 | +9pt (z=2.13) | **-1pt (z=-0.17)** |
| **22** | **+8pt (z=2.06)** | **+6pt (z=1.57)** |
| 16 | +11pt (z=2.65) | +11pt (z=2.61) |

16 removes the map dependence entirely but **inverts the intended price
ordering**, making hard CC cheaper than a dispel. The ordering is deliberate:

> **DoT (16) < Dispel Magic (18) < Fear (22)**

Hard crowd control is the strongest of the three effects and should cost the
most. 22 keeps that and recovers most of the effect, at the price of the
PillaredArena cell not quite clearing significance (z=1.57).

## Scope and follow-up

- **Only Corruption changed.** Curse of Agony (25), Immolate (25) and Unstable
  Affliction (30) are untouched. The same argument plausibly applies to the first
  two; UA is a different case because its value is partly the dispel *punish*
  (138 damage + silence), which rewards being dispelled. Left open deliberately —
  what was measured is what was changed.
- **Canonical baselines need regenerating.** Corruption is in every Warlock
  matchup, so `canonical_1v1_n100`, `canonical_2v2_full_n100` and
  `canonical_3v3_full_n50` are all stale with respect to this change. The +10pt
  here is one comp on two maps: it establishes direction and rough magnitude, not
  a balance verdict.
- **Mana sustain is the real missing lever.** Both changes here were forced to
  move the mana dial in order to fix *relative* pricing, because per-spell cost
  is the only economy lever that exists. No class has sustain tooling — no Life
  Tap or Drain Mana, no Evocation or mana gems, no Shadowfiend. With those, class
  longevity could be tuned without re-pricing what spells are worth against each
  other. Recorded in `design-docs/roadmap.md`.
- **Watch for over-correction.** The prior canonical outcome had Warlock at
  ~45% 2v2 / ~47% 3v3 after Death Coil. A +10pt shove in one comp could put the
  class over the line; the Mage+Warlock package was already flagged as the
  nerf-watch item there.
