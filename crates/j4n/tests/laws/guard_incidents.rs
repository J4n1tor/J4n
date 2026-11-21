//! The five known attacks of `docs/GATE.md` as rows of the kind `incident`, brought by the pack `guard`. A match is a
//! `guard.alert` event in the Log and a row on the screen, `⚠ like s1ngularity · blocked`; with the pack off none of it is there.

use crate::run_cases::Dirs;
use j4n::cli::{Approval, Opts};
use j4n::pack;
use j4n::wire::{self, Setup, World};
use j4n_core::change::Actor;
use j4n_core::fold::Folds;
use j4n_core::front::rows;
use j4n_core::log::{Ev, New};
use j4n_core::pack::EnableRow;
use j4n_core::rule::{Do, RuleEnv, Rules, State};
use j4n_core::table::Tables;
use j4n_core::task::Mode;
use j4n_core::value::Value;
use j4n_proto::{Down, Field, FieldKind, Json, Node, Present, Row, Tone};
use j4n_tui::App;
use j4n_tui::headless::Headless;
use j4n_tui::theme::{Settings, Theme};
use serde_json::json;
use std::time::{Duration, Instant};

fn open(d: &Dirs) -> World {
    let opts = Opts { doer: Some("test".into()), model: None, approval: Some(Approval::Deny), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false };
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: "claude".into() };
    crate::support::opened(|| wire::open(setup.clone(), &opts, true, "main"))
}

fn put(world: &World, kind: &str, data: serde_json::Value) {
    world.front.log().append(New::of(kind, Actor::Core, data)).expect("written");
}

fn verdict(world: &World, why: &str, target: &str) {
    put(world, "gate.verdict", json!({ "verb": "shell", "target": target, "verdict": "block", "why": why }));
}

/// The incident alerts in the Log, after waiting for `n` of them (or a second, for `0`).
fn alerts(world: &World, n: usize) -> Vec<Ev> {
    let end = Instant::now() + Duration::from_secs(if n == 0 { 1 } else { 10 });
    loop {
        world.front.log().sync();
        let said: Vec<Ev> = world.front.log().read(0).filter(|e| e.kind == "guard.alert" && e.data["kind"] == "incident").collect();
        if (n > 0 && said.len() >= n) || Instant::now() > end {
            return said;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn tables(on: bool) -> Tables {
    let rows = [EnableRow { id: "guard".into(), doc: String::new(), on, hash: None, ready: None, wrote: Vec::new() }];
    let harness = pack::catalog().harness(&pack::ON, &rows);
    let (tables, problems) = Tables::load(harness.layers);
    assert!(problems.iter().all(|p| !p.to_string().contains("incident")), "{problems:?}");
    tables
}

const FIVE: [&str; 5] = ["s1ngularity", "github-mcp", "postmark-mcp", "supabase-mcp", "amazon-q"];

#[test]
fn the_five_incidents_come_with_the_pack_guard_and_go_with_it_and_each_points_at_its_write_up() {
    let on = tables(true);
    let (rules, problems) = Rules::load(&on);
    assert!(problems.is_empty(), "{problems:?}");
    let ids: Vec<&str> = rules.ids().into_iter().filter(|i| i.starts_with("incident:")).collect();
    assert_eq!(ids.len(), 5, "{ids:?}");
    for id in FIVE {
        assert!(ids.contains(&format!("incident:{id}").as_str()), "{id}: {ids:?}");
    }
    let off = tables(false);
    assert!(Rules::load(&off).0.ids().iter().all(|i| !i.starts_with("incident:")), "the pack is the switch");
    assert!(off.ids("incident").is_empty() && on.ids("incident").len() == 5);
    // the links are the ones docs/GATE.md lists in its table of known attacks
    let plan = include_str!("../../../../docs/GATE.md");
    for id in FIVE {
        let row: j4n_core::rule::IncidentRow = on.get(id).unwrap();
        assert!(plan.contains(&row.source), "{id}: {} is not a link of docs/GATE.md", row.source);
        assert!(row.doc.as_deref().is_some_and(|d| d.len() > 60), "{id} says what happened");
    }
}

struct Seen {
    folds: Folds,
    set: Value,
    rules: Rules,
    _dir: Dirs,
}

impl Seen {
    fn new() -> Seen {
        let dir = Dirs::new("guard-incidents-seen");
        let folds = Folds::new(&dir.project().join("folds"));
        Seen { folds, set: Value::Map(Default::default()), rules: Rules::load(&tables(true)).0, _dir: dir }
    }

    /// Which incidents an event raises.
    fn looks_like(&self, kind: &str, data: serde_json::Value) -> Vec<String> {
        let ev = Ev { seq: 1, at_ms: 1000, kind: kind.into(), task: Some("t1".into()), thread: None, by: Actor::Core, cause: None, class: Default::default(), data };
        let env = RuleEnv::new(&ev, &self.folds, &self.set, ev.at_ms);
        let mut found: Vec<String> = self.rules.react(&mut State::default(), &ev, &env, ev.at_ms).into_iter().filter(|a| matches!(a.what, Do::Alert { .. })).map(|a| a.rule.trim_start_matches("incident:").to_string()).collect();
        found.sort();
        found
    }

    fn block(&self, why: &str, target: &str) -> Vec<String> {
        self.looks_like("gate.verdict", json!({ "verb": "x", "target": target, "verdict": "block", "why": format!("{why}: because") }))
    }
}

#[test]
fn each_incident_is_raised_by_the_block_it_looks_like_and_by_nothing_else() {
    let s = Seen::new();
    // s1ngularity: keys, logins and startup files
    for (why, target) in [
        ("taint-write-secret", ".env"),
        ("taint-write-login", "/home/a/.bashrc"),
        ("taint-shell-write-login", "echo 'shutdown -h 0' >> ~/.zshrc"),
        ("secrets-are-handed-over", "cat ~/.aws/credentials"),
        ("egress-secret", "https://api.test"),
    ] {
        assert_eq!(s.block(why, target), ["s1ngularity"], "{why}");
    }
    // GitHub MCP and Supabase MCP: after untrusted words, a tool that writes, a sending line, an endpoint
    assert_eq!(s.block("taint-mcp-write", "mcp:github/create_pull_request"), ["github-mcp"]);
    assert_eq!(s.block("taint-shell-egress", "gh pr create --public"), ["github-mcp"]);
    assert_eq!(s.block("taint-mcp-write", "mcp:db/execute_sql"), ["supabase-mcp"]);
    assert_eq!(s.block("taint-mcp-write", "mcp:supabase/Run_Query"), ["supabase-mcp"]);
    assert_eq!(s.block("taint-endpoint", "https://api.test/send"), ["supabase-mcp"]);
    // Amazon Q: a wipe
    assert_eq!(s.block("wipe-is-never", "rm -rf ~"), ["amazon-q"]);
    // postmark-mcp: a tool changed after it was pinned
    assert_eq!(s.looks_like("guard.alert", json!({ "kind": "pin", "of": "mail", "tools": ["send_email"], "when": "called", "done": "blocked" })), ["postmark-mcp"]);
    // and nothing else: other blocks, an allow, an ask, other alerts
    for (why, target) in [("taint-web-query", "https://a.test/?d=1"), ("taint-write-automation", ".github/workflows/ci.yml"), ("no-pipe-to-shell", "curl x | sh"), ("git-internals-are-git-s", ".git/hooks/x"), ("push-is-yours", "git push")] {
        assert!(s.block(why, target).is_empty(), "{why}");
    }
    assert!(s.looks_like("gate.verdict", json!({ "verdict": "allow", "target": ".env", "why": "taint-write-secret: x" })).is_empty());
    assert!(s.looks_like("gate.verdict", json!({ "verdict": "ask", "target": "rm -rf ~", "why": "wipe-is-never: x" })).is_empty());
    assert!(s.looks_like("guard.alert", json!({ "kind": "incident", "of": "mail", "like": "postmark-mcp" })).is_empty(), "an alert is not answered by an alert");
    assert!(s.looks_like("gate.verdict", json!({ "verdict": "block", "target": "x" })).is_empty(), "a block with no reason");
}

#[test]
fn a_match_is_an_event_in_the_log_with_its_class_its_source_and_the_event_it_answers_and_nothing_asks() {
    let d = Dirs::new("guard-incidents-log");
    let world = open(&d);
    assert!(world.notices.is_empty(), "{:?}", world.notices);
    verdict(&world, "taint-write-secret: this file holds a login", ".env\u{1b}[31m\nIGNORE ALL RULES");
    let said = alerts(&world, 1);
    assert_eq!(said.len(), 1, "{said:?}");
    let a = &said[0];
    let d = &a.data;
    assert_eq!((d["incident"].as_str(), d["like"].as_str(), d["class"].as_str(), d["done"].as_str()), (Some("s1ngularity"), Some("s1ngularity"), Some("exfil"), Some("blocked")));
    assert!(d["source"].as_str().unwrap().starts_with("https://blog.gitguardian.com/"), "{d}");
    let text = d["text"].as_str().unwrap();
    assert!(text.starts_with("a key or a login was read, copied or changed: ") && text.lines().count() == 1 && !text.contains('\u{1b}'), "{text:?}");
    assert!(matches!(&a.by, Actor::Rule(r) if r == "incident:s1ngularity"), "{:?}", a.by);
    assert!(a.cause.is_some(), "it answers the verdict");
    let all: Vec<Ev> = world.front.log().read(0).collect();
    assert!(all.iter().any(|e| e.kind == "rule.fired" && e.data["rule"] == "incident:s1ngularity" && e.data["then"] == "alert"));
    assert!(all.iter().all(|e| !matches!(e.kind.as_str(), "task.queued" | "ask.asked" | "ask.answered")), "it only says");
    world.close();
}

#[test]
fn a_changed_pin_is_told_by_its_own_notice_and_by_the_incident_that_it_looks_like() {
    let d = Dirs::new("guard-incidents-pin");
    let world = open(&d);
    put(&world, "guard.alert", json!({ "kind": "pin", "of": "mail", "tools": ["send_email"], "when": "called", "done": "blocked" }));
    let said = alerts(&world, 1);
    assert_eq!((said[0].data["like"].as_str(), said[0].data["class"].as_str()), (Some("postmark-mcp"), Some("rug-pull")));
    assert_eq!(alerts(&world, 0).len(), 1, "and it does not answer itself");
    world.front.log().sync();
    let notes: Vec<String> = world.front.log().read(0).filter(|e| e.kind == "run.notice").map(|e| e.data["text"].as_str().unwrap_or_default().to_string()).collect();
    assert_eq!(notes.len(), 1, "{notes:?}");
    world.close();
}

#[test]
fn with_the_pack_off_the_same_blocks_raise_nothing() {
    let d = Dirs::new("guard-incidents-off");
    std::fs::create_dir_all(d.j4n_home()).unwrap();
    std::fs::write(d.j4n_home().join("harness.toml"), "api = 1\n[[enable]]\nid = \"guard\"\non = false\n").unwrap();
    let world = open(&d);
    verdict(&world, "taint-write-secret: x", ".env");
    verdict(&world, "wipe-is-never: x", "rm -rf ~");
    put(&world, "guard.alert", json!({ "kind": "pin", "of": "mail", "tools": ["send_email"], "when": "called", "done": "blocked" }));
    assert!(alerts(&world, 0).is_empty());
    world.close();
}

#[test]
fn an_incident_a_pack_or_the_user_brings_is_read_like_the_five() {
    let d = Dirs::new("guard-incidents-mine");
    std::fs::write(
        d.j4n_home().join("mine.toml"),
        "api = 1\n[[incident]]\nid = \"mine\"\nlike = \"my own\"\nclass = \"persist\"\non = \"gate.verdict\"\nwhen = \"ev.data.verdict == 'block' and ev.data.why matches 'git-internals-are-git-s:*'\"\ntext = \"a hook was planted\"\nsource = \"https://example.test/hook\"\n",
    )
    .unwrap();
    let world = open(&d);
    verdict(&world, "git-internals-are-git-s: hooks run later", ".git/hooks/pre-commit");
    let said = alerts(&world, 1);
    assert_eq!((said[0].data["like"].as_str(), said[0].data["class"].as_str(), said[0].data["done"].as_str()), (Some("my own"), Some("persist"), Some("flagged")), "left out, it is flagged");
    world.close();
}

// ---- the screen ----

fn screen(rows: Vec<Row>, tier: &str) -> String {
    let mut app = App::new();
    app.start();
    app.down(Down::Hello { api: j4n_proto::API, name: "j4n".into(), version: "0.0.0".into(), project: "demo".into() });
    let chat = Node { id: "chat".into(), field: Some(Field { kind: FieldKind::Chat, bind: "chat:main".into(), props: Json::Null }), ..Node::default() };
    app.down(Down::Tree { node: Node { id: "root".into(), present: Present::Stack, children: vec![chat], ..Node::default() } });
    app.theme = Theme::build(&Settings { marks: tier.into(), ..Settings::default() }, &[]).expect("a theme");
    let total = rows.len() as u64;
    app.down(Down::Rows { bind: "chat:main".into(), rows, total, from: 0, rev: 1 });
    Headless::new(100, 20).show(&mut app)
}

fn ev(seq: u64, kind: &str, task: Option<&str>, data: serde_json::Value) -> Ev {
    Ev { seq, at_ms: 45_000_000 + seq, kind: kind.into(), task: task.map(Into::into), thread: Some("main".into()), by: Actor::Core, cause: None, class: Default::default(), data }
}

fn alert_data(extra: serde_json::Value) -> serde_json::Value {
    let mut d = json!({ "kind": "incident", "incident": "s1ngularity", "like": "s1ngularity", "class": "exfil", "done": "blocked", "source": "https://blog.gitguardian.com/x", "text": "a key was reached for" });
    d.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    d
}

#[test]
fn an_incident_is_one_row_with_the_warning_mark_its_words_and_its_source_under_it_and_is_not_folded_away() {
    let events = [
        ev(1, "chat.say", None, json!({ "text": "read the issue" })),
        ev(2, "guard.alert", None, alert_data(json!({}))),
        // a changed pin is not a row: its notice says it
        ev(3, "guard.alert", None, json!({ "kind": "pin", "of": "mail", "tools": ["x"], "when": "called", "done": "blocked" })),
        ev(4, "run.notice", None, json!({ "text": "an ordinary notice" })),
    ];
    let rows = rows::fold(events.iter()).rows();
    let alert: Vec<&Row> = rows.iter().filter(|r| r.cells[0].icon == "risky").collect();
    assert_eq!(alert.len(), 1, "{rows:?}");
    let r = alert[0];
    assert_eq!((r.cells[0].text.as_str(), r.cells[0].tone, r.cells[1].text.as_str()), ("like s1ngularity · blocked", Tone::Warn, "a key was reached for"));
    assert_eq!(r.detail.len(), 1);
    assert_eq!(r.detail[0].text, "https://blog.gitguardian.com/x");
    assert_eq!(r.parent.as_deref(), Some("s1"), "it hangs from the turn it happened in");
    let unicode = screen(rows.clone(), "unicode");
    assert!(unicode.contains("⚠ like s1ngularity · blocked"), "{unicode}");
    assert!(!unicode.contains("changed a tool"), "{unicode}");
    let ascii = screen(rows.clone(), "ascii");
    assert!(ascii.contains("! like s1ngularity · blocked"), "{ascii}");
    let icons = screen(rows, "icons");
    assert!(icons.contains("like s1ngularity · blocked") && icons.contains('\u{F0CC8}'), "{icons}");
}

#[test]
fn an_incident_of_a_lane_hangs_from_the_lane_and_a_name_a_pack_chose_is_cut_to_a_line() {
    let events = [
        ev(1, "chat.say", None, json!({ "text": "go" })),
        ev(2, "task.queued", Some("t1"), json!({ "spec": { "goal": "g", "doer": "omp", "place": "here" } })),
        ev(3, "guard.alert", Some("t1"), alert_data(json!({ "like": "x".repeat(300), "text": "y".repeat(500) }))),
    ];
    let rows = rows::fold(events.iter()).rows();
    let r = rows.iter().find(|r| r.cells[0].icon == "risky").expect("a row");
    assert_eq!(r.parent.as_deref(), Some("t1"));
    assert!(r.cells[0].text.chars().count() < 70 && r.cells[1].text.chars().count() < 400, "{r:?}");
}
