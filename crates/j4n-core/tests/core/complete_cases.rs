//! What the core completes (`complete:`): the screen asks with a window whose find is a slot and a token, and is sent `~for` (the find
//! as asked) and the candidates of the slot, best first and whole. The slots of a command's words are its pack rows; a file name the
//! guard would not let a model read is never offered. Plain words are completed by whole lines: the project's own lines by how often and
//! how lately, the newest answer's next steps, the phrases' wordings; an empty line by its next line. A line that held a key never comes back.

use crate::front_cases::{Fx, chatty, fx, fx_screened};
use crate::world::{Scratch, layer, refused};
use j4n_core::front::models::ModelLists;
use j4n_core::front::phrase::Phrases;
use j4n_core::front::screen::Screen;
use j4n_core::change::Actor;
use j4n_core::front::Said;
use j4n_core::front::serve::Serve;
use j4n_core::log::New;
use j4n_core::setting::Settings;
use j4n_core::table::{LayerKind, Tables};
use j4n_proto::{COMPLETE, Down, Row, Up};
use std::collections::BTreeMap;
use std::sync::Arc;

fn serve(f: &Fx) -> Serve {
    let mut s = Serve::new(f.front.clone());
    let (downs, _) = s.up(Up::Watch { bind: COMPLETE.into() });
    assert!(answer(downs).is_some(), "watching `complete:` is answered with its rows");
    s
}

fn answer(downs: Vec<Down>) -> Option<Vec<Row>> {
    downs.into_iter().find_map(|d| match d {
        Down::Rows { bind, rows, .. } if bind == COMPLETE => Some(rows),
        _ => None,
    })
}

/// The rows the core sends for `find`.
fn ask(s: &mut Serve, find: &str) -> Vec<Row> {
    let (downs, _) = s.up(Up::Window { bind: COMPLETE.into(), from: 0, count: 0, sort: String::new(), find: find.into() });
    let rows = answer(downs).unwrap_or_else(|| panic!("`{find}` was answered"));
    assert_eq!(rows[0].key, "~for", "{rows:?}");
    rows
}

/// The slot `~for` says, and each candidate with what stands beside it.
fn said(rows: &[Row]) -> (String, Vec<(String, String)>) {
    let cell = |r: &Row, i: usize| r.cells.get(i).map(|c| c.text.clone()).unwrap_or_default();
    (cell(&rows[0], 1), rows[1..].iter().map(|r| (cell(r, 0), cell(r, 1))).collect())
}

fn texts(rows: &[Row]) -> Vec<String> {
    said(rows).1.into_iter().map(|(t, _)| t).collect()
}

/// What a screen keeps of an answer: the candidates when `~for` is the draft it holds now, else nothing.
fn kept<'a>(rows: &'a [Row], draft: &str) -> &'a [Row] {
    if rows[0].cells[0].text == draft { &rows[1..] } else { &[] }
}

struct Kept;

impl ModelLists for Kept {
    fn models(&self, _: &str) -> Option<Vec<String>> {
        panic!("a key of the completion never reads a doer's models from the provider")
    }

    fn kept(&self, doer: &str) -> Vec<String> {
        match doer {
            "claude" => vec!["haiku-5".into(), "sonnet-5.5".into(), "opus-5".into()],
            _ => Vec::new(),
        }
    }
}

#[test]
fn the_window_of_complete_names_its_draft_and_an_older_answer_is_dropped() {
    let f = fx(vec![chatty("claude", &[]), chatty("codex", &[])], |c| c.models = Arc::new(Kept));
    let mut s = serve(&f);
    // the find is the slot and the token, not a word the rows hold: the candidates are made from it and are not cut by it
    let rows = ask(&mut s, "setting look.ke");
    assert_eq!(rows[0].cells[0].text, "setting look.ke");
    assert!(texts(&rows).contains(&"look.keys_row".to_string()), "{rows:?}");
    let older = ask(&mut s, "line :switch c");
    let newer = ask(&mut s, "line :switch cl");
    assert_eq!(texts(&older), ["codex", "claude"], "the shorter first");
    assert_eq!(texts(&newer), ["claude"]);
    // the screen holds `:switch cl` now: the answer for `:switch c` names its own draft and is dropped
    assert!(kept(&older, "line :switch cl").is_empty());
    assert_eq!(kept(&newer, "line :switch cl").len(), 1);
    // the core keeps the newest window only: sent again, it is the answer for the newest draft
    let again = answer(s.resync()).expect("sent again");
    assert_eq!(again[0].cells[0].text, "line :switch cl");
}

#[test]
fn setting_ids_and_their_values() {
    let f = fx(vec![chatty("claude", &[])], |_| {});
    let mut s = serve(&f);
    let (slot, found) = said(&ask(&mut s, "setting look.ke"));
    assert_eq!(slot, "setting");
    assert_eq!(found.first(), Some(&("look.keys_row".to_string(), "true".to_string())), "a setting is offered with its value now: {found:?}");
    // a word of the id, not only its start
    assert!(texts(&ask(&mut s, "setting gauge")).contains(&"look.gauge".to_string()));
    // a setting's values: its words, or a switch's two, the one it holds now said
    let (_, words) = said(&ask(&mut s, "value:look.gauge "));
    assert_eq!(words, [("auto".to_string(), "now".to_string()), ("line".to_string(), String::new()), ("slice".to_string(), String::new())]);
    assert_eq!(texts(&ask(&mut s, "value:look.keys_row ")), ["true", "false"]);
    assert_eq!(texts(&ask(&mut s, "value:look.keys_row f")), ["false"]);
    // the look's parts with the word each holds now, then a part's words
    let (slot, parts) = said(&ask(&mut s, "line :look pal"));
    assert_eq!((slot.as_str(), parts.first().map(|(t, _)| t.as_str())), ("word:look", Some("palette")));
    assert!(!parts[0].1.is_empty(), "the palette's word now: {parts:?}");
    let (_, words) = said(&ask(&mut s, "line :look frame "));
    assert_eq!(words.iter().map(|(w, _)| w.as_str()).collect::<Vec<_>>(), ["hang", "airy", "pipes", "round", "gutter"], "the shorter first, then as the part lists them");
    assert_eq!(words.iter().filter(|(_, now)| now == "now").count(), 1, "{words:?}");
    // the pages, with their titles
    let (_, pages) = said(&ask(&mut s, "page cha"));
    assert_eq!(pages.first().map(|(id, _)| id.as_str()), Some("chat"), "{pages:?}");
}

#[test]
fn a_file_the_guard_denies_is_never_a_candidate() {
    let (project, away) = (Scratch::new("complete-files"), Scratch::new("complete-away"));
    for rel in [
        "src/main.rs",
        "src/lexer/token.rs",
        ".env",
        ".env.example",
        "deploy/id_rsa",
        "deploy/id_rsa.pub",
        "certs/server.pem",
        "config/client_secret.json",
        "config/api-key.txt",
        "password.txt",
        "passwd",
        "apikey.txt",
        "db.password",
        "conf/secret.json",
        "conf/app.secret.json",
        "docs/auth.md",
        ".ssh/config",
        ".git/config",
        ".j4n/keys/own",
        ".github/workflows/ci.yml",
    ] {
        project.put(rel, "");
    }
    away.put("notes.txt", "");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(away.path(), project.join("away")).unwrap();
        std::os::unix::fs::symlink(project.join("src/main.rs"), project.join("alias.rs")).unwrap();
    }
    let root = project.path().to_path_buf();
    let f = fx(vec![chatty("claude", &[])], move |c| c.root = root);
    let mut s = serve(&f);
    let mut all = texts(&ask(&mut s, "file "));
    all.sort();
    assert_eq!(all, [".env.example", ".github/workflows/ci.yml", "deploy/id_rsa.pub", "docs/auth.md", "src/lexer/token.rs", "src/main.rs"]);
    assert_eq!(texts(&ask(&mut s, "file src/m")), ["src/main.rs"]);
    // a name typed outside the project's folder is not looked up
    for find in ["file ../", "file ~/", "file /etc/"] {
        assert!(texts(&ask(&mut s, find)).is_empty(), "{find}");
    }
}

#[test]
fn the_slots_of_a_command_are_its_rows() {
    let f = fx(vec![chatty("claude", &[]), chatty("codex", &[])], |c| c.models = Arc::new(Kept));
    let mut s = serve(&f);
    let slot = |s: &mut Serve, find: &str| said(&ask(s, find));
    // `:switch` (a phrase): a doer, then that doer's models as kept
    assert_eq!(slot(&mut s, "line :switch "), ("doer".to_string(), vec![("codex".to_string(), String::new()), ("claude".to_string(), String::new())]));
    assert_eq!(slot(&mut s, "line :switch claude so").0, "model:claude");
    assert_eq!(texts(&ask(&mut s, "line :switch claude so")), ["sonnet-5.5"]);
    assert!(texts(&ask(&mut s, "line :switch codex ")).is_empty());
    // with the connectors, a doer's models are the ones its row names and then the ones read for it, none read on a key
    let tables = crate::front_cases::base();
    let screen = Screen::load(tables.clone(), &Settings::load(&tables).0, Vec::new(), &BTreeMap::new()).0;
    let f = fx_screened(vec![chatty("claude", &[])], screen, |c| c.models = Arc::new(Kept));
    assert_eq!(texts(&ask(&mut serve(&f), "line :switch claude ")), ["opus", "haiku", "sonnet", "opus-5", "haiku-5", "sonnet-5.5"]);
    // `:pack` (a command): its own words, then a pack
    assert_eq!(texts(&ask(&mut s, "line :pack ")), ["on", "off", "ready"]);
    assert_eq!(slot(&mut s, "line :pack on ").0, "pack");
    // a slot the screen holds is `~for` alone: the chats
    assert_eq!(slot(&mut s, "line :archive "), ("chat".to_string(), Vec::new()));
    // a command with no `complete` completes nothing after its name
    assert_eq!(slot(&mut s, "line :status "), (String::new(), Vec::new()));
    // a slot that is none of the kinds is refused where the row is read
    let (tables, problems) = Tables::load(vec![layer(LayerKind::Pack, "x", &[("phrases/x.toml", "api = 1\n[[phrase]]\nid = \"tint\"\nwords = [\":tint\"]\ncommand = \"status\"\ncomplete = [\"colour\"]\n")])]);
    assert!(refused(&problems).is_empty(), "{problems:?}");
    let (phrases, problems) = Phrases::load(&tables);
    assert!(refused(&problems).iter().any(|p| p.contains("`colour` is not a slot")), "{problems:?}");
    assert!(phrases.slots(":tint").is_empty());
}

#[test]
fn with_complete_on_off_the_answer_says_so_and_holds_no_word() {
    let on = ask(&mut serve(&fx(vec![chatty("claude", &[])], |_| {})), "line :pack ");
    assert_eq!(on[0].data.as_ref().and_then(|d| d.get("on")), Some(&j4n_proto::json!(true)), "the switch is on as shipped");
    assert!(on.len() > 1);
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/base");
    let (mut base, _) = j4n_core::table::Layer::from_dir(LayerKind::Pack, "base-data", &dir);
    base.files.retain(|f| !f.path.starts_with("kinds/") && f.path != "pack.toml");
    let off = layer(LayerKind::User, "you", &[("settings/mine.toml", "api = 1\n[[setting]]\nid = \"complete.on\"\nvalue = false\n")]);
    let (tables, problems) = Tables::load(vec![base, off]);
    assert!(problems.is_empty(), "{problems:?}");
    let screen = Screen::load(tables.clone(), &Settings::load(&tables).0, Vec::new(), &BTreeMap::new()).0;
    let rows = ask(&mut serve(&fx_screened(vec![chatty("claude", &[])], screen, |_| {})), "line :pack ");
    assert_eq!(rows[0].data.as_ref().and_then(|d| d.get("on")), Some(&j4n_proto::json!(false)), "`~for` tells the screen the switch is off");
    assert_eq!(rows.len(), 1, "and no word is sent: {rows:?}");
}


#[test]
fn the_command_slot_is_the_phrases_words_and_the_palettes() {
    // `:switch`, `:status` and `:stop` are phrases, not rows of the palette: the core answers the slot with both
    let tables = crate::front_cases::base();
    let screen = Screen::load(tables.clone(), &Settings::load(&tables).0, Vec::new(), &BTreeMap::new()).0;
    let mut s = serve(&fx_screened(vec![chatty("claude", &[])], screen, |_| {}));
    let (slot, words) = said(&ask(&mut s, "line :s"));
    assert_eq!(slot, "command");
    let words: Vec<&str> = words.iter().map(|(t, _)| t.as_str()).collect();
    for w in [":switch", ":status", ":stop"] {
        assert!(words.contains(&w), "{w}: {words:?}");
    }
    assert_eq!(texts(&ask(&mut s, "line :sw")).first().map(String::as_str), Some(":switch"));
    // the palette's own commands and flows are there too, each once
    let pa = texts(&ask(&mut s, "line :pa"));
    assert_eq!(pa.iter().filter(|w| *w == ":pack").count(), 1, "{pa:?}");
    assert!(texts(&ask(&mut s, "line /us")).contains(&"/usage".to_string()));
    // a word after the command is still its slot's, not a command
    assert_eq!(said(&ask(&mut s, "line :switch ")).0, "doer");
}

// ---- whole lines: the newest answer's steps, the project's lines, and what never comes back ----

const DAY: u64 = 86_400_000;

/// A line the operator typed in `thread`, written to the Log now.
fn typed(f: &Fx, line: &str) {
    f.rig.log.append(New::of("chat.say", Actor::Operator, j4n_proto::json!({ "text": line })).thread("main")).expect("written");
}

/// Each candidate and its score.
fn scored(rows: &[Row]) -> Vec<(String, i64)> {
    rows[1..].iter().map(|r| (r.cells[0].text.clone(), r.data.as_ref().and_then(|d| d.get("score")).and_then(j4n_proto::Json::as_i64).unwrap_or(0))).collect()
}

#[test]
fn the_newest_answers_next_steps_are_sentences() {
    let first = "The login page works again. Proposed next steps: 1. Deploy the fix to the login page. 2. Add a test for the empty password.";
    let second = "Done.\n\nNext:\n- run the tests again\n- Tag the release v0.2\n\nNotes:\n- the cache was cold\n- Check the log once more.";
    let f = fx(vec![chatty("claude", &[first, second])], |c| c.doer = Some("claude".into()));
    let mut s = serve(&f);
    let Said::Started { task } = f.say("fix the login page") else { panic!("the line starts a task") };
    f.rig.until(&task, "settled", |r| r.state.is_terminal());
    // the step whose first word is typed, whole, cut at its first sentence and as a line to type
    let rows = ask(&mut s, "line dep");
    assert_eq!(said(&rows).0, "sentence");
    assert_eq!(texts(&rows), ["deploy the fix to the login page"], "{rows:?}");
    assert_eq!(texts(&ask(&mut s, "line add a ")), ["add a test for the empty password"], "every word typed is the step's");
    assert!(texts(&ask(&mut s, "line add the")).is_empty(), "a word that is not the step's next one matches nothing");
    assert_eq!(texts(&ask(&mut s, "line depoly")), ["deploy the fix to the login page"], "one typo in the last word");
    assert_eq!(scored(&ask(&mut s, "line dep"))[0].1, 300 + 25, "a start, and a step's weight");
    // an empty line is the next line: the steps in the answer's order, then the lines typed
    let rows = ask(&mut s, "line ");
    assert_eq!(said(&rows).0, "next");
    assert_eq!(texts(&rows), ["deploy the fix to the login page", "add a test for the empty password", "fix the login page"]);
    // the newest answer's only: a list under `Next:` and an item elsewhere that starts with a verb, not one that does not
    let Said::Started { task } = f.say("deploy the fix to the login page") else { panic!("the line starts a task") };
    f.rig.until(&task, "settled", |r| r.state.is_terminal());
    // the two lines were typed once each at the same time: the shorter first
    assert_eq!(texts(&ask(&mut s, "line ")), ["run the tests again", "tag the release v0.2", "check the log once more", "fix the login page", "deploy the fix to the login page"]);
    assert!(texts(&ask(&mut s, "line add")).is_empty(), "the older answer's steps are gone");
}

#[test]
fn the_projects_lines_rank_by_how_often_and_how_lately() {
    let f = fx(vec![chatty("claude", &[])], |_| {});
    let mut s = serve(&f);
    for _ in 0..3 {
        typed(&f, "deploy the old build");
    }
    f.rig.clock.advance(60 * DAY);
    for _ in 0..3 {
        typed(&f, "deploy the docs site to the mirror");
    }
    typed(&f, "deploy the fix");
    typed(&f, "review the plan");
    // typed three times beats typed once, though it is longer; typed today beats typed as often two months ago
    let rows = ask(&mut s, "line dep");
    assert_eq!(texts(&rows), ["deploy the docs site to the mirror", "deploy the fix", "deploy the old build"], "{:?}", scored(&rows));
    let scores: Vec<i64> = scored(&rows).into_iter().map(|(_, n)| n).collect();
    assert!(scores[0] > scores[1] && scores[1] > scores[2] && scores[2] >= 300, "{scores:?}");
    assert!(scores[0] <= 330, "the lines add 30 at the most: {scores:?}");
    // two weeks on, the line typed once again today comes before the one typed three times then
    f.rig.clock.advance(14 * DAY);
    typed(&f, "deploy the fix");
    assert_eq!(texts(&ask(&mut s, "line dep")), ["deploy the fix", "deploy the docs site to the mirror", "deploy the old build"]);
    // the line as typed is not offered, and the words typed must start the line
    assert_eq!(texts(&ask(&mut s, "line deploy the fix")), Vec::<String>::new());
    assert_eq!(texts(&ask(&mut s, "line the")), Vec::<String>::new(), "`the` starts none of them");
    // on an empty line, the lines by how often and how lately alone
    assert_eq!(texts(&ask(&mut s, "line ")).first().map(String::as_str), Some("deploy the fix"));
    // the phrases' wordings are whole lines too (a greeting's is not: it stands for no command)
    let rows = ask(&mut s, "line how much is");
    assert_eq!(scored(&rows), [("how much is left".to_string(), 300 + 20)]);
    assert!(texts(&ask(&mut s, "line hello th")).is_empty());
}

#[test]
fn a_line_that_held_a_key_never_comes_back_as_a_sentence() {
    const FAKE: &str = "Qm4tH7vR2kX9pL0wZ8cN3bF6yD1sJ5gA2eU7iO4n";
    let f = fx(vec![chatty("claude", &[])], |_| {});
    let mut s = serve(&f);
    // a key typed on a `:key` line: the Log keeps `:key …` of it, and nothing of the key
    f.front.say("main", &format!(":key {FAKE}"));
    let logged = format!("{:?}", f.rig.events());
    assert!(logged.contains(":key …") && !logged.contains(&FAKE[..8]), "{logged}");
    // a key of no known shape sent as text on its own line, and a line whose key the redactor took out before the Log kept it
    typed(&f, FAKE);
    typed(&f, &format!("deploy with OPENAI_API_KEY={FAKE}"));
    typed(&f, "deploy the fix");
    let logged = format!("{:?}", f.rig.events());
    assert!(logged.contains("deploy with OPENAI_API_KEY=«"), "the redactor took the key out: {logged}");
    assert_eq!(texts(&ask(&mut s, "line ")), ["deploy the fix"], "the next line is none of them");
    assert_eq!(texts(&ask(&mut s, "line dep")), ["deploy the fix"]);
    assert!(texts(&ask(&mut s, "line Qm4t")).is_empty());
    assert!(texts(&ask(&mut s, "line :ke")).iter().all(|t| !t.contains('…')), "nor as a command");
    // a find that is one key-like word is answered with `~for` alone
    assert_eq!(ask(&mut s, &format!("line {FAKE}")).len(), 1);
}

// ---- the Decider's order: after a pause, one ask at a time, kept, capped, redacted ----

const FIX: &str = "deploy the fix to the login page";
const DOCS: &str = "deploy the docs site to the mirror";
const DEMO: &str = "deploy the demo build";

/// A front whose Decider puts the second candidate first (else the first), the project's three lines, and a screen that watches `complete:`.
fn deciding_over(f: Fx) -> (Fx, Serve) {
    for l in [FIX, DOCS, DEMO] {
        typed(&f, l);
    }
    let s = serve(&f);
    (f, s)
}

fn deciding() -> (Fx, Serve) {
    deciding_over(crate::front_cases::fx_deciding(vec![chatty("claude", &[])], Some("complete=2,1"), |_| {}))
}

/// The questions the Decider was put at the point `complete`.
fn asked(f: &Fx) -> Vec<j4n_core::answer::Question> {
    f.asked.lock().unwrap().iter().filter(|q| q.point == "complete").cloned().collect()
}

/// Tick the screen's clock until `ok`, failing after a while.
fn until(s: &mut Serve, what: &str, ok: impl Fn() -> bool) {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !ok() {
        assert!(std::time::Instant::now() < end, "never {what}");
        s.tick();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// Tick until the window of `complete:` is sent again: an ask came back.
fn sent_again(s: &mut Serve) -> Vec<Row> {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if let Some(rows) = answer(s.tick()) {
            return rows;
        }
        assert!(std::time::Instant::now() < end, "the Decider's order was never sent");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// Ticks for a while, so a question a thread was about to put has been put: what is not asked by then is not asked.
fn settle(s: &mut Serve) {
    for _ in 0..10 {
        s.tick();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn flag(r: &Row, key: &str) -> Option<j4n_proto::Json> {
    r.data.as_ref().and_then(|d| d.get(key)).cloned()
}

#[test]
fn three_failed_asks_rest_the_completion_and_leave_the_deciders_breaker_closed() {
    let f = crate::front_cases::fx_deciding(vec![chatty("claude", &["Blue light scatters most."])], Some("complete=fail;intake=answer"), |_| {});
    let (f, mut s) = deciding_over(f);
    for (i, find) in ["line dep", "line deploy", "line deploy the d"].into_iter().enumerate() {
        ask(&mut s, find);
        f.rig.clock.advance(600);
        until(&mut s, "the ask said", || f.rig.of_kind("complete.ranked").len() == i + 1);
    }
    // the Decider's breaker, which every point shares, is closed: the next line's intake is still put to it
    let Said::Started { task } = f.say("why is the sky blue?") else { panic!("{}", f.rig.dump()) };
    f.rig.until(&task, "settled", |r| r.state.is_terminal());
    let by: Vec<_> = f.rig.of_kind("intake.decided").iter().map(|e| e.data["by"].clone()).collect();
    assert_eq!(by, [j4n_proto::json!("decider:jev")], "intake is still asked of the Decider: {}", f.rig.dump());
    let failed: Vec<_> = f.rig.of_kind("complete.ranked").iter().map(|e| (e.data["failed"].clone(), e.data["cached"].clone())).collect();
    assert_eq!(failed, vec![(j4n_proto::json!(true), j4n_proto::json!(false)); 3], "each ask failed and none is kept");
    // the completion rests ten minutes: a pause is not asked, then it is again
    assert_eq!(texts(&ask(&mut s, "line deploy the")).len(), 3);
    f.rig.clock.advance(600);
    settle(&mut s);
    assert_eq!(asked(&f).len(), 3, "it rests after three failures");
    f.rig.clock.advance(600_000);
    assert_eq!(texts(&ask(&mut s, "line depl")), [DEMO, FIX, DOCS]);
    f.rig.clock.advance(600);
    until(&mut s, "asked after the rest", || asked(&f).len() == 4);
}

#[test]
fn a_pause_asks_once() {
    let (f, mut s) = deciding();
    let rows = ask(&mut s, "line dep");
    assert_eq!(texts(&rows), [DEMO, FIX, DOCS], "code's order at once");
    f.rig.clock.advance(599);
    settle(&mut s);
    assert!(asked(&f).is_empty(), "nothing is asked before the find stood still 600 ms");
    f.rig.clock.advance(1);
    let rows = sent_again(&mut s);
    assert_eq!(asked(&f).len(), 1);
    // the Decider put the second first: its confidence × 60 is added, and the row says it made the order
    assert_eq!(texts(&rows), [FIX, DEMO, DOCS], "{:?}", scored(&rows));
    assert_eq!((flag(&rows[1], "made"), flag(&rows[1], "decider")), (Some(j4n_proto::json!(true)), Some(j4n_proto::json!(54))));
    assert!(rows[2..].iter().all(|r| flag(r, "made").is_none()), "only the pick carries the mark");
    // more time, more ticks, the same window sent again: no new pause
    for _ in 0..5 {
        f.rig.clock.advance(1_000);
        s.tick();
    }
    assert_eq!(texts(&ask(&mut s, "line dep")), [FIX, DEMO, DOCS]);
    f.rig.clock.advance(1_000);
    settle(&mut s);
    assert_eq!(asked(&f).len(), 1, "one pause, one ask");
}

#[test]
fn a_sent_line_takes_no_pause_until_a_window_comes() {
    let (f, mut s) = deciding();
    ask(&mut s, "line deploy the d");
    s.up(Up::Intent { intent: j4n_proto::Intent::Say { text: "deploy the d".into(), with: Vec::new() } });
    // the screen asks no window for the empty line: an event sends the views again, and the sent line's find with them
    let ev = f.rig.log.append(New::of("chat.note", Actor::Core, j4n_proto::json!({ "text": "a note", "tone": "dim" })).thread("main")).expect("written");
    s.ev(&ev);
    f.rig.clock.advance(1_000);
    s.ev(&ev);
    settle(&mut s);
    assert!(asked(&f).is_empty(), "the sent line's find is not asked");
    // the next window is the line as typed again
    ask(&mut s, "line dep");
    f.rig.clock.advance(600);
    until(&mut s, "the next window asked", || asked(&f).len() == 1);
}

#[test]
fn with_the_switch_off_or_no_decider_nothing_is_asked_logged_or_said() {
    let off = {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/base");
        let (mut base, _) = j4n_core::table::Layer::from_dir(LayerKind::Pack, "base-data", &dir);
        base.files.retain(|f| !f.path.starts_with("kinds/") && f.path != "pack.toml");
        let mine = layer(LayerKind::User, "you", &[("settings/mine.toml", "api = 1\n[[setting]]\nid = \"complete.decider\"\nvalue = false\n")]);
        let (tables, problems) = Tables::load(vec![base, mine]);
        assert!(problems.is_empty(), "{problems:?}");
        let screen = Screen::load(tables.clone(), &Settings::load(&tables).0, Vec::new(), &BTreeMap::new()).0;
        crate::front_cases::fx_reflexes_in(vec![chatty("claude", &[])], Some("complete=2,1"), screen, false, |_| {})
    };
    let absent = crate::front_cases::fx_deciding(vec![chatty("claude", &[])], Some("absent"), |_| {});
    for (f, why) in [(off, "complete.decider off"), (absent, "no Decider set up")] {
        let (f, mut s) = deciding_over(f);
        let before = f.rig.events().len();
        let rows = ask(&mut s, "line dep");
        assert!(rows[1..].iter().all(|r| flag(r, "made").is_none()), "{why}: code's order");
        f.rig.clock.advance(1_000);
        settle(&mut s);
        let new: Vec<String> = f.rig.events()[before..].iter().map(|e| e.kind.clone()).collect();
        assert!(asked(&f).is_empty(), "{why}: nothing asked");
        assert!(new.iter().all(|k| !k.starts_with("complete.") && !k.starts_with("ask.") && k != "chat.note"), "{why}: nothing written: {new:?}");
    }
}

#[test]
fn typing_drops_the_ask_that_waits_and_never_blocks_a_key() {
    let (f, mut s) = deciding();
    *f.lag.lock().unwrap() = std::time::Duration::from_secs(2);
    ask(&mut s, "line dep");
    f.rig.clock.advance(400);
    ask(&mut s, "line deploy the d");
    f.rig.clock.advance(400);
    settle(&mut s);
    assert!(asked(&f).is_empty(), "the older pause was dropped and the newer has not stood 600 ms");
    f.rig.clock.advance(200);
    until(&mut s, "the newer find asked", || !asked(&f).is_empty());
    let q = asked(&f).remove(0);
    assert!(q.text.starts_with("deploy the d\ncandidates: 1: deploy the demo build; 2: deploy the docs site to the mirror\n"), "{}", q.text);
    // the Decider sleeps two seconds: every key's window is answered meanwhile, at once
    let t = std::time::Instant::now();
    for find in ["line deploy the do", "line deploy the doc", "line deploy the docs"] {
        assert_eq!(ask(&mut s, find)[0].cells[0].text, find);
        s.tick();
    }
    assert!(t.elapsed() < std::time::Duration::from_millis(1_000), "a key waited on the Decider: {:?}", t.elapsed());
    assert_eq!(asked(&f).len(), 1);
}

#[test]
fn one_ask_in_flight() {
    let (f, mut s) = deciding();
    *f.lag.lock().unwrap() = std::time::Duration::from_millis(1_500);
    ask(&mut s, "line dep");
    f.rig.clock.advance(600);
    until(&mut s, "the first asked", || asked(&f).len() == 1);
    ask(&mut s, "line deploy the d");
    f.rig.clock.advance(600);
    // the second pause is due while the first sleeps: ticks for a while, and the Decider is put no second question
    for _ in 0..20 {
        s.tick();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(asked(&f).len(), 1, "a second pause waits while the first is in flight");
    until(&mut s, "the second asked once the first came back", || asked(&f).len() == 2);
    assert!(asked(&f)[1].text.starts_with("deploy the d\n"));
    // the first one's answer is kept, but the screen is sent the window it asks for now
    until(&mut s, "both said", || f.count("complete.ranked") == 2);
}

#[test]
fn a_cached_order_asks_nothing() {
    let (f, mut s) = deciding();
    ask(&mut s, "line dep");
    f.rig.clock.advance(600);
    sent_again(&mut s);
    ask(&mut s, "line deploy the d");
    // back to the same candidates: the Decider's order at once, from the cache
    let rows = ask(&mut s, "line dep");
    assert_eq!(texts(&rows), [FIX, DEMO, DOCS]);
    assert_eq!(flag(&rows[1], "made"), Some(j4n_proto::json!(true)));
    f.rig.clock.advance(600);
    until(&mut s, "the pause on a kept order said", || f.count("complete.ranked") == 2);
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert_eq!(asked(&f).len(), 1, "a kept order asks nothing");
    let said: Vec<j4n_proto::Json> = f.rig.of_kind("complete.ranked").into_iter().map(|e| e.data["cached"].clone()).collect();
    assert_eq!(said, [j4n_proto::json!(false), j4n_proto::json!(true)], "the pause on a kept order is said, as cached");
}

/// [`deciding`] with `complete.decider_day` at `cap`.
fn capped(cap: u32) -> (Fx, Serve) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/base");
    let (mut base, _) = j4n_core::table::Layer::from_dir(LayerKind::Pack, "base-data", &dir);
    base.files.retain(|f| !f.path.starts_with("kinds/") && f.path != "pack.toml");
    let mine = format!("api = 1\n[[setting]]\nid = \"complete.decider_day\"\nvalue = {cap}\n");
    let (tables, problems) = Tables::load(vec![base, layer(LayerKind::User, "you", &[("settings/mine.toml", mine.as_str())])]);
    assert!(problems.is_empty(), "{problems:?}");
    let screen = Screen::load(tables.clone(), &Settings::load(&tables).0, Vec::new(), &BTreeMap::new()).0;
    deciding_over(crate::front_cases::fx_reflexes_in(vec![chatty("claude", &[])], Some("complete=2,1"), screen, false, |_| {}))
}

#[test]
fn the_cap_stops_the_asks_and_says_so_once_with_its_key() {
    let (f, mut s) = capped(1);
    ask(&mut s, "line dep");
    f.rig.clock.advance(600);
    sent_again(&mut s);
    let notes = || f.rig.of_kind("chat.note").into_iter().filter(|e| e.data["text"].as_str().is_some_and(|t| t.starts_with("suggestions by code only today"))).collect::<Vec<_>>();
    for find in ["line deploy the d", "line deploy"] {
        ask(&mut s, find);
        f.rig.clock.advance(600);
        settle(&mut s);
    }
    assert_eq!(asked(&f).len(), 1, "past the day's asks, code's order alone");
    let said = notes();
    assert_eq!(said.len(), 1, "said once: {said:?}");
    let note = &said[0].data;
    assert!(note["text"].as_str().unwrap().ends_with("⏎ opens Settings › Features"), "{note}");
    assert_eq!(note["act"]["intent"], j4n_proto::json!({ "intent": "open", "node": "features" }), "its key opens the row's page: {note}");
    // the next day the Decider is asked again
    f.rig.clock.advance(DAY);
    ask(&mut s, "line deploy the");
    f.rig.clock.advance(600);
    until(&mut s, "an ask the next day", || asked(&f).len() == 2);
}

#[test]
fn the_decider_is_sent_no_file_text_and_no_secret() {
    // a key of no shape the redactor knows, typed in a line and kept as a provider's key only afterwards: the Log and the task hold it
    // as it was typed, and only the redactor run over what is sent keeps it from the Decider
    const VALUE: &str = "zq81kd02vmx7tt4";
    let project = Scratch::new("complete-sent");
    project.put("notes.md", "the text of the notes, never sent");
    project.put("notes.txt", "more text, never sent");
    let root = project.path().to_path_buf();
    let f = crate::front_cases::fx_deciding(vec![chatty("claude", &["Done: the vault is deployed."])], Some("complete=2,1"), move |c| {
        c.root = root;
        c.doer = Some("claude".into());
    });
    let (f, mut s) = deciding_over(f);
    let Said::Started { task } = f.say(&format!("deploy the vault with {VALUE} now")) else { panic!("the line starts a task") };
    f.rig.until(&task, "settled", |r| r.state.is_terminal());
    assert!(format!("{:?}", f.rig.events()).contains(VALUE), "the Log holds it as typed: nothing knew it was a key");
    assert!(f.rig.log.redactor().register("OWN_KEY", VALUE));
    // the project's lines: the one that held the key is never a candidate sent, and the turn's head is redacted
    ask(&mut s, "line dep");
    f.rig.clock.advance(600);
    until(&mut s, "the lines asked", || !asked(&f).is_empty());
    let put = { let q = asked(&f).remove(0); format!("{} {:?}", q.text, q.shape) };
    assert!(put.contains(FIX) && put.contains("- you: deploy the vault with «secret:OWN_KEY» now"), "{put}");
    assert!(!put.contains(VALUE), "a secret was sent: {put}");
    // two names whose best scores are the same: the Decider is asked which, with the names only
    ask(&mut s, "file notes");
    f.rig.clock.advance(600);
    until(&mut s, "the files asked", || asked(&f).len() == 2);
    let put = { let q = asked(&f).remove(1); format!("{} {:?}", q.text, q.shape) };
    assert!(put.contains("notes.md") && put.contains("notes.txt") && put.contains("\nslot: file"), "{put}");
    assert!(!put.contains("never sent") && !put.contains(VALUE), "a file's text or a secret was sent: {put}");
}

#[test]
fn complete_ranked_holds_its_fields() {
    let (f, mut s) = deciding();
    *f.pays.lock().unwrap() = Some(j4n_core::answer::Paid { usd: Some(0.0004), tokens_in: 120, tokens_out: 1 });
    ask(&mut s, "line dep");
    f.rig.clock.advance(600);
    sent_again(&mut s);
    let ev = f.rig.of_kind("complete.ranked").remove(0);
    let d = &ev.data;
    assert_eq!((d["slot"].as_str(), d["n"].as_u64(), d["top"].as_str(), d["by"].as_str(), d["conf"].as_f64(), d["cached"].as_bool()), (Some("sentence"), Some(3), Some(FIX), Some("decider:jev"), Some(0.9), Some(false)), "{d}");
    assert!(d["ms"].is_u64() && d["usd"].as_f64().is_some_and(|u| (u - 0.0004).abs() < 1e-9), "{d}");
    // a Decider that is not sure: code's order stands, and the pause is said with no pick
    let g = crate::front_cases::fx_deciding(vec![chatty("claude", &[])], None, |_| {});
    let (g, mut t) = deciding_over(g);
    ask(&mut t, "line dep");
    g.rig.clock.advance(600);
    until(&mut t, "an unsure ask said", || g.count("complete.ranked") == 1);
    let d = g.rig.of_kind("complete.ranked").remove(0).data;
    assert!(d["top"].is_null() && d["conf"].is_null() && d["by"].as_str().is_some_and(|b| b.contains("rank")), "{d}");
    assert_eq!(texts(&ask(&mut t, "line dep")), [DEMO, FIX, DOCS], "code's order");
}

#[test]
fn the_points_probe_cases_load_in_pairs_and_are_put_as_the_front_puts_its_question() {
    let tables = crate::front_cases::base();
    let (points, problems) = j4n_core::answer::point::points(&tables);
    assert!(problems.is_empty(), "{problems:?}");
    let points = points.into_iter().map(|p| (p.id.clone(), p)).collect();
    let (set, problems) = j4n_core::answer::probe::ProbeSet::load(&tables, &points);
    assert!(problems.is_empty(), "{problems:?}");
    let cases = set.cases("complete");
    assert!(cases.len() >= 6, "{}", cases.len());
    for c in cases {
        let q = c.question();
        // the case is put as the front puts its question: the line, then the candidates, the chat and the slot
        let lines: Vec<&str> = q.text.lines().skip(1).map(|l| l.split(':').next().unwrap_or_default()).collect();
        assert_eq!(lines, ["candidates", "chat", "slot"], "{}", q.text);
        assert!(q.shape.check(&c.wanted()).is_ok(), "{}", c.id);
    }
}
