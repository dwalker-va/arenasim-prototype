use bevy::prelude::*;
use bevy::color::LinearRgba;
use crate::states::play_match::components::*;

// ==============================================================================
// Berserker Rage Mask (TBC-style black angry mask + red glow at the head)
// ==============================================================================

/// Head height for the Berserker Rage mask/glow (above the model, below FCT).
const BERSERK_MASK_HEIGHT: f32 = 2.6;

/// Hot rage red-orange for the Berserker Rage glow (base, emissive). Emissive
/// pushed above 1.0 so the glow blooms behind the flat black mask.
fn berserk_glow_colors() -> (Color, LinearRgba) {
    (
        Color::srgba(1.0, 0.35, 0.1, 0.7),
        LinearRgba::new(4.0, 0.9, 0.2, 1.0),
    )
}

/// Spawn the visuals for new Berserker Rage masks (graphical-only): the flat
/// black glyph quad on the marker entity, plus a separate additive glow sphere.
///
/// The mask uses `AlphaMode::Mask` (alpha cutout), NOT the usual `Add` — a
/// flat BLACK shape is invisible under additive blending (black contributes
/// zero light), and a hard cutout has no blend-sorting, so it also avoids the
/// Z-fighting flicker that made `Add` the house default. The glow behind it is
/// standard additive.
pub fn spawn_berserk_mask_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
    new_masks: Query<(Entity, &BerserkMask), (Added<BerserkMask>, Without<Mesh3d>)>,
    transforms: Query<&Transform>,
) {
    for (mask_entity, mask) in new_masks.iter() {
        let Ok(caster_transform) = transforms.get(mask.caster) else {
            continue;
        };
        let head = caster_transform.translation + Vec3::Y * BERSERK_MASK_HEIGHT;

        // The black glyph quad (billboarded each frame by the update system).
        let glyph: Handle<Image> = asset_server.load("textures/effects/berserk_mask.png");
        let mask_mesh = meshes.add(Rectangle::new(1.3, 1.3));
        let mask_material = materials.add(StandardMaterial {
            base_color: Color::WHITE, // texture supplies the flat black + alpha
            base_color_texture: Some(glyph),
            unlit: true,              // flat black regardless of arena lighting
            alpha_mode: AlphaMode::Mask(0.5),
            ..default()
        });
        commands.entity(mask_entity).try_insert((
            Mesh3d(mask_mesh),
            MeshMaterial3d(mask_material),
            Transform::from_translation(head).with_scale(Vec3::splat(0.3)),
        ));

        // The emissive red-orange glow behind it.
        let (base_color, emissive) = berserk_glow_colors();
        let glow_mesh = meshes.add(Sphere::new(0.55));
        let glow_material = materials.add(StandardMaterial {
            base_color,
            emissive,
            alpha_mode: AlphaMode::Add,
            ..default()
        });
        commands.spawn((
            BerserkGlow {
                caster: mask.caster,
                lifetime: mask.lifetime,
                initial_lifetime: mask.initial_lifetime,
            },
            Mesh3d(glow_mesh),
            MeshMaterial3d(glow_material),
            Transform::from_translation(head).with_scale(Vec3::splat(0.3)),
            PlayMatchEntity,
        ));
    }
}

/// Update Berserker Rage masks and glows: follow the caster's head, billboard
/// the mask toward the camera, pop-overshoot in, hold, then collapse/fade.
pub fn update_berserk_masks(
    time: Res<Time>,
    mut masks: Query<(&mut BerserkMask, &mut Transform), (Without<BerserkGlow>, Without<Camera3d>)>,
    mut glows: Query<
        (&mut BerserkGlow, &mut Transform, &MeshMaterial3d<StandardMaterial>),
        (Without<BerserkMask>, Without<Camera3d>),
    >,
    mut materials: ResMut<Assets<StandardMaterial>>,
    transforms: Query<&Transform, (Without<BerserkMask>, Without<BerserkGlow>, Without<Camera3d>)>,
    camera: Query<&Transform, With<Camera3d>>,
) {
    let dt = time.delta_secs();
    let cam = camera.iter().next();

    for (mut mask, mut mask_transform) in masks.iter_mut() {
        mask.lifetime -= dt;
        let elapsed = mask.initial_lifetime - mask.lifetime;

        let mut head = mask_transform.translation;
        if let Ok(caster_transform) = transforms.get(mask.caster) {
            head = caster_transform.translation + Vec3::Y * BERSERK_MASK_HEIGHT;
        }

        if let Some(cam_transform) = cam {
            // Billboard: face the camera plane, and sit slightly in front of
            // the glow along the view axis so the black glyph always wins.
            mask_transform.rotation = cam_transform.rotation;
            let toward_cam = (cam_transform.translation - head).normalize_or_zero();
            mask_transform.translation = head + toward_cam * 0.35;
        } else {
            mask_transform.translation = head;
        }

        // Pop in with overshoot (0-0.15s), settle (0.15-0.3s), hold, then
        // collapse over the final 0.35s — scale-collapse instead of an alpha
        // fade because Mask-mode alpha fades pop out binarily at the cutoff.
        let scale = if elapsed < 0.15 {
            0.3 + (elapsed / 0.15) * (1.15 - 0.3)
        } else if elapsed < 0.3 {
            1.15 - ((elapsed - 0.15) / 0.15) * 0.15
        } else if mask.lifetime < 0.35 {
            (mask.lifetime / 0.35).max(0.0)
        } else {
            1.0
        };
        mask_transform.scale = Vec3::splat(scale);
    }

    for (mut glow, mut glow_transform, material_handle) in glows.iter_mut() {
        glow.lifetime -= dt;
        let elapsed = glow.initial_lifetime - glow.lifetime;
        let progress = (glow.lifetime / glow.initial_lifetime).max(0.0);

        if let Ok(caster_transform) = transforms.get(glow.caster) {
            glow_transform.translation = caster_transform.translation + Vec3::Y * BERSERK_MASK_HEIGHT;
        }

        // Quick flare-in, then an angry pulse while the mask holds.
        let grow = (elapsed / 0.15).min(1.0);
        let pulse = 1.0 + 0.12 * (elapsed * 18.0).sin();
        glow_transform.scale = Vec3::splat(grow * pulse);

        // Fade the emissive out over the effect's life.
        let (base_color, emissive) = berserk_glow_colors();
        if let Some(material) = materials.get_mut(&material_handle.0) {
            material.base_color = base_color.with_alpha(base_color.alpha() * progress);
            material.emissive = LinearRgba::new(
                emissive.red * progress,
                emissive.green * progress,
                emissive.blue * progress,
                1.0,
            );
        }
    }
}

/// Cleanup expired Berserker Rage masks and glows.
pub fn cleanup_expired_berserk_masks(
    mut commands: Commands,
    masks: Query<(Entity, &BerserkMask)>,
    glows: Query<(Entity, &BerserkGlow)>,
) {
    for (entity, mask) in masks.iter() {
        if mask.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
    for (entity, glow) in glows.iter() {
        if glow.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

