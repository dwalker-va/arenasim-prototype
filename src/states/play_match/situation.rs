//! Situations: the team-level read model the class AI decides from.
//!
//! A *situation* is a named, discrete fact about the fight that more than one
//! decision cares about — "the enemy healer is locked", "we are pressing". The
//! class AI used to recompute each one ad hoc at every site that read it; here
//! it is classified ONCE, from the same snapshot every unit decides from,
//! named in the decision trace, and handed to the class AI through
//! [`CombatContext::situation`](super::class_ai::CombatContext::situation).
//! The design is `docs/design/proposals/2026-10-11-ai-architecture.md` §4.2.
//!
//! v0 models two situations:
//!
//! - **`enemy_healer_locked`** — the enemy healer cannot cast a heal: it is
//!   under a CAST-PREVENTING crowd control ([`locks_healer`]). Its
//!   [`Window`] names the healer, when the lock began and when the longest
//!   such CC on it runs out.
//! - **`tempo`** — whether the team is [`Tempo::Pressing`] its health lead,
//!   [`Tempo::Trailing`], or [`Tempo::Even`] (`press_advantage_margin` in
//!   `movement.ron`).
//!
//! ## Where it is classified, and why there
//!
//! [`update_situations`] runs inside `decide_abilities`, on the snapshot that
//! function builds, and again after every same-frame instant CC that snapshot
//! reflects — not as a Phase 1 system. Both are load-bearing for byte-identity:
//! `process_aura_breaks` removes damage-broken CC at the START of Phase 2,
//! after any Phase 1 system could have read the auras, and an instant CC landed
//! by an earlier unit in the dispatch (a Kidney Shot on the healer) is visible
//! to every later unit in the same frame. A classification taken anywhere else
//! would disagree with what the decision actually saw.
//!
//! ## Two properties that keep it safe
//!
//! - **Pure and RNG-free.** [`classify`] only reads. Adding the layer changes
//!   no output until a site reads it.
//! - **Built from what the team perceives.** The enemy healer is chosen with
//!   the [`stealth_visible`] rule every unit's view applies. Only a Rogue can
//!   stealth, so a healer is visible to its whole enemy team or to none of it,
//!   and one classification serves every member.

use std::collections::BTreeMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::class_ai::combat_snapshot::CombatSnapshot;
use super::class_ai::{pressing_when_ahead, stealth_visible, team_hp_sums, CombatantInfo};
use super::components::{Aura, AuraType};
use super::decision_trace::{DecisionTrace, SituationChangeKind, SituationName};
use super::movement_config::MovementConfig;

/// Whether a team is ahead on health, behind, or level — read against
/// `press_advantage_margin`. Serializes snake_case (`"pressing"`).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Tempo {
    #[default]
    Even,
    /// Own team's summed health fraction leads by at least the margin.
    Pressing,
    /// The enemy leads by at least the margin.
    Trailing,
}

/// One live window of a situation: who it is about, when it began, and when it
/// is known to end. Times are on the decision trace's `sim_time` clock, so a
/// window joins directly against the events recorded inside it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Window {
    /// The unit the situation is about (the locked healer).
    pub subject: Entity,
    /// When this window opened. Carried across frames while the subject stays
    /// the same, so it is the ONSET, not the latest frame.
    pub since: f32,
    /// When it is due to end, if that is known: the longest remaining locking
    /// CC on the subject. Moves later when a new CC extends the lock.
    pub until: Option<f32>,
}

/// Everything one team's units know about the situation this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TeamSituation {
    pub tempo: Tempo,
    pub enemy_healer_locked: Option<Window>,
}

/// What a unit does about a situation: a named reordering of its own priority
/// chain, nothing more. The class AI keeps its `try_*` steps and gates; a play
/// only says which order they run in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Play {
    /// The class's ordinary priority order.
    Default,
    /// The enemy healer cannot answer: spend the hard-hitting step first.
    Burst,
}

/// The part a unit plays for its team, as far as choosing a play goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    MeleeDps,
    RangedDps,
    Healer,
}

impl TeamSituation {
    /// The play `role` runs in this situation. Exhaustive over [`Role`], so a
    /// new role is a compile error here until it answers.
    pub fn play_for(&self, role: Role) -> Play {
        match role {
            Role::MeleeDps | Role::RangedDps if self.enemy_healer_locked.is_some() => Play::Burst,
            Role::MeleeDps | Role::RangedDps | Role::Healer => Play::Default,
        }
    }
}

/// The situation of both teams. Team `t` lives at index `t - 1`.
///
/// Inserted per match beside `TeamPlans`; `decide_abilities` keeps it current
/// (see the module docs for why there). Read it from another system and you
/// get the previous frame's classification.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct Situations {
    pub teams: [TeamSituation; 2],
}

impl Situations {
    /// Team `team`'s situation; the default (nothing going on) for a team id
    /// outside 1..=2, matching `TeamPlans::for_team`.
    pub fn for_team(&self, team: u8) -> TeamSituation {
        team.checked_sub(1)
            .and_then(|i| self.teams.get(i as usize))
            .copied()
            .unwrap_or_default()
    }
}

/// The thresholds the classifier reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SituationConfig {
    /// `shared.press_advantage_margin` in `movement.ron`: the team-HP-fraction
    /// lead at which a team presses (and the deficit at which it trails).
    pub press_advantage_margin: f32,
}

impl Default for SituationConfig {
    fn default() -> Self {
        Self::from_movement(&MovementConfig::default())
    }
}

impl SituationConfig {
    pub fn from_movement(config: &MovementConfig) -> Self {
        Self {
            press_advantage_margin: config.shared.press_advantage_margin,
        }
    }
}

/// Crowd control that stops a healer casting a heal: Stun, Fear, Polymorph,
/// Incapacitate, Cyclone. NOT Root: a rooted healer heals freely, so a root
/// must not open a burst window. The one copy of the set behind
/// `enemy_healer_locked`.
pub fn locks_healer(effect: AuraType) -> bool {
    matches!(
        effect,
        AuraType::Stun
            | AuraType::Fear
            | AuraType::Polymorph
            | AuraType::Incapacitate
            | AuraType::Cyclone
    )
}

/// Classify `team`'s situation from `snapshot`. Pure: the snapshot, the
/// previous classification (for each window's onset), the clock, and the
/// thresholds.
pub fn classify(
    snapshot: &CombatSnapshot,
    team: u8,
    prev: &TeamSituation,
    now: f32,
    cfg: &SituationConfig,
) -> TeamSituation {
    classify_roster(
        &snapshot.combatants,
        &snapshot.active_auras,
        team,
        prev,
        now,
        cfg,
    )
}

/// [`classify`] over the two maps it reads, so a `CombatContext` built without
/// a snapshot can classify its own view.
pub(crate) fn classify_roster(
    roster: &BTreeMap<Entity, CombatantInfo>,
    auras: &BTreeMap<Entity, Vec<Aura>>,
    team: u8,
    prev: &TeamSituation,
    now: f32,
    cfg: &SituationConfig,
) -> TeamSituation {
    TeamSituation {
        tempo: tempo(roster, team, cfg.press_advantage_margin),
        enemy_healer_locked: enemy_healer_locked(roster, auras, team, prev, now),
    }
}

/// The enemy healer `team` would shut down: the first living, non-pet enemy
/// healer in entity order that the team can see. Entity order, and only the
/// FIRST healer, because that is the one every reader of this fact has always
/// meant (`CombatContext::enemy_healer`); a second enemy healer is not
/// consulted.
pub(crate) fn enemy_healer(
    roster: &BTreeMap<Entity, CombatantInfo>,
    auras: &BTreeMap<Entity, Vec<Aura>>,
    team: u8,
) -> Option<Entity> {
    let has_shadow_sight = |entity: Entity| {
        auras
            .get(&entity)
            .is_some_and(|list| list.iter().any(|a| a.effect_type == AuraType::ShadowSight))
    };
    roster
        .values()
        .find(|info| {
            info.team != team
                && info.is_alive
                && !info.is_pet
                && info.class.is_healer()
                // The observer's own Shadow Sight is per-unit and cannot enter
                // a team classification; it never matters, because only a
                // Rogue stealths (see the module docs).
                && stealth_visible(info.stealthed, false, has_shadow_sight(info.entity))
        })
        .map(|info| info.entity)
}

fn enemy_healer_locked(
    roster: &BTreeMap<Entity, CombatantInfo>,
    auras: &BTreeMap<Entity, Vec<Aura>>,
    team: u8,
    prev: &TeamSituation,
    now: f32,
) -> Option<Window> {
    let healer = enemy_healer(roster, auras, team)?;
    let remaining = auras
        .get(&healer)?
        .iter()
        .filter(|a| locks_healer(a.effect_type))
        .map(|a| a.duration)
        .reduce(f32::max)?;
    let since = prev
        .enemy_healer_locked
        .filter(|w| w.subject == healer)
        .map_or(now, |w| w.since);
    Some(Window {
        subject: healer,
        since,
        until: Some(now + remaining),
    })
}

/// `team`'s tempo: [`pressing_when_ahead`] on its team-HP-fraction advantage
/// (own alive non-pet health fractions minus everyone else's, the sum
/// `CombatContext::team_hp_advantage` takes), and the same test from the
/// other side for trailing.
fn tempo(roster: &BTreeMap<Entity, CombatantInfo>, team: u8, margin: f32) -> Tempo {
    let sums = team_hp_sums(roster);
    let own = sums.get(&team).copied().unwrap_or(0.0);
    let enemy: f32 = sums
        .iter()
        .filter(|(t, _)| **t != team)
        .map(|(_, sum)| *sum)
        .sum();
    let advantage = own - enemy;
    if pressing_when_ahead(advantage, margin) {
        Tempo::Pressing
    } else if pressing_when_ahead(-advantage, margin) {
        Tempo::Trailing
    } else {
        Tempo::Even
    }
}

/// A lock's `until` moves only when a new CC lands, but recomputing it each
/// frame as `now + remaining` wobbles in the last bits. A move of less than
/// this is the same `until`.
const UNTIL_EPSILON: f32 = 0.05;

/// Re-classify both teams from `snapshot`, store the result in `situations`,
/// and record every transition as a `situation_change` trace event.
///
/// `decide_abilities` calls it once on the snapshot it builds, and again after
/// each same-frame instant CC that snapshot reflects (see the module docs).
pub fn update_situations(
    situations: &mut Situations,
    snapshot: &CombatSnapshot,
    now: f32,
    cfg: &SituationConfig,
    trace: &mut DecisionTrace,
) {
    for (index, team) in [1u8, 2].into_iter().enumerate() {
        let prev = situations.teams[index];
        let next = classify(snapshot, team, &prev, now, cfg);
        record_transitions(team, &prev, &next, trace);
        situations.teams[index] = next;
    }
}

/// Emit one `situation_change` event per situation that changed between
/// `prev` and `next`. Transitions only: a frame on which nothing changed
/// records nothing.
fn record_transitions(
    team: u8,
    prev: &TeamSituation,
    next: &TeamSituation,
    trace: &mut DecisionTrace,
) {
    if prev.tempo != next.tempo {
        trace.record_tempo_change(team, prev.tempo, next.tempo);
    }
    let change = match (prev.enemy_healer_locked, next.enemy_healer_locked) {
        (None, None) => None,
        (None, Some(w)) => Some((SituationChangeKind::Onset, w)),
        (Some(w), None) => Some((SituationChangeKind::End, w)),
        (Some(old), Some(new)) if old.subject != new.subject => {
            Some((SituationChangeKind::Onset, new))
        }
        (Some(old), Some(new)) => match (old.until, new.until) {
            (Some(a), Some(b)) if b > a + UNTIL_EPSILON => {
                Some((SituationChangeKind::Extended, new))
            }
            _ => None,
        },
    };
    if let Some((kind, window)) = change {
        trace.record_window_change(team, SituationName::EnemyHealerLocked, kind, &window);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::states::match_config::CharacterClass;

    fn unit(entity: Entity, team: u8, class: CharacterClass, hp: f32) -> CombatantInfo {
        CombatantInfo {
            entity,
            team,
            slot: 0,
            class,
            current_health: hp,
            max_health: 100.0,
            current_mana: 100.0,
            max_mana: 100.0,
            position: Vec3::ZERO,
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

    fn cc(effect_type: AuraType, duration: f32) -> Aura {
        Aura {
            effect_type,
            duration,
            ..Default::default()
        }
    }

    struct Fixture {
        roster: BTreeMap<Entity, CombatantInfo>,
        auras: BTreeMap<Entity, Vec<Aura>>,
        healer: Entity,
    }

    fn fixture() -> Fixture {
        let mut world = World::new();
        let warrior = world.spawn_empty().id();
        let rogue = world.spawn_empty().id();
        let healer = world.spawn_empty().id();
        let mage = world.spawn_empty().id();
        // Two a side at full health: level, so tempo starts Even.
        let roster = [
            unit(warrior, 1, CharacterClass::Warrior, 100.0),
            unit(rogue, 1, CharacterClass::Rogue, 100.0),
            unit(healer, 2, CharacterClass::Priest, 100.0),
            unit(mage, 2, CharacterClass::Mage, 100.0),
        ]
        .into_iter()
        .map(|u| (u.entity, u))
        .collect();
        Fixture {
            roster,
            auras: BTreeMap::new(),
            healer,
        }
    }

    fn classify_now(f: &Fixture, prev: &TeamSituation, now: f32) -> TeamSituation {
        classify_roster(
            &f.roster,
            &f.auras,
            1,
            prev,
            now,
            &SituationConfig::default(),
        )
    }

    #[test]
    fn a_root_does_not_lock_the_healer_and_a_polymorph_does() {
        let mut f = fixture();
        f.auras.insert(f.healer, vec![cc(AuraType::Root, 4.0)]);
        assert_eq!(
            classify_now(&f, &TeamSituation::default(), 10.0).enemy_healer_locked,
            None
        );
        f.auras
            .get_mut(&f.healer)
            .unwrap()
            .push(cc(AuraType::Polymorph, 6.0));
        let window = classify_now(&f, &TeamSituation::default(), 10.0)
            .enemy_healer_locked
            .expect("a polymorphed healer is locked");
        assert_eq!(window.subject, f.healer);
        assert_eq!(window.since, 10.0);
        assert_eq!(window.until, Some(16.0), "until is the longest locking CC");
    }

    #[test]
    fn the_onset_survives_reclassification_and_resets_on_a_new_subject() {
        let mut f = fixture();
        f.auras.insert(f.healer, vec![cc(AuraType::Stun, 4.0)]);
        let first = classify_now(&f, &TeamSituation::default(), 10.0);
        let later = classify_now(&f, &first, 11.0);
        assert_eq!(later.enemy_healer_locked.unwrap().since, 10.0);
        let other = TeamSituation {
            enemy_healer_locked: Some(Window {
                subject: Entity::from_raw(999),
                since: 3.0,
                until: None,
            }),
            ..Default::default()
        };
        assert_eq!(
            classify_now(&f, &other, 11.0)
                .enemy_healer_locked
                .unwrap()
                .since,
            11.0
        );
    }

    #[test]
    fn tempo_presses_and_trails_at_the_margin_and_not_inside_it() {
        let mut f = fixture();
        let margin = SituationConfig::default().press_advantage_margin;
        let healer = f.healer;
        f.roster.get_mut(&healer).unwrap().current_health = 100.0 * (1.0 - margin);
        assert_eq!(
            classify_now(&f, &TeamSituation::default(), 0.0).tempo,
            Tempo::Pressing
        );
        let team2 = classify_roster(
            &f.roster,
            &f.auras,
            2,
            &TeamSituation::default(),
            0.0,
            &SituationConfig::default(),
        );
        assert_eq!(team2.tempo, Tempo::Trailing);
        f.roster.get_mut(&healer).unwrap().current_health = 100.0 * (1.0 - margin / 2.0);
        assert_eq!(
            classify_now(&f, &TeamSituation::default(), 0.0).tempo,
            Tempo::Even
        );
    }

    #[test]
    fn transitions_are_recorded_once_each() {
        let mut f = fixture();
        let mut trace = DecisionTrace::default();
        let quiet = TeamSituation::default();
        f.auras.insert(f.healer, vec![cc(AuraType::Fear, 8.0)]);
        let locked = classify_now(&f, &quiet, 5.0);
        record_transitions(1, &quiet, &locked, &mut trace);
        record_transitions(1, &locked, &classify_now(&f, &locked, 5.0), &mut trace);
        record_transitions(1, &locked, &quiet, &mut trace);
        let changes: Vec<String> = trace
            .pending_events
            .iter()
            .map(|e| serde_json::to_value(e).unwrap()["change"].to_string())
            .collect();
        assert_eq!(changes, vec!["\"onset\"", "\"end\""]);
    }

    #[test]
    fn only_melee_and_ranged_dps_burst() {
        let locked = TeamSituation {
            enemy_healer_locked: Some(Window {
                subject: Entity::from_raw(1),
                since: 0.0,
                until: None,
            }),
            ..Default::default()
        };
        assert_eq!(locked.play_for(Role::MeleeDps), Play::Burst);
        assert_eq!(locked.play_for(Role::RangedDps), Play::Burst);
        assert_eq!(locked.play_for(Role::Healer), Play::Default);
        assert_eq!(
            TeamSituation::default().play_for(Role::MeleeDps),
            Play::Default
        );
    }
}
