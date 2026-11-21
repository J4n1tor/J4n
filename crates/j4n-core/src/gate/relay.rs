//! A lane's remote shell goes through J4n: what a line prints on the other side of an `ssh` comes straight back to the
//! model, so the line is run inside `j4n relay`, which replaces every secret in the output by its placeholder before anything is printed.
//!
//! [`route`] says what J4n does with a line the Gate lets run: it is not a remote shell and runs as it is; it is one, and this is the line it runs instead
//! (the whole line, run by `bash -c` inside the relay, so a pipe, a `&&` and the rest of the line are the lane's own and all of its output is looked at); or it is
//! one whose output J4n cannot put its redaction on (the output goes to a file, a background job, a file copied back), which the Gate asks about with the reason.
//! What decides is [`super::danger::Remote`], which the line matcher gives.

use super::danger::{Remote, danger};

/// What is done with a line that is to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// Not a remote shell: it runs as it is.
    Not,
    /// A remote shell: this line runs instead of it.
    Routed(String),
    /// A remote shell whose output J4n cannot redact: the operator is asked first, and it runs as it is when they say yes.
    Cannot,
}

/// `s` as one word of a shell line, in single quotes.
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// The line J4n runs in place of `cmd`: `cmd` run by `bash -c` inside `<exe> relay --`.
pub fn wrap(cmd: &str, exe: &str) -> String {
    format!("{} relay -- bash -c {}", quote(exe), quote(cmd))
}

/// What J4n does with the line `cmd`, given `exe`, the path of its own program (the one that has `relay`).
pub fn route(cmd: &str, exe: &str) -> Route {
    match danger(cmd).remote {
        Remote::No => Route::Not,
        Remote::Via => Route::Routed(wrap(cmd, exe)),
        Remote::Cannot => Route::Cannot,
    }
}
