//! Sand legibility (AS-435): the levels the game's general effects share so they
//! read on the bright arena sand.
//!
//! The sand sits on the tonemapper's shoulder, so added light barely moves a
//! pixel there (AS-401). What reads is DARKENING: an alpha-blended layer in the
//! effect's own colour, or a lower level on an opaque lit surface. Every value
//! here is the Sand Legibility Bench's signed-off proposal
//! (`docs/design/benches/2026-10-29-sand-legibility-bench.html`, artifact
//! <https://claude.ai/artifact/8qLsKmuRQTVwa5Kzdimkgo>, version id
//! 1793335026-f991), which composites through Bevy's own TonyMcMapface LUT over
//! sand calibrated to a match, at the match camera.
//!
//! Only the levels other effects REUSE live here: the dispel ribbon's (the
//! Medallion's gold ribbon follows it, AS-432) and the crescents' (Hand of
//! Justice borrows the rogue crescents, AS-434). The stun whirl's disc and the
//! aura band's levels are their own effects' and sit in `hard_cc.rs` and
//! `aura_band.rs`, citing the same bench.

use bevy::prelude::*;

/// The bench these values were signed off on.
pub const SAND_LEGIBILITY_BENCH: &str = "https://claude.ai/artifact/8qLsKmuRQTVwa5Kzdimkgo";
/// Its signed-off version id.
pub const SAND_LEGIBILITY_BENCH_VERSION: &str = "1793335026-f991";

/// × an opaque ribbon's WHOLE colour in linear light: the lit base, the
/// emissive and the ignition spike on it (the spike's own boost is kept). One
/// level for every dispeller, so each keeps its hue and reads dark on the sand.
pub const SAND_RIBBON_LEVEL: f32 = 0.1;

/// × a blended crescent stroke's texture opacity, capped at full opacity.
pub const SAND_CRESCENT_GAIN: f32 = 2.0;
/// × a blended crescent stroke's colour track in linear light. 1.0 keeps
/// Cheap Shot's stroke near its white.
pub const SAND_CRESCENT_LEVEL: f32 = 1.0;

/// `color` scaled by `level` in LINEAR light, as the bench scales it, with
/// `alpha`.
pub fn linear_level(color: Color, level: f32, alpha: f32) -> Color {
    let l = color.to_linear();
    Color::LinearRgba(LinearRgba::new(
        l.red * level,
        l.green * level,
        l.blue * level,
        alpha,
    ))
}

/// A linear emissive scaled by `level`.
pub fn emissive_level(emissive: LinearRgba, level: f32) -> LinearRgba {
    LinearRgba::new(
        emissive.red * level,
        emissive.green * level,
        emissive.blue * level,
        emissive.alpha,
    )
}
