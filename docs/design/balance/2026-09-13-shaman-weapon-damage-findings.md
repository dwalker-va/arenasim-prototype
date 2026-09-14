# Shaman weapon damage: making the main-hand mace live (AS-97)

**Date:** 2026-09-13
**Change:** `Combatant::apply_equipment` picks the replacement socket from a new
`CharacterClass::weapon_slot()` instead of `CharacterClass::is_melee()`.
**Effect:** Shaman `attack_damage` 7.0 -> 12.5, `attack_speed` 0.8 -> 1.0. No
other class moves.

---

## 1. What was wrong

`apply_equipment` replaces a combatant's `attack_damage` / `attack_speed` from
exactly one socket, and every other equipped item only adds. The socket was
picked by `is_melee()`, which answers a *different* question — how far away the
class swings, at the auto-attack range gate in `combat_core/auto_attack.rs`.

Those two answers coincide for seven of the eight classes, so the conflation
looked correct for as long as those seven were the only classes. The Shaman
breaks the coincidence: it wields a MainHand mace but attacks at wand range. The
conflated predicate therefore sent it to its **Ranged** socket, which holds
`TotemOfLife` — a relic, not a weapon — so nothing ever replaced its class base
weapon stats. `Hammer of the Righteous` was inert.

There was no panic, no warning, and nothing in a match log saying so.

## 2. The fix

Split the two questions. `is_melee()` is untouched, so the auto-attack **range**
gate is unchanged and the Shaman still attacks at wand range. `weapon_slot()` is
a new exhaustive `match` with no `_` arm — MainHand for Warrior, Rogue, Paladin,
Shaman; Ranged for Mage, Priest, Warlock, Hunter — so adding a class is a
compile error rather than a silent default to the wrong socket.

**The no-op for the other seven classes is structural, not hoped-for.** Every
class except the Shaman maps to precisely what `is_melee()` returned before, so
their socket pick is unchanged by construction. §4 measures that it holds.

### A consequence worth naming

The Shaman now does a 12.5-average hit at 30 yards. The range is gated by
`is_melee()`, which this card deliberately does not move, so the mace currently
reads as a thrown one. That fiction is filed as its own card and is not
addressed here.

## 3. Positive attribution

Both arms read byte-identical config assets (`diff -rq assets/config` is clean),
so this is a code-only change and the two binaries differ only in the socket
pick.

The diff predicts two specific, checkable things about the Shaman's ranged auto
("Wand Shot" in the log): each hit should scale by 12.5 / 7.0 = **1.79x**, and
the swing interval should fall from 1 / 0.8 = 1.25s to 1 / 1.0 = 1.00s. One
seeded match (`Shaman+Priest` vs `Warrior+Priest`, seed 7, BasicArena) through
both binaries:

| | before | after |
|---|---|---|
| Wand Shot normal hit **vs the Warrior** (post-armor) | 5 | 9 (**1.80x**) |
| Modal swing interval | **1.27s** | **1.02s** |
| Shaman Wand Shots in the match | 20 | 59 |
| Match result | Team 2 at 42.4s | Team 1 at 72.4s |

Both predictions land. The interval sits a tick above nominal in both arms
because the fixed timestep quantises it, and the ratio is what the change
predicts. This is the difference attributed *positively* — not by elimination.

**Compare per target, not in aggregate.** Damage per hit is post-armor, so it
depends on who is being shot. In the before arm the Shaman only ever shoots the
Warrior (17 normal hits, all for 5). In the after arm it shoots both: the Warrior
for 9 and the cloth-wearing Priest for 12. The like-for-like comparison is
therefore the Warrior column above — 5 -> 9. Taking the mode across *both*
targets instead gives 5 -> 12, which overstates the change by mixing in a softer
target the before arm never reached.

## 4. Split control

The load-bearing shape: comps with **no Shaman on either side must be
byte-identical**, and comps with one **must differ**. A uniformly identical
result would mean the change never reached the sim; a uniformly differing one
would mean it reached more than the Shaman.

1400 paired matches, 2v2-with-healer, BasicArena, seeds 0-99 per cell, 300s cap.
Inputs and both CSVs are committed beside this doc.

| slice | n | identical | differing |
|---|---|---|---|
| **control** (no Shaman either side) | 500 | **500** | **0** |
| **clean** (team1 Shaman, team2 Priest) | 500 | 52 | 448 (89.6%) |
| **mirrored** (both sides Shaman) | 400 | 62 | 338 (84.5%) |

Both directions pass. The 500 control rows agree on winner, end reason and
duration to the printed precision.

### Non-vacuity

Every one of the 1400 matches in each arm ended by `kill` — none timed out at
the cap — and durations are near-continuous (382 / 408 / 234 distinct values per
slice before; 382 / 387 / 239 after). Draws: 4 of 500 in the clean slice, 0
elsewhere, identical in both arms.

Over a separate 12-seed log-level run of `Warrior+Shaman` vs `Warrior+Priest`:

| | before | after |
|---|---|---|
| Shaman Wand Shots | 261 | 195 |
| of which crits | 7 | 8 |
| total crits (all units) | 100 | 102 |
| total damage events | 1493 | 1233 |

The after-arm has *fewer* wand shots and damage events because the matches are
shorter, which is the expected direction.

(Count the Shaman as the *actor*: `Team \d Shaman #\d's Wand Shot`. A looser
"Shaman and Wand Shot on the same line" also matches the enemy Priest wanding
*at* the Shaman, which inflates these counts by roughly half.)

## 4a. The sweep read the right assets (CWD contamination re-check)

**A development build resolves its asset root to the RELATIVE path `assets`.**
`paths::install_root` returns `None` when `is_development_build` finds a
`Cargo.toml` above the executable — which is always true for
`<worktree>/target/release/arenasim` — and `assets_dir_from(None)` is then
literally `PathBuf::from("assets")`. Every RON config a run loads is therefore
resolved against the **process working directory**, not against the binary.

That makes a sweep silently readable from another worktree's asset tree if the
CWD is not what you think it is. It is not hypothetical here: another card was
concurrently mid-edit on `items.ron` and `loadouts.ron` — including
`HammerOfTheRighteous`, this card's exact item — and a run that picked those up
would look like a real result.

Re-verified rather than assumed, two ways:

1. **Positive asset check.** One seeded match per arm, absolute binary path and
   CWD forced to that arm's worktree, reading the Shaman's behaviour back out of
   the log: before 20 wand shots on a 1.27s cadence hitting the Warrior for 5,
   after 59 on 1.02s hitting the Warrior for 9. Those are the class-base and
   mace-equipped numbers respectively, so each binary demonstrably read its own
   `loadouts.ron` / `items.ron`. (`…-assetcheck.py` reports damage per target,
   because post-armor damage is only comparable against the same one — the after
   arm also shoots the cloth Priest for 12.)
2. **Re-run and diff.** Seeds 0-9 of all 14 cells (140 matches per arm) re-run
   from the committed input with the same hard pinning, then compared to the
   committed CSVs: **280/280 rows reproduce exactly** on winner, end reason and
   duration. (Run on the `3c61185` arms. The later full re-run on `323bd93` —
   §5 — is a stronger version of the same check: all 2800 rows regenerated from
   source on a rebuilt baseline, under the same hard pinning.)

A sample suffices because CWD is fixed for a process's lifetime, so this failure
mode is all-or-nothing per run, never a scattering of bad rows.

**For the next sweep:** pass an absolute binary path and an explicit CWD. A
relative `./target/release/arenasim` is doubly unsafe — under a drifted CWD it
resolves to *another tree's binary* as well as another tree's assets.

## 5. The measured delta

Paired by seed; McNemar over per-seed flips, Wilson 95% intervals on the level.

| slice | before | after | delta | gained / lost | McNemar |
|---|---|---|---|---|---|
| control | 63.4% [59.1, 67.5] | 63.4% [59.1, 67.5] | **+0.0 pt** | 0 / 0 | p = 1 |
| **clean** | 57.0% [52.6, 61.3] | 72.4% [68.3, 76.1] | **+15.4 pt** | 109 / 32 | **p = 5.0e-11** |
| mirrored | 51.7% [46.9, 56.6] | 50.2% [45.4, 55.1] | -1.5 pt | 19 / 25 | p = 0.45 |

**Clean slice** — the enemy has no Shaman, so the move is attributable to the
Shaman's own weapon: **+15.4 points**, 109 seeds flipped to a win against 32 the
other way, p = 5.0e-11. Not noise.

**Mirrored slice** — both sides field a Shaman, so the buff applies to both:
-1.5 points, 19 versus 25 flips, p = 0.45. Net-neutral, as it should be. The
individual outcomes still move on 85% of seeds; what does not move is who wins.

**Control slice** — zero flips, as §4 requires.

### Re-measured on the post-AS-54 base, and why the headline did not move

These figures are from a **complete re-run of both arms on `323bd93`** (main
after AS-54 made Frost Armor's chill one debuff). The baseline arm was rebuilt
from that commit — a rebase argument is evidence about the patch, not about
behaviour, and AS-54 is a real sim change.

The clean slice came back **bit-for-bit what it was on the old base** — same
57.0% -> 72.4%, same 109/32 flips — while control moved 64.6% -> 63.4% and
mirrored moved 56.5% -> 51.7%. That is a suspicious-looking coincidence, so it
was checked rather than accepted.

AS-54's chill fires when a **melee unit hits a Mage**, so it can only move a cell
that contains both — and where the melee unit actually attacks the Mage.
Comparing the old-base baseline against the new-base baseline cell by cell
(`…-as54-blast.py`), 13 of 14 cells match that prediction exactly. No clean-slice
cell pairs a Mage with a melee unit (`Mage+Shaman vs Mage+Priest` has no melee;
`Warrior+Shaman vs Warrior+Priest` has no Mage), so AS-54 is inert across the
whole slice and the delta it measures is untouched.

The fourteenth cell, `Warrior+Shaman vs Mage+Shaman`, has both and still did not
move. Checked directly rather than left as an exception: over six seeds the
Warrior lands **every** one of its attacks on the enemy Shaman and never once on
the Mage — it trains the healer — so the chill never fires there either. All 14
cells are accounted for.

### What this does NOT claim

This is a **delta between two binaries on one input file**, and nothing more. The
canonical class baselines are stale by user decision, so these figures say
nothing about where the Shaman now sits against any other class. Per-cell
figures are deliberately not broken out: at n=100 per cell they would be noise
around the slice-level values.

A +15.4 point swing is large enough that the Shaman's standing is worth
re-measuring against a fresh baseline before anything else is tuned around it.

## 6. Reproducing

```bash
# Generate the input (map is encoded in the label: --batch's CSV has no map
# column, so recording it any other way loses it)
docs/design/balance/2026-09-13-shaman-weapon-damage-gen.py 100 /tmp/sweep.jsonl

# Run it through each arm. ABSOLUTE binary path, and CWD forced to that arm's
# checkout -- a dev build reads `assets/` relative to the working directory
# (§4a), so a relative `cargo run` here would resolve BOTH the binary and the
# assets against wherever the reader happens to be standing.
ARM=/abs/path/to/the/checkout
(cd "$ARM" && "$ARM/target/release/arenasim" --batch /tmp/sweep.jsonl --out /tmp/arm.csv)

# Analyse the pair
docs/design/balance/2026-09-13-shaman-weapon-damage-analyze.py /tmp/before.csv /tmp/after.csv
```

The committed `sweep.jsonl` is the exact input both arms ran. "Before" was built
from `323bd93` (main after AS-54 merged); "after" from this card's branch
rebased onto that same commit. Both arms were rebuilt from source on that base —
see §5 for why the headline figure is unchanged from the earlier `3c61185` run.

## 7. The regression guard

`tests/weapon_slot_audit.rs` pins `weapon_slot()` against the socket each of the
8 shipped loadouts actually fills. That relation is the thing that was wrong —
the predicate was self-consistent, and so was the data; only the pairing was
broken, which no unit test of either side alone could see.

Verified to fail, not just to pass: mutating the predicate back yields
*"Shaman carries its weapon in MainHand, but CharacterClass::weapon_slot() names
Ranged ... the item in MainHand is inert"*, and deleting the mace from the
loadout yields *"Shaman should fill exactly one replacement-eligible weapon
socket, found []"*.

## 8. Follow-ups

- `tests/movement_probes.rs::juke_chase::juke_bounded_seed_b` needed re-pinning
  twice. Its 2v1 is `Mage+Priest vs Warrior+Shaman`, so BOTH this card and AS-54
  move it — and both cards independently re-picked it from the drifted seed 2
  (AS-54 chose 11, this card chose 34), neither measured on a tree carrying both
  changes. Resolved by re-running the probe's own `scan_seeds` on the merged
  tree: seed 11 survives as a candidate and is kept, with the bound re-derived
  from the observed 2 fizzle windows to 6 — tighter than the 8 it replaces. Seed
  11's dance is now thin (3.1s occlusion against a 1.0s vacuity floor), so the
  comment names robust alternatives for whoever re-pins next.
- The 30-yard thrown mace (its own card).
- Re-baseline the Shaman against a fresh canonical sweep.
