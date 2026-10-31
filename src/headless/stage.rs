//! Staged matches: a headless match that starts from a constructed state.
//!
//! Tier 3 of the AI testing pyramid
//! (`docs/design/proposals/2026-10-11-ai-architecture.md` §3.2). A [`Stage`]
//! sets the state a behaviour needs — positions, health, resource, auras,
//! cooldowns, crowd control, posture, the match clock — and
//! [`run_headless_match_staged`](super::runner::run_headless_match_staged)
//! plays forward from it through the real `add_core_combat_systems`
//! schedule, under the same frame observer and decision trace as any probe.
//! The seed still governs the rolls, but the situation under test holds BY
//! CONSTRUCTION: a change that moves the RNG stream cannot remove it.
//!
//! **When the stage lands.** The match spawns as usual and plays its
//! countdown, so the pre-match buffs land as they do in every match. The stage
//! is applied on the frame the gates open — after the countdown has opened
//! them and held every resource at its pre-combat level, and before any other
//! system runs that frame — or, with [`Stage::after_gates`], at the start of
//! the first frame at or past that many seconds of combat. Either way it is
//! applied ONCE, between `update_countdown` and the rest of the schedule, so
//! every system that frame sees the staged world and nothing sees a half-built
//! one.
//!
//! **The clock.** [`Stage::clock`] sets the match's seconds since the gates:
//! the trace's `sim_time`, the dampening ramp (and so the match phase the AI
//! reads), the shadow-sight timer and the timeout. The combat log's
//! timestamps, a frame's `sim_time` and the posture machines' `now` are the
//! app's own clock, countdown included, and keep counting from where they
//! were.
//!
//! ```ignore
//! let stage = Stage::new()
//!     .place(DRUID, -30.0, 0.0)
//!     .place(MAGE, -50.0, 0.0)
//!     .health(MAGE, 0.47)
//!     .land(MAGE, AbilityType::Polymorph, ENEMY_MAGE);
//! run_headless_match_staged(config, true, None, stage, |frame| { ... })?;
//! ```
//!
//! Units are named by team and slot ([`Unit`]), the way the config lists them,
//! so a stage reads as the lineup it describes.

use bevy::prelude::*;

use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::components::{
    ActiveAuras, Aura, AuraPending, Combatant, HealerPosture, KitePosture, MeleeResetState, Pet,
};
use crate::states::play_match::constants::PET_SLOT_BASE;
use crate::states::play_match::decision_trace::DecisionTrace;
use crate::states::play_match::systems::{ArenaDampening, MatchCountdown, ShadowSightState};

use super::runner::HeadlessMatchState;

/// A combatant a stage addresses: by team (1 or 2) and slot, the way the
/// config's team lists name it. A pet is addressed by its owner's slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Unit {
    pub team: u8,
    pub slot: u8,
    pub pet: bool,
}

impl Unit {
    /// The combatant in `slot` of `team`.
    pub const fn new(team: u8, slot: u8) -> Self {
        Self {
            team,
            slot,
            pet: false,
        }
    }

    /// The pet of the combatant in `slot` of `team`.
    pub const fn pet_of(team: u8, slot: u8) -> Self {
        Self {
            team,
            slot,
            pet: true,
        }
    }

    /// The entity standing for this unit in `world`. Panics, naming the unit,
    /// when the match spawned no such combatant.
    pub fn entity(self, world: &mut World) -> Entity {
        let slot = if self.pet {
            PET_SLOT_BASE + self.slot
        } else {
            self.slot
        };
        let mut query = world.query::<(Entity, &Combatant, Option<&Pet>)>();
        query
            .iter(world)
            .find(|(_, c, pet)| c.team == self.team && c.slot == slot && pet.is_some() == self.pet)
            .map(|(e, _, _)| e)
            .unwrap_or_else(|| panic!("the staged match has no {self:?}"))
    }
}

type Step = Box<dyn FnOnce(&mut World) + Send + Sync>;

/// A constructed match state, applied once to a running headless match. See
/// the module docs.
#[derive(Default)]
pub struct Stage {
    /// Seconds after the gates at which the stage lands (0 = the gate frame).
    after_gates: f32,
    steps: Vec<Step>,
}

impl Stage {
    /// An empty stage, landing on the frame the gates open. An empty stage
    /// changes nothing: the match plays exactly as its seed does.
    pub fn new() -> Self {
        Self::default()
    }

    /// Land the stage at the start of the first frame at or past `secs` of
    /// combat, instead of on the gate frame. The match plays its seed until
    /// then.
    pub fn after_gates(mut self, secs: f32) -> Self {
        self.after_gates = secs;
        self
    }

    /// The seconds of combat at which the stage lands.
    pub fn lands_after(&self) -> f32 {
        self.after_gates
    }

    fn step(mut self, f: impl FnOnce(&mut World) + Send + Sync + 'static) -> Self {
        self.steps.push(Box::new(f));
        self
    }

    fn with_combatant(
        self,
        who: Unit,
        f: impl FnOnce(&mut Combatant) + Send + Sync + 'static,
    ) -> Self {
        self.step(move |world| {
            let e = who.entity(world);
            f(&mut world.get_mut::<Combatant>(e).expect("a combatant"));
        })
    }

    /// `who` stands at `(x, z)` on the ground plane (its height is kept).
    pub fn place(self, who: Unit, x: f32, z: f32) -> Self {
        self.step(move |world| {
            let e = who.entity(world);
            let mut transform = world.get_mut::<Transform>(e).expect("a transform");
            transform.translation.x = x;
            transform.translation.z = z;
        })
    }

    /// `who`'s health, as a fraction of its maximum.
    pub fn health(self, who: Unit, fraction: f32) -> Self {
        self.with_combatant(who, move |c| c.current_health = c.max_health * fraction)
    }

    /// `who`'s mana or energy, as a fraction of its maximum.
    pub fn mana(self, who: Unit, fraction: f32) -> Self {
        self.with_combatant(who, move |c| c.current_mana = c.max_mana * fraction)
    }

    /// `who`'s resource in points (a Warrior's rage).
    pub fn resource(self, who: Unit, points: f32) -> Self {
        self.with_combatant(who, move |c| c.current_mana = points)
    }

    /// `who` targets `whom`.
    pub fn target(self, who: Unit, whom: Unit) -> Self {
        self.step(move |world| {
            let target = whom.entity(world);
            let e = who.entity(world);
            world.get_mut::<Combatant>(e).expect("a combatant").target = Some(target);
        })
    }

    /// `ability` on cooldown for `who`, with `secs` left — as a match would
    /// have left it part-way through. Not a cooldown START: nothing in a match
    /// calls this, and a cooldown category's other members are left as they
    /// are (stage them too if the test needs them locked).
    pub fn cooldown(self, who: Unit, ability: AbilityType, secs: f32) -> Self {
        self.with_combatant(who, move |c| {
            c.ability_cooldowns.insert(ability, secs);
        })
    }

    /// Anything on `who`'s `Combatant` the builder does not name.
    pub fn combatant(
        self,
        who: Unit,
        f: impl FnOnce(&mut Combatant) + Send + Sync + 'static,
    ) -> Self {
        self.with_combatant(who, f)
    }

    /// `aura` on `who`, as given — straight into its `ActiveAuras`, without
    /// passing through `apply_pending_auras` (no DR, no immunity check, no log
    /// line). For an aura as an ability lands it, use [`Self::land`].
    pub fn aura(self, who: Unit, aura: Aura) -> Self {
        self.step(move |world| {
            let e = who.entity(world);
            match world.get_mut::<ActiveAuras>(e) {
                Some(mut auras) => auras.auras.push(aura),
                None => {
                    world
                        .entity_mut(e)
                        .insert(ActiveAuras { auras: vec![aura] });
                }
            }
        })
    }

    /// The aura `ability` applies, cast by `caster`, landed on `target`
    /// THROUGH `apply_pending_auras` this very frame — so diminishing
    /// returns, immunities, stacking and the `[AURA]` log line all apply, as
    /// for a cast that landed. A blocked aura is simply not there; assert it
    /// landed if the test depends on it.
    pub fn land(self, target: Unit, ability: AbilityType, caster: Unit) -> Self {
        self.step(move |world| {
            let (t, c) = (target.entity(world), caster.entity(world));
            // Read from the caster's own book, as a cast that landed is. A
            // stage may name any unit as the aura's source, though — a Hunter's
            // trap "cast" by its Druid teammate — so an ability outside the
            // caster's book is read from the table, as the table writes it.
            let pending = {
                let from_book = world
                    .get::<Combatant>(c)
                    .and_then(|caster| caster.book().get(&ability).cloned());
                let def = from_book.unwrap_or_else(|| {
                    world
                        .resource::<AbilityDefinitions>()
                        .get_unchecked(&ability)
                        .clone()
                });
                AuraPending::from_ability(t, c, &def)
                    .unwrap_or_else(|| panic!("{ability:?} applies no aura"))
            };
            world.spawn(pending);
        })
    }

    /// Remove every aura on `who` (the pre-match buffs included).
    pub fn clear_auras(self, who: Unit) -> Self {
        self.step(move |world| {
            let e = who.entity(world);
            if let Some(mut auras) = world.get_mut::<ActiveAuras>(e) {
                auras.auras.clear();
            }
        })
    }

    /// The healer posture state `who` carries into the stage.
    pub fn healer_posture(self, who: Unit, posture: HealerPosture) -> Self {
        self.step(move |world| {
            let e = who.entity(world);
            world.entity_mut(e).insert(posture);
        })
    }

    /// The kiter posture state `who` carries into the stage.
    pub fn kite_posture(self, who: Unit, posture: KitePosture) -> Self {
        self.step(move |world| {
            let e = who.entity(world);
            world.entity_mut(e).insert(posture);
        })
    }

    /// The melee tempo-reset state `who` carries into the stage.
    pub fn melee_reset(self, who: Unit, state: MeleeResetState) -> Self {
        self.step(move |world| {
            let e = who.entity(world);
            world.entity_mut(e).insert(state);
        })
    }

    /// The match clock: `secs` since the gates opened. Sets every combat
    /// clock the simulation reads — the trace's and the situation layer's
    /// `sim_time` (`DecisionTrace::current_sim_time`), the dampening ramp
    /// (which the match phase reads), the shadow-sight timer and the timeout
    /// — and the dampening reduction follows on this frame's tick.
    ///
    /// It does NOT set the posture machines' `now`: `decide_abilities` hands
    /// them the app clock (`Time::elapsed_secs`, countdown included), which no
    /// stage moves. A posture or directive the stage carries in keeps its
    /// absolute times, so stage them against the app clock, read off `Time`
    /// in a [`Stage::world`] step.
    pub fn clock(self, secs: f32) -> Self {
        self.step(move |world| {
            world.resource_mut::<DecisionTrace>().current_sim_time = secs;
            world.resource_mut::<ArenaDampening>().time_since_gates = secs;
            world.resource_mut::<ShadowSightState>().combat_time = secs;
            world.resource_mut::<HeadlessMatchState>().elapsed_time = secs;
        })
    }

    /// Anything else, with the whole world in hand.
    pub fn world(self, f: impl FnOnce(&mut World) + Send + Sync + 'static) -> Self {
        self.step(f)
    }
}

/// The stage a staged match has yet to land.
#[derive(Resource)]
pub(super) struct PendingStage(pub(super) Option<Stage>);

/// Lands the pending stage once it is due: the gates are open and the match
/// is at least `after_gates` seconds in. Registered by the staged runner
/// alone, right after `update_countdown` and before every other combat
/// system, so the frame the stage lands on runs wholly on the staged world.
pub(super) fn land_stage(world: &mut World) {
    let due = world
        .get_resource::<PendingStage>()
        .and_then(|p| p.0.as_ref())
        .is_some_and(|stage| {
            world.resource::<MatchCountdown>().gates_opened
                && world.resource::<HeadlessMatchState>().elapsed_time >= stage.after_gates
        });
    if !due {
        return;
    }
    let stage = world
        .resource_mut::<PendingStage>()
        .0
        .take()
        .expect("a due stage");
    for step in stage.steps {
        step(world);
    }
}
