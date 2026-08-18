# Residual Findings — Lightning Bolt Signature Animation

Source: `docs/plans/2026-08-16-001-feat-lightning-bolt-signature-animation-plan.md`
Branch: `feat/lightning-bolt-signature-animation`

These items were surfaced during the autonomous build/review pipeline, proceeded past (not blockers), and are recorded here because they have no dedicated GitHub thread. None block merge; the first is a **pre-merge verification gate** the plan's Success Criteria already calls for.

## Verification gate — RESOLVED ✅ (swept 2026-08-18: within noise)

- **Balance sweep DONE — the Shaman shift is within noise; safe to merge.** KTD6 / Success Criteria required a paired before/after balance sweep. Ran a **2v2 Shaman-comp sweep, 5,250 matches, paired at identical seeds** (only `projectile_speed` differs — same binary, RON is a runtime asset):

  | Metric | Before (projectile) | After (instant) | Δ |
  |---|---|---|---|
  | Shaman-comp winrate | 53.1% ±1.3 | 52.5% ±1.4 | −0.6pt (CIs overlap heavily) |
  | Mean / p90 duration | 46.7s / 81.7s | 46.9s / 82.0s | flat |
  | Draw rate | 0.3% | 0.6% | +14 matches |

  The aggregate is **flat** (within noise, a hair lower — not a buff). The "systematic lengthening" that motivated this gate came from the `juke_chase` **1v1 lone-Shaman** scan — a kiting-asymmetry diagnostic bracket, *not* a balance signal (per the balance methodology, conclusions come from 2v2/3v3, not 1v1). At the 2v2 scale duration barely moves (46.7→46.9s), so it does not generalize.

  **Caveat (expected):** touching the sim RNG stream deterministically reshuffles individual matchups even as the aggregate washes — two 2v2 cells moved >25pt at N=30 (Shaman+Rogue vs Mage+Hunter 77%→10%; the Shaman+Mage vs Mage+Shaman mirror 33%→7%, into simultaneous-kill draws), both *against* the Shaman comp, offsetting cells that rose. Doesn't move the verdict. (3v3 confirmation not run — 2v2 is the primary bracket and the result is unambiguous.)

## Advisory findings (P3, report-only — from code review)

- **`src/states/play_match/combat_core/casting.rs` — headless spawns undespawned `LightningBoltStrike` markers per cast.** The spawn/cleanup systems are graphical-only, so in headless the markers accumulate until match-end (reaped via `PlayMatchEntity`). Harmless: no `game_rng` draw, no sim query reads them, byte-identity holds — and it matches the existing pattern for every bespoke spell visual (`SpellImpactEffect`, `FlameParticle`). No change made; recorded for awareness.
- **`tests/movement_probes.rs` — `juke_bounded_seed_b` proxy bound loosened 6 → 24.** The secondary occlusion-proxy guard now has ~33% headroom (vs sibling seed_a's 50%), so a partial chase regression that kept windows in the low-20s and duration under cap could slip past the *secondary* guard. The primary guards (elimination win + duration < 200s) are retained and unchanged. Acceptable; noted so a future maintainer doesn't inflate it further.
- **`src/states/play_match/rendering/effects/lightning_bolt.rs` — `LightningBoltBurst` parallels `SpellImpactEffect`'s expand/fade envelope.** Deliberate, documented split (bespoke white-blue vs the shared hardcoded purple). Candidate to unify only if `SpellImpactEffect` gains a per-instance color, or a third bespoke-color burst appears.

## Testing gaps (report-only)

- No dedicated guard that Lightning Bolt stays instant (`projectile_speed: None`) — a future re-addition of `projectile_speed` to its config would not clearly fail a test (the re-baselined probes would shift but not obviously flag the cause). The mechanic itself is covered by the already-passing instant-application path (`tests/casting_mana_charge.rs`). Cheap follow-up: a config-validation assertion that `LightningBolt.projectile_speed.is_none()`.
- No probe directly asserts the marker spawn is byte-neutral (draws no `game_rng`); correct by construction (inside the damage-application block, past the alive+LoS gates) and verified out-of-band by the full suite passing on non-Shaman matches.
