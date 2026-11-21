//! The project's glossary and the sentence of `brief.plain_answer`. The scanner
//! reads table rows of the docs; the glossary is sent as the bind `terms:` and read again when a doc changed; `:term` says a row; and the
//! sentence is in the contract of the turn that writes the answer, with the switch on, and nowhere else.

use crate::front_cases::{Fx, chatty, fx_placed_on};
use crate::scripted::{Beat, Scripted, end, say};
use crate::world::Scratch;
use j4n_core::brief::PLAIN;
use j4n_core::doer::Caps;
use j4n_core::front::screen::Screen;
use j4n_core::front::serve::Serve;
use j4n_core::front::terms::{Terms, is_id, scan, sentence};
use j4n_core::setting::Settings;
use j4n_core::value::Value as Set;
use j4n_proto::{Down, Intent, Json, TERMS, Up};
use serde_json::json;
use std::sync::Arc;

const DOC: &str = "\
# A doc

| # | Decision | Status |
|---|---|---|
| ADR-12 | **The menu is data.** It is read from a file, one entry a line. | built |
| `BUG-7` | a fenced `code` example | open |
| **UX3** | The next key opens the next page; the rest follows. | built |
| D-10–D-11 | spare | — |
| Name | not an id | x |
| ABCD1 | four capitals are not an id | x |

| Id | Found by | Symptom |
|---|---|---|
| B13 | a test | a list carries codes a reader cannot read |
| B14 | same | the bar has one mark for both. And more words. |

```
| D-9 | inside a fence | skipped |
```

text | D-8 | not a table row
";

#[test]
fn the_scanner_takes_table_rows_whose_first_cell_is_an_id_and_the_first_sentence_of_the_next_cell() {
    let got: Vec<(String, String, String)> = scan("docs/A.md", DOC).into_iter().map(|(id, t)| (id, t.words, t.at)).collect();
    let row = |id: &str, words: &str, at: &str| (id.to_string(), words.to_string(), at.to_string());
    assert_eq!(
        got,
        [
            row("ADR-12", "The menu is data", "docs/A.md:5"),
            row("BUG-7", "a fenced code example", "docs/A.md:6"),
            row("UX3", "The next key opens the next page; the r…", "docs/A.md:7"),
            row("B13", "a list carries codes a reader cannot re…", "docs/A.md:14"),
            row("B14", "the bar has one mark for both", "docs/A.md:15"),
        ],
        "bold and code marks are stripped; a range, a non-id and a row in a fence are skipped; a column that says who found it is passed over"
    );
}

#[test]
fn an_id_is_one_to_three_capitals_a_dash_and_digits() {
    for yes in ["ADR-12", "BUG-7", "UX3", "S300", "XY12", "B01"] {
        assert!(is_id(yes), "{yes}");
    }
    for no in ["D-", "12", "ABCD1", "adr-12", "ADR-12a", "D-10–D-11", "", "Name", "A-"] {
        assert!(!is_id(no), "{no}");
    }
}

#[test]
fn the_words_are_the_first_sentence_cut_to_forty_columns() {
    assert_eq!(sentence("**Orchestration is single + offload.** `single` stays the default"), "Orchestration is single + offload", "a bold opening is the title");
    assert_eq!(sentence("Reads e.g. the card. Then more"), "Reads e.g. the card", "an abbreviation does not end it");
    assert_eq!(sentence("see [the plan](docs/PLAN.md) first"), "see the plan first");
    let long = sentence("one two three four five six seven eight nine ten eleven twelve");
    assert_eq!((long.chars().count(), long.ends_with('…')), (40, true), "{long}");
    assert_eq!(sentence("short"), "short");
}

fn root_with(docs: &[(&str, &str)]) -> Scratch {
    let root = Scratch::new("terms");
    for (rel, text) in docs {
        root.put(rel, text);
    }
    root
}

#[test]
fn the_glossary_is_read_once_and_again_only_when_a_doc_changed() {
    let root = root_with(&[("docs/B.md", "| Id | What |\n|---|---|\n| D-1 | later doc |\n"), ("docs/A.md", "| Id | What |\n|---|---|\n| D-1 | earlier doc |\n| D-2 | second |\n"), ("README.md", "no table")]);
    let mut t = Terms::default();
    assert!(t.refresh(root.path()), "the first read is a change");
    assert_eq!(t.json(), json!({ "D-1": "earlier doc", "D-2": "second" }), "the first doc by name wins an id both define");
    assert!(!t.refresh(root.path()), "nothing changed, nothing read again");
    root.put("docs/B.md", "| Id | What |\n|---|---|\n| D-1 | later doc |\n| D-3 | a new row, and longer |\n");
    assert!(t.refresh(root.path()), "a doc changed");
    assert_eq!(t.get("D-3").map(|t| t.words.as_str()), Some("a new row, and longer"));
    // not a doc: other folders and files are not read
    root.put("src/notes.md", "| Id | What |\n|---|---|\n| D-9 | in src |\n");
    assert!(t.refresh(root.path()) || t.get("D-9").is_none());
    assert!(t.get("D-9").is_none());
}

/// An id that two docs define takes the words of `docs/dev/DECISIONS.md` or `docs/dev/BUGS.md`, whatever the order of the names; the other docs keep the order of their names.
#[test]
fn an_id_two_docs_define_is_read_from_the_decisions_and_the_bugs_first() {
    let row = |id: &str, words: &str| format!("| Id | What |\n|---|---|\n| {id} | {words} |\n");
    let root = root_with(&[
        ("docs/ACCOUNTS.md", &row("S1", "the plan says")),
        ("docs/TESTING.md", &(row("S1", "the test table says") + "| S2 | tests only |\n| S3 | tests second |\n")),
        ("docs/dev/DECISIONS.md", &(row("S1", "the decision says") + "| D-7 | decided |\n")),
        ("docs/dev/BUGS.md", &(row("S1", "the bug says") + "| D-7 | bugs second | \n| J9 | a bug |\n")),
        ("docs/AAA.md", &(row("S3", "first by name") + "| J9 | a plan row |\n")),
    ]);
    let mut t = Terms::default();
    t.refresh(root.path());
    let words = |id: &str| t.get(id).map(|t| (t.words.clone(), t.at.split(':').next().unwrap().to_string()));
    assert_eq!(words("S1"), Some(("the decision says".into(), "docs/dev/DECISIONS.md".into())), "DECISIONS.md before every other doc");
    assert_eq!(words("D-7"), Some(("decided".into(), "docs/dev/DECISIONS.md".into())), "DECISIONS.md before BUGS.md");
    assert_eq!(words("J9"), Some(("a bug".into(), "docs/dev/BUGS.md".into())), "BUGS.md before a doc that sorts earlier");
    assert_eq!(words("S3"), Some(("first by name".into(), "docs/AAA.md".into())), "ids that neither doc defines: the first by name");
    assert_eq!(words("S2").map(|w| w.1), Some("docs/TESTING.md".into()));
}

fn with_terms(docs: &[(&str, &str)], read_terms: bool) -> (Fx, Scratch) {
    let root = root_with(docs);
    let mut screen = Screen::builtin();
    screen.set = Settings::from_pairs([("read.terms", Set::Bool(read_terms))]).tree();
    let path = root.path().to_path_buf();
    let f = fx_placed_on(vec![chatty("claude", &["ok"])], None, Vec::new(), screen, move |c| c.root = path);
    (f, root)
}

fn terms_of(downs: &[Down]) -> Json {
    downs.iter().find_map(|d| match d {
        Down::Value { bind, value, .. } if bind == TERMS => Some(value.clone()),
        _ => None,
    }).expect("the glossary was sent")
}

#[test]
fn the_glossary_is_sent_when_watched_with_the_setting_on_and_not_read_with_it_off() {
    let docs = [("docs/A.md", "| Id | What |\n|---|---|\n| D-1 | the first |\n")];
    let (f, _root) = with_terms(&docs, true);
    let mut s = Serve::new(f.front.clone());
    let (downs, _) = s.up(Up::Watch { bind: TERMS.into() });
    assert_eq!(terms_of(&downs), json!({ "D-1": "the first" }));
    let (f, _root) = with_terms(&docs, false);
    let mut s = Serve::new(f.front.clone());
    assert_eq!(terms_of(&s.up(Up::Watch { bind: TERMS.into() }).0), Json::Null, "read.terms off: nothing is read or sent");
}

#[test]
fn a_doc_that_changed_while_a_task_worked_is_sent_again_when_the_task_settles() {
    let (f, root) = with_terms(&[("docs/A.md", "| Id | What |\n|---|---|\n| D-1 | the first |\n")], true);
    let mut s = Serve::new(f.front.clone());
    s.up(Up::Watch { bind: TERMS.into() });
    root.put("docs/A.md", "| Id | What |\n|---|---|\n| D-1 | the first |\n| D-2 | added while it worked |\n");
    f.turn("say ok");
    let downs: Vec<Down> = f.rig.log.read(0).flat_map(|e| s.ev(&e)).collect();
    assert_eq!(terms_of(&downs), json!({ "D-1": "the first", "D-2": "added while it worked" }));
}

#[test]
fn the_command_term_says_the_row_and_where_it_stands() {
    let (f, _root) = with_terms(&[("docs/A.md", "| Id | What |\n|---|---|\n| D-1 | **Title of one.** The rest of the row. |\n")], true);
    let mut s = Serve::new(f.front.clone());
    s.up(Up::Intent { intent: Intent::Run { command: "term".into(), args: vec!["D-1".into(), "D-77".into()] } });
    let notes: Vec<String> = f.rig.log.read(0).filter(|e| e.kind == "chat.note").map(|e| e.data.to_string()).collect();
    let all = notes.join("\n");
    assert!(all.contains("D-1") && all.contains("Title of one") && all.contains("docs/A.md:3") && all.contains("The rest of the row"), "{all}");
    assert!(all.contains("D-77: no table row"), "an id nothing defines is said so: {all}");
}

// ---- brief.plain_answer ----

fn screen(on: bool) -> Screen {
    let mut s = Screen::builtin();
    s.set = Settings::from_pairs([("brief.plain_answer", Set::Bool(on))]).tree();
    s
}

fn caps(brief: bool) -> Caps {
    Caps { live: true, resume: true, brief, ..Caps::default() }
}

fn doer(id: &str, brief: bool, n: usize) -> Arc<Scripted> {
    Scripted::with_caps(id, caps(brief), (0..n).map(|_| vec![Beat::Session(format!("{id}-s")), say("ok"), end("ok")]).collect())
}

/// The inputs of the turns of a thread, the first two lines.
fn inputs(on: bool, brief: bool) -> Vec<Json> {
    let f = fx_placed_on(vec![doer("claude", brief, 2)], None, Vec::new(), screen(on), |c| c.doer = Some("claude".into()));
    f.turn("fix the parser in src");
    f.turn("and add a test for it");
    f.tasks().iter().map(|r| r.spec.input.clone()).collect()
}

#[test]
fn with_the_switch_on_the_answer_turns_contract_ends_with_the_sentence_and_with_it_off_the_brief_is_what_it_was() {
    let on = inputs(true, true);
    let off = inputs(false, true);
    for (i, turn) in on.iter().enumerate() {
        let brief = turn["brief"].as_str().unwrap();
        assert!(brief.contains(PLAIN), "turn {i}: {brief}");
        assert_eq!(turn["plain_answer"], true);
        assert_eq!(brief.replace(&format!(" {PLAIN}"), ""), off[i]["brief"].as_str().unwrap(), "the sentence is all that differs");
    }
    assert!(off.iter().all(|t| !t["brief"].as_str().unwrap().contains(PLAIN) && t.get("plain_answer").is_none()));
}

#[test]
fn a_doer_that_takes_no_brief_is_only_marked_for_the_wrapper_that_tells_it() {
    let on = inputs(true, false);
    assert!(on.iter().all(|t| t["plain_answer"] == true && t.get("brief").is_none()), "{on:?}");
    assert!(inputs(false, false).iter().all(|t| t.get("plain_answer").is_none()));
}
