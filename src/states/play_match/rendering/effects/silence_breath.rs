use std::collections::HashSet;
use std::f32::consts::TAU;

use bevy::color::LinearRgba;
use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;

use super::body_hold::{held_in, BodyHold, HoldDisjoint};
use super::conflagrate::{conflagrate_material, ConflagrateSprite, CONFLAGRATE_GLOW};
use super::druid_heals::{body_point, keyed, keyed_color};
use super::hunter_shots::draw;
use super::school_impact::IMPACT_PET_STATURE;
use super::shield::disc_texture;
use crate::states::play_match::components::*;

// ==============================================================================
// Silence — the breath at the victim's mouth while the silence holds
// ==============================================================================
//
// From the client data, `docs/design/2026-10-17-priest-talent-client-data.md`
// §8 (AS-335): Silence (15487) plays visual 179, shared by eleven silences.
//
// - **Cast** (kit 418): `shadow_precast_low_hand.m2` on both hands. Silence is
//   an instant the Priest AI applies inline, with no cast state and no
//   instant marker, and no Shadow instant draws its hands today (Shadow Word:
//   Pain is the precedent), so nothing is drawn on the Priest.
// - **No landing** (`(6,13)` row).
// - **State for the silence** (kit 297): `shadowwordsilence_breath.m2` at
//   attachment 17, Breath — the mouth. Its Stand sequence (1167 ms) is the
//   inhale: violet, magenta and green motes sucked into the mouth for 167 ms,
//   then smoke puffs pulled in and sparkles to 867 ms. Its 158 sequence
//   (4167 ms) is the loop: a small exhale of grey-magenta smoke 233-800 ms in
//   and sparkles 367-1167 ms in, then nothing until it repeats. TBC does not
//   re-enable the inhale, so it plays once. A 5 s silence shows one exhale.
//
// Played as the signed-off AS-335 Priest Talent Visuals Bench plays it
// (https://claude.ai/artifact/PaMJeCzqUTVThWXZcNsyoc, version
// 1792207490-53c9, "Silence" tab, ruled client-faithful with all four layers
// on; its source as committed with the sign-off is
// `docs/design/benches/2026-10-17-priest-talent-bench.html`). Its constants
// block, at the ruling, is the block below. The bench draws the model's
// thirteen emitters as four layers, all following the victim, all at the
// mouth (the head anchor, 0.15 yd down and 0.3 yd forward):
//
//   inhale   three `gradient64flipb` emitters as one: soft glows, 240/s for
//            0-167 ms, born on a 0.75 yd shell and pulled in at 3.3 yd/s,
//            violet -> magenta -> green. No `blend`: `Emitter.draw` draws it
//            `lighter` — ADDITIVE.
//   insmoke  `toonsmoke16` x 2: smoke, 100/s for 233-867 ms, born on a
//            0.6 yd shell and pulled in at 0.83 yd/s, 0.56 -> 0.03 yd,
//            grey -> magenta. `blend: 'alpha'`: source-over — BLENDED.
//   exhale   `toonsmoke16` x 2: smoke, 100/s for 1400-1967 ms (the loop's
//            233-800 ms), breathed out in a 0.8 rad cone at 0.28 yd/s,
//            0.03 -> 0.08 -> 0.28 yd, grey -> magenta -> pale blue. BLENDED.
//   sparkle  `star5a` x 2: stars, 60/s for 233-867 and 1534-2334 ms (the
//            loop's 367-1167 ms), out in every direction at 0.4 yd/s,
//            spinning 1.1 turns a second, green -> magenta -> dark blue.
//            ADDITIVE.
//
// The bench plays 5 s and stops, so its windows are the first pass of the
// loop; the loop's period, 4.167 s, is the client's 158 sequence, and shows
// only on a silence that outlasts 5.334 s (a refresh).
//
// Where the card's prose and the bench disagree, the bench was signed off and
// wins: the dark `flare` specks the research lists in both the inhale (100/s,
// 233-867 ms) and the loop (733-967 ms) are not on the bench, so none are
// drawn; the inhale's three emitters are one layer with one colour track.
//
// ## Departures from the bench (each deliberate)
//
// 1. **Held in front of the body.** The mouth sits at the front of the head,
//    0.46 yd from the centre of the capsule's top sphere — inside its 0.5 yd
//    radius — so every piece of the breath starts inside the body. The bench
//    paints it all over the body; here the body is an `AlphaMode::Blend`
//    mesh that writes no depth, and the transparent pass sorts by each mesh's
//    translation, so the body would paint over everything deeper than its
//    centre. Each particle is drawn on its own view ray, slid in front of the
//    body and scaled by the same ratio — the shared body hold
//    (`body_hold.rs`) — so it projects where the bench puts it, whichever way
//    the victim faces.
// 2. **A star's white-hot centre is tinted**, as Shadow Word: Pain's,
//    Siphon Life's and Conflagrate's are: the sprite is one shape in the
//    layer's colour.
// 3. **Follows in height too.** The bench's `follow` moves the breath with
//    the victim in x and z only; here the particles ride the rig at the
//    mouth.
// 4. **The mouth turns with the victim.** The bench's victim does not turn;
//    here the mouth is 0.3 yd along the victim's facing.
//
// ## Lifecycle
//
// The breath shows the silence HOLDING: it is raised while a living unit
// holds an aura `AuraApplyRoute::for_instance` routes to
// `AuraApplyOwner::SilenceBreath` — Priest Silence by its name, never the
// Unstable Affliction backlash's silence, which keeps its burst — and retired,
// particles and all, the frame it no longer does: it expired, it was
// dispelled, or its victim died (death keeps the aura on the corpse, so it is
// checked apart) or is gone. A refresh keeps the breath and its clock: the
// inhale does not replay, and the loop runs on. A polymorphed victim's breath
// is hidden while it is a sheep (its clock runs on), Siphon Life's
// precedent: the sheep wears nothing. A stealthed victim keeps it, as Shadow
// Dance's state does: stealth only fades the body. Everything is a
// `PlayMatchEntity` rig with its particles as children, so a match that ends
// mid-silence takes all of it.
//
// ## At the match camera (60 yd, pitch 38.7°, FOV 45°)
//
// About 22 px a yard at 1080p: the inhale's motes are 1-2 px, the inhaled
// smoke ~24 px shrinking to a point at the mouth, the exhale ~1 px growing to
// ~12 px, a sparkle ~2 px. Small and mostly idle, as the client draws it
// ("small... a puff every 4.2 s"); client size, by the ruling.
//
// **Rides the rendered body.** The mouth is measured from the victim's
// `VisualBody`, not its root, and so is the body the breath is held in front
// of (`rendered_root`): the walk bob, the fear bob (a silenced caster is
// often feared too) and the swing step carry it, as `shield.rs`,
// `vengeance.rs` and `shadow_dance.rs` ride the body. So the placement runs
// in PostUpdate, after every writer of a body's transform and before
// propagation (`place_silence_breaths`).
//
// House rules: spawn / animate / age / place / cleanup systems,
// `Res<Time>`, registered in `states/mod.rs` only (headless never runs any of
// it), no `game_rng` — all scatter is hash-seeded off the rig's entity index
// and emission count.

// ── Bench sign-off constants (AS-335, version 1792207490-53c9) ───────────────

// Silence: shadowwordsilence_breath.m2 (kit 297) at the victim's mouth for the silence. AS-335 bench.
pub const SILENCE_INHALE: bool = true;
pub const SILENCE_EXHALE: bool = true; // every 4.167 s
pub const SILENCE_SPARKLES: bool = true;
// Route: Priest Silence gets this state, not UA's BacklashBurst.

// ── The anchor ───────────────────────────────────────────────────────────────

/// The bench's head anchor above the floor (`ANCH.head`).
pub const SILENCE_HEAD_ABOVE_FLOOR: f32 = 2.25;
/// The mouth, from the head anchor: this far down and this far forward along
/// the victim's facing (the bench's `pos: [0, 0.3, -0.15]`).
pub const SILENCE_MOUTH_DROP: f32 = 0.15;
pub const SILENCE_MOUTH_FORWARD: f32 = 0.3;

// ── The sequences ────────────────────────────────────────────────────────────

/// The Stand sequence, the inhale, seconds; the 158 loop starts as it ends.
pub const SILENCE_STAND_SECS: f32 = 1.167;
/// The 158 loop's period, seconds.
pub const SILENCE_LOOP_SECS: f32 = 4.167;

/// When an emitter emits, in milliseconds.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum BreathWindow {
    /// `[start, end)` after the onset, once (the Stand sequence).
    Onset(f32, f32),
    /// `[start, end)` into every pass of the 158 loop.
    Loop(f32, f32),
}

impl BreathWindow {
    /// Whether the window is open `ms` after the onset.
    pub fn open_at(self, ms: f32) -> bool {
        match self {
            BreathWindow::Onset(a, b) => ms >= a && ms < b,
            BreathWindow::Loop(a, b) => {
                let into = ms - SILENCE_STAND_SECS * 1000.0;
                if into < 0.0 {
                    return false;
                }
                let at = into.rem_euclid(SILENCE_LOOP_SECS * 1000.0);
                at >= a && at < b
            }
        }
    }
}

/// How a particle launches.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum BreathLaunch {
    /// Born on a shell about the mouth — `0.6..1.0` of `shell` out, along a
    /// normalised cube sample — and travelling straight in (the bench's
    /// `inward`).
    Inward { shell: f32 },
    /// Out of the mouth at a polar angle up to `half_angle` off straight up,
    /// at any bearing (the bench's `vRange`).
    Cone { half_angle: f32 },
    /// Out of the mouth along a normalised cube sample (`vRange >= 3.1`).
    Sphere,
}

/// One emitter, as the bench's layer record carries it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct BreathEmitter {
    /// Unique key.
    pub name: &'static str,
    /// Whether the layer is kept (the constants block above).
    pub on: bool,
    pub sprite: ConflagrateSprite,
    /// `lighter` (additive) when `true`; `source-over` (blended) when `false`.
    pub additive: bool,
    pub launch: BreathLaunch,
    /// Launch speed, yd/s (its magnitude, for an inward launch).
    pub speed: f32,
    pub life: f32,
    /// Particles per second while a window is open.
    pub rate: f32,
    pub windows: &'static [BreathWindow],
    /// Colour, alpha and scale tracks over the particle's normalised life.
    pub color: &'static [(f32, [u8; 3])],
    pub alpha: &'static [(f32, f32)],
    /// Sprite RADIUS, yards.
    pub scale: &'static [(f32, f32)],
    /// Sprite spin, turns a second.
    pub spin: f32,
}

impl BreathEmitter {
    /// Whether the emitter emits `ms` after the onset (the bench's `windowOn`).
    pub fn emitting_at(&self, ms: f32) -> bool {
        self.on && self.windows.iter().any(|w| w.open_at(ms))
    }
}

/// `shadowwordsilence_breath.m2`'s emitters as the bench's four layers, in its
/// order.
pub const SILENCE_EMITTERS: [BreathEmitter; 4] = [
    BreathEmitter {
        name: "shadowwordsilence_breath/inhale motes",
        on: SILENCE_INHALE,
        sprite: ConflagrateSprite::Glow,
        additive: true,
        launch: BreathLaunch::Inward { shell: 0.75 },
        speed: 3.3,
        life: 0.25,
        rate: 240.0,
        windows: &[BreathWindow::Onset(0.0, 167.0)],
        color: &[
            (0.0, [150, 60, 255]),
            (0.5, [255, 40, 200]),
            (1.0, [60, 220, 120]),
        ],
        alpha: &[(0.0, 0.0), (0.5, 1.0), (1.0, 0.0)],
        scale: &[(0.0, 0.02), (1.0, 0.035)],
        spin: 0.0,
    },
    BreathEmitter {
        name: "shadowwordsilence_breath/inhaled smoke",
        on: SILENCE_INHALE,
        sprite: ConflagrateSprite::Smoke,
        additive: false,
        launch: BreathLaunch::Inward { shell: 0.6 },
        speed: 0.83,
        life: 0.6,
        rate: 100.0,
        windows: &[BreathWindow::Onset(233.0, 867.0)],
        color: &[(0.0, [120, 120, 120]), (1.0, [163, 0, 175])],
        alpha: &[(0.0, 0.7), (1.0, 0.0)],
        scale: &[(0.0, 0.56), (1.0, 0.03)],
        spin: 0.0,
    },
    BreathEmitter {
        name: "shadowwordsilence_breath/exhaled smoke",
        on: SILENCE_EXHALE,
        sprite: ConflagrateSprite::Smoke,
        additive: false,
        launch: BreathLaunch::Cone { half_angle: 0.8 },
        speed: 0.28,
        life: 1.5,
        rate: 100.0,
        windows: &[BreathWindow::Loop(233.0, 800.0)],
        color: &[
            (0.0, [130, 130, 130]),
            (0.5, [163, 0, 175]),
            (1.0, [160, 190, 240]),
        ],
        alpha: &[(0.0, 0.8), (1.0, 0.0)],
        scale: &[(0.0, 0.03), (0.5, 0.08), (1.0, 0.28)],
        spin: 0.0,
    },
    BreathEmitter {
        name: "shadowwordsilence_breath/sparkles",
        on: SILENCE_SPARKLES,
        sprite: ConflagrateSprite::Star,
        additive: true,
        launch: BreathLaunch::Sphere,
        speed: 0.4,
        life: 0.8,
        rate: 60.0,
        windows: &[
            BreathWindow::Onset(233.0, 867.0),
            BreathWindow::Loop(367.0, 1167.0),
        ],
        color: &[
            (0.0, [60, 220, 120]),
            (0.5, [255, 40, 200]),
            (1.0, [30, 40, 160]),
        ],
        alpha: &[(0.0, 1.0), (1.0, 0.0)],
        scale: &[(0.0, 0.05), (1.0, 0.03)],
        spin: 1.1,
    },
];

// ── The model ────────────────────────────────────────────────────────────────

const SPRITE_PX: u32 = 64;

fn stature(is_pet: bool) -> f32 {
    if is_pet {
        IMPACT_PET_STATURE
    } else {
        1.0
    }
}

/// The mouth of a victim whose rendered body stands as a unit at `root`
/// would at rest (`rendered_root`), facing `facing`: the head anchor, dropped
/// and brought forward along its facing (local +Z, the way the sim turns every
/// unit).
pub fn silence_mouth(root: Vec3, facing: Quat, is_pet: bool) -> Vec3 {
    body_point(SILENCE_HEAD_ABOVE_FLOOR - SILENCE_MOUTH_DROP, root, is_pet)
        + facing * Vec3::Z * (SILENCE_MOUTH_FORWARD * stature(is_pet))
}

/// Where a particle of `e` is born, relative to the mouth, and how it
/// launches (the bench's `spawn`, at spread 1.0), on a body of `stature`.
pub fn silence_particle_launch(e: &BreathEmitter, stature: f32, seed: u32) -> (Vec3, Vec3) {
    let cube = || {
        Vec3::new(
            draw(seed, 1) * 2.0 - 1.0,
            draw(seed, 2) * 2.0 - 1.0,
            draw(seed, 3) * 2.0 - 1.0,
        )
        .try_normalize()
        .unwrap_or(Vec3::Y)
    };
    match e.launch {
        BreathLaunch::Inward { shell } => {
            let d = cube();
            let r = shell * stature * (0.6 + 0.4 * draw(seed, 4));
            (d * r, -d * e.speed * stature)
        }
        BreathLaunch::Cone { half_angle } => {
            let polar = (draw(seed, 1) * 2.0 - 1.0) * half_angle;
            let azimuth = draw(seed, 2) * TAU;
            let dir = Vec3::new(
                polar.sin() * azimuth.cos(),
                polar.cos(),
                polar.sin() * azimuth.sin(),
            );
            (Vec3::ZERO, dir * e.speed * stature)
        }
        BreathLaunch::Sphere => (Vec3::ZERO, cube() * e.speed * stature),
    }
}

/// A particle's look `t` of the way through its life: colour, alpha and sprite
/// radius (yards, before the victim's stature).
pub fn silence_particle_look(e: &BreathEmitter, t: f32) -> (Color, f32, f32) {
    (
        keyed_color(e.color, t),
        keyed(e.alpha, t),
        keyed(e.scale, t),
    )
}

/// The rotation a particle's sprite takes facing the camera `camera`, rolled
/// `angle` as the bench rolls it (`ctx.rotate(ang)`: clockwise on the screen,
/// so the other way about the view axis).
pub fn silence_sprite_rotation(angle: f32, camera: Quat) -> Quat {
    camera * Quat::from_rotation_z(-angle)
}

/// The Priest Silence a unit with these auras holds, if any: an aura its
/// application routes to the breath.
pub fn holds_silence_breath(auras: Option<&ActiveAuras>) -> bool {
    auras.is_some_and(|a| {
        a.auras.iter().any(|aura| {
            AuraApplyRoute::for_instance(aura.effect_type, &aura.ability_name)
                == AuraApplyRoute::Owned(AuraApplyOwner::SilenceBreath)
        })
    })
}

// ── Components ───────────────────────────────────────────────────────────────

/// One victim's breath: a top-level entity parked at its mouth, parent of
/// every particle it emits. Lives exactly as long as a living victim holds a
/// Priest Silence.
#[derive(Component)]
pub struct SilenceBreathRig {
    pub target: Entity,
    /// Seconds since the onset.
    pub age: f32,
    /// Fractional particles owed, per emitter.
    pub carry: [f32; 4],
    pub emitted: u32,
    pub stature: f32,
    /// Whether the victim is a pet: its mouth and the body the particles are
    /// held in front of are a pet's.
    pub is_pet: bool,
}

/// One particle: a child of its rig, at the point it is DRAWN.
#[derive(Component)]
pub struct SilenceBreathParticle {
    /// Index into [`SILENCE_EMITTERS`].
    pub emitter: usize,
    /// Where the particle truly is, relative to the mouth.
    pub offset: Vec3,
    pub velocity: Vec3,
    pub age: f32,
    pub life: f32,
    pub stature: f32,
    /// The sprite's roll (the bench's `ang`), radians; it turns at its
    /// emitter's spin.
    pub angle: f32,
}

/// The quad and the three sprites every breath draws with, built once.
#[derive(Clone)]
pub struct SilenceAssets {
    pub quad: Handle<Mesh>,
    pub glow: Handle<Image>,
    pub smoke: Handle<Image>,
    pub star: Handle<Image>,
}

impl SilenceAssets {
    fn build(meshes: &mut Assets<Mesh>, images: &mut Assets<Image>) -> Self {
        let sprite = |images: &mut Assets<Image>, kind: ConflagrateSprite| {
            images.add(disc_texture(SPRITE_PX, move |r, a| kind.alpha_at(r, a)))
        };
        Self {
            quad: meshes.add(Rectangle::new(1.0, 1.0)),
            glow: sprite(images, ConflagrateSprite::Glow),
            smoke: sprite(images, ConflagrateSprite::Smoke),
            star: sprite(images, ConflagrateSprite::Star),
        }
    }

    fn sprite(&self, kind: ConflagrateSprite) -> Handle<Image> {
        match kind {
            ConflagrateSprite::Glow => self.glow.clone(),
            ConflagrateSprite::Star => self.star.clone(),
            ConflagrateSprite::Smoke | ConflagrateSprite::Cloud => self.smoke.clone(),
        }
    }
}

// ── Systems ──────────────────────────────────────────────────────────────────

/// Spawn (graphical-only): a breath at the mouth of every living unit that
/// holds a Priest Silence and has none. A refresh finds its breath already
/// there, so the inhale does not replay.
pub fn spawn_silence_breaths(
    mut commands: Commands,
    rigs: Query<&SilenceBreathRig>,
    units: Query<(
        Entity,
        &Combatant,
        &Transform,
        Option<&ActiveAuras>,
        Has<Pet>,
    )>,
) {
    let breathing: HashSet<Entity> = rigs.iter().map(|r| r.target).collect();
    for (entity, combatant, transform, auras, is_pet) in units.iter() {
        if breathing.contains(&entity) || !combatant.is_alive() || !holds_silence_breath(auras) {
            continue;
        }
        commands.spawn((
            SilenceBreathRig {
                target: entity,
                age: 0.0,
                carry: [0.0; 4],
                emitted: 0,
                stature: stature(is_pet),
                is_pet,
            },
            Transform::from_translation(silence_mouth(
                transform.translation,
                transform.rotation,
                is_pet,
            )),
            Visibility::default(),
            PlayMatchEntity,
        ));
    }
}

/// Animate (graphical-only): hide each breath while its victim is a sheep,
/// and emit every layer whose window is open, integrating the rate over the
/// frame so a frame-rate change does not change the density. Where it stands
/// is [`place_silence_breaths`]'s.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn animate_silence_breaths(
    mut commands: Commands,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut assets: Local<Option<SilenceAssets>>,
    mut rigs: Query<(Entity, &mut SilenceBreathRig, &mut Visibility)>,
    victims: Query<Has<PolymorphedVisual>, (With<Combatant>, Without<SilenceBreathRig>)>,
) {
    let dt = time.delta_secs();
    let assets = assets
        .get_or_insert_with(|| SilenceAssets::build(&mut meshes, &mut images))
        .clone();
    for (rig_entity, mut rig, mut visibility) in rigs.iter_mut() {
        let Ok(sheep) = victims.get(rig.target) else {
            continue;
        };
        rig.age += dt;
        visibility.set_if_neq(if sheep {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        });
        let ms = rig.age * 1000.0;
        for (ei, e) in SILENCE_EMITTERS.iter().enumerate() {
            if !e.emitting_at(ms) {
                continue;
            }
            rig.carry[ei] += e.rate * dt;
            while rig.carry[ei] >= 1.0 {
                rig.carry[ei] -= 1.0;
                let seed = rig_entity
                    .index()
                    .wrapping_mul(2_654_435_761)
                    .wrapping_add(rig.emitted);
                rig.emitted = rig.emitted.wrapping_add(1);
                let (offset, velocity) = silence_particle_launch(e, rig.stature, seed);
                let (color, alpha, _) = silence_particle_look(e, 0.0);
                let particle = commands
                    .spawn((
                        SilenceBreathParticle {
                            emitter: ei,
                            offset,
                            velocity,
                            age: 0.0,
                            life: e.life,
                            stature: rig.stature,
                            angle: draw(seed, 6) * TAU,
                        },
                        Mesh3d(assets.quad.clone()),
                        MeshMaterial3d(materials.add(conflagrate_material(
                            e.additive,
                            color,
                            alpha,
                            Some(assets.sprite(e.sprite)),
                        ))),
                        Transform::from_translation(offset).with_scale(Vec3::splat(1e-4)),
                        NotShadowCaster,
                    ))
                    .id();
                commands.entity(rig_entity).add_child(particle);
            }
        }
    }
}

/// Age, move and spin every particle, and retire it at the end of its life.
pub fn age_silence_breath_particles(
    mut commands: Commands,
    time: Res<Time>,
    mut particles: Query<(Entity, &mut SilenceBreathParticle)>,
) {
    let dt = time.delta_secs();
    for (entity, mut p) in particles.iter_mut() {
        p.age += dt;
        if p.age >= p.life {
            commands.entity(entity).despawn();
            continue;
        }
        let step = p.velocity * dt;
        p.offset += step;
        p.angle += SILENCE_EMITTERS[p.emitter].spin * TAU * dt;
    }
}

/// PostUpdate (graphical-only), after every writer of a body's transform and
/// before propagation: stand each breath at its victim's RENDERED mouth, and
/// place each particle where it is drawn — held in front of that rendered
/// body for the camera (departure 1) — turned to the camera and rolled as the
/// bench rolls it, sized and coloured along its life. The rig is unrotated,
/// so a particle's local pose is its world offset.
#[allow(clippy::type_complexity)]
pub fn place_silence_breaths(
    mut materials: ResMut<Assets<StandardMaterial>>,
    hold: BodyHold,
    mut rigs: Query<
        (&SilenceBreathRig, &mut Transform, &Children),
        (Without<SilenceBreathParticle>, HoldDisjoint),
    >,
    mut particles: Query<
        (
            &SilenceBreathParticle,
            &mut Transform,
            &MeshMaterial3d<StandardMaterial>,
        ),
        (Without<SilenceBreathRig>, HoldDisjoint),
    >,
) {
    let view = hold.view();
    let cam = view.map(|c| c.rotation).unwrap_or(Quat::IDENTITY);
    for (rig, mut rig_transform, children) in rigs.iter_mut() {
        let Some(body) = hold.body(rig.target) else {
            continue;
        };
        let mouth = silence_mouth(body.root, body.rotation, rig.is_pet);
        rig_transform.translation = mouth;
        let capsule = body.capsule();
        for child in children.iter() {
            let Ok((p, mut transform, material)) = particles.get_mut(child) else {
                continue;
            };
            let e = &SILENCE_EMITTERS[p.emitter];
            let at = mouth + p.offset;
            let (drawn, k) = held_in(view, at, capsule);
            let (color, alpha, radius) = silence_particle_look(e, (p.age / p.life).clamp(0.0, 1.0));
            transform.translation = drawn - mouth;
            transform.rotation =
                rig_transform.rotation.inverse() * silence_sprite_rotation(p.angle, cam);
            transform.scale = Vec3::splat((radius * p.stature * k * 2.0).max(1e-4));
            if let Some(m) = materials.get_mut(&material.0) {
                m.base_color = color.with_alpha(alpha);
                if e.additive {
                    let c = color.to_linear();
                    m.emissive = LinearRgba::rgb(
                        c.red * CONFLAGRATE_GLOW,
                        c.green * CONFLAGRATE_GLOW,
                        c.blue * CONFLAGRATE_GLOW,
                    );
                }
            }
        }
    }
}

/// End the breath the frame its silence is gone (expiry and dispel are one
/// path — the detector keys on presence) or its victim dies or is gone.
/// Despawning the rig takes its particles with it; nothing lingers.
pub fn cleanup_silence_breaths(
    mut commands: Commands,
    rigs: Query<(Entity, &SilenceBreathRig)>,
    units: Query<(&Combatant, Option<&ActiveAuras>)>,
) {
    for (entity, rig) in rigs.iter() {
        let holds = units
            .get(rig.target)
            .map(|(c, auras)| c.is_alive() && holds_silence_breath(auras))
            .unwrap_or(false);
        if !holds {
            commands.entity(entity).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_emitter_has_a_unique_name() {
        let mut names: Vec<&str> = SILENCE_EMITTERS.iter().map(|e| e.name).collect();
        let n = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), n);
    }

    #[test]
    fn the_loop_opens_after_the_stand_and_repeats() {
        let w = BreathWindow::Loop(233.0, 800.0);
        assert!(!w.open_at(233.0));
        assert!(w.open_at(1167.0 + 233.0));
        assert!(!w.open_at(1167.0 + 800.0));
        assert!(w.open_at(1167.0 + 4167.0 + 233.0));
        assert!(!w.open_at(1167.0 + 4167.0 + 100.0));
    }
}
