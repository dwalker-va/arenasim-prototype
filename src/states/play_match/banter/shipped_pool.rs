//! Resolution tests over the SHIPPED `banter.ron`.
//!
//! The resolver and scheduler suites deliberately use hand-built pools, so a
//! content edit can never fail a logic test. These are the opposite: they pin
//! what the real pool can say to whom, by resolving it across every lineup,
//! so a gated line that leaks to the wrong speaker or target fails here.

use bevy::prelude::Entity;

use super::super::banter_config::{load_banter_config, BanterConfig, BanterContext};
use super::super::match_config::CharacterClass;
use super::resolver::{
    cc_target_class, resolve_exchange, BanterCall, BanterCombatant, BanterLineup,
    ResolvedExchange,
};

/// Seeds tried per lineup. Enough that every satisfiable entry in a pool of
/// a few dozen is drawn many times over.
const SEEDS: u64 = 96;

/// One beat of a resolved exchange, with who said it and against whom.
struct Spoken {
    speaker: CharacterClass,
    target: Option<CharacterClass>,
    text: String,
}

fn team(classes: &[CharacterClass], first_id: u32) -> Vec<BanterCombatant> {
    classes
        .iter()
        .enumerate()
        .map(|(index, class)| BanterCombatant {
            entity: Entity::from_raw(first_id + index as u32),
            class: *class,
            alive: true,
        })
        .collect()
}

/// Resolve one team's opening call exactly as the scheduler builds it: the
/// called slot indexes the enemy roster, and `{cctarget}` comes from the same
/// roster.
fn resolve(
    config: &BanterConfig,
    allies: &[CharacterClass],
    enemies: &[CharacterClass],
    called: usize,
    context: BanterContext,
    seed: u64,
) -> Option<(BanterLineup, BanterCall, ResolvedExchange)> {
    let lineup = BanterLineup {
        team: 1,
        allies: team(allies, 1),
    };
    let enemy_roster = team(enemies, 100);
    let call = BanterCall {
        target: enemies.get(called).copied(),
        prev_target: None,
        cc_target: cc_target_class(&enemy_roster, Some(called)),
        enemy_team: 2,
    };
    let resolved = resolve_exchange(config, &lineup, call, context, Some(seed), 0)?;
    Some((lineup, call, resolved))
}

/// Every beat the shipped pool speaks across every 2v2 lineup (allies any
/// two classes, enemies the called class plus a second), every context and
/// `SEEDS` seeds.
fn everything_said() -> Vec<Spoken> {
    let config = load_banter_config().expect("assets/config/banter.ron must load");
    let classes = CharacterClass::all();
    let mut said = Vec::new();
    for &a in classes {
        for &b in classes {
            for &target in classes {
                // A second enemy, so `{cctarget}` entries are satisfiable too.
                let enemies = [target, CharacterClass::Warrior];
                for context in BanterContext::all() {
                    for seed in 0..SEEDS {
                        let Some((lineup, call, resolved)) =
                            resolve(&config, &[a, b], &enemies, 0, *context, seed)
                        else {
                            continue;
                        };
                        for beat in resolved.beats {
                            let speaker = lineup
                                .allies
                                .iter()
                                .find(|ally| ally.entity == beat.speaker)
                                .expect("a beat's speaker is bound from its lineup")
                                .class;
                            said.push(Spoken {
                                speaker,
                                target: call.target,
                                text: beat.text,
                            });
                        }
                    }
                }
            }
        }
    }
    said
}

/// The Shaman's purge call on a Druid's Rejuvenation / Innervate is spoken by
/// a Shaman, against a Druid, and by nobody else — and it IS spoken, so the
/// gate is not passing on an unreachable line.
///
/// The anti-Druid opener calls out stopping the Cyclone, and that call is only
/// ever made against a Druid.
#[test]
fn gated_druid_lines_resolve_only_for_their_speaker_and_target() {
    let said = everything_said();

    let purges: Vec<&Spoken> = said
        .iter()
        .filter(|s| s.text.contains("{ability:Purge}"))
        .collect();
    assert!(!purges.is_empty(), "the Shaman purge line never resolved");
    for spoken in &purges {
        assert_eq!(
            spoken.speaker,
            CharacterClass::Shaman,
            "a non-Shaman spoke the purge line: {:?}",
            spoken.text
        );
        assert_eq!(
            spoken.target,
            Some(CharacterClass::Druid),
            "the purge line was spoken against {:?}",
            spoken.target
        );
    }

    let cyclones: Vec<&Spoken> = said
        .iter()
        .filter(|s| s.text.contains("{ability:Cyclone}"))
        .collect();
    assert!(!cyclones.is_empty(), "the stop-the-Cyclone line never resolved");
    for spoken in &cyclones {
        assert_eq!(
            spoken.target,
            Some(CharacterClass::Druid),
            "the Cyclone line was spoken against {:?}",
            spoken.target
        );
    }

    // The Cyclone call is an any-speaker line, so a team with no Shaman makes
    // it too.
    assert!(
        cyclones.iter().any(|s| s.speaker != CharacterClass::Shaman),
        "the Cyclone call is an any-speaker line"
    );
}
