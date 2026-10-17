# AS-291: a silence stops only Silence-prevented abilities

Tier: **directional**. Success condition: **nothing moves** (`--expect nothing`),
for the reason given under *Why nothing moves*.

## The change

Every ability in `abilities.ron` now declares `prevention`. This is TBC's
`SpellCategories.PreventionType`, and it replaces AS-287's `uninterruptible`
flag. A silence stops an ability only when its prevention is `Silence` and it
costs mana. That holds whether the ability is about to be cast (the AI's
`pre_cast_ok`) or is already being cast (`process_casting`) or channelled
(`process_channeling`).
An interrupt still takes only a `Silence` cast bar.

The TBC client gives these mana-costing abilities a prevention other than
Silence, so a silence no longer stops them:

- Aimed Shot, Arcane Shot, Concussive Shot, Serpent Sting and Disengage are
  `Pacify`;
- Travel Form is `None`.

The Warrior's and Rogue's kits are mostly `Pacify` too. Silence never applied
to them, so nothing about them changes. Nothing in the sim pacifies.

`sweeps/2026-10-12-as287-tbc-interruptibility.py` now checks every ability's
value against the TBC client, build 2.5.6.69795, and prints `HELD`. Four
abilities have no TBC spell under their own name, so the script names the
spells it reads them from instead:

| ability | value | read from |
|---|---|---|
| Spider Web | Silence | 4167 "Web", TBC client |
| Boar Charge | Pacify | "Charge" on the Pet - Boar line, TBC client |
| Master's Call | None | 53271, Wrath Classic 3.80.2.70315 (no row) |
| Wind Shear | Silence | 57994, Wrath Classic 3.80.2.70315 |

Master's Call is read from 53271, the spell the Hunter casts, not from the two
effect spells it triggers (54216 on the pet, 62305 on the ally), which both
read `Pacify`. `PreventionType` gates the spell a caster BEGINS: the server
checks it with the caster's other auras when a cast starts, and a spell
triggered by another spell's effect skips those checks. So 53271's value is the
one that says whether a silenced or pacified Hunter can use Master's Call.

## Arms

- **before**: `a2448a85`, which is main.
- **after**: the PR head.

The batch is `sweeps/2026-10-16-as291-directional.jsonl.gz`, generated with
`gen_sweep.py --full 2 --exclude-double-healer --affects Hunter,Druid
--control-cells 8 --n 10`. That gives 576 reachable cells and 8 control cells,
5,840 matches per arm, on BasicArena under `Legacy`. The results are in
`sweeps/2026-10-16-as291-{before,after}.csv.gz`.

## Result

```
CONTROL: 80/80 matches with no affected class on either side are IDENTICAL in winner and duration.
  ALL reachable                      n=5760  49.2% -> 49.2%  delta +0.0pt  flips 0
  CLEAN (only team1 affected)        n=2160  46.5% -> 46.5%  delta +0.0pt  flips 0
  AGAINST (only team2 affected)      n=2160  51.9% -> 51.9%  delta +0.0pt  flips 0
  MIRRORED (both sides)              n=1440  49.4% -> 49.4%  delta +0.0pt  flips 0
NON-VACUITY: 5787/5840 paired matches ended by elimination (53 draws); 0 moved in winner or duration; distinct durations 2641 before / 2641 after.
```

The two CSVs are byte-identical.

## Why nothing moves

The only silence in the sim is the Unstable Affliction backlash
(`effects/backlash.rs`). It lands on whoever dispelled the UA. Neither class
whose rule changed can be that dispeller:

- The Druid has no dispel.
- The Hunter's only friendly removal is its pet's Master's Call
  (`class_ai::ally_removal`). Master's Call takes only roots and snares
  (`MASTERS_CALL_IMPAIRMENTS`), and UA is neither.

So no match puts a silence on a Hunter or a Druid, and the new rule has no
moment at which to act. The prediction is that nothing moves, and the
byte-identical arms confirm it. They also show that the refactor from a flag
to an enum moved nothing for the other seven classes.

The change takes effect as soon as anything can silence a Hunter or a Druid,
such as a Silence spell or a silencing Spell Lock. Its behaviour is pinned by
staged tests (`tests/silence_prevention.rs`), not by this sweep.
