//! `j4n doctor`, the Setup page and the first-start notice. Tier 0: the doers are scripts in a folder of the
//! test's own (found, signed in or not, or missing), no network, no sign-in; the binary is run with a PATH that holds only that folder.

use crate::screen_drive::Operator;
use crate::run_cases::{Dirs, Out, j4n, log_events};
use j4n::cli::{Approval, Cmd, Opts, parse};
use j4n::wire::{self, Setup};
use j4n_core::task::Mode;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

fn script(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// A claude that answers `--version` and `auth status` like a signed-in one.
fn claude(d: &Dirs) -> String {
    let p = d.dir.join("bin/claude");
    script(&p, "case \"$1\" in --version) echo '2.1.0 (Claude Code)';; auth) echo '{\"loggedIn\":true,\"authMethod\":\"claude.ai\"}';; esac");
    p.display().to_string()
}

/// An agent program that answers any question with a version, and the login folder it keeps (`None`: it is there, and not signed in: its
/// `login status` fails and nothing is kept).
fn agent(d: &Dirs, name: &str, login: Option<&str>) {
    let out = if login.is_some() { "" } else { "[ \"$1 $2\" = \"login status\" ] && exit 1; " };
    script(&d.dir.join("bin").join(name), &format!("{out}echo '{name} 1.0.0'"));
    if let Some(l) = login {
        fs::create_dir_all(d.home().join(l)).unwrap();
    }
}

fn doctor(d: &Dirs, env: &[(&str, &str)]) -> Out {
    let bin = d.dir.join("bin").display().to_string();
    let mut all = vec![("PATH", bin.as_str())];
    all.extend_from_slice(env);
    j4n(d, &["doctor"], None, &all)
}

/// The line of the report whose id (the second column, after the kind) is `id`.
fn line<'a>(out: &'a str, id: &str) -> &'a str {
    out.lines().find(|l| l.get(8..).is_some_and(|rest| rest.starts_with(&format!("{id} ")))).unwrap_or_else(|| panic!("no line for {id}: {out}"))
}

#[test]
fn doctor_says_each_doer_found_signed_in_or_missing_with_the_line_that_mends_it() {
    let d = Dirs::new("doctor-lines");
    let program = claude(&d);
    agent(&d, "codex", Some(".codex"));
    agent(&d, "gemini", None); // there, not signed in (no login kept)
    agent(&d, "opencode", Some(".config/opencode"));
    // pi is not there at all
    let o = doctor(&d, &[("J4N_CLAUDE", program.as_str())]);
    assert_eq!(o.rc, 0, "the default doer works: {}{}", o.out, o.err);
    let [claude, codex, gemini, opencode, pi, own, light] = ["claude", "codex", "gemini", "opencode", "pi", "own", "light"].map(|id| line(&o.out, id));
    assert!(claude.contains("found · signed in") && claude.contains("2.1.0"), "{claude}");
    assert!(codex.contains("found · signed in") && opencode.contains("found · signed in"), "{codex} / {opencode}");
    assert!(gemini.contains("found · not signed in — ") && gemini.contains("gemini"), "a mend for a doer that is not signed in: {gemini}");
    assert!(pi.contains("not found — `pi` is not installed · Agents › Setup › Agent › pi: r looks again"), "{pi}");
    // the key is typed on its row: the OpenRouter key on Setup's Decider row, which also gives the small model and J4n's own agent
    let span = "Agents › Setup › Decider › Span through OpenRouter: ⏎ types the key";
    assert!(own.contains(&format!("no key — {span}")) && light.contains(&format!("no key — {span}")), "{own} / {light}");
    assert!(line(&o.out, "openrouter key").contains(&format!("not set — {span}")), "{}", o.out);
    assert!(line(&o.out, "default doer").contains("claude"), "{}", o.out);
    // a doer that is fine has no mend after it
    assert!(!claude.contains(" — ") && !codex.contains(" — "), "{claude} / {codex}");
}

#[test]
fn doctor_names_the_plans_with_their_prices_the_safety_defaults_and_never_prints_the_key() {
    let d = Dirs::new("doctor-plans");
    let program = claude(&d);
    let o = doctor(&d, &[("J4N_CLAUDE", program.as_str()), ("OPENROUTER_API_KEY", "sk-or-very-secret-1234567890")]);
    assert_eq!(o.rc, 0, "{}{}", o.out, o.err);
    assert!(line(&o.out, "plan max5").contains("$100 a month") && line(&o.out, "plan pro").contains("$20 a month"), "{}", o.out);
    assert!(line(&o.out, "plan claude").contains("no price — Agents › Plans: ⏎ on claude sets its price"), "the connector's own plan is the operator's to price: {}", o.out);
    for words in ["a write outside the project asks first", "J4n's own folder and git's internals are blocked", "a checkpoint is made before work", "claude's writes and commands come to the Gate first"] {
        assert!(o.out.contains(words), "{words}: {}", o.out);
    }
    // on Linux the binary is not dumpable, and the row says how a debugger opens it
    #[cfg(target_os = "linux")]
    assert!(line(&o.out, "memory").contains("memory dumps and other programs' reads of J4n are blocked (J4N_DUMPABLE=1 allows them)"), "{}", o.out);
    assert!(line(&o.out, "openrouter key").contains("set") && line(&o.out, "own").contains("key set") && line(&o.out, "light").contains("key set"), "{}", o.out);
    assert!(!o.out.contains("sk-or-very-secret") && !o.err.contains("sk-or-very-secret"), "the key is never printed");
}

#[test]
fn doctor_exits_non_zero_when_no_doer_works_and_writes_nothing() {
    let d = Dirs::new("doctor-exit");
    agent(&d, "codex", None); // there, not signed in: it does not count
    let o = doctor(&d, &[]);
    assert_eq!(o.rc, 1, "{}{}", o.out, o.err);
    assert!(line(&o.out, "default doer").contains("none works — Agents › Setup › Agent: ⏎ sets one up"), "{}", o.out);
    assert!(line(&o.out, "claude").contains("not found — `claude` is not installed"), "{}", o.out);
    assert!(fs::read_dir(d.project()).unwrap().next().is_none(), "no .j4n, nothing in the folder");
    // signed in, the same folder is fine
    let program = claude(&d);
    assert_eq!(doctor(&d, &[("J4N_CLAUDE", program.as_str())]).rc, 0);
}

#[test]
fn a_small_chat_model_with_a_key_is_not_a_default_doer_for_work() {
    let d = Dirs::new("doctor-small-chat-model");
    let none = d.dir.join("bin");
    let path = none.to_str().unwrap();
    let key = [("PATH", path), ("OPENROUTER_API_KEY", "sk-or-test-key-000000")];
    // with the gateway key, `own` (an agent with tools) and `light` (a chat model) are both ready: the first election is `own`, not
    // `light`, though `light` has the smallest id
    let o = j4n(&d, &["doctor"], None, &key);
    assert_eq!(o.rc, 0, "{}{}", o.out, o.err);
    assert!(line(&o.out, "default doer").contains("own") && !line(&o.out, "default doer").contains("light"), "{}", o.out);
    // `own` has no key of its own here: only `light` is ready, and it takes no work
    fs::create_dir_all(d.j4n_home().join("connectors")).unwrap();
    fs::write(d.j4n_home().join("connectors/own.toml"), "api = 1\n[[connector]]\nid = \"own\"\nkey = \"env:J4N_NO_SUCH_KEY\"\n").unwrap();
    let o = j4n(&d, &["doctor"], None, &key);
    assert_eq!(o.rc, 1, "light is not a doer for work: {}{}", o.out, o.err);
    assert!(line(&o.out, "light").contains("key set") && line(&o.out, "own").contains("no key"), "{}", o.out);
    let default = line(&o.out, "default doer");
    assert!(default.contains("none works") && !default.contains("doer light"), "{default}");
    assert!(default.contains("light only answers questions and takes no work: sign in to one of claude, codex, gemini"), "it says why and what to sign in to: {default}");
    // an election for work says the same, and starts nothing
    let run = j4n(&d, &["run", "say ok", "--approval", "deny", "--wall", "20s"], None, &key);
    assert_ne!(run.rc, 0, "{}{}", run.out, run.err);
    assert!(run.err.contains("good for `work`") && !run.out.contains("ok"), "{}{}", run.out, run.err);
    assert!(log_events(&d).iter().all(|e| e["data"]["spec"]["doer"] != "light"), "no task was started on it");
    // a signed-in claude beside it is the default
    let program = claude(&d);
    let both = j4n(&d, &["doctor"], None, &[("PATH", path), ("J4N_CLAUDE", program.as_str()), ("OPENROUTER_API_KEY", "sk-or-test-key-000000")]);
    assert_eq!(both.rc, 0, "{}{}", both.out, both.err);
    assert!(line(&both.out, "default doer").contains("claude"), "{}", both.out);
}

#[test]
fn doctor_takes_no_words() {
    let words = |s: &str| s.split_whitespace().map(String::from).collect::<Vec<_>>();
    assert_eq!(parse(&words("doctor")), Ok(Cmd::Doctor));
    assert!(matches!(parse(&words("doctor --help")), Ok(Cmd::Help(h)) if h.contains("j4n doctor")));
    assert!(parse(&words("doctor now")).is_err());
}

/// A world whose doers are all there and signed in, but pi when `pi_there` is not: the rows of the operator's own folder say where each is.
pub(crate) fn world(d: &Dirs, pi_there: bool) -> wire::World {
    let key = d.dir.join("key");
    crate::support::key_file(&key, "not-a-real-key\n");
    let mut rows = String::from("api = 1\n");
    for (id, is, dialect) in [("codex", "agent", "jsonl-map"), ("gemini", "agent", "acp"), ("opencode", "agent", "jsonl-map"), ("pi", "agent", "pi-rpc")] {
        let at = if id == "pi" && !pi_there { "/nonexistent/pi".to_string() } else { d.dir.join("bin").join(id).display().to_string() };
        agent(d, id, Some(".kept"));
        rows.push_str(&format!("[[connector]]\nid = \"{id}\"\nis = \"{is}\"\ndialect = \"{dialect}\"\nprogram = \"{at}\"\nprobe = {{ argv = [\"{at}\", \"--version\"] }}\nlogin = []\n"));
    }
    for id in ["own", "light"] {
        rows.push_str(&format!("[[connector]]\nid = \"{id}\"\nis = \"{}\"\ndialect = \"openai\"\nkey = \"file:{}\"\n", if id == "own" { "agent" } else { "model" }, key.display()));
    }
    fs::create_dir_all(d.j4n_home().join("connectors")).unwrap();
    fs::write(d.j4n_home().join("connectors/doctor-rows.toml"), rows).unwrap();
    let opts = Opts { doer: Some("test".into()), model: None, approval: Some(Approval::Ask), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false };
    // a world that was closed a moment ago may still hold the Log: it is asked again for a few seconds
    crate::support::opened(|| wire::open(Setup { root: d.project(), home: d.j4n_home(), claude: claude(d) }, &opts, true, "main"))
}

#[test]
fn the_first_start_in_a_new_folder_adds_what_is_missing_to_its_one_row() {
    let d = Dirs::new("doctor-first");
    let fine = world(&d, true);
    assert_eq!(j4n::first_start_note(&fine.front, true, "here").as_deref(), Some("here"), "all is fine: the place alone");
    fine.close();
    let d = Dirs::new("doctor-first-missing");
    let w = world(&d, false);
    let note = j4n::first_start_note(&w.front, true, "here").expect("pi is not there");
    assert_eq!(note, "here · pi not found · ⏎ opens Agents › Setup");
    assert_eq!(j4n::first_start_note(&w.front, false, "here"), None, "a folder J4n has worked in before is not checked");
    w.front.note("main", &note, "warn");
    let mut op = Operator::sit(&w);
    op.until("the notice in the chat", |s| s.contains("pi not found · ⏎ opens Agents › Setup"));
    drop(op);
    w.close();
}

#[test]
fn setup_shows_the_same_facts_as_doctor_and_the_plan_rows_page_shows_the_prices() {
    let d = Dirs::new("doctor-page");
    let w = world(&d, false);
    // the rows of `view:setup`, one line each: no page shows them now, the view still says them
    let s: String = {
        use j4n_core::front::serve::Serve;
        use j4n_proto::{Down, Up};
        // the programs have answered, as they have by the time an operator reads the rows
        if let Some(c) = w.front.cfg().setup.as_ref() {
            c.report_now(None);
        }
        let (downs, _) = Serve::new(w.front.clone()).up(Up::Watch { bind: "view:setup".into() });
        let rows: Vec<_> = downs.iter().filter_map(|d| if let Down::Rows { rows, .. } = d { Some(rows.clone()) } else { None }).flatten().collect();
        rows.iter().map(|r| r.cells.iter().map(|c| c.text.as_str()).collect::<Vec<_>>().join(" ")).collect::<Vec<_>>().join("\n")
    };
    let has = |what: &[&str]| s.lines().any(|l| what.iter().all(|w| l.contains(w)));
    let mut op = Operator::sit(&w);
    assert!(has(&["claude", "found · signed in"]) && has(&["pi", "not found"]) && has(&["own", "key set"]), "{s}");
    assert!(has(&["plan max5", "$100 a month"]) && has(&["plan pro", "$20 a month"]) && has(&["plan claude", "no price"]), "{s}");
    assert!(has(&["openrouter key", "set"]) && has(&["writes", "ask"]) && has(&["default doer", "claude"]), "{s}");
    op.type_line(":plans");
    let s = op.until("the plan rows", |s| s.contains("max5") && s.contains("$100"));
    assert!(s.contains("pro") && s.contains("$20"), "{s}");
    drop(op);
    w.close();
}

/// The rows a view sends, as the core holds them once the programs' probes have answered.
fn view_rows(w: &wire::World, view: &str) -> Vec<j4n_proto::Row> {
    use j4n_core::front::serve::Serve;
    use j4n_proto::{Down, Up};
    if let Some(c) = w.front.cfg().setup.as_ref() {
        c.report_now(w.front.main_pick());
    }
    let mut serve = Serve::new(w.front.clone());
    serve.up(Up::Hello { api: j4n_proto::API });
    let (downs, _) = serve.up(Up::Watch { bind: format!("view:{view}") });
    downs.into_iter().filter_map(|d| if let Down::Rows { rows, .. } = d { Some(rows) } else { None }).flatten().collect()
}

/// Every mend line of the doctor's (a doer, a key, the default doer) names a row of Agents › Setup and the key that mends it there, and
/// that row is on the page with that key; the line's own `⏎` opens the page. Two homes bring out the mends: J4n basic, with the packs of the
/// other agents, J4n's own agent and the Decider off and an OpenRouter key kept; and one with those packs on, a `claude` that is not signed
/// in, the other programs missing and no key at all.
#[test]
fn every_mend_line_of_the_doctor_opens_a_row_of_setup_with_the_key_that_mends_it() {
    let program = |d: &Dirs, signed: bool| {
        let p = d.dir.join("bin/claude");
        script(&p, &format!("case \"$1\" in --version) echo '2.1.0 (Claude Code)';; auth) echo '{{\"loggedIn\":{signed},\"authMethod\":\"claude.ai\"}}';; esac"));
        p.display().to_string()
    };
    let open = |d: &Dirs, claude: String| {
        let opts = Opts { doer: None, model: None, approval: Some(Approval::Ask), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false };
        crate::support::opened(|| wire::open(Setup { root: d.project(), home: d.j4n_home(), claude: claude.clone() }, &opts, true, "main"))
    };
    let basic = Dirs::bare("doctor-mend-basic");
    let keys = basic.j4n_home().join("keys");
    fs::create_dir_all(&keys).unwrap();
    fs::set_permissions(&keys, fs::Permissions::from_mode(0o700)).unwrap();
    crate::support::key_file(&keys.join("openrouter"), "mend-made-up-openrouter-4e5f6a7b\n");
    let full = Dirs::new("doctor-mend-full");
    let mut seen = Vec::new();
    let mut wrong = Vec::new();
    for (d, signed) in [(&basic, true), (&full, false)] {
        let w = open(d, program(d, signed));
        let page = view_rows(&w, "setup-flow");
        for r in view_rows(&w, "setup") {
            // the lines of a doer, a key and the default doer: the plans, the needs and the packs are mended elsewhere
            let fix = r.cells.get(2).map(|c| c.text.clone()).unwrap_or_default();
            let mends = j4n_core::doctor::DOERS.contains(&r.key.as_str()) || ["openrouter key", "jev key", "default doer"].contains(&r.key.as_str());
            // a row with nothing to mend draws its empty cell as a dash
            if fix.is_empty() || fix == "—" || !mends {
                continue;
            }
            seen.push(fix.clone());
            // `… · Agents › Setup › <step> › <row>: <key> …`
            let Some(at) = fix.split(" · ").find_map(|p| p.strip_prefix("Agents › Setup › ")) else {
                wrong.push(format!("{}: names no row of Setup: {fix}", r.key));
                continue;
            };
            let (path, key) = at.split_once(": ").unwrap_or((at, ""));
            let row = path.rsplit(" › ").next().unwrap_or(path);
            let key = match key.split_whitespace().next() {
                Some("⏎") => "enter",
                Some(k) => k,
                None => "",
            };
            match page.iter().find(|p| p.key == row) {
                None => wrong.push(format!("{}: Setup has no row `{row}`: {fix}", r.key)),
                // ⏎ on a step opens it to its rows; on a row it is the row's own act
                Some(p) if p.key != path || key != "enter" => {
                    if !p.actions.iter().any(|a| a.key == key) {
                        wrong.push(format!("{}: the row `{row}` has no `{key}`: {fix} (its keys: {:?})", r.key, p.actions.iter().map(|a| &a.key).collect::<Vec<_>>()));
                    }
                }
                Some(_) => {}
            }
            if !r.actions.iter().any(|a| a.key == "enter" && format!("{:?}", a.intent).contains("setup")) {
                wrong.push(format!("{}: its ⏎ does not open Setup: {:?}", r.key, r.actions));
            }
        }
        w.close();
    }
    // the mends the two homes were made to bring out
    for want in ["r looks again", "s signs it in", "⏎ turns it on", "o turns it on", "⏎ types the key", "⏎ sets one up"] {
        assert!(seen.iter().any(|f| f.contains(want)), "no mend line says `{want}`: {seen:#?}");
    }
    assert!(wrong.is_empty(), "mend lines that do not lead to a row of Agents › Setup:\n{}", wrong.join("\n"));
}

/// The rows of `bind` in a `Down`, one line each, when it holds them.
fn rows_of(down: &j4n_proto::Down, bind: &str) -> Option<Vec<String>> {
    match down {
        j4n_proto::Down::Rows { bind: b, rows, .. } if b == bind => Some(rows.iter().map(|r| r.cells.iter().map(|c| c.text.as_str()).collect::<Vec<_>>().join(" ")).collect()),
        _ => None,
    }
}

#[test]
fn a_program_that_hangs_in_its_probe_holds_no_page_its_row_says_checking_and_the_others_fill() {
    use j4n_proto::Up;
    use std::time::{Duration, Instant};
    let d = Dirs::new("doctor-hang");
    let w = world(&d, true);
    // opencode here is a launcher that installs on its first call: it does not answer within its probe's time
    script(&d.dir.join("bin/opencode"), "exec sleep 30");
    let (up, down) = w.front.connect();
    up.send(Up::Hello { api: j4n_proto::API }).unwrap();
    up.send(Up::Watch { bind: "view:setup".into() }).unwrap();
    // a page opened after Setup is drawn while the probe hangs, and Setup's rows fill as the others answer
    up.send(Up::Watch { bind: "view:providers".into() }).unwrap();
    let began = Instant::now();
    let (mut providers, mut setup) = (None, Vec::new());
    let filled = |rows: &[String]| rows.iter().any(|r| r.starts_with("claude found · signed in")) && rows.iter().any(|r| r.starts_with("pi found · signed in"));
    while !(providers.is_some() && filled(&setup)) {
        let left = Duration::from_secs(10).saturating_sub(began.elapsed());
        let Ok(d) = down.recv_timeout(left) else { break };
        providers = providers.or(rows_of(&d, "view:providers"));
        setup = rows_of(&d, "view:setup").unwrap_or(setup);
    }
    assert!(providers.is_some(), "Agents › Providers was never drawn while opencode hangs");
    assert!(filled(&setup), "the doers that answer at once never filled Setup: {setup:#?}");
    assert!(setup.iter().any(|r| r.starts_with("opencode checking…")), "the program that hangs is neither found nor missing yet: {setup:#?}");
    let _ = up.send(Up::Quit);
    w.close();
}
