# AS-287: Aimed Shot is uninterruptible

Tier: **directional**. Success condition: **a Hunter gain** (`--expect move`).

## The change

Aimed Shot carries `uninterruptible: true` in `abilities.ron`. No interrupt takes
it: Kick, Pummel, Wind Shear and Spell Lock pass it by unspent and trace the pass
as `Uninterruptible`, and `process_interrupts` cuts and locks nothing if one
reaches it. Loss of control still stops it, with no school lockout. Its cast bar
is grey, and its text says "Cannot be interrupted."

Aimed Shot is the only cast-time ability in the book that TBC makes
uninterruptible. `sweeps/2026-10-12-as287-tbc-interruptibility.py` joins every
cast and channel in `abilities.ron` to its player ranks in the TBC client
(build 2.5.6.69795). Aimed Shot alone has `PreventionType` 2 (Pacify). The other
16, Drain Life's channel included, have 1 (Silence), so an interrupt takes them.
The script exits non-zero if the RON flag ever disagrees with the client data.

## Arms

- **before**: `a94271d` (main).
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
  ALL reachable                      n=8320  49.5% -> 49.4%  delta -0.1pt  flips 380 (+175/-183) z=0.37 ns   resolves >=0.5pt
  CLEAN (only team1 affected)        n=3520  39.7% -> 42.0%  delta +2.4pt  flips 151 (+109/-26) z=7.06 SIG  resolves >=0.7pt
  AGAINST (only team2 affected)      n=3520  59.8% -> 57.1%  delta -2.7pt  flips 153 (+27/-121) z=7.64 SIG  resolves >=0.7pt
  MIRRORED (both sides)              n=1280  48.6% -> 48.8%  delta +0.2pt  flips 76 (+39/-36)   z=0.23 ns   resolves >=1.4pt
NON-VACUITY: 8406/8480 paired matches ended by elimination (74 draws); 1399 moved in winner or duration.
```

**The Hunter gains about 2.5pt.** On team 1 its side wins 2.4pt more often
(109 flips to 26). On team 2 the other side wins 2.7pt less often (121 flips to
27). Both slices clear their floor by a wide margin. The reachable aggregate
nets to zero, because the CLEAN and AGAINST slices are the same size and move in
opposite directions. MIRRORED cells field a Hunter on both sides, so the change
washes out there. This is a direction, not a standing.

## What moved

`sweeps/2026-10-12-as287-interrupt-counts.py` replays every sweep match that
fields a Hunter, 8,320 per arm across both maps, on both binaries with the match
log on. It counts the log's own lines. Per-match counts are in
`sweeps/2026-10-12-as287-interrupt-counts.csv.gz`.

| per arm, 8,320 matches        | before | after |
|-------------------------------|-------:|------:|
| Aimed Shots begun             | 10,914 | 10,925 |
| interrupts on an Aimed Shot   |  1,704 |      0 |
| interrupts on any other cast  |  7,953 |  8,026 |
| Physical school lockouts      |  1,704 |      0 |
| Aimed Shots stopped by CC     |    352 |   461 |

- **Interrupts moved off Aimed Shot: 1,704 to 0.** Before, about one Aimed
  Shot in six was interrupted. Interrupts on other casts rose by only 73, so
  most of the 1,704 were not redirected onto another cast. These counts do not
  say what happened to the unspent cooldowns.
- **Crowd control still stops Aimed Shot.** It stopped 461 casts after, about
  one in 24, up from 352. The counts do not separate why it rose. One candidate
  is that casts an interrupt used to cut now run on into a stun or a fear.
- **Physical lockouts: 1,704 to 0.** Every Physical lockout in these matches
  came from an interrupted Aimed Shot. So the lockout that blocked Disengage and
  Concussive Shot after AS-282 no longer arises.
