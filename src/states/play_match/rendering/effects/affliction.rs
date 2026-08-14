use bevy::prelude::*;
use bevy::color::LinearRgba;
use crate::states::play_match::components::*;

// ==============================================================================
// Unstable Affliction DoT Glow
// ==============================================================================
//
// Spawn/update/cleanup three-system pattern. The glow is a deep-violet sphere
// that pulses at ~0.5 Hz around afflicted combatants. Distinct from Corruption
// (faster green tendrils) so stacked Corruption + UA reads independently.
//
// Per project memory: AlphaMode::Add, Res<Time>, try_insert, Without<T>.

/// Spawn the UA glow mesh when a `UnstableAfflictionGlow` component is added.
pub fn spawn_ua_glow_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_glows: Query<(Entity, &UnstableAfflictionGlow), (Added<UnstableAfflictionGlow>, Without<Mesh3d>)>,
    transforms: Query<&Transform, Without<UnstableAfflictionGlow>>,
) {
    for (glow_entity, glow) in new_glows.iter() {
        let Ok(target_transform) = transforms.get(glow.target) else {
            continue;
        };

        let mesh = meshes.add(Sphere::new(0.55));
        let material = materials.add(StandardMaterial {
            base_color: Color::srgba(0.35, 0.05, 0.55, 0.30),
            emissive: LinearRgba::new(0.55, 0.10, 0.85, 1.0),
            alpha_mode: AlphaMode::Add,
            unlit: true,
            ..default()
        });

        let position = target_transform.translation + Vec3::Y * 1.0;
        commands.entity(glow_entity).try_insert((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(position),
        ));
    }
}

/// Update the UA glow: follow target, pulse opacity at ~0.5 Hz.
pub fn update_ua_glow(
    time: Res<Time>,
    mut glows: Query<(&mut UnstableAfflictionGlow, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    transforms: Query<&Transform, Without<UnstableAfflictionGlow>>,
) {
    let dt = time.delta_secs();
    for (mut glow, mut glow_transform, material_handle) in glows.iter_mut() {
        glow.phase += dt;

        if let Ok(target_transform) = transforms.get(glow.target) {
            glow_transform.translation = target_transform.translation + Vec3::Y * 1.0;
        }

        // 0.5 Hz pulse — period 2.0s, oscillates between 0.20 and 0.55 alpha.
        let pulse = (glow.phase * std::f32::consts::TAU * 0.5).sin() * 0.5 + 0.5; // [0,1]
        let alpha = 0.20 + 0.35 * pulse;
        let intensity = 0.55 + 0.45 * pulse;

        if let Some(material) = materials.get_mut(&material_handle.0) {
            material.base_color = Color::srgba(0.35, 0.05, 0.55, alpha);
            material.emissive = LinearRgba::new(0.55 * intensity, 0.10 * intensity, 0.85 * intensity, 1.0);
        }
    }
}

/// Despawn the UA glow when its target loses the UA aura (or dies).
pub fn cleanup_ua_glow(
    mut commands: Commands,
    glows: Query<(Entity, &UnstableAfflictionGlow)>,
    targets: Query<&ActiveAuras>,
) {
    for (glow_entity, glow) in glows.iter() {
        let still_afflicted = targets
            .get(glow.target)
            .map(|auras| {
                auras.auras.iter().any(|a| {
                    a.effect_type == AuraType::DamageOverTime
                        && a.ability_name == "Unstable Affliction"
                })
            })
            .unwrap_or(false);

        if !still_afflicted {
            commands.entity(glow_entity).despawn();
        }
    }
}

// ==============================================================================
// Backlash Burst (UA dispel impact on dispeller)
// ==============================================================================
//
// Distinct from DispelBurst: dark-violet shadow color, ~2x scale, snappier
// 0.3s lifetime. Fired on the dispeller the frame backlash damage lands.

/// Spawn the BacklashBurst mesh when the component is added.
pub fn spawn_backlash_burst_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_bursts: Query<(Entity, &BacklashBurst), (Added<BacklashBurst>, Without<Mesh3d>)>,
    transforms: Query<&Transform, Without<BacklashBurst>>,
) {
    for (burst_entity, burst) in new_bursts.iter() {
        let Ok(target_transform) = transforms.get(burst.target) else {
            continue;
        };

        let mesh = meshes.add(Sphere::new(0.6));
        let material = materials.add(StandardMaterial {
            base_color: Color::srgba(0.20, 0.0, 0.35, 0.85),
            emissive: LinearRgba::new(1.6, 0.20, 2.0, 1.0),
            alpha_mode: AlphaMode::Add,
            unlit: true,
            ..default()
        });

        let position = target_transform.translation + Vec3::Y * 1.0;
        commands.entity(burst_entity).try_insert((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(position),
        ));
    }
}

/// Update BacklashBurst: expand quickly (1x -> 2.5x) and fade in 0.3s.
pub fn update_backlash_bursts(
    time: Res<Time>,
    mut bursts: Query<(&mut BacklashBurst, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    transforms: Query<&Transform, Without<BacklashBurst>>,
) {
    let dt = time.delta_secs();
    for (mut burst, mut burst_transform, material_handle) in bursts.iter_mut() {
        burst.lifetime -= dt;

        if let Ok(target_transform) = transforms.get(burst.target) {
            burst_transform.translation = target_transform.translation + Vec3::Y * 1.0;
        }

        let progress = (burst.lifetime / burst.initial_lifetime).max(0.0);
        let scale = 1.0 + (1.0 - progress) * 1.5; // 1.0 -> 2.5
        burst_transform.scale = Vec3::splat(scale);

        if let Some(material) = materials.get_mut(&material_handle.0) {
            material.base_color = Color::srgba(0.20, 0.0, 0.35, 0.85 * progress);
            material.emissive = LinearRgba::new(
                1.6 * progress,
                0.20 * progress,
                2.0 * progress,
                1.0,
            );
        }
    }
}

/// Despawn BacklashBurst entities once their lifetime expires.
pub fn cleanup_expired_backlash_bursts(
    mut commands: Commands,
    bursts: Query<(Entity, &BacklashBurst)>,
) {
    for (entity, burst) in bursts.iter() {
        if burst.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

/// Detect targets that have an Unstable Affliction aura but no `UnstableAfflictionGlow`
/// visual yet, and spawn the glow. Cleanup is handled by `cleanup_ua_glow` once the
/// UA aura is no longer present.
pub fn spawn_ua_glow_for_afflicted(
    mut commands: Commands,
    afflicted: Query<(Entity, &ActiveAuras)>,
    existing_glows: Query<&UnstableAfflictionGlow>,
) {
    use std::collections::HashSet;
    let already_glowing: HashSet<Entity> = existing_glows.iter().map(|g| g.target).collect();

    for (entity, auras) in afflicted.iter() {
        let has_ua = auras.auras.iter().any(|a| {
            a.effect_type == AuraType::DamageOverTime
                && a.ability_name == "Unstable Affliction"
        });
        if has_ua && !already_glowing.contains(&entity) {
            commands.spawn((
                UnstableAfflictionGlow { target: entity, phase: 0.0 },
                PlayMatchEntity,
            ));
        }
    }
}

// ==============================================================================
// DoT Drip Indicators (poison / bleed)
// ==============================================================================
//
// Generic affliction indicator: a `DotDripEmitter` per (target, kind) spawns
// small falling drops — green for poisons, red for bleeds. Color is game
// language shared across abilities; new afflictions are one row in
// `drip_kind_for_aura`. Drips mirror the FlameParticle idiom (velocity +
// lifetime + shrink), emitters follow the detector/cleanup convention.

/// Map an aura to the affliction family it should drip as, or None for DoTs
/// with their own identity (UA glow) or no body visual (Corruption, CoA).
/// Keys on the exact RON `name:` string, same as the class-AI dedup checks.
fn drip_kind_for_aura(aura: &Aura) -> Option<DripKind> {
    if aura.effect_type != AuraType::DamageOverTime {
        return None;
    }
    match aura.ability_name.as_str() {
        "Serpent Sting" => Some(DripKind::Poison), // future rogue poisons join here
        "Rend" => Some(DripKind::Bleed),           // future Rupture/Garrote join here
        _ => None,
    }
}

/// Cheap deterministic jitter in [0,1) from a seed — visual-only, so it does
/// not touch the sim's seeded GameRng.
fn drip_jitter(seed: u32) -> f32 {
    let s = seed.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    ((s >> 9) & 0xFFFF) as f32 / 65536.0
}

/// Detect combatants carrying a mapped DoT and ensure one emitter per
/// (target, kind). Emitter removal is handled by `update_drip_emitters`
/// once the mapped aura is gone.
pub fn spawn_drip_emitters_for_afflicted(
    mut commands: Commands,
    afflicted: Query<(Entity, &ActiveAuras)>,
    existing: Query<&DotDripEmitter>,
) {
    use std::collections::HashSet;
    let already: HashSet<(Entity, DripKind)> =
        existing.iter().map(|e| (e.target, e.kind)).collect();

    for (entity, auras) in afflicted.iter() {
        let mut kinds: HashSet<DripKind> = HashSet::new();
        for aura in auras.auras.iter() {
            if let Some(kind) = drip_kind_for_aura(aura) {
                kinds.insert(kind);
            }
        }
        for kind in kinds {
            if !already.contains(&(entity, kind)) {
                commands.spawn((
                    DotDripEmitter {
                        target: entity,
                        kind,
                        spawn_accumulator: 0.0,
                        drips_spawned: 0,
                    },
                    PlayMatchEntity,
                ));
            }
        }
    }
}

/// Tick emitters: despawn when the mapped aura is gone, otherwise spawn a
/// drip every `DRIP_INTERVAL` at a jittered point around the target's torso.
pub fn update_drip_emitters(
    mut commands: Commands,
    time: Res<Time>,
    mut emitters: Query<(Entity, &mut DotDripEmitter)>,
    targets: Query<(&ActiveAuras, &Transform)>,
) {
    const DRIP_INTERVAL: f32 = 0.45;
    let dt = time.delta_secs();

    for (emitter_entity, mut emitter) in emitters.iter_mut() {
        let Ok((auras, target_transform)) = targets.get(emitter.target) else {
            commands.entity(emitter_entity).despawn();
            continue;
        };
        let still_afflicted = auras
            .auras
            .iter()
            .any(|a| drip_kind_for_aura(a) == Some(emitter.kind));
        if !still_afflicted {
            commands.entity(emitter_entity).despawn();
            continue;
        }

        emitter.spawn_accumulator += dt;
        while emitter.spawn_accumulator >= DRIP_INTERVAL {
            emitter.spawn_accumulator -= DRIP_INTERVAL;
            let seed = emitter.target.index().wrapping_add(emitter.drips_spawned.wrapping_mul(3));
            emitter.drips_spawned = emitter.drips_spawned.wrapping_add(1);

            // Jittered spawn point around the torso (body capsule radius 0.5).
            let angle = drip_jitter(seed) * std::f32::consts::TAU;
            let radius = 0.35 + 0.20 * drip_jitter(seed + 1);
            let height = 0.6 + 0.5 * drip_jitter(seed + 2);
            let offset = Vec3::new(angle.cos() * radius, height, angle.sin() * radius);

            commands.spawn((
                DotDrip {
                    kind: emitter.kind,
                    velocity: Vec3::new(
                        angle.cos() * 0.25,
                        -2.2,
                        angle.sin() * 0.25,
                    ),
                    lifetime: 0.7,
                    initial_lifetime: 0.7,
                },
                Transform::from_translation(target_transform.translation + offset),
                PlayMatchEntity,
            ));
        }
    }
}

/// Spawn visual meshes for newly created drips: small glowing drops,
/// green for poison, red for bleed.
pub fn spawn_drip_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_drips: Query<(Entity, &DotDrip), (Added<DotDrip>, Without<Mesh3d>)>,
) {
    for (drip_entity, drip) in new_drips.iter() {
        // Opaque, not additive: drops are liquid, not glow. Additive blending
        // can only add light, so it can never produce a deep saturated color;
        // opaque unlit gives true deep green/red and depth-sorts like any
        // solid geometry (the documented Z-fighting pitfall is Blend-specific).
        // A whisper of emissive keeps drops readable in arena shadow.
        let (base, emissive) = match drip.kind {
            DripKind::Poison => (
                Color::srgb(0.04, 0.45, 0.07),
                LinearRgba::new(0.02, 0.35, 0.04, 1.0),
            ),
            DripKind::Bleed => (
                Color::srgb(0.50, 0.03, 0.03),
                LinearRgba::new(0.40, 0.02, 0.02, 1.0),
            ),
        };

        let mesh = meshes.add(Sphere::new(0.13));
        let material = materials.add(StandardMaterial {
            base_color: base,
            emissive,
            unlit: true,
            ..default()
        });

        commands.entity(drip_entity).try_insert((Mesh3d(mesh), MeshMaterial3d(material)));
    }
}

/// Animate drips: fall along velocity, shrink over lifetime, despawn at zero.
pub fn update_drips(
    mut commands: Commands,
    time: Res<Time>,
    mut drips: Query<(Entity, &mut DotDrip, &mut Transform)>,
) {
    let dt = time.delta_secs();

    for (entity, mut drip, mut transform) in drips.iter_mut() {
        drip.lifetime -= dt;

        if drip.lifetime <= 0.0 || transform.translation.y <= 0.02 {
            commands.entity(entity).despawn();
            continue;
        }

        transform.translation += drip.velocity * dt;

        let life_ratio = (drip.lifetime / drip.initial_lifetime).max(0.1);
        transform.scale = Vec3::splat(life_ratio);
    }
}

// Silence visibility note: an earlier iteration spawned a dedicated "Silenced"
// floating combat text on apply, but that diverged from how Stun / Fear / Polymorph
// surface to viewers. Those CC types use the `[CC]` combat-log entry plus the
// HUD aura icon (rendered by `render_aura_icons`) and skip floating text entirely.
// Silence now follows the same pattern — the existing CC log line
// "[CC] Unstable Affliction on Team X (5.0s, DR: ...)" plus the aura icon over
// the silenced combatant covers the visibility need without bespoke FCT.

