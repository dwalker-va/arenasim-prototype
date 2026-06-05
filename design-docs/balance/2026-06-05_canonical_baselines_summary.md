# Canonical Balance Baselines — 2026-06-05

Authoritative current-state balance references, generated with the `balance-sweep`
harness (`arenasim --batch`, single-threaded executor, 300s cap). Use these as
the "before" when assessing any balance change — **compare batch-vs-batch only**
(these numbers differ a few points from the older multithreaded `--matrix` data).

| Format | File | Coverage | N | Matches |
|---|---|---|---|---|
| 1v1 | `2026-06-04_canonical_1v1_n100_300s.csv` | full 7×7 | 100 | 4,900 |
| 2v2 | `2026-06-05_canonical_2v2_full_n100_300s.csv` | every distinct-class pair × every pair (441) | 100 | 44,100 |
| 3v3 | `2026-06-05_canonical_3v3_full_n50_300s.csv` | every distinct-class triple × every triple (1225) | 50 | 61,250 |

Scope: **distinct-class comps** (no same-class stacking), **both orderings**
(captures spawn-side effects), double-healer comps included. Draw rates are tiny
(2v2 0.5%, 3v3 0.2%) — outcomes are decisive at 300s.

Regenerate with:
```bash
python3 scripts/gen_sweep.py --full 2 --n 100 > /tmp/full_2v2.jsonl   # 441 matchups
python3 scripts/gen_sweep.py --full 3 --n 50  > /tmp/full_3v3.jsonl   # 1225 matchups
target/release/arenasim --batch /tmp/full_2v2.jsonl --out 2v2.csv --jobs 16
python3 scripts/agg_sweep.py 2v2.csv
```

---

## The balance story

Every causal claim below was checked against an actual match log (per-combatant
"Damage Dealt"), not inferred from winrates alone.

### Per-class value (winrate of comps containing the class)

| Class | 2v2 | 3v3 |
|---|---|---|
| **Mage** | **74.3%** | **68.6%** |
| Paladin | 53.1% | 55.7% |
| Warrior | 55.0% | 52.3% |
| Rogue | 49.6% | 46.8% |
| Priest | 42.8% | 47.6% |
| Warlock | 41.8% | 44.5% |
| **Hunter** | **31.7%** | **33.6%** |

### 1. One class defines the meta: the Mage
Mage-containing comps lead by ~15–19 points. Five of the top six 2v2 comps and
the top three 3v3 comps contain a Mage (2v2: Mage+Priest 82.9, Mage+Paladin 82.6;
3v3: Warrior+Mage+Priest 87.3, Warrior+Mage+Paladin 85.8, Mage+Warlock+Paladin 85.7).

**Verified:** Mage+Priest vs Warrior+Priest — the Mage dealt **1191** of its team's
damage, took 231, ended at **full HP**. In a 3v3, a single Mage dealt **1495** and
took 60 — out-damaging the entire opposing three-player team (834 combined). The
Mage free-casts Frostbolt from range while partners peel; it is rarely touched.
This is the Frostbolt-burst problem from the Hunter/Mage investigation, now
visible roster-wide.

### 2. Two roles, one ladder: carries ≫ enablers ≫ dead weight
The roster splits into **carries** (Mage/Warrior/Rogue — kill pressure) and
**enablers** (Paladin/Priest — sustain). Top comps pair a carry with an enabler
or stack two carries. Everything lacking a real carry sinks.

### 3. Healers amplify a carry — they can't be one
Healer + real carry tops the chart; healer + weak DPS sinks it. **Two healers can
sustain forever but never close**: 2v2 Priest+Paladin 27.8%; the single worst 3v3
comp is **Priest+Paladin+Hunter at 4.2%**.

**Verified:** Priest+Paladin vs Warrior+Mage — the double-healer dealt **313 damage
in 49s** and lost; the Paladin OOM'd (5 mana), absorbed 1393, and died, while the
enemy Mage finished at 94% HP. Burst beats sustain at this gear level.

### 4. Paladin out-enables Priest
Paladin comps beat Priest comps; 2v2 Warrior+Paladin (80.6) ≫ Warrior+Priest (63.2);
Paladin is the #2 class in 3v3 (55.7).

**Verified:** both beat Rogue+Warlock, but the Paladin version left the Warrior
carry at 253/419 HP vs the Priest version's 76/449. Paladin enables through
*survivability* (425 HP plate + bigger heals), not damage — it converts the close
games Priest comps drop.

### 5. Hunter is the game's dead weight
Hunter is last in both formats (31.7% / 33.6%). The bottom of both tier lists is
dominated by Hunter comps; the two worst 2v2 comps are Priest+Hunter (12%) and
Paladin+Hunter (14%); the worst 3v3 is Priest+Paladin+Hunter (4.2%).

**Verified:** Priest+Hunter vs Warrior+Mage — the Hunter dealt **0 damage** and
died first. The log shows it was the focus target, chain-Frostbolt-slowed, and
used *only utility* (Freezing Trap, Concussive Shot, Frost Trap) while its Priest
spent its whole mana bar babysitting it. Hunter brings traps and slows but no kill
pressure, so its comps can't close. Its one above-water comp — Mage+Hunter (57.9%)
— is the Mage carrying a passenger.

### The one-line story
Balance is gated by **burst kill-pressure**, and the **Mage has too much of it**.
It is the best partner for everyone; healers exist only to amplify a carry;
Paladin out-enables Priest through bulk; the Hunter contributes utility but no
damage and drags down every comp it joins.

---

## Caveats

- **Spawn-side asymmetry** of up to ~18% exists in some matchups (e.g. Warrior+Mage
  vs Mage+Priest sums to 82, not 100). The full matrix runs both orderings so the
  tier lists average it out and stay fair, but it is real and **not yet
  root-caused** (likely an AI positioning / first-mover artifact). Do not read a
  single ordered cell as definitive.
- **Harness order-sensitivity** (deferred): the batch path is internally
  deterministic but differs a few points from the old multithreaded `--matrix`
  numbers. Always compare against these batch baselines, not historical ones.
- These are **default loadouts and default strategy choices**. Strategy-var sweeps
  (pets, openers, curses, etc.) are a separate axis — see the `balance-sweep` skill.
