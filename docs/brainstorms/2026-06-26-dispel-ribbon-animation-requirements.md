# Dispel Ribbon Animation — Requirements

**Date:** 2026-06-26
**Scope:** Standard (visual-effects change, no combat logic)
**Status:** Ready for planning

## Summary

Replace the dispel's expanding-sphere burst with a twisting ribbon of geometry that spirals up off the dispelled combatant's head and fades. The goal is a distinct silhouette that reads instantly as "this combatant just got cleansed" — anchored to the right person and lasting long enough to catch the eye in a busy fight.

## Problem Frame

When a dispel lands today, players can't reliably tell it happened. Two specific failures (confirmed in dialogue):

1. **It looks like everything else.** The current effect is a small glowing sphere that expands and fades — visually indistinguishable from heals, impacts, and the other sphere/particle bursts in the game. It doesn't say "dispel."
2. **It's hard to attribute.** Players can't tell *who* just got dispelled — the effect doesn't draw the eye to the specific combatant.

The current implementation (`src/states/play_match/rendering/effects.rs:931`, `DispelBurst`) is a 0.3-radius sphere expanding to ~3× and fading over **0.5s**, positioned 1 yd above the target, tinted by caster class. Spawned on a successful dispel in `src/states/play_match/effects/dispels.rs:88`.

A spiral ribbon — the WoW dispel idiom — fixes both: a silhouette nothing else uses (distinctiveness) and upward travel off the head (no other burst moves upward, so motion + head-anchor solve attribution).

## Requirements

- **R1 — Spiraling ribbon shape.** The effect is a ribbon of geometry that follows a helical/spiral path, replacing the expanding sphere as the dispel-success visual. It must be a clearly different silhouette from the existing sphere and particle bursts.
- **R2 — Head anchor.** The ribbon originates at / just above the dispelled combatant's head and stays anchored to that combatant (follows the target's position, as the current burst does).
- **R3 — Upward rise.** Over its lifetime the ribbon travels upward off the head. This motion is a primary distinctiveness/attribution lever, not decoration — every other burst expands in place.
- **R4 — Longer, readable lifetime.** Extend the effect lifetime from the current 0.5s to roughly **0.8–1.0s** so the eye can register it. Final value is a tuning decision; the requirement is "long enough to read without lingering."
- **R5 — Class-tinted color.** Preserve caster-class coloring (Priest silver-blue, Paladin gold, others fall back to the current neutral tint) so the effect still attributes to the dispeller's class. Reuse the existing `dispel_burst_colors` mapping.
- **R6 — Fade-out.** The ribbon fades over its lifetime (alpha + emissive ramp to zero), consistent with existing effect behavior, and despawns when expired.
- **R7 — Pure visual, headless-safe.** No combat logic changes. The effect follows the spawn/update/cleanup 3-system pattern and is registered in graphical mode only (`src/states/mod.rs`), not in `add_core_combat_systems`. Headless simulation and match outcomes are unaffected.

## Key Decisions

- **True ribbon mesh over particle helix.** A literal spiraling ribbon is the only candidate shape that *cannot* be mistaken for the existing sphere/particle bursts, which directly targets the "looks like everything else" complaint. A particle helix was considered and rejected as the primary approach because it reads as "swirling sparkles" and shares visual language with existing particle effects. (See Open Questions / fallback.)
- **WHO-attribution only; no "what was removed."** The effect conveys *who* got dispelled via head-anchor + motion. It deliberately does **not** surface which buff/debuff was stripped (no icon, no floating text) — that adds text/icon plumbing and clutter for marginal value. Kept as pure VFX.
- **Motion is part of the fix, not polish.** The upward rise and longer lifetime (R3, R4) are load-bearing requirements — they do as much attribution work as the silhouette. A static ribbon at the old 0.5s would only partially solve the problem.

## Scope Boundaries

- **Not in scope:** Surfacing the removed aura's name or icon (text/icon UI).
- **Not in scope:** Changing dispel *mechanics*, success rules, or which auras are dispellable — visual only.
- **Not in scope:** Reworking other burst effects (heals, impacts, Psychic Scream) to deduplicate visual language. This change makes the dispel distinct; harmonizing the rest is separate work.
- **Fallback, not a parallel deliverable:** The particle-helix approach is documented as a fallback if the ribbon mesh's feel proves fussy in practice, but it is not built alongside the ribbon.

## Open Questions

- **Ribbon geometry generation** — how the helical strip mesh is built and animated (vertex generation, taper, UV/alpha along length, transparent-mesh sorting) is an implementation decision for planning. The brainstorm commits to the ribbon *shape and behavior*, not the mesh-construction technique.
- **Exact tuning values** — final lifetime, rise speed, twist rate/turns, ribbon width and length, and emissive intensity are tuning to be dialed in visually during implementation.

## Success Criteria

- Watching a match, a player can tell a dispel landed and on which combatant, without prior knowledge of where to look.
- The dispel effect is not visually confusable with heal/impact/other bursts.
- `cargo test` passes (including `registration_audit`); headless match outcomes are bit-identical to before (no combat behavior change).

## References

- Current effect: `src/states/play_match/rendering/effects.rs:931` (`spawn_dispel_visuals` / `update_dispel_bursts` / `cleanup_expired_dispel_bursts`)
- Component: `src/states/play_match/components/visual.rs:145` (`DispelBurst`)
- Spawn site: `src/states/play_match/effects/dispels.rs:88`
- Color mapping: `dispel_burst_colors` in `rendering/effects.rs:935`
- System registration: `src/states/mod.rs:200`
- Effect pattern reference: `docs/solutions/implementation-patterns/adding-visual-effect-bevy.md`
