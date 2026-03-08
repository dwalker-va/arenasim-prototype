---
title: "feat: Make freezing trap easier to see - deeper blue and larger"
type: feat
status: completed
date: 2026-03-07
deepened: 2026-03-07
---

# Make Freezing Trap Easier to See - Deeper Blue and Larger

## Enhancement Summary

**Deepened on:** 2026-03-07
**Sections enhanced:** Color differentiation, size validation
**Key findings:**
1. Frost trap uses cyan `(0.3, 0.8, 1.0)` — Freezing must use royal/deep blue (low green) to avoid confusion
2. Trigger radius is 5.0 units — visual radius 2.0 is well within bounds (safe)
3. Burst scale formula needs updating: `1.0 + (1.0 - progress) * 2.0` → `* 3.0` for max scale 4.0

## Overview

The freezing trap's visual effects are currently ice-white/pale blue, making them hard to distinguish during combat. Shift all freezing trap colors to a deeper, more saturated blue and increase the size of the ground circle, burst, and ice block to improve visibility.

## Problem Statement

Current freezing trap colors are near-white (`(0.8, 0.9, 1.0)` base RGB), which blends into the arena floor and is hard to spot. The ground circle (radius 1.5) and burst sphere (max scale 3.0) are also on the small side for a major CC ability.

## Proposed Solution

Update color values and sizes in `src/states/play_match/rendering/effects.rs`. All changes are confined to this single file.

### Color Changes

All in `effects.rs`. **Important:** Frost trap is cyan (high green channel). Freezing must stay in the royal/deep blue range (low green, high blue) to remain visually distinct.

| Element | Current | New | Rationale |
|---|---|---|---|
| `trap_type_rgb` (line 1068) | `(0.8, 0.9, 1.0)` ice-white | `(0.3, 0.55, 1.0)` deep blue | Low green separates from Frost's cyan `(0.3, 0.8, 1.0)` |
| `trap_type_emissive` (line 1076) | `LinearRgba::new(1.6, 1.8, 2.0, 1.0)` | `LinearRgba::new(0.6, 1.2, 2.8, 1.0)` deep blue glow | Blue-dominant emissive, distinct from Frost's `(0.4, 1.2, 2.0)` |
| Burst emissive (line 1168) | `LinearRgba::new(2.0, 2.2, 2.5, 1.0)` | `LinearRgba::new(0.8, 1.5, 3.5, 1.0)` bright blue burst | Higher blue channel for dramatic trigger flash |
| Ice block base_color (line 1268) | `srgba(0.6, 0.85, 1.0, 0.4)` | `srgba(0.3, 0.6, 1.0, 0.45)` deeper blue, slightly more opaque | More saturated blue, +0.05 alpha for visibility |
| Ice block emissive (line 1269) | `LinearRgba::new(0.8, 1.2, 2.0, 1.0)` | `LinearRgba::new(0.5, 1.0, 2.8, 1.0)` deeper blue glow | Matches overall deeper blue theme |

**Color spectrum reference (for visual distinction):**
- Frost trap: **Cyan** — `(0.3, 0.8, 1.0)` base, green-dominant
- Freezing trap (new): **Royal blue** — `(0.3, 0.55, 1.0)` base, blue-dominant
- Priest dispel: **White-blue** — `(0.85, 0.85, 1.0)` base, neutral
- Disengage trail: **Pale blue-white** — `(0.85, 0.9, 1.0)` base, neutral

### Size Changes

All in `effects.rs`:

| Element | Current | New | Notes |
|---|---|---|---|
| Ground circle cylinder (line 1093) | `Cylinder::new(1.5, 0.05)` | `Cylinder::new(2.0, 0.05)` | Trigger radius is 5.0 — visual 2.0 is well within bounds |
| Burst sphere (line 1161) | `Sphere::new(0.5)` | `Sphere::new(0.6)` | Slightly larger initial sphere |
| Burst max scale (line 1206) | `1.0 + (1.0 - progress) * 2.0` (max 3.0) | `1.0 + (1.0 - progress) * 3.0` (max 4.0) | More dramatic expansion on trigger |
| Ice block cuboid (line 1266) | `Cuboid::new(1.2, 2.0, 1.2)` | `Cuboid::new(1.5, 2.3, 1.5)` | Larger encasing block, more imposing |

### Doc Comment Update

Update the doc comment on `trap_type_rgb` (line 1064) from:
```rust
/// Base RGB color for a trap type. Frost = cyan, Freezing = ice-white.
```
to:
```rust
/// Base RGB color for a trap type. Frost = cyan, Freezing = deep blue.
```

## Acceptance Criteria

- [x] Freezing trap ground circle is noticeably blue (not white) — `effects.rs:trap_type_rgb`
- [x] Freezing trap ground circle is larger (radius 2.0) — `effects.rs:spawn_trap_visuals`
- [x] Trigger burst is deeper blue and larger — `effects.rs:spawn_trap_burst_visuals`
- [x] Ice block is deeper blue and slightly larger — `effects.rs:spawn_ice_block_visuals`
- [x] Frost trap visuals are NOT changed (only Freezing)
- [x] Freezing and Frost traps are visually distinguishable (blue vs cyan)
- [x] Headless simulation still compiles and runs

## Context

- All freezing trap visuals are in `src/states/play_match/rendering/effects.rs`
- Helper functions `trap_type_rgb` (line 1065) and `trap_type_emissive` (line 1073) handle ground circle + burst colors
- Ice block has its own hardcoded colors (lines 1268-1269, Freezing-specific)
- Burst scale formula is at line 1206: `let scale = 1.0 + (1.0 - progress) * 2.0;`
- Trigger radius is `TRAP_TRIGGER_RADIUS = 5.0` in `constants.rs:149`
- No data-driven config needed — these are render-only values

## Sources

- Verified code: `src/states/play_match/rendering/effects.rs` (lines 1064-1309)
- Constants: `src/states/play_match/constants.rs` (line 149, trigger radius)
- Pattern doc: `docs/solutions/implementation-patterns/adding-visual-effect-bevy.md`
