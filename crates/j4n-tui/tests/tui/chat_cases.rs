//! The chat field. The transcript the rows make — turn › lane › step on pipes, the steps of one kind merged, what folds and
//! what stays open, stats at the right edge, output on a raised box, the breath of the doer that works — and the composer under it:
//! the strip, the line that grows upward, its target. The sheets are the goldens of the scenes `main`, `lanes`, `grammar`, `composer`
//! and `breath`. These tests draw in the `unicode` tier, so the marks are letters a test can read.

use crate::world::{hello, screen, still};
use j4n_proto::{AskView, By, Cell, Choice, Control, Down, Field, FieldKind, Fold, Intent, Json, Node, Present, Row, Shape, Tone, Up, json};
use j4n_tui::App;
use j4n_tui::headless::{Headless, KeyCode, alt_key, ctrl, press, typed};
use j4n_tui::theme::{Settings, Theme};
use std::time::{Duration, Instant};

fn look(a: &mut App, s: Settings) {
    a.theme = Theme::build(&s, &[]).expect("a theme");
}

/// The tests of the rows and the pipes keep the operator's line as the mark (`theme.you = mark`); the band has its own tests.
fn unicode() -> Settings {
    Settings { marks: "unicode".into(), you: "mark".into(), ..Settings::default() }
}

fn started(props: Json) -> App {
    let mut a = App::new();
    a.start();
    a.down(hello());
    let chat = Node { id: "chat".into(), field: Some(Field { kind: FieldKind::Chat, bind: "chat:main".into(), props }), ..Node::default() };
    a.down(Down::Tree { node: Node { id: "root".into(), present: Present::Stack, children: vec![chat], ..Node::default() } });
    look(&mut a, unicode());
    a
}

/// A row: `what` with the mark `icon`, hanging from `parent` (none for a turn).
fn row(key: &str, what: &str, icon: &str, parent: &str) -> Row {
    Row {
        key: key.into(),
        cells: vec![Cell::with_icon(icon, what, Tone::Plain), Cell::new("", Tone::Dim)],
        parent: (!parent.is_empty()).then(|| parent.to_string()),
        ..Row::default()
    }
}

fn turn(key: &str, what: &str, time: &str) -> Row {
    stat(row(key, what, "", ""), "", time)
}

fn with(mut r: Row, text: &str) -> Row {
    r.cells[1].text = text.into();
    r
}

fn stat(mut r: Row, icon: &str, text: &str) -> Row {
    r.cells.push(Cell::with_icon(icon, text, Tone::Dim));
    r
}

fn merged(mut r: Row) -> Row {
    r.merge = true;
    r
}

fn out(mut r: Row, lines: &[&str]) -> Row {
    r.detail = lines.iter().map(|l| Cell::new(*l, Tone::Plain)).collect();
    r
}

fn send(a: &mut App, rows: Vec<Row>) {
    let total = rows.len() as u64;
    a.down(Down::Rows { bind: "chat:main".into(), rows, total, from: 0, rev: 1 });
}

fn app_with(props: Json, rows: Vec<Row>) -> App {
    let mut a = started(props);
    send(&mut a, rows);
    a
}

/// The transcript as lines: what is above the composer's frame, without the blank rows at the top.
fn transcript(a: &mut App, cols: u16, rows: u16) -> Vec<String> {
    let all = screen(a, cols, rows);
    let end = all.iter().rposition(|l| l.starts_with('╭')).unwrap_or(all.len());
    all[..end].iter().map(|l| l.trim_end().to_string()).skip_while(|l| l.is_empty()).collect()
}

fn lines(a: &mut App, cols: u16, rows: u16) -> Vec<String> {
    screen(a, cols, rows).into_iter().map(|l| l.trim_end().to_string()).collect()
}

/// A line of `w` columns with `left` at the left edge and `right` at the right (every mark of the unicode tier is one column).
fn flush(left: &str, right: &str, w: usize) -> String {
    let gap = w - left.chars().count() - right.chars().count();
    format!("{left}{}{right}", " ".repeat(gap))
}

fn hit(a: &mut App, code: KeyCode) -> Vec<Up> {
    a.key(press(code))
}

fn typing(a: &mut App, text: &str) {
    typed(text, false).into_iter().for_each(|k| drop(a.key(k)));
}

fn one_lane() -> Vec<Row> {
    vec![
        turn("t", "fix the parser", "12:29"),
        stat(stat(with(row("a", "A omp", "done", "t"), "sonnet"), "", "21s"), "", "$0.03"),
        merged(row("a1", "x.rs", "read", "a")),
        row("a2", "trailing", "search", "a"),
        stat(with(row("b", "B claude", "working", "t"), "sonnet"), "", "1:04"),
        merged(row("b1", "a.rs", "read", "b")),
        merged(row("b2", "b.rs", "read", "b")),
        merged(row("b3", "c.rs", "read", "b")),
        stat(row("b4", "parser.rs", "edit", "b"), "", "+12 −3"),
        row("b5", "Trailing commas now parse.", "", "b"),
    ]
}

// ---- pipes ----

#[test]
fn a_turn_is_a_line_of_yours_with_its_time_and_the_lanes_and_steps_of_what_it_caused_hang_from_it_on_pipes() {
    let mut a = app_with(Json::Null, one_lane());
    let s = transcript(&mut a, 60, 20);
    assert_eq!(
        s,
        [
            flush("fix the parser", "12:29", 59),
            flush("├─ ✓ A omp sonnet  ≡ 1  ⊙ 1", "21s  $0.03 ▸", 59),
            flush("╰─ ▷ B claude sonnet", "1:04", 59),
            flush("   ├─ ≡ a.rs · b.rs · c.rs", "3", 59),
            flush("   ├─ ± parser.rs", "+12 −3", 59),
            "   ╰─ Trailing commas now parse.".to_string(),
        ]
        .to_vec(),
        "{s:#?}"
    );
}

#[test]
fn the_pipe_of_a_lane_goes_on_past_its_steps_while_there_is_a_lane_after_it() {
    let mut rows = one_lane();
    rows.push(row("c", "C pi", "queued", "t"));
    let mut a = app_with(Json::Null, rows);
    let s = transcript(&mut a, 60, 20);
    assert!(s[2].starts_with("├─ ▷ B claude") && s[3].starts_with("│  ├─ ≡ ") && s[5].starts_with("│  ╰─ Trailing"), "{s:#?}");
    assert!(s[6].starts_with("╰─ ⊸ C pi"), "{s:#?}");
}

#[test]
fn turns_are_apart_by_one_blank_row_and_the_newest_is_at_the_bottom() {
    let mut a = app_with(Json::Null, vec![turn("t1", "one", "12:00"), turn("t2", "two", "12:01")]);
    let s = transcript(&mut a, 40, 12);
    assert_eq!(s, [flush("one", "12:00", 39), String::new(), flush("two", "12:01", 39)].to_vec());
    // the transcript sits on the composer: nothing is between them
    let all = lines(&mut a, 40, 12);
    assert!(all[8].starts_with("two") && all[9].starts_with('╭'), "{all:#?}");
}

#[test]
fn a_row_that_is_not_under_any_row_the_screen_has_is_a_turn() {
    let mut a = app_with(Json::Null, vec![row("x", "orphan", "done", "nowhere")]);
    assert!(transcript(&mut a, 40, 8)[0].starts_with("orphan"));
}

#[test]
fn a_lane_without_a_mark_still_has_a_blank_after_its_pipe() {
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1"), row("x", "just words", "", "t"), row("y", "after", "done", "t")]);
    let s = transcript(&mut a, 40, 8);
    assert!(s[1].starts_with("├─ just words"), "{s:#?}");
}

// ---- merging ----

#[test]
fn steps_of_one_kind_in_a_row_become_one_with_a_count_where_the_core_said_they_may() {
    let mut a = app_with(Json::Null, one_lane());
    assert!(transcript(&mut a, 60, 20).iter().any(|l| l.starts_with("   ├─ ≡ a.rs · b.rs · c.rs") && l.ends_with('3')));
    // the option turns it off
    let mut b = app_with(json!({"merge_same": false}), one_lane());
    let s = transcript(&mut b, 60, 20);
    assert_eq!(s.iter().filter(|l| l.contains("≡ ")).count(), 3 + 1, "three reads of their own and the summary of the folded lane: {s:#?}");
}

#[test]
fn a_step_the_core_did_not_mark_is_not_merged_and_neither_is_one_of_another_kind_or_with_output() {
    let mut rows = vec![turn("t", "go", "1"), row("l", "B x", "working", "t")];
    rows.push(merged(row("r1", "a.rs", "read", "l")));
    rows.push(row("r2", "b.rs", "read", "l"));
    rows.push(merged(row("r3", "c.rs", "read", "l")));
    rows.push(merged(row("s1", "x", "search", "l")));
    rows.push(out(merged(row("o1", "ls", "run", "l")), &["a"]));
    rows.push(out(merged(row("o2", "ls", "run", "l")), &["b"]));
    let mut a = app_with(Json::Null, rows);
    let s = transcript(&mut a, 60, 30);
    let body: Vec<&str> = s.iter().filter(|l| l.starts_with("  ")).map(String::as_str).collect();
    assert!(body.iter().any(|l| l.contains("≡ a.rs")) && body.iter().any(|l| l.contains("≡ b.rs")) && body.iter().any(|l| l.contains("≡ c.rs")), "{s:#?}");
    assert_eq!(body.iter().filter(|l| l.contains("❱ ls")).count(), 2, "{s:#?}");
}

#[test]
fn a_merged_row_is_the_names_once_each_in_order_and_the_count_alone_at_the_right() {
    let mut rows = vec![turn("t", "go", "1"), row("l", "B x", "working", "t")];
    for (i, n) in ["a.rs", "b.rs", "a.rs"].iter().enumerate() {
        rows.push(stat(merged(row(&format!("r{i}"), n, "read", "l")), "", "9s"));
    }
    let mut a = app_with(Json::Null, rows);
    let s = transcript(&mut a, 50, 10);
    assert_eq!(s[2], flush("   ╰─ ≡ a.rs · b.rs · a.rs", "3", 49), "{s:#?}");
}

// ---- folding ----

#[test]
fn a_lane_with_steps_shows_the_marks_of_them_with_how_many_when_it_is_folded() {
    let mut rows = vec![turn("t", "go", "1"), stat(row("a", "A omp", "done", "t"), "", "21s")];
    for (i, ic) in ["read", "read", "read", "search"].iter().enumerate() {
        rows.push(row(&format!("a{i}"), "x", ic, "a"));
    }
    rows.push(row("a9", "the answer", "", "a"));
    rows.push(row("b", "B omp", "working", "t"));
    let mut a = app_with(Json::Null, rows);
    let s = transcript(&mut a, 60, 20);
    assert_eq!(s[1], flush("├─ ✓ A omp  ≡ 3  ⊙ 1", "21s ▸", 59), "{s:#?}");
    assert_eq!(s[2], "│    the answer", "the steps are not shown, the words of the lane stand under its head: {s:#?}");
    assert_eq!(s.len(), 4, "{s:#?}");
}

#[test]
fn a_lane_that_failed_stays_open_and_so_does_one_whose_successor_has_not_started() {
    let rows = vec![
        turn("t", "go", "1"),
        row("a", "A omp", "failed", "t"),
        row("a1", "clippy", "error", "a"),
        row("b", "B omp", "done", "t"),
        row("b1", "x", "read", "b"),
        row("c", "C omp", "queued", "t"),
    ];
    let mut a = app_with(Json::Null, rows);
    let s = transcript(&mut a, 40, 20);
    assert!(s.iter().any(|l| l.contains("⊗ clippy")), "a failed lane keeps its steps: {s:#?}");
    assert!(s.iter().any(|l| l.contains("≡ x")), "a newer lane that is only queued has not started: {s:#?}");
}

#[test]
fn the_options_say_whether_settled_lanes_fold_and_failed_lanes_stay() {
    let rows = || vec![turn("t", "go", "1"), row("a", "A omp", "done", "t"), row("a1", "x", "read", "a"), row("f", "F omp", "failed", "t"), row("f1", "boom", "error", "f"), row("b", "B omp", "working", "t")];
    let mut a = app_with(json!({"fold_older": false}), rows());
    assert!(transcript(&mut a, 40, 20).iter().any(|l| l.contains("≡ x")), "settled lanes stay open");
    let mut b = app_with(json!({"keep_failed": false}), rows());
    let s = transcript(&mut b, 40, 20);
    assert!(!s.iter().any(|l| l.contains("⊗ boom")), "a failed lane folds like another: {s:#?}");
    assert!(s.iter().any(|l| l.starts_with("├─ ✗ F omp") && l.contains("⊗ 1")), "{s:#?}");
    let mut c = app_with(json!({}), rows());
    let s = transcript(&mut c, 40, 20);
    assert!(s.iter().any(|l| l.contains("⊗ boom")) && !s.iter().any(|l| l.contains("≡ x")), "a failed lane stays and the settled one folds: {s:#?}");
}

#[test]
fn the_core_can_say_a_row_is_open_shut_or_pinned_and_the_rules_do_not_override_it() {
    let mut open = row("a", "A omp", "done", "t");
    open.fold = Some(Fold::Open);
    let mut shut = row("b", "B omp", "working", "t");
    shut.fold = Some(Fold::Folded);
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1"), open, row("a1", "x", "read", "a"), shut, row("b1", "y", "read", "b"), row("c", "C omp", "working", "t")]);
    let s = transcript(&mut a, 40, 20);
    assert!(s.iter().any(|l| l.contains("≡ x")) && !s.iter().any(|l| l.contains("≡ y")), "{s:#?}");
    let mut pinned = row("p", "P omp", "done", "t");
    pinned.fold = Some(Fold::Pinned);
    let mut b = app_with(Json::Null, vec![turn("t", "go", "1"), pinned, row("p1", "z", "read", "p"), row("n", "N omp", "working", "t")]);
    assert!(transcript(&mut b, 40, 20).iter().any(|l| l.contains("≡ z")), "a pinned lane is open whatever the rule says");
}

#[test]
fn the_route_is_one_row_that_is_folded_until_it_is_opened() {
    let mut route = row("r", "omp ▸ claude ▸ pi", "route", "t");
    route.by = By::Core;
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1"), stat(route, "", "0.4s"), with(row("why", "omp 0.71", "", "r"), ""), row("l", "A x", "working", "t")]);
    let s = transcript(&mut a, 40, 10);
    assert_eq!(s[1], flush("├─ ➜ omp ▸ claude ▸ pi", "0.4s ▸", 39), "{s:#?}");
    assert!(!s.iter().any(|l| l.contains("omp 0.71")));
    hit_chord(&mut a, &["ctrl+up", "down", "enter"]);
    let s = transcript(&mut a, 40, 10);
    assert!(s[1].ends_with("0.4s ▾") && s[2].starts_with("│  ╰─ omp 0.71"), "{s:#?}");
}

fn hit_chord(a: &mut App, keys: &[&str]) -> Vec<Up> {
    keys.iter().flat_map(|k| a.key(j4n_tui::scene::key(k).unwrap_or_else(|| panic!("`{k}` is a key")))).collect()
}

// ---- control rows ----

fn controls() -> Vec<Row> {
    let mut rows = vec![turn("t", "go", "1"), row("l", "A omp", "working", "t")];
    for (k, ic, w) in [("c1", "session", "claude-7c1 resumes"), ("c2", "grant", "reads · session"), ("c3", "compact", "A at 70%")] {
        let mut r = row(k, w, ic, "t");
        r.by = By::Core;
        rows.push(r);
    }
    rows
}

#[test]
fn the_control_rows_of_a_turn_are_one_row_with_their_number_at_the_end_of_it() {
    let mut a = app_with(Json::Null, controls());
    let s = transcript(&mut a, 40, 10);
    assert_eq!(s, [flush("go", "1", 39), "├─ ▷ A omp".to_string(), flush("╰─ ∙ 3", "▸", 39)].to_vec(), "{s:#?}");
}

#[test]
fn opened_the_control_rows_are_there_and_say_they_are_in_the_context() {
    let mut a = app_with(Json::Null, controls());
    hit_chord(&mut a, &["ctrl+up", "down", "down", "enter"]);
    let s = transcript(&mut a, 40, 10);
    assert_eq!(s[2], flush("╰─ ∙ 3", "✓ in context ▾", 39), "{s:#?}");
    assert_eq!((s[3].as_str(), s[4].as_str(), s[5].as_str()), ("   ├─ ∞ claude-7c1 resumes", "   ├─ ⊞ reads · session", "   ╰─ ⊟ A at 70%"));
}

// ---- the line of a row ----

#[test]
fn the_stats_stand_at_the_right_edge_two_cells_or_more_from_the_words() {
    for w in [30u16, 40, 57, 80] {
        let mut a = app_with(Json::Null, vec![turn("t", "go", "12:00"), stat(stat(stat(with(row("b", "B claude", "working", "t"), "sonnet"), "", "1:04"), "", "$0.11"), "worktree", "b-claude")]);
        let s = transcript(&mut a, w, 8);
        assert_eq!(s[1].chars().count().max(w as usize), w as usize, "{s:#?}");
    }
    let mut a = app_with(Json::Null, vec![turn("t", "go", "12:00"), stat(stat(with(row("b", "B claude", "working", "t"), "sonnet"), "", "1:04"), "", "$0.11")]);
    let s = transcript(&mut a, 40, 8);
    assert_eq!(s[1], flush("╰─ ▷ B claude sonnet", "1:04  $0.11", 39), "{s:#?}");
    assert_eq!(s[0], flush("go", "12:00", 39));
}

#[test]
fn when_a_line_is_too_narrow_the_with_goes_first_then_the_stats_from_the_left_then_the_end_of_the_words() {
    let mk = || vec![turn("t", "go", "1"), stat(stat(stat(with(row("b", "B claude", "working", "t"), "sonnet"), "", "1:04"), "", "$0.11"), "", "3 files")];
    let mut a = app_with(Json::Null, mk());
    assert_eq!(transcript(&mut a, 50, 8)[1], flush("╰─ ▷ B claude sonnet", "1:04  $0.11  3 files", 49));
    assert_eq!(transcript(&mut a, 40, 8)[1], flush("╰─ ▷ B claude", "1:04  $0.11  3 files", 39), "the with goes first");
    assert_eq!(transcript(&mut a, 30, 8)[1], flush("╰─ ▷ B claude", "$0.11  3 files", 29), "then the first stat");
    assert_eq!(transcript(&mut a, 23, 8)[1], flush("╰─ ▷ B claude", "3 files", 22));
    assert_eq!(transcript(&mut a, 14, 8)[1], "╰─ ▷ B claude", "then all of them");
    assert_eq!(transcript(&mut a, 10, 8)[1], "╰─ ▷ B c…", "and last the end of the words");
}

#[test]
fn a_mark_alone_in_the_stats_is_followed_by_its_gap_and_not_two() {
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1"), stat(stat(row("b", "B claude", "working", "t"), "read-only", ""), "", "21s")]);
    let s = transcript(&mut a, 40, 8);
    assert_eq!(s[1], flush("╰─ ▷ B claude", "⊘  21s", 39), "{s:#?}");
}

#[test]
fn a_summary_of_a_folded_lane_gives_way_before_its_stats_do() {
    let mut rows = vec![turn("t", "go", "1"), stat(row("a", "A omp", "done", "t"), "", "21s")];
    rows.extend((0..3).map(|i| row(&format!("a{i}"), "x", "read", "a")));
    rows.push(row("b", "B omp", "working", "t"));
    let mut a = app_with(Json::Null, rows);
    assert_eq!(transcript(&mut a, 30, 8)[1], flush("├─ ✓ A omp  ≡ 3", "21s ▸", 29));
    assert_eq!(transcript(&mut a, 20, 8)[1], flush("├─ ✓ A omp", "21s ▸", 19), "the summary goes before the stats do");
}

#[test]
fn a_row_a_model_made_starts_with_a_tilde_and_one_an_agent_wrote_does_not() {
    let mut r = row("m", "two clippy errors", "error", "l");
    r.by = By::Model;
    let mut doer = row("d", "committed", "done", "l");
    doer.by = By::Doer;
    let mut p = row("p", "I think the cause is the loop.", "", "l");
    p.by = By::Model;
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1"), row("l", "A x", "working", "t"), r, doer, p]);
    let s = transcript(&mut a, 40, 10);
    assert_eq!((s[2].as_str(), s[3].as_str(), s[4].as_str()), ("   ├─ ⊗ ~two clippy errors", "   ├─ ✓ committed", "   ╰─ ~I think the cause is the loop."), "{s:#?}");
}

#[test]
fn the_tilde_is_in_the_made_role_and_the_words_after_it_are_not() {
    let mut r = row("m", "guess", "", "l");
    r.by = By::Model;
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1"), row("l", "A x", "working", "t"), r]);
    let tones = Headless::new(30, 8).tones(&mut a);
    let faint = j4n_tui::theme::palette::Role::Made;
    let _ = faint;
    let at = lines(&mut a, 30, 8).iter().position(|l| l.contains("~guess")).expect("the row");
    assert!(tones[at].chars().nth(6) != tones[at].chars().nth(7), "the tilde and the word are not the same colour: {:?}", tones[at]);
}

// ---- output ----

fn shell() -> Vec<Row> {
    let lines: Vec<String> = (1..=14).map(|i| format!("line {i}")).collect();
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    vec![turn("t", "go", "1"), row("l", "B x", "working", "t"), stat(out(row("s", "cargo test", "run", "l"), &refs), "done", "3.1s")]
}

#[test]
fn a_step_with_output_shows_the_last_three_lines_and_how_many_are_not_shown() {
    let mut a = app_with(Json::Null, shell());
    let s = transcript(&mut a, 40, 12);
    assert_eq!(s[2], flush("   ╰─ ❱ cargo test", "✓ 3.1s", 39), "{s:#?}");
    // the box goes to the right edge, with a blank inside it after the last word
    assert_eq!((s[3].as_str(), s[4].as_str(), s[5].as_str()), ("        line 12", "        line 13", flush("        line 14", "+11 ▸ ", 39).trim_end()), "{s:#?}");
}

#[test]
fn the_last_line_of_the_output_is_the_one_that_carries_the_rest_and_enter_opens_all_of_it() {
    let mut a = app_with(Json::Null, shell());
    hit_chord(&mut a, &["ctrl+up", "down", "down", "enter"]);
    let s = transcript(&mut a, 40, 20);
    assert!(s[2].ends_with("✓ 3.1s ▾"), "{s:#?}");
    assert_eq!(s.iter().filter(|l| l.trim_start().starts_with("line ")).count(), 14, "{s:#?}");
    assert!(!s.iter().any(|l| l.contains("+11")));
}

#[test]
fn output_that_fits_in_the_tail_is_all_shown_and_does_not_fold() {
    let mut rows = shell();
    rows[2].detail.truncate(3);
    let mut a = app_with(Json::Null, rows);
    let s = transcript(&mut a, 40, 12);
    assert_eq!(s.iter().filter(|l| l.trim_start().starts_with("line ")).count(), 3, "{s:#?}");
    assert!(!s[2].contains('▸') && !s.iter().any(|l| l.contains('+')), "{s:#?}");
    hit_chord(&mut a, &["ctrl+up", "down", "down", "enter"]);
    assert_eq!(transcript(&mut a, 40, 12), s, "there is nothing to open");
}

#[test]
fn the_tail_is_an_option() {
    let mut a = app_with(json!({"tail": 1}), shell());
    let s = transcript(&mut a, 40, 12);
    assert_eq!(s.iter().filter(|l| l.trim_start().starts_with("line ")).count(), 1, "{s:#?}");
    assert!(s[3].ends_with("+13 ▸"), "{s:#?}");
}

#[test]
fn the_output_is_on_a_raised_box_that_goes_to_the_right_edge() {
    let mut a = app_with(Json::Null, shell());
    let raise = a.theme.roles.get(j4n_tui::theme::palette::Role::Raise);
    let ansi = Headless::new(40, 12).snap(&mut a).ansi;
    let line = ansi.lines().find(|l| l.contains("line 12")).expect("the line");
    assert!(line.contains(&format!("48;2;{};{};{}", raise.0, raise.1, raise.2)), "{line:?}");
    assert!(line.trim_end().ends_with("m") || line.ends_with(' '), "{line:?}");
}

#[test]
fn an_edit_shows_its_diff_only_when_it_is_opened_and_the_plus_and_the_minus_have_their_colours() {
    let mut edit = stat(out(row("e", "parser.rs", "edit", "l"), &["229 − old", "+ added", "- removed", "ctx"]), "", "+3 −7");
    edit.detail[0].text = "- gone".into();
    edit.detail[1].text = "+ new".into();
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1"), row("l", "B x", "working", "t"), edit]);
    let s = transcript(&mut a, 40, 12);
    assert_eq!(s[2], flush("   ╰─ ± parser.rs", "+3 −7 ▸", 39), "{s:#?}");
    assert_eq!(s.len(), 3);
    hit_chord(&mut a, &["ctrl+up", "down", "down", "enter"]);
    let s = transcript(&mut a, 40, 12);
    assert!(s[2].ends_with("▾") && s[3].trim_start().starts_with("- gone") && s[4].trim_start().starts_with("+ new"), "{s:#?}");
    let tones = Headless::new(40, 12).tones(&mut a);
    let shown = lines(&mut a, 40, 12);
    let at = |t: &str| shown.iter().position(|l| l.contains(t)).expect("the line");
    let (minus, plus) = (at("- gone"), at("+ new"));
    assert_eq!(tones[minus].chars().nth(8), Some('r'), "{tones:#?}");
    assert_eq!(tones[plus].chars().nth(8), Some('g'), "{tones:#?}");
}

// ---- words ----

#[test]
fn prose_wraps_at_the_width_of_the_option_and_under_its_pipe() {
    let words = "alpha beta gamma delta epsilon zeta eta theta";
    let mut a = app_with(json!({"width": 20}), vec![turn("t", "go", "1"), row("l", "A x", "working", "t"), row("p", words, "", "l")]);
    let s = transcript(&mut a, 60, 10);
    assert_eq!((s[2].as_str(), s[3].as_str(), s[4].as_str()), ("   ╰─ alpha beta gamma", "      delta epsilon zeta", "      eta theta"), "{s:#?}");
}

#[test]
fn prose_stays_inside_a_narrow_screen_too() {
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1"), row("l", "A x", "working", "t"), row("p", "word ".repeat(30).trim(), "", "l")]);
    for l in transcript(&mut a, 30, 20) {
        assert!(l.chars().count() <= 30, "{l:?}");
    }
}

#[test]
fn a_bullet_a_quote_and_a_fenced_block_are_drawn_as_themselves() {
    let text = "intro\n- one thing that is long enough to wrap onto a second line\n> an aside\n```rs\nlet a = 1;\n```\nlast";
    let mut a = app_with(json!({"width": 30}), vec![turn("t", "go", "1"), row("l", "A x", "working", "t"), row("p", text, "", "l")]);
    let s = transcript(&mut a, 60, 20);
    let body: Vec<&str> = s[2..].iter().map(String::as_str).collect();
    // the reader: a blank row between the blocks, the quote bar dim and thin, the language on a row of its own at the right
    assert_eq!(body, ["   ╰─ intro", "", "      • one thing that is long", "        enough to wrap onto a second", "        line", "", "      ▏ an aside", "", "                                 rs", "       let a = 1;", "", "      last"], "{s:#?}");
}

// ---- the keys on the transcript ----

fn long() -> Vec<Row> {
    let mut rows = Vec::new();
    for t in 0..6 {
        rows.push(turn(&format!("t{t}"), &format!("turn {t}"), "1"));
        rows.push(row(&format!("l{t}"), &format!("lane {t}"), "working", &format!("t{t}")));
        rows.push(row(&format!("s{t}"), &format!("step {t}"), "read", &format!("l{t}")));
    }
    rows
}

#[test]
fn the_transcript_follows_its_end_and_page_up_goes_back_and_page_down_returns() {
    let mut a = app_with(Json::Null, long());
    let end = transcript(&mut a, 40, 10);
    assert!(end.last().is_some_and(|l| l.contains("step")) && end.iter().any(|l| l.contains("turn 5")), "{end:#?}");
    hit(&mut a, KeyCode::PageUp);
    let back = transcript(&mut a, 40, 10);
    assert!(!back.iter().any(|l| l.contains("turn 5")) || back != end, "{back:#?}");
    for _ in 0..10 {
        hit(&mut a, KeyCode::PageUp);
    }
    let top = transcript(&mut a, 40, 10);
    assert!(top.iter().any(|l| l.contains("turn 0")), "{top:#?}");
    for _ in 0..12 {
        hit(&mut a, KeyCode::PageDown);
    }
    assert_eq!(transcript(&mut a, 40, 10), end);
}

#[test]
fn a_new_row_when_the_operator_looks_at_the_end_is_shown_at_once() {
    let mut a = app_with(Json::Null, long());
    let mut rows = long();
    rows.push(turn("t9", "the newest", "2"));
    send(&mut a, rows);
    assert!(transcript(&mut a, 40, 10).last().is_some_and(|l| l.starts_with("the newest")));
}

#[test]
fn control_up_puts_the_keys_on_the_newest_turn_and_up_and_down_step_over_the_lines_that_head_a_row() {
    let mut a = app_with(Json::Null, long());
    assert!(hit_chord(&mut a, &["ctrl+up"]).is_empty());
    assert_eq!(a.wins.get("chat:main").and_then(|w| w.at.clone()).as_deref(), Some("t5"));
    hit(&mut a, KeyCode::Down);
    assert_eq!(a.wins["chat:main"].at.as_deref(), Some("l5"));
    hit(&mut a, KeyCode::Down);
    assert_eq!(a.wins["chat:main"].at.as_deref(), Some("s5"));
    hit(&mut a, KeyCode::Up);
    hit(&mut a, KeyCode::Up);
    hit(&mut a, KeyCode::Up);
    assert_eq!(a.wins["chat:main"].at.as_deref(), Some("s4"), "back over the turn before");
    hit_chord(&mut a, &["ctrl+up"]);
    assert_eq!(a.wins["chat:main"].at.as_deref(), Some("t4"));
    hit_chord(&mut a, &["ctrl+down", "ctrl+down"]);
    assert_eq!(a.wins["chat:main"].at.as_deref(), None, "past the newest turn the keys are the composer's again");
}

#[test]
fn down_from_the_last_line_gives_the_keys_back_to_the_composer_and_so_does_esc() {
    let mut a = app_with(Json::Null, long());
    hit_chord(&mut a, &["ctrl+up", "down", "down", "down"]);
    assert_eq!(a.wins["chat:main"].at, None);
    hit_chord(&mut a, &["ctrl+up", "esc"]);
    assert_eq!(a.wins["chat:main"].at, None);
    assert!(transcript(&mut a, 40, 10).last().is_some_and(|l| l.contains("step")), "and the transcript is at its end");
}

#[test]
fn typing_with_the_keys_on_the_transcript_goes_to_the_composer() {
    let mut a = app_with(Json::Null, long());
    hit_chord(&mut a, &["ctrl+up", "down"]);
    typing(&mut a, "hi");
    assert_eq!((a.wins["chat:main"].at.clone(), a.line.buf.as_str()), (None, "hi"));
    hit_chord(&mut a, &["ctrl+up", "backspace"]);
    assert_eq!((a.wins["chat:main"].at.clone(), a.line.buf.as_str()), (None, "h"));
}

#[test]
fn enter_on_the_transcript_folds_or_opens_and_never_sends() {
    let mut a = app_with(Json::Null, one_lane());
    typing(&mut a, "draft");
    let sent = hit_chord(&mut a, &["ctrl+up", "down", "enter"]);
    assert!(sent.is_empty() && a.line.buf == "draft", "{sent:?}");
    assert!(transcript(&mut a, 60, 20).iter().any(|l| l.starts_with("├─ ✓ A omp sonnet") && l.ends_with('▾')));
    hit_chord(&mut a, &["enter"]);
    assert!(transcript(&mut a, 60, 20).iter().any(|l| l.starts_with("├─ ✓ A omp") && l.ends_with('▸')), "shut again");
    // a row with nothing to fold does nothing
    hit_chord(&mut a, &["enter"]);
    hit_chord(&mut a, &["ctrl+up"]);
    let before = a.wins["chat:main"].flip.clone();
    hit_chord(&mut a, &["enter"]);
    assert_eq!(a.wins["chat:main"].flip, before, "a turn does not fold");
}

#[test]
fn z_opens_everything_that_folds_and_shuts_it_when_it_all_is_open() {
    let mut a = app_with(Json::Null, one_lane());
    hit_chord(&mut a, &["ctrl+up", "z"]);
    let s = transcript(&mut a, 60, 20);
    assert!(s.iter().any(|l| l.contains("A omp") && l.ends_with('▾')) && s.iter().any(|l| l.contains("≡ ")), "{s:#?}");
    hit_chord(&mut a, &["z"]);
    let s = transcript(&mut a, 60, 20);
    assert!(s.iter().any(|l| l.contains("A omp") && l.ends_with('▸')) && s.iter().any(|l| l.contains("B claude") && l.ends_with('▸')), "{s:#?}");
}

#[test]
fn the_line_the_keys_are_on_is_in_sight_whatever_is_above_it() {
    let mut a = app_with(Json::Null, long());
    for _ in 0..12 {
        hit(&mut a, KeyCode::PageUp);
    }
    hit_chord(&mut a, &["ctrl+up"]);
    assert!(transcript(&mut a, 40, 10).iter().any(|l| l.starts_with("turn 5")));
    hit_chord(&mut a, &["ctrl+up", "ctrl+up", "ctrl+up", "ctrl+up", "ctrl+up"]);
    assert!(transcript(&mut a, 40, 10).iter().any(|l| l.starts_with("turn 0")));
}

#[test]
fn the_head_the_keys_are_on_has_the_selection_behind_it_and_its_pipe_in_the_focus_colour() {
    let mut a = app_with(Json::Null, one_lane());
    hit_chord(&mut a, &["ctrl+up", "down"]);
    let sel = a.theme.roles.get(j4n_tui::theme::palette::Role::Sel);
    let focus = a.theme.roles.get(j4n_tui::theme::palette::Role::Focus);
    let ansi = Headless::new(60, 20).snap(&mut a).ansi;
    let line = ansi.lines().find(|l| l.contains("A omp")).expect("the lane");
    assert!(line.contains(&format!("48;2;{};{};{}", sel.0, sel.1, sel.2)) && line.contains(&format!("38;2;{};{};{}", focus.0, focus.1, focus.2)), "{line:?}");
    let other = ansi.lines().find(|l| l.contains("B claude")).expect("the other lane");
    assert!(!other.contains(&format!("48;2;{};{};{}", sel.0, sel.1, sel.2)), "{other:?}");
}

// ---- breath ----

fn breathing() -> App {
    app_with(Json::Null, vec![turn("t", "go", "1"), row("l", "B claude", "working", "t")])
}

fn name_colour(a: &mut App) -> String {
    let ansi = Headless::new(40, 8).snap(a).ansi;
    let at = ansi.find("B claude").expect("the name");
    let sgr = ansi[..at].rfind("\u{1b}[").expect("a colour");
    let end = ansi[sgr..].find('m').expect("the end of it");
    ansi[sgr..sgr + end + 1].to_string()
}

#[test]
fn the_name_of_the_doer_that_works_goes_from_the_accent_to_its_soft_twin_and_back_over_eight_ticks_of_a_quarter_second() {
    let mut a = breathing();
    let t0 = Instant::now();
    let mut seen = Vec::new();
    for tick in 0..9u64 {
        a.tick(t0 + Duration::from_millis(250 * tick));
        seen.push(name_colour(&mut a));
    }
    assert_eq!(seen[0], seen[8], "a breath is eight ticks");
    assert_eq!(seen[1], seen[7]);
    assert_eq!(seen[2], seen[6]);
    assert_eq!(seen[3], seen[5]);
    let mut distinct = seen[..5].to_vec();
    distinct.dedup();
    assert_eq!(distinct.len(), 5, "each tick on the way down is another colour: {seen:?}");
    let accent = a.theme.roles.get(j4n_tui::theme::palette::Role::Accent);
    assert!(seen[0].contains(&format!("38;2;{};{};{}", accent.0, accent.1, accent.2)), "the breath starts at the full accent: {:?}", seen[0]);
}

#[test]
fn only_the_lane_that_works_breathes() {
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1"), row("l", "B claude", "done", "t"), row("m", "C pi", "queued", "t")]);
    a.tick(Instant::now());
    let before = Headless::new(40, 8).snap(&mut a).ansi;
    a.tick(Instant::now() + Duration::from_millis(1000));
    assert_eq!(Headless::new(40, 8).snap(&mut a).ansi, before);
    assert!(!a.breathes);
}

#[test]
fn the_screen_is_drawn_again_for_the_clock_only_when_a_doer_works_in_sight_and_the_tick_changed() {
    let mut a = breathing();
    let t0 = Instant::now();
    a.tick(t0);
    Headless::new(40, 8).show(&mut a);
    assert!(a.breathes && !a.take_redraw(), "the first tick is the one drawn");
    a.tick(t0 + Duration::from_millis(100));
    assert!(!a.take_redraw(), "the tick has not changed");
    a.tick(t0 + Duration::from_millis(250));
    assert!(a.take_redraw());
    assert!(!a.take_redraw(), "and reading it clears it");
    let mut idle = app_with(Json::Null, vec![turn("t", "go", "1"), row("l", "B", "done", "t")]);
    idle.tick(t0);
    Headless::new(40, 8).show(&mut idle);
    idle.tick(t0 + Duration::from_millis(250));
    assert!(!idle.take_redraw(), "nothing breathes, so nothing is drawn for the clock");
}

#[test]
fn without_motion_the_name_is_bold_and_still_and_without_colour_it_is_bold_too() {
    let mut a = breathing();
    look(&mut a, Settings { marks: "unicode".into(), you: "mark".into(), motion: false, ..Settings::default() });
    let t0 = Instant::now();
    a.tick(t0);
    let first = name_colour(&mut a);
    a.tick(t0 + Duration::from_millis(1000));
    assert_eq!(name_colour(&mut a), first);
    assert!(first.contains(";1;") || first.contains("[1;"), "{first:?}");
    a.tick(t0 + Duration::from_millis(1250));
    assert!(!a.take_redraw(), "no motion, no redraws for it");
    look(&mut a, Settings { marks: "unicode".into(), you: "mark".into(), no_color: true, ..Settings::default() });
    assert_eq!(name_colour(&mut a), "\u{1b}[0;1m");
}

// ---- the composer ----

fn strip() -> Json {
    json!({"tags": [{"text": "parser.rs", "icon": "tag", "pct": 88}, {"text": "plan", "icon": "route", "pct": 72}], "gauge": 0.41, "route": "claude"})
}

#[test]
fn an_idle_composer_is_a_rounded_frame_round_one_row_with_the_example_in_faint() {
    let mut a = app_with(json!({"placeholder": "fix the failing test"}), vec![turn("t", "go", "1")]);
    let s = lines(&mut a, 40, 8);
    assert_eq!(&s[5..], ["╭──────────────────────────────────────╮", "│ fix the failing test                 │", "╰──────────────────────────────────────╯"].map(String::from).to_vec(), "{s:#?}");
    let faint = a.theme.roles.get(j4n_tui::theme::palette::Role::Faint);
    let ansi = Headless::new(40, 8).snap(&mut a).ansi;
    assert!(ansi.lines().find(|l| l.contains("fix the failing")).is_some_and(|l| l.contains(&format!("38;2;{};{};{}", faint.0, faint.1, faint.2))));
    typing(&mut a, "x");
    assert_eq!(lines(&mut a, 40, 8)[6], "│ x                                    │", "the example gives way to what is typed");
}

#[test]
fn the_strip_is_the_tags_with_how_sure_the_core_is_then_the_gauge_and_the_guess_at_the_right_over_a_dashed_rule() {
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1")]);
    a.down(Down::Value { bind: "composer:strip".into(), value: strip(), rev: 1 });
    typing(&mut a, "hello");
    let s = lines(&mut a, 70, 8);
    assert_eq!(s[3], flush("╭", "╮", 70).replace(' ', "─"));
    assert_eq!(s[4], flush("│ # parser.rs ~88%  ➜ plan ~72%", "~➜ claude ━━──── 41% │", 70));
    assert_eq!(s[5], format!("│{}│", "╌".repeat(68)));
    assert_eq!(s[6], "│ hello                                                              │");
}

#[test]
fn while_a_step_of_j4n_runs_its_words_stand_dim_in_the_empty_line_then_as_the_word_typed_and_go_with_any_other_line() {
    // the composer says what ends the check that runs, quietly and once; the words are the core's, typed in full to act
    let mut a = app_with(json!({"placeholder": "fix the failing test"}), vec![turn("t", "go", "1")]);
    a.down(Down::Value { bind: "composer:strip".into(), value: json!({"route": "claude", "work": [{"label": "working"}], "skip": {"words": ["skip", "stop"], "what": "check t50"}}), rev: 1 });
    let s = lines(&mut a, 50, 8);
    let line = s.iter().find(|l| l.contains("skip")).expect("the hint is drawn");
    assert!(line.starts_with("│ skip · stop  check t50") && !s.iter().any(|l| l.contains("fix the failing")), "in place of the example: {s:#?}");
    assert_eq!(s.iter().filter(|l| l.contains("skip")).count(), 1, "one hint: {s:#?}");
    typing(&mut a, "sk");
    let s = lines(&mut a, 50, 8);
    assert!(s.iter().any(|l| l.starts_with("│ sk ") && l.trim_end_matches('│').trim_end().ends_with("skip ⏎")), "the word the line starts, with its key: {s:#?}");
    typing(&mut a, "x");
    let s = lines(&mut a, 50, 8);
    assert!(!s.iter().any(|l| l.contains("skip")), "any other line hides it: {s:#?}");
    // no step, no hint
    let mut a = app_with(json!({"placeholder": "fix the failing test"}), vec![turn("t", "go", "1")]);
    a.down(Down::Value { bind: "composer:strip".into(), value: json!({"route": "claude", "work": []}), rev: 1 });
    assert!(!lines(&mut a, 50, 8).iter().any(|l| l.contains("skip")));
}

#[test]
fn a_route_is_marked_as_a_guess_unless_the_core_says_it_is_its_own_word() {
    let route = |extra: Json| {
        let mut a = app_with(Json::Null, vec![turn("t", "go", "1")]);
        let mut v = json!({"gauge": 0.41, "route": "claude"});
        v.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        a.down(Down::Value { bind: "composer:strip".into(), value: v, rev: 1 });
        lines(&mut a, 70, 8)[4].clone()
    };
    assert!(route(json!({})).contains("~➜ claude"), "a route nobody vouches for is a guess");
    assert!(route(json!({"route_by": "model"})).contains("~➜ claude"));
    for by in ["core", "rule", "operator"] {
        let l = route(json!({"route_by": by}));
        assert!(l.contains("➜ claude") && !l.contains('~'), "{by}: {l}");
    }
}

#[test]
fn the_gauge_is_a_thin_line_in_halves_and_goes_warm_from_the_line_and_red_over_all_of_it() {
    let at = |v: f64| {
        let mut a = app_with(Json::Null, vec![turn("t", "go", "1")]);
        a.down(Down::Value { bind: "composer:strip".into(), value: json!({"gauge": v}), rev: 1 });
        let line = lines(&mut a, 40, 8)[4].clone();
        let tones = Headless::new(40, 8).tones(&mut a)[4].clone();
        (line, tones)
    };
    let (l, _) = at(0.0);
    assert!(l.contains("────── 0%"), "{l}");
    let (l, _) = at(0.15);
    assert!(l.contains("╸───── 15%"), "{l}");
    let (l, _) = at(0.41);
    assert!(l.contains("━━──── 41%"), "{l}");
    let (l, _) = at(1.0);
    assert!(l.contains("━━━━━━ 100%"), "{l}");
    let col = |t: &str, l: &str| l.find('━').or_else(|| l.find('╸')).map(|byte| t.chars().nth(l[..byte].chars().count()).unwrap_or('?'));
    let (l, t) = at(0.5);
    assert_eq!(col(&t, &l), Some('c'), "accent: {t}");
    let (l, t) = at(0.8);
    assert_eq!(col(&t, &l), Some('y'), "warm: {t}");
    let (l, t) = at(1.2);
    assert_eq!(col(&t, &l), Some('r'), "red: {t}");
}

#[test]
fn the_gauge_and_the_guess_go_under_the_tags_when_they_do_not_fit_beside_them_and_the_last_tags_go_when_they_fill_a_row() {
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1")]);
    a.down(Down::Value { bind: "composer:strip".into(), value: json!({"tags": [{"text": "parser.rs", "icon": "tag", "pct": 88}, {"text": "args.rs", "icon": "tag", "pct": 64}, {"text": "plan-work-check", "icon": "route", "pct": 72}], "gauge": 0.15, "route": "sonnet"}), rev: 1 });
    let s = lines(&mut a, 60, 10);
    let at = s.iter().position(|l| l.starts_with('╭')).expect("the frame");
    assert!(s[at + 1].contains("plan-work-check") && !s[at + 1].contains('╸'), "{s:#?}");
    assert!(s[at + 2].contains("╸───── 15%") && s[at + 2].contains("~➜ sonnet"), "{s:#?}");
    assert!(s[at + 3].starts_with("│╌"), "{s:#?}");
    let mut b = app_with(Json::Null, vec![turn("t", "go", "1")]);
    b.down(Down::Value { bind: "composer:strip".into(), value: json!({"tags": [{"text": "a-very-long-tag-name.rs", "pct": 88}, {"text": "another-long-tag-name.rs", "pct": 64}]}), rev: 1 });
    let s = lines(&mut b, 40, 10);
    let at = s.iter().position(|l| l.starts_with('╭')).expect("the frame");
    assert!(s[at + 1].contains("a-very-long") && !s[at + 1].contains("another"), "the tags that do not fit are left out: {s:#?}");
}

#[test]
fn a_composer_with_nothing_in_the_strip_has_no_rule() {
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1")]);
    a.down(Down::Value { bind: "composer:strip".into(), value: json!({}), rev: 1 });
    assert!(!lines(&mut a, 40, 8).iter().any(|l| l.starts_with("│╌")));
}

#[test]
fn the_line_grows_upward_a_row_at_a_time_up_to_eight_and_then_scrolls_with_the_cursor() {
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1")]);
    for i in 1..=3 {
        typing(&mut a, &format!("row {i}"));
        hit_chord(&mut a, &["ctrl+j"]);
    }
    typing(&mut a, "row 4");
    let s = lines(&mut a, 40, 14);
    let at = s.iter().position(|l| l.starts_with('╭')).expect("the frame");
    assert_eq!(s[at + 1..at + 4].iter().map(|l| l.trim_matches(|c| c == '│' || c == ' ').to_string()).collect::<Vec<_>>(), ["row 1", "row 2", "row 3"], "{s:#?}");
    assert!(s[at + 4].starts_with("│ row 4"), "{s:#?}");
    assert!(s[at + 4].contains("⇧⏎ line · ⏎ send"), "the keys of a line of many rows are on its last row: {s:#?}");
    assert!(s[at + 5].starts_with('╰'));
    for i in 5..=12 {
        hit_chord(&mut a, &["ctrl+j"]);
        typing(&mut a, &format!("row {i}"));
    }
    let s = lines(&mut a, 40, 14);
    let at = s.iter().position(|l| l.starts_with('╭')).expect("the frame");
    let body: Vec<String> = s[at + 1..].iter().take_while(|l| !l.starts_with('╰')).map(|l| l.trim_matches(|c| c == '│' || c == ' ').trim_end_matches("⇧⏎ line · ⏎ send").trim().to_string()).collect();
    assert_eq!(body.len(), 8, "{s:#?}");
    assert_eq!((body[0].as_str(), body[7].as_str()), ("row 5", "row 12"), "the last eight rows, the cursor in sight: {body:?}");
}

#[test]
fn a_long_line_wraps_inside_the_frame_and_the_cursor_follows_it() {
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1")]);
    typing(&mut a, "word ".repeat(12).trim_end());
    let mut screen = Headless::new(30, 12);
    let snap = screen.snap(&mut a);
    let rows: Vec<&str> = snap.text.lines().collect();
    let at = rows.iter().position(|l| l.starts_with('╭')).expect("the frame");
    assert_eq!((rows[at + 1], rows[at + 2], rows[at + 3]), ("│ word word word word word   │", "│ word word word word word   │", "│ word word                  │"), "{rows:#?}");
    assert!(rows.iter().all(|l| l.chars().count() <= 30));
    assert_eq!(snap.cursor, (2 + 9, (at + 3) as u16), "after the last word, on the last row");
}

#[test]
fn a_long_line_gives_the_target_the_room_it_needs_at_the_end_of_the_last_row() {
    let mut a = app_with(Json::Null, lanes());
    hit_chord(&mut a, &["ctrl+t"]);
    typing(&mut a, "word ".repeat(8).trim_end());
    let s = lines(&mut a, 30, 14);
    let at = s.iter().position(|l| l.starts_with('╭')).expect("the frame");
    assert_eq!(&s[at + 1..at + 3], ["│ word word word word word   │", "│ word word word  → B claude │"], "{s:#?}");
}

#[test]
fn the_cursor_is_in_the_frame_where_the_text_ends_and_moves_with_the_arrows() {
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1")]);
    typing(&mut a, "abc");
    let mut screen = Headless::new(30, 8);
    let at = screen.snap(&mut a).cursor;
    assert_eq!(at, (5, 6), "after `abc`, the frame's edge and a blank before them");
    hit(&mut a, KeyCode::Left);
    assert_eq!(screen.snap(&mut a).cursor, (4, 6));
    typing(&mut a, "é世");
    hit(&mut a, KeyCode::End);
    assert_eq!(screen.snap(&mut a).cursor.0, 2 + 1 + 1 + 1 + 2 + 1, "a wide letter is two columns");
}

#[test]
fn enter_with_shift_and_control_j_add_a_line_and_enter_sends_it_whole() {
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1")]);
    typing(&mut a, "one");
    a.key(crossterm::event::KeyEvent::new(KeyCode::Enter, crossterm::event::KeyModifiers::SHIFT));
    typing(&mut a, "two");
    hit_chord(&mut a, &["ctrl+j"]);
    typing(&mut a, "three");
    assert_eq!(a.line.buf, "one\ntwo\nthree");
    let sent = hit(&mut a, KeyCode::Enter);
    assert!(matches!(&sent[..], [Up::Intent { intent: Intent::Say { text, .. } }] if text == "one\ntwo\nthree"), "{sent:?}");
}

#[test]
fn the_frame_is_accent_and_bold_while_typing_warm_while_a_question_is_open_and_quiet_when_the_keys_are_on_the_transcript() {
    let frame_colour = |a: &mut App| {
        let ansi = Headless::new(40, 10).snap(a).ansi;
        // the composer's frame is the last on the screen: a question's popup is a round box too
        ansi.lines().rfind(|l| l.contains('╭')).expect("the top of the frame").to_string()
    };
    let mut a = app_with(Json::Null, one_lane());
    let c = |r: j4n_tui::theme::palette::Role, a: &App| {
        let v = a.theme.roles.get(r);
        format!("38;2;{};{};{}", v.0, v.1, v.2)
    };
    let typing_style = frame_colour(&mut a);
    assert!(typing_style.contains(&c(j4n_tui::theme::palette::Role::Accent, &a)) && typing_style.contains(";1;"), "{typing_style:?}");
    hit_chord(&mut a, &["ctrl+up"]);
    let quiet = frame_colour(&mut a);
    assert!(quiet.contains(&c(j4n_tui::theme::palette::Role::Edge, &a)) && !quiet.contains(";1;"), "{quiet:?}");
    hit_chord(&mut a, &["esc"]);
    a.down(Down::Asks { open: vec![AskView { ask: "q".into(), asker: "b".into(), question: "go?".into(), shape: Shape::YesNo, options: Vec::new(), effect: String::new(), risk: 0, grants: Vec::new(), ..AskView::default() }] });
    let asked = frame_colour(&mut a);
    assert!(asked.contains(&c(j4n_tui::theme::palette::Role::Warn, &a)), "{asked:?}");
}

// ---- the target ----

fn lanes() -> Vec<Row> {
    vec![turn("t", "go", "1"), row("a", "A omp", "done", "t"), row("b", "B claude", "working", "t"), row("c", "C pi", "ask", "t"), row("d", "D x", "queued", "t")]
}

#[test]
fn control_t_picks_the_next_lane_that_works_and_after_the_last_none() {
    let mut a = app_with(Json::Null, lanes());
    let pick = |a: &mut App| a.aim.clone();
    assert_eq!(pick(&mut a), None);
    hit_chord(&mut a, &["ctrl+t"]);
    assert_eq!(pick(&mut a).as_deref(), Some("b"), "A is done, B works");
    hit_chord(&mut a, &["ctrl+t"]);
    assert_eq!(pick(&mut a).as_deref(), Some("c"), "C waits for an answer");
    hit_chord(&mut a, &["ctrl+t"]);
    assert_eq!(pick(&mut a), None, "D is queued; after the last lane the thread's own doer");
}

#[test]
fn the_target_is_shown_at_the_right_of_the_last_row_of_the_line() {
    let mut a = app_with(Json::Null, lanes());
    typing(&mut a, "also run clippy");
    let find = |a: &mut App| lines(a, 40, 12).into_iter().find(|l| l.contains("also run clippy")).expect("the line");
    assert_eq!(find(&mut a), "│ also run clippy                      │");
    hit_chord(&mut a, &["ctrl+t"]);
    assert_eq!(find(&mut a), "│ also run clippy           → B claude │");
}

/// The shaper's pass: a faint mark at the edge of the line, after the target when there is one, and not on a line of several rows (its keys are there).
#[test]
fn a_draft_the_shaper_passed_has_a_mark_after_the_target_and_none_on_a_line_of_several_rows() {
    let pass = |a: &mut App, text: &str| {
        a.down(Down::Value { bind: "shape:on".into(), value: json!(true), rev: 1 });
        let head = Row { key: "~draft".into(), cells: vec![Cell::new(text, Tone::Plain), Cell::new("pass", Tone::Plain), Cell::new("", Tone::Plain)], ..Row::default() };
        a.down(Down::Rows { bind: "shape:card".into(), rows: vec![head], total: 1, from: 0, rev: 1 });
    };
    let mark = "✓";
    let mut a = app_with(Json::Null, lanes());
    typing(&mut a, "also run clippy");
    pass(&mut a, "also run clippy");
    hit_chord(&mut a, &["ctrl+t"]);
    let l = lines(&mut a, 44, 12).into_iter().find(|l| l.contains("also run clippy")).expect("the line");
    assert!(l.contains(&format!("→ B claude {mark} │")), "a blank between the target and the mark: {l:?}");
    let shown = lines(&mut a, 44, 12);
    let row = shown.iter().position(|l| l.contains("also run clippy")).expect("the row");
    let col = shown[row].chars().position(|c| c == '✓').expect("the mark");
    assert_eq!(Headless::new(44, 12).tones(&mut a)[row].chars().nth(col), Some('d'), "the mark is faint, not a warning");
    let mut a = app_with(Json::Null, lanes());
    a.paste("one\ntwo");
    pass(&mut a, "one\ntwo");
    let s = lines(&mut a, 44, 12).into_iter().filter(|l| l.starts_with('│')).collect::<Vec<_>>().join("\n");
    assert!(!s.contains(mark) && s.contains("send"), "{s}");
}

#[test]
fn a_line_with_a_lane_chosen_steers_that_lane_and_a_command_does_not() {
    let mut a = app_with(Json::Null, lanes());
    hit_chord(&mut a, &["ctrl+t"]);
    typing(&mut a, "use the lexer tests");
    let sent = hit(&mut a, KeyCode::Enter);
    assert!(matches!(&sent[..], [Up::Intent { intent: Intent::Control { task, control: Control::Steer { text } } }] if task == "b" && text == "use the lexer tests"), "{sent:?}");
    typing(&mut a, ":fresh");
    let sent = hit(&mut a, KeyCode::Enter);
    assert!(matches!(&sent[..], [Up::Intent { intent: Intent::Say { text, .. } }] if text == ":fresh"), "{sent:?}");
}

#[test]
fn a_lane_that_stopped_working_is_no_longer_the_target() {
    let mut a = app_with(Json::Null, lanes());
    hit_chord(&mut a, &["ctrl+t"]);
    let mut rows = lanes();
    rows[2] = row("b", "B claude", "done", "t");
    send(&mut a, rows);
    typing(&mut a, "go on");
    let sent = hit(&mut a, KeyCode::Enter);
    assert!(matches!(&sent[..], [Up::Intent { intent: Intent::Say { .. } }]), "{sent:?}");
    assert!(!lines(&mut a, 40, 12).iter().any(|l| l.contains('→')));
}

#[test]
fn an_open_question_is_the_target_and_it_is_named_by_its_lane() {
    let mut a = app_with(Json::Null, lanes());
    a.down(Down::Asks { open: vec![AskView { ask: "q".into(), asker: "c".into(), question: "go?".into(), shape: Shape::Choice, options: vec![Choice { id: "1".into(), label: "yes".into(), recommended: true, ..Choice::default() }], effect: String::new(), risk: 0, grants: Vec::new(), ..AskView::default() }] });
    typing(&mut a, "ok");
    let s = lines(&mut a, 40, 12);
    assert!(s.iter().any(|l| l.starts_with("│ ok") && l.contains("→ ◇ C")), "{s:#?}");
}

// ---- a chat without rows ----

#[test]
fn a_chat_the_core_has_sent_no_rows_for_is_the_composer_alone() {
    let mut a = started(Json::Null);
    typing(&mut a, "hello");
    let s = lines(&mut a, 30, 6);
    assert!(s.iter().any(|l| l.starts_with('╭')) && s.iter().any(|l| l.starts_with("│ hello")), "{s:#?}");
    assert_eq!(a.rows.get("chat:main"), None);
}

// ---- the keys that were not the chat's ----

#[test]
fn the_keys_of_the_tabs_still_work_with_the_keys_on_the_transcript() {
    let mut a = app_with(Json::Null, long());
    hit_chord(&mut a, &["ctrl+up"]);
    hit(&mut a, KeyCode::PageUp);
    assert!(a.wins["chat:main"].at.is_some(), "paging does not give the keys back");
    a.key(alt_key(KeyCode::Right));
    a.key(ctrl('k'));
    assert!(a.jump.is_some());
}

// ---- a transcript that is long ----

#[test]
fn five_thousand_rows_are_drawn_and_walked_without_a_wait() {
    let mut rows = Vec::new();
    for t in 0..500 {
        rows.push(turn(&format!("t{t}"), &format!("turn {t}"), "1"));
        rows.push(row(&format!("l{t}"), "A omp", "done", &format!("t{t}")));
        for s in 0..8 {
            rows.push(row(&format!("s{t}.{s}"), "step", "read", &format!("l{t}")));
        }
    }
    let mut a = app_with(Json::Null, rows);
    let mut screen = Headless::new(120, 40);
    // the CPU the thread spent, not the time that passed: a gate that shares the machine makes the second longer, never the work
    let ((), cpu) = crate::cpu::of(|| {
        screen.show(&mut a);
        for _ in 0..20 {
            hit(&mut a, KeyCode::PageUp);
            screen.show(&mut a);
        }
    });
    let per = cpu / 21;
    eprintln!("5,000 rows (500 turns): a frame {per:?} of CPU while paging");
    crate::cpu::assert_within(Duration::from_millis(if cfg!(debug_assertions) { 250 } else { 16 }), "a frame while paging", || per);
    hit_chord(&mut a, &["ctrl+up"]);
    let ((), cpu) = crate::cpu::of(|| {
        for _ in 0..5 {
            hit(&mut a, KeyCode::Up);
            screen.show(&mut a);
        }
    });
    let per = cpu / 5;
    eprintln!("5,000 rows (500 turns): a key with the keys on the transcript and its frame {per:?} of CPU");
    crate::cpu::assert_within(Duration::from_millis(if cfg!(debug_assertions) { 600 } else { 40 }), "a key and a frame", || per);
}

// ---- edge cases ----

fn marked(mut r: Row, by: By) -> Row {
    r.by = by;
    r
}

#[test]
fn a_step_that_has_steps_under_it_is_open_whatever_its_mark_and_a_control_row_is_not_a_newer_lane() {
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1"), row("l", "B x", "working", "t"), row("s", "inner step", "done", "l"), row("k", "kid", "read", "s"), row("n", "after", "read", "l")]);
    let s = transcript(&mut a, 40, 12);
    assert!(s.iter().any(|l| l.contains("≡ kid")), "a step is open: {s:#?}");
    // a lane is not folded by a row that is not a lane
    let mut b = app_with(Json::Null, vec![turn("t", "go", "1"), row("a", "A x", "done", "t"), row("a1", "x", "read", "a"), marked(row("c", "resumed", "session", "t"), By::Core)]);
    assert!(transcript(&mut b, 40, 12).iter().any(|l| l.contains("≡ x")), "control rows start nothing");
}

#[test]
fn only_steps_of_one_mark_and_one_maker_with_nothing_under_them_merge() {
    let two = |a: Row, b: Row| {
        let mut app = app_with(Json::Null, vec![turn("t", "go", "1"), row("l", "B x", "working", "t"), a, b]);
        transcript(&mut app, 50, 12).split_off(2)
    };
    let rd = |k: &str, n: &str| merged(row(k, n, "read", "l"));
    assert_eq!(two(rd("r1", "a.rs"), rd("r2", "b.rs")), [flush("   ╰─ ≡ a.rs · b.rs", "2", 49)]);
    // words with no mark are not merged, flagged or not
    assert_eq!(two(merged(row("p1", "one", "", "l")), merged(row("p2", "two", "", "l"))).len(), 2);
    // another mark
    assert_eq!(two(rd("r1", "a.rs"), merged(row("s1", "x", "search", "l"))), ["   ├─ ≡ a.rs", "   ╰─ ⊙ x"].map(String::from).to_vec());
    // another maker
    assert_eq!(two(marked(rd("r1", "a.rs"), By::Doer), marked(rd("r2", "b.rs"), By::Model)), ["   ├─ ≡ a.rs", "   ╰─ ≡ ~b.rs"].map(String::from).to_vec());
    // output under either
    let with_out = |k: &str, n: &str| out(rd(k, n), &["x"]);
    assert_eq!(two(rd("r1", "a.rs"), with_out("r2", "b.rs")).len(), 3);
    assert_eq!(two(with_out("r1", "a.rs"), rd("r2", "b.rs")).len(), 3);
}

#[test]
fn a_step_with_steps_under_it_is_not_merged_with_the_one_before_or_the_one_after() {
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1"), row("l", "B x", "working", "t"), merged(row("r1", "a.rs", "read", "l")), merged(row("r2", "b.rs", "read", "l")), row("k", "inner", "think", "r2")]);
    let mut s = transcript(&mut a, 50, 12);
    assert_eq!(s.split_off(2), ["   ├─ ≡ a.rs", "   ╰─ ≡ b.rs", "      ╰─ ∴ inner"].map(String::from).to_vec());
    let mut b = app_with(Json::Null, vec![turn("t", "go", "1"), row("l", "B x", "working", "t"), merged(row("r1", "a.rs", "read", "l")), row("k", "inner", "think", "r1"), merged(row("r2", "b.rs", "read", "l"))]);
    let mut s = transcript(&mut b, 50, 12);
    assert_eq!(s.split_off(2), ["   ├─ ≡ a.rs", "   │  ╰─ ∴ inner", "   ╰─ ≡ b.rs"].map(String::from).to_vec());
}

#[test]
fn the_stats_leave_two_blanks_before_them_at_the_least_and_the_edge_of_the_turn_wraps_before_it_takes_them() {
    let mk = || vec![turn("t", "go", "1"), stat(stat(with(row("b", "B claude", "working", "t"), "sonnet"), "", "1:04"), "", "$0.11")];
    let mut a = app_with(Json::Null, mk());
    // 20 + 11 = 31 columns of words and stats: 34 are needed for the gap and the spare cell
    assert_eq!(transcript(&mut a, 34, 8)[1], flush("╰─ ▷ B claude sonnet", "1:04  $0.11", 33));
    assert_eq!(transcript(&mut a, 33, 8)[1], flush("╰─ ▷ B claude", "1:04  $0.11", 32), "one blank between them is not enough");
    // a turn's own words and its time
    let mut t = app_with(Json::Null, vec![turn("t", "aaaaaaaaaa bbbbbbbbbbb", "12:00")]);
    assert_eq!(transcript(&mut t, 29, 6), [flush("aaaaaaaaaa", "12:00", 28), "bbbbbbbbbbb".to_string()].to_vec(), "22 letters and the time need two blanks between, and the spare cell: 30 columns (the words start at the edge)");
    assert_eq!(transcript(&mut t, 30, 6), [flush("aaaaaaaaaa bbbbbbbbbbb", "12:00", 29)].to_vec());
}

#[test]
fn one_control_row_is_still_folded_to_a_row_of_its_own() {
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1"), row("l", "A x", "working", "t"), marked(row("c", "resumed", "session", "t"), By::Core)]);
    let s = transcript(&mut a, 30, 8);
    assert_eq!(s[2], flush("╰─ ∙ 1", "▸", 29), "{s:#?}");
}

#[test]
fn a_step_without_a_mark_that_has_steps_under_it_has_no_blank_where_the_mark_would_be_and_a_lane_has() {
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1"), row("l", "B x", "working", "t"), row("g", "a group", "", "l"), row("k", "inner", "read", "g")]);
    let s = transcript(&mut a, 30, 8);
    assert_eq!((s[2].as_str(), s[3].as_str()), ("   ╰─ a group", "      ╰─ ≡ inner"), "{s:#?}");
    let mut b = app_with(Json::Null, vec![turn("t", "go", "1"), row("g", "a lane", "", "t"), row("k", "inner", "read", "g")]);
    assert_eq!(transcript(&mut b, 30, 8)[1], "╰─  a lane");
}

#[test]
fn only_a_lane_breathes_and_a_step_marked_as_working_is_a_step() {
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1"), row("l", "B x", "done", "t"), row("s", "still going", "working", "l")]);
    let before = Headless::new(40, 8).snap(&mut a).ansi;
    assert!(!a.breathes, "no lane works");
    a.tick(Instant::now() + Duration::from_millis(1000));
    assert_eq!(Headless::new(40, 8).snap(&mut a).ansi, before);
    let bold = a.theme.tone(Tone::Plain);
    let _ = bold;
    assert!(!before.lines().find(|l| l.contains("still going")).is_some_and(|l| l.contains(";1;")), "a step's words are not the bold of a lane's name");
}

#[test]
fn a_new_file_hides_its_diff_like_an_edit() {
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1"), row("l", "B x", "working", "t"), out(row("w", "new.rs", "new", "l"), &["+ fn main() {}", "+ // x"])]);
    let s = transcript(&mut a, 40, 8);
    assert_eq!(s, [flush("go", "1", 39), "╰─ ▷ B x".to_string(), flush("   ╰─ ⊕ new.rs", "▸", 39)].to_vec());
}

#[test]
fn a_minus_sign_of_either_kind_is_a_removed_line_and_a_line_with_a_tone_of_its_own_keeps_it() {
    let mut bad = Cell::new("boom", Tone::Bad);
    bad.icon = String::new();
    let mut r = out(row("e", "parser.rs", "edit", "l"), &["− gone", "- also gone", "kept"]);
    r.detail.push(bad);
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1"), row("l", "B x", "working", "t"), r]);
    hit_chord(&mut a, &["ctrl+up", "down", "down", "enter"]);
    let shown = lines(&mut a, 40, 12);
    let tones = Headless::new(40, 12).tones(&mut a);
    let at = |t: &str| shown.iter().position(|l| l.contains(t)).expect("the line");
    for (text, want) in [("− gone", 'r'), ("also gone", 'r'), ("kept", 'd'), ("boom", 'r')] {
        assert_eq!(tones[at(text)].chars().nth(shown[at(text)].find(text).map_or(0, |b| shown[at(text)][..b].chars().count())), Some(want), "{text}: {tones:#?}");
    }
}

#[test]
fn the_last_line_of_output_gives_way_to_its_count_and_nothing_goes_past_the_edge() {
    let long = "x".repeat(80);
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1"), row("l", "B x", "working", "t"), out(row("s", "cmd", "run", "l"), &["a", "b", "c", &long, &long])]);
    let s = transcript(&mut a, 40, 12);
    assert_eq!(s.len(), 6, "{s:#?}");
    assert!(s[5].ends_with("+2 ▸") && s[5].chars().count() <= 40 && s[5].contains("xxx…  "), "the words are cut to leave the count its room and a blank before it: {s:#?}");
    assert!(s.iter().all(|l| l.chars().count() <= 40), "{s:#?}");
}

#[test]
fn a_model_made_line_wraps_a_cell_earlier_for_its_tilde_and_its_other_lines_stand_under_the_words() {
    let words = format!("{} {}", "a".repeat(12), "b".repeat(12));
    let prose = |by: By| {
        let mut app = app_with(Json::Null, vec![turn("t", "go", "1"), row("l", "A x", "working", "t"), marked(row("p", &words, "", "l"), by)]);
        transcript(&mut app, 33, 8).split_off(2)
    };
    assert_eq!(prose(By::Doer), [format!("   ╰─ {words}")], "25 letters fit beside a pipe in 33 columns");
    assert_eq!(prose(By::Model), [format!("   ╰─ ~{}", "a".repeat(12)), format!("       {}", "b".repeat(12))], "the tilde takes a cell: the second line is under the words");
}

#[test]
fn a_star_is_a_bullet_a_quote_wraps_inside_its_bar_and_only_the_first_line_of_a_block_has_the_language() {
    let text = "* starred\n> a quoted aside that is long enough to need two rows\n```rs\n\tlet a = 1;\nlet b = 2;\n```";
    let mut a = app_with(json!({"width": 24}), vec![turn("t", "go", "1"), row("l", "A x", "working", "t"), row("p", text, "", "l")]);
    let s = transcript(&mut a, 60, 20);
    let body: Vec<&str> = s[2..].iter().map(String::as_str).collect();
    assert_eq!(body, ["   ╰─ • starred", "", "      ▏ a quoted aside that is", "      ▏ long enough to need", "      ▏ two rows", "", "                           rs", "         let a = 1;", "       let b = 2;"], "{s:#?}");
}

#[test]
fn a_code_line_as_wide_as_the_box_still_has_a_blank_before_the_language() {
    let mut a = app_with(json!({"width": 12}), vec![turn("t", "go", "1"), row("l", "A x", "working", "t"), row("p", "```rs\n0123456789ab\n```", "", "l")]);
    let s = transcript(&mut a, 60, 8);
    assert!(s[2].contains("0123456789… rs") || s[2].contains("0123456789a… rs") || s[2].ends_with(" rs"), "{s:#?}");
    assert!(!s[2].contains("…rs"), "{s:#?}");
}

// ---- the edges of the composer ----

fn gauge_line(v: f64) -> (String, String) {
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1")]);
    a.down(Down::Value { bind: "composer:strip".into(), value: json!({"gauge": v}), rev: 1 });
    let line = lines(&mut a, 40, 8)[4].clone();
    (line, Headless::new(40, 8).tones(&mut a)[4].clone())
}

#[test]
fn the_gauge_has_a_half_cell_from_a_twelfth_up_is_six_cells_at_the_most_and_is_warm_before_it_is_red() {
    assert!(gauge_line(0.04).0.contains("────── 4%"));
    assert!(gauge_line(0.07).0.contains("────── 7%"), "less than half a cell is none: the line never says more than there is");
    assert!(gauge_line(0.09).0.contains("╸───── 9%"));
    assert!(gauge_line(1.2).0.contains("━━━━━━ 120%"), "{:?}", gauge_line(1.2).0);
    let at = |v: f64| {
        let (l, t) = gauge_line(v);
        let byte = l.find('━').expect("a full cell");
        t.chars().nth(l[..byte].chars().count()).expect("a colour")
    };
    assert_eq!(at(1.0), 'y', "all of it is warm, not yet red");
    assert_eq!(at(1.01), 'r');
}

#[test]
fn a_strip_of_a_route_alone_is_a_strip_and_a_tag_without_a_mark_has_the_tag_mark() {
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1")]);
    a.down(Down::Value { bind: "composer:strip".into(), value: json!({"route": "claude"}), rev: 1 });
    let s = lines(&mut a, 40, 8);
    assert!(s[4].ends_with("~➜ claude │") && s[5].starts_with("│╌"), "{s:#?}");
    let mut b = app_with(Json::Null, vec![turn("t", "go", "1")]);
    b.down(Down::Value { bind: "composer:strip".into(), value: json!({"tags": [{"text": "lexer.rs"}]}), rev: 1 });
    assert!(lines(&mut b, 40, 8)[4].starts_with("│ # lexer.rs"));
}

#[test]
fn the_tags_and_the_right_of_the_strip_share_a_row_only_when_they_leave_two_blanks_and_a_tag_too_long_is_cut() {
    let strip = |text: &str| json!({"tags": [{"text": text, "icon": "tag"}], "gauge": 0.4, "route": "x"});
    // inside 36: a blank, `# ` 2, the words, `~➜ x` 4 + 1 + the gauge 11
    let right = "~➜ x ━━──── 40%";
    let fits = |n: usize| {
        let mut a = app_with(Json::Null, vec![turn("t", "go", "1")]);
        a.down(Down::Value { bind: "composer:strip".into(), value: strip(&"w".repeat(n)), rev: 1 });
        let s = lines(&mut a, 40, 8);
        (s.iter().any(|l| l.contains(right) && l.contains("# w")), 0)
    };
    let room = 36 - 2 - right.chars().count() - 2;
    assert!(fits(room).0, "{room} letters leave the two blanks");
    assert!(!fits(room + 1).0, "one more and the right goes under");
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1")]);
    a.down(Down::Value { bind: "composer:strip".into(), value: json!({"tags": [{"text": "x".repeat(60), "icon": "tag", "pct": 99}]}), rev: 1 });
    let s = lines(&mut a, 30, 8);
    assert!(s.iter().all(|l| l.chars().count() <= 30), "{s:#?}");
    assert!(s[4].starts_with("│ # xxx") && s[4].contains("…") && s[4].contains(" ~99% ") && s[4].ends_with('│') && s[4].chars().count() == 30, "the border and the percent are still there: {s:#?}");
    // and in a frame too narrow for most of it
    let t = lines(&mut a, 12, 8);
    assert!(t.iter().any(|l| l.contains("xxx…")) && t.iter().all(|l| l.chars().count() <= 12), "{t:#?}");
}

#[test]
fn the_line_breaks_at_the_width_a_row_has_and_never_before_it() {
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1")]);
    typing(&mut a, &"w".repeat(36));
    let s = lines(&mut a, 40, 8);
    assert_eq!(s[6], format!("│ {} │", "w".repeat(36)), "a row of exactly the width is one row: {s:#?}");
    assert_eq!(s[7], format!("╰{}╯", "─".repeat(38)));
    typing(&mut a, "w");
    let s = lines(&mut a, 40, 9);
    assert_eq!((s[6].clone(), s[7].clone()), (format!("│ {} │", "w".repeat(36)), format!("│ w{} │", " ".repeat(35))), "{s:#?}");
}

#[test]
fn a_blank_that_starts_a_row_is_not_a_place_to_break_a_word_at() {
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1")]);
    typing(&mut a, &format!("{} {}", "a".repeat(36), "b".repeat(5)));
    let s = lines(&mut a, 40, 9);
    assert_eq!((s[6].as_str(), s[7].as_str()), (format!("│ {} │", "a".repeat(36)).as_str(), format!("│ {}{} │", "b".repeat(5), " ".repeat(31)).as_str()), "{s:#?}");
    let mut b = app_with(Json::Null, vec![turn("t", "go", "1")]);
    typing(&mut b, &format!(" {}", "c".repeat(36)));
    let s = lines(&mut b, 40, 9);
    assert_eq!(s[6], format!("│  {} │", "c".repeat(35)), "{s:#?}");
}

#[test]
fn a_wide_letter_in_a_screen_that_is_too_narrow_does_not_stop_the_screen() {
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1")]);
    typing(&mut a, "世世世");
    for w in [5u16, 4, 3, 2, 1] {
        let s = lines(&mut a, w, 6);
        assert!(s.iter().all(|l| l.chars().count() <= usize::from(w) * 2), "{w}: {s:#?}");
    }
}

#[test]
fn a_short_screen_gives_the_composer_what_it_can_and_the_line_is_never_cut_below_a_row() {
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1")]);
    a.down(Down::Value { bind: "composer:strip".into(), value: strip(), rev: 1 });
    for _ in 0..6 {
        typing(&mut a, "x");
        hit_chord(&mut a, &["ctrl+j"]);
    }
    let s = lines(&mut a, 40, 8);
    assert_eq!(s.len(), 8);
    assert!(s[0].starts_with("go") && s[1].starts_with('╭') && s[7].starts_with('╰'), "a row is left to the transcript: {s:#?}");
    assert!(s[2].contains("parser.rs") && s[4].starts_with("│╌") && s[5].starts_with("│ x") && s[6].contains("⇧⏎ line"), "the last rows of the line, under the strip: {s:#?}");
    // a screen a row taller has the line a row taller
    let t = lines(&mut a, 40, 9);
    assert_eq!(t.iter().filter(|l| l.starts_with("│ x")).count(), 2);
    assert_eq!(s.iter().filter(|l| l.starts_with("│ x")).count(), 1);
}

#[test]
fn the_cursor_is_on_the_row_it_is_in_at_the_end_of_a_row_that_wrapped_and_in_a_line_that_scrolls() {
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1")]);
    typing(&mut a, &format!("{} {}", "a".repeat(30), "b".repeat(10)));
    let mut screen = Headless::new(40, 10);
    let at = |s: &mut Headless, a: &mut App| s.snap(a).cursor;
    let end = at(&mut screen, &mut a);
    for _ in 0..10 {
        hit(&mut a, KeyCode::Left);
    }
    assert_eq!(at(&mut screen, &mut a), (2, end.1), "after the blank, at the start of the second row");
    hit(&mut a, KeyCode::Left);
    assert_eq!(at(&mut screen, &mut a), (2 + 30, end.1 - 1), "before the blank that broke the row: at the end of the first row");
    hit(&mut a, KeyCode::Left);
    assert_eq!(at(&mut screen, &mut a), (2 + 29, end.1 - 1));
    // twelve rows: the last eight show, and the cursor at the top of the line is the top of them
    let mut b = app_with(Json::Null, vec![turn("t", "go", "1")]);
    for i in 0..12 {
        typing(&mut b, &format!("r{i}"));
        hit_chord(&mut b, &["ctrl+j"]);
    }
    let mut screen = Headless::new(40, 14);
    let bottom = at(&mut screen, &mut b);
    for _ in 0..20 {
        hit(&mut b, KeyCode::Left);
    }
    let up = at(&mut screen, &mut b);
    assert!(up.1 < bottom.1 && up.1 > 0, "{up:?} {bottom:?}");
    // three rows up from the end the line has scrolled by three: the cursor is on the row it is in
    let mut c = app_with(Json::Null, vec![turn("t", "go", "1")]);
    for i in 0..12 {
        typing(&mut c, &format!("r{i}"));
        hit_chord(&mut c, &["ctrl+j"]);
    }
    for _ in 0..3 {
        hit(&mut c, KeyCode::Left);
        hit(&mut c, KeyCode::Left);
        hit(&mut c, KeyCode::Left);
    }
    let mut screen = Headless::new(40, 20);
    let snap = screen.snap(&mut c);
    let row = snap.text.lines().nth(usize::from(snap.cursor.1)).unwrap_or_default();
    assert!(row.starts_with("│ r") && snap.text.lines().filter(|l| l.starts_with("│ r")).count() == 8, "{}", snap.text);
    let digits: String = row[row.find('r').unwrap_or(0) + 1..].chars().take_while(char::is_ascii_digit).collect();
    let at_cursor: usize = digits.parse().unwrap_or(99);
    assert_eq!(c.line.buf.chars().take(c.line.at).filter(|ch| *ch == '\n').count(), at_cursor, "the row under the cursor is the line it is in: {}", snap.text);
}

#[test]
fn the_keys_and_the_target_stand_on_the_last_row_shown_and_leave_the_room_they_need() {
    let mut a = app_with(Json::Null, lanes());
    hit_chord(&mut a, &["ctrl+t"]);
    for i in 0..12 {
        typing(&mut a, &format!("r{i}"));
        hit_chord(&mut a, &["ctrl+j"]);
    }
    for _ in 0..30 {
        hit(&mut a, KeyCode::Left);
    }
    let s = lines(&mut a, 40, 20);
    let at = s.iter().position(|l| l.starts_with('╭')).expect("the frame");
    let body: Vec<&String> = s[at + 1..].iter().take_while(|l| !l.starts_with('╰')).collect();
    assert_eq!(body.len(), 8, "{s:#?}");
    assert!(body[7].ends_with("→ B claude │"), "the target is on the last row that is shown: {s:#?}");
    assert!(body[..7].iter().all(|l| !l.contains('→')));
}

#[test]
fn the_last_row_of_a_line_leaves_the_target_two_blanks_and_the_rows_before_it_have_all_the_width() {
    let mut a = app_with(Json::Null, lanes());
    hit_chord(&mut a, &["ctrl+t"]);
    // 36 inside the frame; the target `→ B claude` is 10: a last row of 24 leaves exactly two
    let first = "f".repeat(36);
    typing(&mut a, &first);
    hit_chord(&mut a, &["ctrl+j"]);
    typing(&mut a, &"l".repeat(24));
    let s = lines(&mut a, 40, 14);
    let at = s.iter().position(|l| l.starts_with('╭')).expect("the frame");
    assert_eq!(s[at + 1], format!("│ {first} │"), "{s:#?}");
    assert_eq!(s[at + 2], format!("│ {}  → B claude │", "l".repeat(24)), "{s:#?}");
    typing(&mut a, "l");
    let t = lines(&mut a, 40, 14);
    let at = t.iter().position(|l| l.starts_with('╭')).expect("the frame");
    assert_eq!(t[at + 1], format!("│ {first} │"));
    assert!(t[at + 2].starts_with("│ llll") && t[at + 3].contains("→ B claude"), "one letter more, and the last line breaks sooner: {t:#?}");
    // the keys of a line of many rows are given the room the same way
    let mut b = app_with(Json::Null, vec![turn("t", "go", "1")]);
    typing(&mut b, &"a".repeat(36));
    hit_chord(&mut b, &["ctrl+j"]);
    typing(&mut b, &"b".repeat(18));
    let u = lines(&mut b, 40, 14);
    let at = u.iter().position(|l| l.starts_with('╭')).expect("the frame");
    assert_eq!(u[at + 2], format!("│ {}  ⇧⏎ line · ⏎ send │", "b".repeat(18)), "{u:#?}");
    typing(&mut b, "b");
    let v = lines(&mut b, 40, 14);
    let at = v.iter().position(|l| l.starts_with('╭')).expect("the frame");
    assert!(v[at + 2].starts_with("│ bbbb") && v[at + 2].chars().count() == 40 && v[at + 3].contains("⇧⏎ line"), "{v:#?}");
}

#[test]
fn a_card_taller_than_the_screen_leaves_the_composer_and_does_not_stop_the_screen() {
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1")]);
    a.down(Down::Value { bind: "shape:on".into(), value: json!(true), rev: 1 });
    let mut card = vec![Row { key: "~draft".into(), cells: vec![Cell::new("hello", Tone::Plain), Cell::new("ready", Tone::Plain)], ..Row::default() }];
    for i in 0..6 {
        card.push(row(&format!("n{i}"), &format!("note {i}"), "", ""));
    }
    a.down(Down::Rows { bind: "shape:card".into(), rows: card, total: 7, from: 0, rev: 1 });
    typing(&mut a, "hello");
    let s = lines(&mut a, 40, 6);
    assert_eq!(s.len(), 6);
    assert!(s.iter().any(|l| l.starts_with('╭')) && s.iter().any(|l| l.starts_with('╰')), "{s:#?}");
}

// ---- the keys, at their edges ----

#[test]
fn the_line_just_below_what_is_shown_is_brought_into_sight_by_one_row() {
    let mut a = app_with(Json::Null, long());
    for _ in 0..12 {
        hit(&mut a, KeyCode::PageUp);
    }
    hit_chord(&mut a, &["ctrl+up"]);
    for _ in 0..40 {
        hit(&mut a, KeyCode::Down);
        let s = transcript(&mut a, 40, 10);
        let at = a.wins["chat:main"].at.clone();
        let want = match at.as_deref() {
            Some(k) if k.starts_with('t') => format!("turn {}", &k[1..]),
            Some(k) if k.starts_with('l') => format!("lane {}", &k[1..]),
            Some(k) => format!("step {}", &k[1..]),
            None => break,
        };
        assert!(s.iter().any(|l| l.contains(&want)), "{at:?}: {s:#?}");
    }
}

#[test]
fn up_at_the_first_line_keeps_the_keys_and_down_past_the_last_gives_them_back() {
    let mut a = app_with(Json::Null, long());
    hit_chord(&mut a, &["ctrl+up", "ctrl+up", "ctrl+up", "ctrl+up", "ctrl+up", "ctrl+up"]);
    assert_eq!(a.wins["chat:main"].at.as_deref(), Some("t0"));
    hit(&mut a, KeyCode::Up);
    assert_eq!(a.wins["chat:main"].at.as_deref(), Some("t0"), "nothing above it");
    hit(&mut a, KeyCode::Down);
    assert_eq!(a.wins["chat:main"].at.as_deref(), Some("l0"));
}

#[test]
fn space_folds_like_enter_and_g_esc_give_the_keys_back_and_page_keys_move_them_half_a_page() {
    let mut a = app_with(Json::Null, one_lane());
    hit_chord(&mut a, &["ctrl+up", "down", "space"]);
    assert!(transcript(&mut a, 60, 20).iter().any(|l| l.contains("A omp") && l.ends_with('▾')));
    hit_chord(&mut a, &["G"]);
    assert_eq!((a.wins["chat:main"].at.clone(), a.line.buf.as_str()), (None, ""), "G goes back to the composer and the end, and is not a letter of the line");
    let mut b = app_with(Json::Null, long());
    still(&mut b);
    lines(&mut b, 40, 10);
    hit_chord(&mut b, &["ctrl+up", "pageup"]);
    assert_eq!(b.wins["chat:main"].at.as_deref(), Some("s4"), "half a page is half the rows of the screen less six, here two lines");
    hit_chord(&mut b, &["pagedown"]);
    assert_eq!(b.wins["chat:main"].at.as_deref(), Some("t5"));
    hit_chord(&mut b, &["pagedown", "pagedown"]);
    assert_eq!(b.wins["chat:main"].at.as_deref(), Some("s5"), "a page down goes to the end and stops there");
}

#[test]
fn a_delete_and_a_backspace_are_the_composers_and_a_control_key_is_not_a_letter() {
    let mut a = app_with(Json::Null, long());
    typing(&mut a, "ab");
    hit_chord(&mut a, &["ctrl+up", "delete"]);
    assert_eq!(a.wins["chat:main"].at, None);
    hit_chord(&mut a, &["ctrl+up", "ctrl+k"]);
    assert!(a.jump.is_some() && a.wins["chat:main"].at.is_some() || a.jump.is_some(), "the jump opens");
    let mut b = app_with(Json::Null, long());
    hit_chord(&mut b, &["ctrl+up", "ctrl+u"]);
    assert!(b.wins["chat:main"].at.is_some(), "a control key keeps the keys where they are");
}

#[test]
fn a_line_that_starts_with_a_slash_is_a_command_and_not_a_steer_and_a_lane_in_its_wait_can_be_steered() {
    let mut a = app_with(Json::Null, lanes());
    hit_chord(&mut a, &["ctrl+t"]);
    typing(&mut a, "/status");
    let sent = hit(&mut a, KeyCode::Enter);
    assert!(matches!(&sent[..], [Up::Intent { intent: Intent::Say { text, .. } }] if text == "/status"), "{sent:?}");
    let mut rows = lanes();
    rows[2] = row("b", "B claude", "cooling", "t");
    let mut b = app_with(Json::Null, rows);
    hit_chord(&mut b, &["ctrl+t"]);
    assert_eq!(b.aim.as_deref(), Some("b"));
}

#[test]
fn the_cursor_is_drawn_in_the_composer_and_left_where_it_was_while_the_keys_are_on_the_transcript() {
    let mut a = app_with(Json::Null, one_lane());
    let mut screen = Headless::new(40, 20);
    let typing_at = screen.snap(&mut a).cursor;
    hit_chord(&mut a, &["ctrl+up"]);
    a.paste("abc");
    assert_eq!(screen.snap(&mut a).cursor, typing_at, "the line grew and the cursor did not follow it");
    hit_chord(&mut a, &["esc"]);
    assert_eq!(screen.snap(&mut a).cursor, (typing_at.0 + 3, typing_at.1), "and it is the composer's again");
}

#[test]
fn the_keys_that_change_the_page_work_with_the_keys_on_the_transcript() {
    let chat = Node { id: "chat".into(), title: "Chat".into(), field: Some(Field { kind: FieldKind::Chat, bind: "chat:main".into(), props: Json::Null }), ..Node::default() };
    let other = Node { id: "other".into(), title: "Other".into(), field: Some(Field { kind: FieldKind::List, bind: "list:x".into(), props: Json::Null }), ..Node::default() };
    let mut a = App::new();
    a.start();
    a.down(hello());
    a.down(Down::Tree { node: Node { id: "root".into(), present: Present::Tabs, children: vec![chat, other], ..Node::default() } });
    look(&mut a, unicode());
    send(&mut a, long());
    hit_chord(&mut a, &["ctrl+up"]);
    assert_eq!(a.tab, 0);
    hit_chord(&mut a, &["f2"]);
    assert_eq!(a.tab, 1, "F2 is the second page");
    hit_chord(&mut a, &["f1"]);
    hit_chord(&mut a, &["alt+2"]);
    assert_eq!(a.tab, 1, "and so is ⌥2");
}

#[test]
fn the_clock_wakes_the_loop_for_the_next_breath_only_while_a_doer_works_in_sight() {
    let mut a = breathing();
    let t0 = Instant::now();
    assert_eq!(a.wake(t0), None, "nothing is drawn yet, so nothing breathes");
    a.tick(t0);
    Headless::new(40, 8).show(&mut a);
    assert_eq!(a.wake(t0), Some(Duration::from_millis(250)), "a tick is a quarter of a second");
    assert_eq!(a.wake(t0 + Duration::from_millis(100)), Some(Duration::from_millis(150)), "and the wait is what is left of it");
    assert_eq!(a.wake(t0 + Duration::from_millis(1000)), Some(Duration::from_millis(250)), "at the start of the next one it is whole again");
    let mut idle = app_with(Json::Null, vec![turn("t", "go", "1"), row("l", "B", "done", "t")]);
    idle.tick(t0);
    Headless::new(40, 8).show(&mut idle);
    assert_eq!(idle.wake(t0), None, "no doer works: the loop sleeps until a message");
    look(&mut a, Settings { marks: "unicode".into(), you: "mark".into(), motion: false, ..Settings::default() });
    Headless::new(40, 8).show(&mut a);
    assert_eq!(a.wake(t0), None, "and a look without motion has no breath to wake for");
}

#[test]
fn the_loop_wakes_at_the_nearer_of_the_breath_and_the_pause() {
    let mut a = breathing();
    a.down(Down::Value { bind: "shape:on".into(), value: Json::Bool(true), rev: 1 });
    for k in typed("fix the parser thing", false) {
        a.key(k);
    }
    let t0 = Instant::now();
    a.tick(t0);
    Headless::new(40, 8).show(&mut a);
    assert_eq!(a.wake(t0), Some(Duration::from_millis(250)), "the breath is nearer than the pause of 800 ms");
    assert_eq!(a.wake(t0 + Duration::from_millis(760)), Some(Duration::from_millis(40)), "and the pause when it is: 40 ms of it left, 240 of the tick");
}

// ---- the frames: pipes, hang, gutter ----

fn framed(frame: &str, marks: &str, rows: Vec<Row>) -> App {
    let mut a = app_with(Json::Null, rows);
    look(&mut a, Settings { marks: marks.into(), frame: frame.into(), you: "mark".into(), ..Settings::default() });
    a
}

#[test]
fn on_the_hang_a_lane_has_no_tree_and_its_steps_hang_from_a_rail_that_ends_in_an_elbow() {
    let mut a = framed("hang", "unicode", one_lane());
    let s = transcript(&mut a, 60, 20);
    assert_eq!(
        s,
        [
            flush("fix the parser", "12:29", 59),
            flush("✓ A omp sonnet  ≡ 1  ⊙ 1", "21s  $0.03 ▸", 59),
            flush("▷ B claude sonnet", "1:04", 59),
            flush("┆ ≡ a.rs · b.rs · c.rs", "3", 59),
            flush("┆ ± parser.rs", "+12 −3", 59),
            "╰ Trailing commas now parse.".to_string(),
        ]
        .to_vec(),
        "{s:#?}"
    );
    let mut a = framed("hang", "unicode", shell());
    let s = transcript(&mut a, 40, 12);
    assert_eq!((s[1].as_str(), s[3].as_str()), ("▷ B x", "    line 12"), "the output of the last step has no rail under it, and its text stands under the step's: {s:#?}");
}

#[test]
fn on_the_hang_the_rail_goes_on_under_a_step_that_has_output_while_there_is_a_step_after_it() {
    let mut rows = shell();
    rows.push(row("e", "done it", "edit", "l"));
    let mut a = framed("hang", "unicode", rows);
    let s = transcript(&mut a, 40, 12);
    assert_eq!((s[2].as_str(), s[3].as_str(), s[6].as_str()), ("┆ ❱ cargo test                   ✓ 3.1s", "┆   line 12", "╰ ± done it"), "{s:#?}");
}

#[test]
fn in_the_gutter_look_every_row_has_one_word_at_the_right_of_a_column_and_the_text_from_the_same_place() {
    let mut a = framed("gutter", "unicode", one_lane());
    let s = transcript(&mut a, 60, 20);
    assert_eq!(
        s,
        [
            flush("        ❯  fix the parser", "12:29", 59),
            flush("    A omp  sonnet  ≡ 1  ⊙ 1", "✓  21s  $0.03 ▸", 59),
            flush(" B claude  sonnet", "▷  1:04", 59),
            flush("        ≡  a.rs · b.rs · c.rs", "3", 59),
            flush("        ±  parser.rs", "+12 −3", 59),
            "           Trailing commas now parse.".to_string(),
        ]
        .to_vec(),
        "the state of a lane is at the right edge, with its stats: {s:#?}"
    );
    let mut a = framed("gutter", "unicode", shell());
    let s = transcript(&mut a, 40, 12);
    assert_eq!((s[1].as_str(), s[3].as_str()), ("   B x                                ▷", "        line 12"), "the gutter is 6 on a narrow screen, and the output stands under the text: {s:#?}");
}

#[test]
fn a_name_wider_than_the_gutter_loses_its_end_and_not_its_letter() {
    let mut a = framed("gutter", "unicode", vec![turn("t", "go", "1"), with(row("l", "C longnamed-agent", "done", "t"), "sonnet")]);
    let s = transcript(&mut a, 60, 8);
    assert!(s[1].starts_with("C longna…  sonnet"), "{s:#?}");
}

#[test]
fn a_state_the_stats_already_say_is_not_said_twice_in_the_gutter_look() {
    let rows = vec![turn("t", "go", "1"), stat(with(row("l", "D gem", "cooling", "t"), "pro"), "cooling", "0:40")];
    let mut a = framed("gutter", "unicode", rows);
    let s = transcript(&mut a, 60, 8);
    assert_eq!(s[1], flush("    D gem  pro", "◔ 0:40", 59), "{s:#?}");
}

#[test]
fn in_the_words_tier_a_lane_that_works_is_its_name_breathing_and_the_gutter_says_working() {
    let mut a = framed("pipes", "words", one_lane());
    let s = transcript(&mut a, 60, 20);
    assert!(s[2].starts_with("└─  B claude sonnet") && !s.concat().contains('●'), "no word stands before the name that breathes: {s:#?}");
    let mut a = framed("gutter", "words", one_lane());
    let s = transcript(&mut a, 60, 20);
    assert_eq!(s[2], flush(" B claude  sonnet", "working  1:04", 59), "{s:#?}");
    assert_eq!(s[3].trim_start().split(' ').next(), Some("read"), "a step has its verb in the gutter: {s:#?}");
}

#[test]
fn the_keys_fold_and_open_the_same_rows_in_every_frame() {
    for frame in ["pipes", "hang", "gutter"] {
        let mut a = framed(frame, "unicode", one_lane());
        let shut = transcript(&mut a, 60, 20).len();
        for k in ["ctrl+up", "down", "enter"] {
            hit_chord(&mut a, &[k]);
        }
        let open = transcript(&mut a, 60, 20);
        assert!(open.len() > shut, "{frame}: the lane under the keys opened: {open:#?}");
    }
}

#[test]
fn a_turn_that_wraps_keeps_the_gutter_blank_under_its_word_and_the_rail_of_the_hang_off_it() {
    let long = "fix the parser so trailing commas work and update the docs and then check it";
    for frame in ["hang", "gutter"] {
        let mut a = framed(frame, "unicode", vec![turn("t", long, "12:29")]);
        let s = transcript(&mut a, 60, 8);
        let lead = if frame == "gutter" { 11 } else { 0 };
        assert!(s.len() > 1 && s[1].starts_with(&" ".repeat(lead)) && !s[1].trim_start().is_empty(), "{frame}: {s:#?}");
    }
}

#[test]
fn words_of_the_doer_straight_under_a_turn_stand_where_the_text_of_a_row_with_a_mark_would_in_every_frame() {
    let rows = vec![turn("t", "go", "1"), row("p", "All done.", "", "t")];
    for (frame, want) in [("pipes", "└─ All done."), ("hang", "  All done."), ("gutter", "           All done.")] {
        let mut a = framed(frame, "unicode", rows.clone());
        let s = transcript(&mut a, 60, 8);
        assert_eq!(s[1], want, "{frame}: {s:#?}");
    }
}

// ---- air in the pipes, the spare cell, the band, the notice band ----

fn icons(you: &str) -> Settings {
    Settings { you: you.into(), ..Settings::default() }
}

fn notice(key: &str, text: &str, parent: &str, by: By, tone: Tone) -> Row {
    let mut r = row(key, text, "notice", parent);
    r.by = by;
    r.cells[0].tone = tone;
    r
}

#[test]
fn a_level_is_three_cells_and_every_icon_has_its_space_so_the_glyph_never_touches_the_words() {
    // an icon has a blank after it, so the glyph never touches the words
    let mut a = app_with(Json::Null, one_lane());
    look(&mut a, icons("band"));
    let s = transcript(&mut a, 60, 20);
    assert!(s[1].starts_with("├─ \u{F05E1}  A omp sonnet"), "{s:#?}");
    assert!(s[2].starts_with("╰─ \u{F0F1B}  B claude"), "{s:#?}");
    assert!(s[3].starts_with("   ├─ \u{F06D0}  a.rs · b.rs · c.rs"), "the same at every depth: {s:#?}");
    let mut rows = one_lane();
    rows.push(row("c", "C pi", "queued", "t"));
    let mut b = app_with(Json::Null, rows);
    look(&mut b, icons("band"));
    let s = transcript(&mut b, 60, 20);
    assert!(s[3].starts_with("│  ├─ \u{F06D0}  a.rs") && s[5].starts_with("│  ╰─ Trailing"), "a bar is three cells too: {s:#?}");
}

#[test]
fn rows_end_one_cell_before_the_edge_and_an_icon_that_ends_a_row_keeps_its_space() {
    // an icon that ends a row keeps its blank
    for w in [40u16, 60, 80] {
        let mut a = app_with(Json::Null, vec![turn("t", "go", "12:00"), stat(stat(row("b", "B claude", "working", "t"), "", "1:04"), "read-only", "")]);
        look(&mut a, icons("band"));
        let all = screen(&mut a, w, 10);
        let at = all.iter().position(|l| l.contains("B claude")).expect("the lane");
        assert!(all[at].ends_with('\u{F0341}') && all[at].chars().count() == usize::from(w) - 2, "the last stat is an icon with its blank, and then the spare cell: {:?}", all[at]);
        let s = transcript(&mut a, w, 10);
        assert!(s.iter().all(|l| l.chars().count() < usize::from(w)), "no row reaches the edge: {s:#?}");
    }
}

#[test]
fn your_line_is_a_raised_band_with_the_time_at_its_right_and_no_icon_and_a_blank_row_above_it() {
    let mut a = app_with(Json::Null, vec![turn("t", "fix the parser", "12:29"), row("l", "A x", "working", "t")]);
    look(&mut a, icons("band"));
    let all = lines(&mut a, 40, 10);
    let at = all.iter().position(|l| l.contains("fix the parser")).expect("your line");
    assert_eq!(all[at], flush("  fix the parser", "12:29", 38), "your words start on the composer's text column and the time ends where its text ends: {all:#?}");
    assert!(all[at - 1].is_empty(), "a blank row above the first turn too: {all:#?}");
    assert!(!all.iter().any(|l| l.contains('\u{F0013}')), "{all:#?}");
    // the band is the Raise background across the whole width, to the last cell
    let raise = a.theme.roles.get(j4n_tui::theme::palette::Role::Raise);
    let ansi = Headless::new(40, 10).snap(&mut a).ansi;
    let line = ansi.lines().find(|l| l.contains("fix the parser")).expect("the line");
    assert!(line.contains(&format!("48;2;{};{};{}", raise.0, raise.1, raise.2)), "{line:?}");
    assert!(!ansi.lines().find(|l| l.contains("A x")).expect("the lane").contains(&format!("48;2;{};{};{}", raise.0, raise.1, raise.2)));
    // a line of several rows is a band on each
    let mut b = app_with(Json::Null, vec![turn("t", "aaaaaaaaaa bbbbbbbbbbb ccccccccccc", "12:00")]);
    look(&mut b, icons("band"));
    let ansi = Headless::new(32, 8).snap(&mut b).ansi;
    assert_eq!(ansi.lines().filter(|l| l.contains(&format!("48;2;{};{};{}", raise.0, raise.1, raise.2))).count(), 3, "{ansi:?}");
    // and each is the width of the screen: the last cell of a wrapped row is in the band too
    let shown = |l: &str| {
        let (mut n, mut esc) = (0, false);
        for c in l.chars() {
            match (esc, c) {
                (_, '\x1b') => esc = true,
                (true, 'm') => esc = false,
                (false, _) => n += 1,
                _ => {}
            }
        }
        n
    };
    let cells: Vec<usize> = ansi.lines().filter(|l| l.contains(&format!("48;2;{};{};{}", raise.0, raise.1, raise.2))).map(shown).collect();
    assert!(cells.iter().all(|n| *n == 32), "{cells:?} {ansi:?}");
}

#[test]
fn the_mark_look_has_no_person_icon_on_the_pipes_and_a_thin_mark_in_the_gutter() {
    // on the pipes the words start at the left edge; in the gutter the mark is the thin one-cell `❯` of the slot, in every tier of marks
    let mut a = app_with(Json::Null, vec![turn("t", "go", "12:29")]);
    look(&mut a, icons("mark"));
    let s = transcript(&mut a, 40, 6);
    assert_eq!(s[0], flush("go", "12:29", 39), "{s:#?}");
    let mut g = framed("gutter", "icons", vec![turn("t", "fix the parser", "12:29")]);
    let s = transcript(&mut g, 60, 8);
    assert_eq!(s[0], flush("        ❯  fix the parser", "12:29", 59), "{s:#?}");
}

#[test]
fn what_the_doer_says_about_the_run_leaves_the_tree_for_a_band_after_it() {
    let words = "claude · 80% of the 5-hour limit used · resets 14:20";
    let rows = vec![
        turn("t", "go", "1"),
        row("l", "A x", "working", "t"),
        row("s1", "a.rs", "read", "l"),
        notice("n1", words, "l", By::Doer, Tone::Warn),
        notice("n2", words, "l", By::Doer, Tone::Warn),
        row("s2", "done", "", "l"),
        notice("c1", "claude-7c1 resumes", "t", By::Core, Tone::Dim),
    ];
    let mut a = app_with(Json::Null, rows);
    look(&mut a, icons("band"));
    let s = transcript(&mut a, 70, 20);
    let band = s.iter().position(|l| l.contains("80%")).expect("the notice");
    assert_eq!(s[band - 1], "", "a blank row, then the band: {s:#?}");
    assert_eq!(s[band], flush(" \u{F009C}  claude · 80% of the 5-hour limit used · resets 14:20", "", 69).trim_end(), "no pipe, its own icon: {s:#?}");
    assert_eq!(s.iter().filter(|l| l.contains("80%")).count(), 1, "said twice, shown once: {s:#?}");
    assert_eq!(band, s.len() - 1, "after the turn's tree: {s:#?}");
    assert!(s[band - 2].contains("∙") || s[band - 2].contains('\u{F08BB}'), "the tree ends with the control rows `╰`: {s:#?}");
    assert!(!s[..band].iter().any(|l| l.contains("80%")), "{s:#?}");
    // the lane no longer shows a folder of it: its steps are the answer's
    assert!(s.iter().any(|l| l.contains("a.rs")) && s.iter().any(|l| l.contains("done")));
    // the band has a wash of its own, not the raised one
    let ansi = Headless::new(70, 20).snap(&mut a).ansi;
    let raise = a.theme.roles.get(j4n_tui::theme::palette::Role::Raise);
    let line = ansi.lines().find(|l| l.contains("80%")).expect("the line");
    assert!(line.contains("48;2;") && !line.contains(&format!("48;2;{};{};{}", raise.0, raise.1, raise.2)), "{line:?}");
}

#[test]
fn a_notice_of_j4n_s_own_about_a_task_stands_in_its_lane_where_it_was_said() {
    // the quiet notice was lifted after the turn, below the steps that came after it; J4n's own words about a task stay in its lane
    let rows = vec![
        turn("t", "go", "1"),
        row("l", "A x", "working", "t"),
        row("s1", "a.rs", "read", "l"),
        notice("q", "Nothing has been heard from this task for 5 minutes.", "l", By::Core, Tone::Dim),
        row("s2", "b.rs", "read", "l"),
    ];
    let mut a = app_with(Json::Null, rows);
    look(&mut a, icons("band"));
    let s = transcript(&mut a, 70, 20);
    let at = |w: &str| s.iter().position(|l| l.contains(w)).unwrap_or_else(|| panic!("no {w}: {s:#?}"));
    assert!(at("a.rs") < at("Nothing has been heard") && at("Nothing has been heard") < at("b.rs"), "between the steps it came between: {s:#?}");
}

#[test]
fn a_notice_that_wraps_stands_under_its_words_in_the_band() {
    let words = "claude · 80% of the 5-hour limit used · resets Tue 14:20 so be careful with the next long run";
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1"), row("l", "A x", "working", "t"), notice("n", words, "l", By::Doer, Tone::Dim)]);
    look(&mut a, icons("band"));
    let s = transcript(&mut a, 40, 20);
    let at = s.iter().position(|l| l.contains("80%")).expect("the notice");
    assert!(s[at + 1].starts_with("    ") && !s[at + 1].trim().is_empty(), "{s:#?}");
    assert!(s.iter().all(|l| l.chars().count() < 40), "{s:#?}");
}

// ---- scrolling ----

/// A turn whose words wrap over many lines at 20 columns, then another.
fn wrapped() -> Vec<Row> {
    let words: Vec<String> = (1..=24).map(|n| format!("w{n:02}")).collect();
    vec![turn("t0", &words.join(" "), "1"), turn("t1", "after", "2")]
}

#[test]
fn shift_up_and_shift_down_scroll_the_chat_a_line_and_leave_the_keys_in_the_composer() {
    let mut a = app_with(Json::Null, long());
    typing(&mut a, "draft");
    let end = transcript(&mut a, 40, 10);
    assert!(hit_chord(&mut a, &["shift+up"]).is_empty());
    assert_eq!(a.scroll.get("chat:main"), Some(&1), "one line");
    assert_eq!((a.wins.get("chat:main").and_then(|w| w.at.clone()), a.line.buf.as_str()), (None, "draft"), "the keys are still the composer's and nothing was recalled");
    let one = transcript(&mut a, 40, 10);
    assert!(end.iter().any(|l| l.contains("step 5")) && !one.iter().any(|l| l.contains("step 5")), "the last line left the screen: {one:#?}");
    hit_chord(&mut a, &["shift+up", "shift+up", "shift+down"]);
    assert_eq!(a.scroll.get("chat:main"), Some(&2));
    hit_chord(&mut a, &["shift+down", "shift+down", "shift+down"]);
    assert_eq!(a.scroll.get("chat:main").copied().unwrap_or(0), 0, "no further down than the end");
    assert_eq!(a.line.buf, "draft");
}

#[test]
fn with_the_keys_on_the_chat_up_and_down_walk_the_lines_of_a_wrapped_row_and_the_view_follows() {
    let mut a = app_with(Json::Null, wrapped());
    hit_chord(&mut a, &["ctrl+up", "ctrl+up"]);
    assert_eq!(a.wins["chat:main"].at.as_deref(), Some("t0"));
    // the row's own lines, then the one after: each ↓ is one line
    let mut seen = Vec::new();
    for _ in 0..40 {
        hit(&mut a, KeyCode::Down);
        let w = &a.wins["chat:main"];
        let Some(at) = w.at.clone() else { break };
        seen.push((at, w.sub));
        let band = Headless::new(20, 10).banded(&mut a);
        assert!(!band.is_empty(), "the line the keys are on is in sight: {seen:?}");
    }
    let inside: Vec<usize> = seen.iter().take_while(|s| s.0 == "t0").map(|s| s.1).collect();
    assert!(inside.len() >= 5 && inside == (1..=inside.len()).collect::<Vec<_>>(), "a wrapped row is walked one line at a time: {seen:?}");
    assert!(seen.iter().any(|s| s.0 == "t1"), "and then the next row: {seen:?}");
    assert_eq!(seen.last().map(|s| s.0.as_str()), Some("t1"));
    // and back: each ↑ is one line, the head last
    hit_chord(&mut a, &["ctrl+up", "ctrl+up"]);
    let first = a.wins["chat:main"].sub;
    assert_eq!((a.wins["chat:main"].at.as_deref(), first), (Some("t0"), 0), "a jump of turns lands on the head");
    hit(&mut a, KeyCode::Down);
    hit(&mut a, KeyCode::Down);
    assert_eq!(a.wins["chat:main"].sub, 2);
    hit(&mut a, KeyCode::Up);
    assert_eq!(a.wins["chat:main"].sub, 1);
    hit(&mut a, KeyCode::Up);
    assert_eq!((a.wins["chat:main"].at.as_deref(), a.wins["chat:main"].sub), (Some("t0"), 0));
}

#[test]
fn the_line_walked_to_has_the_selection_behind_it_and_is_on_screen_at_every_step() {
    let mut edit = stat(out(row("e", "parser.rs", "edit", "l"), &["- gone", "+ new", "- removed", "ctx"]), "", "+3 −7");
    edit.detail[0].text = "- gone".into();
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1"), row("l", "B x", "working", "t"), edit]);
    hit_chord(&mut a, &["ctrl+up", "down", "down", "enter"]);
    for (n, want) in ["- gone", "+ new", "- removed", "ctx"].into_iter().enumerate() {
        hit(&mut a, KeyCode::Down);
        let w = &a.wins["chat:main"];
        assert_eq!((w.at.as_deref(), w.sub), (Some("e"), n + 1));
        let band = Headless::new(40, 12).banded(&mut a);
        assert!(band.len() == 2 && band[0].contains("parser.rs") && band[1].contains(want), "the head and the line the keys are on: {band:#?}");
    }
    hit(&mut a, KeyCode::Down);
    assert_eq!(a.wins["chat:main"].at, None, "past the last line the keys go back to the composer");
}

#[test]
fn esc_or_a_letter_from_a_line_under_a_head_goes_back_to_the_composer_at_the_end() {
    let mut a = app_with(Json::Null, wrapped());
    lines(&mut a, 20, 10);
    hit_chord(&mut a, &["ctrl+up", "ctrl+up", "down", "down", "down"]);
    assert!(a.wins["chat:main"].sub > 0);
    hit(&mut a, KeyCode::Esc);
    assert_eq!((a.wins["chat:main"].at.clone(), a.wins["chat:main"].sub), (None, 0));
    assert!(!a.scroll.contains_key("chat:main"));
    hit_chord(&mut a, &["ctrl+up", "ctrl+up", "down", "down"]);
    assert!(a.wins["chat:main"].sub > 0);
    typing(&mut a, "q");
    assert_eq!((a.wins["chat:main"].at.clone(), a.line.buf.as_str(), a.wins["chat:main"].sub), (None, "q", 0), "a letter is the composer's");
}

#[test]
fn page_keys_glide_half_a_screen_in_steps_and_are_over_in_a_tenth_of_a_second() {
    let mut a = app_with(Json::Null, long());
    a.size = (40, 20);
    let t0 = Instant::now();
    a.set_clock(t0);
    let lines = |a: &App| a.scroll.get("chat:main").copied().unwrap_or(0);
    hit(&mut a, KeyCode::PageUp);
    // half of 20 − 6 is 7 lines: 2, 2, 2, 1, the first with the key
    assert_eq!(lines(&a), 2, "the first step comes with the key");
    assert_eq!(a.wake(t0), Some(Duration::from_millis(30)), "and the next is due in a frame");
    a.tick(t0 + Duration::from_millis(10));
    assert_eq!(lines(&a), 2, "not before it is due");
    let mut seen = vec![lines(&a)];
    for step in 1..=3 {
        a.tick(t0 + Duration::from_millis(30 * step));
        assert!(a.take_redraw(), "each step is drawn");
        seen.push(lines(&a));
    }
    assert_eq!(seen, [2, 4, 6, 7], "{seen:?}");
    assert_eq!(a.wake(t0 + Duration::from_millis(90)), None, "ninety milliseconds in, it is over and the screen sleeps");
    a.tick(t0 + Duration::from_millis(500));
    assert_eq!(lines(&a), 7);
    // a second press while it glides adds to it; the other way takes from it
    hit(&mut a, KeyCode::PageDown);
    assert_eq!(lines(&a), 5);
    a.tick(t0 + Duration::from_millis(600));
    a.tick(t0 + Duration::from_millis(700));
    a.tick(t0 + Duration::from_millis(800));
    a.tick(t0 + Duration::from_millis(900));
    assert_eq!(lines(&a), 0, "down by the same half screen");
}

#[test]
fn a_glide_stops_at_any_other_key() {
    let mut a = app_with(Json::Null, long());
    a.size = (40, 20);
    let t0 = Instant::now();
    a.set_clock(t0);
    hit(&mut a, KeyCode::PageUp);
    assert!(a.wake(t0).is_some());
    typing(&mut a, "x");
    assert_eq!(a.wake(t0), None);
    a.tick(t0 + Duration::from_millis(200));
    assert_eq!(a.scroll.get("chat:main"), Some(&2), "what was drawn stays");
}

#[test]
fn without_motion_the_page_keys_jump_half_a_screen_at_once() {
    let mut a = app_with(Json::Null, long());
    still(&mut a);
    a.size = (40, 20);
    let t0 = Instant::now();
    a.set_clock(t0);
    hit(&mut a, KeyCode::PageUp);
    assert_eq!(a.scroll.get("chat:main"), Some(&7));
    assert_eq!(a.wake(t0), None, "nothing is left to draw later");
    hit(&mut a, KeyCode::PageDown);
    assert_eq!(a.scroll.get("chat:main").copied().unwrap_or(0), 0);
}

#[test]
fn with_the_keys_on_the_chat_the_page_keys_move_the_line_by_half_a_screen_and_the_view_follows() {
    let mut a = app_with(Json::Null, long());
    a.size = (40, 20);
    let t0 = Instant::now();
    a.set_clock(t0);
    lines(&mut a, 40, 20);
    hit_chord(&mut a, &["ctrl+up", "pageup"]);
    // t5 is line 20 of 23: two of the seven lines now …
    assert_eq!(a.wins["chat:main"].at.as_deref(), Some("s4"));
    for step in 1..=3 {
        a.tick(t0 + Duration::from_millis(30 * step));
    }
    // … and seven when the glide is over: line 13, the head of the lane of turn 3
    assert_eq!((a.wins["chat:main"].at.as_deref(), a.wins["chat:main"].sub), (Some("l3"), 0));
    assert!(transcript(&mut a, 40, 20).iter().any(|l| l.contains("lane 3")));
    // without motion it is the same place at once
    let mut b = app_with(Json::Null, long());
    still(&mut b);
    b.size = (40, 20);
    lines(&mut b, 40, 20);
    hit_chord(&mut b, &["ctrl+up", "pageup"]);
    assert_eq!((b.wins["chat:main"].at.as_deref(), b.wins["chat:main"].sub), (Some("l3"), 0));
}

#[test]
fn the_wheel_is_three_lines_from_the_composer_and_three_lines_of_the_cursor_on_the_chat() {
    let mut a = app_with(Json::Null, long());
    a.wheel(true);
    assert_eq!(a.scroll.get("chat:main"), Some(&3));
    a.wheel(false);
    assert_eq!(a.scroll.get("chat:main").copied().unwrap_or(0), 0);
    hit_chord(&mut a, &["ctrl+up"]);
    a.wheel(true);
    assert_eq!(a.wins["chat:main"].at.as_deref(), Some("l4"), "three lines up from the newest turn, over the blank row between turns");
}

// ---- a lone lane has no row ----

fn lane_of(mut r: Row, doer: &str, own: bool) -> Row {
    r.data = Some(json!({ "lane": { "doer": doer, "own": own } }));
    r
}

fn lone_turn() -> Vec<Row> {
    vec![
        turn("t", "fix the parser", "12:29"),
        lane_of(stat(stat(row("a", "A work", "working", "t"), "", "1:04"), "", "2.1k tok"), "claude", true),
        row("a1", "a.rs", "read", "a"),
        row("a2", "Trailing commas now parse.", "", "a"),
    ]
}

#[test]
fn a_turn_with_one_lane_has_no_row_for_it_and_its_steps_hang_from_the_line() {
    let mut a = app_with(Json::Null, lone_turn());
    look(&mut a, icons("band"));
    let s = transcript(&mut a, 70, 12);
    assert!(!s.iter().any(|l| l.contains("A work")), "no lane row: {s:#?}");
    assert!(s[0].starts_with("  fix the parser") && s[0].ends_with("1:04  2.1k tok  12:29"), "the lane's clock and tokens come before the time of day: {s:#?}");
    assert!(s[1].starts_with("├─ \u{F06D0}  a.rs"), "a step hangs from the line, one level up: {s:#?}");
    assert!(s[2].starts_with("╰─ Trailing commas now parse."), "{s:#?}");
}

#[test]
fn a_turn_with_two_lanes_keeps_the_lettered_rows() {
    let mut rows = lone_turn();
    rows.push(lane_of(row("b", "B review", "queued", "t"), "claude", true));
    let mut a = app_with(Json::Null, rows);
    look(&mut a, icons("band"));
    let s = transcript(&mut a, 70, 12);
    assert!(s.iter().any(|l| l.contains("A work")) && s.iter().any(|l| l.contains("B review")), "{s:#?}");
    assert!(s[0].ends_with("12:29") && !s[0].contains("tok"), "the facts stay on the lane rows: {s:#?}");
    assert!(s.iter().any(|l| l.starts_with("│  ├─")), "a step is a level down: {s:#?}");
}

/// A flow and its one lane of code are one lane to the screen: the flow's row is not drawn, the code step stands as the lone lane, and what hung
/// from the flow's row hangs from the line. A flow with two steps of code keeps its rows.
#[test]
fn a_flow_with_one_step_of_code_has_one_lane_and_one_with_two_keeps_its_rows() {
    let flow = |key: &str, text: &str| {
        let mut r = row(key, text, "done", "t");
        r.data = Some(json!({ "lane": { "doer": "flow:account", "own": false, "flow": true } }));
        r
    };
    let code = |key: &str, text: &str| {
        let mut r = row(key, text, "done", "t");
        r.data = Some(json!({ "lane": { "doer": "account", "own": false, "code": true } }));
        r
    };
    let mut rows = vec![turn("t", ":account check work", "12:29"), flow("a", "A flow:account"), code("b", "B account"), row("b1", "account `work` is signed in.", "", "b")];
    let mut a = app_with(Json::Null, rows.clone());
    look(&mut a, icons("band"));
    let s = transcript(&mut a, 70, 12);
    assert!(!s.iter().any(|l| l.contains("A flow:account") || l.contains("B account")), "no row for the flow or for its step: {s:#?}");
    assert!(s[0].starts_with("  :account check work") && s[0].contains("account"), "the doer is named on the line, which is not the thread's own: {s:#?}");
    assert!(s.iter().any(|l| l.starts_with("╰─ account work is signed in.")), "the step's words hang from the line: {s:#?}");
    rows.push(code("c", "C account"));
    let mut a = app_with(Json::Null, rows);
    look(&mut a, icons("band"));
    let s = transcript(&mut a, 70, 14);
    assert!(s.iter().any(|l| l.contains("A flow:account")) && s.iter().any(|l| l.contains("B account")) && s.iter().any(|l| l.contains("C account")), "{s:#?}");
}

#[test]
fn a_lone_lane_on_another_doer_says_it_on_the_line_and_a_failed_one_keeps_its_mark() {
    let mut rows = lone_turn();
    rows[1] = lane_of(with(stat(row("a", "A omp", "failed", "t"), "", "1:04"), "sonnet"), "omp", false);
    let mut a = app_with(Json::Null, rows);
    look(&mut a, icons("band"));
    let s = transcript(&mut a, 70, 12);
    assert!(!s.iter().any(|l| l.contains("A omp")), "{s:#?}");
    assert!(s[0].contains("omp · sonnet  1:04"), "{s:#?}");
    assert!(s[0].trim_end_matches('5').len() > s[0].find("1:04").unwrap() + 4, "{s:#?}");
}

#[test]
fn a_lane_that_did_not_run_is_the_warn_mark_and_the_words_not_run_and_never_the_tick() {
    // two lanes, so that each has its row: A did its work, B (a check) ended well and could not run
    let not_run = |icon: &str| {
        let mut b = row("b", "B check", icon, "t");
        b.cells.push(Cell::new("not run", Tone::Warn));
        lane_of(b, "check", false)
    };
    let mut a = app_with(Json::Null, vec![turn("t", "go", "12:29"), lane_of(row("a", "A work", "done", "t"), "claude", true), not_run("risky")]);
    look(&mut a, unicode());
    let s = transcript(&mut a, 50, 12);
    let at = s.iter().position(|l| l.contains("B check")).expect("the lane");
    assert!(s[at].contains('⚠') && s[at].contains("not run") && !s[at].contains('✓'), "{s:#?}");
    assert!(s.iter().any(|l| l.contains("A work") && l.contains('✓')), "a lane that did its work is still a tick: {s:#?}");
    // a lone lane says it on the line of the turn, where the tick of a lane that is well is not drawn at all
    let mut rows = lone_turn();
    rows[1] = lane_of(stat(with(row("a", "A check", "risky", "t"), ""), "", "not run"), "check", false);
    let mut a = app_with(Json::Null, rows);
    look(&mut a, unicode());
    let s = transcript(&mut a, 70, 12);
    assert!(s[0].contains("not run") && s[0].contains('⚠') && !s[0].contains('✓'), "{s:#?}");
}

#[test]
fn the_facts_of_a_lone_lane_give_way_when_the_line_is_narrow() {
    let mut a = app_with(Json::Null, lone_turn());
    look(&mut a, icons("band"));
    let s = transcript(&mut a, 30, 12);
    assert!(s[0].ends_with("12:29") && s[0].chars().count() <= 28, "{s:#?}");
}

#[test]
fn the_band_meets_the_composer_in_the_same_columns() {
    // the words on the composer's text column (2), the time ending where the composer's text ends, the band over the border cells
    for w in [40u16, 60, 80, 120] {
        let mut a = app_with(Json::Null, vec![turn("t", "hello", "12:29")]);
        look(&mut a, icons("band"));
        let all = lines(&mut a, w, 12);
        let at = all.iter().position(|l| l.contains("hello")).expect("your line");
        let box_top = all.iter().position(|l| l.starts_with('╭')).expect("the composer");
        assert_eq!(all[at].find("hello"), Some(2), "{all:#?}");
        assert_eq!(all[at].chars().count(), usize::from(w) - 2, "the time ends two cells before the edge, as the composer's text does: {all:#?}");
        let ansi = Headless::new(w, 12).snap(&mut a).ansi;
        let raise = a.theme.roles.get(j4n_tui::theme::palette::Role::Raise);
        let line = ansi.lines().find(|l| l.contains("hello")).expect("the band");
        assert!(line.starts_with(&format!("\x1b[0;48;2;{};{};{}", raise.0, raise.1, raise.2)) || line.contains(&format!("48;2;{};{};{}", raise.0, raise.1, raise.2)), "{line:?}");
        assert_eq!(all[box_top].chars().count(), usize::from(w));
    }
}

#[test]
fn the_look_word_mouse_decides_whether_the_mouse_is_asked_for_and_the_variable_overrides_it() {
    use j4n_tui::term::mouse_wanted;
    use std::ffi::OsStr;
    assert!(Theme::default().mouse, "on by default, and shift-drag selects");
    let off = Theme::build(&Settings { mouse: "off".into(), ..Settings::default() }, &[]).expect("a theme");
    assert!(!off.mouse);
    assert!(Theme::build(&Settings { mouse: "sometimes".into(), ..Settings::default() }, &[]).is_err());
    assert!(mouse_wanted(true, None) && !mouse_wanted(false, None));
    assert!(mouse_wanted(false, Some(OsStr::new("1"))) && !mouse_wanted(true, Some(OsStr::new("0"))));
    assert!(mouse_wanted(true, Some(OsStr::new("x"))), "any other word leaves it to the look");
}

// ---- the ids of the project's docs in an answer ----

fn answer_with_ids() -> App {
    let mut a = app_with(Json::Null, vec![turn("t", "what is open", "12:30"), row("p", "Open loose ends: X-101 and B12, also X-999.", "", "t")]);
    a.down(Down::Value { bind: j4n_proto::TERMS.into(), value: json!({ "X-101": "some row of text", "B12": "a test bug" }), rev: 1 });
    a
}

#[test]
fn a_known_id_in_the_doers_words_is_drawn_with_its_words_and_one_the_docs_lack_is_not() {
    let mut a = answer_with_ids();
    let t = transcript(&mut a, 90, 12).join("\n");
    assert!(t.contains("X-101 · some row of text and B12 · a test bug, also X-999."), "{t}");
}

#[test]
fn enter_with_the_keys_on_a_line_that_names_ids_asks_for_their_rows_and_never_sends() {
    let mut a = answer_with_ids();
    typing(&mut a, "draft");
    let sent = hit_chord(&mut a, &["ctrl+up", "down", "enter"]);
    assert_eq!(sent, [Up::Intent { intent: Intent::Run { command: "term".into(), args: vec!["X-101".into(), "B12".into()] } }], "the ids the docs define, in the order named");
    assert_eq!(a.line.buf, "draft");
    // a line with none does nothing
    let mut a = app_with(Json::Null, vec![turn("t", "what is open", "12:30"), row("p", "Nothing to cite.", "", "t")]);
    assert!(hit_chord(&mut a, &["ctrl+up", "down", "enter"]).is_empty());
}

#[test]
fn the_setting_read_terms_off_draws_the_ids_as_written() {
    let mut a = answer_with_ids();
    let mut look = j4n_proto::Look { palette: "jjj".into(), marks: "unicode".into(), frame: "pipes".into(), corners: "round".into(), title_place: "inside".into(), density: "normal".into(), fill: "match".into(), you: "mark".into(), motion: true, mouse: "on".into(), read: Default::default(), time: "ago".into(), chart: "dither".into() };
    look.read.terms = false;
    a.down(Down::Theme { look });
    let t = transcript(&mut a, 90, 12).join("\n");
    assert!(t.contains("Open loose ends: X-101 and B12, also X-999.") && !t.contains('·'), "{t}");
    // and a glossary that comes after is not kept either
    a.down(Down::Value { bind: j4n_proto::TERMS.into(), value: json!({ "X-101": "x" }), rev: 2 });
    assert!(!transcript(&mut a, 90, 12).join("\n").contains("· x"));
}

// ---- the strip carries the chat's state ----

fn working(a: &mut App, work: Json) {
    let v = json!({"tags": [], "gauge": 0.42, "route": "claude · sonnet 5.5", "route_by": "core", "work": work, "cost": "0.8% of 5h · $0.33 list", "mode": "write"});
    a.down(Down::Value { bind: "composer:strip".into(), value: v, rev: 1 });
}

/// The strip's row: the one under the frame's top edge.
fn strip_row(a: &mut App, cols: u16) -> String {
    let s = lines(a, cols, 12);
    let at = s.iter().position(|l| l.starts_with('╭')).expect("the frame");
    s[at + 1].clone()
}

/// The mark of the work, as the tier draws it.
fn mark_of(a: &App) -> String {
    a.theme.glyph(j4n_tui::theme::glyph::Slot::Working).to_string()
}

#[test]
fn the_strip_is_hidden_in_a_new_chat_before_its_first_line_and_the_empty_state_says_no_doer_yet() {
    let mut a = app_with(Json::Null, vec![]);
    a.down(Down::Value { bind: "composer:strip".into(), value: json!({"tags": [], "gauge": null, "route": "no doer yet", "route_by": "none", "work": [], "cost": "", "mode": "write"}), rev: 1 });
    let s = lines(&mut a, 60, 10);
    let at = s.iter().position(|l| l.starts_with('╭')).expect("the frame");
    assert!(s[at + 1].starts_with("│ ") && !s[at + 1].contains("no doer yet") && s[at + 2].starts_with('╰'), "the line sits right under the frame, no strip and no rule: {s:#?}");
    assert!(s[..at].iter().any(|l| l.contains("no doer yet · the first line picks one")), "the chat's empty state says it: {s:#?}");
    // working with no doer is a strip (a mark and its count) and the empty state is gone
    a.down(Down::Value { bind: "composer:strip".into(), value: json!({"tags": [], "gauge": null, "route": "no doer yet", "route_by": "none", "work": [{"label": "starting"}], "cost": "", "mode": "write"}), rev: 2 });
    let s = lines(&mut a, 60, 10);
    let at = s.iter().position(|l| l.starts_with('╭')).expect("the frame");
    assert!(s[at + 1].contains(&format!("{} 1", mark_of(&a))) && s[at + 2].starts_with("│╌"), "{s:#?}");
    assert!(!s.iter().any(|l| l.contains("the first line picks one")), "{s:#?}");
    // the core sends none for the orchestrator (null): nothing is drawn and no state is told
    a.down(Down::Value { bind: "composer:strip".into(), value: Json::Null, rev: 3 });
    let s = lines(&mut a, 60, 10);
    let at = s.iter().position(|l| l.starts_with('╭')).expect("the frame");
    assert!(s[at + 2].starts_with('╰') && !s.iter().any(|l| l.contains("no doer yet")), "{s:#?}");
}

#[test]
fn two_tasks_are_one_mark_with_a_count_and_not_a_bar_each() {
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1")]);
    working(&mut a, json!([{"label": "work", "frac": 0.5}, {"label": "review", "frac": null}]));
    let row = strip_row(&mut a, 100);
    let mark = format!("{} 2", mark_of(&a));
    assert!(row.starts_with(&format!("│ {mark}")), "the mark and the count are the first thing at the left: {row}");
    assert_eq!(row.matches("━━").count(), 1, "only the gauge is a line: {row}");
    assert!(!row.contains("work") && !row.contains("review") && !row.contains("+1"), "no words per task: {row}");
    working(&mut a, json!([{"label": "a"}, {"label": "b"}, {"label": "c"}, {"label": "d"}, {"label": "e"}]));
    assert!(strip_row(&mut a, 100).contains(&format!("{} 5", mark_of(&a))));
    working(&mut a, json!([]));
    assert!(!strip_row(&mut a, 100).contains(&mark_of(&a)), "no mark while idle");
}

#[test]
fn nothing_in_the_strip_moves_and_the_clock_is_not_woken_for_it() {
    for motion in [true, false] {
        let mut a = app_with(Json::Null, vec![turn("t", "go", "1")]);
        look(&mut a, Settings { motion, ..unicode() });
        working(&mut a, json!([{"label": "working", "frac": null}, {"label": "review", "frac": null}]));
        let t0 = Instant::now();
        a.tick(t0);
        let (first, wake) = (Headless::new(100, 12).snap(&mut a).ansi, a.wake(t0));
        a.tick(t0 + Duration::from_millis(500));
        assert_eq!(first, Headless::new(100, 12).snap(&mut a).ansi, "motion {motion}: the strip is the same two ticks on");
        assert_eq!(wake, None, "motion {motion}: no work bar to wake the clock for");
        assert!(!a.breathes, "motion {motion}");
    }
}

fn working_lanes() -> Vec<Row> {
    vec![turn("t", "go", "1"), row("a", "A omp", "working", "t"), row("b", "B claude", "working", "t")]
}

/// The lines of the screen that differ between the first tick and the one after.
fn moving(a: &mut App) -> Vec<String> {
    let t0 = Instant::now();
    a.tick(t0);
    let first = Headless::new(40, 12).snap(a).ansi;
    a.tick(t0 + Duration::from_millis(1000));
    let later = Headless::new(40, 12).snap(a).ansi;
    first.lines().zip(later.lines()).filter(|(x, y)| x != y).map(|(x, _)| x.to_string()).collect()
}

#[test]
fn only_one_lane_row_breathes_the_one_in_focus_else_the_one_aimed_at_else_the_newest() {
    let mut a = app_with(Json::Null, working_lanes());
    let m = moving(&mut a);
    assert!(m.len() == 1 && m[0].contains("B claude"), "the newest lane alone breathes: {m:?}");
    a.aim = Some("a".into());
    let m = moving(&mut a);
    assert!(m.len() == 1 && m[0].contains("A omp"), "the lane `⌃t` aims at: {m:?}");
    a.aim = None;
    // the keys on the first lane (`⌃↑` to the turn, `↓` to its lane)
    a.wins.entry("chat:main".into()).or_default().at = Some("a".into());
    let m = moving(&mut a);
    assert!(m.len() == 1 && m[0].contains("A omp"), "the lane the keys are on: {m:?}");
}

#[test]
fn the_right_group_is_the_cost_then_the_doer_and_model_then_its_gauge_stuck_to_the_edge() {
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1")]);
    for work in [json!([]), json!([{"label": "working"}]), json!([{"label": "a"}, {"label": "b"}, {"label": "c"}, {"label": "d"}])] {
        working(&mut a, work);
        for w in [120u16, 100, 80] {
            let row = strip_row(&mut a, w);
            assert_eq!(row.chars().count(), w as usize, "{w}: {row}");
            assert!(row.ends_with("━━╸─── 42% │"), "{w}: the gauge is the last cell before the border: {row}");
            let (cost, doer, gauge) = (row.find("$0.33 list").unwrap(), row.find("claude · sonnet 5.5").unwrap(), row.find("━━╸").unwrap());
            assert!(cost < doer && doer < gauge, "{w}: cost, doer · model, gauge: {row}");
            assert!(!row.contains("write"), "{w}: the usual mode says nothing: {row}");
        }
    }
    // the mode that is the unusual one is said, first of the group
    a.down(Down::Value { bind: "composer:strip".into(), value: json!({"tags": [], "gauge": 0.42, "route": "claude · sonnet 5.5", "route_by": "core", "work": [], "cost": "$0.33 list", "mode": "read"}), rev: 5 });
    let row = strip_row(&mut a, 100);
    assert!(row.find("read").unwrap() < row.find("$0.33").unwrap(), "{row}");
    // at 60 columns the group goes under the tags, and still ends with the gauge
    working(&mut a, json!([{"label": "a"}]));
    let s = lines(&mut a, 60, 12);
    let at = s.iter().position(|l| l.starts_with('╭')).expect("the frame");
    assert!(s[at + 1].starts_with("│ ") && s[at + 1].contains(&mark_of(&a)) || s[at + 1].contains("claude"), "{s:#?}");
    assert!(s[at + 1].trim_end_matches([' ', '│']).ends_with("42%") || s[at + 2].trim_end_matches([' ', '│']).ends_with("42%"), "{s:#?}");
}

#[test]
fn a_narrow_strip_gives_up_the_cost_then_the_gauge_and_keeps_the_mark_and_the_doer() {
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1")]);
    working(&mut a, json!([{"label": "working", "frac": null}]));
    // the cost, its share, the gauge as a line, the gauge as its number alone (`g11`)
    let said = |row: &str| (row.contains("$0.33 list"), row.contains("0.8% of 5h"), row.contains("━━╸─── 42%"), row.contains("claude · sonnet 5.5 42%"));
    for (w, want) in [(100, (true, true, true, false)), (62, (false, true, true, false)), (52, (false, true, false, true)), (45, (false, false, false, true))] {
        let row = strip_row(&mut a, w);
        assert_eq!(said(&row), want, "{w}: {row}");
    }
    for w in 40..=100u16 {
        let row = strip_row(&mut a, w);
        assert!(row.chars().count() <= w as usize && row.contains(&format!("{} 1", mark_of(&a))) && row.contains("claude · sonnet 5.5"), "{w}: {row}");
    }
}

#[test]
fn the_plan_is_a_thin_line_in_the_strip_that_gives_up_its_line_before_the_cost() {
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1")]);
    let plan = |a: &mut App, done: u64, total: u64| {
        let v = json!({"tags": [], "gauge": 0.42, "route": "claude · sonnet 5.5", "route_by": "core", "work": [{"label": "w"}], "cost": "$0.33 list", "mode": "write", "plan": {"done": done, "total": total}});
        a.down(Down::Value { bind: "composer:strip".into(), value: v, rev: done + 10 });
    };
    for (done, total, line) in [(0, 3, "plan 0/3 ──────"), (2, 3, "plan 2/3 ━━━━──"), (1, 4, "plan 1/4 ━╸────"), (3, 3, "plan 3/3 ━━━━━━")] {
        plan(&mut a, done, total);
        assert!(strip_row(&mut a, 100).contains(line), "{line}: {}", strip_row(&mut a, 100));
    }
    plan(&mut a, 2, 3);
    let row = strip_row(&mut a, 62);
    assert!(row.contains("plan 2/3") && !row.contains('━') && row.contains("$0.33 list") && row.contains(" 42%"), "narrow: the words stay, the line goes before the cost: {row}");
    let row = strip_row(&mut a, 70);
    assert!(row.contains("plan 2/3 ━━━━──") && row.contains(" 42%") && !row.contains("━━╸───"), "a little wider: the gauge gave up its line first, the plan's is still there: {row}");
    assert_eq!(strip_row(&mut a, 100).matches('╸').count(), 1, "the plan is a line and the gauge another, and only the gauge has a half cell");
    // an idle chat with no plan has no plan words
    working(&mut a, json!([]));
    assert!(!strip_row(&mut a, 100).contains("plan"));
}

// ---- a wheel notch that came as three arrows ----

fn sent_lines(a: &mut App, lines: &[&str]) {
    for l in lines {
        typed(l, true).into_iter().for_each(|k| drop(a.key(k)));
    }
}

#[test]
fn three_arrows_within_a_few_milliseconds_are_a_wheel_notch_and_scroll_the_chat_and_one_arrow_still_recalls() {
    let mut a = app_with(Json::Null, long());
    sent_lines(&mut a, &["one", "two"]);
    let t0 = Instant::now();
    // one arrow alone is a press
    a.set_clock(t0);
    hit(&mut a, KeyCode::Up);
    assert_eq!((a.line.buf.as_str(), a.scroll.get("chat:main").copied().unwrap_or(0)), ("two", 0));
    // a second, long after, is another press
    a.set_clock(t0 + Duration::from_millis(40));
    hit(&mut a, KeyCode::Up);
    assert_eq!((a.line.buf.as_str(), a.scroll.get("chat:main").copied().unwrap_or(0)), ("one", 0), "an arrow held down repeats at its own pace");
    // three within a few milliseconds are the terminal's wheel: the draft is as it was, and the chat is three lines up
    let mut b = app_with(Json::Null, long());
    sent_lines(&mut b, &["one", "two"]);
    b.set_clock(t0);
    typing(&mut b, "draft");
    for ms in [0, 2, 4] {
        b.set_clock(t0 + Duration::from_millis(ms));
        assert!(hit(&mut b, KeyCode::Up).is_empty());
    }
    assert_eq!((b.line.buf.as_str(), b.scroll.get("chat:main").copied()), ("draft", Some(3)), "history was not recalled");
    // the other way takes the three lines back, and the draft is still its own
    for ms in [100, 102, 104] {
        b.set_clock(t0 + Duration::from_millis(ms));
        hit(&mut b, KeyCode::Down);
    }
    assert_eq!((b.line.buf.as_str(), b.scroll.get("chat:main").copied().unwrap_or(0)), ("draft", 0));
    // a fourth arrow after a notch is a press of its own
    b.set_clock(t0 + Duration::from_millis(300));
    hit(&mut b, KeyCode::Up);
    assert_eq!(b.line.buf, "two");
}

#[test]
fn with_no_clock_each_arrow_is_a_press() {
    let mut a = app_with(Json::Null, long());
    sent_lines(&mut a, &["one", "two"]);
    for _ in 0..2 {
        hit(&mut a, KeyCode::Up);
    }
    assert_eq!((a.line.buf.as_str(), a.scroll.get("chat:main").copied().unwrap_or(0)), ("one", 0));
}

// ---- a heading in the doer's words, in kitty's text sizing ----

fn big_app(big: bool, sizing: bool) -> App {
    let mut a = app_with(Json::Null, vec![turn("t", "what is open", "12:30"), row("p", "# The plan", "", "t")]);
    a.read = j4n_tui::read::Opts { big_heads: big, caps: j4n_tui::read::Caps { sizing, ..j4n_tui::read::Caps::ALL }, ..Default::default() };
    a
}

#[test]
fn a_heading_with_the_setting_and_the_terminal_leaves_the_cell_of_its_words_and_a_blank_row_under_them() {
    let mut a = big_app(true, true);
    let rows = screen(&mut a, 60, 14);
    let [b] = &a.bigs[..] else { panic!("one heading: {:?}", a.bigs) };
    assert_eq!(b.text, "The plan");
    let at = &rows[usize::from(b.y)];
    assert_eq!(at.chars().skip(usize::from(b.x)).take(8).collect::<String>(), "The plan", "{rows:#?}");
    assert!(rows[usize::from(b.y) + 1].chars().skip(usize::from(b.x)).all(|c| c == ' '), "the row under it is blank: {rows:#?}");
}

#[test]
fn with_either_the_setting_or_the_terminal_missing_nothing_is_asked_for_and_the_screen_is_as_it_was() {
    let mut plain = big_app(false, false);
    let was = screen(&mut plain, 60, 14);
    for (big, sizing) in [(false, true), (true, false)] {
        let mut a = big_app(big, sizing);
        assert_eq!(screen(&mut a, 60, 14), was, "big_heads {big}, sizing {sizing}");
        assert!(a.bigs.is_empty());
    }
}

// ---- a message between chats ----

/// The turn that holds the row of a message, as the core sends it: the row's `data.message` is what the screen reads, its mark is the envelope.
fn mail_rows(head: &str, state: &str, from: &str, to: &str) -> Vec<Row> {
    let mut m = row("m", &format!("{head}  the parser's API changed"), "message", "t");
    m.cells[1] = Cell::new(state, Tone::Dim);
    m.data = Some(json!({ "message": { "id": "m1", "from": from, "to": to, "via": "J4n", "text": "the parser's API changed", "state": "delivered" } }));
    vec![row("t", "fix trailing commas", "", ""), m]
}

#[test]
fn a_message_is_drawn_with_the_envelope_in_each_tier_in_the_sender_the_receiver_and_the_orchestrator() {
    // the sender says `→ c2` and then `delivered`, the receiver `from c1`, the orchestrator `c1 → c2` (the core words them: the row's `data.message` is the same)
    for (head, state, from, to) in [("→ c2", "delivered", "c1", "c2"), ("from c1", "14:02", "c1", "c2"), ("c1 → c2", "delivered", "c1", "c2")] {
        let rows = mail_rows(head, state, from, to);
        assert_eq!(j4n_proto::Message::of(&rows[1]).map(|m| (m.from, m.to, m.via)), Some((from.into(), to.into(), "J4n".into())));
        for (marks, mark) in [("unicode", "◫"), ("ascii", "m"), ("icons", "\u{F01EE}")] {
            let mut a = framed("pipes", marks, rows.clone());
            let s = transcript(&mut a, 70, 10);
            let line = s.iter().find(|l| l.contains(head)).unwrap_or_else(|| panic!("{marks}: no row says {head:?}: {s:#?}"));
            assert!(line.contains(mark) && line.contains("the parser's API changed") && line.trim_end().ends_with(state), "{marks}: {line}");
            let after = line.split_once(mark).map_or("", |(_, rest)| rest);
            assert!(after.trim_start().starts_with(head), "{marks}: the mark, then the words: {line}");
        }
    }
}

// ---- what is shown belongs to what it is about ----

#[test]
fn the_gauge_is_one_circle_slice_when_the_core_says_so_and_stands_beside_the_name_of_its_doer_and_model() {
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1")]);
    let v = |form: &str| json!({"tags": [], "gauge": 0.42, "route": "claude · sonnet 5.5", "route_by": "core", "work": [], "cost": "$0.33 list", "mode": "write", "form": form});
    a.down(Down::Value { bind: "composer:strip".into(), value: v("slice"), rev: 1 });
    // the plain tiers take the rising block: 42% of seven steps is the fourth, and the number is four cells wide so the row does not move
    assert!(strip_row(&mut a, 100).ends_with("claude · sonnet 5.5 ▄ 42% │"), "{}", strip_row(&mut a, 100));
    // the icons tier takes the circle slice of its own: the fourth of nine for 42%
    look(&mut a, Settings::default());
    let row = strip_row(&mut a, 100);
    assert!(row.contains("claude · sonnet 5.5 \u{F0AA0} 42% "), "{row}");
    // the thin line is the other form, and the default
    look(&mut a, unicode());
    a.down(Down::Value { bind: "composer:strip".into(), value: v("line"), rev: 2 });
    assert!(strip_row(&mut a, 100).ends_with("claude · sonnet 5.5 ━━╸─── 42% │"), "{}", strip_row(&mut a, 100));
}

#[test]
fn under_auto_the_strip_takes_the_circle_slice_at_every_width_and_only_the_word_line_keeps_the_thin_line() {
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1")]);
    let v = |form: &str| json!({"tags": [], "gauge": 0.42, "route": "claude · sonnet 5.5", "route_by": "core", "work": [], "cost": "$0.33 list", "mode": "write", "form": form});
    look(&mut a, unicode());
    for (rev, (form, thin)) in [("auto", false), ("slice", false), ("line", true), ("", true)].into_iter().enumerate() {
        a.down(Down::Value { bind: "composer:strip".into(), value: v(form), rev: 10 + rev as u64 });
        for width in [60, 100, 160] {
            let row = strip_row(&mut a, width);
            assert_eq!(row.contains('━') || row.contains('╸'), thin, "{form:?} at {width}: {row}");
            assert!(row.contains("42%"), "{form:?} at {width}: {row}");
        }
    }
}

#[test]
fn the_empty_part_of_a_gauge_is_faint_and_never_the_colour_of_the_pipes_and_the_number_is_dim() {
    use j4n_tui::theme::palette::Role;
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1")]);
    working(&mut a, json!([]));
    let th = a.theme.clone();
    let rgb = |r: Role| {
        let c = th.roles.get(r);
        format!("38;2;{};{};{}", c.0, c.1, c.2)
    };
    let ansi = Headless::new(100, 12).snap(&mut a).ansi;
    let row = ansi.lines().find(|l| l.contains("claude · sonnet 5.5")).expect("the strip's row").to_string();
    // the style a run of cells was drawn in: the escape just before it
    let style = |what: &str| {
        let at = row.find(what).unwrap_or_else(|| panic!("{what} in {row:?}"));
        row[row[..at].rfind('\x1b').unwrap()..at].to_string()
    };
    assert!(style("───").contains(&rgb(Role::Faint)), "the track is faint: {row:?}");
    assert!(!style("───").contains(&rgb(Role::Edge)), "and never edge: {row:?}");
    assert!(style("42%").contains(&rgb(Role::Dim)), "the number is dim: {row:?}");
}

/// A chat's row of the list: its parent and mark, then `(id, title, doer · model, cost, time)`, its window and whether it is old.
fn chat_row(parent: &str, icon: &str, (id, title, who, cost, when): (&str, &str, &str, &str, &str), window: Option<f64>, old: bool) -> Row {
    let mut r = row(id, "", icon, parent);
    r.cells = vec![Cell::with_icon(icon, "", Tone::Plain), Cell::new(id, Tone::Dim), Cell::new(title, Tone::Plain), Cell::new(who, Tone::Dim), Cell::new("", Tone::Dim), Cell::new(cost, Tone::Dim), Cell::new(when, Tone::Dim)];
    r.data = Some(json!({"chat": id, "window": window, "old": old}));
    r
}

fn orchestrator() -> App {
    orchestrator_with(json!([{"doer": "claude", "window": "5h", "used": 0.92, "resets": "14:10"}, {"doer": "claude", "window": "7d", "used": 0.41, "resets": "20:00"}]))
}

fn orchestrator_with(windows: Json) -> App {
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1")]);
    let here = json!({"top": true, "id": "", "crumb": [], "chats": 3, "running": 1, "waits": 0, "state": null, "icon": "", "today": "$1.82", "form": "line", "windows": windows});
    a.down(Down::Value { bind: "chat:here".into(), value: here, rev: 1 });
    let rows = vec![
        chat_row("", "working", ("c3", "fix trailing commas", "claude · sonnet 5.5", "$0.31", "1:04"), Some(0.41), false),
        chat_row("c3", "working", ("c3.1", "parser", "omp · sonnet 5.5", "$0.03", "21s"), Some(0.12), false),
        chat_row("", "done", ("c2", "what does the gate check", "claude · haiku 5.5", "$0.02", "12:20"), Some(0.11), true),
        chat_row("", "done", ("c1", "rename the helper", "omp · sonnet 5.5", "$0.04", "12:10"), None, true),
    ];
    a.down(Down::Rows { bind: "chats:list".into(), rows, total: 4, from: 0, rev: 1 });
    a
}

#[test]
fn the_orchestrator_draws_a_windows_row_and_each_chats_gauge_with_its_doer_and_model_and_folds_the_chats_that_are_done() {
    let mut a = orchestrator();
    let s = lines(&mut a, 110, 24);
    let windows = s.iter().find(|l| l.contains("claude 5h")).expect("the windows row");
    assert!(windows.contains("claude 5h ━━━━━╸ 92% resets 14:10   7d ━━━━ 41%") || windows.contains("claude 5h ━━━━━╸ 92% resets 14:10   7d ━━────"), "{windows}");
    assert!(windows.ends_with("today $1.82"), "what today has cost stands at the right edge: {windows}");
    assert!(!windows.contains("resets 20:00"), "a window far from its limit does not say when it resets: {windows}");
    let c3 = s.iter().find(|l| l.contains("c3 fix trailing")).expect("c3's row");
    assert!(c3.contains("claude · sonnet 5.5  ━━──── 41%  $0.31  1:04"), "doer · model, its own gauge, the cost, the time: {c3}");
    let c31 = s.iter().find(|l| l.contains("c3.1 parser")).expect("the branch's row");
    assert!(c31.contains("omp · sonnet 5.5  ╸───── 12%"), "{c31}");
    assert!(s.iter().all(|l| !l.contains("c2 what does") && !l.contains("c1 rename")), "chats that are done for a while are folded: {s:#?}");
    let fold = s.iter().find(|l| l.contains("done before")).expect("the fold row");
    assert!(fold.contains("2 done before") && fold.trim_end().ends_with('▸'), "{fold}");
}

#[test]
fn the_fold_opens_with_enter_and_shows_the_chats_that_are_done_and_folds_again() {
    let mut a = orchestrator();
    // `⌃↑` to the turn and again into the list, down to the fold row (the last), `⏎`
    for _ in 0..2 {
        a.key(j4n_tui::headless::KeyEvent::new(KeyCode::Up, crossterm_ctrl()));
    }
    for _ in 0..2 {
        hit(&mut a, KeyCode::Down);
    }
    let ups = hit(&mut a, KeyCode::Enter);
    let s = lines(&mut a, 110, 24);
    assert!(ups.is_empty(), "the fold is not a door: {ups:?}\n{s:#?}");
    assert!(s.iter().any(|l| l.contains("c2 what does")) && s.iter().any(|l| l.contains("c1 rename")), "the chats that were done are shown: {s:#?}");
    assert!(s.iter().find(|l| l.contains("done before")).is_some_and(|l| l.trim_end().ends_with('▾')), "{s:#?}");
    // the cursor stayed on the fold row, whose place moved down by the two chats it showed: `⏎` again folds, and does not go into a chat
    let ups = hit(&mut a, KeyCode::Enter);
    assert!(ups.is_empty(), "the second `⏎` is the fold's, not c2's: {ups:?}");
    assert!(lines(&mut a, 110, 24).iter().all(|l| !l.contains("c2 what does")), "and folded again");
    // and once more, from where the cursor is now: it is still on the fold row, inside the shorter list
    assert!(hit(&mut a, KeyCode::Enter).is_empty());
    assert!(lines(&mut a, 110, 24).iter().any(|l| l.contains("c2 what does")), "opened again");
}

#[test]
fn a_plan_is_one_row_with_the_thin_line_and_its_lanes_and_a_chats_end_is_a_door() {
    let mut a = app_with(Json::Null, vec![turn("t", "fix the parser", "12:29")]);
    let mut plan = row("p", "plan 2/3", "round", "t");
    plan.cells.push(Cell::new("12:32", Tone::Dim));
    plan.data = Some(json!({"plan": {"chat": "c3", "done": 2, "total": 3, "lanes": "c3.1 omp ▸ c3.2 claude ▸ c3.3 pi"}}));
    let mut ended = row("e", "c2 the gate runs the laws, clippy and the tests of every crate", "done", "t");
    ended.cells.push(Cell::new("12:20", Tone::Dim));
    ended.data = Some(json!({"door": "c2"}));
    send(&mut a, vec![turn("t", "fix the parser", "12:29"), plan, ended]);
    let s = transcript(&mut a, 110, 20);
    let p = s.iter().find(|l| l.contains("plan 2/3")).expect("the plan row");
    assert!(p.contains("plan 2/3  ━━━━──  c3.1 omp ▸ c3.2 claude ▸ c3.3 pi") && p.ends_with("12:32"), "{p}");
    let e = s.iter().find(|l| l.contains("c2 the gate")).expect("the end row");
    assert!(e.ends_with("12:20 ▸"), "the row of a chat's end is its door: {e}");
    // `⏎` on it goes in
    a.wins.entry("chat:main".into()).or_default().at = Some("e".into());
    let ups = hit(&mut a, KeyCode::Enter);
    assert!(matches!(ups.as_slice(), [Up::Intent { intent: Intent::Enter { chat } }] if chat == "c2"), "{ups:?}");
}

fn crossterm_ctrl() -> ratatui::crossterm::event::KeyModifiers {
    ratatui::crossterm::event::KeyModifiers::CONTROL
}

#[test]
fn a_gauge_goes_by_whole_percents_for_its_colour_and_its_line_as_its_number_does() {
    // 99.6% says 100% and is a full line, 79.97% says 80% and is warm, a negative value is 0% and an empty line
    let at = |v: f64| gauge_line(v);
    let (l, _) = at(0.996);
    assert!(l.contains("━━━━━━ 100%"), "{l}");
    let (l, t) = at(0.7997);
    assert!(l.contains("━━━━╸─ 80%"), "{l}");
    let byte = l.find('━').expect("a full cell");
    assert_eq!(t.chars().nth(l[..byte].chars().count()), Some('y'), "warm: {t}");
    let (l, _) = at(-0.05);
    assert!(l.contains("────── 0%") && !l.contains('-'), "{l}");
}

#[test]
fn the_windows_row_goes_by_whole_percents_too_and_says_no_reset_it_was_not_told() {
    // the same share, the same colour, the same line as the gauge; a reset the core could not make is not printed
    let mut a = orchestrator_with(json!([
        {"doer": "claude", "window": "5h", "used": 0.996, "resets": "14:10"},
        {"doer": "claude", "window": "7d", "used": 0.7997, "resets": "20:00"},
        {"doer": "codex", "window": "5h", "used": -0.05, "resets": ""},
        {"doer": "gemini", "window": "5h", "used": 0.92, "resets": null}
    ]));
    let s = lines(&mut a, 160, 24);
    let row = s.iter().find(|l| l.contains("claude 5h")).expect("the windows row");
    assert!(row.contains("claude 5h ━━━━━━ 100% resets 14:10"), "{row}");
    assert!(row.contains("7d ━━━━╸─ 80% resets 20:00"), "79.97% is 80%, warm, and says when it resets: {row}");
    assert!(row.contains("codex 5h ──────  0%") || row.contains("codex 5h ────── 0%"), "{row}");
    assert!(row.contains("gemini 5h ━━━━━╸ 92%") && !row.contains("92% resets"), "a null reset prints nothing: {row}");
    assert!(!row.contains('-'), "no negative number: {row}");
    let ansi = Headless::new(160, 24).snap(&mut a).ansi;
    let line = ansi.lines().find(|l| l.contains(" 80%")).expect("the row").to_string();
    let warm = a.theme.roles.get(j4n_tui::theme::palette::Role::Warn);
    let at = line.find(" 80%").expect("the number");
    assert!(line[line[..at].rfind('\x1b').unwrap()..at].contains(&format!("38;2;{};{};{}", warm.0, warm.1, warm.2)), "80% is in warn: {line:?}");
}

// ---- ↓ from an empty line into the list of the chats, ← back to the orchestrator ----

fn went(ups: &[Up]) -> Option<&str> {
    ups.iter().find_map(|u| match u {
        Up::Intent { intent: Intent::Enter { chat } } => Some(chat.as_str()),
        _ => None,
    })
}

/// The orchestrator with a line of history to walk: `one` then `two` were sent.
fn with_history() -> App {
    let mut a = orchestrator();
    for line in ["one", "two"] {
        typing(&mut a, line);
        assert!(matches!(hit(&mut a, KeyCode::Enter).as_slice(), [Up::Intent { intent: Intent::Say { .. } }]));
    }
    a
}

#[test]
fn down_on_an_empty_line_puts_the_keys_on_the_first_chat_and_enter_goes_in() {
    let mut a = orchestrator();
    assert!(hit(&mut a, KeyCode::Down).is_empty(), "the key only moves the keys");
    assert_eq!(went(&hit(&mut a, KeyCode::Enter)), Some("c3"), "the first chat of the list is the one selected");
}

#[test]
fn down_walks_the_history_first_and_the_list_has_the_keys_only_past_the_newest_line() {
    let mut a = with_history();
    hit(&mut a, KeyCode::Up);
    hit(&mut a, KeyCode::Up);
    assert_eq!(a.line.buf, "one");
    hit(&mut a, KeyCode::Down);
    assert_eq!(a.line.buf, "two", "↓ is the history's while there is a newer line");
    hit(&mut a, KeyCode::Down);
    assert_eq!(a.line.buf, "", "the end of the history is the empty line");
    hit(&mut a, KeyCode::Down);
    assert_eq!(went(&hit(&mut a, KeyCode::Enter)), Some("c3"), "and the next ↓ is the list's, on its first chat");
}

#[test]
fn up_and_down_move_over_the_list_and_up_on_the_first_chat_hands_the_keys_back() {
    let mut a = orchestrator();
    hit(&mut a, KeyCode::Down);
    hit(&mut a, KeyCode::Down);
    hit(&mut a, KeyCode::Down);
    hit(&mut a, KeyCode::Up);
    assert_eq!(went(&hit(&mut a, KeyCode::Enter)), Some("c3.1"), "down, down, up: the second chat");
    let mut b = orchestrator();
    hit(&mut b, KeyCode::Down);
    hit(&mut b, KeyCode::Up);
    assert_eq!(went(&hit(&mut b, KeyCode::Enter)), None, "↑ on the first chat ended the mode: ⏎ is the line's, and it is empty");
    typing(&mut b, "x");
    assert_eq!(b.line.buf, "x");
}

#[test]
fn tab_goes_in_as_enter_does_and_is_not_the_next_tab() {
    let mut a = orchestrator();
    hit(&mut a, KeyCode::Down);
    hit(&mut a, KeyCode::Down);
    let ups = hit(&mut a, KeyCode::Tab);
    assert_eq!(went(&ups), Some("c3.1"), "{ups:?}");
    assert!(!a.numbers_shown(), "no numbers on the strip");
}

#[test]
fn esc_and_a_typed_key_end_the_mode_and_open_nothing() {
    let mut a = orchestrator();
    hit(&mut a, KeyCode::Down);
    assert!(hit(&mut a, KeyCode::Esc).is_empty());
    assert_eq!(went(&hit(&mut a, KeyCode::Enter)), None, "⏎ is the line's again");
    let mut b = orchestrator();
    hit(&mut b, KeyCode::Down);
    typing(&mut b, "hi");
    assert_eq!(b.line.buf, "hi", "the key went to the line, and so did the next");
    assert!(matches!(hit(&mut b, KeyCode::Enter).as_slice(), [Up::Intent { intent: Intent::Say { text, .. } }] if text == "hi"));
}

#[test]
fn a_few_seconds_with_no_key_end_the_mode_by_the_clock_and_a_key_starts_the_wait_again() {
    let mut a = orchestrator();
    let t0 = Instant::now();
    a.set_clock(t0);
    hit(&mut a, KeyCode::Down);
    assert_eq!(a.wake(t0), Some(Duration::from_secs(4)), "the clock is due in four seconds");
    a.tick(t0 + Duration::from_secs(3));
    hit(&mut a, KeyCode::Down);
    a.tick(t0 + Duration::from_secs(6));
    assert!(!a.take_redraw(), "three seconds after the last key: still on the list");
    a.tick(t0 + Duration::from_millis(7_100));
    assert!(a.take_redraw(), "the mode ended, and the screen is drawn without it");
    assert_eq!(went(&hit(&mut a, KeyCode::Enter)), None, "nothing is opened by the end of the wait");
    assert_eq!(a.wake(t0 + Duration::from_secs(8)), None, "and the clock sleeps");
}

#[test]
fn a_wheel_notch_on_an_empty_line_scrolls_the_chat_and_does_not_take_the_list() {
    let mut a = orchestrator();
    let t0 = Instant::now();
    for ms in [0, 2, 4] {
        a.set_clock(t0 + Duration::from_millis(ms));
        hit(&mut a, KeyCode::Down);
    }
    a.set_clock(t0 + Duration::from_secs(1));
    assert_eq!(went(&hit(&mut a, KeyCode::Enter)), None, "three ↓ within a few ms are one notch of the wheel: the keys stay with the line");
}

fn in_a_chat() -> App {
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1")]);
    let here = json!({"top": false, "id": "c3", "crumb": [{"id": "c3", "title": "fix trailing commas"}], "chats": 3, "running": 1, "waits": 0, "state": "working", "icon": "working", "today": "", "form": "line", "windows": []});
    a.down(Down::Value { bind: "chat:here".into(), value: here, rev: 1 });
    a
}

#[test]
fn left_on_an_empty_line_in_a_chat_goes_back_to_the_orchestrator_and_with_text_it_is_the_cursors() {
    let mut a = in_a_chat();
    let ups = hit(&mut a, KeyCode::Left);
    assert!(matches!(ups.as_slice(), [Up::Intent { intent: Intent::Enter { chat } }] if chat.is_empty()), "{ups:?}");
    let mut b = in_a_chat();
    typing(&mut b, "ab");
    assert!(hit(&mut b, KeyCode::Left).is_empty());
    typing(&mut b, "X");
    assert_eq!(b.line.buf, "aXb", "the cursor moved, and nothing was left");
    // at the top there is nowhere to go back to
    assert!(hit(&mut orchestrator(), KeyCode::Left).is_empty());
}

#[test]
fn a_chat_has_no_list_so_down_there_is_the_historys() {
    let mut a = in_a_chat();
    assert!(hit(&mut a, KeyCode::Down).is_empty());
    assert_eq!(went(&hit(&mut a, KeyCode::Enter)), None);
}

// ---- the archive is the last row of the list, closed; the walk skips it until it is opened ----

fn archived_row(id: &str, title: &str) -> Row {
    let mut r = chat_row("", "done", (id, title, "claude · haiku 5.5", "$0.02", "12:10"), None, false);
    r.data = Some(json!({"chat": id, "window": null, "old": false, "archived": true}));
    r
}

fn with_archive() -> App {
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1")]);
    let here = json!({"top": true, "id": "", "crumb": [], "chats": 2, "running": 1, "waits": 0, "state": null, "icon": "", "today": "", "form": "line", "windows": []});
    a.down(Down::Value { bind: "chat:here".into(), value: here, rev: 1 });
    let rows = vec![
        chat_row("", "working", ("c4", "fix trailing commas", "claude · sonnet 5.5", "$0.31", "1:04"), None, false),
        chat_row("", "done", ("c3", "what does the gate check", "claude · haiku 5.5", "$0.02", "12:20"), None, false),
        archived_row("c2", "rename the helper"),
        archived_row("c1", "write the notes"),
    ];
    a.down(Down::Rows { bind: "chats:list".into(), rows, total: 4, from: 0, rev: 1 });
    a
}

fn said(ups: &[Up]) -> Option<&str> {
    ups.iter().find_map(|u| match u {
        Up::Intent { intent: Intent::Say { text, .. } } => Some(text.as_str()),
        _ => None,
    })
}

#[test]
fn the_archive_is_one_closed_row_at_the_end_of_the_list_and_the_walk_skips_the_chats_in_it() {
    let mut a = with_archive();
    let s = lines(&mut a, 110, 24);
    assert!(s.iter().all(|l| !l.contains("c2 rename") && !l.contains("c1 write")), "archived chats are not drawn: {s:#?}");
    let fold = s.iter().find(|l| l.contains("archived (2)")).expect("the archive row");
    assert!(fold.trim_end().ends_with('▸'), "{fold}");
    let at = |k: &str| s.iter().position(|l| l.contains(k)).unwrap_or(usize::MAX);
    assert!(at("c3 what does") < at("archived (2)"), "after the chats: {s:#?}");
    // ↓ walks c4, c3 and then the archive row: the chats in it are not on the way
    for _ in 0..3 {
        hit(&mut a, KeyCode::Down);
    }
    assert!(lines(&mut a, 110, 24).iter().any(|l| l.contains("archived (2)") && l.contains('▸')));
    assert!(hit(&mut a, KeyCode::Down).is_empty(), "past the archive row the keys go back to the line");
    assert_eq!(went(&hit(&mut a, KeyCode::Enter)), None);
}

#[test]
fn enter_on_the_archive_row_opens_it_and_shows_the_chats_and_enter_again_closes_it() {
    let mut a = with_archive();
    for _ in 0..3 {
        hit(&mut a, KeyCode::Down);
    }
    let ups = hit(&mut a, KeyCode::Enter);
    let s = lines(&mut a, 110, 24);
    assert!(ups.is_empty(), "the archive row is not a door: {ups:?}");
    assert!(s.iter().any(|l| l.contains("c2 rename")) && s.iter().any(|l| l.contains("c1 write")), "{s:#?}");
    assert!(s.iter().find(|l| l.contains("archived (2)")).is_some_and(|l| l.trim_end().ends_with('▾')), "{s:#?}");
    // the cursor stayed on the row; ↓ walks into the chats that hang under it now, and ⏎ goes into one
    hit(&mut a, KeyCode::Down);
    assert_eq!(went(&hit(&mut a, KeyCode::Enter)), Some("c2"));
    let mut b = with_archive();
    for _ in 0..3 {
        hit(&mut b, KeyCode::Down);
    }
    hit(&mut b, KeyCode::Enter);
    hit(&mut b, KeyCode::Enter);
    assert!(lines(&mut b, 110, 24).iter().all(|l| !l.contains("c2 rename")), "folded again");
}

#[test]
fn the_key_a_archives_the_chat_the_cursor_is_on_or_takes_an_archived_one_out_and_anywhere_else_it_is_typed() {
    let mut a = with_archive();
    hit(&mut a, KeyCode::Down);
    hit(&mut a, KeyCode::Down);
    assert_eq!(said(&hit(&mut a, KeyCode::Char('a'))), Some(":archive c3"), "the keys stay on the list, and the core is told in the words of the command");
    hit(&mut a, KeyCode::Down);
    hit(&mut a, KeyCode::Enter);
    hit(&mut a, KeyCode::Down);
    assert_eq!(said(&hit(&mut a, KeyCode::Char('a'))), Some(":unarchive c2"), "on an archived row it is the way back");
    // on the archive row, and with the keys on the line, it is a letter
    let mut b = with_archive();
    for _ in 0..3 {
        hit(&mut b, KeyCode::Down);
    }
    assert!(hit(&mut b, KeyCode::Char('a')).is_empty());
    assert_eq!(b.line.buf, "a", "the row is not a chat: the key ended the mode and was typed");
}

// ---- the answer is where the operator looks ----

/// A lane the core marks (`data.lane`): whose doer it is, whether that is the thread's own, and whether it runs code and no model.
fn lane(key: &str, what: &str, icon: &str, doer: &str, own: bool, code: bool) -> Row {
    let mut r = row(key, what, icon, "t");
    r.data = Some(if code { json!({"lane": {"doer": doer, "own": own, "code": true}}) } else { json!({"lane": {"doer": doer, "own": own}}) });
    r
}

/// The words of a model's turn, a step of its lane.
fn says(key: &str, text: &str, parent: &str) -> Row {
    let mut r = row(key, text, "", parent);
    r.by = By::Doer;
    r
}

#[test]
fn a_check_lane_that_starts_after_a_work_lane_does_not_fold_it_and_its_answer_stays_on_the_screen() {
    let rows = vec![
        turn("t", "fix the login page", "12:00"),
        lane("a", "A work", "done", "claude", true, false),
        row("a1", "main.rs", "read", "a"),
        says("a2", "**Nothing is changed.** The login page works again.", "a"),
        lane("b", "B check", "done", "check", false, true),
        row("b1", "pytest", "shell", "b"),
    ];
    let mut a = app_with(Json::Null, rows);
    let s = transcript(&mut a, 70, 20);
    assert!(s.iter().any(|l| l.contains("≡ main.rs")), "the work lane keeps its steps: a lane of code is not one of its kind: {s:#?}");
    assert!(s.iter().any(|l| l.contains("Nothing is changed. The login page works again.")), "the answer, with its marks read: {s:#?}");
    assert!(!s.iter().any(|l| l.contains("**")), "{s:#?}");
}

#[test]
fn a_lane_that_folds_shows_its_answer_under_its_head_six_lines_and_the_count_of_the_rest() {
    let items: String = (1..=9).map(|n| format!("- item {n}\n")).collect();
    let rows = vec![
        turn("t", "go", "1"),
        lane("a", "A omp", "done", "omp", true, false),
        row("a1", "x", "read", "a"),
        says("a2", &items, "a"),
        lane("b", "B omp", "working", "omp", true, false),
        row("b1", "y", "read", "b"),
    ];
    let mut a = app_with(Json::Null, rows);
    let s = transcript(&mut a, 60, 30);
    assert!(!s.iter().any(|l| l.contains("≡ x")), "the older lane of its own kind folds: {s:#?}");
    for n in 1..=6 {
        assert!(s.iter().any(|l| l.contains(&format!("item {n}"))), "item {n}: {s:#?}");
    }
    assert!(!s.iter().any(|l| l.contains("item 7")), "{s:#?}");
    assert!(s.iter().any(|l| l.contains("item 6") && l.ends_with("+3 ▸")), "the rest is counted at the end of the last line: {s:#?}");
}

/// A chat's end as the core sends it: its own row, the answer one line to a cell, the word of its check before the time.
fn ended(key: &str, head: &str, answer: &[&str], verdict: Option<&str>) -> Row {
    let mut r = row(key, head, "done", "");
    r.by = By::Core;
    r.cells.extend(verdict.map(|v| Cell::new(v, Tone::Warn)));
    r.cells.push(Cell::new("12:10", Tone::Dim));
    r.detail = answer.iter().map(|l| Cell::new(*l, Tone::Plain)).collect();
    r.data = Some(json!({"door": "c1", "read": true}));
    r
}

#[test]
fn a_chats_end_is_a_preview_of_its_answer_read_as_markdown_cut_at_eight_lines_and_space_opens_it() {
    let mut text = vec!["**Nothing is changed.** I read `state` only."];
    let bullets: Vec<String> = (1..=11).map(|n| format!("- step {n}")).collect();
    text.extend(bullets.iter().map(String::as_str));
    let mut a = app_with(Json::Null, vec![turn("t", "/usage", "12:05"), ended("e", "c1 · Fix the login page", &text, Some("not run"))]);
    let s = transcript(&mut a, 64, 30);
    let at = s.iter().position(|l| l.contains("c1 · Fix the login page")).expect("the head");
    assert!(s[at].contains("not run") && s[at].contains("12:10"), "the verdict is a word on the row of the turn it checked: {s:#?}");
    assert!(!s[at].starts_with(['├', '╰', '│']) && !s[at - 1].starts_with(['├', '╰', '│']) && !s[at - 2].starts_with(['├', '╰', '│']), "no pipe joins it to the line typed last: {s:#?}");
    assert!(s[at + 1].contains("Nothing is changed. I read state only."), "markdown read, not shown with its marks: {s:#?}");
    assert!(!s.iter().any(|l| l.contains("**") || l.contains('`')), "{s:#?}");
    // eight rendered lines: the sentence, the blank line of the paragraph break, six bullets
    assert!(s.iter().any(|l| l.contains("step 6") && l.ends_with("+5 ▸")), "eight lines of it, and what is left out counted: {s:#?}");
    assert!(!s.iter().any(|l| l.contains("step 7")), "{s:#?}");
    hit_chord(&mut a, &["ctrl+up", "space"]);
    let s = transcript(&mut a, 64, 30);
    assert!(s.iter().any(|l| l.contains("step 11")) && !s.iter().any(|l| l.contains("+5 ▸")), "space opens all of it in place: {s:#?}");
}

#[test]
fn two_returns_of_one_chat_are_two_previews() {
    let mut a = app_with(Json::Null, vec![ended("e1", "c1 · Fix", &["first answer"], None), ended("e2", "c1 · Fix", &["second answer", "and more"], Some("passed"))]);
    let s = transcript(&mut a, 60, 20);
    assert_eq!(s.iter().filter(|l| l.contains("c1 · Fix")).count(), 2, "{s:#?}");
    assert!(s.iter().any(|l| l.contains("first answer")) && s.iter().any(|l| l.contains("and more")), "{s:#?}");
}

// ---- the list of the chats is one tree with one end, on the base of `display::layout` ----

fn archived_of(mut r: Row) -> Row {
    r.data = Some(json!({"chat": r.key, "window": null, "old": false, "archived": true}));
    r
}

/// Two live chats (the first with a branch), two done before (the first with a branch), and an archived chat with a branch.
fn forest(frame: &str) -> App {
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1")]);
    look(&mut a, Settings { marks: "unicode".into(), frame: frame.into(), you: "mark".into(), ..Settings::default() });
    let here = json!({"top": true, "id": "", "crumb": [], "chats": 7, "running": 2, "waits": 0, "state": null, "icon": "", "today": "", "form": "line", "windows": []});
    a.down(Down::Value { bind: "chat:here".into(), value: here, rev: 1 });
    let c = |parent: &str, id: &str, old: bool| chat_row(parent, "done", (id, "words", "claude · haiku 5.5", "$0.02", "12:10"), None, old);
    let rows = vec![
        c("", "c7", false),
        c("c7", "c7.1", false),
        c("", "c6", false),
        c("", "c5", true),
        c("c5", "c5.1", true),
        c("", "c4", true),
        archived_of(c("", "c2", false)),
        archived_of(c("c2", "c2.1", false)),
    ];
    a.down(Down::Rows { bind: "chats:list".into(), rows, total: 8, from: 0, rev: 1 });
    a
}

/// What stands before the marks of each row of the list, as the pipes: the leading pipe characters and blanks after the first blank, and the row's word.
fn pipes_of(a: &mut App) -> Vec<(String, String)> {
    let ids = ["c7.1", "c7", "c6", "c5.1", "c5", "c4", "c2.1", "c2"];
    lines(a, 110, 24)
        .into_iter()
        .filter_map(|l| {
            let word = if l.contains("done before") {
                "done"
            } else if l.contains("archived (") {
                "archived"
            } else {
                ids.iter().find(|i| l.split_whitespace().any(|w| w.trim_start_matches(|c: char| !c.is_ascii()) == **i))?
            };
            let pipes: String = l.chars().skip(1).take_while(|c| "├╰└╭│┆─·› ".contains(*c)).collect();
            Some((pipes.trim_end().to_string(), word.to_string()))
        })
        .collect()
}

fn shape(a: &mut App) -> Vec<String> {
    pipes_of(a).into_iter().map(|(p, w)| if p.is_empty() { w } else { format!("{p} {w}") }).collect()
}

#[test]
fn the_closed_list_has_one_elbow_and_the_branch_bar_stops_before_the_folds() {
    let mut a = forest("pipes");
    assert_eq!(shape(&mut a), ["├─ c7", "│ └─ c7.1", "├─ c6", "├─ done", "└─ archived"], "{:#?}", lines(&mut a, 110, 24));
}

#[test]
fn an_open_fold_holds_its_chats_one_level_in_and_the_end_is_one_elbow() {
    let mut a = forest("pipes");
    // the cursor goes to the fold row `done before` (after c7, c7.1 is a branch; c6; then the fold) and `⏎` opens it
    a.key(j4n_tui::headless::KeyEvent::new(KeyCode::Up, crossterm_ctrl()));
    a.key(j4n_tui::headless::KeyEvent::new(KeyCode::Up, crossterm_ctrl()));
    for _ in 0..3 {
        hit(&mut a, KeyCode::Down);
    }
    hit(&mut a, KeyCode::Enter);
    assert_eq!(shape(&mut a), ["├─ c7", "│ └─ c7.1", "├─ c6", "├─ done", "│ ├─ c5", "│ │ └─ c5.1", "│ └─ c4", "└─ archived"], "{:#?}", lines(&mut a, 110, 24));
    // the archive too: it is the end, so what hangs from it has no bar
    for _ in 0..4 {
        hit(&mut a, KeyCode::Down);
    }
    hit(&mut a, KeyCode::Enter);
    let s = shape(&mut a);
    assert_eq!(s.iter().filter(|l| l.starts_with('└')).count(), 1, "exactly one tree end: {s:#?}");
    assert_eq!(&s[s.len() - 3..], ["└─ archived", "  └─ c2", "    └─ c2.1"], "{s:#?}");
}

#[test]
fn the_list_draws_the_look_of_theme_frame() {
    let mut hang = forest("hang");
    assert_eq!(shape(&mut hang), ["c7", "╰ c7.1", "c6", "done", "archived"], "the hang: no tree at the top, a rail where one nests: {:#?}", lines(&mut hang, 110, 24));
    let mut gutter = forest("gutter");
    assert_eq!(shape(&mut gutter), ["c7", "c7.1", "c6", "done", "archived"], "{:#?}", lines(&mut gutter, 110, 24));
    let rows = lines(&mut gutter, 110, 24);
    let at = |k: &str| rows.iter().find(|l| l.contains(k)).and_then(|l| l.find(k)).expect(k);
    assert!(at("c7.1") > at("c7 ") + 1, "the branch is indented: {rows:#?}");
}

#[test]
fn the_list_draws_round_and_airy_and_the_round_list_opens_with_its_corner() {
    let mut round = forest("round");
    assert_eq!(shape(&mut round), ["╭─ c7", "│ ╰─ c7.1", "├─ c6", "├─ done", "╰─ archived"], "the first row of a list is the corner that opens it; a branch is not: {:#?}", lines(&mut round, 110, 24));
    let mut airy = forest("airy");
    assert_eq!(shape(&mut airy), ["· c7", "   › c7.1", "· c6", "· done", "· archived"], "a dot at the top, an arrow for what nests, no line: {:#?}", lines(&mut airy, 110, 24));
}

// ---- the transcript on the base ----

/// A turn with two lanes that work, the first with two steps, the second with a step that has a message under it.
fn two_lanes() -> Vec<Row> {
    vec![
        turn("t", "fix the parser", "12:29"),
        with(row("a", "A omp", "working", "t"), "sonnet"),
        row("a1", "x.rs", "read", "a"),
        row("a2", "trailing", "search", "a"),
        with(row("b", "B claude", "working", "t"), "sonnet"),
        row("b1", "a.rs", "read", "b"),
        row("b2", "Trailing commas now parse.", "", "b"),
    ]
}

fn shape_of(a: &mut App) -> Vec<String> {
    transcript(a, 60, 20)
}

#[test]
fn the_transcript_draws_round_the_way_it_draws_pipes_because_the_head_above_it_opens_the_tree() {
    let mut pipes = framed("pipes", "unicode", two_lanes());
    let mut round = framed("round", "unicode", two_lanes());
    let (p, r) = (shape_of(&mut pipes), shape_of(&mut round));
    // the two differ only in the corner of an end: `└─` square for pipes, `╰─` rounded for round
    assert_eq!(p, r.iter().map(|l| l.replace('╰', "└")).collect::<Vec<_>>(), "a tree that hangs from a line has no corner of its own to open it: {r:#?}");
    assert!(r[1].starts_with("├─ ▷ A omp") && r[4].starts_with("╰─ ▷ B claude"), "{r:#?}");
    assert!(p[4].starts_with("└─ ▷ B claude"), "the pipes end square: {p:#?}");
}

#[test]
fn the_transcript_draws_airy_with_no_line_a_dot_for_a_lane_and_an_arrow_for_what_nests() {
    let mut a = framed("airy", "unicode", two_lanes());
    let s = shape_of(&mut a);
    assert_eq!(s[0].split_whitespace().next(), Some("fix"), "{s:#?}");
    assert!(s[1].starts_with("· ▷ A omp sonnet"), "a lane: the dot, the mark, the words: {s:#?}");
    assert!(s[2].starts_with("   › ≡ x.rs") && s[3].starts_with("   › ⊙ trailing"), "a step: two blanks of the column and the arrow, one cell right of the words of the lane: {s:#?}");
    assert!(s[4].starts_with("· ▷ B claude") && s[5].starts_with("   › "), "{s:#?}");
    assert!(s.iter().all(|l| !l.contains('│') && !l.contains('├') && !l.contains('╰')), "no line: {s:#?}");
}

#[test]
fn the_transcript_of_the_hang_and_the_pipes_is_what_it_was_before_the_base() {
    // the same words as before the move; the base (`display/layout`) only says where each piece goes
    let mut a = framed("pipes", "unicode", two_lanes());
    let s = shape_of(&mut a);
    assert_eq!(s[1..6].iter().map(|l| l.chars().take(5).collect::<String>()).collect::<Vec<_>>(), ["├─ ▷ ", "│  ├─", "│  └─", "└─ ▷ ", "   ├─"], "a bar is three cells and runs while a lane follows it: {s:#?}");
    let mut h = framed("hang", "unicode", two_lanes());
    let s = shape_of(&mut h);
    assert!(s[2].starts_with("┆ ") && s[3].starts_with("╰ "), "the hang's rail and its end: {s:#?}");
}

// ---- the bracket head ----

fn bracket(marks: &str) -> Settings {
    Settings { marks: marks.into(), ..Settings::default() }
}

#[test]
fn your_line_is_between_two_caps_with_the_time_inside_and_its_tree_hangs_from_the_words() {
    let mut a = app_with(Json::Null, one_lane());
    look(&mut a, bracket("unicode"));
    let all = lines(&mut a, 80, 24);
    let at = all.iter().position(|l| l.contains("fix the parser")).expect("your line");
    assert_eq!(all[at], flush("▌ fix the parser", "12:29 ▐", 80), "caps at both ends, one blank inside each, the words plain: {all:#?}");
    assert!(all[at - 1].is_empty(), "a blank row above the first line: {all:#?}");
    assert!(all[at + 1].starts_with("  ├─ "), "the first pipe of the lanes hangs from the words, two cells in: {all:#?}");
    assert_eq!(all.iter().filter(|l| l.contains('▌') || l.contains('▐')).count(), 1, "no other row of the chat uses the caps: {all:#?}");
}

#[test]
fn a_line_of_several_rows_has_the_caps_on_every_row_and_every_row_is_as_wide_as_the_first() {
    let mut a = app_with(Json::Null, vec![turn("t", "fix the retry in the claude doer and add a test for the second case, the one that dropped the first answer", "12:04"), row("l", "A work", "working", "t")]);
    look(&mut a, bracket("unicode"));
    let all = lines(&mut a, 60, 12);
    let head: Vec<&String> = all.iter().filter(|l| l.starts_with('▌')).collect();
    assert!(head.len() >= 2, "the line wraps: {all:#?}");
    for l in &head {
        assert!(l.ends_with('▐') && l.chars().count() == 60, "every row ends in the cap at the last cell: {l:?}");
        assert!(l.starts_with("▌ ") && l.ends_with(" ▐"), "the same blank inside each cap: {l:?}");
    }
    assert!(head[0].contains("12:04 ▐") && !head[1].contains("12:04"), "the time is on the first row only: {head:#?}");
    let at = all.iter().position(|l| l == head[head.len() - 1]).expect("the last row of the line");
    assert!(all[at + 1].starts_with("  ╰─ ") || all[at + 1].starts_with("  ├─ "), "the tree hangs from the words: {all:#?}");
}

#[test]
fn without_colour_the_caps_are_the_only_mark_and_the_line_reads() {
    let mut a = app_with(Json::Null, one_lane());
    look(&mut a, Settings { no_color: true, ..bracket("unicode") });
    let ansi = Headless::new(80, 24).snap(&mut a).ansi;
    let line = ansi.lines().find(|l| l.contains("fix the parser")).expect("your line");
    assert!(!line.contains("38;2") && !line.contains("48;2"), "no colour on the line: {line:?}");
    let plain = lines(&mut a, 80, 24);
    assert!(plain.iter().any(|l| l.starts_with("▌ fix the parser") && l.ends_with("12:29 ▐")), "{plain:#?}");
    // the ascii tier has its own caps
    let mut b = app_with(Json::Null, one_lane());
    look(&mut b, bracket("ascii"));
    let plain = lines(&mut b, 80, 24);
    assert!(plain.iter().any(|l| l.starts_with("[ fix the parser") && l.ends_with("12:29 ]")), "{plain:#?}");
}

#[test]
fn band_and_mark_stay_as_choices_and_the_gutter_look_keeps_its_band() {
    let mut a = app_with(Json::Null, one_lane());
    look(&mut a, Settings { you: "band".into(), ..bracket("unicode") });
    let all = lines(&mut a, 80, 24);
    assert!(!all.iter().any(|l| l.contains('▌')), "the band has no caps: {all:#?}");
    let mut g = app_with(Json::Null, one_lane());
    look(&mut g, Settings { frame: "gutter".into(), ..bracket("unicode") });
    let all = lines(&mut g, 80, 24);
    assert!(!all.iter().any(|l| l.contains('▌')), "the gutter look has a word column, not a bracket: {all:#?}");
}

// ---- the tree under the bracket ----

/// A turn of each kind: lanes with steps and an answer, a lone answer, a wrapped line with a lone lane.
fn kinds() -> Vec<Row> {
    let mut rows = one_lane();
    rows.extend([
        turn("u", "thanks", "12:46"),
        { let mut r = row("ua", "You are welcome.", "", "u"); r.by = By::Doer; r },
        turn("w", "fix the retry in the claude doer and add a test for the second case, the one that dropped the first answer", "12:58"),
        row("wl", "A work", "working", "w"),
        row("wl0", "retry.rs", "edit", "wl"),
    ]);
    rows
}

/// The glyphs a tree is drawn from, in every look and tier: a row that starts with one stands on the tree's first column.
const TREE_AT_THE_EDGE: [char; 14] = ['├', '╰', '└', '│', '┆', '╭', '·', '›', '|', '`', '\\', '+', '.', ':'];

#[test]
fn no_row_under_a_bracket_starts_a_pipe_at_the_caps_column() {
    for frame in ["round", "pipes", "airy", "hang"] {
        for marks in ["unicode", "ascii", "icons"] {
            for cols in [60u16, 80, 120] {
                let mut a = app_with(Json::Null, kinds());
                look(&mut a, Settings { marks: marks.into(), frame: frame.into(), ..Settings::default() });
                let all = lines(&mut a, cols, 40);
                let heads: Vec<usize> = (0..all.len()).filter(|i| all[*i].starts_with('▌') || all[*i].starts_with("[ ")).collect();
                assert!(heads.len() >= 3, "{frame}/{marks}/{cols}: the three turns have brackets: {all:#?}");
                for i in heads {
                    let under = &all[i + 1];
                    if under.is_empty() || under.starts_with('▌') || under.starts_with("[ ") {
                        continue;
                    }
                    let first = under.chars().next().unwrap_or(' ');
                    assert!(!TREE_AT_THE_EDGE.contains(&first), "{frame}/{marks}/{cols}: a row under the bracket starts at the cap's column with {first:?}: {all:#?}");
                    assert!(under.starts_with("  "), "{frame}/{marks}/{cols}: the tree hangs from the words: {all:#?}");
                }
            }
        }
    }
}

#[test]
fn the_whole_tree_hangs_two_cells_in_and_keeps_its_shape() {
    let mut a = app_with(Json::Null, one_lane());
    look(&mut a, bracket("unicode"));
    let all = lines(&mut a, 60, 20);
    let at = all.iter().position(|l| l.starts_with("▌ fix the parser")).expect("your line");
    let tree: Vec<&str> = all[at + 1..].iter().map(|l| l.as_str()).take_while(|l| !l.is_empty() && !l.starts_with('╭')).collect();
    assert!(tree.len() >= 5 && tree.iter().all(|l| l.starts_with("  ")), "every row of the tree is two cells in: {tree:#?}");
    assert!(tree[0].starts_with("  ├─ ✓ A omp") && tree[1].starts_with("  ╰─ ▷ B claude"), "{tree:#?}");
    assert!(tree[2].starts_with("     ├─ ≡ a.rs"), "a step stands a level in under its lane: {tree:#?}");
    assert!(tree.iter().all(|l| l.chars().count() <= 59), "a row still ends one cell before the edge: {tree:#?}");
}

#[test]
fn the_band_the_mark_and_a_branch_chats_first_row_keep_their_tree_at_the_left_edge() {
    for you in ["band", "mark"] {
        let mut a = app_with(Json::Null, one_lane());
        look(&mut a, Settings { you: you.into(), ..bracket("unicode") });
        let all = lines(&mut a, 80, 24);
        let at = all.iter().position(|l| l.contains("fix the parser")).expect("your line");
        assert!(all[at + 1].starts_with("├─ "), "{you}: no cap, so the tree starts at its own column: {all:#?}");
    }
    let mut from = turn("t", "from main: the parser work", "13:02");
    from.data = Some(json!({ "from": "main" }));
    let mut a = app_with(Json::Null, vec![from, row("l", "A work", "working", "t")]);
    look(&mut a, bracket("unicode"));
    let all = lines(&mut a, 80, 20);
    let at = all.iter().position(|l| l.contains("from main")).expect("the first row");
    assert!(!all[at].starts_with('▌') && all[at + 1].starts_with("╰─ "), "a branch chat's first row has no cap and its tree at the edge: {all:#?}");
}

// ---- the group view: side lanes ----

/// A lane headed by its role, as the core sends it in the group view: `main` for the thread's own work.
fn role(mut r: Row, role: &str, main: bool) -> Row {
    r.data = Some(json!({ "lane": { "doer": "claude", "own": main, "role": role, "main": main } }));
    r
}

fn sides() -> Vec<Row> {
    vec![
        turn("t", "ship it", "02:23"),
        role(with(row("m", "Main", "working", "t"), "· claude sonnet 5.5"), "Main", true),
        row("m1", "TESTING.md", "read", "m"),
        row("m2", "git status --short", "run", "m"),
        role(stat(with(row("r", "review", "working", "t"), "· omp glm 5.3"), "", "of 02:09"), "review", false),
        row("r1", "json_out", "search", "r"),
        row("r2", "reading the report tests", "read", "r"),
    ]
}

#[test]
fn the_roles_that_run_at_once_are_side_lanes_one_open_and_the_others_folded_to_their_last_step() {
    let mut a = app_with(Json::Null, sides());
    let t = transcript(&mut a, 80, 20).join("\n");
    // Main's lane is open, the review folds to its head and the step it is on
    assert!(t.contains("TESTING.md") && t.contains("git status --short"), "{t}");
    assert!(t.contains("reading the report tests") && !t.contains("json_out"), "{t}");
    let review = t.lines().find(|l| l.contains("review · omp glm 5.3")).unwrap_or_default();
    assert!(review.contains("of 02:09") && review.ends_with('▸'), "{review}\n{t}");
    // opening the review folds Main: one is open
    a.wins.get_mut("chat:main").expect("the chat's window").flip.insert("r".into());
    let t = transcript(&mut a, 80, 20).join("\n");
    assert!(t.contains("json_out") && t.contains("reading the report tests"), "{t}");
    assert!(!t.contains("TESTING.md") && t.lines().find(|l| l.contains("Main · claude")).is_some_and(|l| l.ends_with('▸')), "{t}");
    // a line you sent that the core has not shown yet leaves the side lanes as they are
    a.wins.get_mut("chat:main").expect("the chat's window").flip.clear();
    typing(&mut a, "and the changelog");
    hit(&mut a, KeyCode::Enter);
    let t = transcript(&mut a, 80, 24).join("\n");
    assert!(t.contains("and the changelog") && t.contains("TESTING.md") && !t.contains("json_out"), "{t}");
    // the review ends while Main works: it keeps its fold, and shows its last words in place of a step
    let mut ended = sides();
    ended[4].cells[0].icon = "done".into();
    ended.push(row("r3", "Two tests fail without the flag.", "", "r"));
    let t = transcript(&mut app_with(Json::Null, ended.clone()), 80, 24).join("\n");
    assert!(t.lines().find(|l| l.contains("review · omp")).is_some_and(|l| l.ends_with('▸')) && !t.contains("json_out") && t.contains("TESTING.md"), "{t}");
    // and a review you opened stays open when it ends
    let mut b = app_with(Json::Null, sides());
    transcript(&mut b, 80, 24);
    b.wins.get_mut("chat:main").expect("the chat's window").flip.insert("r".into());
    send(&mut b, ended);
    let t = transcript(&mut b, 80, 24).join("\n");
    assert!(t.contains("json_out") && t.contains("Two tests fail"), "{t}");
    // a role that works alone under the line is open, as any lane
    let alone: Vec<Row> = sides().into_iter().filter(|r| !r.key.starts_with('r')).collect();
    let t = transcript(&mut app_with(Json::Null, alone), 80, 20).join("\n");
    assert!(t.contains("TESTING.md") && t.contains("Main · claude sonnet 5.5"), "{t}");
}

// ---- the suggestions in the strip's left ----

/// A palette of four commands: `:s` starts each, so the shorter go first, then as listed.
fn palette(a: &mut App) {
    let rows: Vec<Row> = ["switch", "status", "stop", "setup"].iter().map(|w| Row { key: (*w).into(), cells: vec![Cell::new(format!(":{w}"), Tone::Accent)], ..Row::default() }).collect();
    a.down(Down::Rows { bind: j4n_proto::PALETTE.into(), total: rows.len() as u64, rows, from: 0, rev: 1 });
}

/// The composer's rows, from its top edge to its bottom edge.
fn composer(a: &mut App, cols: u16, rows: u16) -> Vec<String> {
    let s = lines(a, cols, rows);
    let top = s.iter().rposition(|l| l.starts_with('╭')).expect("a composer");
    let end = s.iter().rposition(|l| l.starts_with('╰')).expect("its bottom edge");
    s[top..=end].to_vec()
}

#[test]
fn the_list_takes_the_strips_left_and_the_right_group_stays() {
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1")]);
    a.down(Down::Value { bind: "composer:strip".into(), value: strip(), rev: 1 });
    palette(&mut a);
    typing(&mut a, ":s");
    let c = composer(&mut a, 100, 12);
    assert_eq!(c[1], flush("│ ⇥ :stop  :setup  :switch  :status", "~➜ claude ━━──── 41% │", 100), "{c:#?}");
    assert!(!c.join("\n").contains("parser.rs"), "the strip's left gives way: {c:#?}");
    // under 80 columns the list takes the first row whole and the right group keeps the second
    let c = composer(&mut a, 60, 12);
    assert!(c[1].starts_with("│ ⇥ :stop  :setup  :switch  :status") && !c[1].contains('➜'), "{c:#?}");
    assert!(c[2].contains("~➜ claude"), "{c:#?}");
    // under 50, the one in the line and its place
    let c = composer(&mut a, 44, 12);
    assert!(c[1].starts_with("│ ⇥ :stop 1/4 "), "{c:#?}");
    // the line emptied: the strip's own left comes back
    a.key(ctrl('u'));
    let c = composer(&mut a, 100, 12);
    assert!(c[1].contains("parser.rs") && !c[1].contains('⇥'), "{c:#?}");
}

#[test]
fn the_composer_keeps_its_height_while_typing_once_the_list_came() {
    // no strip, as in the orchestrator: the frame and the line
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1")]);
    palette(&mut a);
    assert_eq!(composer(&mut a, 80, 12).len(), 3);
    typing(&mut a, ":s");
    let c = composer(&mut a, 80, 12);
    assert_eq!(c.len(), 5, "the list's row and its rule appear: {c:#?}");
    assert!(c[1].starts_with("│ ⇥ :stop  :setup  :switch  :status") && c[2].starts_with("│╌"), "{c:#?}");
    for more in ["x", " and more words"] {
        typing(&mut a, more);
        let c = composer(&mut a, 80, 12);
        assert_eq!(c.len(), 5, "it stays while the line is not empty: {c:#?}");
        assert!(c[1].starts_with("│ ⇥ ") && c[1]["│ ⇥ ".len()..].trim_end_matches('│').trim().is_empty(), "with nothing to offer, its row is empty: {c:#?}");
    }
    hit(&mut a, KeyCode::Enter);
    assert_eq!(composer(&mut a, 80, 12).len(), 3, "the line sent, the row goes");
    typing(&mut a, ":s");
    assert_eq!(composer(&mut a, 80, 12).len(), 5);
    hit(&mut a, KeyCode::Esc);
    assert_eq!((composer(&mut a, 80, 12).len(), a.line.buf.as_str()), (3, ":s"), "esc: the list goes, the line stays");
}

#[test]
fn no_list_over_a_question_card() {
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1")]);
    palette(&mut a);
    let options = vec![Choice { id: "1".into(), label: "yes".into(), recommended: true, ..Choice::default() }, Choice { id: "2".into(), label: "no".into(), ..Choice::default() }];
    a.down(Down::Asks { open: vec![AskView { ask: "q".into(), asker: "c".into(), question: "go?".into(), shape: Shape::Choice, options, ..AskView::default() }] });
    typing(&mut a, ":s");
    let c = composer(&mut a, 80, 20);
    assert_eq!(c.len(), 3, "the question's card stands over the composer, and no list: {c:#?}");
    // the question put off, the list comes
    a.key(ctrl('u'));
    hit(&mut a, KeyCode::Esc);
    typing(&mut a, ":s");
    let c = composer(&mut a, 80, 20);
    assert!(c.len() == 5 && c[1].starts_with("│ ⇥ :stop"), "{c:#?}");
}

#[test]
fn esc_with_the_list_up_and_no_ghost_puts_the_list_away_and_keeps_the_line() {
    let mut a = app_with(Json::Null, vec![turn("t", "go", "1")]);
    palette(&mut a);
    typing(&mut a, ":s");
    assert_eq!(composer(&mut a, 80, 12).len(), 5);
    typing(&mut a, "x");
    assert_eq!(composer(&mut a, 80, 12).len(), 5, "the list stays up, with nothing in it and no ghost");
    hit(&mut a, KeyCode::Esc);
    assert_eq!((composer(&mut a, 80, 12).len(), a.line.buf.as_str()), (3, ":sx"), "the first esc is the list's");
    // a line sent: the list that was up is forgotten before any frame, and esc is the screen's again
    a.key(ctrl('u'));
    typing(&mut a, ":s");
    assert_eq!(composer(&mut a, 80, 12).len(), 5);
    hit(&mut a, KeyCode::Enter);
    typing(&mut a, "z");
    assert!(!a.suggest.open, "the list that was up is forgotten before any frame");
    assert_eq!(composer(&mut a, 80, 12).len(), 3, "no list was drawn for this line");
    hit(&mut a, KeyCode::Esc);
    assert_eq!(a.line.buf, "z", "and esc keeps the line");
}

// ---- the composer guard: a key typed on a line ----

/// A line that holds a key goes to the core by one road only: a `:key …` line is said (the core keeps `:key …` in the Log) and is kept
/// nowhere on the screen; a line that is one key-like word is held on a card before anything of it goes, and leaves only as the card says.
mod key_guard {
    use crate::world::{app, chat, rows, screen, type_in};
    use j4n_proto::{Act, Cell, Down, Intent, Json, Role, Row, Tone, Up};
    use j4n_tui::App;
    use j4n_tui::headless::{KeyCode, ctrl, press};
    use std::time::{Duration, Instant};

    /// A made-up key of no vendor's shape: the guard is to see its look, not a prefix it knows.
    const FAKE: &str = "Qm4tH7vR2kX9pL0wZ8cN3bF6yD1sJ5gA2eU7iO4n";

    fn chatting() -> App {
        let mut a = app();
        chat(&mut a, &[(Role::You, "hello"), (Role::Doer, "hi")]);
        a
    }

    fn enter(a: &mut App) -> Vec<Up> {
        a.key(press(KeyCode::Enter))
    }

    fn shown(a: &mut App) -> String {
        screen(a, 120, 30).join("\n")
    }

    fn said(ups: &[Up]) -> Vec<String> {
        ups.iter().filter_map(|u| if let Up::Intent { intent: Intent::Say { text, .. } } = u { Some(text.clone()) } else { None }).collect()
    }

    /// What each `Intent::Key` sent: the id and the key.
    fn keys(ups: &[Up]) -> Vec<(String, String)> {
        ups.iter().filter_map(|u| if let Up::Intent { intent: Intent::Key { id, value } } = u { Some((id.clone(), value.expose().to_string())) } else { None }).collect()
    }

    /// A row of Library › Providers: `⏎` types the key of `keyid` when it takes one.
    fn provider(id: &str, keyid: Option<&str>) -> Row {
        let acts = keyid.map(|k| Act { key: "enter".into(), label: "type the key".into(), intent: Intent::Set { kind: "key".into(), id: k.into(), field: "value".into(), value: Json::Null }, risk: 0 });
        Row { key: id.into(), cells: vec![Cell::new(id, Tone::Plain), Cell::new("api", Tone::Dim), Cell::new("", Tone::Dim), Cell::new("not set", Tone::Dim)], actions: acts.into_iter().collect(), ..Row::default() }
    }

    #[test]
    fn a_key_line_is_drawn_masked_after_its_first_word_while_it_is_typed() {
        let mut a = chatting();
        type_in(&mut a, ":key");
        assert!(shown(&mut a).contains(":key"), "the word itself is readable:\n{}", shown(&mut a));
        type_in(&mut a, &format!(" {}", &FAKE[..12]));
        let s = shown(&mut a);
        assert!(!s.contains(&FAKE[..4]), "no piece of the key is drawn while it is typed:\n{s}");
        assert!(s.contains(":key ••••••••••••"), "one mask a character, so the cursor stays where it is:\n{s}");
        type_in(&mut a, &FAKE[12..]);
        let s = shown(&mut a);
        assert!(!s.contains(&FAKE[20..28]), "nor once it is whole:\n{s}");
        // a line that only starts with the letters of the word is drawn as typed
        let mut b = chatting();
        type_in(&mut b, ":keys of the house");
        assert!(shown(&mut b).contains(":keys of the house"));
    }

    #[test]
    fn a_key_line_is_kept_in_neither_the_history_nor_the_echo_nor_asked_of_completion() {
        let mut a = chatting();
        type_in(&mut a, "fix the parser");
        enter(&mut a);
        let mut ups = type_in(&mut a, &format!(":key {FAKE}"));
        ups.extend(enter(&mut a));
        // the line goes to the core, which keeps `:key …` of it and refuses it there; nothing else carries a piece of it
        assert_eq!(said(&ups), [format!(":key {FAKE}")]);
        let others: Vec<&Up> = ups.iter().filter(|u| !matches!(u, Up::Intent { intent: Intent::Say { .. } })).collect();
        assert!(!format!("{others:?}").contains(&FAKE[..8]), "no window of completion holds a piece of it: {others:?}");
        assert!(a.history.iter().all(|h| !h.contains(&FAKE[..8])), "{:?}", a.history);
        assert!(!shown(&mut a).contains(&FAKE[..8]), "no echo of it in the chat");
        a.key(press(KeyCode::Up));
        assert_eq!(a.line.buf, "fix the parser", "↑ recalls the line before it, never it");
        assert!(!shown(&mut a).contains(&FAKE[..8]));
        // in a chat (`@c4 :key …`) and in any case, the same
        a.key(ctrl('u'));
        let mut ups = type_in(&mut a, &format!("@c4 :KEY {FAKE}"));
        ups.extend(enter(&mut a));
        assert_eq!(said(&ups).len(), 1);
        assert!(a.history.iter().all(|h| !h.contains(&FAKE[..8])) && !shown(&mut a).contains(&FAKE[..8]), "{:?}", a.history);
    }

    #[test]
    fn a_lone_key_like_line_is_held_on_a_card_and_drop_sends_nothing() {
        let mut a = chatting();
        let mut ups = type_in(&mut a, FAKE);
        ups.extend(enter(&mut a));
        assert!(said(&ups).is_empty(), "the line is held, not said: {ups:?}");
        assert!(!format!("{ups:?}").contains(&FAKE[..8]), "nothing that went holds a piece of it: {ups:?}");
        let s = shown(&mut a);
        for w in ["looks like a key", "keep it as a key", "send it as text", "drop it"] {
            assert!(s.contains(w), "`{w}` on the card:\n{s}");
        }
        assert!(!s.contains(&FAKE[..8]), "the card shows nothing of it:\n{s}");
        assert!(a.line.buf.is_empty() && a.history.is_empty(), "{:?}", a.history);
        // a key pressed while the card is up does nothing: it never sends the line as text
        let ups = a.key(press(KeyCode::Char('x')));
        assert!(said(&ups).is_empty() && shown(&mut a).contains("looks like a key"));
        let ups = a.key(press(KeyCode::Char('3')));
        assert!(ups.iter().all(|u| !matches!(u, Up::Intent { .. })), "drop sends nothing: {ups:?}");
        let s = shown(&mut a);
        assert!(!s.contains("looks like a key") && !s.contains(&FAKE[..8]), "{s}");
        a.key(press(KeyCode::Up));
        assert!(a.line.buf.is_empty(), "↑ recalls nothing of it");
        // esc drops it too
        type_in(&mut a, FAKE);
        enter(&mut a);
        let ups = a.key(press(KeyCode::Esc));
        assert!(ups.iter().all(|u| !matches!(u, Up::Intent { .. })) && !shown(&mut a).contains("looks like a key"), "{ups:?}");
    }

    #[test]
    fn keep_it_as_a_key_asks_which_provider_and_sends_the_key_on_its_one_road() {
        let mut a = chatting();
        type_in(&mut a, FAKE);
        let ups = enter(&mut a);
        assert!(ups.contains(&Up::Watch { bind: "view:providers".into() }), "the providers are asked for while the card is up: {ups:?}");
        rows(&mut a, "view:providers", vec![provider("acme", Some("acme")), provider("local", None), provider("gateway", Some("gw"))]);
        let ups = a.key(press(KeyCode::Char('1')));
        assert!(ups.iter().all(|u| !matches!(u, Up::Intent { .. })), "two take a key: it asks which: {ups:?}");
        let s = shown(&mut a);
        assert!(s.contains("which provider") && s.contains("acme") && s.contains("gateway") && !s.contains("local"), "the providers that take a key:\n{s}");
        assert!(!s.contains(&FAKE[..8]), "{s}");
        let ups = a.key(press(KeyCode::Char('2')));
        assert_eq!(keys(&ups), [("gw".to_string(), FAKE.to_string())], "the key goes once, as `Intent::Key` for the row picked: {ups:?}");
        assert!(said(&ups).is_empty() && !ups.iter().any(|u| matches!(u, Up::Intent { intent: Intent::Set { .. } })), "never a line or a `Set`: {ups:?}");
        assert!(ups.contains(&Up::Unwatch { bind: "view:providers".into() }), "{ups:?}");
        assert!(!shown(&mut a).contains("which provider") && a.history.is_empty());
        // one provider takes a key: `keep` (or ⏎, its recommended answer) sends it there at once
        type_in(&mut a, FAKE);
        enter(&mut a);
        rows(&mut a, "view:providers", vec![provider("acme", Some("acme")), provider("local", None)]);
        assert_eq!(keys(&enter(&mut a)), [("acme".to_string(), FAKE.to_string())]);
        // an environment line is read as the key it holds, as the key's editor reads a paste: its name is said and left out, and so are quotes
        type_in(&mut a, &format!("OPENAI_API_KEY=\"{FAKE}\""));
        enter(&mut a);
        rows(&mut a, "view:providers", vec![provider("acme", Some("acme"))]);
        let s = shown(&mut a);
        assert!(s.contains("OPENAI_API_KEY") && !s.contains(&FAKE[..8]), "the name is said, nothing of the key:\n{s}");
        assert_eq!(keys(&a.key(press(KeyCode::Char('1')))), [("acme".to_string(), FAKE.to_string())]);
        type_in(&mut a, &format!("'{FAKE}'"));
        enter(&mut a);
        rows(&mut a, "view:providers", vec![provider("acme", Some("acme"))]);
        assert_eq!(keys(&enter(&mut a)), [("acme".to_string(), FAKE.to_string())], "a key in quotes is held, and kept without them");
    }

    #[test]
    fn send_it_as_text_says_the_line_and_a_long_word_in_a_sentence_or_a_path_is_sent_at_once() {
        let mut a = chatting();
        type_in(&mut a, FAKE);
        enter(&mut a);
        assert_eq!(said(&a.key(press(KeyCode::Char('2')))), [FAKE]);
        let word = "internationalization0123456789abcdefghij";
        for line in [format!("please keep {word} as it is"), "crates/j4n-core/src/front/keyfile.rs".into(), "supercalifragilisticexpialidocious".into(), "123e4567-e89b-12d3-a456-426614174000".into()] {
            type_in(&mut a, &line);
            assert_eq!(said(&enter(&mut a)), std::slice::from_ref(&line), "{line} is sent at once");
        }
    }

    #[test]
    fn a_lone_key_is_never_sent_as_a_draft_nor_asked_of_completion_as_a_file() {
        let mut a = chatting();
        a.down(Down::Value { bind: "shape:on".into(), value: Json::Bool(true), rev: 1 });
        let t0 = Instant::now();
        let mut ups = type_in(&mut a, FAKE);
        ups.extend(a.tick(t0));
        ups.extend(a.tick(t0 + Duration::from_secs(5)));
        assert!(!format!("{ups:?}").contains(&FAKE[..8]), "the shaper is never sent it: {ups:?}");
        // an ordinary line still goes as a draft, so the case above is the guard and not a shaper that sends nothing
        a.key(ctrl('u'));
        type_in(&mut a, "fix the parser thing");
        a.tick(t0 + Duration::from_secs(6));
        assert!(a.tick(t0 + Duration::from_secs(12)).iter().any(|u| matches!(u, Up::Draft { .. })));
        // a key with a `/` in it is a file to the completion: no start of it is asked, not even before it is 20 long
        a.key(ctrl('u'));
        let slashed = "Qm4tH7vR2kX9/pL0wZ8cN3bF6yD1sJ5gA2eU7iO4n";
        let ups = type_in(&mut a, slashed);
        let sent = format!("{ups:?}");
        assert!(!sent.contains(&slashed[..8]) && !sent.contains("find: \"file "), "{ups:?}");
        // the boundary: a part shorter than a key's run of 12 is not told from a name and is still asked; a hash folder after another word is
        a.key(ctrl('u'));
        assert!(format!("{:?}", type_in(&mut a, "Qm4tH7vR2kX/")).contains("file Qm4tH7vR2kX/"));
        a.key(ctrl('u'));
        assert!(format!("{:?}", type_in(&mut a, "open 3f9a2c71b8e4d05a/")).contains("file 3f9a2c71b8e4d05a/"));
    }
}
