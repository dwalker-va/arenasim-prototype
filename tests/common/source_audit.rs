//! The shared **reading** layer for this repo's lexical source audits.
//!
//! Three audits ask different questions of the same text:
//!
//! * `tests/registration_audit.rs` — is every Bevy system under `play_match/`
//!   registered somewhere?
//! * `tests/snapshot_font_audit.rs` — does every `egui_kittest` harness install
//!   the client's fonts?
//! * `tests/icon_loader_registration_audit.rs` — does every state that reads a
//!   lazily-loaded resource register that resource's loader?
//!
//! The questions are theirs. The *reading* — finding the files, blanking the
//! comments, matching braces, expanding type aliases and `SystemParam` bundles,
//! and parsing the `.add_systems` calls in `StatesPlugin::build` — is the same
//! job three times, and it lives here so that a gap closed once is closed for
//! all three. Each audit keeps its own predicate, its own allowlist, its own
//! non-vacuity assertions and its own failure message.
//!
//! # What this layer is and is not
//!
//! It is a lexer-grade reader of raw source text, not a Rust parser. It knows
//! about comments, string literals and nesting depth; it does not know about
//! macro expansion, `cfg` resolution, or types reached through a trait. The
//! audits built on it are drift guards, and their own doc comments carry the
//! limitations that survive.
//!
//! Three gaps were reported against the audits separately, each found by a
//! different reviewer, each fixed in only the one audit that was reviewed.
//! They are closed here, once:
//!
//! 1. **`Res<'w, X>` inside a `#[derive(SystemParam)]` bundle.** A system that
//!    takes a bundle reads every resource the bundle holds, but the bundle's
//!    fields are in another `struct`, so a signature scan sees nothing. See
//!    [`SystemParamBundles`], which expands a bundle parameter into its field
//!    types, and the lifetime-tolerant resource patterns in [`resource_uses`].
//! 2. **Block comments.** [`blank_comments`] handles `//`, `/* */` (nested),
//!    and knows string and char literals well enough not to be fooled by a
//!    `"http://"` or a `'/'`.
//! 3. **Type aliases and unusual whitespace.** [`TypeAliases`] resolves both
//!    `type A = B;` and `use path::B as A;` before any pattern runs, and every
//!    pattern in this module is written `\s*`-tolerant so a spelling rustfmt
//!    would never produce is still read.
//!
//! # Offsets are preserved
//!
//! Every blanking function in this module returns a string the **same byte
//! length** as its input, with newlines kept in place: blanked bytes become
//! spaces. A line number computed against the blanked text is therefore the
//! line number in the file on disk, and offsets can be carried between the two.

use regex::{Captures, Regex};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Compile a fixed pattern once per process. Every audit runs its patterns over
/// a few hundred files, and recompiling a regex per file — or per parameter —
/// is most of what a lexical audit would otherwise spend its time on.
macro_rules! pattern {
    ($cell:ident, $src:expr) => {{
        static $cell: OnceLock<Regex> = OnceLock::new();
        $cell.get_or_init(|| Regex::new($src).expect("audit pattern must compile"))
    }};
}

// ---------------------------------------------------------------------------
// discovery
// ---------------------------------------------------------------------------

/// An absolute path to `rel` within the crate.
pub fn repo_path(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)
}

/// `path` rendered relative to the crate root, for failure messages that a
/// reader can paste into an editor.
pub fn rel_display(path: &Path) -> String {
    path.strip_prefix(env!("CARGO_MANIFEST_DIR"))
        .unwrap_or(path)
        .display()
        .to_string()
}

/// Every `.rs` file under `root` (recursively), sorted for a stable order.
pub fn rust_files(root: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    collect_rust_files(root, &mut out)?;
    out.sort();
    Ok(out)
}

fn collect_rust_files(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_rust_files(&path, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
    Ok(())
}

/// One source file, read once: the bytes on disk plus the comment-blanked view
/// the audits actually scan.
pub struct SourceFile {
    pub path: PathBuf,
    /// Exactly what is on disk.
    pub raw: String,
    /// `raw` with every comment blanked out. Same byte length, same lines.
    pub code: String,
}

impl SourceFile {
    pub fn read(path: &Path) -> std::io::Result<Self> {
        let raw = fs::read_to_string(path)?;
        let code = blank_comments(&raw);
        Ok(SourceFile {
            path: path.to_path_buf(),
            raw,
            code,
        })
    }

    /// `code` with `#[cfg(test)]` modules blanked as well.
    pub fn code_without_test_modules(&self) -> String {
        blank_test_modules(&self.code)
    }

    pub fn rel_display(&self) -> String {
        rel_display(&self.path)
    }
}

/// Read every `.rs` file under each crate-relative root, in a stable order.
pub fn load_sources(roots: &[&str]) -> std::io::Result<Vec<SourceFile>> {
    let mut out = Vec::new();
    for root in roots {
        for path in rust_files(&repo_path(root))? {
            out.push(SourceFile::read(&path)?);
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// lexing: blanking comments, strings and test modules
// ---------------------------------------------------------------------------

/// Blank every comment — `//` to end of line, and `/* */` including Rust's
/// nested form — replacing their bytes with spaces and keeping newlines.
///
/// String, byte-string, raw-string and char literals are recognised so that a
/// `"//"` or a `'/'` inside one cannot start a comment. Their contents are left
/// alone; use [`blank_comments_and_strings`] when literal text would otherwise
/// be read as code.
pub fn blank_comments(text: &str) -> String {
    blank(text, true, false)
}

/// [`blank_comments`], and blank the contents of string and char literals too
/// (the delimiters stay, so the token shape survives).
///
/// For an audit that scans for a pattern which can legitimately appear inside a
/// literal — a test that pins its own detector by feeding it example source, say
/// — this is the difference between reading the program and reading its prose.
pub fn blank_comments_and_strings(text: &str) -> String {
    blank(text, true, true)
}

fn blank(text: &str, comments: bool, strings: bool) -> String {
    let b = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0usize;

    // Blank one byte (or keep it), always preserving newlines.
    macro_rules! hide {
        ($idx:expr, $on:expr) => {{
            let byte = b[$idx];
            out.push(if byte == b'\n' || !$on { byte } else { b' ' });
        }};
    }

    while i < b.len() {
        // Line comment.
        if b[i] == b'/' && i + 1 < b.len() && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                hide!(i, comments);
                i += 1;
            }
            continue;
        }

        // Block comment, nested per Rust's rules.
        if b[i] == b'/' && i + 1 < b.len() && b[i + 1] == b'*' {
            let mut depth = 0usize;
            while i < b.len() {
                if b[i] == b'/' && i + 1 < b.len() && b[i + 1] == b'*' {
                    depth += 1;
                    hide!(i, comments);
                    hide!(i + 1, comments);
                    i += 2;
                    continue;
                }
                if b[i] == b'*' && i + 1 < b.len() && b[i + 1] == b'/' {
                    depth -= 1;
                    hide!(i, comments);
                    hide!(i + 1, comments);
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                    continue;
                }
                hide!(i, comments);
                i += 1;
            }
            continue;
        }

        // Raw string: [b]r#*"…"#*
        if let Some((prefix_len, hashes)) = raw_string_start(b, i) {
            out.extend_from_slice(&b[i..i + prefix_len]);
            i += prefix_len;
            loop {
                if i >= b.len() {
                    break;
                }
                if b[i] == b'"' && closing_hashes(b, i + 1, hashes) {
                    out.extend_from_slice(&b[i..i + 1 + hashes]);
                    i += 1 + hashes;
                    break;
                }
                hide!(i, strings);
                i += 1;
            }
            continue;
        }

        // Ordinary string (and byte string — the `b` prefix is just an ident
        // char, so it is already in `out` by the time we get here).
        if b[i] == b'"' {
            out.push(b'"');
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' && i + 1 < b.len() {
                    hide!(i, strings);
                    hide!(i + 1, strings);
                    i += 2;
                    continue;
                }
                if b[i] == b'"' {
                    out.push(b'"');
                    i += 1;
                    break;
                }
                hide!(i, strings);
                i += 1;
            }
            continue;
        }

        // `'` is a char literal or a lifetime. A lifetime is far more common in
        // this codebase (`Res<'w, X>`), and mistaking one for an unterminated
        // literal would swallow the rest of the file.
        if b[i] == b'\'' {
            if let Some(len) = char_literal_len(b, i) {
                out.push(b'\'');
                for &byte in &b[i + 1..i + len - 1] {
                    out.push(if byte == b'\n' || !strings {
                        byte
                    } else {
                        b' '
                    });
                }
                out.push(b'\'');
                i += len;
            } else {
                out.push(b'\'');
                i += 1;
            }
            continue;
        }

        out.push(b[i]);
        i += 1;
    }

    String::from_utf8(out).expect("blanking only ever emits ASCII or whole source bytes")
}

/// If a raw-string literal starts at `i`, its prefix length (`r#"` etc.) and
/// hash count. `None` when `r` is just part of an identifier (`for`, `char`).
fn raw_string_start(b: &[u8], i: usize) -> Option<(usize, usize)> {
    let mut k = i;
    if b[k] == b'b' {
        k += 1;
    }
    if k >= b.len() || b[k] != b'r' {
        return None;
    }
    // `i` must begin a token, or this `r` belongs to an identifier.
    if i > 0 && is_ident_byte(b[i - 1]) {
        return None;
    }
    k += 1;
    let hash_start = k;
    while k < b.len() && b[k] == b'#' {
        k += 1;
    }
    if k < b.len() && b[k] == b'"' {
        Some((k + 1 - i, k - hash_start))
    } else {
        None
    }
}

fn closing_hashes(b: &[u8], from: usize, hashes: usize) -> bool {
    (0..hashes).all(|n| from + n < b.len() && b[from + n] == b'#')
}

/// Length of the char literal starting at `i`, or `None` if that quote opens a
/// lifetime instead.
fn char_literal_len(b: &[u8], i: usize) -> Option<usize> {
    if i + 2 >= b.len() {
        return None;
    }
    if b[i + 1] == b'\\' {
        let mut k = i + 2;
        while k < b.len() && b[k] != b'\'' {
            k += 1;
        }
        return if k < b.len() { Some(k + 1 - i) } else { None };
    }
    // A single (possibly multi-byte) character followed by a closing quote.
    let mut k = i + 1;
    k += 1;
    while k < b.len() && (b[k] & 0b1100_0000) == 0b1000_0000 {
        k += 1; // UTF-8 continuation bytes
    }
    if k < b.len() && b[k] == b'\'' {
        Some(k + 1 - i)
    } else {
        None
    }
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Blank `#[cfg(test)] mod … { … }` bodies. Test modules take SystemParam types
/// in harness signatures and build harnesses of their own, so an audit asking
/// about shipping code has to exclude them.
pub fn blank_test_modules(text: &str) -> String {
    let opener = pattern!(
        TEST_MOD,
        r"(?m)#\[\s*cfg\s*\(\s*test\s*\)\s*\]\s*(?:#\[[^\]]*\]\s*)*(?:pub\s+)?mod\s+\w+\s*\{"
    );
    let b = text.as_bytes();
    let mut out: Vec<u8> = text.as_bytes().to_vec();
    let mut search_from = 0usize;
    while let Some(m) = opener.find_at(text, search_from) {
        let end = match_brace(b, m.end() - 1).unwrap_or(b.len());
        for (k, slot) in out.iter_mut().enumerate().take(end).skip(m.start()) {
            if b[k] != b'\n' {
                *slot = b' ';
            }
        }
        search_from = end;
    }
    String::from_utf8(out).expect("blanking only ever emits ASCII or whole source bytes")
}

// ---------------------------------------------------------------------------
// structure: braces, parens, parameter lists
// ---------------------------------------------------------------------------

/// Index one past the `}` matching the `{` at `open`.
fn match_brace(b: &[u8], open: usize) -> Option<usize> {
    debug_assert_eq!(b[open], b'{');
    let mut depth = 0usize;
    let mut i = open;
    while i < b.len() {
        match b[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// The body of the next `{ … }` at or after `from`, braces excluded.
pub fn brace_body(text: &str, from: usize) -> Option<String> {
    let b = text.as_bytes();
    let mut i = from;
    while i < b.len() && b[i] != b'{' {
        i += 1;
    }
    if i >= b.len() {
        return None;
    }
    let end = match_brace(b, i)?;
    Some(text[i + 1..end - 1].to_string())
}

/// The text between the parens of a parameter list whose `(` is at `open`.
pub fn param_list(text: &str, open: usize) -> Option<String> {
    let b = text.as_bytes();
    debug_assert_eq!(b[open], b'(');
    let mut depth = 0usize;
    let mut i = open;
    while i < b.len() {
        match b[i] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(text[open + 1..i].to_string());
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Split a parameter list on TOP-LEVEL commas; generics, tuples and slices nest.
pub fn split_params(params: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();
    for ch in params.chars() {
        match ch {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                out.push(std::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        current.push(ch);
    }
    if !current.trim().is_empty() {
        out.push(current);
    }
    out
}

/// The body of `fn NAME<…>(…) [-> T] { … }`, braces excluded. Tolerates
/// generics, multi-line parameter lists and return types.
pub fn find_fn_body(text: &str, fn_name: &str) -> Option<String> {
    let re = Regex::new(&format!(r"\bfn\s+{}\b", regex::escape(fn_name))).ok()?;
    let m = re.find(text)?;
    let b = text.as_bytes();
    let mut i = m.end();

    i = skip_ws(b, i);
    if i < b.len() && b[i] == b'<' {
        let mut depth = 0usize;
        while i < b.len() {
            match b[i] {
                b'<' => depth += 1,
                b'>' => {
                    depth -= 1;
                    if depth == 0 {
                        i += 1;
                        break;
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }
    i = skip_ws(b, i);
    if i < b.len() && b[i] == b'(' {
        let params = param_list(text, i)?;
        i += params.len() + 2;
    }
    brace_body(text, i)
}

/// The body of `impl TRAIT for TYPE { … }`, braces excluded.
pub fn find_impl_body(text: &str, trait_name: &str, type_name: &str) -> Option<String> {
    let re = Regex::new(&format!(
        r"\bimpl\s+{}\s+for\s+{}\b",
        regex::escape(trait_name),
        regex::escape(type_name)
    ))
    .ok()?;
    let m = re.find(text)?;
    brace_body(text, m.end())
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && b[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

// ---------------------------------------------------------------------------
// type aliases
// ---------------------------------------------------------------------------

/// The file-scoped type aliases in one source file: both `type A = B;` and
/// `use path::B as A;`.
///
/// A pattern that matches a type by name sees nothing when the type is spelled
/// through an alias, so every audit expands aliases before matching.
pub struct TypeAliases {
    map: BTreeMap<String, String>,
    any: Option<Regex>,
}

impl TypeAliases {
    /// Scan one file's (comment-blanked) source for alias declarations.
    pub fn from_source(text: &str) -> Self {
        // Both patterns lead with their keyword rather than with a `^\s*`
        // anchor: a leading literal is what lets the regex engine skip through
        // a file instead of trying every position in it, and these run over
        // every source file in the tree.
        let type_alias = pattern!(TYPE_ALIAS, r"\btype\s+(\w+)\s*(?:<[^>]*>)?\s*=\s*([^;]+);");
        // `as` renames are read only inside `use` statements: elsewhere `as` is
        // a cast, and `x as usize` is not a type alias.
        let use_stmt = pattern!(USE_STMT, r"\buse\s+[^;]+;");
        let renamed = pattern!(USE_RENAME, r"(\w+)\s+as\s+(\w+)");

        let mut map = BTreeMap::new();
        for cap in type_alias.captures_iter(text) {
            let name = cap[1].to_string();
            let target = cap[2].split_whitespace().collect::<Vec<_>>().join(" ");
            if target.contains(&name) {
                continue; // self-referential; expanding it would not terminate
            }
            map.insert(name, target);
        }
        for stmt in use_stmt.find_iter(text) {
            for cap in renamed.captures_iter(stmt.as_str()) {
                let (target, alias) = (cap[1].to_string(), cap[2].to_string());
                if alias == "_" || alias == target {
                    continue;
                }
                map.insert(alias, target);
            }
        }
        map.remove("_");

        let any = if map.is_empty() {
            None
        } else {
            let alts = map
                .keys()
                .map(|k| regex::escape(k))
                .collect::<Vec<_>>()
                .join("|");
            Some(Regex::new(&format!(r"\b(?:{alts})\b")).expect("alias alternation must compile"))
        };
        TypeAliases { map, any }
    }

    /// An empty alias table, for callers with no file in hand.
    pub fn none() -> Self {
        TypeAliases {
            map: BTreeMap::new(),
            any: None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn target(&self, alias: &str) -> Option<&str> {
        self.map.get(alias).map(|s| s.as_str())
    }

    /// Rewrite every alias in `text` to what it stands for, chasing chains of
    /// aliases up to a small fixed depth (beyond which a cycle is the likelier
    /// explanation than real code).
    pub fn expand(&self, text: &str) -> String {
        let Some(any) = &self.any else {
            return text.to_string();
        };
        if !any.is_match(text) {
            return text.to_string();
        }
        let mut current = text.to_string();
        for _ in 0..4 {
            let next = any.replace_all(&current, |caps: &Captures| {
                self.map
                    .get(&caps[0])
                    .cloned()
                    .unwrap_or_else(|| caps[0].to_string())
            });
            if next == current {
                break;
            }
            current = next.into_owned();
        }
        current
    }
}

// ---------------------------------------------------------------------------
// SystemParam bundles
// ---------------------------------------------------------------------------

/// Every `#[derive(… SystemParam …)] struct` in the tree, with the types of its
/// fields.
///
/// A bundle is how a system stays under Bevy's 16-parameter limit, and a system
/// that takes one reads or writes everything the bundle holds — but the `Res<'w,
/// X>` is in a `struct` in another file, so a scan of the system's own signature
/// sees a single opaque type name. Expanding the bundle is what keeps such a
/// resource from disappearing from an audit.
pub struct SystemParamBundles {
    fields: BTreeMap<String, Vec<String>>,
}

impl SystemParamBundles {
    /// Scan comment-blanked sources for bundle definitions.
    pub fn scan(files: &[SourceFile]) -> Self {
        let decl = pattern!(
            DERIVED_STRUCT,
            r"(?s)#\[\s*derive\s*\(([^)]*)\)\s*\]\s*(?:pub(?:\s*\([^)]*\))?\s+)?struct\s+(\w+)\s*(?:<[^>]*>)?\s*\{"
        );
        let is_system_param = pattern!(SYSTEM_PARAM_DERIVE, r"\bSystemParam\b");
        let mut fields: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for file in files {
            // A file that never says `SystemParam` declares no bundle, and the
            // structure pattern below is far too costly to run over every file.
            if !is_system_param.is_match(&file.code) {
                continue;
            }
            for cap in decl.captures_iter(&file.code) {
                if !is_system_param.is_match(&cap[1]) {
                    continue;
                }
                let open = cap.get(0).unwrap().end() - 1;
                let Some(body) = brace_body(&file.code, open) else {
                    continue;
                };
                let types = split_params(&body)
                    .iter()
                    .filter_map(|field| field.split_once(':').map(|(_, ty)| ty.trim().to_string()))
                    .filter(|ty| !ty.is_empty())
                    .collect::<Vec<_>>();
                fields.insert(cap[2].to_string(), types);
            }
        }
        SystemParamBundles { fields }
    }

    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    pub fn names(&self) -> impl Iterator<Item = &str> + '_ {
        self.fields.keys().map(|k| k.as_str())
    }

    pub fn field_types(&self, bundle: &str) -> Option<&[String]> {
        self.fields.get(bundle).map(|v| v.as_slice())
    }

    /// Every field type reachable from `param`, following bundle types
    /// transitively. Empty when `param` names no bundle.
    pub fn expand(&self, param: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut queue: Vec<String> = vec![param.to_string()];
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let ident = pattern!(TYPE_IDENT, r"\b([A-Z]\w*)\b");
        while let Some(next) = queue.pop() {
            for cap in ident.captures_iter(&next) {
                let name = cap[1].to_string();
                if !self.fields.contains_key(&name) || !seen.insert(name.clone()) {
                    continue;
                }
                for ty in &self.fields[&name] {
                    out.push(ty.clone());
                    queue.push(ty.clone());
                }
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// signatures
// ---------------------------------------------------------------------------

/// A `pub fn` item found by the signature scan.
pub struct FnSignature {
    pub name: String,
    /// The text between the parens of the parameter list.
    pub params: String,
    /// 1-based line of the `pub fn`, valid against the file on disk.
    pub line: usize,
}

/// Every top-level-ish `pub fn` in `text`, with its parameter list.
///
/// Whitespace-tolerant: `pub  fn   name <T> (` reads the same as the rustfmt
/// spelling.
pub fn pub_fn_signatures(text: &str) -> Vec<FnSignature> {
    // Leads with the `pub` literal rather than a `^[ \t]*` anchor so the engine
    // can skip through the file; the line-start requirement is then checked
    // directly, which is both faster and exactly as strict.
    let re = pattern!(PUB_FN, r"\bpub\s+fn\s+(\w+)\s*(?:<[^>]*>)?\s*\(");
    let mut out = Vec::new();
    for cap in re.captures_iter(text) {
        let whole = cap.get(0).unwrap();
        if !only_indentation_before(text, whole.start()) {
            continue;
        }
        let Some(params) = param_list(text, whole.end() - 1) else {
            continue;
        };
        out.push(FnSignature {
            name: cap[1].to_string(),
            params,
            line: text[..whole.start()].matches('\n').count() + 1,
        });
    }
    out
}

/// Is everything between the start of the line and `idx` indentation?
fn only_indentation_before(text: &str, idx: usize) -> bool {
    text[..idx]
        .bytes()
        .rev()
        .take_while(|b| *b != b'\n')
        .all(|b| b == b' ' || b == b'\t')
}

/// The resources a signature reads and writes.
pub struct ResourceUses {
    /// Taken as `Res<R>` / `Option<Res<R>>` — the system reads R.
    pub reads: BTreeSet<String>,
    /// Taken as `ResMut<R>` — the system fills R.
    pub writes: BTreeSet<String>,
}

/// `Res<…>` with the lifetime a `SystemParam` bundle field carries
/// (`Res<'w, X>`), any module path, and any amount of whitespace.
fn res_pattern() -> &'static Regex {
    pattern!(
        RES,
        r"\bRes\s*<\s*(?:'\w+\s*,\s*)?(?:\w+\s*::\s*)*(\w+)\s*>"
    )
}

/// `ResMut<…>`, read the same way.
fn res_mut_pattern() -> &'static Regex {
    pattern!(
        RES_MUT,
        r"\bResMut\s*<\s*(?:'\w+\s*,\s*)?(?:\w+\s*::\s*)*(\w+)\s*>"
    )
}

/// Read the resources a parameter list uses, seeing through type aliases and
/// `SystemParam` bundles.
///
/// A parameter whose type is a reference is a HELPER argument, not a Bevy
/// system parameter — its caller is the system — so it is skipped. The test is
/// the TOP LEVEL of the type only: `&mut Commands` is a helper argument,
/// `Query<&mut Transform>` is a system parameter that happens to borrow inside
/// its generics.
pub fn resource_uses(
    params: &str,
    aliases: &TypeAliases,
    bundles: &SystemParamBundles,
) -> ResourceUses {
    let res_re = res_pattern();
    let res_mut_re = res_mut_pattern();

    let mut reads = BTreeSet::new();
    let mut writes = BTreeSet::new();
    for param in expanded_param_types(params, aliases, bundles) {
        for cap in res_mut_re.captures_iter(&param) {
            writes.insert(cap[1].to_string());
        }
        for cap in res_re.captures_iter(&param) {
            // `ResMut<…>` contains `Res` but not as a word-boundary match of
            // `Res\s*<`, so the two patterns are disjoint.
            reads.insert(cap[1].to_string());
        }
    }
    ResourceUses { reads, writes }
}

/// Every parameter type a signature effectively takes: aliases expanded,
/// `SystemParam` bundle fields appended, helper (`&`) arguments dropped.
pub fn expanded_param_types(
    params: &str,
    aliases: &TypeAliases,
    bundles: &SystemParamBundles,
) -> Vec<String> {
    let mut out = Vec::new();
    for param in split_params(params) {
        if is_reference_param(&param) {
            continue;
        }
        let expanded = aliases.expand(&param);
        for field in bundles.expand(&expanded) {
            out.push(aliases.expand(&field));
        }
        out.push(expanded);
    }
    out
}

/// The type half of `name: Type` (or the whole thing, for an unnamed param).
pub fn param_type(param: &str) -> &str {
    let bytes = param.as_bytes();
    let mut depth = 0i32;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'<' | b'(' | b'[' => depth += 1,
            b'>' | b')' | b']' => depth -= 1,
            b':' if depth == 0 => {
                // `::` is a path separator, not the name/type split.
                if bytes.get(i + 1) == Some(&b':') {
                    i += 2;
                    continue;
                }
                return param[i + 1..].trim();
            }
            _ => {}
        }
        i += 1;
    }
    param.trim()
}

/// Is this parameter taken by reference at the top level of its type?
pub fn is_reference_param(param: &str) -> bool {
    param_type(param).starts_with('&')
}

// ---------------------------------------------------------------------------
// StatesPlugin::build — the .add_systems map
// ---------------------------------------------------------------------------

/// One `.add_systems(…)` call: the text inside its parens, and the game
/// state(s) the call gates its systems on.
pub struct AddSystemsBlock {
    pub text: String,
    pub states: BTreeSet<String>,
}

/// The body of `impl Plugin for StatesPlugin { fn build(…) { … } }`.
pub fn states_plugin_build(text: &str) -> Option<String> {
    let impl_body = find_impl_body(text, "Plugin", "StatesPlugin")?;
    find_fn_body(&impl_body, "build")
}

/// Every `.add_systems(…)` call inside `body`, paired with the state(s) it is
/// gated on.
///
/// Three gate shapes appear in `StatesPlugin::build`:
///
/// * `run_if(in_state(GameState::X))` -> `{X}`
/// * `OnEnter(GameState::X)` / `OnExit(GameState::X)` -> `{X}`
/// * `run_if(<fn>)` for a named `-> bool` state predicate (`in_combat_scene`)
///   -> the states named in that function's body, read out of `file` rather
///   than hardcoded here.
///
/// A call with no recognised gate yields an empty state set — its systems are
/// registered, just not under any one state.
pub fn add_systems_blocks(body: &str, file: &str) -> Vec<AddSystemsBlock> {
    let add_systems_re = pattern!(ADD_SYSTEMS, r"\.\s*add_systems\s*\(");
    let in_state_re = pattern!(IN_STATE, r"in_state\s*\(\s*GameState\s*::\s*(\w+)\s*\)");
    let on_enter_exit_re = pattern!(
        ON_ENTER_EXIT,
        r"On(?:Enter|Exit)\s*\(\s*GameState\s*::\s*(\w+)\s*\)"
    );
    let run_if_fn_re = pattern!(RUN_IF_FN, r"run_if\s*\(\s*([a-z_][a-z0-9_]*)\s*\)");
    let game_state_re = pattern!(GAME_STATE, r"GameState\s*::\s*(\w+)");

    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(m) = add_systems_re.find_at(body, from) {
        let open = m.end() - 1;
        let Some(text) = param_list(body, open) else {
            break;
        };
        let end = open + text.len() + 2;

        let mut states: BTreeSet<String> = BTreeSet::new();
        for cap in in_state_re.captures_iter(&text) {
            states.insert(cap[1].to_string());
        }
        for cap in on_enter_exit_re.captures_iter(&text) {
            states.insert(cap[1].to_string());
        }
        for cap in run_if_fn_re.captures_iter(&text) {
            if let Some(predicate) = find_fn_body(file, &cap[1]) {
                for c in game_state_re.captures_iter(&predicate) {
                    states.insert(c[1].to_string());
                }
            }
        }

        out.push(AddSystemsBlock { text, states });
        from = end.min(body.len());
    }
    out
}
