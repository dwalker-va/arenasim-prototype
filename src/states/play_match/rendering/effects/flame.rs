use crate::states::play_match::components::*;
use bevy::color::LinearRgba;
use bevy::prelude::*;

// ==============================================================================
// Flame Particle Visual Effects (Immolate)
// ==============================================================================

/// Visual-only PRNG (xorshift64) for the apply burst. Never `game_rng`: the
/// burst's scatter must not share a stream with crits and damage rolls.
struct BurstRng(u64);

impl BurstRng {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }
    fn next_f32(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x >> 40) as f32 / (1u32 << 24) as f32
    }
}

/// The particles of one Immolate apply burst, as `(offset from the victim,
/// particle)` pairs: 8-12 flames starting near the ground, rising at
/// 2.0-3.5 units/sec with a slight drift, living 0.6-1.0s.
pub fn immolate_apply_burst_particles(seed: u64) -> Vec<(Vec3, FlameParticle)> {
    let mut rng = BurstRng::new(seed);
    let count = 8 + (rng.next_f32() * 5.0) as usize;
    (0..count)
        .map(|_| {
            let offset = Vec3::new(
                (rng.next_f32() - 0.5) * 1.0, // -0.5 to 0.5
                rng.next_f32() * 0.5,         // 0 to 0.5 (start near ground)
                (rng.next_f32() - 0.5) * 1.0,
            );
            let velocity = Vec3::new(
                (rng.next_f32() - 0.5) * 0.5, // Slight horizontal drift
                2.0 + rng.next_f32() * 1.5,   // Upward: 2.0-3.5 units/sec
                (rng.next_f32() - 0.5) * 0.5,
            );
            let lifetime = 0.6 + rng.next_f32() * 0.4; // 0.6-1.0 sec
            (
                offset,
                FlameParticle {
                    velocity,
                    lifetime,
                    initial_lifetime: lifetime,
                },
            )
        })
        .collect()
}

/// Turn each [`ImmolateApplyBurst`] marker into its flame particles, then
/// despawn the marker. Graphical-only: in headless the marker is inert.
pub fn spawn_immolate_apply_bursts(
    mut commands: Commands,
    bursts: Query<(Entity, &ImmolateApplyBurst)>,
) {
    for (entity, burst) in bursts.iter() {
        // Seeded from the marker's entity id and the landing position, so two
        // bursts in one frame still scatter differently.
        let seed = (entity.index() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ ((burst.origin.x.to_bits() as u64) << 1)
            ^ ((burst.origin.z.to_bits() as u64) << 29);
        for (offset, particle) in immolate_apply_burst_particles(seed) {
            commands.spawn((
                particle,
                Transform::from_translation(burst.origin + offset),
                PlayMatchEntity,
            ));
        }
        commands.entity(entity).despawn();
    }
}

/// Update flame particles: move upward, shrink, and despawn when expired.
pub fn update_flame_particles(
    mut commands: Commands,
    time: Res<Time>,
    mut particles: Query<(Entity, &mut FlameParticle, &mut Transform)>,
) {
    let dt = time.delta_secs();

    for (entity, mut particle, mut transform) in particles.iter_mut() {
        particle.lifetime -= dt;

        if particle.lifetime <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }

        // Move in velocity direction (primarily upward)
        transform.translation += particle.velocity * dt;

        // Shrink as lifetime decreases
        let life_ratio = (particle.lifetime / particle.initial_lifetime).max(0.1);
        transform.scale = Vec3::splat(life_ratio);
    }
}

/// Spawn visual meshes for newly created flame particles.
/// Creates small glowing orange/red spheres.
pub fn spawn_flame_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_particles: Query<(Entity, &FlameParticle), (Added<FlameParticle>, Without<Mesh3d>)>,
) {
    for (entity, _particle) in new_particles.iter() {
        // Create a small sphere mesh for the flame particle
        let mesh = meshes.add(Sphere::new(0.15));

        // Fire colors - orange base with bright emissive glow
        let material = materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 0.4, 0.1, 0.9),
            emissive: LinearRgba::rgb(2.0, 0.8, 0.1), // Bright orange glow
            alpha_mode: AlphaMode::Blend,
            ..default()
        });

        // Add visual mesh to the particle entity
        commands
            .entity(entity)
            .try_insert((Mesh3d(mesh), MeshMaterial3d(material)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The burst keeps the look it had when it drew from `game_rng`: the same
    /// count and the same ranges, now from the visual stream.
    #[test]
    fn apply_burst_keeps_its_count_and_ranges() {
        let mut counts = std::collections::BTreeSet::new();
        for seed in 0..500u64 {
            let burst = immolate_apply_burst_particles(seed.wrapping_mul(0x2545_F491_4F6C_DD1D));
            counts.insert(burst.len());
            for (offset, p) in &burst {
                assert!((-0.5..=0.5).contains(&offset.x) && (-0.5..=0.5).contains(&offset.z));
                assert!((0.0..=0.5).contains(&offset.y));
                assert!((-0.25..=0.25).contains(&p.velocity.x));
                assert!((2.0..=3.5).contains(&p.velocity.y));
                assert!((-0.25..=0.25).contains(&p.velocity.z));
                assert!((0.6..=1.0).contains(&p.lifetime));
                assert_eq!(p.lifetime, p.initial_lifetime);
            }
        }
        assert_eq!(counts, (8..=12).collect());
    }
}
