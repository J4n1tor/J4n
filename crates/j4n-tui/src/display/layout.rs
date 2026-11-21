//! `layout`: where the pipes of a tree go, said once. The base is a pure function of a flat list of items, each with a
//! key, the key of its parent and whether it is a fold row; it knows no glyph. A style maps its pieces to glyphs and indent (`Look`, from `theme.frame`).
//! The rules: an item is the last of its parent when no later item has the same parent; a bar runs in an ancestor's column only while that ancestor has a
//! later sibling, folded or not; a fold row is an item like any other, and the chats it holds hang under it, one level in. [`place`] is the same rule for a
//! caller that walks a tree itself (the chat transcript): one row among `n` siblings under a parent it already placed.

use crate::theme::glyph::Slot;
use crate::theme::{Frame, Theme};
use std::collections::HashMap;
use unicode_width::UnicodeWidthStr;

/// One piece of what stands before a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Piece {
    /// An ancestor that has a later sibling: its column goes on.
    Bar,
    /// An ancestor that was the last.
    Blank,
    Tee,
    Elbow,
    /// The state of a fold row, drawn at its end.
    FoldOpen,
    FoldClosed,
}

/// An item of the flat list, in the order the tree is read (a parent before its children, the siblings in their order).
#[derive(Clone, Copy, Debug)]
pub struct Item<'a> {
    pub key: &'a str,
    /// The key of the parent; none, or a key that is not in the list, is the top.
    pub parent: Option<&'a str>,
    /// A fold row: `Some(open)`.
    pub fold: Option<bool>,
}

impl<'a> Item<'a> {
    pub fn new(key: &'a str, parent: Option<&'a str>) -> Self {
        Item { key, parent, fold: None }
    }

    pub fn fold(key: &'a str, parent: Option<&'a str>, open: bool) -> Self {
        Item { key, parent, fold: Some(open) }
    }
}

/// What the base says of one line: the pieces before it, one for each ancestor and then its own tee or elbow, and the fold piece at its end.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Laid {
    pub pipes: Vec<Piece>,
    /// The first of its parent.
    pub first: bool,
    /// The last of its parent.
    pub last: bool,
    /// How many ancestors it has.
    pub level: usize,
    pub fold: Option<Piece>,
}

impl Laid {
    /// The columns of the lines that go on under this row: a bar for each ancestor, and for the row itself, that it has a later sibling (a tee) or not (an elbow).
    pub fn stem(&self) -> Vec<Piece> {
        self.pipes.iter().map(|x| if matches!(x, Piece::Tee | Piece::Bar) { Piece::Bar } else { Piece::Blank }).collect()
    }
}

/// The place of the row at `at` of `n` siblings under `parent` (none: at the top). The rule of [`lay`] for a caller that has the tree already: the last of the
/// siblings is the elbow, every other a tee, and the columns of the parent's pipes go on as bars or blanks.
pub fn place(parent: Option<&Laid>, at: usize, n: usize) -> Laid {
    let mut pipes = parent.map_or_else(Vec::new, Laid::stem);
    let last = at + 1 >= n;
    pipes.push(if last { Piece::Elbow } else { Piece::Tee });
    Laid { level: pipes.len() - 1, pipes, first: at == 0, last, fold: None }
}

/// A row of a stack at the top (the fields of a form, the rows of a card, a section of a page) when only whether it ends the stack is known (`last`).
pub fn one(last: bool) -> Laid {
    Laid { pipes: vec![if last { Piece::Elbow } else { Piece::Tee }], first: false, last, level: 0, fold: None }
}

/// Lay the items out: one [`Laid`] for each, in their order.
pub fn lay(items: &[Item]) -> Vec<Laid> {
    let index: HashMap<&str, usize> = items.iter().enumerate().map(|(i, it)| (it.key, i)).collect();
    // a parent that is not before its child in the list is the top
    let parent: Vec<Option<usize>> = items.iter().enumerate().map(|(i, it)| it.parent.and_then(|p| index.get(p).copied()).filter(|p| *p < i)).collect();
    let mut last = vec![true; items.len()];
    let mut first = vec![true; items.len()];
    let mut seen: HashMap<Option<usize>, usize> = HashMap::new();
    for (i, p) in parent.iter().enumerate() {
        if let Some(before) = seen.insert(*p, i) {
            last[before] = false;
            first[i] = false;
        }
    }
    let mut out: Vec<Laid> = Vec::with_capacity(items.len());
    for (i, it) in items.iter().enumerate() {
        let mut pipes: Vec<Piece> = parent[i].map_or_else(Vec::new, |p| out[p].stem());
        pipes.push(if last[i] { Piece::Elbow } else { Piece::Tee });
        let fold = it.fold.map(|open| if open { Piece::FoldOpen } else { Piece::FoldClosed });
        out.push(Laid { level: pipes.len() - 1, pipes, first: first[i], last: last[i], fold });
    }
    out
}

/// A style: the mapping from the pieces to glyphs and indent, one for each look of `theme.frame`. The widths are cells.
#[derive(Clone, Copy)]
pub struct Look<'a> {
    th: &'a Theme,
    /// The tree hangs from a head above it (the operator's line in the chat): its first row is a tee, not the corner that opens a tree, and the columns of pipes and
    /// rounds are a blank wider, the room the icon of a row has.
    wide: bool,
    /// The pipes of a list whose rows start with words (a page, a card, an event): a column is a glyph and a blank (`├ `, `╰ `), not the two glyphs of a tree (`├─`), and no corner opens it.
    tight: bool,
    /// Blank cells before the first column: the tree hangs from the head's words and not from its edge.
    lead: usize,
}

impl<'a> Look<'a> {
    /// The look of a list that opens its own tree.
    pub fn new(th: &'a Theme) -> Self {
        Look { th, wide: false, tight: false, lead: 0 }
    }

    /// The look of a tree that hangs from a head (the chat transcript): `lead` is the head's text column when a cap thicker than a pipe stands at its left edge, else 0.
    pub fn headed(th: &'a Theme, lead: usize) -> Self {
        Look { th, wide: true, tight: false, lead }
    }

    /// The look of a list whose rows start with words: a column of pipes is a glyph and a blank, so a row's words start two cells in (the pages of lists, the cards, the events).
    pub fn tight(th: &'a Theme) -> Self {
        Look { th, wide: false, tight: true, lead: 0 }
    }

    /// The text of one piece, from the column it stands in (0 is the top); `top`: it is the first row of a tree that opens (the corner of `round`).
    /// **pipes:** `│ ` for an ancestor that goes on, `├─` and the square `└─` for a row's own. **round** (the default): the same with the rounded `╰─`, and `╭─` at the first row of a list. **hang:** the top column
    /// has no tree, a rail (`┆ `) runs only where something nests and `╰ ` ends it. **airy:** no lines, `· ` at the top and ` › ` for what nests, the blank
    /// columns two cells at the top and three below. **gutter:** the list has no word column, so nesting is an indent of two cells and the top column is empty.
    fn cell(&self, piece: Piece, col: usize, top: bool) -> String {
        let g = |s: Slot| self.th.glyph(s);
        let pad = if self.wide { " " } else { "" };
        match (self.th.frame, piece) {
            (_, Piece::FoldOpen) => g(Slot::FoldOpen).to_string(),
            (_, Piece::FoldClosed) => g(Slot::FoldClosed).to_string(),
            (Frame::Pipes | Frame::Round, Piece::Bar) => format!("{} {pad}", g(Slot::PipeBar)),
            (Frame::Pipes | Frame::Round, Piece::Blank) => format!("  {pad}"),
            (Frame::Pipes | Frame::Round, Piece::Tee) if self.tight => format!("{} ", g(Slot::BranchTee)),
            (Frame::Pipes, Piece::Elbow) if self.tight => format!("{} ", g(Slot::BranchSquare)),
            (Frame::Round, Piece::Elbow) if self.tight => format!("{} ", g(Slot::BranchElbow)),
            (Frame::Pipes | Frame::Round, Piece::Tee) => format!("{}{pad}", g(if top { Slot::PipeTop } else { Slot::PipeTee })),
            (Frame::Pipes, Piece::Elbow) => format!("{}{pad}", g(Slot::PipeSquare)),
            (Frame::Round, Piece::Elbow) => format!("{}{pad}", g(Slot::PipeElbow)),
            (Frame::Airy, Piece::Bar | Piece::Blank) => " ".repeat(if col == 0 { 2 } else { 3 }),
            (Frame::Airy, _) if col == 0 => format!("{} ", g(Slot::AiryTop)),
            (Frame::Airy, _) => format!(" {} ", g(Slot::AiryChild)),
            (_, _) if col == 0 => String::new(),
            (Frame::Hang, Piece::Bar | Piece::Tee) => format!("{} ", g(Slot::HangBar)),
            (Frame::Hang, Piece::Elbow) => format!("{} ", g(Slot::BranchElbow)),
            (_, _) => "  ".to_string(),
        }
    }

    /// What stands before a row, one text for each column, its own last: the edge colour is the caller's.
    pub fn cells(&self, laid: &Laid) -> Vec<String> {
        let top = self.th.frame == Frame::Round && !self.wide && !self.tight && laid.first && !laid.last && laid.level == 0;
        self.hung(laid.pipes.iter().enumerate().map(|(col, p)| self.cell(*p, col, top && col == laid.level)))
    }

    /// The columns of a row after the blank the tree hangs in (`lead`).
    fn hung(&self, cols: impl Iterator<Item = String>) -> Vec<String> {
        Some(" ".repeat(self.lead)).filter(|s| !s.is_empty()).into_iter().chain(cols).collect()
    }

    /// What stands before a row: the text of its pieces.
    pub fn prefix(&self, laid: &Laid) -> String {
        self.cells(laid).concat()
    }

    /// What stands before a row of a stack that has one level (a form's fields, a card's rows, a section of a page), and before the lines that go on under it: two cells
    /// each, whatever the look (the top column of `hang` has no tree, a blank stands in its place), and no corner of `round`: a stack does not open a tree.
    pub fn stack(&self, laid: &Laid) -> (String, String) {
        let two = |s: String| format!("{s}{}", " ".repeat(2usize.saturating_sub(s.width())));
        let look = Look { tight: true, wide: false, ..*self };
        let (own, under) = (laid.pipes.last().copied().unwrap_or(Piece::Elbow), laid.stem().last().copied().unwrap_or(Piece::Blank));
        (two(look.cell(own, 0, false)), two(look.cell(under, 0, false)))
    }

    /// What stands before the lines that go on under a row (its output, its preview), one text for each column.
    pub fn under(&self, laid: &Laid) -> Vec<String> {
        self.hung(laid.stem().iter().enumerate().map(|(col, p)| self.cell(*p, col, false)))
    }

    /// The end of a fold row: its glyph.
    pub fn fold(&self, piece: Piece) -> &'static str {
        match piece {
            Piece::FoldOpen => self.th.glyph(Slot::FoldOpen),
            _ => self.th.glyph(Slot::FoldClosed),
        }
    }

    /// The row that says how many are left out: the elbow of the list.
    pub fn more(&self) -> String {
        self.prefix(&place(None, 0, 1))
    }
}
