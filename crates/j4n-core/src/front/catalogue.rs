//! What Main is told of J4n: the features it may use for a line, the doers it may give work to, and the skills that fit the project. Built by
//! code from the packs and the roster each time it is asked for, never written by hand, so a pack the operator adds later is in it the next time.
//!
//! A model that chooses only reads what a pack is *for*, so a pack's `use_when` line (what a line looks like that the pack serves) leads, and its
//! summary, written for a developer, is cut short after it. The whole is kept under a token cap (`orchestrate.catalogue`) so the text a cache holds
//! stays small: when it is over, the commands go first, then the summaries, then the skills, then the packs that are off.

use super::Front;
use crate::pack::Info;
use j4n_proto::{Act, Cell, Intent, Row, Tone};

/// Packs no line is about: the core words, the offline test tool, the cases of J4n's own behaviour, the reports to J4n's authors and the seats, which
/// Main is shown on their own when it briefs chats.
pub const LEFT_OUT: [&str; 5] = ["base", "test", "bench", "contribute", "crew"];

/// The most skills named.
const SKILLS: usize = 12;

/// The catalogue as a model is shown it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalogue {
    pub text: String,
    /// An estimate of the tokens in `text` (about four characters each).
    pub tokens: usize,
    /// The packs it names, in its order.
    pub packs: Vec<String>,
}

/// One doer the roster offers and the models it serves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Roster {
    pub doer: String,
    pub models: Vec<String>,
}

/// How much of each line is kept: the levels the cap takes away one after another.
#[derive(Clone, Copy)]
struct Detail {
    commands: bool,
    summary: usize,
    skills: bool,
}

const LEVELS: [Detail; 4] = [Detail { commands: true, summary: 90, skills: true }, Detail { commands: false, summary: 90, skills: true }, Detail { commands: false, summary: 0, skills: true }, Detail { commands: false, summary: 0, skills: false }];

fn tier(model: &str) -> &'static str {
    match model {
        m if m.contains("haiku") => " (cheap)",
        m if m.contains("sonnet") => " (mid)",
        m if m.contains("opus") => " (strong, dear)",
        _ => "",
    }
}

fn render(packs: &[&Info], roster: &[Roster], skills: &[String], d: Detail) -> String {
    let mut out = String::from("Features of J4n, each with when to use it (`off` ones are switched on by a proposal the operator accepts):\n");
    for p in packs {
        let what = if d.summary > 0 { format!("{}; ", crate::text::line(&p.summary, d.summary)) } else { String::new() };
        let when = if p.use_when.is_empty() { String::new() } else { format!("use when {}", p.use_when) };
        let words = if d.commands && !p.words.is_empty() { format!(" ({})", p.words.join(" ")) } else { String::new() };
        let off = if p.on { "" } else { " [off]" };
        out.push_str(&format!("- {}{off}: {what}{when}{words}\n", p.id));
    }
    out.push_str("Doers a chat can be given:");
    for r in roster {
        let models: Vec<String> = r.models.iter().map(|m| format!("{m}{}", tier(m))).collect();
        out.push_str(&format!(" {}{};", r.doer, if models.is_empty() { String::new() } else { format!(" [{}]", models.join(", ")) }));
    }
    out.push('\n');
    if d.skills && !skills.is_empty() {
        out.push_str(&format!("Skills that fit this project: {}.\n", skills.join(", ")));
    }
    out
}

/// The packs the catalogue may name: those a line can be about, and that loaded.
fn listed(packs: &[Info]) -> Vec<&Info> {
    packs.iter().filter(|p| !LEFT_OUT.contains(&p.id.as_str()) && p.note.is_empty()).collect()
}

/// The catalogue of `packs`, `roster` and `skills`, within `cap` tokens as far as shortening can bring it (the roster and one line of each pack stay).
pub fn build(packs: &[Info], roster: &[Roster], skills: &[String], cap: usize) -> Catalogue {
    let listed = listed(packs);
    let skills: Vec<String> = skills.iter().take(SKILLS).cloned().collect();
    let mut text = String::new();
    for d in LEVELS {
        text = render(&listed, roster, &skills, d);
        if crate::brief::tokens(&text) <= cap {
            break;
        }
    }
    // still over: the packs that are off go, the one thing left to take
    let mut on: Vec<&Info> = listed.clone();
    while crate::brief::tokens(&text) > cap && on.iter().any(|p| !p.on) {
        if let Some(at) = on.iter().rposition(|p| !p.on) {
            on.remove(at);
        }
        text = render(&on, roster, &[], LEVELS[3]);
    }
    Catalogue { tokens: crate::brief::tokens(&text), text, packs: on.iter().map(|p| p.id.clone()).collect() }
}

impl Front {
    /// The catalogue as the roster and the packs stand now.
    pub fn catalogue(&self) -> Catalogue {
        let roster = self.roster();
        let skills: Vec<String> = self.0.cfg().skills.as_ref().map(|l| l.all().iter().filter(|s| l.is_on(&s.id)).map(|s| s.id.clone()).collect()).unwrap_or_default();
        build(&self.screen().packs, &roster, &skills, self.0.cfg().orchestrate.catalogue)
    }
}

/// The rows of the page: see [`Front::catalogue_rows`]. `told` is what the catalogue named, so a feature it left out says so.
pub fn rows(packs: &[Info], roster: &[Roster], told: &Catalogue, look: bool) -> Vec<Row> {
    let set = |value: bool| Act { key: "enter".into(), label: if value { "turn the quick look on" } else { "turn the quick look off" }.into(), intent: Intent::Set { kind: "setting".into(), id: "orchestrate.look".into(), field: "value".into(), value: value.into() }, risk: 0 };
    let undo = Act { key: "u".into(), label: "undo".into(), intent: Intent::Run { command: "undo".into(), args: Vec::new() }, risk: 0 };
    let state = |on: bool| Cell::new(if on { "on" } else { "off" }, if on { Tone::Good } else { Tone::Dim });
    let mut rows = vec![Row {
        key: "look".into(),
        cells: vec![Cell::new("quick look", Tone::Plain), state(look), Cell::new(if look { "a line under orchestrate is read against this list first" } else { "off · ⏎ turns it on: a line goes to Main as typed" }, Tone::Dim)],
        actions: vec![set(!look), undo],
        ..Row::default()
    }];
    for p in listed(packs) {
        let line = if told.packs.contains(&p.id) { Cell::new(p.use_when.clone(), Tone::Plain) } else { Cell::new("not told to Main: the list is over its cap", Tone::Warn) };
        rows.push(Row { key: format!("pack:{}", p.id), cells: vec![Cell::new(p.id.clone(), Tone::Plain), state(p.on), line], detail: vec![Cell::new(p.summary.clone(), Tone::Dim)], ..Row::default() });
    }
    for r in roster {
        rows.push(Row { key: format!("doer:{}", r.doer), cells: vec![Cell::new(r.doer.clone(), Tone::Plain), Cell::new("doer", Tone::Dim), Cell::new(r.models.join(", "), Tone::Dim)], ..Row::default() });
    }
    rows
}

impl Front {
    /// The roster as the doers stand now.
    fn roster(&self) -> Vec<Roster> {
        self.0.who.ready("scout", &[]).into_iter().map(|o| Roster { models: self.0.cfg().models.models(&o.id).unwrap_or_default(), doer: o.id }).collect()
    }

    /// `catalogue:main`, read only: the first row says whether the quick look reads it (`⏎` turns that on or off, `u` takes it back), then one row for each feature the
    /// catalogue may name, with when to use it and whether Main is told of it (a cap leaves some out), then the doers a chat can be given. It is made from the data the
    /// catalogue's text is made from, so the page and what Main reads cannot differ.
    pub fn catalogue_rows(&self) -> Vec<Row> {
        rows(&self.screen().packs, &self.roster(), &self.catalogue(), self.0.cfg().orchestrate.look)
    }
}
