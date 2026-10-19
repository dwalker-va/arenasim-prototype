# AS-380: the Paladin's dip keeps its budget and its dying-teammate abort in the pressure band

**Tier:** DIRECTIONAL, scored from the Paladin's side. **Success condition,
stated up front:** a bug fix, so no balance movement is sought; every `Legacy`
diff is a dip the fix aborted, and `TeamPlan` is byte-identical. Measured
2026-10-19, `main` at `55849194` against this branch. The Paladin twin of
AS-371 (`2026-10-19-as371-priest-dip-abort.md`).

## The defect and the fix

`evaluate_paladin_posture` ran the Hammer of Justice dip's abort check
(`dip_should_abort`: budget spent, dip target no longer eligible, a teammate at
or below `urgency_hp_threshold`) only while the Paladin was not `focused` (the
plain PRESSURED trigger). A dip is preempted into PRESSURED on `focused_onset`,
which under `Legacy` is narrower than `focused` for a closing caster (AS-249: a
caster targeting the healer between its preferred range and
`threat_intent_radius`, a 2yd band for a 28yd caster, holds the trigger without
the onset). In that band the dip was neither preempted nor checked: it walked
on past its budget and past a dying teammate.

The abort is now evaluated whatever the focus. The onset preemption still
outranks it in the posture match, so a dip becoming PRESSURED still enters
PRESSURED rather than aborting. Under `TeamPlan`, `compound_pressure_onset`
returns the trigger itself, so wherever the old `!focused` gate decided
anything the preemption had already fired: `TeamPlan` cannot change.

## The probes

`tests/paladin_dip_abort_band.rs`, staged matches (tier 3) on BasicArena,
Paladin+Warrior vs Warlock+Priest, seeds 1-2 (rolls only). The Paladin is
carried into the stage mid-dip at the enemy Priest 20yd away (beyond Hammer of
Justice's 10yd); the Warlock stands 29yd off, targeting it (in the band:
asserted on the landing frame). The `HealerPosture` deadline is staged on the
app clock, which is what the posture machine's `now` reads.

- **`a_dying_teammate_aborts_the_dip_with_a_caster_in_the_band`**: Warrior at
  30% health, budget left: `DipAbort` on the first evaluation.
- **`a_spent_budget_aborts_the_dip_with_a_caster_in_the_band`**: Warrior
  healthy, budget spent: `DipAbort` on the first evaluation.
- Both pair with the control (Warrior healthy, budget left, same stage): no
  transition out of DIP for 0.5s, and the Paladin walks over 1yd toward its
  target. On `main` both aborts came 0.2s late, once the Warlock closed out of
  the band.
- **`under_teamplan_the_band_preempts_the_dip`**: the dying stage under
  `TeamPlan` leaves DIP on its first evaluation as `PressuredEnter`.

Mutation-checked; each mutant fails the named tests:

| Mutation | Fails |
|---|---|
| The abort gated on `!focused` again (the defect) | dying, spent |
| The budget clause dropped from `dip_should_abort` | spent |
| The dying-teammate clause dropped | dying |
| Every dip aborts | dying, spent (their control) |
| The abort arm ahead of the preemption arm | `under_teamplan_…` |
| `TeamPlan`'s onset given the `Legacy` band | `under_teamplan_…` |

## `Legacy`: paired sweep

2,830 matches (275 cells a Paladin change reaches + 8 control, seeds 0-9),
BasicArena, `--affects Paladin`:

| Slice | n | before | after | flips | z |
|---|---|---|---|---|---|
| ALL reachable | 2750 | 49.7% | 49.7% | 1 (+1/-0) | 0.00 ns |
| CLEAN (Paladin on team 1) | 1250 | 65.3% | 65.4% | 1 (+1/-0) | 0.00 ns |
| AGAINST (Paladin on team 2) | 1250 | 33.8% | 33.8% | 0 | — |
| MIRRORED | 250 | 50.8% | 50.8% | 0 | — |

Control 80/80 identical. 2,818/2,830 ended by elimination; 1,615 distinct
durations in each arm. Two matches moved in winner or duration. Consistent
with no movement at a 0.1pt floor.

**Every frame the fix can change, found directly.** A diagnostic build of
`main` (the old gate, plus a line logged on every frame where the abort held
but the old gate suppressed it, and on every DIP transition) reproduced the
`before` CSV byte for byte. Over the whole sweep that line fired on 240 frames,
all inside 10 matches, and in every one the band threat was the dip's own
target: the enemy Druid at its preferred range, 28.1-30.0yd off, targeting the
Paladin.

| Match | Gate frames | Why the abort was owed |
|---|---|---|
| Mage+Paladin v Warlock+Druid, s6 | 152 | budget spent (by 3.5-6.0s) |
| Warlock+Druid v Warlock+Paladin, s8 | 31 | Warlock 48% → 40% |
| Mage+Paladin v Mage+Druid, s9 | 24 | Mage 47% |
| Mage+Druid v Mage+Paladin, s8 | 24 | Mage 50% |
| Warlock+Druid v Mage+Paladin, s4 | 3 | Mage 34-49% |
| Warlock+Druid v Warlock+Paladin, s6 | 2 | Warlock 49% |
| Warrior+Paladin v Mage+Druid, s4, and its mirror | 1 each | Warrior 42% |
| Rogue+Paladin v Hunter+Druid, s4, and its mirror | 1 each | Rogue 40% |

The eight with 1-24 frames ended in the same winner and duration. **The two
that moved are the two longest:**

- **Mage+Paladin v Warlock+Druid, s6** (team 2 won in 130.6s; now team 1 in
  82.9s). The Paladin entered its dip at 4.20s and was feared from 5.67s, inside
  its budget, until 13.65s. Its first evaluation after the Fear, at 13.67s, is the
  first gate frame, and the first event that differs between the arms' traces
  is the branch's `DipAbort` there. `main`'s dip ran on to 16.20s, the frame
  after the last gate frame, while the Druid stood in the band.
- **Warlock+Druid v Warlock+Paladin, s8** (106.2s; now 153.9s, same winner).
  The dip entered at 17.22s. At 17.78s its Warlock fell to 48%; that frame is
  the first gate frame and the first traced difference, the branch's
  `DipAbort`. `main`'s dip ran on to 18.32s, again the frame after the last
  gate frame.

## `TeamPlan`: byte-identical

The same 2,830 configs under `TeamPlan` on PillaredArena: the two CSVs are
byte-identical (2,823 kills, 7 at the cap, 1,893 distinct durations). Not
vacuous: the same diagnostic build over that batch logged 5,972 dip entries,
5,020 preempted into PRESSURED and 545 aborted, and the gate line fired on
zero frames, as the onset being the trigger requires.
