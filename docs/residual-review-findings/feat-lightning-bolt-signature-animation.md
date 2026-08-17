# Residual Findings — Lightning Bolt Signature Animation

Source: `docs/plans/2026-08-16-001-feat-lightning-bolt-signature-animation-plan.md`
Branch: `feat/lightning-bolt-signature-animation`

These items were surfaced during the autonomous build/review pipeline, proceeded past (not blockers), and are recorded here because they have no dedicated GitHub thread. None block merge; the first is a **pre-merge verification gate** the plan's Success Criteria already calls for.

## Proceeded-and-flagged (from implementation)

- **P1 (verification gate) — Balance sweep NOT run; the Shaman shift may exceed "within noise."** KTD6 / Success Criteria require a paired before/after balance sweep, and the user's expectation is the shift "should not be significant." Incidental evidence from the `juke_chase` seed scan (`tests/movement_probes.rs`) shows the instant-strike buff **systematically lengthens lone-Shaman kite endgames** — e.g. seed 2 went from 3 → 18 fizzle-length occlusion windows and ~52s occlusion, with the match now resolving by elimination at ~102s; many seeds now run 80–133s. Team 1 still wins by elimination everywhere (chase still closes), so this is a *reliability buff to the Shaman*, not a chase-logic regression — but it suggests the win-rate shift may be non-trivial. **Action before merge:** run the definitive sweep (`balance-sweep` skill / `scripts/headtohead_sweep.py`, ~100 matches/cell across representative comps) and confirm the Shaman shift is within noise, or treat a significant shift as the signal to revisit (per the plan).

## Advisory findings (P3, report-only — from code review)

- **`src/states/play_match/combat_core/casting.rs` — headless spawns undespawned `LightningBoltStrike` markers per cast.** The spawn/cleanup systems are graphical-only, so in headless the markers accumulate until match-end (reaped via `PlayMatchEntity`). Harmless: no `game_rng` draw, no sim query reads them, byte-identity holds — and it matches the existing pattern for every bespoke spell visual (`SpellImpactEffect`, `FlameParticle`). No change made; recorded for awareness.
- **`tests/movement_probes.rs` — `juke_bounded_seed_b` proxy bound loosened 6 → 24.** The secondary occlusion-proxy guard now has ~33% headroom (vs sibling seed_a's 50%), so a partial chase regression that kept windows in the low-20s and duration under cap could slip past the *secondary* guard. The primary guards (elimination win + duration < 200s) are retained and unchanged. Acceptable; noted so a future maintainer doesn't inflate it further.
- **`src/states/play_match/rendering/effects/lightning_bolt.rs` — `LightningBoltBurst` parallels `SpellImpactEffect`'s expand/fade envelope.** Deliberate, documented split (bespoke white-blue vs the shared hardcoded purple). Candidate to unify only if `SpellImpactEffect` gains a per-instance color, or a third bespoke-color burst appears.

## Testing gaps (report-only)

- No dedicated guard that Lightning Bolt stays instant (`projectile_speed: None`) — a future re-addition of `projectile_speed` to its config would not clearly fail a test (the re-baselined probes would shift but not obviously flag the cause). The mechanic itself is covered by the already-passing instant-application path (`tests/casting_mana_charge.rs`). Cheap follow-up: a config-validation assertion that `LightningBolt.projectile_speed.is_none()`.
- No probe directly asserts the marker spawn is byte-neutral (draws no `game_rng`); correct by construction (inside the damage-application block, past the alive+LoS gates) and verified out-of-band by the full suite passing on non-Shaman matches.
