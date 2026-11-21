//! What the TUI is told (`Down`) for what it says (`Up`) and for what the Log records: the chat as lines, the rows, the open
//! questions. The front, the loop and the Log are the real ones; the doers are scripted.

use crate::front_cases::{Fx, fx, fx_screened};
use crate::scripted::{Beat, Scripted, approval, end, say};
use j4n_core::change::Actor;
use j4n_core::front::Said;
use j4n_core::front::serve::Serve;
use j4n_core::log::Ev;
use j4n_core::task::{Ctl, State};
use j4n_proto::{Control, Down, Intent, Line, Reply, Role, Shape, Tone, Up};
use serde_json::{Value, json};
use std::sync::Arc;

fn serve(f: &Fx) -> Serve {
    Serve::new(f.front.clone())
}

fn up(s: &mut Serve, u: Up) -> Vec<Down> {
    let (downs, quit) = s.up(u);
    assert!(!quit, "{downs:?}");
    downs
}

fn watch(s: &mut Serve, bind: &str) -> Vec<Down> {
    up(s, Up::Watch { bind: bind.into() })
}

fn say_up(s: &mut Serve, text: &str) {
    up(s, Up::Intent { intent: Intent::Say { text: text.into(), with: Vec::new() } });
}

/// Feed the events the Log holds from `from` on to the client, as the server thread would.
fn feed(s: &mut Serve, f: &Fx, from: u64) -> Vec<Down> {
    f.rig.log.read(from).flat_map(|e| s.ev(&e)).collect()
}

/// The chat as the words it holds, from the rows of its last `Rows`: a turn is the operator's line, a step that is plain words the doer's,
/// a notice or an error as it is marked; the lanes (a mark of their state, a letter and a doer) are not words and are left out.
fn lines(downs: &[Down], bind: &str) -> Vec<Line> {
    let Some(rows) = downs.iter().rev().find_map(|d| match d {
        Down::Rows { bind: b, rows, .. } if b == bind => Some(rows),
        _ => None,
    }) else {
        return Vec::new();
    };
    let lane = |icon: &str| matches!(icon, "queued" | "working" | "done" | "failed" | "stopped" | "cooling" | "ask") && rows.iter().any(|r| r.parent.is_none());
    rows.iter()
        .filter_map(|r| {
            let first = r.cells.first()?;
            let role = match (r.parent.is_none(), first.icon.as_str()) {
                (true, _) => Role::You,
                (_, "notice" | "session") => Role::Notice,
                (_, "") if r.by == j4n_proto::By::Core => Role::Notice,
                (_, "error") => Role::Error,
                (_, "think") => Role::Reasoning,
                (_, "") => Role::Doer,
                (_, "run") => Role::Shell,
                (_, i) if r.parent.as_ref().is_some_and(|p| rows.iter().any(|t| &t.key == p && t.parent.is_none())) && lane(i) => return None,
                _ => Role::Tool,
            };
            Some(Line { key: r.key.clone(), role, text: first.text.clone(), task: None, partial: false, tone: first.tone })
        })
        .collect()
}

/// The money of today in the bar: the text of its cell, without the word that leads it.
fn today(bar: &[j4n_proto::Row]) -> String {
    bar[0].cells.iter().find_map(|c| c.text.strip_prefix("today ").map(String::from)).unwrap_or_default()
}

fn texts(ls: &[Line]) -> Vec<(Role, String)> {
    ls.iter().map(|l| (l.role, l.text.clone())).collect()
}

fn chatty(answers: &[&str]) -> Arc<Scripted> {
    Scripted::new("alpha", answers.iter().map(|a| vec![Beat::Session("s".into()), say(a), end(a)]).collect())
}

fn rows<'a>(downs: &'a [Down], bind: &str) -> &'a [j4n_proto::Row] {
    downs.iter().rev().find_map(|d| match d {
        Down::Rows { bind: b, rows, .. } if b == bind => Some(rows.as_slice()),
        _ => None,
    }).unwrap_or(&[])
}

// ---- hello and the tree ----

#[test]
fn hello_is_answered_with_who_this_is_and_the_tree() {
    let f = fx(vec![chatty(&["a"])], |c| c.project = "demo".into());
    let mut s = serve(&f);
    let downs = up(&mut s, Up::Hello { api: j4n_proto::API });
    assert!(matches!(&downs[0], Down::Hello { api, name, project, .. } if *api == j4n_proto::API && name == "j4n" && project == "demo"), "{downs:?}");
    let screen = j4n_core::front::screen::Screen::builtin();
    assert_eq!(downs[1], Down::Keys { binds: screen.keys.clone() }, "then what the keys do, before the tree");
    assert!(matches!(&downs[1], Down::Keys { binds } if binds.len() > 60 && binds.iter().all(|b| b.to == b.key)), "{:?}", downs[1]);
    assert_eq!(downs[2], Down::Theme { look: screen.look() }, "then the look, before the tree");
    assert_eq!(downs[3], Down::Tree { node: screen.titled(screen.pages.tree("main", &j4n_core::view::outer(screen.set.clone()))) });
    assert_eq!(downs.len(), 4);
}

#[test]
fn the_look_is_the_words_of_the_theme_settings_and_the_design_where_a_pack_says_none() {
    use j4n_core::front::screen::Screen;
    use j4n_core::setting::Settings;
    use j4n_core::table::{Layer, LayerKind, SourceFile, Tables};
    let none = Screen::builtin().look();
    assert_eq!((none.palette.as_str(), none.marks.as_str(), none.frame.as_str(), none.corners.as_str()), ("jjj", "icons", "round", "round"));
    assert_eq!((none.title_place.as_str(), none.density.as_str(), none.motion), ("inside", "normal", true));
    assert_eq!(none.fill, "match", "the terminal's background follows the palette unless a layer says otherwise");
    assert_eq!(none.you, "bracket", "your line is a bracket between two caps unless a layer says `band` or `mark`");
    let file = |path: &str, text: &str| SourceFile { path: path.into(), text: text.into() };
    let pack = Layer::new(LayerKind::Pack, "base", vec![file("settings/theme.toml", include_str!("../../../../packs/base/settings/theme.toml"))]);
    let mine = "api = 1\n[[setting]]\nid = \"theme.frame\"\ndoc = \"x\"\nty = \"str\"\ndefault = \"pipes\"\nvalue = \"gutter\"\n\n[[setting]]\nid = \"theme.motion\"\ndoc = \"x\"\nty = \"bool\"\ndefault = true\nvalue = false\n";
    let you = "api = 1\n[[setting]]\nid = \"theme.you\"\ndoc = \"x\"\nty = \"str\"\ndefault = \"bracket\"\nvalue = \"mark\"\n";
    let load = |layers: Vec<Layer>| {
        let (tables, problems) = Tables::load(layers);
        assert!(problems.is_empty(), "{problems:?}");
        let (settings, problems) = Settings::load(&tables);
        assert!(problems.is_empty(), "{problems:?}");
        Screen::load(tables, &settings, Vec::new(), &Default::default()).0.look()
    };
    assert_eq!(load(vec![pack.clone()]), none, "the pack's defaults are the design's");
    let chosen = load(vec![pack, Layer::new(LayerKind::User, "home", vec![file("mine.toml", mine)])]);
    assert_eq!((chosen.frame.as_str(), chosen.motion, chosen.palette.as_str()), ("gutter", false, "jjj"), "the operator's words win, the rest stays");
    let marked = load(vec![Layer::new(LayerKind::Pack, "base", vec![file("settings/theme.toml", include_str!("../../../../packs/base/settings/theme.toml"))]), Layer::new(LayerKind::User, "home", vec![file("you.toml", you)])]);
    assert_eq!(marked.you, "mark", "the operator can keep the icon of their line");
    let empty = "api = 1\n[[setting]]\nid = \"theme.frame\"\ndoc = \"x\"\nty = \"str\"\ndefault = \"pipes\"\nvalue = \"\"\n";
    let blank = load(vec![Layer::new(LayerKind::Pack, "base", vec![file("settings/theme.toml", include_str!("../../../../packs/base/settings/theme.toml"))]), Layer::new(LayerKind::User, "home", vec![file("mine.toml", empty)])]);
    assert_eq!(blank.frame, "round", "an empty word is no word: the design's stands");
}

#[test]
fn the_look_says_how_the_charts_are_drawn_dithered_unless_the_setting_says_plain() {
    use j4n_core::front::screen::Screen;
    use j4n_core::setting::Settings;
    use j4n_core::table::{Layer, LayerKind, SourceFile, Tables};
    assert_eq!(Screen::builtin().look().chart, "dither", "no pack, no word: the dithered charts");
    let file = |path: &str, text: &str| SourceFile { path: path.into(), text: text.into() };
    let load = |mine: &str| {
        let pack = Layer::new(LayerKind::Pack, "base", vec![file("settings/look.toml", include_str!("../../../../packs/base/settings/look.toml"))]);
        let layers = if mine.is_empty() { vec![pack] } else { vec![pack, Layer::new(LayerKind::User, "home", vec![file("mine.toml", mine)])] };
        let (tables, problems) = Tables::load(layers);
        assert!(problems.is_empty(), "{problems:?}");
        let (settings, problems) = Settings::load(&tables);
        assert!(problems.is_empty(), "{problems:?}");
        Screen::load(tables, &settings, Vec::new(), &Default::default()).0.look().chart
    };
    assert_eq!(load(""), "dither", "the pack's default");
    assert_eq!(load("api = 1\n[[setting]]\nid = \"look.chart\"\nvalue = \"plain\"\n"), "plain", "the operator's word wins");
}

#[test]
fn a_screen_that_speaks_a_newer_protocol_is_told_so_and_closed() {
    let f = fx(vec![chatty(&["a"])], |_| {});
    let (downs, quit) = serve(&f).up(Up::Hello { api: j4n_proto::API + 1 });
    assert!(quit);
    assert!(matches!(&downs[..], [Down::Toast { tone: Tone::Bad, text }] if text.contains("protocol")), "{downs:?}");
}

#[test]
fn the_tree_has_the_pages_and_every_field_is_bound_to_something_the_server_answers() {
    let f = fx(vec![chatty(&["a"])], |_| {});
    let mut s = serve(&f);
    let tree = j4n_core::front::screen::Screen::builtin().pages.tree("main", &j4n_core::eval::Scope::new());
    let mut binds = Vec::new();
    fn walk(n: &j4n_proto::Node, out: &mut Vec<String>) {
        out.extend(n.field.iter().map(|f| f.bind.clone()));
        n.children.iter().for_each(|c| walk(c, out));
    }
    walk(&tree, &mut binds);
    // the tree of TUI-SIMPLE §3: Models (Library, Providers, Plans, Trust with the base pack alone), then Connect › Setup and Tools, then Spend and Grow
    let views = ["ask-points", "library", "providers", "plans", "trust", "setup-flow", "connector-agents", "connector-tools", "connector-services", "connector-mcp", "connector-deciders"];
    let settings = ["settings-features", "settings-look", "form:thread", "settings-keys", "settings-loop", "settings-gates", "settings", "commands:all"];
    let want: Vec<String> = ["chat:main", "tasks:all"].into_iter().map(String::from).chain(views.iter().map(|v| format!("view:{v}"))).chain(["view:spend".into(), "view:proposals".into(), "packs:all".into(), "view:upstream-news".into(), "view:upstream-todo".into(), "log:all".into()]).chain(settings.iter().map(|s| if s.contains(':') { s.to_string() } else { format!("view:{s}") })).chain(["asks:open".into(), "bar:status".into()]).collect();
    let (charts, binds): (Vec<String>, Vec<String>) = binds.into_iter().partition(|b| b.starts_with("chart:"));
    assert_eq!(charts, ["chart:wall", "chart:spend", "chart:bench"], "the charts over Models, Spend and Grow (the one over Tasks stands while the lanes are not single)");
    assert_eq!(binds, want);
    for b in binds.iter().chain(&charts) {
        let downs = watch(&mut s, b);
        assert!(!downs.iter().any(|d| matches!(d, Down::Toast { .. })), "{b}: {downs:?}");
    }
}

#[test]
fn a_binding_nobody_knows_is_a_toast_not_a_silence() {
    let f = fx(vec![chatty(&["a"])], |_| {});
    let downs = watch(&mut serve(&f), "table:nope");
    assert!(matches!(&downs[..], [Down::Toast { tone: Tone::Warn, text }] if text.contains("table:nope")), "{downs:?}");
}

// ---- the chat ----

#[test]
fn a_line_typed_and_its_answer_arrive_as_lines_of_the_chat() {
    let f = fx(vec![chatty(&["it prints a greeting"])], |_| {});
    let mut s = serve(&f);
    watch(&mut s, "chat:main");
    say_up(&mut s, "what does main.rs do?");
    f.rig.finished("t1");
    let downs = feed(&mut s, &f, 0);
    let got = texts(&lines(&downs, "chat:main"));
    assert_eq!(got[0], (Role::You, "what does main.rs do?".to_string()));
    assert!(got.contains(&(Role::Notice, "bound to alpha (elected)".to_string())), "{got:?}");
    assert_eq!(got.last().unwrap(), &(Role::Doer, "it prints a greeting".to_string()));
}

#[test]
fn streamed_words_become_one_line_that_the_settled_message_then_replaces() {
    let f = fx(vec![chatty(&["x"])], |_| {});
    let mut s = serve(&f);
    watch(&mut s, "chat:main");
    let delta = |text: &str| Ev { seq: 0, at_ms: 1, kind: "run.delta".into(), task: Some("t1".into()), thread: Some("main".into()), by: Actor::Doer("alpha".into()), cause: None, class: Default::default(), data: json!({ "text": text }) };
    let message = |seq: u64, text: &str| Ev { seq, kind: "run.message".into(), data: json!({ "role": "assistant", "text": text }), ..delta("") };
    let mut downs = Vec::new();
    downs.extend(s.ev(&delta("It pri")));
    downs.extend(s.ev(&delta("nts")));
    let said = |downs: &[Down]| texts(&lines(downs, "chat:main")).into_iter().filter(|(r, _)| *r == Role::Doer).map(|(_, t)| t).collect::<Vec<_>>();
    assert_eq!(said(&downs), ["It prints"]);
    downs.extend(s.ev(&message(9, "It prints a greeting.")));
    assert_eq!(said(&downs), ["It prints a greeting."], "the same step, not a second one");
    downs.extend(s.ev(&delta("More")));
    assert_eq!(said(&downs), ["It prints a greeting.", "More"], "the next words are a step of their own");
    assert!(s.ev(&delta("")).is_empty(), "an empty delta says nothing");
}

#[test]
fn events_of_another_thread_or_of_nothing_the_chat_shows_are_not_in_it() {
    let f = fx(vec![chatty(&["a"])], |_| {});
    let mut s = serve(&f);
    watch(&mut s, "chat:main");
    let other = Ev { seq: 5, at_ms: 1, kind: "chat.say".into(), task: None, thread: Some("elsewhere".into()), by: Actor::Operator, cause: None, class: Default::default(), data: json!({ "text": "x" }) };
    assert!(s.ev(&other).is_empty());
    let odd = Ev { thread: Some("main".into()), kind: "something.new".into(), ..other };
    assert!(s.ev(&odd).is_empty(), "an event kind this version does not know is skipped");
}

#[test]
fn nothing_is_sent_for_a_binding_that_is_not_watched() {
    let f = fx(vec![chatty(&["a"])], |_| {});
    let mut s = serve(&f);
    say_up(&mut s, "hello");
    f.rig.finished("t1");
    assert!(feed(&mut s, &f, 0).is_empty());
    watch(&mut s, "chat:main");
    up(&mut s, Up::Unwatch { bind: "chat:main".into() });
    f.front.say("main", "again");
    f.rig.finished("t2");
    assert!(feed(&mut s, &f, 0).is_empty());
}

#[test]
fn a_chat_read_back_after_a_restart_is_the_chat_that_was_shown() {
    let doer = || chatty(&["one", "two"]);
    let f = fx(vec![doer()], |_| {});
    let mut live = serve(&f);
    watch(&mut live, "chat:main");
    f.turn("first");
    f.turn("second");
    f.say(":status");
    let shown = lines(&feed(&mut live, &f, 0), "chat:main");
    assert!(shown.len() >= 6, "{shown:?}");
    let fresh = serve(&f);
    let mut fresh = fresh;
    let again = lines(&watch(&mut fresh, "chat:main"), "chat:main");
    assert_eq!(texts(&again), texts(&shown));
    assert!(again.iter().all(|l| !l.partial));
}

#[test]
fn a_replay_is_the_last_rows_only_a_turn_with_the_last_of_its_steps() {
    let f = fx(vec![chatty(&["a"])], |_| {});
    for i in 0..620 {
        f.front.note("main", &format!("note {i}"), "dim");
    }
    let mut s = serve(&f);
    let downs = watch(&mut s, "chat:main");
    let [Down::Rows { rows, total, from, .. }] = &downs[..] else { panic!("{downs:?}") };
    assert_eq!((rows.len(), *total, *from), (500, 500, 0));
    assert!(rows[0].parent.is_none() && rows[1..].iter().all(|r| r.parent.as_deref() == Some(rows[0].key.as_str())), "a turn, and the steps that hang from it");
    let last = lines(&downs, "chat:main").pop().unwrap();
    assert_eq!(last.text, "note 619");
}

#[test]
fn a_replay_holds_the_events_of_the_thread_and_no_others() {
    let f = fx(vec![chatty(&["a"])], |_| {});
    f.front.note("elsewhere", "not for this chat", "dim");
    f.front.note("main", "for this chat", "dim");
    let mut s = serve(&f);
    let shown = texts(&lines(&watch(&mut s, "chat:main"), "chat:main"));
    assert!(shown.contains(&(Role::Notice, "for this chat".to_string())) && !shown.iter().any(|(_, t)| t.contains("not for this")), "{shown:?}");
}

// ---- the log ----

#[test]
fn the_log_field_gets_the_written_events_and_never_the_flashed_ones() {
    let f = fx(vec![chatty(&["a"])], |_| {});
    let mut s = serve(&f);
    let first = watch(&mut s, "log:all");
    assert!(matches!(&first[..], [Down::Rows { bind, .. }] if bind == "log:all"), "{first:?}");
    f.turn("hello");
    let mut downs = feed(&mut s, &f, 0);
    downs.extend(s.tick());
    let shown = rows(&downs, "log:all");
    assert!(shown.iter().any(|r| r.cells[1].text.starts_with("task.settled")), "{shown:?}");
    let flash = Ev { seq: 0, at_ms: 1, kind: "run.delta".into(), task: Some("t1".into()), thread: None, by: Actor::Core, cause: None, class: Default::default(), data: json!({ "text": "x" }) };
    assert!(s.ev(&flash).is_empty());
}

// ---- the rows ----

#[test]
fn the_task_list_has_a_row_for_each_task_of_the_thread_and_an_abort_for_those_that_go_on() {
    let f = fx(vec![Scripted::new("alpha", vec![vec![Beat::Session("s".into()), say("first"), end("first")], vec![Beat::Forever]])], |_| {});
    let mut s = serve(&f);
    f.turn("one");
    let Said::Started { task } = f.say("two") else { panic!() };
    f.rig.until(&task, "running", |r| r.state == State::Running);
    let downs = watch(&mut s, "tasks:all");
    let r = rows(&downs, "tasks:all");
    assert_eq!(r.len(), 2);
    assert_eq!(r[0].key, "t2", "the newest first");
    assert_eq!(r[0].cells[1].text, "running");
    assert_eq!(r[0].actions[0].intent, Intent::Control { task: "t2".into(), control: Control::Abort });
    assert!(r[1].actions.is_empty(), "a finished task has nothing to abort");
    assert_eq!((r[1].cells[1].text.as_str(), r[1].cells[1].tone, r[1].cells[3].text.as_str()), ("done", Tone::Good, "one"));
    assert_eq!(r[1].detail[0].text, "first", "the answer is under the row");
    f.rig.tasks.ctl(&task, Ctl::Abort, Actor::Operator).unwrap();
}

#[test]
fn a_question_is_not_a_task_of_the_list_but_it_is_an_ask() {
    let asker = Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::Ask(approval("a", "make build")), Beat::Hold, end("x")]]);
    let f = fx(vec![asker], |_| {});
    let mut s = serve(&f);
    let Said::Started { task } = f.say("build") else { panic!() };
    f.rig.until(&task, "waiting", |r| matches!(r.state, State::Waiting { .. }));
    let tasks = watch(&mut s, "tasks:all");
    assert_eq!(rows(&tasks, "tasks:all").len(), 1);
    let asks = watch(&mut s, "asks:open");
    let Down::Asks { open } = &asks[0] else { panic!("{asks:?}") };
    assert_eq!(open.len(), 1);
    assert_eq!((open[0].shape, open[0].asker.as_str()), (Shape::Approval, task.as_str()));
    assert_eq!(open[0].risk, 40);
    assert!(watch(&mut s, "asks:open").is_empty(), "the same questions are not sent twice");
}

#[test]
fn an_approval_that_names_its_effect_shows_it_and_the_grants_the_gate_would_give() {
    let mut spec = approval("a", "make build");
    spec.act = Some(Box::new(j4n_core::gate::Action::shell(Actor::Doer("alpha".into()), "make build")));
    let asker = Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::Ask(spec), Beat::Hold, end("x")]]);
    let f = fx(vec![asker], |_| {});
    let mut s = serve(&f);
    let Said::Started { task } = f.say("build") else { panic!() };
    f.rig.until(&task, "waiting", |r| matches!(r.state, State::Waiting { .. }));
    let Down::Asks { open } = watch(&mut s, "asks:open").remove(0) else { panic!() };
    assert_eq!(open[0].effect, "shell: make build");
    assert!(open[0].grants.contains(&"session".to_string()) && !open[0].grants.contains(&"once".to_string()), "{:?}", open[0].grants);
    // the answer comes by the button, and the next event takes the question off the screen
    let before = f.rig.log.head();
    up(&mut s, Up::Intent { intent: Intent::Answer { ask: open[0].ask.clone(), reply: Reply::Yes } });
    f.rig.finished(&task);
    let downs = feed(&mut s, &f, before);
    assert!(downs.iter().any(|d| matches!(d, Down::Asks { open } if open.is_empty())), "{downs:?}");
}

#[test]
fn the_bar_says_what_the_screen_is_doing_and_what_today_cost_and_not_who_it_is_with_nor_the_chats_own_state() {
    let f = fx(vec![Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::Spend(0.5), say("a"), end("a")]])], |c| c.model = Some("haiku".into()));
    let mut s = serve(&f);
    let cells = |s: &mut Serve| rows(&watch(s, "bar:status"), "bar:status")[0].cells.iter().map(|c| c.text.clone()).collect::<Vec<_>>();
    // the doer and the model are the composer strip's to say; the bar does not say them, nor the mode
    // the icon of the Decider has no word while it can be asked
    assert_eq!(cells(&mut s), ["idle", "", "proj"]);
    f.turn("go");
    assert_eq!(cells(&mut s), ["idle", "today $0.5000", "", "proj"]);
    let strip = j4n_proto::COMPOSER_STRIP;
    let route = |s: &mut Serve| watch(s, strip).iter().rev().find_map(|d| match d { Down::Value { bind, value, .. } if bind == strip => value.get("route").cloned(), _ => None });
    assert_eq!(route(&mut s), Some(json!("alpha · haiku")));
}

#[test]
fn a_frame_the_dialect_cannot_read_is_a_run_event_of_its_own_with_the_dialect_and_the_kind_and_no_text() {
    let f = fx(vec![Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::Unread("claude-stream", "mystery_frame"), say("a"), end("a")]])], |_| {});
    f.turn("go");
    f.front.log().sync();
    let got: Vec<_> = f.front.log().read(0).filter(|e| e.kind == "run.unread").collect();
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].data, json!({ "dialect": "claude-stream", "kind": "mystery_frame" }));
    assert_eq!(j4n_core::log::actor_word(&got[0].by), "doer:alpha");
    assert!(got[0].task.is_some(), "it belongs to the task whose stream it was");
}

/// The cells of the status row, with the tone of the row.
fn bar_of(s: &mut Serve) -> (Vec<String>, Tone) {
    let got = watch(s, "bar:status");
    let row = &rows(&got, "bar:status")[0];
    (row.cells.iter().map(|c| c.text.clone()).collect(), row.tone)
}

#[test]
fn the_bottom_bar_is_global_idle_running_waiting_money_today_and_a_window_that_runs_low() {
    let script = |beats: Vec<Beat>| Scripted::new("alpha", vec![beats]);
    // idle: nothing runs, the row is plain (the screen shows the keys)
    let f = fx(vec![script(vec![Beat::Session("s".into()), say("a"), end("a")])], |_| {});
    let mut s = serve(&f);
    assert_eq!(bar_of(&mut s), (vec!["idle".into(), "".into(), "proj".into()], Tone::Plain));
    // one runs: the count, in the accent (the screen shows the state)
    let f = fx(vec![script(vec![Beat::Session("s".into()), Beat::Hold, end("x")])], |_| {});
    let mut s = serve(&f);
    let Said::Started { task } = f.say("go") else { panic!() };
    f.rig.until(&task, "running", |r| matches!(r.state, State::Running));
    assert_eq!(bar_of(&mut s), (vec!["1 running".into(), "".into(), "proj".into()], Tone::Accent));
    // one waits for the operator: it says so, in warn, and wins the tone over the one that runs
    let f = fx(vec![script(vec![Beat::Session("s".into()), Beat::Ask(approval("a", "make build")), Beat::Hold, end("x")])], |_| {});
    let mut s = serve(&f);
    let Said::Started { task } = f.say("build") else { panic!() };
    f.rig.until(&task, "waiting", |r| matches!(r.state, State::Waiting { .. }));
    assert_eq!(bar_of(&mut s), (vec!["1 waits for you".into(), "".into(), "proj".into()], Tone::Warn));
    // money today: the sum of the tasks queued since the machine's midnight, and none of the days before
    let f = fx(vec![Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::Spend(0.5), say("a"), end("a")]])], |_| {});
    let mut s = serve(&f);
    f.turn("go");
    assert_eq!(bar_of(&mut s).0, ["idle", "today $0.5000", "", "proj"]);
    f.rig.clock.advance(2 * 86_400_000);
    assert_eq!(bar_of(&mut s).0, ["idle", "", "proj"], "two days on, it was not today");
    // a window past four fifths used, until it resets, said bare; the one that is not low is not said
    let f = fx(vec![script(vec![Beat::Session("s".into()), Beat::Limits(vec![("five_hour", 0.92, 200_000), ("seven_day", 0.4, 200_000)]), say("a"), end("a")])], |_| {});
    let mut s = serve(&f);
    f.turn("go");
    f.rig.log.sync();
    assert_eq!(bar_of(&mut s), (vec!["idle".into(), "5h 92%".into(), "".into(), "proj".into()], Tone::Plain));
    assert_eq!(rows(&watch(&mut s, "bar:status"), "bar:status")[0].cells[1].tone, Tone::Warn);
    f.rig.clock.advance(300_000_000);
    assert_eq!(bar_of(&mut s).0, ["idle", "", "proj"], "the window has reset");
}

#[test]
fn the_context_gauge_and_the_threads_cost_are_the_strips_and_the_bar_has_neither() {
    let f = fx(vec![Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::Spend(0.5), Beat::Window(0.72), say("a"), end("a")]])], |c| c.usd_soft = Some(2.0));
    let mut s = serve(&f);
    let bar = |s: &mut Serve| rows(&watch(s, "bar:status"), "bar:status")[0].cells.iter().map(|c| c.text.clone()).collect::<Vec<_>>();
    f.turn("go");
    assert_eq!(bar(&mut s), ["idle", "today $0.5000", "", "proj"], "no ctx, no thread cost with its limit, no mode");
    let strip = j4n_proto::COMPOSER_STRIP;
    let value = watch(&mut s, strip).iter().rev().find_map(|d| match d { Down::Value { bind, value, .. } if bind == strip => Some(value.clone()), _ => None }).unwrap();
    assert_eq!((value["gauge"].as_f64(), value["cost"].as_str(), value["mode"].as_str()), (Some(0.72), Some("$0.5000 of $2.0000"), Some("write")), "the chat's own state stands once, in the composer strip");
}

#[test]
fn the_form_lists_the_binding_and_a_change_of_doer_or_model_is_a_switch() {
    let f = fx(vec![chatty(&["a"]), Scripted::new("beta", vec![vec![end("b")]])], |_| {});
    let mut s = serve(&f);
    let form = |s: &mut Serve| rows(&watch(s, "form:thread"), "form:thread").iter().map(|r| (r.key.clone(), r.cells[1].text.clone(), r.actions.len())).collect::<Vec<_>>();
    assert_eq!(form(&mut s), [("doer".into(), "(chosen by the first line)".into(), 1), ("model".into(), "(the doer's own)".into(), 0), ("how".into(), "-".into(), 0), ("mode".into(), "write".into(), 0)]);
    f.turn("go");
    let set = |s: &mut Serve, field: &str, value: Value| up(s, Up::Intent { intent: Intent::Set { kind: "thread".into(), id: "main".into(), field: field.into(), value } });
    set(&mut s, "doer", json!("beta"));
    let b = f.front.binding("main").unwrap();
    assert_eq!((b.doer.as_str(), b.how.as_str()), ("beta", "switch"));
    set(&mut s, "model", json!("m2"));
    assert_eq!(f.front.binding("main").unwrap().model.as_deref(), Some("m2"));
    set(&mut s, "doer", json!("nosuch"));
    assert_eq!(f.front.binding("main").unwrap().doer, "beta", "a doer that is not there changes nothing");
    assert!(f.notes().last().unwrap().contains("nosuch"));
    up(&mut s, Up::Intent { intent: Intent::Set { kind: "lamp".into(), id: "x".into(), field: "oil".into(), value: json!(1) } });
    assert!(f.notes().last().unwrap().contains("nothing of kind `lamp`"));
    set(&mut s, "color", json!("red"));
    assert!(f.notes().last().unwrap().contains("which doer"), "{:?}", f.notes());
}

// ---- what the operator says ----

#[test]
fn every_intent_reaches_the_front_and_quit_closes_the_client() {
    let hang = Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::Forever]]);
    let f = fx(vec![hang], |_| {});
    let mut s = serve(&f);
    say_up(&mut s, "go");
    f.rig.until("t1", "running", |r| r.state == State::Running);
    up(&mut s, Up::Intent { intent: Intent::Run { command: "status".into(), args: vec![] } });
    assert!(f.notes().last().unwrap().contains("bound to alpha"));
    assert_eq!(up(&mut s, Up::Intent { intent: Intent::Open { node: "log".into() } }), [Down::Focus { node: "log".into() }]);
    up(&mut s, Up::Intent { intent: Intent::Control { task: "t1".into(), control: Control::Steer { text: "faster".into() } } });
    f.rig.until_event("the steer", |e| e.kind == "task.steered");
    up(&mut s, Up::Intent { intent: Intent::Control { task: "t1".into(), control: Control::Kill } });
    assert_eq!(f.rig.finished("t1").state, State::Stopped { why: j4n_core::task::Why::Killed });
    assert!(up(&mut s, Up::Resize { cols: 80, rows: 24 }).is_empty());
    assert!(s.up(Up::Quit).1);
    let (downs, quit) = s.up(Up::Intent { intent: Intent::Say { text: ":quit".into(), with: Vec::new() } });
    assert!(quit && downs.is_empty());
}

#[test]
fn a_refused_order_is_told_in_the_chat_not_dropped() {
    let f = fx(vec![chatty(&["a"])], |_| {});
    let mut s = serve(&f);
    up(&mut s, Up::Intent { intent: Intent::Control { task: "t9".into(), control: Control::Abort } });
    assert!(f.notes().last().unwrap().contains("t9"), "{:?}", f.notes());
    up(&mut s, Up::Intent { intent: Intent::Answer { ask: "t9".into(), reply: Reply::Yes } });
    assert_eq!(f.notes().len(), 2);
}

// ---- a client on threads ----

#[test]
fn a_client_on_threads_gets_the_answer_and_the_channel_closes_when_it_leaves() {
    let f = fx(vec![chatty(&["pong"])], |_| {});
    let (tx, rx) = f.front.connect();
    tx.send(Up::Hello { api: j4n_proto::API }).unwrap();
    tx.send(Up::Watch { bind: "chat:main".into() }).unwrap();
    tx.send(Up::Intent { intent: Intent::Say { text: "ping".into(), with: Vec::new() } }).unwrap();
    let mut got = Vec::new();
    let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < end {
        if let Ok(d) = rx.recv_timeout(std::time::Duration::from_millis(50)) {
            got.push(d);
        }
        if lines(&got, "chat:main").iter().any(|l| l.role == Role::Doer && l.text == "pong") {
            break;
        }
    }
    let chat = lines(&got, "chat:main");
    assert!(chat.iter().any(|l| l.role == Role::Doer && l.text == "pong"), "{got:?}");
    assert!(matches!(got[0], Down::Hello { .. }) && matches!(got[1], Down::Keys { .. }) && matches!(got[2], Down::Theme { .. }) && matches!(got[3], Down::Tree { .. }));
    tx.send(Up::Quit).unwrap();
    for _ in 0..200 {
        if let Err(std::sync::mpsc::RecvTimeoutError::Disconnected) = rx.recv_timeout(std::time::Duration::from_millis(50)) {
            return;
        }
    }
    panic!("the channel stayed open after Quit");
}

#[test]
fn two_clients_see_the_same_chat() {
    let f = fx(vec![chatty(&["shared"])], |_| {});
    let (a_tx, a_rx) = f.front.connect();
    let (b_tx, b_rx) = f.front.connect();
    for tx in [&a_tx, &b_tx] {
        tx.send(Up::Watch { bind: "chat:main".into() }).unwrap();
    }
    a_tx.send(Up::Intent { intent: Intent::Say { text: "hello".into(), with: Vec::new() } }).unwrap();
    let drain = |rx: &std::sync::mpsc::Receiver<Down>| {
        let mut got = Vec::new();
        let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while std::time::Instant::now() < end && !lines(&got, "chat:main").iter().any(|l| l.text == "shared") {
            if let Ok(d) = rx.recv_timeout(std::time::Duration::from_millis(50)) {
                got.push(d);
            }
        }
        texts(&lines(&got, "chat:main"))
    };
    let (a, b) = (drain(&a_rx), drain(&b_rx));
    assert!(a.contains(&(Role::Doer, "shared".into())));
    assert_eq!(a, b);
}

// ---- a client that falls behind ----

#[test]
fn a_resync_sends_everything_that_is_watched_as_it_stands_now() {
    let asker = Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::Ask(approval("a", "make build")), Beat::Hold, end("x")]]);
    let f = fx(vec![asker], |_| {});
    let mut s = serve(&f);
    for b in ["chat:main", "tasks:all", "bar:status", "asks:open", "form:thread"] {
        watch(&mut s, b);
    }
    // events happen that the client is never told of
    let Said::Started { task } = f.say("build") else { panic!() };
    f.rig.until(&task, "waiting", |r| matches!(r.state, State::Waiting { .. }));
    let downs = s.resync();
    assert_eq!(lines(&downs, "chat:main").iter().filter(|l| l.role == Role::You).count(), 1);
    assert_eq!(rows(&downs, "tasks:all").len(), 1);
    assert!(rows(&downs, "bar:status")[0].cells[0].text.contains("waits for you"));
    assert!(downs.iter().any(|d| matches!(d, Down::Asks { open } if open.len() == 1)), "the question is sent again though it was not new: {downs:?}");
    assert_eq!(rows(&downs, "form:thread").len(), 4);
    assert!(s.resync().iter().any(|d| matches!(d, Down::Asks { open } if open.len() == 1)), "and again");
    assert!(serve(&f).resync().is_empty(), "a client that watches nothing is sent nothing");
}

#[test]
fn a_client_with_room_for_one_event_still_ends_up_with_the_whole_chat() {
    let f = fx(vec![chatty(&["a"])], |_| {});
    let (tx, rx) = f.front.connect_with(1);
    tx.send(Up::Watch { bind: "chat:main".into() }).unwrap();
    for i in 0..300 {
        f.front.note("main", &format!("note {i}"), "dim");
    }
    let mut got = Vec::new();
    let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while std::time::Instant::now() < end {
        if let Ok(d) = rx.recv_timeout(std::time::Duration::from_millis(50)) {
            got.push(d);
        }
        if lines(&got, "chat:main").iter().any(|l| l.text == "note 299") {
            // a replay may still be on its way after the last note: give it a moment, then count
            std::thread::sleep(std::time::Duration::from_millis(300));
            while let Ok(d) = rx.try_recv() {
                got.push(d);
            }
            break;
        }
    }
    let shown = lines(&got, "chat:main");
    let notes: Vec<&str> = shown.iter().filter(|l| l.role == Role::Notice).map(|l| l.text.as_str()).collect();
    assert_eq!(notes.len(), 300, "every note, once, whatever was lost on the way");
    assert_eq!((notes[0], notes[299]), ("note 0", "note 299"));
}

// ---- what a refresh is for ----

fn ev_of(seq: u64, kind: &str, task: Option<&str>, data: Value) -> Ev {
    Ev { seq, at_ms: 1, kind: kind.into(), task: task.map(Into::into), thread: Some("main".into()), by: Actor::Core, cause: None, class: Default::default(), data }
}

#[test]
fn a_verdict_of_the_gate_and_a_count_of_the_spend_refresh_the_rows_that_show_them() {
    let f = fx(vec![chatty(&["a"])], |_| {});
    let mut s = serve(&f);
    for b in ["bar:status", "tasks:all"] {
        watch(&mut s, b);
    }
    for kind in ["gate.verdict", "run.metrics"] {
        let downs = s.ev(&ev_of(9, kind, Some("t1"), json!({})));
        assert!(downs.iter().any(|d| matches!(d, Down::Rows { bind, .. } if bind == "bar:status")), "{kind}: {downs:?}");
    }
    assert!(s.ev(&ev_of(10, "run.tool_call", Some("t1"), json!({}))).is_empty(), "a tool call changes no row");
}

#[test]
fn the_log_a_client_is_shown_on_coming_is_a_window_at_the_end_of_it() {
    let f = fx(vec![chatty(&["a"])], |_| {});
    for i in 0..620 {
        f.front.note("main", &format!("note {i}"), "dim");
    }
    let mut s = serve(&f);
    let downs = watch(&mut s, "log:all");
    let [Down::Rows { rows, total, from, .. }] = &downs[..] else { panic!("{downs:?}") };
    assert_eq!((rows.len(), *total >= 620, *from + 200), (200, true, *total));
    assert!(rows[199].cells[1].text.contains("note 619"), "{:?}", rows[199]);
}

#[test]
fn after_a_replay_words_still_streaming_are_sent_whole_for_the_client_has_none_to_add_to() {
    let f = fx(vec![chatty(&["a"])], |_| {});
    let mut s = serve(&f);
    watch(&mut s, "chat:main");
    let mut delta = ev_of(0, "run.delta", Some("t1"), json!({ "text": "wor" }));
    delta.at_ms = 1;
    let said = |downs: &[Down]| texts(&lines(downs, "chat:main")).into_iter().filter(|(r, _)| *r == Role::Doer).map(|(_, t)| t).collect::<Vec<_>>();
    assert_eq!(said(&s.ev(&delta)), ["wor"]);
    s.resync();
    assert_eq!(said(&s.ev(&delta)), ["wor"], "the rows are whole: the client adds to nothing");
}

// ---- the screen from data ----

fn rows_of(downs: &[Down], bind: &str) -> Vec<j4n_proto::Row> {
    downs.iter().find_map(|d| if let Down::Rows { bind: b, rows, .. } = d { (b == bind).then(|| rows.clone()) } else { None }).unwrap_or_else(|| panic!("no rows for {bind}: {downs:?}"))
}

#[test]
fn the_trust_view_the_palette_and_the_packs_are_bindings_the_server_answers_and_a_view_nobody_has_is_a_toast() {
    let f = fx(vec![chatty(&["a"])], |_| {});
    let mut s = serve(&f);
    assert!(rows_of(&watch(&mut s, "view:trust"), "view:trust").is_empty(), "no answerer has a record yet");
    let palette = rows_of(&watch(&mut s, "commands:all"), "commands:all");
    assert!(palette.iter().any(|r| r.key == "trust" && r.cells[0].text == ":trust"), "{palette:?}");
    assert!(rows_of(&watch(&mut s, "packs:all"), "packs:all").is_empty(), "a front made with no harness lists no packs");
    let downs = watch(&mut s, "view:nothing-like-it");
    assert!(matches!(&downs[..], [Down::Toast { tone: Tone::Warn, .. }]), "{downs:?}");
}

#[test]
fn a_command_that_opens_a_page_sends_the_screen_there_and_so_does_the_intent_to_open_one() {
    let f = fx(vec![chatty(&["a"])], |_| {});
    let mut s = serve(&f);
    let (downs, quit) = s.up(Up::Intent { intent: Intent::Say { text: ":trust".into(), with: Vec::new() } });
    assert!(!quit);
    assert_eq!(downs, [Down::Focus { node: "trust".into() }]);
    let (downs, _) = s.up(Up::Intent { intent: Intent::Open { node: "packs".into() } });
    assert_eq!(downs, [Down::Focus { node: "packs".into() }]);
}

#[test]
fn the_binders_guess_is_answered_with_nothing_and_the_strip_says_no_doer_yet_until_the_core_has_more() {
    let f = fx(vec![chatty(&["a"])], |_| {});
    let mut s = serve(&f);
    let downs = watch(&mut s, j4n_proto::COMPOSER_BIND);
    assert!(matches!(&downs[..], [Down::Value { bind: b, value: Value::Null, .. }] if b == j4n_proto::COMPOSER_BIND), "{downs:?}");
    let downs = watch(&mut s, j4n_proto::COMPOSER_STRIP);
    assert!(matches!(&downs[..], [Down::Value { bind: b, value, .. }] if b == j4n_proto::COMPOSER_STRIP && value["route"] == "no doer yet"), "{downs:?}");
}

#[test]
fn the_bar_and_the_tasks_say_what_a_task_took_of_the_plan_with_its_list_cost_beside_it() {
    let f = fx(vec![Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::Spend(0.5), say("a"), end("a")]])], |_| {});
    let mut s = serve(&f);
    let task = match f.say("go") {
        Said::Started { task } => task,
        other => panic!("{other:?}"),
    };
    f.rig.finished(&task);
    let bar = |s: &mut Serve| today(rows(&watch(s, "bar:status"), "bar:status"));
    let tasks = |s: &mut Serve| rows(&watch(s, "tasks:all"), "tasks:all")[0].cells.last().unwrap().text.clone();
    assert_eq!((bar(&mut s), tasks(&mut s)), ("$0.5000".into(), "$0.5000".into()), "a task on no plan has one number");
    // the plan cost of the next request, as the loop writes it beside the list cost
    f.rig.log.append(j4n_core::log::New::of("run.metrics", Actor::Core, json!({ "usd": 0.25, "plan_usd": 0.006, "source": "assumed" })).task(&task)).unwrap();
    assert_eq!(bar(&mut s), "$0.5060 plan · $0.7500 list");
    assert_eq!(tasks(&mut s), "$0.5060 plan · $0.7500 list");
}

#[test]
fn cost_show_says_which_money_the_bar_the_tasks_and_the_chat_have() {
    for (word, in_bar, in_chat) in [("plan", "$0.5060 plan", "$0.51 plan"), ("list", "$0.7500 list", "$0.75 list"), ("both", "$0.5060 plan · $0.7500 list", "$0.51 plan · $0.75 list")] {
        let mut screen = j4n_core::front::screen::Screen::builtin();
        screen.set = j4n_core::setting::Settings::from_pairs([("cost.show", j4n_core::value::Value::text(word))]).tree();
        let f = fx_screened(vec![Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::Spend(0.5), say("a"), end("a")]])], screen, |_| {});
        let mut s = serve(&f);
        let task = match f.say("go") {
            Said::Started { task } => task,
            other => panic!("{other:?}"),
        };
        f.rig.finished(&task);
        f.rig.log.append(j4n_core::log::New::of("run.metrics", Actor::Core, json!({ "usd": 0.25, "plan_usd": 0.006, "source": "assumed" })).task(&task).thread("main")).unwrap();
        let bar = today(rows(&watch(&mut s, "bar:status"), "bar:status"));
        let task_cell = rows(&watch(&mut s, "tasks:all"), "tasks:all")[0].cells.last().unwrap().text.clone();
        assert_eq!((bar, task_cell), (in_bar.to_string(), in_bar.to_string()), "{word}");
        let downs = watch(&mut s, "chat:main");
        let chat = rows(&downs, "chat:main");
        assert!(chat.iter().any(|r| r.cells.last().is_some_and(|c| c.text == in_chat)), "{word}: {:?}", chat.iter().map(|r| r.cells.iter().map(|c| c.text.clone()).collect::<Vec<_>>()).collect::<Vec<_>>());
    }
}

#[test]
fn on_a_plan_the_first_money_word_is_the_share_of_the_window_and_an_unpriced_plan_says_how_to_set_it() {
    let f = fx(vec![Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::Spend(0.5), say("a"), end("a")]])], |_| {});
    let mut s = serve(&f);
    let task = match f.say("go") {
        Said::Started { task } => task,
        other => panic!("{other:?}"),
    };
    f.rig.finished(&task);
    let bar = |s: &mut Serve| today(rows(&watch(s, "bar:status"), "bar:status"));
    let tasks = |s: &mut Serve| rows(&watch(s, "tasks:all"), "tasks:all")[0].clone();
    // a request on a plan with no price: the share is the number, the list cost beside it, and the way to set the price under the row
    let how = "no price: Library › Plans, ⏎ on claude sets it";
    f.rig.log.append(j4n_core::log::New::of("run.metrics", Actor::Core, json!({ "usd": 0.25, "share": 0.008, "share_of": "5h", "source": "assumed", "plan": how })).task(&task)).unwrap();
    assert_eq!(bar(&mut s), "0.8% of 5h · $0.7500 list");
    let row = tasks(&mut s);
    assert_eq!(row.cells.last().unwrap().text, "0.8% of 5h · $0.7500 list");
    assert!(row.detail.iter().any(|c| c.text == how), "{:?}", row.detail);
    // once it has a price the plan cost follows the share, and the list cost is last
    f.rig.log.append(j4n_core::log::New::of("run.metrics", Actor::Core, json!({ "usd": 0.25, "plan_usd": 0.006, "share": 0.008, "share_of": "5h", "source": "assumed" })).task(&task)).unwrap();
    assert_eq!(bar(&mut s), "1.6% of 5h · $0.7560 plan · $1.0000 list");
    assert_eq!(tasks(&mut s).cells.last().unwrap().text, "1.6% of 5h · $0.7560 plan · $1.0000 list");
}

struct Follows(std::sync::atomic::AtomicUsize);

impl j4n_core::front::Upstream for Follows {
    fn show(&self) -> String {
        "claude: running 2.1.310 · latest 2.1.312 · tested —".into()
    }

    fn once(&self) -> Result<String, String> {
        if self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 { Ok("asked 1 source(s), 0 quiet".into()) } else { Err("`watch.upstream` is off".into()) }
    }

    fn plan(&self, id: &str) -> Result<String, String> {
        if id == "claude-code:2.1.312" { Ok("proposed as p7".into()) } else { Err(format!("no release is listed as `{id}`")) }
    }
}

#[test]
fn watch_shows_where_the_tools_stand_and_asks_only_with_once() {
    let notes = |f: &Fx| f.rig.of_kind("chat.note").into_iter().map(|e| e.data["text"].as_str().unwrap_or_default().to_string()).collect::<Vec<_>>();
    let follows = Arc::new(Follows(Default::default()));
    let up = follows.clone();
    let f = fx(vec![Scripted::new("alpha", vec![vec![end("a")]])], move |c| c.upstream = Some(up));
    assert_eq!(f.say(":watch"), Said::Command("watch".into()));
    assert_eq!(follows.0.load(std::sync::atomic::Ordering::SeqCst), 0, "showing asks nothing");
    assert_eq!(f.say(":watch once"), Said::Command("watch".into()));
    assert_eq!(follows.0.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(notes(&f), ["claude: running 2.1.310 · latest 2.1.312 · tested —", "asked 1 source(s), 0 quiet"]);
    // a refusal is said, not hidden
    assert!(matches!(f.say(":watch once"), Said::Refused(why) if why.contains("watch.upstream")));
    // "plan it" names the release it is about, and says what came of it
    assert_eq!(f.say(":watch plan claude-code:2.1.312"), Said::Command("watch".into()));
    assert_eq!(notes(&f).last().map(String::as_str), Some("proposed as p7"));
    assert!(matches!(f.say(":watch plan nothing:1"), Said::Refused(why) if why.contains("no release is listed")));
    assert!(matches!(f.say(":watch plan"), Said::Refused(why) if why.contains("`p` on a row of Grow › Updates")));
    // nothing wired: it says so
    let bare = fx(vec![Scripted::new("alpha", vec![vec![end("a")]])], |_| {});
    assert!(matches!(bare.say(":watch"), Said::Refused(why) if why.contains("nothing follows")));
}

#[test]
fn keys_row_is_the_hints_of_the_pack_in_order_and_the_tree_ends_with_its_bar() {
    let f = fx(vec![chatty(&["a"])], |_| {});
    let mut s = serve(&f);
    let downs = up(&mut s, Up::Hello { api: j4n_proto::API });
    let Some(Down::Tree { node }) = downs.iter().find(|d| matches!(d, Down::Tree { .. })) else { panic!("{downs:?}") };
    let last = node.children.last().and_then(|c| c.field.as_ref());
    assert_eq!(last.map(|f| (f.kind, f.bind.as_str())), Some((j4n_proto::FieldKind::Bar, j4n_proto::KEYS_ROW)));
    let got = watch(&mut s, j4n_proto::KEYS_ROW);
    let hints = rows(&got, j4n_proto::KEYS_ROW);
    // the completion's hints (`ghost`, `step`) come first and show only while they apply
    let first = hints.iter().find(|r| !r.key.starts_with("ghost/") && !r.key.starts_with("step/"));
    assert_eq!(first.map(|r| (r.key.as_str(), r.cells[0].text.as_str(), r.cells[1].text.as_str())), Some(("global/tabs", "⌥1–9", "tabs")));
    assert!(hints.len() > 20 && hints.iter().all(|r| r.cells.len() == 2), "{hints:?}");
}

#[test]
fn the_keys_of_a_question_stand_before_the_lines_and_one_dash_and_one_word_for_a_key_everywhere() {
    let f = fx(vec![chatty(&["a"])], |_| {});
    let mut s = serve(&f);
    let got = watch(&mut s, j4n_proto::KEYS_ROW);
    let hints = rows(&got, j4n_proto::KEYS_ROW);
    let at = |key: &str| hints.iter().position(|r| r.key == key).unwrap_or_else(|| panic!("{key}: {hints:?}"));
    let word = |key: &str| format!("{} {}", hints[at(key)].cells[0].text, hints[at(key)].cells[1].text);
    // the en dash the sheet and the box use, and the word the Look page's row says for ⏎
    assert_eq!(word("ask/pick"), "1–9 answer");
    assert_eq!(word("page:look/open"), "⏎ change");
    // on a narrow screen the row drops from the right, so what a question needs stands before the line's own keys
    for ask in ["ask/pick", "ask/recommended", "ask/why", "ask/later"] {
        for chat in ["chat/send", "chat/newline", "chat/history", "chat/lane"] {
            assert!(at(ask) < at(chat), "{ask} before {chat}");
        }
    }
    assert!(at("global/tabs") < at("ask/pick") && at("chat/scroll") < at("global/bar"), "the tabs lead, and the key that turns the row over is the line's last: {hints:?}");
    // with the mouse the screen's, the terminal's own selection needs ⇧, and the row says so
    assert_eq!(word("chat/select"), "⇧drag select");
    assert!(at("global/bar") < at("chat/select"), "it is the last, so that on a narrow screen it goes first and no other key is dropped for it: {hints:?}");
}

// ---- who did the work is the doer and the model that ran ----

#[test]
fn the_tasks_tab_and_the_strip_name_the_model_that_ran_and_not_the_one_asked_for() {
    let f = fx(vec![Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::Served("m-5-5-full", "anthropic"), Beat::Spend(0.5), say("a"), end("a")]])], |c| c.model = Some("m".into()));
    let mut s = serve(&f);
    let tasks = |s: &mut Serve| rows(&watch(s, "tasks:all"), "tasks:all").iter().map(|r| r.cells[2].text.clone()).collect::<Vec<_>>();
    let strip = j4n_proto::COMPOSER_STRIP;
    let route = |s: &mut Serve| watch(s, strip).iter().rev().find_map(|d| match d { Down::Value { bind, value, .. } if bind == strip => value.get("route").cloned(), _ => None });
    f.turn("go");
    assert_eq!(tasks(&mut s), ["alpha · m 5.5-full"], "the Tasks tab");
    assert_eq!(route(&mut s), Some(json!("alpha · m 5.5-full")), "the composer strip says who the line goes to, and the bar does not");
    assert_eq!(rows(&watch(&mut s, "bar:status"), "bar:status")[0].cells[0].text, "idle");
    // the pin is what was asked for and stays in the task's spec
    assert_eq!(f.rig.tasks.book().rows()[0].spec.tune.model.as_deref(), Some("m"));
}

// ---- the numbers behind the charts ----

/// The value of the last `Value` for a binding.
fn value_of(downs: &[Down], bind: &str) -> Value {
    downs.iter().rev().find_map(|d| match d {
        Down::Value { bind: b, value, .. } if b == bind => Some(value.clone()),
        _ => None,
    }).unwrap_or(Value::Null)
}

#[test]
fn the_spend_chart_is_the_dollars_and_tokens_of_each_day_and_grows_with_the_events() {
    let f = fx(vec![Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::Spend(0.5), say("a"), end("a")]])], |_| {});
    let mut s = serve(&f);
    assert_eq!(value_of(&watch(&mut s, "chart:spend"), "chart:spend"), Value::Null, "nothing spent, nothing to draw");
    let task = match f.say("go") {
        Said::Started { task } => task,
        other => panic!("{other:?}"),
    };
    f.rig.finished(&task);
    let last = |v: &Value, name: &str| v["series"][name].as_array().and_then(|l| l.last()).and_then(Value::as_f64);
    let v = value_of(&feed(&mut s, &f, 0), "chart:spend");
    assert!((last(&v, "usd").unwrap_or(0.0) - 0.5).abs() < 1e-9, "{v}");
    let before = f.rig.log.head();
    f.rig.log.append(j4n_core::log::New::of("run.metrics", Actor::Core, json!({ "usd": 0.25, "tokens_in": 100, "tokens_out": 20 })).task(&task)).unwrap();
    let downs = feed(&mut s, &f, before);
    let v = value_of(&downs, "chart:spend");
    assert!((last(&v, "usd").unwrap_or(0.0) - 0.75).abs() < 1e-9 && last(&v, "tokens").is_some_and(|t| t >= 120.0), "an event of the day adds to the day: {v}");
    assert_eq!(v["x"].as_array().map(Vec::len), v["series"]["usd"].as_array().map(Vec::len), "a label for each point");
}

#[test]
fn the_bench_chart_is_the_share_of_the_last_five_runs_that_passed_after_each_run() {
    let f = fx(vec![Scripted::new("alpha", vec![vec![end("a")]])], |_| {});
    let mut s = serve(&f);
    assert_eq!(value_of(&watch(&mut s, "chart:bench"), "chart:bench"), Value::Null);
    for pass in [true, true, false, true] {
        f.rig.log.append(j4n_core::log::New::of("bench.case", Actor::Core, json!({ "case": "c", "doer": "d", "pass": pass }))).unwrap();
    }
    let v = value_of(&feed(&mut s, &f, 0), "chart:bench");
    assert_eq!(v["series"]["pass"], json!([1.0, 1.0, 2.0 / 3.0, 0.75]));
    assert_eq!(value_of(&watch(&mut s, "chart:lanes"), "chart:lanes"), Value::Null, "no lanes, no bars");
}
