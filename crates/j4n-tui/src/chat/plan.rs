//! The transcript as lines: the rows the core sent, a tree of turn › lane › step, laid out on pipes. The
//! rows say what they are by where they hang (`parent`) and by the mark of their first cell; the folding rules, the merge of steps of
//! one kind, the stats at the right edge and the breath of the doer that works are all decided here, from the rows and the options.
//!
//! A row's cells are `[what, with, stats…]`: `what` is the words (its icon the lane's state or the step's kind), `with` follows it
//! in dim (the model, a summary), and the rest stand at the right edge.

use super::Look;
use crate::display::layout::{self, Laid};
use crate::display::{mark_sp, named, width_of};
use crate::read::{self, Link, Opts};
use crate::theme::glyph::{Slot, Tier};
use crate::theme::{Frame, Theme, You};
use crate::theme::palette::Role as Hue;
use crate::wrap::{fit, wrap};
use j4n_proto::{AskView, By, Cell, Fold, Json, Row, Tone};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use std::collections::{BTreeSet, HashMap};
use unicode_width::UnicodeWidthStr;

/// The marks of the rows that are the core's own business (a session resumed, a grant given, a compaction, a notice): they fold into
/// one row for each turn.
const CONTROL: [&str; 5] = ["control", "session", "grant", "compact", "notice"];

/// How many rendered lines of an answer a preview shows before `+N ▸`: about eight of a chat's end in the orchestrator, six under a lane.
const SAID_CHAT: usize = 8;
const SAID_LANE: usize = 6;

/// One line of the transcript.
pub(crate) struct Out {
    pub line: Line<'static>,
    /// The key of the row this line heads, for the keys.
    pub head: Option<String>,
    /// For a head that folds: whether it is open now.
    pub fold: Option<bool>,
    /// The line is the turn's own.
    pub turn: bool,
    /// The links among its cells, for the terminal to open (OSC 8).
    pub links: Vec<Link>,
    /// The ids of the project's docs named on it: ⏎ with the keys on the line opens their rows.
    pub terms: Vec<String>,
    /// The line is the first row of a heading in kitty's text sizing: the column it starts at and its words.
    pub big: Option<(usize, String)>,
}

impl Out {
    fn blank() -> Out {
        Out { line: Line::default(), head: None, fold: None, turn: false, links: Vec::new(), terms: Vec::new(), big: None }
    }
}

static DEFAULT_READ: Opts = Opts::DEFAULT;

enum Entry {
    Rows(Vec<usize>),
    /// The track of a line that is being taken, at the mark it has come to.
    Track(usize),
    /// The control rows of a turn, folded to one.
    Control(Vec<usize>, String),
}

pub(crate) struct Plan<'a> {
    th: &'a Theme,
    rows: &'a [Row],
    kids: Vec<Vec<usize>>,
    roots: Vec<usize>,
    /// What the doer said about the run (limits, a retry, a model that was switched): `(turn, row)`. These rows leave the tree and stand
    /// after it as bands.
    lifted: Vec<(usize, usize)>,
    flip: &'a BTreeSet<String>,
    look: &'a Look,
    w: usize,
    phase: u8,
    /// The frame of the loading sign (`App.sign`).
    pub sign: u8,
    focus: Option<&'a str>,
    /// The questions that are open: a row whose key is one of them shows its answers.
    pub asks: &'a [AskView],
    /// A doer that works was laid out.
    pub breathes: bool,
    /// A loading sign (`parts::track`) was laid out: the screen steps its frame on its own clock while one is in sight.
    pub signs: bool,
    /// The one lane row that breathes while its lane works: the one the keys are on, else the one `⌃t` aims at, else the newest.
    breather: Option<usize>,
    /// How prose is drawn (`read.*`).
    pub read: &'a Opts,
    /// The turns whose line is still being taken, by the key of their row: each has the track under it.
    pub live: &'a [String],
    /// The rows of the list of the chats, which say whether the chat a line was routed to has started.
    pub pinned: &'a [Row],
    /// A row of the orchestrator's own is being drawn at the left edge, under no turn and on no tree.
    flat: bool,
    /// The text column of a bracket head, where its tree hangs from.
    hung: usize,
}

/// The pieces of a head, before they are fitted to the width.
struct Head {
    pre: Vec<Span<'static>>,
    mark: Option<Span<'static>>,
    model: bool,
    what: String,
    what_style: Style,
    with: String,
    summary: Vec<Span<'static>>,
    stats: Vec<Vec<Span<'static>>>,
    marker: Option<Span<'static>>,
    /// A role's head (the group view): who serves it gives up its last words one by one, and the counts go first.
    role: bool,
}

impl<'a> Plan<'a> {
    pub(crate) fn new(th: &'a Theme, rows: &'a [Row], flip: &'a BTreeSet<String>, look: &'a Look, w: usize, phase: u8, focus: Option<&'a str>) -> Plan<'a> {
        let index: HashMap<&str, usize> = rows.iter().enumerate().map(|(i, r)| (r.key.as_str(), i)).collect();
        let mut kids = vec![Vec::new(); rows.len()];
        let (mut roots, mut lifted) = (Vec::new(), Vec::new());
        let mut turn_of = vec![0; rows.len()];
        for (i, r) in rows.iter().enumerate() {
            match r.parent.as_deref().and_then(|p| index.get(p).copied()).filter(|p| *p < i) {
                Some(p) => {
                    turn_of[i] = turn_of[p];
                    if r.by == By::Doer && r.cells.first().is_some_and(|c| c.icon == "notice") {
                        lifted.push((turn_of[p], i));
                    } else {
                        kids[p].push(i);
                    }
                }
                None => {
                    turn_of[i] = i;
                    roots.push(i);
                }
            }
        }
        // a row ends one cell before the edge, so that an icon at the right never touches it
        Plan { th, rows, kids, roots, lifted, flip, look, w: w.saturating_sub(1).max(8), phase, sign: 0, focus, asks: &[], breathes: false, signs: false, breather: None, read: &DEFAULT_READ, live: &[], pinned: &[], flat: false, hung: 0 }.aimed(None)
    }

    /// Choose the lane that breathes: only one does, however many work. A lane is a row of a turn, so a step that says `working` is no candidate.
    pub(crate) fn aimed(mut self, aim: Option<&str>) -> Plan<'a> {
        let lanes: Vec<usize> = (0..self.rows.len()).filter(|i| self.icon(*i) == "working" && self.rows[*i].parent.as_deref().is_some_and(|p| self.roots.iter().any(|r| self.rows[*r].key == p))).collect();
        let pick = |k: Option<&str>| k.and_then(|k| lanes.iter().copied().find(|i| self.rows[*i].key == k));
        self.breather = pick(self.focus).or_else(|| pick(aim)).or_else(|| lanes.last().copied());
        self
    }

    /// The turns, as the places of their rows.
    pub(crate) fn turns(&self) -> &[usize] {
        &self.roots
    }

    fn icon(&self, i: usize) -> &str {
        self.rows[i].cells.first().map_or("", |c| c.icon.as_str())
    }

    fn is_control(&self, i: usize) -> bool {
        CONTROL.contains(&self.icon(i))
    }

    /// A lane the core says is one (`data.lane`: whose doer it is, and whether that is the thread's own).
    fn is_lane(&self, i: usize) -> bool {
        self.rows[i].data.as_ref().is_some_and(|d| d.get("lane").is_some())
    }

    /// Whether the core marks a lane as `key` (`flow`: a flow's own lane, `code`: a lane of a doer that runs code and no model).
    fn lane_is(&self, i: usize, key: &str) -> bool {
        self.rows[i].data.as_ref().and_then(|d| d.get("lane")).and_then(|l| l.get(key)).and_then(|v| v.as_bool()) == Some(true)
    }

    /// The role a lane is headed by in the group view (`Main`, `review`, `check`): such a lane keeps its row when it is the turn's only one.
    fn role(&self, i: usize) -> Option<&str> {
        self.rows[i].data.as_ref().and_then(|d| d.get("lane")).and_then(|l| l.get("role")).and_then(|v| v.as_str())
    }

    /// The roles under the newest line, when lane `i` is one of them and they are two or more: side lanes, of which one is open. Whether a lane is one
    /// does not hang on its state, so a side lane that ends keeps its fold and the one you opened stays open.
    fn sides(&self, i: usize) -> Option<Vec<usize>> {
        let live = |j: usize| self.role(j).is_some();
        // a line you sent that the core has not shown yet is not the newest line: the lanes move under it once it is
        let newest = self.roots.iter().rev().copied().find(|r| !self.is_report(*r) && !self.rows[*r].key.starts_with(super::live::PENDING))?;
        let turn = self.rows[newest].key.as_str();
        if !live(i) || self.rows[i].parent.as_deref() != Some(turn) {
            return None;
        }
        let sides: Vec<usize> = self.kids[newest].iter().copied().filter(|j| live(*j)).collect();
        (sides.len() > 1).then_some(sides)
    }

    /// A lane that has not ended: a folded side lane shows the step it is on; one that ended shows its last words (`said`).
    fn lives(&self, i: usize) -> bool {
        matches!(self.icon(i), "working" | "queued" | "ask" | "cooling")
    }

    /// What a lane is made of: a flow's own lane, a lane of code (the check), or a model's. Lanes of different kinds do not fold one another.
    fn lane_kind(&self, i: usize) -> &'static str {
        match (self.lane_is(i, "flow"), self.lane_is(i, "code")) {
            (true, _) => "flow",
            (_, true) => "code",
            _ => "model",
        }
    }

    /// A flow's lane and its one lane of code: the flow is no lane of its own, and the turn is a turn with one lane.
    fn wrapped(&self, lanes: &[usize]) -> Option<(usize, usize)> {
        match lanes {
            [a, b] if self.lane_is(*a, "flow") && self.lane_is(*b, "code") => Some((*a, *b)),
            [a, b] if self.lane_is(*b, "flow") && self.lane_is(*a, "code") => Some((*b, *a)),
            _ => None,
        }
    }

    /// What a turn with one lane says on its own line in place of the lane's row: the doer when it is not the thread's own, the
    /// model, the place, the clock, the tokens and the money, and the mark of a lane that failed or was stopped.
    fn lone(&self, lane: usize) -> Vec<Cell> {
        let r = &self.rows[lane];
        let who = r.data.as_ref().and_then(|d| d.get("lane"));
        let named = who.filter(|w| w["own"] == false).and_then(|w| w["doer"].as_str());
        let model = r.cells.get(1).map_or("", |c| c.text.as_str());
        let words = match (named, model) {
            (Some(d), "") => d.to_string(),
            (Some(d), m) => format!("{d} · {m}"),
            (None, m) => m.to_string(),
        };
        let mut out = vec![Cell::new(words, Tone::Dim)];
        out.extend(r.cells.iter().skip(2).cloned());
        // a lane that ended well and did not do its job (a check that could not run) has the warn mark and its words, never the tick
        if let Some(c) = r.cells.first().filter(|c| matches!(c.icon.as_str(), "failed" | "stopped" | "risky")) {
            out.push(Cell::with_icon(&c.icon, "", match c.icon.as_str() { "failed" => Tone::Bad, "risky" => Tone::Warn, _ => Tone::Dim }));
        }
        out
    }

    // ---- what folds ----

    /// Whether a row with rows under it is open before the operator touches it: the core's word, else the rules.
    fn open_at_first(&self, i: usize, lane: bool) -> bool {
        match self.rows[i].fold {
            Some(Fold::Open | Fold::Pinned) => return true,
            Some(Fold::Folded) => return false,
            None => {}
        }
        // the roles that run at once: Main's is open, else the newest, until you open another; the others fold to their head and their last step
        if lane && let Some(sides) = self.sides(i) {
            let first = sides.iter().copied().find(|j| self.lane_is(*j, "main")).unwrap_or(sides[sides.len() - 1]);
            return first == i && !sides.iter().any(|j| *j != i && self.flip.contains(&self.rows[*j].key));
        }
        let icon = self.icon(i);
        if icon == "route" {
            return false;
        }
        if !lane {
            return true;
        }
        let failed = matches!(icon, "failed" | "error");
        if failed && self.look.keep_failed {
            return true;
        }
        // a lane that is settled folds when a newer one of its kind has started after it: the check that follows a work lane does not fold it
        if self.look.fold_older && (failed || matches!(icon, "done" | "stopped")) {
            let turn = self.rows[i].parent.as_deref();
            let kind = self.lane_kind(i);
            let later = (i + 1..self.rows.len()).any(|j| self.rows[j].parent.as_deref() == turn && !self.is_control(j) && self.icon(j) != "queued" && self.lane_kind(j) == kind);
            return !later;
        }
        true
    }

    fn is_open(&self, i: usize, lane: bool) -> bool {
        self.open_at_first(i, lane) != self.flip.contains(&self.rows[i].key)
    }

    fn mergeable(&self, a: usize, b: usize) -> bool {
        let (x, y) = (&self.rows[a], &self.rows[b]);
        self.look.merge && x.merge && y.merge && !self.icon(a).is_empty() && self.icon(a) == self.icon(b) && x.by == y.by && x.detail.is_empty() && y.detail.is_empty() && self.kids[a].is_empty() && self.kids[b].is_empty()
    }

    fn groups(&self, sibs: &[usize]) -> Vec<Vec<usize>> {
        let mut out: Vec<Vec<usize>> = Vec::new();
        for &i in sibs {
            match out.last_mut() {
                Some(g) if self.mergeable(g[0], i) => g.push(i),
                _ => out.push(vec![i]),
            }
        }
        out
    }

    // ---- the pieces of a line ----

    fn edge(&self) -> Style {
        self.th.fg(Hue::Edge)
    }

    /// The width of the gutter of the gutter look: one word a row, right-aligned, and two blanks before the text.
    fn gutter(&self) -> usize {
        if self.w >= 50 { 9 } else { 6 }
    }

    /// A word of the gutter, right-aligned in it, with the blanks that part it from the text; one that is too wide loses its end.
    fn in_gutter(&self, mut word: Vec<Span<'static>>) -> Vec<Span<'static>> {
        let (g, w) = (self.gutter(), width_of(&word));
        if w > g && let Some(l) = word.last_mut() {
            l.content = fit(&l.content, g.saturating_sub(w - l.content.width()).max(1)).into();
        }
        let mut out = vec![Span::raw(" ".repeat(g.saturating_sub(width_of(&word))))];
        out.extend(word);
        out.push(Span::raw("  "));
        out
    }

    /// The slot called `name` as a chat row draws it: an icon is a slot of two cells, and a blank more so that its glyph, which is wider than
    /// its cell, never touches the words.
    fn marked(&self, name: &str) -> Option<Span<'static>> {
        named(self.th, name).map(|m| self.airy(m))
    }

    fn airy(&self, mut m: Span<'static>) -> Span<'static> {
        if self.th.frame != Frame::Gutter && m.content.chars().next().is_some_and(|c| c >= '\u{E000}') && m.content.ends_with(' ') {
            m.content = format!("{} ", m.content).into();
        }
        m
    }

    /// A mark as the word of the gutter.
    fn mark_word(&self, m: &Span<'static>) -> Vec<Span<'static>> {
        self.in_gutter(vec![Span::styled(m.content.trim_end().to_string(), m.style)])
    }

    /// What stands before a row, with the place of its own tee or elbow if it has one: the pieces the base laid (`display/layout`), drawn in the
    /// look of `theme.frame`, the tree hanging from the operator's line. In the gutter look: the blanks of the gutter, for the lines that have no word of
    /// their own.
    fn prefix(&self, at: &Laid) -> (Vec<Span<'static>>, Option<usize>) {
        if self.th.frame == Frame::Gutter {
            return (self.cont(at), None);
        }
        if self.flat && at.level == 0 {
            return (Vec::new(), None);
        }
        let cells = layout::Look::headed(self.th, self.hung).cells(at);
        let own = cells.last().is_some_and(|c| !c.is_empty());
        let spans: Vec<Span<'static>> = cells.into_iter().filter(|c| !c.is_empty()).map(|c| Span::styled(c, self.edge())).collect();
        let at = own.then(|| spans.len() - 1);
        (spans, at)
    }

    /// The columns of the lines that go on under a row.
    fn cont(&self, at: &Laid) -> Vec<Span<'static>> {
        if self.th.frame == Frame::Gutter {
            return vec![Span::raw(" ".repeat(self.gutter() + 1))];
        }
        if self.flat && at.level == 0 {
            return Vec::new();
        }
        layout::Look::headed(self.th, self.hung).under(at).into_iter().filter(|c| !c.is_empty()).map(|c| Span::styled(c, self.edge())).collect()
    }

    fn stats(&self, cells: &[Cell]) -> Vec<Vec<Span<'static>>> {
        let shown: Vec<&Cell> = cells.iter().skip(2).filter(|c| !c.text.is_empty() || !c.icon.is_empty()).collect();
        let n = shown.len();
        shown
            .into_iter()
            .enumerate()
            .map(|(at, c)| {
                let mut spans = crate::display::cell(self.th, c, Tone::Dim);
                // a mark alone has no word after it to be a blank away from, unless it is the last: that one keeps its blank, so an icon
                // never stands against the edge
                if c.text.is_empty() && at + 1 < n && let Some(m) = spans.first_mut() {
                    m.content = m.content.trim_end().to_string().into();
                }
                spans
            })
            .collect()
    }

    /// The marks of what is under a lane, with how many of each: what a folded lane shows in place of its steps.
    fn summary(&self, i: usize) -> Vec<Span<'static>> {
        let mut seen: Vec<(&str, usize)> = Vec::new();
        for &k in &self.kids[i] {
            let icon = self.icon(k);
            if icon.is_empty() {
                continue;
            }
            match seen.iter_mut().find(|(name, _)| *name == icon) {
                Some(e) => e.1 += 1,
                None => seen.push((icon, 1)),
            }
        }
        let mut out = Vec::new();
        for (n, (icon, count)) in seen.into_iter().enumerate() {
            if n > 0 {
                out.push(Span::raw("  "));
            }
            out.extend(self.marked(icon));
            out.push(Span::styled(count.to_string(), self.th.tone(Tone::Dim)));
        }
        out
    }

    fn marker(&self, open: bool) -> Span<'static> {
        Span::styled(self.th.glyph(if open { Slot::FoldOpen } else { Slot::FoldClosed }).to_string(), self.th.fg(Hue::Faint))
    }

    fn tone_of(&self, r: &Row, c: Option<&Cell>) -> Tone {
        match c {
            Some(c) if c.tone != Tone::Plain => c.tone,
            _ => r.tone,
        }
    }

    /// A head fitted to the width: the words and the stats a gap apart, giving up what matters least when they do not fit — the
    /// `with`, the summary, then the stats from the left, then the end of the words.
    fn fit_head(&self, h: Head) -> (Vec<Span<'static>>, usize) {
        let th = self.th;
        let pre_w = width_of(&h.pre) + h.mark.as_ref().map_or(0, |m| m.content.width());
        let tilde = usize::from(h.model);
        let marker_w = h.marker.as_ref().map_or(0, |m| 1 + m.content.width());
        let right_w = |from: usize| {
            let parts: Vec<usize> = h.stats[from..].iter().map(|s| width_of(s)).collect();
            parts.iter().sum::<usize>() + 2 * parts.len().saturating_sub(1) + marker_w
        };
        let mid_w = |with_s: &str, with: bool, summary: bool| {
            let mut n = 0;
            if with && !with_s.is_empty() {
                n += 1 + with_s.width();
            }
            if summary && !h.summary.is_empty() {
                n += 2 + width_of(&h.summary);
            }
            n
        };
        let fits = |with_s: &str, what_w: usize, with: bool, summary: bool, from: usize| {
            let right = right_w(from);
            pre_w + tilde + what_w + mid_w(with_s, with, summary) + if right > 0 { right + 2 } else { 0 } <= self.w
        };
        let what_w = h.what.width();
        let mut with_s = h.with.clone();
        let (mut with, mut summary, mut from) = (true, true, 0);
        if h.role {
            summary = fits(&with_s, what_w, true, true, 0);
            // a version goes before a name (`haiku + Decider`), then the words from the end
            while !fits(&with_s, what_w, true, false, 0) && with_s.matches(' ').count() > 1 {
                let words: Vec<&str> = with_s.split(' ').collect();
                let cut = (2..words.len()).rev().find(|n| words[*n].contains(|c: char| c.is_ascii_digit())).unwrap_or(words.len() - 1);
                with_s = words.iter().enumerate().filter(|(n, _)| *n != cut).map(|(_, w)| *w).collect::<Vec<_>>().join(" ").trim_end_matches(" +").to_string();
            }
        }
        let fits = |what_w: usize, with: bool, summary: bool, from: usize| fits(&with_s, what_w, with, summary, from);
        let mid_w = |with: bool, summary: bool| mid_w(&with_s, with, summary);
        if !fits(what_w, with, summary, from) {
            with = false;
        }
        if !fits(what_w, with, summary, from) {
            summary = false;
        }
        while !fits(what_w, with, summary, from) && from < h.stats.len() {
            from += 1;
        }
        let room = self.w.saturating_sub(pre_w + tilde + mid_w(with, summary) + if right_w(from) > 0 { right_w(from) + 2 } else { 0 });
        let what = fit(&h.what, room.max(1));
        let mut spans = h.pre;
        let indent = width_of(&spans) + h.mark.as_ref().map_or(0, |m| m.content.width());
        spans.extend(h.mark);
        if h.model {
            spans.push(Span::styled("~", th.fg(Hue::Made)));
        }
        let said = !what.is_empty();
        spans.push(Span::styled(what, h.what_style));
        if with && !with_s.is_empty() {
            spans.push(Span::styled(format!(" {with_s}"), th.tone(Tone::Dim)));
        }
        if summary && !h.summary.is_empty() {
            // after the words it is two blanks away; with none, it stands where they would
            if said || with && !with_s.is_empty() {
                spans.push(Span::raw("  "));
            }
            spans.extend(h.summary);
        }
        let mut right: Vec<Span<'static>> = Vec::new();
        for (n, s) in h.stats.into_iter().skip(from).enumerate() {
            if n > 0 {
                right.push(Span::raw("  "));
            }
            right.extend(s);
        }
        if let Some(m) = h.marker {
            right.push(Span::raw(" "));
            right.push(m);
        }
        let gap = self.w.saturating_sub(width_of(&spans) + width_of(&right));
        spans.push(Span::raw(" ".repeat(gap)));
        spans.extend(right);
        (spans, indent)
    }

    fn head_line(&self, spans: Vec<Span<'static>>, key: &str, branch: Option<usize>, fold: Option<bool>) -> Out {
        let on = self.focus == Some(key);
        let mut spans = spans;
        if on && let Some(b) = branch {
            spans[b].style = self.th.fg(Hue::Focus);
        }
        let line = Line::from(spans);
        Out { line: if on { line.style(self.th.back(Hue::Sel)) } else { line }, head: Some(key.to_string()), fold, turn: false, links: Vec::new(), terms: Vec::new(), big: None }
    }

    // ---- the turns ----

    /// The lines of the turn whose row is at `root`.
    pub(crate) fn turn(&mut self, root: usize) -> Vec<Out> {
        if self.is_report(root) {
            return self.report(root);
        }
        let (th, rows) = (self.th, self.rows);
        let r = &rows[root];
        let (control, rest): (Vec<usize>, Vec<usize>) = self.kids[root].clone().into_iter().partition(|k| self.is_control(*k));
        // a turn with one lane has no row for it: its steps hang from the operator's line and its facts stand at that line's right edge
        let lanes: Vec<usize> = rest.iter().copied().filter(|k| self.is_lane(*k)).collect();
        let flow = self.wrapped(&lanes);
        let lane = match flow {
            Some((_, code)) => Some(code),
            None => Some(lanes).filter(|l| l.len() == 1 && self.role(l[0]).is_none()).map(|l| l[0]),
        };
        let mut cells = r.cells.clone();
        let mut stats = self.stats(&cells);
        if let Some(l) = lane {
            let mut extra = self.lone(l);
            // the facts give way to the line when the width is short: the oldest first, the time of day last
            while !extra.is_empty() {
                cells.splice(2..2, extra.clone());
                stats = self.stats(&cells);
                let wide = stats.iter().map(|s| width_of(s) + 2).sum::<usize>();
                if wide * 2 <= self.w {
                    break;
                }
                cells = r.cells.clone();
                extra.remove(0);
            }
            if extra.is_empty() {
                stats = self.stats(&r.cells);
            }
        }
        let right: Vec<Span<'static>> = stats.iter().enumerate().flat_map(|(n, s)| if n > 0 { [vec![Span::raw("  ")], s.clone()].concat() } else { s.clone() }).collect();
        let text = r.cells.first().map_or("", |c| c.text.as_str());
        // the operator's line: their words on a raised band with the time at its right and no icon; `theme.you = mark` keeps the icon
        // the first row of a branch chat says where it came from, in words that are not yours: no band, no mark of yours
        let from = r.data.as_ref().is_some_and(|d| d.get("from").is_some());
        let gut = th.frame == Frame::Gutter;
        // the bracket is the head of the tree the base lays under it; the gutter look has its own column of words, so there it stays the band
        let caps = th.you == You::Bracket && !from && !gut;
        let band = (th.you == You::Band || th.you == You::Bracket && gut) && !from;
        // `theme.you = mark` on the pipes or the hang: no mark at all. The words start at the left edge, in the operator's colour,
        // closer to the border than the rows under them, so the turn begins where the tree below it does not
        let bare = !band && !caps && !from && !gut;
        let mark = if bare { Span::raw("") } else { self.airy(mark_sp(th, if from { Slot::Route } else { Slot::You })) };
        // the band's words start on the composer's text column and its time ends where the composer's text ends: two cells in from each
        // border
        let lead = if gut { self.gutter() + 2 } else if band { 2 } else { mark.content.width() };
        let w = if band && !gut { self.w.saturating_sub(1) } else { self.w };
        let room = w.saturating_sub(lead + width_of(&right) + 2).max(8);
        let on = self.focus == Some(r.key.as_str());
        // the tree hangs from the bracket's words (a cap and a blank in), so no pipe starts under the cap
        self.hung = if caps { self.th.glyph(Slot::YouCapL).width() + 1 } else { 0 };
        let mut out = if caps { self.bracket(&r.key, text, &right) } else { Vec::new() };
        for (n, part) in (if caps { Vec::new() } else { wrap(text, room) }).into_iter().enumerate() {
            let mut spans = match (n, gut, band) {
                (0, true, _) => self.mark_word(&mark),
                (0, false, false) => vec![mark.clone()],
                _ => vec![Span::raw(" ".repeat(lead))],
            };
            spans.push(if from { Span::styled(part, th.tone(Tone::Dim)) } else if bare { Span::styled(part, th.fg(Hue::You)) } else { Span::raw(part) });
            // every row of the band is as wide as the first, to the cell
            let gap = w.saturating_sub(width_of(&spans) + if n == 0 { width_of(&right) } else { 0 });
            if n == 0 || band {
                spans.push(Span::raw(" ".repeat(gap)));
            }
            if n == 0 {
                spans.extend(right.clone());
            }
            // the band runs on over the cells at the right
            if band {
                spans.push(Span::raw(if gut { " " } else { "  " }));
            }
            let mut o = self.head_line(spans, &r.key, None, None);
            if band && !on {
                o.line = o.line.style(th.back(Hue::Raise));
            }
            o.turn = true;
            if n > 0 {
                o.head = None;
            }
            out.push(o);
        }
        let key = r.key.clone();
        let mut entries: Vec<Entry> = Vec::new();
        for g in self.groups(&rest) {
            match lane {
                Some(l) if g[0] == l || flow.is_some_and(|(w, _)| w == g[0]) => entries.extend(self.groups(&self.kids[g[0]].clone()).into_iter().map(Entry::Rows)),
                _ => entries.push(Entry::Rows(g)),
            }
        }
        if !control.is_empty() {
            entries.push(Entry::Control(control, format!("ctl:{key}")));
        }
        // a line that is still being taken says how far it has come
        if self.live.contains(&key)
            && let Some(stage) = super::live::stage(rows, self.pinned, &key)
        {
            entries.push(Entry::Track(stage));
        }
        let n = entries.len();
        for (at, e) in entries.iter().enumerate() {
            match e {
                Entry::Rows(g) => {
                    // the steps of a lone lane stand where its row stood; on the hang a rail still marks them as a lane's
                    let own = self.rows[g[0]].parent.as_deref() == Some(&key);
                    let lane_end = (!own && th.frame == Frame::Hang).then(|| layout::place(None, 0, 1));
                    self.step(g, &layout::place(lane_end.as_ref(), at, n), own, &mut out)
                }
                Entry::Control(rows, key) => self.control(rows, key, &layout::place(None, at, n), &mut out),
                Entry::Track(stage) => self.track_row(root, *stage, &layout::place(None, at, n), &mut out),
            }
        }
        let said: Vec<usize> = self.lifted.iter().filter(|(t, _)| *t == root).map(|(_, i)| *i).collect();
        self.notices(&said, &mut out);
        out
    }

    /// The operator's line as a bracket: the words plain between two caps in the accent, one blank inside each cap, the time at the right of the
    /// first row, every row as wide as the screen. The tree under it hangs from the words, two cells in, so the cap stands alone.
    fn bracket(&self, key: &str, text: &str, right: &[Span<'static>]) -> Vec<Out> {
        let th = self.th;
        let cap = |slot: Slot| Span::styled(th.glyph(slot).to_string(), th.fg(Hue::Accent));
        let (open, shut) = (cap(Slot::YouCapL), cap(Slot::YouCapR));
        let total = self.w + 1;
        let room = total.saturating_sub(open.content.width() + shut.content.width() + 2 + width_of(right) + 2).max(8);
        wrap(text, room)
            .into_iter()
            .enumerate()
            .map(|(n, part)| {
                let mut spans = vec![open.clone(), Span::raw(" "), Span::raw(part)];
                let tail = if n == 0 { right } else { &[] };
                let gap = total.saturating_sub(width_of(&spans) + width_of(tail) + 1 + shut.content.width());
                spans.push(Span::raw(" ".repeat(gap)));
                spans.extend(tail.iter().cloned());
                spans.push(Span::raw(" "));
                spans.push(shut.clone());
                let mut o = self.head_line(spans, key, None, None);
                o.turn = true;
                if n > 0 {
                    o.head = None;
                }
                o
            })
            .collect()
    }

    /// A row at the top that the core made about a chat (its end, its merge, its plan: a mark, and no line of the operator's): the orchestrator's own.
    fn is_report(&self, root: usize) -> bool {
        self.rows[root].by == By::Core && !self.icon(root).is_empty()
    }

    /// The orchestrator's own row: at the left edge with its mark, no tree over it and no band, its preview under it, and a stop for the keys like a turn.
    fn report(&mut self, root: usize) -> Vec<Out> {
        let mut out = Vec::new();
        self.flat = true;
        self.step(&[root], &layout::place(None, 0, 1), true, &mut out);
        self.flat = false;
        if let Some(o) = out.first_mut() {
            o.turn = true;
        }
        out
    }

    /// What the doer said about the run, after the turn's tree: a blank row, then a band for each, with its own icon and a faint
    /// wash of its own, no pipe. A notice the doer says twice in a row is said once.
    fn notices(&self, said: &[usize], out: &mut Vec<Out>) {
        let th = self.th;
        let mut last: Option<&str> = None;
        let mut first = true;
        for &i in said {
            let r = &self.rows[i];
            let text = r.cells.first().map_or("", |c| c.text.as_str());
            if text.trim().is_empty() || last == Some(text) {
                continue;
            }
            last = Some(text);
            if std::mem::take(&mut first) {
                out.push(Out::blank());
            }
            let tone = match self.tone_of(r, r.cells.first()) {
                Tone::Plain => Tone::Dim,
                t => t,
            };
            let mark = self.marked("notice").unwrap_or_else(|| Span::raw(""));
            let lead = 1 + mark.content.width();
            let wash = th.tint(Hue::Dim);
            for (n, part) in read::spans(th, self.read, th.tone(tone), text, self.w.saturating_sub(lead + 1).max(8)).into_iter().enumerate() {
                let mut spans = vec![Span::raw(" ")];
                spans.push(if n == 0 { Span::styled(mark.content.to_string(), th.tone(tone)) } else { Span::raw(" ".repeat(mark.content.width())) });
                spans.extend(part);
                let used = width_of(&spans);
                spans.push(Span::raw(" ".repeat((self.w + 1).saturating_sub(used))));
                let mut o = self.head_line(spans, &r.key, None, None);
                if self.focus != Some(r.key.as_str()) {
                    o.line = o.line.style(wash);
                }
                if n > 0 {
                    o.head = None;
                }
                out.push(o);
            }
        }
    }

    /// The blank row between two turns.
    pub(crate) fn gap() -> Out {
        Out::blank()
    }

    /// The row under a turn whose line is being taken: the track, what it is at in words, and one line of what is happening: the line
    /// that was sent, the chat it opens, the doer and model that start. It gives up its preview, then cells of its line, as the width shortens.
    fn track_row(&mut self, root: usize, stage: usize, at: &Laid, out: &mut Vec<Out>) {
        let th = self.th;
        self.signs = true;
        let (pre, _) = self.prefix(at);
        let line = super::live::clip(self.rows[root].cells.first().map_or("", |c| c.text.as_str()), 36);
        let route = self.kids[root].iter().find(|k| super::live::is_route(&self.rows[**k])).and_then(|k| self.rows[*k].cells.first()).map(|c| c.text.clone()).unwrap_or_default();
        let preview = match stage {
            2 if route.contains("new chat") => format!("a chat for {line}"),
            2 => route.trim_start_matches('→').trim().split(" · ").next().map_or(line.clone(), |c| format!("{line} into {c}")),
            3 => self.kids[root].iter().copied().find(|k| self.is_lane(*k)).and_then(|l| self.lone(l).first().map(|c| c.text.clone())).filter(|w| !w.is_empty()).unwrap_or(line),
            _ => line,
        };
        let word = Span::styled(super::live::WORDS[stage], th.fg(Hue::Accent));
        let rest = |p: &str| if p.is_empty() { Vec::new() } else { vec![Span::styled(format!(" · {p}"), th.tone(Tone::Dim))] };
        let draw = |cells: usize, preview: &str| {
            let mut spans = pre.clone();
            if cells > 0 {
                spans.extend(crate::parts::track::track(th, self.sign, cells));
                spans.push(Span::raw(" "));
            }
            spans.push(word.clone());
            spans.extend(rest(preview));
            spans
        };
        // as the width shortens the preview goes first, then the line (twelve cells, eight, six), then the line altogether
        let mut spans = draw(crate::parts::track::CELLS, &preview);
        for cells in [crate::parts::track::CELLS, crate::parts::track::NARROW, 0] {
            if width_of(&spans) <= self.w {
                break;
            }
            spans = draw(cells, "");
        }
        out.push(Out { line: Line::from(spans), ..Out::blank() });
    }

    fn control(&mut self, rows: &[usize], key: &str, at: &Laid, out: &mut Vec<Out>) {
        let th = self.th;
        let open = self.flip.contains(key);
        let (mut pre, branch) = self.prefix(at);
        let mut mark = Some(self.airy(mark_sp(th, Slot::Control)));
        if th.frame == Frame::Gutter && let Some(m) = mark.take() {
            pre = self.mark_word(&m);
        }
        let mut stats: Vec<Vec<Span<'static>>> = Vec::new();
        if open {
            stats.push(vec![mark_sp(th, Slot::Yes), Span::styled("in context", th.tone(Tone::Dim))]);
        }
        let h = Head { pre, mark, model: false, what: rows.len().to_string(), what_style: th.tone(Tone::Dim), with: String::new(), summary: Vec::new(), stats, marker: Some(self.marker(open)), role: false };
        let (spans, _) = self.fit_head(h);
        out.push(self.head_line(spans, key, branch, Some(open)));
        if open {
            let n = rows.len();
            for (k, i) in rows.iter().enumerate() {
                self.step(std::slice::from_ref(i), &layout::place(Some(at), k, n), false, out);
            }
        }
    }

    /// A row and what hangs from it. `lane`: the row is one of the turn's own rows, as the rows at the top are, and not a step.
    fn step(&mut self, g: &[usize], at: &Laid, lane: bool, out: &mut Vec<Out>) {
        let (th, rows) = (self.th, self.rows);
        let i = g[0];
        let r = &rows[i];
        if let Some(view) = r.data.as_ref().and_then(|d| d.get("view")) {
            return self.held(i, view, at, out);
        }
        if self.icon(i).is_empty() && self.kids[i].is_empty() {
            return self.prose(i, at, out);
        }
        let icon = self.icon(i).to_string();
        let (mut pre, branch) = self.prefix(at);
        let first = r.cells.first();
        let working = lane && icon == "working";
        // the state of a doer's lane, as the gutter look puts it at the right edge: in the words tier a lane that works is the name
        // breathing, and the word for it is said out
        let doer = lane && matches!(icon.as_str(), "queued" | "working" | "ask" | "cooling" | "done" | "risky" | "failed" | "stopped" | "error");
        let state = if working && th.tier == Tier::Words { Some(Span::styled("working", th.fg(Hue::Accent))) } else { named(th, &icon).map(|m| Span::styled(m.content.trim_end().to_string(), m.style)) };
        let mut mark = self.marked(&icon);
        if working && th.tier == Tier::Words {
            mark = None;
        }
        // a row of a core note's card has no mark and so no slot for one: its words stand where a row with none under it has them
        if mark.is_none() && lane && !(r.by == By::Core && icon.is_empty()) {
            mark = Some(Span::raw(" "));
        }
        let breathing = working && self.breather == Some(i);
        self.breathes |= breathing;
        let tone = self.tone_of(r, first);
        let what_style = if breathing {
            th.breath(self.phase)
        } else if lane {
            th.tone(tone).add_modifier(Modifier::BOLD)
        } else {
            th.tone(tone)
        };
        let kids = !self.kids[i].is_empty();
        let open = kids && self.is_open(i, lane);
        // the answer of a chat's end, or of a lane that settled and folded, as a preview under its head: read as markdown, cut at a few lines
        let lead = if th.frame == Frame::Gutter { self.gutter() + 2 } else { width_of(&pre) + mark.as_ref().map_or(0, |m| m.content.width()) };
        let (said, whole) = self.said(i, lane && kids && !open, lead);
        let d = if whole { said.as_ref().map_or(0, |(l, _)| l.len()) } else { r.detail.len() };
        let tail = match &said {
            Some((_, cap)) if whole => *cap,
            _ if matches!(icon.as_str(), "edit" | "new") => 0,
            _ => self.look.tail,
        };
        let foldable_detail = d > tail;
        let opened = self.flip.contains(&r.key);
        let mut fold = None;
        let mut marker = None;
        if kids {
            fold = Some(open);
            // a lane that was shut by a rule shows it; one the operator opened shows that
            if !open || !self.open_at_first(i, lane) {
                marker = Some(self.marker(open));
            }
        } else if foldable_detail {
            fold = Some(opened);
            if opened || tail == 0 {
                marker = Some(self.marker(opened));
            }
        }
        let (mut what, mut with, mut stats) = if g.len() > 1 {
            let names: Vec<&str> = g.iter().map(|k| rows[*k].cells.first().map_or("", |c| c.text.as_str())).collect();
            (names.join(" · "), String::new(), vec![vec![Span::styled(g.len().to_string(), th.tone(Tone::Dim))]])
        } else {
            // a question that is still open shows the marks of its answers in place of the stats the core sent for it
            let stats = match self.asks.iter().find(|a| a.ask == r.key) {
                Some(a) => crate::ask::inline(self.th, a, first.map_or("", |c| c.text.split(' ').next().unwrap_or(""))),
                None => self.stats(&r.cells),
            };
            // a row of a core note's card that has rows under it holds the operator's own words: read, on one line, not shown with their marks
            let said = first.map_or(String::new(), |c| c.text.clone());
            (if r.by == By::Core && icon.is_empty() { read::flat(th, &said.split_whitespace().collect::<Vec<_>>().join(" ")) } else { said }, r.cells.get(1).map_or(String::new(), |c| c.text.clone()), stats)
        };
        // the orchestrator's plan row: the thin line and the lanes after the words, drawn by the one function that draws the strip's line too
        let plan = r.data.as_ref().and_then(|d| d.get("plan"));
        let summary = if let Some(p) = plan {
            let n = |k: &str| p.get(k).and_then(|v| v.as_u64()).unwrap_or(0);
            let mut spans: Vec<Span<'static>> = super::compose::plan_line(th, n("done"), n("total"), true).into_iter().skip(2).collect();
            if let Some(lanes) = p.get("lanes").and_then(|l| l.as_str()).filter(|l| !l.is_empty()) {
                spans.push(Span::styled(format!("  {lanes}"), th.tone(Tone::Dim)));
            }
            spans
        } else if kids && !open {
            self.summary(i)
        } else {
            Vec::new()
        };
        // a row that is the door of a chat (a chat's end) says so at its edge: `⏎` goes in
        if marker.is_none() && r.data.as_ref().is_some_and(|d| d.get("door").is_some()) {
            marker = Some(self.marker(false));
        }
        let mut what_style = what_style;
        if th.frame == Frame::Gutter {
            let mut word = Vec::new();
            if doer {
                // the letter and the name in the gutter, the model where the words go, the state before the stats
                let (letter, name) = what.split_once(' ').unwrap_or(("", &what));
                if !letter.is_empty() {
                    word.push(Span::styled(format!("{letter} "), th.fg(Hue::Faint)));
                }
                word.push(Span::styled(name.to_string(), what_style));
                (what, with, what_style) = (with.trim_start_matches("· ").to_string(), String::new(), th.tone(Tone::Dim));
                if !r.cells.iter().skip(2).any(|c| c.icon == icon || c.text == icon) && let Some(s) = state {
                    stats.insert(0, vec![s]);
                }
            } else if let Some(m) = mark.take() {
                word.push(Span::styled(m.content.trim_end().to_string(), m.style));
            }
            pre = self.in_gutter(word);
            mark = None;
        }
        let h = Head { pre, mark, model: r.by == By::Model, what, what_style, with, summary, stats, marker, role: self.role(i).is_some() || icon == "decide" };
        let (spans, indent) = self.fit_head(h);
        out.push(self.head_line(spans, &r.key, branch, fold));
        if let Some((lines, cap)) = said {
            let all = whole && (opened || !foldable_detail);
            self.preview(lines, cap, all, &self.cont(at), lead, out);
        } else {
            // the output under a step, on a raised box
            let (shown, hidden): (Vec<&Cell>, usize) = if opened || !foldable_detail { (r.detail.iter().collect(), 0) } else { (r.detail.iter().skip(d - tail).collect(), d - tail) };
            self.output(&shown, hidden, &self.cont(at), indent, out);
        }
        if open {
            let kids = self.kids[i].clone();
            let groups = self.groups(&kids);
            let n = groups.len();
            for (k, g) in groups.iter().enumerate() {
                self.step(g, &layout::place(Some(at), k, n), false, out);
            }
        } else if lane && kids && self.lives(i) && self.sides(i).is_some() && let Some(&last) = self.kids[i].last() {
            // a role folded beside the open one still shows what it does now
            self.step(&[last], &layout::place(Some(at), 0, 1), false, out);
        }
    }

    /// The words a preview shows under row `i`, read as markdown and wrapped, with its cap in lines: the row's own detail when the core says it is
    /// markdown (`data.read`: a chat's end; the bool is true), else, for a lane that folded, its last message (`under`). `lead` is the width of what
    /// stands before the head's words.
    fn said(&self, i: usize, under: bool, lead: usize) -> (Option<(Vec<read::RLine>, usize)>, bool) {
        let r = &self.rows[i];
        let (text, cap, whole) = if r.data.as_ref().is_some_and(|d| d.get("read").is_some()) {
            (r.detail.iter().map(|c| c.text.as_str()).collect::<Vec<_>>().join("\n"), SAID_CHAT, true)
        } else if under && matches!(self.icon(i), "done" | "failed" | "stopped") {
            let last = self.kids[i].iter().rev().copied().find(|k| self.icon(*k).is_empty() && self.kids[*k].is_empty() && self.rows[*k].by != By::Core && self.rows[*k].data.is_none());
            (last.map_or(String::new(), |k| self.rows[k].cells.first().map_or(String::new(), |c| c.text.clone())), SAID_LANE, false)
        } else {
            return (None, false);
        };
        if text.trim().is_empty() {
            return (None, false);
        }
        let pw = self.look.width.min(self.w.saturating_sub(lead + 1)).max(8);
        (Some((read::read(self.th, self.read, self.th.tone(Tone::Plain), &text, pw), cap)), whole)
    }

    /// An answer as a message under its head, the one drawing of it: its lines read as markdown, at most `cap` of them with `+N ▸` at the end of the
    /// last for what is left out, or all of them when `all`.
    fn preview(&self, lines: Vec<read::RLine>, cap: usize, all: bool, cont: &[Span<'static>], lead: usize, out: &mut Vec<Out>) {
        let th = self.th;
        let shown = if all { lines.len() } else { cap.min(lines.len()) };
        let hidden = lines.len() - shown;
        let front: Vec<Span<'static>> = cont.iter().cloned().chain(std::iter::once(Span::raw(" ".repeat(lead.saturating_sub(width_of(cont)))))).collect();
        let more = format!("+{hidden} {}", th.glyph(Slot::FoldClosed));
        let mut stray = hidden > 0;
        for (n, l) in lines.into_iter().take(shown).enumerate() {
            let mut l = l.after(&front);
            let used = width_of(&l.spans);
            if n + 1 == shown && hidden > 0 && used + more.width() + 2 <= self.w {
                l.spans.push(Span::raw(" ".repeat(self.w - used - more.width() - 1)));
                l.spans.push(Span::styled(format!("{more} "), th.fg(Hue::Faint)));
                stray = false;
            }
            out.push(Out { line: Line::from(l.spans), head: None, fold: None, turn: false, links: l.links, terms: l.terms, big: l.big });
        }
        // the last line is too long for the mark: it stands on a line of its own
        if stray {
            let spans = front.into_iter().chain(std::iter::once(Span::styled(more, th.fg(Hue::Faint)))).collect::<Vec<_>>();
            out.push(Out { line: Line::from(spans), ..Out::blank() });
        }
    }

    /// A view the core holds in the chat (`data.view`): its title on a head, then what its field draws in the width that is left, on the same rail.
    fn held(&mut self, i: usize, view: &Json, at: &Laid, out: &mut Vec<Out>) {
        let (pre, branch) = self.prefix(at);
        let cont = self.cont(at);
        let title = self.rows[i].cells.first().map_or(String::new(), |c| c.text.clone());
        let h = Head { pre, mark: None, model: false, what: title, what_style: self.th.tone(Tone::Dim), with: String::new(), summary: Vec::new(), stats: Vec::new(), marker: None, role: false };
        let (spans, _) = self.fit_head(h);
        out.push(self.head_line(spans, &self.rows[i].key.clone(), branch, None));
        for l in crate::display::held(self.th, view, self.w.saturating_sub(width_of(&cont)).max(8)) {
            let mut spans = cont.clone();
            spans.extend(l.spans);
            out.push(Out { line: Line::from(spans), head: None, fold: None, turn: false, links: Vec::new(), terms: Vec::new(), big: None });
        }
    }

    /// Lines of output (or of a diff) on a raised box under the words of their step, with the number of lines that are left out.
    fn output(&self, lines: &[&Cell], hidden: usize, cont: &[Span<'static>], indent: usize, out: &mut Vec<Out>) {
        let th = self.th;
        let pad = indent.saturating_sub(width_of(cont) + 1);
        let boxed = th.tone(Tone::Dim).patch(th.back(Hue::Raise));
        let n = lines.len();
        for (at, c) in lines.iter().enumerate() {
            let tail = if at + 1 == n && hidden > 0 { format!("+{hidden} {}", th.glyph(Slot::FoldClosed)) } else { String::new() };
            let room = self.w.saturating_sub(width_of(cont) + pad + 2 + if tail.is_empty() { 0 } else { tail.width() + 2 });
            let text = fit(&c.text, room.max(1));
            let style = match (c.tone, c.text.chars().next()) {
                (Tone::Plain, Some('+')) => th.fg(Hue::Add).patch(th.back(Hue::Raise)),
                (Tone::Plain, Some('-' | '−')) => th.fg(Hue::Del).patch(th.back(Hue::Raise)),
                (Tone::Plain, _) => boxed,
                (t, _) => th.tone(t).patch(th.back(Hue::Raise)),
            };
            let mut spans = cont.to_vec();
            spans.push(Span::raw(" ".repeat(pad)));
            let used = width_of(&spans) + 1 + text.width();
            spans.push(Span::styled(format!(" {text}"), style));
            let fill = self.w.saturating_sub(used + tail.width() + usize::from(!tail.is_empty()));
            spans.push(Span::styled(" ".repeat(fill), boxed));
            if !tail.is_empty() {
                spans.push(Span::styled(format!("{tail} "), boxed));
            }
            out.push(Out { line: Line::from(spans), head: None, fold: None, turn: false, links: Vec::new(), terms: Vec::new(), big: None });
        }
    }

    fn prose(&mut self, i: usize, at: &Laid, out: &mut Vec<Out>) {
        let (th, rows) = (self.th, self.rows);
        let r = &rows[i];
        let (pre, branch) = self.prefix(at);
        let cont = self.cont(at);
        let first_pre = width_of(&pre);
        let model = r.by == By::Model;
        let pw = self.look.width.min(self.w.saturating_sub(first_pre + usize::from(model) + 1)).max(8);
        let base = th.tone(self.tone_of(r, r.cells.first()));
        let text = r.cells.first().map_or("", |c| c.text.as_str());
        // under a lane the rail is the mark's slot; at the top, text stands where a mark's text would
        let pad = " ".repeat(match th.frame {
            Frame::Gutter => 1,
            Frame::Hang if at.level == 0 => 2,
            _ => 0,
        });
        for (n, l) in read::read(th, self.read, base, text, pw).into_iter().enumerate() {
            let mut front = if n == 0 { pre.clone() } else { cont.clone() };
            front.push(Span::raw(pad.clone()));
            if model && n == 0 {
                front.push(Span::styled("~", th.fg(Hue::Made)));
            } else if model {
                front.push(Span::raw(" "));
            }
            let links = l.after(&front);
            let mut o = if n == 0 { self.head_line(links.spans, &r.key, branch, None) } else { Out { line: Line::from(links.spans), head: None, fold: None, turn: false, links: Vec::new(), terms: Vec::new(), big: None } };
            (o.links, o.terms, o.big) = (links.links, links.terms, links.big);
            out.push(o);
        }
    }
}
