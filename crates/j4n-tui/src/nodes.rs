//! The node tree: how the children of a node are shown (by its `present`, or by depth for `auto`), which child
//! is open or has the focus, what is on the screen, and drawing each presentation. The tree is data the core sent; the state of it
//! (the tab each group has open, the sections folded, the nodes asked for) is the screen's.

use crate::app::App;
use crate::fuzzy;
use crate::parts::{line::Typed, scroll::{keep, keep_by}, select::Sel};
use crate::theme::{Density, Theme};
use crate::theme::glyph::{Slot, Tier};
use crate::theme::palette::Role as Hue;
use crate::wrap::fit;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use j4n_proto::{FieldKind, Intent, Node, PALETTE, Present, Tone, Up};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

/// Below this many columns a split is stacked and a label sits above its value (`narrow.at`).
pub const NARROW: u16 = 60;

/// How a node at `depth` (the root is 0) shows its children: what it says, or for `auto` what its depth says —
/// tabs, then sub-tabs, then sections, then a stack of fields.
pub fn how(n: &Node, depth: usize) -> Present {
    match (n.present, depth) {
        (Present::Auto, 0) => Present::Tabs,
        (Present::Auto, 1) => Present::Subtabs,
        (Present::Auto, 2) => Present::Sections,
        (Present::Auto, _) => Present::Stack,
        (p, _) => p,
    }
}

pub fn tabbed(p: Present) -> bool {
    matches!(p, Present::Tabs | Present::Subtabs)
}

/// The path from the root down to the first group of top tabs, as the position of each child, if there is one.
pub fn find_top(root: &Node) -> Option<Vec<usize>> {
    fn go(n: &Node, d: usize, path: &mut Vec<usize>) -> bool {
        if how(n, d) == Present::Tabs {
            return true;
        }
        for (i, c) in n.children.iter().enumerate() {
            path.push(i);
            if go(c, d + 1, path) {
                return true;
            }
            path.pop();
        }
        false
    }
    let mut path = Vec::new();
    go(root, 0, &mut path).then_some(path)
}

fn at<'a>(root: &'a Node, path: &[usize]) -> Option<&'a Node> {
    path.iter().try_fold(root, |n, i| n.children.get(*i))
}

/// A value field that can be changed: the keys can go to it.
pub fn focusable(n: &Node) -> bool {
    let button = n.field.as_ref().filter(|f| f.kind == FieldKind::Action);
    crate::fields::Spec::of(n).is_some_and(|s| !s.disabled()) || button.is_some_and(|f| !crate::fields::Spec::new(&n.title, f).disabled())
}

/// A field the keys go to when a stack has to choose one: not the bar, not the questions and not a chart (it takes no key).
fn interactive(n: &Node) -> bool {
    n.field.as_ref().is_none_or(|f| !matches!(f.kind, FieldKind::Bar | FieldKind::Ask | FieldKind::Line | FieldKind::Bars) && ((!f.kind.is_value() && f.kind != FieldKind::Action) || focusable(n)))
}

impl App {
    pub(crate) fn root(&self) -> Option<&Node> {
        self.tree.as_deref()
    }

    /// The group of top tabs.
    pub fn top(&self) -> Option<&Node> {
        at(self.root()?, self.top.as_deref()?)
    }

    /// The child of `n` (at `depth`) that is open when its children are shown one at a time, or has the focus in a split, a grid or
    /// a sections group. The top tabs go by `tab`, every other group by its own entry in `sub`.
    pub fn open_of<'a>(&self, n: &'a Node) -> Option<&'a Node> {
        let top = self.top().is_some_and(|t| t.id == n.id);
        let at = if top { self.tab } else { self.sub.get(&n.id).copied().unwrap_or(0) };
        n.children.get(at.min(n.children.len().saturating_sub(1)))
    }

    /// The nodes from the root to the one that has the keys, each with its depth: tabs and sub-tabs go to the open child, a split or a
    /// grid to the focused one, a stack to its first field that is not the bar or the questions. A sections group is where it stops:
    /// its headers are what the keys move.
    pub fn trail(&self) -> Vec<(&Node, usize)> {
        let mut out = Vec::new();
        let Some(mut n) = self.root() else { return out };
        let mut d = 0;
        loop {
            out.push((n, d));
            if n.field.is_some() || n.children.is_empty() {
                break;
            }
            let next = match how(n, d) {
                Present::Tabs | Present::Subtabs | Present::Split | Present::Grid => self.open_of(n),
                // the value field the keys are on, when there are any
                Present::Stack if n.children.iter().any(focusable) => {
                    let vals: Vec<&Node> = n.children.iter().filter(|c| focusable(c)).collect();
                    vals.get(self.sub.get(&n.id).copied().unwrap_or(0).min(vals.len() - 1)).copied()
                }
                Present::Stack => n.children.iter().find(|c| interactive(c)),
                // the keys went into the body of the open section
                Present::Sections if self.inside.contains(&n.id) => self.open_of(n),
                _ => None,
            };
            let Some(next) = next else { break };
            (n, d) = (next, d + 1);
        }
        out
    }

    /// The node that has the keys.
    pub fn page(&self) -> Option<&Node> {
        self.trail().last().map(|(n, _)| *n)
    }

    /// The field of the page that has the keys, when it is one.
    pub fn field(&self) -> Option<&j4n_proto::Field> {
        self.page().and_then(|p| p.field.as_ref())
    }

    /// The group of tabs or sub-tabs that is the deepest on the open path, unless it is the top.
    pub(crate) fn inner_tabs(&self) -> Option<&Node> {
        let top = self.top().map(|t| t.id.as_str());
        self.trail().into_iter().rev().find(|(n, d)| tabbed(how(n, *d)) && Some(n.id.as_str()) != top).map(|(n, _)| n)
    }

    /// The group of sub-tabs that is the deepest on the open path.
    fn subtabs(&self) -> Option<&Node> {
        self.trail().into_iter().rev().find(|(n, d)| how(n, *d) == Present::Subtabs).map(|(n, _)| n)
    }

    /// Go to the node called `id`: the tab, side, cell or section on the way to it is opened.
    pub(crate) fn focus(&mut self, id: &str) -> Vec<Up> {
        fn path<'a>(n: &'a Node, id: &str, out: &mut Vec<&'a Node>) -> bool {
            out.push(n);
            if n.id == id || n.children.iter().any(|c| path(c, id, out)) {
                return true;
            }
            out.pop();
            false
        }
        let Some(root) = self.tree.clone() else { return Vec::new() };
        let mut on = Vec::new();
        if !path(&root, id, &mut on) {
            return Vec::new();
        }
        let top = self.top().map(|t| t.id.clone());
        for (depth, pair) in on.windows(2).enumerate() {
            let p = how(pair[0], depth);
            if p == Present::Stack && focusable(pair[1]) {
                let at = pair[0].children.iter().filter(|c| focusable(c)).position(|c| c.id == pair[1].id).unwrap_or(0);
                self.sub.insert(pair[0].id.clone(), at);
                continue;
            }
            if !matches!(p, Present::Tabs | Present::Subtabs | Present::Split | Present::Grid | Present::Sections) {
                continue;
            }
            let i = pair[0].children.iter().position(|c| c.id == pair[1].id).unwrap_or(0);
            if Some(&pair[0].id) == top.as_ref() {
                self.tab = i;
            } else {
                self.sub.insert(pair[0].id.clone(), i);
            }
            if p == Present::Sections {
                self.shut.remove(&pair[1].id);
                // a node inside the section: the keys go in, to it
                if pair[1].id != id {
                    self.inside.insert(pair[0].id.clone());
                } else {
                    self.inside.remove(&pair[0].id);
                }
            }
        }
        self.sync()
    }

    /// Every node that is on the screen now: the open child of tabs, sub-tabs, a split's panes, a grid's cells, a stack's children, and
    /// the children of the sections that are not folded.
    pub(crate) fn shown<'a>(&'a self, n: &'a Node, d: usize, f: &mut impl FnMut(&'a Node)) {
        f(n);
        match how(n, d) {
            Present::Tabs | Present::Subtabs => {
                if let Some(c) = self.open_of(n) {
                    self.shown(c, d + 1, f);
                }
            }
            Present::Sections => {
                for c in &n.children {
                    // a folded section is its header, which the group draws
                    if !self.shut.contains(&c.id) {
                        self.shown(c, d + 1, f);
                    }
                }
            }
            _ => n.children.iter().for_each(|c| self.shown(c, d + 1, f)),
        }
    }

    /// The bindings that are needed now: the field of every node that is shown, and the chat wherever it is (so what was said while
    /// another tab was open is there).
    pub fn needed(&self) -> std::collections::BTreeSet<String> {
        let mut out = self.chats.clone();
        if let Some(root) = self.root() {
            self.shown(root, 0, &mut |n| {
                if let Some(f) = &n.field {
                    out.insert(f.bind.clone());
                    // a Log draws its events in the look file's look
                    if f.kind == j4n_proto::FieldKind::Log {
                        out.insert(j4n_proto::LOOK_FILE.to_string());
                    }
                }
            });
        }
        // the commands, which the jump lists beside the nodes
        if self.tree.is_some() {
            out.insert(PALETTE.to_string());
        }
        // the providers a held line may be kept as the key of (`chat::held`), while it is
        if self.held.is_some() {
            out.insert(crate::chat::held::PROVIDERS.to_string());
        }
        // the card and the switch of the composer, and what Tab completes, wherever a chat is
        if out.iter().any(|b| b.starts_with("chat:")) {
            out.extend([j4n_proto::SHAPE_CARD, j4n_proto::SHAPE_ON, j4n_proto::COMPOSER_STRIP, j4n_proto::COMPOSER_BIND, j4n_proto::TERMS, j4n_proto::CHATS, j4n_proto::CHAT_HERE, j4n_proto::COMPLETE].map(String::from));
        }
        out
    }

    /// Ask the core for the children of every shown node that has none yet, once each.
    fn lazy(&mut self) -> Vec<Up> {
        let mut want = Vec::new();
        if let Some(root) = self.root() {
            self.shown(root, 0, &mut |n| {
                if n.lazy && n.children.is_empty() && !self.asked.contains(&n.id) {
                    want.push(n.id.clone());
                }
            });
        }
        want.into_iter()
            .map(|node| {
                self.asked.insert(node.clone());
                Up::Intent { intent: Intent::Open { node } }
            })
            .collect()
    }

    pub fn sync(&mut self) -> Vec<Up> {
        let need = self.needed();
        let mut ups: Vec<Up> = self.watched.difference(&need).map(|b| Up::Unwatch { bind: b.clone() }).collect();
        ups.extend(need.difference(&self.watched).map(|b| Up::Watch { bind: b.clone() }));
        self.watched = need;
        ups.extend(self.lazy());
        self.stamp(&ups);
        ups.extend(self.windows());
        ups
    }

    /// A tree arrived: the whole of it, or the part under a node the screen asked for.
    pub(crate) fn plant(&mut self, node: Node) {
        fn put(n: &mut Node, new: &Node) -> bool {
            if n.id == new.id {
                *n = new.clone();
                return true;
            }
            n.children.iter_mut().any(|c| put(c, new))
        }
        // the tab and the sub-tabs that are open, by id: a tree sent again with a tab more or less (a setting's `when`) keeps them open
        fn find<'a>(n: &'a Node, id: &str) -> Option<&'a Node> {
            if n.id == id {
                return Some(n);
            }
            n.children.iter().find_map(|c| find(c, id))
        }
        let open_tab = self.top().and_then(|t| self.open_of(t)).map(|c| c.id.clone());
        let open_subs: Vec<(String, String)> = self.root().map(|r| self.sub.iter().filter_map(|(g, at)| Some((g.clone(), find(r, g)?.children.get(*at)?.id.clone()))).collect()).unwrap_or_default();
        self.asked.remove(&node.id);
        let part = self.tree.as_mut().is_some_and(|t| t.id != node.id && put(std::sync::Arc::make_mut(t), &node));
        if !part {
            self.asked.clear();
            self.tree = Some(std::sync::Arc::new(node));
        }
        self.top = self.root().and_then(find_top);
        if let Some(at) = open_tab.and_then(|id| self.top().and_then(|t| t.children.iter().position(|c| c.id == id))) {
            self.tab = at;
        }
        for (g, id) in open_subs {
            if let Some(at) = self.root().and_then(|r| find(r, &g)).and_then(|n| n.children.iter().position(|c| c.id == id)) {
                self.sub.insert(g, at);
            }
        }
        // the chats, wherever they are: found once here, not on every key
        fn chats(n: &Node, out: &mut std::collections::BTreeSet<String>) {
            if let Some(f) = n.field.as_ref().filter(|f| f.kind == FieldKind::Chat) {
                out.insert(f.bind.clone());
            }
            n.children.iter().for_each(|c| chats(c, out));
        }
        let mut found = std::collections::BTreeSet::new();
        if let Some(root) = self.root() {
            chats(root, &mut found);
        }
        self.chats = found;
    }

    fn step(&mut self, id: String, n: usize, forward: bool) -> Vec<Up> {
        if n == 0 {
            return Vec::new();
        }
        let top = self.top().is_some_and(|t| t.id == id);
        let now = if top { self.tab } else { self.sub.get(&id).copied().unwrap_or(0) }.min(n - 1);
        let next = if forward { (now + 1) % n } else { (now + n - 1) % n };
        if top {
            self.tab = next;
        } else {
            self.sub.insert(id, next);
        }
        self.sync()
    }

    // ---- keys ----

    /// The keys of the groups, for a page that is not a chat: `1–9` and `[ ]` for the top tabs, `{ }` for the sub-tabs, `←→↑↓` for the
    /// panes of a split and the cells of a grid, `↑↓ ⏎ space z` for the headers of sections. `None` when the key is not one of them.
    pub(crate) fn node_key(&mut self, k: KeyEvent, field_is_text: bool) -> Option<Vec<Up>> {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let plain = !ctrl && !k.modifiers.contains(KeyModifiers::ALT);
        let trail: Vec<(String, Present, usize)> = self.trail().into_iter().map(|(n, d)| (n.id.clone(), how(n, d), n.children.len())).collect();
        let last = trail.last().cloned();
        // sections: the headers
        if let (true, Some((id, Present::Sections, n))) = (plain, &last) {
            let at = self.sub.get(id).copied().unwrap_or(0).min(n.saturating_sub(1));
            match k.code {
                KeyCode::Up | KeyCode::Down => {
                    let to = if k.code == KeyCode::Down { (at + 1).min(n.saturating_sub(1)) } else { at.saturating_sub(1) };
                    self.sub.insert(id.clone(), to);
                    return Some(Vec::new());
                }
                KeyCode::Enter | KeyCode::Char(' ') => {
                    let child = self.page_children(id).get(at).cloned()?;
                    crate::display::toggle(&mut self.shut, child);
                    return Some(self.sync());
                }
                KeyCode::Right => {
                    let child = self.trail().last().and_then(|(g, _)| g.children.get(at)).filter(|c| !self.shut.contains(&c.id) && has_values(c)).map(|c| c.id.clone());
                    if child.is_some() {
                        self.inside.insert(id.clone());
                    }
                    return Some(Vec::new());
                }
                KeyCode::Char('z') => {
                    let all = self.page_children(id);
                    if all.iter().any(|c| !self.shut.contains(c)) {
                        self.shut.extend(all);
                    } else {
                        all.iter().for_each(|c| {
                            self.shut.remove(c);
                        });
                    }
                    return Some(self.sync());
                }
                _ => {}
            }
        }
        // a split or a grid: the nearest one above the page; the arrows alone when the page is not text, with ctrl always
        let pane = trail.iter().rev().find(|(_, p, _)| matches!(p, Present::Split | Present::Grid)).cloned();
        if let (Some((id, p, n)), true) = (pane, ctrl || (plain && !field_is_text)) {
            let cols = if p == Present::Grid { self.grid_cols(&id) } else { n.max(1) };
            let at = self.sub.get(&id).copied().unwrap_or(0).min(n.saturating_sub(1));
            let to = match k.code {
                KeyCode::Left => at.checked_sub(1),
                KeyCode::Right => (at + 1 < n).then_some(at + 1),
                KeyCode::Up if p == Present::Grid => at.checked_sub(cols),
                KeyCode::Down if p == Present::Grid => (at + cols < n).then_some(at + cols),
                _ => return self.tab_keys(k, field_is_text),
            };
            if let Some(to) = to {
                self.sub.insert(id, to);
                return Some(self.sync());
            }
            return Some(Vec::new());
        }
        self.tab_keys(k, field_is_text)
    }

    fn page_children(&self, id: &str) -> Vec<String> {
        self.trail().into_iter().find(|(n, _)| n.id == id).map(|(n, _)| n.children.iter().map(|c| c.id.clone()).collect()).unwrap_or_default()
    }

    /// The cells in a row of the grid called `id`: what it says, or by width (one to a row under 60 columns, then one for each 40).
    fn grid_cols(&self, id: &str) -> usize {
        let says = self.trail().into_iter().find(|(n, _)| n.id == id).map_or(0, |(n, _)| usize::from(n.cols));
        if says > 0 { says } else { grid_auto(self.size.0) }
    }

    fn tab_keys(&mut self, k: KeyEvent, field_is_text: bool) -> Option<Vec<Up>> {
        if field_is_text || k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) {
            return None;
        }
        let KeyCode::Char(c) = k.code else { return None };
        let top = self.top().map(|t| (t.id.clone(), t.children.len()))?;
        match c {
            // a digit or a bracket that is a key of the row on the screen is the row's
            '1'..='9' if self.row_act(Some(&c.to_string())).is_none() => {
                let to = c as usize - '1' as usize;
                (to < top.1).then(|| {
                    self.tab = to;
                    self.sync()
                })
            }
            '[' | ']' if self.row_act(Some(&c.to_string())).is_none() => Some(self.step(top.0, top.1, c == ']')),
            '{' | '}' if self.row_act(Some(&c.to_string())).is_none() => {
                let (id, n) = self.subtabs().map(|s| (s.id.clone(), s.children.len()))?;
                Some(self.step(id, n, c == '}'))
            }
            _ => None,
        }
    }
}

/// Whether a value field that can be changed is in this node or below it.
fn has_values(n: &Node) -> bool {
    focusable(n) || n.children.iter().any(has_values)
}

/// Cells in a row of a grid with no `cols`.
pub fn grid_auto(width: u16) -> usize {
    if width < NARROW { 1 } else { usize::from(width / 40).max(2) }
}

// ---- drawing ----

fn label(th: &Theme, n: &Node, icons_only: bool, waiting: Option<u64>) -> String {
    let icon = Slot::by_name(&n.icon).map(|s| th.mark(s).0.trim_end().to_string()).unwrap_or_default();
    let title = if icons_only && !icon.is_empty() { String::new() } else { n.title.clone() };
    [icon, title, n.count.or(waiting).map(|c| c.to_string()).unwrap_or_default()].into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" ")
}

/// The row of titles and the row under it: `━` under the open tab in the accent colour (`▔` for sub-tabs), a rule under the rest.
fn tab_bar(app: &App, n: &Node, sub: bool, area: Rect, f: &mut Frame) -> Rect {
    let th = &app.theme;
    let top = app.top().is_some_and(|t| t.id == n.id);
    // the top row of tabs stands in from the edges: a blank row above it and a cell at each side; the page keeps the whole width
    let outer = area;
    let lead = if top { lead(app, area.height) } else { 0 };
    let full = format!("{} {} ", app.name, app.version);
    // what is given up, in this order, so that the names of the tabs stay as long as they can: the title of the core, then the side cell,
    // and only then the names (the icons stand for them, below)
    let steps: &[(u16, bool)] = if top { &[(1, true), (1, false), (0, false)] } else { &[(0, false)] };
    let layout = |(inset, with): (u16, bool)| {
        let aw = outer.width.saturating_sub(2 * inset);
        let head = if with { full.clone() } else { String::new() };
        // the name of the core takes the end of the top row, when there is room for both
        let room = if (head.width() as u16) < aw / 2 { usize::from(aw).saturating_sub(head.width()) } else { usize::from(aw) };
        (inset, head, room)
    };
    let open = app.open_of(n).map(|c| c.id.clone());
    let width = |ls: &[(String, bool)]| ls.iter().map(|(l, _)| l.width() + 1).sum::<usize>();
    // the titles stand apart by a space each side; with too many for the room they stand close, then the tabs that have an icon show it in
    // place of the title (always, on a narrow screen), and only then are some left out
    let make = |icons: bool, pad: &str| -> Vec<(String, bool)> {
        n.children.iter().map(|c| (format!("{pad}{}{pad}", label(th, c, icons, crate::ask::tab_count(app, c, Some(&c.id) == open.as_ref()))), Some(&c.id) == open.as_ref())).collect()
    };
    let names_fit = |room: usize| outer.width >= NARROW && [" ", ""].iter().any(|pad| width(&make(false, pad)) <= room);
    let pick = steps.iter().map(|s| layout(*s)).find(|(_, _, room)| names_fit(*room)).unwrap_or_else(|| layout(steps[0]));
    let (inset, head, room) = pick;
    let area = Rect { x: outer.x + inset, y: outer.y + lead, width: outer.width.saturating_sub(2 * inset), height: outer.height - lead };
    let narrow = outer.width < NARROW;
    let (spaced, labels) = [(false, " "), (false, ""), (true, " ")].into_iter().filter(|(i, _)| *i || !narrow).map(|(i, p)| (!p.is_empty(), make(i, p))).find(|(_, ls)| width(ls) <= room).unwrap_or_else(|| (false, make(true, "")));
    // the tabs that do not fit are cut from the start, so the open one is always in sight, and then from the end, so the name of the core is not drawn over them
    let (mut from, mut to) = (0, labels.len());
    while width(&labels[from..to]) > room && from + 1 < to {
        match (labels[from].1, labels[to - 1].1) {
            (false, _) => from += 1,
            (_, false) => to -= 1,
            _ => break,
        }
    }
    // the numbers a ⌥ digit goes to, the tree's, for a moment: each over a cell that is blank already, so no tab moves. A small one in the
    // blank before a spaced tab; in the rule under a close one, whose cell before touches the tab before (and in a tier whose numbers are
    // plain digits, where `1c` would read as one word)
    let numbers = th.glyph(Slot::TabNumber);
    let beside = spaced && !numbers.is_ascii();
    let number = |i: usize, on: bool| {
        let digits = if beside { numbers } else { Slot::TabNumber.cell(Tier::Ascii) };
        let style = match (on, th.no_color) {
            (true, true) => Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            (true, false) => th.fg(Hue::Accent),
            _ => th.tone(Tone::Dim),
        };
        (top && app.numbers_shown()).then(|| digits.chars().nth(i)).flatten().map(|d| Span::styled(d.to_string(), style))
    };
    let (mut titles, mut rule) = (Vec::new(), Vec::new());
    let mut used = 0;
    for (i, (text, on)) in labels.iter().enumerate().take(to).skip(from) {
        let (w, num) = (text.width(), number(i, *on));
        let style = if *on { th.fg(Hue::Accent) } else { th.tone(Tone::Dim) };
        match num.clone().filter(|_| beside) {
            Some(d) => titles.extend([d, Span::styled(text[1..].to_string(), style)]),
            None => titles.push(Span::styled(text.clone(), style)),
        }
        titles.push(Span::raw(" "));
        if sub {
            rule.push(Span::styled(if *on { th.glyph(Slot::RuleTop).repeat(w) } else { " ".repeat(w) }, th.fg(Hue::Accent)));
            rule.push(Span::raw(" "));
        } else {
            let (cell, hue) = if *on { (th.glyph(Slot::RuleHeavy), Hue::Accent) } else { (th.glyph(Slot::BoxH), Hue::Edge) };
            // under the tab's first glyph, past the blank of a spaced one
            let pad = usize::from(spaced);
            match num.filter(|_| !beside && w > pad) {
                Some(d) => rule.extend([Span::styled(cell.repeat(pad), th.fg(hue)), d, Span::styled(cell.repeat(w - 1 - pad), th.fg(hue))]),
                None => rule.push(Span::styled(cell.repeat(w), th.fg(hue))),
            }
            rule.push(Span::styled(th.glyph(Slot::BoxH), th.fg(Hue::Edge)));
        }
        used += w + 1;
    }
    if !sub {
        rule.push(Span::styled(th.glyph(Slot::BoxH).repeat((area.width as usize).saturating_sub(used)), th.fg(Hue::Edge)));
    }
    f.render_widget(Paragraph::new(Line::from(titles)), Rect { height: 1, ..area });
    if area.height > 1 {
        f.render_widget(Paragraph::new(Line::from(rule)), Rect { y: area.y + 1, height: 1, ..area });
    }
    if top {
        let w = head.width() as u16;
        if w < area.width / 2 {
            f.render_widget(Paragraph::new(head).style(th.tone(Tone::Dim)), Rect { x: area.right() - w, width: w, height: 1, ..area });
        }
    }
    let used = area.height.min(2);
    Rect { x: outer.x, y: area.y + used, width: outer.width, height: area.height - used }
}

/// The rows of room above the top tabs: one blank row, none with `density = compact` or on a screen too short to spare it.
fn lead(app: &App, height: u16) -> u16 {
    u16::from(app.theme.density != Density::Compact && height >= 6)
}

/// The rows the top tabs take from the top of the screen: the room above them, the titles and the rule.
pub(crate) fn tab_rows(app: &App, height: u16) -> u16 {
    if app.top().is_none() { 0 } else { (lead(app, height) + 2).min(height) }
}

/// How much room a node asks for in a stack.
fn want(app: &App, n: &Node, d: usize) -> Constraint {
    // a template may size a part itself (`height` wins over `grow`); the questions that float take no rows whatever it says
    let floats = n.field.as_ref().is_some_and(|f| f.kind == FieldKind::Ask) && crate::ask::floats(app);
    if !floats && n.height > 0 {
        return Constraint::Length(u16::from(n.height));
    }
    if !floats && n.grow > 0 {
        return Constraint::Fill(u16::from(n.grow));
    }
    match n.field.as_ref().map(|f| f.kind) {
        Some(k) if k.is_value() => Constraint::Length(crate::fields::height(app, n, app.size.0)),
        // the keys row that shares the row of the bar before it takes none of its own
        Some(FieldKind::Bar) => Constraint::Length(u16::from(!crate::keysrow::shared(n))),
        Some(k) if crate::display::shows(k) => crate::display::height(app, n, app.size.0).map_or(Constraint::Min(1), Constraint::Length),
        Some(FieldKind::Ask) => Constraint::Length(if crate::ask::floats(app) { 0 } else { crate::ask::height(app, app.size.0, app.size.1 / 2) }),
        _ if how(n, d) == Present::Stack && !n.children.is_empty() && n.children.iter().all(|c| matches!(want(app, c, d + 1), Constraint::Length(_))) => {
            Constraint::Length(n.children.iter().map(|c| if let Constraint::Length(h) = want(app, c, d + 1) { h } else { 0 }).sum())
        }
        _ => Constraint::Min(1),
    }
}

pub fn draw(app: &mut App, n: &Node, d: usize, area: Rect, f: &mut Frame) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    // the help of a page of rows is its last line, in a dim tone
    if !n.help.is_empty() && area.height > 2 && n.field.as_ref().is_some_and(|x| matches!(x.kind, FieldKind::List | FieldKind::Table)) {
        let (body, foot) = (Rect { height: area.height - 1, ..area }, Rect { y: area.bottom() - 1, height: 1, ..area });
        f.render_widget(Paragraph::new(fit(&n.help, usize::from(area.width))).style(app.theme.tone(Tone::Dim)), foot);
        return draw(app, &Node { help: String::new(), ..n.clone() }, d, body, f);
    }
    if let Some(field) = &n.field {
        if field.kind.is_value() {
            return crate::fields::draw(app, n, area, f);
        }
        if crate::display::shows(field.kind) {
            return crate::display::draw(app, n, area, f);
        }
        return crate::view::draw_field(app, field, area, f);
    }
    if n.lazy && n.children.is_empty() {
        // asked for and not come: the sign once the wait is long enough to see, and until then the mark that something is coming
        let width = usize::from(area.width);
        match app.node_wait(&n.id).and_then(|w| app.wait_line(w, "", width)) {
            Some(line) => f.render_widget(Paragraph::new(line), area),
            None => f.render_widget(Paragraph::new("…").style(app.theme.tone(Tone::Dim)), area),
        }
        return;
    }
    match how(n, d) {
        p @ (Present::Tabs | Present::Subtabs) => {
            let body = tab_bar(app, n, p == Present::Subtabs, area, f);
            if let Some(c) = app.open_of(n) {
                draw(app, c, d + 1, body, f);
            }
        }
        Present::Sections => sections(app, n, d, area, f),
        Present::Split => split(app, n, d, area, f),
        Present::Grid => grid(app, n, d, area, f),
        _ => {
            // a stack taller than its room starts at the field that keeps the one the keys are on in sight
            let start = stack_start(app, n, d, area.height);
            // only as many children as there are rows can be seen, and the solver is not asked about the rest
            let seen = &n.children[start..n.children.len().min(start + usize::from(area.height))];
            let sizes: Vec<Constraint> = seen.iter().map(|c| want(app, c, d + 1)).collect();
            let parts = Layout::vertical(sizes).split(area);
            for (i, (c, r)) in seen.iter().zip(parts.iter()).enumerate() {
                match seen.get(i + 1).filter(|k| crate::keysrow::shared(k)) {
                    Some(keys) => crate::keysrow::pair(app, c, keys, d + 1, *r, f),
                    None if crate::keysrow::shared(c) && i > 0 => {}
                    None => draw(app, c, d + 1, *r, f),
                }
            }
        }
    }
}

/// The first child of a stack to draw: 0 unless the stack is of children that know their height, they are more than the room, and the keys
/// are on one of them; then the first that leaves the focused one, and every child between, in sight. The place is kept, so the view moves
/// only when the focus leaves it.
fn stack_start(app: &mut App, n: &Node, d: usize, room: u16) -> usize {
    let heights: Option<Vec<usize>> = n.children.iter().map(|c| if let Constraint::Length(h) = want(app, c, d + 1) { Some(usize::from(h)) } else { None }).collect();
    let Some(hs) = heights.filter(|hs| !hs.is_empty() && hs.iter().sum::<usize>() > usize::from(room)) else {
        app.stack_top.remove(&n.id);
        return 0;
    };
    let focus = app.page().and_then(|p| n.children.iter().position(|c| c.id == p.id));
    let room = usize::from(room);
    let top = app.stack_top.get(&n.id).copied().unwrap_or(0).min(hs.len() - 1);
    let mut top = focus.map_or(top, |f| keep_by(top as u64, f as u64, room, |i| hs[i as usize]) as usize);
    // no blank at the end while there are fields above
    while top > 0 && hs[top - 1..].iter().sum::<usize>() <= room {
        top -= 1;
    }
    app.stack_top.insert(n.id.clone(), top);
    top
}

fn sections(app: &mut App, n: &Node, d: usize, area: Rect, f: &mut Frame) {
    let focused = app.page().is_some_and(|p| p.id == n.id);
    let at = app.sub.get(&n.id).copied().unwrap_or(0).min(n.children.len().saturating_sub(1));
    // the headers from the one that keeps the selected header in sight; only as many as the screen can show are laid out
    // (a section takes at least a row for its header and one for its body, if it is open)
    let rows_of = |c: &Node| usize::from(!c.title.is_empty()) + usize::from(!app.shut.contains(&c.id));
    let (mut start, mut above) = (at, 0);
    while start > 0 && above + rows_of(&n.children[start - 1]) <= usize::from(area.height) / 2 {
        start -= 1;
        above += rows_of(&n.children[start]);
    }
    let shown: Vec<(usize, &Node)> = n.children.iter().enumerate().skip(start).take(usize::from(area.height)).collect();
    let open: Vec<bool> = shown.iter().map(|(_, c)| !app.shut.contains(&c.id)).collect();
    let headers = shown.iter().filter(|(_, c)| !c.title.is_empty()).count();
    let free = usize::from(area.height).saturating_sub(headers);
    // a body that knows its height (a stack of value fields) takes it, if there is room; the rest share what is left, each at least a row
    let mut left = free;
    let mut size: Vec<Option<usize>> = Vec::with_capacity(shown.len());
    for ((_, c), o) in shown.iter().zip(&open) {
        size.push(match (o, want(app, c, d + 1)) {
            (true, Constraint::Length(h)) => {
                let h = usize::from(h).min(left);
                left -= h;
                Some(h)
            }
            _ => None,
        });
    }
    let flex = open.iter().zip(&size).filter(|(o, s)| **o && s.is_none()).count();
    let per = left.checked_div(flex).map_or(0, |p| p.max(1));
    let extra = if left > flex { left.checked_rem(flex).unwrap_or(0) } else { 0 };
    let (mut y, mut given) = (area.y, 0usize);
    for (((i, c), open), fixed) in shown.iter().zip(open).zip(size) {
        if y >= area.bottom() {
            break;
        }
        if !c.title.is_empty() {
            let th = &app.theme;
            let (fold, _) = th.mark(if open { Slot::FoldOpen } else { Slot::FoldClosed });
            let on = focused && *i == at;
            let mut spans = vec![Span::styled(format!("{fold} "), th.fg(Hue::Faint)), Span::styled(c.title.clone(), if on { th.fg(Hue::Accent) } else { Style::default() })];
            if let Some(count) = c.count {
                spans.push(Span::styled(format!("  {count}"), th.tone(Tone::Dim)));
            }
            let style = if on { th.back(Hue::Sel) } else { Style::default() };
            f.render_widget(Paragraph::new(Line::from(spans)).style(style), Rect { y, height: 1, ..area });
            y += 1;
        }
        if open {
            let h = match fixed {
                Some(h) => h as u16,
                None => {
                    given += 1;
                    (per + usize::from(given <= extra)) as u16
                }
            };
            let body = Rect { y, height: h.min(area.bottom().saturating_sub(y)), ..area };
            draw(app, c, d + 1, body, f);
            y += h;
        }
    }
}

fn split(app: &mut App, n: &Node, d: usize, area: Rect, f: &mut Frame) {
    let k = n.children.len();
    if k == 0 {
        return;
    }
    if area.width < NARROW {
        // too narrow for side by side: one under the other
        let parts = Layout::vertical(vec![Constraint::Ratio(1, k as u32); k]).split(area);
        for (c, r) in n.children.iter().zip(parts.iter()) {
            draw(app, c, d + 1, *r, f);
        }
        return;
    }
    let at = app.sub.get(&n.id).copied().unwrap_or(0).min(k - 1);
    let first = if n.ratio == 0 { 50 } else { u32::from(n.ratio.clamp(1, 99)) };
    let mut sizes = Vec::new();
    for i in 0..k {
        if i > 0 {
            sizes.push(Constraint::Length(1));
        }
        sizes.push(if k == 2 { Constraint::Percentage(if i == 0 { first } else { 100 - first } as u16) } else { Constraint::Ratio(1, k as u32) });
    }
    let parts = Layout::horizontal(sizes).split(area);
    for (i, c) in n.children.iter().enumerate() {
        draw(app, c, d + 1, parts[i * 2], f);
        if i + 1 < k {
            // the bar next to the focused pane is heavy and in the accent colour
            let near = i == at || i + 1 == at;
            let th = &app.theme;
            let (glyph, style) = if near { (th.glyph(Slot::Focus), th.fg(Hue::Accent)) } else { (th.glyph(Slot::BoxV), th.fg(Hue::Edge)) };
            let bar = Rect { ..parts[i * 2 + 1] };
            f.render_widget(Paragraph::new((0..bar.height).map(|_| Line::styled(glyph, style)).collect::<Vec<_>>()), bar);
        }
    }
}

fn grid(app: &mut App, n: &Node, d: usize, area: Rect, f: &mut Frame) {
    let k = n.children.len();
    if k == 0 {
        return;
    }
    let cols = (if n.cols > 0 { usize::from(n.cols) } else { grid_auto(area.width) }).clamp(1, k);
    let rows = k.div_ceil(cols);
    let at = app.sub.get(&n.id).copied().unwrap_or(0).min(k - 1);
    // a column between the cells and a row of rule between the lines, with a joint where they cross
    let mut v = Vec::new();
    for r in 0..rows {
        if r > 0 {
            v.push(Constraint::Length(1));
        }
        v.push(Constraint::Ratio(1, rows as u32));
    }
    let lines = Layout::vertical(v).split(area);
    let mut h = Vec::new();
    for c in 0..cols {
        if c > 0 {
            h.push(Constraint::Length(1));
        }
        h.push(Constraint::Ratio(1, cols as u32));
    }
    for r in 0..rows {
        let line = lines[r * 2];
        let cells = Layout::horizontal(h.clone()).split(line);
        for c in 0..cols {
            let i = r * cols + c;
            let cell = cells[c * 2];
            if let Some(child) = n.children.get(i) {
                if i == at {
                    f.buffer_mut().set_style(cell, app.theme.back(Hue::Raise));
                }
                draw(app, child, d + 1, cell, f);
            }
            if c + 1 < cols {
                let bar = cells[c * 2 + 1];
                let th = &app.theme;
                f.render_widget(Paragraph::new((0..bar.height).map(|_| Line::styled(th.glyph(Slot::BoxV), th.fg(Hue::Edge))).collect::<Vec<_>>()), bar);
            }
        }
        if r + 1 < rows {
            let th = &app.theme;
            let rule = lines[r * 2 + 1];
            let mut s = String::new();
            for c in 0..cols {
                if c > 0 {
                    s.push('┼');
                }
                s.push_str(&th.glyph(Slot::BoxH).repeat(cells_width(&Layout::horizontal(h.clone()).split(rule), c)));
            }
            f.render_widget(Paragraph::new(Line::styled(s, th.fg(Hue::Edge))), rule);
        }
    }
}

fn cells_width(cells: &[Rect], c: usize) -> usize {
    cells.get(c * 2).map_or(0, |r| r.width as usize)
}

// ---- the jump ----

/// `⌃k`: a line to type in and the nodes that match it.
#[derive(Debug, Clone, Default)]
pub struct Jump {
    pub line: Typed,
    pub sel: Sel,
    pub hits: Vec<fuzzy::Hit>,
    /// `:chats`: only the chats are listed.
    pub chats: bool,
}

impl App {
    pub(crate) fn open_jump(&mut self) {
        self.jump = Some(Jump { hits: self.found(""), ..Jump::default() });
    }

    /// `:chats`: the chats, the ones that wait first, `⏎` goes into one.
    pub(crate) fn open_chats(&mut self) {
        self.jump = Some(Jump { hits: self.chat_hits(""), chats: true, ..Jump::default() });
    }

    /// The chats a query finds, in the order the core sent them (the ones that wait first); an empty query finds them all.
    fn chat_hits(&self, query: &str) -> Vec<fuzzy::Hit> {
        let q: Vec<char> = query.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).collect();
        self.pinned()
            .iter()
            .filter_map(|r| {
                let title = format!("{} {}", r.cells.get(1).map_or("", |c| c.text.as_str()), r.cells.get(2).map_or("", |c| c.text.as_str()));
                let score = fuzzy::score(&q, &title)?;
                Some(fuzzy::Hit { id: r.key.clone(), title, above: "chats".into(), icon: r.cells.first().map(|c| c.icon.clone()).unwrap_or_default(), score, command: false, chat: true })
            })
            .collect()
    }

    /// What `query` finds among the nodes and the commands of the palette, and the chats.
    fn found(&self, query: &str) -> Vec<fuzzy::Hit> {
        let palette = self.rows.get(PALETTE).map_or(&[][..], Vec::as_slice);
        let mut hits = self.root().map(|r| fuzzy::search_with(r, palette, query)).unwrap_or_default();
        hits.extend(self.chat_hits(query).into_iter().take(8));
        hits
    }

    /// A command picked in the jump: the chat is where it is said. One that takes nothing more is said at once; one that takes more
    /// leaves its words in the composer to be finished.
    fn run_command(&mut self, key: &str) -> Vec<Up> {
        let Some(row) = self.rows.get(PALETTE).and_then(|rs| rs.iter().find(|r| r.key == key)).cloned() else { return Vec::new() };
        fn chat(n: &Node) -> Option<&Node> {
            if n.field.as_ref().is_some_and(|f| f.kind == FieldKind::Chat) { Some(n) } else { n.children.iter().find_map(chat) }
        }
        let to = self.root().and_then(chat).map(|n| n.id.clone());
        let mut ups = to.map(|id| self.focus(&id)).unwrap_or_default();
        match row.actions.into_iter().find(|a| a.key == "enter") {
            Some(act) => ups.extend(self.run_act(act)),
            None => {
                let word = row.cells.first().and_then(|c| c.text.split_whitespace().next()).unwrap_or_default();
                self.set_input(format!("{word} "));
            }
        }
        ups
    }

    pub(crate) fn jump_key(&mut self, k: KeyEvent) -> Vec<Up> {
        let Some(mut j) = self.jump.take() else { return Vec::new() };
        let mut search = false;
        match k.code {
            KeyCode::Esc => return Vec::new(),
            KeyCode::Char('k' | 'g') if k.modifiers.contains(KeyModifiers::CONTROL) => return Vec::new(),
            KeyCode::Up => j.sel.by(-1, j.hits.len() as u64),
            KeyCode::Down => j.sel.by(1, j.hits.len() as u64),
            KeyCode::Enter => {
                return match j.hits.get(j.sel.at as usize) {
                    Some(h) if h.chat => App::enter(&h.id),
                    Some(h) if h.command => self.run_command(&h.id.clone()),
                    Some(h) => self.focus(&h.id.clone()),
                    None => Vec::new(),
                };
            }
            _ => search = j.line.edit(&k),
        }
        if search {
            j.hits = if j.chats { self.chat_hits(&j.line.buf) } else { self.found(&j.line.buf) };
            j.sel = Sel::default();
        }
        self.jump = Some(j);
        Vec::new()
    }
}

pub fn draw_jump(app: &App, area: Rect, f: &mut Frame) {
    let Some(j) = &app.jump else { return };
    let th = &app.theme;
    // under the tab rows, not over them; the room is what is left below them
    let top = tab_rows(app, area.height).min(area.height.saturating_sub(6));
    let (w, room) = (area.width.saturating_sub(2).min(64), area.height - top);
    if w < 12 || room < 6 {
        return;
    }
    let iw = usize::from(w) - 4;
    let q: Vec<char> = j.line.buf.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).collect();
    // the hits as lines, a dim head once for each parent; the line of the chosen hit is kept in sight
    let (mut body, mut chosen) = (Vec::new(), 0);
    let mut group: Option<(bool, &str)> = None;
    for (i, h) in j.hits.iter().enumerate() {
        let g = fuzzy::group_of(h);
        if group != Some(g) {
            group = Some(g);
            // the tabs themselves have no parent: they are headed "tabs" unless they come first
            let head = if h.command { "commands" } else if h.above.is_empty() && i > 0 { "tabs" } else { h.above.as_str() };
            if !head.is_empty() {
                body.push(Line::styled(fit(head, iw), th.tone(Tone::Dim)));
            }
        }
        let on = i as u64 == j.sel.at;
        if on {
            chosen = body.len();
        }
        let mut spans = vec![Span::styled(if on { "▸ " } else { "  " }, th.fg(Hue::Accent))];
        let (mark, role) = Slot::by_name(&h.icon).map(|s| th.mark(s)).unzip();
        if let (Some(mark), Some(role)) = (mark, role) {
            spans.push(Span::styled(mark, role));
        }
        let at = fuzzy::positions(&q, &h.title);
        let tail = if h.command && !h.above.is_empty() { format!("  {}", h.above) } else { String::new() };
        let used = spans.iter().map(|s| s.content.width()).sum::<usize>();
        let title = fit(&h.title, iw.saturating_sub(used));
        let mut taken = used + title.width();
        for (ci, c) in title.chars().enumerate() {
            spans.push(Span::styled(c.to_string(), if at.contains(&ci) { th.fg(Hue::Accent) } else { Style::default() }));
        }
        if !tail.is_empty() {
            let tail = fit(&tail, iw.saturating_sub(taken));
            taken += tail.width();
            spans.push(Span::styled(tail, th.tone(Tone::Dim)));
        }
        if on {
            spans.push(Span::raw(" ".repeat(iw.saturating_sub(taken))));
        }
        body.push(Line::from(spans).style(if on { th.back(Hue::Sel) } else { Style::default() }));
    }
    // the query and a blank row above the hits, a blank row and the keys below them
    let space = usize::from(room - 2).saturating_sub(4).max(1);
    let from = keep(0, chosen as u64, chosen as u64, space as u64) as usize;
    let shown: Vec<Line> = body.into_iter().skip(from).take(space).collect();
    let r = Rect { x: area.x + (area.width - w) / 2, y: area.y + top, width: w, height: shown.len() as u16 + 6 };
    let inside = crate::parts::popup::boxed(th, f, r, "find");
    let mut lines = vec![Line::from(vec![Span::styled(format!("{} ", th.glyph(Slot::Chevron)), th.fg(Hue::Accent)), Span::raw(j.line.buf.clone()), Span::styled("▏", th.fg(Hue::Accent))]), Line::raw("")];
    lines.extend(shown);
    lines.push(Line::raw(""));
    lines.push(Line::styled(format!("{} jump   esc close", th.glyph(Slot::KeyEnter)), th.tone(Tone::Dim)));
    f.render_widget(Paragraph::new(lines), inside);
}
