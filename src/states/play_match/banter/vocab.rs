//! The banter vocabulary: how a pictographic line is written, parsed, and
//! kept renderable.
//!
//! Combatants do not speak English. A line is a short sequence of GAME
//! ICONOGRAPHY — ability art, team-tinted class portraits — stitched together
//! with a small set of symbol glyphs. `⚔ ➡ {priest}` reads as "attack the
//! Priest" in any language, and more importantly it reads in the game's own
//! visual language rather than borrowing a human one.
//!
//! ## Grammar
//!
//! A line is plain text with `{...}` tokens spliced in:
//!
//! | Token | Renders as |
//! |---|---|
//! | `{target}` | class portrait of the called enemy, tinted by ITS team |
//! | `{prev_target}` | the replaced target's portrait (`Correction` only) |
//! | `{speaker}` | the speaking combatant's own portrait |
//! | `{ability:Mortal Strike}` | that ability's icon, untinted |
//!
//! The three portrait tokens are AUTHORING tokens: the resolver rewrites them
//! into the resolved `{class:<Class>:<team>}` form once it knows who is
//! speaking about whom, and the renderer only ever sees the resolved form.
//! Everything outside a token is literal text.
//!
//! ## Why an allowlist
//!
//! egui ships a limited monochrome emoji subset, so most emoji render as tofu
//! boxes — `→`, `✓`, and `✗` all fail, while `➡`, `✔`, and `✖` work. The
//! difference is invisible when authoring and obvious in the client, which is
//! the worst possible place to find out. [`GLYPHS`] is the set verified to
//! render, and `BanterConfig::validate()` rejects any line using a character
//! outside it, so an unrenderable line cannot reach a match.
//!
//! Adding a glyph means proving it renders first — see `tests/glyph_probe.rs`
//! in the history of this branch for the harness that established this set.

use crate::states::match_config::CharacterClass;

/// Symbol glyphs verified to render in egui's default fonts.
///
/// Grouped by the job each does in a line. Everything here is monochrome —
/// egui has no colour-emoji support — which suits the UI: colour is reserved
/// for the team tint on class portraits, so the symbols never compete with it.
pub const GLYPHS: &[char] = &[
    // Direction and negation — the grammar of a line.
    '➡', '✖', '⛔', '🚫',
    // Assent.
    '✔', '☑', '👍',
    // Emphasis and inquiry.
    '!', '‼', '⚠', '❗', '❓', '⁉', '…',
    // Combat nouns.
    '💀', '☠', '⚔', '🛡', '❤', '♥',
    // Timing.
    '⏱', '⌛',
    // Flavour.
    '★', '☆', '⭐', '⚡', '🔥', '❄', '🎯', '💬',
];

// Deliberately NOT approved, having been rendered and rejected rather than
// assumed: `👀` draws as two dots barely visible at bubble size, and `→ ✓ ✗ ⋯
// ⊘ 🗣` draw as empty boxes. `❓` and `🎯` cover what `👀` was reaching for.

/// Whether a character may appear as literal text in a banter line.
///
/// Spaces are allowed as separators. Everything else must be an approved
/// glyph — ordinary letters are rejected too, because the whole point is that
/// combatants communicate pictographically and a stray English word would
/// break the conceit as loudly as a tofu box would.
pub fn is_speakable(c: char) -> bool {
    c == ' ' || GLYPHS.contains(&c)
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
        // `split(':')` on the ability name would cut a name containing a colon,
        // so take everything after the first separator verbatim.
        Some("ability") => match body.split_once(':') {
            Some((_, name)) if !name.is_empty() => Span::Ability(name.to_string()),
            _ => Span::Unknown,
        },
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
    fn plain_glyphs_are_one_text_span() {
        assert_eq!(parse("⚔ ➡"), vec![Span::Text("⚔ ➡".into())]);
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
            parse("⚔ ➡ {class:Mage:1}!"),
            vec![
                Span::Text("⚔ ➡ ".into()),
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
        assert_eq!(parse("⚔ {class"), vec![Span::Text("⚔ {class".into())]);
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
    fn letters_are_not_speakable_but_approved_glyphs_are() {
        assert!(is_speakable('⚔'));
        assert!(is_speakable(' '));
        assert!(!is_speakable('a'), "combatants do not speak English");
        assert!(!is_speakable('→'), "the plain arrow renders as tofu");
        assert!(is_speakable('➡'), "the heavy arrow is the one that renders");
    }
}
