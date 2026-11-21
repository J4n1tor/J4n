//! Words with their styles, cut to a width: the paragraph of the reader.

use ratatui::style::Style;
use ratatui::text::Span;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// A piece of a line: its style, and the address it opens when it is a link.
#[derive(Debug, Clone)]
pub struct Run {
    pub text: String,
    pub style: Style,
    pub link: Option<String>,
    /// Never cut at a blank (inline code, a path).
    pub atom: bool,
    /// The id of the project's docs this run is: the keys open its row.
    pub term: Option<String>,
}

impl Run {
    pub fn new(text: impl Into<String>, style: Style) -> Run {
        Run { text: text.into(), style, link: None, atom: false, term: None }
    }
}

/// Where a link stands on its line: from `col`, `w` cells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub col: usize,
    pub w: usize,
    pub url: String,
}

/// A line of the reader: the spans, and the links among them.
#[derive(Debug, Clone, Default)]
pub struct RLine {
    pub spans: Vec<Span<'static>>,
    pub links: Vec<Link>,
    /// The ids of the project's docs named on the line, each once.
    pub terms: Vec<String>,
    /// The line is the first row of a heading kitty draws twice as large: the column its words start at and the words. The
    /// row under it is blank, and the terminal draws over both once the frame is out.
    pub big: Option<(usize, String)>,
}

impl RLine {
    pub fn width(&self) -> usize {
        self.spans.iter().map(|s| s.content.width()).sum()
    }

    /// The line with `spans` in front of it.
    pub fn after(mut self, before: &[Span<'static>]) -> RLine {
        let w: usize = before.iter().map(|s| s.content.width()).sum();
        for l in &mut self.links {
            l.col += w;
        }
        if let Some(b) = &mut self.big {
            b.0 += w;
        }
        self.spans.splice(0..0, before.iter().cloned());
        self
    }

    pub fn of(runs: &[Run]) -> RLine {
        let mut out = RLine::default();
        let mut col = 0;
        for r in runs {
            let w = r.text.width();
            if let Some(url) = &r.link {
                match out.links.last_mut() {
                    Some(l) if l.url == *url && l.col + l.w == col => l.w += w,
                    _ => out.links.push(Link { col, w, url: url.clone() }),
                }
            }
            if let Some(t) = r.term.as_ref().filter(|t| !out.terms.contains(t)) {
                out.terms.push(t.clone());
            }
            out.spans.push(Span::styled(r.text.clone(), r.style));
            col += w;
        }
        out
    }
}

enum Item {
    Word(Vec<Run>),
    Break,
}

fn words(runs: &[Run]) -> Vec<Item> {
    let mut out = Vec::new();
    let mut cur: Vec<Run> = Vec::new();
    let end = |cur: &mut Vec<Run>, out: &mut Vec<Item>| {
        if !cur.is_empty() {
            out.push(Item::Word(std::mem::take(cur)));
        }
    };
    for r in runs {
        if r.text == "\n" {
            end(&mut cur, &mut out);
            out.push(Item::Break);
        } else if r.atom {
            cur.push(r.clone());
        } else {
            for (i, part) in r.text.split(' ').enumerate() {
                if i > 0 {
                    end(&mut cur, &mut out);
                }
                if !part.is_empty() {
                    cur.push(Run { text: part.to_string(), ..r.clone() });
                }
            }
        }
    }
    end(&mut cur, &mut out);
    out
}

/// `runs` as lines no wider than `width`: words stay whole when they fit, a word wider than a line is cut.
pub(crate) fn wrap(runs: &[Run], width: usize) -> Vec<Vec<Run>> {
    let width = width.max(1);
    let mut lines: Vec<Vec<Run>> = vec![Vec::new()];
    let mut used = 0;
    for item in words(runs) {
        let Item::Word(word) = item else {
            lines.push(Vec::new());
            used = 0;
            continue;
        };
        let w: usize = word.iter().map(|r| r.text.width()).sum();
        let gap = usize::from(used > 0);
        if used > 0 && used + gap + w > width {
            lines.push(Vec::new());
            used = 0;
        }
        let line = lines.last_mut().unwrap_or_else(|| unreachable!("a line is always there"));
        if w <= width.saturating_sub(used + usize::from(used > 0)) {
            if used > 0 {
                // the blank between two words of one mark (a link, a strike) carries it
                let same = line.last().zip(word.first()).filter(|(a, b)| a.style == b.style && a.link == b.link);
                line.push(Run { text: " ".into(), style: same.map_or_else(Style::default, |(a, _)| a.style), link: same.and_then(|(a, _)| a.link.clone()), atom: false, term: None });
                used += 1;
            }
            line.extend(word);
            used += w;
            continue;
        }
        // a word wider than a line is cut where the line ends
        if used > 0 {
            lines.push(Vec::new());
            used = 0;
        }
        for run in word {
            let mut piece = String::new();
            for ch in run.text.chars() {
                let cw = ch.width().unwrap_or(0);
                if used + cw > width {
                    let line = lines.last_mut().unwrap_or_else(|| unreachable!("a line is always there"));
                    line.push(Run { text: std::mem::take(&mut piece), ..run.clone() });
                    lines.push(Vec::new());
                    used = 0;
                }
                piece.push(ch);
                used += cw;
            }
            let line = lines.last_mut().unwrap_or_else(|| unreachable!("a line is always there"));
            line.push(Run { text: piece, ..run.clone() });
        }
    }
    lines
}
