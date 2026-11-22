use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;

use super::finisher_flash::socket_world;
use crate::states::play_match::components::*;

// ==============================================================================
// The backhand's trail — a pale streak along the sweep
// ==============================================================================
//
// The signed-off AS-336 bench (version 1792302646-d16f, `drawApplyRogue`,
// committed as `docs/design/benches/2026-10-17-rogue-talent-bench.html`)
// draws the backhand's sweep with a trail: the last 120 ms of the HAND's path,
// sampled every 15 ms, as a pale additive line (235,235,245) that tapers in
// width (0.12 yd) and fades (0.55) toward its tail, from the windup's end to
// just past the hold (220-560 ms).
//
// It follows the hand, not the blade. The engine's weapon trail
// (`mortal_strike.rs`) sweeps the blade segment, and the backhand lays its
// blade ALONG the cut, so a blade-swept ribbon would have next to no width;
// the bench's line is the hand's own path, so that is what this draws.
//
// Owned by the stroke (`SwingStyle::Backhand`), so every ability that swings
// it — Rupture, and Expose Armor (AS-366) — draws it.
//
// Graphical-only: spawned by the instant-ability router, writing no sim
// state, registered in `states/mod.rs` only — headless stays byte-identical.

/// The trail shows while the stroke is between these two moments (seconds).
pub const BACKHAND_TRAIL_FROM_SECS: f32 = 0.22;
pub const BACKHAND_TRAIL_UNTIL_SECS: f32 = 0.56;
/// The hand's path is sampled this often back from the present...
pub const BACKHAND_TRAIL_STEP_SECS: f32 = 0.015;
/// ...for this many segments. The bench's eighth segment tapers to nothing,
/// so seven are drawn.
pub const BACKHAND_TRAIL_SEGMENTS: u32 = 7;
/// The bench's eight-step taper: segment `k` is `1 - k/8` of the head's
/// width and alpha.
const BACKHAND_TRAIL_TAPER: f32 = 8.0;
pub const BACKHAND_TRAIL_RGB: [u8; 3] = [235, 235, 245];
pub const BACKHAND_TRAIL_ALPHA: f32 = 0.55;
/// The head's width in yards.
pub const BACKHAND_TRAIL_WIDTH: f32 = 0.12;

/// One backhand's trail, following `owner`'s main hand. The rig stays at the
/// world origin; its segments are its children, placed in world space.
#[derive(Component)]
pub struct BackhandTrail {
    pub owner: Entity,
    pub age: f32,
    /// `(stroke seconds, hand position)` every frame so far.
    pub history: Vec<(f32, Vec3)>,
}

/// Segment `k` (1-based) of a [`BackhandTrail`]: from the hand `k - 1` steps
/// back to the hand `k` steps back.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub struct BackhandTrailSegment(pub u32);

/// Segment `k`'s share of the head's width and alpha.
pub fn backhand_trail_taper(k: u32) -> f32 {
    1.0 - k as f32 / BACKHAND_TRAIL_TAPER
}

/// The hand at stroke time `t`, interpolated from `history`; `None` before
/// the first sample.
fn hand_at(history: &[(f32, Vec3)], t: f32) -> Option<Vec3> {
    let first = history.first()?;
    if t <= first.0 {
        return (t >= first.0 - 1e-5).then_some(first.1);
    }
    for pair in history.windows(2) {
        let ((t0, p0), (t1, p1)) = (pair[0], pair[1]);
        if t <= t1 {
            let f = if t1 > t0 { (t - t0) / (t1 - t0) } else { 1.0 };
            return Some(p0.lerp(p1, f));
        }
    }
    history.last().map(|(_, p)| *p)
}

/// Segment `k`'s two ends at stroke time `age`: the hand `k - 1` and `k` steps
/// back. `None` while the trail is not showing, or when the older end reaches
/// back before the windup's end (the bench stops the line there).
pub fn backhand_trail_segment(history: &[(f32, Vec3)], age: f32, k: u32) -> Option<(Vec3, Vec3)> {
    if age <= BACKHAND_TRAIL_FROM_SECS || age >= BACKHAND_TRAIL_UNTIL_SECS {
        return None;
    }
    let older = age - BACKHAND_TRAIL_STEP_SECS * k as f32;
    if older < BACKHAND_TRAIL_FROM_SECS {
        return None;
    }
    let newer = age - BACKHAND_TRAIL_STEP_SECS * (k - 1) as f32;
    Some((hand_at(history, newer)?, hand_at(history, older)?))
}

/// The assets every trail draws with, built once.
pub struct BackhandTrailAssets {
    quad: Handle<Mesh>,
}

/// Spawn a backhand's trail on `owner`. Called by the instant-ability router
/// as the stroke starts.
pub fn spawn_backhand_trail(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    assets: &mut Option<BackhandTrailAssets>,
    owner: Entity,
) {
    let assets = assets.get_or_insert_with(|| BackhandTrailAssets {
        quad: meshes.add(Rectangle::new(1.0, 1.0)),
    });
    let rig = commands
        .spawn((
            BackhandTrail {
                owner,
                age: 0.0,
                history: Vec::new(),
            },
            Transform::default(),
            Visibility::default(),
            PlayMatchEntity,
        ))
        .id();
    let [r, g, b] = BACKHAND_TRAIL_RGB.map(|c| c as f32 / 255.0);
    let parts: Vec<Entity> = (1..=BACKHAND_TRAIL_SEGMENTS)
        .map(|k| {
            let material = materials.add(StandardMaterial {
                base_color: Color::srgba(r, g, b, BACKHAND_TRAIL_ALPHA * backhand_trail_taper(k)),
                // A solid line, as the bench strokes it, adding light.
                unlit: true,
                alpha_mode: AlphaMode::Add,
                cull_mode: None,
                double_sided: true,
                ..default()
            });
            commands
                .spawn((
                    BackhandTrailSegment(k),
                    Mesh3d(assets.quad.clone()),
                    MeshMaterial3d(material),
                    Transform::from_scale(Vec3::splat(1e-3)),
                    Visibility::Hidden,
                    NotShadowCaster,
                ))
                .id()
        })
        .collect();
    commands.entity(rig).add_children(&parts);
}

/// Update (graphical-only), after the swing and the body lean: record the
/// hand, lay each segment along its stretch of the path facing the camera, and
/// retire the trail when it is done or its owner dies or is gone.
#[allow(clippy::type_complexity)]
pub fn update_backhand_trails(
    mut commands: Commands,
    time: Res<Time>,
    camera: Query<&Transform, (With<Camera3d>, Without<BackhandTrailSegment>)>,
    mut rigs: Query<(Entity, &mut BackhandTrail, &Children)>,
    units: Query<
        (&Combatant, &Transform),
        (
            Without<BackhandTrailSegment>,
            Without<WeaponSocket>,
            Without<VisualBody>,
        ),
    >,
    bodies: Query<&Transform, (With<VisualBody>, Without<BackhandTrailSegment>)>,
    sockets: Query<(&WeaponSocket, &Transform, &ChildOf), Without<BackhandTrailSegment>>,
    mut segments: Query<
        (&BackhandTrailSegment, &mut Transform, &mut Visibility),
        (
            Without<Camera3d>,
            Without<WeaponSocket>,
            Without<VisualBody>,
            Without<Combatant>,
        ),
    >,
) {
    let dt = time.delta_secs();
    let camera = camera.iter().next().map(|c| c.rotation);
    for (rig, mut trail, children) in rigs.iter_mut() {
        // On the stroke's own clock: it too advances before it poses.
        trail.age += dt;
        let Ok((owner, unit)) = units.get(trail.owner) else {
            commands.entity(rig).despawn();
            continue;
        };
        if trail.age >= BACKHAND_TRAIL_UNTIL_SECS || !owner.is_alive() {
            commands.entity(rig).despawn();
            continue;
        }
        let hand = sockets
            .iter()
            .find(|(s, _, _)| s.owner == trail.owner && s.hand == WeaponHand::Main)
            .and_then(|(_, local, parent)| {
                bodies
                    .get(parent.parent())
                    .ok()
                    .map(|body| socket_world(unit, body, local))
            });
        let age = trail.age;
        if let Some(hand) = hand {
            trail.history.push((age, hand));
        }
        for child in children.iter() {
            let Ok((segment, mut transform, mut visibility)) = segments.get_mut(child) else {
                continue;
            };
            let k = segment.0;
            let ends = hand.and_then(|_| backhand_trail_segment(&trail.history, age, k));
            let Some((head, tail)) = ends.filter(|(h, t)| h.distance(*t) > 1e-5) else {
                *visibility = Visibility::Hidden;
                continue;
            };
            *visibility = Visibility::Inherited;
            let along = head - tail;
            transform.translation = (head + tail) * 0.5;
            transform.scale = Vec3::new(
                BACKHAND_TRAIL_WIDTH * backhand_trail_taper(k),
                along.length(),
                1.0,
            );
            if let Some(camera) = camera {
                // Face the camera, rolled so the quad's long (local Y) axis
                // lies along the segment as the camera sees it.
                let (right, up) = (camera * Vec3::X, camera * Vec3::Y);
                let (sx, sy) = (along.dot(right), along.dot(up));
                transform.rotation = camera * Quat::from_rotation_z((-sx).atan2(sy));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line() -> Vec<(f32, Vec3)> {
        (0..=60)
            .map(|i| (i as f32 * 0.01, Vec3::X * i as f32))
            .collect()
    }

    #[test]
    fn the_trail_shows_from_the_windups_end_to_just_past_the_hold() {
        let h = line();
        assert!(backhand_trail_segment(&h, 0.22, 1).is_none());
        assert!(backhand_trail_segment(&h, 0.24, 1).is_some());
        assert!(backhand_trail_segment(&h, 0.55, 1).is_some());
        assert!(backhand_trail_segment(&h, 0.56, 1).is_none());
    }

    #[test]
    fn a_segment_spans_one_step_of_the_hands_path() {
        let h = line();
        let (head, tail) = backhand_trail_segment(&h, 0.40, 3).unwrap();
        assert!((head.x - 37.0).abs() < 1e-3, "{head}");
        assert!((tail.x - 35.5).abs() < 1e-3, "{tail}");
    }

    #[test]
    fn the_tail_stops_at_the_windups_end() {
        let h = line();
        // At 0.25 s only the segments reaching back no further than 0.22 s.
        assert!(backhand_trail_segment(&h, 0.25, 2).is_some());
        assert!(backhand_trail_segment(&h, 0.25, 3).is_none());
    }

    #[test]
    fn the_taper_is_the_benchs_eighths() {
        assert!((backhand_trail_taper(1) - 0.875).abs() < 1e-6);
        assert!((backhand_trail_taper(7) - 0.125).abs() < 1e-6);
    }
}
