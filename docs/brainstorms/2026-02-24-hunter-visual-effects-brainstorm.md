# Hunter Visual Effects Brainstorm

**Date:** 2026-02-24
**Status:** Complete
**Scope:** Full Hunter visual package — traps, slow zones, disengage, ranged shots, pet abilities, and Freezing Trap ice block

## What We're Building

Visual effects for all Hunter and Hunter pet abilities. Currently, the Hunter class has fully functional combat mechanics but zero visual feedback beyond floating combat text and speech bubbles. This brainstorm covers every Hunter-unique visual.

## Key Decisions

### 1. Trap Placement Visual — Glowing Ground Circle

Traps appear as flat disc meshes on the ground with two phases:

- **Arming phase (1.5s):** Dim glow, slow alpha pulse. Communicates "trap is being set."
- **Armed phase:** Full emissive brightness, subtle shimmer. Communicates "trap is ready."

**Colors:**
- Frost Trap: Cyan `(0.3, 0.8, 1.0)` with blue emissive `(0.4, 1.2, 2.0)`
- Freezing Trap: Ice-white `(0.8, 0.9, 1.0)` with cool-white emissive `(1.6, 1.8, 2.0)`

**Mesh:** Flat cylinder, radius ~1.5, height ~0.05, placed at y=0.02.

### 2. Trap Trigger — Burst + Target Effect

When a trap triggers:

1. Trap disc flashes full brightness (1 frame)
2. Expanding ring burst outward (reuse DispelBurst pattern — spawn sphere, scale 1.0 → 3.0, fade alpha)
3. Apply target-specific effect:
   - **Freezing Trap** → Ice Block on target (see below)
   - **Frost Trap** → Spawn SlowZone ground disc (see below)

**Burst color:** Matches trap color (cyan for Frost, ice-white for Freezing).

### 3. Freezing Trap Ice Block — Translucent Cuboid

When Freezing Trap incapacitates a target, spawn a semi-transparent ice cuboid around them:

- **Mesh:** Cuboid `1.2 x 2.0 x 1.2` (slightly larger than character capsule)
- **Material:** Ice-blue `(0.6, 0.85, 1.0)`, alpha 0.4, `AlphaMode::Blend` (needs transparency, not additive)
- **Emissive:** Frost glow `(0.8, 1.2, 2.0)`
- **Target visible inside** the ice block
- **On break:** Flash bright + despawn (could reuse DispelBurst pattern for the shatter flash)

**Component:** `IceBlockVisual { target: Entity, lifetime: f32 }` — follows target position, cleaned up when Incapacitate aura expires or breaks on damage.

**Note:** This is NOT the same as ShieldBubble (which is a sphere). This is a sharp-edged cuboid for a frozen/encased feel.

### 4. Frost Trap SlowZone — Flat Pulsing Disc

When Frost Trap triggers, it spawns a SlowZone entity. The visual:

- **Mesh:** Flat cylinder, radius = `FROST_TRAP_ZONE_RADIUS` (8.0), height ~0.03, y=0.02
- **Material:** Cyan `(0.3, 0.8, 1.0)`, low alpha (0.15-0.25), `AlphaMode::Add`
- **Animation:** Gentle alpha pulse (sine wave, period ~2s)
- **Fade out:** In the last 2 seconds of `duration_remaining`, lerp alpha → 0

**Component:** Attach visual directly to the existing `SlowZone` entity (it already has a Transform).

### 5. Disengage — Wind Streak Trail

When the Hunter disengages (backward leap, 15 yards):

- **Mesh:** Elongated cylinder stretched along the leap direction
- **Material:** White/light-blue `(0.85, 0.9, 1.0)` with wind-like emissive `(1.5, 1.7, 2.0)`
- **Behavior:** Spawned at start position, oriented along `disengage.direction`. Fades alpha over 0.4s, then despawns.
- **Height:** Mid-body level, y=0.5

**Component:** `DisengageTrail { lifetime: f32 }` — static position (doesn't follow the Hunter), just fades and cleans up.

### 6. Ranged Shots — Arrow-Shaped Projectiles

Hunter projectiles use thin elongated cuboids instead of the default sphere projectile mesh:

- **Mesh:** Cuboid `0.6 x 0.08 x 0.08`, oriented along flight path (rotated to face target)
- **Flight:** Uses existing `Projectile` component and `projectile_speed` from abilities.ron

**Per-ability colors:**
- **Auto Shot:** Brown/gold `(0.8, 0.6, 0.3)`, no emissive (understated, frequent shot)
- **Aimed Shot:** Gold `(1.0, 0.85, 0.4)`, bright emissive `(1.5, 1.3, 0.6)` (big shot = big glow)
- **Arcane Shot:** Golden-arcane `(1.0, 0.8, 0.3)`, arcane emissive `(1.5, 1.2, 0.5)`

**Implementation:** The existing `spawn_projectile_visuals` system creates the mesh for `Added<Projectile>` entities. Add a check: if the projectile's ability is a Hunter shot, use cuboid mesh instead of sphere. This keeps projectile flight/hit logic unchanged.

### 7. Concussive Shot — Arrow + Impact Flash

Concussive Shot fires an arrow projectile (Physical color) and on hit, spawns a brief impact flash on the target:

- **Arrow:** Same Physical arrow as Auto Shot
- **Impact:** Small golden flash/stun-star at target position, fades over 0.3s
- **Component:** `ConcussiveImpact { lifetime: f32 }` at target position

### 8. Pet Abilities — Full Visual Treatment

**Spider Web:**
- White thread-like projectile (elongated cuboid, white `(0.95, 0.95, 0.9)`)
- On hit: brief white flash on target (root applied)

**Boar Charge:**
- Speed streak trail behind the Boar during charge movement (similar to Disengage trail but brown/earthy)
- Component: `ChargeTrail { lifetime: f32 }` — spawned at charge start, fades 0.3s

**Bird Master's Call:**
- Golden flash/burst on the Hunter when Master's Call removes a debuff
- Reuse DispelBurst pattern with gold color `(1.0, 0.85, 0.3)`, emissive `(2.0, 1.7, 0.6)`

## Implementation Pattern

All effects follow the established 3-system pattern:

```
1. spawn_<effect>_visuals()   — Added<Component> filter, create mesh + material
2. update_<effect>()          — per-frame animation (pulse, fade, follow target)
3. cleanup_<effect>()         — despawn when lifetime <= 0 or source component removed
```

Register in `states/mod.rs` only (headless mode must not load visual systems).

## New Visual Components Summary

| Component | Mesh | Follows Target? | Lifetime Source |
|-----------|------|-----------------|-----------------|
| `TrapVisual` | Flat cylinder | No (static position) | Cleaned up when `Trap` despawns |
| `TrapBurst` | Expanding sphere | No (static position) | 0.3s timer |
| `IceBlockVisual` | Cuboid 1.2x2.0x1.2 | Yes (tracks target) | Until Incapacitate aura removed |
| `SlowZoneVisual` | Flat cylinder r=8 | No (on SlowZone entity) | Matches `duration_remaining` |
| `DisengageTrail` | Elongated cylinder | No (static at start pos) | 0.4s timer |
| `ConcussiveImpact` | Small sphere | No (at impact position) | 0.3s timer |
| `ChargeTrail` | Elongated cylinder | No (static at start pos) | 0.3s timer |

**Arrow projectiles** don't need a new component — modify `spawn_projectile_visuals` to use cuboid mesh when ability is a Hunter shot.

**Master's Call** reuses the existing `DispelBurst` component with gold color.

## Open Questions

- **Trap visibility to enemy team:** In WoW, traps are invisible to enemies. Should we hide the trap disc for the opposing team? Currently there's no per-team rendering filter. Could defer this to a future "fog of war" feature.
- **Ice block shatter particles:** When ice block breaks, should there be debris particles flying outward? Would require spawning multiple small entities. Could be a nice-to-have.
- **Arrow orientation:** The elongated cuboid needs to be rotated to face the target during flight. The existing projectile system moves projectiles toward target but may not rotate the Transform. Need to verify and add rotation if missing.

## References

- Existing visual effects: `src/states/play_match/rendering/effects.rs`
- Visual components: `src/states/play_match/components/visual.rs`
- Projectile system: `src/states/play_match/projectiles.rs`
- System registration: `src/states/mod.rs` (line 181+)
- Visual effect implementation guide: `docs/solutions/implementation-patterns/adding-visual-effect-bevy.md`
