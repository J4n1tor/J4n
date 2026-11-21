//! The leak canary: a key given to J4n is kept in its own key file and given to the one doer bound to its provider, and is nowhere else.
//! In a fresh HOME the headless TUI (the real wire, front, Serve and TUI) is given made-up keys of no vendor's shape, so what keeps them out
//! is the registered value and not a pattern. The walk ([`STEPS`]):
//! - a key typed on a key row of Agents › Setup (OpenRouter's, on the Decider's `Span through OpenRouter`);
//! - one pasted into Agents › Providers as an environment line (TypeSafe's, the Decider's), one typed there and then typed over
//!   (Anthropic's: the old value is replaced), one typed and removed with `x` on its card (Google's);
//! - a key mistyped on a chat line: `:key <value>`, and a line that is one key-like word, held on its card and dropped.
//!
//! Then the `test` doer takes a turn on a chat line, and the Decider answers a point: `decider:jev`, made live by the pasted key. No answerer
//! of J4n's own is a test one, so it is `deciders_flow`'s gateway on the loopback, which answers its probe cases right. Then Setup's Test
//! makes its proof call: the `test` doer's turn on a thread of its own and the Decider's probe cases, with the pasted key. A program bound to a
//! provider's key reads it at its next start, so the bound doer's turn is the real binary's, over the same HOME: a fake `claude` (a script
//! in the test's folder) given Anthropic's key by `doer.keys` writes its argv and environment at every start, probes too, keeps what it is
//! sent in a transcript folder of its own under the HOME, and says its key back in its answer. A second program, bound to no provider,
//! takes a turn too and writes the same record. Then the real binary is made to panic with the keys loaded from their files: its standard
//! output is a pipe with no reader, so its first print panics.
//!
//! Each value is then looked for in the redactor's four forms (as given, as JSON writes it, percent-encoded in upper and in lower case) and
//! by a piece of its head and of its tail (a frame cuts a long line), in:
//! - every frame the TUI drew, each one looked at as it is drawn, the waits' frames too (`Operator::never`), the composer's history and its line.
//!   A `:key …` line is looked at in every frame, as it is typed too: the composer draws its words after `:key` masked. A line that is
//!   one key-like word is not looked at for that word while it is typed, up to its ⏎: it is known as a key only once it is whole;
//! - the Log as the front reads it, the state of every fold, and every file under the HOME and the project: the Log's segments and blobs,
//!   the folds' snapshots, the settings, the fake program's transcript folder (`~/.claude/projects`). The files of `keys/` are the exception;
//! - the argv of every start of both programs, and their environment: the bound one's may hold Anthropic's current key as
//!   `ANTHROPIC_API_KEY` and nothing else, the unbound one's none at all;
//! - every request each loopback gateway was sent (the Decider's and the one OpenRouter's connectors are pointed at): never in a body, and in
//!   `authorization` only the key of that gateway's provider;
//! - what the fake program was sent on its stdin: its prompts and briefs;
//! - stdout and stderr of the binary's run, and stderr of its panic.
//!
//! Each `keys/<id>` holds only its own provider's key, mode 0600 in a folder of mode 0700. The replaced and the removed values are in no
//! file at all.
//!
//! Not reached:
//! - the `test` doer keeps nothing of what it is given. Its goal is in the Log, which is scanned, and it takes no brief, so no brief of its
//!   own is looked at. The fake program's stdin is what a doer is given;
//! - the argv and environment of a child other than the fake program, such as a real `codex`, `gemini` or `pi` on the PATH, probed in
//!   process. `scrub_sites` and the kept-key argv law of `run_cases` hold those sites;
//! - a panic whose message J4n builds from a value. The forced panic is std's own, a print to a closed pipe. What it proves is that the
//!   panic hook and the backtrace of a J4n that holds the keys print none of them, not what every panic site says;
//! - a lone key-like word on a chat line is on the screen while it is typed, up to its ⏎: it is known as a key only once it is whole;
//! - Setup's other key rows (an agent's `API key`, J4n's own agent's key) take the same editor and the same road as the Decider's row
//!   the walk types on; they are not typed on one by one.

use crate::deciders_flow::{Cases, events, gateway, jev_probes, learn};
use crate::models_cases::mock::{Mock, Say, Seen};
use crate::run_cases::{Dirs, connector, j4n, program};
use crate::screen_drive::{Operator, on_chat, wait_idle};
use crate::support::{PATIENCE, read, root, walk};
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use j4n_core::answer::Question;
use j4n_core::answer::chain::Walk;
use j4n_core::ask::{AskShape, Opt};
use j4n_core::task::Mode;
use j4n_tui::headless::{KeyCode, KeyEvent, alt, press, typed};
use serde_json::json;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// A made-up value and its name in what the law says. None has a vendor's shape; each has characters that the redactor's other forms
/// change (`+ / =`, and `"` or `\` where the value never goes through a shell or a JSON template), so the four forms are four strings.
pub struct Canary {
    pub name: &'static str,
    pub value: &'static str,
}

/// Typed on Setup's row `Span through OpenRouter`: OpenRouter's key.
const TYPED: Canary = Canary { name: "the typed key", value: r#"Rv8kTq3Wm-typed+9x/Lp\Zd"4e=Hw"# };
/// Pasted into TypeSafe's row as an environment line: the Decider's.
const PASTED: Canary = Canary { name: "the pasted key", value: r#"Hn2cPasted"Vx7+Qm/Jt\8wKd=Ys"# };
/// Typed into Anthropic's row, then typed over.
const OLD: Canary = Canary { name: "the replaced key", value: r#"Wd4qOldKey+Nk8/Gh\Pz"2t=Yq"# };
/// Typed over it: the key the bound program has. It goes through the fake program's shell into a JSON line, so it has no `"` or `\`.
const NEW: Canary = Canary { name: "the key typed over it", value: "Bf6wNewKey+Rm3/Xs9kL=wTq" };
/// Typed into Google's row, then removed.
const GONE: Canary = Canary { name: "the removed key", value: r#"Kp9tGoneKy+Ta5/Vw\Ue"3m=Jr"# };
/// On a chat line after `:key`.
const LINE: Canary = Canary { name: "the key on a :key line", value: r#"Yt3sLineKy+Hb7/Qn\Rc"6p=Gv"# };
/// A chat line of its own, held on the card and dropped: a key-like word is of letters, digits and `- _ . + / =` alone.
const LONE: Canary = Canary { name: "the lone key on a line", value: "Zm5rLoneQx8Wd2+Fj/Ka=7bNc" };

/// Every value of the walk.
pub const VALUES: [&Canary; 7] = [&TYPED, &PASTED, &OLD, &NEW, &GONE, &LINE, &LONE];

/// One step of the walk, done through the screen.
enum Step {
    /// Agents › Setup, `⏎` on the first row whose line holds the words, the value typed key by key in its masked editor, `⏎`.
    Setup(&'static str, &'static Canary),
    /// Agents › Providers, `⏎` on the row, the value typed key by key, `⏎`.
    Type(&'static str, &'static Canary),
    /// Agents › Providers, `⏎` on the row, `<NAME>=<value>` pasted whole, `⏎`.
    Paste(&'static str, &'static str, &'static Canary),
    /// Agents › Providers, `x` on the row, `y` on the card.
    Remove(&'static str),
    /// A chat line, the word and then the value typed key by key, `⏎`.
    Line(&'static str, &'static Canary),
    /// A chat line of one key-like word, `⏎`, and `drop it` on the card.
    Drop(&'static Canary),
}

/// The walk, in order. The Decider's key is pasted after the doer's turn, so the line's point is not the Decider's to answer.
const STEPS: [Step; 7] = [
        Step::Setup("Span through", &TYPED),
        Step::Type("anthropic", &OLD),
        Step::Type("anthropic", &NEW),
        Step::Type("google", &GONE),
        Step::Remove("google"),
        Step::Line(":key ", &LINE),
        Step::Drop(&LONE),
];

/// The redactor's four forms of a value (`redact.rs`, `Redactor::register`): as given, as JSON writes it inside a string, and
/// percent-encoded in upper and in lower case.
pub fn forms(value: &str) -> [String; 4] {
    let json = serde_json::to_string(value).expect("a string");
    let percent = |upper: bool| {
        value
            .bytes()
            .map(|b| match b {
                b if b.is_ascii_alphanumeric() || b"-._~".contains(&b) => (b as char).to_string(),
                b if upper => format!("%{b:02X}"),
                b => format!("%{b:02x}"),
            })
            .collect::<String>()
    };
    [value.to_string(), json[1..json.len() - 1].to_string(), percent(true), percent(false)]
}

/// What is looked for, by name: the four forms, and ten characters of the head and of the tail of the value.
pub fn needles(c: &Canary) -> Vec<(String, String)> {
    let [raw, json, upper, lower] = forms(c.value);
    let chars: Vec<char> = c.value.chars().collect();
    let head: String = chars[..10].iter().collect();
    let tail: String = chars[chars.len() - 10..].iter().collect();
    [("as given", raw), ("JSON-escaped", json), ("percent-encoded, upper", upper), ("percent-encoded, lower", lower), ("its head", head), ("its tail", tail)]
        .into_iter()
        .map(|(how, n)| (format!("{} ({how})", c.name), n))
        .collect()
}

/// `<what>: <name> (<form>)` for every form of every value found in `bytes`.
pub fn found(what: &str, bytes: &[u8], values: &[&Canary]) -> Vec<String> {
    let has = |n: &[u8]| bytes.windows(n.len()).any(|w| w == n);
    values.iter().flat_map(|c| needles(c)).filter(|(_, n)| has(n.as_bytes())).map(|(name, _)| format!("{what}: {name}")).collect()
}

/// Every hit in every file under `dirs`, but for the files directly in a `skip` folder (`keys/`), which are judged by what they hold.
pub fn in_files(dirs: &[PathBuf], skip: &Path, values: &[&Canary]) -> Vec<String> {
    let mut hits = Vec::new();
    for f in dirs.iter().flat_map(|d| walk(d)) {
        if f.parent() == Some(skip) {
            continue;
        }
        hits.extend(found(&f.display().to_string(), &fs::read(&f).unwrap_or_default(), values));
    }
    hits
}

/// Every hit in what the in-process J4n holds: the composer's history and its line, the Log as the front reads it, the state of every fold,
/// and the notices of the start.
pub fn in_world(op: &Operator, world: &World, values: &[&Canary]) -> Vec<String> {
    let mut hits = Vec::new();
    for h in op.app.history.iter().chain([&op.app.line.buf]) {
        hits.extend(found("the composer's history", h.as_bytes(), values));
    }
    hits.extend(found("the Log", log_text(world).as_bytes(), values));
    let folds = world.front.tasks().folds();
    assert!(!folds.ids().is_empty(), "the folds are read");
    for id in folds.ids() {
        hits.extend(found(&format!("the fold {id}"), folds.state(&id).unwrap_or_default().to_string().as_bytes(), values));
    }
    hits.extend(world.notices.iter().flat_map(|n| found("a notice", n.as_bytes(), values)));
    hits
}

/// The fake program's record, `start <argv>` and then `env <NAME=value>` lines for each start: every hit in an argument, and in the
/// environment every hit but the `allowed` lines (the variable of the provider the program is bound to, with a key it was given).
pub fn in_children(seen: &str, allowed: &[String], values: &[&Canary]) -> Vec<String> {
    let mut hits = Vec::new();
    for (n, line) in seen.lines().enumerate() {
        if let Some(argv) = line.strip_prefix("start") {
            hits.extend(found(&format!("the argv of a child (line {})", n + 1), argv.as_bytes(), values));
        } else if let Some(var) = line.strip_prefix("env ").filter(|v| !allowed.iter().any(|a| a == v)) {
            // the variable's name only: the rest of the environment is the machine's
            let name = var.split('=').next().unwrap_or_default();
            hits.extend(found(&format!("the environment of a child ({name})"), var.as_bytes(), values));
        }
    }
    hits
}

/// Every hit in what a loopback gateway was sent: a body never, a header never but `authorization` saying `right` itself.
pub fn in_requests(what: &str, seen: &[Seen], right: &str, values: &[&Canary]) -> Vec<String> {
    let mut hits = Vec::new();
    for (n, r) in seen.iter().enumerate() {
        hits.extend(found(&format!("{what}, request {n} {}: the body", r.path), r.body.as_bytes(), values));
        for (k, v) in r.headers.iter().filter(|(k, v)| !(k.as_str() == "authorization" && *v == &format!("Bearer {right}"))) {
            hits.extend(found(&format!("{what}, request {n}: the header {k}"), v.as_bytes(), values));
        }
    }
    hits
}

/// The fake program: it writes its argv and its environment for every start (a probe's too) into `seen`, outside the HOME and the
/// project; what it is sent goes to `transcript`, its own folder under the HOME; and its answer says back the Anthropic key it was given.
fn fake_agent(d: &Dirs, seen: &Path, transcript: &Path) -> PathBuf {
    let real = program(d, "claude-real", &read(&root().join("fixtures/claude-frames/fake-claude.sh")));
    fs::create_dir_all(transcript.parent().unwrap()).unwrap();
    let two = root().join("fixtures/claude-frames/two-turns.jsonl");
    let bin = d.dir.join("bin");
    let script = format!(
        "#!/bin/sh\n{{ printf 'start'; for a in \"$@\"; do printf ' %s' \"$a\"; done; printf '\\n'; env | sort | sed 's/^/env /'; }} >> '{seen}'\n\
         said='{bin}/said-'$$'.jsonl'\n\
         sed \"s|\\\"text\\\":\\\"one\\\"|\\\"text\\\":\\\"the key is ${{ANTHROPIC_API_KEY:-none}}\\\"|; s|\\\"result\\\":\\\"one\\\"|\\\"result\\\":\\\"the key is ${{ANTHROPIC_API_KEY:-none}}\\\"|\" '{two}' > \"$said\"\n\
         FAKE_FIXTURE=\"$said\" FAKE_LOG='{transcript}' exec '{real}' \"$@\"\n",
        seen = seen.display(),
        bin = bin.display(),
        two = two.display(),
        transcript = transcript.display(),
        real = real.display(),
    );
    program(d, "claude-here", &script)
}

fn open(d: &Dirs, claude: &Path) -> World {
    let opts = Opts { doer: Some("test".into()), model: None, approval: Some(Approval::Ask), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false };
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: claude.display().to_string() };
    crate::support::opened(|| wire::open(setup.clone(), &opts, true, "main"))
}

fn count(world: &World, kind: &str) -> usize {
    events(world).iter().filter(|(_, k, _)| k == kind).count()
}

fn log_text(world: &World) -> String {
    events(world).iter().map(|(_, k, d)| format!("{k} {d}")).collect::<Vec<_>>().join("\n")
}

/// Keys one at a time, the screen drawn after each: a line's echo stands only until the core's row comes.
pub fn keys(op: &mut Operator, keys: Vec<KeyEvent>) {
    for k in keys {
        op.press(vec![k]);
        op.look();
    }
}

/// A chat line typed key by key and sent. A `:key …` line is masked as it is typed, so every frame is looked at for its value. Any other
/// line is drawn as typed until ⏎, as a terminal does: those frames are not looked at for the value being typed (for the rest they are),
/// and every frame from the ⏎ on is.
fn say(op: &mut Operator, word: &str, c: &Canary) {
    if word.trim().eq_ignore_ascii_case(":key") {
        keys(op, typed(&format!("{word}{}", c.value), false));
        keys(op, vec![press(KeyCode::Enter)]);
        return;
    }
    let all = std::mem::take(&mut op.never);
    let (typing, rest): (Vec<_>, Vec<_>) = all.into_iter().partition(|(name, _)| name.starts_with(c.name));
    op.never = rest;
    keys(op, typed(&format!("{word}{}", c.value), false));
    op.never.extend(typing);
    keys(op, vec![press(KeyCode::Enter)]);
}

/// Draw and take what the core sends until `ok`.
pub fn until(op: &mut Operator, world: &World, what: &str, ok: impl Fn() -> bool) {
    let end = Instant::now() + PATIENCE;
    while !ok() {
        op.until("a frame", |_| true);
        assert!(Instant::now() < end, "never saw {what}; the screen is\n{}\nthe Log:\n{}", op.look(), log_text(world));
        std::thread::sleep(Duration::from_millis(20));
    }
    // the keys that come within a moment of the ⏎ that kept a key are dropped as the rest of its burst: the next step waits it out, and
    // takes the rows that came meanwhile
    std::thread::sleep(Duration::from_millis(80));
    op.until("a frame", |_| true);
}

/// The cursor on the Providers row `row`, the page opened first when it is not open.
fn to_row(op: &mut Operator, row: &str) {
    if !op.look().contains("J4n keeps a key and never shows it") {
        op.go(":providers", "Agents › Providers", |s| s.contains("Providers") && s.contains("typesafe"));
    }
    let shown = op.look();
    let rows: Vec<&str> = shown.lines().filter(|l| l.starts_with(['├', '╰'])).collect();
    let at = rows.iter().position(|l| l.split_whitespace().any(|w| w == row)).unwrap_or_else(|| panic!("no Providers row {row}:\n{shown}"));
    keys(op, (0..rows.len()).map(|_| press(KeyCode::Up)).collect());
    keys(op, (0..at).map(|_| press(KeyCode::Down)).collect());
}

pub fn to_chat(op: &mut Operator) {
    if !on_chat(&op.look()) {
        keys(op, vec![alt('1')]);
        op.until("the chat", on_chat);
    }
}

fn step(op: &mut Operator, world: &World, s: &Step) {
    let sets = count(world, "key.set");
    match s {
        Step::Setup(row, c) => {
            to_chat(op);
            crate::setup_flow::setup_page(op);
            crate::setup_flow::to_row(op, row);
            keys(op, vec![press(KeyCode::Enter)]);
            op.until("the masked editor", |s| s.contains("paste or type the key"));
            keys(op, typed(c.value, false));
            keys(op, vec![press(KeyCode::Enter)]);
            until(op, world, &format!("key.set for {}", c.name), || count(world, "key.set") > sets);
            // a line is typed from the chat
            to_chat(op);
        }
        Step::Type(row, c) => {
            to_row(op, row);
            keys(op, vec![press(KeyCode::Enter)]);
            keys(op, typed(c.value, false));
            keys(op, vec![press(KeyCode::Enter)]);
            until(op, world, &format!("key.set for {}", c.name), || count(world, "key.set") > sets);
        }
        Step::Paste(row, name, c) => {
            to_row(op, row);
            keys(op, vec![press(KeyCode::Enter)]);
            op.app.paste(&format!("{name}={}", c.value));
            op.look();
            keys(op, vec![press(KeyCode::Enter)]);
            until(op, world, &format!("key.set for {}", c.name), || count(world, "key.set") > sets);
        }
        Step::Remove(row) => {
            to_row(op, row);
            let removed = count(world, "key.removed");
            // `x` is the row's once its row says J4n keeps the key, which comes a moment after `key.set`
            let end = Instant::now() + PATIENCE;
            while op.app.confirm.is_none() {
                assert!(Instant::now() < end, "never saw the card of `x` on {row}; the screen is\n{}", op.look());
                keys(op, vec![press(KeyCode::Char('x'))]);
                op.until("a frame", |_| true);
                std::thread::sleep(Duration::from_millis(20));
            }
            keys(op, vec![press(KeyCode::Char('y'))]);
            until(op, world, &format!("key.removed for {row}"), || count(world, "key.removed") > removed);
        }
        Step::Line(word, c) => {
            to_chat(op);
            let said = count(world, "chat.say");
            say(op, word, c);
            until(op, world, "the line said", || count(world, "chat.say") > said);
            wait_idle(world);
        }
        Step::Drop(c) => {
            to_chat(op);
            say(op, "", c);
            op.until("the card", |s| s.contains("looks like a key"));
            keys(op, vec![press(KeyCode::Char('3'))]);
            op.until("the card gone", |s| !s.contains("looks like a key"));
            wait_idle(world);
        }
    }
}

/// `j4n look …` with its standard output a pipe that nobody reads: its first print panics, with the keys loaded from their files.
fn panicked(d: &Dirs, env: &[(&str, &str)]) -> String {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        let (reader, writer) = std::io::pipe().expect("a pipe");
        drop(reader);
        fs::create_dir_all(d.dir.join("tmp")).unwrap();
        let out = Command::new(env!("CARGO_BIN_EXE_j4n"))
            .args(["look", "what does the readme say"])
            .current_dir(d.project())
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", d.home())
            .env("J4N_HOME", d.j4n_home())
            .env("TMPDIR", d.dir.join("tmp"))
            .env("https_proxy", "http://127.0.0.1:9")
            .envs(env.iter().copied())
            .stdin(Stdio::null())
            .stdout(writer)
            .stderr(Stdio::piped())
            .output()
            .expect("j4n runs");
        let err = String::from_utf8_lossy(&out.stderr).into_owned();
        if !(err.contains("another J4n") && Instant::now() < end) {
            return err;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn a_key_given_to_j4n_is_in_its_key_file_and_its_bound_doer_and_in_no_other_sink() {
    let d = Dirs::new("key-canary-given");
    let seen = d.dir.join("bin/agent-seen");
    let transcript = d.home().join(".claude/projects/-home-user-project/fake-session.jsonl");
    let claude = fake_agent(&d, &seen, &transcript);
    // the Decider's gateway, and one for OpenRouter's connectors (it refuses every key): no request leaves the loopback
    let cases: Cases = std::sync::Arc::default();
    let jev = gateway(&cases, PASTED.value);
    let router = Mock::start(|_, _| Say::Json(401, json!({ "error": { "message": "no such key" } })));
    let conns = d.j4n_home().join("connectors");
    fs::create_dir_all(&conns).unwrap();
    fs::write(conns.join("jev.toml"), format!("api = 1\n[[connector]]\nid = \"jev\"\nurl = \"{}\"\n", jev.url())).unwrap();
    let here = |id: &str, path: &str| format!("[[connector]]\nid = \"{id}\"\nurl = \"{}{path}\"\n", router.url());
    fs::write(conns.join("router.toml"), format!("api = 1\n{}{}{}", here("span", ""), here("light", "/api/v1"), here("own", "/api/v1"))).unwrap();
    // a program bound to no provider: what it is started with is written down outside the HOME and the project
    let plain_seen = d.dir.join("bin/plain-seen");
    program(&d, "plain", &format!("#!/bin/sh\n{{ printf 'start'; for a in \"$@\"; do printf ' %s' \"$a\"; done; printf '\\n'; env | sort | sed 's/^/env /'; }} >> '{}'\necho plain done\n", plain_seen.display()));
    connector(&d, "plain.toml", "api = 1\n[[connector]]\nid = \"plain\"\nis = \"agent\"\ndialect = \"text\"\nprogram = \"{bin}/plain\"\nprobe = { argv = [\"{bin}/plain\", \"--version\"] }\nstart = [\"{prompt}\"]\n");
    // claude is given Anthropic's key, as Setup's "use an API key" writes it
    fs::write(d.j4n_home().join("settings/keys.toml"), "api = 1\n[[setting]]\nid = \"doer.keys\"\nvalue = \"claude\"\n").unwrap();

    let world = open(&d, &claude);
    learn(&world, &cases);
    let never: Vec<(String, String)> = VALUES.iter().flat_map(|c| needles(c)).collect();
    let mut op = Operator::watching(&world, never);
    for s in STEPS.iter() {
        step(&mut op, &world, s);
    }
    // a doer's turn
    to_chat(&mut op);
    let settled = count(&world, "task.settled");
    keys(&mut op, typed("say the turn is over", true));
    until(&mut op, &world, "the turn's end", || count(&world, "task.settled") > settled);
    wait_idle(&world);
    // the Decider's key, pasted, and its probes passed
    step(&mut op, &world, &Step::Paste("typesafe", "TYPESAFE_API_KEY", &PASTED));
    until(&mut op, &world, "the Decider's probes passed", || jev_probes(&world).iter().any(|(_, p)| p["point"] == "intake" && p["passed"] == true));
    // the Decider answers a point
    let options = ["answer", "command", "setting", "task", "flow"].map(|id| Opt { id: id.into(), label: id.into(), recommended: false }).to_vec();
    let text = "what does the readme say about the build".to_string();
    let q = Question { ask: "key-canary-next".into(), point: "intake".into(), asker: "test".into(), thread: None, text: text.clone(), shape: AskShape::Choice { options }, goal: text, act: None };
    let asked = world.front.who().ask("intake", None, &q, Some("decider:jev")).expect("the point intake");
    assert!(matches!(&asked.walk, Walk::Answered(o) if o.by == "decider:jev"), "the Decider answered: {:?}", asked.walk);
    // Setup's proof call: the turn and the Decider's cases, its key on their requests
    let before = jev.seen().len();
    to_chat(&mut op);
    crate::setup_flow::setup_page(&mut op);
    crate::setup_flow::to_row(&mut op, "not run");
    keys(&mut op, vec![press(KeyCode::Enter)]);
    until(&mut op, &world, "Setup's test", || count(&world, "setup.tested") > 0);
    let proof = events(&world).into_iter().rev().find(|(_, k, _)| k == "setup.tested").map(|(_, _, d)| d).unwrap();
    assert_eq!(proof["passed"], true, "{proof:#}");
    let parts: Vec<(&str, &str, bool)> = proof["parts"].as_array().unwrap().iter().map(|p| (p["part"].as_str().unwrap_or(""), p["id"].as_str().unwrap_or(""), p["ok"] == true)).collect();
    assert_eq!(parts, [("agent", "test", true), ("decider", "decider:jev", true)], "the doer's turn and the Decider's cases, both ok");
    assert!(jev.seen().len() > before, "the test asked the Decider");
    op.until("the test's rows", |s| s.contains("passed ·"));

    // what the in-process J4n holds: the screen, the history, the Log as read, the folds
    let mut hits = std::mem::take(&mut op.shown);
    assert!(op.drawn > 50, "the walk drew its frames: {}", op.drawn);
    let log = log_text(&world);
    for (what, kind) in [("the key typed", "key.set"), ("the key removed", "key.removed"), ("the :key line, as `:key …`", "chat.say")] {
        assert!(log.contains(kind), "{what}: no {kind}\n{log}");
    }
    assert!(log.contains(r#""text":":key …""#), "the :key line reached the core, kept as `:key …`:\n{log}");
    hits.extend(in_world(&op, &world, &VALUES));
    drop(op);
    world.close();
    drop(world);

    // the real binary over the same HOME: the bound program's turn, which says its key back, and a panic
    let env = [("J4N_CLAUDE", claude.to_str().unwrap()), ("RUST_BACKTRACE", "1")];
    let end = Instant::now() + Duration::from_secs(10);
    let run = loop {
        let o = j4n(&d, &["--doer", "claude", "--model", "haiku", "--approval", "full"], Some("say the key back\n"), &env);
        if !(o.err.contains("another J4n") && Instant::now() < end) {
            break o;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(run.rc, 0, "{}\n{}", run.out, run.err);
    assert!(run.out.contains("the key is «secret:"), "the program said its key and J4n shows the marker:\n{}", run.out);
    let plain = j4n(&d, &["run", "say hello", "--doer", "plain"], None, &env);
    assert_eq!((plain.rc, plain.out.as_str()), (0, "plain done\n"), "{}", plain.err);
    for (what, o) in [("j4n", &run), ("j4n with the unbound program", &plain)] {
        hits.extend(found(&format!("stdout of {what}"), o.out.as_bytes(), &VALUES));
        hits.extend(found(&format!("stderr of {what}"), o.err.as_bytes(), &VALUES));
    }
    let panic = panicked(&d, &env);
    assert!(panic.contains("panicked"), "the binary panicked:\n{panic}");
    hits.extend(found("stderr of the panic", panic.as_bytes(), &VALUES));

    // the children: argv never, the environment only the bound program's own key
    let children = read(&seen);
    // the bound program is first started after the key was typed over: the replaced key is nowhere, its environment too
    let bound = vec![format!("ANTHROPIC_API_KEY={}", NEW.value)];
    assert!(children.lines().any(|l| l == format!("env {}", bound[0])), "the bound program was given its key: the record is read");
    assert!(children.lines().filter(|l| l.starts_with("start")).count() >= 2, "a probe and a turn");
    hits.extend(in_children(&children, &bound, &VALUES));
    let unbound = read(&plain_seen);
    assert!(unbound.lines().any(|l| l.starts_with("start say hello")) && unbound.lines().any(|l| l.starts_with("env PATH=")), "the unbound program's turn is recorded: {}", unbound.lines().filter(|l| l.starts_with("start")).collect::<Vec<_>>().join(" | "));
    hits.extend(in_children(&unbound, &[], &VALUES).into_iter().map(|h| format!("the unbound program: {h}")));
    assert!(read(&transcript).contains("say the key back"), "the transcript folder holds what the program was sent");
    hits.extend(in_requests("the Decider's gateway", &jev.seen(), PASTED.value, &VALUES));
    assert!(jev.seen().iter().any(|r| r.headers.get("authorization") == Some(&format!("Bearer {}", PASTED.value))), "the Decider was given its key");
    // the model and the Decider OpenRouter's key made live put their probe cases there at once
    assert!(router.seen().iter().any(|r| r.headers.get("authorization") == Some(&format!("Bearer {}", TYPED.value))), "OpenRouter's connectors were given its key");
    hits.extend(in_requests("OpenRouter's gateway", &router.seen(), TYPED.value, &VALUES));

    // every file under the HOME and the project, the key files apart, and those hold their own provider's key alone
    let kept = d.j4n_home().join("keys");
    hits.extend(in_files(&[d.home(), d.project()], &kept, &VALUES));
    assert_eq!(fs::metadata(&kept).unwrap().permissions().mode() & 0o777, 0o700, "the keys' folder");
    let mut files: Vec<(String, String)> = Vec::new();
    for f in walk(&kept) {
        assert_eq!(fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o600, "{}", f.display());
        files.push((f.file_name().unwrap().to_string_lossy().into_owned(), fs::read_to_string(&f).unwrap()));
    }
    let of = |c: &Canary| files.iter().filter(|(_, v)| v == c.value).map(|(n, _)| n.as_str()).collect::<Vec<_>>();
    assert_eq!(of(&NEW), ["anthropic"], "Anthropic's key, typed over");
    assert!(!of(&TYPED).is_empty() && of(&TYPED).iter().all(|n| ["openrouter", "own", "light", "span"].contains(n)), "OpenRouter's: {:?}", of(&TYPED));
    assert!(!of(&PASTED).is_empty() && of(&PASTED).iter().all(|n| ["typesafe", "jev"].contains(n)), "TypeSafe's: {:?}", of(&PASTED));
    for (name, value) in &files {
        assert!([&TYPED, &PASTED, &NEW].iter().any(|c| c.value == value), "keys/{name} holds one key it was given and no other: {:?}", found("", value.as_bytes(), &VALUES));
    }
    assert!(!kept.join("google").exists(), "the removed key's file is gone");
    assert!(hits.is_empty(), "a key is where it never goes:\n{}", hits.join("\n"));
}

// ---- the detector, on bad input ----

const PLANT: Canary = Canary { name: "a planted key", value: r#"Qa1zPlant3d+Kx/Wv\Ro"9s=Lm"# };

#[test]
fn the_four_forms_are_the_redactors_and_four_strings() {
    let red = j4n_core::redact::Redactor::new();
    for c in VALUES.iter().chain([&&PLANT]) {
        assert!(red.register("CANARY", c.value), "{}", c.name);
        let all = forms(c.value);
        for (i, f) in all.iter().enumerate() {
            let scrubbed = red.scrub(&format!("before {f} after"));
            assert!(!scrubbed.contains(f.as_str()) && scrubbed.contains("«secret:CANARY»"), "{} form {i} is one the redactor knows: {scrubbed}", c.name);
        }
        // a value the JSON template carries has no `"` or `\`, so its JSON form is itself; the rest have four strings
        let distinct: std::collections::BTreeSet<&String> = all.iter().collect();
        assert_eq!(distinct.len(), if c.value.contains(['"', '\\']) { 4 } else { 3 }, "{}", c.name);
    }
}

#[test]
fn each_form_and_piece_of_a_value_is_found_and_named() {
    for (name, needle) in needles(&PLANT) {
        let text = format!("a Log line {{\"x\":\"{needle}\"}} and more");
        assert_eq!(found("the Log", text.as_bytes(), &[&PLANT]).first().map(String::as_str), Some(format!("the Log: {name}").as_str()), "{name}");
    }
    assert!(found("the Log", b"a line with nothing in it", &[&PLANT]).is_empty());
}

#[test]
fn a_planted_value_is_found_in_a_file_an_argument_an_environment_and_a_request_and_only_the_key_files_and_the_bound_variable_are_let_be() {
    let d = Dirs::new("key-canary-plant");
    let kept = d.j4n_home().join("keys");
    fs::create_dir_all(&kept).unwrap();
    fs::write(kept.join("acme"), PLANT.value).unwrap();
    assert!(in_files(&[d.home(), d.project()], &kept, &[&PLANT]).is_empty(), "a key file is judged by what it holds");
    let [_, json, _, lower] = forms(PLANT.value);
    d.put(".j4n/log/blobs/ab/cdef", &format!("{{\"text\":\"{json}\"}}"));
    fs::write(kept.parent().unwrap().join("settings/x.toml"), format!("url = \"https://example.com/?k={lower}\"")).unwrap();
    let hits = in_files(&[d.home(), d.project()], &kept, &[&PLANT]);
    assert!(hits.iter().any(|h| h.contains("blobs/ab/cdef") && h.contains("JSON-escaped")), "{hits:?}");
    assert!(hits.iter().any(|h| h.contains("settings/x.toml") && h.contains("percent-encoded, lower")), "{hits:?}");
    let bound = format!("ACME_API_KEY={}", PLANT.value);
    let seen = format!("start --version\nenv HOME=/home/user\nenv {bound}\nstart -p {}\nenv OTHER={}\n", PLANT.value, PLANT.value);
    let hits = in_children(&seen, std::slice::from_ref(&bound), &[&PLANT]);
    assert!(hits.iter().any(|h| h.contains("argv of a child (line 4)")), "{hits:?}");
    assert!(hits.iter().any(|h| h.contains("environment of a child (OTHER)")) && !hits.iter().any(|h| h.contains("ACME_API_KEY")), "{hits:?}");
    assert!(!hits.iter().any(|h| h.contains(PLANT.value)), "a hit names the place, never the value: {hits:?}");
    let req = |auth: &str, body: String| Seen { method: "POST".into(), path: "/v1".into(), headers: [("authorization".to_string(), format!("Bearer {auth}"))].into(), body };
    let hits = in_requests("a gateway", &[req(PLANT.value, "{}".into()), req("another", format!("{{\"k\":\"{json}\"}}")), req(&format!("x{}", PLANT.value), "{}".into())], PLANT.value, &[&PLANT]);
    assert!(hits.iter().any(|h| h.contains("request 1 /v1: the body")), "{hits:?}");
    assert!(hits.iter().any(|h| h.contains("request 2: the header authorization")), "another key's header is not its own: {hits:?}");
    assert!(!hits.iter().any(|h| h.contains("request 0")), "its own provider's key in its own header is let be: {hits:?}");
}
