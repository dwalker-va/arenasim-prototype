# Warlock movement: kiting is net-negative — keep stand-and-cast

**Date:** 2026-06-14
**Outcome:** Investigated and **reverted**. The Warlock stays on legacy
target-pursuit (stand-and-cast). Putting it on the ENGAGE/KITE posture machine
regressed it by ~4–7 points in 2v2 at every tuning tried.

This doc is the durable record so the next person doesn't re-run the
investigation. The hypothesis was sound; the data disproved it cleanly.

Origin: [`docs/brainstorms/2026-06-13-warlock-movement-ai-requirements.md`](../../docs/brainstorms/2026-06-13-warlock-movement-ai-requirements.md),
[`docs/plans/2026-06-13-001-feat-warlock-movement-posture-plan.md`](../../docs/plans/2026-06-13-001-feat-warlock-movement-posture-plan.md).

---

## The hypothesis

The Warlock was one of three classes still on legacy pursuit (Warrior, Rogue,
Warlock). The brainstorm premise: a squishy cloth DoT caster *should kite when a
melee focuses it* — flee while throwing instants, plant to hardcast when safe —
the same intuition that motivated the Mage/Hunter migrations.

We built it: the full ENGAGE/KITE migration (a `warlock` config block,
proximity-gated dispatch via `melee_within`, hardcast suppression while kiting,
and a `warlock_postures` probe module). It passed all unit/probe tests and
behaved as designed in traces. Then we measured balance.

## The result: net-negative, concentrated where it was meant to help

Warlock 2v2 sweep, 2400 matches, n=20, 300s cap, batch harness. **After** = new
movement (current HEAD); **before** = legacy pursuit (`origin/main`); same
matchup set through each binary.

| | Overall | Enemy-has-melee slice | Enemy-no-melee slice |
|---|---|---|---|
| **Legacy (stand & cast)** | **39.6% ±2.0** | **42.2%** | 36.4% |
| New movement (shipped 12/14) | 32.5% ±1.9 | 28.9% | 36.9% |

Non-overlapping CIs — a real **−7pt** drop. Cells the Warlock *used to win*
collapsed to 0% (e.g. `Warlock+Mage vs Warrior+Priest: 100% → 0%`).

The slice cut is the tell:

- **Enemy has no melee** (Warlock never kites): dead neutral (36.4% → 36.9%).
  This *validated the implementation* — zero spurious behavior change when the
  Warlock shouldn't kite, and no regression to any other class.
- **Enemy has melee** (Warlock kites): **−13pts** (42.2% → 28.9%). All the loss
  lives here — exactly the matchups the feature targeted. In legacy the Warlock
  was *better* vs melee (42%) than vs casters (36%): it out-sustains melee by
  standing and casting. Kiting throws that away.

## Mechanism (three things measured, two hypotheses killed)

Per-entity instrumentation on `Warlock+Priest vs Warrior+Priest` (the
user-relevant healer-comp cell, which went 40% → 0%):

| Metric | Legacy | New |
|---|---|---|
| Avg casts/match | 13.3 | 7.1 |
| Avg damage dealt | 566 | 307 |
| Warlock survived | 8–10 / 15–20 | **0** |
| Drain Life self-heals | 0.6/match | **0.0** |
| Max distance to ally healer | 18yd | 34yd |
| % time beyond 40yd heal range | **0%** | **0%** |

1. **Suppression halves output** — casts and damage both roughly halved, because
   in 2v2 a melee is almost always within the suppression radius, so the Warlock
   perpetually withholds Immolate/UA/Shadow Bolt **and Drain Life** (its
   self-heal). That explains both the damage drop and the survival crash.

2. **Refuted: "it flees its healer."** An early hypothesis was that the
   flee-dominant movement carried the Warlock out of its Priest's 40yd heal
   range. Direct measurement killed it: neither version ever exceeds 40yd (new
   maxes at 34yd; Hunter/Mage at 26/14yd), and the healer's posture machine
   keeps the Priest close. **There is no ally-anchor bug** for any class.

3. **Key combat-model fact: only movement (and CC/Silence) interrupts a cast —
   melee damage does not** (`combat_core/casting.rs`: "Root does NOT interrupt
   casting — only movement"). So the legacy Warlock stands in melee and casts
   its full kit; auto-attacks don't stop it. This means the *correct* hardcast
   suppression is "am I moving (in KITE)?", **not** "is a melee near?". We
   reworked suppression to source from the resolved KITE posture
   (`evaluate_dps_posture` returning `DpsPosture`) instead of proximity — the
   right altitude. It helped only +0.8pts.

## Tuning can't rescue it

Rework binary (posture-based suppression), sweeping kite radii — runtime
`movement.ron` edits, no rebuild:

| Config | Overall | Melee slice |
|---|---|---|
| Legacy | **39.6%** | **42.2%** |
| rework + kite 6/8 | 36.2% | 35.4% |
| rework + kite 4/5 | 35.2% | 33.0% |
| rework + kite 3/4 | 35.8% | 33.6% |

It **plateaus at ~35–36% and does not converge toward legacy as the radii
shrink to nothing.** That rules out a tuning miss: if kiting were merely
mistuned, near-zero radii would approach legacy. Instead, *any* kiting costs
~4pts — the feature's core action (moving) is itself the cost. After a kite the
Warlock oscillates (ENGAGE drops the directive and pursues back to preferred
range), wasting time a stationary caster spends casting.

## Conclusion

In this combat model a DoT caster with a Drain Life self-heal and a Fear peel is
**strictly better standing still and casting** than kiting. Its answer to melee
is Fear (8s peel on the chaser) + DoT pressure + Drain Life sustain — none of
which want movement. The premise "the Warlock should kite when focused" does not
survive contact with the sim.

"Plant-via-posture" (Warlock on the machine with KITE disabled) was considered
for consistency, but it is behaviorally identical to legacy — ENGAGE falls
through to the same pursuit, the scorer's corner/spacing terms only act in KITE,
and no `movement_decision` traces fire without transitions. It would add inert
scaffolding and a re-regression trap (a stray non-zero radius silently restores
the loss) for no functional gain. **Decision: keep the Warlock on legacy
stand-and-cast, as a deliberate documented choice.**

## If you want to revisit

The migration is a recipe-follow from the plan (config block → dispatch branch →
suppression signal → probes). But **re-measure** — balance will have drifted,
and the conclusion is balance-dependent. It would only flip if the combat model
changes such that movement stops forfeiting cast time, or a Warlock kit gains a
mobile-cast / instant-nuke that makes kiting damage-neutral.

The one reusable insight regardless of class: **suppress interruptible casts on
the movement posture (am I about to move?), not on melee proximity** — only
movement interrupts casts here, so a planted caster should cast even in melee.

## Reproduction

```bash
cargo build --release && cp target/release/arenasim /tmp/after
git stash; git checkout origin/main && cargo build --release && cp target/release/arenasim /tmp/before
git checkout - # back to the feature branch

python3 scripts/gen_sweep.py --t1 'Warlock+{p}' --t2-size 2 --n 20 \
  --exclude-double-healer > /tmp/wl.jsonl
/tmp/after  --batch /tmp/wl.jsonl --out /tmp/after.csv  --jobs 16
/tmp/before --batch /tmp/wl.jsonl --out /tmp/before.csv --jobs 16
python3 scripts/agg_sweep.py /tmp/after.csv --compare /tmp/before.csv
# slice: --include '_vs_.*(Warrior|Rogue)' for the enemy-has-melee cut
```
