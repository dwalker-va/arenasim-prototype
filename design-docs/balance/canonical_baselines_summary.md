# Canonical Balance Baselines

**Generated 2026-06-28** — first full-matrix baseline with the **8th class, the
Shaman** (offensive totem healer; PR #78). All three formats regenerated on the
batch harness at 300s cap. Supersedes the 2026-06-22 7-class Crippling-Poison
baselines.

**Headline: the Shaman enters as the #2 team-format class (2v2 55.7%, 3v3
54.4%), behind only the Mage — and Mage+Shaman is the new #1 2v2 comp (82.0%).**
The offensive-healer design overshot the middle: its Flametongue spell-power
totem + Purge (strip enemy defensives) + Wind Shear (lock enemy heals) make a
ranged carry (esp. the Mage) substantially stronger, and unlike the pure
healers it adds its own kill pressure. Mage stays #1; Paladin and Priest both
slip a tier as the Shaman absorbs healer-slot share. **No buff — if anything the
Shaman is a mild nerf-watch at the top (Mage+Shaman 82%).**

Authoritative current-state references. Use as the "before" when assessing a
balance change — **compare batch-vs-batch only**, and **full-canonical vs
full-canonical**.

| Format | File | Coverage | N | Matches | Draws |
|---|---|---|---|---|---|
| 1v1 | `canonical_1v1_n100_300s.csv` | full 8×8 | 100 | 6,400 | 11.1% |
| 2v2 | `canonical_2v2_full_n100_300s.csv` | every distinct-class pair × pair (784) | 100 | 78,400 | 2.4% |
| 3v3 | `canonical_3v3_full_n50_300s.csv` | every distinct-class triple × triple (3,136) | 50 | 156,800 | 1.1% |

Distinct-class comps, both orderings, 300s cap, default loadouts/strategy.
Regenerate via `scripts/gen_sweep.py --full {2,3}` (and `--t1 '{p}' --t2-size 1`
for 1v1) + `arenasim --batch` (see the `balance-sweep` skill).

---

## Class tier lists (winrate of comps containing the class)

| Tier | 1v1 | 2v2 | 3v3 |
|---|---|---|---|
| **S — meta-defining** | **Rogue 81.0** | **Mage 64.6** | **Mage 62.4** |
| **A — strong** | Mage 63.2, Paladin 58.2 | **Shaman 55.7**, Paladin 53.0 | **Shaman 54.4**, Paladin 52.8, Priest 50.3 |
| **B — playable** | Hunter 44.9, **Shaman 37.8** | Rogue 47.2, Hunter 44.8, Priest 43.0 | Rogue 47.8 |
| **C — weak** | Warlock 34.8, Warrior 28.4 | Warrior 40.9, Warlock 40.8 | Warrior 43.6, Warlock 43.2, Hunter 40.9 |
| **D — bottom** | Priest 8.9 | — | — |

**The team meta is now Mage + Shaman**, with Paladin a clear third. The Shaman
debuts at A-tier #2 in both team formats; the Mage holds #1 and is *amplified*
by the Shaman partnership. **The Priest drops to mid-B in 2v2** (43.0) — the
Shaman is a strictly-better offensive healer in most comps. **Hunter is the 3v3
floor** (40.9), surfaced as Warlock/Warrior stay weak. 1v1 is unchanged in
character: Rogue the lone-target bully, the pure-healer Priest the basement, and
the Shaman mid-pack (37.8) but far above Priest — its offense carries 1v1 where
the Priest has none.

## 2v2 comp tier list (784 matchups)

**Meta-defining (top):**
| Winrate | Comp |
|---|---|
| **82.0%** | **Mage+Shaman** |
| 78.4% | Mage+Paladin |
| 72.1% | Mage+Priest |
| **70.8%** | **Rogue+Shaman** |
| 66.7% | Warrior+Paladin |
| 61.1% | Mage+Warlock |

**Unplayable (bottom):**
| Winrate | Comp |
|---|---|
| 33.4% | Warlock+Hunter |
| 33.3% | Warrior+Hunter |
| 30.0% | Warrior+Rogue |
| 22.6% | Rogue+Warlock |
| 18.9% | Warlock+Warrior |
| **8.8%** | **Priest+Paladin** |

**Mage+Shaman (82.0%) is the new top 2v2 comp**, edging out Mage+Paladin — the
Flametongue totem turns the Mage's burst lethal while Purge/Wind Shear strip the
enemy's answers. **Rogue+Shaman (70.8%) is the #4 comp** — the same tempo on a
sticky melee. Notably the **Shaman double-healer comps are playable** (Priest+
Shaman 37.2%, Paladin+Shaman 49.8%) unlike the pure Priest+Paladin floor (8.8%):
the offensive healer gives a double-healer comp a real win condition.

## 3v3 comp tier list (3,136 matchups)

**Meta-defining (top):**
| Winrate | Comp |
|---|---|
| **78.8%** | **Mage+Warlock+Shaman** |
| 77.7% | Mage+Warlock+Paladin |
| **77.1%** | **Mage+Priest+Shaman** |
| 77.0% | Mage+Rogue+Paladin |
| **75.9%** | **Mage+Paladin+Shaman** |
| 74.6% | Mage+Priest+Warlock |

**Unplayable (bottom):**
| Winrate | Comp |
|---|---|
| 26.6% | Priest+Paladin+Hunter |
| 19.1% | Priest+Paladin+Shaman |
| 18.6% | Rogue+Warlock+Hunter |
| 18.0% | Warrior+Rogue+Warlock |
| 16.8% | Warrior+Warlock+Hunter |
| **12.5%** | **Warrior+Rogue+Hunter** |

Mage-anchored Shaman triples own the top (Mage+Warlock+Shaman 78.8% is the new
#1). No-healer melee piles are the floor (Warrior+Rogue+Hunter 12.5%), with the
triple-healer Priest+Paladin+Shaman near the bottom (19.1% — three healers, not
enough kill pressure even with the Shaman's offense).

## What's meta-defining vs unplayable — the read

- **Mage is still clear #1 in teams** (64.6 / 62.4) and anchors every top comp;
  LoS/pillar play remains the deferred structural answer to its kiting.
- **The Shaman is the new #2** (55.7 / 54.4) and the story this cycle: a healer
  that adds damage. It pairs best with ranged carries — **Mage+Shaman is the top
  2v2 comp and Mage+*+Shaman the top 3v3s.** Watch it for over-tuning before
  considering any Shaman buff; the likely lever is its Flametongue spell-power
  magnitude (the Mage-amplifier) rather than its healing — edit `totem_spec` in
  `class_ai/shaman.rs` (tuning the magnitude there updates the tooltip too).
- **Paladin holds A-tier #3**; still the premier healer for melee/caster comps
  (it beats the Shaman head-to-head healing a Warrior or Mage via HoJ + bubble),
  just no longer the top healer overall.
- **The Priest slips to mid-B in 2v2** (43.0) — outclassed by the Shaman as an
  offensive healer in most pairings. A Priest-buff conversation, not a Shaman
  nerf.
- **Double/triple-healer and no-sustain melee piles remain the structural
  floors** (Priest+Paladin 8.8% 2v2; Warrior+Rogue+Hunter 12.5% 3v3) — though
  the Shaman makes *its own* double-healer comps playable.

## Changes this cycle (vs the 2026-06-22 7-class Crippling baseline)

- **The Shaman shipped (PR #78)** — an 8th class (mana ranged caster-healer,
  offensive slant): Lightning Bolt, Frost Shock, Lesser Healing Wave, Purge
  (offensive dispel), Wind Shear (ranged interrupt), and four element totems
  (Flametongue +SP, Windfury melee proc, Strength of Earth +AP, Healing Stream
  HoT). Tuning shipped with it (totem mana 12, Flametongue SP 18, AP 15, Windfury
  12%, purge priority floor, threat_repulsion 2.8).
- **Effect:** debuts #2 in 2v2 (55.7) and 3v3 (54.4); Mage+Shaman becomes the #1
  2v2 comp (82.0%). Mage edges up (61.6 → 64.6 in 2v2 as it gains the Shaman
  partner); Paladin (56.6 → 53.0) and Priest (46.7 → 43.0) slip as the Shaman
  takes healer-slot share. Pre-existing non-Shaman cells are otherwise stable.
- **Draw rates** 2v2 2.4% / 3v3 1.1% — no stall pathology from the new healer.
- See `2026-06-28-shaman-8class-balance.md` for the symmetrized healer-framed
  read (Shaman vs Priest/Paladin as the same DPS's healer) and the 1v1 detail.

## Caveats

- **Spawn-side asymmetry** up to ~18% in some matchups; the full matrix runs both
  orderings so tier lists average it out.
- **Batch harness order-sensitivity** (deferred): a few points off the historical
  multithreaded `--matrix` numbers. Compare batch-vs-batch only.
- **Default loadouts & strategy.** Strategy-var sweeps (poisons, openers, pets,
  totems, curses…) are a separate axis — see the `balance-sweep` skill.
