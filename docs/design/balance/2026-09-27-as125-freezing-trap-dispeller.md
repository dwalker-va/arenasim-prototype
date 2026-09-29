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
**Arms:** before = `main` at `17cb9f0`, whose headless output is
byte-identical to `bf76f09`'s (2,190 seeded matches compared row for row: the
whole 1v1 file and one 520-config chunk of each 2v2 file); after = this branch
on `bf76f09` (release binary sha1 `ef7598fb`). Same JSONL both arms. Three attribution
arms are the after binary with one piece taken out, each committed as a patch
beside the sweeps: `…-arm-trade-trap.patch`, `…-arm-trap-setup-off.patch`,
`…-arm-opener-ignores-hidden.patch`.
**Raw rows:** `2026-09-27-as125_{1v1,2v2,2v2_kt0}_{base_17cb9f0,after}.csv`
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

Until the Hunter's first Aimed Shot or Serpent Sting of the match, a due sting
waits behind Aimed Shot when the cast can finish (`aimed_shot_has_time`): no
enemy that could stop it reaches it first. Each visible such enemy has a
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
interrupt the cast`). The cast time is read from `abilities.ron`. After the opener, a due sting keeps
its place, so a sting re-applied after a Devour Magic is not delayed.

## The mechanism: where the traps went

27 trap-relevant comps (2v2, 3v3, 1v1), seeds 0-19, traced: 540 matches per
arm, once with no kill target and once with both kill targets at slot 0. The
set fields every melee + healer shape the healer trap reaches, and Rogue +
Priest in **both slot orders** (Hunter first, and partner first as the
directional sweep fields it).

| | no kill target: before | after | kill targets at 0: before | after |
|---|---|---|---|---|
| traps thrown | 501 | 319 | 416 | 358 |
| **sprung on the enemy the throw was decided on** | **103 of 451 named** | **196 of 204 named, + 40 unseen** | **84 of 344** | **228 of 236, + 40 unseen** |
| **sprung on a healer** | **31** (101s held in all) | **151** (1,005s) | **44** (233s) | **188** (1,308s) |
| removed by an enemy dispel or Divine Shield | 292 (0 by the bubble) | 26 (2) | 240 (15) | 26 (18) |
| broken by damage | 3 | 5 | 0 | 6 |
| ran the full duration | 156 | 213 | 104 | 244 |
| never sprung | 50 | 75 | 72 | 82 |

**The Hunter fights the melee while its trap holds the healer, kill target
or not.** With the Hunter's team told to kill the enemy healer
(`team1_kill_target: 1`) in the nine Rogue + healer and Warrior + Shaman
comps, 55 healer traps sprang on the Hunter's own kill target. Across them the
Hunter made **no** decision aimed at the frozen healer, and landed damage on
someone else during 29 of the 55. `hunter_on_victim_held` in the trap rows
counts this for every trap that sprang on the enemy it was thrown at. The
directional sweep never reaches this case: with kill targets at 0, the only
enemy comp with a healer in slot 0 is Priest+Warlock, which has no melee to
trigger the healer trap.

**The partner-healer exception, measured.** Holding the pressure trap whenever
a partner healer targets the enemy healer (the lane throw's own rule) was
built and measured on this set, with no kill target, on the branch before its
rebase onto `17cb9f0`: healer traps fell from 158 to 39 and Hunter-side wins
from 237 to 217 of 540 (`H+Pri vs Warrior+Priest` 20 → 2 traps, `H+Pal vs
Rogue+Priest` and `Pal+H vs Rogue+Priest` 25 and 18 → 0). What the exception
costs: a partner Priest's Mind Blast broke **4 of the 151** healer traps with
no kill target (`H+Pri vs Rogue+Priest` seeds 15 and 16, `H+Pri vs
Rogue+Paladin` seed 17, and one Shaman trap in `H+Pri vs Warrior+Shaman` that
had lain 39s before the Shaman walked into it) and **1 of the 188** with kill
targets at 0 (the same comp, a trap that had lain 25s). The other broken
traps: the Hunter's own Aimed Shot, begun on a Rogue before a trap sprang on
it (twice: a lane trap, and a healer trap the Rogue walked into instead of
its Paladin), a partner Warrior's Rend on a trapped Priest, a partner Mind
Blast on the Rogue where a healer trap sprang on the Rogue instead (twice),
and the Hunter's own Serpent Sting on a Shaman whose trap had lain 15s.

### Per comp, no kill target

Traps per match, matches with none, decided victim vs springer (`+Nu` counts
sprung throws decided on an enemy the Hunter could not see), fate as
removed/broken/full/unsprung, traps that sprang on a healer with the mean
seconds it was held (8.0s is the full trap). Hunter-side wins of 20.

| comp | before: traps/m | zero | decided = springer | fate | on a healer | after: traps/m | zero | decided = springer | fate | on a healer | Hunter wins |
|---|---|---|---|---|---|---|---|---|---|---|---|
| H+Pri vs Rogue+Priest | 1.00 | 0 | 0/20 | 20/0/0/0 | 0 | 0.85 | 3 | 13/14 | 0/2/12/3 | 13 (6.9s) | 1 → 3 |
| H+Pri vs Warrior+Priest | 0.15 | 17 | 2/2 | 0/0/2/1 | 2 (8.0s) | 1.00 | 0 | 20/20 | 0/0/20/0 | 20 (8.0s) | 19 → 19 |
| H+Pri vs Mage+Priest | 0.00 | 20 | - | - | 0 | 0.00 | 20 | - | - | 0 | 0 → 0 |
| H+Pri vs Warlock+Priest | 0.00 | 20 | - | - | 0 | 0.00 | 20 | - | - | 0 | 3 → 3 |
| H+Pri vs Warlock+Paladin | 0.00 | 20 | - | - | 0 | 0.00 | 20 | - | - | 0 | 1 → 1 |
| H+Pri vs Paladin+Warrior | 1.00 | 0 | 20/20 | 0/0/20/0 | 0 | 0.60 | 8 | 1/1 | 1/0/0/11 | 1 (0.0s) | 0 → 0 |
| H+Pri vs Rogue+Warrior | 1.00 | 0 | 0/20 | 0/0/20/0 | 0 | 1.00 | 0 | 0/0 +20u | 0/0/20/0 | 0 | 20 → 20 |
| H+Pri vs Priest+Paladin | 1.00 | 0 | 20/20 | 20/0/0/0 | 20 (0.6s) | 1.10 | 0 | 22/22 | 20/0/2/0 | 22 (1.0s) | 20 → 20 |
| H+Pri vs Warlock+Rogue | 1.90 | 0 | 18/38 | 20/3/15/0 | 0 | 0.15 | 17 | 3/3 | 0/1/2/0 | 0 | 20 → 19 |
| H+Pri vs Shaman+Rogue | 1.10 | 0 | 1/22 | 0/0/22/0 | 1 (8.0s) | 1.80 | 0 | 14/15 +20u | 0/0/35/1 | 13 (8.0s) | 0 → 0 |
| H+War vs Rogue+Priest | 1.00 | 0 | 0/20 | 20/0/0/0 | 0 | 0.00 | 20 | - | - | 0 | 0 → 0 |
| H+Pri+War vs Mage+Priest+Rogue | 1.00 | 0 | 0/20 | 20/0/0/0 | 0 | 0.00 | 20 | - | - | 0 | 3 → 0 |
| H+Pri+War vs Warlock+Priest+Rogue | 1.00 | 0 | 0/20 | 20/0/0/0 | 0 | 0.00 | 20 | - | - | 0 | 2 → 4 |
| H+Pri vs Rogue+Paladin | 1.00 | 0 | 0/20 | 20/0/0/0 | 0 | 1.05 | 0 | 6/11 | 4/1/6/10 | 6 (5.5s) | 0 → 5 |
| H+Pri vs Warrior+Shaman | 0.90 | 2 | - | 0/0/0/18 | 0 | 1.20 | 0 | 14/14 | 0/1/13/10 | 14 (7.2s) | 20 → 16 |
| H+War vs Warrior+Priest | 1.00 | 0 | 0/20 | 0/0/20/0 | 0 | 0.00 | 20 | - | - | 0 | 20 → 20 |
| H+Mage vs Rogue+Priest | 1.00 | 0 | 0/20 | 20/0/0/0 | 0 | 0.00 | 20 | - | - | 0 | 3 → 13 |
| H+Wlk vs Rogue+Priest | 1.00 | 0 | 0/20 | 20/0/0/0 | 0 | 0.00 | 20 | - | - | 0 | 1 → 8 |
| H+Rogue vs Rogue+Priest | 1.00 | 0 | 0/20 | 20/0/0/0 | 0 | 0.00 | 20 | - | - | 0 | 13 → 0 |
| H+Pal vs Rogue+Priest | 1.00 | 0 | 0/20 | 20/0/0/0 | 0 | 1.40 | 0 | 25/26 | 1/0/25/2 | 25 (8.0s) | 2 → 8 |
| Pri+H vs Rogue+Priest | 1.85 | 0 | 7/27 | 20/0/7/10 | 0 | 1.10 | 0 | 18/18 | 0/0/18/4 | 18 (8.0s) | 8 → 7 |
| War+H vs Rogue+Priest | 1.00 | 0 | 0/20 | 20/0/0/0 | 0 | 0.60 | 8 | 2/2 | 0/0/2/10 | 1 (8.0s) | 20 → 2 |
| Pal+H vs Rogue+Priest | 2.05 | 0 | 14/41 | 32/0/9/0 | 7 (8.0s) | 1.00 | 0 | 18/18 | 0/0/18/2 | 18 (8.0s) | 11 → 12 |
| H vs Warrior | 1.00 | 0 | 20/20 | 0/0/20/0 | 0 | 1.00 | 0 | 20/20 | 0/0/20/0 | 0 | 20 → 20 |
| H vs Rogue | 0.10 | 18 | - | 0/0/0/2 | 0 | 0.10 | 18 | - | 0/0/0/2 | 0 | 3 → 3 |
| H vs Priest | 1.00 | 0 | 1/1 | 0/0/1/19 | 1 (8.0s) | 1.00 | 0 | - | 0/0/0/20 | 0 | 17 → 18 |
| H vs Warlock | 1.00 | 0 | 0/20 | 0/0/20/0 | 0 | 1.00 | 0 | 20/20 | 0/0/20/0 | 0 | 16 → 15 |

### Per comp, kill targets at slot 0

| comp | before: traps/m | zero | decided = springer | fate | on a healer | after: traps/m | zero | decided = springer | fate | on a healer | Hunter wins |
|---|---|---|---|---|---|---|---|---|---|---|---|
| H+Pri vs Rogue+Priest | 1.55 | 0 | 0/20 | 20/0/0/11 | 0 | 0.70 | 6 | 7/7 | 0/0/7/7 | 7 (8.0s) | 20 → 12 |
| H+Pri vs Warrior+Priest | 0.00 | 20 | - | - | 0 | 1.00 | 0 | 20/20 | 0/0/20/0 | 20 (8.0s) | 0 → 20 |
| H+Pri vs Mage+Priest | 0.00 | 20 | - | - | 0 | 0.00 | 20 | - | - | 0 | 0 → 0 |
| H+Pri vs Warlock+Priest | 0.00 | 20 | - | - | 0 | 0.00 | 20 | - | - | 0 | 0 → 0 |
| H+Pri vs Warlock+Paladin | 0.05 | 19 | 1/1 | 0/0/1/0 | 1 (0.0s) | 0.05 | 19 | 1/1 | 0/0/1/0 | 1 (0.0s) | 0 → 0 |
| H+Pri vs Paladin+Warrior | 0.10 | 18 | - | 0/0/0/2 | 0 | 1.35 | 0 | 21/22 | 1/0/21/5 | 21 (6.1s) | 20 → 18 |
| H+Pri vs Rogue+Warrior | 1.00 | 0 | 20/20 | 0/0/20/0 | 0 | 1.00 | 0 | 0/0 +20u | 0/0/20/0 | 0 | 20 → 20 |
| H+Pri vs Priest+Paladin | 1.95 | 0 | 29/29 | 20/0/9/10 | 29 (4.2s) | 1.95 | 0 | 30/30 | 20/0/10/9 | 30 (4.4s) | 20 → 20 |
| H+Pri vs Warlock+Rogue | 1.05 | 0 | 0/20 | 20/0/0/1 | 0 | 0.00 | 20 | - | - | 0 | 9 → 0 |
| H+Pri vs Shaman+Rogue | 1.00 | 0 | 0/20 | 0/0/20/0 | 0 | 1.00 | 0 | 0/0 +20u | 0/0/20/0 | 0 | 0 → 0 |
| H+War vs Rogue+Priest | 1.00 | 0 | 0/20 | 20/0/0/0 | 0 | 0.00 | 20 | - | - | 0 | 20 → 0 |
| H+Pri+War vs Mage+Priest+Rogue | 0.00 | 20 | - | - | 0 | 0.80 | 4 | 10/12 | 1/1/10/4 | 10 (7.6s) | 7 → 7 |
| H+Pri+War vs Warlock+Priest+Rogue | 0.00 | 20 | - | - | 0 | 0.00 | 20 | - | - | 0 | 19 → 19 |
| H+Pri vs Rogue+Paladin | 1.00 | 0 | 0/20 | 20/0/0/0 | 0 | 0.95 | 1 | 13/17 | 3/3/11/2 | 13 (6.3s) | 20 → 19 |
| H+Pri vs Warrior+Shaman | 0.85 | 3 | - | 0/0/0/17 | 0 | 1.00 | 0 | 10/10 | 0/2/8/10 | 10 (6.8s) | 19 → 20 |
| H+War vs Warrior+Priest | 0.05 | 19 | - | 0/0/0/1 | 0 | 1.00 | 0 | 6/6 | 0/0/6/14 | 6 (8.0s) | 1 → 14 |
| H+Mage vs Rogue+Priest | 1.20 | 0 | 0/20 | 20/0/0/4 | 0 | 0.00 | 20 | - | - | 0 | 20 → 5 |
| H+Wlk vs Rogue+Priest | 1.00 | 0 | 0/20 | 20/0/0/0 | 0 | 0.00 | 20 | - | - | 0 | 20 → 9 |
| H+Rogue vs Rogue+Priest | 1.00 | 0 | 0/20 | 20/0/0/0 | 0 | 0.40 | 12 | 7/7 | 0/0/7/1 | 7 (8.0s) | 20 → 20 |
| H+Pal vs Rogue+Priest | 1.10 | 0 | 0/20 | 20/0/0/2 | 0 | 0.95 | 1 | 16/16 | 0/0/16/3 | 16 (8.0s) | 16 → 13 |
| Pri+H vs Rogue+Priest | 1.05 | 0 | 0/20 | 20/0/0/1 | 0 | 1.00 | 0 | 16/17 | 1/0/16/3 | 16 (8.0s) | 20 → 13 |
| War+H vs Rogue+Priest | 1.15 | 0 | 1/21 | 20/0/1/2 | 1 (8.0s) | 1.05 | 0 | 21/21 | 0/0/21/0 | 21 (8.0s) | 16 → 19 |
| Pal+H vs Rogue+Priest | 1.60 | 0 | 12/32 | 20/0/12/0 | 12 (8.0s) | 0.60 | 10 | 10/10 | 0/0/10/2 | 10 (8.0s) | 20 → 20 |
| H vs Warrior | 1.00 | 0 | 20/20 | 0/0/20/0 | 0 | 1.00 | 0 | 20/20 | 0/0/20/0 | 0 | 20 → 20 |
| H vs Rogue | 0.10 | 18 | - | 0/0/0/2 | 0 | 0.10 | 18 | - | 0/0/0/2 | 0 | 3 → 3 |
| H vs Priest | 1.00 | 0 | 1/1 | 0/0/1/19 | 1 (8.0s) | 1.00 | 0 | - | 0/0/0/20 | 0 | 17 → 18 |
| H vs Warlock | 1.00 | 0 | 0/20 | 0/0/20/0 | 0 | 1.00 | 0 | 20/20 | 0/0/20/0 | 0 | 16 → 15 |

**Rogue + Priest.** Where the Rogue opens on the Hunter and nobody else on the
Hunter's team is killing the Priest, the trap goes on the Priest and runs its
full 8s: `H+Pri` 13 traps, `H+Pal` 25, `Pri+H` 18, `Pal+H` 18 (no kill
target). Four comps throw no trap, for reasons the rule states:

- **`H+War vs Rogue+Priest`:** the Warrior is killing the Priest ("a teammate
  is killing the healer") and its bleeds make the Priest untrappable ("cannot
  be trapped now"). While the Rogue is in stealth the Hunter begins no Aimed
  Shot, and the Rogue opens on the Hunter with Cheap Shot while the Warrior
  works on the Priest. 0 → 0 wins with no kill target; 20 → 0 with the kill
  target on the Rogue.
- **`H+Mage vs Rogue+Priest`:** the Mage is killing the Priest ("a teammate is
  killing the healer"). **`H+Wlk`:** the Warlock's DoTs make it untrappable.
- **`H+Pri+War vs Mage+Priest+Rogue`:** the Rogue opens on the Warrior, then
  the Priest, never the Hunter — the pressure the rule waits for never comes
  (with kill targets at 0 it does, and the trap catches the Priest).

**What the held opener costs: the reveal.** At `17cb9f0` the gates-open lane
trap sprang on the stealthed Rogue and **revealed** it; the Priest dispelled
it within a second, but the Rogue had lost its opener. That throw is held now
(its victim would just be dispelled), and the Rogue opens from stealth. It is
the whole of the Rogue-comp losses below, and it is larger when the kill
target is the Rogue: then the Hunter's team was converging on the revealed
Rogue from the first second (`H+War vs Rogue+Priest` 20 → 0, `H+Mage` 20 → 5,
`H+Wlk` 20 → 9, `H+Pri vs Warlock+Rogue` 9 → 0). AS-166 (Hunter Flare) is the
planned answer: a reveal on purpose, instead of a trap that happened to be
one. Against a Rogue with no dispeller the opening throw still goes and still
reveals it, and the Hunter stays on a trapped Rogue kill target for the team
to converge on (`Rogue+Shaman` 98.6% → 100% with kill targets at 0).

**Warlock + Rogue** no longer throws the gates-open trap the Felhunter
devoured every match; the three later throws are decided on the Rogue running
the lane and spring on it. **`Priest+Paladin`** keeps its off-target trap on
one healer freed by the other, by design (20 of the 26 removals with no kill
target). **`Hunter vs Priest`** throws at 1.0s every match and the Priest,
closing on the Hunter, stops short of the landing; the win rate is unchanged.

**Paladins.** Divine Shield lifted 2 of the 29 traps that sprang on a Paladin
with no kill target, and 18 of 65 with kill targets at 0 (at `17cb9f0`, 0 of
20 and 15 of 30). The trap is not held for it: the bubble lifts a trap only
when the Paladin's AI spends its 5-minute cooldown on it, and a trap that
draws it has spent the Paladin's one emergency button.

## Win rate (directional)

### 1v1 — 1,150 matches, 15 reachable + 8 control cells, n=50 per cell

```
CONTROL: 400/400 matches with no affected class on either side are IDENTICAL
  ALL reachable  n=750  49.9% -> 47.1%  -2.8pt  flips 44 (+3/-24)  z=3.85 SIG  resolves >=1.5pt
  CLEAN          n=350  40.3% -> 40.0%  -0.3pt  flips 3 (+1/-2)    ns
  AGAINST        n=350  60.3% -> 60.3%  +0.0pt  flips 2 (+1/-1)    ns
  MIRRORED       n=50   44.0% ->  4.0%  -40.0pt flips 39 (+1/-21) z=4.05 SIG
NON-VACUITY: 1105/1150 ended by elimination; 271 moved in winner or duration
```

| enemy (1v1) | n | Hunter before | after | flips |
|---|---|---|---|---|
| Warlock | 100 | 74.0% | 73.0% | +0/-1 |
| Priest | 100 | 94.0% | 94.0% | +2/-2 |
| Paladin | 100 | 0.0% | 0.0% | 0 |
| Warrior | 100 | 100.0% | 100.0% | 0 |
| Rogue | 100 | 12.0% | 12.0% | 0 |
| Mage | 100 | 0.0% | 0.0% | 0 |
| Shaman | 100 | 0.0% | 0.0% | 0 |

Every non-mirror 1v1 cell moves by at most one net flip. The whole reachable
delta is the **Hunter mirror**: both Hunters now open with the same Aimed
Shot, and the mirror goes from 22/19/9 (team 1 / team 2 / draw) to 2/8/40 — a
symmetric race that ends in simultaneous deaths, which count as draws. A 1v1
has one enemy, so its kill target is that enemy either way.

### 2v2 — 3,090 matches, 301 reachable + 8 control cells, n=10 per cell

```
no kill target
CONTROL: 80/80 matches with no affected class on either side are IDENTICAL
  ALL reachable  n=3010  49.5% -> 49.4%  -0.1pt  flips 561 (+275/-277)  z=0.04 ns  resolves >=1.6pt
  CLEAN          n=1260  38.7% -> 38.4%  -0.2pt  flips 216 (+105/-108)  ns
  AGAINST        n=1260  60.6% -> 60.3%  -0.3pt  flips 218 (+107/-111)  ns
  MIRRORED       n=490   48.8% -> 49.8%  +1.0pt  flips 127 (+63/-58)    ns
NON-VACUITY: 3081/3090 ended by elimination; 2104 moved in winner or duration

kill targets at slot 0
CONTROL: 80/80 matches with no affected class on either side are IDENTICAL
  ALL reachable  n=3010  49.4% -> 49.9%  +0.5pt  flips 491 (+245/-229)  z=0.69 ns  resolves >=1.5pt
  CLEAN          n=1260  55.7% -> 53.3%  -2.4pt  flips 201 (+83/-113)   z=2.07 SIG
  AGAINST        n=1260  43.7% -> 46.6%  +2.9pt  flips 190 (+113/-77)   z=2.54 SIG
  MIRRORED       n=490   47.6% -> 49.6%  +2.0pt  flips 100 (+49/-39)    ns
NON-VACUITY: 3043/3090 ended by elimination; 2242 moved in winner or duration
```

With kill targets at 0 the Hunter's side loses about 2.5pt (CLEAN is the
Hunter as team 1, AGAINST as team 2, so both slices say the same thing).

Per enemy comp, oriented to the Hunter's side (each row is 7 Hunter partners
x 2 sides x 10 seeds). About 60% of a row's side-swapped pairs replay the same
match mirrored, so a row's flips are not all independent and its z
overstates; the slices above carry the significance. The sweep puts the
Hunter in the second slot with every partner except the Shaman.

| enemy comp | healer | Rogue | n | no kill target: before | after | flips +/- | z | kill targets at 0: before | after | flips +/- | z |
|---|---|---|---|---|---|---|---|---|---|---|---|
| **Priest+Rogue** | Priest | yes | 140 | 42.1% | **24.3%** | +19/-44 | -3.1 | 97.1% | **65.7%** | +4/-48 | -6.1 |
| **Rogue+Warlock** | - | yes | 140 | 91.4% | **75.0%** | +10/-33 | -3.5 | 100.0% | **59.3%** | +0/-57 | -7.5 |
| **Paladin+Rogue** | Paladin | yes | 140 | 12.1% | 21.4% | +26/-13 | +2.1 | 55.7% | **18.6%** | +3/-55 | -6.8 |
| Rogue+Shaman | Shaman | yes | 140 | 50.0% | 51.4% | +6/-4 | +0.6 | 98.6% | 100.0% | +2/-0 | +1.4 |
| Mage+Warrior | - |  | 140 | 40.0% | 31.4% | +14/-26 | -1.9 | 42.1% | 42.1% | 0 | 0 |
| Priest+Warlock | Priest |  | 140 | 30.7% | 26.4% | +3/-9 | -1.7 | 20.0% | 45.7% | +41/-5 | +5.3 |
| Warlock+Warrior | - |  | 140 | 82.9% | 79.3% | +4/-9 | -1.4 | 72.9% | 86.4% | +27/-8 | +3.2 |
| Mage+Paladin | Paladin |  | 140 | 5.0% | 3.6% | +3/-5 | -0.7 | 27.1% | 22.9% | +11/-17 | -1.1 |
| Paladin+Warrior | Paladin |  | 140 | 19.3% | 16.4% | +17/-21 | -0.6 | 38.6% | 46.4% | +14/-3 | +2.7 |
| Mage+Shaman | Shaman |  | 140 | 2.9% | 2.9% | 0 | 0 | 6.4% | 6.4% | 0 | 0 |
| Rogue+Warrior | - | yes | 140 | 100.0% | 100.0% | 0 | 0 | 97.1% | 97.1% | 0 | 0 |
| Shaman+Warlock | Shaman |  | 140 | 11.4% | 12.9% | +7/-5 | +0.6 | 37.1% | 36.4% | +1/-2 | -0.6 |
| Mage+Warlock | - |  | 140 | 17.9% | 20.0% | +9/-6 | +0.8 | 25.0% | 29.3% | +13/-7 | +1.3 |
| Mage+Priest | Priest |  | 140 | 12.1% | 13.6% | +4/-2 | +0.8 | 11.4% | 12.1% | +5/-4 | +0.3 |
| Mage+Rogue | - | yes | 140 | 77.9% | 80.0% | +6/-3 | +1.0 | 77.1% | 81.4% | +12/-6 | +1.4 |
| Paladin+Warlock | Paladin |  | 140 | 4.3% | 7.9% | +8/-3 | +1.5 | 33.6% | 30.0% | +2/-7 | -1.7 |
| **Shaman+Warrior** | Shaman |  | 140 | 46.4% | **63.6%** | +42/-18 | +3.1 | 90.7% | 97.9% | +11/-1 | +2.9 |
| **Priest+Warrior** | Priest |  | 140 | 54.3% | **71.4%** | +36/-12 | +3.5 | 69.3% | 76.4% | +12/-2 | +2.7 |
| **all 18** | | | 2520 | 38.9% | 39.0% | +214/-213 | +0.0 | 55.6% | 53.0% | +158/-222 | -3.3 |

**The losses, plainly.** Against a Rogue with a dispeller the Hunter loses
the reveal: `Priest+Rogue` 42.1% → 24.3% with no kill target and 97.1% →
65.7% with it; `Rogue+Warlock` 91.4% → 75.0% and 100% → 59.3%;
`Paladin+Rogue` gains 9.3pt with no kill target and loses 37.1pt with it.
With the kill target on the Rogue these are the largest moves in the sweep,
and the graphical client plays with a kill target by default.

**The gains.** The melee + healer comps: `Priest+Warrior` +17pt, `Shaman+Warrior`
+17pt with no kill target; with kill targets at 0 the Warlock comps gain
(`Priest+Warlock` +25.7pt, `Warlock+Warrior` +13.5pt).

**The no-healer comps.** `Mage+Warrior` -8.6pt with no kill target and
unchanged with it; the others within noise.

**Warlock cells, explicitly.** 1v1 Hunter vs Warlock: 74.0% → 73.0% (one
flip). With no kill target the Warlock rows move with the reveal
(`Rogue+Warlock`) and within noise otherwise; with kill targets at 0 the
non-Rogue Warlock rows gain. The Felhunter devouring a trapped teammate is
untouched; a trapped Felhunter stays trapped.

### What each piece is worth

Each attribution arm is the after binary with one piece taken out, same JSONL,
so the difference is that piece alone.

**The trade trap** (a non-healer trapped when the only teammate who could
free it is one the Hunter's team is killing — `…-arm-trade-trap.patch` puts
it back; no kill target). Removing it:

| enemy comp | with the trade trap | without | flips +/- |
|---|---|---|---|
| Priest+Rogue | 12.9% | 24.3% | +20/-4 |
| Paladin+Rogue | 13.6% | 21.4% | +14/-3 |
| Priest+Warlock, Priest+Warrior, Mage+Paladin, Mage+Priest | | | +16/-13 |
| **Paladin+Warrior** | 25.0% | **16.4%** | +2/-14 |
| the other 11 | | | identical |

Against Rogue + healer the trade trap caught the Rogue and was dispelled
within a GCD or two, so dropping it gains. Against `Paladin+Warrior` it caught
the Warrior and ran the full 8s every time — 20 of 20 in `H+Pri vs
Paladin+Warrior` — because the Paladin AI does not cleanse a trapped Warrior,
although the engine lets it. The rule asks what an enemy could do, not what
its AI does; that is its cost here.

**The positioning** (`trap_setup: 0.0`, no kite pull and no Disengage bend —
`…-arm-trap-setup-off.patch`; no kill target):

| enemy comp | setup off | setup on | flips +/- |
|---|---|---|---|
| Shaman+Warrior | 53.6% | 63.6% | +25/-11 |
| Priest+Rogue | 19.3% | 24.3% | +8/-1 |
| Priest+Warrior | 67.1% | 71.4% | +15/-9 |
| Paladin+Rogue, Priest+Warlock | | | +7/-4 |
| **Paladin+Warrior** | 27.9% | **16.4%** | +7/-23 |
| the other 12 | | | identical |

The one regression is `Paladin+Warrior`, carried by two partners: Paladin
(14 → 2 wins of 20) and Shaman (9 → 1). Traced at seed 1 (`Warrior+Paladin`
vs `Paladin+Hunter`): the kite pull toward the enemy Paladin puts the trap's
landing where the Paladin never walks, so it lies unsprung, where the unbent
kite's trap catches it; the Hunter dies at 46s of the log instead of 151s.
The pull is kept (the user's ruling); what makes a Paladin the healer it
misjudges is not isolated beyond this — it fights in melee range, so its led
position is a worse guess of where it will be than a Priest's.

**No Aimed Shot while an enemy is hidden** (`…-arm-opener-ignores-hidden.patch`
lets Aimed Shot begin while a Rogue is in stealth, in the opener and the
rotation; measured on the 1,250 Rogue configs). An Aimed Shot begun into a
stealthed Rogue is Kicked as it opens; the rule is worth:

| enemy comp | no kill target: Aimed Shot allowed | held | flips +/- | kill targets at 0: allowed | held | flips +/- |
|---|---|---|---|---|---|---|
| Priest+Rogue | 4.3% | 24.3% | +31/-3 | 45.7% | 65.7% | +46/-18 |
| Paladin+Rogue | 12.9% | 21.4% | +18/-6 | 16.4% | 18.6% | +5/-2 |
| Rogue+Warlock | 77.9% | 75.0% | +4/-8 | 47.9% | 59.3% | +23/-7 |
| Mage+Rogue | 82.9% | 80.0% | +2/-6 | 85.0% | 81.4% | +2/-7 |

## Baselines this invalidates

The Hunter cells of `canonical_1v1_n100_300s.csv` (the Hunter mirror most of
all), `canonical_2v2_full_n100_300s.csv` (every row above that moved), and
`canonical_3v3_full_n50_300s.csv` (3v3 was not swept; the 3v3 comps in the
mechanism set moved). Every cell without a Hunter is unchanged — the controls
are byte-identical in winner and duration, with and without kill targets.

## Follow-ups

- **An opener counter to stealth, on purpose.** The measured value of the old
  gates-open throw was the reveal — `Priest+Rogue`, `Rogue+Warlock`,
  `Paladin+Rogue`, and far more so under the graphical client's default kill
  target. AS-166 (Hunter Flare) is that decision.
- **The Paladin AI never cleanses a trapped Warrior.** The removal predicate
  says it can; its AI does not, so every rule that asks "could they free it"
  over-counts the Paladin.
- **The Hunter mirror is a draw.** Identical openers race to simultaneous
  deaths in 40 of 50 matches.
- **A cast in flight breaks the trap.** A damaging cast begun on an enemy
  before a trap springs on it — the Hunter's own Aimed Shot on a lane trap's
  victim — lands and breaks it. The throw does not ask what is already on its
  way to the victim.
- **AS-129** (healers dispel pets) moves the pet row of
  `who_can_free_a_freezing_trap` and nothing else; the Hunter AI follows it.
