# AS-167: every weapon swings at its own Classic speed

Card AS-167. Every weapon now takes its swing speed from its item, and that
speed is a real Classic item's. Weapon DPS holds for every one-hander, wand
and bow. Two-handers gain Classic's two-hander premium. Warrior rage per swing
and the per-swing procs are rescaled so that slower swings do not quietly
change them.

## Where it stood

The equipped item was already the source of each hand's speed. Since AS-60 and
AS-122, `Combatant::apply_equipment` has copied the live socket's
`attack_speed`, and the off-hand weapon's, into the combatant, on both the
graphical and headless spawn paths. The card's premise, that only the Serpent
Fang pair's speed was read, was wrong. The only weapon speeds nothing read were
the casters' main hands: a Mage, Priest or Warlock auto-attacks from the Ranged
socket (`CharacterClass::weapon_slot`), so its main hand is a stat stick by
design.

The speeds themselves were not Classic's. They were swings per second on a
compressed clock: melee about 3x faster than Classic (the Arcanite Reaper every
1.11s against Classic's 3.8s), ranged about the same (a wand every 1.43s
against 1.5-1.8s, a bow every 2.5s against 2.4-2.9s).

## What changed

- **`weapon_speed` is seconds per swing, the Classic tooltip's "Speed".** It
  replaces `attack_speed` (swings per second) on both `ItemConfig` and
  `Combatant`, with `offhand_weapon_speed` for the off hand. The per-class base
  speed is gone. A combatant built without equipment swings at
  `UNARMED_WEAPON_SPEED` (2.0s, Classic's unarmed speed). An equipped combatant
  whose live socket is empty gets `AutoAttackKind::None` and does not
  auto-attack at all, so the fallback reaches only unit tests and the
  animation sandbox. Pets are not weapons and keep their own speeds.
- **Every weapon's speed is a named Classic item's, exactly** (table below).
  `every_weapon_declares_its_classic_speed` names all 20 weapons with their
  speeds and Wowhead ids, and asserts set equality against every `is_weapon`
  item. A new weapon therefore fails `cargo test` until it has a row.
- **Damage per swing = the item's existing weapon DPS x its new speed.** A
  one-hander, wand or bow keeps its DPS exactly; only its cadence changes. Min
  and max scale by the same factor, so each weapon keeps its spread.
- **Two-handers carry Classic's premium** (below), pinned by
  `two_handers_carry_the_classic_premium_over_the_one_hander_they_displace`.
- **Rage per swing = `RAGE_PER_WEAPON_SECOND` (9.0) x the weapon's base
  speed.** 9.0 is the Arcanite Reaper's old income (10 rage per swing at 0.9
  swings a second), so the default Warrior's rage per second did not move:
  34.2 rage every 3.8s. An off-hand swing pays half of its own weapon's share.
  An attack-speed slow still costs rage, because it means fewer swings.
- **Per-swing procs are rated per minute** (below).
- **Heroic Strike is uncapped** (the user's ruling). It adds
  `0.5 x attack_damage`, which is now a 3.8s two-hander's per-swing damage:
  about +33 on the Arcanite Reaper against +8 before.

## The Classic stand-ins

Only two of the 20 sim weapons are real Classic items (Arcanite Reaper and
Witchblade), and two more share a name with a real item of the same kind
(Claw of Chromaggus, Azuresong Mageblade). The sim's "Staff of Dominance" is a
WAND named after a real two-handed staff. So each sim weapon takes the speed
of a stand-in chosen by one rule:

> **The same-named real item, if it is the same weapon type and hand and
> within 5 item levels. Otherwise the real item of the same weapon type, hand
> and role (physical or caster stats) nearest in item level.**
> Ties go to the lower item level.

Every number here was read from the Wowhead Classic MCP (`lookup_item_by_id`).

| Sim item (type, ilvl) | Classic stand-in (Wowhead id, ilvl) | Speed | Classic DPS | Old sim swing | New damage | Weapon DPS |
|---|---|---|---|---|---|---|
| Arcanite Reaper (2H axe, 60) | Arcanite Reaper (12784, 63) | 3.8 | 53.82 | 1.11s | 57.02-73.30 | 14.40 -> **17.15** |
| Frostbite Blade (1H sword, 58) | Dal'Rend's Sacred Charge (12940, 63) | 2.8 | 41.43 | 0.91s | 30.8-43.12 | 13.20 |
| Serpent Fang Dagger (dagger, 58) | Felstriker (12590, 63) | 1.7 | 45.59 | 0.67s | 20.4-30.6 | 15.00 |
| Hammer of the Righteous (caster mace, 58) | The Hammer of Grace (11923, 57) | 2.7 | 37.59 | 1.00s | 27.0-40.5 | 12.50 |
| Crescent Staff (caster staff, 58) | Elemental Mage Staff (944, 61) | 3.2 | 57.50 | 1.43s | 45.86-76.42 | 5.60 -> **19.11** |
| Witchblade (caster dagger, 58) | Witchblade (13964, 62) | 1.6 | 40.63 | 0.67s | 16.8-26.4 | 13.50 |
| Wand of Shadows (wand, 56) | Skul's Ghastly Touch (13396, 57) | 1.8 | 55.83 | 1.43s | 10.08-15.12 | 7.00 |
| Staff of Dominance (wand, 58) | Mana Channeling Wand (18483, 61) | 1.6 | 60.94 | 1.25s | 11.52-16.64 | 8.80 |
| Ashwood Bow (bow, 58) | Satyr's Bow (18323, 58) | 2.4 | 29.79 | 2.50s | 28.8-37.44 | 13.80 |
| Sniper Scope Crossbow (crossbow, 60) | Stoneshatter (18388, 62) | 2.9 | 31.72 | 2.86s | 22.33-28.42 | 8.75 |
| Bloodlord's Battleaxe (2H axe, 75) | Draconic Avenger (19354, 71) | 3.2 | 68.13 | 1.11s | 59.75-79.67 | 18.90 -> **21.78** |
| Stormblade Edge (1H sword, 73) | Brutality Blade (18832, 70) | 2.5 | 51.60 | 0.91s | 33.0-49.5 | 16.50 |
| Fang of the Viper (dagger, 73) | Core Hound Tooth (18805, 70) | 1.6 | 51.25 | 0.67s | 24.0-38.4 | 19.50 |
| Mace of the Redeemer (caster mace, 73) | Aurastone Hammer (17105, 69) | 2.7 | 44.63 | 1.00s | 32.4-48.6 | 15.00 |
| Runestaff of Elements (caster staff, 73) | Staff of Dominance (18842, 70) | 2.9 | 57.07 | 1.43s | 49.05-85.83 | 7.70 -> **23.26** |
| Claw of Chromaggus (caster dagger, 73) | Claw of Chromaggus (19347, 77) | 1.5 | 42.33 | 0.67s | 20.25-31.5 | 17.25 |
| Azuresong Mageblade (caster sword, 73) | Azuresong Mageblade (17103, 71) | 2.4 | 42.50 | 0.91s | 29.04-44.88 | 15.40 |
| Wand of the Invoker (wand, 71) | Cold Snap (19130, 70) | 1.7 | 85.29 | 1.43s | 11.9-19.04 | 9.10 |
| Eaglestrike Bow (bow, 73) | Rhok'delar, Longbow of the Ancient Keepers (18713, 75) | 2.9 | 43.97 | 2.50s | 27.84-37.12 | 11.20 |
| Deadeye Crossbow (crossbow, 75) | Crossbow of Imminent Doom (21459, 72) | 3.1 | 41.61 | 2.86s | 28.21-36.89 | 10.50 |

Weapon DPS is `mid damage / speed`. Classic DPS is the stand-in's tooltip DPS.
It is used only to compute the two-hander ratios; sim damage is not Classic
damage scaled by one factor (see the wand check below for why that would not
work).

## The two-hander premium, and what it means

**A two-hander deals Classic's two-hander premium more weapon DPS than the
one-hander it displaces.** That premium is what buys back the off hand it gives
up. Each ratio is the two stand-ins' own tooltip DPS, and the displaced
one-hander is the pair AS-115 priced each two-hander against
(`docs/design/balance/2026-09-27-as115-two-hander-budget.md`).

| Two-hander | Displaces | Classic ratio (2H / 1H stand-in) | Sim weapon DPS, 2H : 1H |
|---|---|---|---|
| Arcanite Reaper | Frostbite Blade | 53.82 / 41.43 = **1.30** (both ilvl 63) | 17.15 : 13.20 |
| Bloodlord's Battleaxe | Stormblade Edge | 68.13 / 51.60 = **1.32** (ilvl 71 / 70) | 21.78 : 16.50 |
| Crescent Staff | Witchblade | 57.50 / 40.63 = **1.42** (ilvl 61 / 62) | 19.11 : 13.50 |
| Runestaff of Elements | Claw of Chromaggus | 57.07 / 42.33 = **1.35** (ilvl 70 / 77) | 23.26 : 17.25 |

So an axe two-hander deals about 30% more weapon DPS than the sword it
replaces, and a staff 35-42% more than the caster dagger it replaces. The
Runestaff's pair spans 7 item levels. Checked against a caster one-hander at
the staff's own level, Azuresong Mageblade (ilvl 71, 42.50 DPS), the ratio is
57.07 / 42.50 = 1.34, the same premium.

Before this card the premium was negative. Serpent Fang (15.0) out-DPSed the
Arcanite Reaper (14.4), and the staves ran at 5.6 and 7.7 weapon DPS against
13.5 and 17.25 for their daggers.

**For the casters the staff premium is inert.** A Mage, Priest or Warlock
auto-attacks with its wand. `apply_equipment` takes weapon damage only from
the socket `class.weapon_slot()` names, and for those three classes that is
the Ranged socket. A staff's damage never reaches their sim, so the card's
"staff users" slice has nothing to measure, and this sweep did not measure it.
The premium is live for any class whose live socket is the main hand and that
trains the weapon. In the shipped item set that means the Warrior's axes. It
also means a Shaman's staff, but no default loadout gives a Shaman one.

## The wand check

The user disputed an earlier figure of mine: "Classic wands about 110 DPS
against about 50 for melee". The user was right. That figure set Wand of Fates
(ilvl 83, 113.67 DPS) against melee weapons around ilvl 63, which is two tiers
apart. At matched item levels:

| Item level | Wand (id, DPS) | One-hander (id, DPS) | Ratio |
|---|---|---|---|
| 57 | Skul's Ghastly Touch (13396), 55.83 | The Hammer of Grace (11923), 37.59 | 1.49 |
| 61-62 | Mana Channeling Wand (18483), 60.94 | Witchblade (13964), 40.63 | 1.50 |
| 70 | Cold Snap (19130), 85.29 | Core Hound Tooth (18805), 51.25 | 1.66 |
| 70 | Cold Snap (19130), 85.29 | Brutality Blade (18832), 51.60 | 1.65 |

A same-tier Classic wand hits 1.5-1.66x as hard as a one-hander, which is under
the user's "no more than about twice". Wand DPS carries no attack power, which
is part of why its tooltip number runs higher. Rule 2 keeps today's sim wand
DPS whatever the Classic figure, so this corrects a stated fact and moves no
number.

## Per-swing procs are rated per minute

Nothing in the sim ruled on this, so it is the pipeline's default and the user
can overrule it. Three effects rolled a flat chance on every landed swing.
Left per-swing, a 3.8s weapon would proc about a third as often as it did at
1.11s. Each now declares procs per minute and converts on every landed swing
with Classic's PPM formula, `ppm x weapon speed / 60`, clamped at 1.0
(`proc_trinkets::per_swing_chance`). The conversion keeps each effect's old
rate at 1 swing a second, the old sim's reference melee speed (the class base
Warrior speed and both caster maces): PPM = old chance x 60.

| Effect | Old | New | Per swing now |
|---|---|---|---|
| Crippling Poison (Rogue) | 50% a swing | 30 per minute | 85% on the 1.7s dagger, each hand |
| Windfury Totem (main hand only) | 12% a swing | 7.2 per minute | 46% on a 3.8s two-hander, 20% on a 1.7s dagger, 32% on a 2.7s mace |
| Dragonspine Trophy (`MeleeHit`) | 15% a hit | 9 per minute | 57% on a 3.8s two-hander |
| Whetstone of Fury (`MeleeHit`) | 10% a hit | 6 per minute | 38% on a 3.8s two-hander |

A `MeleeHit` trinket must now be rated `PerMinute`, and a `SpellCast` or
`Heal` trinket `Chance`. `validate_proc` enforces the pairing, because a cast
has no swing speed to convert. The price is unchanged: a proc is priced at its
ICD's long-run uptime, which ignores the rate. For the Rogue, 30 a minute is
fewer procs than its old 45 a minute per hand (50% at 1.5 swings a second).
The 8s slow was already near full uptime at either rate.

## Every consumer of the swing interval

| Consumer | Where | How it reads the speed now |
|---|---|---|
| Main-hand swing timer: melee, wand, Auto Shot | `combat_auto_attack` via `effective_attack_interval` | `weapon_speed`, stretched by each `AttackSpeedSlow` |
| Off-hand swing timer | `effective_offhand_interval` | `offhand_weapon_speed`, same slows |
| Swing wind-up animation, per hand | `rendering/effects/weapon_swing.rs` | the same two functions, so the stroke tracks the sim |
| Warrior rage per swing | `combat_auto_attack` | `RAGE_PER_WEAPON_SECOND x` base speed; off hand x 0.5 |
| Windfury bonus swing | `windfury_procs_per_minute` -> `per_swing_chance` | main-hand base speed |
| `MeleeHit` proc trinkets | `roll_procs(.., Some(speed), ..)` | the landing hand's base speed |
| Crippling Poison | `combat_auto_attack` apply loop | the landing hand's base speed |
| Heroic Strike | `warrior.rs`, `0.5 x attack_damage` | per-swing damage, so it grows with a slower weapon (uncapped, by ruling) |
| Frost Armor chill | on a landed melee swing | no roll; fewer swings re-apply it less often |
| Pets | `Combatant::new_pet` | their own speeds (0.83s / 0.77s), unchanged |
| View Combatant "Weapon Speed", encyclopedia "Speed" | UI | `item.weapon_speed`, the value `apply_equipment` copies. It is the base speed; a slow in a match stretches it |

## Measured

**DIRECTIONAL** ([sweep tiers](sweep-tiers.md)): which way, and roughly how
far. These numbers may not be cited as a class's standing, and nothing here was
tuned toward parity.

The sweep covers the whole 2v2 matrix without double healers: 625 cells, 10
seeds each, 6,250 matches per arm. It is paired at identical seeds, with a
300s cap. The arms are `main` @ `17cb9f0` and this branch. The input is
`sweeps/2026-09-28-as167-weapon-speed.jsonl`, the CSVs are
`2026-09-28-as167_before_17cb9f0.csv` and `2026-09-28-as167_after.csv`, and
the tables come from `sweeps/2026-09-28-as167-slices.py`. Everything below
recomputes from those committed files. Nothing needs the arms rebuilt.

**There is no control group, because no cell is out of reach.** Every class
auto-attacks with a weapon whose speed moved: a two-hander, a mace, daggers, a
bow or a wand. The directional tier's "cut cells" lever therefore had nothing
to cut, and this is the full matrix at 10 seeds. Pairing still holds. Both
arms ran the same JSONL, and each binary's output is deterministic (a rebuilt
branch binary reproduced its logs byte for byte). What the missing control
costs is a separate estimate of spill-over: every slice below is net of the
class's own change and everyone else's.

### Non-vacuity

- **Matches ended by a kill:** 6,224 of 6,250 before and 6,225 after.
  **Distinct durations:** 2,553 and 2,498.
- **Movement:** the winner or the duration moved in 5,728 matches, and the
  winner flipped in 1,246 (19.9%). The median duration went from 31.7s to
  31.0s.
- **Swings landed, per hand.** These counts come from 105 logged matches,
  every 60th line of the sweep JSONL (`awk 'NR%60==1'`), each run with
  `--headless --output` on both binaries and counted by
  `sweeps/2026-09-28-as167-swings.py`. The Rogue's hands are split on
  magnitude, since an off-hand swing deals half.

| Swing | per combat-second, before | after | predicted from speed alone |
|---|---|---|---|
| Warrior main hand (Auto Attack + Heroic Strike) | 0.444 | 0.153 | 0.130 (1.11s -> 3.8s) |
| Rogue main hand | 0.467 | 0.195 | 0.183 (0.67s -> 1.7s) |
| Rogue off hand | 0.471 | 0.195 | 0.183 |
| Paladin mace | 0.184 | 0.084 | 0.068 (1.0s -> 2.7s) |
| Hunter Auto Shot | 0.216 | 0.234 | 0.225 (2.5s -> 2.4s) |
| Mage wand | 0.162 | 0.113 | 0.126 (1.43s -> 1.8s) |
| Priest wand | 0.312 | 0.246 | 0.244 (1.25s -> 1.6s) |
| Warlock wand | 0.348 | 0.281 | 0.276 (1.43s -> 1.8s) |

  Every hand moved by about its speed ratio. Both of the Rogue's hands still
  land, at the same rate, since both hold the same dagger. The Shaman's mace
  landed too rarely to rate (24 and 15 swings), because it seldom melees.
- **Heroic Strike** landed for 21.4 a hit before and 79.1 after, and went from
  22% to 34% of the Warrior's melee damage (Mortal Strike: 51% -> 42%).

### Per class, each class's own side

A match counts for a class when exactly one team fields it. Win rates are that
side's. The half-width is the 95% resolution the flips bought on the delta.

| Class | n | before | after | delta | flips | z |
|---|---|---|---|---|---|---|
| **Warrior** (two-hander) | 2520 | 37.9% | 46.6% | **+8.7pt** (+/-1.8) | +381/-161 | +9.45 |
| Rogue (dagger main/off hand) | 2520 | 46.6% | 45.9% | -0.8pt (+/-1.7) | +220/-239 | -0.89 |
| Hunter (bow) | 2520 | 38.9% | 34.2% | -4.8pt (+/-1.5) | +127/-247 | -6.21 |
| Mage (wand) | 2520 | 67.5% | 63.8% | -3.7pt (+/-1.6) | +176/-269 | -4.41 |
| Priest (wand) | 2000 | 55.7% | 55.7% | +0.0pt (+/-2.1) | +227/-227 | 0.00 |
| Warlock (wand) | 2520 | 38.3% | 37.5% | -0.8pt (+/-1.8) | +246/-265 | -0.84 |
| Paladin (mace) | 2000 | 67.0% | 66.3% | -0.6pt (+/-2.0) | +198/-210 | -0.59 |
| Shaman (mace) | 2000 | 52.4% | 54.1% | +1.7pt (+/-1.9) | +209/-175 | +1.74 |

**The Warrior's two-hander is the change that moved the matrix.** It is the
only weapon whose DPS rose: +19% from Classic's premium, plus a Heroic Strike
that now adds half of a 3.8s swing. Its rage per second is unchanged. It gains
+8.7pt (z=9.5). Split every other class by whether it faced a Warrior, and the
losses are almost all against one:

| Class | vs an enemy Warrior | vs no Warrior |
|---|---|---|
| Rogue | -6.5pt (z=-4.0, n=840) | +2.1pt (z=+2.2, n=1680) |
| Hunter | -11.8pt (z=-7.8) | -1.2pt (z=-1.5) |
| Mage | -8.9pt (z=-6.2) | -1.1pt (z=-1.0) |
| Priest | -6.8pt (z=-3.4, n=600) | +2.9pt (z=+2.3, n=1400) |
| Warlock | -5.5pt (z=-3.1) | +1.6pt (z=+1.6) |
| Paladin | -9.0pt (z=-4.6) | +3.0pt (z=+2.6) |
| Shaman | -5.3pt (z=-2.6) | +4.7pt (z=+4.3) |

Away from a Warrior the other weapons' cadence change, which keeps their DPS,
nets out small. The largest are the maces and healers (+2 to +5pt), and the
Hunter and Mage move by -1pt, which this run cannot tell from zero. The
Hunter's and Mage's headline losses are the Warrior's gain seen from the other
side, not their own weapons.

Heroic Strike was left uncapped, per the ruling. Nothing here shows it breaks
a rule: rage per second did not move, and the gain is a stronger Warrior rather
than a degenerate one. But it is the largest single term in the Warrior's gain
that is not Classic's own weapon number, and this sweep does not separate it
from the premium. Separating the two needs a capped-Heroic-Strike arm (a
follow-up, not done here). Nothing here was tuned to parity.

### TeamPlan mechanism (`camp_sweep`)

The Nagrand camp still works on this branch, over 12 seeds. The TeamPlan heal
line is occluded 17% of the time (26% after contact). The Warlock denies the
Priest for 15.1s before contact and 10.2s after. TeamPlan wins 11/12, against
9/12 for Legacy. Those figures are in the band CLAUDE.md records for the
shipped solve.

Two single-seed outliers turned up while re-pinning the TeamPlan probes, and
both are recorded in case they are the start of something. Both are opt-in
TeamPlan behaviour, neither is in a probe's pinned set, and both probes pass.
Nagrand heal line, seed 11: 47% blocked, against 27% on `main`, with the other
eleven seeds at 4-29%. Anti-statue, seed 16: 0.37 u/s under pressure, against
1.98 on `main`, with the twenty-seed median up (1.58 -> 1.86 u/s).

## Baselines this invalidates

Every class's standing moved, so every published win rate from before this card
describes a sim that no longer exists:

- `docs/design/balance/canonical_baselines_summary.md` and
  `canonical_3v3_full_n50_300s.csv`, plus the dated `matrix_baseline_*` files;
- the AS-115 two-hander sweep (`2026-09-27-as115-two-hander-budget.md`). Its
  budget prices still stand. Its win rates were measured at the old weapon
  speeds;
- the AS-60 and AS-122 dual-wield findings (`2026-09-18-as60-dual-wield-findings.md`,
  `2026-09-18-as122-rogue-offhand-findings.md`). The Rogue's two daggers now
  swing every 1.7s, not every 0.67s;
- `tests/baselines/legacy_behaviour_2026-09-28_two_hander_budget.txt`,
  superseded by `legacy_behaviour_2026-09-28_weapon_speed.txt` (all 27 cells
  moved, each attributed in `tests/baselines/README.md`);
- both `determinism_pin` cells, and a dozen seed-pinned probes in
  `movement_probes`, `proc_trinket_probes` and `headless_tests`. Each was
  re-pinned from its own scanner, with the first differing log line cited
  beside it.
