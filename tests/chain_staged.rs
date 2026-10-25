//! The Chain play in a STAGED match (tier 3, `headless::stage`): the enemy
//! Priest is stunned for [`LOCK`] seconds, and a Mage 20yd off has it as its
//! crowd-control target. Under the play the Mage holds its global cooldown,
//! then begins a Polymorph timed to land in the stun's last moments — and the
//! Priest is never free between the two. With the play withheld the hold
//! rule refuses the Polymorph until the stun is all but over, so it lands
//! after the stun has ended and the Priest acts in between.
//!
//! The scene tests (`tests/chain_scenes.rs`) pin the one-tick decision; this
//! pins the outcome: the hand-off goes through the real schedule — the cast,
//! the aura's landing — and the Priest's lockout has no gap.

mod common;

use std::collections::BTreeMap;

use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use arenasim::states::play_match::class_ai::chain::HANDOFF_LEAD;
use arenasim::states::play_match::class_ai::plays::every_exception_entry;
use arenasim::states::play_match::components::Combatant;
use arenasim::states::play_match::situation::PlayId;
use arenasim::states::play_match::utils::is_incapacitating;
use arenasim::states::play_match::{Aura, AuraType};
use common::staged::{entity_id, of_class, play, sim_time, Played};

const MAGE: Unit = Unit::new(1, 0);
const PRIEST: Unit = Unit::new(1, 1);
const ENEMY_WARRIOR: Unit = Unit::new(2, 0);
const ENEMY_PRIEST: Unit = Unit::new(2, 1);

/// The stun on the enemy Priest.
const LOCK: f32 = 3.0;
/// How long after the stun the test watches the Priest.
const WATCH: f32 = 3.0;
/// Polymorph's cast time.
const POLYMORPH_CAST: f32 = 1.5;

fn config(seed: u64, chain: bool) -> HeadlessMatchConfig {
    // Chain graduated: its control excepts it everywhere on team 1.
    let withheld = if chain {
        BTreeMap::new()
    } else {
        BTreeMap::from([("Chain".to_string(), every_exception_entry(PlayId::Chain))])
    };
    HeadlessMatchConfig {
        team1: vec!["Mage".into(), "Priest".into()],
        team2: vec!["Warrior".into(), "Priest".into()],
        map: "BasicArena".into(),
        ai_profile: Some("TeamPlan".into()),
        team1_exceptions: withheld,
        random_seed: Some(seed),
        max_duration_secs: 15.0,
        ..Default::default()
    }
}

/// The Mage 20yd from the enemy Priest, which is stunned for [`LOCK`] and is
/// the Mage's crowd-control target; the enemy Warrior on the Mage's Priest,
/// 30yd from the Mage, which targets it.
fn stage() -> Stage {
    Stage::new()
        .place(MAGE, -10.0, 0.0)
        .place(ENEMY_PRIEST, -10.0, 20.0)
        .place(ENEMY_WARRIOR, 20.0, 0.0)
        .place(PRIEST, 17.0, -3.0)
        .target(ENEMY_WARRIOR, PRIEST)
        .target(MAGE, ENEMY_WARRIOR)
        .world(|w| {
            let healer = ENEMY_PRIEST.entity(w);
            let mage = MAGE.entity(w);
            w.get_mut::<Combatant>(mage).expect("the Mage").cc_target = Some(healer);
        })
        .aura(
            ENEMY_PRIEST,
            Aura {
                effect_type: AuraType::Stun,
                duration: LOCK,
                break_on_damage_threshold: -1.0,
                ability_name: "Hammer of Justice".into(),
                ..Default::default()
            },
        )
}

/// The seconds, from the stage, the enemy Priest spent FREE (no
/// incapacitating aura) in the stage's first `LOCK + WATCH` seconds.
fn free_seconds(played: &Played) -> f32 {
    let start = played.landed().app;
    let priest = played.frames[0]
        .combatants
        .iter()
        .find(|(_, c)| c.team == 2 && c.slot == 1 && !c.is_pet)
        .map(|(e, _)| *e)
        .expect("the enemy Priest");
    played
        .staged_frames()
        .filter(|f| f.sim_time < start + LOCK + WATCH)
        .filter(|f| {
            !f.combatants[&priest]
                .aura_types
                .iter()
                .any(is_incapacitating)
        })
        .count() as f32
        / 60.0
}

#[test]
fn the_mage_hands_polymorph_off_into_the_end_of_the_stun_and_the_priest_is_never_free() {
    for seed in [1, 2] {
        let played = play(config(seed, true), stage());
        let start = played.landed().combat;
        let priest = entity_id(&played, 2, 1);
        let handoff = of_class(played.staged_trace(), "Mage")
            .filter(|e| e["kind"] == "ability_decision" && e["actor"]["team"] == 1)
            .find(|e| e["situation"]["play_step"] == "chain" && e["outcome"]["ability"].is_string())
            .unwrap_or_else(|| panic!("seed {seed}: the Mage never hands off"));
        assert_eq!(handoff["outcome"]["ability"], "Polymorph", "seed {seed}");
        assert_eq!(handoff["outcome"]["target_id"].as_u64(), Some(priest));
        // Begun so that it lands in the stun's last `HANDOFF_LEAD`.
        let lands = sim_time(handoff) - start + POLYMORPH_CAST;
        assert!(
            (LOCK - HANDOFF_LEAD - 0.02..=LOCK + 0.02).contains(&lands),
            "seed {seed}: begun to land {lands:.2}s in"
        );
        // Before it, the Mage held its global cooldown: it cast nothing.
        assert!(
            of_class(played.staged_trace(), "Mage")
                .filter(|e| e["kind"] == "ability_decision" && sim_time(e) < sim_time(handoff))
                .all(|e| !e["outcome"]["ability"].is_string()),
            "seed {seed}: the Mage cast before its hand-off"
        );
        assert!(
            played.log.contains("Polymorph polymorphs Team 2 Priest"),
            "seed {seed}: the Polymorph never landed"
        );
        let free = free_seconds(&played);
        assert_eq!(free, 0.0, "seed {seed}: the Priest was free {free:.2}s");

        // Control: the play withheld. The Polymorph lands after the stun, so
        // the Priest has a gap.
        let played = play(config(seed, false), stage());
        if let Some(e) =
            of_class(played.staged_trace(), "Mage").find(|e| e["situation"]["play_step"] == "chain")
        {
            panic!("seed {seed}: a chain step with the play withheld: {e}");
        }
        let free = free_seconds(&played);
        assert!(
            free >= 0.5,
            "seed {seed}: without the play the Priest was free only {free:.2}s"
        );
    }
}
