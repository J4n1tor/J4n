//! The Brief: the contract for a line, who gets which, the cards a line picks, and what is cut when the window is small.
//! The rule that these are work lives here: "continue", "yes", "2", "go on", "do it" in a thread that is going on are work.

use j4n_core::brief::{self, Brief, Card, LEAN, OUTPUT_RESERVE, READ_ONLY, WORK, cards, contract, is_short, tokens};
use j4n_core::schema::DataClass;

#[test]
fn small_talk_is_the_first_line_of_a_thread_and_nothing_after_it() {
    assert_eq!(contract("hi", false, false), LEAN);
    assert_eq!(contract("thanks", false, false), LEAN);
    // the same words in a thread that is going are the normal contract
    assert_eq!(contract("hi", true, false), WORK);
    for line in ["continue", "yes", "2", "go on", "do it", "ok", "no", "the second one"] {
        assert_eq!(contract(line, true, false), WORK, "{line} in a going thread is work");
    }
    // a first line that is a task is a task, short or not
    for line in ["fix it", "add a flag", "run the tests", "what does this do?"] {
        assert_eq!(contract(line, false, false), WORK, "{line}");
    }
}

#[test]
fn a_task_that_only_reads_is_told_so_and_keeps_its_contract() {
    let c = contract("review the diff", true, true);
    assert!(c.starts_with(WORK) && c.ends_with(READ_ONLY), "{c}");
    assert_eq!(contract("review the diff", true, false), WORK);
}

#[test]
fn a_short_line_is_three_words_and_no_task_in_them() {
    assert!(is_short("hi") && is_short("good morning") && is_short("go on please"));
    assert!(!is_short("") && !is_short("what is this?") && !is_short("fix the bug now") && !is_short("add tests"));
}

fn offered() -> Vec<Card> {
    let card = |id: &str, words: &[&str]| Card { id: id.into(), words: words.iter().map(|w| w.to_string()).collect(), text: format!("about {id}") };
    vec![card("models", &["model", "continue"]), card("git", &["commit", "branch"])]
}

#[test]
fn a_card_is_picked_by_a_word_of_the_line_and_a_short_line_in_a_going_thread_picks_none() {
    let all = offered();
    assert_eq!(cards("which model is this", false, &all).iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["models"]);
    assert_eq!(cards("make a commit on a new branch", false, &all).len(), 1);
    // "continue" in the text of a help card is no match: in a going thread it selects nothing
    assert!(cards("continue", true, &all).is_empty());
    assert!(cards("continue", false, &all).len() == 1, "the first line of a thread may");
    // a long line in a going thread may
    assert_eq!(cards("please switch the model for the next part", true, &all).len(), 1);
}

#[test]
fn a_brief_has_its_sections_in_a_fixed_order_whatever_order_they_were_given_in() {
    let a = Brief::new().with("notes", DataClass::Project, "n").with("contract", DataClass::Public, "c").with("concerns", DataClass::Project, "k");
    let b = Brief::new().with("concerns", DataClass::Project, "k").with("contract", DataClass::Public, "c").with("notes", DataClass::Project, "n");
    assert_eq!(a.render(10_000), b.render(10_000));
    let r = a.render(10_000);
    assert_eq!(r.sections, ["contract", "concerns", "notes"]);
    assert_eq!(r.text, "## contract\nc\n\n## concerns\nk\n\n## notes\nn");
    assert_eq!(r.class, DataClass::Project, "the most sensitive of what it carries");
}

#[test]
fn a_section_with_no_text_or_no_place_in_the_order_adds_nothing() {
    let r = Brief::new().with("contract", DataClass::Public, "c").with("concerns", DataClass::Project, "  ").with("gossip", DataClass::Project, "x").render(1000);
    assert_eq!(r.sections, ["contract"]);
    assert_eq!(Brief::new().render(1000).class, DataClass::Public);
}

#[test]
fn the_window_less_its_reserve_is_the_budget_and_the_last_section_goes_first() {
    let big = "word ".repeat(400);
    let brief = Brief::new().with("contract", DataClass::Public, WORK).with("concerns", DataClass::Project, &big).with("notes", DataClass::Project, &big);
    let whole = brief.render(100_000);
    assert!(whole.cut.is_empty() && whole.tokens > 500);
    // a window that holds the contract and one of the others, less 15% kept for the answer
    let window = ((tokens(WORK) + tokens(&big) + 40) as f64 / (1.0 - OUTPUT_RESERVE)) as usize;
    let cut = brief.render(window);
    assert_eq!(cut.sections, ["contract", "concerns"], "{cut:?}");
    assert_eq!(cut.cut, ["notes"]);
    // the contract is never cut, however small the window
    let tiny = brief.render(1);
    assert_eq!(tiny.sections, ["contract"]);
    assert_eq!(tiny.cut, ["notes", "concerns"]);
}

#[test]
fn tokens_are_four_characters_each_rounded_up() {
    assert_eq!((tokens(""), tokens("abcd"), tokens("abcde"), brief::OUTPUT_RESERVE), (0, 1, 2, 0.15));
}

#[test]
fn four_words_are_no_longer_small_talk_and_a_brief_that_just_fits_is_not_cut() {
    assert!(is_short("good morning to you".split(' ').take(3).collect::<Vec<_>>().join(" ").as_str()));
    assert!(!is_short("good morning to you"));
    // the budget is the window less its reserve: a window that holds the sections only without the reserve cuts one
    let one = "word ".repeat(100);
    let brief = Brief::new().with("contract", DataClass::Public, WORK).with("notes", DataClass::Project, &one);
    // what the sections take: their text, their name and the two marks of a heading
    let size = tokens(WORK) + tokens("contract") + 2 + tokens(&one) + tokens("notes") + 2;
    let exact = ((size as f64) / (1.0 - OUTPUT_RESERVE)).ceil() as usize + 1;
    assert!(brief.render(exact).cut.is_empty(), "{:?}", brief.render(exact));
    assert_eq!(brief.render(size + 1).cut, ["notes"], "without the reserve it would fit; with it, it does not");
}

#[test]
fn the_operators_lines_in_the_section_inbox_are_never_cut_whatever_the_window() {
    let lines = (1..=40).map(|n| format!("{n}. a line the operator typed, word for word")).collect::<Vec<_>>().join("\n");
    let brief = Brief::new()
        .with("contract", DataClass::Public, WORK)
        .with("project", DataClass::Project, &"p ".repeat(600))
        .with("inbox", DataClass::Project, &lines)
        .with("notes", DataClass::Project, &"n ".repeat(600));
    let r = brief.render(500);
    // the others go, last first; the lines stay, in full, after the contract
    assert_eq!(r.cut, ["notes", "project"]);
    assert_eq!(r.sections, ["contract", "inbox"]);
    assert!(r.text.contains(&format!("## inbox\n{lines}")), "{}", r.text);
    // a window too small for even that cuts nothing more
    assert_eq!(brief.render(10).sections, ["contract", "inbox"]);
}
