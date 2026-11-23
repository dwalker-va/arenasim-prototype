//! Cutting a top-level block out of `talents.ron`'s text.
//!
//! Tests that edit a tree draw the file without its `reference_builds`: an
//! edit to a tree would leave a reference build taking it illegal, and the
//! loader would refuse the reference before the rule under test. The block is
//! cut by matching its brackets, so the cut does not depend on where in the
//! file the block sits.

/// `text` with the top-level field `key` (`"reference_builds"`) cut out: from
/// the start of its line through its closing bracket, its trailing comma and
/// the rest of that line. Brackets inside comments and strings are skipped.
/// Panics when the field is absent or its brackets do not close.
pub fn without_block(text: &str, key: &str) -> String {
    let needle = format!("\n    {key}: ");
    let at = text
        .find(&needle)
        .unwrap_or_else(|| panic!("talents.ron has no top-level `{key}`"))
        + 1;
    let open = at + needle.len() - 1;
    let end =
        closing_bracket(text, open).unwrap_or_else(|| panic!("`{key}`'s brackets do not close"));
    let rest = &text[end + 1..];
    let rest = rest.strip_prefix(',').unwrap_or(rest);
    let line_end = rest.find('\n').map_or(rest.len(), |i| i + 1);
    format!("{}{}", &text[..at], &rest[line_end..])
}

/// `text` without its `reference_builds` block.
pub fn without_reference_builds(text: &str) -> String {
    without_block(text, "reference_builds")
}

/// The byte index of the bracket closing the one at or after `from`.
fn closing_bracket(text: &str, from: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut i = from;
    while i < bytes.len() && !matches!(bytes[i], b'{' | b'[' | b'(') {
        i += 1;
    }
    let mut depth = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i += 2;
                continue;
            }
            b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
            }
            b'{' | b'[' | b'(' => depth += 1,
            b'}' | b']' | b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}
