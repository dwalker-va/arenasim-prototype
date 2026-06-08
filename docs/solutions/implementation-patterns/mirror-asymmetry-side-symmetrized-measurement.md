---
title: "Mirror-matchup side bias: measure balance deltas on side-symmetrized cells"
category: implementation-patterns
tags:
  - balance
  - matrix-runner
  - determinism
  - ecs-iteration-order
  - measurement
  - mirror-matchup
module: project-wide
symptom: "Mirror matchups land far from 50% (e.g. Rogue mirror 13%/87% by side); ordered cells differ by up to ~18%"
root_cause: "Same-frame action races resolve in ECS query/spawn iteration order, reshuffled by archetype churn — a deterministic but side-correlated first-mover advantage"
date: 2026-06-07
---

# Mirror-matchup side bias → measure on side-symmetrized cells

## Problem

Mirror matchups (a comp vs an identical comp) do not land at ~50%. The Rogue
mirror is 13% / 87% by spawn side; some ordered cells differ from their transpose
by up to ~18%. This is not RNG — it is deterministic and side-correlated, so a
raw ordered matrix cell over- or under-states a matchup by a fixed margin.

## Root cause

In a perfectly symmetric mirror, both sides reach every decision point on the
exact same frame. Same-frame races then resolve in **ECS query/spawn iteration
order** (team 1 spawns first → earlier table rows → iterated first), and that
order is reshuffled mid-match by archetype churn (component insert/remove, e.g.
`ActiveAuras` removed when empty). Three winner-take-all resolvers convert the
ordering into a side advantage: the same-frame instant-CC queue
(`reflect_instant_cc` in `decide_abilities`), orb-pickup tie-breaks, and
lethal-swing suppression in auto-attack (`died_this_frame`).

Full mechanism, per-mirror magnitudes, and exact binomial p-values:
`docs/reports/2026-06-mirror-asymmetry-diagnostic.md`.

## Why the fix is deferred (and why this is a measurement doc, not a bug fix)

Every *localized* fix only relocates or inverts the bias:
- BTreeMap swaps — not implicated (the hot-path collections were already ordered,
  or the ties are exact-equal f32s).
- Nearest-wins orb tie-break — distances are exactly tied.
- Freezing archetype order — flips the Rogue mirror to ~87–100% T1 instead of
  fixing it.

The real fix is a same-frame-resolution redesign (resolve simultaneous actions
order-independently), which is out of scope as a large change. Until then, **work
around the bias in measurement** rather than pretending a raw cell is unbiased.

## Guidance — the side-symmetrized protocol

When reporting any matrix/sweep winrate delta, **symmetrize each cell**: average a
matchup with its transpose so first-mover bias cancels to first order.

- Cell A vs B: `winrate = avg( (A,B), (B,A complement) )`, i.e.
  `(t1_wins(A,B) + t1_wins(B,A as t2)) / total`.
- Row-vs-col form: `symmetrized(row,col) = avg( raw(row,col), 100 − raw(col,row) )`.
- **Never use raw mirror/diagonal cells as tuning targets** — they carry the full
  bias and there is no transpose to cancel it. Report them raw, labeled, and
  excluded from balance decisions.

The full matrix already runs both orderings of every pair, so symmetrizing is
free — it is purely an aggregation choice at read time.

## When to apply

- Any before/after balance assessment from `--matrix` or the `--batch` sweep
  harness (see the `balance-sweep` skill and
  [[dep-upgrade-with-matrix-verification]]).
- Any time you are tempted to read a single ordered cell as a matchup's "true"
  winrate — don't; read the symmetrized value.

This is the **standing protocol until the same-frame-resolution bias is fixed.**
When it is fixed, raw and symmetrized cells will converge and this workaround can
be retired.

## Related

- [[dep-upgrade-with-matrix-verification]] — the matrix as a behavioral oracle;
  same N=100 / 300s-cap discipline.
- [[casting-visibility-snapshot-blind-spot]] — a balance-shifting change measured
  with this protocol.
