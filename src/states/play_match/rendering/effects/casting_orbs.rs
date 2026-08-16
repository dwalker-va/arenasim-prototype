use bevy::prelude::*;
use bevy::color::LinearRgba;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::components::*;

// ==============================================================================
// Casting Orb (gathering-orb casting animation)
// ==============================================================================

/// Full-size orb radius (world units) at cast completion; growth scales up to it.
const CASTING_ORB_FULL_SCALE: f32 = 0.35;
/// Height of the orb anchor above the caster's transform (the projectile
/// spawn height, so the completion flash sits where the bolt appears).
const CASTING_ORB_HEIGHT: f32 = 1.5;
/// Horizontal offset from the caster toward the cast target.
const CASTING_ORB_FORWARD: f32 = 0.6;
/// Sputter (interrupt/fizzle) duration — matches the HUD's 0.5s
/// interrupted-display window so both cues end together.
const CASTING_ORB_SPUTTER_SECS: f32 = 0.5;
/// Release-flash duration after a landed completion.
const CASTING_ORB_FLASH_SECS: f32 = 0.25;
/// Seconds between mote spawns while the orb is active.
const CASTING_ORB_MOTE_INTERVAL: f32 = 0.1;
/// Radius of the ring motes stream in from.
const CASTING_ORB_MOTE_RADIUS: f32 = 1.2;
/// Mote travel speed in progress-units per second (~0.4s to reach the orb).
const CASTING_ORB_MOTE_SPEED: f32 = 2.5;
/// Golden angle (radians) — deterministic angular spread for mote offsets
/// without touching any RNG.
const GOLDEN_ANGLE: f32 = 2.399_963;

/// Where the orb sits for a caster at `caster_pos` casting at `target_pos`:
/// chest/launch height, nudged horizontally toward the target.
fn casting_orb_anchor(caster_pos: Vec3, target_pos: Option<Vec3>) -> Vec3 {
    let base = caster_pos + Vec3::Y * CASTING_ORB_HEIGHT;
    let Some(target_pos) = target_pos else {
        return base;
    };
    let mut dir = target_pos - caster_pos;
    dir.y = 0.0;
    if dir.length_squared() < 0.0001 {
        return base;
    }
    base + dir.normalize() * CASTING_ORB_FORWARD
}

/// Spawn a casting orb when a combatant starts a hard cast or channel.
/// No ability filter (R3): anything with cast/channel state gets the orb.
/// Guards: one live (non-ending) orb per caster (drain-life duplicate-check
/// idiom, scoped to Growing/Holding so a back-to-back cast whose prior orb is
/// still in its Sputter/Flash ending doesn't get swallowed), and a cast
/// already flagged `interrupted` never spawns one — a same-frame interrupt's
/// `CastEnding` marker was already consumed, so a late orb would linger.
pub fn spawn_casting_orbs(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    abilities: Res<AbilityDefinitions>,
    new_casts: Query<(Entity, &CastingState), Added<CastingState>>,
    new_channels: Query<(Entity, &ChannelingState), Added<ChannelingState>>,
    existing_orbs: Query<&CastingOrb>,
    casters: Query<&Transform, With<Combatant>>,
) {
    let starts = new_casts
        .iter()
        .map(|(e, c)| {
            (
                e,
                c.ability,
                c.interrupted,
                CastingOrbPhase::Growing,
                Some(c.time_remaining),
            )
        })
        .chain(new_channels.iter().map(|(e, c)| {
            (e, c.ability, c.interrupted, CastingOrbPhase::Holding, None)
        }));

    for (caster_entity, ability, interrupted, phase, time_remaining) in starts {
        if interrupted {
            continue;
        }
        if existing_orbs.iter().any(|orb| {
            orb.caster == caster_entity
                && matches!(orb.phase, CastingOrbPhase::Growing | CastingOrbPhase::Holding)
        }) {
            continue;
        }

        let def = abilities.get_unchecked(&ability);
        let ([r, g, b], [er, eg, eb]) = def.cast_color();

        let mesh = meshes.add(Sphere::new(1.0));
        let material = materials.add(StandardMaterial {
            base_color: Color::srgb(r, g, b),
            emissive: LinearRgba::rgb(er, eg, eb),
            // Opaque, not Add/Blend: an additive orb only brightens what's
            // behind it, so it read as ghostly and hard to distinguish in
            // play. A solid emissive sphere occludes the background and stays
            // unmistakable, and Opaque is depth-tested so the overlapping
            // orb + mote stack has no blend-order flicker at all (the concern
            // that ruled out Blend). Motes share this material and become
            // crisp solid sparks.
            alpha_mode: AlphaMode::Opaque,
            ..default()
        });

        let initial_intensity = match phase {
            CastingOrbPhase::Holding => 1.0,
            _ => 0.0,
        };

        // cast_total tracks the LIVE cast time (incl. CastTimeIncrease auras
        // such as Curse of Tongues), not the base config value — see the
        // field doc comment. Unused in Holding, so 0.0 is fine there.
        let cast_total = match time_remaining {
            Some(remaining) => remaining.max(def.cast_time),
            None => def.cast_time,
        };

        // A casting orb represents cast-time WINDUP (it grows with progress), so
        // an instant (0 cast time) must not show one. This matters in two places:
        // the Animation Sandbox routes instants through a synthetic 0-cast
        // `CastingState` for effect application (a physical instant like Mortal
        // Strike getting a magical cast flash is exactly wrong), and in real
        // matches Frost Shock is the one ability coded as a 0-cast `CastingState`
        // — it should not blip a windup orb either. Skip the 0-total Growing case
        // everywhere; hard casts (orb) and channels (Holding phase) are untouched.
        // Graphical-only, so headless byte-identity is unaffected.
        if matches!(phase, CastingOrbPhase::Growing) && cast_total <= 0.0 {
            continue;
        }

        // Real initial translation (not the world origin) so a mote spawned
        // before the first `update_casting_orbs` tick still streams toward
        // the caster instead of Vec3::ZERO.
        let initial_translation = casters
            .get(caster_entity)
            .map(|caster_transform| casting_orb_anchor(caster_transform.translation, None))
            .unwrap_or(Vec3::ZERO);

        commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            // Scale ~0 until the first update grows it; translation is real.
            Transform::from_translation(initial_translation).with_scale(Vec3::splat(0.001)),
            CastingOrb {
                caster: caster_entity,
                intensity: initial_intensity,
                phase,
                ending_remaining: 0.0,
                mote_spawn_timer: 0.0,
                mote_index: 0,
                cast_total,
            },
            PlayMatchEntity,
        ));
    }
}

/// Per-frame orb animation: follow the caster, grow with cast progress (hard
/// casts) or hold at full intensity (channels), and play the Sputter/Flash
/// ending animations. Time comes from `Res<Time>` accumulation — never gated
/// on per-frame sim movement (fixed-timestep strobe lesson).
pub fn update_casting_orbs(
    time: Res<Time>,
    abilities: Res<AbilityDefinitions>,
    mut orbs: Query<(&mut CastingOrb, &mut Transform)>,
    casters: Query<&Transform, (With<Combatant>, Without<CastingOrb>)>,
    cast_states: Query<&CastingState>,
    channel_states: Query<&ChannelingState>,
) {
    let dt = time.delta_secs();

    for (mut orb, mut orb_transform) in orbs.iter_mut() {
        let Ok(caster_transform) = casters.get(orb.caster) else {
            continue; // caster entity gone; cleanup handles despawn
        };

        match orb.phase {
            CastingOrbPhase::Growing => {
                let Ok(casting) = cast_states.get(orb.caster) else {
                    continue; // state gone; ending marker or cleanup resolves this
                };
                if !casting.interrupted {
                    let def = abilities.get_unchecked(&casting.ability);
                    let total = if orb.cast_total > 0.0 {
                        orb.cast_total
                    } else {
                        def.cast_time
                    };
                    if total > 0.0 {
                        orb.intensity =
                            (1.0 - casting.time_remaining / total).clamp(0.0, 1.0);
                    }
                }
                let target_pos = casting
                    .target
                    .and_then(|t| casters.get(t).ok())
                    .map(|t| t.translation);
                orb_transform.translation =
                    casting_orb_anchor(caster_transform.translation, target_pos);
                // Ease-in growth: quadratic reads as "gathering power".
                let eased = orb.intensity * orb.intensity;
                orb_transform.scale =
                    Vec3::splat((0.15 + 0.85 * eased) * CASTING_ORB_FULL_SCALE);
            }
            CastingOrbPhase::Holding => {
                let target_pos = channel_states
                    .get(orb.caster)
                    .ok()
                    .and_then(|c| casters.get(c.target).ok())
                    .map(|t| t.translation);
                orb.intensity = 1.0;
                orb_transform.translation =
                    casting_orb_anchor(caster_transform.translation, target_pos);
                orb_transform.scale = Vec3::splat(CASTING_ORB_FULL_SCALE);
            }
            CastingOrbPhase::Sputter => {
                orb.ending_remaining -= dt;
                let t = (orb.ending_remaining / CASTING_ORB_SPUTTER_SECS).clamp(0.0, 1.0);
                // Shrink from the captured intensity down to nothing, with a
                // slight sag — reads as the gathered power dissipating.
                let scale = (0.15 + 0.85 * orb.intensity * orb.intensity)
                    * CASTING_ORB_FULL_SCALE
                    * t;
                orb_transform.scale = Vec3::splat(scale.max(0.001));
                orb_transform.translation.y -= 0.4 * dt;
            }
            CastingOrbPhase::Flash => {
                orb.ending_remaining -= dt;
                let t = 1.0 - (orb.ending_remaining / CASTING_ORB_FLASH_SECS).clamp(0.0, 1.0);
                // Expanding pulse under additive blending reads as a release
                // flash at the projectile launch point.
                orb_transform.scale =
                    Vec3::splat(CASTING_ORB_FULL_SCALE * (1.0 + 1.5 * t));
            }
        }
    }
}

/// Stream motes toward active orbs on a fixed interval. Offsets use a
/// golden-angle sequence keyed on the orb's monotonic mote counter, so the
/// spread looks scattered while staying fully deterministic.
pub fn spawn_casting_orb_motes(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    time: Res<Time>,
    mut orbs: Query<(Entity, &mut CastingOrb, &Transform, &MeshMaterial3d<StandardMaterial>)>,
) {
    let dt = time.delta_secs();

    for (orb_entity, mut orb, orb_transform, orb_material) in orbs.iter_mut() {
        if !matches!(orb.phase, CastingOrbPhase::Growing | CastingOrbPhase::Holding) {
            continue;
        }

        orb.mote_spawn_timer -= dt;
        if orb.mote_spawn_timer > 0.0 {
            continue;
        }
        orb.mote_spawn_timer = CASTING_ORB_MOTE_INTERVAL;

        let angle = orb.mote_index as f32 * GOLDEN_ANGLE;
        // Vertical variation from the same counter — deterministic scatter.
        let y = ((orb.mote_index % 7) as f32 / 6.0 - 0.5) * 0.8;
        orb.mote_index = orb.mote_index.wrapping_add(1);
        let start_offset = Vec3::new(
            angle.cos() * CASTING_ORB_MOTE_RADIUS,
            y,
            angle.sin() * CASTING_ORB_MOTE_RADIUS,
        );

        let mesh = meshes.add(Sphere::new(0.06));

        commands.spawn((
            Mesh3d(mesh),
            // Reuse the orb's material: same resolved color, same Add mode,
            // and no per-mote material allocation.
            MeshMaterial3d(orb_material.0.clone()),
            Transform::from_translation(orb_transform.translation + start_offset),
            CastingOrbMote {
                orb: orb_entity,
                progress: 0.0,
                speed: CASTING_ORB_MOTE_SPEED,
                start_offset,
            },
            PlayMatchEntity,
        ));
    }
}

/// Move motes along their lerp into the orb; despawn on arrival, or the
/// moment the parent orb is gone or ending (the sputter/flash owns the
/// screen at that point).
pub fn update_casting_orb_motes(
    mut commands: Commands,
    time: Res<Time>,
    mut motes: Query<(Entity, &mut CastingOrbMote, &mut Transform)>,
    orbs: Query<(&CastingOrb, &Transform), Without<CastingOrbMote>>,
) {
    let dt = time.delta_secs();

    for (mote_entity, mut mote, mut mote_transform) in motes.iter_mut() {
        let Ok((orb, orb_transform)) = orbs.get(mote.orb) else {
            commands.entity(mote_entity).despawn();
            continue;
        };
        if !matches!(orb.phase, CastingOrbPhase::Growing | CastingOrbPhase::Holding) {
            commands.entity(mote_entity).despawn();
            continue;
        }

        mote.progress += mote.speed * dt;
        if mote.progress >= 1.0 {
            commands.entity(mote_entity).despawn();
            continue;
        }

        // Lerp from the (orb-relative) start offset into the orb center, so a
        // moving caster carries the whole stream with it.
        let from = orb_transform.translation + mote.start_offset;
        mote_transform.translation = from.lerp(orb_transform.translation, mote.progress);
    }
}

/// Consume `CastEnding` markers spawned by core combat at cast/channel
/// resolution sites, transitioning the matching orb into its ending phase.
/// Runs in `FixedUpdate` after `CombatSystemPhase::CombatResolution` — the
/// `consume_swing_signals` placement — because FixedUpdate can tick multiple
/// times per rendered frame, and an Update-schedule consumer could miss a
/// marker whose cast started and ended inside one rendered frame.
pub fn consume_cast_ending_signals(
    mut commands: Commands,
    signals: Query<(Entity, &CastEnding)>,
    mut orbs: Query<&mut CastingOrb>,
) {
    for (signal_entity, ending) in signals.iter() {
        for mut orb in orbs.iter_mut() {
            if orb.caster != ending.caster {
                continue;
            }
            if !matches!(orb.phase, CastingOrbPhase::Growing | CastingOrbPhase::Holding) {
                continue; // already ending; nothing to do
            }
            match ending.kind {
                CastEndingKind::Landed => {
                    orb.phase = CastingOrbPhase::Flash;
                    orb.ending_remaining = CASTING_ORB_FLASH_SECS;
                }
                CastEndingKind::Fizzled | CastEndingKind::Interrupted => {
                    orb.phase = CastingOrbPhase::Sputter;
                    orb.ending_remaining = CASTING_ORB_SPUTTER_SECS;
                }
            }
            // At most one NON-ENDING orb per caster (spawn dedup guard) — done.
            break;
        }
        commands.entity(signal_entity).despawn();
    }
}

/// Despawn orbs whose ending animation finished, and silently vanish orbs
/// whose caster lost its cast/channel state with no `CastEnding` marker —
/// caster death, match end, and natural channel completion (all
/// by-design silent; the death/celebration animation owns those moments).
/// Runs in Update AFTER the FixedUpdate consumer, so a marker always wins
/// over the state-gone check within the same rendered frame.
pub fn cleanup_casting_orbs(
    mut commands: Commands,
    orbs: Query<(Entity, &CastingOrb)>,
    cast_states: Query<&CastingState>,
    channel_states: Query<&ChannelingState>,
) {
    for (orb_entity, orb) in orbs.iter() {
        match orb.phase {
            CastingOrbPhase::Sputter | CastingOrbPhase::Flash => {
                if orb.ending_remaining <= 0.0 {
                    commands.entity(orb_entity).despawn();
                }
            }
            CastingOrbPhase::Growing => {
                if cast_states.get(orb.caster).is_err() {
                    commands.entity(orb_entity).despawn();
                }
            }
            CastingOrbPhase::Holding => {
                if channel_states.get(orb.caster).is_err() {
                    commands.entity(orb_entity).despawn();
                }
            }
        }
    }
}
