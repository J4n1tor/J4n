//! Tier 0: a fake `ssh` on the PATH prints a made-up env file with a token, a database URL with a password and a key; the line a
//! lane runs, as J4n routes it, hands the model placeholders and not the values, with the exit status kept; nothing of the values is written under J4n's folder. No real host and
//! no real secret is used: the values are made up for the test.

use crate::run_cases::{Dirs, Out, j4n};
use j4n_core::gate::relay::{Route, route};
use std::fs;
use std::os::unix::fs::PermissionsExt;

const TOKEN: &str = "Zq8Lm2Vx9Rt4Ys6Wb3Nc7Hd5Jk1Pf0Ae";
const PASS: &str = "Wp4Rt7Yu2Io9Lk3S";
const KEY: &str = "b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZWQyNTUxOQAAACD0Xk3QpLm9Zr2VfHt7Qe1UoY8cNwJ5bT6sA4dRgIhK2gAAAJgq";

/// A `ssh` that ignores its words and prints a server's env file, a warning on stderr that holds the token, and fails with 3. The PATH that finds it.
fn fake_ssh(d: &Dirs) -> String {
    let body = format!(
        "#!/bin/sh\ncat <<'EOF'\nAPP_TOKEN={TOKEN}\nDATABASE_URL=postgres://app:{PASS}@db.internal:5432/app\nPORT=8088\nthe service is active\n-----BEGIN OPENSSH PRIVATE KEY-----\n{KEY}\n{KEY}\n-----END OPENSSH PRIVATE KEY-----\nEOF\necho \"warning: APP_TOKEN={TOKEN} was used\" >&2\nexit 3\n"
    );
    let path = d.dir.join("bin/ssh");
    fs::write(&path, body).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    format!("{}:{}", d.dir.join("bin").display(), std::env::var("PATH").unwrap_or_default())
}

fn clean(out: &Out) {
    for (what, text) in [("stdout", &out.out), ("stderr", &out.err)] {
        for secret in [TOKEN, PASS, KEY] {
            assert!(!text.contains(secret), "{what} holds `{secret}`: {text}");
        }
    }
}

#[test]
fn the_relay_hands_on_what_a_remote_shell_printed_with_every_secret_replaced_and_its_exit_status() {
    let d = Dirs::new("relay1");
    let path = fake_ssh(&d);
    let out = j4n(&d, &["relay", "--", "ssh", "host", "cat", "/srv/app.env"], None, &[("PATH", &path)]);
    clean(&out);
    assert_eq!(out.rc, 3, "the program's own status: {}", out.err);
    assert!(out.out.contains("PORT=8088") && out.out.contains("the service is active"), "what is no secret is kept: {}", out.out);
    assert!(out.out.contains("APP_TOKEN=") && out.out.contains("REDACTED"), "the token's place is marked: {}", out.out);
    assert!(out.out.contains("postgres://app:") && out.out.contains("@db.internal:5432/app"), "the URL keeps its shape: {}", out.out);
    assert!(out.err.contains("REDACTED"), "stderr is looked at as well: {}", out.err);
}

#[test]
fn the_line_j4n_routes_a_remote_shell_to_gives_the_model_placeholders_and_nothing_of_the_values_is_kept() {
    let d = Dirs::new("relay2");
    let path = fake_ssh(&d);
    let exe = env!("CARGO_BIN_EXE_j4n");
    let Route::Routed(line) = route("ssh -p 22 root@host 'cat /srv/app.env' 2>&1 | tail -20", exe) else { panic!("a remote shell is routed") };
    assert!(line.starts_with(&format!("'{exe}' relay -- bash -c ")), "{line}");
    let out = std::process::Command::new("sh").arg("-c").arg(&line).env("PATH", &path).env("HOME", d.home()).env("J4N_HOME", d.j4n_home()).output().unwrap();
    let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned());
    for secret in [TOKEN, PASS, KEY] {
        assert!(!stdout.contains(secret) && !stderr.contains(secret), "the model would read `{secret}`: {stdout}{stderr}");
    }
    assert!(stdout.contains("PORT=8088") && stdout.contains("REDACTED"), "{stdout}");
    for f in crate::support::walk(&d.dir).into_iter().filter(|f| !f.starts_with(d.dir.join("bin"))) {
        let text = fs::read_to_string(&f).unwrap_or_default();
        assert!(!text.contains(TOKEN) && !text.contains(PASS), "{} keeps a value", f.display());
    }
}

#[test]
fn a_line_that_cannot_be_wrapped_is_not_routed() {
    let exe = env!("CARGO_BIN_EXE_j4n");
    assert_eq!(route("ssh h 'cat /srv/x' > out.txt", exe), Route::Cannot);
    assert_eq!(route("ls -la", exe), Route::Not);
}

#[test]
fn the_relay_with_nothing_to_run_or_a_program_that_is_not_there_says_so() {
    let d = Dirs::new("relay3");
    assert_eq!(j4n(&d, &["relay"], None, &[]).rc, 2);
    assert_eq!(j4n(&d, &["relay", "--", "no-such-program-here"], None, &[]).rc, 127);
}
