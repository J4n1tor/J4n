//! The base of the tree drawers (`display::layout`), pure: a flat list of items in, the pieces before each line out. The tables are the shapes a
//! tree must draw.

use j4n_tui::display::layout::{Item, Laid, Look, Piece::*, lay, place};
use j4n_tui::theme::{Settings, Theme};

/// The pieces of every line as one word: `T` tee, `E` elbow, `B` bar, `_` blank, and `+`/`-` for a fold's open and closed end.
fn word(items: &[Item]) -> Vec<String> {
    lay(items)
        .iter()
        .map(|l| {
            let p: String = l.pipes.iter().map(|p| match p { Tee => 'T', Elbow => 'E', Bar => 'B', Blank => '_', FoldOpen => '+', FoldClosed => '-' }).collect();
            format!("{p}{}", match l.fold { Some(FoldOpen) => "+", Some(FoldClosed) => "-", _ => "" })
        })
        .collect()
}

fn it<'a>(key: &'a str, parent: &'a str) -> Item<'a> {
    Item::new(key, (!parent.is_empty()).then_some(parent))
}

#[test]
fn a_list_with_two_fold_rows_has_one_elbow_and_the_bar_of_a_branch_stops_before_the_folds() {
    // c7 with a branch, c6, then the folds `done before` and `archived`: one elbow, and no bar from the last chat into the first fold
    let items = [it("c7", ""), it("c7.1", "c7"), it("c6", ""), Item::fold("done", None, false), Item::fold("archive", None, false)];
    assert_eq!(word(&items), ["T", "BE", "T", "T-", "E-"]);
}

#[test]
fn an_open_fold_holds_its_chats_one_level_in_and_a_bar_runs_only_while_the_fold_has_a_sibling_after_it() {
    let items = [it("c7", ""), Item::fold("done", None, true), it("c5", "done"), it("c5.1", "c5"), it("c4", "done"), Item::fold("archive", None, true), it("c2", "archive"), it("c2.1", "c2")];
    assert_eq!(word(&items), ["T", "T+", "BT", "BBE", "BE", "E+", "_E", "__E"]);
    assert_eq!(lay(&items).iter().filter(|l| l.level == 0 && l.last).count(), 1, "the top has one end");
}

#[test]
fn a_note_that_hangs_under_the_newest_turn_is_the_last_of_that_turn_and_not_a_second_end_of_the_list() {
    // two turns and a note under the second: the second is the end of the list, and the note hangs under it with no bar in its column
    let items = [it("t1", ""), it("t2", ""), it("note", "t2")];
    assert_eq!(word(&items), ["T", "E", "_E"]);
    // a note under the first turn: the bar runs through the column of the first, which has a turn after it
    let items = [it("t1", ""), it("note", "t1"), it("t2", "")];
    assert_eq!(word(&items), ["T", "BE", "E"]);
}

#[test]
fn the_selected_last_row_is_the_last_whatever_is_selected() {
    // the layout knows no selection: the last row is an elbow, every time
    let items = [it("a", ""), it("b", ""), it("c", "")];
    assert_eq!(word(&items), ["T", "T", "E"]);
}

#[test]
fn two_elbows_in_a_row_only_when_each_is_the_last_of_its_own_parent() {
    // a lane with one step that has one step: `╰─` under `╰─` is right, each one the last of its own parent, and the blank column keeps them apart
    let items = [it("lane", ""), it("step", "lane"), it("sub", "step")];
    assert_eq!(word(&items), ["E", "_E", "__E"]);
    // with a later lane the column is a bar, never a blank
    let items = [it("lane", ""), it("step", "lane"), it("sub", "step"), it("other", "")];
    assert_eq!(word(&items), ["T", "BE", "B_E", "E"]);
}

#[test]
fn no_level_more_than_the_parents_and_a_parent_that_is_not_there_is_the_top() {
    let items = [it("a", ""), it("b", "a"), it("lost", "gone"), it("late", "next"), it("next", "")];
    let laid = lay(&items);
    assert_eq!(laid.iter().map(|l| l.level).collect::<Vec<_>>(), [0, 1, 0, 0, 0], "a parent after its child, or missing, is no parent");
    assert_eq!(word(&items), ["T", "BE", "T", "T", "E"]);
}

#[test]
fn an_empty_list_and_a_single_item() {
    assert!(lay(&[]).is_empty());
    assert_eq!(word(&[it("only", "")]), ["E"]);
}

#[test]
fn a_bar_stays_in_every_column_whose_ancestor_has_one_after_it_however_deep() {
    // a row three levels down keeps the bar of the first level
    let items = [it("a", ""), it("b", "a"), it("c", "b"), it("d", "c"), it("e", "a"), it("z", "")];
    assert_eq!(word(&items), ["T", "BT", "BBE", "BB_E", "BE", "E"]);
}

/// A theme of a frame and a tier, to read the glyphs of a look.
fn theme(frame: &str, marks: &str) -> Theme {
    Theme::build(&Settings { frame: frame.into(), marks: marks.into(), ..Settings::default() }, &[]).expect("a theme")
}

/// The sample tree of the pipes design: open branches, a fold that is shut, one that ends the list.
fn sample() -> Vec<Item<'static>> {
    vec![it("c1", ""), it("A", "c1"), it("B", "c1"), it("c2", ""), it("c2.1", "c2"), Item::fold("old", None, false), Item::fold("arch", None, false)]
}

fn drawn(look: &Look, items: &[Item]) -> Vec<String> {
    lay(items).iter().zip(items).map(|(l, i)| format!("{}{}", look.prefix(l), i.key)).collect()
}

#[test]
fn round_opens_the_list_with_its_corner_and_nothing_else_does() {
    let th = theme("round", "unicode");
    assert_eq!(drawn(&Look::new(&th), &sample()), ["╭─c1", "│ ├─A", "│ ╰─B", "├─c2", "│ ╰─c2.1", "├─old", "╰─arch"]);
    // ascii: `.-` for the corner, the others as pipes
    let th = theme("round", "ascii");
    assert_eq!(drawn(&Look::new(&th), &sample()), [".-c1", "| |-A", "| `-B", "|-c2", "| `-c2.1", "|-old", "`-arch"]);
    // a list of one is its own end, and a tree that hangs from a head has no corner: its first row is a tee, and a column is a blank wider
    let th = theme("round", "unicode");
    assert_eq!(drawn(&Look::new(&th), &[it("only", "")]), ["╰─only"]);
    assert_eq!(drawn(&Look::headed(&th, 0), &sample()), ["├─ c1", "│  ├─ A", "│  ╰─ B", "├─ c2", "│  ╰─ c2.1", "├─ old", "╰─ arch"]);
}

#[test]
fn airy_has_no_lines_a_dot_at_the_top_and_an_arrow_for_what_nests() {
    let th = theme("airy", "unicode");
    assert_eq!(drawn(&Look::new(&th), &sample()), ["· c1", "   › A", "   › B", "· c2", "   › c2.1", "· old", "· arch"]);
    // ascii, as its glyphs draw it
    let th = theme("airy", "ascii");
    assert_eq!(drawn(&Look::new(&th), &sample()), [". c1", "   > A", "   > B", ". c2", "   > c2.1", ". old", ". arch"]);
    // what goes on under a row stands under its words: two cells under a top row, five under a nested one
    let th = theme("airy", "unicode");
    let laid = lay(&sample());
    assert_eq!(Look::new(&th).under(&laid[0]).concat().chars().count(), 2);
    assert_eq!(Look::new(&th).under(&laid[1]).concat().chars().count(), 5);
}

/// The transcript's walk (`chat/plan.rs`) places a row among its siblings under the row it placed before; the flat list says the same.
#[test]
fn place_says_what_lay_says_for_a_tree_walked_a_level_at_a_time() {
    // t: [lane a: [s1, s2: [n]], lane b: [s3]], lane c
    let items = [it("a", ""), it("s1", "a"), it("s2", "a"), it("n", "s2"), it("b", ""), it("s3", "b"), it("c", "")];
    let flat = lay(&items);
    let top: Vec<Laid> = (0..3).map(|k| place(None, k, 3)).collect();
    let a_kids: Vec<Laid> = (0..2).map(|k| place(Some(&top[0]), k, 2)).collect();
    let n = place(Some(&a_kids[1]), 0, 1);
    let s3 = place(Some(&top[1]), 0, 1);
    let walked = [top[0].clone(), a_kids[0].clone(), a_kids[1].clone(), n, top[1].clone(), s3, top[2].clone()];
    assert_eq!(walked.to_vec(), flat, "the same pieces, first and last, level");
    assert_eq!(word(&items), ["T", "BT", "BE", "B_E", "T", "BE", "E"]);
}

/// The shapes of the transcript: a turn's lanes at the top, the control row open, a note under the newest turn.
#[test]
fn the_transcript_shapes_on_the_pipes_the_hang_and_the_airy_look_of_a_head() {
    // lane A (two steps), a control row (open, two rows under it), then the lane B; the note is the end of the list under the newest turn
    let items = [it("A", ""), it("a1", "A"), it("a2", "A"), Item::fold("ctl", None, true), it("c1", "ctl"), it("c2", "ctl"), it("B", ""), it("note", "B")];
    let th = theme("pipes", "unicode");
    assert_eq!(drawn(&Look::headed(&th, 0), &items), ["├─ A", "│  ├─ a1", "│  └─ a2", "├─ ctl", "│  ├─ c1", "│  └─ c2", "└─ B", "   └─ note"]);
    let th = theme("round", "unicode");
    assert_eq!(drawn(&Look::headed(&th, 0), &items), ["├─ A", "│  ├─ a1", "│  ╰─ a2", "├─ ctl", "│  ├─ c1", "│  ╰─ c2", "╰─ B", "   ╰─ note"], "round: the rounded end, and no corner under a head");
    let th = theme("hang", "unicode");
    assert_eq!(drawn(&Look::headed(&th, 0), &items), ["A", "┆ a1", "╰ a2", "ctl", "┆ c1", "╰ c2", "B", "╰ note"]);
    let th = theme("airy", "unicode");
    assert_eq!(drawn(&Look::headed(&th, 0), &items), ["· A", "   › a1", "   › a2", "· ctl", "   › c1", "   › c2", "· B", "   › note"]);
}

/// The Tasks tree is a head and what hangs from it (the head has no pipe and its column is left out), a page is stacks of rows under heads that end each run.
#[test]
fn the_shapes_of_the_tasks_tree_and_of_a_page_with_sections() {
    // fix: [plan, work: [commit], check: [lint]]
    let items = [it("fix", ""), it("plan", "fix"), it("work", "fix"), it("commit", "work"), it("check", "fix"), it("lint", "check")];
    assert_eq!(word(&items), ["E", "_T", "_T", "_BE", "_E", "__E"]);
    // a bar in every column of a tree three levels in: `e` follows `b`, so the column of `b` is a bar for `d`
    let items = [it("a", ""), it("b", "a"), it("c", "b"), it("d", "c"), it("e", "a")];
    assert_eq!(word(&items), ["E", "_T", "_BE", "_B_E", "_E"]);
    // a page: each section is a run of rows laid on its own, so each ends in one elbow and the head of the next section cuts the run
    let run = |n: usize| -> Vec<String> { word(&(0..n).map(|i| Item::new(["r1", "r2", "r3"][i], None)).collect::<Vec<_>>()) };
    assert_eq!((run(1), run(3)), (vec!["E".to_string()], vec!["T".to_string(), "T".to_string(), "E".to_string()]));
}

/// A row of a stack (the fields of a form, the rows of a page or a card) is two cells in every look, and what goes on under it is two cells too.
#[test]
fn a_stack_row_is_two_cells_in_every_look() {
    use j4n_tui::display::layout::one;
    for (frame, tee, elbow, under) in [("pipes", "├ ", "└ ", "│ "), ("round", "├ ", "╰ ", "│ "), ("airy", "· ", "· ", "  "), ("hang", "  ", "  ", "  "), ("gutter", "  ", "  ", "  ")] {
        let th = theme(frame, "unicode");
        let look = Look::new(&th);
        assert_eq!(look.stack(&one(false)), (tee.to_string(), under.to_string()), "{frame}");
        assert_eq!(look.stack(&one(true)).0, elbow, "{frame}");
        assert_eq!(look.stack(&one(true)).1, "  ", "{frame}: nothing hangs under the last");
    }
    let th = theme("pipes", "ascii");
    assert_eq!(Look::new(&th).stack(&one(false)), ("| ".to_string(), "| ".to_string()));
    // a tight tree has a glyph and a blank for a column, and the first row of a round list has no corner
    let th = theme("round", "unicode");
    assert_eq!(drawn(&Look::tight(&th), &sample()), ["├ c1", "│ ├ A", "│ ╰ B", "├ c2", "│ ╰ c2.1", "├ old", "╰ arch"]);
}
