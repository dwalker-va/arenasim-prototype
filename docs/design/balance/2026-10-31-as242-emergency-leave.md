# AS-242: the dying Druid's emergency leave from Travel Form

**Tier: DIRECTIONAL.** Paired arms at identical seeds. Nothing here may be cited
as the Druid's standing.

## The change

A shifted Druid below `DRUID_EMERGENCY_HP` (45%) leaves Travel Form as soon as
its emergency heal would go out on its first frame out of the form
(`class_ai::druid::should_leave_form`, `emergency_heal_lands`):

- its global cooldown is ready, steps 2-3 pick a dying ally in reach, and the
  cast gate clears the heal they would cast — Swiftmend when its own
  Rejuvenation is on that ally, else the Rejuvenation — asked of the Druid's
  auras without the form;
- no visible melee enemy or pet is within `DRUID_EMERGENCY_LEAVE_YARDS` (10yd);
- the existing rules still hold it: never with a chaser in striking reach, never
  while the shift rule would fire.

It skips the 3s dwell, the "work to do" test and the danger-radius test (12yd,
for the first 8s) that keep an ordinary leave back. Only the Druid's own
health triggers it: a dying teammate does not pull a healthy Druid out early.
The heal goes out on the next frame, the leave being free and off the global
cooldown; after the leave the shift is held back one global cooldown, as for
every voluntary leave.

## Nagrand Druid panel (mechanism)

The AS-241 panel: the AS-164 Nagrand Druid panel (3,000 configs) under `Legacy`
and again under `TeamPlan` (`sweeps/2026-10-31-as241-panel.py`), through
`tests/milestone_mechanisms.rs`, which now records every leave of the form
(health, nearest melee or pet, seconds to the next shift, the next heal, death)
and the seconds a unit spent shifted and dying. Analysis:
`sweeps/2026-10-31-as242-leave.py`; rows slimmed to the Druid:
`sweeps/2026-10-31-as242-{before,after}.rows.jsonl.gz`.

A STROBE is an unrooted leave followed by a shift back within 3s (the dwell the
form is meant to buy). None comes inside 1.5s in any arm: the re-shift hold sees
to that.

| per 3,000 matches | `Legacy` before | after | `TeamPlan` before | after |
|---|---|---|---|---|
| Druid deaths | 1,512 | 1,504 | 1,492 | 1,496 |
| ...in Travel Form | 358 (23.7%) | 326 (21.7%) | 380 (25.5%) | 363 (24.3%) |
| shifted and dying, s/match | 1.78 | 1.65 | 1.91 | 1.80 |
| unrooted leaves | 2,781 | 2,880 | 2,535 | 2,596 |
| ...strobes | 515 (18.5%) | 525 (18.2%) | 342 (13.5%) | 361 (13.9%) |
| unrooted leaves below 45% | 588 | 654 | 681 | 725 |
| ...strobes | 71 (12.1%) | 74 (11.3%) | 87 (12.8%) | 95 (13.1%) |
| Druid side's wins (McNemar) | 55.2% | 55.3% (+15/-12, z=0.58) | 53.7% | 53.4% (+5/-12, z=-1.70) |

Read with care:

- **Deaths move out of the form; they do not fall.** Shifted deaths fall by 32
  (`Legacy`) and 17 (`TeamPlan`); deaths overall are flat. The heal it leaves
  for is nearly always the Rejuvenation (the Druid rarely carries its own
  Rejuvenation while shifted, so Swiftmend is rarely the one ready), a heal over
  time that does not outpace the damage that put it below 45%.
- **No new strobing as a rate.** Strobes per unrooted leave are within half a
  point of before in both profiles, and per dying leave likewise.
- The McNemar z is inflated by the panel's repeated deterministic situations
  (AS-241's note); the directional sweep below is the win-rate reading.

### The clearance

The clearance was measured at three values, each a full panel arm against the
same `before` (`sweeps/2026-10-31-as242-{reach,after,danger-radius}.rows.jsonl.gz`):

| clearance | `Legacy` shifted deaths | strobes | wins | `TeamPlan` shifted deaths | strobes | wins |
|---|---|---|---|---|---|---|
| before | 358 | 515 (18.5%) | | 380 | 342 (13.5%) | |
| 7.5yd (striking reach) | 330 | 601 (20.4%) | +27/-33 | 330 | 396 (14.8%) | +22/-33 |
| **10yd (shipped)** | 326 | 525 (18.2%) | +15/-12 | 363 | 361 (13.9%) | +5/-12 |
| 12yd (danger radius) | 348 | 516 (18.5%) | +3/-5 | 382 | 344 (13.5%) | +2/-6 |

At striking reach the leave goes with the chaser on the Druid's heels: of the
dying leaves with a chaser 7.5-9yd off, 21% (`Legacy`) and 25% (`TeamPlan`)
were strobes. At the danger radius almost nothing is left for the leave to
answer. 10yd keeps most of the shifted-death drop at the old strobe rate.

## Directional sweep (win rate)

`scripts/gen_sweep.py --full 2 --exclude-double-healer --affects Druid
--control-cells 8 --n 10`, BasicArena, three seed blocks (`--seed-base` 0, 1000,
2000; 2,830 matches each), per profile (`--extra '{"ai_profile": ...}'`);
`scripts/paired_sweep.py --affects Druid --tier directional --expect nothing`.
Control identical in every block (80/80). CLEAN is the Druid on team 1 only;
AGAINST on team 2 only (team 1's win rate, so a negative delta there is the
Druid winning).

| | CLEAN by block | pooled flips, z | AGAINST by block | pooled flips, z |
|---|---|---|---|---|
| `Legacy` | +0.6 / +0.4 / +1.0pt | +58/-33, +2.62 | -1.0 / -1.4 / -1.3pt | +25/-70, -4.62 |
| `TeamPlan` | -0.2 / -0.1 / +0.3pt | +32/-32, 0.00 | -0.1 / -0.1 / -0.4pt | +28/-35, -0.88 |

Under `Legacy` the Druid side gains about 1pt on BasicArena, the same way in
every block and both slices. Under `TeamPlan` nothing measurable. On Nagrand
(the panel above) neither profile moves significantly.

## Not bundled: the anchor mask on the run

A shifted Druid's run is still held inside heal range of its anchor by the
ESCAPE window's anchor mask (`escape_tick`), though it cannot heal while
shifted. The emergency leave does not need it: the heal it leaves for is the
Druid's own. Dropping the mask for the chaser escape alone (`chasers_only`) was
measured as a further arm on top of this change
(`sweeps/2026-10-31-as242-noanchor.rows.jsonl.gz`):

| per 3,000 Nagrand matches | `Legacy` this change | + no anchor | `TeamPlan` this change | + no anchor |
|---|---|---|---|---|
| Druid deaths | 1,504 | 1,461 | 1,496 | 1,483 |
| ...in Travel Form | 326 | 259 | 363 | 327 |
| mean ground gained per escape window | 2.48yd | 2.78yd | 2.77yd | 2.91yd |
| strobes | 525 (18.2%) | 634 (19.5%) | 361 (13.9%) | 404 (14.5%) |
| Druid side's wins | | +80/-70, z=0.82 | | +80/-78, z=0.16 |

On BasicArena (same three blocks): `Legacy` CLEAN +42/-26 (z=1.94), AGAINST
+29/-34; `TeamPlan` CLEAN +35/-50 (z=-1.63), AGAINST +38/-47. It is the larger
mechanism change of the two — six to nine times the flips on the panel, the run
longer, fewer Druid deaths overall — and it moves both profiles on its own, so
it is a separate card rather than part of this one. Recommended next: drop the
anchor mask for the chaser escape, and re-measure the strobe rate it raises.

## What is not settled

- Whether a heal over time is worth leaving the form for. The leave mostly buys
  a Rejuvenation; a Druid that Rejuvenates itself BEFORE it shifts would have
  Swiftmend to leave for. Not tried here.
- Deaths overall did not fall on Nagrand, so the BasicArena `Legacy` gain is
  not explained by the panel's mechanism counts.
