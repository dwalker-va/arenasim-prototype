//! AS-413 — a Shadowform Priest, played: it deals damage and casts no heal.
//!
//! A STAGED match (tier 3, `headless::stage`): the Priest's Warrior teammate
//! is dropped below Flash Heal's trigger as the gates open, so a heal is owed
//! from the first frame by construction. The control is the same stage with
//! the Priest's form left at its default: that Priest heals.

mod common;

use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use arenasim::states::play_match::talents::{Build, TalentId};
use common::staged::{play, Played};

const WARRIOR: Unit = Unit::new(1, 0);

/// The Priest is team 1's slot 1: its combat-log id.
const PRIEST_ID: &str = "Team 1 Priest #2";

/// The Priest's heals (its kit has one).
const HEALS: [&str; 1] = ["Flash Heal"];

/// Its damage: the Shadow spells, as the log names them.
const DAMAGE: [&str; 2] = ["Mind Blast", "Shadow Word: Pain"];

fn config(form: &str) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec!["Warrior".into(), "Priest".into()],
        team2: vec!["Warrior".into(), "Mage".into()],
        team1_builds: vec![
            Build::default(),
            Build::of(&[(TalentId::ImprovedMindBlast, 2), (TalentId::Shadowform, 1)]),
        ],
        team1_priest_forms: vec!["NoForm".into(), form.into()],
        map: "BasicArena".into(),
        random_seed: Some(41),
        max_duration_secs: 25.0,
        ..Default::default()
    }
}

/// The Warrior at a third of its health as the gates open, the Priest beside
/// it: a heal is owed from the first frame.
fn stage() -> Stage {
    Stage::new().health(WARRIOR, 0.35)
}

/// How many log lines show the Priest casting one of `names`, or dealing
/// damage with one.
fn priest_lines(played: &Played, verb: &str, names: &[&str]) -> usize {
    played
        .log
        .lines()
        .filter(|l| {
            names
                .iter()
                .any(|n| l.contains(&format!("{PRIEST_ID}{verb}{n}")))
        })
        .count()
}

fn casts(played: &Played, names: &[&str]) -> usize {
    priest_lines(played, " casts ", names) + priest_lines(played, " begins casting ", names)
}

fn hits(played: &Played, names: &[&str]) -> usize {
    played
        .log
        .lines()
        .filter(|l| l.contains("[DMG]"))
        .filter(|l| {
            names
                .iter()
                .any(|n| l.contains(&format!("{PRIEST_ID}'s {n}")))
        })
        .count()
}

#[test]
fn a_shadowform_priest_deals_damage_and_casts_no_heal() {
    let shadow = play(config("Shadowform"), stage());
    assert!(
        shadow
            .log
            .contains(&format!("{PRIEST_ID} casts Shadowform")),
        "the Priest takes the form"
    );
    assert_eq!(casts(&shadow, &HEALS), 0, "{}", shadow.log);
    assert!(
        hits(&shadow, &DAMAGE) > 0,
        "the Priest deals Shadow damage: {}",
        shadow.log
    );

    // Control: the same stage, no form. A heal is owed, and the Priest heals.
    let healer = play(config("NoForm"), stage());
    assert!(
        !healer.log.contains("casts Shadowform"),
        "the control takes no form"
    );
    assert!(casts(&healer, &HEALS) > 0, "control: {}", healer.log);
}

/// A Shadowform Priest is no healer to its pursuit: in range of its target
/// but out of its sight across a Nagrand pillar, it walks round the pillar
/// until it sees it (`combat_core::movement`'s walk for sight, which a healer
/// does not take), and deals damage. The scene is `tests/pursuit_sight_walk.rs`'s
/// with the Warlock's place taken by the Priest; the control is the same
/// placement on BasicArena, where nothing hides the target and the Priest
/// stands and casts.
#[test]
fn an_occluded_shadowform_priest_walks_for_sight() {
    use arenasim::states::match_config::ArenaMap;
    use arenasim::states::play_match::map_config::load_map_geometry_config;
    use arenasim::states::play_match::map_geometry::has_line_of_sight;
    use bevy::prelude::{Vec2, Vec3};

    const SHAMAN: Unit = Unit::new(1, 0);
    const PRIEST: Unit = Unit::new(2, 0);
    // The Shaman against the east face of the pillar at (40, 20); the Priest
    // 23yd west of it, the pillar square between them.
    const SHAMAN_AT: Vec2 = Vec2::new(48.0, 20.0);
    const PRIEST_AT: Vec2 = Vec2::new(25.0, 20.0);
    const BASIC_SHIFT: Vec2 = Vec2::new(-36.0, -20.0);

    let config = |map: &str| HeadlessMatchConfig {
        team1: vec!["Shaman".into()],
        team2: vec!["Priest".into()],
        team2_builds: vec![Build::of(&[
            (TalentId::ImprovedMindBlast, 2),
            (TalentId::Shadowform, 1),
        ])],
        team2_priest_forms: vec!["Shadowform".into()],
        map: map.into(),
        random_seed: Some(1),
        max_duration_secs: 12.0,
        ..Default::default()
    };
    let stage = |shift: Vec2| {
        Stage::new()
            .place(SHAMAN, SHAMAN_AT.x + shift.x, SHAMAN_AT.y + shift.y)
            .mana(SHAMAN, 0.0)
            .place(PRIEST, PRIEST_AT.x + shift.x, PRIEST_AT.y + shift.y)
            .target(PRIEST, SHAMAN)
            .target(SHAMAN, PRIEST)
    };
    let priest_at = |f: &arenasim::headless::FrameObservation| -> Vec3 {
        f.combatants
            .values()
            .find(|c| c.team == 2 && c.slot == 0 && !c.is_pet)
            .map(|c| c.position)
            .expect("the Priest is on the field")
    };
    let walked = |played: &Played| -> f32 {
        let at: Vec<Vec3> = played.staged_frames().map(priest_at).collect();
        at.windows(2)
            .map(|w| Vec2::new(w[1].x - w[0].x, w[1].z - w[0].z).length())
            .sum()
    };
    let volumes = load_map_geometry_config()
        .expect("maps.ron loads")
        .active_for(ArenaMap::PillaredArena)
        .volumes;

    let occluded = play(config("PillaredArena"), stage(Vec2::ZERO));
    let frames: Vec<_> = occluded.staged_frames().collect();
    let shaman_at = Vec3::new(SHAMAN_AT.x, 1.0, SHAMAN_AT.y);
    assert!(
        !has_line_of_sight(&volumes, priest_at(frames[0]), shaman_at),
        "the stage put the Priest out of sight"
    );
    assert!(
        frames
            .iter()
            .any(|f| has_line_of_sight(&volumes, priest_at(f), shaman_at)),
        "the Priest never regained sight (walked {:.1}yd)",
        walked(&occluded)
    );
    assert!(walked(&occluded) >= 3.0, "{:.1}yd", walked(&occluded));
    assert!(
        occluded.result.team2_combatants[0].damage_dealt > 0.0,
        "the Priest dealt no damage: {}",
        occluded.log
    );

    // Control: nothing hides the Shaman, and the Priest, in range, stands.
    let open = play(config("BasicArena"), stage(BASIC_SHIFT));
    assert!(
        walked(&open) < 1.0,
        "in sight, the Priest walked {:.1}yd",
        walked(&open)
    );
}

/// The Hunt (AS-394), with a Shadowform Priest in it: `tests/hunt_staged.rs`'s
/// endgame, our Priest in Shadowform. A lone enemy Priest is rooted on the
/// east face of Nagrand's south-east pillar; our Mage and Shadowform Priest
/// stand west of it. Both are ranged damage dealers, so both take stations,
/// and the Priest walks to its own (`MovementTrigger::Hunt`) — from the
/// Shadowform arm of the dispatch, which has no posture machine to do it.
/// Control: the same endgame with the play excepted everywhere on the team:
/// the hunt is classified, but the Priest walks to no station.
#[test]
fn a_shadowform_priest_takes_a_hunt_station() {
    use arenasim::states::play_match::class_ai::plays::every_exception_entry;
    use arenasim::states::play_match::situation::PlayId;
    use arenasim::states::play_match::{Aura, AuraType};

    const MAGE: Unit = Unit::new(1, 0);
    const PRIEST: Unit = Unit::new(1, 1);
    const HIDER: Unit = Unit::new(2, 0);

    let config = |hunt: bool| HeadlessMatchConfig {
        team1: vec!["Mage".into(), "Priest".into()],
        team2: vec!["Priest".into()],
        team1_builds: vec![
            Build::default(),
            Build::of(&[(TalentId::ImprovedMindBlast, 2), (TalentId::Shadowform, 1)]),
        ],
        team1_priest_forms: vec!["NoForm".into(), "Shadowform".into()],
        map: "PillaredArena".into(),
        ai_profile: Some("TeamPlan".into()),
        team1_exceptions: if hunt {
            Default::default()
        } else {
            [("Hunt".into(), every_exception_entry(PlayId::Hunt))].into()
        },
        random_seed: Some(1),
        max_duration_secs: 30.0,
        ..Default::default()
    };
    let root = || Aura {
        effect_type: AuraType::Root,
        duration: 60.0,
        break_on_damage_threshold: -1.0,
        ability_name: "Entangling Roots".into(),
        ..Default::default()
    };
    let stage = || {
        Stage::new()
            .place(HIDER, 47.0, -20.0)
            .aura(HIDER, root())
            .place(MAGE, 16.0, -18.0)
            .target(MAGE, HIDER)
            .place(PRIEST, 12.0, -23.0)
            .target(PRIEST, HIDER)
            .target(HIDER, MAGE)
    };
    // The hunt onsets on team 1, and the Shadowform Priest's station walks.
    let read = |played: &Played| {
        let onsets = played
            .staged_trace()
            .filter(|e| {
                e["kind"] == "situation_change"
                    && e["name"] == "hunt"
                    && e["change"] == "onset"
                    && e["team"] == 1
            })
            .count();
        let walks = played
            .staged_trace()
            .filter(|e| {
                e["kind"] == "movement_decision"
                    && e["trigger"] == "Hunt"
                    && e["actor"]["team"] == 1
                    && e["actor"]["class"] == "Priest"
            })
            .count();
        (onsets, walks)
    };

    let hunting = play(config(true), stage());
    assert!(
        hunting.log.contains("Team 1 Priest #2 casts Shadowform"),
        "the Priest is in Shadowform"
    );
    let (onsets, walks) = read(&hunting);
    assert!(onsets >= 1, "the team never hunted");
    assert!(
        walks >= 1,
        "the Shadowform Priest never walked to a station"
    );

    let excepted = read(&play(config(false), stage()));
    assert!(excepted.0 >= 1, "excepted, the hunt never began");
    assert_eq!(
        excepted.1, 0,
        "excepted, the Priest still walked to a station"
    );
}
