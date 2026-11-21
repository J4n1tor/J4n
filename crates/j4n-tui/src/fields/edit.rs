//! What a key does to a value field: at rest (start an edit, flip a switch, step a number) and while it is being edited (type, pick,
//! tick, add a row, keep, give up). The edit is a small state of its own; nothing is sent until a value is kept, except for the fields
//! that are lists of rows or ticks, which send each change as it is made.

use super::show::{columns, path_options, ref_options};
use crate::parts::suggest::Step;
use super::{Spec, draft, pairs, parse, strings, text_of};
use crate::app::App;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use crate::parts::line::Typed;
use j4n_proto::{FieldKind, Intent, Json, Node, Present, Secret, Tone, Up, Zeroize};

/// The row of a list or a map that is being typed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    New,
    At(usize),
}

/// A value being edited. It has no `Debug`: a key's text is never printed, and the line wipes it when the edit goes.
#[derive(Clone, Default)]
pub struct Ed {
    pub bind: String,
    /// The text typed (for a list or a map: the row being typed), and the cursor in it.
    pub line: Typed,
    /// The row that has the cursor: an option, a candidate, a row; one past the last row is the one that adds.
    pub pick: usize,
    /// The arguments of an argv that are done.
    pub items: Vec<String>,
    pub row: Option<Row>,
    /// Tab stepping through the names a Path or Ref could be (`parts::suggest`).
    pub tab: Option<Step>,
    /// A key is typed: drawn as a mask of a fixed width, the cursor kept at the end, a paste taken whole.
    pub secret: bool,
    /// How the key's text came, for the word after the mask.
    pub how: How,
}

/// How a key's text came: typed, pasted whole, pasted as `NAME=value` with the name left out, or a paste refused and why.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum How {
    #[default]
    Typed,
    Pasted,
    Named(String),
    Refused(String),
}

/// The room a key's text is given before it is typed, so that it grows in place and leaves no copy behind that is not wiped.
const KEY_ROOM: usize = 512;

impl Ed {
    fn with(bind: &str, buf: String) -> Ed {
        Ed { bind: bind.into(), line: Typed::new(buf), ..Ed::default() }
    }

    /// A paste into a key, taken whole in place of what was typed and trimmed of the blanks around it: `NAME=value` keeps the value,
    /// and a second line or a blank inside is refused and keeps nothing.
    fn paste_key(&mut self, text: &str) {
        self.line.buf.zeroize();
        self.line.at = 0;
        let t = text.trim();
        let lines = t.split(['\n', '\r']).filter(|l| !l.trim().is_empty()).count();
        self.how = if lines > 1 {
            How::Refused(format!("the paste had {lines} lines · paste only the key"))
        } else if t.is_empty() || t.chars().any(|c| c.is_whitespace() || c.is_control()) {
            How::Refused("the paste had a blank in it · paste only the key".into())
        } else {
            let (name, value) = named(t);
            value.chars().for_each(|c| self.line.insert(c));
            name.map_or(How::Pasted, |n| How::Named(n.into()))
        };
    }
}

/// `NAME=value` as an environment line has it (`OPENROUTER_API_KEY=…`): the name, when it is one (capitals, digits and `_`, with a `_` in
/// it, so that a key's own `=` is never taken for one), and the value, out of its quotes.
pub(crate) fn named(t: &str) -> (Option<&str>, &str) {
    let Some((name, value)) = t.split_once('=') else { return (None, t) };
    let env = name.contains('_') && name.starts_with(|c: char| c.is_ascii_uppercase()) && name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
    if !env || value.is_empty() || value.starts_with('=') {
        return (None, t);
    }
    let quoted = value.len() >= 2 && ['"', '\''].iter().any(|q| value.starts_with(*q) && value.ends_with(*q));
    (Some(name), if quoted { &value[1..value.len() - 1] } else { value })
}

/// The edit that starts when a field that has the keys is opened (`⏎`).
fn begin(spec: &Spec, value: &Json) -> Ed {
    let bind = spec.bind;
    let options = spec.words("options");
    match spec.kind {
        FieldKind::Bool => Ed { pick: usize::from(!value.as_bool().unwrap_or(false)), ..Ed::with(bind, String::new()) },
        FieldKind::Enum => Ed { pick: options.iter().position(|o| Some(*o) == value.as_str()).unwrap_or(0), ..Ed::with(bind, String::new()) },
        FieldKind::Ref => {
            let near = ref_options(spec, "");
            Ed { pick: near.iter().position(|o| Some(*o) == value.as_str()).unwrap_or(0), ..Ed::with(bind, String::new()) }
        }
        // a key starts empty (the one kept is never shown), with its room made
        FieldKind::Secret => Ed { secret: true, ..Ed::with(bind, String::with_capacity(KEY_ROOM)) },
        // the start of a long text is what is wanted first
        FieldKind::Markdown => Ed { line: Typed { at: 0, buf: text_of(value) }, ..Ed::with(bind, String::new()) },
        FieldKind::Dur if value.is_number() => Ed::with(bind, parse::dur_short(value.as_u64().unwrap_or(0))),
        FieldKind::Argv => {
            let mut items = strings(value);
            let last = items.pop().unwrap_or_default();
            Ed { items, ..Ed::with(bind, last) }
        }
        _ => Ed::with(bind, text_of(value)),
    }
}

/// A key that is not for the tab bar or the jump, in a field that is being typed in.
fn printable(k: &KeyEvent) -> Option<char> {
    match k.code {
        KeyCode::Char(c) if !k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => Some(c),
        _ => None,
    }
}

impl App {
    /// The row of the open list the keys are on, when it carries its editor.
    fn edit_row(&self) -> Option<(String, j4n_proto::Row)> {
        let bind = self.field()?.bind.clone();
        let row = self.rows.get(&bind)?.get(self.place(&bind))?.clone();
        row.data.as_ref()?.get("edit")?;
        Some((bind, row))
    }

    /// `Intent::Set` from a row that carries its editor: a value is sent as it is (`space` on a switch, the next word); with
    /// none, a switch flips and anything else opens the editor of its kind in the row's place. `None` for a row that carries none.
    pub(crate) fn row_set(&mut self, value: &Json) -> Option<Vec<Up>> {
        let (list, row) = self.edit_row()?;
        let (field, now, edit) = super::row_field(&row)?;
        // a key is typed in its masked editor and nowhere else, and that editor sends to J4n's key file and nowhere else
        if field.bind.starts_with("key:") != (field.kind == FieldKind::Secret) || (field.kind == FieldKind::Secret && !value.is_null()) {
            self.toast = Some((NO_ROAD.into(), Tone::Bad));
            return Some(Vec::new());
        }
        if !value.is_null() {
            return Some(self.row_sent(&field.bind, value.clone(), &edit));
        }
        if field.kind == FieldKind::Bool {
            return Some(self.row_sent(&field.bind, Json::Bool(!now.as_bool().unwrap_or(false)), &edit));
        }
        self.values.insert(field.bind.clone(), now.clone());
        self.val = Some(begin(&Spec::new(&row.key, &field), &now));
        self.row_ed = Some(super::RowEd { list, key: row.key, field });
        Some(Vec::new())
    }

    /// The value of a row sent; a setting that acts from the next start, now not what it was at the start, brings up the restart card.
    fn row_sent(&mut self, bind: &str, value: Json, edit: &Json) -> Vec<Up> {
        self.wait_check(bind, &value, edit);
        self.set_value(bind, value)
    }

    fn wait_check(&mut self, bind: &str, value: &Json, edit: &Json) {
        let start = edit.get("start").map(text_of).unwrap_or_default();
        if edit.get("next_start").and_then(Json::as_bool).unwrap_or(false) && text_of(value) != start {
            self.restart = Some(bind.trim_start_matches("setting:").trim_end_matches(".value").to_string());
        }
    }

    /// A key while a row is being changed in place: the editor of its kind has it; when the edit ends (kept or given up) the row is a row again.
    pub(crate) fn row_key(&mut self, k: KeyEvent) -> Vec<Up> {
        let Some(re) = self.row_ed.clone() else { return Vec::new() };
        let ed = self.val.take();
        let row = self.rows.get(&re.list).and_then(|rows| rows.iter().find(|r| r.key == re.key)).cloned();
        let (Some(ed), Some(row), true) = (ed, row, self.field().is_some_and(|f| f.bind == re.list)) else {
            self.row_ed = None;
            return Vec::new();
        };
        if k.modifiers.contains(KeyModifiers::CONTROL) && matches!(k.code, KeyCode::Char('c' | 'd')) {
            self.row_ed = None;
            self.quit = true;
            return vec![Up::Quit];
        }
        let edit = super::row_field(&row).map(|(.., e)| e).unwrap_or_default();
        let ups = self.editing_key(&Spec::new(&re.key, &re.field), ed, k);
        if self.val.is_none() {
            self.row_ed = None;
        }
        for up in &ups {
            if let Up::Intent { intent: j4n_proto::Intent::Set { value, .. } } = up {
                self.wait_check(&re.field.bind, value, &edit);
            }
        }
        ups
    }

    /// The lines of the row being changed, drawn as its editor in its place, `w` wide; `None` for any other row.
    pub(crate) fn row_lines(&self, list: &str, row: &j4n_proto::Row, w: usize, last: bool) -> Option<Vec<ratatui::text::Line<'static>>> {
        let re = self.row_ed.as_ref().filter(|e| e.list == list && e.key == row.key)?;
        let spec = Spec::new(&re.key, &re.field);
        Some(super::rows(&super::View { th: &self.theme, spec, value: self.value(&re.field.bind), st: super::State::Editing, ed: self.val.as_ref(), width: w, last }))
    }

    /// The node with the keys, when it is a value field.
    fn val_node(&self) -> Option<Node> {
        self.page().filter(|n| Spec::of(n).is_some_and(|s| !s.disabled())).cloned()
    }

    /// The value fields of the stack that holds the page, and the place of the page among them.
    fn siblings(&self) -> Option<(String, Vec<String>, usize)> {
        let trail = self.trail();
        let (page, parent) = (trail.last()?.0, trail.len().checked_sub(2).and_then(|i| trail.get(i))?);
        // only a stack holds fields side by side; a section that is one field has none beside it
        if crate::nodes::how(parent.0, parent.1) != Present::Stack {
            return None;
        }
        let ids: Vec<String> = parent.0.children.iter().filter(|c| crate::nodes::focusable(c)).map(|c| c.id.clone()).collect();
        let at = ids.iter().position(|i| *i == page.id)?;
        Some((parent.0.id.clone(), ids, at))
    }

    /// `↑↓` between the value fields of a stack; `↑` from the first leaves the section the keys went into.
    fn field_move(&mut self, down: bool) -> Vec<Up> {
        if let Some((stack, ids, at)) = self.siblings() {
            let to = if down { (at + 1).min(ids.len() - 1) } else { at.saturating_sub(1) };
            if !down && at == 0 {
                self.leave();
            } else {
                self.sub.insert(stack, to);
            }
        } else if !down {
            self.leave();
        }
        Vec::new()
    }

    /// Out of the body of a section, to its header; false when the keys are in none.
    pub(crate) fn leave(&mut self) -> bool {
        let groups: Vec<String> = self.trail().into_iter().map(|(n, _)| n.id.clone()).filter(|id| self.inside.contains(id)).collect();
        groups.last().is_some_and(|last| self.inside.remove(last))
    }

    /// `↑↓` between the fields of a stack and `esc` out of a section, for a field that has no keys of its own for them (a button).
    pub(crate) fn val_move(&mut self, k: KeyEvent) -> Option<Vec<Up>> {
        if k.modifiers.contains(KeyModifiers::CONTROL) {
            return None;
        }
        match k.code {
            KeyCode::Up => Some(self.field_move(false)),
            KeyCode::Down => Some(self.field_move(true)),
            // out of a section; in none, Esc goes on (to the first tab)
            KeyCode::Esc => self.leave().then(Vec::new),
            _ => None,
        }
    }

    /// A key for the value field that has the keys, if there is one: `Some` when the field took it.
    pub(crate) fn val_key(&mut self, k: KeyEvent) -> Option<Vec<Up>> {
        let n = self.val_node()?;
        let spec = Spec::of(&n)?;
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        if let Some(ed) = self.val.take().filter(|e| e.bind == spec.bind) {
            if ctrl && matches!(k.code, KeyCode::Char('c' | 'd')) {
                self.val = Some(ed);
                return None;
            }
            return Some(self.editing_key(&spec, ed, k));
        }
        self.val = None;
        let value = self.value(spec.bind).clone();
        let kind = spec.kind;
        match k.code {
            KeyCode::Up if !ctrl => Some(self.field_move(false)),
            KeyCode::Down if !ctrl => Some(self.field_move(true)),
            // out of a section; in none, Esc goes on (to the first tab)
            KeyCode::Esc => self.leave().then(Vec::new),
            KeyCode::Char(' ') if kind == FieldKind::Bool => Some(self.set_value(spec.bind, Json::Bool(!value.as_bool().unwrap_or(false)))),
            KeyCode::Enter | KeyCode::Char(' ') if !ctrl => {
                self.val = Some(begin(&spec, &value));
                Some(Vec::new())
            }
            KeyCode::Char(c) if matches!(kind, FieldKind::Int | FieldKind::Num) && !ctrl && (c.is_ascii_digit() || c == '-') => {
                self.val = Some(Ed::with(spec.bind, c.to_string()));
                Some(Vec::new())
            }
            KeyCode::Left | KeyCode::Right if matches!(kind, FieldKind::Int | FieldKind::Num) && !ctrl => {
                let mut ed = begin(&spec, &value);
                step(&spec, &mut ed, k.code == KeyCode::Right);
                self.val = Some(ed);
                Some(Vec::new())
            }
            KeyCode::Left | KeyCode::Right if kind == FieldKind::Enum && !ctrl => {
                let mut ed = begin(&spec, &value);
                let last = spec.words("options").len().saturating_sub(1);
                ed.pick = if k.code == KeyCode::Right { (ed.pick + 1).min(last) } else { ed.pick.saturating_sub(1) };
                self.val = Some(ed);
                Some(Vec::new())
            }
            _ => None,
        }
    }

    /// A paste: into the text being typed, a line break kept only where the field has lines.
    pub(crate) fn val_paste(&mut self, text: &str) -> bool {
        let Some(mut ed) = self.val.take() else { return false };
        ed.tab = None;
        if ed.secret {
            ed.paste_key(text);
            self.val = Some(ed);
            return true;
        }
        let lines = self.val_node().is_some_and(|n| Spec::of(&n).is_some_and(|s| matches!(s.kind, FieldKind::Text | FieldKind::Markdown)));
        for c in text.chars().filter(|c| !c.is_control() || (lines && *c == '\n')) {
            ed.line.insert(c);
        }
        self.val = Some(ed);
        true
    }

    /// `⏎` on a key: it goes once, as `Intent::Key` made from the text itself (no copy, no `Set`, no draft), and the screen keeps only that
    /// it is set. A secret bound to anything but a provider's key sends nothing. The keys that come in the same burst are dropped.
    fn keep_key(&mut self, spec: &Spec, mut ed: Ed) -> Vec<Up> {
        if ed.line.buf.trim().is_empty() {
            ed.how = How::Refused("nothing typed".into());
            self.val = Some(ed);
            return Vec::new();
        }
        let Some(id) = spec.bind.strip_prefix("key:").map(|r| r.rsplit_once('.').map_or(r, |(id, _)| id).to_string()) else {
            self.toast = Some((NO_ROAD.into(), Tone::Bad));
            return Vec::new();
        };
        let value = Secret::new(std::mem::take(&mut ed.line.buf));
        self.values.insert(spec.bind.into(), Json::Bool(true));
        self.burst = self.clock;
        vec![Up::Intent { intent: Intent::Key { id, value } }]
    }

    fn keep(&mut self, spec: &Spec, ed: Ed) -> Vec<Up> {
        if ed.secret {
            return self.keep_key(spec, ed);
        }
        match draft(spec, &ed.line.buf, &ed.items) {
            Ok(v) if v == *self.value(spec.bind) => Vec::new(),
            Ok(v) => self.set_value(spec.bind, v),
            // it stays open, with what is wrong under it
            Err(_) => {
                self.val = Some(ed);
                Vec::new()
            }
        }
    }

    fn editing_key(&mut self, spec: &Spec, mut ed: Ed, k: KeyEvent) -> Vec<Up> {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let value = self.value(spec.bind).clone();
        let kind = spec.kind;
        let stay = |app: &mut App, ed: Ed| {
            app.val = Some(ed);
            Vec::new()
        };
        match kind {
            FieldKind::Bool => match k.code {
                KeyCode::Char(' ') => {
                    ed.pick ^= 1;
                    return stay(self, ed);
                }
                KeyCode::Enter if Some(ed.pick == 1) == self.value(spec.bind).as_bool() => return Vec::new(),
                KeyCode::Enter => return self.set_value(spec.bind, Json::Bool(ed.pick == 1)),
                KeyCode::Esc => return Vec::new(),
                _ => return stay(self, ed),
            },
            FieldKind::Enum => {
                let options = spec.words("options");
                let last = options.len().saturating_sub(1);
                match k.code {
                    KeyCode::Left | KeyCode::Up => ed.pick = ed.pick.saturating_sub(1),
                    KeyCode::Right | KeyCode::Down => ed.pick = (ed.pick + 1).min(last),
                    KeyCode::Enter => {
                        return match options.get(ed.pick) {
                            Some(o) if Some(*o) != value.as_str() => self.set_value(spec.bind, Json::String((*o).into())),
                            _ => Vec::new(),
                        };
                    }
                    KeyCode::Esc => return Vec::new(),
                    _ => {}
                }
                return stay(self, ed);
            }
            FieldKind::Multi => return self.ticking_key(spec, ed, k, &value),
            FieldKind::Seq | FieldKind::Map => return self.listing_key(spec, ed, k, &value),
            _ => {}
        }
        // the kinds with a line of text; while Tab steps, `esc` puts back what was typed, Tab goes on, any other key goes on from the line
        let tab = ed.tab.take();
        if let Some(t) = tab.as_ref().filter(|_| k.code == KeyCode::Esc) {
            ed.line = t.before.clone();
            return stay(self, ed);
        }
        let before = ed.line.buf.len();
        match k.code {
            KeyCode::Tab | KeyCode::BackTab if matches!(kind, FieldKind::Ref | FieldKind::Path) && self.complete_on() => {
                let back = k.code == KeyCode::BackTab || k.modifiers.contains(KeyModifiers::SHIFT);
                ed.tab = match tab {
                    Some(mut t) => {
                        t.next(back);
                        Some(t)
                    }
                    None => {
                        let near: Vec<String> = if kind == FieldKind::Ref { ref_options(spec, &ed.line.buf) } else { path_options(spec, &ed.line.buf) }.into_iter().map(String::from).collect();
                        // the first Tab takes the name the cursor is on, as `→` does
                        let pick = ed.pick.min(near.len().saturating_sub(1));
                        Step::begin(&ed.line, 0, ed.line.chars(), near, back).map(|t| if back { t } else { Step { at: pick, ..t } })
                    }
                };
                // the name in the line is the one `⏎` keeps
                if let Some(t) = &ed.tab {
                    (ed.line, ed.pick) = (t.line(), 0);
                }
            }
            KeyCode::Tab | KeyCode::BackTab => {}
            KeyCode::Esc => return Vec::new(),
            KeyCode::Enter if kind == FieldKind::Markdown => ed.line.insert('\n'),
            KeyCode::Char('s') if ctrl && kind == FieldKind::Markdown => return self.keep(spec, ed),
            KeyCode::Enter if k.modifiers.intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) && kind == FieldKind::Text => ed.line.insert('\n'),
            KeyCode::Char('j') if ctrl && matches!(kind, FieldKind::Text | FieldKind::Markdown) => ed.line.insert('\n'),
            KeyCode::Enter if kind == FieldKind::Ref => {
                let near = ref_options(spec, &ed.line.buf);
                return match near.get(ed.pick.min(near.len().saturating_sub(1))) {
                    Some(o) => {
                        ed.line = Typed::new(*o);
                        self.keep(spec, ed)
                    }
                    None => stay(self, ed),
                };
            }
            KeyCode::Enter => return self.keep(spec, ed),
            KeyCode::Up | KeyCode::Down if matches!(kind, FieldKind::Ref | FieldKind::Path) => {
                let n = if kind == FieldKind::Ref { ref_options(spec, &ed.line.buf).len() } else { path_options(spec, &ed.line.buf).len() };
                ed.pick = if k.code == KeyCode::Down { (ed.pick + 1).min(n.saturating_sub(1)) } else { ed.pick.saturating_sub(1) };
            }
            // `→` at the end of the path, or `⌃→` anywhere, takes the name it could be (Tab never types)
            KeyCode::Right if kind == FieldKind::Path && (ctrl || ed.line.at >= ed.line.chars()) => {
                let near = path_options(spec, &ed.line.buf);
                if let Some(o) = near.get(ed.pick.min(near.len().saturating_sub(1))) {
                    ed.line = Typed::new(*o);
                    ed.pick = 0;
                }
            }
            KeyCode::Left | KeyCode::Right if matches!(kind, FieldKind::Int | FieldKind::Num) => step(spec, &mut ed, k.code == KeyCode::Right),
            // a key's cursor stays at its end: nothing of it is drawn to move in
            KeyCode::Left | KeyCode::Right | KeyCode::Home | KeyCode::End | KeyCode::Delete if ed.secret => {}
            _ if ed.line.cursor_key(&k) => {}
            KeyCode::Char('u') if ctrl => {
                ed.line.buf.zeroize();
                ed.line.at = 0;
            }
            KeyCode::Backspace if kind == FieldKind::Argv && ed.line.buf.is_empty() => {
                if let Some(last) = ed.items.pop() {
                    ed.line = Typed::new(last);
                }
            }
            KeyCode::Backspace => {
                ed.line.erase(false);
                ed.pick = 0;
            }
            KeyCode::Delete => drop(ed.line.erase(true)),
            KeyCode::Char(' ') if kind == FieldKind::Argv => {
                if !ed.line.buf.is_empty() {
                    let done = ed.line.take();
                    ed.items.push(done);
                }
            }
            _ => {
                if let Some(c) = printable(&k) {
                    let ok = match kind {
                        FieldKind::Int => c.is_ascii_digit() || c == '-',
                        FieldKind::Num => c.is_ascii_digit() || matches!(c, '-' | '.'),
                        _ => true,
                    };
                    if ok {
                        ed.line.insert(c);
                        ed.pick = 0;
                    }
                }
            }
        }
        if ed.secret && ed.line.buf.len() != before {
            ed.how = How::Typed;
        }
        stay(self, ed)
    }

    fn ticking_key(&mut self, spec: &Spec, mut ed: Ed, k: KeyEvent, value: &Json) -> Vec<Up> {
        let options = spec.words("options");
        let per = columns(spec, self.size.0 as usize);
        let last = options.len().saturating_sub(1);
        match k.code {
            KeyCode::Left => ed.pick = ed.pick.saturating_sub(1),
            KeyCode::Right => ed.pick = (ed.pick + 1).min(last),
            KeyCode::Up => ed.pick = ed.pick.saturating_sub(per),
            KeyCode::Down => ed.pick = (ed.pick + per).min(last),
            KeyCode::Enter | KeyCode::Esc => return Vec::new(),
            KeyCode::Char(' ') => {
                let Some(name) = options.get(ed.pick) else { return Vec::new() };
                let mut have = strings(value);
                if let Some(i) = have.iter().position(|h| h == name) {
                    have.remove(i);
                } else {
                    have.push((*name).into());
                }
                // in the order the options have
                have.sort_by_key(|h| options.iter().position(|o| o == h).unwrap_or(usize::MAX));
                self.val = Some(ed);
                return self.set_value(spec.bind, Json::from(have));
            }
            _ => {}
        }
        self.val = Some(ed);
        Vec::new()
    }

    fn listing_key(&mut self, spec: &Spec, mut ed: Ed, k: KeyEvent, value: &Json) -> Vec<Up> {
        let map = spec.kind == FieldKind::Map;
        let mut rows: Vec<(String, String)> = if map { pairs(value) } else { strings(value).into_iter().map(|s| (s, String::new())).collect() };
        let n = rows.len();
        let shift = k.modifiers.contains(KeyModifiers::SHIFT);
        if let Some(row) = ed.row {
            match k.code {
                KeyCode::Esc => ed.row = None,
                KeyCode::Enter => {
                    let (name, text) = match (map, row) {
                        (true, Row::New) => match ed.line.buf.split_once('=') {
                            Some((a, b)) => (a.trim().to_string(), b.trim().to_string()),
                            None => (String::new(), String::new()),
                        },
                        (true, Row::At(i)) => (rows[i].0.clone(), ed.line.buf.trim().to_string()),
                        (false, _) => (ed.line.buf.trim().to_string(), String::new()),
                    };
                    if map && !parse::name(&name) {
                        // a name that is no good stays open, with the note under it
                        self.val = Some(ed);
                        return Vec::new();
                    }
                    match row {
                        Row::New if name.is_empty() => {}
                        Row::New => {
                            if let Some(r) = rows.iter_mut().find(|(k, _)| *k == name && map) {
                                r.1 = text;
                            } else {
                                rows.push((name, text));
                            }
                        }
                        Row::At(i) if !map && name.is_empty() => {
                            rows.remove(i);
                        }
                        Row::At(i) => rows[i] = (name, text),
                    }
                    ed.row = None;
                    ed.line.take();
                    ed.pick = ed.pick.min(rows.len());
                    self.val = Some(ed);
                    return self.set_value(spec.bind, rows_json(map, &rows));
                }
                _ if ed.line.cursor_key(&k) => {}
                KeyCode::Char('u') if k.modifiers.contains(KeyModifiers::CONTROL) => ed.line = Typed::default(),
                KeyCode::Backspace => drop(ed.line.erase(false)),
                KeyCode::Delete => drop(ed.line.erase(true)),
                _ => {
                    if let Some(c) = printable(&k) {
                        ed.line.insert(c);
                    }
                }
            }
            self.val = Some(ed);
            return Vec::new();
        }
        match k.code {
            KeyCode::Esc => return Vec::new(),
            // shift and an arrow moves the row, and never the cursor
            KeyCode::Up | KeyCode::Down if shift => {
                let to = if k.code == KeyCode::Up { ed.pick.checked_sub(1) } else { Some(ed.pick + 1) };
                if let Some(to) = to.filter(|t| !map && ed.pick < n && *t < n) {
                    rows.swap(ed.pick, to);
                    ed.pick = to;
                    self.val = Some(ed);
                    return self.set_value(spec.bind, rows_json(map, &rows));
                }
            }
            KeyCode::Up => ed.pick = ed.pick.saturating_sub(1),
            KeyCode::Down => ed.pick = (ed.pick + 1).min(n),
            KeyCode::Char('d') if ed.pick < n => {
                rows.remove(ed.pick);
                ed.pick = ed.pick.min(rows.len());
                self.val = Some(ed);
                return self.set_value(spec.bind, rows_json(map, &rows));
            }
            KeyCode::Char('a' | '+') => {
                ed.row = Some(Row::New);
                ed.line = Typed::default();
            }
            KeyCode::Enter => {
                if ed.pick < n {
                    ed.row = Some(Row::At(ed.pick));
                    let text = if map { rows[ed.pick].1.clone() } else { rows[ed.pick].0.clone() };
                    ed.line = Typed::new(text);
                } else {
                    ed.row = Some(Row::New);
                    ed.line = Typed::default();
                }
            }
            _ => {}
        }
        self.val = Some(ed);
        Vec::new()
    }
}

fn rows_json(map: bool, rows: &[(String, String)]) -> Json {
    if map { Json::Object(rows.iter().map(|(k, v)| (k.clone(), Json::String(v.clone()))).collect()) } else { Json::from(rows.iter().map(|r| r.0.clone()).collect::<Vec<_>>()) }
}

/// Why a key was not taken: a key has one road, its masked editor on a provider's row, to J4n's key file.
pub(crate) const NO_ROAD: &str = "a key is typed only in its masked editor: Agents › Providers, ⏎ on its row · nothing was sent";

/// `←→` on a number: by `step`, held to `min` and `max`.
fn step(spec: &Spec, ed: &mut Ed, up: bool) {
    let by = spec.num("step").unwrap_or(1.0);
    let now = ed.line.buf.trim().parse::<f64>().unwrap_or(0.0);
    let mut next = if up { now + by } else { now - by };
    // 0.1 steps drift; round them to the step's own places
    let places = parse::num(by).split('.').nth(1).map_or(0, str::len) as i32;
    let p = 10f64.powi(places);
    next = (next * p).round() / p;
    if let Some(lo) = spec.num("min") {
        next = next.max(lo);
    }
    if let Some(hi) = spec.num("max") {
        next = next.min(hi);
    }
    ed.line = Typed::new(parse::num(next));
}
