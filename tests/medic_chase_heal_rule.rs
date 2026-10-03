//! AS-202 — the medic chase and the heal fallback compose.
//!
//! A heal whose lowest ally is behind a pillar now falls back to the lowest
//! ally the healer can see. The medic chase still walks a healer round the
//! pillar to a DYING teammate it cannot see. The rule between them: while that
//! walk is live, the walk wins over a movement-locking cast that is not itself
//! critical. A hardcast heal on a less-hurt ally in sight defers, because it
//! would root the healer short of the dying one; a heal on an ally in sight who
//! is also dying (at or below `urgency_hp_threshold`) fires, and so does every
//! instant. It is the deferral a dip, an ESCAPE window and a dispel walk
//! already apply.
//!
//! These cases drive the real posture ticks and the Priest's real rotation over
//! a hand-built scene: the healer at the origin, a dying Warrior 16yd down +X
//! behind a pillar, and a Mage 16yd down +Z in sight.

use std::collections::{BTreeMap, HashSet};

use bevy::ecs::world::CommandQueue;
use bevy::prelude::*;

use arenasim::combat::log::CombatLog;
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::class_ai::caster_healer_posture::evaluate_caster_healer_posture;
use arenasim::states::play_match::class_ai::paladin::evaluate_paladin_posture;
use arenasim::states::play_match::class_ai::priest::{
    decide_priest_action, evaluate_priest_posture, PriestMovementPlan,
};
use arenasim::states::play_match::class_ai::{CombatContext, CombatantInfo};
use arenasim::states::play_match::decision_trace::DecisionTrace;
use arenasim::states::play_match::map_geometry::ObstacleVolume;
use arenasim::states::play_match::{
    AbilityDefinitions, AbilityType, ActiveAuras, Aura, AuraType, Combatant, MovementConfig,
    MovementDirective, MovementGoal,
};

const HEALER_POS: Vec3 = Vec3::new(0.0, 1.0, 0.0);
const WARRIOR_POS: Vec3 = Vec3::new(16.0, 1.0, 0.0);
const MAGE_POS: Vec3 = Vec3::new(0.0, 1.0, 16.0);
const NOW: f32 = 20.0;
/// The dying Warrior: below the shipped `urgency_hp_threshold` (0.5).
const DYING: f32 = 0.3;

static EMPTY_DR: BTreeMap<Entity, arenasim::states::play_match::DRTracker> = BTreeMap::new();
static EMPTY_CD: BTreeMap<Entity, BTreeMap<AbilityType, f32>> = BTreeMap::new();

fn info(entity: Entity, team: u8, class: CharacterClass, position: Vec3, hp: f32) -> CombatantInfo {
    CombatantInfo {
        entity,
        team,
        slot: 0,
        class,
        current_health: 100.0 * hp,
        max_health: 100.0,
        current_mana: 100.0,
        max_mana: 100.0,
        position,
        velocity: Vec3::ZERO,
        is_alive: true,
        stealthed: false,
        target: None,
        is_pet: false,
        casting_ability: None,
        pet_type: None,
        pet: None,
    }
}

/// The pillar between the healer and the Warrior.
fn pillar() -> Vec<ObstacleVolume> {
    vec![ObstacleVolume::Cylinder {
        center_xz: Vec2::new(8.0, 0.0),
        radius: 2.0,
        base_y: 0.0,
        height: 10.0,
    }]
}

struct Scene {
    world: World,
    healer: Entity,
    warrior: Entity,
    mage: Entity,
    roster: BTreeMap<Entity, CombatantInfo>,
    auras: BTreeMap<Entity, Vec<Aura>>,
    combatant: Combatant,
    obstacles: Vec<ObstacleVolume>,
}

/// `class` heals; the Warrior at `warrior_hp` behind the pillar, the Mage at
/// `mage_hp` in sight, and an enemy Hunter beside the Warrior attacking it
/// (well outside the healer's danger radius, so nothing presses the healer).
fn scene(class: CharacterClass, warrior_hp: f32, mage_hp: f32) -> Scene {
    let mut world = World::new();
    let healer = world.spawn_empty().id();
    let warrior = world.spawn_empty().id();
    let mage = world.spawn_empty().id();
    let hunter = world.spawn_empty().id();

    let mut combatant = Combatant::new(1, 0, class);
    combatant.target = Some(hunter);
    let mut me = info(healer, 1, class, HEALER_POS, 1.0);
    me.current_mana = combatant.current_mana;
    me.max_mana = combatant.max_mana;
    let mut enemy = info(
        hunter,
        2,
        CharacterClass::Hunter,
        WARRIOR_POS + Vec3::new(5.0, 0.0, 0.0),
        1.0,
    );
    enemy.target = Some(warrior);
    let roster = [
        me,
        info(warrior, 1, CharacterClass::Warrior, WARRIOR_POS, warrior_hp),
        info(mage, 1, CharacterClass::Mage, MAGE_POS, mage_hp),
        enemy,
    ]
    .into_iter()
    .map(|u| (u.entity, u))
    .collect();

    Scene {
        world,
        healer,
        warrior,
        mage,
        roster,
        auras: BTreeMap::new(),
        combatant,
        obstacles: pillar(),
    }
}

impl Scene {
    fn ctx(&self) -> CombatContext<'_> {
        CombatContext::new(
            self.healer,
            1,
            &self.roster,
            &self.auras,
            &EMPTY_DR,
            &EMPTY_CD,
            &self.obstacles,
            Default::default(),
            Default::default(),
        )
    }

    /// The walk the posture tick issued, as `(goal point, traced trigger)`.
    fn walk(&self, trace: &mut DecisionTrace) -> (Option<Vec3>, Vec<String>) {
        let point = self
            .world
            .get::<MovementDirective>(self.healer)
            .and_then(|d| match d.goal {
                MovementGoal::Point(p) => Some(p),
                _ => None,
            });
        let triggers = trace
            .pending_events
            .drain(..)
            .filter_map(|e| {
                let v = serde_json::to_value(&e).ok()?;
                Some(v["trigger"].as_str()?.to_string())
            })
            .collect();
        (point, triggers)
    }

    /// One Priest posture tick: its plan, the walk it issued and its triggers.
    fn priest_posture(&mut self) -> (PriestMovementPlan, Option<Vec3>, Vec<String>) {
        let abilities = AbilityDefinitions::default();
        let movement = MovementConfig::default();
        let mut trace = DecisionTrace::default();
        let mut queue = CommandQueue::default();
        let plan = {
            let ctx = self.ctx();
            let mut commands = Commands::new(&mut queue, &self.world);
            evaluate_priest_posture(
                &mut commands,
                self.healer,
                &self.combatant,
                HEALER_POS,
                &ctx,
                &abilities,
                None,
                None,
                None,
                &movement,
                NOW,
                &mut trace,
            )
        };
        queue.apply(&mut self.world);
        let (point, triggers) = self.walk(&mut trace);
        (plan, point, triggers)
    }

    /// One Priest rotation turn under `plan`: the chosen ability and target,
    /// and the Flash Heal candidate's traced status/reason.
    fn priest_decide(
        &mut self,
        plan: &PriestMovementPlan,
    ) -> (Option<(String, Option<u64>)>, serde_json::Value) {
        let abilities = AbilityDefinitions::default();
        let movement = MovementConfig::default();
        let mut trace = DecisionTrace::default();
        let mut queue = CommandQueue::default();
        // Everyone already carries Fortitude, and Weakened Soul keeps Power
        // Word: Shield out, so the heal ladder is what decides.
        let mut fortified: HashSet<Entity> = self.roster.keys().copied().collect();
        for unit in [self.healer, self.warrior, self.mage] {
            self.auras.insert(
                unit,
                vec![Aura {
                    effect_type: AuraType::WeakenedSoul,
                    ability_name: "Weakened Soul".to_string(),
                    duration: 15.0,
                    ..Default::default()
                }],
            );
        }
        let self_auras = ActiveAuras { auras: Vec::new() };
        {
            // Built from the fields, not `self.ctx()`, so the rotation can
            // borrow the Priest's `Combatant` mutably alongside it.
            let ctx = CombatContext::new(
                self.healer,
                1,
                &self.roster,
                &self.auras,
                &EMPTY_DR,
                &EMPTY_CD,
                &self.obstacles,
                Default::default(),
                Default::default(),
            );
            let mut commands = Commands::new(&mut queue, &self.world);
            decide_priest_action(
                &mut commands,
                &mut CombatLog::default(),
                &abilities,
                self.healer,
                &mut self.combatant,
                HEALER_POS,
                Some(&self_auras),
                &ctx,
                &mut HashSet::new(),
                &mut fortified,
                plan,
                &movement,
                &mut Vec::new(),
                &mut trace,
            );
        }
        let event = serde_json::to_value(&trace.pending_events[0]).unwrap();
        let chosen = event["outcome"]["ability"]
            .as_str()
            .map(|a| (a.to_string(), event["outcome"]["target_id"].as_u64()));
        let flash_heal = event["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["ability"] == "FlashHeal")
            .cloned()
            .unwrap_or_default();
        (chosen, flash_heal)
    }

    fn id(&self, e: Entity) -> Option<u64> {
        Some(e.index() as u64)
    }
}

/// The dying Warrior behind the pillar draws the Priest round it, and that
/// walk defers the heal ladder's non-critical casts.
#[test]
fn the_medic_walk_to_a_dying_ally_defers_non_critical_casts() {
    let mut s = scene(CharacterClass::Priest, DYING, 0.7);
    let (plan, point, triggers) = s.priest_posture();
    assert_eq!(point, Some(WARRIOR_POS), "walks at the dying Warrior");
    assert_eq!(triggers, vec!["SeekLos".to_string()]);
    assert_eq!(
        plan.escape_defer,
        Some(MovementConfig::default().shared.urgency_hp_threshold),
        "the walk defers non-critical movement-locking casts"
    );

    // Open ground: no medic walk (the formation tick runs), nothing defers.
    let mut s = scene(CharacterClass::Priest, DYING, 0.7);
    s.obstacles.clear();
    let (plan, point, triggers) = s.priest_posture();
    assert_ne!(point, Some(WARRIOR_POS));
    assert!(!triggers.contains(&"SeekLos".to_string()), "{triggers:?}");
    assert_eq!(plan.escape_defer, None);
}

/// Mid-walk, the Flash Heal that would fall back to the Mage at 70% defers —
/// casting it would root the Priest short of the dying Warrior.
#[test]
fn mid_walk_a_less_hurt_ally_in_sight_waits() {
    let mut s = scene(CharacterClass::Priest, DYING, 0.7);
    let (plan, _, _) = s.priest_posture();
    let (chosen, flash_heal) = s.priest_decide(&plan);
    assert_eq!(chosen, None, "nothing roots the Priest mid-walk");
    assert_eq!(flash_heal["status"], "rejected");
    assert!(
        flash_heal["reason"]["PreconditionUnmet"]["note"]
            .as_str()
            .is_some_and(|n| n.contains("non-critical heal deferred")),
        "{flash_heal}"
    );

    // Without the walk the same Flash Heal falls back to the Mage in sight.
    let (chosen, _) = s.priest_decide(&PriestMovementPlan::default());
    assert_eq!(chosen, Some(("FlashHeal".to_string(), s.id(s.mage))));
}

/// Mid-walk, an ally in sight who is ALSO dying is healed: a critical heal
/// always wins, and the Priest cannot save the Warrior it cannot see any
/// faster by letting the Mage die.
#[test]
fn mid_walk_a_dying_ally_in_sight_is_healed() {
    let mut s = scene(CharacterClass::Priest, DYING, 0.4);
    let (plan, point, _) = s.priest_posture();
    assert_eq!(point, Some(WARRIOR_POS), "still walking at the Warrior");
    let (chosen, _) = s.priest_decide(&plan);
    assert_eq!(chosen, Some(("FlashHeal".to_string(), s.id(s.mage))));
}

/// The Paladin's medic walk defers its non-critical heals the same way.
#[test]
fn the_paladin_medic_walk_defers_non_critical_heals() {
    let mut s = scene(CharacterClass::Paladin, DYING, 0.7);
    let abilities = AbilityDefinitions::default();
    let movement = MovementConfig::default();
    let mut trace = DecisionTrace::default();
    let mut queue = CommandQueue::default();
    let plan = {
        let ctx = s.ctx();
        let mut commands = Commands::new(&mut queue, &s.world);
        evaluate_paladin_posture(
            &mut commands,
            &abilities,
            s.healer,
            &s.combatant,
            HEALER_POS,
            None,
            &ctx,
            None,
            None,
            &movement,
            NOW,
            &mut trace,
        )
    };
    queue.apply(&mut s.world);
    let (point, triggers) = s.walk(&mut trace);
    assert_eq!(point, Some(WARRIOR_POS), "walks at the dying Warrior");
    assert_eq!(triggers, vec!["SeekLos".to_string()]);
    assert_eq!(
        plan.cast_defer,
        Some(movement.shared.urgency_hp_threshold),
        "the walk defers Flash of Light / Holy Light on a less-hurt ally"
    );
}

/// The Shaman's (the shared caster-healer machine's) medic walk defers its
/// non-critical casts the same way.
#[test]
fn the_shaman_medic_walk_defers_non_critical_casts() {
    let mut s = scene(CharacterClass::Shaman, DYING, 0.7);
    let movement = MovementConfig::default();
    let mut trace = DecisionTrace::default();
    let mut queue = CommandQueue::default();
    let plan = {
        let ctx = s.ctx();
        let mut commands = Commands::new(&mut queue, &s.world);
        evaluate_caster_healer_posture(
            &mut commands,
            s.healer,
            &s.combatant,
            HEALER_POS,
            &ctx,
            None,
            None,
            &movement,
            &movement.shaman,
            false,
            NOW,
            &mut trace,
        )
    };
    queue.apply(&mut s.world);
    let (point, triggers) = s.walk(&mut trace);
    assert_eq!(point, Some(WARRIOR_POS), "walks at the dying Warrior");
    assert_eq!(triggers, vec!["SeekLos".to_string()]);
    assert_eq!(
        plan.escape_defer,
        Some(movement.shared.urgency_hp_threshold),
        "the walk defers Lesser Healing Wave on a less-hurt ally"
    );
}
