//! The Medallion of the Alliance's break (AS-432) — graphical only.
//!
//! The client plays SpellVisual 86 for the PvP trinket (TBC 42292, Era
//! Insignia 5579): kit 270 on both hands — Power Word: Shield's cast kit, the
//! Holy launch `heal_cast.rs` already draws — and kit 226 on the wearer,
//! `dispel_low_base.m2`, Dispel Magic's impact, which the engine draws as the
//! dispel ribbon (`dispel_ribbon.rs`). It has no state: the break is the whole
//! visual (`docs/design/2026-10-29-trinket-effects-client-data.md` §2).
//!
//! Three layers, from the Trinket Effects Bench's "Signed off" preset, Medallion
//! tab ([`MEDALLION_BENCH`], version id [`MEDALLION_BENCH_VERSION`]):
//!
//! 1. **The hands.** The Holy launch on the wearer, at its shipped values: the
//!    self-cast cue.
//! 2. **The ribbon.** The dispel ribbon on the wearer, from the feet as a
//!    Priest's Dispel Magic, 1.15 s with its sparks, in the client's gold
//!    (`ribbonblur1bea_gold`). The strip is darkened to the level every dispel
//!    ribbon shares (`SAND_RIBBON_LEVEL`, AS-435's ruling), and it stands on the
//!    wearer's DRAWN body (`place_medallion_ribbons`). Its sparks are the gold at
//!    full level, the engine's rule for every ribbon's sparks.
//! 3. **The broken crowd control scatters** (authored). The CC's own visual
//!    flies apart instead of playing its end, so the eye reads it broken rather
//!    than expired. The stun whirl's beads scatter (`hard_cc.rs`,
//!    [`stun_scatter_bead`](super::stun_scatter_bead)); every other kind the
//!    Medallion breaks keeps its ordinary end ([`break_end`]).
//!
//! **When.** A use is an [`OnUsePending`] the wearer's AI spawns in Phase 2 and
//! `process_on_use_items` resolves in Phase 1 of the next sim tick.
//! [`spawn_medallion_breaks`] reads it in `FixedUpdate` just before that
//! resolution, so it sees the wearer exactly as the break does: alive or not,
//! and holding exactly the crowd control the break takes off. A Medallion that
//! waits for a teammate's dispel has spawned no pending, so it draws nothing
//! until it fires.
//!
//! No `game_rng` draw, no sim component written: headless stays
//! byte-identical by construction.

use bevy::color::LinearRgba;
use bevy::prelude::*;

use super::heal_cast::{flash_hands_of_kind, HealCastAssets};
use crate::states::play_match::components::*;
use crate::states::play_match::on_use_items::{cc_break_breaks, OnUseEffect, OnUsePending};

/// The bench the Medallion's layers were signed off on (AS-338).
pub const MEDALLION_BENCH: &str = "https://claude.ai/artifact/1FRTwgEeJZWYK1iFcHPkY2";
/// Its signed-off version id.
pub const MEDALLION_BENCH_VERSION: &str = "1793333359-b46c";

/// The ribbon's gold: the client's `ribbonblur1bea_gold`, the bench's
/// `RIBBON_COL.gold.base` (sRGB).
pub const MEDALLION_RIBBON_GOLD: Color = Color::srgb(1.0, 0.82, 0.0);
/// Its emissive, the bench's `RIBBON_COL.gold.em` (linear). The ribbon trims
/// it to 0.6 and holds it at the shared level, as every dispel ribbon's.
pub const MEDALLION_RIBBON_GOLD_EMISSIVE: LinearRgba = LinearRgba::new(2.5, 2.05, 0.6, 1.0);
/// The ribbon's life: the dispel ribbon's (`process_dispels`), the bench's
/// `RIB.life`.
pub const MEDALLION_RIBBON_SECS: f32 = 1.15;
/// The hands: kit 270, the Holy launch.
pub const MEDALLION_HANDS: HealCastKind = HealCastKind::Holy;

/// What the client draws when an on-use effect lands, if the effect owns a
/// visual of its own. The one routing for this module's spawn and the
/// lands-silently audit's on-use family. A buff draws through its aura's own
/// route instead (the aura band).
pub fn on_use_visual(effect: OnUseEffect) -> Option<&'static str> {
    match effect {
        OnUseEffect::CcBreak => {
            Some("Holy launch hands, gold dispel ribbon, broken stun scatters (medallion.rs)")
        }
        OnUseEffect::Buff(_) => None,
    }
}

/// How a crowd control the Medallion breaks ends on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BreakEnd {
    /// Its rig scatters (`hard_cc.rs`).
    Scatters(CcKind),
    /// It plays its ordinary end: no scatter carries over to its visual
    /// directly. The card that will draw one, and why.
    KnownSilent {
        card: &'static str,
        why: &'static str,
    },
    /// The Medallion does not break it.
    NotBroken,
    /// Broken, but given no row here. `medallion_break_ends_cover_every_cc`
    /// fails on any aura that lands here.
    Unrouted,
}

/// How the crowd control `aura` ends when the Medallion breaks it.
pub fn break_end(aura: AuraType) -> BreakEnd {
    if !cc_break_breaks(aura) {
        return BreakEnd::NotBroken;
    }
    let silent = |why| BreakEnd::KnownSilent {
        card: "AS-432",
        why,
    };
    match aura {
        AuraType::Stun => BreakEnd::Scatters(CcKind::Stun),
        AuraType::Fear => silent("a body tint and a fitted shroud (fear.rs), not beads"),
        AuraType::Polymorph => silent("a mesh swap with a puff (polymorph.rs), not beads"),
        AuraType::Incapacitate => silent("Freezing Trap's lit ice block (traps.rs), not beads"),
        AuraType::Root => {
            silent("lit crystals, a web or bark roots (hard_cc.rs), which cannot fade")
        }
        AuraType::MovementSpeedSlow => silent("the slow ring at the feet (slow_ring.rs)"),
        AuraType::Cyclone => silent("the funnel and the lift (druid_control.rs)"),
        _ => BreakEnd::Unrouted,
    }
}

/// The rigs a break of `auras` scatters: every crowd control the break takes
/// off whose end is a scatter. What it takes off is read off a copy broken by
/// the break's own rule (`ActiveAuras::break_debuffs_where` with
/// `cc_break_breaks`, a compound's riders included), so the two cannot
/// disagree.
pub fn break_scatters(auras: Option<&ActiveAuras>) -> Vec<CcKind> {
    let Some(before) = auras else {
        return Vec::new();
    };
    let mut after = ActiveAuras {
        auras: before.auras.clone(),
    };
    after.break_debuffs_where(cc_break_breaks);
    let count = |list: &[Aura], t: AuraType| list.iter().filter(|a| a.effect_type == t).count();
    let mut kinds = Vec::new();
    for aura in &before.auras {
        let t = aura.effect_type;
        if count(&before.auras, t) == count(&after.auras, t) {
            continue;
        }
        if let BreakEnd::Scatters(kind) = break_end(t) {
            if !kinds.contains(&kind) {
                kinds.push(kind);
            }
        }
    }
    kinds
}

/// `FixedUpdate`, just before `process_on_use_items` resolves the uses the AI
/// queued last tick: for each Medallion use whose wearer is alive, the Holy
/// launch on its hands, the gold ribbon on it, and — when the break takes off
/// a crowd control whose end scatters — a [`MedallionBreak`] for
/// `update_hard_cc_visuals` to read.
#[allow(clippy::too_many_arguments)]
pub fn spawn_medallion_breaks(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<HealCastAssets>>,
    uses: Query<&OnUsePending>,
    wearers: Query<(&Combatant, Option<&ActiveAuras>)>,
    casters: Query<&Children>,
    bodies: Query<(), With<VisualBody>>,
) {
    for pending in uses.iter() {
        if on_use_visual(pending.config.effect).is_none() {
            continue;
        }
        // `process_on_use_items` resolves nothing for a wearer that is gone
        // or dead, so nothing is drawn for one.
        let Ok((combatant, auras)) = wearers.get(pending.wearer) else {
            continue;
        };
        if !combatant.is_alive() {
            continue;
        }
        flash_hands_of_kind(
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut images,
            &mut assets,
            &casters,
            &bodies,
            pending.wearer,
            MEDALLION_HANDS,
        );
        commands.spawn((
            DispelRibbon {
                target: pending.wearer,
                source: RibbonSource::Medallion,
                lifetime: MEDALLION_RIBBON_SECS,
                initial_lifetime: MEDALLION_RIBBON_SECS,
                spin: 0.0,
            },
            PlayMatchEntity,
        ));
        let scatters = break_scatters(auras);
        if !scatters.is_empty() {
            commands.spawn((
                MedallionBreak {
                    wearer: pending.wearer,
                    scatters,
                },
                PlayMatchEntity,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every crowd control the Medallion breaks has a row, and nothing else
    /// does.
    #[test]
    fn medallion_break_ends_cover_every_cc() {
        for aura in AuraType::ALL {
            let end = break_end(aura);
            assert_ne!(end, BreakEnd::Unrouted, "{aura:?} is broken but unrouted");
            assert_eq!(
                end != BreakEnd::NotBroken,
                cc_break_breaks(aura),
                "{aura:?}"
            );
        }
    }
}
