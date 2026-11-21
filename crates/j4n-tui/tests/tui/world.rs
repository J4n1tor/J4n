//! What the tests share: the page tree the core sends (written out here, because the TUI does not know the core), an app
//! that has been given it, and ways to put rows on the screen.

use crossterm::event::KeyEvent;
use j4n_proto::{AskView, Cell, Choice, Down, Field, FieldKind, Json, Node, Present, Role, Row, Shape, Tone, Up};
use j4n_tui::App;
use j4n_tui::headless::{Headless, typed};

pub fn leaf(id: &str, title: &str, kind: FieldKind, bind: &str) -> Node {
    Node { id: id.into(), title: title.into(), field: Some(Field { kind, bind: bind.into(), props: Json::Null }), ..Node::default() }
}

pub fn tree() -> Node {
    let pages = Node {
        id: "pages".into(),
        present: Present::Tabs,
        children: vec![
            leaf("chat", "Chat", FieldKind::Chat, "chat:main"),
            leaf("tasks", "Tasks", FieldKind::List, "tasks:all"),
            leaf("log", "Log", FieldKind::Log, "log:all"),
            leaf("settings", "Settings", FieldKind::Form, "form:thread"),
        ],
        ..Node::default()
    };
    Node { id: "root".into(), children: vec![pages, leaf("asks", "Questions", FieldKind::Ask, "asks:open"), leaf("bar", "", FieldKind::Bar, "bar:status")], ..Node::default() }
}

pub fn hello() -> Down {
    Down::Hello { api: j4n_proto::API, name: "j4n".into(), version: "0.0.0".into(), project: "demo".into() }
}

/// An app that has said hello and been given the tree; the `Up` it sent on the way are returned too.
pub fn started() -> (App, Vec<Up>) {
    let mut app = App::new();
    let mut sent = app.start();
    sent.extend(app.down(hello()));
    sent.extend(app.down(Down::Tree { node: tree() }));
    (app, sent)
}

pub fn app() -> App {
    started().0
}

/// A transcript as the core sends it (`chat:main`): each line of the operator's is a turn, and the rest are steps that hang from it
/// (the doer's words with no mark, the others with theirs). Lines before the first of the operator's stand as turns of their own.
pub fn chat(app: &mut App, lines: &[(Role, &str)]) {
    let mut all: Vec<Row> = Vec::new();
    let mut turn = String::new();
    for (i, (who, text)) in lines.iter().enumerate() {
        let key = format!("k{i}");
        let icon = match who {
            Role::Reasoning => "think",
            Role::Tool => "tool",
            Role::Notice => "notice",
            Role::Error => "error",
            _ => "",
        };
        let mut r = Row { key: key.clone(), cells: vec![Cell::with_icon(icon, *text, if *who == Role::Error { Tone::Bad } else { Tone::Plain }), Cell::new("", Tone::Dim)], ..Row::default() };
        if *who == Role::You {
            r.cells.push(Cell::new("12:00", Tone::Dim));
            turn = key;
        } else if !turn.is_empty() {
            r.parent = Some(turn.clone());
        }
        all.push(r);
    }
    rows(app, "chat:main", all);
}

pub fn row(key: &str, cells: &[&str]) -> Row {
    Row { key: key.into(), cells: cells.iter().map(|c| Cell::new(*c, Tone::Plain)).collect(), ..Row::default() }
}

pub fn rows(app: &mut App, bind: &str, rows: Vec<Row>) {
    let total = rows.len() as u64;
    assert!(app.down(Down::Rows { bind: bind.into(), rows, total, from: 0, rev: 1 }).is_empty());
}

pub fn ask(shape: Shape) -> AskView {
    AskView { ask: "t2".into(), asker: "t1".into(), question: "may I run `make build`?".into(), shape, options: Vec::new(), effect: String::new(), risk: 0, grants: Vec::new(), ..AskView::default() }
}

pub fn options(list: &[(&str, bool)]) -> Vec<Choice> {
    list.iter().map(|(id, rec)| Choice { id: id.to_string(), label: format!("the {id}"), recommended: *rec, ..Choice::default() }).collect()
}

/// Press every key; everything that was sent.
pub fn press(app: &mut App, keys: Vec<KeyEvent>) -> Vec<Up> {
    keys.into_iter().flat_map(|k| app.key(k)).collect()
}

pub fn type_in(app: &mut App, text: &str) -> Vec<Up> {
    press(app, typed(text, false))
}

pub fn show(app: &mut App, cols: u16, rows: u16) -> String {
    Headless::new(cols, rows).show(app)
}

/// The screen as lines, so a test can say which row something is on.
pub fn screen(app: &mut App, cols: u16, rows: u16) -> Vec<String> {
    show(app, cols, rows).lines().map(String::from).collect()
}

/// The rows that have the selection behind them: where the keys are on a list, a tray or a stack of fields. The pipe in front of such a
/// row is an elbow like any other: what marks it is the band.
pub fn banded(app: &mut App, cols: u16, rows: u16) -> Vec<String> {
    Headless::new(cols, rows).banded(app)
}

/// The look with no motion: `PgUp` and `PgDn` jump where they would glide.
pub fn still(app: &mut App) {
    let s = j4n_tui::theme::Settings { motion: false, ..Default::default() };
    app.theme = j4n_tui::theme::Theme::build(&s, &[]).expect("a theme");
}
