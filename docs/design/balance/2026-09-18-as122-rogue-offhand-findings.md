# AS-122 — what arming the Rogue's off hand actually costs

Measured 2026-09-18/19 on `card/AS-122-rogue-dual-wield`. Four raw per-match
CSVs sit beside this file; every number below is recomputable from them with
`scripts/agg_sweep.py`.

AS-60 built the dual-wield machinery and shipped it inert, then measured that
opting a Rogue in was worth +10.8pt
(`2026-09-18-as60-dual-wield-findings.md`). This card opts the Rogue in for
real — one line in `loadouts.ron` — and measures the thing that ships, which
is **not** the same quantity.

## Design of the measurement

**One binary, one variable.** Both arms of every comparison are the same
release binary at the same seeds; the only difference is the Rogue's `OffHand`
entry in `assets/config/loadouts.ron`, which the binary reads at runtime. The
harness records the binary's md5 before the first arm and after the last and
refuses to continue if it moved, so "same binary" is checked rather than
assumed.

**Not against the canonical baselines**, which AS-96 records as stale. Nothing
here is compared to a recorded baseline.

**Measured at `f9c4bf8`.** Main has moved on since (PR #202, the UI
input-injection driver), but that change lives in `src/ui/driver/` behind
`UiDriverPlugin`, which only `build_graphical_app` registers — it adds nothing
to the headless path, so the data describes the sim that ships. The SHAs
differ; the simulation does not.

**The two bases this was run on agree byte-for-byte.** The whole four-arm
sweep was run twice, once at `954910e` and once after rebasing onto `f9c4bf8`,
because the intervening merge (#201) touched `class_ai/hunter.rs`. All four
CSVs came back with identical md5s, so that merge's instrumentation-only claim
holds across 10,000 matches and 25 class pairings — and this measurement has a
same-seed control spanning two commits rather than one.

Tier: **directional**. 25 cells (every distinct opposing pair, double-healer
excluded) × n=100 × 2 arms, 300s cap, `--seed-base 0`, so match *i* of a cell
uses seed *i* in both arms. That buys ±1.9pt on an overall and ±9.7pt on a
single cell: it resolves the overall and separates effects of about 4pt or
more, but it ranks individual cells only coarsely. Read a single cell's delta
as a direction, not a value.

**Why the off-hand item is a second copy of the main hand.** It isolates the
mechanic from an item upgrade, so the figure is the conservative one. A
tier-1 off-hand would land higher. The hands are not a unique-equipped pair —
`unique_equip_pairs()` derives from `ItemSlotType::sockets()` and yields only
rings and trinkets — so this needed no new item in `items.ron`.

## Non-vacuity

**All 10,000 matches ended in a KILL. Zero timeouts.** Distinct durations per
arm: 1329 / 1345 / 1520 / 1524. Paired outcome flips at identical seeds: 450
of 2500 (18.0%) across the Rogue arms, 66 of 2500 (2.6%) across the Hunter
arms.

**The off-hand swing is observed firing, not inferred from the win rate.** Over
24 seeded Rogue matches, one binary, the RON the only variable:

| | swings/sec | same-tick swing pairs |
|---|---|---|
| empty off hand | 0.525 | 0 |
| second dagger | 0.835 | 374 |

1.59× the swing rate, against a predicted 2 × (1 − 0.19 dual-wield miss) =
1.62×. The same-tick pairs are the mechanism's fingerprint: a single weapon
cannot land two autos on one tick, so 0 → 374 attributes the change
positively rather than by elimination. The match log's `[EQUIPMENT]` line
names `Off Hand=Serpent Fang Dagger` directly.

A second consequence worth knowing, because it is part of the number: the
Crippling Poison proc site sits in the damage-application loop and fires for
every landed swing, so arming the off hand roughly doubles the Rogue's poison
application rate as well as its white damage.

## Result

Rogue+Priest vs every distinct opposing pair:

| arm | overall | CSV |
|---|---|---|
| empty off hand | 59.9% [57.9–61.8] | `...-as122-rogue-control-n100.csv` |
| second dagger (ships) | **65.0% [63.1–66.9]** | `...-as122-rogue-offhand-n100.csv` |

**+5.1pt, non-overlapping intervals.** Six of 25 cells moved beyond noise, all
upward: vs Mage+Priest 14→44, vs Mage+Shaman 9→35, vs Priest+Warlock 79→100,
vs Priest+Hunter 23→42, vs Warrior+Rogue 12→28, vs Warrior+Mage 84→98.

**+5.1pt is the number to quote for what this change cost.** It is roughly
half AS-60's +10.8pt, and the reason is visible in the split below rather than
inferred.

### Why it is smaller than AS-60's figure

AS-60 armed **team 1 only**, via a `team1_equipment` override. That is the
right way to measure a mechanic, but it means its seven enemy-Rogue cells
pitted a dual-wielding Rogue against a single-wielding one. Arming the socket
in `loadouts.ron` gives it to **every** Rogue, so those cells largely wash:

| slice | cells | control | after | delta |
|---|---|---|---|---|
| enemy has no Rogue | 18 | 70.5% [68.4–72.6] | 77.9% [75.9–79.7] | **+7.4pt**, non-overlapping |
| enemy has a Rogue | 7 | 32.6% [29.2–36.1] | 32.0% [28.7–35.5] | **−0.6pt**, fully overlapping |

The gain appears exactly where the buff is one-sided and vanishes exactly
where both sides get it. That is the mechanism showing itself in the data, and
it is why the overall sits between the two.

Within the both-armed slice the cells are noisier than their mean suggests
(+16, +3, 0, 0, −2, −10, −11); at ±9.7pt per cell only Warrior+Rogue's +16 is
interesting, and one cell out of seven at that width is not something to read
into.

## The Hunter's off-hand socket, priced

A Hunter's live weapon socket is the Ranged one, so its main hand does not
swing and an off-hand weapon arms no second swing — it is pure stats, and
`is_dual_wielding()` is false for it, so it does not even pay the miss
penalty. AS-60 flagged that as a live affordance. Both arms below sit in the
**shipped** world (the Rogue's off hand armed), so the only variable is the
Hunter's own override.

| arm | overall |
|---|---|
| empty off hand | 41.2% [39.2–43.1] |
| Serpent Fang Dagger | 42.8% [40.9–44.8] |

**+1.6pt, intervals overlapping, and not one of the 25 cells moved beyond
noise.** Cheap, not free, but on this evidence I would not call it a real
effect. A free stat stick in an otherwise dead socket is worth less than the
budget-asymmetry argument would suggest; if it is ever worth closing, close it
because the socket is incoherent, not because it is a balance problem.

## The change is confined to the Rogue, and that is checked

`scripts/behaviour_baseline.sh` runs 27 seeded matches over three comps and
three maps, digesting each whole match log. Only its `pet_comp` cells field a
Rogue (`Hunter,Shaman` vs `Rogue,Priest`); `healer_v_healer` and
`ranged_v_melee` contain none. Run with the same binary and only the RON
switched:

| comparison | cells differing |
|---|---|
| control vs armed off hand | **9 of 27 — every one a `pet_comp` cell** |

**No cell without a Rogue in it moved.** That is the change's blast radius
measured rather than asserted, and it is the reason `determinism_pin` (whose
two cells are Mage/Warrior/Priest) still passes untouched.

**A separate finding, not caused by this card:** the committed baseline
`tests/baselines/legacy_behaviour_2026-09-13_frost_armor_chill.txt` differs
from a fresh run of an UNMODIFIED build at this HEAD in **all 27 cells**,
including the 18 that contain no Rogue. That drift predates this branch and
belongs to changes merged since 2026-09-13. It is deliberately NOT regenerated
here: re-recording it on this card would bake eighteen cells of somebody
else's undocumented drift into a file that says AS-122 blessed it. The split
above is recorded so whoever does regenerate it knows which nine cells are
this card's.

## What this means

The Rogue is now stronger by a measured, deliberate 5.1 points, and this
change was made knowing that. Per the standing direction, the machinery for
assessing balance is the deliverable rather than parity itself: nothing here
is tuned to land at 50%, and the Rogue's off hand was not weakened to hide the
gain. The figure is recorded so the next retune starts from a known position
instead of a guess.

## Reproducing

From a release build of this branch:

```bash
cargo build --release

# The Rogue arms differ ONLY by assets/config/loadouts.ron: run once with the
# Rogue's `OffHand: SerpentFangDagger` line removed, once with it present.
scripts/gen_sweep.py --t1 'Rogue+Priest' --t2-size 2 --n 100 --seed-base 0 \
  --exclude-double-healer > /tmp/rogue.jsonl

# Hunter arms: both in the shipped world, off-hand override the only variable.
scripts/gen_sweep.py --t1 'Hunter+Priest' --t2-size 2 --n 100 --seed-base 0 \
  --exclude-double-healer > /tmp/hunter_control.jsonl
scripts/gen_sweep.py --t1 'Hunter+Priest' --t2-size 2 --n 100 --seed-base 0 \
  --exclude-double-healer \
  --extra '{"team1_equipment":[{"OffHand":"SerpentFangDagger"},{}]}' \
  > /tmp/hunter_offhand.jsonl

./target/release/arenasim --batch /tmp/<arm>.jsonl --out /tmp/<arm>.csv --jobs 16

scripts/agg_sweep.py rogue_offhand.csv --compare rogue_control.csv
scripts/agg_sweep.py rogue_offhand.csv --exclude 'vs_.*Rogue' --overall-only
scripts/agg_sweep.py rogue_offhand.csv --include 'vs_.*Rogue' --overall-only
scripts/agg_sweep.py hunter_offhand.csv --compare hunter_control.csv
```
