//! `hint`: the words a step of J4n's own takes while it runs (a check, a review), drawn dim in the composer's own line and nowhere else. The core
//! sends the words and the step (`composer:strip`'s `skip`); the screen holds none of them. One hint at a time and only while it can act: in the empty
//! line it stands where the placeholder would (`skip · stop  check t50`), and while the line is the start of one of its words it is that word at the
//! right edge with the key that sends it (`skip ⏎`); any other line, or a question's card, hides it. A word typed in full and `⏎` is the confirm, so the
//! part has no key of its own.

use crate::theme::Theme;
use crate::theme::glyph::Slot;
use crate::theme::palette::Role as Hue;
use j4n_proto::{Json, Tone};
use ratatui::text::Span;

/// What the core says can end the step that runs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hint {
    pub words: Vec<String>,
    /// The step the words end: `check t50`.
    pub what: String,
}

impl Hint {
    /// The hint of `composer:strip`'s `skip`; none when it holds no word.
    pub fn of(v: Option<&Json>) -> Option<Hint> {
        let v = v?;
        let words: Vec<String> = v.get("words")?.as_array()?.iter().filter_map(Json::as_str).filter(|w| !w.is_empty()).map(str::to_string).collect();
        (!words.is_empty()).then(|| Hint { words, what: v.get("what").and_then(Json::as_str).unwrap_or_default().to_string() })
    }

    /// The word the line is the start of (case and the spaces around it do not count); none for an empty line or any other.
    pub fn word(&self, typed: &str) -> Option<&str> {
        let t = typed.trim().to_lowercase();
        (!t.is_empty()).then(|| self.words.iter().find(|w| w.starts_with(&t)).map(String::as_str)).flatten()
    }
}

/// The hint in the empty line: the words, then the step they end, all dim.
pub fn empty(th: &Theme, h: &Hint) -> Vec<Span<'static>> {
    let mut out = vec![Span::styled(h.words.join(" · "), th.tone(Tone::Dim))];
    if !h.what.is_empty() {
        out.push(Span::styled(format!("  {}", h.what), th.fg(Hue::Faint)));
    }
    out
}

/// The hint at the right of a line that is the start of one of its words: that word and the key that sends it.
pub fn tail(th: &Theme, h: &Hint, typed: &str) -> Option<Vec<Span<'static>>> {
    let w = h.word(typed)?;
    Some(vec![Span::styled(format!("{w} {}", th.glyph(Slot::KeyEnter)), th.tone(Tone::Dim))])
}
