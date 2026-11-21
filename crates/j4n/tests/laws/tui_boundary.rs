//! Law 8, one view layer: `j4n-tui` draws what `j4n-proto` carries and nothing else. Its manifest names the protocol
//! and UI crates only, the protocol depends on no other workspace crate, only the binary depends on the view layer,
//! and the view layer's code never names the core or touches files, processes or sockets.

use crate::support::{
    Token, assert_clean, code_only, crate_manifests, dependency_names, positions, read, rel, root, tokens, walk_ext,
    workspace_manifest,
};

/// The only crates `j4n-tui` may depend on. Adding one is a deliberate edit, in a step that says why: `unicode-width` is
/// for how many columns a text takes on a screen (`ratatui` depends on it already); `pulldown-cmark` is for the reader's CommonMark; `dither-core` is the pixel engine of the charts (it has no dependency).
const TUI_DEPENDENCIES: [&str; 6] = ["j4n-proto", "ratatui", "crossterm", "unicode-width", "pulldown-cmark", "dither-core"];

/// The one crate that may depend on `j4n-tui`: the binary wires the view layer to the core.
const BINARY: &str = "j4n";

/// What the view layer's code never names: the core crates, the std modules that reach files, processes and sockets, and
/// the `Path` methods that read the disk without `std::fs` (`path.read_dir()`, `path.canonicalize()`).
const FORBIDDEN: [&str; 15] = [
    "j4n_core",
    "j4n_connect",
    "j4n_reach",
    "std::fs",
    "std::process",
    "std::net",
    "std::os",
    "File::open",
    "OpenOptions",
    "read_to_string",
    "read_dir",
    "canonicalize",
    "read_link",
    "symlink_metadata",
    "try_exists",
];

/// Dependencies of `j4n-tui` that are not in `TUI_DEPENDENCIES`.
fn tui_dependency_violations(manifest: &str, workspace: &str) -> Vec<String> {
    dependency_names(manifest, workspace)
        .into_iter()
        .filter(|name| !TUI_DEPENDENCIES.contains(&name.as_str()))
        .map(|name| format!("j4n-tui depends on `{name}`; it may depend on {} only", TUI_DEPENDENCIES.join(", ")))
        .collect()
}

/// Dependencies of `j4n-proto` on a workspace crate: the protocol is plain data that every other crate may read.
fn proto_dependency_violations(manifest: &str, workspace: &str) -> Vec<String> {
    dependency_names(manifest, workspace)
        .into_iter()
        .filter(|name| name == BINARY || name.starts_with("j4n-"))
        .map(|name| format!("j4n-proto depends on `{name}`; the protocol depends on no workspace crate"))
        .collect()
}

/// `crate_name` depends on `j4n-tui` and is not the binary; that covers `j4n-core`, `j4n-connect` and `j4n-reach`.
fn tui_dependent_violations(crate_name: &str, manifest: &str, workspace: &str) -> Vec<String> {
    let depends = dependency_names(manifest, workspace).iter().any(|name| name == "j4n-tui");
    if crate_name == BINARY || !depends {
        return Vec::new();
    }
    vec![format!("{crate_name} depends on j4n-tui; only the binary `{BINARY}` wires the view layer in")]
}

/// Where the view layer's code names something in `FORBIDDEN`, as (line, what), sorted. A name hidden in a
/// `std::{ … }` group (`use std::{fmt, fs};`) is named all the same.
fn view_layer_hits(src: &str) -> Vec<(usize, &'static str)> {
    let code = code_only(src);
    let stream = tokens(&code);
    let mut hits = Vec::new();
    for path in FORBIDDEN {
        hits.extend(positions(&stream, path).into_iter().map(|i| (stream[i].0, path)));
        if let Some(module) = path.strip_prefix("std::") {
            hits.extend(in_std_groups(&stream, module).into_iter().map(|line| (line, path)));
        }
    }
    hits.sort_unstable();
    hits
}

/// The lines where `module` is named inside a `std::{ … }` group.
fn in_std_groups(stream: &[Token<'_>], module: &str) -> Vec<usize> {
    let mut lines = Vec::new();
    for open in positions(stream, "std::{") {
        let mut depth = 0;
        for &(line, text) in &stream[open + 3..] {
            match text {
                "{" => depth += 1,
                "}" => depth -= 1,
                _ if text == module => lines.push(line),
                _ => {}
            }
            if depth == 0 {
                break;
            }
        }
    }
    lines
}

#[test]
fn the_view_layer_depends_only_on_the_protocol_and_ui_crates() {
    let manifest = read(&root().join("crates/j4n-tui/Cargo.toml"));
    assert_clean("law 8 (one view layer)", &tui_dependency_violations(&manifest, &workspace_manifest()));
}

#[test]
fn the_protocol_crate_depends_on_no_workspace_crate() {
    let manifest = read(&root().join("crates/j4n-proto/Cargo.toml"));
    assert_clean("law 8 (one view layer)", &proto_dependency_violations(&manifest, &workspace_manifest()));
}

#[test]
fn only_the_binary_depends_on_the_view_layer() {
    let workspace = workspace_manifest();
    let violations: Vec<String> = crate_manifests()
        .iter()
        .flat_map(|(name, manifest)| tui_dependent_violations(name, manifest, &workspace))
        .collect();
    assert_clean("law 8 (one view layer)", &violations);
}

#[test]
fn the_view_layer_sources_never_reach_the_core_or_the_disk() {
    let files = walk_ext(&root().join("crates/j4n-tui/src"), "rs");
    assert!(!files.is_empty(), "crates/j4n-tui/src has no Rust files");
    let mut violations = Vec::new();
    for path in files {
        for (line, what) in view_layer_hits(&read(&path)) {
            violations
                .push(format!("{}:{line}: names `{what}` — the view layer draws what the protocol sends", rel(&path)));
        }
    }
    assert_clean("law 8 (one view layer)", &violations);
}

#[test]
fn a_manifest_with_only_the_allowed_crates_passes() {
    let manifest = r#"
[dependencies]
j4n-proto.workspace = true
ratatui = "0.30"
crossterm = { version = "0.29", default-features = false }

[dev-dependencies]
j4n-proto = { path = "../j4n-proto" }

[target.'cfg(unix)'.dependencies]
crossterm = "0.29"
"#;
    assert!(tui_dependency_violations(manifest, "").is_empty());
    assert!(tui_dependency_violations("[package]\nname = \"j4n-tui\"\n", "").is_empty());
}

#[test]
fn a_foreign_dependency_is_caught_in_every_dependency_table() {
    let manifest = r#"
[package]
name = "j4n-tui"

[dependencies]
j4n-proto.workspace = true
serde = "1"

[dev-dependencies]
tempfile = "3"

[build-dependencies]
cc = "1"

[target.'cfg(unix)'.dependencies]
libc = "0.2"

[target.'cfg(windows)'.dev-dependencies.winapi]
version = "0.3"
"#;
    let mut names = dependency_names(manifest, "");
    names.sort();
    assert_eq!(names, ["cc", "j4n-proto", "libc", "serde", "tempfile", "winapi"]);
    let violations = tui_dependency_violations(manifest, "");
    assert_eq!(violations.len(), 5);
    assert!(violations[0].contains("`serde`") && violations[0].contains("j4n-proto, ratatui, crossterm"));
}

#[test]
fn a_renamed_dependency_is_judged_by_its_package() {
    let manifest = "[dependencies]\ncore = { package = \"j4n-core\", path = \"../j4n-core\" }\nui = { package = \"ratatui\", version = \"0.30\" }\n";
    assert_eq!(dependency_names(manifest, ""), ["j4n-core", "ratatui"]);
    let violations = tui_dependency_violations(manifest, "");
    assert_eq!(violations.len(), 1);
    assert!(violations[0].contains("`j4n-core`"));
}

#[test]
fn a_rename_declared_in_the_workspace_is_judged_by_its_package() {
    let workspace =
        "[workspace.dependencies]\ncore = { package = \"j4n-core\", path = \"crates/j4n-core\" }\nratatui = \"0.30\"\n";
    let manifest = "[dependencies]\ncore.workspace = true\nratatui.workspace = true\n";
    assert_eq!(dependency_names(manifest, workspace), ["j4n-core", "ratatui"]);
    assert_eq!(tui_dependency_violations(manifest, workspace).len(), 1);
}

#[test]
fn the_underscore_spellings_cargo_still_reads_are_read_too() {
    let manifest = "[dev_dependencies]\nj4n_core = { path = \"../j4n-core\" }\n\n[build_dependencies]\ncc = \"1\"\n";
    assert_eq!(dependency_names(manifest, ""), ["j4n-core", "cc"]);
    assert_eq!(tui_dependency_violations(manifest, "").len(), 2);
}

#[test]
fn a_workspace_dependency_of_the_protocol_crate_is_caught() {
    let bad = "[dependencies]\nserde = \"1\"\nj4n-core.workspace = true\nj4n-tui = { path = \"../j4n-tui\" }\nj4n = { path = \"../j4n\" }\n";
    assert_eq!(proto_dependency_violations(bad, "").len(), 3);
    let renamed = "[dependencies]\ndata = { package = \"j4n-core\", path = \"../j4n-core\" }\n";
    assert_eq!(proto_dependency_violations(renamed, "").len(), 1);
    assert!(proto_dependency_violations("[dependencies]\nserde = \"1\"\nserde_json = \"1\"\n", "").is_empty());
}

#[test]
fn a_dependent_other_than_the_binary_is_caught() {
    let wired = "[dependencies]\nj4n-tui.workspace = true\n";
    assert!(tui_dependent_violations("j4n", wired, "").is_empty());
    for name in ["j4n-core", "j4n-connect", "j4n-reach", "j4n-proto"] {
        assert_eq!(tui_dependent_violations(name, wired, "").len(), 1, "{name}");
    }
    let dev = "[dev-dependencies]\nj4n-tui = { path = \"../j4n-tui\" }\n";
    assert_eq!(tui_dependent_violations("j4n-core", dev, "").len(), 1);
    assert!(tui_dependent_violations("j4n-core", "[dependencies]\nj4n-proto.workspace = true\n", "").is_empty());
}

#[test]
fn names_of_the_core_and_of_the_disk_are_caught_with_their_lines() {
    let src = "use j4n_core::Table;\nuse std::fs;\nfn f() {\n    let c = std::process::Command::new(x);\n    let s = std::net::TcpStream::connect(a);\n    let f = File::open(p);\n    let o = OpenOptions::new();\n    r.read_to_string(&mut b);\n    j4n_connect::run();\n    j4n_reach::get();\n}\n";
    assert_eq!(
        view_layer_hits(src),
        [
            (1, "j4n_core"),
            (2, "std::fs"),
            (4, "std::process"),
            (5, "std::net"),
            (6, "File::open"),
            (7, "OpenOptions"),
            (8, "read_to_string"),
            (9, "j4n_connect"),
            (10, "j4n_reach"),
        ]
    );
    assert_eq!(view_layer_hits("let r = std\n    ::fs::read(p);\n"), [(1, "std::fs")]);
}

#[test]
fn reading_the_disk_through_path_methods_or_the_os_module_is_caught() {
    let src = "use std::os::unix::net::UnixStream;\nfn f(p: &Path) {\n    let d = p.read_dir();\n    let c = p.canonicalize();\n    let l = p.read_link();\n    let m = p.symlink_metadata();\n    let e = p.try_exists();\n}\n";
    assert_eq!(
        view_layer_hits(src),
        [
            (1, "std::os"),
            (3, "read_dir"),
            (4, "canonicalize"),
            (5, "read_link"),
            (6, "symlink_metadata"),
            (7, "try_exists")
        ]
    );
    assert!(view_layer_hits("fn f(row: &Row) -> bool { row.metadata.exists }\n").is_empty());
}

#[test]
fn a_std_group_import_cannot_hide_a_module() {
    let src = "use std::{fmt, fs};\nuse std::{\n    io,\n    process::Command,\n};\nuse std::{collections::HashMap, fmt::Write, net};\n";
    assert_eq!(view_layer_hits(src), [(1, "std::fs"), (4, "std::process"), (6, "std::net")]);
    assert!(view_layer_hits("use std::{collections::HashMap, fmt, io::{self, Write}};\nfn fs() {}\n").is_empty());
}

#[test]
fn similar_names_are_not_the_forbidden_ones() {
    let src = "use std::fmt;\nuse std::fsx::a;\nlet a = mystd::fs::read(p);\nlet b = my_j4n_core::x();\nlet c = j4n_core_ext::y();\nlet d = MyFile::open(p);\nlet e = r.read_to_string_lossy();\nlet f = File::create(p);\nlet g = OpenOptionsExt;\n";
    assert!(view_layer_hits(src).is_empty());
}

#[test]
fn comments_and_string_bodies_in_view_code_do_not_count() {
    let src = r##"
// use std::fs;
/* j4n_core::x */
const NAME: &str = "read_to_string File::open";
const RAW: &str = r#"std::process"#;
/// OpenOptions
fn f() {}
"##;
    assert!(view_layer_hits(src).is_empty());
}

/// The files that may name a pipe glyph or a pipe slot the base of the trees and the table of glyphs, and two files that draw lines that
/// are no tree: the line chart (`plot.rs`: its cells are a mask of line pieces) and the reader's grid of a Markdown table (`read/lay.rs`).
const PIPE_FILES: [&str; 4] = ["display/layout.rs", "theme/glyph.rs", "display/plot.rs", "read/lay.rs"];

/// The slots that are the pieces of a tree: no drawer names one but the base.
const PIPE_SLOTS: [&str; 9] = ["PipeBar", "PipeTee", "PipeElbow", "PipeTop", "BranchTee", "BranchElbow", "HangBar", "AiryTop", "AiryChild"];

#[test]
fn no_file_of_the_view_layer_draws_a_pipe_but_the_base_of_the_trees() {
    let base = root().join("crates/j4n-tui/src");
    let mut violations = Vec::new();
    for path in walk_ext(&base, "rs") {
        let name = path.strip_prefix(&base).expect("under src").to_string_lossy().into_owned();
        if PIPE_FILES.contains(&name.as_str()) {
            continue;
        }
        let src = read(&path);
        for (n, line) in code_only(&src).lines().enumerate() {
            if let Some(slot) = PIPE_SLOTS.iter().find(|s| line.contains(&format!("Slot::{s}"))) {
                violations.push(format!("{}:{}: names the slot `{slot}`: a pipe is drawn by `display/layout` (`Look`), `parts/rows` for a stack", rel(&path), n + 1));
            }
        }
        for (n, line) in src.lines().enumerate() {
            if !line.trim_start().starts_with("//") && line.contains(['├', '╰', '└', '┆']) {
                violations.push(format!("{}:{}: a pipe glyph in code: draw it with `display/layout` (`Look`)", rel(&path), n + 1));
            }
        }
    }
    assert_clean("one base for the pipes of trees", &violations);
}
