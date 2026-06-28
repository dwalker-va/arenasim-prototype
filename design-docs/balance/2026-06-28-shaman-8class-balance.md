# Shaman (8th class) — balance sweep & summary

**Date:** 2026-06-28
**Build:** `feat/shaman-class` (Shaman shipped + tuned, PR #78)
**Harness:** `--batch` (canonical, internally deterministic), 300s cap.
Raw data: `2026-06-28_1v1_8class_n100_300s.csv`,
`2026-06-28_shaman_2v2_healer_framed_n100_300s.csv`.

## TL;DR

The Shaman is a **viable, competitive offensive healer — not underpowered.**
The earlier "0% / weak" read was a double-healer measurement artifact; framed
correctly (Shaman as a healer paired with a DPS), it is **as good as or better
than the Priest** for 4 of 5 DPS partners, and **trades with the Paladin**
(wins Rogue/Warlock comps, loses Warrior/Mage/Hunter comps to Paladin's
HoJ + bubble). Two outliers worth watching: **Rogue+Shaman is over-strong**
(84–98%) and **Mage+Shaman is weak** (3–38%, caster-redundancy, no peel for the
Mage). No buff is warranted.

## Machinery accommodation (8 classes)

Done so every sweep tool covers the Shaman:

- `src/headless/matrix.rs` — already iterates `CharacterClass::all()`, so the
  matrix auto-expanded to 8×8; refreshed the stale "7×7" doc comments.
- `scripts/gen_sweep.py` — `CLASSES` and `HEALERS` were hardcoded to 7 (no
  Shaman). Added Shaman to both (Shaman ∈ HEALERS so `--exclude-double-healer`
  treats Shaman+Priest as a double-healer comp).
- `scripts/{hunter,mage,shaman}_2v2_matrix.sh` — opponent lists were missing
  Shaman (and the Shaman script was missing Hunter); completed each to all seven
  other classes.

## Methodology

- **2v2 is authoritative; 1v1 is diagnostic only** (1v1 has ranged-vs-melee
  kiting asymmetries that are not balance bugs).
- The 2v2 read is **symmetrized**: each matchup runs N=100 with the Shaman on
  team1 *and* N=100 swapped, so team-order bias cancels. The Shaman-vs-Shaman
  mirror lands at exactly **50% decisive in every cell**, confirming the
  symmetrization is clean.
- "Decisive %" = Shaman-side wins / (wins + losses), i.e. which healer's team
  wins when the game resolves. Wilson 95% CIs on N=200.

## 1v1 — full 8×8 (diagnostic)

Overall winrate by class (team1, N=100 each; overall team1 = 44.6%, a known
slight team-order bias):

| Rank | Class | 1v1 winrate |
|---|---|---|
| 1 | Rogue | 81.0% |
| 2 | Mage | 63.2% |
| 3 | Paladin | 58.2% |
| 4 | Hunter | 44.9% |
| 5 | **Shaman** | **37.8%** |
| 6 | Warlock | 34.8% |
| 7 | Warrior | 28.4% |
| 8 | Priest | 8.9% |

Win grid (cell = team1 win%, row vs column; draws fill the remainder):

| t1\t2 | War | Mag | Rog | Pri | Wlk | Pal | Hun | Sha |
|---|---|---|---|---|---|---|---|---|
| **War** | 51 | 0 | 0 | 96 | 61 | 19 | 0 | 0 |
| **Mag** | 100 | 28 | 0 | 100 | 51 | 28 | 100 | 99 |
| **Rog** | 100 | 100 | 57 | 100 | 100 | 27 | 64 | 100 |
| **Pri** | 4 | 0 | 0 | 28 | 0 | 38 | 1 | 0 |
| **Wlk** | 33 | 46 | 0 | 100 | 42 | 24 | 23 | 10 |
| **Pal** | 81 | 73 | 73 | 10 | 75 | 47 | 100 | 7 |
| **Hun** | 100 | 0 | 36 | 99 | 85 | 0 | 39 | 0 |
| **Sha** | 100 | 1 | 0 | 1 | 87 | 0 | 100 | 13 |

**Shaman 1v1 read:** beats lone melee it can kite (Warrior 100, Hunter 100) and
the Warlock (87 — Purge strips defensives, Wind Shear locks Drain); gets kited
or trained by Mage/Rogue (1/0); stalemates the other healers (Priest, Paladin
near-all-draws). At 37.8% it sits mid-pack and **far above the pure-healer
Priest (8.9%)** — the offensive kit is doing exactly what it was designed to.
1v1 is not its frame; the numbers below are.

## 2v2 — healer-framed (authoritative)

Each DPS healed by the Shaman vs the **same DPS** healed by Priest / Paladin.
Decisive % = how often the Shaman-healed team wins the resolved games.

**Shaman vs Priest** (as the same DPS's healer):

| Comp | Shaman win% (N=200) | decisive % | verdict |
|---|---|---|---|
| Warrior+Shaman vs Warrior+Priest | 59.5% [53,66] | 73% | Shaman better |
| Rogue+Shaman vs Rogue+Priest | 84.5% [79,89] | 86% | Shaman much better |
| Mage+Shaman vs Mage+Priest | 38.0% [32,45] | 38% | Priest better |
| Hunter+Shaman vs Hunter+Priest | 100% [98,100] | 100% | Shaman dominates |
| Warlock+Shaman vs Warlock+Priest | 82.5% [77,87] | 82% | Shaman much better |

**Shaman vs Paladin:**

| Comp | Shaman win% (N=200) | decisive % | verdict |
|---|---|---|---|
| Warrior+Shaman vs Warrior+Paladin | 3.5% [2,7] | 5% | Paladin dominates |
| Rogue+Shaman vs Rogue+Paladin | 98.0% [95,99] | 98% | Shaman dominates |
| Mage+Shaman vs Mage+Paladin | 3.0% [1,6] | 3% | Paladin dominates |
| Hunter+Shaman vs Hunter+Paladin | 26.0% [20,32] | 31% | Paladin better |
| Warlock+Shaman vs Warlock+Paladin | 74.5% [68,80] | 82% | Shaman better |

Mirror control (Shaman vs Shaman) is 50% decisive in all five — clean.

## Interpretation

- **Shaman ≥ Priest as a 2v2 healer** for 4 of 5 DPS (only Mage prefers Priest).
  Its offensive kit (Lightning Bolt chip + Purge stripping enemy defensives +
  Wind Shear locking enemy heals) adds a win condition the passive Priest lacks.
  If anything this is a *Priest-is-outclassed* signal, not a Shaman-is-weak one.
- **Shaman trades with Paladin.** Paladin remains the premier healer for
  Warrior and Mage comps (HoJ stun + Divine Shield + melee presence hard-counter
  the no-CC Shaman: Warrior 5%, Mage 3%). The Shaman wins where its tempo
  compounds a fast-killing partner (Rogue 98%, Warlock 82%). This is the
  intended "CC-less, hard-vs-Paladin" identity, now quantified.
- **Rogue+Shaman is the standout (84–98% vs both healers)** — Rogue burst +
  Shaman Purge/Wind Shear/AP-totem is likely over-tuned. Watch it.
- **Mage+Shaman is the floor (3–38%)** — caster+caster redundancy with no peel
  for the Mage, and the enemy healer's defensives matter more vs a single
  ranged threat. This is a comp-synergy weakness, not a class-power problem.

## Verdict

**Balanced-to-slightly-strong. No buff.** The Shaman achieved its design goal:
an offensive healer that contributes to the kill rather than out-sustaining, and
it is competitive with the established healers. The open questions are *strength*
outliers, not weakness:

- Watch **Rogue+Shaman** — the most over-tuned cell. Hard to nerf without
  touching Rogue or broad Shaman offense; do not knee-jerk a single-class change
  (it washes out, per methodology). Re-measure before acting.
- The **Shaman-over-Priest** breadth may make the Priest feel obsolete in 2v2.
  That is a *Priest buff* conversation, not a Shaman nerf.
- **Mage+Shaman** weakness is acceptable comp identity; leave it.

## Follow-ups

- The canonical **2v2/3v3 full matrices** (`canonical_2v2_full_n100_300s.csv`,
  `canonical_3v3_full_n50_300s.csv`) are still 7-class and now stale. Regenerating
  them for 8 classes is a large sweep (8-class 2v2 ≈ 36 comps² × N) — deferred;
  do it before the next cross-class balance pass.
- `canonical_1v1_n100_300s.csv` **was** regenerated to 8×8 by this run.
- Totem buff magnitudes that feed both balance and tooltips live in
  `class_ai/shaman.rs`; a retune needs the matching `abilities.ron` tooltip edit.
