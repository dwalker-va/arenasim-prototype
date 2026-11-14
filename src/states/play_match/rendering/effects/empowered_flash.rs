use std::f32::consts::FRAC_PI_2;
use std::sync::Arc;

use bevy::color::LinearRgba;
use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;

use super::druid_heals::{body_point, keyed, keyed_color};
use super::hunter_shots::{draw, particle_rotation, ClientEmitter, EmitterAssets, EmitterSprite};
use super::shield::disc_texture;
use crate::states::play_match::components::*;

// ==============================================================================
// The empowered ground flash (Amplify Curse, Shadow Trance) and Shadow Trance's
// dark speck halo
// ==============================================================================
//
// From the Classic client data, joined in
// `docs/design/2026-10-17-warlock-talent-client-data.md` §1-2 (AS-334):
//
// - Amplify Curse (18288 -> visual 4600) casts `presenceofmind_cast_base.m2`
//   at the Warlock's feet (kit 4012), with no hands and no body animation. It
//   is the client's generic "empowered next cast" flash (Presence of Mind and
//   Fel Domination share it): three concentric additive discs on the ground,
//   half-widths 0.63 / 0.51 / 0.41 yd, blooming 0.613 -> 3.243x at 467 ms (the
//   outer one about 4.1 yd across), teal (38,97,91) pushing to blue (0,97,255),
//   alpha 0 -> 0.6 at 133 ms -> 0 at 467 ms. The client also lists a (7,8)
//   state with the same model; the ruling (2026-10-17) is ONCE, on cast, with
//   no repeat while the buff waits — its icon carries it.
// - Shadow Trance (Nightfall's proc, 17941 -> visual 5219) plays kit 4569 once
//   on the Warlock: the same flash, and `shadow_impactdot_med_head.m2` at the
//   head — a sparse halo of tiny near-black specks (2,2,2) drifting in at
//   0.28 yd/s, 12 a second, each living 4 s, alpha-blended. Ruled (2026-10-17)
//   at client size. The instant Shadow Bolt it grants is the shipped bolt.
//
// Played as the signed-off bench plays them (AS-334 Warlock Talent Visuals
// Bench, https://claude.ai/artifact/KwBsy13gPub9sKmnKdYMgg, version
// 1792203924-603a, the "Amplify Curse" and "Shadow Trance" tabs, at their
// defaults; source committed as
// `docs/design/benches/2026-10-17-warlock-talent-bench.html`).
//
// **Lifetimes.** The flash is a landing: it plays out its half second on a
// living Warlock even once the curse or the bolt has spent the buff (Amplify
// Curse is usually spent 20 ms after it lands). The halo shows the proc
// WAITING: it emits for the client's window from the proc and retires, specks
// and all, the moment Shadow Trance is spent or its Warlock dies
// ([`SHADOW_TRANCE_HALO_ENDS_WITH_PROC`]). Neither is drawn on a corpse.
//
// Spawning rides the sim's `AuraLanding` marker (`apply_pending_auras`, on a
// fresh application or a refresh). Scatter is the shared deterministic hash
// (never `game_rng`), nothing here writes sim state, and every system is
// registered in `states/mod.rs` only — headless stays byte-identical.

// ── Bench sign-off constants (AS-334, version 1792203924-603a) ──────────────

// Amplify Curse: presenceofmind_cast_base.m2 (kits 4012 / 4013) at the Warlock's feet. AS-334 bench.
pub const AMPLIFY_FLASH_SECS: f32 = 1.50; // visible 0-0.467 s, peak alpha 0.6 at 0.133 s
pub const AMPLIFY_FLASH_MAX_DIAMETER: f32 = 4.10;
pub const AMPLIFY_OPACITY_MUL: f32 = 1.00;
// Ruled: once on cast; no repeat while buffed (the bench's AMPLIFY_REPEAT_WHILE_BUFFED = false).
// Shared with Shadow Trance's proc flash (one helper).

// Shadow Trance proc: kit 4569, one-shot on the Warlock. AS-334 bench.
pub const SHADOW_TRANCE_FLASH: bool = true; // the Amplify Curse helper
pub const SHADOW_TRANCE_SPECKS: bool = true;
pub const SHADOW_TRANCE_SPECK_SIZE: f32 = 0.028;
pub const SHADOW_TRANCE_SPECK_LIFE_SECS: f32 = 4.00;
// The instant Shadow Bolt: unchanged (shipped missile and impact).

/// Whether the halo retires the moment Shadow Trance is spent. `true` ties it
/// to the waiting proc; `false` plays it as the client's one-shot impact
/// (kit 4569 has no state), its whole window whatever the buff does. Either
/// way it retires with a dead Warlock.
pub const SHADOW_TRANCE_HALO_ENDS_WITH_PROC: bool = true;

// ── The flash (presenceofmind_cast_base.m2) ──────────────────────────────────

/// The model's one Stand sequence, seconds. [`AMPLIFY_FLASH_SECS`] over it is
/// the bench's duration knob.
const FLASH_MODEL_SECS: f32 = 1.5;
/// The outer disc's diameter at the bloom's peak, at client size: the bench's
/// `4.1 × size × spread`.
const FLASH_CLIENT_MAX_DIAMETER: f32 = 4.1;
/// The three concentric discs' half-widths, yards, before the bloom scale.
pub const FLASH_DISC_HALF_WIDTHS: [f32; 3] = [0.63, 0.51, 0.41];
/// The bone scale every disc rides, keyed in milliseconds (Era).
pub const FLASH_SCALE_KEYS: [(f32, f32); 3] = [(0.0, 0.613), (467.0, 3.243), (1500.0, 0.613)];
/// The texture weight, keyed in milliseconds: brightest at 133 ms, gone at
/// 467 ms.
pub const FLASH_ALPHA_KEYS: [(f32, f32); 4] =
    [(0.0, 0.0), (133.0, 0.6), (467.0, 0.0), (1500.0, 0.0)];
/// The colour track: teal at the cast, pushed to blue by 467 ms (the client's
/// `(-0.32, 0.38, 1.355)` clamped).
pub const FLASH_COLOR_KEYS: [(f32, [u8; 3]); 2] = [(0.0, [38, 97, 91]), (467.0, [0, 97, 255])];
/// From here on the flash's alpha is zero for good: the rig retires.
pub const FLASH_GONE_MS: f32 = 467.0;
/// The bench draws each disc at 0.8 of the flash's alpha (`groundDisc(..., a * 0.8)`).
pub const FLASH_DISC_ALPHA: f32 = 0.8;
/// The discs' soft radial falloff, over the disc's radius: the bench's
/// gradient stops.
pub const FLASH_DISC_FALLOFF: [(f32, f32); 3] = [(0.0, 1.0), (0.55, 0.55), (1.0, 0.0)];
/// Height of the discs above the floor (the model's quads sit at z = 0.03).
pub const FLASH_ABOVE_FLOOR: f32 = 0.03;
/// Emissive gain of the additive discs over their colour — the house's 2-4x
/// glow convention (`hunter_shots.rs` emitters use the same).
pub const FLASH_GLOW: f32 = 2.5;
/// The disc texture's side, pixels.
const FLASH_DISC_PX: u32 = 128;

/// The flash's size multiplier over the client: the bench's size × spread.
pub fn flash_size_mul() -> f32 {
    AMPLIFY_FLASH_MAX_DIAMETER / FLASH_CLIENT_MAX_DIAMETER
}

/// The bench's duration knob: how far the flash is stretched over the
/// model's 1.5 s.
pub fn flash_time_scale() -> f32 {
    AMPLIFY_FLASH_SECS / FLASH_MODEL_SECS
}

/// How long a flash lives before it retires, seconds: until its alpha is zero
/// for good.
pub fn flash_life() -> f32 {
    FLASH_GONE_MS / 1000.0 * flash_time_scale()
}

/// One frame of the flash: the scale every disc rides, the alpha each disc is
/// drawn at, and the colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlashFrame {
    pub scale: f32,
    pub disc_alpha: f32,
    pub color: Color,
}

/// The flash `age` seconds after it began, or `None` once it has gone.
pub fn flash_frame(age: f32) -> Option<FlashFrame> {
    let ms = age * 1000.0 / flash_time_scale();
    if !(0.0..FLASH_GONE_MS).contains(&ms) {
        return None;
    }
    Some(FlashFrame {
        scale: keyed(&FLASH_SCALE_KEYS, ms),
        disc_alpha: keyed(&FLASH_ALPHA_KEYS, ms) * AMPLIFY_OPACITY_MUL * FLASH_DISC_ALPHA,
        color: keyed_color(&FLASH_COLOR_KEYS, ms),
    })
}

/// The soft round disc the flash draws on each quad.
pub fn flash_disc_texture() -> Image {
    disc_texture(FLASH_DISC_PX, |r, _| keyed(&FLASH_DISC_FALLOFF, r))
}

/// The flash's own material at one frame.
fn flash_material(texture: &Handle<Image>, frame: FlashFrame) -> StandardMaterial {
    let c = frame.color.to_linear();
    StandardMaterial {
        base_color: frame.color.with_alpha(frame.disc_alpha),
        base_color_texture: Some(texture.clone()),
        // The texture drives BOTH channels, and `unlit` stays false or Bevy
        // discards the emissive (see `spell_bolts.rs`).
        emissive: LinearRgba::rgb(
            c.red * FLASH_GLOW,
            c.green * FLASH_GLOW,
            c.blue * FLASH_GLOW,
        ),
        emissive_texture: Some(texture.clone()),
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        double_sided: true,
        ..default()
    }
}

/// Write `frame`'s colour and alpha onto a flash's material.
fn paint_flash(material: &mut StandardMaterial, frame: FlashFrame) {
    let c = frame.color.to_linear();
    material.base_color = frame.color.with_alpha(frame.disc_alpha);
    material.emissive = LinearRgba::rgb(
        c.red * FLASH_GLOW,
        c.green * FLASH_GLOW,
        c.blue * FLASH_GLOW,
    );
}

// ── The speck halo (shadow_impactdot_med_head.m2) ────────────────────────────

/// The bench's head anchor, above the floor (`ANCH.head`).
pub const TRANCE_HEAD_ABOVE_FLOOR: f32 = 2.25;
/// The flat shell the specks are born on: its radius (each speck 0.6-1.0 of
/// it out — the model's 1.39 yd plane) and how far it is squashed vertically.
pub const SPECK_SHELL_RADIUS: f32 = 0.7;
pub const SPECK_SHELL_FLAT: f32 = 0.15;

/// The model's two emitters (P0, P1: `flare`, blend 2, 6/s each) as the bench
/// merges them: 12 specks a second for 4.5 s from the proc, each living 4 s,
/// drifting in at 0.28 yd/s and sinking under the model's 0.139 gravity,
/// near-black, fading in to full and out again. The bench draws them as hard
/// `blob` discs whose size key is a RADIUS.
pub const TRANCE_SPECKS: ClientEmitter = ClientEmitter {
    name: "shadow_impactdot_med_head/p0+p1 dark specks",
    sprite: EmitterSprite::Blob,
    additive: false,
    rate: 12.0,
    window: Some((0.0, 4.5)),
    life: SHADOW_TRANCE_SPECK_LIFE_SECS,
    // Negative: inward. The halo carries its own shell launch.
    speed: -0.28,
    gravity: 0.139,
    area: 0.0,
    column: None,
    rise: false,
    color: [[2, 2, 2]; 3],
    alpha: [0.0, 1.0, 0.0],
    size: ParticleSize::radii([SHADOW_TRANCE_SPECK_SIZE; 3]),
    mid: 0.5,
    track: None,
};

/// How long after its (latest) proc a halo has nothing left to draw: its
/// window plus its specks' life.
pub fn halo_life() -> f32 {
    TRANCE_SPECKS.end() + TRANCE_SPECKS.life
}

/// Where a speck is born, relative to the head, and how it moves: on the
/// bench's flat shell (a cube direction squashed vertically, normalised, at
/// 0.6-1.0 of the radius), travelling straight in.
pub fn speck_launch(seed: u32) -> (Vec3, Vec3) {
    let d = Vec3::new(
        draw(seed, 1) * 2.0 - 1.0,
        (draw(seed, 2) * 2.0 - 1.0) * SPECK_SHELL_FLAT,
        draw(seed, 3) * 2.0 - 1.0,
    )
    .try_normalize()
    .unwrap_or(Vec3::X);
    let r = SPECK_SHELL_RADIUS * (0.6 + 0.4 * draw(seed, 4));
    (d * r, -d * TRANCE_SPECKS.speed.abs())
}

/// Whether Shadow Trance is up on a unit with these auras: the aura whose
/// landing is the Shadow Trance treatment.
pub fn shadow_trance_waiting(auras: Option<&ActiveAuras>) -> bool {
    auras.is_some_and(|a| {
        a.auras.iter().any(|aura| {
            AuraLandingKind::for_aura(aura.effect_type, &aura.ability_name)
                == Some(AuraLandingKind::ShadowTrance)
        })
    })
}

// ── Assets ───────────────────────────────────────────────────────────────────

/// The flash's disc quad and texture, built once, lazily. (The specks' sprite
/// and palette are the shared `EmitterAssets`, built by the halo system.)
pub struct EmpoweredFlashAssets {
    disc_quad: Handle<Mesh>,
    disc_texture: Handle<Image>,
}

impl EmpoweredFlashAssets {
    fn build(meshes: &mut Assets<Mesh>, images: &mut Assets<Image>) -> Self {
        Self {
            disc_quad: meshes.add(Rectangle::new(1.0, 1.0)),
            disc_texture: images.add(flash_disc_texture()),
        }
    }
}

// ── Systems ──────────────────────────────────────────────────────────────────

/// Spawn (graphical-only): a flash for every Amplify Curse or Shadow Trance
/// landing the sim marked on a living unit, and Shadow Trance's halo — one per
/// Warlock, so a proc that refreshes Shadow Trance restarts the halo it has.
/// Only these landings' markers are consumed.
#[allow(clippy::too_many_arguments)]
pub fn spawn_empowered_flashes(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<EmpoweredFlashAssets>>,
    landings: Query<(Entity, &AuraLanding)>,
    holders: Query<(&Combatant, &Transform, Option<&Pet>)>,
    mut halos: Query<&mut ShadowTranceHalo>,
) {
    let mut haloed: Vec<Entity> = Vec::new();
    for (marker, landing) in landings.iter() {
        let (flash, halo) = match landing.kind {
            AuraLandingKind::AmplifyFlash => (true, false),
            AuraLandingKind::ShadowTrance => (SHADOW_TRANCE_FLASH, SHADOW_TRANCE_SPECKS),
            AuraLandingKind::RejuvenationSwirl
            | AuraLandingKind::MarkOfTheWildGlyph
            | AuraLandingKind::ShadowWordPainHit
            | AuraLandingKind::DivineFavorBurst
            | AuraLandingKind::AuraMasteryWard
            | AuraLandingKind::SacrificeShell
            | AuraLandingKind::RetributionDisc
            | AuraLandingKind::InfusionHalo
            | AuraLandingKind::ArtOfWarRing => continue,
        };
        commands.entity(marker).despawn();
        let Ok((holder, transform, pet)) = holders.get(landing.target) else {
            continue;
        };
        if !holder.is_alive() {
            continue;
        }
        let is_pet = pet.is_some();
        let assets =
            assets.get_or_insert_with(|| EmpoweredFlashAssets::build(&mut meshes, &mut images));

        if flash {
            let first = flash_frame(0.0).expect("a flash is drawn from its first instant");
            let material = materials.add(flash_material(&assets.disc_texture, first));
            let discs: Vec<Entity> = FLASH_DISC_HALF_WIDTHS
                .iter()
                .map(|&half_width| {
                    commands
                        .spawn((
                            EmpoweredFlashDisc { half_width },
                            Mesh3d(assets.disc_quad.clone()),
                            MeshMaterial3d(material.clone()),
                            // `Rectangle` faces +Z; tipped back a quarter turn
                            // it lies flat, facing up. Its scale is the disc's
                            // diameter before the bloom (the rig carries it).
                            Transform::from_rotation(Quat::from_rotation_x(-FRAC_PI_2))
                                .with_scale(Vec3::splat(half_width * 2.0)),
                            NotShadowCaster,
                        ))
                        .id()
                })
                .collect();
            commands
                .spawn((
                    EmpoweredFlash {
                        caster: landing.target,
                        age: 0.0,
                        material,
                    },
                    Transform::from_translation(body_point(
                        FLASH_ABOVE_FLOOR,
                        transform.translation,
                        is_pet,
                    ))
                    .with_scale(Vec3::splat(first.scale * flash_size_mul())),
                    Visibility::default(),
                    PlayMatchEntity,
                ))
                .add_children(&discs);
        }

        if halo {
            if let Some(mut existing) = halos.iter_mut().find(|h| h.owner == landing.target) {
                existing.age = 0.0;
                continue;
            }
            if haloed.contains(&landing.target) {
                continue;
            }
            haloed.push(landing.target);
            commands.spawn((
                ShadowTranceHalo {
                    owner: landing.target,
                    age: 0.0,
                    carry: 0.0,
                    emitted: 0,
                },
                Transform::from_translation(body_point(
                    TRANCE_HEAD_ABOVE_FLOOR,
                    transform.translation,
                    is_pet,
                )),
                Visibility::default(),
                PlayMatchEntity,
            ));
        }
    }
}

/// Animate (graphical-only): bloom and fade each flash at its Warlock's feet,
/// and retire it once it has gone, or with its Warlock.
pub fn animate_empowered_flashes(
    mut commands: Commands,
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut flashes: Query<(Entity, &mut EmpoweredFlash, &mut Transform)>,
    holders: Query<(&Combatant, &Transform, Option<&Pet>), Without<EmpoweredFlash>>,
) {
    let dt = time.delta_secs();
    for (entity, mut flash, mut transform) in flashes.iter_mut() {
        let Ok((holder, at, pet)) = holders.get(flash.caster) else {
            commands.entity(entity).despawn();
            continue;
        };
        flash.age += dt;
        let frame = match flash_frame(flash.age) {
            Some(frame) if holder.is_alive() => frame,
            _ => {
                commands.entity(entity).despawn();
                continue;
            }
        };
        transform.translation = body_point(FLASH_ABOVE_FLOOR, at.translation, pet.is_some());
        transform.scale = Vec3::splat((frame.scale * flash_size_mul()).max(1e-4));
        if let Some(material) = materials.get_mut(&flash.material) {
            paint_flash(material, frame);
        }
    }
}

/// Animate (graphical-only): follow each halo's Warlock at the head, retire
/// its spent specks and emit what it owes this frame. A halo whose Shadow
/// Trance is spent, whose Warlock has died, or whose last speck has died goes
/// at once, specks and all.
#[allow(clippy::too_many_arguments)]
pub fn update_shadow_trance_halos(
    mut commands: Commands,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut assets: Local<Option<EmitterAssets>>,
    mut halos: Query<(
        Entity,
        &mut ShadowTranceHalo,
        &mut Transform,
        Option<&Children>,
    )>,
    holders: Query<
        (&Combatant, &Transform, Option<&ActiveAuras>, Option<&Pet>),
        Without<ShadowTranceHalo>,
    >,
    particles: Query<&ClientParticle>,
) {
    let dt = time.delta_secs();
    for (entity, mut halo, mut transform, children) in halos.iter_mut() {
        let waiting = holders
            .get(halo.owner)
            .ok()
            .filter(|(holder, _, auras, _)| {
                holder.is_alive()
                    && (!SHADOW_TRANCE_HALO_ENDS_WITH_PROC || shadow_trance_waiting(*auras))
            });
        let Some((_, at, _, pet)) = waiting else {
            commands.entity(entity).despawn();
            continue;
        };
        let t0 = halo.age;
        halo.age += dt;
        if halo.age >= halo_life() {
            commands.entity(entity).despawn();
            continue;
        }
        transform.translation = body_point(TRANCE_HEAD_ABOVE_FLOOR, at.translation, pet.is_some());
        for child in children.into_iter().flat_map(|c| c.iter()) {
            if particles.get(child).is_ok_and(|p| p.age >= p.life) {
                commands.entity(child).despawn();
            }
        }

        halo.carry += TRANCE_SPECKS.count_between(t0, halo.age);
        if halo.carry < 1.0 {
            continue;
        }
        let assets = assets.get_or_insert_with(|| EmitterAssets::build(&mut meshes, &mut images));
        let quad = assets.quad();
        let palette = assets.palette(&mut materials, &TRANCE_SPECKS);
        while halo.carry >= 1.0 {
            halo.carry -= 1.0;
            let seed = halo
                .owner
                .index()
                .wrapping_mul(0x9E37_79B9)
                .wrapping_add(halo.emitted.wrapping_mul(0x85EB_CA6B));
            halo.emitted = halo.emitted.wrapping_add(1);
            let speck = spawn_speck(&mut commands, &quad, &palette, seed);
            commands.entity(entity).add_child(speck);
        }
    }
}

/// One speck, a child of its halo (so it rides the Warlock's head), turned to
/// the camera by [`billboard_shadow_trance_specks`].
fn spawn_speck(
    commands: &mut Commands,
    quad: &Handle<Mesh>,
    palette: &Arc<[Handle<StandardMaterial>]>,
    seed: u32,
) -> Entity {
    let e = &TRANCE_SPECKS;
    let (offset, velocity) = speck_launch(seed);
    commands
        .spawn((
            ClientParticle {
                age: 0.0,
                life: e.life,
                velocity,
                gravity: e.gravity,
                size: e.size,
                mid: e.mid,
                palette: palette.clone(),
                step: 0,
                facing: ParticleFacing::Camera,
                owned: true,
            },
            Mesh3d(quad.clone()),
            MeshMaterial3d(palette[0].clone()),
            Transform::from_translation(offset),
            NotShadowCaster,
        ))
        .id()
}

/// Turn every speck to face the camera.
pub fn billboard_shadow_trance_specks(
    camera: Query<
        &Transform,
        (
            With<Camera3d>,
            Without<ShadowTranceHalo>,
            Without<ClientParticle>,
        ),
    >,
    halos: Query<(&Transform, &Children), With<ShadowTranceHalo>>,
    mut particles: Query<
        (&ClientParticle, &mut Transform),
        (Without<Camera3d>, Without<ShadowTranceHalo>),
    >,
) {
    let Some(cam) = camera.iter().next() else {
        return;
    };
    for (halo, children) in halos.iter() {
        for child in children.iter() {
            if let Ok((particle, mut part)) = particles.get_mut(child) {
                part.rotation = particle_rotation(particle, halo.rotation, cam.rotation);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bench's numbers, as its `pomFlash` and its constants block state
    /// them.
    #[test]
    fn the_flash_is_the_benchs_pom_flash() {
        assert_eq!(flash_size_mul(), 1.0);
        assert_eq!(flash_time_scale(), 1.0);
        // The outer disc at the peak: 2 × 0.63 × 3.243 = 4.086, which the
        // bench rounds to its 4.1.
        let peak = 2.0 * FLASH_DISC_HALF_WIDTHS[0] * flash_frame(0.467 - 1e-4).unwrap().scale;
        assert!((peak - AMPLIFY_FLASH_MAX_DIAMETER).abs() < 0.05, "{peak}");
        // Brightest at 133 ms: 0.6 × 0.8 per disc.
        let bright = flash_frame(0.133).unwrap();
        assert!((bright.disc_alpha - 0.48).abs() < 1e-5);
        assert!(flash_frame(0.0).unwrap().disc_alpha.abs() < 1e-6);
        assert!(flash_frame(FLASH_GONE_MS / 1000.0).is_none());
        assert!((flash_life() - 0.467).abs() < 1e-6);
    }

    #[test]
    fn the_halo_lives_its_window_and_its_last_specks_life() {
        assert!((halo_life() - 8.5).abs() < 1e-6);
        // 12 a second for 4.5 s.
        assert!((TRANCE_SPECKS.count_between(0.0, 10.0) - 54.0).abs() < 1e-4);
    }

    #[test]
    fn a_speck_is_born_on_the_flat_shell_and_travels_straight_in() {
        for seed in 0..200 {
            let (offset, velocity) = speck_launch(seed);
            let r = offset.length();
            assert!((0.42 - 1e-4..=0.7 + 1e-4).contains(&r), "{r}");
            assert!((velocity.length() - 0.28).abs() < 1e-5);
            assert!(velocity.normalize().dot(-offset.normalize()) > 0.9999);
        }
        // Squashed: the shell is a flat ring about the head, not a sphere.
        let mean_rise = (0..1000)
            .map(|seed| speck_launch(seed).0.normalize().y.abs())
            .sum::<f32>()
            / 1000.0;
        assert!(mean_rise < 0.15, "{mean_rise}");
    }
}
