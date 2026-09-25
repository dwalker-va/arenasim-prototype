//! Lands-silently audits: every ability in `abilities.ron` that lands
//! something — damage, a DoT, a crowd-control effect, a buff or debuff, a
//! dispel, an interrupt, a heal — must put SOME visual in the scene when it
//! does, or be named on a known-silent list with the card that will fix it.
//!
//! # Why this exists
//!
//! Immolate's 15-second burn rendered nothing, and every test was green. The
//! DoT visuals were routed by exact string match with a silent fallthrough,
//! and only heals and projectile landings had a sweep that failed when a
//! member landed with no visual. This file is the sweep for every family.
//!
//! # The shape of each sweep — two claims, two tests
//!
//! Each family SCANS the config for its subjects, so each has two claims,
//! written as separate tests because they fail for different reasons and the
//! second is worth nothing without the first:
//!
//! 1. **`<family>_finds_every_member`** — the scan finds exactly the NAMED
//!    members (set equality, never a count or a floor). A new ability joining
//!    the family must be added by name; a predicate that breaks and finds
//!    nothing fails instead of passing vacuously.
//! 2. **`<family>_lands_nothing_silently`** — the family's judge finds a visual
//!    for every member except exactly the `KNOWN_SILENT` ones. Set equality in
//!    BOTH directions: a new silent member fails, AND a known-silent member
//!    that has since gained a visual fails — so the list can only shrink and
//!    can never go stale. Each entry names the card that clears it.
//!
//! On top of the families, `every_ability_belongs_to_a_family` makes the
//! families a cover of the whole config: an ability that no family scans is
//! a failure unless it is named in `NO_FAMILY` with the visual that covers it.
//! The harness's own failure modes are proved by planted offenders at the
//! bottom of the file.
//!
//! # What a judge accepts as "a visual"
//!
//! Something in world space that the ability's landing routes to — through a
//! router that names the ability or its aura type (`SchoolImpact::anchor_for`,
//! `bolt_kind_for`, `InstantAbilityFired::is_spawned_for`,
//! `DotStateVisual::for_dot`, `HealImpact::kind_for`, the curse table), or an
//! explicitly named bespoke branch. GENERIC visuals count (a stock
//! school-impact row is a visual); a channel every ability shares regardless
//! of which one was used (the auto-attack swing, floating combat text, the
//! team-frame icon) does not, because it cannot tell the player the ability
//! landed. Heroic Strike is the example: its bonus rides the ordinary swing,
//! so casting it changes nothing on screen.

use std::collections::BTreeSet;

use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::{AbilityConfig, AbilityDefinitions};
use arenasim::states::play_match::class_ai::shaman::totem_spec;
use arenasim::states::play_match::components::{
    AuraType, CurseKind, HealImpact, InstantAbilityFired, SchoolImpact, TotemElement,
};
use arenasim::states::play_match::{bolt_kind_for, curse_spec, DotStateVisual};

use AbilityType::*;

// ── the harness ─────────────────────────────────────────────────────────────

/// A judge: the visual an ability's landing reaches in this family, or `None`
/// if it lands silently.
type Judge = fn(AbilityType, &AbilityConfig) -> Option<&'static str>;

/// Claim 1: the scan found exactly the named members.
fn check_finds(
    family: &str,
    scanned: &BTreeSet<AbilityType>,
    named: &[AbilityType],
) -> Result<(), String> {
    let named: BTreeSet<AbilityType> = named.iter().copied().collect();
    let mut problems = Vec::new();
    for lost in named.difference(scanned) {
        problems.push(format!(
            "the {family} scan no longer finds {lost:?} — the scan predicate broke, or \
             {lost:?} left the family (then remove it from the {family} member list)"
        ));
    }
    for new in scanned.difference(&named) {
        problems.push(format!(
            "{new:?} is now a {family} member — add it to the {family} member list \
             (and check the {family} judge draws it)"
        ));
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("\n"))
    }
}

/// Claim 2: the judge draws every member except exactly the known-silent
/// ones.
fn check_judged(
    family: &str,
    members: &BTreeSet<AbilityType>,
    judge: impl Fn(AbilityType) -> Option<&'static str>,
    known_silent: &[(AbilityType, &str)],
) -> Result<(), String> {
    let mut problems = Vec::new();
    let mut listed = BTreeSet::new();
    for (ability, card) in known_silent {
        if !listed.insert(*ability) {
            problems.push(format!(
                "{ability:?} is on the {family} known-silent list twice — remove one"
            ));
        }
        if !members.contains(ability) {
            problems.push(format!(
                "{ability:?} is on the {family} known-silent list ({card}) but is not a \
                 {family} member — remove it from that list"
            ));
        }
    }
    for ability in members {
        match (judge(*ability), listed.contains(ability)) {
            (None, false) => problems.push(format!(
                "{ability:?} lands with no {family} visual — give it one, or add \
                 ({ability:?}, \"<the card that will fix it>\") to the {family} \
                 known-silent list"
            )),
            (Some(visual), true) => problems.push(format!(
                "{ability:?} now draws `{visual}` — remove it from the {family} \
                 known-silent list"
            )),
            _ => {}
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("\n"))
    }
}

fn scan(pred: impl Fn(AbilityType, &AbilityConfig) -> bool) -> BTreeSet<AbilityType> {
    AbilityDefinitions::default()
        .iter()
        .filter(|(a, c)| pred(**a, c))
        .map(|(a, _)| *a)
        .collect()
}

/// Run a family's two checks against the real config.
fn assert_finds(
    family: &str,
    pred: fn(AbilityType, &AbilityConfig) -> bool,
    named: &[AbilityType],
) {
    if let Err(e) = check_finds(family, &scan(pred), named) {
        panic!("{e}");
    }
}

fn assert_judged(
    family: &str,
    pred: fn(AbilityType, &AbilityConfig) -> bool,
    judge: Judge,
    known_silent: &[(AbilityType, &str)],
) {
    let defs = AbilityDefinitions::default();
    let members = scan(pred);
    let judged = check_judged(
        family,
        &members,
        |a| judge(a, defs.get(&a).expect("member is in the config")),
        known_silent,
    );
    if let Err(e) = judged {
        panic!("{e}");
    }
}

// ── aura classification ─────────────────────────────────────────────────────

/// Which sweep an aura type belongs to. Exhaustive with no wildcard, on
/// purpose: a new `AuraType` does not compile until someone decides which
/// family judges it.
#[derive(Clone, Copy, PartialEq, Debug)]
enum AuraFamily {
    /// Damage over time — judged by the DoT router.
    Dot,
    /// Takes control away (stuns, roots, fears, incapacitates, silences,
    /// slows) — judged by the crowd-control sweep.
    Control,
    /// Every other buff or debuff — judged by the aura-application sweep.
    Status,
}

fn aura_family(t: AuraType) -> AuraFamily {
    match t {
        AuraType::DamageOverTime => AuraFamily::Dot,
        AuraType::Stun
        | AuraType::Root
        | AuraType::Fear
        | AuraType::Polymorph
        | AuraType::Incapacitate
        | AuraType::Silence
        | AuraType::SpellSchoolLockout
        | AuraType::MovementSpeedSlow
        | AuraType::AttackSpeedSlow => AuraFamily::Control,
        AuraType::MaxHealthIncrease
        | AuraType::HealingReduction
        | AuraType::MaxManaIncrease
        | AuraType::AttackPowerIncrease
        | AuraType::ShadowSight
        | AuraType::Absorb
        | AuraType::WeakenedSoul
        | AuraType::DamageReduction
        | AuraType::CastTimeIncrease
        | AuraType::DamageTakenReduction
        | AuraType::DamageImmunity
        | AuraType::SpellResistanceBuff
        | AuraType::AttackPowerReduction
        | AuraType::CritChanceIncrease
        | AuraType::ManaRegenIncrease
        | AuraType::LockoutDurationReduction
        | AuraType::FrostArmorBuff
        | AuraType::WeaponPoison
        | AuraType::SpellPowerIncrease
        | AuraType::HealingOverTime
        | AuraType::WindfuryBuff
        | AuraType::FearImmunity => AuraFamily::Status,
    }
}

/// Every aura an ability grants: its RON `applies_aura`, plus — for a totem,
/// whose buff is pulsed by code rather than declared in the RON — the aura
/// its `totem_spec` pulses (the same data gameplay reads).
fn granted_auras(ability: AbilityType, config: &AbilityConfig) -> Vec<AuraType> {
    let mut auras: Vec<AuraType> = config.applies_aura.iter().map(|a| a.aura_type).collect();
    for element in TotemElement::ALL {
        let (totem_ability, aura, _, _) = totem_spec(element);
        if totem_ability == ability {
            auras.push(aura);
        }
    }
    auras
}

fn grants(ability: AbilityType, config: &AbilityConfig, family: AuraFamily) -> bool {
    granted_auras(ability, config)
        .into_iter()
        .any(|t| aura_family(t) == family)
}

// ── family: heals ───────────────────────────────────────────────────────────

fn is_heal(_: AbilityType, c: &AbilityConfig) -> bool {
    c.is_heal()
}

const HEAL_MEMBERS: &[AbilityType] = &[
    FlashHeal,
    FlashOfLight,
    HolyLight,
    HolyShock,
    LesserHealingWave,
];

fn heal_landing(a: AbilityType, _: &AbilityConfig) -> Option<&'static str> {
    HealImpact::kind_for(a).map(|_| "heal landing (heal_impact.rs)")
}

#[test]
fn heal_finds_every_member() {
    assert_finds("heal", is_heal, HEAL_MEMBERS);
}

#[test]
fn heal_lands_nothing_silently() {
    assert_judged("heal", is_heal, heal_landing, &[]);
}

// ── family: direct damage ───────────────────────────────────────────────────

/// Abilities whose damage is resolved in class AI rather than declared in the
/// config's damage fields, so no config predicate can see it.
const CODE_RESOLVED_DAMAGE: &[AbilityType] = &[HeroicStrike];

fn is_direct_damage(a: AbilityType, c: &AbilityConfig) -> bool {
    c.is_damage() || c.mana_burn_amount > 0.0 || CODE_RESOLVED_DAMAGE.contains(&a)
}

const DIRECT_DAMAGE_MEMBERS: &[AbilityType] = &[
    AimedShot,
    Ambush,
    ArcaneShot,
    DeathCoil,
    DrainLife,
    FrostNova,
    FrostShock,
    Frostbolt,
    HeroicStrike,
    HolyShock,
    Immolate,
    LightningBolt,
    ManaBurn,
    MindBlast,
    MortalStrike,
    Shadowbolt,
    SinisterStrike,
];

fn damage_landing(a: AbilityType, _: &AbilityConfig) -> Option<&'static str> {
    if bolt_kind_for(a).is_some() {
        return Some("bespoke bolt impact (spell_bolts.rs)");
    }
    if SchoolImpact::anchor_for(a).is_some() {
        return Some("school impact (school_impact.rs)");
    }
    if InstantAbilityFired::is_spawned_for(a) {
        return Some("instant-ability stroke (instant_ability.rs)");
    }
    // Bespoke landings spawned by an `ability ==` branch at the resolution
    // site. A new ability cannot reach these arms without being named here.
    match a {
        DeathCoil => Some("DeathCoilBurst (projectiles.rs → death_coil.rs)"),
        LightningBolt => Some("forked-arc strike (casting.rs → lightning_bolt.rs)"),
        Immolate => Some("landing flame burst (casting.rs → flame.rs)"),
        DrainLife => Some("drain beam (drain_life.rs, keyed on the channel's ability)"),
        _ => None,
    }
}

const DIRECT_DAMAGE_KNOWN_SILENT: &[(AbilityType, &str)] = &[(
    HeroicStrike,
    "no card yet — its bonus rides the ordinary auto-attack swing",
)];

#[test]
fn direct_damage_finds_every_member() {
    assert_finds("direct-damage", is_direct_damage, DIRECT_DAMAGE_MEMBERS);
}

#[test]
fn direct_damage_lands_nothing_silently() {
    assert_judged(
        "direct-damage",
        is_direct_damage,
        damage_landing,
        DIRECT_DAMAGE_KNOWN_SILENT,
    );
}

// ── family: damage over time ────────────────────────────────────────────────

fn is_dot(a: AbilityType, c: &AbilityConfig) -> bool {
    grants(a, c, AuraFamily::Dot)
}

const DOT_MEMBERS: &[AbilityType] = &[
    Corruption,
    CurseOfAgony,
    Immolate,
    Rend,
    SerpentSting,
    UnstableAffliction,
];

/// The DoT STATE a member's aura routes to. The aura carries the ability's
/// RON `name:`, so this sweeps the config name through the very router every
/// DoT renderer asks.
fn dot_state(_: AbilityType, c: &AbilityConfig) -> Option<&'static str> {
    DotStateVisual::for_dot(&c.name).map(|v| match v {
        DotStateVisual::Drip(_) => "affliction drips (affliction.rs)",
        DotStateVisual::CorruptionShroud => "Corruption shroud (warlock_dots.rs)",
        DotStateVisual::UnstableAfflictionState => "UA glow + crackle (warlock_dots.rs)",
        DotStateVisual::ImmolateBurn => "Immolate burn (immolate.rs)",
        DotStateVisual::ApplyApparitionOnly(_) => "curse apply apparition (warlock_dots.rs)",
    })
}

#[test]
fn dot_finds_every_member() {
    assert_finds("DoT", is_dot, DOT_MEMBERS);
}

#[test]
fn dot_lands_nothing_silently() {
    assert_judged("DoT", is_dot, dot_state, &[]);
}

/// The router names each state it draws — a pin on the judge, so a wrong
/// route (Immolate drawn as a bleed, say) fails even though it is not silent.
#[test]
fn the_dot_router_names_each_members_state() {
    use arenasim::states::play_match::components::DripKind;
    let defs = AbilityDefinitions::default();
    let route = |a: AbilityType| DotStateVisual::for_dot(&defs.get(&a).unwrap().name);
    assert_eq!(route(Rend), Some(DotStateVisual::Drip(DripKind::Bleed)));
    assert_eq!(
        route(SerpentSting),
        Some(DotStateVisual::Drip(DripKind::Poison))
    );
    assert_eq!(route(Corruption), Some(DotStateVisual::CorruptionShroud));
    assert_eq!(
        route(UnstableAffliction),
        Some(DotStateVisual::UnstableAfflictionState)
    );
    assert_eq!(route(Immolate), Some(DotStateVisual::ImmolateBurn));
    assert_eq!(
        route(CurseOfAgony),
        Some(DotStateVisual::ApplyApparitionOnly(CurseKind::Agony))
    );
    // A DoT nothing draws routes nowhere — the fallthrough is `None`, which
    // the sweep above fails on, never a default visual.
    assert_eq!(DotStateVisual::for_dot("Shadow Word: Pain"), None);
    // The curse table only answers for DoT curses: Weakness is not a DoT.
    assert_eq!(
        DotStateVisual::for_dot(curse_spec(CurseKind::Weakness).aura_name),
        None
    );
}

// ── family: crowd control ───────────────────────────────────────────────────

fn is_control(a: AbilityType, c: &AbilityConfig) -> bool {
    grants(a, c, AuraFamily::Control)
}

const CONTROL_MEMBERS: &[AbilityType] = &[
    BoarCharge,
    CheapShot,
    ConcussiveShot,
    CripplingPoison,
    DeathCoil,
    Fear,
    FreezingTrap,
    FrostNova,
    FrostShock,
    Frostbolt,
    HammerOfJustice,
    KidneyShot,
    Polymorph,
    PsychicScream,
    SpiderWeb,
];

/// The victim treatment of a member's control aura. Keyed on the aura TYPE
/// (the renderers poll `ActiveAuras` for it), except Incapacitate, whose ice
/// block is spawned by the trap trigger rather than by the aura.
fn control_treatment(a: AbilityType, c: &AbilityConfig) -> Option<&'static str> {
    let mut treatment = None;
    for t in granted_auras(a, c) {
        if aura_family(t) != AuraFamily::Control {
            continue;
        }
        let this = match t {
            AuraType::Stun => Some("stun whirl (hard_cc.rs)"),
            AuraType::Root => Some("root restraint (hard_cc.rs)"),
            AuraType::Fear => Some("fear shroud (fear.rs)"),
            AuraType::Polymorph => Some("sheep swap (polymorph.rs)"),
            AuraType::Incapacitate if a == FreezingTrap => Some("ice block (traps.rs)"),
            _ => None,
        };
        // Every control aura must be drawn for the member to pass.
        treatment = Some(this?);
    }
    treatment
}

const CONTROL_KNOWN_SILENT: &[(AbilityType, &str)] = &[
    // `MovementSpeedSlow` has no victim treatment anywhere.
    (Frostbolt, "AS-133 (slows)"),
    (ConcussiveShot, "AS-133 (slows)"),
    (FrostShock, "AS-133 (slows)"),
    (CripplingPoison, "AS-133 (slows)"),
];

#[test]
fn control_finds_every_member() {
    assert_finds("crowd-control", is_control, CONTROL_MEMBERS);
}

#[test]
fn control_lands_nothing_silently() {
    assert_judged(
        "crowd-control",
        is_control,
        control_treatment,
        CONTROL_KNOWN_SILENT,
    );
}

// ── family: buff / debuff application ───────────────────────────────────────

fn is_status(a: AbilityType, c: &AbilityConfig) -> bool {
    grants(a, c, AuraFamily::Status)
}

const STATUS_MEMBERS: &[AbilityType] = &[
    AimedShot,
    AirTotem,
    ArcaneIntellect,
    BattleShout,
    BerserkerRage,
    CommandingShout,
    ConcentrationAura,
    CurseOfTongues,
    CurseOfWeakness,
    DemoralizingShout,
    DevotionAura,
    DivineShield,
    EarthTotem,
    FireTotem,
    FrostArmor,
    IceBarrier,
    MageArmorSpell,
    MoltenArmor,
    MortalStrike,
    PowerWordFortitude,
    PowerWordShield,
    ShadowResistanceAura,
    WaterTotem,
];

/// The family-wide aura-application cue — what draws a status aura that no
/// bespoke effect owns. Nothing does today, which is the AS-134 gap. When
/// AS-134's `AuraApplyRoute` lands, this body becomes
/// `AuraApplyRoute::for_aura(t).is_drawn().then_some("…")` and the AS-134
/// entries below come off the known-silent list in the same diff (the sweep
/// fails until they do).
fn family_application_cue(_t: AuraType) -> Option<&'static str> {
    None
}

fn status_visual(a: AbilityType, c: &AbilityConfig) -> Option<&'static str> {
    let mut visual = None;
    for t in granted_auras(a, c) {
        if aura_family(t) != AuraFamily::Status {
            continue;
        }
        let curse = CurseKind::ALL.into_iter().any(|k| {
            let spec = curse_spec(k);
            spec.aura_type == t && spec.aura_name == c.name
        });
        let this = match t {
            _ if curse => Some("curse apply apparition (warlock_dots.rs)"),
            AuraType::Absorb | AuraType::DamageImmunity => {
                Some("shield bubble (shield_bubbles.rs)")
            }
            AuraType::HealingReduction => Some("heal-refused tell (mortal_wounds.rs)"),
            AuraType::HealingOverTime => {
                HealImpact::kind_for_hot_tick(t).map(|_| "per-tick heal pulse (heal_impact.rs)")
            }
            AuraType::FearImmunity if a == BerserkerRage => {
                Some("berserk mask (effects/berserker_rage.rs → berserk.rs)")
            }
            _ => family_application_cue(t),
        };
        // Every status aura must be drawn for the member to pass.
        visual = Some(this?);
    }
    visual
}

const STATUS_KNOWN_SILENT: &[(AbilityType, &str)] = &[
    (ArcaneIntellect, "AS-134 (aura application)"),
    (BattleShout, "AS-134 (aura application)"),
    (CommandingShout, "AS-134 (aura application)"),
    (ConcentrationAura, "AS-134 (aura application)"),
    (DemoralizingShout, "AS-134 (aura application)"),
    (DevotionAura, "AS-134 (aura application)"),
    (FrostArmor, "AS-134 (aura application)"),
    (MageArmorSpell, "AS-134 (aura application)"),
    (MoltenArmor, "AS-134 (aura application)"),
    (PowerWordFortitude, "AS-134 (aura application)"),
    (ShadowResistanceAura, "AS-134 (aura application)"),
    // The totem OBJECT is drawn; the buff it pulses onto allies is not.
    (AirTotem, "AS-134 (aura application — totem pulse)"),
    (EarthTotem, "AS-134 (aura application — totem pulse)"),
    (FireTotem, "AS-134 (aura application — totem pulse)"),
];

#[test]
fn status_finds_every_member() {
    assert_finds("aura-application", is_status, STATUS_MEMBERS);
}

#[test]
fn status_lands_nothing_silently() {
    assert_judged(
        "aura-application",
        is_status,
        status_visual,
        STATUS_KNOWN_SILENT,
    );
}

// ── family: dispels ─────────────────────────────────────────────────────────

fn is_dispel(_: AbilityType, c: &AbilityConfig) -> bool {
    c.is_dispel
}

const DISPEL_MEMBERS: &[AbilityType] =
    &[DevourMagic, DispelMagic, MastersCall, PaladinCleanse, Purge];

/// The ribbon is spawned by `process_dispels` (effects/dispels.rs) for every
/// successful `DispelPending`, so a dispel is drawn exactly when its
/// resolution site spawns one. That is a per-ability fact about the producer,
/// so it is named per ability: a new dispel is judged silent until someone
/// confirms its producer and adds it here.
fn dispel_visual(a: AbilityType, _: &AbilityConfig) -> Option<&'static str> {
    match a {
        // class_ai/priest.rs, class_ai/paladin.rs, class_ai/mod.rs (Purge)
        DispelMagic | PaladinCleanse | Purge => Some("dispel ribbon (DispelPending → dispels.rs)"),
        // class_ai/pet_ai.rs
        DevourMagic => Some("dispel ribbon (DispelPending → dispels.rs)"),
        // class_ai/pet_ai.rs — also a DispelBurst at the dispatch site.
        MastersCall => Some("dispel ribbon + DispelBurst (pet_ai.rs)"),
        _ => None,
    }
}

#[test]
fn dispel_finds_every_member() {
    assert_finds("dispel", is_dispel, DISPEL_MEMBERS);
}

#[test]
fn dispel_lands_nothing_silently() {
    assert_judged("dispel", is_dispel, dispel_visual, &[]);
}

// ── family: interrupts ──────────────────────────────────────────────────────

fn is_interrupt(_: AbilityType, c: &AbilityConfig) -> bool {
    c.is_interrupt
}

const INTERRUPT_MEMBERS: &[AbilityType] = &[Kick, Pummel, SpellLock, WindShear];

/// The INTERRUPTER's side. The victim's side is the casting-orb sputter
/// (`casting_orbs.rs`), which every interrupt shares and which cannot tell
/// the player who interrupted — so it does not count here.
fn interrupt_visual(a: AbilityType, _: &AbilityConfig) -> Option<&'static str> {
    InstantAbilityFired::is_spawned_for(a).then_some("interrupt stroke (instant_ability.rs)")
}

const INTERRUPT_KNOWN_SILENT: &[(AbilityType, &str)] = &[
    (SpellLock, "AS-137 (interrupts)"),
    (WindShear, "AS-137 (interrupts)"),
];

#[test]
fn interrupt_finds_every_member() {
    assert_finds("interrupt", is_interrupt, INTERRUPT_MEMBERS);
}

#[test]
fn interrupt_lands_nothing_silently() {
    assert_judged(
        "interrupt",
        is_interrupt,
        interrupt_visual,
        INTERRUPT_KNOWN_SILENT,
    );
}

// ── the families cover the config ───────────────────────────────────────────

/// Abilities no family scans, each with the visual that covers it. Anything
/// else outside every family fails `every_ability_belongs_to_a_family`.
const NO_FAMILY: &[(AbilityType, &str)] = &[
    (Charge, "a dash — charge trail (movement_trails.rs)"),
    (Disengage, "a leap — disengage trail (movement_trails.rs)"),
    (
        FrostTrap,
        "a placement — trap, trigger burst and slow-zone decal (traps.rs, ice_block.rs); \
         its slow is applied by the zone, not declared in the RON",
    ),
];

const FAMILIES: &[fn(AbilityType, &AbilityConfig) -> bool] = &[
    is_heal,
    is_direct_damage,
    is_dot,
    is_control,
    is_status,
    is_dispel,
    is_interrupt,
];

#[test]
fn every_ability_belongs_to_a_family() {
    let outside = scan(|a, c| !FAMILIES.iter().any(|f| f(a, c)));
    let named: Vec<AbilityType> = NO_FAMILY.iter().map(|(a, _)| *a).collect();
    if let Err(e) = check_finds("no-family", &outside, &named) {
        panic!(
            "{e}\nAn ability outside every family is outside every lands-silently \
             sweep. Widen a family's scan to cover it, or name it in NO_FAMILY with \
             the visual that covers it."
        );
    }
}

// ── the harness catches what it claims to catch (planted offenders) ─────────

fn set(xs: &[AbilityType]) -> BTreeSet<AbilityType> {
    xs.iter().copied().collect()
}

fn draws_all_but(silent: &'static [AbilityType]) -> impl Fn(AbilityType) -> Option<&'static str> {
    move |a| (!silent.contains(&a)).then_some("planted visual")
}

#[test]
fn harness_passes_a_clean_family() {
    let members = set(&[Frostbolt, Fear]);
    assert_eq!(check_finds("planted", &members, &[Fear, Frostbolt]), Ok(()));
    assert_eq!(
        check_judged(
            "planted",
            &members,
            draws_all_but(&[Fear]),
            &[(Fear, "AS-0")]
        ),
        Ok(())
    );
}

#[test]
fn harness_fails_a_silent_member_that_is_not_listed() {
    let err = check_judged(
        "planted",
        &set(&[Frostbolt, Fear]),
        draws_all_but(&[Fear]),
        &[],
    )
    .unwrap_err();
    assert!(err.contains("Fear lands with no planted visual"), "{err}");
    assert!(
        err.contains("add (Fear,"),
        "the message must say what to add: {err}"
    );
}

#[test]
fn harness_fails_a_listed_member_that_now_draws() {
    let err = check_judged(
        "planted",
        &set(&[Frostbolt, Fear]),
        draws_all_but(&[]),
        &[(Fear, "AS-0")],
    )
    .unwrap_err();
    assert!(err.contains("Fear now draws `planted visual`"), "{err}");
    assert!(
        err.contains("remove it from the planted known-silent list"),
        "{err}"
    );
}

#[test]
fn harness_fails_a_listed_ability_that_is_not_a_member() {
    let err = check_judged(
        "planted",
        &set(&[Frostbolt]),
        draws_all_but(&[]),
        &[(Fear, "AS-0")],
    )
    .unwrap_err();
    assert!(
        err.contains("Fear is on the planted known-silent list (AS-0) but is not"),
        "{err}"
    );
}

#[test]
fn harness_fails_a_duplicated_listing() {
    let err = check_judged(
        "planted",
        &set(&[Fear]),
        draws_all_but(&[Fear]),
        &[(Fear, "AS-0"), (Fear, "AS-0")],
    )
    .unwrap_err();
    assert!(err.contains("twice"), "{err}");
}

#[test]
fn harness_fails_a_scan_that_lost_a_member() {
    let err = check_finds("planted", &set(&[Frostbolt]), &[Frostbolt, Fear]).unwrap_err();
    assert!(err.contains("no longer finds Fear"), "{err}");
}

#[test]
fn harness_fails_a_scan_that_found_an_unnamed_member() {
    let err = check_finds("planted", &set(&[Frostbolt, Fear]), &[Frostbolt]).unwrap_err();
    assert!(err.contains("Fear is now a planted member"), "{err}");
}

/// The harness checks run against the real config see the real config: a
/// scan that finds nothing would pass claim 2 vacuously, so claim 1 must
/// fail on it.
#[test]
fn harness_fails_an_empty_scan() {
    let err = check_finds("planted", &BTreeSet::new(), DOT_MEMBERS).unwrap_err();
    for m in DOT_MEMBERS {
        assert!(err.contains(&format!("no longer finds {m:?}")), "{err}");
    }
}
