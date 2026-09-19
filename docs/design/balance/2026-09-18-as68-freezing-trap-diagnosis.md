# Why Freezing Traps still get dispelled — measurement

**Date:** 2026-09-18
**Card:** AS-68
**Raw rows:** `2026-09-18-as68_trap_events.csv` (one row per trap that sprang),
`2026-09-18-as68_pet_trap_events.csv` (the pet check in §"Can a pet be trapped")

AS-44 raised `Incapacitate` to `dispel_priority` 100 and then measured 9 of 11
Freezing Traps removed. A trapped healer cannot dispel itself, so the card asked
who did. This is the answer, and it is not the one the card expected.

**The trap is not being dispelled off the healer. It is landing on someone
else.** In 125 of 154 traps that sprang, the victim was not a healer at all —
and a non-healer is precisely the target an enemy healer can and will free.

## Method

300 headless matches, `Legacy` profile, BasicArena, 300s cap, seeds 0-24 across
12 comps chosen to separate the card's four candidate explanations: comps with
one enemy healer, two enemy healers, no enemy healer, a Felhunter with and
without a healer beside it, 1v1 where the healer IS the kill target, and two
3v3s. Every trap was then followed from placement to removal through the match
log, and a 12-match subset was re-run with the AI decision trace to attribute
the aim positively rather than by inference.

**Instrumentation added to make the attribution possible.** A trap is the only
ability whose victim is not its target: it is placed at a POSITION and springs
on the first enemy to reach it, so the outcome cannot say who the Hunter meant
to catch. `try_place_trap_at` recorded `builder.choose(ability, None, ..)` — no
target at all — which is why this question had never been answerable from a
sweep. It now records the intended victim. The change is trace-only:
**300/300 match logs byte-identical** before and after, over a batch carrying
300 deaths, 203 trap casts and 154 trap triggers.

## What ends a Freezing Trap

154 traps sprang across the 300 matches.

| fate | n | share |
|---|---|---|
| dispelled by the enemy **Priest** (Dispel Magic) | 74 | 48% |
| devoured by the enemy **Felhunter** (Devour Magic) | 25 | 16% |
| ran its full 8s | 55 | 36% |
| broke from damage | **0** | 0% |
| removed by the trapped unit itself | **0** | 0% |
| cleansed by an enemy **Paladin** | **0** | 0% |

**Every one of the 99 removals was a third party freeing its own trapped ALLY.**
Not once did the trapped unit remove its own trap — it cannot, and it never did.

**The removals are effectively instant.** Median time from trigger to removal is
**0.28s**; 94 of the 99 are under one second. The longest is 5.27s. A trap that
lands on a dispeller's ally is not a crowd-control window, it is a spent GCD.

## Who gets trapped

| victim | n | dispelled | devoured | ran out |
|---|---|---|---|---|
| Rogue | 100 | 50 | 25 | 25 |
| Warrior | 25 | 0 | 0 | 25 |
| Paladin (healer) | 25 | 24 | 0 | 1 |
| Priest (healer) | 4 | 0 | 0 | 4 |

**29 of 154 traps caught an enemy healer. 125 caught a DPS.** The two rows with
no removals at all — Warrior and Priest — are the comps with no third party able
to reach the trap, not comps where the targeting worked.

## Three mechanisms, each attributed positively

### 1. The trap is aimed at the healer and springs on someone else

Traced, 12 matches, pairing each trap's recorded intended victim with the
combatant it actually sprang on:

| aimed at | sprang on | n | |
|---|---|---|---|
| Priest | Rogue | 6 | **MISS** |
| Warrior | Warrior | 3 | hit |
| Paladin | Paladin | 3 | hit |

Every miss is the same shape, and the trace names the cause. At gates-open the
enemy Rogue is **stealthed**, so `dip_target_eligible` rejects it
(`!info.stealthed`); the enemy healer is the Hunter's own kill target AND its
Priest teammate's, so it is in the focus set. `opportunistic_off_target` has no
candidate left and returns `None`, and control falls to the LEGACY peel branch,
which throws the trap at the **midpoint between the Hunter and the healer** —
at t=0, with the healer 70 yards away, that is dead arena centre. The Rogue
unstealths, runs down the middle, and eats it 15 seconds later. The enemy Priest
dispels it 0.3s after that.

The off-target branch ten lines above refuses to throw unless the intended
victim is the only enemy within the trigger radius of the landing
(`healer_triggers`). The fallback branch has no such guard, and its chosen
landing is by construction in the lane the enemy melee runs down.

### 2. When the aim works, it is aimed at a non-healer by design

The two hit rows are the off-target rule doing exactly what it specifies: trap
the enemy the team is NOT killing. Because the Hunter's kill target is the enemy
healer in both comps, the off-target is necessarily the enemy DPS. The trap
lands correctly on the Warrior or the Paladin — and hands the surviving healer a
free 18-mana GCD to undo it, now that `Incapacitate` outranks everything else it
could dispel.

`hp_v_palwar` is the exception that proves the rule: the Warrior is trapped
25/25 and freed 0/25, because the trap landed at (-27, -1) — next to the Hunter,
63 yards from the enemy Paladin, outside Cleanse range. The trap survives there
by accident of geometry, not by targeting.

### 3. The second healer and the Felhunter

Both candidate explanations the card named are real, and both are the *smaller*
half of the picture.

- **Double-healer comps:** `Hunter+Priest vs Priest+Paladin` is the only cell
  where a healer is reliably trapped (Paladin, 25/25) — and the enemy Priest
  frees it 24/25.
- **Felhunter:** `Hunter+Priest+Warrior vs Warlock+Priest+Rogue`, 25/25 traps
  devoured. The Felhunter is freeing the **Rogue**, not itself and not the
  Warlock.

The `unwrap_or(fallback)` no-healer path was also checked and is not implicated:
in `Hunter+Priest vs Rogue+Warrior` the trap lands on the Rogue 25/25 and runs
its full duration 25/25, because there is nobody who can remove it.

## Can a pet be trapped? Yes — and in Warlock comps it already is

The card left this open. It is settled in both directions, code and
measurement.

`trap_system` scans every `Combatant` on the opposing team with no pet filter,
and `apply_pending_auras` has none either, so a pet both triggers a trap and
receives the incapacitate. 125 further matches across five Warlock- and
pet-heavy comps confirm it: **52 Freezing Traps sprang, all 52 on a Felhunter,
and all 52 ran the full 8 seconds.** Not one was removed — `try_dispel_ally`
skips pets outright (`info.is_pet`), and a Felhunter cannot cast Devour Magic
while incapacitated, so a trapped Felhunter has nobody to free it.

Two consequences, and they pull in opposite directions:

- Mechanism 1 again, at its most extreme. In a Warlock comp the pet is
  *always* the body that reaches the midpoint first — the Warlock itself was
  never trapped once in 52 springs. The Hunter is already spending its trap on
  the Felhunter in every Warlock comp measured, unintentionally.
- **This is where the card's forbidden regression actually lives.** In
  `Hunter+Priest+Warrior vs Warlock+Priest+Rogue` the trap lands on the Rogue
  and the Felhunter devours it 25/25. A dispel-capability predicate would move
  the aim onto the Felhunter — which the measurement says sticks for the full
  8s — removing the Warlock's dispel from the fight by design. That is the
  intended counter being engineered out, so it is a regression to report, not a
  win to claim.

## What this means for the card's Phase 2

The card's proposed refinement is to prefer targets that **can dispel** rather
than targets that are healers. Against the measurement, that predicate does not
address what is going wrong:

- It cannot fix mechanism 1 at all. It changes who the Hunter AIMS at; the trap
  still lands at the midpoint and still springs on whoever arrives first. The
  six traced misses were already aimed at the healer.
- In the comps measured, the only place it changes the aim is toward the
  **Felhunter** — a dispeller that is not a healer, that a trap provably holds
  for the full 8s, and that nothing on its team can free. That is the Warlock's
  dispel taken off the board by design, which the card explicitly forbids.

The defect the measurement actually points at is **placement and trigger
ownership**, not target preference: a trap springs on the first body to reach
it, and the fallback branch aims it into the lane the enemy melee runs down. Two
follow-ups are named on the card rather than built here, because either is a
behaviour change with its own sweep.

## Caveat — AS-67 will move these numbers

Every removal in this set happens at a median of 0.28s because the enemy
healer's AI re-evaluates every tick and `Incapacitate` sits at the top of its
dispel table. AS-67 (trained-human reaction latency) puts a delay in front of
exactly that decision. The **identity** of the remover is a mechanism and will
not change — reaction latency changes when a dispel happens, not who can cast
one — so §"What ends a Freezing Trap" and §"Who gets trapped" survive it. The
0.28s median does not, and any win-rate claim built on this world would not
either. That is why nothing was tuned here.
