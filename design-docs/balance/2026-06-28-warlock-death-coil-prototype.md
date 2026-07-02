# Warlock Death Coil — instant horror + lifesteal peel (prototype)

**Date:** 2026-06-28
**Outcome:** Prototyped and measured. Death Coil gives the Warlock the instant,
damage-proof peel its kit lacked. **Marginal effect on top of the healer-lockout
change: 2v2 +6.0pt, 3v3 +3.5pt** — landing the Warlock at ~47% / ~53% in the two
target brackets (from 39% baseline on `main`). Concentrated vs melee, as designed.

Builds on [the healer-lockout findings](2026-06-28-warlock-balance-findings.md);
all deltas here are measured against that branch (so they isolate Death Coil).

---

## Why

The healer-lockout change left two matchups untouched — **1v1 vs Rogue (0%) and
vs Shaman (10%)** — and more generally the Warlock had no answer to being trained
by a melee. Its only CC that could peel, Fear, breaks after 100 damage, so a
melee on the cloth caster simply face-tanks it. The kit lacked an instant CC that
survives damage.

Death Coil is the canonical fix: instant, horror that does **not** break on
damage, plus a self-heal. Pure ability — no passive/talent (those are deferred to
the future "talents" feature).

## What shipped (prototype)

- **Ability** (`abilities.ron`): instant, 30yd, Shadow, 60 mana. Fires a fast
  projectile that on impact deals damage, heals the Warlock 100% of the damage
  dealt, and applies a 3s horror (`AuraType::Fear`, `break_on_damage: -1.0` =
  never breaks). WoW's 2-min cooldown is scaled to **30s** for this sim's ~45s
  matches. Shares the Fears DR category — a deliberate limiter on chaining Fear +
  Death Coil on one target.
- **Lifesteal** (`projectiles.rs`): on Death Coil impact the caster gains health
  equal to the damage actually dealt (capped at missing health), with a `[HEAL]`
  log line.
- **AI** (`warlock.rs`): new priority-0 reactive gate — when an enemy is within
  8yd and is either targeting the Warlock or a melee class (Warrior/Rogue), Death
  Coil it (peel + lifesteal), gated on not-immune / not-Fear-DR-immune /
  not-already-CC'd.
- Wiring: `AbilityType::DeathCoil` + validation + View Combatant UI + icon. All
  185+ tests pass.

## Result (marginal effect vs the healer-lockout branch)

Final tuned values (16–22 base + 0.25 SP ≈ 49 dmg/heal per cast):

| bracket | OVERALL | vs healer | vs melee | no-melee |
|---|---|---|---|---|
| **2v2** | 41.3 → **47.3** (+6.0) | 31.6 → 38.4 (+6.8) | 42.6 → **55.6** (+13.0) | 39.9 → 38.3 (−1.6) |
| **3v3** | 49.7 → **53.2** (+3.5) | 48.6 → 51.3 (+2.7) | 51.3 → 56.4 (+5.1) | 46.7 → 47.2 (+0.5) |

1v1 (diagnostic): overall 34.8 → 39.5; **vs Rogue 0 → 3, vs Warrior 33 → 88**.
Both 3v3 Warlock comps improve (Priest+Mage 77→80, Priest+Warrior 22→28).

### Reading it

- **The value is the peel + heal, not the nuke.** Trimming the hit from 84 → 49
  damage moved the 2v2/3v3 aggregates by only ~1pt (48.3 → 47.3 / 54.2 → 53.2) —
  the win comes from the guaranteed 3s horror and the self-heal, not the burst.
  The trimmed values are kept because a 30s-CD utility tool shouldn't also be a
  2× Shadow Bolt; the trim also softened the 1v1 Warrior matchup (100 → 88).
- **Concentrated vs melee (+13pt 2v2)** — the design signature. Death Coil is an
  anti-melee peel, so the Warlock getting better against melee is correct, not a
  bug. It lifts healer comps too (+6.8) via the extra burst + sustain in the race.
- **Net landing:** ~47% 2v2 / ~53% 3v3, from 39% on `main` (pre-healer-lockout).
  Both target brackets within a few points of 50 — a well-centered result.

## Open items / caveats

- **`no-melee` 2v2 dips ~1.6pt.** Persists at both damage levels, so it's not the
  damage — the priority-0 gate occasionally spends Death Coil on a non-melee enemy
  that wanders within 8yd (a kiting Mage/Hunter pinned briefly), costing a GCD +
  mana for little peel value. If centering matters, tighten the gate to
  melee-classes-only (loses the "caster/pet training me" peel) or add a
  "low-ish HP or genuinely pinned" guard.
- **1v1 vs Shaman still 10%, vs Paladin dipped.** Shaman/Paladin aren't in the
  melee-class trigger and kite/stay at range, so Death Coil rarely fires there;
  the Paladin dip is the extra mana spent for no peel. These are 1v1 (off-target
  bracket) and not pursued.
- **DR:** Death Coil diminishes on its own `Horror` DR bucket, NOT Fears (WoW
  Classic behavior — horror and fear are separate DRs). It reuses
  `AuraType::Fear` only for the flee locomotion + dispel classification; the
  `dr_category_override: Some(Horror)` decouples DR (the Kidney Shot idiom). So a
  Fear and a Death Coil on one target both land at full duration. Verified in a
  trace (Fear lands DR 100% / 8.0s even 6s after a Death Coil); balance impact
  negligible (2v2 47.3 → 47.5, 3v3 53.2 → 53.9 — they rarely hit the same target,
  since Fear goes to the healer and Death Coil to a training melee).

## Reproduction

```bash
# before = healer-lockout branch; after = this branch. Assets must match the
# binary (the before-binary panics on the DeathCoil RON entry), so check out the
# matching branch before each binary's sweep.
git checkout feat/warlock-healer-lockout && cargo build --release && cp target/release/arenasim /tmp/before
git checkout feat/warlock-death-coil     && cargo build --release && cp target/release/arenasim /tmp/after

python3 scripts/gen_sweep.py --t1 'Warlock+{p}' --t2-size 2 --n 20 --exclude-double-healer > /tmp/2v2.jsonl
git checkout feat/warlock-healer-lockout && /tmp/before --batch /tmp/2v2.jsonl --out /tmp/b.csv --jobs 16
git checkout feat/warlock-death-coil     && /tmp/after  --batch /tmp/2v2.jsonl --out /tmp/a.csv --jobs 16
# slice by enemy-has-healer / has-melee (see session python)
```
