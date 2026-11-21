//! The open-ask binder through its public API: which open question a typed line answers (`src/binder.rs` is the contract).

use j4n_core::ask::{Answer, AskShape, Opt, TEXT_MAX};
use j4n_core::binder::{Bind, OpenAsk, bind};

fn options(list: &[(&str, &str)]) -> AskShape {
    AskShape::Choice {
        options: list.iter().map(|(id, label)| Opt { id: (*id).into(), label: (*label).into(), recommended: false }).collect(),
    }
}

fn numbered(n: usize) -> AskShape {
    AskShape::Choice { options: (0..n).map(|i| Opt { id: format!("o{i}"), label: "opt".into(), recommended: false }).collect() }
}

fn three() -> AskShape {
    options(&[("fast", "Fast check"), ("thorough", "Thorough check"), ("skip", "Skip the check")])
}

fn approval() -> AskShape {
    AskShape::Approval { action: "make build".into(), risk: 40 }
}

fn mk(id: &str, asker: &str, question: &str, shape: AskShape, opened_ms: u64) -> OpenAsk {
    OpenAsk { ask: id.into(), asker: asker.into(), thread: "t1".into(), question: question.into(), shape, opened_ms }
}

/// The answer when `line` is bound with `shape` as the only open ask and no focus.
fn answers(shape: AskShape, line: &str) -> Option<Answer> {
    match bind(line, &[mk("a1", "task-1", "Which one?", shape, 10)], None) {
        Bind::Answer { ask, answer } => {
            assert_eq!(ask, "a1");
            Some(answer)
        }
        Bind::NotAnswer => None,
        other => panic!("{line:?}: {other:?}"),
    }
}

fn pick(id: &str) -> Option<Answer> {
    Some(Answer::Pick { id: id.into() })
}

fn text(t: &str) -> Option<Answer> {
    Some(Answer::Text { text: t.into() })
}

fn which(b: Bind) -> Vec<String> {
    match b {
        Bind::Which(w) => w.into_iter().map(|w| w.ask).collect(),
        other => panic!("a menu was expected: {other:?}"),
    }
}

fn target(b: Bind) -> (String, Answer) {
    match b {
        Bind::Answer { ask, answer } => (ask, answer),
        other => panic!("an answer was expected: {other:?}"),
    }
}

const ASSENT: &[&str] = &[
    "yes", "y", "yep", "yeah", "yup", "sure", "ok", "okay", "do it", "go ahead", "go for it", "please do", "sounds good",
    "that's fine", "thats fine", "fine", "proceed", "confirmed", "correct", "agreed", "yes please", "yes do it", "yes go ahead",
    "ok do it", "okay do it", "please", "of course", "go on then", "do so", "make it so", "that works", "works for me", "lgtm",
];
const REFUSAL: &[&str] = &[
    "no", "n", "nope", "nah", "don't", "dont", "do not", "not now", "not today", "not right now", "not yet", "stop", "cancel",
    "never mind", "nevermind", "no thanks", "no thank you", "skip it", "leave it", "don't do it", "dont do it", "no don't",
    "no dont", "hold off", "wait",
];

// ---- yes and no ----

#[test]
fn every_assent_word_answers_yes_to_a_yes_no_question_and_to_an_approval() {
    for shape in [AskShape::YesNo, approval()] {
        for w in ASSENT {
            for line in [w.to_string(), w.to_uppercase(), format!("{w}!"), format!("  {w}.  "), format!("{w}\n")] {
                assert_eq!(answers(shape.clone(), &line), Some(Answer::Yes), "{line:?}");
            }
        }
    }
}

#[test]
fn every_refusal_word_answers_no_to_a_yes_no_question_and_to_an_approval() {
    for shape in [AskShape::YesNo, approval()] {
        for w in REFUSAL {
            for line in [w.to_string(), w.to_uppercase(), format!("{w}!"), format!("  {w}.  "), format!("{w}\r\n")] {
                assert_eq!(answers(shape.clone(), &line), Some(Answer::No), "{line:?}");
            }
        }
    }
}

#[test]
fn polite_filler_and_a_typographic_apostrophe_do_not_change_an_assent_or_a_refusal() {
    for line in ["yes please do it", "Yes, go ahead", "ok thanks", "yes, of course", "sure, that works", "please, go ahead", "yes yes"] {
        assert_eq!(answers(approval(), line), Some(Answer::Yes), "{line}");
    }
    for line in ["no thanks", "no, thank you", "No don't", "not now, thanks", "don\u{2019}t", "don\u{2019}t do it", "no no no"] {
        assert_eq!(answers(approval(), line), Some(Answer::No), "{line}");
    }
}

#[test]
fn an_assent_or_refusal_with_anything_else_in_it_answers_nothing() {
    for line in [
        "yes, but only the tests",
        "yes and delete the old ones",
        "no, but later",
        "yes no",
        "no yes",
        "please add retries to the fetch",
        "yes?",
        "sure about that",
        "okay so what now",
        "do it later",
        "not now, ask me tomorrow",
        "thanks yes",
    ] {
        assert_eq!(answers(AskShape::YesNo, line), None, "{line}");
        assert_eq!(answers(approval(), line), None, "{line}");
    }
}

#[test]
fn an_approval_is_never_answered_by_free_text_nor_by_always() {
    let asks = [mk("a1", "task-1", "Run make build?", approval(), 1)];
    for line in [
        "always",
        "yes always",
        "yes, always allow it",
        "yes for this session",
        "allow for the session",
        "don't ask again",
        "dont ask again",
        "no, never ask again",
        "yes and don't ask again",
        "always here",
        "forever",
        "make build",
        "go ahead and run make build with sudo",
        "main",
        "1",
        "2",
        "the first one",
        "approve",
        "yes\u{200b}",
    ] {
        assert_eq!(answers(approval(), line), None, "{line}");
        // not even when the operator is looking at it
        assert_eq!(bind(line, &asks, Some("task-1")), Bind::NotAnswer, "{line}");
    }
}

#[test]
fn a_yes_no_question_takes_only_yes_or_no_never_a_pick_or_a_text() {
    for line in ["1", "2", "main", "the first one", "maybe", "mainnet please"] {
        assert_eq!(answers(AskShape::YesNo, line), None, "{line}");
    }
}

#[test]
fn assent_and_refusal_never_answer_a_text_or_a_choice_that_has_no_such_option() {
    for line in ["yes", "no", "ok", "not now", "yes please do it", "y"] {
        for shape in [AskShape::Text, three()] {
            assert_eq!(answers(shape.clone(), line), None, "{line}");
            assert_eq!(bind(line, &[mk("a1", "task-1", "?", shape, 1)], Some("task-1")), Bind::NotAnswer, "{line}");
        }
    }
    // an option that is called `yes` is a name, and a name is a pick
    let yes_no = options(&[("yes", "Yes, overwrite"), ("no", "No, keep it")]);
    assert_eq!(answers(yes_no.clone(), "yes"), pick("yes"));
    assert_eq!(answers(yes_no, "No"), pick("no"));
}

// ---- picks ----

#[test]
fn a_position_picks_the_option_at_that_place() {
    let cases: &[(&str, &str)] = &[
        ("1", "fast"),
        ("2", "thorough"),
        ("3", "skip"),
        ("1.", "fast"),
        ("(2)", "thorough"),
        ("2)", "thorough"),
        ("[2]", "thorough"),
        ("#3", "skip"),
        ("# 3", "skip"),
        ("02", "thorough"),
        ("option 2", "thorough"),
        ("Option 2.", "thorough"),
        ("number 3", "skip"),
        ("the second one", "thorough"),
        ("second", "thorough"),
        ("The Second.", "thorough"),
        ("2nd", "thorough"),
        ("the 3rd", "skip"),
        ("first option", "fast"),
        ("third please", "skip"),
        ("two", "thorough"),
        ("two please", "thorough"),
        ("go with 3", "skip"),
        ("i'll go with 2", "thorough"),
        ("pick 1", "fast"),
        ("choose the third", "skip"),
        ("last", "skip"),
        ("the last one", "skip"),
        ("1 please", "fast"),
        ("2 is fine", "thorough"),
        ("2. that one", "thorough"),
        ("1, then run it", "fast"),
        ("3!", "skip"),
        ("  2  \n", "thorough"),
    ];
    for (line, id) in cases {
        assert_eq!(answers(three(), line), pick(id), "{line:?}");
    }
}

#[test]
fn a_number_outside_the_options_is_not_a_pick_and_not_turned_into_text() {
    for line in ["0", "4", "00", "9", "13", "99", "(7)", "option 4", "the fifth one", "tenth", "ten"] {
        assert_eq!(answers(three(), line), None, "{line}");
    }
    assert_eq!(answers(options(&[]), "1"), None);
    assert_eq!(answers(options(&[]), "last"), None);
    // three digits are not a position at all, just a word
    assert_eq!(answers(three(), "100"), text("100"));
}

#[test]
fn a_number_with_a_quantity_after_it_is_a_task_not_a_pick() {
    for line in [
        "3 weeks of logs",
        "2 files changed",
        "1 of them",
        "2-3 weeks",
        "1.5",
        "1,000",
        "10:30",
        "first, check the log",
        "one moment",
        "2x",
        "3abc",
        "2 + 2",
        "3 minutes later",
        "1 What do you mean?",
    ] {
        let got = answers(three(), line);
        assert!(!matches!(got, Some(Answer::Pick { .. })), "{line}: {got:?}");
    }
    assert_eq!(answers(three(), "first, check the log"), None, "a new request, not an answer");
    assert_eq!(answers(three(), "3 weeks of logs"), None);
}

#[test]
fn an_option_is_picked_by_its_id_its_label_or_a_word_that_only_it_has() {
    let cases: &[(&str, &str)] = &[
        ("fast", "fast"),
        ("FAST", "fast"),
        ("Thorough check", "thorough"),
        ("thorough check.", "thorough"),
        ("skip the check", "skip"),
        ("thorough", "thorough"),
        ("use the thorough one", "thorough"),
        ("the fast one please", "fast"),
        ("i think thorough", "thorough"),
        ("go with skip", "skip"),
        ("Skip!", "skip"),
    ];
    for (line, id) in cases {
        assert_eq!(answers(three(), line), pick(id), "{line:?}");
    }
}

#[test]
fn a_name_that_fits_several_options_or_disagrees_with_a_position_picks_nothing() {
    // `check` is in every label
    assert_eq!(answers(three(), "check"), None);
    assert_eq!(answers(three(), "the check one"), None);
    // two options with the same id
    assert_eq!(answers(options(&[("a", "One"), ("a", "Two")]), "a"), None);
    // ids that are numbers in another order than the places they are shown in
    assert_eq!(answers(options(&[("2", "Two"), ("1", "One")]), "1"), None);
    assert_eq!(answers(options(&[("1", "One"), ("2", "Two")]), "2"), pick("2"));
    // an id that is a number no place has is still its name
    assert_eq!(answers(options(&[("13", "Thirteen"), ("x", "Ex")]), "13"), pick("13"));
}

#[test]
fn a_choice_also_takes_a_plain_line_of_text() {
    let env = options(&[("stg", "Staging"), ("prod", "Production")]);
    assert_eq!(answers(env.clone(), "the eu cluster"), text("the eu cluster"));
    assert_eq!(answers(env.clone(), "  eu-west-1  "), text("eu-west-1"));
    assert_eq!(answers(env.clone(), "ping the host"), None);
    assert_eq!(answers(env, "git status"), None);
}

// ---- text ----

#[test]
fn a_plain_reply_answers_a_text_question() {
    for line in [
        "main",
        "5",
        "about ten",
        "10:30",
        "tomorrow morning",
        "feature/login",
        "src/lib.rs",
        "/etc/hosts",
        "the release branch",
        "my laptop",
        "use the release branch",
        "call it fresh-start",
        "i think the second one",
        "0042_add_index",
        "2024-10-23",
        "the one in the root folder",
        "ünïcödé",
        "日本語",
        "no idea",
    ] {
        assert_eq!(answers(AskShape::Text, line), text(line), "{line}");
    }
}

#[test]
fn a_new_request_a_question_or_a_command_is_no_reply() {
    for line in [
        "git status",
        "run the tests",
        "ping the host",
        "delete the old ones",
        "what is a branch?",
        "which file?",
        "how do I do that",
        "also add retries to the fetch",
        "now show me the log",
        "then push it",
        "and the other one",
        "thanks",
        "thank you very much",
        "hello",
        "hi there",
        "the build is broken",
        "my disk is almost full",
        "i want a new feature",
        "you should add tests",
        "btw the build fails",
        "this is a new task for you",
        "commit",
        "push",
        "run",
        "/plan",
        "/plan the migration",
        ":switch claude",
        ":",
        "",
        "   ",
        "...",
        "?!",
        "---",
        "()",
        "one two three four five six seven eight nine ten eleven twelve thirteen",
        "a question? yes",
    ] {
        assert_eq!(answers(AskShape::Text, line), None, "{line:?}");
    }
}

#[test]
fn a_reply_is_one_line_of_at_most_the_text_limit_and_is_given_trimmed() {
    let edge = "a".repeat(TEXT_MAX);
    assert_eq!(answers(AskShape::Text, &edge), text(&edge));
    assert_eq!(answers(AskShape::Text, &format!(" {edge} ")), text(&edge));
    assert_eq!(answers(AskShape::Text, &format!("{edge}a")), None);
    assert_eq!(answers(AskShape::Text, &"é".repeat(TEXT_MAX + 1)), None);
    assert_eq!(answers(AskShape::Text, &"é".repeat(TEXT_MAX)), text(&"é".repeat(TEXT_MAX)));
    assert_eq!(answers(AskShape::Text, "  main \t\n"), text("main"));
    assert_eq!(answers(AskShape::Text, "the  eu   cluster"), text("the  eu   cluster"), "inner spacing is as typed");
    assert_eq!(answers(AskShape::Text, "Main"), text("Main"), "case is as typed");
}

#[test]
fn a_line_with_a_control_or_invisible_character_answers_nothing() {
    for line in [
        "ma\nin",
        "ma\rin",
        "ma\tin",
        "ma\0in",
        "yes\0",
        "\0yes",
        "ye\x1bs",
        "yes\nno",
        "1\n2",
        "\x07",
        "y\u{200b}es",
        "ye\u{202e}s",
        "ma\u{2028}in",
        "\u{feff}main",
        "ma\u{7f}in",
        "ma\u{85}in",
        "the\u{2066}x\u{2069}",
    ] {
        for s in [AskShape::Text, AskShape::YesNo, approval(), three()] {
            assert_eq!(answers(s.clone(), line), None, "{line:?}");
            assert_eq!(bind(line, &[mk("a1", "t1", "?", s, 1)], Some("t1")), Bind::NotAnswer, "{line:?}");
        }
    }
}

#[test]
fn unicode_look_alikes_are_not_the_words_they_look_like() {
    // the Kelvin sign lowercases to `k` in Unicode but not in ASCII
    for line in ["o\u{212a}ay", "\u{ff59}\u{ff45}\u{ff53}", "yés", "ү", "n\u{333}o"] {
        assert_eq!(answers(AskShape::YesNo, line), None, "{line}");
        assert_eq!(answers(approval(), line), None, "{line}");
    }
    assert_eq!(answers(three(), "\u{ff11}"), text("\u{ff11}"), "a full-width one is not a position");
    assert_eq!(answers(three(), "ＦＡＳＴ"), text("ＦＡＳＴ"), "no case folding beyond ASCII");
}

#[test]
fn a_huge_line_is_refused_quickly() {
    let started = crate::cpu::start();
    for line in ["a".repeat(100_000), " ".repeat(100_000), "yes ".repeat(25_000), "é".repeat(100_000), "1".repeat(100_000), "\n".repeat(100_000)] {
        for s in [AskShape::Text, AskShape::YesNo, approval(), three()] {
            let open = [mk("a1", "task-1", "?", s, 1), mk("a2", "task-2", "?", AskShape::Text, 2)];
            assert_eq!(bind(&line, &open, None), Bind::NotAnswer);
            assert_eq!(bind(&line, &open, Some("task-1")), Bind::NotAnswer);
        }
    }
    crate::cpu::assert_within(std::time::Duration::from_secs(5), "the binder over every line", || started.elapsed());
}

#[test]
fn only_punctuation_and_stray_marks_answer_nothing_but_the_numbered_forms_still_pick() {
    for line in [".", "..", "!!!", "?", "-", "*", "#", "(", ")", "()", "[]", ",", "…", "¿", "\"\"", "''", "#.", "(.)"] {
        for s in [AskShape::Text, AskShape::YesNo, approval(), three()] {
            assert_eq!(answers(s, line), None, "{line}");
        }
    }
    for line in ["1.", "(2)", "2)", "#3"] {
        assert!(matches!(answers(three(), line), Some(Answer::Pick { .. })), "{line}");
    }
}

// ---- focus ----

fn pair() -> Vec<OpenAsk> {
    vec![
        mk("a1", "task-1", "Run make build?", approval(), 100),
        mk("a2", "task-2", "Which check should I run?", three(), 200),
        mk("a3", "task-3", "What should I call the branch?", AskShape::Text, 300),
    ]
}

#[test]
fn the_focused_task_gets_the_line_by_its_id_or_by_the_ask_itself() {
    assert_eq!(target(bind("2", &pair(), Some("task-2"))), ("a2".into(), Answer::Pick { id: "thorough".into() }));
    assert_eq!(target(bind("2", &pair(), Some("a2"))), ("a2".into(), Answer::Pick { id: "thorough".into() }));
    assert_eq!(target(bind("yes", &pair(), Some("task-1"))), ("a1".into(), Answer::Yes));
    assert_eq!(target(bind("main", &pair(), Some("task-3"))), ("a3".into(), Answer::Text { text: "main".into() }));
}

#[test]
fn the_focus_decides_where_a_line_goes_that_two_asks_would_both_take() {
    let two = vec![mk("a1", "t1", "Run it?", AskShape::YesNo, 1), mk("a2", "t2", "Run that?", AskShape::YesNo, 2)];
    assert_eq!(which(bind("yes", &two, None)), ["a1", "a2"]);
    assert_eq!(target(bind("yes", &two, Some("t1"))), ("a1".into(), Answer::Yes));
    assert_eq!(target(bind("no", &two, Some("t2"))), ("a2".into(), Answer::No));
}

#[test]
fn the_focused_tasks_newest_open_ask_takes_the_line() {
    let asks = vec![
        mk("old", "t1", "Run it?", AskShape::YesNo, 5),
        mk("new", "t1", "Run that?", AskShape::YesNo, 9),
        mk("mid", "t1", "Run this?", AskShape::YesNo, 7),
        mk("other", "t2", "Run more?", AskShape::YesNo, 50),
    ];
    assert_eq!(target(bind("ok", &asks, Some("t1"))).0, "new");
    // equal times: the one listed last
    let tie = vec![mk("x", "t1", "?", AskShape::YesNo, 5), mk("y", "t1", "?", AskShape::YesNo, 5)];
    assert_eq!(target(bind("ok", &tie, Some("t1"))).0, "y");
}

#[test]
fn a_line_that_does_not_fit_the_focused_ask_is_not_re_routed_to_another() {
    // `2` fits the other task's choice, but the operator is looking at the approval
    assert_eq!(bind("2", &pair(), Some("task-1")), Bind::NotAnswer);
    assert_eq!(bind("yes", &pair(), Some("task-2")), Bind::NotAnswer);
    assert_eq!(bind("fast", &pair(), Some("task-1")), Bind::NotAnswer);
    assert_eq!(bind("fast", &pair(), Some("task-3")), Bind::Answer { ask: "a3".into(), answer: Answer::Text { text: "fast".into() } });
}

#[test]
fn a_focus_that_names_no_open_ask_is_ignored() {
    let asks = vec![mk("a1", "doer-1", "Run make build?", approval(), 1)];
    for focus in [Some("chat"), Some("nothing"), Some(""), None] {
        assert_eq!(target(bind("yes", &asks, focus)), ("a1".into(), Answer::Yes), "{focus:?}");
    }
    assert_eq!(bind("yes", &[], Some("chat")), Bind::NotAnswer);
    // a different case is not the focus
    let two = [asks[0].clone(), mk("a2", "doer-2", "?", AskShape::YesNo, 2)];
    assert_eq!(which(bind("yes", &two, Some("DOER-1"))).len(), 2);
}

#[test]
fn a_focused_text_ask_takes_any_plain_line_but_never_a_command() {
    let asks = vec![mk("a1", "t1", "What should the commit message say?", AskShape::Text, 1)];
    for line in ["fix the retry loop in the fetch client and add a test for it", "run the tests", "what?", "also add docs", "git status"] {
        assert_eq!(bind(line, &asks, Some("t1")), Bind::Answer { ask: "a1".into(), answer: Answer::Text { text: line.into() } }, "{line}");
        assert_eq!(bind(line, &asks, None), Bind::NotAnswer, "without the focus the line is a new request: {line}");
    }
    let long = "x ".repeat(200);
    for line in [":switch claude", "", "yes", "no thanks", "...", long.as_str()] {
        assert_eq!(bind(line, &asks, Some("t1")), Bind::NotAnswer, "{line:?}");
    }
}

// ---- which question ----

#[test]
fn a_line_only_one_open_ask_takes_goes_to_it() {
    assert_eq!(target(bind("yes", &pair(), None)), ("a1".into(), Answer::Yes));
    assert_eq!(target(bind("no thanks", &pair(), None)), ("a1".into(), Answer::No));
    assert_eq!(target(bind("2", &pair(), None)), ("a2".into(), Answer::Pick { id: "thorough".into() }));
    assert_eq!(target(bind("skip", &pair(), None)), ("a2".into(), Answer::Pick { id: "skip".into() }));
    assert_eq!(
        target(bind("the release branch", &pair(), None)),
        ("a3".into(), Answer::Text { text: "the release branch".into() }),
        "a free line is the text question's before it is the choice's"
    );
    assert_eq!(bind("fix the build please", &pair(), None), Bind::NotAnswer);
    // `4` is no pick of the three, but it is a plain reply to the text question, which is then the only one that takes it
    assert_eq!(target(bind("4", &pair(), None)), ("a3".into(), Answer::Text { text: "4".into() }));
    assert_eq!(bind("4", &pair()[..2], None), Bind::NotAnswer);
}

#[test]
fn two_asks_that_take_the_line_equally_make_a_menu_never_a_guess() {
    let asks = vec![
        mk("late", "t2", "Overwrite the file?", AskShape::YesNo, 20),
        mk("early", "t1", "Run make build?", approval(), 10),
        mk("never", "t3", "Which check?", three(), 30),
    ];
    // listed in the order the asks were opened, not the order they were given
    assert_eq!(which(bind("yes", &asks, None)), ["early", "late"]);
    assert_eq!(which(bind("not now", &asks, None)), ["early", "late"]);
    let two_choices = vec![mk("c1", "t1", "Which check?", three(), 1), mk("c2", "t2", "Which mode?", options(&[("x", "Xs"), ("y", "Ys")]), 2)];
    assert_eq!(which(bind("2", &two_choices, None)), ["c1", "c2"]);
    // `3` only fits the one that has three
    assert_eq!(target(bind("3", &two_choices, None)), ("c1".into(), Answer::Pick { id: "skip".into() }));
    let two_texts = vec![mk("q1", "t1", "What should I call it?", AskShape::Text, 1), mk("q2", "t2", "What is the project called?", AskShape::Text, 2)];
    assert_eq!(which(bind("falcon", &two_texts, None)), ["q1", "q2"]);
}

#[test]
fn a_menu_is_the_same_whatever_order_the_asks_were_listed_in() {
    let mut asks: Vec<OpenAsk> = (0..6).map(|i| mk(&format!("a{i}"), &format!("t{i}"), "Go on?", AskShape::YesNo, 100 - i * 10)).collect();
    let first = which(bind("yes", &asks, None));
    assert_eq!(first, ["a5", "a4", "a3", "a2", "a1", "a0"]);
    asks.reverse();
    assert_eq!(which(bind("yes", &asks, None)), first);
}

#[test]
fn a_reply_is_told_apart_by_what_the_question_wants() {
    let asks = vec![
        mk("n", "t1", "How many runs do you want?", AskShape::Text, 1),
        mk("f", "t2", "Which file should I edit?", AskShape::Text, 2),
        mk("w", "t3", "When should I start it?", AskShape::Text, 3),
    ];
    assert_eq!(which(bind("5", &asks, None)), ["n", "w"], "a number is a quantity or a time, not a file");
    assert_eq!(target(bind("src/lib.rs", &asks, None)).0, "f");
    assert_eq!(target(bind("tomorrow", &asks, None)).0, "w");
    assert_eq!(target(bind("about ten", &[asks[0].clone(), asks[1].clone()], None)).0, "n");
    // a reply of no kind in particular could be any
    assert_eq!(which(bind("the config one", &asks, None)).len(), 3);
    // a question that wants nothing in particular takes any kind
    let open = vec![mk("o", "t1", "What should I do next?", AskShape::Text, 1), mk("f", "t2", "Which file should I edit?", AskShape::Text, 2)];
    assert_eq!(target(bind("lib/util.py", &open, None)).0, "f", "a path is what the file question wants");
    assert_eq!(target(bind("5", &open, None)).0, "o", "a number is no file");
}

#[test]
fn a_position_beats_a_plain_reply_unless_the_other_question_wants_exactly_that_kind() {
    let asks = vec![mk("c", "t1", "Which check?", three(), 1), mk("t", "t2", "What should I name it?", AskShape::Text, 2)];
    assert_eq!(target(bind("2", &asks, None)).0, "c");
    let asks = vec![mk("c", "t1", "Which check?", three(), 1), mk("t", "t2", "How many runs?", AskShape::Text, 2)];
    assert_eq!(which(bind("2", &asks, None)), ["c", "t"]);
    assert_eq!(target(bind("thorough", &asks, None)).0, "c", "a name is not a number");
    // a reply of the wrong kind for a question is dropped when another ask takes it
    let asks = vec![mk("c", "t1", "Which check?", three(), 1), mk("t", "t2", "Which file?", AskShape::Text, 2)];
    assert_eq!(target(bind("2", &asks, None)).0, "c");
}

#[test]
fn a_question_is_shown_on_one_line_of_at_most_eighty_characters() {
    let q = format!("Line one\nline two\t{}\u{0}\r\né", "x".repeat(200));
    let asks = vec![mk("a", "t1", &q, AskShape::YesNo, 1), mk("b", "t2", "", AskShape::YesNo, 2), mk("c", "t3", "Short?", AskShape::YesNo, 3)];
    let Bind::Which(menu) = bind("yes", &asks, None) else { panic!("a menu was expected") };
    assert_eq!(menu.len(), 3);
    for w in &menu {
        assert!(w.question.chars().count() <= 80, "{w:?}");
        assert!(!w.question.chars().any(char::is_control), "{w:?}");
    }
    assert!(menu[0].question.starts_with("Line one line two xxx"));
    assert_eq!(menu[1].question, "");
    assert_eq!(menu[2].question, "Short?");
}

#[test]
fn nothing_open_means_nothing_to_answer() {
    for line in ["yes", "1", "main", "no", "", ":q"] {
        assert_eq!(bind(line, &[], None), Bind::NotAnswer);
        assert_eq!(bind(line, &[], Some("t1")), Bind::NotAnswer);
    }
}

#[test]
fn the_same_ask_listed_twice_is_one_ask() {
    let a = mk("a1", "t1", "Run it?", approval(), 1);
    assert_eq!(target(bind("yes", &[a.clone(), a.clone(), a], None)), ("a1".into(), Answer::Yes));
}

#[test]
fn a_command_is_never_an_answer_even_when_an_ask_would_take_the_word() {
    for line in [":switch claude", ":yes", ":1", ": ok", ":fresh", ":gate make build"] {
        assert_eq!(bind(line, &pair(), None), Bind::NotAnswer, "{line}");
        assert_eq!(bind(line, &pair(), Some("task-3")), Bind::NotAnswer, "{line}");
    }
}

// ---- hostile shapes and many asks ----

#[test]
fn odd_shapes_from_a_careless_caller_do_not_panic_or_answer_wrongly() {
    let huge_label = "word ".repeat(50_000);
    let shapes = [
        options(&[("", ""), ("x", "")]),
        options(&[("  ", "  "), ("a b", "a b"), ("é", "É")]),
        options(&[("a", &huge_label), ("b", "b")]),
        numbered(200),
        options(&[]),
        AskShape::Approval { action: String::new(), risk: 255 },
    ];
    for shape in shapes {
        for line in ["1", "2", "x", "a b", "é", "É", "word", "opt", "o7", "last", "yes", "no", "the thing", "200", "201", "   x"] {
            if let Bind::Answer { answer, .. } = bind(line, &[mk("a1", "t1", "", shape.clone(), 0)], None) {
                assert_eq!(shape.check(&answer), Ok(()), "{line}: {answer:?}");
            }
        }
    }
    assert_eq!(answers(numbered(200), "99"), pick("o98"));
    assert_eq!(answers(numbered(200), "100"), text("100"), "a position has one or two digits");
    assert_eq!(answers(numbered(200), "last"), pick("o199"));
    assert_eq!(answers(numbered(200), "opt"), None, "every option is called that");
}

#[test]
fn ten_thousand_open_asks_are_bound_in_a_blink() {
    let asks: Vec<OpenAsk> = (0..10_000).map(|i| mk(&format!("a{i}"), &format!("t{}", i % 50), "Run it?", AskShape::YesNo, i)).collect();
    let started = crate::cpu::start();
    assert_eq!(which(bind("yes", &asks, None)).len(), 10_000);
    assert_eq!(target(bind("yes", &asks, Some("t7"))).0, "a9957");
    crate::cpu::assert_within(std::time::Duration::from_millis(2_000), "ten thousand open questions", || started.elapsed());
}

// ---- the property: an answer always passes the check of the ask it is for ----

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn pieces() -> Vec<String> {
    let mut v: Vec<String> = [
        "yes", "no", "ok", "please", "thanks", "1", "2", "3", "4", "0", "13", "(2)", "#3", "2)", "1.", "first", "second", "last", "one", "two",
        "option", "the", "fast", "thorough", "skip", "check", "main", "a", "b", "id", "use", "go", "with", "it", "that", "my", "about", "ten",
        "tomorrow", "src/lib.rs", "10:30", "not", "now", "always", "don't", "ask", "again", "?", "!", ".", ",", ":", ":x", "/", "-", "é", "日本",
        "\u{2019}", "\n", "\t", "\0", "\u{202e}", "and", "also", "what", "which", "how many", "when", "where", "btw", "is", "broken", "i",
        "want", "call", "think",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    v.push("x".repeat(300));
    v
}

fn random_line(rng: &mut Rng, pieces: &[String]) -> String {
    let n = 1 + rng.below(7);
    let glue = if rng.below(5) == 0 { "" } else { " " };
    (0..n).map(|_| pieces[rng.below(pieces.len())].as_str()).collect::<Vec<_>>().join(glue)
}

fn random_ask(rng: &mut Rng, i: usize) -> OpenAsk {
    let shape = match rng.below(5) {
        0 => AskShape::YesNo,
        1 => approval(),
        2 => AskShape::Text,
        3 => three(),
        _ => options(&[("1", "One"), ("2", "Two"), ("fast", "Fast"), ("a b", "A B"), ("é", "É")][..1 + rng.below(5)]),
    };
    let questions = ["Run it?", "How many runs?", "Which file?", "When should I start?", "What should I call it?", "\n\0"];
    mk(&format!("a{i}"), &format!("t{}", rng.below(3)), questions[rng.below(questions.len())], shape, rng.below(5) as u64)
}

#[test]
fn any_answer_passes_the_check_of_its_own_ask_and_a_menu_has_at_least_two_different_asks() {
    let (mut rng, pieces) = (Rng(0x00C0_FFEE_D15E_A5E5), pieces());
    let (mut answered, mut menus, mut nothing) = (0, 0, 0);
    for _ in 0..30_000 {
        let n = 1 + rng.below(4);
        let asks: Vec<OpenAsk> = (0..n).map(|i| random_ask(&mut rng, i)).collect();
        let line = random_line(&mut rng, &pieces);
        let focus = [None, Some("t0"), Some("a1"), Some("t9")][rng.below(4)];
        let b = bind(&line, &asks, focus);
        assert_eq!(b, bind(&line, &asks, focus), "deterministic: {line:?}");
        match b {
            Bind::Answer { ask, answer } => {
                answered += 1;
                let a = asks.iter().find(|a| a.ask == ask).expect("an ask that was open");
                assert_eq!(a.shape.check(&answer), Ok(()), "{line:?} -> {answer:?} for {a:?}");
                if matches!(a.shape, AskShape::Approval { .. }) {
                    assert!(matches!(answer, Answer::Yes | Answer::No), "{line:?}");
                }
                if let Answer::Text { text } = &answer {
                    assert_eq!(text, text.trim(), "{line:?}");
                    assert!(!text.starts_with(':'), "{line:?}");
                }
            }
            Bind::Which(menu) => {
                menus += 1;
                assert!(menu.len() >= 2 && focus.is_none_or(|f| !asks.iter().any(|a| a.ask == f || a.asker == f)), "{line:?}");
                let ids: std::collections::BTreeSet<&str> = menu.iter().map(|w| w.ask.as_str()).collect();
                assert_eq!(ids.len(), menu.len(), "{line:?}");
                assert!(ids.iter().all(|id| asks.iter().any(|a| a.ask == *id)));
                assert!(menu.iter().all(|w| w.question.chars().count() <= 80 && !w.question.chars().any(char::is_control)));
            }
            Bind::NotAnswer => nothing += 1,
        }
    }
    assert!(answered > 1_000 && menus > 100 && nothing > 1_000, "the generator reaches every result: {answered} {menus} {nothing}");
}

#[test]
fn a_line_that_binds_to_an_approval_never_binds_to_anything_but_yes_or_no_under_any_focus() {
    let (mut rng, pieces) = (Rng(0x0BAD_5EED_1234_ABCD), pieces());
    for _ in 0..20_000 {
        let line = random_line(&mut rng, &pieces);
        for focus in [None, Some("t1")] {
            let asks = [mk("ap", "t1", "Run make build?", approval(), 1)];
            if let Bind::Answer { answer, .. } = bind(&line, &asks, focus) {
                assert!(matches!(answer, Answer::Yes | Answer::No), "{line:?}");
            }
        }
    }
}
