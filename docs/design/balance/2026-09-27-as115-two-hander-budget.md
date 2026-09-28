# AS-115: what a two-hander is priced at, measured

Every hand item in `items.ron` draws `item_level x 0.75 x 0.5625`
(`slot_budget_multiplier`, `constants.rs`): `MainHand` and `OffHand` are both
0.5625, and `two_handed: true` does not enter the budget. A two-hander fills
both hand sockets on the budget of one.

## The shipped hand items

`used` is `calculate_budget_usage` (the budget test's own weights); `wDPS` is
the free stat, `(min + max) / 2 x attack_speed` (attacks per second — the
primary weapon's `attack_speed` already replaces the class base in
`Combatant::apply_equipment`, so speed is counted here, not deferred).

| Item | ilvl | Held | used / budget | armor | wDPS |
|---|---|---|---|---|---|
| Arcanite Reaper | 60 | 2H | 6.0 / 25.31 (24%) | — | 14.40 |
| Crescent Staff | 58 | 2H | 24.0 / 24.47 (98%) | — | 5.60 |
| Bloodlord's Battleaxe | 75 | 2H | 18.0 / 31.64 (57%) | — | 18.90 |
| Runestaff of Elements | 73 | 2H | 30.0 / 30.80 (97%) | — | 7.70 |
| Frostbite Blade | 58 | 1H | 7.5 / 24.47 | — | 13.20 |
| Serpent Fang Dagger | 58 | 1H | 12.0 / 24.47 | — | 15.00 |
| Hammer of the Righteous | 58 | 1H | 9.0 / 24.47 | — | 12.50 |
| Witchblade | 58 | 1H | 13.5 / 24.47 | — | 13.50 |
| Stormblade Edge | 73 | 1H | 15.0 / 30.80 | — | 16.50 |
| Fang of the Viper | 73 | 1H | 18.0 / 30.80 | — | 19.50 |
| Claw of Chromaggus | 73 | 1H | 21.0 / 30.80 | — | 17.25 |
| Tome of Knowledge | 55 | OH | 19.0 / 23.20 | — | — |
| Wall of the Dead | 56 | OH | 18.0 / 23.62 | 448 | — |
| Grimoire of Shadows | 70 | OH | 26.0 / 29.53 | — | — |
| Bulwark of the Guardian | 71 | OH | 24.0 / 29.95 | 560 | — |

## Two-hander against the pair it displaces

| Two-hander | stats | wDPS | Pair | stats | wDPS |
|---|---|---|---|---|---|
| Crescent Staff | SP 10, mana 9 (24.0) | inert | Witchblade + Tome | SP 11, mana 16 (32.5) | inert |
| Runestaff | SP 12, mana 12 (30.0) | inert | Claw + Grimoire | SP 15, mana 22, mp5 0.5 (47.0) | inert |
| Arcanite Reaper | AP 4 (6.0) | 14.40 | Frostbite + Wall of the Dead | AP 5, crit 1%, hp 15 (25.5) + 448 armor | 13.20 |
| Arcanite Reaper | AP 4 (6.0) | 14.40 | Frostbite + Serpent Fang (dual wield) | AP 7, crit 3% (19.5) | 16.77 |
| Bloodlord's Battleaxe | AP 8, crit 2% (18.0) | 18.90 | Stormblade + Bulwark | AP 10, crit 2%, hp 18 (39.0) + 560 armor | 16.50 |

Dual-wield wDPS is `(MH + 0.5 x OH) x (1 - 0.19)` — `OFFHAND_DAMAGE_MULTIPLIER`
and `DUAL_WIELD_MISS_CHANCE`.

## The gap is real once free stats are counted

**Direction: two-handers are under-priced — they carry less, never more.**

- **Casters: the free stat is worth nothing.** Mage, Priest and Warlock swing
  their `Ranged` socket (`CharacterClass::weapon_slot`), so a staff's weapon
  damage never reaches the sim. The pair beats the staff on every stat at both
  tiers; the staff is strictly dominated, which is the fake choice AS-87 set
  out to remove. (The Shaman's live socket is `MainHand`, but a staff's 5.6-7.7
  wDPS is below every one-hander's.)
- **Melee: the free stat does not compensate either.** Weapon damage is the
  only place a two-hander could make up the difference, and the shipped
  two-handers carry no damage premium: Serpent Fang (1H, 15.0) out-DPSes the
  Arcanite Reaper (14.4), Fang of the Viper (1H, 19.5) out-DPSes Bloodlord's
  Battleaxe (18.9). Against sword-and-board the Reaper buys +1.2 wDPS for
  -19.5 budget points and -448 armor; against dual wield it loses on both axes.
  Melee abilities scale with AP, not weapon damage (Heroic Strike's
  `0.5 x attack_damage` is the one exception), so wDPS reaches the sim only
  through auto-attacks.
- **Part of the gap is authorship, not the multiplier.** The Reaper spends 24%
  of even its halved budget. The budget is a ceiling only — nothing prices an
  item UP — so a corrected multiplier moves no stat by itself; only re-pricing
  the two-handers does, and that is the balance change to measure.
