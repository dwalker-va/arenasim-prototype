# 2v2 Baseline — Hunter+Priest vs Class+Priest (Pre-Change)

- **Wrapper:** `scripts/hunter_2v2_matrix.sh`
- **Runs per matchup:** 10
- **Seed base:** 0 (matchup-local; match `i` uses seed `seed_base + i`)
- **Total matches:** 60
- **Max match duration:** 120s (combat phase)
- **Hunter state:** pre-change (`max_mana=150`, `mana_regen=3.0`, original ability costs)

This captures the pre-change 2v2 baseline for the Hunter mana economy tuning
(see `docs/plans/2026-05-22-001-fix-hunter-mana-economy-plan.md` U2). N=10 was
chosen for fast autopilot validation signal. **N=100 rerun is a deferred
follow-up before merge.**

## Team 1 (Hunter+Priest) Winrate

| Opponent | Hunter+Priest Wins | Opp+Priest Wins | Draws | T1 Winrate | Avg Duration |
|---|---:|---:|---:|---:|---:|
| Warrior+Priest |  0 | 10 | 0 |   0% |  56.0s |
| Mage+Priest    |  0 | 10 | 0 |   0% |  35.4s |
| Rogue+Priest   |  0 | 10 | 0 |   0% |  58.0s |
| Priest+Priest  |  7 |  3 | 0 |  70% |  79.6s |
| Warlock+Priest |  0 | 10 | 0 |   0% |  55.6s |
| Paladin+Priest |  1 |  4 | 5 |  10% | 121.2s |

## Interpretation

- Hunter+Priest wins zero of ten in 4 of 6 matchups: Warrior, Mage, Rogue, Warlock.
  The healer partner doesn't change the binding constraint — Hunter still
  contributes near-zero pressure inside the mana window, then idles.
- Hunter+Priest vs Priest+Priest is the only positive matchup (70%). Two
  healers with no damage threat can't out-pressure Hunter's Auto Shot + the
  Priest's Mind Blast, so the Hunter-side team's modest sustained damage wins.
  This is the Priest mirror weakness, not a Hunter strength.
- Hunter+Priest vs Paladin+Priest is the longest matchup at ~121s (against the
  120s combat cap → 5/10 are draws). Paladin's heals trade-stall through the
  Hunter's trickle. Hunter never CCs the Paladin healer, which is the diagnosed
  failure mode (separate survivor `Team-comp awareness` in the ideation doc).

## Comparison target

This baseline is the comparison row in
`docs/reports/2026-05-22-hunter-mana-tuning.md`, written after U5 captures the
post-change measurements.
