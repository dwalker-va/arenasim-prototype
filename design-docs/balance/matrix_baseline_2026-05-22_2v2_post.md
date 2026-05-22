# 2v2 Baseline — Hunter+Priest vs Class+Priest (Post-Change)

- **Wrapper:** `scripts/hunter_2v2_matrix.sh`
- **Runs per matchup:** 10
- **Seed base:** 0
- **Total matches:** 60
- **Max match duration:** 120s (combat phase)
- **Hunter state:** post-change (`max_mana=240`, `mana_regen=0.0`, ability costs cut ~15%)

Comparison run for `design-docs/balance/matrix_baseline_2026-05-22_2v2_pre.md`
following U3+U4 of `docs/plans/2026-05-22-001-fix-hunter-mana-economy-plan.md`.

## Team 1 (Hunter+Priest) Winrate

| Opponent | Pre H+P Wins | Post H+P Wins | Δ | Pre Avg | Post Avg |
|---|---:|---:|---:|---:|---:|
| Warrior+Priest |  0 |  0 |  0 |  56.0s |  59.6s |
| Mage+Priest    |  0 |  0 |  0 |  35.4s |  36.9s |
| Rogue+Priest   |  0 |  0 |  0 |  58.0s |  58.0s |
| Priest+Priest  |  7 |  6 | -1 |  79.6s |  88.6s |
| Warlock+Priest |  0 |  0 |  0 |  55.6s |  55.6s |
| Paladin+Priest |  1 |  0 | -1 | 121.2s | 130.0s |

## Interpretation

No matchup moved meaningfully in the Hunter+Priest 2v2 axis.

- Hunter+Priest vs Warrior/Mage/Rogue/Warlock+Priest stays at 0/10. The mana
  fix increases Hunter's cast frequency, but the enemy Priest absorbs the
  marginal pressure and the Hunter still cannot kill anyone before being
  killed.
- Hunter+Priest vs Priest+Priest dropped from 7/10 → 6/10 (small, plausibly
  within N=10 noise) but with longer average duration (79.6s → 88.6s),
  suggesting marginal effect from more Hunter casts.
- Hunter+Priest vs Paladin+Priest went from 1 win / 5 draws to 0 wins / 10
  draws. With more mana, Hunter sustains long enough that more matchups
  hit the 120s timeout cap. The +9s avg duration delta reflects this.

**The 2v2 result confirms the brainstorm's expectation**: mana economy fixes
do not improve healer-vs-healer matchups because the binding constraint there
is Hunter's lack of healer pressure (no team-comp-aware target switching,
trap-on-healer logic). Those are separately tracked survivors in the ideation
doc.

The 1v1 axis tells the more revealing story — see
`design-docs/balance/matrix_baseline_2026-05-22.md`.
