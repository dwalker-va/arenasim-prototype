# Freezing Trap: the healer, the enemy it would catch, and the opener — measurement

**Date:** 2026-09-29
**Card:** AS-125
**Tier:** DIRECTIONAL (`sweep-tiers.md`), 1v1 and 2v2, each 2v2 run with no
kill target and again with both teams' kill target at slot 0 (the graphical
client's default). Success condition: no opening trap is thrown where its
victim is just dispelled; a trap springs on the enemy it was decided on; with
a melee on the Hunter, the trap goes on the enemy healer and the Hunter fights
the melee while it is frozen, kill target or not; the Hunter positions for
that trap only while it is ready; in the opener Aimed Shot goes before Serpent
Sting when it has time to finish. The Warlock cells, the Rogue + healer cells
and the no-healer cells are reported whichever way they move. Win rate is
reported, not tuned.
**Arms:** before = `main` at `c79d8cc` (Classic weapon speeds, Warriors enter
the gates at 0 rage); after = this branch on it (release binary sha1
`3ab7b02e`). Same JSONL both arms. Three attribution
arms are the after binary with one piece taken out, each committed as a patch
beside the sweeps: `…-arm-trade-trap.patch`, `…-arm-trap-setup-off.patch`,
`…-arm-opener-ignores-hidden.patch`.
**Raw rows:** `2026-09-27-as125_{1v1,2v2,2v2_kt0}_{base_c79d8cc,after}.csv`
(one row per match, from `sweeps/2026-09-27-as125-hunter-{1v1,directional,directional-kt0}.jsonl`);
`2026-09-27-as125_2v2_arm_{trade_trap,trap_setup_off}.csv`;
`2026-09-27-as125_2v2{,_kt0}_rogue_arm_opener_ignores_hidden.csv` (the Rogue
cells only); `2026-09-27-as125_trap_events.csv` (one row per Freezing Trap
thrown) and `2026-09-27-as125_trap_matches.csv` (one row per match), both
arms, both kill-target settings, from the 27-comp mechanism set.

## What the Hunter does now

Freezing Trap asks two questions before it is thrown: **who will spring it,
and would that enemy just be freed?** Who would free a victim comes from the
engine's own removal rules (`ally_removal` / `can_free_ally` / `ally_freers`,
asked of the aura the trap actually lands, `traps::freezing_trap_aura`), never
from a class list: the Priest, the Paladin and the Felhunter free a trapped
teammate; only the Felhunter frees a trapped pet; nobody frees a trapped
Felhunter. A Paladin's own Divine Shield is not asked: the bubble lifts a trap
only when the Paladin's AI spends its 5-minute cooldown on it, and a trap that
draws it has spent the Paladin's one emergency button.

### With a melee on the Hunter: the trap goes on the healer

While a Warrior or Rogue is inside the Hunter's 20yd closing band and the trap
is ready, Freezing Trap is thrown at the **enemy healer**
(`hunter::try_pressure_trap`) — first in the closing band, and second to
Disengage in the dead zone. The healer must be (`pressure_trap_healer`):

- **trap-eligible** — alive, not immune, not incapacitate-DR-immune, and no
  friendly DoT on it that would break the trap on its first tick;
- **not freed** — no teammate of the healer's can lift it (a Rogue or Warrior
  partner cannot; a Felhunter or a second healer can);
- **not a DPS teammate's target** — every damage source holds fire on a
  trapped enemy, so trapping the healer a partner Warrior is killing idles it
  for the trap's duration.

Two targets do not hold it. A **partner healer's** does not: a healer's target
is wherever acquisition left it, usually the enemy healer, so counting it would
hold this trap in nearly every Hunter + healer team (measured below). And the
Hunter's **own** does not: the throw places a target hold on the Hunter
(`TrapRetarget`), and target acquisition (`acquire_targets`) moves it onto the
melee — traced as a target acquisition, so the target-switch recipe in
CLAUDE.md shows it — and keeps it off the healer from the throw, before the
trap has sprung, and for as long as the trap holds it, however late it
springs. A configured kill target cannot pull it back: the hold is applied
after the kill-target re-force. The move-off is for healers only: a
non-healer the Hunter's own trap holds — a stealthed Rogue kill target caught
by the opening lane trap — keeps the Hunter on it, because the Hunter's team
converges on it the moment the trap ends.

The trap must land cleanly (`pressure_trap_landing`): the healer's led
position within the trap's configured 30yd range, the healer predicted to
spring it (`predicted_trap_springer`), and no other enemy near the landing. A
held throw is traced with why (`healer trap held: …`). Serpent Sting is held
off a healer the trap is reserved for while a melee is in view, since a sting
would make it untrappable.

### Positioning for it, only while the trap is ready

While the trap is ready and such a healer exists (`trap_setup_healer`), the
Hunter's repositioning under melee pressure takes the healer into account:

- **Kite** — a `trap_setup` scorer term (`movement.ron`, Hunter 3.0) pulls
  toward the healer once a step would leave it beyond throw range (the trap's
  range less its 5yd trigger radius); zero inside that range. It sits below
  `flee` (6.0), so the kite still escapes the melee, bent toward the healer.
- **Disengage** — the leap is bent by the same two weights when a straight
  leap would land beyond that range, never so far that it stops carrying the
  Hunter away from the melee (`trap_setup_disengage`).

The bend happens only **before the throw**: once the trap is on cooldown, or
with no such healer, movement is exactly as before. The trace carries a
`trap_setup` scorer term on KITE decisions and names the healer as the
Disengage's `target_id` when the leap bent.

### The lane trap and the off-target trap

- **Off-target (aimed):** the best eligible enemy the team is not killing —
  healer first — at its position (led if moving), only when no other enemy is
  near the landing. A healer is always worth one; anyone else only when no
  teammate of its could free it (`trap_victim_worth_it`), however hard the
  Hunter's team is pressing the teammate that would.
- **Lane throw (no off-target):** to the lane midpoint toward the enemy
  healer (or the kill target), decided on the enemy predicted to **spring** it
  — the first inside the trigger radius once armed. Held when nobody is
  predicted to reach it within 3s of arming, when a teammate of the springer
  would free it, or when the Hunter's team is attacking the springer (healers
  included). While an enemy is in stealth the springer is that unseen enemy;
  the throw is held when a visible enemy would free it, and otherwise goes
  with no aim recorded.
- **Range:** every placement refuses a landing beyond the ability's configured
  `range` (30yd); ranged placement itself is intended (the Trap Launcher
  model).

### The opener: Aimed Shot before Serpent Sting when it has time

*Since AS-179 (`2026-09-29-as179-hunter-followups.md`) the Aimed Shot goes
first only when the sting's GCD would cost it the window
(`aimed_shot_before_sting`); the 1v1 Priest loss below is gone.* As built
here: until the Hunter's first Aimed Shot or Serpent Sting of the match, a
due sting waits behind Aimed Shot when the cast can finish
(`aimed_shot_has_time`): no enemy that could stop it reaches it first. Each
visible such enemy has a
reach — its own kit's interrupt range (`interrupt_reach`, from
`abilities.ron`'s `is_interrupt`: Spell Lock and Wind Shear at 30yd, Kick and
Pummel in melee) or the Hunter's 8yd dead zone for a Warrior or Rogue — and
reaches it at its distance over its closing speed; one already within reach
leaves no time, one held in hard CC past the cast cannot act. **An enemy the
Hunter cannot see counts as one that could stop it**, and that question is
asked wherever Aimed Shot sits in the rotation, not only in the opener: a
stealthed Rogue's distance and heading are unknown until it opens, and it
opens in melee range, so while it is in stealth the Hunter casts Serpent Sting
and instants and begins no Aimed Shot (traced as `an unseen enemy could
interrupt the cast`). The cast time is read from `abilities.ron`. After the
opener, a due sting keeps its place, so a sting re-applied after a Devour
Magic is not delayed.

## The mechanism: where the traps went

27 trap-relevant comps (2v2, 3v3, 1v1), seeds 0-19, traced: 540 matches per
arm, once with no kill target and once with both kill targets at slot 0. The
set fields every melee + healer shape the healer trap reaches, and Rogue +
Priest in **both slot orders** (Hunter first, and partner first as the
directional sweep fields it).

| | no kill target: before | after | kill targets at 0: before | after |
|---|---|---|---|---|
| traps thrown | 507 | 331 | 420 | 330 |
| **sprung on the enemy the throw was decided on** | **114 of 459 named** | **209 of 221 named, + 40 unseen** | **88 of 350** | **194 of 202, + 40 unseen** |
| **sprung on a healer** | **31** (126s held in all) | **155** (1,062s) | **48** (247s) | **156** (1,106s) |
| removed by an enemy dispel or Divine Shield | 290 (0 by the bubble) | 32 (4) | 242 (15) | 24 (17) |
| broken by damage | 6 | 9 | 3 | 1 |
| ran the full duration | 163 | 220 | 105 | 217 |
| never sprung | 48 | 70 | 70 | 88 |

**The Hunter fights the melee while its trap holds the healer, kill target
or not.** With the Hunter's team told to kill the enemy healer
(`team1_kill_target: 1`) in the nine Rogue + healer and Warrior + Shaman
comps, 56 healer traps sprang on the Hunter's own kill target. Across them the
Hunter made **no** decision aimed at the frozen healer, and landed damage on
someone else during 41 of the 56. `hunter_on_victim_held` in the trap rows
counts this for every trap that sprang on the enemy it was thrown at. The
directional sweep never reaches this case: with kill targets at 0, the only
enemy comp with a healer in slot 0 is Priest+Warlock, which has no melee to
trigger the healer trap.

**The partner-healer exception, measured.** Holding the pressure trap whenever
a partner healer targets the enemy healer (the lane throw's own rule) was
built and measured on this set, with no kill target, on the branch at
`4c4689c` (an earlier main): healer traps fell from 158 to 39 and Hunter-side
wins from 237 to 217 of 540 (`H+Pri vs Warrior+Priest` 20 → 2 traps, `H+Pal
vs Rogue+Priest` and `Pal+H vs Rogue+Priest` 25 and 18 → 0). What the
exception costs here: a partner Priest's Mind Blast broke **1 of the 155**
healer traps with no kill target (`H+Pri vs Rogue+Paladin` seed 14) and
**0 of the 156** with kill targets at 0.

**The Hunter breaks its own trap.** 8 of the 9 broken traps with no kill
target were broken by the Hunter's own Aimed Shot, begun on a Rogue before a
trap sprang on it and landing after. Six are in `H+Pri vs Warlock+Rogue`:
the partner Priest dies, the fight becomes the Hunter against the Rogue, and
the lane trap goes on its own kill target (allowed with no teammate left to
break it) while an Aimed Shot is already begun. It is why that comp goes
20 → 10 in the table below. The one broken trap with kill targets at 0 was
the Hunter's own Serpent Sting on a Rogue that walked into a healer trap.

### Per comp, no kill target

Traps per match, matches with none, decided victim vs springer (`+Nu` counts
sprung throws decided on an enemy the Hunter could not see), fate as
removed/broken/full/unsprung, traps that sprang on a healer with the mean
seconds it was held (8.0s is the full trap). Hunter-side wins of 20.

| comp | before: traps/m | zero | decided = springer | fate | on a healer | after: traps/m | zero | decided = springer | fate | on a healer | Hunter wins |
|---|---|---|---|---|---|---|---|---|---|---|---|
| H+Pri vs Rogue+Priest | 1.05 | 0 | 1/21 | 20/0/1/0 | 0 | 0.85 | 3 | 13/13 | 0/0/13/4 | 13 (8.0s) | 1 → 7 |
| H+Pri vs Warrior+Priest | 0.10 | 18 | 2/2 | 0/0/2/0 | 2 (8.0s) | 1.00 | 0 | 20/20 | 0/0/20/0 | 20 (8.0s) | 18 → 18 |
| H+Pri vs Mage+Priest | 0.00 | 20 | - | - | 0 | 0.00 | 20 | - | - | 0 | 0 → 0 |
| H+Pri vs Warlock+Priest | 0.00 | 20 | - | - | 0 | 0.00 | 20 | - | - | 0 | 5 → 5 |
| H+Pri vs Warlock+Paladin | 0.00 | 20 | - | - | 0 | 0.00 | 20 | - | - | 0 | 3 → 3 |
| H+Pri vs Paladin+Warrior | 1.00 | 0 | 20/20 | 0/0/20/0 | 0 | 0.85 | 3 | 9/9 | 1/0/8/8 | 9 (7.6s) | 0 → 0 |
| H+Pri vs Rogue+Warrior | 1.00 | 0 | 0/20 | 0/0/20/0 | 0 | 1.00 | 0 | 0/0 +20u | 0/0/20/0 | 0 | 20 → 20 |
| H+Pri vs Priest+Paladin | 1.05 | 0 | 21/21 | 16/1/4/0 | 21 (2.2s) | 1.00 | 0 | 19/20 | 20/0/0/0 | 20 (0.8s) | 20 → 20 |
| H+Pri vs Warlock+Rogue | 1.90 | 0 | 18/38 | 20/5/13/0 | 0 | 0.50 | 10 | 10/10 | 0/6/4/0 | 0 | 20 → 10 |
| H+Pri vs Shaman+Rogue | 1.35 | 0 | 4/24 | 0/0/24/3 | 0 | 1.70 | 0 | 12/13 +20u | 0/1/32/1 | 10 (8.0s) | 0 → 2 |
| H+War vs Rogue+Priest | 1.00 | 0 | 0/20 | 20/0/0/0 | 0 | 0.00 | 20 | - | - | 0 | 2 → 19 |
| H+Pri+War vs Mage+Priest+Rogue | 1.00 | 0 | 0/20 | 20/0/0/0 | 0 | 0.00 | 20 | - | - | 0 | 5 → 1 |
| H+Pri+War vs Warlock+Priest+Rogue | 1.00 | 0 | 0/20 | 20/0/0/0 | 0 | 0.05 | 19 | 1/1 | 0/0/1/0 | 1 (8.0s) | 4 → 3 |
| H+Pri vs Rogue+Paladin | 1.05 | 0 | 0/20 | 20/0/0/1 | 0 | 1.30 | 0 | 14/21 | 8/1/12/5 | 14 (6.1s) | 0 → 5 |
| H+Pri vs Warrior+Shaman | 0.95 | 1 | - | 0/0/0/19 | 0 | 1.25 | 0 | 16/16 | 0/0/16/9 | 16 (7.8s) | 20 → 16 |
| H+War vs Warrior+Priest | 1.00 | 0 | 0/20 | 0/0/20/0 | 0 | 0.00 | 20 | - | - | 0 | 20 → 20 |
| H+Mage vs Rogue+Priest | 1.00 | 0 | 0/20 | 20/0/0/0 | 0 | 0.00 | 20 | - | - | 0 | 3 → 14 |
| H+Wlk vs Rogue+Priest | 1.00 | 0 | 0/20 | 20/0/0/0 | 0 | 0.05 | 19 | 1/1 | 0/0/1/0 | 1 (8.0s) | 1 → 8 |
| H+Rogue vs Rogue+Priest | 1.00 | 0 | 0/20 | 20/0/0/0 | 0 | 0.00 | 20 | - | - | 0 | 11 → 2 |
| H+Pal vs Rogue+Priest | 1.00 | 0 | 0/20 | 20/0/0/0 | 0 | 1.30 | 0 | 16/19 | 3/0/16/7 | 16 (8.0s) | 1 → 12 |
| Pri+H vs Rogue+Priest | 1.85 | 0 | 9/29 | 20/0/9/8 | 0 | 1.00 | 0 | 13/13 | 0/0/13/7 | 13 (8.0s) | 10 → 6 |
| War+H vs Rogue+Priest | 1.00 | 0 | 0/20 | 20/0/0/0 | 0 | 0.70 | 6 | 5/5 | 0/1/4/9 | 2 (8.0s) | 20 → 2 |
| Pal+H vs Rogue+Priest | 2.05 | 0 | 16/41 | 34/0/7/0 | 5 (8.0s) | 1.00 | 0 | 20/20 | 0/0/20/0 | 20 (8.0s) | 8 → 10 |
| H vs Warrior | 1.00 | 0 | 20/20 | 0/0/20/0 | 0 | 1.00 | 0 | 20/20 | 0/0/20/0 | 0 | 20 → 20 |
| H vs Rogue | 0.00 | 20 | - | - | 0 | 0.00 | 20 | - | - | 0 | 7 → 7 |
| H vs Priest | 1.00 | 0 | 3/3 | 0/0/3/17 | 3 (8.0s) | 1.00 | 0 | - | 0/0/0/20 | 0 | 20 → 17 |
| H vs Warlock | 1.00 | 0 | 0/20 | 0/0/20/0 | 0 | 1.00 | 0 | 20/20 | 0/0/20/0 | 0 | 12 → 12 |

### Per comp, kill targets at slot 0

| comp | before: traps/m | zero | decided = springer | fate | on a healer | after: traps/m | zero | decided = springer | fate | on a healer | Hunter wins |
|---|---|---|---|---|---|---|---|---|---|---|---|
| H+Pri vs Rogue+Priest | 1.50 | 0 | 0/20 | 20/0/0/10 | 0 | 0.55 | 9 | 1/1 | 0/0/1/10 | 1 (8.0s) | 20 → 11 |
| H+Pri vs Warrior+Priest | 0.00 | 20 | - | - | 0 | 1.00 | 0 | 20/20 | 0/0/20/0 | 20 (8.0s) | 0 → 20 |
| H+Pri vs Mage+Priest | 0.00 | 20 | - | - | 0 | 0.00 | 20 | - | - | 0 | 0 → 1 |
| H+Pri vs Warlock+Priest | 0.00 | 20 | - | - | 0 | 0.00 | 20 | - | - | 0 | 0 → 0 |
| H+Pri vs Warlock+Paladin | 0.00 | 20 | - | - | 0 | 0.00 | 20 | - | - | 0 | 1 → 1 |
| H+Pri vs Paladin+Warrior | 0.60 | 8 | 2/2 | 0/2/0/10 | 2 (0.9s) | 1.15 | 0 | 17/18 | 0/0/18/5 | 17 (7.5s) | 20 → 20 |
| H+Pri vs Rogue+Warrior | 1.00 | 0 | 20/20 | 0/0/20/0 | 0 | 1.00 | 0 | 0/0 +20u | 0/0/20/0 | 0 | 20 → 20 |
| H+Pri vs Priest+Paladin | 1.90 | 0 | 28/30 | 22/0/8/8 | 30 (3.9s) | 1.90 | 0 | 29/31 | 21/0/10/7 | 31 (4.4s) | 18 → 19 |
| H+Pri vs Warlock+Rogue | 1.05 | 0 | 1/21 | 20/0/1/0 | 0 | 0.00 | 20 | - | - | 0 | 11 → 0 |
| H+Pri vs Shaman+Rogue | 1.00 | 0 | 0/20 | 0/0/20/0 | 0 | 1.00 | 0 | 0/0 +20u | 0/0/20/0 | 0 | 3 → 0 |
| H+War vs Rogue+Priest | 1.00 | 0 | 0/20 | 20/0/0/0 | 0 | 0.00 | 20 | - | - | 0 | 20 → 4 |
| H+Pri+War vs Mage+Priest+Rogue | 0.00 | 20 | - | - | 0 | 0.40 | 12 | 2/4 | 0/0/4/4 | 2 (8.0s) | 14 → 13 |
| H+Pri+War vs Warlock+Priest+Rogue | 0.05 | 19 | 1/1 | 0/1/0/0 | 0 | 0.00 | 20 | - | - | 0 | 18 → 18 |
| H+Pri vs Rogue+Paladin | 1.05 | 0 | 1/21 | 20/0/1/0 | 1 (8.0s) | 0.90 | 2 | 10/12 | 3/1/8/6 | 10 (5.8s) | 20 → 16 |
| H+Pri vs Warrior+Shaman | 0.65 | 7 | - | 0/0/0/13 | 0 | 1.00 | 0 | 8/8 | 0/0/8/12 | 8 (8.0s) | 17 → 20 |
| H+War vs Warrior+Priest | 0.00 | 20 | - | - | 0 | 1.00 | 0 | 10/10 | 0/0/10/10 | 10 (8.0s) | 0 → 19 |
| H+Mage vs Rogue+Priest | 1.25 | 0 | 0/20 | 20/0/0/5 | 0 | 0.00 | 20 | - | - | 0 | 20 → 3 |
| H+Wlk vs Rogue+Priest | 1.00 | 0 | 0/20 | 20/0/0/0 | 0 | 0.00 | 20 | - | - | 0 | 20 → 8 |
| H+Rogue vs Rogue+Priest | 1.00 | 0 | 0/20 | 20/0/0/0 | 0 | 0.05 | 19 | 1/1 | 0/0/1/0 | 1 (8.0s) | 20 → 20 |
| H+Pal vs Rogue+Priest | 1.25 | 0 | 1/21 | 20/0/1/4 | 1 (8.0s) | 1.00 | 0 | 19/19 | 0/0/19/1 | 19 (8.0s) | 20 → 17 |
| Pri+H vs Rogue+Priest | 1.00 | 0 | 0/20 | 20/0/0/0 | 0 | 1.00 | 0 | 9/10 | 0/0/10/10 | 9 (8.0s) | 20 → 17 |
| War+H vs Rogue+Priest | 1.10 | 0 | 0/20 | 20/0/0/2 | 0 | 1.00 | 1 | 18/18 | 0/0/18/2 | 18 (8.0s) | 19 → 14 |
| Pal+H vs Rogue+Priest | 1.60 | 0 | 11/31 | 20/0/11/1 | 11 (8.0s) | 0.55 | 12 | 10/10 | 0/0/10/1 | 10 (8.0s) | 20 → 20 |
| H vs Warrior | 1.00 | 0 | 20/20 | 0/0/20/0 | 0 | 1.00 | 0 | 20/20 | 0/0/20/0 | 0 | 20 → 20 |
| H vs Rogue | 0.00 | 20 | - | - | 0 | 0.00 | 20 | - | - | 0 | 7 → 7 |
| H vs Priest | 1.00 | 0 | 3/3 | 0/0/3/17 | 3 (8.0s) | 1.00 | 0 | - | 0/0/0/20 | 0 | 20 → 17 |
| H vs Warlock | 1.00 | 0 | 0/20 | 0/0/20/0 | 0 | 1.00 | 0 | 20/20 | 0/0/20/0 | 0 | 12 → 12 |

**Rogue + Priest.** Where the Rogue opens on the Hunter and nobody else on the
Hunter's team is killing the Priest, the trap goes on the Priest and runs its
full 8s: `H+Pri` 13 traps, `H+Pal` 16, `Pri+H` 13, `Pal+H` 20 (no kill
target). Four comps throw no trap, for reasons the rule states:

- **`H+War vs Rogue+Priest`:** the Warrior is killing the Priest ("a teammate
  is killing the healer") and its bleeds make the Priest untrappable ("cannot
  be trapped now"). While the Rogue is in stealth the Hunter begins no Aimed
  Shot, and the Rogue opens on the Hunter with Cheap Shot while the Warrior
  works on the Priest. 2 → 19 wins with no kill target; 20 → 4 with the kill
  target on the Rogue.
- **`H+Mage vs Rogue+Priest`:** the Mage is killing the Priest ("a teammate is
  killing the healer"). **`H+Wlk`:** the Warlock's DoTs make it untrappable.
- **`H+Pri+War vs Mage+Priest+Rogue`:** the Rogue opens on the Warrior, then
  the Priest, never the Hunter — the pressure the rule waits for never comes
  (with kill targets at 0 it does, and the trap catches the Priest).

**What the held opener costs: the reveal.** At `c79d8cc` the gates-open lane
trap sprang on the stealthed Rogue and **revealed** it; the Priest dispelled
it within a second, but the Rogue had lost its opener. That throw is held now
(its victim would just be dispelled), and the Rogue opens from stealth. It
carries the Rogue-comp losses below, and it is larger when the kill target is
the Rogue: then the Hunter's team was converging on the revealed Rogue from
the first second (`H+War vs Rogue+Priest` 20 → 4, `H+Mage` 20 → 3, `H+Wlk`
20 → 8, `H+Pri vs Warlock+Rogue` 11 → 0). AS-166 (Hunter Flare) is the planned
answer: a reveal on purpose, instead of a trap that happened to be one.
Against a Rogue with no dispeller the opening throw still goes and still
reveals it, and the Hunter stays on a trapped Rogue kill target for the team
to converge on (`Rogue+Shaman` 98.6% → 100% with kill targets at 0).

**Warlock + Rogue** no longer throws the gates-open trap the Felhunter
devoured every match; the later throws are decided on the Rogue and spring on
it (and the Hunter's own Aimed Shot breaks six of them, above).
**`Priest+Paladin`** keeps its off-target trap on one healer freed by the
other, by design (20 of the 31 removals with no kill target). **`Hunter vs
Priest`** throws at 1.0s every match and the Priest, closing on the Hunter,
stops short of the landing.

**Paladins.** Divine Shield lifted 4 of the 42 traps that sprang on a Paladin
with no kill target, and 17 of 56 with kill targets at 0 (at `c79d8cc`, 0 of
21 and 15 of 31). The trap is not held for it: the bubble lifts a trap only
when the Paladin's AI spends its 5-minute cooldown on it, and a trap that
draws it has spent the Paladin's one emergency button.

## Win rate (directional)

### 1v1 — 1,150 matches, 15 reachable + 8 control cells, n=50 per cell

```
CONTROL: 400/400 matches with no affected class on either side are IDENTICAL
  ALL reachable  n=750  49.3% -> 47.1%  -2.3pt  flips 55 (+10/-27)  z=2.63 SIG  resolves >=1.7pt
  CLEAN          n=350  41.7% -> 39.7%  -2.0pt  flips 7 (+0/-7)     z=2.27 SIG
  AGAINST        n=350  58.0% -> 60.3%  +2.3pt  flips 8 (+8/-0)     z=2.47 SIG
  MIRRORED       n=50   42.0% ->  6.0%  -36.0pt flips 40 (+2/-20)  z=3.62 SIG
NON-VACUITY: 1105/1150 ended by elimination; 284 moved in winner or duration
```

| enemy (1v1) | n | Hunter before | after | flips |
|---|---|---|---|---|
| **Priest** | 100 | 100.0% | **86.0%** | +0/-14 |
| Warlock | 100 | 59.0% | 58.0% | +0/-1 |
| Paladin | 100 | 0.0% | 0.0% | 0 |
| Warrior | 100 | 100.0% | 100.0% | 0 |
| Rogue | 100 | 34.0% | 34.0% | 0 |
| Mage | 100 | 0.0% | 0.0% | 0 |
| Shaman | 100 | 0.0% | 0.0% | 0 |

**Hunter vs Priest loses 14 of 100**, all as the side-swapped pair of the
same seven seeds. Traced at seed 7: both arms throw the opening trap and a
Concussive Shot, and the first decision that differs is the opener — Aimed
Shot where `c79d8cc` cast Serpent Sting; the Hunter dies at 34s of the log
instead of killing the Priest at 45s. The **Hunter mirror** goes from
21/20/9 (team 1 / team 2 / draw) to 3/8/39: both Hunters open with the same
Aimed Shot and race to simultaneous deaths, which count as draws. A 1v1 has
one enemy, so its kill target is that enemy either way.

### 2v2 — 3,090 matches, 301 reachable + 8 control cells, n=10 per cell

```
no kill target
CONTROL: 80/80 matches with no affected class on either side are IDENTICAL
  ALL reachable  n=3010  49.3% -> 49.6%  +0.3pt  flips 573 (+284/-274)  z=0.38 ns  resolves >=1.6pt
  CLEAN          n=1260  34.0% -> 36.1%  +2.1pt  flips 210 (+115/-88)   z=1.82 ns
  AGAINST        n=1260  64.9% -> 63.7%  -1.3pt  flips 218 (+99/-115)   z=1.03 ns
  MIRRORED       n=490   48.6% -> 48.4%  -0.2pt  flips 145 (+70/-71)    ns
NON-VACUITY: 3080/3090 ended by elimination; 2091 moved in winner or duration

kill targets at slot 0
CONTROL: 80/80 matches with no affected class on either side are IDENTICAL
  ALL reachable  n=3010  49.9% -> 49.7%  -0.2pt  flips 498 (+239/-245)  z=0.23 ns  resolves >=1.5pt
  CLEAN          n=1260  53.0% -> 52.5%  -0.5pt  flips 199 (+94/-100)   ns
  AGAINST        n=1260  47.4% -> 47.1%  -0.2pt  flips 199 (+98/-101)   ns
  MIRRORED       n=490   48.6% -> 49.2%  +0.6pt  flips 100 (+47/-44)    ns
NON-VACUITY: 3049/3090 ended by elimination; 2196 moved in winner or duration
```

Flat overall either way; the per-comp rows below move in both directions.

Per enemy comp, oriented to the Hunter's side (each row is 7 Hunter partners
x 2 sides x 10 seeds). About 60% of a row's side-swapped pairs replay the same
match mirrored, so a row's flips are not all independent and its z
overstates; the slices above carry the significance. The sweep puts the
Hunter in the second slot with every partner except the Shaman.

| enemy comp | healer | Rogue | n | no kill target: before | after | flips +/- | z | kill targets at 0: before | after | flips +/- | z |
|---|---|---|---|---|---|---|---|---|---|---|---|
| **Priest+Rogue** | Priest | yes | 140 | 37.9% | 30.0% | +33/-44 | -1.3 | 100.0% | **70.0%** | +0/-42 | -6.5 |
| **Rogue+Warlock** | - | yes | 140 | 97.1% | **70.0%** | +4/-42 | -5.6 | 95.7% | **66.4%** | +6/-47 | -5.6 |
| **Paladin+Rogue** | Paladin | yes | 140 | 6.4% | **26.4%** | +35/-7 | +4.3 | 55.0% | **20.7%** | +7/-55 | -6.1 |
| Rogue+Shaman | Shaman | yes | 140 | 47.1% | 50.7% | +8/-3 | +1.5 | 98.6% | 100.0% | +2/-0 | +1.4 |
| Mage+Rogue | - | yes | 140 | 77.1% | 75.0% | +0/-3 | -1.7 | 78.6% | 80.7% | +8/-5 | +0.8 |
| Rogue+Warrior | - | yes | 140 | 100.0% | 100.0% | 0 | 0 | 97.1% | 97.1% | 0 | 0 |
| Priest+Warlock | Priest |  | 140 | 25.7% | 22.1% | +0/-5 | -2.2 | 20.7% | 42.1% | +37/-7 | +4.5 |
| Mage+Warrior | - |  | 140 | 23.6% | 16.4% | +15/-25 | -1.6 | 37.9% | 41.4% | +5/-0 | +2.2 |
| Warlock+Warrior | - |  | 140 | 70.0% | 67.9% | +4/-7 | -0.9 | 51.4% | 76.4% | +40/-5 | +5.2 |
| Paladin+Warrior | Paladin |  | 140 | 10.0% | 10.0% | +12/-12 | 0 | 24.3% | 28.6% | +11/-5 | +1.5 |
| Mage+Priest | Priest |  | 140 | 15.7% | 15.7% | +2/-2 | 0 | 12.1% | 10.0% | +4/-7 | -0.9 |
| Mage+Shaman | Shaman |  | 140 | 5.7% | 5.7% | +2/-2 | 0 | 5.7% | 5.7% | 0 | 0 |
| Mage+Warlock | - |  | 140 | 21.4% | 22.9% | +6/-4 | +0.6 | 26.4% | 35.0% | +20/-8 | +2.3 |
| Mage+Paladin | Paladin |  | 140 | 0.7% | 1.4% | +1/-0 | +1.0 | 20.0% | 29.3% | +21/-8 | +2.4 |
| Paladin+Warlock | Paladin |  | 140 | 3.6% | 8.6% | +8/-1 | +2.3 | 27.9% | 29.3% | +2/-0 | +1.4 |
| Shaman+Warlock | Shaman |  | 140 | 4.3% | 11.4% | +12/-2 | +2.7 | 37.9% | 35.7% | +0/-3 | -1.7 |
| **Shaman+Warrior** | Shaman |  | 140 | 41.4% | **57.9%** | +44/-21 | +2.9 | 87.1% | 94.3% | +12/-2 | +2.7 |
| **Priest+Warrior** | Priest |  | 140 | 30.7% | **58.6%** | +46/-7 | +5.4 | 69.3% | 80.7% | +18/-2 | +3.6 |
| **all 18** | | | 2520 | 34.4% | 36.2% | +232/-187 | +2.2 | 52.5% | 52.4% | +193/-196 | -0.2 |

**The losses, plainly — accepted for now.** Against a Rogue with a dispeller
the Hunter loses the reveal: `Priest+Rogue` 37.9% → 30.0% with no kill target
and **100% → 70.0%** with it; `Rogue+Warlock` 97.1% → 70.0% and **95.7% →
66.4%** (with no kill target, partly the Hunter's own Aimed Shot breaking its
trap on the Rogue); `Paladin+Rogue` gains 20.0pt with no kill target and
**loses 34.3pt** with it (55.0% → 20.7%). With the kill target on the Rogue
these are the largest losses in the sweep, and the graphical client plays with
a kill target by default. The user has accepted them for now: AS-166 (Hunter
Flare) is the answer, to be sized against these kill-target figures. 1v1
Hunter vs Priest: 100% → 86%.

**The gains.** The melee + healer comps: `Priest+Warrior` +27.9pt,
`Shaman+Warrior` +16.5pt, `Paladin+Rogue` +20.0pt with no kill target; with
kill targets at 0 the Warlock comps gain (`Priest+Warlock` +21.4pt,
`Warlock+Warrior` +25.0pt, `Mage+Warlock` +8.6pt).

**The no-healer comps.** `Mage+Warrior` -7.2pt with no kill target and +3.5pt
with it; the others within noise.

**Warlock cells, explicitly.** 1v1 Hunter vs Warlock: 59.0% → 58.0% (one
flip). With no kill target the Warlock rows move with `Rogue+Warlock` above
and within noise otherwise; with kill targets at 0 the non-Rogue Warlock rows
gain. The Felhunter devouring a trapped teammate is untouched; a trapped
Felhunter stays trapped.

### What each piece is worth

Each attribution arm is the after binary with one piece taken out, same JSONL,
so the difference is that piece alone.

**The trade trap** (a non-healer trapped when the only teammate who could
free it is one the Hunter's team is killing — `…-arm-trade-trap.patch` puts
it back; no kill target). Removing it:

| enemy comp | with the trade trap | without | flips +/- |
|---|---|---|---|
| Priest+Rogue | 16.4% | 30.0% | +24/-5 |
| Paladin+Rogue | 23.6% | 26.4% | +8/-4 |
| Priest+Warrior | 62.9% | 58.6% | +2/-8 |
| Paladin+Warrior, Mage+Priest, Mage+Paladin | | | +10/-7 |
| the other 12 | | | identical |

Against Rogue + healer the trade trap caught the Rogue and was dispelled
within a GCD or two, so dropping it gains; against `Priest+Warrior` it cost
4.3pt.

**The positioning** (`trap_setup: 0.0`, no kite pull and no Disengage bend —
`…-arm-trap-setup-off.patch`; no kill target):

| enemy comp | setup off | setup on | flips +/- |
|---|---|---|---|
| Shaman+Warrior | 43.6% | 57.9% | +26/-6 |
| Priest+Rogue, Paladin+Rogue, Priest+Warrior, Paladin+Warrior | | | +20/-21 |
| the other 13 | | | identical |

**No Aimed Shot while an enemy is hidden** (`…-arm-opener-ignores-hidden.patch`
lets Aimed Shot begin while a Rogue is in stealth, in the opener and the
rotation; measured on the 1,250 Rogue configs). An Aimed Shot begun into a
stealthed Rogue is Kicked as it opens; the rule is worth:

| enemy comp | no kill target: Aimed Shot allowed | held | flips +/- | kill targets at 0: allowed | held | flips +/- |
|---|---|---|---|---|---|---|
| Priest+Rogue | 7.9% | 30.0% | +40/-9 | 50.7% | 70.0% | +40/-13 |
| Paladin+Rogue | 20.0% | 26.4% | +16/-7 | 20.0% | 20.7% | +9/-8 |
| Rogue+Warlock | 67.1% | 70.0% | +4/-0 | 51.4% | 66.4% | +24/-3 |
| Mage+Rogue | 82.1% | 75.0% | +2/-12 | 84.3% | 80.7% | +2/-7 |

## Baselines this invalidates

The Hunter cells of `canonical_1v1_n100_300s.csv` (the Hunter mirror and
Hunter vs Priest most of all), `canonical_2v2_full_n100_300s.csv` (every row
above that moved), and `canonical_3v3_full_n50_300s.csv` (3v3 was not swept;
the 3v3 comps in the mechanism set moved). Every cell without a Hunter is
unchanged — the controls are byte-identical in winner and duration, with and
without kill targets.

## Follow-ups

- **The Hunter breaks its own trap.** An Aimed Shot begun on an enemy before a
  trap springs on it lands and breaks it — 8 of 9 broken traps with no kill
  target, and `H+Pri vs Warlock+Rogue` 20 → 10. Aimed Shot does not ask
  whether its target is about to be trapped by the Hunter itself.
- **An opener counter to stealth, on purpose.** The measured value of the old
  gates-open throw was the reveal — `Priest+Rogue`, `Rogue+Warlock`,
  `Paladin+Rogue`, and far more so under the graphical client's default kill
  target. AS-166 (Hunter Flare) is that decision, sized against the
  kill-target figures above.
- **The opener against a lone Priest.** Aimed Shot first costs Hunter vs
  Priest 14 of 100 on Classic weapon speeds.
- **The Paladin AI never cleanses a trapped Warrior.** The removal predicate
  says it can; its AI does not, so every rule that asks "could they free it"
  over-counts the Paladin.
- **The Hunter mirror is a draw.** Identical openers race to simultaneous
  deaths in 39 of 50 matches.
- **AS-129** (healers dispel pets) moves the pet row of
  `who_can_free_a_freezing_trap` and nothing else; the Hunter AI follows it.
