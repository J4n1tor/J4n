//! Every program J4n starts goes through the scrub: the variables by which the program that runs J4n binds a child to itself
//! (a socket, a token, a session id) are taken out before the variables J4n means to give are put in. A `Command::new` in `crates/*/src` whose function does
//! not call `scrub(` (`proc::scrub`) or the core `Scrub` before its first `.env` / `.envs` / `.env_clear`, and is not on the list below, is a program that
//! would inherit the host's session, and the law names its file and line.
//!
//! The list is by file and program, never by line, each with its reason; an entry that no longer matches a site (the site is gone or now scrubbed) fails too,
//! so the list cannot go stale. A site on the list still takes the operator's keys out (`host::keyless`, or an `.env_clear`): a project's git hooks run there.
//!
//! The operator's own key variables are taken out by the same scrub, and the real binary shows where they still go: a shell line and a tool get none,
//! J4n's own agent reads its key in J4n's process as before, and claude is given the key its row names, the one J4n keeps once it is bound to it.

use crate::models_cases::mock::{Mock, Say};
use crate::run_cases::{Dirs, j4n};
use crate::support::{assert_clean, code_only, rel, read, root, workspace_sources};
use serde_json::json;
use std::fs;
use std::os::unix::fs::PermissionsExt;

/// `(file, the program as written, why it is no program that runs an agent)`.
const ALLOWED: [(&str, &str, &str); 8] = [
    ("crates/j4n-connect/src/place/worktree.rs", "\"git\"", "git of the lane: no agent, no session"),
    ("crates/j4n-core/src/place/checkpoint.rs", "\"git\"", "git of the checkpoint: no agent, no session"),
    ("crates/j4n-qc/src/change.rs", "\"git\"", "git diff of the change: no agent"),
    ("crates/j4n-shape/src/state.rs", "\"git\"", "git ls-files: no agent"),
    ("crates/j4n/src/author.rs", "\"git\"", "git of the author's own check: no agent"),
    ("crates/j4n-forge/src/play.rs", "\"kill\"", "the forge's fake agent kills itself on a cue"),
    ("crates/j4n-connect/src/place/view.rs", "cmd", "the probe of the box's wall (`bwrap … cat`): a fixed script, no agent"),
    ("crates/j4n-connect/src/place/boxed.rs", "cmd", "the probe of the box (`bwrap … true`): a fixed program, no agent"),
];

/// The program of a `Command::new(…)` as written, from the line that has it.
fn program(line: &str) -> String {
    let rest = line.split_once("Command::new(").map_or("", |(_, r)| r);
    let mut depth = 0usize;
    let mut out = String::new();
    for c in rest.chars() {
        match c {
            '(' => depth += 1,
            ')' if depth == 0 => break,
            ')' => depth -= 1,
            _ => {}
        }
        out.push(c);
    }
    out.trim().trim_start_matches('&').to_string()
}

fn starts_fn(line: &str) -> Option<usize> {
    let t = line.trim_start();
    let t = t.strip_prefix("pub(crate) ").or_else(|| t.strip_prefix("pub ")).unwrap_or(t);
    let t = t.strip_prefix("async ").or_else(|| t.strip_prefix("unsafe ")).unwrap_or(t);
    t.starts_with("fn ").then(|| line.len() - line.trim_start().len())
}

/// The sites of `src` (file `path`): `(line, program, scrubbed, keyless)`. A site in a `#[cfg(test)]` part is not looked at.
fn sites(src: &str) -> Vec<(usize, String, bool, bool)> {
    let code = code_only(src);
    let lines: Vec<&str> = code.lines().collect();
    let orig: Vec<&str> = src.lines().collect();
    let end = lines.iter().position(|l| l.trim() == "#[cfg(test)]").unwrap_or(lines.len());
    let mut found = Vec::new();
    for at in (0..end).filter(|i| lines[*i].contains("Command::new(")) {
        let Some(start) = (0..=at).rev().find(|i| starts_fn(lines[*i]).is_some()) else { continue };
        let indent = starts_fn(lines[start]).unwrap_or(0);
        let stop = (at + 1..lines.len()).find(|i| starts_fn(lines[*i]).is_some_and(|n| n <= indent)).unwrap_or(lines.len());
        let body = &lines[start..stop];
        let scrub = body.iter().position(|l| l.contains("scrub(") || l.contains("Scrub::"));
        let env = body.iter().position(|l| [".env(", ".envs(", ".env_clear("].iter().any(|e| l.contains(e)));
        let ok = match (scrub, env) {
            (Some(s), Some(e)) => s <= e,
            (Some(_), None) => true,
            _ => false,
        };
        let keyless = ok || ["keyless(", ".env_clear("].iter().any(|k| lines[at].contains(k));
        found.push((at + 1, program(orig[at]), ok, keyless));
    }
    found
}

/// The violations of one file and the entries of `allowed` it used.
fn judge(path: &str, src: &str, allowed: &[(&str, &str, &str)]) -> (Vec<String>, Vec<usize>) {
    let (mut bad, mut used) = (Vec::new(), Vec::new());
    for (line, program, ok, keyless) in sites(src) {
        if ok {
            continue;
        }
        match allowed.iter().position(|(f, p, _)| *f == path && *p == program) {
            Some(i) if keyless => used.push(i),
            Some(i) => {
                used.push(i);
                bad.push(format!("{path}:{line}: `Command::new({program})` is on the list and hands on the operator's keys: call `j4n_core::host::keyless(&mut cmd)` before its `.env`"));
            }
            None => bad.push(format!("{path}:{line}: `Command::new({program})` starts a program that is not scrubbed: call `proc::scrub(&mut cmd)` before its `.env`, or list it in tests/laws/scrub_sites.rs with its reason")),
        }
    }
    (bad, used)
}

#[test]
fn every_program_j4n_starts_is_scrubbed_or_on_the_list() {
    let (mut bad, mut used) = (Vec::new(), Vec::new());
    for file in workspace_sources() {
        let (b, u) = judge(&rel(&file), &read(&file), &ALLOWED);
        bad.extend(b);
        used.extend(u);
    }
    assert_clean("the scrub covers every program J4n starts", &bad);
    let stale: Vec<String> =
        ALLOWED.iter().enumerate().filter(|(i, _)| !used.contains(i)).map(|(_, (f, p, _))| format!("{f}: `Command::new({p})` is on the list and no longer an unscrubbed site: remove the entry")).collect();
    assert_clean("the list of programs that are not scrubbed holds only live entries", &stale);
}

#[test]
fn the_law_finds_a_program_that_is_not_scrubbed_and_not_one_that_is() {
    let bare = "fn run() {\n    let mut cmd = Command::new(program);\n    cmd.args(a).envs(&e);\n}\n";
    let (bad, _) = judge("a.rs", bare, &[]);
    assert_eq!(bad.len(), 1, "{bad:?}");
    assert!(bad[0].starts_with("a.rs:2:") && bad[0].contains("Command::new(program)"), "{bad:?}");
    let scrubbed = "fn run() {\n    let mut cmd = Command::new(program);\n    crate::proc::scrub(&mut cmd);\n    cmd.args(a).envs(&e);\n}\n";
    assert!(judge("a.rs", scrubbed, &[]).0.is_empty());
    let core = "fn run() {\n    let mut cmd = Command::new(&self.j4n);\n    j4n_core::host::Scrub::base().strip(&mut cmd);\n    cmd.env(\"A\", b);\n}\n";
    assert!(judge("a.rs", core, &[]).0.is_empty());
}

#[test]
fn a_scrub_after_the_variables_or_in_another_function_is_no_scrub() {
    let after = "fn run() {\n    let mut cmd = Command::new(p);\n    cmd.env_clear().env(\"A\", \"b\");\n    scrub(&mut cmd);\n}\n";
    assert_eq!(judge("a.rs", after, &[]).0.len(), 1);
    let other = "fn scrubbed() {\n    scrub(&mut c);\n}\n\nfn run() {\n    let mut cmd = Command::new(p);\n    cmd.args(a);\n}\n";
    let (bad, _) = judge("a.rs", other, &[]);
    assert_eq!(bad.len(), 1, "{bad:?}");
    assert!(bad[0].starts_with("a.rs:6:"), "{bad:?}");
    // a comment or a string that says it is not a call
    let said = "fn run() {\n    let mut cmd = Command::new(p); // scrub(&mut cmd)\n    let why = \"scrub(\";\n    cmd.args(a);\n}\n";
    assert_eq!(judge("a.rs", said, &[]).0.len(), 1);
}

#[test]
fn a_site_in_the_tests_is_not_looked_at_and_the_list_is_by_file_and_program() {
    let tests = "fn a() {}\n#[cfg(test)]\nmod tests {\n    fn t() {\n        Command::new(\"x\");\n    }\n}\n";
    assert!(judge("a.rs", tests, &[]).0.is_empty());
    let git = "fn run() {\n    keyless(&mut Command::new(\"git\")).arg(\"status\").output();\n}\n";
    let allowed = [("a.rs", "\"git\"", "git")];
    let (bad, used) = judge("a.rs", git, &allowed);
    assert!(bad.is_empty() && used == [0], "{bad:?}");
    // on the list, and the keys still go to it
    let keyed = "fn run() {\n    Command::new(\"git\").arg(\"status\").output();\n}\n";
    let (bad, _) = judge("a.rs", keyed, &allowed);
    assert!(bad.len() == 1 && bad[0].contains("hands on the operator's keys"), "{bad:?}");
    assert_eq!(judge("b.rs", git, &allowed).0.len(), 1, "another file is not on the list");
    let sh = "fn run() {\n    Command::new(\"sh\").output();\n}\n";
    assert_eq!(judge("a.rs", sh, &allowed).0.len(), 1, "another program is not on the list");
}

// ---- the operator's own keys ----

/// Made-up values of no vendor shape: the operator's OpenRouter and Anthropic keys, the Anthropic key J4n keeps, and a host's session id.
const ROUTER: &str = "scrubrouter7Qx2vLm9Kp4wTz";
const ANTHROPIC: &str = "scrubanthropic3Rb8nTz5Wq1";
const KEPT: &str = "scrubkept0Hd6yUc2Vj7sNe4k";
const HOST: &str = "scrubhost9Gs1Fa8rLp3Mx6Yv";

/// A reply of the gateway in OpenAI's stream: the calls, or the answer once the calls' results came back.
fn own_says(calls: &[(&str, &str, String)]) -> Say {
    let mut out = Vec::new();
    if calls.is_empty() {
        out.push(json!({ "choices": [{ "delta": { "content": "done" } }] }).to_string());
    } else {
        let frags: Vec<serde_json::Value> = calls.iter().enumerate().map(|(i, (id, name, args))| json!({ "index": i, "id": id, "function": { "name": name, "arguments": args } })).collect();
        out.push(json!({ "choices": [{ "delta": { "tool_calls": frags } }] }).to_string());
    }
    out.push(json!({ "choices": [{ "delta": {}, "finish_reason": if calls.is_empty() { "stop" } else { "tool_calls" } }] }).to_string());
    out.push(json!({ "choices": [], "usage": { "prompt_tokens": 10, "completion_tokens": 2 } }).to_string());
    Say::Sse(out)
}

/// Made-up values of the variables a program's row and the setting `keys.pass` name: a long-lived Claude sign-in and a GitHub token.
const OAUTH: &str = "scruboauth5Wm2Jt8Xc4Bq6Pz";
const GH: &str = "scrubgithub1Kd7Rn3Ys9Ve5Hu";

/// What an environment written by `env | sort` holds of `values`, which must not be there.
fn leaks_of(what: &str, env: &str, values: &[&str]) -> Vec<String> {
    assert!(env.lines().any(|l| l.starts_with("PATH=")), "{what} wrote its environment:\n{env}");
    values.iter().filter(|v| env.contains(*v)).map(|v| format!("{what} was given {v}: {}", env.lines().find(|l| l.contains(*v)).unwrap_or_default())).collect()
}

fn leaks(what: &str, env: &str) -> Vec<String> {
    leaks_of(what, env, &[ROUTER, ANTHROPIC, KEPT, HOST])
}

/// A turn of J4n's own agent (its key read from J4n's environment, `env`) against a gateway that asks for a shell line and a connected tool and
/// then answers: what the shell line and the tool were started with.
fn own_turn(d: &Dirs, env: &[(&str, &str)]) -> (String, String) {
    let dump = d.put("dump.sh", "#!/bin/sh\nenv | sort > \"$1\"\n");
    fs::set_permissions(&dump, fs::Permissions::from_mode(0o755)).unwrap();
    let (shell, tool) = (d.project().join("shell-env.txt"), d.project().join("tool-env.txt"));
    let gateway = Mock::start(|s, _| {
        if s.body.contains("\"role\":\"tool\"") {
            return own_says(&[]);
        }
        own_says(&[("c1", "run", json!({ "cmd": "sh ./dump.sh shell-env.txt" }).to_string()), ("c2", "dump", "{}".into())])
    });
    let conns = d.j4n_home().join("connectors");
    fs::create_dir_all(&conns).unwrap();
    fs::write(conns.join("own.toml"), format!("api = 1\n[[connector]]\nid = \"own\"\nurl = \"{}/v1\"\nmodels = [\"vendor/m\"]\n", gateway.url())).unwrap();
    let argv = json!(["sh", dump.display().to_string(), tool.display().to_string()]).to_string();
    fs::write(conns.join("dump.toml"), format!("api = 1\n[[connector]]\nid = \"dump\"\ndoc = \"Write the environment down.\"\nis = \"tool\"\ndialect = \"text\"\nprogram = \"sh\"\nargv = {argv}\nrisk = \"read\"\n")).unwrap();
    let o = j4n(d, &["run", "write the environment down", "--doer", "own", "--approval", "full", "--wall", "60s"], None, env);
    assert_eq!(o.rc, 0, "{}\n{}", o.out, o.err);
    let asked = gateway.seen();
    assert!(!asked.is_empty() && asked.iter().all(|r| r.headers.get("authorization") == Some(&format!("Bearer {ROUTER}"))), "J4n's own agent still reads its key from J4n's environment: {:?}", asked.iter().map(|r| r.headers.get("authorization")).collect::<Vec<_>>());
    (read(&shell), read(&tool))
}

/// A turn of claude, a stand-in that writes its environment at its start: what it was started with.
fn claude_turn(d: &Dirs, env: &[(&str, &str)]) -> String {
    let real = crate::run_cases::program(d, "claude-real", &read(&root().join("fixtures/claude-frames/fake-claude.sh")));
    let seen = d.dir.join("claude-env.txt");
    let fixture = root().join("fixtures/claude-frames/two-turns.jsonl");
    let mut env = env.to_vec();
    env.extend([("J4N_CLAUDE", real.to_str().unwrap()), ("FAKE_FIXTURE", fixture.to_str().unwrap()), ("FAKE_ENV_LOG", seen.to_str().unwrap())]);
    let o = j4n(d, &["run", "say one", "--doer", "claude", "--model", "haiku", "--approval", "full", "--wall", "60s"], None, &env);
    assert_eq!(o.rc, 0, "{}\n{}", o.out, o.err);
    read(&seen)
}

#[test]
fn a_key_of_the_operators_environment_reaches_only_the_program_whose_row_names_it() {
    let d = Dirs::new("scrub-sites-keys");
    d.offload_off();
    let mine = [("OPENROUTER_API_KEY", ROUTER), ("ANTHROPIC_API_KEY", ANTHROPIC), ("CLAUDE_CODE_SESSION_ID", HOST)];
    let (shell, tool) = own_turn(&d, &mine);
    let mut hits = leaks("a shell line of J4n's own agent", &shell);
    hits.extend(leaks("a connected tool", &tool));
    // claude: the operator's own Anthropic key is the one its row names
    let got = claude_turn(&d, &mine);
    assert!(got.lines().any(|l| l == format!("ANTHROPIC_API_KEY={ANTHROPIC}")), "claude has the key its row names:\n{got}");
    hits.extend(leaks("claude", &got.replace(&format!("ANTHROPIC_API_KEY={ANTHROPIC}"), "")));
    // bound to the key J4n keeps (Setup's "use an API key"): that one, and still no other
    let keys = d.j4n_home().join("keys");
    fs::create_dir_all(&keys).unwrap();
    fs::set_permissions(&keys, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(keys.join("anthropic"), KEPT).unwrap();
    fs::set_permissions(keys.join("anthropic"), fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(d.j4n_home().join("settings/keys.toml"), "api = 1\n[[setting]]\nid = \"doer.keys\"\nvalue = \"claude\"\n").unwrap();
    let got = claude_turn(&d, &mine);
    assert!(got.lines().any(|l| l == format!("ANTHROPIC_API_KEY={KEPT}")), "claude bound to the kept key has it:\n{got}");
    hits.extend(leaks("claude bound to the kept key", &got.replace(&format!("ANTHROPIC_API_KEY={KEPT}"), "")));
    assert!(hits.is_empty(), "a key went where no row names it:\n{}", hits.join("\n"));
}

/// The variables a program reads by itself are its row's to name (`pass`), and those every agent keeps are the setting `keys.pass`'s (the
/// GitHub tokens by default): claude is given its long-lived sign-in and the GitHub token, and a tool and a shell line neither.
#[test]
fn claude_is_given_its_sign_in_token_and_the_github_token_of_keys_pass_and_a_tool_neither() {
    let d = Dirs::new("scrub-sites-pass");
    d.offload_off();
    let mine = [("OPENROUTER_API_KEY", ROUTER), ("CLAUDE_CODE_OAUTH_TOKEN", OAUTH), ("GH_TOKEN", GH)];
    let (shell, tool) = own_turn(&d, &mine);
    let mut hits = leaks_of("a shell line of J4n's own agent", &shell, &[ROUTER, OAUTH, GH]);
    hits.extend(leaks_of("a connected tool", &tool, &[ROUTER, OAUTH, GH]));
    let got = claude_turn(&d, &mine);
    for want in [format!("CLAUDE_CODE_OAUTH_TOKEN={OAUTH}"), format!("GH_TOKEN={GH}")] {
        assert!(got.lines().any(|l| l == want), "claude has {want}:\n{got}");
    }
    hits.extend(leaks_of("claude", &got, &[ROUTER]));
    assert!(hits.is_empty(), "a key went where no row names it:\n{}", hits.join("\n"));
}
