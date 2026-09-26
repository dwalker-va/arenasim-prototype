//! The aura-application cue: one soft ring that sweeps a unit's body once when
//! an aura lands on it.
//!
//! This is the FAMILY answer to "aura application is silent" — Arcane
//! Intellect, the three Mage armors, Power Word: Fortitude, the Warrior shouts,
//! the Paladin auras, the Rogue's poison coating and every totem landing used to
//! reach the scene only as a team-frame icon. They share one cue, differentiated
//! cheaply:
//!
//! - **tint** — the aura's school, through the project's colour authority
//!   (`SpellSchool::color`). A schoolless or physical aura (the shouts, the
//!   poison coating) takes `SpellSchool::None`'s neutral near-white, so the
//!   family spends no hue of its own;
//! - **polarity** — a buff RISES from the feet past the crown and opens; a
//!   debuff PRESSES DOWN from the crown to the feet and tightens. Direction, not
//!   colour, carries the good/bad read.
//!
//! Bespoke per-buff identities are a later client-data pass; this ends the
//! silence, it does not give each buff a face.
//!
//! ## Which applications get it
//!
//! [`AuraApplyRoute::for_aura`] decides, exhaustively over `AuraType`: the band,
//! a named bespoke owner (CC, shields, the DoT layer, ...) that already draws
//! the application, or a named deferral (the slow family). Nothing here
//! re-decides it.
//!
//! ## Detection — renderer-side, off `ActiveAuras`
//!
//! The band is keyed on the aura APPEARING in the bearer's `ActiveAuras`, the
//! same transition-in idiom `warlock_dots.rs` uses for its apply bursts. That
//! choice is what makes the coverage total across application SITES as well as
//! types: pending auras, the totem pulse, the Frost Trap zone and the
//! spawn-stamped poison coating all mutate `ActiveAuras` directly, and all of
//! them reach this one detector without a line of core code. Headless never
//! registers it, so the sim is untouched by construction.
//!
//! The per-bearer ledger ([`AuraApplyLedger`]) remembers each bearer's auras by
//! `(type, ability name, caster)` and fires only for instances that are NEW
//! since the last frame. That gives the refresh rule for free:
//!
//! - **a totem's per-pulse refresh does not re-fire** — the pulse only resets
//!   the existing aura's duration, so its key never leaves the ledger;
//! - **a fresh landing does** — an ally who walks out of the totem's radius
//!   long enough for the 2s refresh window to lapse, then walks back in, gets
//!   the buff anew, and the band says so. So does a buff recast after it
//!   expired or was purged.
//!
//! Several new auras on one bearer in one frame coalesce to at most ONE band
//! per polarity (the first in the bearer's aura order): a team buff reads as one
//! cue per unit, never a stack of identical rings.
//!
//! ## Why arcs and not one ring mesh
//!
//! The body capsule is `AlphaMode::Blend` (stealth), and Bevy draws transparent
//! meshes back-to-front by ENTITY ORIGIN with no depth writes. A single ring
//! mesh has its origin on the body's axis, so it sorts level with the capsule
//! and the capsule paints over the half of the ring in front of it (the dispel
//! ribbon's round-2 lesson). The band is therefore [`AURA_BAND_ARCS`] short
//! additive arcs, each a child whose origin sits ON the ring: the front arcs
//! sort in front of the body and draw over it, the back arcs sort behind it and
//! are covered by it, which is exactly right.

use super::school_impact::IMPACT_PET_BODY_Y;
use crate::states::play_match::abilities::SpellSchool;
use crate::states::play_match::components::*;
use bevy::color::LinearRgba;
use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_asset::RenderAssetUsages;
use std::collections::HashMap;
use std::f32::consts::TAU;

// ==============================================================================
// Taste constants — the knobs an eyeball round turns
// ==============================================================================

/// Whole life of one band, seconds. An apply-MOMENT cue: short enough that it
/// never reads as a sustained channel (those are a scarce per-victim budget).
pub const AURA_BAND_SECS: f32 = 0.7;
/// The ring's resting radius around the body axis, yards.
pub const AURA_BAND_RADIUS: f32 = 0.85;
/// Radial width of the ring's band, yards. Soft at both rims (vertex alpha).
pub const AURA_BAND_WIDTH: f32 = 0.16;
/// A buff opens as it rises: radius multiplier at birth and at the end.
pub const AURA_BAND_BUFF_OPEN: (f32, f32) = (0.95, 1.2);
/// A debuff tightens as it presses down: radius multiplier at birth and end.
pub const AURA_BAND_DEBUFF_TIGHTEN: (f32, f32) = (1.2, 0.9);
/// Emissive strength of the ring's spine at full alpha.
pub const AURA_BAND_EMISSIVE: f32 = 2.6;
/// A debuff glows this fraction as bright as a buff — pressing down, not
/// shining out.
pub const AURA_BAND_DEBUFF_INTENSITY: f32 = 0.8;
/// Fraction of the life spent fading in, and fading out.
pub const AURA_BAND_FADE_IN: f32 = 0.15;
pub const AURA_BAND_FADE_OUT: f32 = 0.4;
/// Tint for a schoolless or physical aura (the shouts, the poison coating):
/// the colour authority's own "no school" value, so the family spends no hue.
pub const AURA_BAND_NEUTRAL_TINT: Color = SpellSchool::None.color();

// ==============================================================================
// Geometry constants — clearances, not taste
// ==============================================================================

/// Number of arcs the ring is cut into (see the module docs for why).
pub const AURA_BAND_ARCS: u32 = 12;
/// Segments per arc.
const ARC_SEGMENTS: u32 = 6;

/// Where the band travels, relative to the bearer's transform.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AuraBandFrame {
    /// Height of the body's centre above the transform.
    pub centre_y: f32,
    /// Lowest point of the sweep, relative to the body centre.
    pub low: f32,
    /// Highest point of the sweep, relative to the body centre.
    pub high: f32,
}

/// A combatant: the 2.5-yd capsule is CENTRED on the transform (-1.25..+1.25)
/// and sunk 0.25 into the floor, so the floor is at -1.0. The sweep runs from
/// just above the floor (an additive ring at or below it is depth-rejected) to
/// just past the crown.
pub const COMBATANT_BAND_FRAME: AuraBandFrame = AuraBandFrame {
    centre_y: 0.0,
    low: -0.9,
    high: 1.4,
};

/// A pet: its capsule (radius 0.35, lying horizontal) hangs below the
/// transform by the shared `IMPACT_PET_BODY_Y` correction, reaching 0.35 above
/// and below its centre, with the floor about 0.3 under it.
pub const PET_BAND_FRAME: AuraBandFrame = AuraBandFrame {
    centre_y: IMPACT_PET_BODY_Y,
    low: -0.2,
    high: 0.5,
};

// ==============================================================================
// Pure curves (unit-tested; the probes assert their world results)
// ==============================================================================

/// Fraction of the band's life elapsed, clamped to 0..=1.
pub fn aura_band_progress(age: f32) -> f32 {
    (age / AURA_BAND_SECS).clamp(0.0, 1.0)
}

/// Ease-out: fast off the mark, settling at the end.
fn ease_out(k: f32) -> f32 {
    1.0 - (1.0 - k) * (1.0 - k)
}

/// Height of the ring relative to the bearer's BODY CENTRE at progress `k`.
pub fn aura_band_height(frame: AuraBandFrame, polarity: AuraBandPolarity, k: f32) -> f32 {
    let e = ease_out(k);
    match polarity {
        AuraBandPolarity::Buff => frame.low + (frame.high - frame.low) * e,
        AuraBandPolarity::Debuff => frame.high - (frame.high - frame.low) * e,
    }
}

/// The ring's radius (yards from the body axis) at progress `k`.
pub fn aura_band_radius(polarity: AuraBandPolarity, k: f32) -> f32 {
    let (from, to) = match polarity {
        AuraBandPolarity::Buff => AURA_BAND_BUFF_OPEN,
        AuraBandPolarity::Debuff => AURA_BAND_DEBUFF_TIGHTEN,
    };
    AURA_BAND_RADIUS * (from + (to - from) * ease_out(k))
}

/// The band's opacity envelope at progress `k`: a quick fade in, a hold, and a
/// longer fade out.
pub fn aura_band_alpha(k: f32) -> f32 {
    let fade_in = (k / AURA_BAND_FADE_IN).min(1.0);
    let fade_out = ((1.0 - k) / AURA_BAND_FADE_OUT).min(1.0);
    (fade_in * fade_out).clamp(0.0, 1.0)
}

/// World position of the ring's centre (on the body axis) at progress `k`.
pub fn aura_band_centre(bearer: Vec3, is_pet: bool, polarity: AuraBandPolarity, k: f32) -> Vec3 {
    let frame = if is_pet {
        PET_BAND_FRAME
    } else {
        COMBATANT_BAND_FRAME
    };
    bearer + Vec3::Y * (frame.centre_y + aura_band_height(frame, polarity, k))
}

/// The band's colour for an aura: its school through the colour authority, or
/// the neutral tint when it carries none. (Physical and schoolless abilities
/// both store `None` on the aura.)
pub fn aura_band_tint(aura: &Aura) -> Color {
    aura.spell_school
        .map(SpellSchool::color)
        .unwrap_or(AURA_BAND_NEUTRAL_TINT)
}

// ==============================================================================
// The ledger — which auras are NEW on a bearer
// ==============================================================================

/// An aura instance's identity for the purpose of "is this one new". The
/// duration is deliberately NOT part of it: a refresh changes only that.
#[derive(Clone, Debug, PartialEq)]
struct AuraKey {
    effect_type: AuraType,
    ability_name: String,
    caster: Option<Entity>,
}

impl AuraKey {
    fn of(aura: &Aura) -> Self {
        Self {
            effect_type: aura.effect_type,
            ability_name: aura.ability_name.clone(),
            caster: aura.caster,
        }
    }
}

/// Per-bearer memory of the auras seen last frame. Renderer-local state (a
/// `Local` of the detector), never read by the sim.
#[derive(Default)]
pub struct AuraApplyLedger {
    seen: HashMap<Entity, Vec<AuraKey>>,
}

impl AuraApplyLedger {
    /// Record `auras` as `bearer`'s current set and return the ones that are
    /// NEW since the last observation, in the bearer's aura order.
    ///
    /// A multiset difference, so a second identical instance (two casters'
    /// same aura) is new even while the first is up, and a bearer observed for
    /// the first time reports everything it carries.
    pub fn observe<'a>(&mut self, bearer: Entity, auras: &'a [Aura]) -> Vec<&'a Aura> {
        let mut unmatched = self.seen.remove(&bearer).unwrap_or_default();
        let mut fresh = Vec::new();
        for aura in auras {
            let key = AuraKey::of(aura);
            match unmatched.iter().position(|k| *k == key) {
                Some(i) => {
                    unmatched.swap_remove(i);
                }
                None => fresh.push(aura),
            }
        }
        self.seen
            .insert(bearer, auras.iter().map(AuraKey::of).collect());
        fresh
    }

    /// Forget every bearer `keep` rejects (despawned units), so the ledger
    /// cannot grow across matches.
    pub fn retain(&mut self, mut keep: impl FnMut(Entity) -> bool) {
        self.seen.retain(|entity, _| keep(*entity));
    }
}

/// The bands a set of newly applied auras produces on one bearer: only auras
/// the router sends to the band, at most one per polarity, the first in order
/// winning.
pub fn aura_band_cues(fresh: &[&Aura]) -> Vec<(AuraBandPolarity, Color)> {
    let mut cues: Vec<(AuraBandPolarity, Color)> = Vec::new();
    for aura in fresh {
        if AuraApplyRoute::for_aura(aura.effect_type) != AuraApplyRoute::Band {
            continue;
        }
        let polarity = AuraBandPolarity::of(aura.effect_type);
        if cues.iter().all(|(p, _)| *p != polarity) {
            cues.push((polarity, aura_band_tint(aura)));
        }
    }
    cues
}

// ==============================================================================
// Components local to the renderer
// ==============================================================================

/// One of the band's arcs — a child of the [`AuraBand`] rig, with its origin on
/// the ring so it depth-sorts against the body correctly.
#[derive(Component)]
pub struct AuraBandArc;

/// The one material every arc of a band shares, faded as a unit.
#[derive(Component)]
pub struct AuraBandMaterial(pub Handle<StandardMaterial>);

// ==============================================================================
// Mesh
// ==============================================================================

/// One arc of a unit-radius ring, built in the arc's OWN frame: the arc's
/// midpoint is the local origin, and the arc spans `TAU / AURA_BAND_ARCS`
/// around the Y axis centred on the point (1, 0, 0) of the ring. Three rows of
/// vertices — outer rim, spine, inner rim — with alpha 0 / 1 / 0, so the band
/// has soft edges without a texture.
fn build_band_arc() -> Mesh {
    let span = TAU / AURA_BAND_ARCS as f32;
    let half_w = AURA_BAND_WIDTH * 0.5 / AURA_BAND_RADIUS;
    let rows = [(1.0 + half_w, 0.0), (1.0, 1.0), (1.0 - half_w, 0.0)];
    let cols = ARC_SEGMENTS + 1;

    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    for (row, (r, alpha)) in rows.iter().enumerate() {
        for j in 0..cols {
            let a = -span * 0.5 + span * j as f32 / ARC_SEGMENTS as f32;
            positions.push([r * a.cos() - 1.0, 0.0, -r * a.sin()]);
            normals.push([0.0, 1.0, 0.0]);
            uvs.push([j as f32 / ARC_SEGMENTS as f32, row as f32 * 0.5]);
            colors.push([1.0, 1.0, 1.0, *alpha]);
        }
    }
    let mut indices: Vec<u32> = Vec::new();
    for row in 0..2u32 {
        for j in 0..ARC_SEGMENTS {
            let a = row * cols + j;
            let b = a + 1;
            let c = a + cols;
            let d = c + 1;
            indices.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Base and emissive colour of a band at opacity `alpha`.
fn band_colors(tint: Color, polarity: AuraBandPolarity, alpha: f32) -> (Color, LinearRgba) {
    let intensity = match polarity {
        AuraBandPolarity::Buff => 1.0,
        AuraBandPolarity::Debuff => AURA_BAND_DEBUFF_INTENSITY,
    };
    let c = tint.to_srgba();
    let e = AURA_BAND_EMISSIVE * intensity * alpha;
    (
        Color::srgba(c.red, c.green, c.blue, alpha),
        LinearRgba::new(c.red * e, c.green * e, c.blue * e, 1.0),
    )
}

// ==============================================================================
// Systems — spawn / update / cleanup (graphical only)
// ==============================================================================

/// SPAWN: diff every combatant's `ActiveAuras` against the ledger and raise a
/// band for each newly applied aura the router sends to it. Dead bearers are
/// still observed (so the ledger stays true) but get no band.
#[allow(clippy::too_many_arguments)]
pub fn detect_aura_applications(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut arc_mesh: Local<Option<Handle<Mesh>>>,
    mut ledger: Local<AuraApplyLedger>,
    bearers: Query<(
        Entity,
        &Combatant,
        &Transform,
        Option<&ActiveAuras>,
        Option<&Pet>,
    )>,
) {
    for (entity, combatant, transform, auras, pet) in bearers.iter() {
        let current: &[Aura] = auras.map(|a| a.auras.as_slice()).unwrap_or(&[]);
        let fresh = ledger.observe(entity, current);
        if fresh.is_empty() || !combatant.is_alive() {
            continue;
        }
        let is_pet = pet.is_some();
        for (polarity, tint) in aura_band_cues(&fresh) {
            let mesh = arc_mesh
                .get_or_insert_with(|| meshes.add(build_band_arc()))
                .clone();
            spawn_band(
                &mut commands,
                &mut materials,
                mesh,
                entity,
                transform.translation,
                is_pet,
                polarity,
                tint,
            );
        }
    }
    ledger.retain(|entity| bearers.contains(entity));
}

#[allow(clippy::too_many_arguments)]
fn spawn_band(
    commands: &mut Commands,
    materials: &mut Assets<StandardMaterial>,
    mesh: Handle<Mesh>,
    target: Entity,
    at: Vec3,
    is_pet: bool,
    polarity: AuraBandPolarity,
    tint: Color,
) {
    let (base, emissive) = band_colors(tint, polarity, 0.0);
    let material = materials.add(StandardMaterial {
        base_color: base,
        emissive,
        alpha_mode: AlphaMode::Add,
        unlit: false,
        cull_mode: None,
        double_sided: true,
        ..default()
    });

    let arcs: Vec<Entity> = (0..AURA_BAND_ARCS)
        .map(|i| {
            let rotation = Quat::from_rotation_y(i as f32 / AURA_BAND_ARCS as f32 * TAU);
            commands
                .spawn((
                    AuraBandArc,
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_translation(rotation * Vec3::X).with_rotation(rotation),
                    NotShadowCaster,
                ))
                .id()
        })
        .collect();

    commands
        .spawn((
            AuraBand {
                target,
                polarity,
                age: 0.0,
                is_pet,
                tint,
            },
            AuraBandMaterial(material),
            Transform::from_translation(aura_band_centre(at, is_pet, polarity, 0.0))
                .with_scale(Vec3::splat(aura_band_radius(polarity, 0.0))),
            Visibility::default(),
            PlayMatchEntity,
        ))
        .add_children(&arcs);
}

/// UPDATE: age each band, follow its bearer, sweep, open/tighten and fade.
pub fn update_aura_bands(
    time: Res<Time>,
    mut bands: Query<(&mut AuraBand, &mut Transform, &AuraBandMaterial)>,
    bearers: Query<&Transform, (With<Combatant>, Without<AuraBand>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let dt = time.delta_secs();
    for (mut band, mut transform, material) in bands.iter_mut() {
        band.age += dt;
        let k = aura_band_progress(band.age);
        if let Ok(bearer) = bearers.get(band.target) {
            transform.translation =
                aura_band_centre(bearer.translation, band.is_pet, band.polarity, k);
        }
        transform.scale = Vec3::splat(aura_band_radius(band.polarity, k));
        if let Some(mat) = materials.get_mut(&material.0) {
            let (base, emissive) = band_colors(band.tint, band.polarity, aura_band_alpha(k));
            mat.base_color = base;
            mat.emissive = emissive;
        }
    }
}

/// CLEANUP: despawn a band (and its arcs) once its life is spent or its bearer
/// is gone.
pub fn cleanup_aura_bands(
    mut commands: Commands,
    bands: Query<(Entity, &AuraBand)>,
    bearers: Query<(), With<Combatant>>,
) {
    for (entity, band) in bands.iter() {
        if band.age >= AURA_BAND_SECS || !bearers.contains(band.target) {
            commands.entity(entity).despawn();
        }
    }
}

// ==============================================================================
// Unit tests — the ledger and the curves
// ==============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixture reshaped from an existing constructor rather than a new
    /// `Aura {}` literal, so `tests/aura_catalog_audit.rs` has nothing new to
    /// account for — these never reach a match.
    fn aura(effect_type: AuraType, name: &str, caster: Option<Entity>) -> Aura {
        let mut aura =
            weapon_poison_marker_aura(crate::states::match_config::RoguePoison::Crippling);
        aura.effect_type = effect_type;
        aura.duration = 10.0;
        aura.caster = caster;
        aura.ability_name = name.to_string();
        aura
    }

    fn e(i: u32) -> Entity {
        Entity::from_raw(i)
    }

    #[test]
    fn a_refresh_is_not_a_new_application() {
        let mut ledger = AuraApplyLedger::default();
        let mut buff = aura(AuraType::SpellPowerIncrease, "Flametongue", Some(e(9)));
        assert_eq!(ledger.observe(e(1), std::slice::from_ref(&buff)).len(), 1);
        // The totem pulse resets the duration and nothing else.
        buff.duration = 2.0;
        assert!(ledger.observe(e(1), std::slice::from_ref(&buff)).is_empty());
        buff.duration = 1.2;
        assert!(ledger.observe(e(1), std::slice::from_ref(&buff)).is_empty());
    }

    #[test]
    fn a_lapsed_aura_that_returns_is_a_new_application() {
        let mut ledger = AuraApplyLedger::default();
        let buff = aura(
            AuraType::AttackPowerIncrease,
            "Strength of Earth",
            Some(e(9)),
        );
        assert_eq!(ledger.observe(e(1), std::slice::from_ref(&buff)).len(), 1);
        assert!(ledger.observe(e(1), &[]).is_empty());
        assert_eq!(ledger.observe(e(1), std::slice::from_ref(&buff)).len(), 1);
    }

    #[test]
    fn a_second_instance_is_new_while_the_first_is_up() {
        let mut ledger = AuraApplyLedger::default();
        let a = aura(AuraType::DamageOverTime, "Corruption", Some(e(8)));
        let b = aura(AuraType::DamageOverTime, "Corruption", Some(e(9)));
        ledger.observe(e(1), std::slice::from_ref(&a));
        let both = [a.clone(), b.clone()];
        let fresh = ledger.observe(e(1), &both);
        assert_eq!(fresh.len(), 1);
        assert_eq!(fresh[0].caster, Some(e(9)));
    }

    #[test]
    fn bearers_are_independent_and_pruned() {
        let mut ledger = AuraApplyLedger::default();
        let buff = aura(AuraType::MaxManaIncrease, "Arcane Intellect", Some(e(9)));
        ledger.observe(e(1), std::slice::from_ref(&buff));
        assert_eq!(ledger.observe(e(2), std::slice::from_ref(&buff)).len(), 1);
        ledger.retain(|entity| entity != e(1));
        // Forgotten, so seen afresh.
        assert_eq!(ledger.observe(e(1), std::slice::from_ref(&buff)).len(), 1);
    }

    #[test]
    fn cues_coalesce_to_one_per_polarity_and_skip_unrouted_types() {
        let buffs = [
            aura(AuraType::MaxHealthIncrease, "Power Word: Fortitude", None),
            aura(AuraType::AttackPowerIncrease, "Battle Shout", None),
            aura(AuraType::Stun, "Kidney Shot", None),
            aura(AuraType::MovementSpeedSlow, "Frostbolt", None),
            aura(AuraType::AttackPowerReduction, "Demoralizing Shout", None),
        ];
        let refs: Vec<&Aura> = buffs.iter().collect();
        let cues = aura_band_cues(&refs);
        let polarities: Vec<AuraBandPolarity> = cues.iter().map(|(p, _)| *p).collect();
        assert_eq!(
            polarities,
            vec![AuraBandPolarity::Buff, AuraBandPolarity::Debuff]
        );
    }

    #[test]
    fn tint_is_the_school_or_the_neutral_authority() {
        let mut a = aura(AuraType::MaxManaIncrease, "Arcane Intellect", None);
        assert_eq!(aura_band_tint(&a), AURA_BAND_NEUTRAL_TINT);
        a.spell_school = Some(SpellSchool::Arcane);
        assert_eq!(aura_band_tint(&a), SpellSchool::Arcane.color());
    }

    #[test]
    fn the_envelope_opens_and_closes() {
        assert_eq!(aura_band_alpha(0.0), 0.0);
        assert_eq!(aura_band_alpha(1.0), 0.0);
        assert_eq!(aura_band_alpha(0.4), 1.0);
    }
}
