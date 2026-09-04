use bevy::color::LinearRgba;
use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::mesh::ConeAnchor;
use std::f32::consts::TAU;

use super::spell_bolts::{build_arc_band, soft_dot_texture, star_flash_texture};
use crate::states::play_match::abilities::{AbilityType, SpellSchool};
use crate::states::play_match::components::*;

// ==============================================================================
// School impact — the shared landing under the bespoke bolts
// ==============================================================================
//
// The third generic hook. `CastingState` drives the casting orb and
// `InstantAbilityFired` drives the caster-side flourishes; both are caster-side.
// A projectile's LANDING is a hook of its own, and until this module only the
// bespoke bolts (`spell_bolts.rs`) and Death Coil reached anything through it.
// Aimed Shot, Arcane Shot, Serpent Sting and Concussive Shot arrived in
// silence, and Mind Blast landed as a hardcoded-purple sphere that nothing else
// could reuse.
//
// This is the recolour tier the animation walk always intended: ONE burst —
// a flash at the point of contact, an optional expanding band, a spray whose
// character comes from the school — parameterised by a per-school row in
// [`impact_style`]. A new school is one row; a new projectile is one arm in
// [`SchoolImpact::anchor_for`]. Signatures (Frostbolt, Shadow Bolt, Death Coil,
// Lightning Bolt) stay bespoke above it and never route here.
//
// From the Classic Era client data (build 1.15.9.69547), walking
// `SpellXSpellVisual -> SpellVisualEvent -> SpellVisualKit ->
// SpellVisualKitModelAttach -> SpellVisualEffectName` and parsing the M2s:
//
// - **Aimed Shot, Arcane Shot and Concussive Shot share ONE visual** —
//   `arcaneshot_missile.m2` in flight and `spells/magic_impact_chest.m2`
//   (fdid 166525) at chest attachment 34 on landing. The three arrows are the
//   same hit in the source, which is exactly the shared-tier premise. The model
//   is pure particles (no mesh): a `cyanstarflash` star flash, a
//   `shockwave10d` ring expanding 0.23 -> 1.37 over 500ms, `blue_glow2` sparks
//   falling under 6.7 gravity, `toonsmoke16` puffs and an `aurarune7` rune —
//   6 emitters, all additive, everything over by 1600ms, bounding radius 3.78.
// - **Serpent Sting** lands with `spells/bestowdisease_impact_chest.m2`
//   (165679): a 167ms burst of ~25 small `flare.blp` droplets at 3.3 yd/s
//   that are ALPHA-blended, not additive, then a 3s lingering
//   `clouds8x8fade` cloud in dark olive. The cloud is the DoT's job here —
//   `affliction.rs` already drips green for the sting's whole duration — so the
//   impact takes only the droplet burst.
// - **Mind Blast is not a burst.** `spells/mindblast_head.m2` (166558) attaches
//   to HEAD attachment 20 and smoulders: `flamelick_purple` licks rising at
//   negative gravity for 200-1400ms, a few `lavalump2` embers, and
//   alpha-blended `toonsmoke16` in violet fading to grey, all over by 2000ms.
//   Its legacy sphere sat at chest height and expanded; the source lingers on
//   the head and rises. This is why [`ImpactAnchor::Head`] exists.
// - **Web has no impact at all.** `web_missile.m2` flies, and the only kit on
//   landing is `web_state.m2` — the root STATE, which `hard_cc.rs` already
//   draws as the shin-high web plus its apply flare. A generic burst on top
//   would double the landing, so `anchor_for` returns `None` for it.
//
// Two deliberate divergences from the source:
//
//   1. **Colour comes from `SpellSchool`, not the models.** The arrows' shared
//      impact is violet-with-green in the source regardless of school; here
//      Arcane Shot is Arcane pink and the two Physical arrows are hueless, on
//      the precedent Frost Nova and Shadow Bolt set. Physical is the one
//      school that cannot use its own `color_rgb8`: that tan (199,156,110) is
//      within a few percent of the arena floor (0.79,0.66,0.46), so an additive
//      tan burst would lift the sand by almost nothing and read as a plain
//      brightening of the capsule. A Physical hit is bone-white splinters and
//      a hueless flash instead — the struck-metal vocabulary Mortal Strike
//      already uses, with the school's identity carried by MATERIAL and
//      MOTION, not hue.
//   2. **Everything is compressed.** The arrows recast on the GCD; a literal
//      1.6s magic burst would still be up when the next landed.

/// Height of the chest anchor above a combatant's transform.
///
/// The capsule is `Capsule3d::new(0.5, 1.5)` CENTRED on the transform, so it
/// spans -1.25..+1.25 and its upper torso is around +0.55. The previous bench
/// drew the unit with its feet at the transform and put attachment 34 at
/// +1.45, which on the real rig is 0.2yd above the top of the head — the bolt
/// impacts shipped playing there. Both tiers now share this anchor.
pub const IMPACT_CHEST_Y: f32 = 0.55;
/// Height of the head anchor (attachment 20) above the transform. The crown
/// is at +1.25; the smoulder rises from just below it.
pub const IMPACT_HEAD_Y: f32 = 1.05;
/// A pet's mesh child sits below its transform (`PET_MESH_Y - pet_position.y`
/// in `play_match/mod.rs`), and its capsule is roughly half a combatant's.
const IMPACT_PET_BODY_Y: f32 = -0.45;
const IMPACT_PET_STATURE: f32 = 0.55;

/// Damage as a fraction of the victim's max health at which a burst reaches
/// full size. Below it the flash and spray shrink toward the floor.
pub const IMPACT_MAGNITUDE_FULL: f32 = 0.20;
/// Size of a burst that did no damage at all (aura-only landings), as a
/// fraction of full. It still has to read as a landing.
pub const IMPACT_MAGNITUDE_FLOOR: f32 = 0.70;
/// Crits get a visibly bigger flash and more debris. Cosmetic only — `is_crit`
/// drove floating text and nothing else in world space until this.
pub const IMPACT_CRIT_SCALE: f32 = 1.35;
pub const IMPACT_CRIT_SPRAY: f32 = 1.5;

/// Soft-rimmed band, same construction as the bolt shockwave.
const IMPACT_RING_THICKNESS: f32 = 0.24;
const IMPACT_RING_SEGMENTS: u32 = 72;

/// Bone-white. See divergence 1 — Physical cannot use its own school colour on
/// this floor.
const PHYSICAL_COLOR: Color = Color::srgb(0.96, 0.94, 0.90);
/// The dark drop Nature's droplets are drawn in. Alpha-blended, so it DARKENS
/// the sand the way the source's `flare.blp` batch does; a bright green
/// additive drop is invisible against a lit capsule.
const NATURE_DROP_COLOR: Color = Color::srgb(0.10, 0.36, 0.03);
/// Shadow's dark blot — the Shadow Bolt lesson, applied to Mind Blast.
const SHADOW_BLOT_COLOR: Color = Color::srgb(0.10, 0.05, 0.17);

/// What a spray's pieces are.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SprayKind {
    /// A thin lit sliver, thrown and falling. Physical.
    Splinter,
    /// A five-sided crystal, the same one the frost family uses.
    Chip,
    /// A small alpha-blended bead with silhouette. Nature.
    Drop,
    /// A soft billboarded glow. Arcane / Holy / Fire / Shadow.
    Spark,
}

/// The debris thrown by a landing.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Spray {
    pub count: u32,
    /// Yards per second at launch.
    pub speed: f32,
    /// Downward acceleration. Negative RISES (embers, smoulder).
    pub gravity: f32,
    pub life: f32,
    pub radius: f32,
    pub kind: SprayKind,
    /// `true` for `AlphaMode::Add`; `false` for a blended piece with a
    /// silhouette that can darken.
    pub additive: bool,
    /// Bias of launch directions toward world up, 0..1.
    pub lift: f32,
    /// Bias of launch directions back toward the caster, 0..1.
    pub back: f32,
    /// A colour of its own, or `None` for the school colour.
    pub color: Option<Color>,
}

/// A lingering emission after the hit — Mind Blast's head smoulder.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Smoulder {
    /// How long the rig keeps emitting.
    pub secs: f32,
    /// Motes per second.
    pub rate: f32,
    /// Upward speed of each mote.
    pub rise: f32,
    pub life: f32,
    pub radius: f32,
}

/// One school's row.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ImpactStyle {
    pub color: Color,
    /// Multiplier from the school colour to the additive emissive.
    pub emissive: f32,
    pub flash_radius: f32,
    pub flash_secs: f32,
    /// `(radius, secs)` of an expanding band — magic language, so Physical
    /// and Nature have none.
    pub ring: Option<(f32, f32)>,
    pub spray: Option<Spray>,
    /// `(radius, secs, alpha)` of a dark blended mass behind the flash.
    pub blot: Option<(f32, f32, f32)>,
    pub smoulder: Option<Smoulder>,
}

impl ImpactStyle {
    /// How long the whole landing plays.
    pub fn life(&self) -> f32 {
        let mut life = self.flash_secs;
        if let Some((_, secs)) = self.ring {
            life = life.max(secs);
        }
        if let Some(spray) = self.spray {
            life = life.max(spray.life);
        }
        if let Some((_, secs, _)) = self.blot {
            life = life.max(secs);
        }
        if let Some(s) = self.smoulder {
            life = life.max(s.secs + s.life);
        }
        life
    }
}

/// The per-school row. Exhaustive, so a new school cannot land silently.
pub fn impact_style(school: SpellSchool) -> ImpactStyle {
    let color = school.color();
    match school {
        SpellSchool::Physical => ImpactStyle {
            color: PHYSICAL_COLOR,
            emissive: 1.6,
            flash_radius: 0.50,
            flash_secs: 0.12,
            ring: None,
            spray: Some(Spray {
                count: 12,
                speed: 5.5,
                gravity: 14.0,
                life: 0.40,
                radius: 0.14,
                kind: SprayKind::Splinter,
                additive: false,
                lift: 0.25,
                back: 0.45,
                color: None,
            }),
            blot: None,
            smoulder: None,
        },
        SpellSchool::Arcane => ImpactStyle {
            color,
            emissive: 2.6,
            flash_radius: 0.70,
            flash_secs: 0.14,
            ring: Some((1.15, 0.32)),
            spray: Some(Spray {
                count: 10,
                speed: 1.7,
                gravity: 0.0,
                life: 0.60,
                radius: 0.12,
                kind: SprayKind::Spark,
                additive: true,
                lift: 0.0,
                back: 0.0,
                color: None,
            }),
            blot: None,
            smoulder: None,
        },
        SpellSchool::Nature => ImpactStyle {
            color,
            emissive: 2.0,
            flash_radius: 0.48,
            flash_secs: 0.12,
            ring: None,
            spray: Some(Spray {
                count: 22,
                speed: 3.3,
                gravity: 9.0,
                life: 0.50,
                radius: 0.075,
                kind: SprayKind::Drop,
                additive: false,
                lift: 0.35,
                back: 0.2,
                color: Some(NATURE_DROP_COLOR),
            }),
            blot: None,
            smoulder: None,
        },
        SpellSchool::Shadow => ImpactStyle {
            color,
            emissive: 3.2,
            flash_radius: 0.55,
            flash_secs: 0.16,
            ring: None,
            spray: None,
            blot: Some((0.42, 0.30, 0.85)),
            smoulder: Some(Smoulder {
                secs: 1.10,
                rate: 22.0,
                rise: 1.10,
                life: 0.70,
                radius: 0.13,
            }),
        },
        SpellSchool::Frost => ImpactStyle {
            color,
            emissive: 2.4,
            flash_radius: 0.65,
            flash_secs: 0.14,
            ring: Some((1.05, 0.32)),
            spray: Some(Spray {
                count: 10,
                speed: 3.2,
                gravity: 6.5,
                life: 0.40,
                radius: 0.09,
                kind: SprayKind::Chip,
                additive: false,
                lift: 0.0,
                back: 0.0,
                color: None,
            }),
            blot: None,
            smoulder: None,
        },
        SpellSchool::Fire => ImpactStyle {
            color,
            emissive: 2.8,
            flash_radius: 0.70,
            flash_secs: 0.14,
            ring: None,
            spray: Some(Spray {
                count: 14,
                speed: 2.2,
                gravity: -3.0,
                life: 0.55,
                radius: 0.11,
                kind: SprayKind::Spark,
                additive: true,
                lift: 0.7,
                back: 0.0,
                color: None,
            }),
            blot: None,
            smoulder: None,
        },
        SpellSchool::Holy => ImpactStyle {
            color,
            emissive: 2.4,
            flash_radius: 0.70,
            flash_secs: 0.16,
            ring: Some((1.10, 0.36)),
            spray: Some(Spray {
                count: 8,
                speed: 1.2,
                gravity: -1.5,
                life: 0.60,
                radius: 0.10,
                kind: SprayKind::Spark,
                additive: true,
                lift: 0.5,
                back: 0.0,
                color: None,
            }),
            blot: None,
            smoulder: None,
        },
        SpellSchool::None => ImpactStyle {
            color,
            emissive: 1.4,
            flash_radius: 0.45,
            flash_secs: 0.12,
            ring: None,
            spray: None,
            blot: None,
            smoulder: None,
        },
    }
}

/// The row a landing actually plays: its school's, unless the ability
/// overrides it.
///
/// Mana Burn is the one override so far. It is Shadow, but the client gives
/// it its own model (`manaburn_chest.m2`, chest attachment 34) where Mind
/// Blast smoulders on the head — so it takes the Shadow colour and none of
/// Mind Blast's shape: a chest flash and a fan of sparks pulled UPWARD out of
/// the victim, the mana leaving. Brief, like the arrows. (The model itself
/// could not be fetched — the CASC endpoint refuses that file — so the
/// shape is designed from the attachment and the name, not the emitters.)
pub fn landing_style(ability: AbilityType, school: SpellSchool) -> ImpactStyle {
    match ability {
        AbilityType::ManaBurn => ImpactStyle {
            color: SpellSchool::Shadow.color(),
            emissive: 3.0,
            flash_radius: 0.55,
            flash_secs: 0.14,
            ring: None,
            spray: Some(Spray {
                count: 14,
                speed: 3.0,
                gravity: -3.0,
                life: 0.55,
                radius: 0.11,
                kind: SprayKind::Spark,
                additive: true,
                lift: 0.9,
                back: 0.0,
                color: None,
            }),
            blot: Some((0.34, 0.22, 0.75)),
            smoulder: None,
        },
        _ => impact_style(school),
    }
}

/// Scale of a landing from how much it hurt.
///
/// `magnitude` is damage dealt (health plus absorbed) over the victim's max
/// health; `0.0` for an aura-only landing. Reaches 1.0 at
/// [`IMPACT_MAGNITUDE_FULL`], never drops below [`IMPACT_MAGNITUDE_FLOOR`], and
/// a crit multiplies the result. Pure so the crit read can be asserted.
pub fn impact_size(magnitude: f32, is_crit: bool) -> f32 {
    let k = (magnitude.max(0.0) / IMPACT_MAGNITUDE_FULL).clamp(0.0, 1.0);
    let size = IMPACT_MAGNITUDE_FLOOR + (1.0 - IMPACT_MAGNITUDE_FLOOR) * k;
    if is_crit {
        size * IMPACT_CRIT_SCALE
    } else {
        size
    }
}

/// How many pieces a spray throws for this landing.
pub fn spray_count(spray: &Spray, is_crit: bool) -> u32 {
    if is_crit {
        (spray.count as f32 * IMPACT_CRIT_SPRAY).round() as u32
    } else {
        spray.count
    }
}

/// Where an impact plays, given its victim's transform.
///
/// Anchors are measured from the capsule CENTRE, which is where a combatant's
/// transform sits (see [`IMPACT_CHEST_Y`]). A pet's body hangs below its
/// transform at about half the stature.
pub fn impact_origin(anchor: ImpactAnchor, translation: Vec3, is_pet: bool) -> Vec3 {
    let height = match anchor {
        ImpactAnchor::Chest => IMPACT_CHEST_Y,
        ImpactAnchor::Head => IMPACT_HEAD_Y,
    };
    let y = if is_pet {
        IMPACT_PET_BODY_Y + height * IMPACT_PET_STATURE
    } else {
        height
    };
    translation + Vec3::Y * y
}

/// The rig's rotation: local **+Z points back toward the caster**, yaw only.
///
/// Yaw only, unlike the bolt rigs, because this rig's motes fall and rise
/// along its local Y and that has to stay world up. The incoming pitch on a
/// chest hit is small anyway.
pub fn impact_rotation(from: Vec3) -> Quat {
    let flat = Vec3::new(from.x, 0.0, from.z);
    if flat.length_squared() > 1e-6 {
        let flat = flat.normalize();
        Quat::from_rotation_y(flat.x.atan2(flat.z))
    } else {
        Quat::IDENTITY
    }
}

/// Unit launch direction of the `i`th of `n` pieces, in the rig's frame.
///
/// A golden-angle spiral over the sphere — even, deterministic, never
/// `game_rng` — then biased: `lift` folds the lower hemisphere upward and
/// `back` folds the far hemisphere toward local +Z, the caster. A fan spread
/// evenly over a sphere reads as an explosion; folding it is what makes
/// splinters splash back off the point of contact.
pub fn spray_direction(i: u32, n: u32, lift: f32, back: f32) -> Vec3 {
    let n = n.max(1) as f32;
    let y = 1.0 - 2.0 * (i as f32 + 0.5) / n;
    let r = (1.0 - y * y).max(0.0).sqrt();
    let theta = i as f32 * 2.399_963_2;
    let mut d = Vec3::new(r * theta.cos(), y, r * theta.sin());
    d.y = d.y * (1.0 - lift) + d.y.abs() * lift;
    d.z = d.z * (1.0 - back) + d.z.abs() * back;
    d.normalize_or_zero()
}

/// Cheap deterministic jitter in [0, 1). Visual only.
fn impact_jitter(seed: u32) -> f32 {
    let s = seed
        .wrapping_mul(747_796_405)
        .wrapping_add(2_891_336_453);
    let s = ((s >> ((s >> 28) + 4)) ^ s).wrapping_mul(277_803_737);
    ((s >> 22) ^ s) as f32 / u32::MAX as f32
}

fn emissive_of(color: Color, strength: f32) -> LinearRgba {
    let c = color.to_linear();
    LinearRgba::rgb(c.red * strength, c.green * strength, c.blue * strength)
}

/// Meshes and sprites every landing shares. Built once, lazily.
pub struct ImpactAssets {
    quad: Handle<Mesh>,
    star: Handle<Image>,
    dot: Handle<Image>,
    ring: Handle<Mesh>,
    splinter: Handle<Mesh>,
    chip: Handle<Mesh>,
    drop: Handle<Mesh>,
    blot: Handle<Mesh>,
}

impl ImpactAssets {
    fn build(meshes: &mut Assets<Mesh>, images: &mut Assets<Image>) -> Self {
        Self {
            quad: meshes.add(Rectangle::new(1.0, 1.0)),
            star: images.add(star_flash_texture()),
            dot: images.add(soft_dot_texture()),
            ring: meshes.add(build_arc_band(
                IMPACT_RING_SEGMENTS,
                TAU,
                IMPACT_RING_THICKNESS,
                false,
            )),
            // Length along local +Z, so it can be aimed with `from_rotation_arc`.
            splinter: meshes.add(Cuboid::new(0.22, 0.22, 1.0)),
            chip: meshes.add(
                Cone::new(0.45, 1.0)
                    .mesh()
                    .resolution(5)
                    .anchor(ConeAnchor::Base),
            ),
            drop: meshes.add(Sphere::new(1.0)),
            blot: meshes.add(Sphere::new(1.0)),
        }
    }

    fn mote_mesh(&self, kind: SprayKind) -> Handle<Mesh> {
        match kind {
            SprayKind::Splinter => self.splinter.clone(),
            SprayKind::Chip => self.chip.clone(),
            SprayKind::Drop => self.drop.clone(),
            SprayKind::Spark => self.quad.clone(),
        }
    }
}

/// Build the landing on a new [`SchoolImpact`].
///
/// The rig is posed at the victim's anchor with local +Z pointing back at the
/// caster and then follows the victim (see [`animate_school_impacts`]). Every
/// piece is a child, so the whole thing dies with the rig.
pub fn spawn_school_impacts(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<ImpactAssets>>,
    new_impacts: Query<(Entity, &SchoolImpact), Added<SchoolImpact>>,
    targets: Query<(&Transform, Option<&Pet>)>,
) {
    if new_impacts.is_empty() {
        return;
    }
    let assets = assets.get_or_insert_with(|| ImpactAssets::build(&mut meshes, &mut images));

    for (entity, impact) in new_impacts.iter() {
        let style = landing_style(impact.ability, impact.school);
        let size = impact_size(impact.magnitude, impact.is_crit);
        let at = targets
            .get(impact.target)
            .map(|(t, pet)| impact_origin(impact.anchor, t.translation, pet.is_some()))
            .unwrap_or(Vec3::ZERO);
        let seed = entity.index();

        let glow = |materials: &mut Assets<StandardMaterial>, texture: Option<Handle<Image>>| {
            materials.add(StandardMaterial {
                base_color: style.color,
                base_color_texture: texture.clone(),
                emissive: emissive_of(style.color, style.emissive),
                emissive_texture: texture,
                alpha_mode: AlphaMode::Add,
                cull_mode: None,
                double_sided: true,
                ..default()
            })
        };

        let mut parts: Vec<Entity> = Vec::new();

        // Blot first, so it sits behind the additive layers.
        if let Some((radius, _, alpha)) = style.blot {
            parts.push(
                commands
                    .spawn((
                        ImpactSprite {
                            role: ImpactRole::Blot,
                            radius: radius * size,
                        },
                        Mesh3d(assets.blot.clone()),
                        MeshMaterial3d(materials.add(StandardMaterial {
                            base_color: SHADOW_BLOT_COLOR.with_alpha(alpha),
                            // Blend, NOT Add — the whole point is to darken.
                            alpha_mode: AlphaMode::Blend,
                            perceptual_roughness: 0.7,
                            ..default()
                        })),
                        Transform::default(),
                        NotShadowCaster,
                    ))
                    .id(),
            );
        }

        parts.push(
            commands
                .spawn((
                    ImpactSprite {
                        role: ImpactRole::Flash,
                        radius: style.flash_radius * size,
                    },
                    Mesh3d(assets.quad.clone()),
                    MeshMaterial3d(glow(&mut materials, Some(assets.star.clone()))),
                    Transform::default(),
                    NotShadowCaster,
                ))
                .id(),
        );

        if let Some((radius, _)) = style.ring {
            parts.push(
                commands
                    .spawn((
                        ImpactSprite {
                            role: ImpactRole::Ring,
                            radius,
                        },
                        Mesh3d(assets.ring.clone()),
                        MeshMaterial3d(glow(&mut materials, None)),
                        Transform::default(),
                        NotShadowCaster,
                    ))
                    .id(),
            );
        }

        // One material for all of a landing's debris: the pieces fade by
        // shrinking, so nothing per-piece needs to be written.
        let mote_material = |materials: &mut Assets<StandardMaterial>,
                             kind: SprayKind,
                             additive: bool,
                             color: Color| {
            let texture = matches!(kind, SprayKind::Spark).then(|| assets.dot.clone());
            if additive {
                materials.add(StandardMaterial {
                    base_color: color,
                    base_color_texture: texture.clone(),
                    emissive: emissive_of(color, style.emissive),
                    emissive_texture: texture,
                    alpha_mode: AlphaMode::Add,
                    cull_mode: None,
                    double_sided: true,
                    ..default()
                })
            } else {
                materials.add(StandardMaterial {
                    base_color: color,
                    emissive: emissive_of(color, 0.35),
                    perceptual_roughness: 0.4,
                    ..default()
                })
            }
        };

        if let Some(spray) = style.spray {
            let material = mote_material(
                &mut materials,
                spray.kind,
                spray.additive,
                spray.color.unwrap_or(style.color),
            );
            let mesh = assets.mote_mesh(spray.kind);
            let n = spray_count(&spray, impact.is_crit);
            for i in 0..n {
                let dir = spray_direction(i, n, spray.lift, spray.back);
                let speed = spray.speed
                    * (0.55 + 0.45 * impact_jitter(seed ^ i.wrapping_mul(0x9E37_79B9)))
                    * (0.85 + 0.15 * size);
                let spin = TAU * (impact_jitter(seed.wrapping_add(i * 31 + 7)) - 0.5) * 2.0;
                parts.push(
                    commands
                        .spawn((
                            ImpactMote {
                                kind: spray.kind,
                                velocity: dir * speed,
                                gravity: spray.gravity,
                                spin,
                                age: 0.0,
                                life: spray.life,
                                radius: spray.radius,
                            },
                            Mesh3d(mesh.clone()),
                            MeshMaterial3d(material.clone()),
                            Transform::from_rotation(mote_rotation(spray.kind, dir)),
                            NotShadowCaster,
                        ))
                        .id(),
                );
            }
        }

        let smoulder_material = style.smoulder.map(|_| {
            mote_material(&mut materials, SprayKind::Spark, true, style.color)
        });

        commands.entity(entity).insert((
            Transform::from_translation(at).with_rotation(impact_rotation(impact.from)),
            Visibility::default(),
            ImpactRig {
                mote_mesh: assets.quad.clone(),
                smoulder_material,
                emit_carry: 0.0,
                emitted: 0,
            },
        ));
        commands.entity(entity).add_children(&parts);
    }
}

/// The rest pose of a piece of debris: its length axis along its launch line.
fn mote_rotation(kind: SprayKind, dir: Vec3) -> Quat {
    match kind {
        SprayKind::Splinter => Quat::from_rotation_arc(Vec3::Z, dir),
        SprayKind::Chip => Quat::from_rotation_arc(Vec3::Y, dir),
        SprayKind::Drop | SprayKind::Spark => Quat::IDENTITY,
    }
}

/// Drive every live landing, and retire it when it is spent.
pub fn animate_school_impacts(
    mut commands: Commands,
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut impacts: Query<(
        Entity,
        &mut SchoolImpact,
        &mut ImpactRig,
        &mut Transform,
        &Children,
    )>,
    // Read-only victim lookup, provably disjoint from the mutable part queries
    // below or Bevy rejects the set as B0001.
    targets: Query<
        (&Transform, Option<&Pet>),
        (
            With<Combatant>,
            Without<SchoolImpact>,
            Without<ImpactSprite>,
            Without<ImpactMote>,
        ),
    >,
    mut sprites: Query<
        (
            &ImpactSprite,
            &mut Transform,
            &MeshMaterial3d<StandardMaterial>,
        ),
        (Without<SchoolImpact>, Without<ImpactMote>),
    >,
    mut motes: Query<
        (&mut ImpactMote, &mut Transform),
        (Without<SchoolImpact>, Without<ImpactSprite>),
    >,
) {
    let dt = time.delta_secs();

    for (entity, mut impact, mut rig, mut transform, children) in impacts.iter_mut() {
        impact.age += dt;
        let age = impact.age;
        let style = landing_style(impact.ability, impact.school);
        if age >= style.life() {
            commands.entity(entity).despawn();
            continue;
        }
        // Attached: follow a victim that is still moving.
        if let Ok((target, pet)) = targets.get(impact.target) {
            transform.translation = impact_origin(impact.anchor, target.translation, pet.is_some());
        }

        // A smoulder keeps emitting for a while after the hit.
        if let (Some(smoulder), Some(material)) = (style.smoulder, rig.smoulder_material.clone()) {
            if age < smoulder.secs {
                rig.emit_carry += smoulder.rate * dt;
                while rig.emit_carry >= 1.0 {
                    rig.emit_carry -= 1.0;
                    let i = rig.emitted;
                    rig.emitted += 1;
                    let seed = entity.index().wrapping_add(i.wrapping_mul(0x85EB_CA6B));
                    let a = impact_jitter(seed) * TAU;
                    let r = 0.16 * impact_jitter(seed ^ 0x51ED);
                    let drift = 0.35 * (impact_jitter(seed ^ 0x27D4) - 0.5);
                    let mote = commands
                        .spawn((
                            ImpactMote {
                                kind: SprayKind::Spark,
                                velocity: Vec3::new(a.cos() * drift, smoulder.rise, a.sin() * drift),
                                gravity: 0.0,
                                spin: 0.0,
                                age: 0.0,
                                life: smoulder.life,
                                radius: smoulder.radius,
                            },
                            Mesh3d(rig.mote_mesh.clone()),
                            MeshMaterial3d(material.clone()),
                            Transform::from_translation(Vec3::new(a.cos() * r, 0.0, a.sin() * r)),
                            NotShadowCaster,
                        ))
                        .id();
                    commands.entity(entity).add_child(mote);
                }
            }
        }

        for child in children.iter() {
            if let Ok((sprite, mut part, material)) = sprites.get_mut(child) {
                let (span, scale, alpha) = match sprite.role {
                    ImpactRole::Flash => {
                        let k = (age / style.flash_secs).clamp(0.0, 1.0);
                        // Snaps open, then collapses — an expanding flash covers
                        // the debris exactly when it needs to be seen.
                        let open = (k / 0.25).clamp(0.0, 1.0);
                        (
                            style.flash_secs,
                            sprite.radius * 2.0 * (0.4 + 0.6 * open) * (1.0 - 0.5 * k),
                            1.0 - k,
                        )
                    }
                    ImpactRole::Ring => {
                        let secs = style.ring.map(|(_, s)| s).unwrap_or(style.flash_secs);
                        let k = (age / secs).clamp(0.0, 1.0);
                        // Fast out of the gate then easing off, as a shockwave
                        // loses speed.
                        (secs, sprite.radius * k.sqrt(), 1.0 - k)
                    }
                    ImpactRole::Blot => {
                        let (_, secs, alpha) = style.blot.unwrap_or((1.0, style.flash_secs, 1.0));
                        let k = (age / secs).clamp(0.0, 1.0);
                        (
                            secs,
                            sprite.radius * (0.45 + 0.55 * k.powf(0.4)),
                            (1.0 - k) * alpha,
                        )
                    }
                };
                if age > span {
                    part.scale = Vec3::ZERO;
                    continue;
                }
                part.scale = Vec3::splat(scale.max(1e-4));
                if let Some(material) = materials.get_mut(&material.0) {
                    material.base_color.set_alpha(alpha);
                }
            }

            if let Ok((mut mote, mut part)) = motes.get_mut(child) {
                mote.age += dt;
                if mote.age > mote.life {
                    part.scale = Vec3::ZERO;
                    continue;
                }
                mote.velocity.y -= mote.gravity * dt;
                let velocity = mote.velocity;
                part.translation += velocity * dt;
                let k = 1.0 - mote.age / mote.life;
                match mote.kind {
                    SprayKind::Splinter => {
                        // Keep the sliver along its line of flight as gravity
                        // bends it, and roll it about that line.
                        if velocity.length_squared() > 1e-6 {
                            let along = Quat::from_rotation_arc(Vec3::Z, velocity.normalize());
                            part.rotation = along * Quat::from_rotation_z(mote.spin * mote.age);
                        }
                        part.scale = Vec3::new(
                            (mote.radius * 0.25 * k.powf(0.5)).max(1e-4),
                            (mote.radius * 0.25 * k.powf(0.5)).max(1e-4),
                            (mote.radius * k.powf(0.3)).max(1e-4),
                        );
                    }
                    SprayKind::Chip => {
                        part.rotate_local_y(mote.spin * dt);
                        part.scale = Vec3::splat((mote.radius * k.powf(0.7)).max(1e-4));
                    }
                    SprayKind::Drop => {
                        part.scale = Vec3::splat((mote.radius * k.powf(0.5)).max(1e-4));
                    }
                    SprayKind::Spark => {
                        // A soft glow: swells slightly then dies.
                        let swell = 1.0 + 0.4 * (1.0 - k) * k * 4.0;
                        part.scale = Vec3::splat((mote.radius * 2.0 * swell * k.powf(0.6)).max(1e-4));
                    }
                }
            }
        }
    }
}

/// Turn the flat pieces of a landing to face the camera.
///
/// The flash, the ring and every spark are flat quads hanging off a rig that
/// is yawed toward the caster, so the rig's rotation has to be cancelled out
/// of each of them — the same correction the bolt sprites make.
pub fn billboard_school_impacts(
    camera: Query<
        &Transform,
        (
            With<Camera3d>,
            Without<SchoolImpact>,
            Without<ImpactSprite>,
            Without<ImpactMote>,
        ),
    >,
    rigs: Query<(&Transform, &Children), With<SchoolImpact>>,
    mut sprites: Query<
        (&ImpactSprite, &mut Transform),
        (Without<SchoolImpact>, Without<Camera3d>, Without<ImpactMote>),
    >,
    mut motes: Query<
        (&ImpactMote, &mut Transform),
        (Without<SchoolImpact>, Without<Camera3d>, Without<ImpactSprite>),
    >,
) {
    let Some(cam) = camera.iter().next() else {
        return;
    };
    for (rig, children) in rigs.iter() {
        let facing = rig.rotation.inverse() * cam.rotation;
        for child in children.iter() {
            if let Ok((sprite, mut part)) = sprites.get_mut(child) {
                if sprite.role == ImpactRole::Blot {
                    // A sphere has no facing to correct.
                    continue;
                }
                part.rotation = facing;
            }
            if let Ok((mote, mut part)) = motes.get_mut(child) {
                if mote.kind == SprayKind::Spark {
                    part.rotation = facing;
                }
            }
        }
    }
}
