# AS-293: interrupts need sight — findings

**Tier: DIRECTIONAL.** Every 2v2 cell an interrupter reaches
(`--affects Rogue,Warrior,Shaman,Warlock`, 851 reachable + 8 control, n=10,
Legacy), paired at identical seeds, per map. Base `380af5f`, after = this
branch. Nothing here may be cited as a class's standing.

## The change

Kick, Pummel, Wind Shear (`combat_ai::check_interrupts`) and the Felhunter's
Spell Lock choose their target through `class_ai::cast_reach` — range, then
sight — in `class_ai::interrupts`. A caster scan ranks only the casters it
reaches, so one behind a pillar yields to one in sight; a cast refused for
sight alone is traced `LosBlocked`. `process_interrupts` refuses an interrupt
that reaches a caster its interrupter cannot see (spent, cutting nothing). On
every map in the sweep that refusal never fires: nothing moves between the
decision and the resolution, so the decision gate already keeps a blind
interrupt from being queued.

## What moved

Interrupts cast, counted by an instrumented build of each arm
(`sweeps/2026-10-12-as293-interrupt-sight-counter.py`; stderr only, and each
instrumented arm's CSV is byte-identical to its clean arm's on all three maps;
counts in `sweeps/2026-10-12-as293-<map>-<arm>-interrupts.txt`):

| map | arm | interrupts | through a pillar | Wind Shear / Spell Lock through a pillar |
|---|---|---:|---:|---|
| BasicArena | before | 14,899 | 0 | 0 / 0 |
| BasicArena | after | 14,899 | 0 | 0 / 0 |
| PillaredArena | before | 15,128 | **19** | 14 / 5 |
| PillaredArena | after | 15,112 | 0 | 0 / 0 |
| TwinPillars | before | 16,416 | **2,229** | 1,238 / 991 |
| TwinPillars | after | 15,530 | 0 | 0 / 0 |

On TwinPillars one Wind Shear in three, and nearly one Spell Lock in four, was
cast through a pillar. No Kick or Pummel ever was, on any map: at 2.5yd a
pillar almost never fits between two bodies. After the fix the Felhunter
still casts about as many Spell Locks (4,427 → 4,247), finding a caster it can
see; Wind Shear drops by a fifth (3,719 → 2,951).

## Win rate

`scripts/paired_sweep.py --affects Rogue,Warrior,Shaman,Warlock --tier
directional`:

- **BasicArena**: before and after CSVs **byte-identical** over 8,590 matches
  (8,496 kills, 94 draws, 3,219 distinct durations; 14,899 interrupts in each
  arm, every one in sight). Also byte-identical against `e1e97de`, the base
  this branch was cut from.
- **PillaredArena**: control 80/80 identical; 15 matches moved in winner or
  duration, 2 flipped. Nothing to resolve.
- **TwinPillars**: control 80/80 identical; 1,661 moved in winner or duration,
  496 flipped. AGAINST +2.8pt for team 1 (z=4.47), CLEAN −0.5pt (z=1.06, ns);
  one significant slice of three, and it says the interrupting side lost.

Scored from the interrupter's side (`sweeps/2026-10-12-as293-oriented.py`):
"any interrupter" is the clean slice, exactly one side fields any of the four;
the per-class rows overlap and may face another interrupter, so they say where
the effect sits, not a class's standing.

| map | slice | n | before | after | delta | moved better / worse | exact sign p |
|---|---|---:|---:|---:|---:|---|---:|
| TwinPillars | any interrupter | 3,220 | 49.5% | 47.9% | −1.58pt | 47 / 99 | <0.001 |
| TwinPillars | Shaman | 2,500 | 58.3% | 56.0% | −2.28pt | 94 / 153 | <0.001 |
| TwinPillars | Rogue | 3,520 | 63.7% | 63.0% | −0.67pt | 47 / 70 | 0.042 |
| TwinPillars | Warrior | 3,520 | 37.8% | 37.0% | −0.85pt | 65 / 94 | 0.026 |
| TwinPillars | Warlock | 3,520 | 37.6% | 37.4% | −0.11pt | 128 / 135 | 0.711 |
| PillaredArena | any interrupter | 3,220 | 48.3% | 48.4% | +0.03pt | 1 / 0 | 1.000 |

**Verdict: the defect is gone (2,229 → 0 interrupts through a pillar on
TwinPillars, 19 → 0 on PillaredArena), and on TwinPillars the interrupting
side pays for it: about 1.6pt in the clean slice, most of it the Shaman's.**
Its Wind Shear was the most-blinded interrupt. The Warlock moves least
although its Spell Lock was blinded nearly as often; the Felhunter's cast
count barely drops, which reads as it mostly finding another caster it can
see. The Rogue and the Warrior move only because their partners and opponents
do. Nagrand's pillars rarely stand between an interrupter and a caster in
range, and its result does not move.

## Probes this moved

The fix moves TwinPillars matches with a Shaman or a Warrior in them, and five
probes pinned to such matches moved with them:

- `tests/casting_mana_juke_probe.rs` asserts that the Mage's mana never
  collapses in the lone-Shaman 2v1 where the Shaman jukes its Frostbolts. The
  fix moved seed 20 into that bank: its 2v1 now runs 45s and holds nine jukes,
  where before it ran 10s and held none. The Mage reaches 0.004 mana there, but
  on Frostbolts that LANDED, before the first juke (its last lands at 65s; it
  wands, dry, until the first juke at 84s). The probe now measures from the
  first juke, the casts the mana fix is about (seed 20's floor from there:
  0.059). Its original mutant, a fizzled Frostbolt charging mana, still fails
  it.
- `movement_probes`' lone-Shaman chase, medic chase and press endgame guard
  were pinned to seeds (re-picked five, six and four times before). Each now
  judges every match of a seed bank that holds its situation, with a floor on
  how many do and on how deep the deepest is. The chase and medic banks fail
  with their mechanism disabled (`seek_chase_timeout` 0.0; the medic walk
  removed); the chase pins on `main` fail the same way.
- The juke probe's bound becomes a rate per minute of the 2v1: the fix lets
  seed 27's lone Shaman heal through a 98s dance (it ran 29s), holding 17
  windows at 9.4 a minute against a bank that never exceeds 11. Neither the
  count nor the rate catches a continuous-clock chase in this bank, measured
  on both sides of the fix.
- `victory_choreography`'s ring-slot-inside-a-pillar match stopped putting a
  slot inside a pillar. That situation is rare after the fix (1 of 33
  three-winner matches across six comps), so the probe stays on one match,
  now chosen by the situation (`slots_inside_a_pillar`) and failing loudly
  when the match stops holding it; `scan_pillar_slots` re-finds one.

## Reproduce

Inputs, CSVs, counts and scripts are under `sweeps/` (see its README). The
counts need the two instrumented arms: run the counter script against a
checkout of each arm, build it, run the batch with stderr captured, and
`grep 'AS293 INT' arm.stderr | sort | uniq -c`.
