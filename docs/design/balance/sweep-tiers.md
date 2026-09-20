# Two tiers of balance sweep: directional and authority

Card AS-104. One of these words was already in use before this file existed:
`2026-09-18-as60-dual-wield-findings.md` and
`2026-09-18-as122-rogue-offhand-findings.md` both open with
`Tier: **directional**`, and nothing in the repo said what that meant. Its
counterpart appeared nowhere at all. This is what they mean.

**The cheap tier is SMALLER, not looser.** Every rule about what a number
means survives both tiers. What changes is how many cells you buy, and what
you are then allowed to say.

---

## The two tiers

**DIRECTIONAL** — a focused slice of the cells the change can plausibly reach,
sized in minutes. It answers *which way, and roughly how much*. Its number is
reported with its limitation in the same breath, and it **may not be cited as
a class's standing**.

**AUTHORITY** — the full treatment, reserved for when a number will be cited
later: a canonical baseline regeneration, or a change whose whole point is the
magnitude.

The card's author picks the tier when the card is written, and says why.

## Picking one: state the success condition

"I expect this to move nothing" and "I need to know how much this moves" are
different questions and deserve different instruments. Say which one the card
is asking, on the card, before the sweep runs.

This is not bookkeeping. AS-87 filled a **content hole** — two classes had no
usable main-hand caster weapon — and spent 2.3 hours of authority-scale sweep
to measure about +1pt, which its own n could not resolve from zero. That was
read as a disappointing run. It was the *ideal* outcome: the hole is filled and
balance did not move. A clean +3pt would have been the worrying result. The
mismatch was never the answer, it was the instrument: that card needed a strong
control and an honest statement of scale, not a resolvable delta.

`paired_sweep.py --expect nothing|move` puts the success condition in the
output, and answers it in its own terms — including printing the resolution the
run actually bought, because **a null result means nothing without it**.

## What both tiers keep

1. **Paired at identical seeds.** Both arms run the same batch JSONL. The
   comparison is match-to-twin, not rate-to-rate.
2. **McNemar on the flips.** Significance comes from the discordant pairs
   across the whole run. Wilson intervals are printed for the LEVEL and are not
   the test.
3. **Non-vacuity reported.** Decisive matches, distinct durations, flip counts.
   A "nothing moved" claim over a batch that drew every match proves nothing.
4. **No per-cell figures at small n.** 100 paired matches buy about ±9.7pt on
   one cell. That is a direction, not a value, and it cannot rank two comps.
5. **The control is stated separately from the delta.** See below.

## Cut CELLS, not seeds

This is the whole trick, and it is sound for a specific reason: **in a paired
design the significance comes from the flip count across the run, not from
per-cell precision.** AS-86's aggregate held at 20 seeds per cell precisely
because no per-cell estimate was used in any claim.

So a Shaman change does not need the 625-cell 2v2 matrix. It needs the cells
with a Shaman, plus enough cells without one to prove the diff stayed home:

```bash
scripts/gen_sweep.py --full 2 --exclude-double-healer \
  --affects Shaman --control-cells 8 --n 10 > sweep.jsonl
# 225 reachable + 8 control of 625 cells -> 2,330 matches, not 62,500
```

Ten seeds a cell looks thin and is not: 233 cells x 10 pools to the same
flip count as 25 cells x 100, and no per-cell figure is quoted from either.

`--affects` keeps every cell the change can reach and samples
`--control-cells` of the ones it cannot, at the **same seeds**. The control
sample is ordered by a digest of the cell rather than by a stride, because the
enumeration is a nested product and a constant stride aliases against it — even
spacing over the 400 Shaman-free 2v2 cells returned eight controls sharing two
opponents.

## Sizing one

**Default: about 2,500 paired matches per arm, plus 8 control cells.** That is
not a round number — it is what AS-122's directional run actually was (25
cells x n=100), and `paired_sweep.py` puts its resolution at **±1.7pt** on the
overall at the 18% flip rate that run produced. Two points is enough to answer
"which way, roughly how much" for almost any card.

Shape it however the change wants: 25 cells x 100 seeds and 250 cells x 10
seeds buy the same significance, because the flips pool across the run. Prefer
more cells when the change interacts with comps, more seeds when it does not.

The resolution is not a function of n alone, so it cannot be looked up in
advance — it depends on how much the change flips. Roughly, N ≈ 4,300 x f
paired matches resolve 3pt, where f is the fraction of matches that flip: a
change that flips 18% of the sweep needs about 800, one that flips 2% needs
about 100. Size generously, then read the floor the run actually printed and
quote that, never the nominal n.

## The control is not the delta, and does not need authority n

AS-87's control came out perfect: 720 matches identical with no affected class
on either side, 1,861 of 3,150 differing with one present. That discharges the
byte-identity requirement and proves the diff touched nothing outside its three
classes — and it is the part a reviewer actually leans on.

**A directional run can carry a full-strength control and a weak delta, and
should say so.** The control is a *correctness* check with a binary answer, so
it saturates at a few hundred matches. The delta is an *estimate*, and only the
delta needs sample size.

`paired_sweep.py` prints the control first, as a bit-exactness claim over
winner **and duration**, and exits non-zero when it fails — a control that
moves is worth more than any delta in the run.

## Verification is cheap too, and for the same reason

The control-versus-delta split has a reviewer-side twin, one step later:
**most of what a reviewer wants to check recomputes from the committed
artifacts, and only claims about the CHANGED side need the experiment
rebuilt.**

The mistake this rule is drawn from: a Tester was told that reproducing a
mechanism slice required rebuilding the experimental arm — about an hour of
cold Bevy build plus two sweeps. It did not. The decision-relevant column was
a **base-arm** statistic, computable from the binary the PR head already
builds. Doing the cheap thing first reproduced it exactly *and* recovered an
undisclosed degenerate cell that had inflated the headline from 41.3% to
52.8% — the most valuable finding on that card, for free.

**A measurement card commits its arm, because the reviewer's ability to close
the expensive column is worth more than the author's estimate of whether it
matters.** That is not hypothetical. On AS-99 the arm existed only as prose
plus a hash; its Tester reasonably declined the expensive verification as
non-decision-critical; the patch was then committed, and the Tester used it to
close the very column it had declined, reproducing every arm-side figure. The
author cannot know in advance which column the reviewer will want, which is
exactly why the decision should not rest on the author's estimate.

Two things a findings doc owes its reviewer, both cheap:

- **Say which claims recompute from the committed data and which need the
  arm.** A short table is enough. It lets a reviewer decline an hour
  deliberately instead of discovering the gap halfway through it.
- **Commit the arm as a PATCH, not as a SHA or a prose description.** A
  reviewer can then rebuild it and re-run the expensive column instead of
  taking the author's word for it. When the change was a one-line revert this
  costs nothing, and it is the difference between a report and evidence.

  **The reproduction claim is identical OUTPUT, not an identical binary.**
  Rust embeds absolute build paths, so a reader building the same patch in a
  different directory gets a different binary hash and the check appears to
  fail when nothing is wrong. Quote both hashes if it helps a local reader,
  but the claim that travels is the CSV.

Commit the sweep JSONL too — `docs/design/balance/sweeps/` exists for exactly
this, and the batch CSV does not carry the map.

### Before qualifying a claim, ask whether the committed artifacts can un-qualify it

The directional tier will produce a lot of small slices, and a small slice
usually has more in it than it looks like. So this habit has to travel with
the tier, or the tier manufactures the failure at scale.

Three times on one card the right fix was *use the data you already have*
rather than *go measure more*, and each caution was individually defensible:

- a per-cell table disclaimed as underpowered at n=40, when the swapped half
  of the same sweep made it n=80 for free;
- an arm documented in prose because rebuilding it looked expensive, when
  committing the patch would have made it reproducible for nothing;
- durations quoted at n=4 with a caveat, when the committed CSVs carried them
  at n=80 — **and the n=4 pair turned out inverted against the n=80 pair.**

A hedge feels like rigour. Sometimes it is a way of not checking. The test is
cheap: name the artifact that would settle it, and see whether it is already
in the repo.

## Reporting

```bash
scripts/paired_sweep.py before.csv after.csv \
  --affects Shaman --tier directional --expect nothing
```

The tool prints four slices — `CONTROL`, `CLEAN` (only team1 affected),
`AGAINST` (only team2), `MIRRORED` (both, so the effect should wash) — plus the
reachable aggregate. Three reporting rules are mechanical rather than
remembered:

- **Every delta carries the floor it sits on, and the floor comes from the
  DISCORDANCE, not from n.** This is the one piece of arithmetic in the tool
  worth defending. A paired test never looks at the concordant pairs — a
  match that came out the same both ways carries no information about the
  change, and McNemar discards it — so the run's power is set by how many
  matches FLIPPED, and n is only an upper bound on that. Two runs of the same
  size differ by 6x in what they can resolve: at n=2,500, a change that flips
  2% of the sweep resolves **0.6pt**, and one that flips 80% resolves
  **3.5pt**. So a figure derived from n (or from the Wilson interval on the
  level, ±1.9pt at that size) is wrong in *both* directions — and it is wrong
  in the dangerous direction, claiming precision the run does not have,
  exactly when the change flips a lot, which is when someone most wants to
  cite a magnitude. Computing it from the observed discordance is also what
  lets a run that flipped nothing print **unresolvable** instead of a
  reassuring small number.
- **The slice count is printed next to any significant slice.** Testing four
  slices and reporting the one that came back is what multiple comparisons
  produce. The directional tier will make this **more** common, not less,
  because more cards will run more small slices.
- **A significant MIRRORED slice is noise unless corroborated.** It is the
  slice where the effect is supposed to wash. Report it as a count, not as a
  finding, unless a mechanism metric backs it.

**Prefer per-frame mechanism metrics over win rate where the MECHANISM is the
claim.** `tests/camp_sweep.rs` aggregates thousands of samples a match;
win rate is one bit. AS-122 did this well — it showed the off-hand swing firing
(0 → 374 same-tick swing pairs) rather than inferring it from a win rate.

## A run sized in hours races the merge queue

This has nothing to do with statistics. **AS-54 merged mid-sweep** and moved the
sim, so AS-87's before-arm was built against a base that no longer existed and
the headline had to be re-measured — twice. A run sized in minutes finishes
inside the window where `main` is stable. That is a structural argument for the
cheap tier on its own.

## What a sweep actually costs

The card was filed on the belief that this box does about **0.57 matches a
second**, and that number drove its whole framing. It is low. But the first
version of this section, written from one run, then over-corrected in the
other direction, and the correction is worth keeping because it is the
mistake `CLAUDE.md` names: **attributing a difference by elimination.**

Two runs of the same binary on the same machine, `/usr/bin/time -p`:

| run | matches | `--jobs` | load | wall | matches/sec | user/match | **sys/match** | effective cores |
|---|---|---|---|---|---|---|---|---|
| A | 1,750 (Rogue 2v2) | 16 | 17–70 | 1147.7s | 1.52 | 0.456s | **0.921s** | 2.10 |
| B | 2,640 (this card's diagonal) | 6 | 6–8 | 1285.8s | 2.05 | 0.513s | **0.847s** | 2.79 |

**The kernel time is not contention.** Run A spends two CPU-seconds in the
kernel for every one simulating, and it is tempting — I did it — to read that
as thrashing, because A ran at load 60 with 101 GB of 128 resident. Run B
says otherwise: at load 6–8 with a third of the workers, the kernel cost per
match is *the same* (0.85s against 0.92s). It is a property of the workload,
present on a quiet box, and it is **62–67% of the total CPU a match costs**
(66.9% in run A, 62.3% in run B, 64.1% pooled over the two).

That kills the inference I drew from run A alone. "A match costs 0.46
CPU-seconds of `user`, so 18 cores should do ~40/sec" ignores the 0.9s of
`sys` that does not go away. **The real per-match cost is ~1.37 CPU-seconds**,
stable across both runs, and the batch runner turns it into only **2–3
effective cores out of 18** whichever `--jobs` it is given. That ceiling is
unexplained and nobody has looked at it; it is the single biggest lever on
sweep cost and it is not a scheduling problem.

**Contention is real, and smaller than the card assumed.** A and B ran
different sweeps, so their matches/sec are not comparable — the honest
contention figure from this pair is effective cores, 2.10 to 2.79, about
+33%. The tighter measurement is AS-99's, same binary and same 520 configs
with load as the only variable:

| | wall clock | matches/sec |
|---|---|---|
| under three-way load | 518.2s | 1.00 |
| quiet, `--jobs 8` | 205.1s | 2.54 |

**2.5x, with the two runs agreeing byte for byte.** One caveat on that pair,
because this doc asks the same of everyone else: a committed CSV does not
carry timing, so unlike every other number here it is a reported observation
rather than something a reader can recompute from the repo.

**So: size a sweep at 1–2.5 matches/sec, not 0.57 and not 40.** Every figure
above lands in that band or at its edge — 1.00 and 1.52 under load, 2.05 and
2.54 quiet. Scheduling is worth somewhere between 1.3x and 2.5x, and it is
free to claim — but a quiet box does not make authority scale cheap, it makes
it about two and a half times less expensive than the card feared.

### Thrashing costs wall clock and nothing else

Worth stating plainly, because it bounds what the cost argument is allowed to
claim. A match's outcome is a function of its seed, comp and map, and **not
of how the box was scheduled while it ran** — measured, not assumed:
Tester-AS-99 re-ran a committed sweep config from an independently rebuilt
binary, on a differently-loaded box with different worker scheduling, and got
520/520 rows byte-identical to the committed CSV, durations to 2dp included.
**Contention changes when a result arrives, not what it is.**

So the cheap tier's case is about **time and merge-window risk only** — never
about result quality. "A directional run is smaller" is a claim about how many
cells you bought; it is never a claim that the matches you did run are worth
less. That is a cleaner argument than "small runs are good enough", and it is
the one to make.

### Sizing `--jobs`

**Key it off the number of agents sweeping right now, not off a number you
chose at launch.** `uptime` is worth reading, but reading it once is not
enough, and this is measured rather than supposed: AS-99 sized its jobs off
`uptime` *before* starting (load 26–39), picked `--jobs 6` as a deliberately
conservative division of 18 cores, and still ran at 1.00 matches/sec — the
same run it later got 2.54 from on a quiet box at `--jobs 8`. A number
chosen once goes stale the moment another agent starts, which is exactly what
happened: three agents each independently reasonable and collectively
thrashing.

A load average well above the core count means a wall-clock estimate is
wrong by a factor of two or three, in the direction you will not enjoy.
Beyond that, `--jobs` is not the lever — see the effective-cores ceiling
above.

## The team-1 mirrored-slice question: settled

Four balance cards in one cycle produced a tally in which two showed a
significant **team-1 gain in the MIRRORED slice** — the slice where the effect
is supposed to wash. If that were systematic it would be a fact about the
MEASUREMENT, quietly biasing every mirrored slice this project has published.

| run | mirrored slice | verdict |
|---|---|---|
| AS-86 relics, Paladin 2v2 | +1.9pt z=2.01 | significant |
| AS-86 relics, Shaman 2v2 | −0.4pt z=0.39 | nothing |
| AS-86 relics, 3v3 | −0.3pt z=0.15 | nothing |
| AS-87 caster 1H, base 1 | +2.2pt z=2.00 | significant |
| AS-87 caster 1H, base 2 | +2.8pt z=2.81 | significant (correlated with base 1 — same seeds and comps) |
| AS-87 caster 1H, base 3 | +2.7pt | **not** significant; a different cell (Priest CLEAN) was |
| AS-54 Frost Armor | +0.1pt z=0.00 | nothing |
| AS-97 Shaman weapon | −1.5pt z=0.45 | nothing |
| AS-122 Rogue off-hand | −0.6pt z=0.23 | nothing (added 2026-09-20, recomputed from the committed CSVs) |

**Nine slices, three significant, and the three sit on two cards** — AS-87's
base 1 and base 2 are the same cell measured twice off shared seeds and comps,
so the tally is closer to two independent hits than three.

**One row is outside the probe's scope, and it is a null one.** AS-86
contributed a 3v3 arm; this probe is 2v2-and-1v1 on `BasicArena` only. The
significant AS-86 row is the 2v2 Paladin slice, which is inside scope, so the
conclusion below covers every row it is asked to explain — but a reader
auditing the tally should be told which row it does not reach rather than left
to work it out.

AS-60's row is deliberately absent: it armed **team 1 only** via a
`team1_equipment` override, so its enemy-Rogue cells are a second one-sided
slice rather than a mirror, and its +19.7pt there is the change working as
designed. `--affects` cannot see that distinction — see the caveat at the top
of `paired_sweep.py`.

### Before you propose a null control, meet this argument

The card proposed settling it with *"a mirrored slice on a NULL change — two
identical binaries, same seeds"*, and two people endorsed that for weeks. It
cannot answer this question. The reason is worth getting exactly right,
because a neighbouring and very similar-looking run **is** worth doing.

**A null probe cannot express a slot asymmetry, whichever way determinism
goes.** Two identical binaries at identical seeds either produce identical
CSVs or they do not. If they do, every flip count is zero and McNemar's z is
0 — the run has no way to say anything about slot 1. If they do *not*, what it
has found is a determinism defect, which is real and worth knowing and still
says nothing about slot 1. There is no outcome of that run which bears on the
question it was proposed to settle.

**And there is no constructible placebo either.** The tempting repair is "run
a change with no directional content and see whether team 1 still gains". No
such change exists here. A paired design differs from its twin only where the
change reaches, so **any match that flipped, flipped because the change
reached it**. There is no perturbation that moves outcomes without also being
the thing under test. That is the real argument, and it does not depend on
determinism at all.

**Do not read any of this as "the sim is deterministic, so a same-binary
control is pointless".** Determinism here is a property the codebase WORKS TO
MAINTAIN, not an axiom. *What a byte-identity result proves* in `CLAUDE.md`
records AS-58, where hash-ordered float reductions reseeded per process gave
genuine run-to-run differences from **one unmodified binary** — Rogue
`crit_chance` took three distinct bit patterns across 40 runs — and AS-75
leaves four more such sites latent, safe only because their results have
test-only callers today. `CLAUDE.md` accordingly *recommends* a same-binary
control as the first thing to try on a difference you cannot attribute. That
guidance stands. Its subject is run-to-run determinism, which is a different
question from this one.

Cross-process determinism is currently **measured, not assumed**:
Tester-AS-99 independently rebuilt the base binary, re-ran a committed sweep
config on a differently-loaded box with different worker scheduling, and got
**520/520 rows byte-identical to the committed CSV, durations included to
2dp**.

**And the split control strictly subsumes a null probe anyway.** A null probe
runs one binary twice; a split control runs *two different binaries* over the
cells the change cannot reach — the same bit-exactness claim, over a stronger
comparison, at no extra cost, and it discharges an obligation the run has to
meet regardless. Prefer it. Reach for a same-binary control when you suspect
non-determinism specifically.

What is left for the slot question is to attack the hypothesis's NECESSARY
CONDITION instead, in one arm, with no before/after at all.

**What is answerable, and was answered.** For a change BOTH sides carry to
still shift team 1's win rate, slot 1 must carry an advantage for the change
to amplify. If the slot is exactly symmetric there is nothing to amplify, and
the mirrored-slice observations are multiple comparisons. Slot symmetry is a
one-arm measurement:

- **DIAGONAL** — identical comps on both sides (25 2v2 teams + all 8 1v1
  self-mirrors, 80 seeds). No comp-strength confound is possible.
- **SWAP-CLOSED** — every ordered pair of 25 distinct 2v2 teams, 5 seeds.
  Comp strength cancels across each mirror pair, and the script checks that
  closure rather than assuming it.

5,640 matches, one unmodified binary, exact two-sided binomial against 0.5:

| arm | team1 wins | rate | p | rules out |
|---|---|---|---|---|
| diagonal | 1219 / 2393 decisive | 50.94% | 0.37 | ±2.00pt |
| swap-closed | 1503 / 2998 decisive | 50.13% | 0.90 | ±1.79pt |
| **combined** | **2722 / 5391 decisive** | **50.49%** | **0.48** | **±1.3pt** |

**The slot is symmetric.** The mirrored gains that prompted the question were
+1.9pt to +2.8pt — larger than this probe excludes. So there is no team-1
advantage of that size for a change to amplify, and the mirrored-slice
observations are what multiple comparisons produce. Full write-up, including
what the probe does NOT settle, in
`2026-09-20-as104-slot-symmetry-findings.md`.

Supporting that reading: AS-87's significant cell **moved between runs** (Mage
mirrored, Mage mirrored, then Priest clean) over three runs sharing seeds and
comps and differing only in base. A significant cell that moves is the
signature of multiple comparisons, not of an effect.

**The rule stands.** If a third *card* lands a significant mirrored slice, add
its row above and open a card. Add the NULL rows too — the denominator matters
as much as the numerator, and four rows of which two are null is a very
different picture from two rows of which two are positive.

## Tooling

| | |
|---|---|
| `scripts/gen_sweep.py` | build the batch JSONL; `--affects` / `--control-cells` cut cells for the directional tier |
| `arenasim --batch f.jsonl --out x.csv --jobs N --trace-mode off` | run one arm |
| `scripts/paired_sweep.py` | the paired analysis: control, four slices, McNemar, resolution floor, verdict |
| `scripts/agg_sweep.py` | single-arm win-rate tables with Wilson intervals |
| `scripts/headtohead_sweep.py` | per-team AI-profile comparisons (a different question) |
| `tests/camp_sweep.rs` | per-frame mechanism metrics — prefer these when the mechanism is the claim |

Commit the JSONL a published finding rests on to
`docs/design/balance/sweeps/`, and name it in the findings doc.
