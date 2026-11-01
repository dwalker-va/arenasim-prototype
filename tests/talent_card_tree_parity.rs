//! Build parity over the four slice classes' trees AS THEIR CARDS WRITE THEM
//! (AS-409..AS-422), before any of them is in `talents.ron`.
//!
//! The shipped table carries only the Demonology tier 1-2 proof, so
//! `tests/talents.rs`'s parity test cannot see the trees the mechanisms of
//! AS-331 and AS-408 serve. This holds their SHAPE to the §3 rules — tier
//! sizes, ranks per tier, prerequisites below their node in the same tree — and
//! every legal build at 2 to 8 points to within one rank (11) of the cheapest,
//! with every rank at its target price (11, a capstone 22): the rank-parity
//! test holds each shipped rank within 15% of that, and the tree cards ship
//! them.
//!
//! The enumeration mirrors `TalentDefinitions::legal_builds`: a node's tier
//! opens at 0 / 2 / 3 / 5 points in its tree's tiers below it, a prerequisite
//! must be met at its rank, and a build spends at most 8 points.

use std::collections::BTreeMap;

/// One node as a card writes it: (name, tree, tier, ranks, prerequisite).
type Node = (&'static str, &'static str, u8, u8, Option<(&'static str, u8)>);

const WARLOCK: &[Node] = &[
    ("Improved Drain Life", "Affliction", 1, 2, None),
    ("Improved Life Tap", "Affliction", 1, 2, None),
    ("Amplify Curse", "Affliction", 2, 1, None),
    ("Nightfall", "Affliction", 2, 2, None),
    ("Siphon Life", "Affliction", 3, 1, None),
    ("Curse of Exhaustion", "Affliction", 3, 1, None),
    ("Shadow Mastery", "Affliction", 4, 1, Some(("Siphon Life", 1))),
    ("Demonic Embrace", "Demonology", 1, 2, None),
    ("Fel Stamina", "Demonology", 1, 2, None),
    ("Unholy Power", "Demonology", 2, 2, None),
    ("Master Demonologist", "Demonology", 2, 1, None),
    ("Demonic Sacrifice", "Demonology", 3, 1, None),
    ("Soul Link", "Demonology", 4, 1, Some(("Fel Stamina", 2))),
    ("Improved Shadow Bolt", "Destruction", 1, 2, None),
    ("Bane", "Destruction", 1, 2, None),
    ("Aftermath", "Destruction", 2, 2, None),
    ("Shadowburn", "Destruction", 2, 1, None),
    ("Improved Immolate", "Destruction", 3, 1, None),
    ("Conflagrate", "Destruction", 4, 1, Some(("Improved Immolate", 1))),
];

const PRIEST: &[Node] = &[
    ("Unbreakable Will", "Discipline", 1, 2, None),
    ("Improved Power Word: Shield", "Discipline", 1, 2, None),
    ("Mental Agility", "Discipline", 2, 2, None),
    ("Inner Focus", "Discipline", 2, 1, None),
    ("Reflective Shield", "Discipline", 3, 1, None),
    ("Power Infusion", "Discipline", 3, 1, None),
    ("Pain Suppression", "Discipline", 4, 1, Some(("Reflective Shield", 1))),
    ("Holy Specialization", "Holy", 1, 2, None),
    ("Spiritual Healing", "Holy", 1, 2, None),
    ("Inspiration", "Holy", 2, 2, None),
    ("Blessed Recovery", "Holy", 2, 1, None),
    ("Holy Concentration", "Holy", 3, 1, None),
    ("Holy Nova", "Holy", 3, 1, None),
    ("Guardian Spirit", "Holy", 4, 1, Some(("Holy Concentration", 1))),
    ("Improved Mind Blast", "Shadow", 1, 2, None),
    ("Improved Psychic Scream", "Shadow", 1, 2, None),
    ("Shadowform", "Shadow", 2, 1, None),
    ("Blackout", "Shadow", 2, 2, None),
    ("Mind Flay", "Shadow", 3, 1, None),
    ("Vampiric Embrace", "Shadow", 3, 1, None),
    ("Silence", "Shadow", 4, 1, Some(("Shadowform", 1))),
];

const ROGUE: &[Node] = &[
    ("Vile Poisons", "Assassination", 1, 2, None),
    ("Improved Poisons", "Assassination", 1, 2, None),
    ("Master Poisoner", "Assassination", 2, 1, None),
    ("Improved Kidney Shot", "Assassination", 2, 2, None),
    ("Malice", "Assassination", 2, 2, None),
    ("Rupture", "Assassination", 3, 1, None),
    ("Blood Spatter", "Assassination", 4, 1, Some(("Rupture", 1))),
    ("Lightning Reflexes", "Combat", 1, 2, None),
    ("Dual Wield Specialization", "Combat", 1, 2, None),
    ("Aggression", "Combat", 2, 2, None),
    ("Vitality", "Combat", 2, 1, None),
    ("Deflection", "Combat", 2, 2, None),
    ("Swift Blades", "Combat", 3, 1, None),
    ("Expose Armor", "Combat", 4, 1, Some(("Swift Blades", 1))),
    ("Master of Subtlety", "Subtlety", 1, 2, None),
    ("Fleet Footed", "Subtlety", 1, 2, None),
    ("Initiative", "Subtlety", 2, 2, None),
    ("Improved Ambush", "Subtlety", 2, 2, None),
    ("Cheat Death", "Subtlety", 3, 1, None),
    ("Hemorrhage", "Subtlety", 3, 1, None),
    ("Shadow Dance", "Subtlety", 4, 1, Some(("Hemorrhage", 1))),
];

const PALADIN: &[Node] = &[
    ("Improved Holy Shock", "Holy", 1, 2, None),
    ("Healing Light", "Holy", 1, 2, None),
    ("Infusion of Light", "Holy", 2, 2, None),
    ("Divine Favor", "Holy", 2, 1, None),
    ("Purifying Power", "Holy", 2, 2, None),
    ("Light's Grace", "Holy", 3, 1, None),
    ("Aura Mastery", "Holy", 4, 1, Some(("Divine Favor", 1))),
    ("Sacred Duty", "Protection", 1, 2, None),
    ("Improved Hammer of Justice", "Protection", 1, 2, None),
    ("Divine Protection", "Protection", 2, 1, None),
    ("Retribution Aura", "Protection", 2, 2, None),
    ("Toughness", "Protection", 2, 2, None),
    ("Blessing of Sacrifice", "Protection", 3, 1, None),
    ("Sacred Shield", "Protection", 4, 1, Some(("Blessing of Sacrifice", 1))),
    ("Conviction", "Retribution", 1, 2, None),
    ("Pursuit of Justice", "Retribution", 1, 2, None),
    ("Two-Handed Weapon Specialization", "Retribution", 2, 2, None),
    ("Vengeance", "Retribution", 2, 2, None),
    ("The Art of War", "Retribution", 3, 1, None),
    ("Repentance", "Retribution", 3, 1, None),
    ("Crusader Strike", "Retribution", 4, 1, Some(("Vengeance", 1))),
];

const TIER_OPENS_AT: [u32; 4] = [0, 2, 3, 5];
const MAX_POINTS: u32 = 8;
const RANK: f32 = 11.0;
const CAPSTONE: f32 = 22.0;

/// The §3 shape rules, on one class's nodes.
fn check_shape(class: &str, nodes: &[Node]) {
    let mut per_tree: BTreeMap<&str, Vec<&Node>> = BTreeMap::new();
    for n in nodes {
        per_tree.entry(n.1).or_default().push(n);
    }
    assert_eq!(per_tree.len(), 3, "{class}: three trees");
    for (tree, tree_nodes) in &per_tree {
        let count = |tier: u8| tree_nodes.iter().filter(|n| n.2 == tier).count();
        assert_eq!(count(1), 2, "{class} {tree}: two tier-1 nodes");
        assert!((1..=3).contains(&count(2)), "{class} {tree}: tier 2");
        assert!((1..=2).contains(&count(3)), "{class} {tree}: tier 3");
        assert_eq!(count(4), 1, "{class} {tree}: one capstone");
        for n in tree_nodes {
            let cap = if n.2 <= 2 { 2 } else { 1 };
            assert!(n.3 >= 1 && n.3 <= cap, "{class} {}: ranks", n.0);
            if let Some((req, rank)) = n.4 {
                let r = tree_nodes
                    .iter()
                    .find(|m| m.0 == req)
                    .unwrap_or_else(|| panic!("{class} {}: requires {req} in its tree", n.0));
                assert!(r.2 < n.2 && rank >= 1 && rank <= r.3, "{class} {}", n.0);
            }
        }
    }
}

fn legal(nodes: &[Node], ranks: &[u8]) -> bool {
    nodes.iter().zip(ranks).all(|(n, r)| {
        if *r == 0 {
            return true;
        }
        let below: u32 = nodes
            .iter()
            .zip(ranks)
            .filter(|(m, _)| m.1 == n.1 && m.2 < n.2)
            .map(|(_, r)| u32::from(*r))
            .sum();
        let open = below >= TIER_OPENS_AT[usize::from(n.2 - 1)];
        let req = n.4.is_none_or(|(name, rank)| {
            nodes
                .iter()
                .zip(ranks)
                .any(|(m, r)| m.0 == name && *r >= rank)
        });
        open && req
    })
}

/// Every legal build's cost at each point count, as (count, costs).
fn costs(nodes: &[Node]) -> BTreeMap<u32, Vec<f32>> {
    let mut out: BTreeMap<u32, Vec<f32>> = BTreeMap::new();
    let mut ranks = vec![0u8; nodes.len()];
    fn walk(
        nodes: &[Node],
        i: usize,
        spent: u32,
        ranks: &mut Vec<u8>,
        out: &mut BTreeMap<u32, Vec<f32>>,
    ) {
        if i == nodes.len() {
            if spent >= 2 && legal(nodes, ranks) {
                let cost = nodes
                    .iter()
                    .zip(ranks.iter())
                    .map(|(n, r)| f32::from(*r) * if n.2 == 4 { CAPSTONE } else { RANK })
                    .sum();
                out.entry(spent).or_default().push(cost);
            }
            return;
        }
        for r in 0..=nodes[i].3 {
            if spent + u32::from(r) > MAX_POINTS {
                break;
            }
            ranks[i] = r;
            walk(nodes, i + 1, spent + u32::from(r), ranks, out);
        }
        ranks[i] = 0;
    }
    walk(nodes, 0, 0, &mut ranks, &mut out);
    out
}

#[test]
fn every_slice_class_tree_holds_build_parity_as_its_cards_write_it() {
    for (class, nodes) in [
        ("Warlock", WARLOCK),
        ("Priest", PRIEST),
        ("Rogue", ROGUE),
        ("Paladin", PALADIN),
    ] {
        check_shape(class, nodes);
        let by_count = costs(nodes);
        assert_eq!(
            by_count.keys().copied().collect::<Vec<_>>(),
            (2..=MAX_POINTS).collect::<Vec<_>>(),
            "{class}: a legal build at every count"
        );
        for (points, costs) in by_count {
            let (lo, hi) = costs
                .iter()
                .fold((f32::MAX, f32::MIN), |(lo, hi), c| (lo.min(*c), hi.max(*c)));
            assert!(
                hi - lo <= RANK + 1e-3,
                "{class} at {points} points: {lo} to {hi} over {} builds",
                costs.len()
            );
            // Up to 5 points no capstone opens, so every build costs the same.
            if points <= 5 {
                assert_eq!(hi, lo, "{class} at {points}");
            }
        }
    }
}

/// The enumeration is not vacuous: a capstone opens at 6 points, and the
/// prerequisite is enforced (Soul Link without Fel Stamina 2/2 is illegal).
#[test]
fn the_enumeration_opens_capstones_and_enforces_prerequisites() {
    let names: Vec<&str> = WARLOCK.iter().map(|n| n.0).collect();
    let at = |picks: &[(&str, u8)]| -> Vec<u8> {
        names
            .iter()
            .map(|n| picks.iter().find(|(p, _)| p == n).map_or(0, |(_, r)| *r))
            .collect()
    };
    let link = at(&[
        ("Demonic Embrace", 2),
        ("Fel Stamina", 2),
        ("Unholy Power", 1),
        ("Demonic Sacrifice", 1),
        ("Soul Link", 1),
    ]);
    assert!(legal(WARLOCK, &link));
    let no_prereq = at(&[
        ("Demonic Embrace", 2),
        ("Fel Stamina", 1),
        ("Unholy Power", 2),
        ("Demonic Sacrifice", 1),
        ("Soul Link", 1),
    ]);
    assert!(!legal(WARLOCK, &no_prereq), "Soul Link needs Fel Stamina 2/2");
    let early = at(&[("Demonic Embrace", 2), ("Soul Link", 1)]);
    assert!(!legal(WARLOCK, &early), "the capstone opens at 5 points below it");
    let six = costs(WARLOCK).remove(&6).unwrap();
    assert!(six.contains(&(5.0 * RANK + CAPSTONE)), "a capstone build at 6");
    assert!(six.contains(&(6.0 * RANK)));
}
