//! Law 10, the cut list: a word the project cut stays cut. It is the backticked word on the line of
//! `docs/dev/VISION.md` that starts with `Cut`, read at run time so this file never holds it. In any case and inside any
//! longer word it appears in no file's text and no file or directory name of the repository, except in that line.

use crate::support::{assert_clean, has_private_record, read, rel, root, walk};
use std::fs;
use std::path::Path;

/// The only file that may hold the cut word: the line that names it.
const ALLOWED: [&str; 1] = ["docs/dev/VISION.md"];

/// The word between backticks on the one line of `vision` that starts with `Cut`. A missing or repeated line, or a
/// line that does not name exactly one word, is an error: the law may not go quiet because its word is gone.
fn cut_word(vision: &str) -> Result<String, String> {
    let lines: Vec<&str> = vision
        .lines()
        .filter(|line| line.strip_prefix("Cut").is_some_and(|rest| rest.starts_with(char::is_whitespace)))
        .collect();
    let [line] = lines.as_slice() else {
        return Err(format!("{} lines start with `Cut`; exactly one names the cut word", lines.len()));
    };
    let spans: Vec<&str> = line.split('`').collect();
    let [_, word, _] = spans[..] else {
        return Err(format!(
            "the `Cut` line holds {} backticks; the cut word is its one backticked span",
            spans.len() - 1
        ));
    };
    if word.is_empty() || word.contains(char::is_whitespace) {
        return Err("the backticked span on the `Cut` line is not one word".to_owned());
    }
    Ok(word.to_owned())
}

/// The start of `path` through the first component whose name holds `needle` (lower case).
fn named_after(needle: &str, path: &str) -> Option<String> {
    let mut end = 0;
    for part in path.split('/') {
        end += part.len();
        if part.to_lowercase().contains(needle) {
            return Some(path[..end].to_owned());
        }
        end += 1;
    }
    None
}

/// Where one file mentions `word`: `path` is relative to the repository root, `text` is the content (decoded lossily
/// when the file is not UTF-8, so a torn character hides nothing). A path in `allowed` is exempt, exactly.
fn violations(word: &str, allowed: &[&str], path: &str, text: &str) -> Vec<String> {
    if allowed.contains(&path) {
        return Vec::new();
    }
    let needle = word.to_lowercase();
    let mut found: Vec<String> =
        named_after(&needle, path).map(|named| format!("{named}: named after the cut word")).into_iter().collect();
    for (n, line) in text.lines().enumerate() {
        if line.to_lowercase().contains(&needle) {
            found.push(format!("{path}:{}: the cut word", n + 1));
        }
    }
    found
}

fn word() -> String {
    cut_word(&read(&root().join("docs/dev/VISION.md"))).unwrap_or_else(|e| panic!("docs/dev/VISION.md: {e}"))
}

/// `path` relative to `base`, with `/` separators.
fn relative(base: &Path, path: &Path) -> String {
    path.strip_prefix(base).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

/// Where the tree at `base` mentions `word` outside `allowed`, each violation once. Files are named relative to `base`.
fn scan(base: &Path, word: &str, allowed: &[&str]) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for path in walk(base) {
        let file = relative(base, &path);
        let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {file}: {e}"));
        for violation in violations(word, allowed, &file, &String::from_utf8_lossy(&bytes)) {
            if !found.contains(&violation) {
                found.push(violation);
            }
        }
    }
    found
}

/// `scan` over this repository for the cut word.
fn scan_repository(allowed: &[&str]) -> Vec<String> {
    scan(&root(), &word(), allowed)
}

#[test]
fn the_cut_word_is_found_only_in_the_allowed_files() {
    if !has_private_record() {
        return; // the public tree: the word is not there to look for; the export's leak scan checked it
    }
    let found = scan_repository(&ALLOWED);
    assert_clean(&format!("law 10 (the cut word lives only in {})", ALLOWED.join(" and ")), &found);
}

#[test]
fn every_allowed_file_exists() {
    if !has_private_record() {
        return;
    }
    let missing: Vec<String> = ALLOWED
        .iter()
        .filter(|path| !root().join(path).is_file())
        .map(|path| format!("{path}: allowed to hold the cut word, but there is no such file"))
        .collect();
    assert_clean("law 10 (the cut word lives only in the allowed files)", &missing);
}

#[test]
fn the_scan_reaches_root_files_other_directories_and_this_law() {
    let seen: Vec<String> = walk(&root()).iter().map(|path| rel(path)).collect();
    for expected in ["Cargo.toml", "scripts/b", "crates/j4n/tests/laws/cut_word.rs"] {
        assert!(seen.iter().any(|path| path == expected), "the scan does not reach {expected}");
    }
}

#[test]
fn the_word_is_the_backticked_one_on_the_cut_line() {
    let vision = "# Vision\nCutting `wrong` is not it.\nCut `zebra` and what hangs on it.\nMore text with `code`.\n";
    assert_eq!(cut_word(vision), Ok("zebra".to_owned()));
}

#[test]
fn a_missing_cut_line_is_an_error() {
    assert!(cut_word("# Vision\nCutting `zebra` is not the line.\n  Cut `zebra` is indented.\n").is_err());
    assert!(cut_word("").is_err());
}

#[test]
fn a_repeated_cut_line_is_an_error() {
    assert!(cut_word("Cut `zebra` here.\nCut `zebra` again.\n").is_err());
}

#[test]
fn a_cut_line_without_exactly_one_word_between_backticks_is_an_error() {
    for line in
        ["Cut zebra here.", "Cut `zebra here.", "Cut `zebra` and `llama`.", "Cut `` here.", "Cut `two words` here."]
    {
        assert!(cut_word(line).is_err(), "accepted: {line}");
    }
}

#[test]
fn a_mention_inside_a_longer_word_is_caught() {
    let found = violations("zebra", &[], "a.md", "fine\nthe unzebrafied bit\nzebras\n");
    assert_eq!(found, ["a.md:2: the cut word", "a.md:3: the cut word"]);
}

#[test]
fn any_case_is_caught_whatever_the_case_of_the_word() {
    let text = "ZEBRA\nZebra\nzEbRa\n";
    assert_eq!(violations("zebra", &[], "a.md", text).len(), 3);
    assert_eq!(violations("ZEBRA", &[], "a.md", "a zebra here"), ["a.md:1: the cut word"]);
}

#[test]
fn near_misses_are_not_caught() {
    assert!(violations("zebra", &[], "a.md", "zebr a\nzeb ra\nz e b r a\nzebr\n").is_empty());
    assert!(violations("zebra", &[], "docs/zebr.md", "").is_empty());
}

#[test]
fn a_file_name_is_caught() {
    assert_eq!(violations("zebra", &[], "docs/Zebra.md", "clean"), ["docs/Zebra.md: named after the cut word"]);
    assert_eq!(
        violations("zebra", &[], "src/my_zebra_tools.rs", ""),
        ["src/my_zebra_tools.rs: named after the cut word"]
    );
}

#[test]
fn a_directory_name_is_caught_and_named_as_the_directory() {
    assert_eq!(
        violations("zebra", &[], "packs/zebra-kit/pack.toml", ""),
        ["packs/zebra-kit: named after the cut word"]
    );
}

#[test]
fn a_file_that_is_not_utf8_is_read_by_name_and_by_the_text_around_its_torn_bytes() {
    let torn = String::from_utf8_lossy(b"\x89PNG\xff\nthe zebra\xfe\n");
    assert_eq!(violations("zebra", &[], "assets/stripes.png", &torn), ["assets/stripes.png:2: the cut word"]);
    let binary = String::from_utf8_lossy(b"\x89PNG\xff\xfe");
    assert_eq!(violations("zebra", &[], "assets/zebra.png", &binary), ["assets/zebra.png: named after the cut word"]);
    assert!(violations("zebra", &[], "assets/stripes.png", &binary).is_empty());
}

#[test]
fn a_name_and_a_line_in_one_file_are_both_reported() {
    let found = violations("zebra", &[], "zebra.txt", "a\nzebra\n");
    assert_eq!(found, ["zebra.txt: named after the cut word", "zebra.txt:2: the cut word"]);
}

#[test]
fn an_allowed_path_is_exempt_and_only_that_exact_path() {
    let allowed = ["docs/a.md"];
    assert!(violations("zebra", &allowed, "docs/a.md", "zebra\n").is_empty());
    for other in ["docs/a.md.bak", "old/docs/a.md", "docs/b.md", "DOCS/a.md"] {
        assert_eq!(violations("zebra", &allowed, other, "zebra\n").len(), 1, "exempt: {other}");
    }
}

/// Build `tree` (file name relative to a fresh directory, bytes) in the temp directory, `scan` it for `zebra` with the
/// real allowed list, remove it again.
fn scanned(name: &str, tree: &[(&str, &[u8])]) -> Vec<String> {
    let base = std::env::temp_dir().join(format!("j4n-cut-word-{}-{name}", std::process::id()));
    for (file, bytes) in tree {
        let path = base.join(file);
        fs::create_dir_all(path.parent().expect("file below the tree")).expect("temp tree");
        fs::write(&path, bytes).expect("temp file");
    }
    let found = scan(&base, "zebra", &ALLOWED);
    fs::remove_dir_all(&base).expect("remove temp tree");
    found
}

#[test]
fn the_scan_reports_a_mention_outside_the_allowed_files_by_its_path_below_the_tree() {
    let found = scanned(
        "tree",
        &[
            ("docs/dev/VISION.md", b"Cut `zebra` and what hangs on it.\n"),
            ("docs/dev/VISION.md.bak", b"ok\nthe zebra\n"),
            ("crates/a/src/lib.rs", b"// a Zebra\nfn a() {}\n"),
            ("zebra-kit/pack.toml", b"api = 1\n"),
            ("zebra-kit/more.toml", b"api = 1\n"),
            ("clean/notes.md", b"nothing here\n"),
        ],
    );
    assert_eq!(
        found,
        [
            "crates/a/src/lib.rs:1: the cut word",
            "docs/dev/VISION.md.bak:2: the cut word",
            "zebra-kit: named after the cut word",
        ]
    );
}

#[test]
fn a_mention_in_a_file_that_is_not_utf8_is_caught() {
    let found = scanned("bytes", &[("fixtures/f.txt", b"Cut the zebra\xff\n"), ("fixtures/g.bin", b"\xff\xfe")]);
    assert_eq!(found, ["fixtures/f.txt:1: the cut word"]);
}

#[test]
fn the_real_scan_finds_the_word_in_the_cut_line_when_nothing_is_allowed() {
    if !has_private_record() {
        return;
    }
    let found = scan_repository(&[]);
    assert!(found.iter().any(|violation| violation.starts_with("docs/dev/VISION.md:")), "{found:?}");
}
