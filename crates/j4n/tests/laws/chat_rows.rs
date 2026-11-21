//! The chat the core makes is the chat the screen was built for. A conversation as events of the Log goes through the
//! core's fold into rows; the same conversation written by hand as the rows the scenes were drawn from goes to the same screen; the two
//! screens are the same at every size. What the events do not carry yet (the +/− of an edit, a route, a commit, a worktree's branch) is
//! left out of both.

use j4n_core::change::Actor;
use j4n_core::front::rows;
use j4n_core::log::Ev;
use j4n_proto::{Cell, Down, Field, FieldKind, Fold, Json, Node, Present, Row, Tone};
use j4n_tui::App;
use j4n_tui::headless::Headless;
use j4n_tui::theme::{Settings, Theme};
use serde_json::json;

pub(crate) struct Feed {
    seq: u64,
    at: u64,
    pub(crate) all: Vec<Ev>,
}

impl Feed {
    pub(crate) fn new() -> Feed {
        Feed { seq: 0, at: 12 * 3_600_000 + 29 * 60_000, all: Vec::new() }
    }

    pub(crate) fn ev(&mut self, kind: &str, task: Option<&str>, data: serde_json::Value) -> &mut Feed {
        self.by(kind, task, Actor::Core, data)
    }

    /// An event written by `by` (an answer the operator or a rule gave).
    pub(crate) fn by(&mut self, kind: &str, task: Option<&str>, by: Actor, data: serde_json::Value) -> &mut Feed {
        self.seq += 1;
        self.all.push(Ev { seq: self.seq, at_ms: self.at, kind: kind.into(), task: task.map(Into::into), thread: Some("main".into()), by, cause: None, class: Default::default(), data });
        self
    }

    pub(crate) fn after(&mut self, secs: u64) -> &mut Feed {
        self.at += secs * 1000;
        self
    }

    fn lane(&mut self, task: &str, doer: &str, model: &str) -> &mut Feed {
        self.ev("task.queued", Some(task), json!({ "spec": { "goal": "g", "doer": doer, "place": "here", "tune": { "model": model } } }));
        self.ev("task.started", Some(task), json!({ "doer": doer, "place": "here" }))
    }

    pub(crate) fn call(&mut self, task: &str, name: &str, detail: &str) -> &mut Feed {
        self.ev("run.tool_call", Some(task), json!({ "name": name, "detail": detail }))
    }

    fn rows(&self) -> Vec<Row> {
        rows::fold(self.all.iter()).rows()
    }
}

fn cell(icon: &str, text: &str, tone: Tone) -> Cell {
    Cell::with_icon(icon, text, tone)
}

fn row(key: &str, parent: &str, cells: Vec<Cell>) -> Row {
    Row { key: key.into(), cells, parent: (!parent.is_empty()).then(|| parent.to_string()), ..Row::default() }
}

fn step(key: &str, parent: &str, icon: &str, text: &str) -> Row {
    row(key, parent, vec![cell(icon, text, Tone::Plain), cell("", "", Tone::Dim)])
}

fn lane(key: &str, parent: &str, icon: &str, what: &str, model: &str, stats: &[&str]) -> Row {
    let mut cells = vec![cell(icon, what, Tone::Plain), cell("", model, Tone::Dim)];
    cells.extend(stats.iter().map(|s| cell("", s, Tone::Dim)));
    let mut r = row(key, parent, cells);
    // what the core says of a lane: the doer is the second word of its name, and no thread is bound here
    r.data = Some(json!({ "lane": { "doer": what.split(' ').nth(1).unwrap_or_default(), "own": false } }));
    r
}

fn merged(mut r: Row) -> Row {
    r.merge = true;
    r
}

/// The screen the rows make, at a size, in the `unicode` tier.
pub(crate) fn screen(rows: Vec<Row>, cols: u16, high: u16) -> String {
    let mut app = App::new();
    app.start();
    app.down(Down::Hello { api: j4n_proto::API, name: "j4n".into(), version: "0.0.0".into(), project: "demo".into() });
    let chat = Node { id: "chat".into(), field: Some(Field { kind: FieldKind::Chat, bind: "chat:main".into(), props: Json::Null }), ..Node::default() };
    app.down(Down::Tree { node: Node { id: "root".into(), present: Present::Stack, children: vec![chat], ..Node::default() } });
    app.theme = Theme::build(&Settings { marks: "unicode".into(), ..Settings::default() }, &[]).expect("a theme");
    let total = rows.len() as u64;
    app.down(Down::Rows { bind: "chat:main".into(), rows, total, from: 0, rev: 1 });
    Headless::new(cols, high).show(&mut app)
}

fn same(core: Vec<Row>, hand: Vec<Row>) {
    for (cols, high) in [(120, 40), (80, 24), (60, 20)] {
        let (a, b) = (screen(core.clone(), cols, high), screen(hand.clone(), cols, high));
        assert_eq!(a, b, "at {cols}×{high} the screen of the core's rows is not the screen of the hand-made ones\n{a}\n----\n{b}");
        assert!(a.contains("fix the parser"), "{a}");
    }
}

#[test]
fn a_conversation_with_two_lanes_is_the_same_screen_made_by_the_core_or_by_hand() {
    let mut f = Feed::new();
    f.ev("chat.say", None, json!({ "text": "fix the parser" }));
    f.lane("t1", "omp", "sonnet");
    f.call("t1", "Read", r#"{"file_path":"x.rs"}"#).call("t1", "Grep", r#"{"pattern":"trailing"}"#);
    f.ev("run.metrics", Some("t1"), json!({ "usd": 0.03, "tokens_in": 1, "tokens_out": 1 }));
    f.after(21).ev("task.settled", Some("t1"), json!({ "ok": true }));
    f.lane("t2", "claude", "sonnet");
    f.call("t2", "Read", r#"{"file_path":"a.rs"}"#).call("t2", "Read", r#"{"file_path":"b.rs"}"#).call("t2", "Read", r#"{"file_path":"c.rs"}"#);
    f.call("t2", "Edit", r#"{"file_path":"parser.rs","old_string":"x"}"#);
    f.ev("run.message", Some("t2"), json!({ "role": "assistant", "text": "Trailing commas now parse." }));
    f.after(64).ev("run.metrics", Some("t2"), json!({ "usd": 0.004, "tokens_in": 1, "tokens_out": 1 }));
    let hand = vec![
        row("s1", "", vec![cell("", "fix the parser", Tone::Plain), cell("", "", Tone::Dim), cell("", "12:29", Tone::Dim)]),
        lane("t1", "s1", "done", "A omp", "sonnet", &["21s", "$0.03"]),
        merged(step("h1", "t1", "read", "x.rs")),
        step("h2", "t1", "search", "trailing"),
        lane("t2", "s1", "working", "B claude", "sonnet", &["1:04", "1 tok", "$0.004"]),
        merged(step("h3", "t2", "read", "a.rs")),
        merged(step("h4", "t2", "read", "b.rs")),
        merged(step("h5", "t2", "read", "c.rs")),
        step("h6", "t2", "edit", "parser.rs"),
        step("h7", "t2", "", "Trailing commas now parse."),
        // the lane works and has said something: the live row, with what it said last, its tokens and how long it has been so
        row("w:t2", "t2", vec![cell("think", "working", Tone::Accent), cell("", "after Trailing commas now parse.", Tone::Dim), cell("", "", Tone::Dim), cell("", "1 tok", Tone::Dim), cell("", "1:04", Tone::Dim)]),
    ];
    same(f.rows(), hand);
}

#[test]
fn a_lane_that_failed_stays_open_with_its_command_its_output_and_why_in_the_core_rows_as_in_the_hand_made_ones() {
    let mut f = Feed::new();
    f.ev("chat.say", None, json!({ "text": "fix the parser" }));
    f.lane("t1", "pi", "sonnet");
    f.ev("run.shell", Some("t1"), json!({ "cmd": "cargo clippy -- -D warnings", "out": "error: this `if` has identical blocks\nerror: could not compile `parser`", "exit": 101 }));
    f.after(14).ev("task.stopped", Some("t1"), json!({ "why": { "kind": "failed", "message": "clippy" } }));
    let mut shell = row("h1", "t1", vec![cell("run", "cargo clippy -- -D warnings", Tone::Plain), cell("", "", Tone::Dim), cell("no", "101", Tone::Bad)]);
    shell.detail = ["error: this `if` has identical blocks", "error: could not compile `parser`"].iter().map(|l| Cell::new(*l, Tone::Plain)).collect();
    let mut failed = lane("t1", "s1", "failed", "A pi", "sonnet", &["14s"]);
    failed.fold = Some(Fold::Pinned);
    let hand = vec![
        row("s1", "", vec![cell("", "fix the parser", Tone::Plain), cell("", "", Tone::Dim), cell("", "12:29", Tone::Dim)]),
        failed,
        shell,
        row("h2", "t1", vec![cell("notice", "failed: clippy", Tone::Bad), cell("", "", Tone::Dim)]),
    ];
    same(f.rows(), hand);
}

#[test]
fn the_reads_note_and_the_live_row_of_a_lane_at_work_read_whole_at_80_columns() {
    let mut f = Feed::new();
    f.ev("chat.say", None, json!({ "text": "write the chess game in one file, then run it" }));
    // what the read says under a line (`quick_look`): taken as, by whom, cost, wait, and why
    let note = "read as split (2 chats) (the line names two files) · Decider read · $0.0004 · 0.6s · one chat writes chess.py, the other runs it";
    f.ev("chat.note", None, json!({ "text": note, "tone": "dim" }));
    f.lane("t1", "claude", "sonnet");
    f.ev("run.open", Some("t1"), json!({ "id": "0", "name": "Write", "target": "/w/chess.py", "bytes": 23_900 }));
    f.after(90).ev("run.tokens", Some("t1"), json!({ "out": 6100 }));
    let screen = screen(f.rows(), 80, 24);
    assert!(screen.lines().all(|l| l.chars().count() <= 80), "{screen}");
    for word in note.split_whitespace().filter(|w| *w != "·") {
        assert!(screen.contains(word), "the note is whole at 80 columns, `{word}` is not on the screen:\n{screen}");
    }
    let live = screen.lines().find(|l| l.contains("writing")).unwrap_or_else(|| panic!("the live row:\n{screen}"));
    assert!(live.contains("chess.py") && live.contains("kB") && live.contains("tok") && !live.contains('…'), "the row says what, how much, tokens: {live}\n{screen}");
}
