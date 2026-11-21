//! Shared by the law tests. A law reads the repository as text; nothing here runs the product.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long a test waits for something that is going to happen: a screen, a row of the Log, a process that ends. A wait returns the
/// moment its condition holds, so a long bound costs nothing on a quiet machine, and a short one fails on a machine that other builds
/// share. A bound is for a hang, never for how fast the code is.
pub const PATIENCE: Duration = Duration::from_secs(90);

/// Directories no law reads when it walks the whole repository: git's, cargo's and tool state. They are skipped only as
/// children of the repository root, so a module or a pack directory that happens to be named `target` is still read.
const SKIPPED_DIRS: [&str; 3] = [".git", "target", ".claude"];

/// The repository root: the directory of the workspace `Cargo.toml`.
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("repository root")
}

/// A key file as J4n takes one, readable by its owner alone: a connector's `file:` that others can read is refused.
pub fn key_file(path: &Path, text: &str) {
    use std::os::unix::fs::PermissionsExt;
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

/// Whether this checkout holds the development record (`docs/dev/`). The public repository is
/// exported without it (`release/public.paths`), so a law that reads that record has nothing to read there and passes.
pub fn has_private_record() -> bool {
    root().join("docs/dev").is_dir()
}

/// Every file under `dir`, sorted. Walking the repository root skips the names in `SKIPPED_DIRS`, directories or files
/// (in a git worktree `.git` is a file that points at the repository); walking anything else skips nothing.
pub fn walk(dir: &Path) -> Vec<PathBuf> {
    walk_skipping(dir, if dir == root() { &SKIPPED_DIRS } else { &[] })
}

/// [`walk`], skipping the entries of `dir` itself that are named in `skipped`.
fn walk_skipping(dir: &Path, skipped: &[&str]) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else { return found };
    for entry in entries.flatten() {
        let path = entry.path();
        if skipped.iter().any(|skip| entry.file_name() == *skip) {
            continue;
        }
        if path.is_dir() {
            found.extend(walk(&path));
        } else {
            found.push(path);
        }
    }
    found.sort();
    found
}

/// Every file under `dir` with the extension `ext` (no dot).
pub fn walk_ext(dir: &Path, ext: &str) -> Vec<PathBuf> {
    walk(dir).into_iter().filter(|path| path.extension().is_some_and(|e| e == ext)).collect()
}

/// The directories `crates/*`, sorted.
pub fn crate_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(root().join("crates"))
        .expect("crates/")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.join("Cargo.toml").is_file())
        .collect();
    dirs.sort();
    dirs
}

/// The path relative to the repository root, with `/` separators, for messages.
pub fn rel(path: &Path) -> String {
    path.strip_prefix(root()).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

/// The file as text, or `None` when it is not UTF-8 (a binary file no text law reads).
pub fn read_text(path: &Path) -> Option<String> {
    String::from_utf8(fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", rel(path)))).ok()
}

/// The file as text; a file a law names must exist and be text.
pub fn read(path: &Path) -> String {
    read_text(path).unwrap_or_else(|| panic!("{} is not UTF-8 text", rel(path)))
}

/// Fail with every violation listed, so one run shows all of them.
pub fn assert_clean(law: &str, violations: &[String]) {
    assert!(violations.is_empty(), "{law}: {} violation(s)\n  {}", violations.len(), violations.join("\n  "));
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Rust source with comments and the bodies of string and char literals blanked to spaces. Newlines and every
/// other character keep their position, so a line number in the result is the line number in the source.
pub fn code_only(src: &str) -> String {
    let b: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let blank = |c: char| if c == '\n' { '\n' } else { ' ' };
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        let at = |n: usize| b.get(i + n).copied();
        if c == '/' && at(1) == Some('/') {
            while i < b.len() && b[i] != '\n' {
                out.push(' ');
                i += 1;
            }
        } else if c == '/' && at(1) == Some('*') {
            let mut depth = 0;
            while i < b.len() {
                if b[i] == '/' && b.get(i + 1) == Some(&'*') {
                    depth += 1;
                    out.push_str("  ");
                    i += 2;
                } else if b[i] == '*' && b.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    out.push_str("  ");
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    out.push(blank(b[i]));
                    i += 1;
                }
            }
        } else if c == 'r' && raw_string_hashes(&b, i).is_some() && (i == 0 || !is_ident(b[i - 1]) || prefix_b(&b, i)) {
            let hashes = raw_string_hashes(&b, i).unwrap_or(0);
            out.push('r');
            out.extend(std::iter::repeat_n('#', hashes));
            out.push('"');
            i += hashes + 2;
            while i < b.len()
                && !(b[i] == '"' && b[i + 1..].iter().take(hashes).filter(|&&h| h == '#').count() == hashes)
            {
                out.push(blank(b[i]));
                i += 1;
            }
            if i < b.len() {
                out.push('"');
                out.extend(std::iter::repeat_n('#', hashes));
                i += hashes + 1;
            }
        } else if c == '"' {
            out.push('"');
            i += 1;
            while i < b.len() && b[i] != '"' {
                let skip = if b[i] == '\\' && i + 1 < b.len() { 2 } else { 1 };
                for &ch in &b[i..i + skip] {
                    out.push(blank(ch));
                }
                i += skip;
            }
            if i < b.len() {
                out.push('"');
                i += 1;
            }
        } else if c == '\'' && at(1) == Some('\\') {
            out.push('\'');
            i += 1;
            while i < b.len() && b[i] != '\'' {
                let skip = if b[i] == '\\' && i + 1 < b.len() { 2 } else { 1 };
                for &ch in &b[i..i + skip] {
                    out.push(blank(ch));
                }
                i += skip;
            }
            if i < b.len() {
                out.push('\'');
                i += 1;
            }
        } else if c == '\'' && at(2) == Some('\'') && at(1) != Some('\'') {
            out.push_str("' '");
            i += 3;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// `r` at `i` opens a raw string `r#…#"`: how many `#`, or `None`.
fn raw_string_hashes(b: &[char], i: usize) -> Option<usize> {
    let hashes = b[i + 1..].iter().take_while(|&&c| c == '#').count();
    (b.get(i + 1 + hashes) == Some(&'"')).then_some(hashes)
}

/// The `r` at `i` follows a `b` that itself starts a token: a `br"…"` byte string.
fn prefix_b(b: &[char], i: usize) -> bool {
    i >= 1 && b[i - 1] == 'b' && (i == 1 || !is_ident(b[i - 2]))
}

/// The tables that hold dependencies, in a manifest and under each `[target.<cfg>]`. The underscore spellings are the
/// old ones cargo still reads.
const DEPENDENCY_TABLES: [&str; 5] =
    ["dependencies", "dev-dependencies", "dev_dependencies", "build-dependencies", "build_dependencies"];

/// A token of source text: its 1-based line, and a whole identifier or one other non-space character.
pub type Token<'a> = (usize, &'a str);

/// The tokens of `code`. Run it on `code_only` text, so comments and string bodies are already blank.
pub fn tokens(code: &str) -> Vec<Token<'_>> {
    let word = |c: char| c.is_alphanumeric() || c == '_';
    let mut found = Vec::new();
    let mut line = 1;
    let mut chars = code.char_indices().peekable();
    while let Some((start, c)) = chars.next() {
        let mut end = start + c.len_utf8();
        if c == '\n' {
            line += 1;
        } else if word(c) {
            while let Some((i, next)) = chars.next_if(|&(_, n)| word(n)) {
                end = i + next.len_utf8();
            }
            found.push((line, &code[start..end]));
        } else if !c.is_whitespace() {
            found.push((line, &code[start..end]));
        }
    }
    found
}

/// Where `path`, written as in source (`std::fs`, `.append(`), occurs in `stream` as whole tokens: the index of its
/// first token. Spaces and line breaks between its tokens do not matter.
pub fn positions(stream: &[Token<'_>], path: &str) -> Vec<usize> {
    let wanted: Vec<&str> = tokens(path).iter().map(|&(_, text)| text).collect();
    let is_path = |window: &[Token<'_>]| window.iter().map(|&(_, text)| text).eq(wanted.iter().copied());
    stream.windows(wanted.len()).enumerate().filter(|(_, window)| is_path(window)).map(|(i, _)| i).collect()
}

/// The packages a crate manifest depends on, from every dependency table and every `[target.<cfg>]` table, renames
/// resolved: `x = { package = "j4n-core" }` is `j4n-core`, and `x.workspace = true` is whatever `x` is in the
/// `workspace` manifest. `_` is spelled `-`, as cargo treats them alike.
pub fn dependency_names(manifest: &str, workspace: &str) -> Vec<String> {
    let manifest: toml::Table = manifest.parse().expect("crate manifest is TOML");
    let workspace: toml::Table = workspace.parse().expect("workspace manifest is TOML");
    let inherited = workspace.get("workspace").and_then(|w| w.get("dependencies"));
    let mut scopes = vec![&manifest];
    if let Some(targets) = manifest.get("target").and_then(toml::Value::as_table) {
        scopes.extend(targets.values().filter_map(toml::Value::as_table));
    }
    let mut names = Vec::new();
    for scope in scopes {
        for table in DEPENDENCY_TABLES {
            let Some(dependencies) = scope.get(table).and_then(toml::Value::as_table) else { continue };
            names.extend(dependencies.iter().map(|(key, spec)| package_of(key, spec, inherited)));
        }
    }
    names
}

/// The package a dependency entry names: its own `package`, else its workspace entry's when it inherits, else its key.
fn package_of(key: &str, spec: &toml::Value, inherited: Option<&toml::Value>) -> String {
    let from_workspace = || {
        if spec.get("workspace").and_then(toml::Value::as_bool) != Some(true) {
            return None;
        }
        renamed(inherited?.get(key)?)
    };
    renamed(spec).or_else(from_workspace).unwrap_or(key).replace('_', "-")
}

/// The `package` an entry renames its dependency to, if it does.
fn renamed(entry: &toml::Value) -> Option<&str> {
    entry.get("package").and_then(toml::Value::as_str)
}

/// The workspace manifest, where `x.workspace = true` dependencies are declared.
pub fn workspace_manifest() -> String {
    read(&root().join("Cargo.toml"))
}

/// Every workspace crate as (directory name, manifest text).
pub fn crate_manifests() -> Vec<(String, String)> {
    crate_dirs()
        .iter()
        .map(|dir| (dir.file_name().unwrap_or_default().to_string_lossy().into_owned(), read(&dir.join("Cargo.toml"))))
        .collect()
}

/// Every Rust file under `crates/*/src`; a law that finds none would pass on nothing.
pub fn workspace_sources() -> Vec<PathBuf> {
    let files: Vec<PathBuf> = crate_dirs().iter().flat_map(|dir| walk_ext(&dir.join("src"), "rs")).collect();
    assert!(!files.is_empty(), "no Rust files under crates/*/src");
    files
}

#[test]
fn code_only_blanks_comments_and_literal_bodies_and_keeps_lines() {
    let src = "let a = \"x // y { \\\" }\"; // tail\nlet b = r#\"/* z \" */\"#; /* c\n d */ let c = 'q'; let d = '\\n'; fn f<'a>(x: &'a str) {}\n";
    let out = code_only(src);
    assert_eq!(out.lines().count(), src.lines().count());
    assert_eq!(out.chars().count(), src.chars().count());
    assert!(!out.contains("tail") && !out.contains("// y") && !out.contains("/* z") && !out.contains("c\n d"));
    assert!(out.contains("let a = \"") && out.contains("let c = ' '") && out.contains("fn f<'a>(x: &'a str)"));
}

#[test]
fn code_only_handles_nested_block_comments_and_byte_raw_strings() {
    let out = code_only("a /* one /* two */ still */ b; br#\"in\"# c");
    assert!(out.contains("a ") && out.contains(" b;") && out.contains(" c"));
    assert!(!out.contains("one") && !out.contains("still") && !out.contains("in\""));
}

#[test]
fn tokens_are_whole_identifiers_or_single_characters_with_their_lines() {
    let stream = tokens("use a::b_1;\n  x.y(é)\n");
    let texts: Vec<&str> = stream.iter().map(|&(_, text)| text).collect();
    assert_eq!(texts, ["use", "a", ":", ":", "b_1", ";", "x", ".", "y", "(", "é", ")"]);
    let lines: Vec<usize> = stream.iter().map(|&(line, _)| line).collect();
    assert_eq!(lines, [1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2]);
}

#[test]
fn a_path_matches_whole_tokens_across_spaces_and_lines_only() {
    let stream = tokens("std\n  :: fs::read; mystd::fs; std::fsx; std::fs");
    let lines: Vec<usize> = positions(&stream, "std::fs").iter().map(|&i| stream[i].0).collect();
    assert_eq!(lines, [1, 2]);
    assert!(positions(&tokens("std"), "std::fs").is_empty());
    assert!(positions(&[], "std").is_empty());
}

#[test]
fn walking_the_repository_skips_git_and_cargo_state_at_the_top_only() {
    let top = root();
    let files = walk(&top);
    assert!(files.iter().any(|path| path.ends_with("Cargo.toml")));
    assert!(!files.iter().any(|path| path.starts_with(top.join("target")) || path.starts_with(top.join(".git"))));
}

#[test]
fn a_git_file_at_the_top_is_skipped_like_a_git_directory() {
    let dir = std::env::temp_dir().join(format!("j4n-walk-git-{}", std::process::id()));
    fs::create_dir_all(dir.join("target")).expect("temp tree");
    fs::write(dir.join(".git"), "gitdir: /elsewhere/.git/worktrees/w\n").expect("temp file");
    fs::write(dir.join("target/x"), "x").expect("temp file");
    fs::write(dir.join("a.rs"), "x").expect("temp file");
    let found = walk_skipping(&dir, &SKIPPED_DIRS);
    fs::remove_dir_all(&dir).expect("remove temp tree");
    assert_eq!(found, [dir.join("a.rs")]);
}

#[test]
fn a_directory_named_target_below_the_top_is_read() {
    let dir = std::env::temp_dir().join(format!("j4n-walk-{}", std::process::id()));
    fs::create_dir_all(dir.join("src/target")).expect("temp tree");
    fs::write(dir.join("src/target/mod.rs"), "x").expect("temp file");
    let found = walk(&dir);
    fs::remove_dir_all(&dir).expect("remove temp tree");
    assert_eq!(found.len(), 1, "{found:?}");
}

/// What `open` gives once it no longer says another J4n has the Log open (for up to 10 s): the Log a test before let go of
/// may still be held a moment. Any other error, or one after 10 s, fails the test.
pub fn opened<W>(mut open: impl FnMut() -> Result<W, String>) -> W {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        match open() {
            Ok(w) => return w,
            Err(e) if e.contains("another J4n") && std::time::Instant::now() < end => std::thread::sleep(std::time::Duration::from_millis(50)),
            Err(e) => panic!("opens: {e}"),
        }
    }
}
