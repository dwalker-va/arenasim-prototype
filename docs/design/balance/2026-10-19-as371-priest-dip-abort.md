# AS-371: the Priest's dip keeps its budget and its dying-teammate abort in the pressure band

**Tier:** DIRECTIONAL, scored from the Priest's side. **Success condition,
stated up front:** a bug fix, so no balance movement is sought; every `Legacy`
diff is a dip the fix aborted, and `TeamPlan` is byte-identical. Measured
2026-10-19, `main` at `55849194` against this branch.

## The defect and the fix

`evaluate_priest_posture` ran the dip's abort check (`scream_dip_should_abort`:
budget spent, dip target ineligible, a teammate at or below
`urgency_hp_threshold`) only while the PRESSURED trigger was off. A dip is
preempted into PRESSURED on the ONSET test, which under `Legacy` is narrower
than the trigger for a closing caster (AS-249: a caster targeting the healer
between its preferred range and `threat_intent_radius`, a 2yd band for a 28yd
caster, holds the trigger without the onset). In that band the dip was neither
preempted nor checked: it walked on past its budget and past a dying teammate.

The abort is now evaluated whatever the trigger. The onset preemption still
outranks it in the posture match, so a dip that is becoming PRESSURED still
enters PRESSURED rather than aborting. Under `TeamPlan` the onset is the
trigger, so wherever the old gate mattered the preemption already fired first:
`TeamPlan` cannot change.

## The probes

`tests/priest_dip_abort_band.rs`, staged matches (tier 3) on BasicArena,
Priest+Warrior vs Warlock+Priest, seeds 1-2 (rolls only). The Priest is carried
into the stage mid-dip at the enemy Priest 20yd away; the Warlock stands 29yd
off, targeting it (in the band: trigger on, onset off — asserted on the landing
frame).

- **`a_dying_teammate_aborts_the_dip_with_a_caster_in_the_band`**: Warrior at
  30% health, budget left — `DipAbort` on the first evaluation. Its pair (the
  Warrior healthy): no transition out of DIP for 0.5s, and the Priest walks
  over 1yd toward its target. On `main` the dip ran on for 1.7s and ended in
  `PressuredEnter`.
- **`a_spent_budget_aborts_the_dip_with_a_caster_in_the_band`**: Warrior healthy,
  budget spent — `DipAbort` on the first evaluation; same pair. On `main` it
  ran on 1.2s.
- **`under_teamplan_the_band_preempts_the_dip`**: the dying stage under
  `TeamPlan` leaves DIP on its first evaluation by preemption, not abort.

Mutation-checked — each mutant fails the named test:

| Mutation | Fails |
|---|---|
| The abort gated on `!trigger` again (the defect) | dying, spent |
| The budget clause dropped | spent |
| The dying-teammate clause dropped | dying |
| Every dip aborts | dying (its control) |
| `TeamPlan`'s onset given the `Legacy` band | `under_teamplan_…` |

## `Legacy`: paired sweep

2,830 matches (275 cells a Priest change reaches + 8 control, seeds 0-9),
BasicArena, `--affects Priest`:

| Slice | n | before | after | flips | z |
|---|---|---|---|---|---|
| ALL reachable | 2750 | 49.9% | 49.8% | 3 (+1/-2) | 0.00 ns |
| CLEAN (Priest on team 1) | 1250 | 48.8% | 48.7% | 1 (+0/-1) | 0.00 ns |
| AGAINST (Priest on team 2) | 1250 | 50.6% | 50.6% | 0 | — |
| MIRRORED | 250 | 51.6% | 51.6% | 2 (+1/-1) | 0.00 ns |

Control 80/80 identical. 2,822/2,830 ended by elimination; four matches moved
in winner or duration. Consistent with no movement at a 0.2pt floor.

**Every moved match is an aborted dip.** Each was replayed with the trace under
both binaries; in each the first differing event is the `after` arm's
`DipAbort`, on the Priest's first evaluation past its 6s budget (a cast or a
Fear had kept it from evaluating at 6.0s), while `main`'s identical dip ran on
until the threat left the band. In all four the band threat was the dip's own
target — the enemy healer, at its 28yd preferred range targeting our Priest:

| Match | Dip entered | Aborted (branch) | `main`'s dip ended | Band threat |
|---|---|---|---|---|
| Mage+Priest v Rogue+Priest, s2 (team 2 Priest) | 3.08s | 9.83s | 9.93s | Priest 28.8yd |
| Mage+Priest v Warlock+Druid, s4 | 4.40s | 13.67s (feared 5.65-13.65) | 13.80s | Druid 28.6yd |
| Mage+Priest v Warlock+Shaman, s2 | 3.10s | 9.70s | 10.48s | Shaman 30.0yd |
| Rogue+Priest v Mage+Priest, s1 | 3.08s | 9.83s | 10.20s | Priest 28.8yd |

## `TeamPlan`: byte-identical

The same 2,830 configs under `TeamPlan` on PillaredArena: the two CSVs are
byte-identical (2,826 kills, 4 at the cap, 1,854 distinct durations). Not
vacuous: a traced sample of 55 of those Priest matches holds 27 dip entries,
23 preempted into PRESSURED and 2 aborted.
