//! A value field as rows: one function of the value, the props, the state and the edit. Nothing
//! here changes anything; the keys are in `edit`.

use super::edit::{Ed, How, Row};
use super::{Spec, State, Style as Pick, draft, pairs, parse, problem, strings, text_of};
use crate::display::{row, width_of};
use crate::fuzzy;
use crate::nodes::NARROW;
use crate::parts::{popup::{POP, frame}, rows::{lead, trunk}};
use crate::theme::Theme;
use crate::theme::glyph::{Slot, Tier};
use crate::theme::palette::Role as Hue;
use crate::wrap::fit;
use j4n_proto::{FieldKind, Json};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// What a field is drawn from.
pub struct View<'a> {
    pub th: &'a Theme,
    pub spec: Spec<'a>,
    pub value: &'a Json,
    pub st: State,
    pub ed: Option<&'a Ed>,
    pub width: usize,
    /// The last field of its stack: its pipe ends in the elbow and nothing hangs under it.
    pub last: bool,
}

type Spans = Vec<Span<'static>>;

/// The glyphs a key is masked with, whatever its length.
const MASK: usize = 8;

/// What the key's editor says under it, the longest first: the one that fits the row is drawn.
const KEY_HINTS: [&str; 3] = ["paste or type the key · ⏎ keeps it · esc drops it · J4n shows it to no one", "paste or type the key · ⏎ keeps · esc drops", "⏎ keeps · esc drops"];

/// The text cut into lines of at most `w` columns, a line break being a break.
fn chunks(text: &str, w: usize) -> Vec<String> {
    let w = w.max(1);
    let mut out = Vec::new();
    for raw in text.split('\n') {
        let (mut line, mut used) = (String::new(), 0);
        for c in raw.chars() {
            let cw = c.width().unwrap_or(0);
            if used + cw > w {
                out.push(std::mem::take(&mut line));
                used = 0;
            }
            line.push(c);
            used += cw;
        }
        out.push(line);
    }
    out
}

pub fn rows(v: &View) -> Vec<Line<'static>> {
    if v.st == State::Editing && v.ed.is_some() { v.editing() } else { v.resting() }
}

impl View<'_> {
    fn off(&self) -> bool {
        self.st == State::Disabled
    }

    fn fg(&self, r: Hue) -> Style {
        if self.off() { self.th.fg(Hue::Faint) } else { self.th.fg(r) }
    }

    fn plain(&self) -> Style {
        if self.off() { self.th.fg(Hue::Faint) } else { Style::default() }
    }

    fn hl(&self) -> Style {
        self.th.back(Hue::Raise)
    }

    fn narrow(&self) -> bool {
        self.width < usize::from(NARROW)
    }

    fn mark(&self, slot: Slot) -> Span<'static> {
        let (g, s) = self.th.mark(slot);
        Span::styled(g, if self.off() { self.th.fg(Hue::Faint) } else { s })
    }

    /// A mark and a blank after it, whether the tier's glyph is an icon (which brings its own) or a letter or a word.
    fn mark_sp(&self, slot: Slot) -> Span<'static> {
        let mut m = self.mark(slot);
        if !m.content.ends_with(' ') {
            m.content = format!("{} ", m.content).into();
        }
        m
    }

    /// `←→`, or `<>` where the glyphs are plain letters.
    fn lr(&self) -> &'static str {
        if self.th.tier == Tier::Ascii { "<>" } else { "←→" }
    }

    /// The pipe at the start of a row: the tee for the first row of a field (the elbow for the last field of a stack), the bar under it (a blank under
    /// the last). The elbow stays while the field has the keys: that is the band behind it and the accent colour of its mark.
    fn lead(&self, first: bool, bad: bool) -> Span<'static> {
        let th = self.th;
        let keys = matches!(self.st, State::Focused | State::Editing);
        let mut span = match (first, self.last) {
            (true, last) => lead(th, last, keys, bad),
            (false, last) => trunk(th, last),
        };
        if self.off() {
            span.style = th.fg(Hue::Faint);
        }
        span
    }

    /// The spans padded to the width of the screen, with the selection behind them if `sel`.
    fn line(&self, spans: Spans, sel: bool) -> Line<'static> {
        row(self.th, spans, self.width, sel)
    }

    /// A row of the field under its first: the pipe, then `spans`.
    fn under(&self, spans: Spans, sel: bool) -> Line<'static> {
        let mut all = vec![self.lead(false, false)];
        all.extend(spans);
        self.line(all, sel)
    }

    /// The label at the left and the value at the right, on one row; under 60 columns the label has a row and the value the next.
    fn pair(&self, bad: bool, label: Style, value: Spans, sel: bool) -> Vec<Line<'static>> {
        let name = self.spec.label;
        if self.narrow() {
            let first = self.line(vec![self.lead(true, bad), Span::styled(name.to_string(), label)], sel);
            let mut second = vec![self.lead(false, false)];
            second.extend(value);
            return vec![first, self.line(second, sel)];
        }
        let room = self.width.saturating_sub(2);
        let vw = width_of(&value);
        let name = fit(name, room.saturating_sub(vw + 1).max(1));
        let pad = room.saturating_sub(name.width() + vw).max(1);
        let mut all = vec![self.lead(true, bad), Span::styled(name, label), Span::raw(" ".repeat(pad))];
        all.extend(value);
        vec![self.line(all, sel)]
    }

    /// The room a value has beside the label.
    fn budget(&self) -> usize {
        if self.narrow() {
            return self.width.saturating_sub(2);
        }
        let room = self.width.saturating_sub(2);
        room.saturating_sub(self.spec.label.width().min(room / 2) + 2)
    }

    // ---- at rest ----

    fn resting(&self) -> Vec<Line<'static>> {
        let bad = if self.off() { None } else { problem(&self.spec, self.value) };
        let focused = self.st == State::Focused;
        let th = self.th;
        // the mark after a value that is not to be changed or not right takes three columns
        let marked = if self.off() || bad.is_some() { 3 } else { 0 };
        let mut value = self.value_spans(self.budget().saturating_sub(marked), focused);
        if self.off() {
            value.iter_mut().for_each(|s| s.style = th.fg(Hue::Faint));
            value.push(Span::raw(" "));
            value.push(self.mark(Slot::ReadOnly));
        } else if bad.is_some() {
            value.iter_mut().for_each(|s| s.style = th.fg(Hue::Error));
            value.push(Span::raw(" "));
            value.push(Span::styled(th.mark(Slot::Error).0, th.fg(Hue::Error)));
        }
        let label = if self.off() {
            th.fg(Hue::Faint)
        } else if focused || bad.is_some() {
            Style::default()
        } else {
            th.fg(Hue::Dim)
        };
        let mut out = self.pair(bad.is_some(), label, value, focused);
        if let Some(why) = bad {
            out.push(self.under(vec![Span::styled(why, th.fg(Hue::Error))], false));
        }
        out
    }

    /// What a value looks like on one row.
    fn value_spans(&self, budget: usize, focused: bool) -> Spans {
        let th = self.th;
        let v = self.value;
        let on = if focused { self.fg(Hue::Accent) } else { self.plain() };
        let faint = self.fg(Hue::Faint);
        let text = |s: String, st: Style| vec![Span::styled(s, st)];
        let or_dash = |s: String| if s.is_empty() { "—".to_string() } else { s };
        match self.spec.kind {
            FieldKind::Str | FieldKind::Path | FieldKind::Pattern | FieldKind::Dur if !(self.spec.kind == FieldKind::Dur && v.is_number()) => text(fit(&or_dash(text_of(v)), budget), on),
            FieldKind::Dur => text(parse::dur_short(v.as_u64().unwrap_or(0)), on),
            FieldKind::Text => {
                let s = text_of(v);
                let mut first = fit(s.lines().next().unwrap_or(""), budget);
                if s.lines().count() > 1 && !first.ends_with('…') {
                    first = fit(&format!("{first}…"), budget);
                }
                text(or_dash(first), on)
            }
            FieldKind::Int | FieldKind::Num => {
                let n = v.as_f64().map(parse::num).unwrap_or_else(|| "—".into());
                let n = match self.spec.text("unit") {
                    Some(u) if v.is_number() => format!("{n} {u}"),
                    _ => n,
                };
                if focused {
                    vec![Span::styled(format!("{} ", th.glyph(Slot::StepLeft)), on), Span::styled(n, on), Span::styled(format!(" {}", th.glyph(Slot::StepRight)), on)]
                } else {
                    text(n, on)
                }
            }
            FieldKind::Bool => {
                let yes = v.as_bool().unwrap_or(false);
                vec![self.mark(if yes { Slot::BoolOn } else { Slot::BoolOff })]
            }
            FieldKind::Enum => self.choice_spans(budget, focused),
            FieldKind::Multi => {
                let have = strings(v);
                let all = self.spec.words("options").len();
                let names = if have.is_empty() { "none".to_string() } else { have.join(" ") };
                let count = if all > 0 { format!(" {}/{all}", have.len()) } else { String::new() };
                let names = fit(&names, budget.saturating_sub(count.width()));
                vec![Span::styled(names, on), Span::styled(count, faint)]
            }
            FieldKind::Ref => {
                let s = fit(&or_dash(text_of(v)), budget.saturating_sub(2));
                vec![Span::styled(s, on), Span::styled(format!(" {}", th.glyph(Slot::Chevron)), faint)]
            }
            FieldKind::Seq => {
                let items = strings(v);
                let more = items.len().saturating_sub(1);
                let first = items.first().cloned().unwrap_or_else(|| "none".into());
                let tail = if more > 0 { format!(" +{more}") } else { String::new() };
                let mut out = vec![Span::styled(fit(&first, budget.saturating_sub(tail.width() + 2)), on), Span::styled(tail, on)];
                if focused {
                    out.push(Span::styled(format!(" {}", th.glyph(Slot::FoldOpen)), on));
                }
                out
            }
            FieldKind::Map => {
                let n = pairs(v).len();
                let mut out = vec![Span::styled(format!("{n} key{}", if n == 1 { "" } else { "s" }), on)];
                if focused {
                    out.push(Span::styled(format!(" {}", th.glyph(Slot::FoldOpen)), on));
                }
                out
            }
            FieldKind::Usd => {
                let x = v.as_f64().or_else(|| v.as_str().and_then(parse::usd));
                text(x.map_or_else(|| "—".into(), parse::money), on)
            }
            FieldKind::Argv => {
                let tokens = strings(v);
                if tokens.is_empty() {
                    return text("—".into(), on);
                }
                let mut out = Vec::new();
                for (i, t) in tokens.iter().enumerate() {
                    if i > 0 {
                        out.push(Span::raw(" "));
                    }
                    let chip = if focused { self.hl() } else { Style::default() };
                    out.extend(self.token(t, chip, on));
                }
                out
            }
            FieldKind::Expr => self.expr_spans(&fit(&or_dash(text_of(v)), budget), on),
            FieldKind::Secret => {
                let set = v.as_bool().unwrap_or(false) || v.as_str().is_some_and(|s| !s.is_empty());
                if set {
                    vec![self.mark_sp(Slot::Yes), Span::styled("set", self.fg(Hue::Ok))]
                } else {
                    vec![self.mark_sp(Slot::Queued), Span::styled("not set", self.fg(Hue::Faint))]
                }
            }
            FieldKind::Markdown => {
                let n = text_of(v).lines().count();
                vec![Span::styled(format!("{n} "), on), Span::styled(th.glyph(Slot::FoldClosed).to_string(), if focused { on } else { faint })]
            }
            _ => Vec::new(),
        }
    }

    /// An argument: `{holes}` in the accent colour, the rest in `base`, on `chip`.
    fn token(&self, t: &str, chip: Style, base: Style) -> Spans {
        let hole = self.fg(Hue::Accent);
        let mut out = Vec::new();
        let (mut rest, mut plain) = (t, String::new());
        let flush = |plain: &mut String, out: &mut Spans| {
            if !plain.is_empty() {
                out.push(Span::styled(std::mem::take(plain), chip.patch(base)));
            }
        };
        while let Some(i) = rest.find('{') {
            let Some(j) = rest[i..].find('}') else { break };
            plain.push_str(&rest[..i]);
            flush(&mut plain, &mut out);
            out.push(Span::styled(rest[i..=i + j].to_string(), chip.patch(hole)));
            rest = &rest[i + j + 1..];
        }
        plain.push_str(rest);
        flush(&mut plain, &mut out);
        out
    }

    /// An expression: a name that is called is in the accent colour, a quoted text in the colour of the good.
    fn expr_spans(&self, s: &str, base: Style) -> Spans {
        let chars: Vec<char> = s.chars().collect();
        let (mut out, mut i) = (Vec::new(), 0);
        let word = |c: char| c.is_alphanumeric() || c == '_';
        while i < chars.len() {
            let c = chars[i];
            let mut j = i + 1;
            let style = if c == '\'' || c == '"' {
                while j < chars.len() && chars[j] != c {
                    j += 1;
                }
                j = (j + 1).min(chars.len());
                self.fg(Hue::Ok)
            } else if word(c) {
                while j < chars.len() && word(chars[j]) {
                    j += 1;
                }
                if chars.get(j) == Some(&'(') { self.fg(Hue::Accent) } else { base }
            } else {
                while j < chars.len() && !word(chars[j]) && chars[j] != '\'' && chars[j] != '"' {
                    j += 1;
                }
                base
            };
            out.push(Span::styled(chars[i..j].iter().collect::<String>(), style));
            i = j;
        }
        out
    }

    fn choice_spans(&self, budget: usize, focused: bool) -> Spans {
        let th = self.th;
        let v = text_of(self.value);
        let on = if focused { self.fg(Hue::Accent) } else { self.plain() };
        let options = self.spec.words("options");
        match self.spec.style() {
            Pick::Dropdown => vec![Span::styled(fit(&v, budget.saturating_sub(2)), on), Span::styled(format!(" {}", th.glyph(Slot::FoldOpen)), if focused { on } else { self.fg(Hue::Faint) })],
            Pick::Radio if focused => vec![Span::styled(format!("{} ", th.glyph(Slot::StepLeft)), on), Span::styled(v, on), Span::styled(format!(" {}", th.glyph(Slot::StepRight)), on)],
            Pick::Segmented if focused && !options.is_empty() => self.segments(&options, options.iter().position(|o| *o == v)),
            _ => vec![Span::styled(fit(&v, budget), on)],
        }
    }

    /// The options in a row, the chosen one inverse: `in│bord│none`.
    fn segments(&self, options: &[&str], at: Option<usize>) -> Spans {
        let mut out = Vec::new();
        for (i, o) in options.iter().enumerate() {
            if i > 0 {
                out.push(Span::styled(self.th.glyph(Slot::BoxV).to_string(), self.fg(Hue::Edge)));
            }
            if at == Some(i) {
                out.push(Span::styled(format!(" {o} "), self.th.inverse()));
            } else {
                out.push(Span::styled(o.to_string(), self.fg(Hue::Dim)));
            }
        }
        out
    }

    // ---- being edited ----

    /// The text with its caret: the part before it, the caret, the rest, all on the raised background, cut to `budget` columns from
    /// the left so that the caret is in sight.
    fn typed(&self, ed: &Ed, secret: bool, budget: usize) -> Spans {
        let th = self.th;
        if secret {
            return self.mask(ed, budget);
        }
        let chars: Vec<char> = ed.line.buf.chars().collect();
        let at = ed.line.at.min(chars.len());
        let cw = |c: &char| c.width().unwrap_or(0);
        let mut from = 0;
        while from < at && chars[from..at].iter().map(cw).sum::<usize>() + 1 > budget {
            from += 1;
        }
        let (mut before, mut after) = (String::new(), String::new());
        let mut used = chars[from..at].iter().map(cw).sum::<usize>() + 1;
        before.extend(&chars[from..at]);
        for c in &chars[at..] {
            if used + cw(c) > budget {
                break;
            }
            after.push(*c);
            used += cw(c);
        }
        let body = self.hl();
        vec![Span::styled(before, body), Span::styled(th.glyph(Slot::Caret).to_string(), th.fg(Hue::Accent)), Span::styled(after, body)]
    }

    /// A key being typed: a mask of [`MASK`] glyphs whatever its length, the caret after it, and how it came (`typed`, `pasted · 1 line`);
    /// nothing at all of it while it is empty. Nothing here reads a character of it.
    fn mask(&self, ed: &Ed, budget: usize) -> Spans {
        let th = self.th;
        let caret = Span::styled(th.glyph(Slot::Caret).to_string(), th.fg(Hue::Accent));
        if ed.line.buf.is_empty() {
            return vec![caret];
        }
        let word = match &ed.how {
            How::Typed => "typed".to_string(),
            How::Pasted => "pasted · 1 line".to_string(),
            How::Named(name) => format!("pasted {name}=… · the name is left out"),
            How::Refused(_) => String::new(),
        };
        let room = budget.saturating_sub(MASK + 2);
        vec![Span::styled(th.glyph(Slot::Mask).repeat(MASK), th.fg(Hue::Dim).patch(self.hl())), caret, Span::styled(if word.is_empty() || room == 0 { String::new() } else { format!(" {}", fit(&word, room)) }, th.fg(Hue::Faint))]
    }

    /// The keys that end an edit: `⏎ ✓  esc ✗`.
    fn keep(&self) -> Spans {
        let th = self.th;
        let f = th.fg(Hue::Faint);
        let (yes, no) = (th.mark(Slot::Yes), th.mark(Slot::No));
        vec![Span::styled(format!("{} ", th.glyph(Slot::KeyEnter)), f), Span::styled(yes.0, yes.1), Span::styled(" esc ", f), Span::styled(no.0, no.1)]
    }

    fn faint(&self, s: impl Into<String>) -> Spans {
        vec![Span::styled(s.into(), self.th.fg(Hue::Faint))]
    }

    fn bad(&self, why: String) -> Spans {
        vec![Span::styled(why, self.th.fg(Hue::Error))]
    }

    /// The row under an input: why the draft is no good, or `ok` (the hint).
    fn note(&self, draft: Result<Json, String>, ok: Spans) -> Line<'static> {
        self.under(draft.err().map_or(ok, |why| self.bad(why)), false)
    }

    fn editing(&self) -> Vec<Line<'static>> {
        let Some(ed) = self.ed else { return Vec::new() };
        let th = self.th;
        let spec = &self.spec;
        let items = &ed.items;
        let label = Style::default();
        let try_ = || draft(spec, &ed.line.buf, items);
        let input = |secret: bool| {
            let budget = if self.narrow() { self.width.saturating_sub(3) } else { self.budget().max(8) };
            self.pair(false, label, self.typed(ed, secret, budget), false)
        };
        match spec.kind {
            FieldKind::Str => {
                let mut out = input(false);
                out.push(self.note(try_(), self.keep()));
                out
            }
            FieldKind::Int | FieldKind::Num => {
                let step = spec.num("step").unwrap_or(1.0);
                let range = parse::range(spec.num("min"), spec.num("max"));
                let mut hint = format!("{} {}{}", self.lr(), if th.tier == Tier::Ascii { "+-" } else { "±" }, parse::num(step));
                if !range.is_empty() {
                    hint.push_str(&format!(" · {range}"));
                }
                let mut out = input(false);
                out.push(self.note(try_(), self.faint(hint)));
                out
            }
            FieldKind::Dur => {
                let words = parse::dur(&ed.line.buf).map(|s| format!("= {}", parse::dur_words(s)));
                let mut out = input(false);
                out.push(self.note(try_(), words.map_or_else(|| self.keep(), |w| self.faint(w))));
                out
            }
            FieldKind::Usd => {
                let hard = spec.num("hard").zip(parse::usd(&ed.line.buf)).map(|(f, x)| format!("hard = {}", parse::money(x * f)));
                let mut out = input(false);
                out.push(self.note(try_(), hard.map_or_else(|| self.keep(), |h| self.faint(h))));
                out
            }
            FieldKind::Pattern => {
                let count = spec.num("matches").map(|n| {
                    let mut m = th.mark(Slot::Output).0;
                    if !m.ends_with(' ') {
                        m.push(' ');
                    }
                    vec![Span::styled(m, th.fg(Hue::Faint)), Span::styled(format!("{} match", n as u64), th.fg(Hue::Faint))]
                });
                let mut out = input(false);
                out.push(self.note(try_(), count.unwrap_or_else(|| self.keep())));
                out
            }
            FieldKind::Secret => {
                let mut out = input(true);
                // the longest of the hints that fits the row: what to do, the keys, and that it is shown to no one
                let room = self.width.saturating_sub(2);
                let hint = KEY_HINTS.iter().find(|h| h.width() <= room).unwrap_or(&KEY_HINTS[KEY_HINTS.len() - 1]);
                let said = if let How::Refused(why) = &ed.how { self.bad(why.clone()) } else { self.faint(*hint) };
                out.push(self.under(said, false));
                out
            }
            FieldKind::Path => {
                let mut out = input(false);
                let near = path_options(spec, &ed.line.buf);
                if near.is_empty() {
                    out.push(self.note(try_(), self.keep()));
                } else {
                    for (i, o) in near.iter().take(4).enumerate() {
                        out.push(self.under(vec![self.mark_sp(Slot::Folder), Span::raw(o.to_string())], i == ed.pick.min(near.len() - 1)));
                    }
                }
                out
            }
            FieldKind::Ref => {
                let mut out = input(false);
                let near = ref_options(spec, &ed.line.buf);
                if near.is_empty() {
                    out.push(self.under(self.bad("not found".into()), false));
                }
                let q: Vec<char> = ed.line.buf.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).collect();
                for (i, o) in near.iter().take(fuzzy::KEEP.min(5)).enumerate() {
                    let at = fuzzy::positions(&q, o);
                    let mut spans = vec![Span::raw("  ")];
                    for (ci, c) in o.chars().enumerate() {
                        spans.push(Span::styled(c.to_string(), if at.contains(&ci) { th.fg(Hue::Accent).add_modifier(Modifier::BOLD) } else { Style::default() }));
                    }
                    out.push(self.under(spans, i == ed.pick.min(near.len() - 1)));
                }
                out
            }
            FieldKind::Bool => {
                let now = ed.pick == 1;
                let mut out = self.pair(false, label, vec![self.mark(if now { Slot::BoolOn } else { Slot::BoolOff })], false);
                out.push(self.under(self.faint("space flips"), false));
                out
            }
            FieldKind::Text | FieldKind::Markdown | FieldKind::Expr => self.boxed(ed),
            FieldKind::Enum => self.choosing(ed),
            FieldKind::Multi => self.ticking(ed),
            FieldKind::Seq | FieldKind::Map => self.listing(ed),
            FieldKind::Argv => {
                let mut value: Spans = Vec::new();
                for t in &ed.items {
                    value.extend(self.token(t, self.hl(), Style::default()));
                    value.push(Span::raw(" "));
                }
                let mut typed = self.typed(ed, false, self.budget().max(8));
                value.append(&mut typed);
                let mut out = self.pair(false, label, value, false);
                out.push(self.note(try_(), self.faint("space new arg · { hole")));
                out
            }
            _ => self.resting(),
        }
    }

    /// Text over several rows on a raised box, with the hint or the check under it.
    fn boxed(&self, ed: &Ed) -> Vec<Line<'static>> {
        let th = self.th;
        let w = self.width.saturating_sub(2).max(1);
        let rows = match self.spec.kind {
            FieldKind::Markdown => self.spec.num("rows").map_or(6, |r| r as usize),
            _ => self.spec.num("rows").map_or(3, |r| r as usize),
        }
        .max(1);
        // the caret is a character of the text while it is drawn
        let mut with: Vec<char> = ed.line.buf.chars().collect();
        with.insert(ed.line.at.min(with.len()), '\u{1}');
        let lines = chunks(&with.iter().collect::<String>(), w);
        let line_of = lines.iter().position(|l| l.contains('\u{1}')).unwrap_or(0);
        let start = (line_of + 1).saturating_sub(rows);
        let mut out = vec![self.line(vec![self.lead(true, false), Span::styled(self.spec.label.to_string(), Style::default())], false)];
        for l in lines.iter().skip(start).take(rows) {
            let mut spans = Vec::new();
            for (i, part) in l.split('\u{1}').enumerate() {
                if i > 0 {
                    spans.push(Span::styled(th.glyph(Slot::Caret).to_string(), th.fg(Hue::Accent)));
                }
                let style = if self.spec.kind == FieldKind::Markdown && part.starts_with('#') { self.hl().patch(th.fg(Hue::Accent)).add_modifier(Modifier::BOLD) } else { self.hl() };
                if self.spec.kind == FieldKind::Expr {
                    spans.extend(self.expr_spans(part, Style::default()).into_iter().map(|s| Span::styled(s.content, self.hl().patch(s.style))));
                } else {
                    spans.push(Span::styled(part.to_string(), style));
                }
            }
            let used = width_of(&spans);
            if used < w {
                spans.push(Span::styled(" ".repeat(w - used), self.hl()));
            }
            out.push(self.under(spans, false));
        }
        let kept = draft(&self.spec, &ed.line.buf, &[]);
        let hint: Spans = match self.spec.kind {
            FieldKind::Expr => match self.spec.text("check") {
                Some(c) => vec![self.mark_sp(Slot::Yes), Span::styled(c.to_string(), th.fg(Hue::Faint))],
                None => self.keep(),
            },
            FieldKind::Markdown => vec![Span::styled(format!("{}{} keep  esc ", th.glyph(Slot::KeyCtrl), "s"), th.fg(Hue::Faint)), self.mark(Slot::No)],
            _ => {
                let (yes, f) = (th.mark(Slot::Yes), th.fg(Hue::Faint));
                vec![Span::styled(format!("{}{} line  {} ", th.glyph(Slot::KeyShift), th.glyph(Slot::KeyEnter), th.glyph(Slot::KeyEnter)), f), Span::styled(yes.0, yes.1)]
            }
        };
        out.push(self.note(kept, hint));
        out
    }

    /// A choice being made.
    fn choosing(&self, ed: &Ed) -> Vec<Line<'static>> {
        let th = self.th;
        let options = self.spec.words("options");
        let now = text_of(self.value);
        let at = ed.pick.min(options.len().saturating_sub(1));
        match self.spec.style() {
            Pick::Radio => {
                let mut out = vec![self.line(vec![self.lead(true, false), Span::styled(self.spec.label.to_string(), Style::default())], false)];
                for (i, o) in options.iter().enumerate() {
                    out.push(self.under(vec![self.mark_sp(if *o == now { Slot::RadioOn } else { Slot::RadioOff }), Span::raw(o.to_string())], i == at));
                }
                out
            }
            Pick::Segmented => {
                let mut out = self.pair(false, Style::default(), self.segments(&options, Some(at)), false);
                out.push(self.under(self.faint(self.lr()), false));
                out
            }
            Pick::Dropdown => {
                let mut out = self.pair(false, Style::default(), vec![Span::styled(format!("{now} {}", th.glyph(Slot::FoldOpen)), th.fg(Hue::Accent))], false);
                let inner = options.iter().map(|o| o.width()).max().unwrap_or(0) + 2;
                let at_col = self.width.saturating_sub(2 + inner + 2);
                let edge = th.fg(Hue::Edge);
                let rows = options.iter().enumerate().map(|(i, o)| {
                    let cell = format!("{} {o}", if i == at { th.glyph(Slot::FoldClosed) } else { " " });
                    let cell = format!("{cell}{}", " ".repeat((inner).saturating_sub(cell.width())));
                    vec![Span::styled(cell, if i == at { th.back(Hue::Sel) } else { Style::default() })]
                });
                for l in frame(th, rows.collect(), inner + 2, "", edge, &POP) {
                    out.push(self.under([vec![Span::raw(" ".repeat(at_col))], l.spans].concat(), false));
                }
                out
            }
        }
    }

    /// Options to tick, in columns.
    fn ticking(&self, ed: &Ed) -> Vec<Line<'static>> {
        let options = self.spec.words("options");
        let have = strings(self.value);
        let cell = options.iter().map(|o| o.width()).max().unwrap_or(0) + 2 + 2;
        let per = columns(&self.spec, self.width);
        let at = ed.pick.min(options.len().saturating_sub(1));
        let mut out = vec![self.line(vec![self.lead(true, false), Span::styled(self.spec.label.to_string(), Style::default())], false)];
        for (r, chunk) in options.chunks(per).enumerate() {
            let mut spans = Vec::new();
            for o in chunk {
                let ticked = have.iter().any(|h| h == o);
                spans.push(self.mark_sp(if ticked { Slot::TickOn } else { Slot::TickOff }));
                let name = format!("{o}{}", " ".repeat(cell - 2 - o.width()));
                spans.push(Span::raw(name));
            }
            out.push(self.under(spans, r == at / per));
        }
        let mut hint = self.faint("space ticks");
        if let Some(why) = problem(&self.spec, self.value) {
            hint = self.bad(why);
        }
        out.push(self.under(hint, false));
        out
    }

    /// The rows of a list or a map, and the row that adds one.
    fn listing(&self, ed: &Ed) -> Vec<Line<'static>> {
        let th = self.th;
        let map = self.spec.kind == FieldKind::Map;
        let rows: Vec<(String, String)> = if map { pairs(self.value) } else { strings(self.value).into_iter().map(|s| (s, String::new())).collect() };
        let mut out = vec![self.line(vec![self.lead(true, false), Span::styled(self.spec.label.to_string(), Style::default())], false)];
        let (tee, elbow) = (lead(th, false, false, false), lead(th, true, false, false));
        let typing = || self.typed(ed, false, self.width.saturating_sub(8).max(6));
        for (i, (k, v)) in rows.iter().enumerate() {
            let sel = i == ed.pick && ed.row.is_none();
            let mut spans = vec![tee.clone()];
            match ed.row {
                Some(Row::At(at)) if at == i => {
                    if map {
                        spans.push(Span::styled(k.clone(), th.fg(Hue::Dim)));
                        spans.push(Span::styled(" = ", th.fg(Hue::Faint)));
                    }
                    spans.extend(typing());
                }
                _ if map => {
                    spans.push(Span::styled(k.clone(), th.fg(Hue::Dim)));
                    spans.push(Span::styled(" = ", th.fg(Hue::Faint)));
                    spans.push(Span::raw(v.clone()));
                }
                _ => spans.push(Span::raw(k.clone())),
            }
            out.push(self.under(spans, sel || ed.row == Some(Row::At(i))));
        }
        if matches!(ed.row, Some(Row::New)) {
            let mut spans = vec![tee.clone()];
            spans.extend(typing());
            out.push(self.under(spans, true));
        }
        let add_sel = ed.pick >= rows.len() && ed.row.is_none();
        let mut tail = vec![elbow, self.mark(Slot::Add)];
        tail.push(Span::styled(" d ".to_string(), th.fg(Hue::Faint)));
        tail.push(self.mark(Slot::Remove));
        if !map {
            tail.push(Span::styled(format!(" {}↑↓ move", th.glyph(Slot::KeyShift)), th.fg(Hue::Faint)));
        }
        out.push(self.under(tail, add_sel));
        if let Some(why) = problem(&self.spec, self.value) {
            out.push(self.under(self.bad(why), false));
        } else if map && ed.row == Some(Row::New) {
            out.push(self.under(self.faint("name=value"), false));
        }
        out
    }
}

/// How many options of a multi are in a row of the box, at this width.
pub(crate) fn columns(spec: &Spec, width: usize) -> usize {
    let cell = spec.words("options").iter().map(|o| o.width()).max().unwrap_or(0) + 2 + 2;
    (width.saturating_sub(2) / cell.max(1)).max(1)
}

/// The paths a path could be completed to: the options that start with what is typed.
pub(crate) fn path_options<'a>(spec: &Spec<'a>, typed: &str) -> Vec<&'a str> {
    spec.words("options").into_iter().filter(|o| o.starts_with(typed) && *o != typed).collect()
}

/// The entries a ref could be: those the typed letters are in, the best first.
pub(crate) fn ref_options<'a>(spec: &Spec<'a>, typed: &str) -> Vec<&'a str> {
    let q: Vec<char> = typed.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).collect();
    let mut scored: Vec<(i32, usize, &str)> = spec.words("options").into_iter().enumerate().filter_map(|(i, o)| fuzzy::score(&q, o).map(|s| (s, i, o))).collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, _, o)| o).collect()
}
