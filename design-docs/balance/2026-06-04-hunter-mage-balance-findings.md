# Hunter & Mage Balance Findings — 2026-06-04

Investigation into why Hunter underperforms, starting from the 2v2 picture and
following the data where it led (which turned out to be Mage). All numbers below
are from the headless matrix tooling; **1v1 is treated as a diagnostic signal
only — 2v2/3v3 is the balance target.**

## TL;DR

- **Hunter is the weakest class in the game** (10.9% overall in the 1v1 matrix),
  not merely countered by one class.
- **Mage / Paladin / Rogue are an overtuned top tier** (~75/79/72%); **Priest /
  Hunter are the punching bags** (~23/11%).
- **Auto Shot damage is Hunter's primary balance lever** — and the only one, since
  Auto Shot scales off the equipped weapon's `attack_damage` with **no attack-power
  scaling at all**. A 1.5× bow buff lifts Hunter 10.9% → 22.0%; 2× → 37.3% (1v1).
- The Auto Shot lever is **blunt**: it scales with match length, so it overshoots
  grind matchups (Warrior flips to a guaranteed win) and does nothing for burst
  matchups (Mage stays 0%) or sustain matchups (Paladin stays ~1%).
- **Hunter is a fine support-DPS, a poor carry.** In 2v2 it wins paired with a
  second damage dealer (Hunter+Mage 77%, Hunter+Rogue 54%) and loses hard as the
  healer-backed carry (Hunter+Priest 6%, Hunter+Paladin 12%).
- A **global Frostbolt nerf** (coefficient 0.8 → 0.6) cuts Mage 75% → 67% (1v1)
  and **doubles** Hunter's winrate vs enemy-Mage comps in 2v2 (clean slice
  6.5% → 15.2%) — but it is **symmetric**, so it also kneecaps Hunter+Mage
  (77% → 61%) and nets to a wash on Hunter's overall 2v2 winrate.

## Method

- Tooling: in-process 1v1 `--matrix N` (`src/headless/matrix.rs`) and the
  `scripts/hunter_2v2_full_matrix.sh` sweep (Hunter+partner vs every distinct
  opposing pair, excluding double-healer comps; 120 matchups).
- Discipline: clean before/after on the **same binary/toolchain**, matched seeds,
  matched N. Both `abilities.ron` and `items.ron` are runtime assets, so balance
  edits need no rebuild.
- Sample sizes: 1v1 at N=100 (4,900 matches/run); 2v2 at N=20 (2,400 matches/run).
  N=5 scouting runs reproduced the N=20 aggregates within ~1 point.

## 1. Hunter is the worst class; Mage/Paladin/Rogue are overtuned (1v1, N=100)

| Class | Winrate | Tier |
|---|---|---|
| Paladin | 79.3% | top |
| Mage | 75.0% | top |
| Rogue | 72.4% | top |
| Warrior | 49.6% | mid |
| Warlock | 43.6% | mid |
| Priest | 23.1% | bottom |
| **Hunter** | **10.9%** | **bottom** |

Mage beats Warrior, Priest, Warlock, Paladin, **and Hunter 100–0**; its only losing
matchup is Rogue. Data: `matrix_1v1_n100_2026-06-04_baseline.csv`.

## 2. Hunter vs Mage is a damage problem, not control

The time-to-death signature rules out kiting/control: Hunter dies to Mage in
**10.5s** — 3–5× faster than to any other class (Warrior 38.7s, Paladin 54.6s).
A control loss produces *long* games; this is the fastest death on the board → burst.
In traced matches the Mage never needs its CC — it stands at range and free-casts.

**Frostbolt is the driver:** base damage 10–15, but a **0.8 spell-power coefficient ×
~122 SP** (50 base + ~72 from the Magister set) makes each bolt hit ~110 — ~73
sustained DPS from one spammable, cheap (20 mana), no-cooldown spell that *also*
applies a 70% slow. Hunter's sustained answer is Auto Shot at ~9 DPS.

## 3. Auto Shot is Hunter's only damage lever — and how it works

`combat_core/auto_attack.rs`: Auto Shot damage = `combatant.attack_damage`
(+ next-attack bonus), with **no attack-power scaling**. The primary weapon slot
*replaces* `attack_damage`, so for the Hunter the only knob is the **Ashwood Bow's**
`attack_damage` (a budget-free stat). Baseline 20–26 ≈ 9 DPS at 0.4 attacks/sec.

| Auto Shot | Hunter 1v1 overall |
|---|---|
| 20–26 (baseline) | 10.9% |
| 30–39 (1.5×) | 22.0% |
| 40–52 (2×) | 37.3% |

Data: `matrix_1v1_n100_2026-06-04_{baseline,autoshot15,autoshot2x}.csv`.

### The lever is blunt — it scales with match length
Because Auto Shot fires once per 2.5s (0.4 attacks/sec, untouched per the "damage
not attack-speed" constraint), the buff compounds in long fights and barely
registers in short ones:

| Hunter vs | base | 1.5× | 2× |
|---|---|---|---|
| Warrior | 25% | **100%** | **100%** |
| Priest | 0% | 6% | 62% |
| Warlock | 4% | 16% | 41% |
| Rogue | 3% | 12% | 22% |
| **Mage** | **0%** | **0%** | **0%** |
| **Paladin** | **1%** | **4%** | **1%** |

- **Warrior flips to a guaranteed win even at 1.5×** — see §4.
- **Mage stays 0%** at every level: a ~10s fight gives Hunter only ~3–4 shots, so
  doubling each is irrelevant to the burst race.
- **Paladin stays ~1%**: healing out-sustains the extra ticks.

## 4. The Warrior cliff is a kit gap, not an AI bug

Tracing Hunter vs Warrior at 1.5× (Warrior loses 100%): the Warrior's main nuke
**Mortal Strike fired zero times** (`OutOfRange ×1176`); its only chosen actions
across a 34s fight were one Battle Shout, one Charge, one Rend.

Cause: the Warrior has **one gap-closer (Charge, 15s CD) and no snare** —
**Hamstring and Intercept do not exist in the game**. The Hunter has five anti-melee
tools (Concussive Shot, Disengage, Frost Trap, Freezing Trap, Spider Web). After the
Warrior's single Charge, the Hunter Disengages and re-slows; with Charge on cooldown
and no snare to stick, the Warrior foot-chases a slowed target forever. The matchup
collapses to a pure DPS race the Warrior can only win via a rare Charge→burst window —
which any Hunter damage buff closes, hence the 25% → 100% *cliff* (not a curve).

**Implication:** the Warrior matchup can't be tuned via Auto Shot. The real fix is
giving melee an anti-kite tool (Hamstring/Intercept). 1v1 is also a weak balance
target here — kiting a lone melee is a legitimate asymmetry.

## 5. 2v2: Hunter is support-DPS, not a carry (N=20)

Overall Hunter-team winrate: **baseline 35.7% → 1.5× Auto Shot 37.7%** (+2.0).
The buff that nearly quadrupled Hunter's 1v1 standing barely moves 2v2.

| Hunter partner | baseline | 1.5× |
|---|---|---|
| Hunter+Mage | 77.2% | 76.5% |
| Hunter+Rogue | 53.5% | 56.5% |
| Hunter+Warrior | 34.2% | 34.8% |
| Hunter+Warlock | 30.8% | 29.2% |
| Hunter+Paladin | 11.8% | 16.8% |
| Hunter+Priest | 6.5% | 12.2% |

The split is clean: Hunter **wins as a second damage dealer**, **loses as the
healer-backed carry**. Every opposing comp containing a Mage stays near-unwinnable
(Mage+Warlock 0%, Mage+Priest 2%, Mage+Paladin 3%) — more sustained Auto Shot DPS
can't change a race the team loses to burst.

Data: `matrix_2v2_full_n20_{baseline,autoshot15}.csv`.

## 6. The Frostbolt nerf: works on its target, but is symmetric

Coefficient 0.8 → 0.6 (Frostbolt ~110 → ~86, a ~22% cut).

- **1v1 (additive with the Auto Shot buff):** Mage 75.0% → 66.9%, Hunter unchanged
  by the nerf (the Auto Shot buff does Hunter's lifting). Combo roster: Hunter +11.1,
  Mage −8.1, others ~flat. Hunter vs Mage **still 0%** — 22% isn't enough to flip the
  burst race.
- **2v2 (combo = 1.5× Auto Shot + Frostbolt 0.6):** overall **37.0%** — *lower* than
  Auto-Shot-only (37.7%), because the global nerf also weakens the *allied* Mage:
  **Hunter+Mage 77.2% → 61.0%**. The friendly-fire cancels the gains.
- **But it works on the intended problem:** the clean slice (enemy has a Mage,
  Hunter's partner does not) **6.5% → 7.3% (Auto Shot only) → 15.2% (combo)** — the
  Frostbolt nerf more than doubled Hunter's odds vs enemy Mages. Even so, 15% is still
  a losing matchup, and Mage+healer comps stay ~0%.

Data: `matrix_1v1_n100_2026-06-04_{frostnerf,combo}.csv`,
`matrix_2v2_full_n20_combo.csv`.

## Conclusions & open problems

1. **Auto Shot damage is Hunter's main knob; recommend a *modest* (~1.5×) buff.**
   It fixes the 1v1 floor (the original "Hunter struggling" complaint) and gives a
   small 2v2 lift, without 2×'s overshoot. It does **not** fix Hunter's structural
   2v2 weaknesses.
2. **Mage and Paladin are overtuned top-tier** (75/79%). The Frostbolt nerf is
   justified as roster health on its own merits, independent of Hunter. Paladin is
   actually the most dominant class and remains unaddressed.
3. **The 2v2 boss is Mage+healer comps** (~0% for Hunter). This is a "Mage+healer is
   oppressive" problem more than a Hunter-damage problem — it needs a deeper Mage cut
   and/or actual Hunter peel/survivability, not more Auto Shot.
4. **Treat the two edits as independent changes:** Auto Shot = a Hunter buff;
   Frostbolt = a Mage/roster nerf. A global Mage nerf is the wrong tool to raise
   Hunter's *overall* 2v2 because it's symmetric.
5. **Melee anti-kite kit gap:** Hamstring/Intercept don't exist; lone melee cannot
   stick to a kiter. Relevant if 1v1 melee-vs-Hunter is ever a balance concern.

## Methodology lessons (for the recurring balance task)

- **Always compute clean slices**, not just aggregates — the Frostbolt nerf looked
  like a wash (37.7 → 37.0) until the enemy-only-Mage slice revealed it doubled the
  target matchup. Symmetric changes and strategy-var confounds hide in aggregates.
- **Watch for symmetric effects** when a class appears on both teams.
- **1v1 is a weak balance signal** (kiting asymmetries, no team dynamics). Balance
  around 2v2/3v3.
- **N matters:** N=5 scouts are directional; N=20+ for findings. Report intervals so
  "37.0 vs 37.7" reads as noise, not signal.

## Status of edits

Both edits were live in the working tree during this investigation and are
**uncommitted**:

- `assets/config/items.ron` — Ashwood Bow `attack_damage` 20–26 → 30–39 (1.5×).
- `assets/config/abilities.ron` — Frostbolt `damage_coefficient` 0.8 → 0.6.

Recommendation: ship them as **two separate commits** (Hunter buff; Mage nerf), or
hold pending the deeper Mage+healer / Paladin work. Data artifacts for every run are
in `design-docs/balance/matrix_{1v1_n100,2v2_full_n20}_*_2026-06-04*.csv`.
