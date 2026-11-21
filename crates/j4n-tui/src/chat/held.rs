//! The composer's guard: a line that may hold a key goes nowhere a line is kept. A `:key …` line is said (the core keeps only `:key …` of
//! it in the Log, and refuses it), but it is kept in neither the history nor the echo, and it is never asked of the completion. A line
//! that is one key-like word is held before anything of it goes (no line, no draft, no completion), on a card: `keep it as a key` sends it
//! on the key's one road, `Intent::Key`, for the provider picked on the card from the rows of Agents › Providers; `send it as text` says it
//! as it was typed; `drop it` (or `esc`) sends nothing. The card shows nothing of the line.

use crate::app::App;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use j4n_proto::{AskView, Choice, FieldKind, Intent, Secret, Shape, Tone, Up};

/// The rows of Agents › Providers, watched while a line is held: a row whose `⏎` types a key is a provider the card offers.
pub(crate) const PROVIDERS: &str = "view:providers";

/// The fewest characters of a word that may be a key, and the longest run of letters and digits it has at the least.
const LEAST: usize = 20;
const RUN: usize = 12;

/// The card's name among the questions: one of the screen's own, with no task behind it.
pub(crate) const ASK: &str = "held:line";

/// Whether a line is `:key …` or `/key …` (in any case, after a chat's name) with a word after it: that word may be a key typed where it does not go.
pub(crate) fn key_line(text: &str) -> bool {
    let mut words = text.split_whitespace().peekable();
    if words.peek().is_some_and(|w| w.starts_with('@')) {
        words.next();
    }
    words.next().is_some_and(|w| w.eq_ignore_ascii_case(":key") || w.eq_ignore_ascii_case("/key")) && words.next().is_some()
}

/// Where the words after `:key` (or `/key`, in any case, after a chat's name) start in a line, counted in characters: the composer draws
/// them masked as they are typed, so a key said on a chat line is never on the screen, not even before its ⏎. `None` for any other line.
pub(crate) fn key_tail(text: &str) -> Option<usize> {
    let chars: Vec<char> = text.chars().collect();
    let mut at = 0;
    let word = |at: &mut usize| -> (usize, usize) {
        while *at < chars.len() && chars[*at].is_whitespace() {
            *at += 1;
        }
        let start = *at;
        while *at < chars.len() && !chars[*at].is_whitespace() {
            *at += 1;
        }
        (start, *at)
    };
    let (mut s, mut e) = word(&mut at);
    if chars.get(s) == Some(&'@') {
        (s, e) = word(&mut at);
    }
    let first: String = chars[s..e].iter().collect();
    // the mask starts at the space after the word, so `:key` itself stays readable and a key typed with no space yet is not drawn
    (e < chars.len() && (first.eq_ignore_ascii_case(":key") || first.eq_ignore_ascii_case("/key"))).then_some(e)
}

/// The key a word stands for, and the name left out of it: the value of an environment line (`NAME=value`, as the key's editor reads a
/// paste), or the word itself; either out of its quotes.
fn bare(text: &str) -> (Option<&str>, &str) {
    let (name, w) = crate::fields::named(text.trim());
    let quoted = w.len() >= 2 && ['"', '\''].iter().any(|q| w.starts_with(*q) && w.ends_with(*q));
    (name, if quoted { &w[1..w.len() - 1] } else { w })
}

/// Whether a line is one word that looks like a key (an environment line's value, a word in quotes, are read as the key they hold): 20
/// characters or more, all of the characters keys are made of (letters, digits, `- _ . + / =`), letters and digits both, a run of 12
/// letters and digits in it; and neither a command, a chat's name, a file (a name with a short extension) nor an id of the shape `8-4-4-4-12`.
pub(crate) fn key_like(text: &str) -> bool {
    let w = bare(text).1;
    if w.chars().count() < LEAST || w.starts_with([':', '/', '@']) || !w.chars().all(|c| c.is_ascii_alphanumeric() || "-_.+/=".contains(c)) {
        return false;
    }
    let file = w.rsplit_once('.').is_some_and(|(_, ext)| (1..=5).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_lowercase()));
    let parts: Vec<&str> = w.split('-').collect();
    let id = parts.iter().map(|p| p.len()).eq([8, 4, 4, 4, 12]) && parts.iter().all(|p| p.chars().all(|c| c.is_ascii_hexdigit()));
    let run = runs(w).map(str::len).max().unwrap_or(0);
    w.chars().any(|c| c.is_ascii_digit()) && w.chars().any(|c| c.is_ascii_alphabetic()) && run >= RUN && !file && !id
}

/// The runs of letters and digits of a word, as [`key_like`] measures them.
fn runs(w: &str) -> impl Iterator<Item = &str> {
    w.split(|c: char| !c.is_ascii_alphanumeric())
}

/// Whether a part of a word has a run of letters and digits as long as a key's, with letters and digits both: the start of a key, or a
/// hash, which no name the completion offers is.
pub(crate) fn keyish(w: &str) -> bool {
    runs(w).any(|r| r.len() >= RUN && r.chars().any(|c| c.is_ascii_digit()) && r.chars().any(|c| c.is_ascii_alphabetic()))
}

/// A line held on the card: the word itself, wiped when it goes, and whether the card asks which provider.
pub struct Held {
    value: Secret,
    which: bool,
}

impl App {
    /// ⏎ on a line that is one key-like word: it is held on the card, the line is emptied, and the keys of the same burst are dropped.
    pub(crate) fn hold(&mut self, text: String) -> Vec<Up> {
        self.held = Some(Held { value: Secret::new(text), which: false });
        self.set_input(String::new());
        self.burst = self.clock;
        self.redraw = true;
        self.sync()
    }

    /// A key while a line is held and a chat has the keys: the card takes it. `1` (or ⏎ or ⇥, its recommended answer) keeps it as a key, `2`
    /// says it as text, `3` or `esc` drops it; when the card asks which provider, a number picks one. Any other key does nothing; a key
    /// with ⌃ or ⌥ goes on.
    pub(crate) fn held_key(&mut self, k: KeyEvent) -> Option<Vec<Up>> {
        let which = self.held.as_ref()?.which;
        if k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) || self.field().is_none_or(|f| f.kind != FieldKind::Chat) {
            return None;
        }
        let picked = |c: char| c.to_digit(10).and_then(|d| usize::try_from(d).ok()).filter(|d| *d > 0).map(|d| d - 1);
        Some(match (which, k.code) {
            (_, KeyCode::Esc) | (false, KeyCode::Char('3')) => {
                self.held = None;
                self.toast = Some(("the line is dropped · nothing of it was sent".into(), Tone::Dim));
                self.sync()
            }
            (false, KeyCode::Char('2')) => {
                let text = self.held.take().map(|h| h.value.expose().to_string()).unwrap_or_default();
                let mut ups = self.say_with(text, Vec::new());
                ups.extend(self.sync());
                ups
            }
            (false, KeyCode::Char('1') | KeyCode::Enter | KeyCode::Tab) => match self.held_rows().as_slice() {
                [(id, _)] => self.held_send(&id.clone()),
                _ => {
                    self.held.iter_mut().for_each(|h| h.which = true);
                    Vec::new()
                }
            },
            (true, KeyCode::Char(c)) => match picked(c).and_then(|i| self.held_rows().get(i).map(|(id, _)| id.clone())) {
                Some(id) => self.held_send(&id),
                None => Vec::new(),
            },
            _ => Vec::new(),
        })
    }

    /// The key the held word stands for, sent once as `Intent::Key` for `id`: never a line or a `Set`. An environment line's name and the
    /// quotes are left out, as the key's editor leaves them out of a paste; the word itself is wiped when it goes.
    fn held_send(&mut self, id: &str) -> Vec<Up> {
        let Some(h) = self.held.take() else { return Vec::new() };
        let value = Secret::new(bare(h.value.expose()).1.to_string());
        let mut ups = vec![Up::Intent { intent: Intent::Key { id: id.to_string(), value } }];
        ups.extend(self.sync());
        ups
    }

    /// The providers the card offers: the rows of Agents › Providers whose `⏎` types a key, as the key's id and the row's name.
    fn held_rows(&self) -> Vec<(String, String)> {
        let rows = self.rows.get(PROVIDERS).map_or(&[][..], Vec::as_slice);
        rows.iter()
            .filter_map(|r| {
                let id = r.actions.iter().find_map(|a| match &a.intent {
                    Intent::Set { kind, id, .. } if a.key == "enter" && kind == "key" => Some(id.clone()),
                    _ => None,
                })?;
                Some((id, r.cells.first().map_or_else(|| r.key.clone(), |c| c.text.clone())))
            })
            .collect()
    }

    /// The card of the held line, drawn as a question above the composer; none while no line is held. Nothing of the line is in it.
    pub(crate) fn held_view(&self) -> Option<AskView> {
        let h = self.held.as_ref()?;
        let rows = self.held_rows();
        let choice = |id: &str, label: String, recommended: bool| Choice { id: id.into(), label, recommended, ..Choice::default() };
        let (question, options) = if h.which {
            let q = if rows.is_empty() { "no provider here takes a key" } else { "keep the key for which provider?" };
            (q.to_string(), rows.iter().map(|(id, name)| choice(id, name.clone(), false)).collect())
        } else {
            let keep = match rows.as_slice() {
                [(_, name)] => format!("keep it as a key for {name}"),
                _ => "keep it as a key".to_string(),
            };
            let q = "this looks like a key, so it is held: nothing of it was sent · a line is kept in the Log and read by the model, a key is shown to no one";
            let q = bare(h.value.expose()).0.map_or_else(|| q.to_string(), |name| format!("{q} · the name {name} is left out of the key"));
            (q, vec![choice("keep", keep, true), choice("text", "send it as text".into(), false), choice("drop", "drop it".into(), false)])
        };
        Some(AskView { ask: ASK.into(), asker: "j4n".into(), question, shape: Shape::Choice, options, ..AskView::default() })
    }
}
