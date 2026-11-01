# AS-245: the Druid's late match — heals give way to damage

**Tier: DIRECTIONAL.** Paired arms at identical seeds. Nothing here may be cited
as the Druid's standing.

## The change

Three rules, all in `class_ai/druid.rs`, under both AI profiles (the Druid's
rotation is shared):

- **Overheal in Attrition** (`attrition_overheal`). From the Attrition onset
  (dampening at half) the focus heals — Rejuvenation and Lifebloom kept on the
  ally the enemy is attacking — skip an ally at `DRUID_FULL_HP` (95%) or above.
  Before Attrition that pre-HoT is the Druid's design and is kept: it lands at
  full value when the damage does. From Attrition on, every heal is worth half
  or less, and the global cooldown is the one a Moonfire needs.
- **Full dampening.** Once dampening reaches 100% a heal heals nothing, so the
  Druid casts none (`DruidTurn::cast`), and a shifted Druid does not leave
  Travel Form to heal (its emergency leave and its "healing needed" work).
- **The duel** (`duel_melee`). A Druid that is its team's last unit, against
  one enemy (pets not counted), that cannot afford a Moonfire closes to melee
  and swings its staff — the posture machine stands aside and the pursuit
  closes to 2yd. A latch: on below one Moonfire's mana, off at two.

## 2v2 sweep

`gen_sweep.py --full 2 --exclude-double-healer --affects Druid
--control-cells 8 --n 4`, three seed blocks (`--seed-base` 0, 1000, 2000),
under `Legacy` and again with `"ai_profile":"TeamPlan"`: 3,396 paired matches
per profile. Control: 96/96 unaffected matches identical in both.

| profile | ALL reachable | CLEAN (team1 Druid) | AGAINST (team2 Druid) |
|---|---|---|---|
| Legacy | +0.5pt, z=2.92 (flips +26/-8) | +1.5pt, z=4.59 | -0.3pt, z=1.33 ns |
| TeamPlan | +0.2pt, z=1.01 ns | +1.1pt, z=3.67 | -0.7pt, z=2.60 |

Read from the Druid's side, the change wins it matches in both slices and
both profiles. Per seed block, Druid-side flips gained/lost: Legacy 16/0, 13/1,
17/0; TeamPlan 16/1, 10/0, 12/1 — the direction holds in every block.

## 1v1

Druid against each class, 10 seeds (BasicArena, `Legacy`):

| Druid vs | before (D / draw / opp) | after |
|---|---|---|
| Druid | 0 / 10 / 0 | 4 / 4 / 2 |
| Shaman | 0 / 10 / 0 | 10 / 0 / 0 |
| Paladin | 0 / 9 / 1 | 10 / 0 / 0 |
| Priest | 0 / 0 / 10 | 10 / 0 / 0 |
| Warlock | 0 / 0 / 10 | 3 / 0 / 7 |

The other classes are unchanged. The duel draws resolve; healer-v-healer 1v1
now goes to the Druid, whose staff is the heaviest melee among the healers.
1v1 is not the balance target.

## Travel Form's mana: measured, not shipped

The card asked whether the mana governor should keep the shift's 25 mana in
reserve. On the Druid 2v2 panel (`Druid+{p}` against every pair, n=3, BasicArena
and PillaredArena, 1,680 Druid-matches, `tests/milestone_mechanisms.rs`), 161 of
563 roots on the Druid were never broken, and 126 of those found it under 25
mana. A floor of the shift's cost on the governor (proactive casts only) moved
none of them: 126/161 in both arms. Every one starts at 0-24 mana, spent there
by the casts the governor does not govern — the emergency heals, a dying
teammate's heals and the peels. Keeping the shift affordable means holding
mana back from a dying teammate's heal, which is a priority call, not a
governor setting. It is left for its own card, with pre-arming Swiftmend.
