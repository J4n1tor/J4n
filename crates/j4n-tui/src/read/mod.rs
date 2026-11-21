//! The reader: one function draws every piece of prose J4n shows — answers, notices, Settings and help
//! descriptions, the Log's detail. It takes text and the theme and gives lines; it knows nothing about how the text was made, and nothing
//! about it goes into any prompt. CommonMark is read by `pulldown-cmark`; the colours are roles and the marks are glyph slots.

mod caps;
mod inline;
mod lay;

pub use caps::{Caps, Late, xtversion};
pub use inline::{Link, RLine};

/// A link on the screen: where its words stand and the address they open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Open {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub url: String,
}

/// A heading to be drawn twice as large: the cell its words start at and the words. It is two rows high and its words are twice as
/// wide as they were laid out; the row under it was left blank.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Big {
    pub x: u16,
    pub y: u16,
    pub text: String,
}

use crate::theme::Theme;
use ratatui::style::Style;

/// How the prose is drawn (`read.*`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opts {
    /// `full`: denser, no extra blank rows, code not folded. `calm` (the default) has the air.
    pub full: bool,
    /// Where prose wraps.
    pub width: usize,
    /// A code block longer than this many lines is folded (calm only); 0 never folds.
    pub fold: usize,
    /// `read.links = osc8`: an address opens on a click where the terminal takes it. `plain`: the address is said in words after the link.
    pub osc8: bool,
    /// Level-1 headings in kitty's text sizing (kitty 0.40 and later, `caps.sizing`): drawn by the terminal after the frame.
    pub big_heads: bool,
    pub caps: Caps,
    /// The folder a path in prose is under, for its `file://` address; none leaves a path as code only.
    pub root: Option<String>,
    /// The ids the project's docs define, with their words (`terms:`); none when `read.terms` is off or nothing came. Only an answer is read with it.
    pub terms: Option<std::sync::Arc<Terms>>,
    /// `read.terms`: the glossary is used when it comes (and kept off when it does not).
    pub terms_on: bool,
}

/// `{id: words}`.
pub type Terms = std::collections::BTreeMap<String, String>;

impl Default for Opts {
    fn default() -> Opts {
        Opts::DEFAULT
    }
}

impl Opts {
    pub const DEFAULT: Opts = Opts { full: false, width: 76, fold: 12, osc8: true, big_heads: false, caps: Caps::PLAIN, root: None, terms: None, terms_on: true };

    /// The options the core named (`read.*`); a word it does not know keeps the calm default.
    pub fn of(r: &j4n_proto::Read) -> Opts {
        Opts { full: r.style == "full", width: (r.width as usize).max(20), fold: r.fold_code as usize, osc8: r.links != "plain", big_heads: r.big_heads, terms_on: r.terms, ..Opts::default() }
    }
}

impl Opts {
    /// The glossary the core sent (`{id: words}`), kept if `read.terms` is on; a value that is not one is none.
    pub fn glossary(&mut self, value: &j4n_proto::Json) {
        let map: Terms = value.as_object().map(|m| m.iter().filter_map(|(k, v)| v.as_str().map(|w| (k.clone(), w.to_string()))).collect()).unwrap_or_default();
        self.terms = (self.terms_on && !map.is_empty()).then(|| std::sync::Arc::new(map));
    }
}

/// `text` as lines at most `width` columns wide (and `o.width`), in the theme's roles, `base` under all of it.
pub fn read(th: &Theme, o: &Opts, base: Style, text: &str, width: usize) -> Vec<RLine> {
    lay::Lay::new(th, o, base, width.min(o.width).max(8)).run(text)
}

/// `text` as plain lines of spans for a place that has no use for links (a description, a notice).
pub fn spans(th: &Theme, o: &Opts, base: Style, text: &str, width: usize) -> Vec<Vec<ratatui::text::Span<'static>>> {
    // a description fills the room it has; only an answer stops at `read.width`
    let o = Opts { width: usize::MAX / 2, terms: None, ..o.clone() };
    read(th, &o, base, text, width).into_iter().map(|l| l.spans).collect()
}

/// `text` as one line of words with its marks taken out (`` `code` ``, `**strong**`), for a cell that is cut to a width.
pub fn flat(th: &Theme, text: &str) -> String {
    let o = Opts { width: 400, ..Opts::DEFAULT };
    let lines: Vec<String> = read(th, &o, Style::default(), text, 400).iter().map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>().trim().to_string()).filter(|l| !l.is_empty()).collect();
    lines.join(" ")
}
