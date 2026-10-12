# AS-287: Aimed Shot is uninterruptible

Tier: **directional**. Success condition: **a Hunter gain** (`--expect move`).

## The change

Aimed Shot carries `uninterruptible: true` in `abilities.ron`. No interrupt takes
it: Kick, Pummel, Wind Shear and Spell Lock pass it by unspent and trace the pass
as `Uninterruptible`, and `process_interrupts` cuts and locks nothing if one
reaches it. Loss of control still stops it, with no school lockout. Its cast bar
is grey, and its text says "Cannot be interrupted."

The Hunter's own model of the cast follows the same rule. `aimed_shot_has_time`
asks whether any enemy could stop a cast begun now. It counts an enemy's
interrupt reach only while Aimed Shot `can_be_interrupted`, which it cannot as
shipped. So a Felhunter or a Shaman 30yd away no longer holds the cast. A melee
closing into the dead zone still does, and so does a hidden enemy, because Cheap
Shot stops any cast. No other class AI models whether its own cast can be
interrupted. `interrupt_reach` has no other caller.

Aimed Shot is the only cast-time ability in the book that TBC makes
uninterruptible. `sweeps/2026-10-12-as287-tbc-interruptibility.py` joins every
cast and channel in `abilities.ron` to its player ranks in the TBC client
(build 2.5.6.69795). Aimed Shot alone has `PreventionType` 2 (Pacify). The other
16, Drain Life's channel included, have 1 (Silence), so an interrupt takes them.
The script exits non-zero if the RON flag ever disagrees with the client data.

## Arms

- **before**: `bd20faa` (main). Its batch CSV is byte-identical to one run at
  `a94271d`. The Peel play that merged between them is gated under `TeamPlan`,
  and this sweep is all `Legacy`.
- **after**: the PR head.

Batch: `sweeps/2026-10-12-as287-directional.jsonl.gz`. It is two `gen_sweep.py
--full 2 --exclude-double-healer --affects Hunter --control-cells 8 --n 10`
runs, one with `--extra '{"map":"BasicArena"}' --label-suffix BasicArena` and
one with PillaredArena. That is 416 reachable cells and 8 control cells per map,
8,480 matches per arm, all `Legacy`. CSVs:
`sweeps/2026-10-12-as287-{before,after}.csv.gz`.

## Result

`paired_sweep.py --affects Hunter --tier directional --expect move`:

```
CONTROL: 160/160 matches with no affected class on either side are IDENTICAL in winner and duration.
  ALL reachable                      n=8320  49.5% -> 49.4%  delta -0.1pt  flips 416 (+191/-203) z=0.55 ns   resolves >=0.5pt
  CLEAN (only team1 affected)        n=3520  39.7% -> 42.2%  delta +2.6pt  flips 161 (+118/-27) z=7.47 SIG  resolves >=0.7pt
  AGAINST (only team2 affected)      n=3520  59.8% -> 56.8%  delta -3.0pt  flips 171 (+30/-136) z=8.15 SIG  resolves >=0.7pt
  MIRRORED (both sides)              n=1280  48.6% -> 48.8%  delta +0.2pt  flips 84 (+43/-40)   z=0.22 ns   resolves >=1.5pt
NON-VACUITY: 8406/8480 paired matches ended by elimination (74 draws); 1594 moved in winner or duration.
```

**The Hunter gains about 2.8pt.**

- On team 1 its side wins 2.6pt more often, with 118 flips its way and 27
  against.
- On team 2 the other side wins 3.0pt less often, with 136 flips the Hunter's
  way and 30 against.
- Both slices clear their floor by a wide margin.
- The reachable aggregate nets to zero, because the CLEAN and AGAINST slices are
  the same size and move in opposite directions. MIRRORED cells field a Hunter
  on both sides, so the change washes out there.

This is a direction, not a standing.

The timing fix accounts for a small part of the gain. Against a version of the
change without it, it moved 328 matches, mostly in duration. Its own flips were
10 to 2 on CLEAN and 15 to 3 on AGAINST, both in the Hunter's favour.

## What moved

`sweeps/2026-10-12-as287-interrupt-counts.py` replays every sweep match that
fields a Hunter, 8,320 per arm across both maps, on both binaries with the match
log on. It counts the log's own lines. Per-match counts are in
`sweeps/2026-10-12-as287-interrupt-counts.csv.gz`.

| per arm, 8,320 matches        | before | after |
|-------------------------------|-------:|------:|
| Aimed Shots begun             | 10,914 | 10,996 |
| interrupts on an Aimed Shot   |  1,704 |      0 |
| interrupts on any other cast  |  7,953 |  8,045 |
| Physical school lockouts      |  1,704 |      0 |
| Aimed Shots stopped by CC     |    352 |   459 |

- **Interrupts moved off Aimed Shot: 1,704 to 0.** Before, about one Aimed Shot
  in six was interrupted. Interrupts on other casts rose by only 92, so most of
  the 1,704 were not redirected onto another cast. These counts do not say what
  happened to the unspent cooldowns.
- **Crowd control still stops Aimed Shot.** It stopped 459 casts after, about
  one in 24, up from 352. The counts do not separate why it rose. One candidate
  is that casts an interrupt used to cut now run on into a stun or a fear.
- **Physical lockouts: 1,704 to 0.** Every Physical lockout in these matches
  came from an interrupted Aimed Shot. So the lockout that blocked Disengage and
  Concussive Shot after AS-282 no longer arises.
- **More Aimed Shots are begun:** 10,996 against 10,914. A version of the
  change without the timing fix began 10,925, so 71 of the 82 extra casts
  arrived with it. That fits the Hunter no longer holding the cast for a
  Felhunter or Shaman at range, though the counts alone do not prove it.
