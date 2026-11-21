//! The node tree. How children are shown (by `present` and by depth), the keys of each presentation, the lazy children, the
//! jump to a node, and what is drawn.

use crate::world::{hello, leaf, row, rows, screen, show};
use j4n_proto::{Down, FieldKind, Intent, Node, Present, Up};
use j4n_tui::App;
use j4n_tui::fuzzy;
use j4n_tui::headless::{KeyCode, Snap, ctrl, press, typed};
use j4n_tui::nodes::{self, how};

fn group(id: &str, title: &str, present: Present, children: Vec<Node>) -> Node {
    Node { id: id.into(), title: title.into(), present, children, ..Node::default() }
}

fn list(id: &str) -> Node {
    leaf(id, id, FieldKind::List, &format!("{id}:rows"))
}

fn started(root: Node) -> App {
    let mut a = App::new();
    a.start();
    a.down(hello());
    a.down(Down::Tree { node: root });
    a
}

fn keys(a: &mut App, text: &str) -> Vec<Up> {
    typed(text, false).into_iter().flat_map(|k| a.key(k)).collect()
}

fn page(a: &App) -> String {
    a.page().map(|p| p.id.clone()).unwrap_or_default()
}

fn watched(ups: &[Up]) -> Vec<String> {
    ups.iter().filter_map(|u| if let Up::Watch { bind } = u { Some(bind.clone()) } else { None }).collect()
}

fn unwatched(ups: &[Up]) -> Vec<String> {
    ups.iter().filter_map(|u| if let Up::Unwatch { bind } = u { Some(bind.clone()) } else { None }).collect()
}

fn opened(ups: &[Up]) -> Vec<String> {
    ups.iter().filter_map(|u| if let Up::Intent { intent: Intent::Open { node } } = u { Some(node.clone()) } else { None }).collect()
}

/// tabs → a, b (sub-tabs: b1, b2), c (sections: c1, c2, c3)
fn sample() -> Node {
    let b = group("b", "B", Present::Subtabs, vec![list("b1"), list("b2")]);
    let c = group("c", "C", Present::Sections, vec![list("c1"), list("c2"), list("c3")]);
    group("root", "", Present::Tabs, vec![list("a"), b, c])
}

#[test]
fn auto_is_decided_by_depth_below_the_root() {
    let auto = |d| how(&Node { present: Present::Auto, ..Node::default() }, d);
    assert_eq!((auto(0), auto(1), auto(2), auto(3), auto(9)), (Present::Tabs, Present::Subtabs, Present::Sections, Present::Stack, Present::Stack));
    for p in [Present::Stack, Present::Tabs, Present::Sections, Present::Subtabs, Present::Split, Present::Grid] {
        assert_eq!(how(&Node { present: p, ..Node::default() }, 5), p, "a node that says how it is shown is shown that way");
    }
    assert!(nodes::tabbed(Present::Tabs) && nodes::tabbed(Present::Subtabs) && !nodes::tabbed(Present::Split) && !nodes::tabbed(Present::Sections));
    let mut root = sample();
    root.present = Present::Auto;
    root.children[1].present = Present::Auto;
    let mut a = started(root);
    a.key(press(KeyCode::F(2)));
    assert_eq!(page(&a), "b1", "the top is tabs, the next level sub-tabs");
    assert!(show(&mut a, 40, 6).contains("▔"), "drawn as sub-tabs");
}

#[test]
fn the_digits_and_the_brackets_move_the_tabs_and_the_braces_the_subtabs() {
    let mut a = started(sample());
    assert_eq!(page(&a), "a");
    let ups = keys(&mut a, "2");
    assert_eq!(page(&a), "b1");
    assert_eq!(unwatched(&ups), ["a:rows"]);
    assert_eq!(watched(&ups), ["b1:rows"]);
    keys(&mut a, "}");
    assert_eq!(page(&a), "b2");
    keys(&mut a, "}");
    assert_eq!(page(&a), "b1", "round the corner");
    keys(&mut a, "{");
    assert_eq!(page(&a), "b2");
    keys(&mut a, "]");
    assert_eq!(page(&a), "c", "a sections group has the keys itself");
    keys(&mut a, "]");
    assert_eq!(page(&a), "a");
    keys(&mut a, "[");
    assert_eq!(page(&a), "c");
    keys(&mut a, "9");
    assert_eq!(page(&a), "c", "there is no ninth tab");
    keys(&mut a, "0");
    assert_eq!(page(&a), "c");
    keys(&mut a, "1");
    assert_eq!(page(&a), "a");
    assert!(keys(&mut a, "{").is_empty() && keys(&mut a, "}").is_empty(), "no sub-tabs open: nothing to move");
}

#[test]
fn a_chat_keeps_its_digits_and_brackets_and_a_row_keeps_its_own_keys() {
    let mut a = started(group("root", "", Present::Tabs, vec![leaf("chat", "Chat", FieldKind::Chat, "chat:main"), list("rows")]));
    keys(&mut a, "2[]{}");
    assert_eq!(a.line.buf, "2[]{}", "typed into the draft");
    assert_eq!(page(&a), "chat");
    a.key(press(KeyCode::F(2)));
    let mut r = row("x", &["x"]);
    r.actions = vec![j4n_proto::Act { key: "1".into(), label: "one".into(), intent: Intent::Open { node: "chat".into() }, risk: 0 }];
    rows(&mut a, "rows:rows", vec![r]);
    let ups = keys(&mut a, "1");
    assert!(matches!(ups.as_slice(), [Up::Intent { intent: Intent::Open { .. } }]), "{ups:?}");
    assert_eq!(page(&a), "rows", "a digit that is the key of a row is the row's");
    keys(&mut a, "1");
    assert_eq!(page(&a), "rows");
}

#[test]
fn sections_fold_with_enter_and_space_and_z_folds_them_all_and_opens_them_all() {
    let mut a = started(sample());
    a.key(press(KeyCode::F(3)));
    assert_eq!(page(&a), "c");
    assert_eq!(a.needed().into_iter().collect::<Vec<_>>(), ["c1:rows", "c2:rows", "c3:rows", "commands:all"], "the sections, and the commands the jump lists");
    a.key(press(KeyCode::Down));
    let ups = a.key(press(KeyCode::Enter));
    assert!(a.shut.contains("c2") && unwatched(&ups) == ["c2:rows"], "{:?} {ups:?}", a.shut);
    let ups = a.key(press(KeyCode::Char(' ')));
    assert!(!a.shut.contains("c2") && watched(&ups) == ["c2:rows"], "space folds and opens");
    a.key(press(KeyCode::Down));
    a.key(press(KeyCode::Down));
    a.key(press(KeyCode::Enter));
    assert!(a.shut.contains("c3"), "the arrows stop at the last header");
    a.key(press(KeyCode::Up));
    a.key(press(KeyCode::Up));
    a.key(press(KeyCode::Up));
    a.key(press(KeyCode::Enter));
    assert!(a.shut.contains("c1"), "and at the first");
    let ups = keys(&mut a, "z");
    assert_eq!(a.shut.len(), 3, "some were open, so z folds all");
    assert_eq!(unwatched(&ups), ["c2:rows"]);
    keys(&mut a, "z");
    assert!(a.shut.is_empty(), "all were folded, so z opens all");
}

#[test]
fn a_section_shows_its_fold_its_title_and_its_count_and_a_folded_one_has_no_body() {
    let mut a = started(group("root", "", Present::Sections, vec![Node { count: Some(6), ..list("one") }, list("two")]));
    a.shut.insert("two".into());
    let s = screen(&mut a, 40, 6);
    assert!(s[0].starts_with("󰁛") || s[0].starts_with("▾ one"), "{s:?}");
    assert!(s[0].contains("one") && s[0].contains("6"), "{s:?}");
    assert!(s.iter().any(|l| l.starts_with("▸ two")), "the fold of a shut section: {s:?}");
    let t = show(&mut a, 40, 6);
    assert_eq!(t.matches("two").count(), 1, "a folded section is its header only");
}

fn split(ratio: u8) -> Node {
    Node { ratio, ..group("s", "", Present::Split, vec![list("left"), list("right")]) }
}

#[test]
fn a_split_gives_the_first_pane_its_share_and_the_arrows_move_between_the_panes() {
    let mut a = started(split(25));
    rows(&mut a, "left:rows", vec![row("l", &["L"])]);
    rows(&mut a, "right:rows", vec![row("r", &["R"])]);
    let s = screen(&mut a, 80, 4);
    let bar = s[1].chars().position(|c| c == '┃').expect("the bar of the focused pane");
    assert!((19..=21).contains(&bar), "a quarter of 80 columns: {s:?}");
    assert_eq!(page(&a), "left");
    let ups = a.key(press(KeyCode::Right));
    assert_eq!(page(&a), "right");
    assert_eq!((unwatched(&ups), watched(&ups)), (Vec::<String>::new(), Vec::<String>::new()), "both panes are on the screen, so both are watched all along");
    a.key(press(KeyCode::Right));
    assert_eq!(page(&a), "right", "the last pane is the last");
    a.key(press(KeyCode::Left));
    a.key(press(KeyCode::Left));
    assert_eq!(page(&a), "left");
    a.key(ctrl_arrow(KeyCode::Right));
    assert_eq!(page(&a), "right", "ctrl with an arrow does it too");
    assert_eq!(nodes::grid_auto(59), 1);
}

fn ctrl_arrow(code: KeyCode) -> crossterm::event::KeyEvent {
    crossterm::event::KeyEvent::new(code, crossterm::event::KeyModifiers::CONTROL)
}

#[test]
fn a_split_under_sixty_columns_is_one_pane_over_the_other_and_the_ratio_is_ignored() {
    let mut a = started(split(20));
    rows(&mut a, "left:rows", vec![row("l", &["LEFT"])]);
    rows(&mut a, "right:rows", vec![row("r", &["RIGHT"])]);
    let s = screen(&mut a, 59, 6);
    assert!(s[0].contains("LEFT") && !s[0].contains("RIGHT") && s.iter().any(|l| l.contains("RIGHT")), "{s:?}");
    assert!(!s.iter().any(|l| l.chars().skip(2).any(|c| c == '┃')), "no bar between panes that are not side by side (the mark of the selected row is the first thing on its line)");
    let wide = screen(&mut a, 60, 6);
    assert!(wide[0].contains("LEFT") && wide[0].contains("RIGHT"), "at 60 they are side by side: {wide:?}");
}

#[test]
fn the_bar_of_a_split_is_in_the_accent_colour_next_to_the_focused_pane_and_an_edge_otherwise() {
    let three = group("s", "", Present::Split, vec![list("a1"), list("a2"), list("a3")]);
    let mut a = started(three);
    let tone = |a: &mut App, at: usize| {
        let Snap { text, ansi, .. } = j4n_tui::headless::Headless::new(60, 3).snap(a);
        let _ = text;
        ansi.lines().next().map(String::from).unwrap_or_default().split('┃').count() + usize::from(at == 99)
    };
    assert_eq!(tone(&mut a, 0), 2, "one heavy bar next to the first pane");
    a.key(press(KeyCode::Right));
    assert_eq!(tone(&mut a, 1), 3, "both bars touch the middle pane");
    a.key(press(KeyCode::Right));
    assert_eq!(tone(&mut a, 2), 2, "one next to the last");
    let t = show(&mut a, 60, 3);
    assert!(t.lines().next().unwrap_or("").contains('│'), "a bar that touches no focused pane is a thin one: {t}");
}

fn grid(cols: u8, n: usize) -> Node {
    Node { cols, ..group("g", "", Present::Grid, (0..n).map(|i| list(&format!("c{i}"))).collect()) }
}

#[test]
fn a_grid_keeps_its_cells_in_order_and_the_arrows_move_by_cell_and_by_line() {
    let mut a = started(grid(2, 5));
    assert_eq!(page(&a), "c0");
    a.key(press(KeyCode::Right));
    assert_eq!(page(&a), "c1");
    a.key(press(KeyCode::Right));
    assert_eq!(page(&a), "c2", "right at the end of a line goes on to the next: cells keep order");
    a.key(press(KeyCode::Down));
    assert_eq!(page(&a), "c4");
    a.key(press(KeyCode::Down));
    assert_eq!(page(&a), "c4", "there is no cell below");
    a.key(press(KeyCode::Up));
    a.key(press(KeyCode::Up));
    assert_eq!(page(&a), "c0");
    a.key(press(KeyCode::Up));
    assert_eq!(page(&a), "c0");
    a.key(press(KeyCode::Left));
    assert_eq!(page(&a), "c0");
    let t = show(&mut a, 80, 12);
    assert!(t.contains('┼') && t.contains('│') && t.contains('─'), "joints, columns and rules:\n{t}");
    assert_eq!(nodes::grid_auto(59), 1);
    assert_eq!(nodes::grid_auto(60), 2);
    assert_eq!(nodes::grid_auto(160), 4);
}

#[test]
fn a_grid_with_no_cols_has_as_many_as_the_width_allows() {
    let mut a = started(grid(0, 4));
    let wide = show(&mut a, 160, 8);
    assert_eq!(wide.lines().next().unwrap_or("").matches('│').count(), 3, "four cells in a line at 160: {wide}");
    let narrow = show(&mut a, 59, 8);
    assert_eq!(narrow.lines().next().unwrap_or("").matches('│').count(), 0, "one in a line under 60: {narrow}");
}

#[test]
fn a_page_with_a_chat_keeps_the_arrows_for_the_draft_and_takes_ctrl_for_the_panes() {
    let two = group("s", "", Present::Split, vec![leaf("chat", "", FieldKind::Chat, "chat:main"), list("side")]);
    let mut a = started(two);
    assert_eq!(page(&a), "chat");
    a.key(press(KeyCode::Right));
    assert_eq!(page(&a), "chat");
    keys(&mut a, "hi");
    a.key(press(KeyCode::Left));
    assert_eq!(a.line.at, 1, "the arrow moved the cursor in the draft");
    a.key(ctrl_arrow(KeyCode::Right));
    assert_eq!(page(&a), "side");
}

#[test]
fn a_stack_goes_to_its_first_field_that_is_not_the_bar_or_the_questions() {
    let stack = group("s", "", Present::Stack, vec![leaf("bar", "", FieldKind::Bar, "bar:status"), leaf("asks", "", FieldKind::Ask, "asks:open"), list("main")]);
    let a = started(stack);
    assert_eq!(page(&a), "main");
    let only_bar = started(group("s", "", Present::Stack, vec![leaf("bar", "", FieldKind::Bar, "bar:status")]));
    assert_eq!(page(&only_bar), "s", "nothing to take the keys: the group is where it stops");
}

#[test]
fn the_screen_can_be_sent_to_a_node_in_a_split_a_grid_or_a_section_and_it_opens_the_way() {
    let root = group("root", "", Present::Tabs, vec![list("x"), group("sp", "", Present::Split, vec![list("p1"), list("p2")]), group("gr", "", Present::Grid, vec![list("g1"), list("g2")]), group("se", "", Present::Sections, vec![list("s1"), list("s2")])]);
    let mut a = started(root);
    a.down(Down::Focus { node: "p2".into() });
    assert_eq!(page(&a), "p2");
    a.down(Down::Focus { node: "g2".into() });
    assert_eq!(page(&a), "g2");
    a.shut.insert("s2".into());
    a.down(Down::Focus { node: "s2".into() });
    assert!(!a.shut.contains("s2"), "a section that is gone to is opened");
    assert_eq!(page(&a), "se");
    assert_eq!(a.sub.get("se"), Some(&1), "and its header is the one selected");
    a.down(Down::Focus { node: "x".into() });
    assert_eq!(page(&a), "x");
}

#[test]
fn a_lazy_node_is_asked_for_once_when_it_is_first_shown_and_its_children_replace_it() {
    let lazy = Node { lazy: true, ..group("later", "Later", Present::Stack, vec![]) };
    let root = group("root", "", Present::Tabs, vec![list("now"), lazy]);
    let mut a = started(root);
    assert!(opened(&a.key(press(KeyCode::Char('x')))).is_empty(), "a node that is not shown is not asked for");
    let ups = keys(&mut a, "2");
    assert_eq!(opened(&ups), ["later"]);
    assert!(screen(&mut a, 30, 4).iter().any(|l| l == "…"), "a mark that something is coming");
    let ups = keys(&mut a, "1");
    let ups2 = keys(&mut a, "2");
    assert!(opened(&ups).is_empty() && opened(&ups2).is_empty(), "not asked for twice");
    let part = group("later", "Later", Present::Stack, vec![list("l1")]);
    let ups = a.down(Down::Tree { node: part });
    assert_eq!(watched(&ups), ["l1:rows"], "its field is watched as soon as it is there");
    assert_eq!(page(&a), "l1");
    assert_eq!(a.tree.as_ref().map(|t| t.children.len()), Some(2), "the rest of the tree is as it was");
    // a node that comes back lazy is asked for again
    let again = Node { lazy: true, ..group("later", "Later", Present::Stack, vec![]) };
    let ups = a.down(Down::Tree { node: again });
    assert_eq!(opened(&ups), ["later"]);
}

#[test]
fn a_lazy_section_is_asked_for_when_it_is_opened_and_a_new_whole_tree_forgets_what_was_asked() {
    let lazy = Node { lazy: true, ..group("deep", "Deep", Present::Stack, vec![]) };
    let mut a = started(group("root", "", Present::Sections, vec![list("one"), lazy.clone()]));
    assert_eq!(a.sync().iter().filter(|u| matches!(u, Up::Intent { .. })).count(), 0, "asked already when the tree came");
    a.shut.insert("deep".into());
    let mut b = started(group("root", "", Present::Sections, vec![list("one"), lazy.clone()]));
    b.shut.insert("deep".into());
    b.asked.clear();
    assert!(opened(&b.sync()).is_empty(), "a folded section is not shown, so not asked for");
    b.shut.clear();
    assert_eq!(opened(&b.sync()), ["deep"]);
    let ups = b.down(Down::Tree { node: group("other-root", "", Present::Sections, vec![lazy]) });
    assert_eq!(opened(&ups), ["deep"], "a whole new tree is asked about again");
}

#[test]
fn a_tab_has_its_icon_its_title_and_its_count_and_a_narrow_screen_has_the_icons_only() {
    let tabs = group("root", "", Present::Tabs, vec![Node { icon: "ask".into(), count: Some(2), ..list("asks") }, Node { icon: "connector".into(), ..list("conn") }, list("plain")]);
    let mut a = started(tabs);
    let wide = screen(&mut a, 70, 4);
    assert!(wide[0].contains("\u{F0625} asks 2") && wide[0].contains("\u{F06A5} conn") && wide[0].contains("plain"), "{wide:?}");
    let narrow = screen(&mut a, 59, 4);
    assert!(narrow[0].contains("\u{F0625} 2") && !narrow[0].contains("asks") && !narrow[0].contains("conn") && narrow[0].contains("plain"), "an icon is enough, a tab with none keeps its title: {narrow:?}");
    let unicode = {
        j4n_tui::theme::Theme::build(&j4n_tui::theme::Settings { marks: "ascii".into(), ..Default::default() }, &[]).expect("theme")
    };
    a.theme = unicode;
    assert!(screen(&mut a, 70, 4)[0].contains("? asks 2"), "the icon is in the tier of the theme");
}

#[test]
fn the_key_of_the_open_tab_goes_on_to_its_next_sub_tab_round_the_end_and_a_tab_with_none_stays_put() {
    let mut a = started(sample());
    a.key(press(KeyCode::F(2)));
    assert_eq!(page(&a), "b1");
    a.key(press(KeyCode::F(2)));
    assert_eq!(page(&a), "b2", "the same key again: the next sub-tab");
    a.key(press(KeyCode::F(2)));
    assert_eq!(page(&a), "b1", "round the end");
    a.key(press(KeyCode::F(1)));
    assert_eq!(page(&a), "a");
    a.key(press(KeyCode::F(1)));
    assert_eq!(page(&a), "a", "a tab with no sub-tabs stays");
    a.key(press(KeyCode::F(3)));
    a.key(press(KeyCode::F(3)));
    assert_eq!(a.tab, 2, "sections are not sub-tabs");
    a.key(press(KeyCode::F(2)));
    assert_eq!(page(&a), "b1", "another tab's key goes to that tab");
}

#[test]
fn the_open_tab_is_underlined_heavy_and_the_rest_of_the_row_is_a_rule() {
    let mut a = started(sample());
    let s = screen(&mut a, 40, 6);
    let heavy = s[2].chars().filter(|c| *c == '━').count();
    assert_eq!(heavy, " a ".len(), "{s:?}");
    // the rule stands in by the margin, one cell each side
    assert!(s[2].starts_with(" ━━━") && s[2].chars().count() == 39 && s[2].ends_with('─'), "{s:?}");
    a.key(press(KeyCode::F(2)));
    let s = screen(&mut a, 40, 6);
    assert!(s[4].trim_start().starts_with("▔▔▔") && !s[4].contains('─'), "sub-tabs: the open one only: {s:?}");
    assert_eq!(s[3].split_whitespace().collect::<Vec<_>>(), ["b1", "b2"], "{s:?}");
}

#[test]
fn tabs_that_do_not_fit_are_cut_from_the_start_so_the_open_one_is_in_sight() {
    let many = group("root", "", Present::Tabs, (0..12).map(|i| list(&format!("tab-number-{i}"))).collect());
    let mut a = started(many);
    for _ in 0..11 {
        a.key(press(KeyCode::Char(']')));
    }
    let s = screen(&mut a, 50, 4);
    assert!(s[0].contains("tab-number-11") && !s[0].contains("tab-number-0 "), "{s:?}");
    let first = {
        a.key(press(KeyCode::Char(']')));
        screen(&mut a, 50, 4)
    };
    assert!(first[0].contains("tab-number-0"), "back at the start: {first:?}");
    // the tabs after the open one that do not fit are left out, and the name of the core is not drawn over what is left
    assert!(!first[0].contains("tab-number-11") && first[0].ends_with(" j4n 0.0.0"), "{first:?}");
    assert!(first[0].contains("tab-number-1 "), "those that fit are there: {first:?}");
}

#[test]
fn the_name_of_the_core_is_at_the_right_of_the_top_tabs_and_gives_way_when_there_is_no_room() {
    let mut a = started(group("root", "", Present::Tabs, vec![list("a")]));
    assert!(screen(&mut a, 40, 3)[0].ends_with("j4n 0.0.0"), "{:?}", screen(&mut a, 40, 3));
    let tiny = screen(&mut a, 8, 3);
    assert!(tiny[0].contains('a'), "{tiny:?}");
}

#[test]
fn a_screen_of_one_row_or_none_does_not_fail() {
    let mut a = started(sample());
    for (w, h) in [(1, 1), (5, 1), (80, 1), (80, 2), (3, 3), (200, 3)] {
        let _ = show(&mut a, w, h);
    }
    a.key(press(KeyCode::F(3)));
    for (w, h) in [(1, 1), (5, 2), (80, 3), (20, 4)] {
        let _ = show(&mut a, w, h);
    }
    let mut g = started(grid(3, 7));
    let mut s = started(split(99));
    for (w, h) in [(1, 1), (60, 1), (61, 2), (200, 3)] {
        let _ = show(&mut g, w, h);
        let _ = show(&mut s, w, h);
    }
}

// ---- the jump ----

fn big() -> Node {
    let settings = group("settings", "Settings", Present::Subtabs, vec![group("look", "look", Present::Sections, vec![list("palette"), list("marks")]), list("keys")]);
    group("root", "", Present::Tabs, vec![list("tasks"), Node { title: "Models".into(), ..list("models") }, settings])
}

#[test]
fn the_jump_opens_with_the_titles_in_tree_order_narrows_as_letters_are_typed_and_enter_goes_there() {
    let mut a = started(big());
    assert!(a.jump.is_none());
    a.key(ctrl('k'));
    let j = a.jump.as_ref().expect("open");
    assert_eq!(j.hits.iter().map(|h| h.title.as_str()).collect::<Vec<_>>(), ["tasks", "Models", "Settings", "look", "keys", "palette", "marks"], "no letters yet: the tree as it stands, a parent's children together");
    keys(&mut a, "pal");
    let j = a.jump.as_ref().expect("open");
    assert_eq!(j.line.buf, "pal");
    assert_eq!(j.hits.len(), 1);
    assert_eq!((j.hits[0].id.as_str(), j.hits[0].above.as_str()), ("palette", "Settings › look"));
    let s = show(&mut a, 80, 14);
    assert!(s.contains("pal") && s.contains("palette") && s.contains("Settings › look"), "{s}");
    a.key(press(KeyCode::Backspace));
    assert_eq!(a.jump.as_ref().map(|j| j.line.buf.as_str()), Some("pa"));
    a.key(press(KeyCode::Backspace));
    a.key(press(KeyCode::Backspace));
    a.key(press(KeyCode::Backspace));
    assert_eq!(a.jump.as_ref().map(|j| j.hits.len()), Some(7), "nothing typed again");
    keys(&mut a, "mar");
    let ups = a.key(press(KeyCode::Enter));
    assert!(a.jump.is_none(), "enter closes it");
    assert_eq!(page(&a), "look", "the sections group of `marks` has the keys");
    assert_eq!(a.sub.get("look"), Some(&1), "and its header is the one selected");
    assert_eq!(watched(&ups), ["marks:rows", "palette:rows"], "the sections are on the screen now");
}

#[test]
fn the_jump_keeps_the_keys_while_it_is_open_and_escape_and_ctrl_k_close_it() {
    let mut a = started(group("root", "", Present::Tabs, vec![leaf("chat", "Chat", FieldKind::Chat, "chat:main"), list("other")]));
    a.key(ctrl('k'));
    keys(&mut a, "ot");
    assert_eq!(a.line.buf, "", "the letters went to the jump, not the draft");
    a.key(press(KeyCode::Esc));
    assert!(a.jump.is_none() && page(&a) == "chat", "escape leaves where it was");
    a.key(ctrl('k'));
    a.key(ctrl('k'));
    assert!(a.jump.is_none(), "ctrl-k again closes it");
    a.key(ctrl('k'));
    a.key(press(KeyCode::Enter));
    assert_eq!(page(&a), "chat", "the first match is Chat itself");
    a.key(ctrl('k'));
    keys(&mut a, "zzz");
    assert_eq!(a.jump.as_ref().map(|j| j.hits.len()), Some(0));
    assert!(a.key(press(KeyCode::Enter)).is_empty() && a.jump.is_none(), "enter on nothing just closes it");
}

#[test]
fn the_arrows_choose_among_the_matches_and_the_jump_does_not_open_without_a_tree_or_in_an_edit() {
    let mut a = started(big());
    a.key(ctrl('k'));
    keys(&mut a, "s");
    a.key(press(KeyCode::Down));
    assert_eq!(a.jump.as_ref().map(|j| j.sel.at as usize), Some(1));
    a.key(press(KeyCode::Up));
    a.key(press(KeyCode::Up));
    assert_eq!(a.jump.as_ref().map(|j| j.sel.at as usize), Some(0), "up stops at the first");
    for _ in 0..30 {
        a.key(press(KeyCode::Down));
    }
    assert_eq!(a.jump.as_ref().map(|j| j.sel.at as usize), a.jump.as_ref().map(|j| j.hits.len() - 1), "down stops at the last");
    let mut empty = App::new();
    empty.key(ctrl('k'));
    assert_eq!(empty.jump.as_ref().map(|j| j.hits.len()), Some(0), "no tree, nothing to find, and nothing breaks");
    let _ = show(&mut empty, 40, 6);
}

// ---- the search ----

fn q(s: &str) -> Vec<char> {
    s.chars().collect()
}

#[test]
fn letters_that_are_not_all_there_in_order_do_not_match_and_better_matches_score_higher() {
    assert_eq!(fuzzy::score(&q("xyz"), "Settings"), None);
    assert_eq!(fuzzy::score(&q("st"), "ts"), None, "in order");
    assert_eq!(fuzzy::score(&q(""), "anything"), Some(0));
    let s = |query: &str, text: &str| fuzzy::score(&q(&query.to_lowercase()), text).expect(text);
    assert!(s("set", "Settings") > s("set", "a stuffed toy"), "letters that follow each other beat letters far apart");
    assert!(s("sg", "Settings gates") > s("sg", "Seasonal tiger"), "the start of a word counts");
    assert!(s("a", "ab") > s("a", "ba"), "a match at the start counts");
    assert!(s("log", "Log") > s("log", "Log of everything that ever happened"), "a shorter title is nearer");
    assert_eq!(s("SET", "settings"), s("set", "SETTINGS"), "case does not matter");
    assert!(s("ab", "a-b") > s("ab", "axxb"), "a word start after a dash");
    assert_eq!(fuzzy::positions(&q("sg"), "Settings gates"), [0, 6]);
    assert_eq!(fuzzy::positions(&q("xyz"), "abc"), Vec::<usize>::new());
    assert_eq!(fuzzy::positions(&q("ab"), "AaBb"), [0, 2]);
}

#[test]
fn a_search_keeps_the_best_few_best_first_and_a_tie_goes_to_the_earlier_node() {
    let kids: Vec<Node> = (0..40).map(|i| Node { id: format!("n{i}"), title: format!("item {i}"), ..Node::default() }).collect();
    let root = group("root", "", Present::Stack, kids);
    let hits = fuzzy::search(&root, "item");
    assert_eq!(hits.len(), fuzzy::KEEP);
    assert_eq!(hits[0].id, "n0");
    assert_eq!(hits[1].id, "n1", "the same score: the order of the tree");
    let best = Node { id: "best".into(), title: "item".into(), ..Node::default() };
    let mut kids: Vec<Node> = (0..30).map(|i| Node { id: format!("n{i}"), title: format!("an item number {i}"), ..Node::default() }).collect();
    kids.push(best);
    let hits = fuzzy::search(&group("root", "", Present::Stack, kids), "item");
    assert_eq!(hits[0].id, "best", "found at the end of the tree, it is first");
    let hits = fuzzy::search(&group("root", "", Present::Stack, vec![Node { id: "untitled".into(), ..Node::default() }]), "");
    assert!(hits.is_empty(), "a node with no title is not something to jump to");
    let deep = group("r", "R", Present::Stack, vec![group("m", "Mid", Present::Stack, vec![Node { id: "leaf".into(), title: "Leaf".into(), ..Node::default() }])]);
    let hit = &fuzzy::search(&deep, "leaf")[0];
    assert_eq!((hit.id.as_str(), hit.above.as_str()), ("leaf", "R › Mid"));
}

#[test]
fn a_tab_that_is_a_group_is_listed_by_its_own_page_under_the_group_and_a_page_without_a_mark_takes_the_tab_s() {
    // `Tasks` then `Tasks · Tasks`, and sub-tabs without a mark
    let own = Node { id: "tasks.own".into(), title: "Tasks".into(), icon: "tasks".into(), ..Node::default() };
    let asks = Node { id: "asks".into(), title: "Asks".into(), icon: "ask".into(), ..Node::default() };
    let plain = Node { id: "agents".into(), title: "Agents".into(), ..Node::default() };
    let tasks = Node { icon: "tasks".into(), ..group("tasks", "Tasks", Present::Tabs, vec![own, asks]) };
    let conn = Node { icon: "connector".into(), ..group("connectors", "Connectors", Present::Tabs, vec![plain]) };
    let hits = fuzzy::search(&group("root", "", Present::Tabs, vec![tasks, conn]), "");
    let seen: Vec<(&str, &str, &str)> = hits.iter().map(|h| (h.id.as_str(), h.above.as_str(), h.icon.as_str())).collect();
    assert_eq!(seen, [("tasks.own", "Tasks", "tasks"), ("asks", "Tasks", "ask"), ("connectors", "", "connector"), ("agents", "Connectors", "connector")]);
}

/// The palette as the core sends it: the words, what follows them, what it does; a command that takes nothing more has the key that says it.
fn palette() -> Vec<j4n_proto::Row> {
    let mut pack = row("pack", &[":pack :packs", "", "List the packs."]);
    pack.actions = vec![j4n_proto::Act { key: "enter".into(), label: "run".into(), intent: Intent::Say { text: ":pack".into(), with: Vec::new() }, risk: 0 }];
    vec![pack, row("remember", &[":remember :note", "<the note>", "Keep a note in the memory."])]
}

#[test]
fn the_jump_lists_the_commands_beside_the_nodes_and_enter_says_one_in_the_chat() {
    let chat = leaf("chat", "Chat", FieldKind::Chat, "chat:main");
    let mut a = started(group("root", "", Present::Tabs, vec![chat, list("tasks")]));
    rows(&mut a, "commands:all", palette());
    keys(&mut a, "");
    a.key(press(KeyCode::F(2)));
    assert_eq!(page(&a), "tasks");
    a.key(ctrl('k'));
    keys(&mut a, ":pack");
    let hits = &a.jump.as_ref().expect("open").hits;
    assert_eq!((hits[0].title.as_str(), hits[0].command, hits[0].above.as_str()), (":pack :packs", true, "List the packs."), "{hits:?}");
    let s = show(&mut a, 80, 10);
    assert!(s.contains(":pack :packs") && s.contains("List the packs."), "{s}");
    let ups = a.key(press(KeyCode::Enter));
    assert_eq!(page(&a), "chat", "a command is said where the answer comes");
    assert!(ups.contains(&Up::Intent { intent: Intent::Say { text: ":pack".into(), with: Vec::new() } }), "{ups:?}");
    assert!(a.jump.is_none() && a.line.buf.is_empty());
}

#[test]
fn a_command_that_takes_more_leaves_its_word_in_the_composer_to_be_finished() {
    let chat = leaf("chat", "Chat", FieldKind::Chat, "chat:main");
    let mut a = started(group("root", "", Present::Tabs, vec![chat, list("tasks")]));
    rows(&mut a, "commands:all", palette());
    a.key(press(KeyCode::F(2)));
    a.key(ctrl('k'));
    keys(&mut a, "remem");
    let ups = a.key(press(KeyCode::Enter));
    assert!(!ups.iter().any(|u| matches!(u, Up::Intent { intent: Intent::Say { .. } })), "nothing is said yet: {ups:?}");
    assert_eq!((page(&a).as_str(), a.line.buf.as_str()), ("chat", ":remember "), "the first of its words");
    assert_eq!(a.line.at, 10);
    let hits = fuzzy::search_with(&group("root", "", Present::Stack, Vec::new()), &palette(), ":remember");
    assert_eq!((hits[0].title.as_str(), hits[0].above.as_str()), (":remember :note", "<the note>  Keep a note in the memory."), "what follows the words, then what it does");
}

#[test]
fn a_search_among_nodes_and_commands_keeps_the_best_few_and_a_node_goes_before_a_command_of_the_same_score() {
    let root = group("root", "", Present::Stack, vec![Node { id: "packs".into(), title: ":pack".into(), ..Node::default() }]);
    let hits = fuzzy::search_with(&root, &palette(), ":pack");
    assert_eq!(hits.iter().map(|h| (h.id.as_str(), h.command)).collect::<Vec<_>>(), [("packs", false), ("pack", true)], "{hits:?}");
    let many: Vec<j4n_proto::Row> = (0..40).map(|i| row(&format!("c{i}"), &[&format!(":cmd{i}"), "", ""])).collect();
    assert_eq!(fuzzy::search_with(&root, &many, ":cmd").len(), fuzzy::KEEP);
    assert!(fuzzy::search_with(&root, &many, "zzz").is_empty());
    assert!(!fuzzy::search_with(&root, &many, ":cm d1").is_empty(), "a space typed in the query is not a letter to find");
}

// ---- 1,000,000 nodes ----

/// 10 tabs × 10 sub-tabs × 100 sections × 100 fields.
fn million() -> Node {
    let tab = |t: usize| {
        let subs = (0..10).map(|s| {
            let sections = (0..100).map(|c| {
                let fields = (0..100).map(|f| Node { id: format!("f{t}.{s}.{c}.{f}"), title: format!("field {f}"), ..Node::default() }).collect();
                Node { id: format!("c{t}.{s}.{c}"), title: format!("section {c}"), present: Present::Stack, children: fields, ..Node::default() }
            });
            Node { id: format!("s{t}.{s}"), title: format!("sub {s}"), present: Present::Sections, children: sections.collect(), ..Node::default() }
        });
        Node { id: format!("t{t}"), title: format!("tab {t}"), present: Present::Subtabs, children: subs.collect(), ..Node::default() }
    };
    group("root", "", Present::Tabs, (0..10).map(tab).collect())
}

#[test]
fn a_million_nodes_open_draw_move_and_search_without_a_wait() {
    // each number is the CPU the thread spent, and is held against what the same machine takes for a fixed piece of work: a gate that
    // shares the machine makes a second longer, never the work
    use crate::cpu::of;
    let root = million();
    let mut a = App::new();
    a.start();
    a.down(hello());
    let ((), open) = of(|| drop(a.down(Down::Tree { node: root })));
    let mut screen = j4n_tui::headless::Headless::new(160, 48);
    let (_, first) = of(|| screen.show(&mut a));
    let ((), moves) = of(|| {
        for _ in 0..20 {
            a.key(press(KeyCode::Down));
            a.key(press(KeyCode::Char('}')));
            let _ = screen.show(&mut a);
        }
    });
    let moves = moves / 20;
    let ((), search) = of(|| {
        a.key(ctrl('k'));
        keys(&mut a, "sec 9");
    });
    let _ = screen.show(&mut a);
    assert!(a.jump.as_ref().is_some_and(|j| j.hits.len() == fuzzy::KEEP));
    // generous for a debug build: the numbers that matter are those of the release bench; held as a quiet machine would take them
    let within = |ms: u64, what: &str, took: std::time::Duration| crate::cpu::assert_within(std::time::Duration::from_millis(ms), what, || took);
    within(3000, "open", open);
    within(500, "first frame", first);
    within(500, "key and frame", moves);
    within(5000, "five letters over a million titles", search);
    eprintln!("1M nodes: open {open:?}, first frame {first:?}, key+frame {moves:?}, five letters {search:?}");
}

#[test]
fn the_top_tabs_are_the_first_tabs_not_the_first_sub_tabs_and_a_lazy_node_with_children_is_not_asked_for() {
    let root = group("root", "", Present::Stack, vec![group("subs", "", Present::Subtabs, vec![list("x1"), list("x2")]), group("tabs", "", Present::Tabs, vec![list("t1"), list("t2")])]);
    let mut a = started(root);
    assert_eq!(a.top().map(|t| t.id.as_str()), Some("tabs"));
    a.key(press(KeyCode::F(2)));
    assert_eq!(a.tab, 1, "F2 moves the top tabs");
    let partly = Node { lazy: true, ..group("p", "P", Present::Stack, vec![list("kid")]) };
    let b = started(group("root", "", Present::Tabs, vec![partly]));
    assert_eq!(b.asked.len(), 0, "a node that has its children is not asked for, whatever it says");
}

#[test]
fn the_selection_never_goes_past_the_ends_of_the_sections_the_panes_the_cells_or_the_tabs() {
    let mut a = started(sample());
    a.key(press(KeyCode::F(3)));
    for _ in 0..9 {
        a.key(press(KeyCode::Down));
    }
    assert_eq!(a.sub.get("c"), Some(&2), "the last header, and no further");
    keys(&mut a, "9");
    keys(&mut a, "4");
    assert_eq!(a.tab, 2, "a tab that is not there is not gone to");
    let mut s = started(split(50));
    for _ in 0..3 {
        s.key(press(KeyCode::Right));
    }
    assert_eq!(s.sub.get("s"), Some(&1));
    let mut g = started(grid(2, 3));
    for _ in 0..4 {
        g.key(press(KeyCode::Down));
    }
    assert_eq!(g.sub.get("g"), Some(&2), "the third cell is the last one down: {:?}", g.sub);
    g.key(press(KeyCode::Right));
    assert_eq!(g.sub.get("g"), Some(&2));
}

#[test]
fn the_name_of_the_core_is_left_out_when_it_would_take_more_than_half_the_row() {
    let mut a = started(group("root", "", Present::Tabs, vec![list("a")]));
    assert!(!screen(&mut a, 14, 3)[0].contains("j4n"), "{:?}", screen(&mut a, 14, 3));
    assert!(screen(&mut a, 24, 3)[0].contains("j4n"));
}

#[test]
fn many_sections_scroll_so_the_selected_header_is_in_sight() {
    let many = group("root", "", Present::Sections, (0..40).map(|i| Node { title: format!("section {i}"), ..list(&format!("s{i}")) }).collect());
    let mut a = started(many);
    for _ in 0..30 {
        a.key(press(KeyCode::Down));
    }
    let s = show(&mut a, 30, 10);
    assert!(s.contains("section 30") && !s.contains("section 0\n"), "{s}");
}

#[test]
fn a_new_letter_in_the_jump_starts_again_at_the_first_match() {
    let mut a = started(big());
    a.key(ctrl('k'));
    a.key(press(KeyCode::Down));
    a.key(press(KeyCode::Down));
    assert_eq!(a.jump.as_ref().map(|j| j.sel.at as usize), Some(2));
    keys(&mut a, "s");
    assert_eq!(a.jump.as_ref().map(|j| j.sel.at as usize), Some(0));
}

#[test]
fn the_room_the_name_takes_is_taken_from_the_tabs_only_when_the_name_is_drawn() {
    let mut a = started(group("root", "", Present::Tabs, vec![list("aaa"), list("bbb"), list("ccc")]));
    a.key(press(KeyCode::F(3)));
    let s = screen(&mut a, 14, 3);
    assert!(s[0].contains("bbb") && s[0].contains("ccc"), "no name, so both fit: {s:?}");
    let mut g = started(grid(2, 3));
    g.key(press(KeyCode::Right));
    g.key(press(KeyCode::Down));
    assert_eq!(g.sub.get("g"), Some(&1), "the cell under the second is not there");
}

fn many(id: &str, n: usize) -> Vec<j4n_proto::Row> {
    (0..n).map(|i| row(&format!("{id}{i}"), &[&format!("{id}-{i}")])).collect()
}

fn sized(id: &str, grow: u8, height: u8) -> Node {
    Node { grow, height, ..list(id) }
}

#[test]
fn a_part_that_says_its_height_has_it_and_the_rest_share_what_is_left_by_their_grow() {
    let mut a = started(group("s", "", Present::Stack, vec![sized("a", 0, 3), sized("b", 1, 0), sized("c", 3, 0)]));
    for id in ["a", "b", "c"] {
        rows(&mut a, &format!("{id}:rows"), many(id, 30));
    }
    let s = screen(&mut a, 40, 23);
    let count = |p: &str| s.iter().filter(|l| l.contains(p)).count();
    assert_eq!(count("a-"), 3, "{s:?}");
    assert_eq!((count("b-"), count("c-")), (5, 15), "the other twenty rows go one to three: {s:?}");
}

#[test]
fn a_height_wins_over_a_grow_and_a_part_that_says_nothing_is_sized_by_what_it_shows() {
    let mut a = started(group("s", "", Present::Stack, vec![sized("a", 5, 2), leaf("bar", "", FieldKind::Bar, "bar:status"), sized("c", 1, 0)]));
    for id in ["a", "c"] {
        rows(&mut a, &format!("{id}:rows"), many(id, 30));
    }
    let s = screen(&mut a, 40, 10);
    let count = |p: &str| s.iter().filter(|l| l.contains(p)).count();
    assert_eq!((count("a-"), count("c-")), (2, 7), "{s:?}");
}

#[test]
fn a_height_is_the_rows_a_part_has_whatever_it_shows() {
    let mut a = started(group("s", "", Present::Stack, vec![Node { height: 6, ..leaf("asks", "", FieldKind::Ask, "asks:open") }, list("c")]));
    rows(&mut a, "c:rows", many("c", 30));
    let s = screen(&mut a, 40, 10);
    let count = |p: &str| s.iter().filter(|l| l.contains(p)).count();
    assert_eq!(count("c-"), 4, "six of the ten rows are the ask field's, with nothing to ask: {s:?}");
}
