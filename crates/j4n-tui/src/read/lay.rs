//! The layout of the reader: the events of `pulldown-cmark` as lines, container by container.

use super::inline::{RLine, Run, wrap};
use super::Opts;
use crate::theme::glyph::{Slot, Tier};
use crate::theme::palette::Role;
use crate::theme::Theme;
use crate::wrap::fit;
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use unicode_width::UnicodeWidthStr;

enum Ctx {
    Root,
    Quote,
    /// A list: the number its next item has (none for a bullet list), and the width of the numbers.
    List(Option<u64>, usize),
    /// An item: the marker of its first line, until it is drawn, and the cells it takes.
    Item(Option<Vec<Span<'static>>>, usize),
}

pub(crate) struct Lay<'a> {
    th: &'a Theme,
    o: &'a Opts,
    base: Style,
    width: usize,
    out: Vec<RLine>,
    ctx: Vec<Ctx>,
    /// How many blocks each container has drawn, for the blank row between two.
    blocks: Vec<usize>,
    runs: Vec<Run>,
    styles: Vec<Style>,
    link: Option<String>,
    heading: Option<HeadingLevel>,
    code: Option<(String, String)>,
    table: Option<Vec<Vec<String>>>,
    cell: String,
    /// The ids of the docs already given their words: an id is explained where it is first named, once in an answer.
    told: std::collections::BTreeSet<String>,
}

impl<'a> Lay<'a> {
    pub(crate) fn new(th: &'a Theme, o: &'a Opts, base: Style, width: usize) -> Lay<'a> {
        Lay { th, o, base, width, out: Vec::new(), ctx: vec![Ctx::Root], blocks: vec![0], runs: Vec::new(), styles: Vec::new(), link: None, heading: None, code: None, table: None, cell: String::new(), told: Default::default() }
    }

    fn fg(&self, r: Role) -> Style {
        self.th.fg(r)
    }

    fn italic(&self) -> Style {
        if self.o.caps.italic { Style::default().add_modifier(Modifier::ITALIC) } else { Style::default() }
    }

    fn quoted(&self) -> bool {
        self.ctx.iter().any(|c| matches!(c, Ctx::Quote))
    }

    /// The style the words of the block are in before an inline mark changes it.
    fn now(&self) -> Style {
        let mut s = self.base;
        if self.quoted() {
            s = self.th.tone(j4n_proto::Tone::Dim).patch(self.italic());
        }
        if self.heading.is_some() {
            s = self.fg(Role::Head).add_modifier(Modifier::BOLD);
        }
        self.styles.iter().fold(s, |s, p| s.patch(*p))
    }

    /// The glyph of a slot as a marker takes it: an icon has a blank more, so that it never touches the word after it.
    fn mark(&self, slot: Slot) -> String {
        let g = self.th.glyph(slot);
        if g.chars().next().is_some_and(|c| c >= '\u{E000}') { format!("{g}  ") } else { format!("{g} ") }
    }

    // ---- the prefix of a line: what the containers put before it ----

    fn indent(&self) -> usize {
        self.ctx.iter().map(|c| match c { Ctx::Quote => 2, Ctx::Item(_, w) => *w, _ => 0 }).sum()
    }

    fn bar(&self) -> Span<'static> {
        Span::styled(format!("{} ", self.th.glyph(Slot::QuoteBar)), self.th.tone(j4n_proto::Tone::Dim))
    }

    fn prefix(&mut self, first: bool) -> Vec<Span<'static>> {
        let mut out = Vec::new();
        for i in 0..self.ctx.len() {
            match &mut self.ctx[i] {
                Ctx::Quote => out.push(self.bar()),
                Ctx::Item(marker, w) => match if first { marker.take() } else { None } {
                    Some(m) => out.extend(m),
                    None => out.push(Span::raw(" ".repeat(*w))),
                },
                _ => {}
            }
        }
        out
    }

    /// A block begins in the container: a blank row after the one before it (calm only), counted for the container. A list inside an item
    /// follows its item's words with no blank.
    fn begin(&mut self, list: bool) {
        let top = self.blocks.len() - 1;
        let had = self.blocks[top];
        self.blocks[top] += 1;
        let in_item = matches!(self.ctx.last(), Some(Ctx::Item(..)));
        if had == 0 || self.o.full || (list && in_item) {
            return;
        }
        let mut gap = RLine::default();
        if self.quoted() {
            let mut spans = self.prefix(false);
            if let Some(l) = spans.last_mut() {
                l.content = l.content.trim_end().to_string().into();
            }
            gap.spans = spans;
        }
        self.out.push(gap);
    }

    /// Lines of a block, with what the containers put in front of the first and of the rest.
    fn put(&mut self, body: Vec<RLine>) {
        self.begin(false);
        for (i, l) in body.into_iter().enumerate() {
            let pre = self.prefix(i == 0);
            self.out.push(l.after(&pre));
        }
    }

    fn room(&self) -> usize {
        self.width.saturating_sub(self.indent()).max(4)
    }

    // ---- blocks ----

    /// The words so far as a paragraph (or the heading).
    fn flush(&mut self) {
        if self.runs.is_empty() {
            return;
        }
        let runs = std::mem::take(&mut self.runs);
        let room = self.room();
        // a level-1 heading in kitty's text sizing is twice as wide and two rows high: it wraps at half the room, and the row under each of
        // its lines is blank for the terminal to draw into
        let big = matches!(self.heading, Some(HeadingLevel::H1)) && self.o.big_heads && self.o.caps.sizing && room >= 8;
        let mut body: Vec<RLine> = wrap(&runs, if big { room / 2 } else { room }).iter().map(|l| RLine::of(&l.iter().filter(|r| !r.text.is_empty()).cloned().collect::<Vec<_>>())).collect();
        if big {
            body = body
                .into_iter()
                .flat_map(|mut l| {
                    let words: String = l.spans.iter().map(|s| s.content.as_ref()).collect();
                    l.big = Some((0, words.trim_end().to_string())).filter(|b| !b.1.is_empty());
                    [l, RLine::default()]
                })
                .collect();
        }
        if let Some(HeadingLevel::H1) = self.heading {
            let w = body.iter().map(RLine::width).max().unwrap_or(0).max(1) * if big { 2 } else { 1 };
            let w = w.min(room).max(4);
            body.push(RLine { spans: vec![Span::styled(self.th.glyph(Slot::Rule).repeat(w), self.fg(Role::Edge))], ..RLine::default() });
        }
        self.put(body);
    }

    fn code_block(&mut self, lang: &str, text: &str) {
        let room = self.room();
        let band = self.th.back(Role::Raise);
        let rows: Vec<&str> = text.trim_end_matches('\n').split('\n').collect();
        let (shown, hidden) = if !self.o.full && self.o.fold > 0 && rows.len() > self.o.fold { (&rows[..self.o.fold], rows.len() - self.o.fold) } else { (&rows[..], 0) };
        let fill = |text: &str, style: Style| {
            let t = format!(" {text}");
            let pad = " ".repeat(room.saturating_sub(t.width() + 1));
            vec![Span::styled(t, style), Span::styled(format!("{pad} "), band)]
        };
        let dim = self.th.tone(j4n_proto::Tone::Dim).patch(band);
        let mut body = Vec::new();
        if !lang.is_empty() {
            let tag = format!("{lang} ");
            let pad = " ".repeat(room.saturating_sub(tag.width()));
            body.push(RLine { spans: vec![Span::styled(pad, band), Span::styled(tag, dim)], ..RLine::default() });
        }
        for r in shown {
            body.push(RLine { spans: fill(&fit(&r.replace('\t', "  "), room.saturating_sub(2).max(1)), band), ..RLine::default() });
        }
        if hidden > 0 {
            body.push(RLine { spans: fill(&format!("+{hidden} {}", self.th.glyph(Slot::FoldClosed)), dim), ..RLine::default() });
        }
        self.put(body);
    }

    fn rule(&mut self) {
        let w = self.room().min(40);
        self.put(vec![RLine { spans: vec![Span::styled(self.th.glyph(Slot::Rule).repeat(w), self.fg(Role::Edge))], ..RLine::default() }]);
    }

    fn table(&mut self, rows: &[Vec<String>]) {
        let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
        if cols == 0 {
            return;
        }
        let w: Vec<usize> = (0..cols).map(|c| rows.iter().filter_map(|r| r.get(c)).map(|s| s.width()).max().unwrap_or(0)).collect();
        let framed = w.iter().sum::<usize>() + 3 * cols + 1;
        let (edge, bold) = (self.fg(Role::Edge), self.base.add_modifier(Modifier::BOLD));
        let mut body = Vec::new();
        if framed > self.room() {
            // too wide for a frame: each row as `key: value` lines
            let head = &rows[0];
            for (n, row) in rows.iter().enumerate().skip(1) {
                if n > 1 && !self.o.full {
                    body.push(RLine::default());
                }
                for (c, cell) in row.iter().enumerate() {
                    let key = head.get(c).cloned().unwrap_or_default();
                    let runs = [Run::new(format!("{key}: "), self.th.tone(j4n_proto::Tone::Dim)), Run::new(cell.clone(), self.base)];
                    body.extend(wrap(&runs, self.room()).iter().map(|l| RLine::of(l)));
                }
            }
            if rows.len() == 1 {
                body.extend(rows[0].iter().map(|c| RLine::of(&[Run::new(c.clone(), bold)])));
            }
            return self.put(body);
        }
        let ascii = self.th.tier == Tier::Ascii;
        let j = |wide: &'static str| if ascii { "+" } else { wide };
        let h = self.th.glyph(Slot::BoxH);
        let line = |l: &str, m: &str, r: &str| {
            let cells: Vec<String> = w.iter().map(|n| h.repeat(n + 2)).collect();
            RLine { spans: vec![Span::styled(format!("{l}{}{r}", cells.join(m)), edge)], ..RLine::default() }
        };
        body.push(line(self.th.glyph(Slot::BoxTl), j("┬"), self.th.glyph(Slot::BoxTr)));
        for (n, row) in rows.iter().enumerate() {
            let mut spans = vec![Span::styled(self.th.glyph(Slot::BoxV), edge)];
            for (c, width) in w.iter().enumerate() {
                let t = row.get(c).map_or("", String::as_str);
                spans.push(Span::styled(format!(" {t}{} ", " ".repeat(width - t.width())), if n == 0 { bold } else { self.base }));
                spans.push(Span::styled(self.th.glyph(Slot::BoxV), edge));
            }
            body.push(RLine { spans, ..RLine::default() });
            if n == 0 && rows.len() > 1 {
                body.push(line(j("├"), j("┼"), j("┤")));
            }
        }
        body.push(line(self.th.glyph(Slot::BoxBl), j("┴"), self.th.glyph(Slot::BoxBr)));
        self.put(body);
    }

    // ---- inline ----

    fn text(&mut self, t: &str) {
        let style = self.now();
        // a path in prose is code, and opens its file where the terminal takes an address
        let mut plain = String::new();
        let linked = self.link.is_some();
        for (i, tok) in t.split(' ').enumerate() {
            if i > 0 {
                plain.push(' ');
            }
            let core = tok.trim_matches(|c: char| ",;:!?()[]\"'".contains(c)).trim_end_matches('.');
            // the ids of a token: a range `ADR-12–ADR-14`, a pair `ADR-12/BUG-7` and a possessive `UX3's` hold ids too
            let ids = if linked || self.code.is_some() { Vec::new() } else { self.ids_of(tok) };
            if !ids.is_empty() {
                // each id as it is, then once the words of those said for the first time, faint
                let (mut at, mut said) = (0, Vec::new());
                for (from, to, words) in ids {
                    plain.push_str(&tok[at..from]);
                    self.words(std::mem::take(&mut plain), style);
                    let id = &tok[from..to];
                    let mut run = Run::new(id, style);
                    run.term = Some(id.to_string());
                    self.runs.push(run);
                    if self.told.insert(id.to_string()) {
                        said.push(words);
                    }
                    at = to;
                }
                if !said.is_empty() {
                    self.runs.push(Run::new(format!(" · {}", said.join(" / ")), self.th.tone(j4n_proto::Tone::Dim)));
                }
                plain.push_str(&tok[at..]);
                continue;
            }
            if linked || self.code.is_some() || !is_path(core) {
                plain.push_str(tok);
                continue;
            }
            let at = tok.find(core).unwrap_or(0);
            plain.push_str(&tok[..at]);
            self.words(std::mem::take(&mut plain), style);
            let mut run = Run::new(core, self.fg(Role::Code).patch(self.th.back(Role::Raise)));
            run.atom = true;
            run.link = self.o.root.as_ref().filter(|_| self.o.osc8 && self.o.caps.osc8).filter(|r| !r.chars().any(char::is_control)).map(|r| format!("file://{}/{}", r.trim_end_matches('/'), core.split(':').next().unwrap_or(core).trim_start_matches("./")));
            self.runs.push(run);
            plain.push_str(&tok[at + core.len()..]);
        }
        self.words(plain, style);
    }

    /// The ids in a token that the docs define: where each starts and ends in it, and its words. A token is cut at `–` and `/`, and an id may
    /// carry a possessive (`ADR-12's`), brackets or a stop after it.
    fn ids_of(&self, tok: &str) -> Vec<(usize, usize, String)> {
        let Some(terms) = self.o.terms.as_ref() else { return Vec::new() };
        let mut found = Vec::new();
        let mut from = 0;
        for piece in tok.split_inclusive(['–', '/']) {
            let trimmed = piece.trim_matches(|c: char| ",;:!?()[]\"'.–/".contains(c));
            let id = trimmed.strip_suffix("'s").or_else(|| trimmed.strip_suffix("\u{2019}s")).unwrap_or(trimmed);
            if let Some(words) = terms.get(id) {
                let at = from + piece.find(id).unwrap_or(0);
                found.push((at, at + id.len(), words.clone()));
            }
            from += piece.len();
        }
        found
    }

    fn words(&mut self, text: String, style: Style) {
        if text.is_empty() {
            return;
        }
        let mut run = Run::new(text, style);
        run.link = self.link.clone().filter(|u| self.o.osc8 && self.o.caps.osc8 && safe(u));
        self.runs.push(run);
    }

    fn start_item(&mut self) {
        let depth = self.ctx.iter().filter(|c| matches!(c, Ctx::List(..))).count();
        let Some(Ctx::List(n, digits)) = self.ctx.iter_mut().rev().find(|c| matches!(c, Ctx::List(..))) else { return };
        let (num, digits) = (*n, *digits);
        if let Some(k) = n.as_mut() {
            *k += 1;
        }
        let dim = self.th.tone(j4n_proto::Tone::Dim);
        let marker = match num {
            Some(k) => format!("{k:>digits$}. "),
            None => self.mark(if depth > 1 { Slot::Bullet2 } else { Slot::Bullet }),
        };
        let w = marker.width();
        self.ctx.push(Ctx::Item(Some(vec![Span::styled(marker, dim)]), w));
        self.blocks.push(0);
    }

    pub(crate) fn run(mut self, text: &str) -> Vec<RLine> {
        let opts = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS;
        let events: Vec<Event> = Parser::new_ext(text, opts).collect();
        for (at, ev) in events.iter().enumerate() {
            if self.code.is_some() || self.table.is_some() {
                self.inside(ev);
                continue;
            }
            match ev {
                Event::Start(tag) => self.start(tag, &events[at + 1..]),
                Event::End(tag) => self.end(tag),
                Event::Text(t) => self.text(t),
                Event::Code(c) => {
                    let mut run = Run::new(c.to_string(), self.fg(Role::Code).patch(self.th.back(Role::Raise)));
                    run.atom = true;
                    self.runs.push(run);
                }
                // raw HTML is shown as the text it is, a line of the block at a time
                Event::Html(h) | Event::InlineHtml(h) => {
                    for (n, line) in h.trim_end_matches('\n').split('\n').enumerate() {
                        if n > 0 {
                            self.runs.push(Run::new("\n", Style::default()));
                        }
                        self.text(line);
                    }
                    if h.ends_with('\n') {
                        self.runs.push(Run::new("\n", Style::default()));
                    }
                }
                // a line break in the words is a line break on the screen: what the core says in lines (a list of tools, a notice with
                // several) stays lines, and a model that wraps its own text is not joined into one long line
                Event::SoftBreak | Event::HardBreak => self.runs.push(Run::new("\n", Style::default())),
                Event::Rule => self.rule(),
                Event::TaskListMarker(done) => {
                    let m = self.mark(if *done { Slot::TaskDone } else { Slot::TaskOpen });
                    let w = m.width();
                    let style = if *done { self.fg(Role::Ok) } else { self.th.tone(j4n_proto::Tone::Dim) };
                    if let Some(Ctx::Item(marker, width)) = self.ctx.last_mut() {
                        *marker = Some(vec![Span::styled(m, style)]);
                        *width = w;
                    }
                }
                _ => {}
            }
        }
        self.flush();
        self.out
    }

    /// The events of a fenced block or a table, which are gathered whole.
    fn inside(&mut self, ev: &Event) {
        match ev {
            Event::Text(t) | Event::Code(t) | Event::Html(t) | Event::InlineHtml(t) => match (&mut self.code, &mut self.table) {
                (Some((_, body)), _) => body.push_str(t),
                _ => self.cell.push_str(t),
            },
            Event::SoftBreak | Event::HardBreak => self.cell.push(' '),
            Event::Start(Tag::TableRow | Tag::TableHead) => {
                if let Some(rows) = &mut self.table {
                    rows.push(Vec::new());
                }
            }
            Event::End(TagEnd::TableCell) => {
                let cell = std::mem::take(&mut self.cell);
                if let Some(row) = self.table.as_mut().and_then(|r| r.last_mut()) {
                    row.push(cell);
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some((lang, body)) = self.code.take() {
                    self.code_block(&lang, &body);
                }
            }
            Event::End(TagEnd::Table) => {
                if let Some(rows) = self.table.take() {
                    self.table(&rows);
                }
            }
            _ => {}
        }
    }

    fn start(&mut self, tag: &Tag, rest: &[Event]) {
        match tag {
            Tag::Paragraph => {}
            Tag::Heading { level, .. } => {
                self.flush();
                self.heading = Some(*level);
            }
            Tag::BlockQuote(_) => {
                self.flush();
                self.begin(false);
                self.ctx.push(Ctx::Quote);
                self.blocks.push(0);
            }
            Tag::CodeBlock(kind) => {
                self.flush();
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => info.split_whitespace().next().unwrap_or("").to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.code = Some((lang, String::new()));
            }
            Tag::List(first) => {
                self.flush();
                self.begin(true);
                // the numbers are as wide as the last of them
                let items = count_items(rest);
                let digits = first.map_or(0, |n| (n + items.saturating_sub(1) as u64).to_string().len());
                self.ctx.push(Ctx::List(first.map(|n| n), digits));
            }
            Tag::Item => self.start_item(),
            Tag::Table(_) => {
                self.flush();
                self.table = Some(Vec::new());
            }
            Tag::Emphasis => self.styles.push(self.italic()),
            Tag::Strong => self.styles.push(self.fg(Role::Strong).add_modifier(Modifier::BOLD)),
            Tag::Strikethrough => self.styles.push(Style::default().add_modifier(Modifier::CROSSED_OUT)),
            Tag::Link { dest_url, .. } => {
                self.styles.push(self.fg(Role::Link).add_modifier(Modifier::UNDERLINED));
                self.link = Some(dest_url.to_string());
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: &TagEnd) {
        match tag {
            TagEnd::Paragraph | TagEnd::Heading(_) => {
                self.flush();
                self.heading = None;
            }
            TagEnd::BlockQuote(_) => {
                self.flush();
                self.ctx.pop();
                self.blocks.pop();
            }
            TagEnd::List(_) => {
                self.flush();
                self.ctx.pop();
            }
            TagEnd::Item => {
                self.flush();
                self.ctx.pop();
                self.blocks.pop();
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
                self.styles.pop();
            }
            TagEnd::Link => {
                self.styles.pop();
                if let Some(url) = self.link.take()
                    && !self.o.osc8
                    && !url.is_empty()
                {
                    self.runs.push(Run::new(format!(" ({url})"), self.th.tone(j4n_proto::Tone::Dim)));
                }
            }
            _ => {}
        }
    }
}

/// Whether an address may be put into an escape sequence: a scheme that opens in a browser or a file, and no control character.
fn safe(url: &str) -> bool {
    ["http://", "https://", "mailto:", "file://"].iter().any(|s| url.starts_with(s)) && !url.chars().any(char::is_control)
}

/// How many items the list that opens before `rest` has.
fn count_items(rest: &[Event]) -> usize {
    let (mut depth, mut n) = (0usize, 0usize);
    for ev in rest {
        match ev {
            Event::Start(Tag::List(_)) => depth += 1,
            Event::End(TagEnd::List(_)) if depth == 0 => break,
            Event::End(TagEnd::List(_)) => depth -= 1,
            Event::Start(Tag::Item) if depth == 0 => n += 1,
            _ => {}
        }
    }
    n
}

/// Whether a word is a path to a file: a slash, a name with an extension, and perhaps a line (`a/b.rs:12`).
fn is_path(w: &str) -> bool {
    let name = w.split(':').next().unwrap_or(w);
    let tail_ok = w.split(':').skip(1).all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
    let chars_ok = name.bytes().all(|b| b.is_ascii_alphanumeric() || b"_./-+@~".contains(&b));
    let file = name.rsplit('/').next().unwrap_or("");
    let ext = file.rsplit_once('.').map(|(stem, e)| (!stem.is_empty(), e));
    name.contains('/') && !name.contains("//") && chars_ok && tail_ok && ext.is_some_and(|(stem, e)| stem && (1..=6).contains(&e.len()) && e.bytes().all(|b| b.is_ascii_alphanumeric()))
}
