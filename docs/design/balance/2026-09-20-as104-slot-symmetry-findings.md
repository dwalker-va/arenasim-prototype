# AS-104 — is team slot 1 advantaged?

Measured 2026-09-20 on `card/AS-104-sweep-tiers`, one release binary at
`aef713c` plus this branch's docs-and-scripts diff (nothing in the diff is
compiled into the simulator). Inputs and the analysis script are committed
beside this file; every number below is recomputable from them.

## The question, and why the card's version of it cannot be run

Four balance cards in one cycle produced a tally in which two showed a
**significant team-1 gain in the MIRRORED slice** — the slice where both sides
carry the change and the effect is supposed to wash. AS-104 recorded that
because of what it would mean if systematic: a fact about the MEASUREMENT,
quietly biasing every mirrored slice this project has published, invisible
from inside any single card.

The card proposed a settling probe: *"a mirrored slice on a NULL change — two
identical binaries, same seeds."*

**That probe cannot answer this question, whichever way determinism goes.**
Two identical binaries at identical seeds either agree or they do not. If they
agree, every flip count is zero and McNemar's z is 0 — the run has no way to
say anything about slot 1. If they disagree, what it has found is a
determinism defect, which is real and worth knowing and still says nothing
about slot 1. No outcome of that run bears on the question.

**Nor is there a constructible placebo.** A paired design only differs where
the change reaches, so any match that flips flipped *because* the change
reached it. There is no perturbation with no directional content to feed
through the pipeline; "a null change that still moves outcomes" is not a thing
this simulator can express. This is the argument that actually does the work,
and it does not rest on determinism at all.

**This is not an argument against same-binary controls**, which `CLAUDE.md`
recommends and which this repo has needed: AS-58 found genuine run-to-run
differences from one unmodified binary. Their subject is run-to-run
determinism, a different question from slot symmetry. See
`docs/design/balance/sweep-tiers.md` for where each belongs.

## What is answerable

The hypothesis has a necessary condition: for a change that both sides carry
to still shift team 1's win rate, **slot 1 must carry an advantage for the
change to amplify**. A constant slot advantage would cancel in a before/after
delta; only an advantage that *scales with unit power* produces the observed
pattern. Either way, if the slot is exactly symmetric there is nothing to
amplify and the observations are multiple comparisons.

Slot symmetry is a ONE-ARM measurement. No before, no after, no change under
test:

**DIAGONAL** — identical comps on both sides (`Warrior+Mage vs Warrior+Mage`).
25 distinct-class 2v2 teams with the double-healer pairs excluded, plus all 8
1v1 self-mirrors, 80 seeds each: **2,640 matches**. No comp-strength confound
is possible, because the comps are the same comp. Under exchangeable slots
team 1 wins exactly half the decisive matches.

**SWAP-CLOSED** — every ordered pair of two *distinct* 2v2 teams from the same
25, 5 seeds each: **3,000 matches over 600 cells**. Each cell's mirror image
is also in the set, so comp strength cancels across the pair and a pooled
team-1 rate off 50% is a slot effect. Lower power per match than the diagonal,
far more cells, and it is the geometry the cards' mirrored slices actually
have. The analysis script *checks* the closure rather than assuming it.

Both pinned to `BasicArena` with the standard 300s cap.

**Conditions, stated because this project requires it — and they do not weaken
the result.** The probe ran at a load average around 57 on an 18-core box,
with two sibling agents building and sweeping; `--jobs 16`, which by this
branch's own guidance was too many. Contention costs wall-clock and nothing
else here: a match's outcome is a function of its seed, comp and map and not
of how the box was scheduled, which is measured rather than assumed —
Tester-AS-99 reproduced a committed sweep byte-identically (520/520 rows,
durations included) from an independently rebuilt binary on a
differently-loaded box. The direction of the risk is worth naming explicitly —
**scheduling noise cannot manufacture symmetry**. The only error it could
introduce is a spurious asymmetry, which is the direction that would have
argued against the null rather than for it. The test is a
two-sided **exact** binomial against 0.5 — exact rather than normal, because
the whole point is a tight null and the tail is the part anyone would argue
with. Draws credit neither slot, so they are excluded from the denominator and
reported separately.

## Results

```
DIAGONAL  (identical comps both sides)
  matches            2640  (2393 decisive, 247 draws, 0 errors)
  team1 wins         1219 of 2393 decisive = 50.94%  [48.94-52.94]
  vs 50%             +0.94pt   exact two-sided p = 0.3684  -> symmetric
  resolution         this n rules out a slot effect above +/-2.00pt
  distinct durations 1339

SWAP-CLOSED  (every ordered pair of distinct comps)
  matches            3000  (2998 decisive, 2 draws, 0 errors)
  team1 wins         1503 of 2998 decisive = 50.13%  [48.34-51.92]
  vs 50%             +0.13pt   exact two-sided p = 0.8983  -> symmetric
  resolution         this n rules out a slot effect above +/-1.79pt
  distinct durations 1573

swap-closure check: 600 cells, 0 without their mirror image

COMBINED: team1 won 2722 of 5391 decisive = 50.49% [49.16-51.83],
          exact two-sided p = 0.4788
```

**The slot is symmetric.** Neither arm separates from 50%, and they agree with
each other. Combined, team 1 won 50.49% of 5,391 decisive matches, p = 0.48.

**Non-vacuity.** 5,391 of 5,640 matches were decided by elimination, with
1,339 and 1,573 distinct durations across the two arms — this is 5,640
different matches, not one match repeated. The draw rates differ sharply and
for a legible reason: **247 draws (9.4%) on the diagonal against 2 (0.07%) on
the swap-closed set.** Every one of those 247 is a self-mirror comp, and they
resolve FAST rather than grinding: 195 of them inside 30 seconds, the largest
single cluster 37 `Mage vs Mage` draws at exactly 11.95s. Nothing in either
arm reached the 300s cap — all 5,640 rows carry `end_reason=kill`, and the
longest match in the probe is 243.02s. These are the dying-blow draws
`CLAUDE.md` describes: two identical comps at the same seed act in lockstep
and land simultaneous mutual-lethal blows, which is a DRAW by design. That is
a sharper check that the diagonal really is mirrored than any attrition
signature would be — a comp ties itself this way only if both sides are
running the same fight tick for tick.

**The closure was checked, not assumed.** All 600 swap-closed cells have
their mirror image present, so comp strength cancels across the set and the
pooled rate is a slot measurement rather than a comp measurement.

**The swap arm's interval errs wide, not narrow.** Of its 1,500 mirror pairs,
**1,367 are comp-determined** — the same comp wins in both orderings, so the
pair contributes exactly one team-1 win and one team-1 loss, with zero
variance. Only **132 pairs are informative**, the ones where the slot-1 side
won both orderings: **68 for team 1 against 64 for team 2**. The pooled 1,503
of 2,998 is algebraically just 1,499 + (68 − 64). Treating all 2,998 matches
as independent Bernoulli trials therefore OVERSTATES the variance, so the
reported ±1.79pt — and the combined ±1.3pt — is *wider* than the run actually
bought. That is the safe direction for a null: the real resolution is tighter
than the one claimed, and the claim is the one being relied on.

## What this does and does not settle

**Settled: there is no team-1 advantage large enough to explain the tally.**
The combined interval rules out a slot effect above **±1.3pt**. The mirrored
gains that prompted the question were **+1.9pt to +2.8pt** — above what this
probe excludes. So slot 1 carries no advantage of that size for a change to
amplify, and the mirrored-slice observations are what multiple comparisons
produce.

That reading is corroborated from the other direction by AS-87's own history:
its significant cell **moved between runs** — Mage mirrored, Mage mirrored,
then Priest clean — across three runs sharing seeds and comps and differing
only in base. A significant cell that moves is the signature of multiple
comparisons, not of an effect.

**Not settled, and out of scope for this probe:**

- **A slot effect below ±1.3pt.** Nothing here excludes one. It would be too
  small to produce the observed rows, which is the question that was asked.
- **An interaction rather than a level.** This measures the *level* of any
  slot advantage at baseline power, and a level probe is weaker against an
  advantage that SCALES with power than against a constant one: a strictly
  conditional effect — one that appears only when both sides get stronger,
  and vanishes at baseline — would not show up here. That is named so nobody
  reads this doc as excluding more than it does, but it is not a live
  hypothesis. It is considerably more contrived than the one tested, no
  evidence points at it, and AS-87's own history explains the rows without
  it. **It does not warrant another sweep; treat the question as closed
  until a row appears that the multiple-comparisons reading cannot hold.**
- **Anything about maps other than `BasicArena`,** or about 3v3. One row in
  the `sweep-tiers.md` tally is 3v3 — AS-86's — and this probe does not reach
  it. It is a null row, and AS-86's *significant* row is the 2v2 Paladin
  slice, which is in scope, so every row the conclusion is asked to explain
  is covered.

**The rule from AS-104 stands unchanged.** If a third *card* lands a
significant mirrored slice, add its row to the tally in
`docs/design/balance/sweep-tiers.md` and open a card. Add the NULL rows too:
the denominator matters as much as the numerator.

## Which claims recompute, and which need a run

| claim | how to check |
|---|---|
| every figure above | `2026-09-20-as104-slot-symmetry.py` over the two committed CSVs |
| the two sweep inputs | `2026-09-20-as104-make-probe.py <dir>`, then diff against the committed JSONL |
| cross-process determinism | needs a run: re-run either JSONL and diff the CSV |

There is no experimental arm to commit: this probe ran one unmodified
binary, and the branch's diff is documentation and Python only — nothing in
it is compiled into the simulator.

### The same-binary control, run as a diagnostic

The diagonal sweep was re-run from the same binary
(`md5 a8990e6d247fa2aef4d6fc37d8417fbd`) at `--jobs 6` on a box at load 6–8,
against the original's `--jobs 16` at load ~57: **2,640/2,640 rows identical
in every field, durations included.**

This was run as the determinism diagnostic `CLAUDE.md` recommends, on this
exact binary and sweep — **not as support for any conclusion above.** The
slot-symmetry result does not rest on it, and would stand unchanged had it
come back dirty (a determinism defect would have been a separate and more
urgent finding, not a reinterpretation of the probe).

It also produced this branch's less-contended throughput figure — 2.05
matches/sec against the loaded run's 1.52, and the per-match kernel cost that
disproved the contention attribution in
`docs/design/balance/sweep-tiers.md`.
