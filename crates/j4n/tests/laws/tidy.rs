//! The comment rule: code and packs do not name the development record. A `//`, `///` or `//!` line of
//! `crates/*/src` and `crates/*/tests`, and the `doc`, `summary`, `about` and `use_when` fields of `packs/`, hold no
//! decision id (`D-` and two to four digits), no bug id (`J` and two or three digits) and no path to a Markdown document
//! that is not in the tree (a development document that was removed). The ids live on in the maintainer's own record
//! (`docs/dev/`, not published); a comment says the reason itself.
//!
//! A hit names the file, the line and the word. The few lines that are about ids on purpose are listed in `ALLOWED`,
//! each with its reason, as the exact text the line must hold; an entry that matches no line fails, so the list cannot
//! outlive its lines.

use crate::support::{assert_clean, read, rel, root, walk, walk_ext};
use std::collections::BTreeSet;

/// `(file, text the line holds, why it may name an id)`. Nothing here is a broad allowance: a line is named by what it says.
/// Empty: the glossary's examples (`ADR-12`, `BUG-7`, `UX3`) are not of the shapes the rule looks for.
const ALLOWED: [(&str, &str, &str); 0] = [];

/// Markdown names that belong to other tools, not to this repository: they are never a development document.
const OTHER_TOOLS: [&str; 4] = ["SKILL.md", "MEMORY.md", "CLAUDE.md", "AGENTS.md"];

/// The pack fields an operator or a doer reads as prose.
const PACK_FIELDS: [&str; 4] = ["doc", "summary", "about", "use_when"];

fn is_word_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// The digits that start at `at`, and where they end.
fn digits(bytes: &[u8], at: usize) -> usize {
    bytes[at..].iter().take_while(|b| b.is_ascii_digit()).count()
}

/// Ids in `text`: `D-` with two to four digits, `J` with two or three, each a whole word.
fn ids(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    for at in 0..bytes.len() {
        if at > 0 && is_word_char(bytes[at - 1]) {
            continue;
        }
        let (start, wanted) = match bytes[at] {
            b'D' if bytes.get(at + 1) == Some(&b'-') => (at + 2, 2..=4),
            b'J' => (at + 1, 2..=3),
            _ => continue,
        };
        let n = digits(bytes, start);
        let end = start + n;
        if wanted.contains(&n) && !bytes.get(end).is_some_and(|b| is_word_char(*b)) {
            found.push(text[at..end].to_owned());
        }
    }
    found
}

/// What a Markdown path or name in `text` points at that is not in the tree. `known` holds the repository paths of the
/// Markdown files that exist and `names` their file names.
fn gone_documents(text: &str, known: &BTreeSet<String>, names: &BTreeSet<String>) -> Vec<String> {
    let token = |c: char| c.is_ascii_alphanumeric() || "_./-".contains(c);
    text.split(|c: char| !token(c))
        .map(|t| t.trim_matches(|c| c == '.' || c == '/'))
        .filter(|t| t.ends_with(".md") && t.len() > 3)
        .filter(|t| match t.rsplit_once('/') {
            Some(_) => !known.contains(*t),
            None => !names.contains(*t) && !OTHER_TOOLS.contains(t),
        })
        .map(str::to_owned)
        .collect()
}

/// Every word in `text` the rule forbids.
fn words(text: &str, known: &BTreeSet<String>, names: &BTreeSet<String>) -> Vec<String> {
    let mut found = ids(text);
    found.extend(gone_documents(text, known, names));
    found
}

/// The comment of a Rust line: from the first `//` that is not inside a string or a character literal, and not the
/// `//` of a URL.
fn comment(line: &str) -> Option<&str> {
    let bytes = line.as_bytes();
    let (mut at, mut quoted) = (0, false);
    while at + 1 < bytes.len() {
        match bytes[at] {
            b'\\' if quoted => at += 1,
            b'"' => quoted = !quoted,
            b'\'' if !quoted && bytes.get(at + 2) == Some(&b'\'') => at += 2,
            b'/' if !quoted && bytes[at + 1] == b'/' && (at == 0 || bytes[at - 1] != b':') => return Some(&line[at..]),
            _ => {}
        }
        at += 1;
    }
    None
}

/// Where a Rust file's comments hold a forbidden word: `file:line: word`.
fn rust_hits(file: &str, text: &str, known: &BTreeSet<String>, names: &BTreeSet<String>) -> Vec<String> {
    let mut found = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if let Some(note) = comment(line) {
            for word in words(note, known, names) {
                found.push(format!("{file}:{}: `{word}` in a comment", n + 1));
            }
        }
    }
    found
}

/// Where a pack file's prose fields hold a forbidden word. A field is `doc = …`, `summary = …`, `about = …` or
/// `use_when = …`; its value runs to the end of the line, or to the closing quotes of a multi-line string.
fn pack_hits(file: &str, text: &str, known: &BTreeSet<String>, names: &BTreeSet<String>) -> Vec<String> {
    let mut found = Vec::new();
    let mut open: Option<&str> = None;
    for (n, line) in text.lines().enumerate() {
        let value = match open {
            Some(quotes) => {
                if line.contains(quotes) {
                    open = None;
                }
                line
            }
            None => {
                let Some((key, value)) = line.split_once('=') else { continue };
                if !PACK_FIELDS.contains(&key.trim()) {
                    continue;
                }
                for quotes in ["\"\"\"", "'''"] {
                    if value.trim_start().starts_with(quotes) && value.trim_start()[3..].matches(quotes).count() == 0 {
                        open = Some(quotes);
                    }
                }
                value
            }
        };
        for word in words(value, known, names) {
            found.push(format!("{file}:{}: `{word}` in a pack field", n + 1));
        }
    }
    found
}

/// The Markdown files of the tree, as repository paths and as file names.
fn documents() -> (BTreeSet<String>, BTreeSet<String>) {
    let known: BTreeSet<String> = walk_ext(&root(), "md").iter().map(|p| rel(p)).collect();
    let names = known.iter().map(|p| p.rsplit('/').next().unwrap_or(p).to_owned()).collect();
    (known, names)
}

/// The files the law reads: the Rust files under `src` and `tests` of every crate.
fn rust_files() -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    for dir in crate::support::crate_dirs() {
        for part in ["src", "tests"] {
            files.extend(walk_ext(&dir.join(part), "rs"));
        }
    }
    files
}

fn scan() -> Vec<String> {
    let (known, names) = documents();
    let mut found = Vec::new();
    for path in rust_files() {
        found.extend(rust_hits(&rel(&path), &read(&path), &known, &names));
    }
    for path in walk_ext(&root().join("packs"), "toml") {
        found.extend(pack_hits(&rel(&path), &read(&path), &known, &names));
    }
    found
}

/// A hit is allowed when an entry names its file and the line's text holds the entry's text. `lines` gives the text of
/// a hit's line.
fn allowed(hit: &str, entry: &(&str, &str, &str), lines: &dyn Fn(&str, usize) -> String) -> bool {
    let mut parts = hit.splitn(3, ':');
    let (file, n) = (parts.next().unwrap_or_default(), parts.next().and_then(|n| n.parse().ok()).unwrap_or(0));
    file == entry.0 && lines(file, n).contains(entry.1)
}

fn line_of(file: &str, n: usize) -> String {
    read(&root().join(file)).lines().nth(n.saturating_sub(1)).unwrap_or_default().to_owned()
}

#[test]
fn the_code_and_the_packs_name_no_id_and_no_removed_document() {
    // the public tree has no private record (`docs/dev/`), so a path into it cannot be checked there; here it is
    let public = !crate::support::has_private_record();
    let found: Vec<String> = scan()
        .into_iter()
        .filter(|hit| !ALLOWED.iter().any(|e| allowed(hit, e, &line_of)))
        .filter(|hit| !(public && hit.contains("`docs/dev/")))
        .collect();
    assert_clean("the comment rule (a comment says the reason, not the ticket)", &found);
}

#[test]
fn every_allowed_line_is_still_there_and_still_names_an_id() {
    let all = scan();
    let stale: Vec<String> = ALLOWED
        .iter()
        .filter(|e| !all.iter().any(|hit| allowed(hit, e, &line_of)))
        .map(|e| format!("{}: allowed to hold `{}` ({}), but no such line names an id", e.0, e.1, e.2))
        .collect();
    assert_clean("the comment rule (the allowed lines)", &stale);
}

#[test]
fn the_scan_reaches_the_sources_the_tests_and_the_packs() {
    let seen: Vec<String> = rust_files().iter().map(|p| rel(p)).chain(walk(&root().join("packs")).iter().map(|p| rel(p))).collect();
    for expected in ["crates/j4n-core/src/lib.rs", "crates/j4n/tests/laws/tidy.rs", "packs/base/pack.toml"] {
        assert!(seen.iter().any(|p| p == expected), "the scan does not reach {expected}");
    }
}

fn tree(paths: &[&str]) -> (BTreeSet<String>, BTreeSet<String>) {
    let known: BTreeSet<String> = paths.iter().map(|p| (*p).to_owned()).collect();
    let names = known.iter().map(|p| p.rsplit('/').next().unwrap_or(p).to_owned()).collect();
    (known, names)
}

#[test]
fn an_id_is_a_decision_or_a_bug_number_standing_alone() {
    let d = |n: &str| format!("D-{n}");
    let j = |n: &str| format!("J{n}");
    assert_eq!(ids(&format!("see {} and {}.", d("407"), j("55"))), [d("407"), j("55")]);
    assert_eq!(ids(&format!("({}–{})", d("1000"), d("1009"))), [d("1000"), d("1009")]);
    for quiet in [d("1"), d("12345"), j("1"), j("1234"), format!("x{}", d("407")), format!("{}a", j("55")), "J4n runs".to_owned(), "D-day".to_owned()] {
        assert!(ids(&quiet).is_empty(), "named as an id: {quiet}");
    }
}

#[test]
fn a_path_to_a_document_that_is_not_in_the_tree_is_found() {
    let (known, names) = tree(&["README.md", "docs/GUIDE.md", "docs/dev/DECISIONS.md"]);
    let gone = |t: &str| gone_documents(t, &known, &names);
    assert_eq!(gone("see `docs/OLD-NOTES.md` §3 and OLD-NOTES.md."), ["docs/OLD-NOTES.md", "OLD-NOTES.md"]);
    assert_eq!(gone("in docs/dev/OLD.md."), ["docs/dev/OLD.md"]);
    for quiet in ["README.md", "`docs/GUIDE.md` §2", "docs/dev/DECISIONS.md.", "DECISIONS.md", "a SKILL.md with a name", "a .md file", "x.md.rs"] {
        assert!(gone(quiet).is_empty(), "found as gone: {quiet}");
    }
}

#[test]
fn only_the_comment_of_a_line_is_read() {
    let id = format!("D-{}", 407);
    assert_eq!(comment(&format!("let a = 1; // see {id}")), Some(&*format!("// see {id}")));
    assert_eq!(comment("/// the doc"), Some("/// the doc"));
    for quiet in [format!("let a = \"{id} // x\";"), "let url = \"https://x\";".to_owned(), "let c = '\"'; let b = 1;".to_owned(), "let a = 1;".to_owned()] {
        assert!(comment(&quiet).is_none(), "read as a comment: {quiet}");
    }
}

#[test]
fn a_rust_comment_and_a_pack_field_are_caught_and_code_and_other_fields_are_not() {
    let (known, names) = tree(&["README.md"]);
    let id = format!("J{}", 110);
    let rust = format!("fn a() {{}} // {id}: the check\nlet s = \"{id}\";\n/// fine\n");
    assert_eq!(rust_hits("a.rs", &rust, &known, &names), [format!("a.rs:1: `{id}` in a comment")]);
    let pack = format!("id = \"{id}\"\nsummary = \"as {id} said\"\ndoc = \"\"\"\nfirst\nsecond {id}\n\"\"\"\nafter = \"{id}\"\n");
    assert_eq!(pack_hits("p.toml", &pack, &known, &names), [format!("p.toml:2: `{id}` in a pack field"), format!("p.toml:5: `{id}` in a pack field")]);
}
