//! Which enemy crowd control can reach a healer, and where (AS-362).
//!
//! A healer's position decides which enemy crowd control can land on it: a
//! Psychic Scream reaches 8yd round the Priest, a Fear or a Polymorph 30yd
//! from its caster with sight, a Freezing Trap 30yd plus its trigger radius.
//! This module is the ONE definition of that set. Positioning reads it through
//! [`CcField`]; nothing re-derives a threat.
//!
//! **A CC threat** is a pair (enemy E, ability A), every part read from data:
//!
//! - A is in E's kit (`AbilityDefinitions`: the class's own abilities, or a
//!   pet's by its `pet_type`), and applies an aura that HOLDS — stops its
//!   victim acting ([`holds`]: stun, fear, polymorph, incapacitate, cyclone).
//!   Roots are out: a rooted healer still heals.
//! - **Reach** is A's `range`, plus the trap trigger radius for an ability
//!   thrown at the ground ([`Aim::Ground`]).
//! - **Ready**: E could start A within the horizon ([`could_cast`]) — the cast
//!   gate's own checks (`cast_guard::own_state_ok`), asked from E's side.
//! - **Seen**: E is in the deciding unit's `ctx.combatants`, so an enemy it
//!   cannot perceive contributes nothing.
//! - **Sight**: a spell aimed at a unit ([`Aim::Unit`]) covers a spot only
//!   where E can see it, because the cast gate refuses it otherwise; an area
//!   round E or a trap covers its disc regardless. So against a caster, cover
//!   and distance are two answers to the same threat.
//! - **Weight**: A's duration times the healer's diminishing-returns
//!   multiplier for its category — the control-seconds a landing would cost.
//!   A healer immune to it (DR-immune, damage-immune, or fear-immune to a
//!   fear) is not threatened at all.
//!
//! **The disc** a healer keeps out of is the reach plus a margin
//! (`shared.cc_reach_margin`). It is a Schmitt pair: a spot counts as covered
//! inside `reach + margin`, but a healer already outside `reach + margin / 2`
//! counts as clear where it stands ([`Disc::Inner`]), so it is not moved off
//! the rim by a caster's every step.
//!
//! The set is general on purpose: AS-362's consumers are the `TeamPlan` team
//! solve (`team_solve::C_CC_REACH`) and the FREE formation reach repair
//! (`formation::reach_repaired_point`), and later positioning work reads the
//! same field. `Legacy` never builds one.

use bevy::prelude::*;

use super::{holds, CombatContext, CombatantInfo};
use crate::states::play_match::abilities::{silence_holds, AbilityType};
use crate::states::play_match::ability_config::{AbilityConfig, AbilityDefinitions};
use crate::states::play_match::components::{ActiveAuras, AuraType, DRCategory};
use crate::states::play_match::constants::{DR_MULTIPLIERS, TRAP_TRIGGER_RADIUS};
use crate::states::play_match::is_spell_school_locked;
use crate::states::play_match::map_geometry::{has_line_of_sight, ObstacleVolume, EYE_HEIGHT};

/// How an ability is aimed — what decides where it can land.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Aim {
    /// At a unit: it needs range and line of sight from its caster.
    Unit,
    /// On its caster, or round it: a self-cast, or an area centred on the
    /// caster that catches everyone inside its range, seen or not.
    Caster,
    /// At a point on the ground, where it then catches whoever comes within
    /// its trigger radius.
    Ground,
}

/// How `ability` is aimed. Exhaustive, so a new ability does not compile until
/// it answers: the one judgement about an ability that no config field
/// carries.
pub const fn aim(ability: AbilityType) -> Aim {
    use AbilityType as A;
    match ability {
        A::FreezingTrap | A::FrostTrap => Aim::Ground,
        A::FrostNova
        | A::PsychicScream
        | A::BerserkerRage
        | A::BattleShout
        | A::DemoralizingShout
        | A::CommandingShout
        | A::IceBarrier
        | A::DivineShield
        | A::DevotionAura
        | A::ShadowResistanceAura
        | A::ConcentrationAura
        | A::FrostArmor
        | A::MageArmorSpell
        | A::MoltenArmor
        | A::Disengage
        | A::Flare
        | A::AirTotem
        | A::WaterTotem
        | A::EarthTotem
        | A::FireTotem
        | A::TravelForm
        | A::LifeTap => Aim::Caster,
        A::Frostbolt
        | A::FlashHeal
        | A::HeroicStrike
        | A::Ambush
        | A::CheapShot
        | A::MindBlast
        | A::SinisterStrike
        | A::Charge
        | A::KidneyShot
        | A::PowerWordFortitude
        | A::Rend
        | A::MortalStrike
        | A::Pummel
        | A::Kick
        | A::CripplingPoison
        | A::DeadlyPoison
        | A::WoundPoison
        | A::MindNumbingPoison
        | A::Corruption
        | A::Shadowbolt
        | A::Fear
        | A::Immolate
        | A::DrainLife
        | A::CurseOfAgony
        | A::CurseOfWeakness
        | A::CurseOfTongues
        | A::UnstableAffliction
        | A::DeathCoil
        | A::ArcaneIntellect
        | A::PowerWordShield
        | A::Polymorph
        | A::DispelMagic
        | A::ManaBurn
        | A::ShadowWordPain
        | A::FlashOfLight
        | A::HolyLight
        | A::HolyShock
        | A::HammerOfJustice
        | A::PaladinCleanse
        | A::SpellLock
        | A::DevourMagic
        | A::AimedShot
        | A::ArcaneShot
        | A::ConcussiveShot
        | A::SerpentSting
        | A::SpiderWeb
        | A::BoarCharge
        | A::MastersCall
        | A::LightningBolt
        | A::FrostShock
        | A::LesserHealingWave
        | A::Purge
        | A::WindShear
        | A::Rejuvenation
        | A::Lifebloom
        | A::Swiftmend
        | A::Moonfire
        | A::MarkOfTheWild
        | A::Innervate
        | A::EntanglingRoots
        | A::Cyclone => Aim::Unit,
    }
}

/// The hard crowd control `def` applies — an aura that HOLDS its victim
/// ([`holds`]) — or `None`.
pub fn held_by(def: &AbilityConfig) -> Option<AuraType> {
    def.applies_aura
        .as_ref()
        .map(|a| a.aura_type)
        .filter(|t| holds(*t))
}

/// How far from its caster `ability` can land its crowd control: its range,
/// and for one thrown at the ground the radius it then springs within.
pub fn reach(ability: AbilityType, def: &AbilityConfig) -> f32 {
    match aim(ability) {
        Aim::Ground => def.range + TRAP_TRIGGER_RADIUS,
        Aim::Unit | Aim::Caster => def.range,
    }
}

/// Whether `enemy` could START `ability` within `horizon` seconds — the cast
/// gate's own-state checks (`cast_guard::own_state_ok`), in its order, asked
/// from the enemy's side of the snapshot: not shapeshifted, not held in hard
/// crowd control, its school not locked, not silenced for a spell a silence
/// stops, the cooldown (its category's included — `start_cooldown` writes
/// every member) at most `horizon`, the resource to pay for it, and stealth
/// for an opener that needs it. Range and sight are the disc's business, not
/// this.
///
/// Conservative where the sim has state this does not read: a Rogue's Kidney
/// Shot counts as ready on energy and cooldown alone, whatever its combo
/// points — the healer gives ground to a finisher the Rogue may not yet have.
pub fn could_cast(
    ctx: &CombatContext,
    enemy: &CombatantInfo,
    ability: AbilityType,
    def: &AbilityConfig,
    horizon: f32,
) -> bool {
    let auras = ctx.active_auras.get(&enemy.entity).map(Vec::as_slice);
    let wrapped = auras.map(|a| ActiveAuras { auras: a.to_vec() });
    if wrapped.as_ref().is_some_and(ActiveAuras::is_shapeshifted) {
        return false;
    }
    if auras.is_some_and(|a| a.iter().any(|x| holds(x.effect_type))) {
        return false;
    }
    if is_spell_school_locked(def.spell_school, wrapped.as_ref()) {
        return false;
    }
    if def.silence_stops() && silence_holds(enemy.class.uses_mana(), auras) {
        return false;
    }
    let cooldown = ctx
        .ability_cooldowns
        .get(&enemy.entity)
        .and_then(|cds| cds.get(&ability))
        .copied()
        .unwrap_or(0.0);
    if cooldown > horizon {
        return false;
    }
    if enemy.current_mana < def.mana_cost {
        return false;
    }
    !(def.requires_stealth && !enemy.stealthed)
}

/// The control-seconds a landing of `def`'s hold on `healer` would cost: its
/// duration times the healer's diminishing-returns multiplier for its
/// category. `None` when it would not land at all: the healer is DR-immune to
/// the category, immune to damage (Divine Shield, Cyclone — which also blocks
/// crowd control), or immune to fear and it is a fear.
pub fn landing_weight(ctx: &CombatContext, healer: Entity, def: &AbilityConfig) -> Option<f32> {
    let effect = def.applies_aura.as_ref()?;
    if ctx.entity_is_immune(healer) {
        return None;
    }
    if effect.aura_type == AuraType::Fear
        && ctx
            .active_auras
            .get(&healer)
            .is_some_and(|a| a.iter().any(|x| x.effect_type == AuraType::FearImmunity))
    {
        return None;
    }
    let multiplier = match effect
        .dr_category
        .or_else(|| DRCategory::from_aura_type(&effect.aura_type))
    {
        Some(category) => {
            let level = ctx
                .dr_trackers
                .get(&healer)
                .map_or(0, |t| t.level(category));
            DR_MULTIPLIERS[usize::from(level.min(3))]
        }
        None => 1.0,
    };
    (multiplier > 0.0).then_some(effect.duration * multiplier)
}

/// One enemy crowd control that can reach the healer: whose, which, the disc
/// it covers, and what a landing costs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CcThreat {
    pub source: Entity,
    pub ability: AbilityType,
    /// The disc's centre: its caster (a pet's ability centres on the pet).
    pub center: Vec2,
    /// How far from `center` it lands ([`reach`]), before the margin.
    pub reach: f32,
    /// Whether it covers only the spots its caster can see ([`Aim::Unit`]).
    pub needs_sight: bool,
    /// The control-seconds a landing would cost ([`landing_weight`]).
    pub weight: f32,
}

/// Every enemy crowd control READY to reach `healer`, as `healer`'s context
/// perceives the arena: the threats a [`CcField`] is built from. In the
/// deterministic order of `ctx.combatants`, then of each kit.
pub fn threats(
    ctx: &CombatContext,
    abilities: &AbilityDefinitions,
    healer: Entity,
    horizon: f32,
) -> Vec<CcThreat> {
    let Some(team) = ctx.combatants.get(&healer).map(|h| h.team) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for enemy in ctx
        .combatants
        .values()
        .filter(|c| c.is_alive && c.team != team)
    {
        let kit: Vec<AbilityType> = if enemy.is_pet {
            abilities
                .pet_abilities_for_class(enemy.class)
                .into_iter()
                .filter(|(pet, _)| Some(*pet) == enemy.pet_type)
                .flat_map(|(_, kit)| kit)
                .collect()
        } else {
            abilities.own_abilities_for_class(enemy.class)
        };
        for ability in kit {
            let Some(def) = abilities.get(&ability) else {
                continue;
            };
            if held_by(def).is_none() || !could_cast(ctx, enemy, ability, def, horizon) {
                continue;
            }
            let Some(weight) = landing_weight(ctx, healer, def) else {
                continue;
            };
            out.push(CcThreat {
                source: enemy.entity,
                ability,
                center: Vec2::new(enemy.position.x, enemy.position.z),
                reach: reach(ability, def),
                needs_sight: aim(ability) == Aim::Unit,
                weight,
            });
        }
    }
    out
}

/// Which side of the Schmitt pair a spot is judged by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disc {
    /// `reach + margin`: whether a spot a healer would walk to is covered.
    Outer,
    /// `reach + margin / 2`: whether the spot a healer already stands on is —
    /// a healer outside it is not moved.
    Inner,
}

/// The crowd control that can reach one healer, and where: the three queries
/// every consumer reads. Empty (the default) covers nothing, so a world built
/// without one — every `Legacy` decision, every solve test — is unchanged.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CcField {
    pub threats: Vec<CcThreat>,
    /// `shared.cc_reach_margin`.
    pub margin: f32,
}

impl CcField {
    /// The field for `healer`, from its own context.
    pub fn for_healer(
        ctx: &CombatContext,
        abilities: &AbilityDefinitions,
        healer: Entity,
        horizon: f32,
        margin: f32,
    ) -> Self {
        CcField {
            threats: threats(ctx, abilities, healer, horizon),
            margin,
        }
    }

    /// This field less every threat whose outer disc reaches `radius` or
    /// beyond: the discs a healer can step out of while still standing inside
    /// `radius` of their caster.
    pub fn within(mut self, radius: f32) -> Self {
        let margin = self.margin;
        self.threats.retain(|t| t.reach + margin < radius);
        self
    }

    /// The nearest spot to `p` out of every disc covering it: for each disc
    /// covering `p` (the inner disc — a healer already outside it stays), the
    /// point just past its outer rim on the ray from its caster through `p`,
    /// kept when no disc covers it and `allowed` admits it. `None` when `p` is
    /// covered by none, or no such spot exists.
    pub fn nearest_exit(
        &self,
        obstacles: &[ObstacleVolume],
        p: Vec2,
        allowed: impl Fn(Vec2) -> bool,
    ) -> Option<Vec2> {
        self.covering(obstacles, p, Disc::Inner)
            .filter_map(|t| {
                let away = (p - t.center).normalize_or_zero();
                (away != Vec2::ZERO).then(|| t.center + away * (self.radius(t, Disc::Outer) + 0.25))
            })
            .filter(|c| self.covering(obstacles, *c, Disc::Outer).next().is_none() && allowed(*c))
            .min_by(|a, b| a.distance(p).total_cmp(&b.distance(p)))
    }

    /// This field less every threat from `source`.
    pub fn without_source(mut self, source: Option<Entity>) -> Self {
        self.threats.retain(|t| Some(t.source) != source);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.threats.is_empty()
    }

    /// The radius of `threat`'s disc on `disc`'s side of the pair.
    pub fn radius(&self, threat: &CcThreat, disc: Disc) -> f32 {
        threat.reach
            + match disc {
                Disc::Outer => self.margin,
                Disc::Inner => self.margin * 0.5,
            }
    }

    /// The threats whose disc covers `p`: within its radius, and seen by its
    /// caster when it needs sight.
    pub fn covering<'a>(
        &'a self,
        obstacles: &'a [ObstacleVolume],
        p: Vec2,
        disc: Disc,
    ) -> impl Iterator<Item = &'a CcThreat> + 'a {
        self.threats.iter().filter(move |t| {
            p.distance(t.center) < self.radius(t, disc)
                && (!t.needs_sight || sees(obstacles, t.center, p))
        })
    }

    /// The summed weight (control-seconds) of the threats covering `p`; zero
    /// where none does.
    pub fn covered(&self, obstacles: &[ObstacleVolume], p: Vec2, disc: Disc) -> f32 {
        self.covering(obstacles, p, disc).map(|t| t.weight).sum()
    }

    /// How many yards `p` stands inside the discs covering it, summed: the
    /// gradient out. Zero where none does.
    pub fn deficit(&self, obstacles: &[ObstacleVolume], p: Vec2, disc: Disc) -> f32 {
        self.covering(obstacles, p, disc)
            .map(|t| self.radius(t, disc) - p.distance(t.center))
            .sum()
    }
}

fn sees(obstacles: &[ObstacleVolume], a: Vec2, b: Vec2) -> bool {
    has_line_of_sight(
        obstacles,
        Vec3::new(a.x, EYE_HEIGHT, a.y),
        Vec3::new(b.x, EYE_HEIGHT, b.y),
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::states::match_config::CharacterClass::{self, Hunter, Paladin, Priest, Rogue};
    use crate::states::play_match::ability_config::load_ability_definitions;
    use crate::states::play_match::ai_profile::AiProfile;
    use crate::states::play_match::class_ai::reach_fixture::{pillar, unit};
    use crate::states::play_match::components::{Aura, DRTracker, PetType};

    const HEALER: Vec3 = Vec3::new(0.0, 1.0, 0.0);

    struct Fixture {
        healer: Entity,
        enemy: Entity,
        roster: BTreeMap<Entity, CombatantInfo>,
        auras: BTreeMap<Entity, Vec<Aura>>,
        dr: BTreeMap<Entity, DRTracker>,
        cds: BTreeMap<Entity, BTreeMap<AbilityType, f32>>,
        obstacles: Vec<ObstacleVolume>,
    }

    /// A Priest healer at the origin, and one enemy of `class` at `pos`.
    fn fixture(class: CharacterClass, pos: Vec3) -> Fixture {
        let mut world = World::new();
        let healer = world.spawn_empty().id();
        let enemy = world.spawn_empty().id();
        let roster = [
            unit(healer, 1, Priest, HEALER, 1.0),
            unit(enemy, 2, class, pos, 1.0),
        ]
        .into_iter()
        .map(|u| (u.entity, u))
        .collect();
        Fixture {
            healer,
            enemy,
            roster,
            auras: BTreeMap::new(),
            dr: BTreeMap::new(),
            cds: BTreeMap::new(),
            obstacles: Vec::new(),
        }
    }

    impl Fixture {
        fn ctx(&self) -> CombatContext<'_> {
            CombatContext::new(
                self.healer,
                1,
                &self.roster,
                &self.auras,
                &self.dr,
                &self.cds,
                &self.obstacles,
                Default::default(),
                AiProfile::TeamPlan,
            )
        }

        fn threats(&self) -> Vec<AbilityType> {
            let abilities = load_ability_definitions().expect("abilities.ron loads");
            threats(&self.ctx(), &abilities, self.healer, 1.5)
                .into_iter()
                .map(|t| t.ability)
                .collect()
        }

        fn field(&self) -> CcField {
            let abilities = load_ability_definitions().expect("abilities.ron loads");
            CcField::for_healer(&self.ctx(), &abilities, self.healer, 1.5, 3.0)
        }

        fn aura(&mut self, who: Entity, effect: AuraType) -> &mut Self {
            self.auras.entry(who).or_default().push(Aura {
                effect_type: effect,
                duration: 10.0,
                ..Default::default()
            });
            self
        }
    }

    /// Every hard-CC ability in the book answers to the rule, and nothing
    /// else does: the set is what the config says, not a list.
    #[test]
    fn the_threat_kits_are_the_holding_abilities_in_the_book() {
        let abilities = load_ability_definitions().expect("abilities.ron loads");
        let mut held: Vec<AbilityType> = abilities
            .iter()
            .filter(|(_, d)| held_by(d).is_some())
            .map(|(a, _)| *a)
            .collect();
        held.sort();
        use AbilityType as A;
        assert_eq!(
            held,
            vec![
                A::CheapShot,
                A::KidneyShot,
                A::Fear,
                A::DeathCoil,
                A::Polymorph,
                A::PsychicScream,
                A::HammerOfJustice,
                A::FreezingTrap,
                A::BoarCharge,
                A::Cyclone,
            ]
        );
    }

    #[test]
    fn a_ready_caster_threatens_and_a_cooldown_or_mana_shortfall_does_not() {
        let mut f = fixture(Priest, Vec3::new(5.0, 1.0, 0.0));
        assert_eq!(f.threats(), vec![AbilityType::PsychicScream]);
        // On cooldown beyond the horizon: not ready.
        f.cds
            .entry(f.enemy)
            .or_default()
            .insert(AbilityType::PsychicScream, 10.0);
        assert!(f.threats().is_empty());
        // Coming off within the horizon: ready.
        f.cds
            .entry(f.enemy)
            .or_default()
            .insert(AbilityType::PsychicScream, 1.0);
        assert_eq!(f.threats(), vec![AbilityType::PsychicScream]);
        // Short of mana.
        f.cds.clear();
        f.roster.get_mut(&f.enemy).unwrap().current_mana = 10.0;
        assert!(f.threats().is_empty());
    }

    #[test]
    fn a_silenced_held_or_locked_out_enemy_threatens_nothing() {
        let enemy = |f: &Fixture| f.enemy;
        let mut f = fixture(Priest, Vec3::new(5.0, 1.0, 0.0));
        let e = enemy(&f);
        f.aura(e, AuraType::Silence);
        assert!(f.threats().is_empty(), "silenced");
        let mut f = fixture(Priest, Vec3::new(5.0, 1.0, 0.0));
        let e = enemy(&f);
        f.aura(e, AuraType::Stun);
        assert!(f.threats().is_empty(), "stunned");
        // Rooted still casts.
        let mut f = fixture(Priest, Vec3::new(5.0, 1.0, 0.0));
        let e = enemy(&f);
        f.aura(e, AuraType::Root);
        assert_eq!(f.threats(), vec![AbilityType::PsychicScream], "rooted");
    }

    #[test]
    fn a_shifted_druid_threatens_nothing_until_it_shifts_back() {
        use crate::states::match_config::CharacterClass::Druid;
        let mut f = fixture(Druid, Vec3::new(10.0, 1.0, 0.0));
        assert_eq!(f.threats(), vec![AbilityType::Cyclone]);
        let e = f.enemy;
        f.aura(e, AuraType::TravelForm);
        assert!(f.threats().is_empty());
    }

    #[test]
    fn a_damage_immune_healer_is_not_threatened() {
        let mut f = fixture(Priest, Vec3::new(5.0, 1.0, 0.0));
        let h = f.healer;
        f.aura(h, AuraType::DamageImmunity);
        assert!(f.threats().is_empty());
    }

    #[test]
    fn a_school_lockout_stops_the_school_it_locks() {
        let mut f = fixture(Priest, Vec3::new(5.0, 1.0, 0.0));
        let e = f.enemy;
        let shadow = crate::states::play_match::abilities::SpellSchool::Shadow;
        f.auras.entry(e).or_default().push(Aura {
            effect_type: AuraType::SpellSchoolLockout,
            duration: 4.0,
            magnitude: shadow.to_lockout_magnitude(),
            ..Default::default()
        });
        assert!(f.threats().is_empty());
    }

    #[test]
    fn a_dr_immune_or_fear_immune_healer_is_not_threatened() {
        let mut f = fixture(Priest, Vec3::new(5.0, 1.0, 0.0));
        let mut tracker = DRTracker::default();
        for _ in 0..3 {
            tracker.apply(DRCategory::Fears);
        }
        f.dr.insert(f.healer, tracker);
        assert!(f.threats().is_empty(), "DR-immune to fear");
        let mut f = fixture(Priest, Vec3::new(5.0, 1.0, 0.0));
        let h = f.healer;
        f.aura(h, AuraType::FearImmunity);
        assert!(f.threats().is_empty(), "fear-immune");
        // Diminished, not immune: threatened at half weight.
        let mut f = fixture(Priest, Vec3::new(5.0, 1.0, 0.0));
        let mut tracker = DRTracker::default();
        tracker.apply(DRCategory::Fears);
        f.dr.insert(f.healer, tracker);
        let field = f.field();
        assert_eq!(field.threats.len(), 1);
        assert!((field.threats[0].weight - 4.0).abs() < 1e-4, "{field:?}");
    }

    #[test]
    fn an_unseen_stealthed_enemy_threatens_nothing_and_an_opener_needs_stealth() {
        let mut f = fixture(Rogue, Vec3::new(2.0, 1.0, 0.0));
        // In sight and out of stealth: Kidney Shot, never Cheap Shot.
        assert_eq!(f.threats(), vec![AbilityType::KidneyShot]);
        // In stealth and unseen: not in the healer's perception at all.
        f.roster.get_mut(&f.enemy).unwrap().stealthed = true;
        assert!(f.threats().is_empty());
    }

    #[test]
    fn a_pets_ability_centres_on_the_pet() {
        let mut f = fixture(Hunter, Vec3::new(20.0, 1.0, 0.0));
        let boar = Entity::from_raw(99);
        let mut pet = unit(boar, 2, Hunter, Vec3::new(-10.0, 1.0, 0.0), 1.0);
        pet.is_pet = true;
        pet.slot = crate::states::play_match::constants::PET_SLOT_BASE;
        pet.pet_type = Some(PetType::Boar);
        f.roster.insert(boar, pet);
        let field = f.field();
        let charge = field
            .threats
            .iter()
            .find(|t| t.ability == AbilityType::BoarCharge)
            .unwrap_or_else(|| panic!("the Boar's Charge threatens: {field:?}"));
        assert_eq!(charge.source, boar);
        assert_eq!(charge.center, Vec2::new(-10.0, 0.0));
        // And the Hunter's trap reaches its throw plus its trigger radius.
        let trap = field
            .threats
            .iter()
            .find(|t| t.ability == AbilityType::FreezingTrap)
            .expect("the trap threatens");
        assert_eq!(trap.reach, 30.0 + TRAP_TRIGGER_RADIUS);
        assert!(!trap.needs_sight);
    }

    #[test]
    fn a_targeted_cc_covers_only_what_its_caster_sees_and_an_area_covers_its_disc() {
        // A Paladin's Hammer of Justice behind the fixture pillar.
        let mut f = fixture(Paladin, Vec3::new(11.0, 1.0, 0.0));
        let field = f.field();
        let behind = Vec2::new(14.0, 0.0);
        let open = Vec2::new(11.0, 6.0);
        assert!(field.covered(&[], behind, Disc::Outer) > 0.0);
        f.obstacles = pillar();
        let blocked = Vec2::new(4.0, 0.0);
        assert_eq!(field.covered(&f.obstacles, blocked, Disc::Outer), 0.0);
        assert!(field.covered(&f.obstacles, open, Disc::Outer) > 0.0);
        // Psychic Scream covers its disc through the pillar.
        let mut f = fixture(Priest, Vec3::new(12.0, 1.0, 0.0));
        f.obstacles = pillar();
        let field = f.field();
        assert!(field.covered(&f.obstacles, Vec2::new(5.0, 0.0), Disc::Outer) > 0.0);
    }

    fn disc(source: u32, x: f32, reach: f32) -> CcThreat {
        CcThreat {
            source: Entity::from_raw(source),
            ability: AbilityType::Fear,
            center: Vec2::new(x, 0.0),
            reach,
            needs_sight: false,
            weight: 8.0,
        }
    }

    #[test]
    fn within_keeps_only_discs_inside_the_radius_and_without_source_drops_one() {
        let field = CcField {
            threats: vec![disc(1, 0.0, 8.0), disc(2, 0.0, 26.0), disc(3, 0.0, 27.0)],
            margin: 3.0,
        };
        let kept: Vec<u32> = field
            .clone()
            .within(30.0)
            .threats
            .iter()
            .map(|t| t.source.index())
            .collect();
        assert_eq!(kept, vec![1, 2]);
        let kept: Vec<u32> = field
            .without_source(Some(Entity::from_raw(2)))
            .threats
            .iter()
            .map(|t| t.source.index())
            .collect();
        assert_eq!(kept, vec![1, 3]);
    }

    /// The nearest exit is just past the outer rim, away from the caster; a
    /// spot another disc covers, or `allowed` refuses, is no exit; a spot
    /// outside the inner disc needs none.
    #[test]
    fn the_nearest_exit_steps_just_past_the_rim() {
        let field = CcField {
            threats: vec![disc(1, 10.0, 8.0)],
            margin: 3.0,
        };
        let exit = field
            .nearest_exit(&[], Vec2::new(1.0, 0.0), |_| true)
            .expect("an exit");
        assert!((exit - Vec2::new(-1.25, 0.0)).length() < 1e-4, "{exit}");
        assert_eq!(
            field.nearest_exit(&[], Vec2::new(-0.2, 0.0), |_| true),
            None
        );
        assert_eq!(
            field.nearest_exit(&[], Vec2::new(1.0, 0.0), |c| c.x > 0.0),
            None
        );
        let blocked = CcField {
            threats: vec![disc(1, 10.0, 8.0), disc(2, -4.0, 3.0)],
            margin: 3.0,
        };
        assert_eq!(
            blocked.nearest_exit(&[], Vec2::new(1.0, 0.0), |_| true),
            None
        );
    }

    #[test]
    fn the_disc_is_a_schmitt_pair() {
        let f = fixture(Priest, Vec3::new(0.0, 1.0, 0.0));
        let field = f.field();
        // Scream reach 8, margin 3: outer 11, inner 9.5.
        let between = Vec2::new(10.0, 0.0);
        assert!(field.covered(&[], between, Disc::Outer) > 0.0);
        assert_eq!(field.covered(&[], between, Disc::Inner), 0.0);
        assert!((field.deficit(&[], Vec2::new(7.0, 0.0), Disc::Outer) - 4.0).abs() < 1e-4);
        assert_eq!(field.deficit(&[], Vec2::new(12.0, 0.0), Disc::Outer), 0.0);
    }
}
