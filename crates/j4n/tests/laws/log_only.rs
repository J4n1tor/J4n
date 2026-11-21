//! Law 3, one log: no file is opened for append outside the Log. A record is an event and a ledger is a fold, so the
//! one append-mode file lives in the Log module (`crates/j4n-core/src/log.rs` or `log/`). Anywhere
//! else under `crates/*/src`, `O_APPEND`, `.append(true)` and an `OpenOptions` chain that calls `.append(…)` are
//! violations.
//!
//! The scan is statement-based. An `OpenOptions` kept in a variable whose append flag is set in a later statement with a
//! value that is not a literal (`o.append(flag)`) is not seen; closing that gap needs binding tracking.

use crate::support::{assert_clean, code_only, positions, read, rel, tokens, workspace_sources};

/// Whether the repo-relative `path` is the Log module, the only place a file may be opened for append.
fn is_log_module(path: &str) -> bool {
    path == "crates/j4n-core/src/log.rs" || path.starts_with("crates/j4n-core/src/log/")
}

/// The lines, sorted, where a file is opened for append: the token `O_APPEND`; `.append(true)` anywhere; and any
/// `.append(…)` in a statement (up to the next `;`) that builds an `OpenOptions` or `File::options()`, which catches
/// `.append(flag)`. `.append(&mut other)` is `Vec::append` and its kin, never an open flag; a Log's own `append(event)`
/// called on its own statement is not an open either.
fn append_opens(src: &str) -> Vec<usize> {
    let code = code_only(src);
    let stream = tokens(&code);
    let mut lines: Vec<usize> = positions(&stream, "O_APPEND").into_iter().map(|i| stream[i].0).collect();
    for statement in stream.split(|&(_, text)| text == ";") {
        let builds_options =
            !positions(statement, "OpenOptions").is_empty() || !positions(statement, "File::options").is_empty();
        for call in positions(statement, ".append(") {
            let flag = statement.get(call + 3).map(|&(_, text)| text);
            if flag == Some("true") || (builds_options && flag != Some("&")) {
                lines.push(statement[call].0);
            }
        }
    }
    lines.sort_unstable();
    lines.dedup();
    lines
}

#[test]
fn no_file_is_opened_for_append_outside_the_log() {
    let mut violations = Vec::new();
    for path in workspace_sources() {
        let name = rel(&path);
        if is_log_module(&name) {
            continue;
        }
        for line in append_opens(&read(&path)) {
            violations.push(format!(
                "{name}:{line}: file opened for append — only the Log (crates/j4n-core/src/log.rs) does that"
            ));
        }
    }
    assert_clean("law 3 (one log)", &violations);
}

#[test]
fn the_log_module_is_allowed_by_path_and_nothing_near_it() {
    assert!(is_log_module("crates/j4n-core/src/log.rs"));
    assert!(is_log_module("crates/j4n-core/src/log/segment.rs"));
    assert!(!is_log_module("crates/j4n-core/src/logger.rs"));
    assert!(!is_log_module("crates/j4n-core/src/log_tests.rs"));
    assert!(!is_log_module("crates/j4n-core/src/table/log.rs"));
    assert!(!is_log_module("crates/j4n-connect/src/log.rs"));
    assert!(!is_log_module("crates/j4n-core/src/lib.rs"));
}

#[test]
fn an_o_append_flag_is_caught() {
    assert_eq!(append_opens("let fd = libc::open(p, libc::O_WRONLY | libc::O_APPEND);\n"), [1]);
    assert_eq!(append_opens("use std::os::unix::fs::OpenOptionsExt;\nlet f = o.custom_flags(O_APPEND);\n"), [2]);
}

#[test]
fn append_true_is_caught_wherever_it_is() {
    assert_eq!(append_opens("fn open(o: &mut OpenOptions) {\n    o.append(true);\n}\n"), [2]);
    assert_eq!(append_opens("let w = opts.append(true).open(p)?;\n"), [1]);
    assert_eq!(append_opens("OpenOptions::new().append(true).open(p)?;"), [1]);
}

#[test]
fn a_flag_on_an_open_options_chain_is_caught_on_the_line_of_the_call() {
    let src = "let file = OpenOptions::new()\n    .create(true)\n    .append(flag)\n    .open(path)?;\n";
    assert_eq!(append_opens(src), [3]);
    assert_eq!(append_opens("let f = std::fs::File::options().append(on).open(p)?;\n"), [1]);
    assert_eq!(append_opens("let f = File::options()\n    .append(self.append())\n    .open(p)?;\n"), [2]);
}

#[test]
fn vec_append_and_its_kin_are_not_opens() {
    assert!(append_opens("a.append(&mut b);\nq.append(&mut r);\n").is_empty());
    assert!(
        append_opens("let f = OpenOptions::new().read(true).open(p).map(|f| out.append(&mut load(f)));\n").is_empty()
    );
    assert!(append_opens("let mut all = Vec::new();\nall.append(&mut part);\n").is_empty());
}

#[test]
fn calling_a_method_named_append_on_a_log_is_not_an_open() {
    let src = "let f = OpenOptions::new().read(true).open(p)?;\nlog.append(event);\nself.log.append(&event);\n";
    assert!(append_opens(src).is_empty());
}

#[test]
fn comments_and_string_bodies_never_trigger() {
    let src = r##"
// OpenOptions::new().append(true)
/* libc::O_APPEND */
/// `.append(true)` opens for append
const NOTE: &str = "O_APPEND and OpenOptions .append(flag)";
const RAW: &str = r#"o.append(true);"#;
fn f() {}
"##;
    assert!(append_opens(src).is_empty());
}

#[test]
fn a_line_is_reported_once_and_in_order() {
    let src =
        "let a = O_APPEND | O_APPEND;\nlet b = 1;\nOpenOptions::new().append(true).append(true);\nlet c = O_APPEND;\n";
    assert_eq!(append_opens(src), [1, 3, 4]);
}
