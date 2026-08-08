//! The banter vocabulary: how a pictographic line is written, parsed, and
//! kept renderable.
//!
//! Combatants do not speak English. A line is a short sequence of GAME
//! ICONOGRAPHY — real ability art and team-framed class portraits — joined by
//! drawn grammar marks. `{ability:Mortal Strike} {sym:arrow} {target}` reads
//! as "attack that one" in no language at all, in the visual vocabulary the
//! player already learned from the ability bar.
//!
//! ## Grammar
//!
//! A line is punctuation with `{...}` tokens spliced in:
//!
//! | Token | Renders as |
//! |---|---|
//! | `{target}` | class portrait of the called enemy, framed by ITS team |
//! | `{prev_target}` | the replaced target's portrait (`Correction` only) |
//! | `{speaker}` | the speaking combatant's own portrait |
//! | `{ability:Mortal Strike}` | that ability's real icon art |
//! | `{sym:arrow}` / `{sym:no}` / `{sym:yes}` | a drawn grammar mark |
//!
//! The three portrait tokens are AUTHORING tokens: the resolver rewrites them
//! into the resolved `{class:<Class>:<team>}` form once it knows who is
//! speaking about whom, and the renderer only ever sees the resolved form.
//!
//! ## Why there are no emoji here
//!
//! The first version of this vocabulary was built on emoji, and it failed
//! twice over. First on coverage: egui carries a limited subset, so `→`, `✓`,
//! and `✗` rendered as empty boxes while `➡`, `✔`, and `✖` happened to work —
//! a difference invisible while authoring and glaring in the client. Then on
//! recognisability, which was the fatal one: everything egui *can* draw is
//! monochrome, because its font atlas is a single coverage channel
//! (`FontImage { pixels: Vec<f32> }`), so no font swap can ever produce colour
//! emoji here. Stripped of colour, the shapes were not familiar enough to
//! carry meaning at bubble size.
//!
//! So nothing pictographic is typeset any more. Nouns are real icon art the
//! player already knows, the three grammar marks are drawn as vector shapes at
//! whatever weight suits, and text carries only punctuation — `!` and `?` are
//! ordinary font characters, not emoji, and about as widely read as marks get.
//! [`GLYPHS`] is that punctuation set, and `BanterConfig::validate()` rejects
//! anything else so an off-conceit line cannot reach a match.
//!
//! `tests/glyph_probe.rs` renders the vocabulary offscreen for iteration.

use crate::states::match_config::CharacterClass;

/// Punctuation that may appear as literal text in a line.
///
/// NOT emoji. These are ordinary characters in egui's proportional font, which
/// is exactly why they survived: `!` and `?` are among the most universally
/// recognised marks there are, while the emoji equivalents (`❗`, `❓`) render
/// as unfamiliar monochrome shapes at bubble size. Everything pictographic is
/// now a real game icon or a drawn [`Symbol`]; text carries only punctuation.
pub const GLYPHS: &[char] = &['!', '?', '…', '.', ',', '·'];

/// Whether a character may appear as literal text in a banter line.
///
/// Spaces are allowed as separators. Everything else must be approved
/// punctuation — letters are rejected, because the whole point is that
/// combatants communicate pictographically and a stray English word would
/// break the conceit; emoji are rejected because egui's font atlas is a single
/// coverage channel (`FontImage { pixels: Vec<f32> }`), so no emoji can ever
/// render in colour here no matter which font is loaded, and the monochrome
/// fallbacks are not recognisable enough to carry meaning.
pub fn is_speakable(c: char) -> bool {
    c == ' ' || GLYPHS.contains(&c)
}

/// A grammar mark drawn as vector shapes rather than typeset.
///
/// These three carry the structure of a line — direct, negate, affirm — and
/// have no ability art to borrow and no recognisable ASCII form (`->`, `X`,
/// and `v` all read as debris). Drawing them means they are crisp at any size,
/// exactly the weight we choose, and immune to font coverage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Symbol {
    /// Directs the line at what follows: "…at this one".
    Arrow,
    /// Negation, refusal, "not that".
    No,
    /// Assent, "on it".
    Yes,
}

impl Symbol {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "arrow" => Some(Symbol::Arrow),
            "no" => Some(Symbol::No),
            "yes" => Some(Symbol::Yes),
            _ => None,
        }
    }
}

/// One renderable piece of a line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Span {
    /// Literal glyphs, drawn as text.
    Text(String),
    /// A class portrait, tinted by the team that owns it.
    Class(CharacterClass, u8),
    /// An ability icon, keyed by the ability's display name.
    Ability(String),
    /// A drawn grammar mark.
    Symbol(Symbol),
    /// A token that named something unresolvable — an ability with no icon, a
    /// portrait for a slot nobody occupies. Rendered as a neutral placeholder
    /// rather than dropped, so a content mistake is visible instead of silent.
    Unknown,
}

/// Split a resolved line into renderable spans.
///
/// Unclosed or unrecognised tokens become [`Span::Unknown`] rather than an
/// error: this runs per frame in the renderer, and a malformed line should
/// show a placeholder in one bubble, not take the client down. Validation at
/// load time is where authoring mistakes are supposed to be caught.
pub fn parse(line: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut text = String::new();
    let mut rest = line;

    while let Some(open) = rest.find('{') {
        text.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            // No closing brace: the rest is literal, brace included.
            text.push_str(&rest[open..]);
            rest = "";
            break;
        };
        if !text.is_empty() {
            spans.push(Span::Text(std::mem::take(&mut text)));
        }
        spans.push(parse_token(&after[..close]));
        rest = &after[close + 1..];
    }

    text.push_str(rest);
    if !text.is_empty() {
        spans.push(Span::Text(text));
    }
    spans
}

/// Reverse of `CharacterClass::name()`.
///
/// Lives here rather than as a method on `CharacterClass` so this pictographic
/// layer needs no change to shared simulation code.
fn class_from_name(name: &str) -> Option<CharacterClass> {
    CharacterClass::all()
        .iter()
        .copied()
        .find(|class| class.name() == name)
}

fn parse_token(body: &str) -> Span {
    let mut parts = body.split(':');
    match parts.next() {
        Some("class") => {
            let class = parts.next().and_then(class_from_name);
            let team = parts.next().and_then(|t| t.parse::<u8>().ok());
            match (class, team) {
                (Some(class), Some(team)) => Span::Class(class, team),
                _ => Span::Unknown,
            }
        }
        // `split(':')` on the ability name would cut a name containing a colon
        // (`Power Word: Shield`), so take everything after the first separator
        // verbatim.
        Some("ability") => match body.split_once(':') {
            Some((_, name)) if !name.is_empty() => Span::Ability(name.to_string()),
            _ => Span::Unknown,
        },
        Some("sym") => parts
            .next()
            .and_then(Symbol::from_name)
            .map_or(Span::Unknown, Span::Symbol),
        _ => Span::Unknown,
    }
}

/// The resolved token for a class portrait, as the resolver emits it.
pub fn class_token(class: CharacterClass, team: u8) -> String {
    format!("{{class:{}:{}}}", class.name(), team)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_punctuation_is_one_text_span() {
        assert_eq!(parse("! ?"), vec![Span::Text("! ?".into())]);
    }

    #[test]
    fn grammar_marks_parse_to_drawn_symbols() {
        assert_eq!(parse("{sym:arrow}"), vec![Span::Symbol(Symbol::Arrow)]);
        assert_eq!(parse("{sym:no}"), vec![Span::Symbol(Symbol::No)]);
        assert_eq!(parse("{sym:yes}"), vec![Span::Symbol(Symbol::Yes)]);
        assert_eq!(parse("{sym:nonsense}"), vec![Span::Unknown]);
    }

    /// Emoji are rejected outright now, not merely unapproved.
    ///
    /// egui's font atlas is a single coverage channel, so no emoji can render
    /// in colour here whatever font is loaded, and the monochrome fallbacks
    /// were not recognisable enough to carry meaning. Punctuation stayed
    /// because `!` and `?` are ordinary font characters, not emoji.
    #[test]
    fn emoji_are_not_speakable_but_punctuation_is() {
        assert!(is_speakable('!'));
        assert!(is_speakable('?'));
        assert!(is_speakable('…'));
        assert!(is_speakable(' '));
        assert!(!is_speakable('a'), "combatants do not speak English");
        assert!(!is_speakable('⚔'), "nouns are real ability art now");
        assert!(!is_speakable('➡'), "grammar marks are drawn, not typeset");
        assert!(!is_speakable('→'), "and this one never rendered at all");
    }

    #[test]
    fn a_class_token_carries_its_team() {
        assert_eq!(
            parse("{class:Priest:2}"),
            vec![Span::Class(CharacterClass::Priest, 2)]
        );
    }

    #[test]
    fn tokens_and_text_interleave_in_order() {
        assert_eq!(
            parse("{sym:arrow} {class:Mage:1}!"),
            vec![
                Span::Symbol(Symbol::Arrow),
                Span::Text(" ".into()),
                Span::Class(CharacterClass::Mage, 1),
                Span::Text("!".into()),
            ]
        );
    }

    #[test]
    fn an_ability_name_may_contain_spaces() {
        assert_eq!(
            parse("{ability:Mortal Strike}"),
            vec![Span::Ability("Mortal Strike".into())]
        );
    }

    /// A malformed token degrades to a visible placeholder rather than
    /// panicking or vanishing — the renderer runs every frame and a content
    /// mistake should cost one bubble, not the client.
    #[test]
    fn malformed_tokens_become_unknown_not_a_panic() {
        assert_eq!(parse("{class:Nonexistent:1}"), vec![Span::Unknown]);
        assert_eq!(parse("{class:Priest}"), vec![Span::Unknown]);
        assert_eq!(parse("{ability:}"), vec![Span::Unknown]);
        assert_eq!(parse("{mystery}"), vec![Span::Unknown]);
    }

    #[test]
    fn an_unclosed_brace_is_literal_text() {
        assert_eq!(parse("! {class"), vec![Span::Text("! {class".into())]);
    }

    #[test]
    fn the_empty_line_has_no_spans() {
        assert!(parse("").is_empty());
    }

    #[test]
    fn class_token_round_trips_through_the_parser() {
        let token = class_token(CharacterClass::Warlock, 1);
        assert_eq!(
            parse(&token),
            vec![Span::Class(CharacterClass::Warlock, 1)]
        );
    }

    #[test]
    fn an_unclosed_brace_after_a_symbol_is_literal() {
        assert_eq!(
            parse("{sym:no} {class"),
            vec![Span::Symbol(Symbol::No), Span::Text(" {class".into())]
        );
    }
}
