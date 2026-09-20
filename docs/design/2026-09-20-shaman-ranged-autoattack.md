# The Shaman's ranged auto-attack: what it is, what it costs to remove

Card: AS-99. Measured against `aef713c` on 2026-09-20. **This is a measurement,
not a proposal.** It ends with options and their prices; which way the fiction
resolves is a design decision and is not made here.

---

## 1. What is actually wrong, in source

Auto-attack RANGE and auto-attack IDENTITY are both derived from the attacker's
CLASS, never from the item in its weapon socket. The predicate is
`CharacterClass::is_melee()`, and `combat_core/auto_attack.rs` asks it — or the
`class == Hunter` fallback beside it — at **five** sites, not the two the card
named:

| site | line | what it decides | arm rewired it? |
|---|---|---|---|
| range ladder | `auto_attack.rs:276` | `MELEE_RANGE` / `AUTO_SHOT_RANGE` / `WAND_RANGE` | yes |
| Hunter dead zone | `auto_attack.rs:289` | the 8yd minimum on Auto Shot | yes |
| **Windfury proc** | **`auto_attack.rs:348`** | **`windfury_bonus_chance` — the bonus swing** | **no** |
| line-of-sight gate | `auto_attack.rs:304` | whether occlusion blocks the swing | yes |
| **Frost Armor proc** | **`auto_attack.rs:579`** | **whether the target's chill fires back** | **no** |
| swing visual flag | `auto_attack.rs:643` | `AutoAttackSwing.ranged` | yes |
| log name | `auto_attack.rs:664` | `"Auto Attack"` / `"Auto Shot"` / `"Wand Shot"` | yes |

**Seven sites, not five** — an earlier draft of this doc listed five and missed
the two proc gates. They matter to Option 1 and are called out again in section
5. (The card cited 261-267 and 577-585; AS-122's dual-wield work shifted them.)

`CharacterClass::weapon_slot()` — added by AS-97 precisely to stop `is_melee`
answering two questions — is consulted by none of them.

**The Shaman is the only class this reaches today, and that is provable from the
loadout table rather than asserted.** Only two classes hold a non-weapon in
their Ranged socket (`weapon_type: Relic`, no `is_weapon`): Paladin
(`LibramOfHope`) and Shaman (`TotemOfLife`). The Paladin is `is_melee`, so it
takes the `MELEE_RANGE` arm and never reaches the relic. Warrior and Rogue hold
no Ranged item at all. Mage, Priest, Warlock and Hunter all hold real weapons
(`Wand` x3, `Bow`). So the Shaman is the one class firing a ranged auto-attack
with nothing to fire it from — and every future relic wearer inherits the path.

### The numbers, recomputed from `items.ron`

`attack_speed` is attacks per second (`swing_interval = 1.0 / speed`,
`auto_attack.rs:949`), so raw auto-attack DPS is `avg_damage x speed`:

| class | live weapon | avg dmg | speed | raw auto DPS | range |
|---|---|---|---|---|---|
| **Shaman** | `HammerOfTheRighteous` (MainHand mace) | **12.5** | **1.0** | **12.5** | **30yd** |
| Hunter | `AshwoodBow` | 34.5 | 0.4 | 13.8 | 35yd (8yd dead zone) |
| Priest | `StaffOfDominance` (Wand) | 11.0 | 0.8 | 8.8 | 30yd |
| Mage / Warlock | `WandOfShadows` | 10.0 | 0.7 | 7.0 | 30yd |
| Shaman *before* AS-97 | class base | 7.0 | 0.8 | 5.6 | 30yd |

AS-97 multiplied the Shaman's auto-attack output by **2.23x**. The result is
**+42%** on the Priest's real wand, **+79%** on the Warlock's, and **91% of a
Hunter's Auto Shot** — delivered at 30 yards by a healer holding a totem.

---

## 2. What a player actually sees — the card's premise is wrong, in a useful way

The card opens "the client renders it wielding that mace." **It does not.**

`class_weapon_loadout` (`play_match/mod.rs:1117`) has arms for Warrior, Rogue,
Hunter and Paladin and `_ => &[]`. Its own comment says so: *"Classes not listed
hold nothing (casters, Shaman) — their auto-attack swing signals no-op against
zero sockets."* The Shaman therefore has **zero `WeaponSocket` children**: no
mace model, no swing animation, and no thrown projectile. The cosmetic arrow is
additionally gated on `WeaponKind::Bow`, so it cannot fire either.

**The fiction is real but it is TEXTUAL.** Where a player meets it is the
in-client Combat Log panel (`rendering/combat_log.rs:118`), which prints every
damage entry verbatim:

```
[ 13.92s] [DMG] Team 1 Shaman #2's Wand Shot hits Team 2 Priest #2 for 12 damage
```

Auto-attacks do not appear in the ability timeline (that reads
`ability_casts_for`), and floating combat text shows only the number.

Two consequences that change how the options price out:

- **Option 2 is cheaper than the card assumes** — there is no thrown-mace
  animation to remove, only a string to change.
- **Option 1 creates a new visual gap** — a melee Shaman would punch with empty
  hands, because the mace still has no model. Making it melee properly wants a
  `Shaman => Mace` arm in `class_weapon_loadout` in the same change.

---

## 3. Method

Two binaries, one config file, identical seeds, one variable.

- **base** = clean `aef713c`, `sha256 58112da2...`
- **arm** = base plus a derived-from-equipment model (section 6), `sha256 9193b81b...`

**Tier: DIRECTIONAL** in AS-104's sense — a focused slice of the cells the change
reaches, sized in minutes, with a full-strength control. The per-cell figures in
section 4.2 are **not individually powered** and must not be cited as any comp's
standing. The aggregate and the control are what carry weight.

- **Win-rate sweep:** 520 paired matches per arm (1,040 total), BasicArena,
  Legacy AI, 300s cap. 400 Shaman matches (5 DPS partners x 40 seeds x both side
  assignments) + 120 control matches with no Shaman on either side.
- **Mechanism slice:** 24 paired matches with full logs, counting Shaman
  auto-attacks and their damage directly off the `[DMG]` lines. Comps are
  `[P, Shaman] vs [P, Priest]` for **P in Warrior, Mage, Rogue, Warlock, Hunter,
  Paladin**, at **seeds 90000-90003** each.

  **The Paladin cell is degenerate and must be read separately.** Paladin is
  itself a healer, so that cell is a **two-healer vs two-healer** match — unlike
  the other five, whose partner is a DPS. All four of its matches end
  `Duration: 309.99s / Winner: DRAW`: they hit the cap without resolving, even
  through the full arena-dampening ramp. Four 310-second draws against a 38-91s
  mean elsewhere means that one cell contributes as much auto-attack damage as
  the other five combined, so section 4.3 reports the slice **both ways**. The
  win-rate sweep above is unaffected — it uses the five DPS partners only.

**Load conditions, stated because they are the point of AS-104's cost question.**
The box (18 cores) was shared with two other Engineers and a concurrent sweep.
`--jobs 6` was chosen deliberately rather than the default of cores-minus-two.
Base: 520 matches in **433.9s (1.20/s)** at load 26-39. Arm: 520 in **518.2s
(1.00/s)** at load ~26. Total **under 16 minutes of batch wall clock** — the
structural argument for the directional tier, since it finished well inside a
window where `main` did not move (`origin/main` was still `aef713c` afterwards).

**Do not read those rates as the cost of a sweep.** AS-104 instrumented the same
box during this run and found it **thrashing**, not merely busy: 1,147s of wall
clock against 797s user and **1,611s system** — two CPU-seconds in the kernel per
one simulating, averaging 2.1 busy cores of 18. Its measured intrinsic cost is
**0.46 CPU-seconds per match**, and AS-122 saw 3.7-5.6 matches/sec on a quiet
box. So the honest reading is that this measurement's *true* cost is a few
minutes, and the 1.0-1.2/s figures above are an artifact of three agents
oversubscribing 18 cores. AS-104's number supersedes the comparison an earlier
draft of this doc drew against AS-86's 0.57/s.

### The harness detects the change before any figure is cited

On the single seeded probe (`Warrior+Shaman vs Warrior+Priest`, seed 90000), the
**first divergent line in the timestamped event stream** is:

```
59d58
< [ 13.92s] [DMG] Team 1 Shaman #2's Wand Shot hits Team 2 Priest #2 for 0 damage (12 absorbed)
```

Nothing before it differs. That is the event the change predicts, found in the
trace — attributed positively, not by elimination. Base fires 11 Shaman Wand
Shots in that match; the arm fires zero auto-attacks of any name.

---

## 4. Results

### 4.1 Split control — the change reaches nothing without a Shaman

**120 matches, 0 rows differing** on winner, end reason and duration. Exact
row-for-row identity. The diff cannot touch a comp with no Shaman in it.

This is a **split control** — cells the change cannot reach, required to come out
bit-identical — and it is a correctness check on the instrument, not a
statistical one. It is deliberately not a null probe (two identical binaries on
the same seeds), which AS-104 established is vacuous here: the sim is
deterministic, so such a run is identical by construction and every flip count is
zero before it starts.

### 4.2 Win rate — the Shaman side loses ~19 points

Both side assignments, so no team-1 ordering artifact can carry the result.

| slice | n | base | arm | delta | flips out / in | z |
|---|---|---|---|---|---|---|
| Shaman on team 1 | 200 | 65.5% | 48.5% | **-17.0pt** | 53 / 19 | **-4.01** |
| Shaman on team 2 (row is the Priest side) | 200 | 33.5% | 54.0% | **+20.5pt** | 21 / 62 | **+4.50** |
| control (no Shaman) | 120 | 45.0% | 45.0% | 0.0 | 0 / 0 | 0.00 |

**Side-symmetrized: the Shaman side loses 18.8 points.** The two assignments
agree in sign and in magnitude, and each is independently significant.

**Non-vacuity:** 392 of 400 Shaman-slice rows differ between arms; 155 discordant
paired outcomes; 3 draws total across both arms, so this is not a draw-rate
artifact. The mechanism slice landed 832 base auto-attacks with no vacuous match
(minimum 2 per match).

Per-cell, directional only — recorded because the shape is informative, not
because any cell is powered at n=40:

| cell (the Shaman side's win rate) | base | arm | delta |
|---|---|---|---|
| Hunter+Shaman | 95.0% | 97.5% | +2.5 |
| Rogue+Shaman | 75.0% | 72.5% | -2.5 |
| Mage+Shaman | 30.0% | 10.0% | -20.0 |
| Warrior+Shaman | 55.0% | 25.0% | -30.0 |
| Warlock+Shaman | 72.5% | 37.5% | -35.0 |

The two flat cells are the two where the Shaman barely wands at all:
Hunter+Shaman is saturated above 95% either way, and Rogue matches end before the
Shaman gets going (2-4 auto-attacks per match, section 4.3). The cells that
collapse are the long ones.

### 4.3 Mechanism — the ranged auto-attack is 41-53% of the Shaman's damage

**Report this as a range, not a point.** The headline depends on whether the
degenerate Paladin cell (section 3) is counted:

| slice | matches | mean dur | autos/match | auto damage | **auto share of the Shaman's own damage** |
|---|---|---|---|---|---|
| all six partners | 24 | 108.4s | 34.7 | 9,192 | **52.8%** |
| **five DPS partners** | 20 | 68.1s | 17.6 | 4,364 | **41.3%** |
| Paladin cell alone | 4 | 310.0s | 120.0 | 4,828 | 70.7% |

The four Paladin draws supply **52.5% of the whole slice's auto-attack damage**.
The 41.3% figure is the one to quote for ordinary play; 52.8% is the slice as
run, and is inflated by matches that never end.

Per cell, base arm:

| partner | mean dur | autos/match | auto share |
|---|---|---|---|
| Rogue | 38.6s | 2.5 | 14.0% |
| Hunter | 48.3s | 12.0 | 28.4% |
| Mage | 79.7s | 17.5 | 33.6% |
| Warlock | 91.4s | 34.8 | 49.6% |
| Warrior | 82.4s | 21.2 | 67.7% |
| *Paladin (4 draws)* | *310.0s* | *120.0* | *70.7%* |

The share tracks match length, which is the honest mechanism: a Shaman that
survives longer spends proportionally more of its output on free auto-attacks
than on mana-limited casts. **Either way it is the single largest component of
the Shaman's damage**, and that is the claim the options rest on.

The arm-side figures below are for the full 24, matching the table's first row:

| | base | arm |
|---|---|---|
| Shaman auto-attacks | **832** (34.7/match) | **6** (0.2/match) |
| named | 100% `Wand Shot` | 100% `Auto Attack` |
| auto-attack damage | 9,192 | 66 |
| **auto share of the Shaman's OWN damage** | **52.8%** | 0.8% |
| auto share of its team's damage | 26.8% | 0.3% |
| Shaman total DPS | 6.69 | 3.32 (**-50.4%**) |
| Shaman team DPS | 13.20 | 9.68 (**-26.7%**) |
| mean match duration | 108.3s | 98.7s |

DPS rather than totals, because the arm's matches are shorter; the share
percentages are within-match ratios and are duration-robust either way.

**The phantom wand is the largest single component of the Shaman's damage** —
41.3% over the five DPS partners, 52.8% including the Paladin draws. Removing it
roughly halves the class's damage output and takes about a quarter off its
team's.

### 4.4 The finding that collapses two of the three options into one

**All six residual melee swings in the arm are in the Rogue cell** — a Rogue
standing on the Shaman. In 20 of 24 matches a melee-ranged Shaman auto-attacked
**zero times**, across whole 100-second matches.

That is not an accident of tuning, and the static read predicts it: the Shaman
has **no pursuit behaviour to lose**. Its only range-seeking scorer term is
`movement.ron:103` `wand_pull: 1.0`, and that term is repurposed — its own
comment and `movement_config.rs:260` say so — as a **Lightning Bolt**-range pull.
Lightning Bolt's range is 30.0 (`abilities.ron:1226`), identical to the shared
`wand_range: 30.0` the term reads. So the Shaman parks at spell range and stays
there.

**Therefore "make it melee-ranged" and "remove it entirely" are the same change,
3 damage a match apart.** The card's worry that melee-ranging it would be a
"MOVEMENT and positioning change" does not materialise: no movement config
changes under any option, and in practice nothing moves.

---

## 5. The options, priced

### Option 1 — derive range and name from the equipped weapon

A class with no weapon in its live socket gets no auto-attack. The general rule
the card asks for; the Shaman falls to melee range and, in practice, to silence.

- **Balance: -18.8pt to the Shaman side** (measured, directional tier), -50% to
  its damage output. That is roughly double what AS-97 added, in the opposite
  direction.
- **Correctness:** puts range, name, LoS, dead zone and the visual flag on one
  derived value, so those five cannot disagree again, and covers every future
  relic class with no further work. It does **not** by itself reach the two proc
  gates (section 1) — a faithful version must decide those deliberately rather
  than leave them on the class ladder.
- **A behavioural consequence the decision turns on: a melee-derived Shaman
  would start self-proccing Windfury from its own totem.** `totem_pulse_system`
  gates only on `ally.team != owner_team` — there is no self-exclusion — so the
  Shaman already carries its own Air Totem's `WindfuryBuff`. It is inert today
  only because `windfury_bonus_chance` returns `None` for a non-melee attacker.
  Reclassify the Shaman as melee and the 12% bonus swing arms on its own
  auto-attacks. Symmetrically, the Frost Armor gate would start chilling a
  Shaman that melees a Frost-Armored Mage. Neither is obviously wrong — an
  Enhancement Shaman self-proccing Windfury is Classic-faithful — but both are
  new behaviour that this option creates and nobody has chosen yet.
- **Code cost: small.** Section 6 — one enum, one `Combatant` field set in
  `apply_equipment`, five call sites.
- **Test cost: exactly one probe, and it is a recalibration not a defect.**
  `movement_probes::oom_wand::mage_oom_closes_to_wand_range_and_breaks_the_dead_window`
  fails on the arm (`movement_probes.rs:6509`: "Mage landed only 2 wand shots",
  floor 4). Its scenario is literally `Mage+Priest vs Warrior+Shaman` and it
  counts Mage wand shots during the **lone-Shaman** 2v1 window — so halving the
  Shaman's damage reshapes the window it measures. Its vacuity guards all still
  pass (the 2v1 opens, ≥200 paired samples, the Mage reaches wand range inside
  the 20s bound), so the mechanism it pins is intact and only the fixed-seed
  count floor has moved. Every test target up to and including `movement_probes`
  passed otherwise; cargo stops at the first failing target, so the ones after it
  were not reached and are unverified.
- **Visual cost: a new gap.** The Shaman would swing empty hands; wants a
  `Shaman => Mace` arm in `class_weapon_loadout` in the same change.
- **Implies:** the Shaman is a pure caster-healer whose mace is a stat stick.
  AS-97's decision to make that mace's damage live then buys almost nothing, and
  is worth revisiting as part of this decision rather than separately.

### Option 2a — keep the numbers, fix only the fiction

Rename it to something a totem-wielding Shaman can plausibly do — a Lightning
Shield discharge, a Shock — and render it as a spell effect rather than a shot.

- **Balance: zero, provably.** The attack name is a `&'static str` consumed only
  by the combat log's own surfaces — the damage-breakdown aggregation, the
  Results screen, the panel's short-label map. Nothing in the sim reads it back
  (verified by grep across `src/`). Sim-identical; only log text changes.
- **Code cost: NOT "one string".** Three items, and the second is a trap:
  - `"Wand Shot"` at `auto_attack.rs:671` is a **shared `else` arm** covering
    Mage, Priest and Warlock as well. A Shaman-specific rename needs a new
    class-conditional arm; renaming the arm in place renames every caster's
    wand.
  - **Renaming the shared arm silently zeroes an existing probe.**
    `tests/movement_probes.rs:6417` detects Mage wand shots with
    `is_wand: line.contains("Wand Shot")`. Rename the shared arm and that count
    becomes 0 for reasons nothing reports — the probe keeps running and stops
    measuring. (A correctly scoped Shaman-only rename leaves it alone, which is
    another reason to add the arm rather than edit the string.)
  - `results_ui.rs:996` asserts `ability_topic("Wand Shot") == None`, plus a
    re-bless of any snapshot whose mock carries the old label.
- **What it does NOT fix:** the 30-yard code path with no weapon behind it
  survives, and the next relic class inherits it. This is the option AS-97's
  Tester meant by "fixing either alone just relocates the fiction" — it relocates
  it into a name that sounds better.
- **Implies:** the Shaman IS a 30-yard auto-attacker by design and its relic is
  the implement. Coherent, but a new design claim rather than the status quo:
  nothing chose it, `is_melee()` did.

### Option 2b — give the Shaman a real ranged implement

**Blocked by the proficiency table, not by taste.** `weapon_proficiency(Shaman)`
makes Bow, Gun, Crossbow, Thrown **and Wand** all `Untrained`; only `Relic` is
`Trained`, with the comment *"A Shaman's ranged socket is a Totem socket, not a
bow socket."* That is Classic-faithful — Shamans have no ranged weapon.

So this means either editing the proficiency table away from Classic, or
inventing a damage-bearing Relic. The second has a trap worth naming:
`attack_damage_min/max` and `attack_speed` are **free stats** under the
item-level budget, so a damaging relic is a budget-free balance lever and the
budget test would not push back on whatever number is chosen for it.

### Option 3, not measured but nameable — keep 30yd, revert the damage

Leave the ranged auto-attack where it is but stop the MainHand mace feeding it
(back to the 5.6 DPS class base). Costs strictly less than Option 1 and strictly
more than zero. I did not measure it and will not interpolate a win rate from a
damage ratio; it is one more 520-match arm, about nine minutes.

---

## 6. The measurement arm

The arm is **not in this branch** — it was built, measured and reverted. It is:

- `AutoAttackKind { Melee, Shot, Wand, None }` in `components/combatant.rs`;
- one `Combatant` field, defaulted from the old class ladder verbatim (so any
  un-equipped combatant is unchanged) and overwritten in `apply_equipment` from
  the item in `class.weapon_slot()` — `None` when that socket holds no weapon;
- pets keep the old ladder exactly (they carry no equipment);
- **five of the seven sites** in section 1 read the derived value (`None`
  `continue`s). The Windfury and Frost Armor gates were left on
  `attacker_is_melee`.

**That gap does not compromise the numbers, and the reason is checkable.** In
the base arm the Shaman is `!is_melee`, so both procs are inert for it there by
construction. In the measurement arm the Shaman landed **6 melee swings across
24 matches**, all in the Rogue cell — a Rogue carries no Frost Armor, and six
swings at a 12% Windfury chance is under one expected bonus swing across the
whole slice. A faithful Option 1 that rewires both gates would therefore move
these figures by less than their rounding. It is still the right thing to
disclose, because the *behaviour* it enables (previous section) is a property of
the option even where its damage contribution is negligible.

The base binary was rebuilt from the reverted tree and hashes identically to the
one that produced the measurements (`58112da2...`), so the revert is exact and
the build reproducible.

Artifacts (session scratchpad, not committed): `sweep.jsonl` — the single config
file both arms ran — plus `base.csv`, `arm.csv`, `analyse.py`, `mech_count.py`,
`share.py` and the 48 mechanism logs.
