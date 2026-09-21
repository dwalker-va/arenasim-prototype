//! Caster wand autos (graphical-only): the missile, and the rod that throws it.
//!
//! Until now a Mage, Priest or Warlock auto-attacking had NO visual on either
//! side — the sim logged "Wand Shot" and nothing happened on screen. The client
//! shows a full cycle, and it is entirely item-driven: the Shoot spell (5019)
//! carries no `SpellXSpellVisual` row at all, and the visuals hang off
//! `ItemDisplayInfo -> ItemRangedDisplayInfo` instead (research §2.1, §2.2).
//!
//! What the client does, and what is reproduced here:
//!
//! * **A visible, school-flavored missile.** All 158 era wands resolve to one
//!   of seven `CastSpellVisualID`s, each reusing a standard school bolt model
//!   (fire / shadow / arcane / nature / frost / lightning / holy) at Scale 1,
//!   aimed at `DestinationAttachment 34` — the chest (§2.3). The variation axis
//!   is the wand's ITEM ART; ArenaSim has no per-wand art, so the school is
//!   keyed by the caster's CLASS instead, which lands each caster on the school
//!   it already reads as.
//! * **Nothing at all on impact.** The seven wand SpellVisuals carry precast
//!   and cast event rows only — zero `StartEvent 6` impact rows, so no impact
//!   model, no impact sound, no commanded victim anim (§2.5). The victim's
//!   reaction is the generic flinch and nothing else; a bespoke wand impact
//!   burst would be un-Classic. `hit_reaction.rs` owns that flinch, and fires
//!   it for a wand hit exactly as for any other auto.
//! * **A rod held raised, flicked per shot.** Precast kit 372 loops
//!   `HoldThrown` (anim 111) between shots and each cast kit plays
//!   `AttackThrown` (anim 107) — the character holds the wand up, thrown-weapon
//!   style, and flicks it (§4). The raised hold is the rod's REST mount
//!   (`weapon_mount`, `mod.rs`); the flick is `swing_pose`'s `Wand` arm scaled
//!   to [`WAND_FLICK_SECS`].
//! * **No hand glow.** None of the wand kits carries an `EffectType 2`
//!   model-attach entry, on any of the seven visuals — so nothing is drawn at
//!   the caster's hands.
//!
//! Registered in `states/mod.rs` only; headless stays byte-identical.

use super::school_impact::impact_origin;
use crate::states::play_match::abilities::SpellSchool;
use crate::states::play_match::components::*;
use crate::states::play_match::match_config::CharacterClass;
use bevy::color::LinearRgba;
use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;

// --- The blessed spec -------------------------------------------------------

/// Missile flight speed, in yards per second.
///
/// NOT client data — `BaseMissileSpeed` is 0 on every one of the seven wand
/// missile rows, so the client's speed is a hardcoded constant we cannot read
/// (research §2.3, constraint 7). At `WAND_RANGE` (30 yd) this is a 1.0 s
/// flight, which is the longest the bolt is ever in the air.
pub const WAND_MISSILE_SPEED: f32 = 30.0;

/// Radius of the missile's glow core, in arena units. Both era exemplars are
/// small: `firebolt_missile_low.m2` has a ~1.4-unit vertex extent and
/// `deathcoil_missile.m2` ~0.4, reading at gameplay range as a colored streak
/// rather than a ball (research §2.4).
pub const WAND_CORE_RADIUS: f32 = 0.11;

/// Length of the trailing band, in arena units. Both exemplar models carry two
/// RIBBON emitters — the trail is the part that reads at range, which is why
/// it is much longer than the core is wide.
pub const WAND_TRAIL_LEN: f32 = 0.9;

/// Peak forward pitch of the caster's flick, in DEGREES (the bench's unit).
pub const WAND_FLICK_DEG: f32 = 25.0;

/// Total duration of one flick, in seconds — release, hold and follow-through
/// together. Well under an ordinary swing's 0.42 s: a wrist snap, not a chop.
pub const WAND_FLICK_SECS: f32 = 0.25;

/// The two shape invariants, checked at COMPILE time rather than in a test,
/// because both are claims about the constants themselves and a compile error
/// stops a drift at the edit that causes it.
///
/// * The flick is quicker than an ordinary swing. The client's contrast is
///   `AttackThrown` against a chop, and the shipped auto profile totals
///   0.12 + 0.05 + 0.25 = 0.42 s (`weapon_swing.rs`).
/// * The trail reads as a BAND, not a ball (house amendment). A trail no
///   longer than the core is wide is a sprite with extra steps.
const _: () = {
    assert!(WAND_FLICK_SECS < 0.42);
    assert!(WAND_TRAIL_LEN > WAND_CORE_RADIUS * 4.0);
};

/// Hard despawn backstop, in seconds. A missile normally despawns on arrival;
/// this only catches a bolt whose victim despawned mid-flight.
const WAND_MISSILE_TTL: f32 = 2.0;

/// How far in front of the caster's chest a bolt is launched when the caster
/// has no wand socket to launch from (a spawn path that skipped the weapon
/// models). Clears the body so the fallback never starts inside the capsule.
const WAND_FALLBACK_MUZZLE_FORWARD: f32 = 0.6;

/// How far along the rod's local +Y the tip sits, in the socket's frame —
/// where the bolt actually leaves from. Mirrors `mortal_strike`'s
/// `TRAIL_TIP_LOCAL` convention: the mount's own scale is applied on top by
/// the socket's `GlobalTransform`.
const WAND_TIP_LOCAL: f32 = 0.42;

/// The school a class's wand throws.
///
/// The client varies this by the wand's item art, over seven families; we have
/// no per-wand art, so the caster's class picks the school it already reads as.
/// Exhaustive — a new class must choose, rather than defaulting into someone
/// else's color.
///
/// The Shaman arm is currently INERT: since AS-138 the Shaman swings the mace
/// in its main hand (`AutoAttackKind::Melee`) and never fires a wand. It is
/// kept correct rather than removed so that re-arming a Shaman wand is a
/// loadout change and not a code change.
pub fn wand_school(class: CharacterClass) -> SpellSchool {
    match class {
        CharacterClass::Mage => SpellSchool::Arcane,
        CharacterClass::Priest => SpellSchool::Holy,
        CharacterClass::Warlock => SpellSchool::Shadow,
        // Nature is the school of the Shaman's own Lightning Bolt.
        CharacterClass::Shaman => SpellSchool::Nature,
        // No other class can hold a wand — `weapon_proficiency` declares every
        // one of them Untrained — so these arms are unreachable in practice.
        // Frost is the neutral bolt color rather than a claim about the class.
        CharacterClass::Warrior
        | CharacterClass::Rogue
        | CharacterClass::Paladin
        | CharacterClass::Hunter => SpellSchool::Frost,
    }
}

// --- Runtime components (graphical-only) ------------------------------------

/// One wand bolt in flight. Purely cosmetic: the sim resolved this hit before
/// the bolt was spawned, so the bolt's arrival carries no damage and its
/// despawn triggers nothing.
#[derive(Component)]
pub struct WandMissile {
    /// Where the bolt is flying, snapshotted at the shot — the sim already
    /// resolved against that position, so it must not chase a victim who has
    /// moved on.
    to: Vec3,
    ttl: f32,
}

// --- Spawn ------------------------------------------------------------------

/// Launch one cosmetic wand bolt from `muzzle` toward `to`.
///
/// Called from `hit_reaction::consume_hit_reactions` at the landed shot, the
/// way `instant_ability.rs` calls into `mortal_strike.rs` — one consumer of
/// the sim's landed-attack marker, routing to the effect the swing's kind
/// selects.
pub fn spawn_wand_missile(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    school: SpellSchool,
    muzzle: Vec3,
    to: Vec3,
) {
    let dir = (to - muzzle).normalize_or_zero();
    let color = school.color();
    let rgba = color.to_linear();

    let core_material = materials.add(StandardMaterial {
        base_color: color,
        // Bright enough to read as a light source at 30 yards; the bolt is the
        // only thing this effect draws, so it carries the whole shot.
        emissive: LinearRgba::new(rgba.red * 3.0, rgba.green * 3.0, rgba.blue * 3.0, 1.0),
        alpha_mode: AlphaMode::Add,
        unlit: true,
        ..default()
    });
    let trail_material = materials.add(StandardMaterial {
        base_color: color,
        emissive: LinearRgba::new(rgba.red * 1.2, rgba.green * 1.2, rgba.blue * 1.2, 1.0),
        alpha_mode: AlphaMode::Add,
        unlit: true,
        ..default()
    });

    // The trail is a BAND stretched along the flight axis, never a round
    // sprite (house amendment) — it is a child of the core so it inherits the
    // aim without a second orientation to keep in sync, and it sits entirely
    // BEHIND the core along local -Z.
    let trail = commands
        .spawn((
            Mesh3d(meshes.add(Cuboid::new(
                WAND_CORE_RADIUS * 0.9,
                WAND_CORE_RADIUS * 0.9,
                WAND_TRAIL_LEN,
            ))),
            MeshMaterial3d(trail_material),
            Transform::from_xyz(0.0, 0.0, -WAND_TRAIL_LEN * 0.5),
            NotShadowCaster,
        ))
        .id();

    let core = commands
        .spawn((
            WandMissile {
                to,
                ttl: WAND_MISSILE_TTL,
            },
            Mesh3d(meshes.add(Sphere::new(WAND_CORE_RADIUS))),
            MeshMaterial3d(core_material),
            Transform::from_translation(muzzle)
                .with_rotation(Quat::from_rotation_arc(Vec3::Z, dir)),
            NotShadowCaster,
            PlayMatchEntity,
        ))
        .id();
    commands.entity(core).add_child(trail);
}

/// Where a caster's bolt leaves from: the tip of the wand rod it is holding,
/// or — for a caster with no wand socket — a point in front of its chest.
///
/// `sockets` is every live weapon socket; only the shooter's own main-hand
/// wand is considered, so a Paladin's shield or a Rogue's off-hand can never
/// be mistaken for a muzzle.
pub fn wand_muzzle(
    shooter: Entity,
    shooter_pos: Vec3,
    aim: Vec3,
    sockets: &Query<(&WeaponSocket, &GlobalTransform)>,
) -> Vec3 {
    let tip = sockets.iter().find_map(|(socket, global)| {
        (socket.owner == shooter
            && socket.hand == WeaponHand::Main
            && socket.kind == WeaponKind::Wand)
            .then(|| global.transform_point(Vec3::Y * WAND_TIP_LOCAL))
    });
    tip.unwrap_or_else(|| {
        let chest = impact_origin(ImpactAnchor::Chest, shooter_pos, false);
        let forward = (aim - shooter_pos).with_y(0.0).normalize_or_zero();
        chest + forward * WAND_FALLBACK_MUZZLE_FORWARD
    })
}

// --- Update / cleanup -------------------------------------------------------

/// Update (graphical-only): fly each bolt toward its snapshotted target point,
/// and despawn it on arrival.
///
/// Same shape as `update_cosmetic_arrows`, and deliberately so: both are
/// unattached cosmetic projectiles chasing a fixed point with a TTL backstop.
/// The bolt despawns on ARRIVAL rather than TTLing out in the victim's chest,
/// which is the gap the Hunter's arrow still has.
pub fn update_wand_missiles(
    mut commands: Commands,
    time: Res<Time>,
    mut missiles: Query<(Entity, &mut WandMissile, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (entity, mut missile, mut transform) in missiles.iter_mut() {
        missile.ttl -= dt;
        let to_target = missile.to - transform.translation;
        let step = WAND_MISSILE_SPEED * dt;
        if missile.ttl <= 0.0 || to_target.length() <= step {
            commands.entity(entity).despawn();
            continue;
        }
        let dir = to_target.normalize_or_zero();
        transform.translation += dir * step;
        transform.rotation = Quat::from_rotation_arc(Vec3::Z, dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_wand_class_throws_its_own_school() {
        // The three classes that actually fire a wand today, each on the
        // school it already reads as. Asserted by NAME, not as a set size, so
        // a future re-mapping has to say what it is doing.
        assert_eq!(wand_school(CharacterClass::Mage), SpellSchool::Arcane);
        assert_eq!(wand_school(CharacterClass::Priest), SpellSchool::Holy);
        assert_eq!(wand_school(CharacterClass::Warlock), SpellSchool::Shadow);
        assert_eq!(wand_school(CharacterClass::Shaman), SpellSchool::Nature);
    }

    #[test]
    fn a_bolt_crosses_wand_range_in_about_a_second() {
        use crate::states::play_match::constants::WAND_RANGE;
        let flight = WAND_RANGE / WAND_MISSILE_SPEED;
        assert!(
            (0.5..=1.5).contains(&flight),
            "max wand flight is {flight}s — a bolt that hangs in the air \
             reads as a lob, and one that teleports reads as nothing"
        );
    }
}
