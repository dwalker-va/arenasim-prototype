# AS-115: pricing two-handers against the hands they fill

A two-handed weapon fills both hand sockets. The item budget now prices it that
way. Hand items are priced by how they are **held** (`held_budget_multiplier`,
`constants.rs`), not by their slot kind:

| Held | Multiplier | What it means |
|---|---|---|
| Two-hander | 1.0 | One full-budget slot, the same as a Head or Chest. |
| One-hander (either hand) | 0.42 | Classic's own one-hander ratio. |
| Off-hand only (shield, frill) | 0.5625 | Unchanged. |

However the hands are filled, they draw about one full slot: a one-hander plus
an off-hand item is 0.9825, and a two-hander is 1.0
(`a_two_hander_draws_what_a_one_hander_and_off_hand_draw_together`). Dual wield
draws 0.84 and is paid back in the second weapon's damage, which is a free stat.

**A two-hander is priced at its pair, not at its cap.** Each shipped two-hander
spends exactly what the one-hander + off-hand pair it displaces spends, at the
same tier and for the same role. The 1.0 cap sits above every one of them. That
headroom is deliberate, and it is closed: `two_handers_are_priced_at_the_pair_they_displace`
pins each two-hander to its pair, and fails on a two-hander missing from its
table. Adding a two-hander, or raising one, means naming the pair that justifies
the price.

No shipped one-hander spends more than 0.42
(`no_shipped_one_hander_exceeds_the_one_hand_multiplier`, with no tolerance).
Tightening the one-hander cap therefore moved no item's stats.

## Why: the gap was real, and free stats did not close it

Before this change, every hand item drew `item_level x 0.75 x 0.5625`. A
two-hander competed against a pair on half the pair's budget. **The error ran
one way: two-handers were under-priced, never over-priced.**

`used` is `calculate_budget_usage`. `wDPS` is the free stat,
`(min + max) / 2 x attack_speed`, in attacks per second. The primary weapon's
`attack_speed` already replaces the class base in `Combatant::apply_equipment`,
so speed is counted here.

| Two-hander (before) | stats | wDPS | Pair it displaces | stats | wDPS |
|---|---|---|---|---|---|
| Crescent Staff | SP 10, mana 9 (24.0) | inert | Witchblade + Tome | SP 11, mana 16 (32.5) | inert |
| Runestaff of Elements | SP 12, mana 12 (30.0) | inert | Claw + Grimoire | SP 15, mana 22, mp5 0.5 (47.0) | inert |
| Arcanite Reaper | AP 4 (6.0) | 14.40 | Frostbite Blade + Wall of the Dead | AP 5, crit 1%, hp 15 (25.5) + 448 armor | 13.20 |
| Arcanite Reaper | AP 4 (6.0) | 14.40 | Frostbite + Serpent Fang (dual wield) | AP 7, crit 3% (19.5) | 16.77 |
| Bloodlord's Battleaxe | AP 8, crit 2% (18.0) | 18.90 | Stormblade Edge + Bulwark | AP 10, crit 2%, hp 18 (39.0) + 560 armor | 16.50 |

Dual-wield wDPS is `(MH + 0.5 x OH) x (1 - 0.19)`, from `OFFHAND_DAMAGE_MULTIPLIER`
and `DUAL_WIELD_MISS_CHANCE`.

- **For casters the free stat is worth nothing.** Mage, Priest and Warlock
  auto-attack from their `Ranged` socket (`CharacterClass::weapon_slot`), so a
  staff's weapon damage never reaches the sim. The pair beat the staff on every
  stat at both tiers, so the staff was strictly dominated.
- **For melee the free stat does not compensate either.** The shipped
  two-handers carry no damage premium over the one-handers: Serpent Fang
  (15.0) out-DPSes the Arcanite Reaper (14.4), and Fang of the Viper (19.5)
  out-DPSes Bloodlord's Battleaxe (18.9). Melee abilities scale with attack
  power, not weapon damage; Heroic Strike's `0.5 x attack_damage` is the one
  exception. Weapon DPS therefore reaches the sim almost entirely through
  auto-attacks.

## The new prices

| Two-hander | Pair it is priced at | Budget spent / cap | Stats |
|---|---|---|---|
| Arcanite Reaper (ilvl 60) | Frostbite Blade + Wall of the Dead | 25.5 / 45.0 | AP 4 -> **AP 11, crit 3%** |
| Crescent Staff (ilvl 58) | Witchblade + Tome of Knowledge | 32.5 / 43.5 | SP 10, mana 9 -> **SP 15, mana 10** |
| Bloodlord's Battleaxe (ilvl 75) | Stormblade Edge + Bulwark of the Guardian | 39.0 / 56.25 | AP 8, crit 2% -> **AP 16, crit 5%** |
| Runestaff of Elements (ilvl 73) | Claw of Chromaggus + Grimoire of Shadows | 47.0 / 54.75 | SP 12, mana 12 -> **SP 20, mana 17** |
| Peacemaker (ilvl 59; added by AS-195) | Frostbite Blade + Serpent Fang Dagger (a Hunter holds no shield) | 19.5 / 44.25 | new -> **AP 7, crit 3%** |

Each two-hander spends what its pair spends but keeps its own mix, so the
choice is real rather than cosmetic:

- **A melee two-hander spends on offense.** The pair spends part of the same
  points on a shield's health and gets armor free. It is priced against
  sword and shield, not a dual-wield pair. Every dual-wield pair spends less:
  Frostbite + Serpent Fang is 19.5 points, and a Rogue's two Serpent Fangs are
  24.0. A dual-wield pair also recovers value in its second weapon's damage,
  which is a free stat. Sword and shield is the pair a Warrior, Paladin or
  Shaman actually gives up for an axe two-hander. A Rogue cannot wield a
  two-hander at all.
- **A staff buys more spell power and a smaller mana pool than its pair.**

Weapon damage and speed are unchanged. Two-handers still carry no weapon-DPS
premium over one-handers. That is a separate question.

## Measured

Both sweeps are **DIRECTIONAL**
([sweep tiers](sweep-tiers.md)): 2v2, `--full 2 --exclude-double-healer`, 10
seeds per cell, paired at identical seeds, 300s cap. The inputs are in
`sweeps/2026-09-27-as115-*.jsonl` (see `sweeps/README.md`). They show which way the
change moves and roughly how far; neither may be cited as a class's standing.
Nothing was tuned toward parity.

### The Warrior: `main` @ `4c4689c` against this branch

Only the Warrior's default loadout wears a two-hander (the Arcanite Reaper), so
the Warrior is the only class a default-loadout sweep can see move. There were
3,090 matches: 301 reachable cells plus 8 control cells. Command:
`gen_sweep.py --full 2 --exclude-double-healer --affects Warrior --n 10`.

- **Control:** 80/80 matches with no Warrior on either side are identical in
  winner and duration. The control fields all 7 other classes on both sides.
- **Non-vacuity:** 3,070 of 3,090 matches ended by elimination. In 1,125, the
  winner or the duration moved.

| Slice | n | before | after | delta | flips | z |
|---|---|---|---|---|---|---|
| CLEAN (only team 1 has a Warrior) | 1260 | 36.2% | 37.8% | +1.6pt | +34/-14 | 2.74 |
| AGAINST (only team 2 has a Warrior) | 1260 | 63.6% | 61.7% | -1.9pt | +11/-35 | 3.39 |
| MIRRORED | 490 | 48.4% | 46.9% | -1.4pt | +21/-28 | 0.86 |

Win rates are team 1's. The Warrior gains about 1.5-2pt on both one-sided
slices, which agree in direction: CLEAN rises and AGAINST falls. MIRRORED is
noise.

Here is the Warrior's own side, by partner class. Each row is 360 matches in
which only one team fields a Warrior. This is a direction, not a per-cell
figure:

| Warrior + | before | after | delta | flips |
|---|---|---|---|---|
| Hunter | 36.7% | 40.0% | +3.3pt | +12/-0 |
| Mage | 40.0% | 40.8% | +0.8pt | +5/-2 |
| Paladin | 59.4% | 60.8% | +1.4pt | +11/-6 |
| Priest | 39.4% | 43.1% | +3.6pt | +20/-7 |
| Rogue | 26.9% | 26.1% | -0.8pt | +0/-3 |
| Shaman | 34.4% | 35.6% | +1.1pt | +9/-5 |
| Warlock | 16.1% | 18.6% | +2.5pt | +12/-3 |

Pooled over all 2,520 one-sided matches, the flips run +69/-26 in the Warrior's
favour, about +1.7pt.

### Staff against pair: this branch, loadout as the only variable

This sweep has two arms on one binary. In the base arm, every Mage, Priest and
Warlock wears the default Witchblade + Tome of Knowledge. In the staff arm, each
of them wears the Crescent Staff instead: a `team{1,2}_equipment` override of
`MainHand: CrescentStaff`, and the two-hand rule strips the off-hand. There
were 5,520 matches: 544 reachable cells plus 8 control cells. Command:
`gen_sweep.py --full 2 --exclude-double-healer --affects Mage,Priest,Warlock --n 10`.

- **Control:** 80/80 matches with no Mage, Priest or Warlock are identical.
  The control fields all 5 other classes on both sides.
- **Non-vacuity:** 5,484 of 5,520 matches ended by elimination. In 2,683, the
  winner or the duration moved.
- **Success condition:** a real choice, with no direction or parity target
  declared. The sweep was run without `--expect`.

| Slice | n | pair arm | staff arm | delta | flips | z |
|---|---|---|---|---|---|---|
| CLEAN (only team 1 has a caster) | 1440 | 54.0% | 55.8% | +1.7pt | +59/-34 | 2.49 |
| AGAINST (only team 2 has a caster) | 1440 | 44.7% | 43.3% | -1.5pt | +33/-54 | 2.14 |
| MIRRORED | 2560 | 49.7% | 48.9% | -0.9pt | +86/-108 | 1.51 |

Win rates are team 1's. The staff now edges the pair by about 1.5pt, and the
two one-sided slices agree in direction. Before this change, the staff lost to
the pair on every stat.

Here is the caster's own side, by caster class. Only one team fields a caster in
these matches. A Mage+Priest or Mage+Warlock team counts under both of its
classes:

| Staff on | n | pair arm | staff arm | delta | flips |
|---|---|---|---|---|---|
| Mage | 1260 | 69.9% | 72.5% | +2.6pt | +57/-24 |
| Priest | 900 | 52.4% | 53.8% | +1.3pt | +45/-33 |
| Warlock | 1260 | 41.8% | 42.3% | +0.5pt | +34/-28 |

At an equal price, spell power over pool favours the burst caster most. The
Mage gains the most (z=3.7), and the Warlock is within noise. This is the
direction at directional size, not a tuning target: the card prices the staff
by budget and leaves the stat mix to be a choice.

## Baselines this invalidates

- **`tests/determinism_pin.rs`, 2v2 cell** (Mage+Priest vs Warrior+Priest
  @424242): re-recorded, from `Some(2)` @ 49.38s to `Some(1)` @ 40.70s. The 1v1
  cell did not move.
- **`tests/baselines/`:** the new file is `legacy_behaviour_2026-09-28_two_hander_budget.txt`.
  17 of 27 cells moved, all of them with a Warrior; the README says why the
  18th Warrior cell did not.
- **Seed-pinned movement probes whose comp has a Warrior** (`tests/movement_probes.rs`)
  were re-pinned from their own scanners. Three had gone vacuous: the Mage
  occlusion seeds 9 and 77 (now 31 and 30), and completion-fizzle seed 26.
  Fizzle seed 9 still clears its floor, but fell from 47 fizzles to 1, so the
  pair was re-pinned to 23 and 31. Medic-chase seed 9 kept its short occluded windows,
  but its first visible heal slid to 21s; it is now 14. The chase bound itself
  holds on all 30 scanned seeds on both binaries.
- **Every balance sweep with a Warrior in it that was recorded before this
  change**, including every 2v2 and 3v3 baseline CSV in this directory that
  fields one. Win rates for comps without a Warrior are unaffected on default
  loadouts: the control above is identical.
