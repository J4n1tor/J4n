use j4n_shape::check::{self, Facts, Limits, Turn, fold, keepables, parse};
use serde_json::json;

fn facts() -> Facts {
    Facts {
        line: "fix the parser thing and make tests pass".into(),
        turns: vec![
            Turn { id: "t1".into(), you: "trailing commas in args break the parser".into(), answer_head: "The bug is in lexer.rs where commas end a token.".into(), files: vec!["crates/cli/src/args.rs".into()] },
            Turn { id: "t2".into(), you: "don't change the public API".into(), answer_head: String::new(), files: vec![] },
        ],
        files_changed: vec!["crates/cli/src/args.rs".into()],
        repo: vec!["crates/cli/src/args.rs".into(), "crates/cli/src/lexer.rs".into()],
        checks: vec!["cargo test -p cli".into()],
        root: std::env::temp_dir().join("j4n-shape-no-such-root"),
    }
}

const LIMITS: Limits = Limits { max_tokens: 120, rewrite: false };

fn reasons(c: &check::Checked) -> Vec<String> {
    c.dropped.iter().map(|d| d.reason.clone()).collect()
}

#[test]
fn the_answer_is_the_first_object_in_it_with_one_of_the_three_verdicts() {
    assert!(parse("```json\n{\"verdict\":\"pass\"}\n```").is_ok(), "a fence round it is left off");
    assert!(parse("Sure! {\"verdict\":\"add\",\"files\":[]} Hope that helps.").is_ok());
    assert!(parse("{\"verdict\":\"ask\"}").is_ok());
    for bad in ["no json here", "{\"verdict\":\"maybe\"}", "{\"files\":[]}", "{broken", "} {"] {
        assert!(parse(bad).is_err(), "{bad}");
    }
    assert!(parse("{\"verdict\":\"pass\"").unwrap_err().contains("not JSON"));
    assert!(parse("{\"verdict\":\"maybe\"}").unwrap_err().contains("`maybe`"));
    assert!(parse("{\"x\":1}").unwrap_err().contains("missing"));
}

#[test]
fn a_quote_is_found_in_the_turn_whatever_the_case_and_the_spacing() {
    assert_eq!(fold("  Trailing   COMMAS\nin args "), "trailing commas in args");
    let reply = json!({ "verdict": "add", "refs": [
        { "phrase": "the parser thing", "turn": "t1", "quote": "TRAILING commas  in args" },
        { "phrase": "it", "turn": "t1", "quote": "commas end a token" },
        { "phrase": "it", "turn": "t1", "quote": "nested quotes" },
        { "phrase": "it", "turn": "t9", "quote": "trailing commas" },
        { "phrase": "it", "turn": "t1", "quote": "" }
    ]});
    let c = check::check(&reply, &facts(), LIMITS);
    assert_eq!(c.items.iter().map(|i| i.key.as_str()).collect::<Vec<_>>(), ["ref:1", "ref:2"], "the operator's line and the answer head both count: {:?}", c.dropped);
    assert_eq!(reasons(&c), ["the quote is not in that turn", "no such turn", "the quote is not in that turn"]);
    assert_eq!(c.items[0].text, "\"the parser thing\" = t1 › TRAILING commas  in args");
    assert_eq!((c.items[0].source.as_str(), c.items[0].proof), ("t1", "quote"));
}

#[test]
fn a_file_is_in_the_repo_map_or_inside_the_project_and_a_path_out_of_it_is_neither() {
    let dir = std::env::temp_dir().join(format!("j4n-shape-files-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/new.rs"), "x").unwrap();
    let f = Facts { root: dir.clone(), ..facts() };
    let reply = json!({ "verdict": "add", "files": [
        { "path": "crates/cli/src/args.rs", "why": "t1" }, { "path": "src/new.rs", "why": "t1" }, { "path": "src/nope.rs", "why": "t1" },
        { "path": "../etc/passwd", "why": "t1" }, { "path": "/etc/passwd", "why": "t1" }, { "path": "", "why": "t1" }
    ]});
    let c = check::check(&reply, &f, LIMITS);
    assert_eq!(c.items.iter().map(|i| i.text.as_str()).collect::<Vec<_>>(), ["crates/cli/src/args.rs", "src/new.rs"]);
    assert_eq!(c.dropped.len(), 4);
    assert!(reasons(&c).iter().all(|r| r == "no such file"));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn a_done_command_is_a_check_of_the_project_or_a_command_that_passed() {
    let ok = check::check(&json!({ "verdict": "add", "done": { "cmd": "cargo test -p cli", "why": "check" } }), &facts(), LIMITS);
    assert_eq!((ok.items[0].kind, ok.items[0].text.as_str(), ok.items[0].key.as_str()), ("done", "cargo test -p cli", "done"));
    for cmd in ["cargo test", "rm -rf /", ""] {
        let c = check::check(&json!({ "verdict": "add", "done": { "cmd": cmd, "why": "check" } }), &facts(), LIMITS);
        assert!(c.items.is_empty() && c.dropped.len() == 1, "{cmd}");
        assert!(c.dropped[0].reason.starts_with("not a check of the project"));
    }
}

#[test]
fn a_note_needs_a_source_and_a_limit_comes_only_from_the_operator() {
    let notes = |v: serde_json::Value| check::check(&json!({ "verdict": "add", "notes": v }), &facts(), LIMITS);
    // no source at all, a source that does not hold the quote, a file as the source
    let c = notes(json!([
        { "text": "the lexer ends tokens at commas", "why": "", "quote": "" },
        { "text": "the lexer ends tokens at commas", "why": "t1", "quote": "nothing like it" },
        { "text": "the lexer ends tokens at commas", "why": "t1", "quote": "commas end a token" },
        { "text": "this file was changed", "why": "crates/cli/src/args.rs", "quote": "" }
    ]));
    assert_eq!(c.items.len(), 2);
    assert_eq!(reasons(&c), ["no source", "no source"]);
    // a limit: the quote must say it, in a line the operator typed
    let c = notes(json!([
        { "text": "keep the public API unchanged", "why": "t2", "quote": "don't change the public API" },
        { "text": "only touch args.rs", "why": "t1", "quote": "trailing commas in args" },
        { "text": "never edit lexer.rs", "why": "t1", "quote": "commas end a token" },
        { "text": "do not rename anything", "why": "t1", "quote": "trailing commas in args" }
    ]));
    assert_eq!(c.items.iter().map(|i| i.text.as_str()).collect::<Vec<_>>(), ["keep the public API unchanged"]);
    assert_eq!(reasons(&c), ["a limit the operator did not state"; 3]);
    // a word that merely holds a limit word is no limit
    let c = notes(json!([{ "text": "the lexer is only used by args.rs", "why": "t1", "quote": "commas end a token" }]));
    assert_eq!(c.dropped.len(), 1, "`only` is a limit word wherever it stands: a fact is worded without it");
}

#[test]
fn shouting_budget_words_and_claims_of_not_being_able_are_not_shown() {
    let notes = |text: &str| check::check(&json!({ "verdict": "add", "notes": [{ "text": text, "why": "t1", "quote": "commas end a token" }] }), &facts(), LIMITS);
    for (text, why) in [
        ("MAKE it work", "emphasis the operator did not type"),
        ("this is IMPORTANT", "emphasis the operator did not type"),
        ("fix it!!", "emphasis the operator did not type"),
        ("MUST pass", "emphasis the operator did not type"),
        ("keep the cost low", "words about `cost` are not for a model"),
        ("you cannot touch it", "words about `cannot` are not for a model"),
        ("two\nlines", "not one short line"),
        ("", "empty"),
    ] {
        let c = notes(text);
        assert!(c.items.is_empty(), "{text:?}");
        assert_eq!(reasons(&c), [why], "{text:?}");
    }
    assert_eq!(notes(&"x".repeat(201)).dropped[0].reason, "not one short line");
    assert_eq!(notes(&"x".repeat(200)).items.len(), 1);
    // capitals the operator typed themselves are theirs; a short word in capitals is not a shout
    let mut f = facts();
    f.line = "update the README and the API docs".into();
    let c = check::check(&json!({ "verdict": "add", "notes": [{ "text": "the README lists the API", "why": "t1", "quote": "commas end a token" }, { "text": "an ID is a key", "why": "t1", "quote": "commas end a token" }] }), &f, LIMITS);
    assert_eq!(c.items.len(), 2, "{:?}", c.dropped);
}

#[test]
fn a_question_has_two_to_four_different_options_of_up_to_eighty_characters() {
    let q = |options: serde_json::Value| check::check(&json!({ "verdict": "ask", "question": { "text": "Which one?", "options": options } }), &facts(), LIMITS);
    let c = q(json!(["the commas", "the quotes"]));
    assert_eq!((c.items[0].kind, c.items[0].options.clone()), ("question", vec!["the commas".to_string(), "the quotes".to_string()]));
    assert_eq!(q(json!(["a", "b", "c", "d"])).items.len(), 1);
    for bad in [json!(["only one"]), json!(["a", "b", "c", "d", "e"]), json!(["same", "same"]), json!(["a", ""]), json!(["a", "b".repeat(81)]), json!(["a", 3])] {
        let c = q(bad.clone());
        assert!(c.items.is_empty(), "{bad}");
    }
    assert_eq!(q(json!(["a", "b".repeat(80)])).items.len(), 1);
    assert_eq!(q(json!(["a"])).dropped[0].reason, "a question has 2 to 4 different options of up to 80 characters");
}

#[test]
fn there_are_at_most_three_refs_five_files_one_done_three_notes_and_one_question() {
    let many = |n: usize, mk: &dyn Fn(usize) -> serde_json::Value| (0..n).map(mk).collect::<Vec<_>>();
    let mut f = facts();
    f.repo = (0..9).map(|i| format!("f{i}.rs")).collect();
    let reply = json!({ "verdict": "add",
        "refs": many(5, &|_| json!({ "phrase": "it", "turn": "t1", "quote": "trailing commas" })),
        "files": many(7, &|i| json!({ "path": format!("f{i}.rs"), "why": "t1" })),
        "notes": many(5, &|i| json!({ "text": format!("note {i}"), "why": "t1", "quote": "trailing commas" }))
    });
    let c = check::check(&reply, &f, Limits { max_tokens: 10_000, rewrite: false });
    let n = |k: &str| c.items.iter().filter(|i| i.kind == k).count();
    assert_eq!((n("ref"), n("file"), n("note")), (3, 5, 3));
    assert_eq!(c.dropped.len(), 2 + 2 + 2);
    assert!(reasons(&c).contains(&"more than 3 refs".to_string()) && reasons(&c).contains(&"more than 5 files".to_string()) && reasons(&c).contains(&"more than 3 notes".to_string()));
}

#[test]
fn the_notes_are_cut_to_the_size_allowed_from_the_end_notes_first_and_files_last() {
    let reply = json!({ "verdict": "add",
        "refs": [{ "phrase": "it", "turn": "t1", "quote": "trailing commas" }],
        "files": [{ "path": "crates/cli/src/args.rs", "why": "t1" }],
        "done": { "cmd": "cargo test -p cli", "why": "check" },
        "notes": [{ "text": "a note that takes some room to say what it says", "why": "t1", "quote": "trailing commas" }]
    });
    let full = check::check(&reply, &facts(), Limits { max_tokens: 400, rewrite: false });
    assert_eq!(full.items.len(), 4);
    let size: usize = full.items.iter().map(|i| i.text.chars().count().div_ceil(4)).sum();
    let cut = check::check(&reply, &facts(), Limits { max_tokens: size - 1, rewrite: false });
    assert_eq!(cut.items.iter().map(|i| i.kind).collect::<Vec<_>>(), ["ref", "file", "done"], "the note went first");
    assert_eq!(cut.dropped[0].reason, "over the size of the notes");
    let exact = check::check(&reply, &facts(), Limits { max_tokens: size, rewrite: false });
    assert_eq!(exact.items.len(), 4, "notes of exactly the size allowed stay");
    let tiny = check::check(&reply, &facts(), Limits { max_tokens: 6, rewrite: false });
    assert_eq!(tiny.items.iter().map(|i| i.kind).collect::<Vec<_>>(), ["file"], "the files go last: {:?}", tiny.items);
    assert!(tiny.items.iter().map(|i| i.text.chars().count().div_ceil(4)).sum::<usize>() <= 6);
}

#[test]
fn what_a_rewrite_must_keep_is_every_path_identifier_number_and_quoted_text() {
    let mut k = keepables("fix `parse_args` in crates/cli/src/args.rs, line 42, and \"trailing comma\" in HttpClient or a.b");
    k.sort();
    assert_eq!(k, ["42", "HttpClient", "a.b", "crates/cli/src/args.rs", "parse_args", "trailing comma"]);
    assert!(keepables("make it faster please").is_empty());
    assert_eq!(keepables("bump to 1.2.3."), ["1.2.3"], "a full stop after it is not part of it");
    assert_eq!(keepables("read the_file"), ["the_file"]);
    assert!(keepables("an unbalanced \"quote here").is_empty() || keepables("an unbalanced \"quote here") == ["quote"], "an open quote makes no string");
}

#[test]
fn a_rewrite_is_kept_only_when_asked_for_and_only_if_it_loses_nothing() {
    let mut f = facts();
    f.line = "fix crates/cli/src/args.rs parse errors in 3 places".into();
    let r = |rewrite: &str, want: bool| check::check(&json!({ "verdict": "add", "rewrite": rewrite }), &f, Limits { max_tokens: 120, rewrite: want });
    assert_eq!(r("Fix the parse errors in crates/cli/src/args.rs, in 3 places.", true).items[0].key, "rewrite");
    let lost = r("Fix the parse errors in 3 places.", true);
    assert!(lost.items.is_empty() && lost.dropped[0].reason == "the rewrite lost crates/cli/src/args.rs", "{:?}", lost.dropped);
    assert!(r("Fix the parse errors in crates/cli/src/args.rs.", true).dropped[0].reason.contains("3"));
    assert_eq!(r("Fix crates/cli/src/args.rs in 3 places.", false).dropped[0].reason, "a rewrite was not asked for");
    assert_eq!(r("FIX crates/cli/src/args.rs in 3 places NOW!!", true).dropped[0].reason, "emphasis the operator did not type");
}
