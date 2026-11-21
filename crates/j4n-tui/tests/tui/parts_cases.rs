//! The parts library on its own: the pipe of a row, the cursor and its keys, the keep-in-view rule, a line
//! of text, the frame of a box, and a form's draft. The screens that use them are the goldens of the other cases.

use crossterm::event::{KeyEvent, KeyModifiers};
use j4n_proto::{Down, Field, FieldKind, Intent, Json, Node, Present, Up, json};
use j4n_tui::App;
use j4n_tui::headless::{KeyCode, ctrl, press, typed};
use j4n_tui::parts::form::Form;
use j4n_tui::parts::line::Typed;
use j4n_tui::parts::popup::{BOX, HEAVY, edge, frame};
use j4n_tui::parts::rows::lead;
use j4n_tui::parts::scroll::{keep, keep_by};
use j4n_tui::parts::select::Sel;
use j4n_tui::theme::{Settings, Theme};
use ratatui::text::Span;
use std::collections::HashMap;

fn theme() -> Theme {
    Theme::build(&Settings::default(), &[]).expect("a theme")
}

fn k(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn the_cursor_moves_by_one_by_a_page_and_to_the_ends_and_stops_at_them() {
    let mut s = Sel::default();
    assert!(s.key(KeyCode::Down, 5, 20) && s.at == 1);
    assert!(s.key(KeyCode::PageDown, 5, 20) && s.at == 6);
    assert!(s.key(KeyCode::End, 5, 20) && s.at == 19, "the last of twenty");
    assert!(s.key(KeyCode::Down, 5, 20) && s.at == 19, "no further than the last");
    assert!(s.key(KeyCode::Char('g'), 5, 20) && s.at == 0);
    assert!(s.key(KeyCode::Up, 5, 20) && s.at == 0, "no further than the first");
    assert!(s.key(KeyCode::Char('G'), 5, 20) && s.at == 19);
    assert!(!s.key(KeyCode::Char('x'), 5, 20) && s.at == 19, "a key that is not a move is left alone");
    s.select(500, 20);
    assert_eq!(s.at, 19, "a place past the rows is the last");
    s.select(3, 0);
    assert_eq!(s.at, 0, "no rows, no place");
    s.at = 10_000_000;
    s.by(1, 20_000_000);
    assert_eq!(s.at, 10_000_001, "the core's rows are counted in u64");
}

#[test]
fn the_view_moves_only_as_far_as_the_row_and_its_detail_need() {
    assert_eq!(keep(5, 7, 7, 10), 5, "in sight: where it was");
    assert_eq!(keep(5, 2, 2, 10), 2, "above: up to it");
    assert_eq!(keep(5, 20, 20, 10), 11, "below: until it is the last shown");
    assert_eq!(keep(0, 4, 12, 10), 3, "a row with its detail: the detail is the last shown");
    assert_eq!(keep(0, 4, 40, 10), 4, "a row taller than the room is shown from its head");
    // rows of different heights: the rows from the top to the one the keys are on fit in `take` lines
    let h = |i: u64| if i.is_multiple_of(2) { 3 } else { 1 };
    assert_eq!(keep_by(0, 3, 6, h), 1, "3+1+3+1 is 8: the first goes, and 1+3+1 is 5");
    assert_eq!(keep_by(0, 3, 8, h), 0, "it does in 8");
    assert_eq!(keep_by(0, 4, 8, h), 1, "3+1+3+1+3 is 11: the first goes, then 1+3+1+3 is 8");
    assert_eq!(keep_by(9, 2, 8, h), 2, "a top below the row is the row");
}

#[test]
fn a_line_is_typed_into_at_the_cursor_in_characters() {
    let mut t = Typed::new("é世b");
    assert_eq!(t.at, 3);
    assert!(t.cursor_key(&k(KeyCode::Left)) && t.cursor_key(&k(KeyCode::Left)) && t.at == 1);
    assert!(t.edit(&typed("-", false)[0]) && t.buf == "é-世b" && t.at == 2);
    assert!(t.erase(false) && t.buf == "é世b" && t.at == 1, "a wide letter is one character");
    assert!(t.erase(true) && t.buf == "éb" && t.at == 1);
    t.at = 0;
    assert!(!t.erase(false), "nothing before the cursor");
    assert!(t.cursor_key(&k(KeyCode::End)) && t.at == 2 && !t.erase(true), "nothing after it");
    assert!(t.cursor_key(&k(KeyCode::Home)) && t.at == 0);
    assert!(t.cursor_key(&ctrl('e')) && t.at == 2 && t.cursor_key(&ctrl('a')) && t.at == 0, "⌃a and ⌃e are home and end");
    assert!(!t.cursor_key(&k(KeyCode::Char('x'))) && !t.edit(&k(KeyCode::Left)), "each key is one of the two");
    let ctrl_x = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL);
    assert!(!t.edit(&ctrl_x) && t.buf == "éb", "a chord is not text");
    assert_eq!(t.take(), "éb");
    assert_eq!(t, Typed::default());
}

#[test]
fn a_row_has_its_pipe_in_the_edge_colour_and_the_accent_where_the_keys_are() {
    let th = theme();
    assert_eq!(lead(&th, false, false, false).content, "├ ");
    assert_eq!(lead(&th, true, false, false).content, "╰ ");
    let (plain, on, bad) = (lead(&th, true, false, false), lead(&th, true, true, false), lead(&th, true, false, true));
    assert_ne!(plain.style, on.style, "the mark of the row the keys are on has the accent");
    assert_ne!(plain.style, bad.style, "a bad one the error colour");
    assert_eq!(lead(&th, true, true, true).style, on.style, "the keys come before the fault");
}

#[test]
fn a_frame_is_as_wide_as_it_is_asked_with_the_title_in_its_top_edge_and_any_set_of_slots() {
    let th = theme();
    let style = th.fg(j4n_tui::theme::palette::Role::Edge);
    for w in [4, 12, 40] {
        let top = edge(&th, w, "find", false, style, &BOX);
        assert_eq!(top.width(), w, "{}", top);
        assert_eq!(edge(&th, w, "find", true, style, &BOX).width(), w);
    }
    assert!(edge(&th, 20, "find", false, style, &BOX).to_string().contains("─ find ─"));
    assert!(!edge(&th, 20, "find", true, style, &BOX).to_string().contains("find"), "the bottom has no title");
    assert!(!edge(&th, 5, "a long title", false, style, &BOX).to_string().contains("title"), "no room, no title");
    let body = vec![vec![Span::raw("a")], vec![Span::raw("longer")]];
    let lines = frame(&th, body.clone(), 12, "", style, &BOX);
    assert_eq!(lines.len(), 4, "two rows between two edges");
    assert!(lines.iter().all(|l| l.width() == 12), "each row is padded to the room inside");
    let heavy = frame(&th, body, 12, "", style, &HEAVY);
    assert_ne!(heavy[0].to_string(), lines[0].to_string(), "the heavy frame is another set of slots");
    assert_eq!(heavy[1].to_string().chars().next(), th.glyph(HEAVY[5]).chars().next());
}

fn field(id: &str, kind: FieldKind) -> Node {
    Node { id: id.into(), title: id.into(), field: Some(Field { kind, bind: format!("acct:new.{id}"), props: Json::Null }), ..Node::default() }
}

#[test]
fn a_form_keeps_what_is_typed_and_says_it_as_one_command_when_it_is_submitted() {
    let form = Form::new("account", &["add"], &["acct:new.connector", "acct:new.name", "acct:new.proxy"]);
    assert!(form.holds("acct:new.name") && !form.holds("acct:other.name"));
    let mut values: HashMap<String, Json> = HashMap::new();
    values.insert("acct:new.connector".into(), json!("claude"));
    values.insert("acct:new.name".into(), json!("claude-2"));
    values.insert("acct:new.proxy".into(), json!(""));
    let sent = form.submit(&values);
    assert_eq!(sent, vec![Up::Intent { intent: Intent::Run { command: "account".into(), args: vec!["add".into(), "claude".into(), "claude-2".into()] } }], "an empty field is left out");
    form.clear(&mut values);
    assert!(values.is_empty(), "the draft goes");

    // typed into the screen's fields, nothing goes to the core until the submit row says it
    let root = Node { id: "root".into(), present: Present::Stack, children: vec![field("connector", FieldKind::Str), field("name", FieldKind::Str)], ..Node::default() };
    let mut a = App::new();
    a.start();
    a.down(hello_down());
    a.down(Down::Tree { node: root });
    a.forms.push(Form::new("account", &["add"], &["acct:new.connector", "acct:new.name"]));
    for ch in [press(KeyCode::Enter)].into_iter().chain(typed("claude", false)).chain([press(KeyCode::Enter)]) {
        assert!(a.key(ch).is_empty(), "a field of a form is a draft: no Set");
    }
    assert_eq!(a.value("acct:new.connector"), &json!("claude"), "the draft is what the field shows");
    a.forms.clear();
    a.key(press(KeyCode::Down));
    a.key(press(KeyCode::Enter));
    typed("x", false).into_iter().for_each(|c| drop(a.key(c)));
    let sent = a.key(press(KeyCode::Enter));
    assert!(matches!(sent.first(), Some(Up::Intent { intent: Intent::Set { .. } })), "a field that is no form's sends as it did: {sent:?}");
}

fn hello_down() -> Down {
    Down::Hello { api: j4n_proto::API, name: "j4n".into(), version: "0".into(), project: "p".into() }
}

#[test]
fn a_track_is_a_two_cell_segment_of_the_thin_line_that_bounces_and_never_fills() {
    // `parts::track`. No marks: the heavy line over the light one of the context gauge, a segment of two cells on a track of six, which runs right and back.
    let text = |s: &[Span]| s.iter().map(|s| s.content.as_ref()).collect::<String>();
    let th = Theme::build(&Settings { marks: "unicode".into(), ..Settings::default() }, &[]).expect("a theme");
    assert_eq!(text(&j4n_tui::parts::track::track(&th, 0, 6)), "━━────");
    assert_eq!(text(&j4n_tui::parts::track::track(&th, 4, 6)), "────━━");
    // the segment is the accent and the rest of the track is the faint one
    let (a, b) = (j4n_tui::parts::track::track(&th, 0, 6), j4n_tui::parts::track::track(&th, 2, 6));
    assert_ne!(text(&a), text(&b), "the segment moved");
    assert_eq!(a[1].style, b[1].style, "the segment is drawn alike wherever it is");
    assert_ne!(a[1].style, a[2].style, "the segment and the track differ");
}

#[test]
fn a_part_that_waits_is_the_track_and_a_dim_word_and_a_narrow_row_drops_the_why_then_cells_of_the_sign() {
    // `parts::track::waiting`: a list or a log that waits has no label, a chart its own
    use j4n_tui::parts::track::waiting;
    let text = |l: &ratatui::text::Line| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>();
    let th = Theme::build(&Settings { marks: "unicode".into(), ..Settings::default() }, &[]).expect("a theme");
    assert_eq!(text(&waiting(&th, 0, false, "", 60)), "━━──── loading");
    assert_eq!(text(&waiting(&th, 0, true, "", 60)), "━━──── loading · the core is busy");
    assert_eq!(text(&waiting(&th, 0, false, "a day", 60)), "  a day  ━━──── loading");
    assert_eq!(text(&waiting(&th, 0, true, "", 20)), "━━──── loading", "the why goes first");
    assert_eq!(text(&waiting(&th, 0, false, "", 12)), "━━── loading", "then the sign is the short one");
    assert_eq!(text(&waiting(&th, 0, false, "", 8)), "loading", "then the word alone");
    let line = waiting(&th, 0, false, "", 60);
    assert_eq!(line.spans.last().map(|s| s.style), Some(th.tone(j4n_proto::Tone::Dim)), "the word is dim");
}

#[test]
fn a_hint_holds_the_core_s_words_and_is_the_word_a_line_starts_and_nothing_for_any_other_line() {
    use j4n_tui::parts::hint::{Hint, empty, tail};
    let th = theme();
    let h = Hint::of(Some(&json!({"words": ["skip", "stop"], "what": "review t51"}))).expect("a hint");
    assert_eq!(h, Hint { words: vec!["skip".into(), "stop".into()], what: "review t51".into() });
    assert_eq!(Hint::of(Some(&json!({"words": []}))), None, "no word, no hint");
    assert_eq!(Hint::of(Some(&Json::Null)), None);
    assert_eq!(empty(&th, &h).iter().map(|s| s.content.to_string()).collect::<String>(), "skip · stop  review t51");
    assert_eq!((h.word("s"), h.word("St"), h.word(" skip "), h.word(""), h.word("deploy it"), h.word("skipped")), (Some("skip"), Some("stop"), Some("skip"), None, None, None));
    assert_eq!(tail(&th, &h, "sto").map(|v| v.iter().map(|s| s.content.to_string()).collect::<String>()), Some("stop ⏎".into()));
    assert!(tail(&th, &h, "deploy").is_none());
}

// `parts::suggest`: the slot of the end of a line, one score for every source, the ghost, and Tab's steps over a list that holds still.
#[test]
fn suggest_finds_the_slot_orders_by_one_score_and_steps_through_a_list_that_holds_still() {
    use j4n_tui::parts::suggest::{At, Slot, Step, WEIGHTS, find, ghost_of, rank, slot_of, word_of};
    let at = |line: &str| slot_of(line);
    assert_eq!(at(""), At { slot: Slot::Empty, token: String::new(), start: 0 });
    assert_eq!(at(":sw"), At { slot: Slot::Command, token: ":sw".into(), start: 0 });
    assert_eq!(at("ask @c3"), At { slot: Slot::Chat, token: "@c3".into(), start: 4 });
    assert_eq!(at(":switch cl"), At { slot: Slot::Word, token: "cl".into(), start: 8 });
    assert_eq!(at(":switch ").slot, Slot::Word, "a blank after a command: its next word");
    assert_eq!(at("look at src/ma").slot, Slot::File);
    assert_eq!(at("open notes.md").slot, Slot::File);
    assert_eq!(at("fix the parser").slot, Slot::Sentence);
    assert_eq!(find(&at(":switch cl"), ":switch cl").as_deref(), Some("line :switch cl"), "the core reads the command's slots");
    assert_eq!(find(&at("see src/ma"), "see src/ma").as_deref(), Some("file src/ma"));
    assert_eq!(find(&at(":sw"), ":sw").as_deref(), Some("line :sw"), "the core holds the phrases' commands, the screen the palette's");
    assert_eq!(find(&at("ask @c"), "ask @c"), None, "the chats are the screen's");
    // the weights, in one table: start, start of a word, letters in order; the slot; a file
    assert_eq!((WEIGHTS.prefix, WEIGHTS.word, WEIGHTS.fuzzy, WEIGHTS.slot, WEIGHTS.file), (300, 200, 100, 50, 10));
    let items = |list: &[(&str, i32)]| list.iter().map(|(t, plus)| ((*t).to_string(), String::new(), *plus)).collect::<Vec<_>>();
    let ranked = rank("pa", items(&[("my-palette", 0), ("spare", 0), ("palettes", 0), ("pack", 0), ("pa", 0), ("zz", 0), ("pack", 0), ("paths.md", 10)]));
    let order: Vec<(&str, i32)> = ranked.iter().map(|c| (c.text.as_str(), c.score)).collect();
    assert_eq!(order[..4], [("paths.md", 360), ("pack", 350), ("palettes", 350), ("my-palette", 250)], "a start beats a word's start; a file adds its weight; ties go to the shorter");
    assert_eq!(order[4].0, "spare", "letters in order last");
    assert!(order[4].1 > WEIGHTS.fuzzy + WEIGHTS.slot - 1 && order[4].1 < WEIGHTS.word + WEIGHTS.slot);
    assert!(!order.iter().any(|(t, _)| *t == "pa" || *t == "zz"), "the token itself and what does not match are left out");
    assert_eq!(order.iter().filter(|(t, _)| *t == "pack").count(), 1, "one of each");
    // the ghost is the rest of the best one, only where it extends the token
    assert_eq!(ghost_of("pa", &ranked[0]).as_deref(), Some("ths.md"));
    assert_eq!(ghost_of("pa", &ranked[3]), None, "`my-palette` does not start with `pa`");
    assert_eq!((word_of("eploy the fix"), word_of(" the fix"), word_of("")), ("eploy", " the", ""));
    // Tab's steps: the candidate stands in for the token, what was after the cursor stays, and the list holds still
    let line = Typed { buf: ":switch cl now".into(), at: 10 };
    let mut s = Step::begin(&line, 8, 10, vec!["claude".into(), "codex".into()], false).expect("two to take");
    assert_eq!(s.line(), Typed { buf: ":switch claude now".into(), at: 14 });
    s.next(false);
    assert_eq!(s.line().buf, ":switch codex now");
    s.next(false);
    assert_eq!(s.line().buf, ":switch claude now", "round the end");
    s.next(true);
    assert_eq!(s.line().buf, ":switch codex now", "back");
    assert_eq!(Step::begin(&line, 8, 10, vec!["a".into(), "b".into()], true).map(|s| s.at), Some(1), "⇧⇥ begins at the last");
    assert_eq!(Step::begin(&line, 8, 10, Vec::new(), false), None, "nothing to take, nothing done");
}

// `parts::suggest::row`: the list of the strip's left, its cuts and its tiers.
#[test]
fn the_list_row_marks_the_one_in_the_line_cuts_what_is_long_and_counts_what_does_not_fit() {
    use j4n_tui::parts::suggest::{Cand, List, Slot, row};
    use j4n_tui::theme::palette::Role;
    use ratatui::style::Modifier;
    let th = |marks: &str, no_color: bool| Theme::build(&Settings { marks: marks.into(), no_color, ..Settings::default() }, &[]).expect("a theme");
    let text = |spans: &[Span]| spans.iter().map(|s| s.content.as_ref()).collect::<String>();
    let cand = |t: &str, side: &str| Cand { text: t.into(), side: side.into(), ..Cand::default() };
    let commands = List { items: [":stop", ":setup", ":switch", ":status"].iter().map(|t| cand(t, "")).collect(), at: Some(0), slot: Slot::Command };
    let u = th("unicode", false);
    let r = row(&u, &commands, 60, false);
    assert_eq!(text(&r), "⇥ :stop  :setup  :switch  :status");
    let pick = r.iter().find(|s| s.content == ":stop").expect("the one in the line");
    assert_eq!((pick.style.fg, pick.style.add_modifier.contains(Modifier::BOLD)), (u.fg(Role::Accent).fg, true), "accent and bold");
    assert_eq!(r.iter().find(|s| s.content == ":setup").map(|s| s.style), Some(Default::default()), "the others plain");
    assert_eq!(text(&row(&u, &commands, 20, false)), "⇥ :stop  :setup  +2", "what does not fit is counted");
    assert_eq!(text(&row(&u, &commands, 60, true)), "⇥ :stop 1/4", "narrow: the one in the line and its place");
    // stepped past what fits: the list starts at the one in the line
    let far = List { at: Some(3), ..commands.clone() };
    assert_eq!(text(&row(&u, &far, 22, false)), "⇥ :status  +3");
    assert_eq!(text(&row(&u, &List { items: Vec::new(), at: None, slot: Slot::Word }, 60, false)), "⇥", "nothing to offer: the mark alone");
    // a value beside its word, a chat by its id and title, a path cut in the middle, a sentence at 32 cells, the Decider's first marked `~`
    let look = List { items: vec![cand("palette", "jjj"), cand("frame", "pipes")], at: None, slot: Slot::Word };
    let r = row(&u, &look, 60, false);
    assert_eq!(text(&r), "⇥ palette jjj  frame pipes");
    assert_eq!(r.iter().find(|s| s.content == " jjj").map(|s| s.style), Some(u.tone(j4n_proto::Tone::Dim)), "the value now is dim");
    assert_eq!(text(&row(&u, &List { items: vec![cand("@c3", "lexer"), cand("@c4", "parser")], at: Some(0), slot: Slot::Chat }, 60, false)), "⇥ c3 lexer  c4 parser");
    let file = List { items: vec![cand("tests/unit/deep/engine_cases.py", "")], at: None, slot: Slot::File };
    assert_eq!(text(&row(&u, &file, 60, false)), "⇥ tests/…/engine_cases.py");
    let long = Cand { made: true, ..cand("deploy the fix to the login page and then run every test", "") };
    let r = row(&u, &List { items: vec![long.clone()], at: Some(0), slot: Slot::Sentence }, 60, false);
    assert_eq!(text(&r), "⇥ ~deploy the fix to the login pa…");
    assert_eq!(r[2].style, u.fg(Role::Made), "`~` in the hue of what a model made");
    assert_eq!(text(&row(&u, &List { items: vec![long], at: Some(0), slot: Slot::Word }, 60, false)), "⇥ ~deploy the fix to the…", "any other item at 24 cells, no blank before the cut");
    // the tiers: ascii's own marks, words says `tab:` and `more`; without colour the one in the line is bold and underlined
    let a = th("ascii", false);
    assert_eq!(text(&row(&a, &commands, 22, false)), "tab :stop  :setup  +2");
    assert_eq!(text(&row(&a, &List { items: vec![cand("deploy the fix to the login page and then", "")], at: None, slot: Slot::Word }, 60, false)), "tab deploy the fix to the...");
    assert_eq!(text(&row(&th("words", false), &commands, 27, false)), "tab: :stop  :setup  +2 more");
    let n = th("unicode", true);
    let pick = row(&n, &commands, 60, false).into_iter().find(|s| s.content == ":stop").expect("the one in the line");
    assert_eq!((pick.style.fg, pick.style.add_modifier), (None, Modifier::BOLD | Modifier::UNDERLINED));
}
