# Caster one-handers — what filling the main-hand socket was worth (AS-87)

Every one-handed weapon a Mage or a Warlock could hold carried attack power and
nothing else. Both train dagger, one-handed sword, staff and wand — no mace —
and the only main-hand items with a caster stat on them were two one-handed
maces and two two-handed staves. So their only main hand worth equipping was a
staff, which costs them the off-hand. The dagger-plus-tome loadout was
unrepresentable, and the staff-versus-one-hander choice was fake: one option
carried a stat and the other did not.

The Priest was a different problem with the same symptom. It may use one-handed
maces, so `MaceOfTheRedeemer` was always legal for it — its main hand was empty
for no reason at all.

AS-87 adds three one-handers that carry spell power, and gives all three caster
classes one by default.

## What shipped

| item | tier | type | ilvl | stats | budget |
|---|---|---|---|---|---|
| Witchblade | base | 1H dagger | 58 | sp 5, mana 6 | 13.5 / 24.5 (55%) |
| Claw of Chromaggus | 1 | 1H dagger | 73 | sp 7, mana 8, mp5 0.5 | 21.0 / 30.8 (68%) |
| Azuresong Mageblade | 1 | 1H sword | 73 | sp 8, hp 4, crit 1% | 19.0 / 30.8 (62%) |

All three are real WoW Classic items, looked up through the Wowhead Classic MCP
by id (13964, 19347, 17103), each with its own era-faithful icon.

**Mage, Warlock and Priest default to Witchblade**, so the measured change is
exactly **+5 spell power and +6 max mana** on each. Nothing else moves: the
dagger's 7-11 swing is INERT for all three, because `apply_equipment` takes
attack damage from the MainHand only when `class.is_melee()`, and none of them
is melee — they swing from the Ranged socket. The tier-1 pair is pool-only; no
default loadout equips it.

## Why they are spent under budget

A one-hander at full budget plus a 19-point off-hand would leave the staves
permanently dominated, because `slot_budget_multiplier` gives a two-hander the
same 0.5625 as a one-hander. Both staves already sit at 97-98% of their own
budget and cannot be raised to meet it.

The card asked whether a staff should out-stat a one-hander plus off-hand "by a
little". Under the current formula it cannot, and forcing it would mean cutting
the one-hander to about 5 budget points — which re-creates the dead slot this
card exists to remove. So Witchblade spends 55% instead:

    staff path      CrescentStaff                    sp 10, mana  9 = 24.0 points
    one-hand path   Witchblade + TomeOfKnowledge     sp 11, mana 16 = 32.5 points

That halves the gap a full-budget one-hander would have opened (43.5 points)
rather than closing it. **The real fix is the budget formula** — a two-hander
occupies two sockets and should draw two sockets' budget — and that is a
separate card, not this one.

## The guard

`every_socket_offers_a_useful_item` asserts that for every class and every
socket, an equippable item exists carrying a stat that class's own kit scales
with. The scaling stat is derived from `abilities.ron` rather than hardcoded, so
a class whose kit changes character changes what counts without anyone updating
a table.

It is the companion to AS-86's `every_class_can_fill_every_socket` and a
strictly stronger claim: "a legal item exists" and "a legal item worth
equipping exists" are different properties. A TWO-HANDER cannot be the item that
proves a main hand usefully fillable, which is what makes this catch the bug
rather than pass on the staff — `enforce_two_hand_conflicts` strips the
off-hand, so "fillable only by a two-hander" means the socket is a trade, not a
slot.

Run against main's data it fires on exactly `Mage/MainHand` and
`Warlock/MainHand`. It does not fire on Priest, matching the card's reading that
Priest was a loadout omission rather than a content hole. Two justified
exemptions are recorded with their reasons: the Rogue and Hunter off-hands,
where the off-hand is a second weapon (dual wield, AS-60) and a held-in-off-hand
frill is a caster stat stick by definition.

## The medic-chase scan — the clearest evidence the buff works

Seven movement probes broke on this change. The identical suite passes 107/107
on a dev build of main, so they are attributable to it rather than to drift.
Six were vacuity failures — a probe's own guard reporting that its pinned seed
no longer exercises its condition. The seventh needed distinguishing from a real
regression before re-pinning past it, so both binaries were scanned over 30
paired seeds on Warrior+Priest vs Warrior+Shaman — a ONE-SIDED buff, since the
Priest gains and the Shaman does not.

**The chase is not slower.** Its bound — longest contiguous occluded-distress
window <= 8s — is breached on no seed on either binary, and its maximum falls
5.40s -> 5.20s.

**What rose was the NUMBER of windows, not their length.** The fraction of match
spent occluded from a distressed ally roughly doubles, 0.029 -> 0.061. The
reason is the opposite of a defect:

| | before | after |
|---|---|---|
| seeds where an ally died before a heal landed | 5 | 1 |
| team-1 (buffed Priest) wins across the scan | 16/30 | 19/30 |

A stronger Priest keeps its Warrior ALIVE at low HP instead of letting it die,
so there are more rescues to perform. More rescues, not slower ones. This is the
clearest evidence in the card that the buff does what it is meant to.

**On match length.** Three of thirty seeds (0, 2, 17) stop resolving and run to
the probe's 200s cap where they previously ended in 37-46s. That is worth
knowing, and it is now card AS-106. But it must not be read as "the change
lengthens matches": excluding those three, the paired duration delta is
**-0.98s, with 12 seeds longer and 14 shorter**. Matches did not get longer.
Three of them stopped ending.

One more thing the scan turned up, now card AS-105: the probe's 10s
time-to-heal ceiling is already breachable on main. Seed 12 breaches it at
13.27s on BOTH binaries — the identical value, that seed being untouched here.
That ceiling holds at the seeds it is pinned to, not universally.

## The paired sweep

<!-- SWEEP RESULTS -->

## Reproducing this

Unlike AS-86, the sweep's input is committed:
`2026-09-14-as87_caster_onehanders_sweep.jsonl`. Every line names its `map` and
`ai_profile`, so the file fully determines the run — the batch CSV has no column
for either.

    arenasim --batch docs/design/balance/2026-09-14-as87_caster_onehanders_sweep.jsonl \
      --out results.csv --jobs 8

Run it from the root of a tree holding the assets you mean to measure. **The sim
loads `assets/` relative to the process working directory**, so a run whose CWD
is not pinned to its own tree can silently read another one's `items.ron`.
