//! The digest and "plan it". What a release said that touches what J4n uses is a row of the fold `watch_items`;
//! the Updates tab (under Grow) lists those rows at any time, and once in a while (`watch.digest`) one line says how many came. A row has one
//! action, a proposal that puts a note on the operator's list when they accept it; nothing is edited by itself.

use j4n_core::change::Where;
use j4n_core::proposal::{Draft, Step, StepOp};
use j4n_core::value::Value;

const DAY_MS: u64 = 24 * 3_600_000;
/// The longest title of a note, and the most names listed in its reason.
const TITLE: usize = 100;
const NAMES: usize = 8;

/// When the digest is said (`watch.digest`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cadence {
    Never,
    /// Once each time J4n starts.
    Open,
    Daily,
    Weekly,
}

impl Cadence {
    pub fn word(w: &str) -> Cadence {
        match w {
            "never" => Cadence::Never,
            "open" => Cadence::Open,
            "daily" => Cadence::Daily,
            _ => Cadence::Weekly,
        }
    }

    /// How long after the last digest (or the oldest thing not yet said) the next one is due; `open` has no wait.
    fn wait(self) -> Option<u64> {
        match self {
            Cadence::Never => None,
            Cadence::Open => Some(0),
            Cadence::Daily => Some(DAY_MS),
            Cadence::Weekly => Some(7 * DAY_MS),
        }
    }

    /// Whether the digest is due at `now`, given the items that came after the last one (the time each came), and, for `open`, whether this
    /// run of J4n has said it. Nothing waiting is nothing to say.
    pub fn due(self, waiting: &[u64], said_this_run: bool, now: u64) -> bool {
        let Some(wait) = self.wait() else { return false };
        let Some(oldest) = waiting.iter().min() else { return false };
        match self {
            Cadence::Open => !said_this_run,
            _ => now.saturating_sub(*oldest) >= wait,
        }
    }
}

/// One release that matters, as the fold holds it.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub id: String,
    pub source: String,
    pub of: String,
    pub version: String,
    pub class: String,
    pub lead: String,
    pub counts: Vec<u64>,
    pub names: Vec<String>,
    pub at_ms: u64,
}

impl Item {
    /// The row of the fold `watch_items` under `id` (`<source>:<version>`).
    pub fn of_row(id: &str, row: &Value) -> Option<Item> {
        let Value::Map(m) = row else { return None };
        let text = |k: &str| match m.get(k) {
            Some(Value::Str(s)) => s.clone(),
            _ => String::new(),
        };
        let counts = match m.get("counts") {
            Some(Value::List(l)) => l.iter().map(|v| if let Value::Num(n) = v { *n as u64 } else { 0 }).collect(),
            _ => Vec::new(),
        };
        let mut names: Vec<String> = Vec::new();
        if let Some(Value::List(items)) = m.get("items") {
            for item in items {
                if let Value::Map(i) = item
                    && let Some(Value::List(ns)) = i.get("names")
                {
                    for n in ns {
                        if let Value::Str(n) = n
                            && !names.contains(n)
                        {
                            names.push(n.clone());
                        }
                    }
                }
            }
        }
        let at_ms = match m.get("at_ms") {
            Some(Value::Num(n)) => *n as u64,
            _ => 0,
        };
        Some(Item { id: id.into(), source: text("source"), of: text("of"), version: text("version"), class: text("class"), lead: text("lead"), counts, names, at_ms })
    }

    /// A word for a file name and an id: the source and the version, lower case, one dash between runs of anything else.
    pub fn slug(&self) -> String {
        let mut out = String::new();
        for c in format!("{}-{}", self.source, self.version).chars() {
            if c.is_ascii_alphanumeric() {
                out.push(c.to_ascii_lowercase());
            } else if !out.ends_with('-') {
                out.push('-');
            }
        }
        out.trim_matches('-').to_string()
    }

    /// The proposal "plan it": a note on the operator's list (kind `todo`, in their own folder). What the release said is clipped and
    /// kept to one line, and is only ever data in the note.
    pub fn plan(&self) -> Draft {
        let name = self.of.strip_prefix("connector:").unwrap_or(&self.of);
        let lead = one_line(&self.lead, TITLE);
        let title = if lead.is_empty() { format!("Look at {name} {}", self.version) } else { format!("Look at {name} {}: {lead}", self.version) };
        let names = if self.names.is_empty() { String::new() } else { format!("; it names {}", self.names.iter().take(NAMES).cloned().collect::<Vec<_>>().join(", ")) };
        let [breaks, touches] = [self.counts.first().copied().unwrap_or(0), self.counts.get(1).copied().unwrap_or(0)];
        let why = format!("{name} {} is out: {breaks} line(s) of its notes say something J4n uses was removed or changed, {touches} more name something it uses{names}.", self.version);
        let mut row = toml::Table::new();
        for (k, v) in [("title", title.as_str()), ("of", &self.of), ("version", &self.version), ("why", &why), ("state", "open")] {
            row.insert(k.into(), toml::Value::String(v.into()));
        }
        Draft {
            key: format!("plan:{}", self.id),
            title: title.clone(),
            why,
            evidence: breaks + touches,
            steps: vec![Step { to: Where::User, file: Some("todo.toml".into()), kind: "todo".into(), id: self.slug(), op: StepOp::Set(row) }],
            widens: Vec::new(),
        }
    }
}

/// Text from outside, as one clipped line with no control characters.
pub fn one_line(text: &str, most: usize) -> String {
    let clean: String = text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    if clean.chars().count() > most { format!("{}…", clean.chars().take(most).collect::<String>()) } else { clean }
}
