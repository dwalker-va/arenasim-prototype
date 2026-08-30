use bevy::color::LinearRgba;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::mesh::ConeAnchor;
use std::f32::consts::TAU;

use crate::states::play_match::components::*;

// ==============================================================================
// Frost Nova — the caster-centred wavefront
// ==============================================================================
//
// From the Classic client data: `frost_nova_area.m2` is anchored at attachment
// 19 (Base — the caster's own feet) and runs 867ms. Its geometry is THREE
// identical flat quads at z = 0 that scale 1x -> 17.98x, 40.76x and 48.10x
// respectively; ring 1 reaches full size at 667ms and then holds while rings 2
// and 3 keep travelling for the full window. Lavender-white #D5D5FF with a
// periwinkle #7A7AFF settling to #2650C9. So: three concentric ground rings
// expanding at DIFFERENT rates, which is what makes them separate as they go.
//
// Three deliberate additions beyond that, all using machinery that already
// ships:
//
//   1. A RAGGED wavefront. The rings are procedural meshes whose radius is
//      perturbed per vertex, with vertex-coloured alpha across the band — the
//      same technique `mortal_strike.rs` uses for its blade ribbon. Frost does
//      not expand as a compass circle.
//   2. CRYSTALS erupting along the wavefront as it passes, then sinking. Same
//      faceted cones the Root treatment already plants (`hard_cc.rs`), which
//      turns the nova from a flat decal into a physical event.
//   3. A PROPAGATED freeze: each victim's root crystals start growing when the
//      wave reaches THEM, not when the aura lands. The wave visibly causes the
//      freeze instead of merely coinciding with it.
//
// One divergence from the source: its rings top out around 6.7yd, but Frost
// Nova's gameplay radius here is 10yd, so the outer ring is scaled to the real
// reach. A wavefront that stops short of the enemies it just rooted would be
// actively misleading about the spell.
//
// The colour follows `SpellSchool::Frost` (100,180,255) rather than the source's
// measured #2650C9 endpoint. That value is a genuinely dark blue, and since the
// ring spends most of its visible life near the end of its travel it read as
// grey rather than frost. `SpellSchool::color_rgb8` is this project's single
// colour authority and it wins.
//
// Graphical-only, keyed on the caster-centred `InstantAbilityFired` marker (the
// one spawned with `target: None`). No `game_rng` draw, no sim write.

/// Total life of the wavefront. Source: 867ms.
const NOVA_SECS: f32 = 0.867;
/// When the innermost ring reaches full size and holds. Source: 667ms.
const NOVA_RING1_FULL: f32 = 0.667;
/// Seconds each successive ring is held back, so the three separate.
const NOVA_RING_STAGGER: f32 = 0.10;
/// Full radii in yards. Frost Nova's gameplay radius is 10, so the outer ring
/// lands on it. That coincidence is load-bearing, not decorative — the
/// wavefront is a promise about where the freeze reaches — so it is pinned
/// against `abilities.ron` by `the_wavefront_lands_on_the_gameplay_radius`.
const NOVA_RADII: [f32; 3] = [5.2, 8.5, 10.0];

/// Where the outer ring stops, for the test that pins it to the RON's range.
pub fn nova_outer_radius() -> f32 {
    NOVA_RADII[2]
}
/// Width of each ring's band.
const NOVA_RING_THICKNESS: f32 = 0.55;
/// Fixed world height, for the same reason the hard-CC ground pieces use one:
/// the arena floor spawns with an identity transform at y=0.
const NOVA_GROUND_Y: f32 = 0.05;
/// Fraction of its radius each ring's outline wobbles by.
const NOVA_RAGGED: f32 = 0.060;
/// How many lobes that wobble has around the circle.
const NOVA_RAGGED_LOBES: f32 = 15.0;
/// Segments per ring. High enough that 15 lobes are actually resolved.
const NOVA_RING_SEGMENTS: u32 = 128;
/// How fast a ring fades as it travels. Tuned up from a plain square, which
/// left the ring nearly gone for the outer half of its journey.
const NOVA_FADE_POW: f32 = 1.25;

const NOVA_CORE_COLOR: Color = Color::srgb(0.84, 0.84, 1.00);
/// `SpellSchool::Frost`, the project's colour authority.
const NOVA_EDGE_COLOR: Color = Color::srgb(0.39, 0.71, 1.00);
const NOVA_EMISSIVE: f32 = 2.4;

/// Crystals thrown up along the outermost wavefront.
const NOVA_CRYSTAL_COUNT: u32 = 17;
const NOVA_CRYSTAL_H: f32 = 0.55;
const NOVA_CRYSTAL_R: f32 = 0.11;
const NOVA_CRYSTAL_LIFE: f32 = 0.55;
const NOVA_CRYSTAL_SIDES: u32 = 5;

/// Deterministic per-piece variation. Same hash as `cc_jitter` — visual only,
/// never `game_rng`.
fn nova_jitter(seed: u32) -> f32 {
    let s = seed.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    ((s >> 9) & 0xFFFF) as f32 / 65536.0
}

/// The ragged radial perturbation for one ring at one angle, as a multiplier on
/// its radius. Two summed sines at incommensurate frequencies plus a per-ring
/// phase, so no two rings share an outline and none closes into a circle.
///
/// Pure and unit-tested: this is the whole of what makes the wavefront ragged.
pub fn nova_wobble(ring: u32, angle: f32) -> f32 {
    let phase = nova_jitter(ring.wrapping_mul(977).wrapping_add(13)) * TAU;
    let n = (angle * NOVA_RAGGED_LOBES + phase).sin() * 0.6
        + (angle * NOVA_RAGGED_LOBES * 1.7 + phase * 2.1).sin() * 0.4;
    1.0 + n * NOVA_RAGGED
}

/// How far through its own travel ring `i` is at nova-age `age`.
///
/// Ring 0 reaches full size early and HOLDS, matching the source; rings 1 and 2
/// run the whole window. Each is held back by `NOVA_RING_STAGGER` so they
/// separate rather than travelling as one thick band.
pub fn nova_ring_progress(ring: u32, age: f32) -> f32 {
    let span = if ring == 0 { NOVA_RING1_FULL } else { NOVA_SECS };
    let delay = ring as f32 * NOVA_RING_STAGGER;
    ((age - delay) / (span - delay).max(0.001)).clamp(0.0, 1.0)
}

/// A ragged unit-radius annulus with vertex-coloured alpha across its band.
///
/// Built ONCE per ring at spawn and then uniformly scaled, because the wobble is
/// fixed and only the radius changes — rebuilding it per frame (the Mortal
/// Strike trail's approach, where the geometry genuinely moves) would be pure
/// waste here.
///
/// The alpha ramp across the band is what gives the ring soft edges without a
/// texture: opaque along the spine, zero at both rims.
fn build_ragged_ring(ring: u32) -> Mesh {
    let seg = NOVA_RING_SEGMENTS;
    let half = NOVA_RING_THICKNESS * 0.5;
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity((seg * 2) as usize);
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity((seg * 2) as usize);
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity((seg * 2) as usize);
    let mut colors: Vec<[f32; 4]> = Vec::with_capacity((seg * 2) as usize);
    let mut indices: Vec<u32> = Vec::with_capacity((seg * 6) as usize);

    for i in 0..seg {
        let a = i as f32 / seg as f32 * TAU;
        // The wobble is applied to the SPINE, and the band keeps a constant
        // thickness around it, so a ragged ring never pinches shut.
        let spine = nova_wobble(ring, a);
        let (sa, ca) = (a.sin(), a.cos());

        positions.push([ca * (spine + half), 0.0, sa * (spine + half)]);
        positions.push([ca * (spine - half), 0.0, sa * (spine - half)]);
        normals.push([0.0, 1.0, 0.0]);
        normals.push([0.0, 1.0, 0.0]);
        uvs.push([i as f32 / seg as f32, 0.0]);
        uvs.push([i as f32 / seg as f32, 1.0]);
        // Transparent at both rims; the spine between them carries the colour.
        colors.push([1.0, 1.0, 1.0, 0.0]);
        colors.push([1.0, 1.0, 1.0, 0.0]);
    }
    // A second concentric pair along the spine itself, at full alpha, so the
    // band ramps rim -> spine -> rim rather than being uniformly translucent.
    for i in 0..seg {
        let a = i as f32 / seg as f32 * TAU;
        let spine = nova_wobble(ring, a);
        let (sa, ca) = (a.sin(), a.cos());
        positions.push([ca * spine, 0.0, sa * spine]);
        normals.push([0.0, 1.0, 0.0]);
        uvs.push([i as f32 / seg as f32, 0.5]);
        colors.push([1.0, 1.0, 1.0, 1.0]);
    }

    let spine_base = seg * 2;
    for i in 0..seg {
        let n = (i + 1) % seg;
        let (o0, i0, s0) = (i * 2, i * 2 + 1, spine_base + i);
        let (o1, i1, s1) = (n * 2, n * 2 + 1, spine_base + n);
        // Outer rim -> spine.
        indices.extend_from_slice(&[o0, s0, o1, o1, s0, s1]);
        // Spine -> inner rim.
        indices.extend_from_slice(&[s0, i0, s1, s1, i0, i1]);
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

/// Spawns the whole nova: three rings, the crystals that erupt along the outer
/// wavefront, and a freeze delay on every enemy the wave will reach.
#[allow(clippy::too_many_arguments)]
pub fn spawn_frost_nova(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    caster: Entity,
    caster_pos: Vec3,
    victims: &[(Entity, Vec3)],
) {
    let origin = caster_pos.with_y(NOVA_GROUND_Y);

    for ring in 0..3u32 {
        let material = materials.add(StandardMaterial {
            base_color: NOVA_CORE_COLOR,
            emissive: LinearRgba::new(
                NOVA_CORE_COLOR.to_srgba().red * NOVA_EMISSIVE,
                NOVA_CORE_COLOR.to_srgba().green * NOVA_EMISSIVE,
                NOVA_CORE_COLOR.to_srgba().blue * NOVA_EMISSIVE,
                1.0,
            ),
            alpha_mode: AlphaMode::Add,
            cull_mode: None,
            double_sided: true,
            ..default()
        });
        commands.spawn((
            Mesh3d(meshes.add(build_ragged_ring(ring))),
            MeshMaterial3d(material),
            Transform::from_translation(origin).with_scale(Vec3::ZERO),
            NovaRing { ring, age: 0.0 },
            PlayMatchEntity,
        ));
    }

    // Crystals along the outermost wavefront. Each is born when the wave passes
    // its own radius, which is why `born_at` inverts the sqrt easing the ring
    // travels on.
    let crystal_mesh = meshes.add(
        Cone::new(NOVA_CRYSTAL_R, NOVA_CRYSTAL_H)
            .mesh()
            .resolution(NOVA_CRYSTAL_SIDES)
            .anchor(ConeAnchor::Base),
    );
    let crystal_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.80, 0.86, 0.95),
        emissive: LinearRgba::new(0.10, 0.16, 0.30, 1.0),
        perceptual_roughness: 0.25,
        ..default()
    });
    for c in 0..NOVA_CRYSTAL_COUNT {
        let a = (c as f32 / NOVA_CRYSTAL_COUNT as f32) * TAU
            + nova_jitter(c.wrapping_mul(31).wrapping_add(5)) * 0.25;
        let frac = nova_jitter(c.wrapping_mul(71).wrapping_add(17)) * 0.85 + 0.10;
        let radius = NOVA_RADII[2] * frac * nova_wobble(2, a);
        let height = NOVA_CRYSTAL_H * (0.7 + 0.6 * nova_jitter(c.wrapping_mul(13).wrapping_add(3)));
        commands.spawn((
            Mesh3d(crystal_mesh.clone()),
            MeshMaterial3d(crystal_material.clone()),
            Transform::from_translation(
                origin.with_y(0.0) + Vec3::new(a.cos() * radius, 0.0, a.sin() * radius),
            )
            .with_scale(Vec3::ZERO),
            NovaShard {
                born_at: nova_arrival_time(frac),
                age: 0.0,
                height,
            },
            PlayMatchEntity,
        ));
    }

    // The propagated freeze. Each victim is told how long to wait before its
    // root crystals start growing, so the wave visibly causes the freeze.
    for (victim, pos) in victims {
        let distance = (pos.with_y(0.0) - caster_pos.with_y(0.0)).length();
        commands.entity(*victim).try_insert(NovaFreezeDelay {
            secs: nova_freeze_delay(distance),
            age: 0.0,
        });
    }

    let _ = caster;
}

/// Expands and fades the three rings.
pub fn update_nova_rings(
    time: Res<Time>,
    mut rings: Query<(&mut NovaRing, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let dt = time.delta_secs();

    for (mut ring, mut transform, handle) in rings.iter_mut() {
        ring.age += dt;
        let k = nova_ring_progress(ring.ring, ring.age);
        if k <= 0.0 {
            transform.scale = Vec3::ZERO;
            continue;
        }
        // sqrt easing: fast off the mark, decelerating — a shockwave, not a
        // balloon.
        let radius = NOVA_RADII[ring.ring as usize] * k.sqrt();
        transform.scale = Vec3::splat(radius);

        if let Some(material) = materials.get_mut(&handle.0) {
            let fade_in = (k / 0.2).min(1.0);
            let fade_out = (1.0 - k).powf(NOVA_FADE_POW);
            let alpha = fade_in * fade_out;
            let core = NOVA_CORE_COLOR.to_srgba();
            let edge = NOVA_EDGE_COLOR.to_srgba();
            let c = [
                core.red + (edge.red - core.red) * k,
                core.green + (edge.green - core.green) * k,
                core.blue + (edge.blue - core.blue) * k,
            ];
            material.base_color = Color::srgba(c[0], c[1], c[2], alpha);
            material.emissive = LinearRgba::new(
                c[0] * NOVA_EMISSIVE * alpha,
                c[1] * NOVA_EMISSIVE * alpha,
                c[2] * NOVA_EMISSIVE * alpha,
                1.0,
            );
        }
    }
}

/// Stabs each crystal up as the wave passes it, then sinks it.
pub fn update_nova_shards(
    time: Res<Time>,
    mut shards: Query<(&mut NovaShard, &mut Transform)>,
) {
    let dt = time.delta_secs();

    for (mut shard, mut transform) in shards.iter_mut() {
        shard.age += dt;
        let since = shard.age - shard.born_at;
        if since < 0.0 || since > NOVA_CRYSTAL_LIFE {
            transform.scale = Vec3::ZERO;
            continue;
        }
        let k = since / NOVA_CRYSTAL_LIFE;
        // Stab up fast, hold, sink.
        let rise = if k < 0.22 {
            (k / 0.22).sqrt()
        } else if k > 0.65 {
            1.0 - (k - 0.65) / 0.35
        } else {
            1.0
        };
        transform.scale = Vec3::new(1.0, (shard.height / NOVA_CRYSTAL_H) * rise.max(0.0), 1.0);
    }
}

/// Drops any freeze delay the wave left on a unit that never got rooted.
///
/// The flourish marks everyone in RADIUS, because the graphical side cannot know
/// who the sim actually rooted — an immune or already-dead target gets no aura
/// and so no rig, and nothing else would ever consume its delay. Left in place
/// it would silently postpone that unit's next root from ANY source.
pub fn expire_nova_freeze_delays(
    mut commands: Commands,
    time: Res<Time>,
    mut delays: Query<(Entity, &mut NovaFreezeDelay)>,
) {
    let dt = time.delta_secs();
    for (entity, mut delay) in delays.iter_mut() {
        delay.age += dt;
        // Generously past the wavefront: by now the rig either consumed it or
        // never will.
        if delay.age > NOVA_SECS * 2.0 {
            commands.entity(entity).remove::<NovaFreezeDelay>();
        }
    }
}

/// Despawns rings and crystals once the wavefront has fully played out.
pub fn cleanup_frost_nova(
    mut commands: Commands,
    rings: Query<(Entity, &NovaRing)>,
    shards: Query<(Entity, &NovaShard)>,
) {
    for (entity, ring) in rings.iter() {
        if ring.age >= NOVA_SECS {
            commands.entity(entity).despawn();
        }
    }
    for (entity, shard) in shards.iter() {
        if shard.age >= shard.born_at + NOVA_CRYSTAL_LIFE {
            commands.entity(entity).despawn();
        }
    }
}

/// When the wavefront first reaches a point `distance` yards from the caster.
///
/// The FIRST ring to arrive, not a fixed one. Each ring inverts its own `sqrt`
/// easing — a point at `f` of a ring's radius is passed at `f^2` of that ring's
/// travel — and each carries its own start offset, so a ring held back two
/// stagger steps genuinely arrives later. Keying everything on the outer ring
/// alone got both halves wrong: it fired early by ignoring the offset, and once
/// that was added it imposed a two-stagger floor on victims standing at
/// the Mage's feet, which for a point-blank AoE is the common case.
///
/// The single source for both things that key on the wave's arrival: the
/// crystals it throws up, and the freeze it propagates.
pub fn nova_arrival_time_at(distance: f32) -> f32 {
    let d = distance.max(0.0);
    (0..3u32)
        .filter(|&i| d <= NOVA_RADII[i as usize])
        .map(|i| {
            let delay = i as f32 * NOVA_RING_STAGGER;
            let span = if i == 0 { NOVA_RING1_FULL } else { NOVA_SECS };
            let f = d / NOVA_RADII[i as usize];
            delay + f * f * (span - delay)
        })
        .fold(f32::INFINITY, f32::min)
        .min(NOVA_SECS)
}

/// As [`nova_arrival_time_at`], but in units of the outer ring's radius — the
/// form the crystal layout works in, since it places crystals by fraction.
pub fn nova_arrival_time(frac: f32) -> f32 {
    nova_arrival_time_at(frac.clamp(0.0, 1.0) * NOVA_RADII[2])
}

/// How long a freshly-rooted unit should wait before its crystals grow.
///
/// Consumed (and removed) by `update_hard_cc_visuals` when it builds the rig.
pub fn nova_freeze_delay(distance: f32) -> f32 {
    nova_arrival_time_at(distance)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rings_separate_as_they_travel() {
        // The whole point of three rings at different rates. If they moved
        // together they would read as one thick band and the second and third
        // would be wasted.
        let mid = NOVA_SECS * 0.6;
        let r: Vec<f32> = (0..3)
            .map(|i| NOVA_RADII[i as usize] * nova_ring_progress(i, mid).sqrt())
            .collect();
        assert!(r[1] > r[0] + 0.5, "rings 0 and 1 overlap: {r:?}");
        assert!(r[2] > r[1] + 0.5, "rings 1 and 2 overlap: {r:?}");
    }

    #[test]
    fn the_outer_ring_reaches_the_spell_radius() {
        // Frost Nova roots everything within 10yd. A wavefront that stops short
        // of a unit it just rooted is actively misleading.
        let full = NOVA_RADII[2] * nova_ring_progress(2, NOVA_SECS).sqrt();
        assert!((full - 10.0).abs() < 0.01, "outer ring reached {full}yd");
    }

    #[test]
    fn ring_one_holds_after_reaching_full_size() {
        // Source behaviour: ring 1 is full at 667ms and then holds for the
        // remaining 200ms rather than continuing to grow.
        assert_eq!(nova_ring_progress(0, NOVA_RING1_FULL), 1.0);
        assert_eq!(nova_ring_progress(0, NOVA_SECS), 1.0);
    }

    #[test]
    fn the_wavefront_is_ragged_but_not_lumpy() {
        // It must actually deviate from a circle, and must never fold inward
        // past the band's own half-thickness, which would pinch the ring shut.
        let mut min = f32::MAX;
        let mut max = f32::MIN;
        for i in 0..512 {
            let w = nova_wobble(2, i as f32 / 512.0 * TAU);
            min = min.min(w);
            max = max.max(w);
        }
        assert!(max - min > 0.02, "the outline is effectively a circle");
        assert!(
            min > NOVA_RING_THICKNESS * 0.5,
            "the wobble can pinch the band shut"
        );
    }

    #[test]
    fn each_ring_has_its_own_outline() {
        // A shared phase would make three concentric copies of one shape, which
        // reads as a printing error rather than a wavefront.
        let a = 0.7;
        assert_ne!(nova_wobble(0, a), nova_wobble(1, a));
        assert_ne!(nova_wobble(1, a), nova_wobble(2, a));
    }

    #[test]
    fn the_wave_arrives_after_the_outer_ring_starts_moving() {
        // The outer ring is held back two stagger steps. Timing anything to
        // "when the wave arrives" without adding that back fires early — by up
        // to 0.2s on a wavefront that only lasts 0.867s, which is a fifth of the
        // whole effect. Crystals erupting before the wave reaches them, and
        // victims freezing before it touches them, both looked like the effect
        // was simply desynced.
        // At the caster's feet the innermost ring is already there, so a
        // point-blank victim freezes at once — Frost Nova is point-blank, so
        // that is the COMMON case, not an edge one.
        assert_eq!(nova_arrival_time_at(0.0), 0.0);
        // At the rim only the outer ring reaches, and it arrives at the very end.
        assert!((nova_arrival_time_at(NOVA_RADII[2]) - NOVA_SECS).abs() < 1e-4);
        // Monotone: further is never sooner.
        let mut previous = -1.0;
        for i in 0..=40 {
            let d = i as f32 / 40.0 * NOVA_RADII[2];
            let t = nova_arrival_time_at(d);
            assert!(t >= previous - 1e-6, "arrival dipped at {d}yd: {t}");
            previous = t;
        }
        // Whichever ring claims a distance really is there at that moment.
        for d in [1.0f32, 3.0, 5.0, 7.0, 9.5] {
            let t = nova_arrival_time_at(d);
            let reached = (0..3u32)
                .map(|i| NOVA_RADII[i as usize] * nova_ring_progress(i, t).sqrt())
                .fold(0.0f32, f32::max);
            assert!(
                reached >= d - 0.05,
                "at {t}s the furthest ring is only at {reached}yd, not {d}yd"
            );
        }
    }

    #[test]
    fn the_freeze_propagates_outward() {
        // A nearer victim must freeze before a further one — the causal chain
        // this whole treatment exists to show.
        let near = nova_freeze_delay(2.0);
        let far = nova_freeze_delay(9.0);
        assert!(near < far, "near {near} should precede far {far}");
        assert_eq!(nova_freeze_delay(0.0), 0.0, "the caster's feet are instant");
        assert!(
            far < NOVA_SECS,
            "every victim must freeze within the wavefront's own life"
        );
    }

    #[test]
    fn the_propagation_spread_is_perceptible() {
        // If the whole spread were a couple of frames it would read as lag
        // rather than propagation, and the feature would not be worth its cost.
        let spread = nova_freeze_delay(NOVA_RADII[2]) - nova_freeze_delay(1.0);
        assert!(
            spread > 0.25,
            "only {spread}s between the nearest and furthest freeze"
        );
    }

    #[test]
    fn a_stranded_freeze_delay_expires() {
        // The flourish marks everyone in RADIUS, but the sim decides who is
        // actually rooted. An immune target (Divine Shield) gets no aura, so no
        // rig, so nothing consumes its delay — and a delay left behind would
        // silently postpone that unit's NEXT root from any source.
        let mut app = App::new();
        app.add_plugins(bevy::time::TimePlugin);
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_millis(100),
        ));
        app.add_systems(Update, expire_nova_freeze_delays);
        let unit = app
            .world_mut()
            .spawn(NovaFreezeDelay { secs: 0.4, age: 0.0 })
            .id();

        app.update();
        assert!(
            app.world().get::<NovaFreezeDelay>(unit).is_some(),
            "it must survive long enough for the rig to claim it"
        );

        // Past twice the wavefront: by now it was either used or never will be.
        for _ in 0..25 {
            app.update();
        }
        assert!(
            app.world().get::<NovaFreezeDelay>(unit).is_none(),
            "a delay nothing consumed must not persist"
        );
    }

    #[test]
    fn the_ragged_ring_mesh_is_closed() {
        let mesh = build_ragged_ring(0);
        let verts = mesh.count_vertices();
        // Two rim loops plus a spine loop.
        assert_eq!(verts, (NOVA_RING_SEGMENTS * 3) as usize);
        let Some(Indices::U32(idx)) = mesh.indices() else {
            panic!("expected u32 indices");
        };
        // Four triangles per segment (rim->spine and spine->rim, two each).
        assert_eq!(idx.len(), (NOVA_RING_SEGMENTS * 12) as usize);
        assert!(
            idx.iter().all(|&i| (i as usize) < verts),
            "an index points past the vertex buffer"
        );
    }

    #[test]
    fn the_ring_band_fades_at_both_rims() {
        // The soft edge lives in vertex alpha, not a texture. Both rims must be
        // transparent and the spine opaque, or the ring is a hard-edged strip.
        use bevy::render::mesh::VertexAttributeValues;
        let mesh = build_ragged_ring(0);
        let Some(VertexAttributeValues::Float32x4(colors)) =
            mesh.attribute(Mesh::ATTRIBUTE_COLOR)
        else {
            panic!("the ring must carry Float32x4 vertex colours");
        };
        let seg = NOVA_RING_SEGMENTS as usize;
        // The first `seg * 2` are the interleaved outer/inner rim pairs.
        for v in colors.iter().take(seg * 2) {
            assert_eq!(v[3], 0.0, "a rim vertex is not transparent");
        }
        // The remaining `seg` are the spine.
        for v in colors.iter().skip(seg * 2) {
            assert_eq!(v[3], 1.0, "a spine vertex is not opaque");
        }
    }
}
