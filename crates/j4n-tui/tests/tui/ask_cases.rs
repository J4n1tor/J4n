//! The ask field. What a question can be answered with, numbered; the popup for one and the tray for several, drawn above
//! the composer; the question inline in the chat; the keys that answer without a line (a digit, `⇥`, `⏎`, `?`, `↑↓`, `esc`); an answer
//! that widens pressed twice; a model's pick never shown as the hint of an approval; and what the binder would make of the line.
//! The sheets are the goldens of the scenes `ask-popup`, `ask-tray`, `ask-hint` and `ask-bind`. These tests draw in the `unicode`
//! tier, so the marks are letters a test can read.

use crate::world::{banded, hello, screen};
use j4n_proto::{AskView, By, Cell, Choice, Control, Down, Field, FieldKind, Intent, Json, Node, Present, Reply, Row, Shape, Tone, Up, json};
use j4n_tui::App;
use j4n_tui::ask::{Send, is_score, options, shape_icon};
use j4n_tui::headless::{Headless, typed};
use j4n_tui::theme::{Settings, Theme};

fn tier(a: &mut App, marks: &str) {
    a.theme = Theme::build(&Settings { marks: marks.into(), you: "mark".into(), ..Settings::default() }, &[]).expect("a theme");
}

fn row(key: &str, what: &str, icon: &str, parent: &str) -> Row {
    Row {
        key: key.into(),
        cells: vec![Cell::with_icon(icon, what, Tone::Plain), Cell::new("", Tone::Dim)],
        parent: (!parent.is_empty()).then(|| parent.to_string()),
        ..Row::default()
    }
}

/// A turn with the lanes A (done), B (waiting for an answer) and C (queued); the question `a1` is the last step of B.
fn lanes() -> Vec<Row> {
    let mut t = row("t", "fix the parser", "", "");
    t.cells.push(Cell::new("12:29", Tone::Dim));
    vec![t, row("a", "A omp", "done", "t"), row("b", "B claude", "ask", "t"), row("b1", "parser.rs", "edit", "b"), row("a1", "commit to b-claude?", "ask", "b"), row("c", "C pi", "queued", "t")]
}

fn started(props: Json, rows: Vec<Row>) -> App {
    let mut a = App::new();
    a.start();
    a.down(hello());
    let chat = Node { id: "chat".into(), field: Some(Field { kind: FieldKind::Chat, bind: "chat:main".into(), props: Json::Null }), ..Node::default() };
    let ask = Node { id: "ask".into(), field: Some(Field { kind: FieldKind::Ask, bind: "asks:open".into(), props }), ..Node::default() };
    a.down(Down::Tree { node: Node { id: "root".into(), present: Present::Stack, children: vec![chat, ask], ..Node::default() } });
    tier(&mut a, "unicode");
    let total = rows.len() as u64;
    a.down(Down::Rows { bind: "chat:main".into(), rows, total, from: 0, rev: 1 });
    a
}

fn open(a: &mut App, asks: Vec<AskView>) {
    a.down(Down::Asks { open: asks });
}

fn cell(icon: &str, text: &str) -> Cell {
    Cell::with_icon(icon, text, Tone::Dim)
}

fn change() -> Vec<Row> {
    let mut one = Row { key: "c1".into(), cells: vec![cell("edit", "parser.rs"), cell("", ""), cell("", "+12 −3")], ..Row::default() };
    one.detail = vec![Cell::new("@@ fn parse_args", Tone::Dim), Cell::new("+    self.bump();", Tone::Plain), Cell::new("-    return Err(Stop);", Tone::Plain)];
    let two = Row { key: "c2".into(), cells: vec![cell("edit", "syntax.md"), cell("", ""), cell("", "+4 −4")], ..Row::default() };
    vec![one, two]
}

fn pick(id: &str, label: &str) -> Choice {
    Choice { id: id.into(), label: label.into(), ..Choice::default() }
}

/// The approval of the scene `ask-popup`.
fn approval() -> AskView {
    AskView {
        ask: "a1".into(),
        asker: "b".into(),
        question: "commit to b-claude?".into(),
        shape: Shape::Approval,
        risk: 30,
        grants: vec!["session".into()],
        wait: "8:42".into(),
        facts: vec![cell("worktree", "b-claude"), cell("commit", "4f2a1c")],
        change: change(),
        why: "A commit changes the branch b-claude; it can be undone with a reset.".into(),
        options: vec![Choice { id: "yes".into(), recommended: true, by: By::Rule, ..Choice::default() }, Choice { id: "session".into(), label: "yes · commits in b-claude this session".into(), widens: true, ..Choice::default() }],
        ..AskView::default()
    }
}

fn choice(id: &str, asker: &str, labels: &[&str]) -> AskView {
    AskView { ask: id.into(), asker: asker.into(), question: format!("which {id}?"), shape: Shape::Choice, options: labels.iter().map(|l| pick(l, l)).collect(), ..AskView::default() }
}

fn lines(a: &mut App, cols: u16, rows: u16) -> Vec<String> {
    screen(a, cols, rows).into_iter().map(|l| l.trim_end().to_string()).collect()
}

/// The box of the questions: from its top `╭` to its bottom `╰`. The composer is a round box too, and the last one on the screen, so a
/// screen with one box has no questions in a box.
fn boxed(s: &[String]) -> Vec<String> {
    let mut boxes = Vec::new();
    let mut from = 0;
    while let Some(t) = s[from..].iter().position(|l| l.starts_with('╭') && l.ends_with('╮')).map(|t| from + t) {
        let Some(b) = s[t..].iter().position(|l| l.starts_with('╰') && l.ends_with('╯')).map(|b| t + b) else { break };
        boxes.push(&s[t..=b]);
        from = b + 1;
    }
    if boxes.len() < 2 { Vec::new() } else { boxes[0].to_vec() }
}

fn hit(a: &mut App, keys: &[&str]) -> Vec<Up> {
    keys.iter().flat_map(|k| a.key(j4n_tui::scene::key(k).unwrap_or_else(|| panic!("`{k}` is a key")))).collect()
}

fn typing(a: &mut App, text: &str) {
    typed(text, false).into_iter().for_each(|k| drop(a.key(k)));
}

fn answer(ask: &str, reply: Reply) -> Up {
    Up::Intent { intent: Intent::Answer { ask: ask.into(), reply } }
}

// ---- what a question can be answered with ----

fn labels(a: &AskView, who: &str) -> Vec<String> {
    options(a, who).into_iter().map(|o| o.label).collect()
}

#[test]
fn an_approval_is_answered_yes_no_for_each_grant_it_offers_or_by_stopping_the_task_that_asked() {
    let mut a = approval();
    a.grants = vec!["task".into(), "session".into(), "here".into()];
    a.options.clear();
    assert_eq!(labels(&a, "B"), ["yes", "no", "yes · this task", "yes · this session", "yes · always here", "stop B"]);
    let sends: Vec<Send> = options(&a, "B").into_iter().map(|o| o.send).collect();
    assert_eq!(
        sends,
        [Send::Reply(Reply::Yes), Send::Reply(Reply::No), Send::Reply(Reply::Grant { scope: "task".into() }), Send::Reply(Reply::Grant { scope: "session".into() }), Send::Reply(Reply::Grant { scope: "here".into() }), Send::Stop]
    );
    a.grants.clear();
    assert_eq!(labels(&a, "B"), ["yes", "no", "stop B"], "no grant is offered that the core did not offer");
}

#[test]
fn a_yes_or_no_has_those_two_and_a_text_question_has_none() {
    let a = AskView { shape: Shape::YesNo, grants: vec!["session".into()], ..approval() };
    assert_eq!(labels(&a, "B"), ["yes", "no"], "a grant and a stop belong to an approval");
    let t = AskView { shape: Shape::Text, ..approval() };
    assert!(options(&t, "B").is_empty());
}

#[test]
fn a_choice_is_its_options_in_the_order_the_core_listed_them() {
    let a = choice("plan", "a", &["cache", "split", "skip"]);
    assert_eq!(labels(&a, "A"), ["cache", "split", "skip", "custom", "decide yourself", "skip it"], "then the row for words of one\'s own and the answers that are ready");
    let sends: Vec<Send> = options(&a, "A").into_iter().map(|o| o.send).collect();
    assert_eq!(sends[1], Send::Reply(Reply::Pick { id: "split".into() }));
    assert!(options(&a, "A").iter().all(|o| !o.hint && !o.widens && o.icon.is_empty()));
}

#[test]
fn the_options_of_an_approval_lay_words_over_the_fixed_answers_and_one_that_names_no_answer_is_ignored() {
    let mut a = approval();
    a.options = vec![
        Choice { id: "no".into(), label: "no, leave it".into(), note: "undo: reset".into(), ..Choice::default() },
        Choice { id: "session".into(), widens: true, ..Choice::default() },
        Choice { id: "nonsense".into(), label: "x".into(), recommended: true, by: By::Core, ..Choice::default() },
    ];
    let o = options(&a, "B");
    assert_eq!((o[1].label.as_str(), o[1].note.as_str()), ("no, leave it", "undo: reset"));
    assert_eq!(o[2].label, "yes · this session", "an empty label leaves the words as they were");
    assert!(o[2].widens && !o[0].widens);
    assert_eq!(o.len(), 4);
    assert!(o.iter().all(|o| !o.hint), "the one that names no answer is not a hint");
}

#[test]
fn a_grant_always_widens_and_a_yes_or_no_never_does() {
    let a = approval();
    let o = options(&a, "B");
    assert!(!o[0].widens && !o[1].widens && o[2].widens && !o[3].widens);
    let mut odd = approval();
    odd.grants = vec!["forever".into()];
    odd.options.clear();
    let o = options(&odd, "B");
    assert_eq!((o[2].label.as_str(), o[2].widens), ("yes · forever", true));
}

#[test]
fn an_approval_shows_the_hint_of_a_rule_or_the_core_and_never_a_models_or_an_unnamed_one() {
    for (by, shown) in [(By::Rule, true), (By::Core, true), (By::Model, false), (By::Doer, false), (By::Operator, false)] {
        let mut a = approval();
        a.options = vec![Choice { id: "yes".into(), recommended: true, by, ..Choice::default() }];
        let hint = options(&a, "B").iter().any(|o| o.hint);
        assert_eq!(hint, shown, "{by:?}");
    }
}

#[test]
fn a_choice_or_a_yes_no_shows_a_models_hint_and_says_it_is_a_models() {
    let mut c = choice("plan", "a", &["x", "y"]);
    c.options[1].recommended = true;
    c.options[1].by = By::Model;
    let o = options(&c, "A");
    assert!(o[1].hint && o[1].model && !o[0].hint);
    let mut y = AskView { shape: Shape::YesNo, ..approval() };
    y.options = vec![Choice { id: "no".into(), recommended: true, by: By::Model, ..Choice::default() }];
    let o = options(&y, "B");
    assert!(o[1].hint && o[1].model);
}

#[test]
fn an_option_is_sent_as_the_reply_it_stands_for_and_stop_is_an_order_to_the_task_that_asked() {
    let a = approval();
    let o = options(&a, "B");
    assert_eq!(o[0].intent(&a), Intent::Answer { ask: "a1".into(), reply: Reply::Yes });
    assert_eq!(o[2].intent(&a), Intent::Answer { ask: "a1".into(), reply: Reply::Grant { scope: "session".into() } });
    assert_eq!(o[3].intent(&a), Intent::Control { task: "b".into(), control: Control::Abort });
}

#[test]
fn a_choice_of_numbers_is_a_score_and_each_kind_of_question_has_its_mark() {
    let score = choice("how", "c", &["1", "2", "3"]);
    assert!(is_score(&score) && !is_score(&choice("how", "c", &["1", "two"])) && !is_score(&choice("how", "c", &[])));
    assert_eq!(shape_icon(&score), "ask-score");
    assert_eq!(shape_icon(&choice("how", "c", &["a"])), "ask-choice");
    assert_eq!(shape_icon(&approval()), "risky");
    assert_eq!(shape_icon(&AskView { shape: Shape::Text, ..approval() }), "ask-text");
    assert_eq!(shape_icon(&AskView { shape: Shape::YesNo, ..approval() }), "ask");
}

// ---- the popup ----

/// A row of the box `w` columns wide: `left` at its left edge, `right` at its right.
fn bx(left: &str, right: &str, w: usize) -> String {
    let used = left.chars().count() + right.chars().count();
    format!("│ {left}{}{right} │", " ".repeat(w - 4 - used))
}

fn rule(l: char, r: char, w: usize) -> String {
    format!("{l}{}{r}", "─".repeat(w - 2))
}

#[test]
fn one_question_is_a_popup_with_the_lane_that_asked_the_risk_what_it_is_about_the_change_and_the_answers() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, vec![approval()]);
    let s = lines(&mut a, 80, 24);
    let w = 80;
    assert_eq!(
        boxed(&s),
        [
            rule('╭', '╮', w),
            bx("◇ B claude", "◔ 8:42", w),
            bx("", "", w),
            bx("commit to b-claude?", "", w),
            bx("", "", w),
            bx("▕██▍░░░░░▏ 30   ⊢ b-claude   ◍ 4f2a1c", "", w),
            bx("├ ± parser.rs", "+12 −3", w),
            bx("╰ ± syntax.md", "+4 −4", w),
            bx("", "", w),
            bx("⇥ 1  ✓ yes", "", w),
            bx("  2  ✗ no", "", w),
            bx("  3  ✓ yes · commits in b-claude this session", "⚠ widens", w),
            bx("  4  ■ stop B", "", w),
            bx("", "", w),
            bx("1–4 · ⏎ diff · ? why · esc later", "", w),
            rule('╰', '╯', w),
        ]
        .map(|l| l.trim_end().to_string())
        .to_vec(),
        "{s:#?}"
    );
}

#[test]
fn the_popup_stands_between_the_transcript_and_the_composer_and_the_transcript_keeps_its_last_lines_above_it() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, vec![approval()]);
    let s = lines(&mut a, 80, 24);
    let top = s.iter().position(|l| l.starts_with('╭') && l.ends_with('╮')).unwrap();
    assert!(s[top - 1].starts_with("╰─ ⊸ C pi"), "{s:#?}");
    assert!(s[top - 2].starts_with("│  ╰─ ◇ commit to b-claude?") && s[top - 2].ends_with("1 ✓  2 ✗   ⇥ 1"), "{s:#?}");
    let bottom = top + s[top..].iter().position(|l| l.starts_with('╰') && l.ends_with('╯')).unwrap();
    assert!(s[bottom + 1].starts_with('╭'), "the composer follows it: {s:#?}");
    assert!(s[bottom + 2].ends_with("→ ◇ B │"), "{s:#?}");
}

#[test]
fn a_question_nobody_can_name_is_headed_by_the_key_of_its_task() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, vec![AskView { asker: "t77".into(), ..approval() }]);
    let s = boxed(&lines(&mut a, 80, 24));
    assert_eq!(s[1], bx("◇ t77", "◔ 8:42", 80).trim_end());
}

#[test]
fn a_question_that_has_waited_no_time_shows_no_clock_and_one_without_facts_or_a_change_shows_neither() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, vec![AskView { wait: String::new(), facts: Vec::new(), change: Vec::new(), ..approval() }]);
    let s = boxed(&lines(&mut a, 80, 24));
    assert_eq!(s[1], "│ ◇ B claude                                                                   │".trim_end());
    assert!(s.iter().all(|l| !l.contains('◔') && !l.contains('⊢') && !l.contains("├ ±")), "{s:#?}");
    assert!(s.iter().any(|l| l.starts_with("│ ▕██▍░░░░░▏ 30")), "{s:#?}");
}

#[test]
fn a_long_question_wraps_inside_the_box_and_nothing_is_wider_than_the_screen() {
    let mut a = started(Json::Null, lanes());
    let q = "should the parser accept a trailing comma in an argument list, in a list of fields, and in a list of imports as well?";
    open(&mut a, vec![AskView { question: q.into(), ..approval() }]);
    let s = lines(&mut a, 60, 24);
    let b = boxed(&s);
    assert!(b.iter().any(|l| l.contains("should the parser accept")) && b.iter().any(|l| l.contains("well?")), "{b:#?}");
    assert!(s.iter().all(|l| l.chars().count() <= 60));
}

#[test]
fn the_popup_takes_the_width_of_the_composer_up_to_100_columns_and_stands_at_the_left() {
    // the popup is cut to the composer's width, up to 100 columns
    for (width, want) in [(140, 100), (120, 100), (90, 90), (60, 60)] {
        let mut a = started(Json::Null, lanes());
        open(&mut a, vec![approval()]);
        let s = lines(&mut a, width, 30);
        let b = boxed(&s);
        assert_eq!(b[0].chars().count(), want, "{width} columns: {b:#?}");
        assert!(b.iter().all(|l| l.chars().count() == want), "{width} columns");
    }
}

#[test]
fn enter_opens_the_diff_of_the_change_and_enter_shuts_it() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, vec![approval()]);
    assert!(hit(&mut a, &["enter"]).is_empty());
    let b = boxed(&lines(&mut a, 80, 30));
    assert!(b.iter().any(|l| l.starts_with("│ │   @@ fn parse_args")), "{b:#?}");
    assert!(b.iter().any(|l| l.starts_with("│ │   +    self.bump();")) && b.iter().any(|l| l.starts_with("│ │   -    return Err(Stop);")), "{b:#?}");
    assert!(b.iter().any(|l| l.contains("╰ ± syntax.md")), "the last row of the tree has no pipe under it");
    hit(&mut a, &["enter"]);
    let b = boxed(&lines(&mut a, 80, 30));
    assert!(b.iter().all(|l| !l.contains("fn parse_args")), "{b:#?}");
}

#[test]
fn the_added_and_removed_lines_of_a_diff_are_coloured_as_added_and_removed() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, vec![approval()]);
    hit(&mut a, &["enter"]);
    let ansi = Headless::new(80, 30).snap(&mut a).ansi;
    let c = |r: j4n_tui::theme::palette::Role, a: &App| {
        let v = a.theme.roles.get(r);
        format!("38;2;{};{};{}", v.0, v.1, v.2)
    };
    let (add, del) = (c(j4n_tui::theme::palette::Role::Add, &a), c(j4n_tui::theme::palette::Role::Del, &a));
    let row = |needle: &str| ansi.lines().find(|l| l.contains(needle)).unwrap_or_else(|| panic!("{needle} is shown")).to_string();
    assert!(row("self.bump();").contains(&add), "{}", row("self.bump();"));
    assert!(row("return Err(Stop);").contains(&del));
    assert!(!row("@@ fn parse_args").contains(&add) && !row("@@ fn parse_args").contains(&del));
}

#[test]
fn a_question_mark_shows_why_and_hides_it_and_is_a_letter_where_there_is_no_why() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, vec![approval()]);
    assert!(hit(&mut a, &["?"]).is_empty() && a.line.buf.is_empty());
    let b = boxed(&lines(&mut a, 80, 30));
    assert!(b.iter().any(|l| l.contains("A commit changes the branch b-claude")), "{b:#?}");
    assert!(b.last().is_some() && b.iter().any(|l| l.contains("? hide")));
    hit(&mut a, &["?"]);
    assert!(boxed(&lines(&mut a, 80, 30)).iter().all(|l| !l.contains("A commit changes")));
    open(&mut a, vec![AskView { why: String::new(), ..approval() }]);
    hit(&mut a, &["?"]);
    assert_eq!(a.line.buf, "?", "with nothing to say the key is a letter of the line");
}

#[test]
fn the_keys_line_names_only_the_keys_that_do_something_for_this_question() {
    let mut a = started(Json::Null, lanes());
    let keys = |a: &mut App| boxed(&lines(a, 80, 30)).iter().rev().nth(1).cloned().unwrap_or_default();
    open(&mut a, vec![AskView { change: Vec::new(), why: String::new(), grants: Vec::new(), ..approval() }]);
    assert!(keys(&mut a).contains("│ 1–3 · esc later"), "{}", keys(&mut a));
    open(&mut a, vec![AskView { why: String::new(), grants: Vec::new(), ..approval() }]);
    assert!(keys(&mut a).contains("1–3 · ⏎ diff · esc later"), "{}", keys(&mut a));
    open(&mut a, vec![AskView { shape: Shape::Text, ..approval() }]);
    assert!(keys(&mut a).contains("⏎ send"), "{}", keys(&mut a));
}

#[test]
fn a_choice_lists_its_options_numbered_with_a_custom_row_and_the_ready_answers_under_it() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, vec![choice("branch", "b", &["main", "b-claude"])]);
    let b = boxed(&lines(&mut a, 80, 24));
    let body: Vec<&str> = b.iter().map(String::as_str).filter(|l| l.contains("main") || l.contains("b-claude") || l.contains("custom") || l.contains("decide") || l.contains("skip it")).collect();
    assert_eq!(
        body,
        [bx("  1  ◎ main", "", 80).trim_end(), bx("  2  ◎ b-claude", "", 80).trim_end(), bx("  3  ◎ custom · type your own", "", 80).trim_end(), bx("  4  ├ decide yourself", "", 80).trim_end(), bx("  5  ╰ skip it", "", 80).trim_end()]
    );
}

#[test]
fn a_text_question_says_to_type_the_answer() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, vec![AskView { shape: Shape::Text, question: "name the new connector?".into(), asker: "b".into(), ..AskView::default() }]);
    let b = boxed(&lines(&mut a, 80, 24));
    assert!(b.iter().any(|l| l.contains("type the answer and send it")), "{b:#?}");
    assert!(b.iter().all(|l| !l.contains("1 ")), "no numbered answers: {b:#?}");
}

#[test]
fn when_the_screen_is_short_what_matters_least_goes_first_and_the_answers_stay() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, vec![approval()]);
    let mut sizes = Vec::new();
    for rows in (7..=24).rev() {
        let s = lines(&mut a, 80, rows);
        assert_eq!(s.len(), rows as usize);
        let b = boxed(&s);
        sizes.push(b.len());
        if rows >= 12 {
            assert!(b.iter().filter(|l| l.contains("yes") || l.contains(" no") || l.contains("stop B")).count() >= 4, "{rows}: {s:#?}");
            assert!(b.iter().any(|l| l.contains("commit to b-claude?")), "{rows}: {s:#?}");
        }
    }
    assert!(sizes.windows(2).all(|w| w[0] >= w[1]), "a shorter screen never gives the box more: {sizes:?}");
    assert!(sizes[0] > sizes[sizes.len() - 1]);
    // the blank rows go first, then the keys, the reason, the change, the facts; the lane that asked goes last of all
    let nine = boxed(&lines(&mut a, 80, 14));
    assert!(nine.iter().all(|l| !l.contains("1–4")), "{nine:#?}");
}

#[test]
fn every_size_is_drawn_without_a_panic_in_every_tier_and_nothing_is_wider_than_the_screen() {
    for marks in ["icons", "unicode", "ascii", "words"] {
        for asks in [vec![approval()], vec![approval(), choice("plan", "a", &["x", "y"]), choice("how", "c", &["1", "2"])]] {
            let mut a = started(Json::Null, lanes());
            tier(&mut a, marks);
            open(&mut a, asks);
            for (c, r) in [(1, 1), (2, 2), (5, 3), (12, 5), (13, 4), (20, 6), (30, 8), (60, 20), (80, 24), (200, 5), (10, 60)] {
                let s = lines(&mut a, c, r);
                assert_eq!(s.len(), r as usize, "{marks} {c}x{r}");
            }
        }
    }
}

// ---- the tray ----

fn three() -> Vec<AskView> {
    let mut plan = choice("plan", "a", &["cache", "split"]);
    plan.wait = "4:10".into();
    plan.question = "which plan should claude build?".into();
    plan.options[0] = Choice { recommended: true, by: By::Model, note: "−60%".into(), ..plan.options[0].clone() };
    plan.options[1].note = "−25%".into();
    let build = AskView { ask: "a1".into(), asker: "b".into(), question: "cargo build --release?".into(), shape: Shape::Approval, risk: 40, wait: "0:12".into(), ..AskView::default() };
    let score = AskView { question: "how useful was this review?".into(), ..choice("how", "c", &["1", "2", "3", "4", "5"]) };
    vec![plan, build, score]
}

fn four() -> Vec<AskView> {
    let mut v = three();
    v.push(AskView { ask: "name".into(), asker: "j4n".into(), question: "name the new connector?".into(), shape: Shape::Text, ..AskView::default() });
    v
}

#[test]
fn two_or_more_questions_are_a_tray_one_row_for_each_and_the_one_the_keys_are_on_opens_in_place() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, four());
    let s = lines(&mut a, 80, 24);
    let w = 80;
    assert_eq!(
        boxed(&s),
        [
            rule('╭', '╮', w),
            bx("◇ 4", "↑↓ · 1–9 · esc", w),
            bx("├ A omp     which plan should claude build?", "◔ 4:10 ▾", w),
            bx("│  ├ ⇥~1  ◉ cache", "~ −60%", w),
            bx("│  ├   2  ◎ split", "−25%", w),
            bx("│  ├   3  ◎ custom · type your own", "", w),
            bx("│  ├   4  ├ decide yourself", "", w),
            bx("│  ╰   5  ╰ skip it", "", w),
            bx("├ B claude  cargo build --release?", "▕██▍░░░▏ 40  1 ✓  2 ✗", w),
            bx("├ C pi      how useful was this review?", "★ 1 2 3 4 5", w),
            bx("╰ j4n       name the new connector?", "✎", w),
            rule('╰', '╯', w),
        ]
        .map(|l| l.trim_end().to_string())
        .to_vec(),
        "{s:#?}"
    );
}

#[test]
fn the_tray_row_of_each_kind_shows_the_marks_of_its_answers_a_score_in_one_row_and_a_text_a_mark() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, four());
    hit(&mut a, &["down", "down"]);
    let b = boxed(&lines(&mut a, 80, 24));
    let on = banded(&mut a, 80, 24);
    assert!(on.iter().any(|l| l.contains("├ C pi      how useful was this review?") && l.trim_end().ends_with("★ 1 2 3 4 5 │")), "{on:#?}");
    assert!(b.iter().all(|l| !l.contains("1  1")), "a score is not opened into a list: {b:#?}");
}

#[test]
fn down_and_up_move_between_the_questions_round_the_ends_and_the_composer_target_follows() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, four());
    let at = |a: &mut App| banded(a, 80, 24).into_iter().find(|l| l.starts_with("│ ├") || l.starts_with("│ ╰")).unwrap_or_default();
    let target = |a: &mut App| lines(a, 80, 24).iter().find(|l| l.contains("→ ◇")).cloned().unwrap_or_default();
    assert!(at(&mut a).contains("A omp") && target(&mut a).ends_with("→ ◇ A │"));
    assert!(hit(&mut a, &["down"]).is_empty());
    assert!(at(&mut a).contains("B claude") && target(&mut a).ends_with("→ ◇ B │"), "{}", target(&mut a));
    hit(&mut a, &["down", "down"]);
    assert!(at(&mut a).contains("j4n"));
    hit(&mut a, &["down"]);
    assert!(at(&mut a).contains("A omp"), "round the end");
    hit(&mut a, &["up"]);
    assert!(at(&mut a).contains("j4n"), "and back round the other way");
}

#[test]
fn a_digit_answers_the_question_the_keys_are_on_not_the_first() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, four());
    assert_eq!(hit(&mut a, &["1"]), [Up::Intent { intent: Intent::Answer { ask: "plan".into(), reply: Reply::Pick { id: "cache".into() } } }]);
    hit(&mut a, &["down"]);
    assert_eq!(hit(&mut a, &["2"]), [answer("a1", Reply::No)]);
    hit(&mut a, &["down"]);
    assert_eq!(hit(&mut a, &["4"]), [Up::Intent { intent: Intent::Answer { ask: "how".into(), reply: Reply::Pick { id: "4".into() } } }]);
}

#[test]
fn the_arrows_are_the_composers_history_when_one_question_is_open_or_a_line_is_being_typed() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, vec![approval()]);
    typing(&mut a, "hello");
    hit(&mut a, &["enter"]);
    open(&mut a, vec![approval()]);
    hit(&mut a, &["up"]);
    assert_eq!(a.line.buf, "hello", "one question: ↑ is the history");
    hit(&mut a, &["ctrl+u"]);
    open(&mut a, four());
    typing(&mut a, "x");
    hit(&mut a, &["down"]);
    assert!(banded(&mut a, 80, 24).iter().any(|l| l.starts_with("│ ├ A omp")), "with a line being typed the focus stays");
}

#[test]
fn the_question_the_keys_are_on_stays_while_it_is_open_and_goes_back_to_the_first_when_it_is_answered() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, four());
    hit(&mut a, &["down", "down"]);
    open(&mut a, four());
    assert!(banded(&mut a, 80, 24).iter().any(|l| l.starts_with("│ ├ C pi")), "a list sent again changes nothing");
    let mut rest = four();
    rest.remove(2);
    open(&mut a, rest);
    assert!(banded(&mut a, 80, 24).iter().any(|l| l.starts_with("│ ├ A omp")), "the one it was on is gone");
}

#[test]
fn escape_puts_every_question_off_and_a_question_mark_brings_them_back_and_a_new_one_comes_at_once() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, four());
    assert!(hit(&mut a, &["esc"]).is_empty());
    let s = lines(&mut a, 80, 24);
    assert!(boxed(&s).is_empty(), "{s:#?}");
    assert!(s.iter().any(|l| l.ends_with("→ ◇ A │")), "the line is still for the first: {s:#?}");
    assert_eq!(hit(&mut a, &["1"]), [], "nothing is answered by a key that has no question to answer");
    assert_eq!(a.line.buf, "1");
    hit(&mut a, &["ctrl+u"]);
    let mut more = four();
    more.push(choice("late", "b", &["x"]));
    open(&mut a, more);
    let b = boxed(&lines(&mut a, 80, 24));
    assert!(b.iter().any(|l| l.contains("which late?")) && b.iter().all(|l| !l.contains("which plan should")), "only the new one is shown: {b:#?}");
    hit(&mut a, &["esc", "?"]);
    assert!(boxed(&lines(&mut a, 80, 24)).iter().any(|l| l.contains("which plan should")));
}

#[test]
fn escape_with_a_line_in_the_composer_keeps_the_line_and_the_question() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, vec![approval()]);
    typing(&mut a, "wait");
    hit(&mut a, &["esc"]);
    assert_eq!(a.line.buf, "wait", "esc never drops a typed line");
    assert!(!boxed(&lines(&mut a, 80, 24)).is_empty());
}

#[test]
fn the_look_of_the_ask_field_says_where_one_and_many_are_drawn() {
    let mut a = started(json!({"one": "tray", "many": "popup"}), lanes());
    open(&mut a, vec![approval()]);
    assert!(boxed(&lines(&mut a, 80, 24)).iter().any(|l| l.starts_with("│ ◇ 1")), "one question in a tray");
    open(&mut a, four());
    let b = boxed(&lines(&mut a, 80, 24));
    assert!(b.iter().any(|l| l.contains("which plan should claude build?")) && b.iter().all(|l| !l.contains("↑↓ ·")), "many questions, the focused one in a popup: {b:#?}");
    let mut a = started(json!({"one": "inline", "many": "inline"}), lanes());
    open(&mut a, four());
    assert!(boxed(&lines(&mut a, 80, 24)).is_empty(), "inline only");
    assert!(lines(&mut a, 80, 24).iter().any(|l| l.contains("→ ◇ A")));
}

#[test]
fn a_tray_that_is_too_tall_keeps_the_question_the_keys_are_on_in_sight_and_drops_those_farthest_from_it() {
    let mut a = started(Json::Null, lanes());
    let many: Vec<AskView> = (0..12).map(|i| AskView { ask: format!("q{i}"), asker: "j4n".into(), question: format!("question {i}?"), shape: Shape::Text, ..AskView::default() }).collect();
    open(&mut a, many);
    for step in 0..12 {
        let s = lines(&mut a, 60, 16);
        let b = boxed(&s);
        assert!(b.len() <= 12, "{step}: {b:#?}");
        let on = banded(&mut a, 60, 16);
        assert!(on.iter().any(|l| l.contains(&format!("j4n  question {step}?"))), "{step}: {on:#?}");
        hit(&mut a, &["down"]);
    }
}

#[test]
fn the_tray_in_every_tier_has_a_row_for_each_question_in_a_box_no_wider_than_the_screen() {
    for (marks, corner) in [("ascii", '+'), ("words", '╭'), ("icons", '╭'), ("unicode", '╭')] {
        let mut a = started(Json::Null, lanes());
        tier(&mut a, marks);
        open(&mut a, four());
        let s = lines(&mut a, 80, 24);
        let at: Vec<usize> = ["which plan", "cargo build", "how useful", "name the new"].iter().map(|q| s.iter().position(|l| l.contains(q)).unwrap_or_else(|| panic!("{marks}: {q} in {s:#?}"))).collect();
        assert!(at.windows(2).all(|w| w[0] < w[1]), "{marks}: in order, {at:?}");
        let tops: Vec<usize> = s.iter().enumerate().filter(|(_, l)| l.starts_with(corner) && l.chars().filter(|c| *c == '-' || *c == '─').count() > 60).map(|(i, _)| i).collect();
        assert!(tops.iter().any(|t| *t < at[0]), "{marks}: the box opens above the first row, {tops:?}");
        assert!(s.iter().all(|l| l.chars().count() <= 80));
    }
}

// ---- the keys ----

#[test]
fn a_digit_is_the_option_with_that_number_and_stop_is_an_order_to_the_task() {
    let mut a = started(Json::Null, lanes());
    let mut ap = approval();
    ap.grants.clear();
    open(&mut a, vec![ap]);
    assert_eq!(hit(&mut a, &["1"]), [answer("a1", Reply::Yes)]);
    assert_eq!(hit(&mut a, &["2"]), [answer("a1", Reply::No)]);
    assert_eq!(hit(&mut a, &["3"]), [Up::Intent { intent: Intent::Control { task: "b".into(), control: Control::Abort } }], "with no grant offered the third is stop");
    assert!(a.line.buf.is_empty());
}

#[test]
fn an_answer_that_widens_is_pressed_twice_and_the_screen_says_so_between() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, vec![approval()]);
    assert!(hit(&mut a, &["3"]).is_empty());
    assert!(boxed(&lines(&mut a, 80, 24)).iter().any(|l| l.contains("3 again")), "{:#?}", boxed(&lines(&mut a, 80, 24)));
    assert_eq!(hit(&mut a, &["3"]), [answer("a1", Reply::Grant { scope: "session".into() })]);
    assert!(boxed(&lines(&mut a, 80, 24)).iter().all(|l| !l.contains("again")), "and it is not armed any more");
    assert!(hit(&mut a, &["3"]).is_empty(), "the next press arms it again");
}

#[test]
fn any_other_key_lets_go_of_an_answer_that_waits_for_its_second_press() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, vec![approval()]);
    hit(&mut a, &["3"]);
    hit(&mut a, &["?"]);
    assert!(hit(&mut a, &["3"]).is_empty(), "it had to start again");
    assert_eq!(hit(&mut a, &["4"]), [Up::Intent { intent: Intent::Control { task: "b".into(), control: Control::Abort } }], "another answer is not held back by it");
    assert!(hit(&mut a, &["3"]).is_empty(), "and it let go of the first");
}

#[test]
fn an_answer_that_widens_is_armed_for_its_own_question_only() {
    let mut a = started(Json::Null, lanes());
    let other = AskView { ask: "a2".into(), ..approval() };
    open(&mut a, vec![approval(), other.clone()]);
    hit(&mut a, &["3"]);
    open(&mut a, vec![other]);
    assert!(hit(&mut a, &["3"]).is_empty(), "the question it was armed for is gone");
}

#[test]
fn a_digit_past_the_options_is_a_letter_of_the_line_except_the_one_line_for_words_of_ones_own() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, vec![choice("branch", "b", &["main", "b-claude"])]);
    assert!(hit(&mut a, &["3"]).is_empty() && a.line.buf.is_empty() && a.ui.typing.as_deref() == Some("branch"), "the row `custom` is the composer, which has the keys already");
    assert!(matches!(hit(&mut a, &["4"])[..], [Up::Intent { intent: Intent::Answer { reply: Reply::Text { .. }, .. } }]), "the next row is an answer that is ready");
    assert!(hit(&mut a, &["7"]).is_empty() && a.line.buf == "7", "{}", a.line.buf);
    let mut t = started(Json::Null, lanes());
    open(&mut t, vec![AskView { shape: Shape::Text, ..approval() }]);
    hit(&mut t, &["1"]);
    assert_eq!(t.line.buf, "1", "a text question has no numbered answers");
}

#[test]
fn a_digit_is_a_letter_while_a_line_is_being_typed_and_a_chord_with_control_is_not_an_answer() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, vec![approval()]);
    typing(&mut a, "x");
    assert!(hit(&mut a, &["1"]).is_empty());
    assert_eq!(a.line.buf, "x1");
    hit(&mut a, &["ctrl+u", "ctrl+1"]);
    assert!(a.line.buf.is_empty());
}

#[test]
fn right_takes_the_hint_of_a_rule_and_of_a_choice_and_not_a_models_on_an_approval() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, vec![approval()]);
    assert_eq!(hit(&mut a, &["right"]), [answer("a1", Reply::Yes)]);
    let mut model = approval();
    model.options[0].by = By::Model;
    open(&mut a, vec![model]);
    assert!(hit(&mut a, &["right"]).iter().all(|u| !matches!(u, Up::Intent { .. })), "nothing is answered");
    let mut c = choice("plan", "a", &["x", "y"]);
    c.options[1].recommended = true;
    c.options[1].by = By::Model;
    open(&mut a, vec![c]);
    assert_eq!(hit(&mut a, &["right"]), [answer("plan", Reply::Pick { id: "y".into() })]);
    open(&mut a, vec![choice("plan", "a", &["x", "y"])]);
    assert!(hit(&mut a, &["right"]).iter().all(|u| !matches!(u, Up::Intent { .. })), "no hint, nothing to take");
}

#[test]
fn tab_takes_the_hint_on_an_empty_line() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, vec![approval()]);
    assert_eq!(hit(&mut a, &["tab"]), [answer("a1", Reply::Yes)], "the mark `⇥ 1` is true: ⇥ on an empty line takes it");
    assert!(hit(&mut a, &["backtab"]).iter().all(|u| !matches!(u, Up::Intent { .. })), "⇧⇥ does not");
    let mut model = approval();
    model.options[0].by = By::Model;
    open(&mut a, vec![model]);
    assert!(hit(&mut a, &["tab"]).iter().all(|u| !matches!(u, Up::Intent { .. })), "a model's pick on an approval is no hint, for ⇥ either");
    open(&mut a, vec![approval()]);
    typing(&mut a, "no");
    assert!(hit(&mut a, &["tab"]).iter().all(|u| !matches!(u, Up::Intent { .. })) && a.line.buf == "no", "on a typed line ⇥ answers nothing and types nothing");
    hit(&mut a, &["left"]);
    assert!(hit(&mut a, &["right"]).is_empty() && a.line.at == 2, "→ inside the line moves the cursor");
    hit(&mut a, &["left"]);
    assert_eq!(hit(&mut a, &["ctrl+right"]), [answer("a1", Reply::Yes)], "⌃→ takes it wherever the cursor is");
    hit(&mut a, &["end"]);
    assert_eq!(hit(&mut a, &["right"]), [answer("a1", Reply::Yes)], "→ at the end of the line takes it");
}

#[test]
fn enter_with_an_empty_line_opens_the_change_of_an_approval_and_picks_the_hint_of_a_choice_and_otherwise_does_nothing() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, vec![AskView { change: Vec::new(), ..approval() }]);
    assert!(hit(&mut a, &["enter"]).is_empty());
    assert!(boxed(&lines(&mut a, 80, 30)).iter().all(|l| !l.contains("fn parse_args")), "an approval with no change has none to open");
    let mut c = choice("plan", "a", &["x", "y"]);
    c.options[0].recommended = true;
    open(&mut a, vec![c.clone()]);
    assert_eq!(hit(&mut a, &["enter"]), [answer("plan", Reply::Pick { id: "x".into() })]);
    c.options[0].recommended = false;
    open(&mut a, vec![c]);
    assert!(hit(&mut a, &["enter"]).is_empty());
}

#[test]
fn enter_with_a_line_sends_it_as_a_line_for_the_binder_to_read() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, vec![approval()]);
    typing(&mut a, "yes please");
    let sent = hit(&mut a, &["enter"]);
    assert!(matches!(&sent[..], [Up::Intent { intent: Intent::Say { text, .. } }] if text == "yes please"), "{sent:?}");
}

#[test]
fn the_keys_work_when_the_transcript_had_them_and_a_digit_takes_them_back() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, vec![approval()]);
    hit(&mut a, &["ctrl+up"]);
    assert_eq!(hit(&mut a, &["1"]), [answer("a1", Reply::Yes)]);
}

#[test]
fn the_chords_with_alt_answer_the_question_the_keys_are_on_and_the_first_when_all_are_put_off() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, three());
    hit(&mut a, &["down"]);
    assert_eq!(hit(&mut a, &["alt+y"]), [answer("a1", Reply::Yes)]);
    hit(&mut a, &["esc"]);
    assert!(hit(&mut a, &["alt+y"]).is_empty(), "the first is a choice, which is no yes or no");
    assert!(hit(&mut a, &["alt+1"]).is_empty(), "⌥1 is a key of the tabs, never a pick");
}

// ---- inline ----

/// The line of the chat that is the question `a1`.
fn inline_row(a: &mut App) -> String {
    lines(a, 80, 24).into_iter().find(|l| l.contains("commit to b-claude?") && l.starts_with("│  ╰─")).unwrap_or_default()
}

#[test]
fn a_question_that_is_open_shows_the_marks_of_its_answers_and_its_hint_at_the_end_of_its_row() {
    let mut a = started(json!({"one": "inline"}), lanes());
    open(&mut a, vec![approval()]);
    assert_eq!(inline_row(&mut a), flush("│  ╰─ ◇ commit to b-claude?", "1 ✓  2 ✗   ⇥ 1", 79));
}

fn flush(left: &str, right: &str, w: usize) -> String {
    format!("{left}{}{right}", " ".repeat(w - left.chars().count() - right.chars().count()))
}

#[test]
fn a_model_pick_on_an_approval_is_not_a_hint_inline_and_on_a_choice_it_is_marked_with_a_tilde() {
    let mut a = started(json!({"one": "inline"}), lanes());
    let mut model = approval();
    model.options[0].by = By::Model;
    open(&mut a, vec![model]);
    assert_eq!(inline_row(&mut a), flush("│  ╰─ ◇ commit to b-claude?", "1 ✓  2 ✗", 79));
    let mut c = choice("plan", "b", &["x", "y"]);
    c.ask = "a1".into();
    c.options[1].recommended = true;
    c.options[1].by = By::Model;
    open(&mut a, vec![c]);
    assert_eq!(inline_row(&mut a), flush("│  ╰─ ◇ commit to b-claude?", "⋮ 2  ~⇥ 2", 79));
}

#[test]
fn a_score_a_text_and_a_choice_each_have_their_own_mark_inline() {
    let mut a = started(json!({"one": "inline"}), lanes());
    let mut s = choice("x", "b", &["1", "2", "3"]);
    s.ask = "a1".into();
    open(&mut a, vec![s]);
    assert_eq!(inline_row(&mut a), flush("│  ╰─ ◇ commit to b-claude?", "★ 1 2 3", 79));
    open(&mut a, vec![AskView { ask: "a1".into(), asker: "b".into(), shape: Shape::Text, ..AskView::default() }]);
    assert_eq!(inline_row(&mut a), flush("│  ╰─ ◇ commit to b-claude?", "✎", 79));
}

#[test]
fn when_the_question_is_answered_the_row_shows_what_the_core_says_of_it() {
    let mut rows = lanes();
    let at = rows.iter().position(|r| r.key == "a1").unwrap();
    rows[at].cells.push(Cell::with_icon("yes", "yes", Tone::Good));
    rows[at].cells.push(Cell::new("you 12:31", Tone::Dim));
    let mut a = started(json!({"one": "inline"}), rows);
    open(&mut a, vec![approval()]);
    assert_eq!(inline_row(&mut a), flush("│  ╰─ ◇ commit to b-claude?", "1 ✓  2 ✗   ⇥ 1", 79), "while it is open: its answers");
    open(&mut a, Vec::new());
    assert_eq!(inline_row(&mut a), flush("│  ╰─ ◇ commit to b-claude?", "✓ yes  you 12:31", 79), "after: the answer, who gave it and when");
}

#[test]
fn a_row_that_is_not_an_open_question_is_drawn_as_it_was() {
    let mut a = started(json!({"one": "inline"}), lanes());
    let before = lines(&mut a, 80, 24);
    open(&mut a, vec![AskView { ask: "elsewhere".into(), ..approval() }]);
    let after = lines(&mut a, 80, 24);
    assert_eq!(before[..before.len() - 4], after[..after.len() - 4].to_vec()[..before.len() - 4]);
}

// ---- what the binder would make of the line ----

fn target(a: &mut App) -> String {
    lines(a, 80, 24).into_iter().find(|l| l.contains("→ ◇")).unwrap_or_default()
}

fn guess(a: &mut App, v: Json) {
    a.down(Down::Value { bind: "composer:bind".into(), value: v, rev: 1 });
}

#[test]
fn the_target_names_the_question_and_what_the_binder_would_make_of_the_line_follows_it() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, vec![approval()]);
    typing(&mut a, "yes");
    assert!(target(&mut a).ends_with("→ ◇ B │"), "{}", target(&mut a));
    guess(&mut a, json!({"text": "yes", "ask": "a1", "mark": "yes", "label": "yes"}));
    assert!(target(&mut a).ends_with("→ ◇ B ✓ yes │"), "{}", target(&mut a));
    guess(&mut a, json!({"text": "no thanks", "ask": "a1", "mark": "no", "label": "no"}));
    assert!(target(&mut a).ends_with("→ ◇ B │"), "a guess for another line is not shown");
}

#[test]
fn the_guess_is_gone_when_the_line_changes_or_is_empty_or_the_core_has_none() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, vec![approval()]);
    typing(&mut a, "yes");
    guess(&mut a, json!({"text": "yes", "ask": "a1", "mark": "yes", "label": "yes"}));
    typing(&mut a, "!");
    assert!(target(&mut a).ends_with("→ ◇ B │"));
    hit(&mut a, &["ctrl+u"]);
    guess(&mut a, json!({"text": "", "ask": "a1", "mark": "yes", "label": "yes"}));
    assert!(target(&mut a).ends_with("→ ◇ B │"), "an empty line answers nothing");
    typing(&mut a, "yes");
    guess(&mut a, Json::Null);
    assert!(target(&mut a).ends_with("→ ◇ B │"));
    guess(&mut a, json!({"text": "yes", "ask": "a1"}));
    assert!(target(&mut a).ends_with("→ ◇ B │"), "{}", target(&mut a));
}

#[test]
fn the_guess_names_the_lane_of_the_question_the_binder_would_answer_not_the_one_the_keys_are_on() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, three());
    typing(&mut a, "no");
    guess(&mut a, json!({"text": "no", "ask": "a1", "mark": "no", "label": "no"}));
    assert!(target(&mut a).ends_with("→ ◇ B ✗ no │"), "{}", target(&mut a));
    guess(&mut a, json!({"text": "no", "ask": "gone", "mark": "no", "label": "no"}));
    assert!(target(&mut a).ends_with("→ ◇ A ✗ no │"), "a question that is not open falls back on the target: {}", target(&mut a));
}

#[test]
fn a_draft_is_sent_after_a_pause_while_a_question_is_open_even_with_shaping_off_and_never_when_none_is() {
    use std::time::{Duration, Instant};
    let mut a = started(Json::Null, lanes());
    let t0 = Instant::now();
    typing(&mut a, "yes");
    a.tick(t0);
    assert!(a.tick(t0 + Duration::from_secs(2)).is_empty(), "no question, no shaping: nothing to feed");
    open(&mut a, vec![approval()]);
    hit(&mut a, &["ctrl+u"]);
    typing(&mut a, "yes");
    a.tick(t0 + Duration::from_secs(3));
    assert!(a.tick(t0 + Duration::from_millis(3300)).is_empty(), "not before the pause");
    assert_eq!(a.tick(t0 + Duration::from_secs(5)), [Up::Draft { text: "yes".into(), target: String::new() }]);
    assert!(a.tick(t0 + Duration::from_secs(9)).is_empty(), "once");
    hit(&mut a, &["ctrl+u"]);
    typing(&mut a, ":fresh");
    a.tick(t0 + Duration::from_secs(10));
    assert!(a.tick(t0 + Duration::from_secs(12)).is_empty(), "a command is not a draft");
}

#[test]
fn the_composer_frame_is_warm_while_a_question_is_put_off_too() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, vec![approval()]);
    hit(&mut a, &["esc"]);
    // the toast that says where the question is has its own box: the frame under test is the composer's
    a.toast = None;
    let ansi = Headless::new(40, 8).snap(&mut a).ansi;
    let top = ansi.lines().find(|l| l.contains('╭')).unwrap().to_string();
    let v = a.theme.roles.get(j4n_tui::theme::palette::Role::Warn);
    assert!(top.contains(&format!("38;2;{};{};{}", v.0, v.1, v.2)), "{top}");
}

// ---- speed ----

#[test]
fn two_hundred_open_questions_and_a_transcript_of_five_thousand_rows_are_drawn_and_stepped_through_without_a_wait() {
    let mut rows = lanes();
    for i in 0..5000 {
        rows.push(row(&format!("s{i}"), &format!("step {i}"), "read", "b"));
    }
    let mut a = started(Json::Null, rows);
    let many: Vec<AskView> = (0..200).map(|i| AskView { ask: format!("q{i}"), asker: if i % 2 == 0 { "b".into() } else { format!("t{i}") }, question: format!("question {i}?"), shape: Shape::Choice, options: vec![pick("x", "x"), pick("y", "y")], ..AskView::default() }).collect();
    open(&mut a, many);
    let mut h = Headless::new(120, 40);
    h.show(&mut a);
    // the CPU the thread spent, not the time that passed
    let ((), cpu) = crate::cpu::of(|| {
        for _ in 0..40 {
            hit(&mut a, &["down"]);
            h.show(&mut a);
        }
    });
    let each = cpu / 40;
    eprintln!("a key and its frame with 200 questions and 5,000 rows: {each:?} of CPU");
    crate::cpu::assert_within(std::time::Duration::from_millis(if cfg!(debug_assertions) { 400 } else { 40 }), "a frame of the question", || each);
    assert!(boxed(&lines(&mut a, 120, 40)).len() <= 40);
}

// ---- edge cases ----

fn colour(a: &App, r: j4n_tui::theme::palette::Role) -> String {
    let v = a.theme.roles.get(r);
    format!("38;2;{};{};{}", v.0, v.1, v.2)
}

fn ansi_row(a: &mut App, cols: u16, rows: u16, needle: &str) -> String {
    Headless::new(cols, rows).snap(a).ansi.lines().find(|l| l.contains(needle)).unwrap_or_else(|| panic!("{needle} is shown")).to_string()
}

#[test]
fn an_overlay_can_make_a_fixed_answer_widen_and_an_option_of_a_choice_can_too() {
    let mut a = approval();
    a.options.push(Choice { id: "no".into(), widens: true, ..Choice::default() });
    assert!(options(&a, "B")[1].widens);
    let mut c = choice("plan", "a", &["x", "y"]);
    c.options[1].widens = true;
    let o = options(&c, "A");
    assert!(!o[0].widens && o[1].widens);
    let mut app = started(Json::Null, lanes());
    open(&mut app, vec![c]);
    assert!(hit(&mut app, &["2"]).is_empty(), "armed, not sent");
    assert_eq!(hit(&mut app, &["2"]), [answer("plan", Reply::Pick { id: "y".into() })]);
}

#[test]
fn only_a_choice_can_be_a_score() {
    let mut a = approval();
    a.options = vec![pick("1", "1"), pick("2", "2")];
    assert!(!is_score(&a) && shape_icon(&a) == "risky");
}

#[test]
fn what_the_screen_kept_about_a_question_is_gone_with_it_when_the_same_id_comes_again() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, vec![approval()]);
    hit(&mut a, &["enter", "?"]);
    hit(&mut a, &["3"]);
    open(&mut a, Vec::new());
    open(&mut a, vec![approval()]);
    let b = boxed(&lines(&mut a, 80, 30));
    assert!(b.iter().all(|l| !l.contains("fn parse_args")), "its diff is shut: {b:#?}");
    assert!(b.iter().all(|l| !l.contains("A commit changes")), "and its reason: {b:#?}");
    assert!(hit(&mut a, &["3"]).is_empty(), "and the second press it waited for is not there");
    assert!(b.iter().all(|l| !l.contains("3 again")));
}

#[test]
fn a_chord_with_control_or_alt_is_never_an_answer_by_number() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, vec![approval()]);
    assert!(hit(&mut a, &["ctrl+1"]).is_empty());
    assert!(hit(&mut a, &["alt+1"]).is_empty(), "alt-1 picks from a choice, and an approval is none");
    assert!(a.line.buf.is_empty());
}

#[test]
fn after_esc_and_the_question_mark_the_keys_are_on_the_first_question_again() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, four());
    hit(&mut a, &["down", "down"]);
    hit(&mut a, &["esc", "?"]);
    assert!(banded(&mut a, 80, 24).iter().any(|l| l.starts_with("│ ├ A omp")));
}

#[test]
fn the_question_mark_brings_back_all_that_was_put_off_even_when_a_new_question_is_shown_that_has_a_reason() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, three());
    hit(&mut a, &["esc"]);
    let mut all = three();
    all.push(AskView { ask: "a9".into(), ..approval() });
    open(&mut a, all);
    let b = boxed(&lines(&mut a, 80, 30));
    assert!(b.iter().any(|l| l.contains("commit to b-claude?")) && b.iter().all(|l| !l.contains("which plan should")), "only the new one: {b:#?}");
    hit(&mut a, &["?"]);
    let b = boxed(&lines(&mut a, 80, 30));
    assert!(b.iter().all(|l| !l.contains("A commit changes")), "it was not a request for the reason: {b:#?}");
    for q in ["which plan should", "cargo build --release?", "how useful", "commit to b-claude?"] {
        assert!(b.iter().any(|l| l.contains(q)), "{q}: {b:#?}");
    }
}

#[test]
fn pressing_another_answer_that_widens_arms_that_one_and_sends_nothing() {
    let mut a = started(Json::Null, lanes());
    let mut two = approval();
    two.grants = vec!["session".into(), "here".into()];
    open(&mut a, vec![two]);
    assert!(hit(&mut a, &["3"]).is_empty());
    assert!(hit(&mut a, &["4"]).is_empty(), "this is another answer: it waits for its own second press");
    assert_eq!(hit(&mut a, &["4"]), [answer("a1", Reply::Grant { scope: "here".into() })]);
}

#[test]
fn a_box_is_never_drawn_with_nothing_inside_it() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, vec![approval()]);
    for rows in 4..=14 {
        let s = lines(&mut a, 80, rows);
        let b = boxed(&s);
        assert!(b.is_empty() || b.len() >= 3, "{rows}: {b:#?}");
    }
}

#[test]
fn the_clock_gives_way_to_the_lane_when_there_are_not_two_blanks_between_them() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, vec![approval()]);
    let head = |a: &mut App, w: u16| boxed(&lines(a, w, 30)).get(1).cloned().unwrap_or_default();
    assert_eq!(head(&mut a, 21), bx("◇ B claude", "", 21), "10 and 6 cells in 17: no room for two blanks");
    assert_eq!(head(&mut a, 22), bx("◇ B claude", "◔ 8:42", 22));
}

#[test]
fn a_long_label_leaves_two_blanks_before_the_note_at_the_right() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, vec![approval()]);
    let b = boxed(&lines(&mut a, 40, 30));
    let row = b.iter().find(|l| l.contains("yes · commits")).expect("the third answer").clone();
    assert!(row.ends_with("…  ⚠ widens │"), "{row}");
}

#[test]
fn the_rows_of_a_change_have_the_words_a_dim_remark_after_them_and_their_stats_at_the_right() {
    let mut a = started(Json::Null, lanes());
    let mut ap = approval();
    ap.change[0].cells[1] = Cell::new("the comma branch", Tone::Plain);
    open(&mut a, vec![ap]);
    let b = boxed(&lines(&mut a, 80, 30));
    assert!(b.iter().any(|l| l.starts_with("│ ├ ± parser.rs  the comma branch") && l.trim_end().ends_with("+12 −3 │")), "{b:#?}");
    assert!(b.iter().any(|l| l.starts_with("│ ╰ ± syntax.md") && l.trim_end().ends_with("+4 −4 │")), "{b:#?}");
}

#[test]
fn a_removed_line_is_known_by_the_minus_sign_of_a_diff_as_well_as_the_hyphen() {
    let mut a = started(Json::Null, lanes());
    let mut ap = approval();
    ap.change[1].detail = vec![Cell::new("−old words", Tone::Plain), Cell::new("-older words", Tone::Plain), Cell::new("+new words", Tone::Plain)];
    open(&mut a, vec![ap]);
    hit(&mut a, &["enter"]);
    let del = colour(&a, j4n_tui::theme::palette::Role::Del);
    assert!(ansi_row(&mut a, 80, 30, "−old words").contains(&del));
    assert!(ansi_row(&mut a, 80, 30, "-older words").contains(&del));
    assert!(!ansi_row(&mut a, 80, 30, "+new words").contains(&del));
}

#[test]
fn facts_that_do_not_fit_on_one_row_have_a_row_each_cut_to_the_width_and_never_run_past_the_box() {
    let long = |t: &str| Cell { text: t.into(), tone: Tone::Dim, ..Cell::default() };
    let facts = vec![long("the quick look: claude is signed in; its cheapest model is haiku"), long("Main: claude is signed in; its newest Sonnet is sonnet, which is a long word"), long("a third fact that is longer than the box is wide, so that it is cut to fit inside its edge")];
    for ask in [AskView { facts: facts.clone(), ..choice("need", "j4n", &["use the suggestion", "pick another"]) }, AskView { facts, ..approval() }] {
        let mut a = started(Json::Null, lanes());
        open(&mut a, vec![ask]);
        let s = lines(&mut a, 80, 30);
        let b = boxed(&s);
        assert!(b.iter().all(|l| unicode_width::UnicodeWidthStr::width(l.as_str()) <= 80), "no row is wider than the screen: {b:#?}");
        assert!(b.iter().any(|l| l.starts_with("│ the quick look:")) && b.iter().any(|l| l.starts_with("│ Main:")) && b.iter().any(|l| l.starts_with("│ a third fact")), "each fact has a row: {b:#?}");
        assert!(b.iter().filter(|l| l.contains("fact") || l.contains("Main:") || l.contains("quick look")).all(|l| l.ends_with('│')), "and ends inside the edge: {b:#?}");
    }
}

#[test]
fn a_risk_over_a_hundred_is_a_hundred() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, vec![AskView { risk: 250, ..approval() }]);
    let b = boxed(&lines(&mut a, 80, 30));
    assert!(b.iter().any(|l| l.starts_with("│ ▕████████▏ 100")), "{b:#?}");
}

#[test]
fn the_effect_is_warm_from_a_risk_of_fifty_and_dim_below() {
    for (risk, role) in [(49, j4n_tui::theme::palette::Role::Dim), (50, j4n_tui::theme::palette::Role::Warn)] {
        let mut a = started(Json::Null, lanes());
        open(&mut a, vec![AskView { risk, effect: "shell: make build".into(), change: Vec::new(), ..approval() }]);
        let c = colour(&a, role);
        assert!(ansi_row(&mut a, 80, 30, "shell: make build").contains(&c), "risk {risk}");
    }
}

#[test]
fn one_answer_is_keyed_as_a_single_one() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, vec![choice("only", "b", &["alone"])]);
    let b = boxed(&lines(&mut a, 80, 30));
    assert!(b.iter().any(|l| l.starts_with("│ 1 · type to answer · esc later")), "{b:#?}");
}

#[test]
fn a_long_lane_name_takes_no_more_than_a_quarter_of_a_row_of_the_tray() {
    let mut rows = lanes();
    rows[1].cells[0].text = "A the-agent-with-a-very-long-name-indeed".into();
    let mut a = started(Json::Null, rows);
    open(&mut a, vec![AskView { ask: "q1".into(), asker: "a".into(), question: "which plan?".into(), shape: Shape::Text, ..AskView::default() }, choice("plan", "b", &["x"])]);
    let b = boxed(&lines(&mut a, 80, 24));
    let row = banded(&mut a, 80, 24).into_iter().find(|l| l.contains("which plan?")).expect("the first row");
    assert!(row.starts_with("│ ├ A the-agent-with-a…  which plan?"), "{row}");
    assert!(b.iter().find(|l| l.contains("which plan?")).is_some_and(|l| l.chars().count() == 80));
}

#[test]
fn a_narrow_tray_row_gives_the_marks_up_before_the_question() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, three());
    let b = boxed(&lines(&mut a, 40, 24));
    let row = b.iter().find(|l| l.contains("cargo build")).expect("the approval row").clone();
    assert!(!row.contains('▕') && row.contains("cargo build --release?"), "{row}");
    let tight = boxed(&lines(&mut a, 45, 24));
    let row = tight.iter().find(|l| l.contains("cargo build")).expect("the approval row").clone();
    assert!(!row.contains('▕') && row.contains("cargo build --release?"), "the marks go before the question is cut: {row}");
    let wide = boxed(&lines(&mut a, 64, 24));
    assert!(wide.iter().find(|l| l.contains("cargo build")).is_some_and(|l| l.contains('▕')));
}

#[test]
fn a_yes_or_no_with_a_change_opens_its_diff_with_enter_and_says_so() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, vec![AskView { shape: Shape::YesNo, grants: Vec::new(), ..approval() }]);
    let b = boxed(&lines(&mut a, 80, 30));
    assert!(b.iter().any(|l| l.contains("⏎ diff")), "{b:#?}");
    hit(&mut a, &["enter"]);
    assert!(boxed(&lines(&mut a, 80, 30)).iter().any(|l| l.contains("fn parse_args")));
}

#[test]
fn in_the_tray_enter_opens_the_change_of_the_question_the_keys_are_on_and_the_others_show_that_they_have_one() {
    let mut a = started(Json::Null, lanes());
    let mut other = approval();
    other.ask = "a2".into();
    other.asker = "a".into();
    open(&mut a, vec![approval(), other]);
    let b = boxed(&lines(&mut a, 100, 40));
    assert!(b.iter().filter(|l| l.contains("commit to b-claude?")).all(|l| l.contains("1 ✓  2 ✗   ⏎") || l.contains("▾")), "{b:#?}");
    assert!(b.iter().all(|l| !l.contains("fn parse_args")));
    hit(&mut a, &["enter"]);
    let open_ = boxed(&lines(&mut a, 100, 40));
    assert!(open_.iter().any(|l| l.contains("fn parse_args")) && open_.iter().any(|l| l.contains("+    self.bump();")), "{open_:#?}");
    hit(&mut a, &["enter"]);
    assert!(boxed(&lines(&mut a, 100, 40)).iter().all(|l| !l.contains("fn parse_args")));
    hit(&mut a, &["down", "enter"]);
    let second = boxed(&lines(&mut a, 100, 40));
    assert_eq!(second.iter().filter(|l| l.contains("fn parse_args")).count(), 1, "only the question the keys are on");
}

#[test]
fn a_guess_with_only_a_word_or_only_a_mark_is_shown_with_what_it_has() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, vec![approval()]);
    typing(&mut a, "yes");
    guess(&mut a, json!({"text": "yes", "ask": "a1", "label": "yes"}));
    assert!(target(&mut a).ends_with("→ ◇ B yes │"), "{}", target(&mut a));
    guess(&mut a, json!({"text": "yes", "ask": "a1", "mark": "yes"}));
    assert!(target(&mut a).ends_with("→ ◇ B ✓ │"), "{}", target(&mut a));
}

#[test]
fn a_list_sent_again_for_a_new_wait_leaves_the_diff_the_reason_and_the_press_that_waits_as_they_were() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, vec![approval()]);
    hit(&mut a, &["enter", "?", "3"]);
    open(&mut a, vec![AskView { wait: "8:43".into(), ..approval() }]);
    let b = boxed(&lines(&mut a, 80, 40));
    assert!(b.iter().any(|l| l.contains("fn parse_args")) && b.iter().any(|l| l.contains("A commit changes")), "{b:#?}");
    assert!(b.iter().any(|l| l.contains("3 again")) && b.iter().any(|l| l.contains("8:43")), "{b:#?}");
    assert_eq!(hit(&mut a, &["3"]), [answer("a1", Reply::Grant { scope: "session".into() })]);
}

#[test]
fn questions_that_float_take_no_rows_whatever_height_or_grow_the_template_gave_the_field() {
    let draw = |height: u8, grow: u8| {
        let mut a = started(Json::Null, lanes());
        if let Some(root) = a.tree.as_mut().map(std::sync::Arc::make_mut) {
            root.children[1].height = height;
            root.children[1].grow = grow;
        }
        open(&mut a, vec![approval()]);
        screen(&mut a, 80, 24)
    };
    let plain = draw(0, 0);
    assert!(plain.iter().any(|l| l.contains("commit to b-claude?")), "{plain:?}");
    assert_eq!(draw(5, 0), plain);
    assert_eq!(draw(0, 4), plain);
}

#[test]
fn a_change_that_only_names_its_files_offers_no_diff_key_and_enter_opens_nothing() {
    let named: Vec<Row> = change().into_iter().map(|r| Row { detail: Vec::new(), ..r }).collect();
    let mut a = started(Json::Null, lanes());
    open(&mut a, vec![AskView { change: named.clone(), ..approval() }]);
    let b = boxed(&lines(&mut a, 80, 30));
    assert!(b.iter().any(|l| l.contains("parser.rs")), "the files it would change are shown: {b:#?}");
    assert!(b.iter().all(|l| !l.contains("⏎ diff")), "there is no diff to open: {b:#?}");
    let before = lines(&mut a, 80, 30);
    assert!(hit(&mut a, &["enter"]).is_empty());
    assert_eq!(lines(&mut a, 80, 30), before, "enter did nothing");
    // in the tray, too
    open(&mut a, vec![AskView { change: named, ..approval() }, choice("a2", "c", &["x", "y"])]);
    assert!(lines(&mut a, 80, 30).iter().all(|l| !l.contains('⏎')), "no row of the tray points at a diff");
}

// ---- the choice part, the custom row and the decision-models icon ----

fn on_screen(s: &[String], from: &str) -> Vec<String> {
    let at = s.iter().position(|l| l.contains(from)).unwrap_or_else(|| panic!("no row with `{from}`:\n{}", s.join("\n")));
    s[at..].to_vec()
}

#[test]
fn a_choice_is_a_selection_with_its_recommended_row_marked_a_last_custom_row_and_the_ready_answers_under_it_at_80_columns() {
    let mut c = choice("plan", "a", &["cache", "split", "skip"]);
    c.options[1].recommended = true;
    c.options[1].by = By::Rule;
    let mut a = started(Json::Null, lanes());
    open(&mut a, vec![c.clone()]);
    let s = lines(&mut a, 80, 30);
    let rows = on_screen(&s, "1  ");
    let want = [("1", "cache"), ("2", "split"), ("3", "skip"), ("4", "custom"), ("5", "decide yourself"), ("6", "skip it")];
    for (row, (n, word)) in rows.iter().zip(want) {
        assert!(row.contains(n) && row.contains(word), "{row:?} should be {n} {word}:\n{}", s.join("\n"));
    }
    assert!(rows[1].contains('⇥') && rows[1].contains('◉'), "the recommended row is marked and filled: {:?}", rows[1]);
    assert!(rows[0].contains('◎') && !rows[0].contains('⇥'), "{:?}", rows[0]);
    assert!(rows[3].contains("type your own"), "{:?}", rows[3]);
    assert!(s.iter().any(|l| l.contains("1–3") && l.contains("type to answer")), "the keys say it:\n{}", s.join("\n"));
    // the first press of the custom row is the input: nothing is sent, and the row has it
    assert_eq!(a.key(j4n_tui::headless::press(j4n_tui::headless::KeyCode::Char('4'))), Vec::<Up>::new());
    let typed_ = lines(&mut a, 80, 30);
    assert!(on_screen(&typed_, "4  ")[0].contains('◉'), "the custom row has the input:\n{}", typed_.join("\n"));
    // a line typed with the card up is that row's answer, as text
    let sent = crate::world::press(&mut a, typed("sub chat 1", true));
    assert!(sent.contains(&Up::Intent { intent: Intent::Answer { ask: "plan".into(), reply: Reply::Text { text: "sub chat 1".into() } } }), "{sent:?}");
    // the ready answer is one key away
    let ready = options(&c, "A");
    assert_eq!(ready[4].intent(&c), Intent::Answer { ask: "plan".into(), reply: Reply::Text { text: "Decide yourself: pick what you judge best and go on.".into() } });
}

#[test]
fn the_same_part_draws_the_routing_question_of_the_front_with_no_custom_row_and_more_than_five_options_say_how_many_are_left() {
    let mut route = choice("route:1", "j4n", &["c1", "c2", "new"]);
    route.options[0].recommended = true;
    route.options[0].by = By::Rule;
    route.options[0].note = "claude · haiku".into();
    let mut a = started(Json::Null, lanes());
    open(&mut a, vec![route.clone()]);
    let s = lines(&mut a, 80, 30);
    let rows = on_screen(&s, "1  ");
    assert!(rows[0].contains('◉') && rows[0].contains("c1") && rows[0].contains("claude · haiku"), "{:?}", rows[0]);
    assert!(s.iter().all(|l| !l.contains("custom")), "a routing question takes no free line:\n{}", s.join("\n"));
    assert_eq!(options(&route, "j4n").len(), 3);
    let many = choice("big", "a", &["a1", "b2", "c3", "d4", "e5", "f6", "g7"]);
    open(&mut a, vec![many.clone()]);
    let s = lines(&mut a, 80, 30);
    assert!(s.iter().any(|l| l.contains("+2 more")), "{}", s.join("\n"));
    assert_eq!(options(&many, "A").iter().filter(|o| matches!(o.send, Send::Reply(Reply::Pick { .. }))).count(), 5);
}

#[test]
fn esc_says_where_the_question_is_and_c_brings_its_card_back_with_the_line_ready() {
    let mut a = started(Json::Null, lanes());
    open(&mut a, vec![choice("plan", "a", &["cache", "split"])]);
    a.key(j4n_tui::headless::press(j4n_tui::headless::KeyCode::Esc));
    assert!(a.toast.as_ref().is_some_and(|(t, _)| t.contains("question kept open") && t.contains("c brings it back")), "{:?}", a.toast);
    assert!(a.asks_shown().is_empty());
    a.key(j4n_tui::headless::press(j4n_tui::headless::KeyCode::Char('c')));
    assert_eq!(a.asks_shown().len(), 1);
    assert_eq!(a.ui.typing.as_deref(), Some("plan"));
}

#[test]
fn alt_digit_no_longer_answers_a_choice_question_on_any_page() {
    // ⌥2 is the second tab wherever the keys are, a page where the question is not drawn included; the question stays open
    for page in [1, 3, 4] {
        let mut a = crate::world::app();
        open(&mut a, vec![choice("plan", "a", &["x", "y", "z"])]);
        a.key(j4n_tui::headless::press(crossterm::event::KeyCode::F(page)));
        let ups = a.key(j4n_tui::headless::alt('2'));
        assert!(!ups.iter().any(|u| matches!(u, Up::Intent { intent: Intent::Answer { .. } })), "from tab {page}: {ups:?}");
        assert_eq!(a.tab, 1, "from tab {page}");
    }
}

#[test]
fn a_plain_digit_on_an_empty_line_still_does() {
    let mut a = crate::world::app();
    open(&mut a, vec![choice("plan", "a", &["x", "y", "z"])]);
    assert_eq!(hit(&mut a, &["2"]), [answer("plan", Reply::Pick { id: "y".into() })]);
}
