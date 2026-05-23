# 2v2 Baseline — Hunter+Priest vs Class+Priest (Post Pet-Engagement)

- **Wrapper:** `scripts/hunter_2v2_matrix.sh`
- **Runs per matchup:** 10
- **Seed base:** 0
- **Total matches:** 60
- **Hunter state:** post pet engagement (U1+U2+U3+U5+U6+U8 landed; U4 Hunter dispatch deferred)

## Team 1 (Hunter+Priest) Winrate

| Opponent | Post-Mana Wins | Post-Pet Wins | Δ | Post-Mana Avg | Post-Pet Avg |
|---|---:|---:|---:|---:|---:|
| Warrior+Priest |  0 |  0 |   0 |  59.6s |  72.8s |
| Mage+Priest    |  0 |  0 |   0 |  36.9s |  37.1s |
| Rogue+Priest   |  0 |  0 |   0 |  58.0s |  58.0s |
| Priest+Priest  |  6 |  8 |  +2 |  88.6s |  83.4s |
| Warlock+Priest |  0 |  0 |   0 |  55.6s |  55.9s |
| Paladin+Priest |  0 |  0 |   0 (10 draws) | 130.0s | 130.0s |

## Interpretation

- Hunter+Priest vs Warrior+Priest avg duration **+13.2s** (59.6 → 72.8). The pet pursuit is doing real damage but not enough to flip wins at this scope.
- Hunter+Priest vs Priest+Priest moved 6 → 8 wins (small lift, N=10 noise-band).
- Other matchups unchanged. Healer absorbs the marginal pet pressure in 2v2.
- Paladin+Priest still 10 draws at the 120s timeout cap.

See `docs/reports/2026-05-22-hunter-pet-engagement.md` for the full assessment.
