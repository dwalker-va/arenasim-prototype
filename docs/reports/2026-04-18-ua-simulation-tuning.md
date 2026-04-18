# Unstable Affliction — Simulation Tuning Report

**Branch:** `feat/unstable-affliction-warlock`
**Plan:** [docs/plans/2026-04-18-001-feat-unstable-affliction-warlock-plan.md](../plans/2026-04-18-001-feat-unstable-affliction-warlock-plan.md)
**Baseline:** [docs/reports/2026-04-12-bug-hunt-2v2-3v3.md](2026-04-12-bug-hunt-2v2-3v3.md)
**Mode:** Measure-and-recommend. No `abilities.ron` changes are applied without user approval.

---

## Iteration 1 — 2026-04-18

### Tuning under test
| Parameter | Value |
|---|---|
| `cast_time` | 1.5s |
| `mana_cost` | 30 |
| DoT damage / tick | 8 (×6 ticks over 18s = 48 base) |
| `break_on_damage` | -1.0 (never breaks) |
| Backlash `damage_base` | 40 |
| Backlash `damage_sp_coefficient` | 0.3 |
| Backlash `silence_duration` | 5.0s |
| AI priority | UA-first when enemy team has Priest or Paladin |

### Results vs baseline

| Bracket | Apr 12 baseline | Iter 1 | Δ |
|---|---|---|---|
| Warlock 2v2 WR | **20%** (1/5) | **0%** (0/5) | **−20pp** ❌ |
| Warlock 3v3 WR | **38%** (3/8) | **22%** (2/9) | **−16pp** ❌ |

Warlock WR **regressed** from baseline in both brackets. Exit gate at 3v3 ≥45% is not met (3v3 is at 22%). Per the plan, this is *not* a "raise the numbers and try again" situation — it's a signal the current shape may be wrong.

### UA mechanic activity

Across 24 matches:
- UA casts initiated: **22**
- UA dispelled by enemy: **7** (32% of casts)
- `[BACKLASH]` events fired: **7** (all 13 Warlock-bearing matches in 3v3 had ≤2)
- Total backlash damage: **327** (avg 47 per backlash event)
- Silence applied: **7**
- Mid-cast Silence interrupts: **0** (silence rarely lands during a healer's in-flight cast)

The mechanic *works* when it triggers — backlash + silence land cleanly. But the trigger rate is too low to compensate for the cost UA imposes on the Warlock's other damage output.

### Root cause analysis

**1. UA's 1.5s cast delays Warlock's damage ramp.**

In matches where Warlock dies in the 17-25s window (most 2v2s and several 3v3s), the 1.5s UA cast is a meaningful fraction of the Warlock's total uptime. They lose 1.5s of Corruption / Curse-of-Agony damage ramp for a UA that often gets cast on a target who never dispels it.

**2. UA per-tick damage is *lower* than Corruption.**

| DoT | dmg/tick | duration | total |
|---|---|---|---|
| Corruption | 10 | 18s (3s tick) | 60 |
| **UA** | **8** | **18s (3s tick)** | **48** |
| Curse of Agony | 14 | 24s (4s tick) | 84 |

When UA-first priority fires, the Warlock effectively trades a 60-damage DoT for a 48-damage DoT-plus-trap. If the trap doesn't trigger (15/22 casts), it's pure damage loss.

**3. Warlock dies before UA matters in 2v2.**

| Match | UA casts | UA dispelled | Warlock 1st death | Result |
|---|---|---|---|---|
| m02 | 1 | 0 | 17.0s | LOSS — 2 Frostbolts + crit (244 dmg) |
| m04 | 1 | 0 | 35.2s | LOSS |
| m07 | 1 | 0 | 30.4s | LOSS |
| m08 | 1 | 0 | 23.2s | LOSS |
| m12 | 2 | 0 | 26.6s | LOSS |

UA never got dispelled in any 2v2 — either the Warlock died before the dispel happened, or the enemy healer didn't get around to it before Warlock damage stopped mattering.

**4. UA-first priority has a hidden cost when the dispeller doesn't dispel UA.**

The mechanic only pays off if the enemy healer dispels UA. If they ignore it (perfectly rational once they learn the silence trap exists, but also if they're busy healing or not dispel-priorising UA over more dangerous CC), UA is a strict downgrade vs leading with Corruption.

### Recommended changes (3 options — pick one or combine)

#### Option A — Make UA instant (revert cast_time to 0.0)
**Pros:** removes the damage-ramp tax. UA goes back to being a free DoT load. Damage output ≈ baseline.
**Cons:** loses the "Warriors can Pummel UA mid-cast" interaction. Less authentic to TBC.
**Predicted lift:** 3v3 +10-15pp (back near baseline 38%, plus silence-fires upside ≈ 40-45%).
**Diff:** `cast_time: 1.5` → `cast_time: 0.0`.

#### Option B — Raise UA per-tick damage to 12 (above Corruption)
**Pros:** keeps the 1.5s cast as a deliberate trade. UA becomes a *premium* DoT — more damage than Corruption to justify the cast time.
**Cons:** if UA never gets dispelled, Warlock still loses the 1.5s ramp. Doesn't help the "dies first" case.
**Predicted lift:** 3v3 +5-10pp.
**Diff:** `applies_aura.magnitude: 8` → `12` in abilities.ron.

#### Option C — Drop the UA-first priority; go back to Corruption-first
**Pros:** restores Warlock's instant damage ramp. UA still goes out (in priority 1.5 fallback), still triggers backlash sometimes, but doesn't gate the Warlock's first-second-of-combat damage.
**Cons:** dispellers may dispel Corruption first and never see UA — silence trap fires less often.
**Predicted lift:** 3v3 +5-10pp.
**Diff:** revert the recent commit `36174b6` AI priority change.

### My recommendation

**Option A (instant UA).** The 1.5s cast was an authenticity choice; the data shows it's costing more than it's worth. Going instant:
- Eliminates the damage-ramp tax (Root Cause 1)
- Makes UA a strict-positive addition to the Warlock kit, not a tradeoff
- Keeps the silence/backlash mechanic intact — the dispel-class healer still has to choose between eating DoT pressure and risking the silence
- Mid-cast Silence interrupts were 0/24 anyway, so the cast-time interaction wasn't paying off

If Option A still falls short, *then* layer Option B (raise damage) on top in iteration 2.

### Per-match detail

| M | Brkt | WL team | Dur | Winner | UA casts | UA dispelled | Backlash dmg | Silence | T1 | T2 |
|---|---|---|---|---|---|---|---|---|---|---|
| m01 | 2v2 | – | 55.6s | T1 | 0 | 0 | 0 | 0 | War+Priest | Rogue+Pal |
| m02 | 2v2 | T2 | 22.6s | T1 | 1 | 0 | 0 | 0 | Mage+Priest | Wl+Pal |
| m03 | 2v2 | – | 36.8s | T2 | 0 | 0 | 0 | 0 | Rogue+Priest | War+Pal |
| m04 | 2v2 | T1 | 38.9s | T2 | 1 | 0 | 0 | 0 | Wl+Priest | Mage+Pal |
| m05 | 2v2 | – | 50.3s | T1 | 0 | 0 | 0 | 0 | War+Pal | Mage+Priest |
| m06 | 2v2 | – | 22.5s | T1 | 0 | 0 | 0 | 0 | Rogue+Mage | War+Priest |
| m07 | 2v2 | T1 | 30.4s | T2 | 1 | 0 | 0 | 0 | War+Wl | Rogue+Pal |
| m08 | 2v2 | T2 | 23.2s | T1 | 1 | 0 | 0 | 0 | Rogue+Mage | War+Wl |
| m09 | 2v2 | – | 69.3s | T2 | 0 | 0 | 0 | 0 | Priest+Pal | War+Priest |
| m10 | 2v2 | – | 42.6s | T1 | 0 | 0 | 0 | 0 | War+Priest | War+Priest |
| m11 | 2v2 | – | 51.0s | T1 | 0 | 0 | 0 | 0 | Mage+Priest | Rogue+Pal |
| m12 | 2v2 | T1 | 56.5s | T2 | 2 | 0 | 0 | 0 | Wl+Pal | War+Priest |
| m13 | 3v3 | T2 | 33.1s | T1 | 1 | 1 | 48 | 1 | War+Mage+Priest | Rogue+Wl+Pal |
| m14 | 3v3 | T2 | 53.6s | T1 | 2 | 2 | 98 | 2 | Rogue+Mage+Priest | War+Wl+Pal |
| m15 | 3v3 | T1 | 39.9s | T2 | 1 | 0 | 0 | 0 | War+Wl+Pal | Rogue+Mage+Priest |
| m16 | 3v3 | T2 | 38.3s | T2 | 3 | 0 | 0 | 0 | War+Rogue+Pal | Mage+Wl+Priest |
| m17 | 3v3 | T2 | 29.1s | T2 | 1 | 0 | 0 | 0 | War+Rogue+Mage | Wl+Mage+Rogue |
| m18 | 3v3 | – | 48.0s | T1 | 0 | 0 | 0 | 0 | Rogue+Mage+Priest | War+Priest+Pal |
| m19 | 3v3 | – | 34.3s | T2 | 0 | 0 | 0 | 0 | 3xWar | 3xMage |
| m20 | 3v3 | – | 77.0s | T2 | 0 | 0 | 0 | 0 | 3xPriest | 3xPal |
| m21 | 3v3 | T2 | 35.9s | T1 | 2 | 0 | 0 | 0 | War+Mage+Pal | Rogue+Wl+Priest |
| m22 | 3v3 | T1 | 49.8s | T2 | 1 | 1 | 67 | 1 | Mage+Wl+Pal | War+Rogue+Priest |
| m23 | 3v3 | T1 | 34.8s | T2 | 1 | 1 | 42 | 1 | Rogue+Wl+Pal | War+Mage+Priest |
| m24 | 3v3 | T2 | 58.5s | T1 | 4 | 2 | 72 | 2 | Rogue+Mage+Priest | War+Wl+Pal |

---

## Awaiting decision

User: pick one of A / B / C above (or propose a different direction). I'll apply the change as a small dedicated commit and re-run the 24-seed bug hunt for iteration 2.
