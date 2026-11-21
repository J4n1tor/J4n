//! The fingerprint of a finding: 12 hex that are the same for the same bug in any version, on any machine, in
//! any folder, and different for a different bug. What goes in is the kind of source, the id of the failure or event, the row it came from, and
//! the message with everything that belongs to one run stripped: quoted text, paths, numbers, hashes and ids. The version is not in it.
//!
//! This is a public surface: once a report carries `j4n-fp: <hex>`, changing how a message is stripped splits every issue in two. A change
//! here needs the cases that pin it.

use j4n_core::change::digest;

/// The longest skeleton that is hashed.
const MOST: usize = 200;

/// The fingerprint: `source`, `id` and `row` as given, and the message as its [`skeleton`].
pub fn fingerprint(source: &str, id: &str, row: &str, message: &str) -> String {
    digest(&format!("{source}\n{id}\n{row}\n{}", skeleton(message))).chars().take(12).collect()
}

/// A message with what belongs to one run taken out: each quoted span (`'…'`, `"…"` or `` `…` ``, where a quote opens after a non-letter and closes
/// before one, so an apostrophe in a word is a letter) becomes `_`; a word that is a path, a hash (seven or more hex digits), a UUID, or a long id
/// (eight or more characters mixing letters and digits) becomes `_`; every other digit run becomes `#`; the rest is lower case with single spaces.
pub fn skeleton(message: &str) -> String {
    let unquoted = unquote(message);
    let words: Vec<String> = unquoted.split_whitespace().map(word).collect();
    let mut out = words.join(" ");
    while out.ends_with(['.', ',', ';', ':', '!', '?']) {
        out.pop();
    }
    out.chars().take(MOST).collect()
}

fn unquote(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let opens = matches!(c, '\'' | '"' | '`') && (i == 0 || !chars[i - 1].is_alphanumeric());
        if opens && let Some(end) = (i + 1..chars.len()).find(|&j| chars[j] == c && chars.get(j + 1).is_none_or(|n| !n.is_alphanumeric())) {
            out.push('_');
            i = end + 1;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

fn word(w: &str) -> String {
    let core = w.trim_matches(|c: char| !c.is_alphanumeric() && c != '/' && c != '~' && c != '\\' && c != '.');
    let hex = core.len() >= 7 && core.chars().all(|c| c.is_ascii_hexdigit()) && core.chars().any(|c| c.is_ascii_digit()) && core.chars().any(|c| c.is_ascii_alphabetic());
    let uuid = core.len() == 36 && core.chars().filter(|c| *c == '-').count() == 4 && core.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
    let path = core.contains('/') || core.contains('\\') || core.starts_with('~') || core.starts_with("./");
    let id = core.len() >= 8 && core.chars().any(|c| c.is_ascii_digit()) && core.chars().any(|c| c.is_ascii_alphabetic()) && core.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if hex || uuid || path || id {
        return "_".into();
    }
    let mut out = String::new();
    let mut digits = false;
    for c in w.to_lowercase().chars() {
        if c.is_ascii_digit() {
            if !digits {
                out.push('#');
            }
            digits = true;
        } else {
            digits = false;
            out.push(c);
        }
    }
    out
}
