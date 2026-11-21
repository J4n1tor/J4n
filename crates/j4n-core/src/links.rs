//! Links: who may talk to whom, as data. A message the front passes between Main, a chat and an envoy names what it carries; code holds it to the rows of the kind `link`
//! (the pack `crew` or the operator's own), and one that no row carries is refused with the reason. This is the data and the decisions with no front and no task in it:
//! the rows, the endpoints, the cycle rule at load and the admission of one message, which a test reads as the front does.
//!
//! Two sets. `star` is code: the routing there is today (the envoy's request to Main, Main's brief to a chat, a chat's report and opinion to Main, Main's report to the
//! envoy, a question of the work to the envoy), with each message bounded by what bounds it today. `declared` is the rows the operator or a pack wrote, and nothing else:
//! a message no row carries is refused, a row's `hops` is the most times one thread may pass it, and a cycle of rows that no `hops` caps is refused at load.

use crate::table::{Kind, Problem, Tables};
use serde::Deserialize;
use std::fmt;

/// What a link may carry.
pub const CARRIES: [&str; 6] = ["request", "brief", "report", "opinion", "question", "note"];
/// The most times a thread passes a link that names no `hops`.
pub const DEFAULT_HOPS: u32 = 3;
/// The most requests one charter sends Main, the first included (the star's `hops` for the envoy's request).
pub const REQUEST_HOPS: u32 = 3;
/// The most rows a set holds.
const MAX_ROWS: usize = 100;

/// An entry of kind `link`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Link {
    pub id: String,
    pub doc: Option<String>,
    pub from: String,
    pub to: String,
    pub carries: Vec<String>,
    /// How many times a thread may pass the link: left out, [`DEFAULT_HOPS`]; `0`, no cap.
    pub hops: Option<u32>,
}

impl Kind for Link {
    const NAME: &'static str = "link";
}

impl Link {
    fn row(id: &str, from: &str, to: &str, carries: &[&str], hops: u32) -> Link {
        Link { id: id.into(), doc: None, from: from.into(), to: to.into(), carries: carries.iter().map(|c| c.to_string()).collect(), hops: Some(hops) }
    }

    /// The most times a thread may pass this link; none: no cap.
    pub fn cap(&self) -> Option<u32> {
        match self.hops {
            None => Some(DEFAULT_HOPS),
            Some(0) => None,
            Some(n) => Some(n),
        }
    }

    fn check(&self, seats: &[String]) -> Result<(), String> {
        let bad = |what: String| Err(format!("link `{}`: {what}", self.id));
        if !word(&self.id) {
            return bad("the id is one lowercase word".into());
        }
        for (name, end) in [("from", &self.from), ("to", &self.to)] {
            if let Err(e) = endpoint_ok(end, seats) {
                return bad(format!("`{name}`: {e}"));
            }
        }
        if self.carries.is_empty() {
            return bad("it carries nothing".into());
        }
        if let Some(c) = self.carries.iter().find(|c| !CARRIES.contains(&c.as_str())) {
            return bad(format!("`{c}` is not something a link carries ({})", CARRIES.join(", ")));
        }
        if self.hops.is_some_and(|h| h > 100) {
            return bad("`hops` is at most 100".into());
        }
        Ok(())
    }
}

fn word(w: &str) -> bool {
    !w.is_empty() && w.len() <= 40 && w.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.'))
}

fn chat_id(e: &str) -> bool {
    e.strip_prefix('c').is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
}

/// Whether `e` names a party: `main`, `any`, `chat` (any chat Main started), `c4` (one chat), `envoy` or `envoy:c4`, or a seat that is declared.
fn endpoint_ok(e: &str, seats: &[String]) -> Result<(), String> {
    match e {
        "main" | "any" | "chat" | "envoy" => Ok(()),
        _ if chat_id(e) => Ok(()),
        _ => match e.strip_prefix("envoy:") {
            Some(id) if chat_id(id) => Ok(()),
            Some(_) => Err("`envoy:` is followed by the envoy chat's id, such as `envoy:c4`".into()),
            None if seats.iter().any(|s| s == e) => Ok(()),
            None => Err(format!("`{e}` is not `main`, `any`, `chat`, `envoy`, `envoy:<chat>`, a chat's id or a seat that is declared")),
        },
    }
}

/// Who sends or takes a message, as the core's own records say it: never a name a model wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Party {
    Main,
    /// A chat Main started, with the seat its brief named.
    Chat { id: String, seat: Option<String> },
    /// An envoy chat.
    Envoy(String),
}

impl Party {
    pub fn name(&self) -> String {
        match self {
            Party::Main => "main".into(),
            Party::Chat { id, seat: Some(s) } => format!("{id} (seat {s})"),
            Party::Chat { id, seat: None } => id.clone(),
            Party::Envoy(id) => format!("envoy:{id}"),
        }
    }

    /// Whether the endpoint `e` of a row names this party.
    pub fn is(&self, e: &str) -> bool {
        match (e, self) {
            ("any", _) | ("main", Party::Main) | ("chat", Party::Chat { .. }) | ("envoy", Party::Envoy(_)) => true,
            (_, Party::Chat { id, seat }) => e == id || seat.as_deref() == Some(e) && !chat_id(e),
            (_, Party::Envoy(id)) => e.strip_prefix("envoy:") == Some(id.as_str()),
            _ => false,
        }
    }
}

/// Whether two endpoints can name the same party: what makes one row follow another. Two chats' ids, two seats and two envoys' ids never do; a chat's id and a seat's
/// name might (the chat sits in that seat).
fn overlap(a: &str, b: &str) -> bool {
    let class = |e: &str| if e == "main" { 1 } else if e == "envoy" || e.starts_with("envoy:") { 2 } else { 3 };
    if a == "any" || b == "any" {
        return true;
    }
    if class(a) != class(b) {
        return false;
    }
    match class(a) {
        1 => true,
        2 => a == b || a == "envoy" || b == "envoy",
        _ => a == b || a == "chat" || b == "chat" || chat_id(a) != chat_id(b),
    }
}

/// Why a message was not passed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// No row carries it.
    Undeclared(String),
    /// A row does, and the thread has passed it as many times as the row allows.
    Hops { link: String, cap: u32 },
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refused::Undeclared(w) => f.write_str(w),
            Refused::Hops { link, cap } => write!(f, "link `{link}` has been passed {cap} time{} in this thread: it stops here, ask the operator before another", if *cap == 1 { "" } else { "s" }),
        }
    }
}

/// The rows that say who may talk to whom.
#[derive(Debug, Clone)]
pub struct Links {
    declared: bool,
    rows: Vec<Link>,
}

impl Default for Links {
    fn default() -> Links {
        Links::star()
    }
}

impl Links {
    /// The routing there is today. Each message is bounded by what bounds it today (a charter's three requests, the chats Main may start, the plan's own rounds), so only
    /// the envoy's request names a cap.
    pub fn star() -> Links {
        let rows = vec![
            Link::row("star-request", "envoy", "main", &["request"], REQUEST_HOPS),
            Link::row("star-brief", "main", "chat", &["brief"], 0),
            Link::row("star-report", "chat", "main", &["report", "opinion"], 0),
            Link::row("star-answer", "main", "envoy", &["report"], 0),
            Link::row("star-question", "any", "envoy", &["question"], 0),
        ];
        Links { declared: false, rows }
    }

    /// The set a setting asks for: the star, or the rows of the Table (kind `link`); a row that does not read, or sits on a cycle no `hops` caps, is left out and its fault
    /// returned. `seats` are the ids of the seats that are declared, which an endpoint may name.
    pub fn load(tables: &Tables, declared: bool, seats: &[String]) -> (Links, Vec<Problem>) {
        if !declared {
            return (Links::star(), Vec::new());
        }
        let (mut rows, mut problems) = (Vec::new(), Vec::new());
        for id in tables.ids(Link::NAME) {
            match tables.get::<Link>(id) {
                Ok(r) => match r.check(seats) {
                    Ok(()) if rows.len() >= MAX_ROWS => problems.push(Problem::refused(format!("link {id}"), format!("a set holds up to {MAX_ROWS} links"))),
                    Ok(()) => rows.push(r),
                    Err(e) => problems.push(Problem::refused(format!("link {id}"), e)),
                },
                Err(p) => problems.push(p),
            }
        }
        let mut links = Links { declared: true, rows };
        for i in links.uncapped_cycles().into_iter().rev() {
            let r = links.rows.remove(i);
            problems.push(Problem::refused(format!("link {}", r.id), format!("link `{}` is on a cycle that no `hops` caps ({} to {}): a message could go round for ever; give one link of the cycle a `hops`", r.id, r.from, r.to)));
        }
        if links.rows.is_empty() {
            problems.push(Problem::warn("crew.links", "`declared` with no link that holds: every message is refused"));
        }
        (links, problems)
    }

    /// From a list of rows: what a test starts from. The cycle rule is not applied (use [`Links::load`]).
    pub fn of(rows: Vec<Link>) -> Links {
        Links { declared: true, rows }
    }

    pub fn declared(&self) -> bool {
        self.declared
    }

    pub fn rows(&self) -> &[Link] {
        &self.rows
    }

    /// The indexes of the rows that are on a cycle of rows no `hops` caps: a row follows another when its `from` can name the party the other's `to` does.
    pub fn uncapped_cycles(&self) -> Vec<usize> {
        let free: Vec<usize> = (0..self.rows.len()).filter(|&i| self.rows[i].cap().is_none()).collect();
        let n = free.len();
        let mut reach = vec![vec![false; n]; n];
        for (a, &i) in free.iter().enumerate() {
            for (b, &j) in free.iter().enumerate() {
                reach[a][b] = overlap(&self.rows[i].to, &self.rows[j].from);
            }
        }
        for k in 0..n {
            let via = reach[k].clone();
            for row in reach.iter_mut().filter(|row| row[k]) {
                for (cell, v) in row.iter_mut().zip(&via) {
                    *cell |= *v;
                }
            }
        }
        (0..n).filter(|&a| reach[a][a]).map(|a| free[a]).collect()
    }

    /// Whether `from` may pass `carries` to `to`: a row names both parties and the word, and the thread has passed it fewer times than the row allows (`used` is how
    /// many times it has: asked of a row only when the row is capped). Returns the row the message goes by.
    pub fn admit(&self, from: &Party, to: &Party, carries: &str, used: &dyn Fn(&str) -> usize) -> Result<&Link, Refused> {
        let mut stopped = None;
        for r in self.rows.iter().filter(|r| r.carries.iter().any(|c| c == carries) && from.is(&r.from) && to.is(&r.to)) {
            match r.cap() {
                Some(cap) if used(&r.id) >= cap as usize => stopped = Some(Refused::Hops { link: r.id.clone(), cap }),
                _ => return Ok(r),
            }
        }
        Err(stopped.unwrap_or_else(|| Refused::Undeclared(format!("no link carries a {carries} from {} to {}", from.name(), to.name()))))
    }

    /// The most times a thread may pass a message: the cap of the row that carries it with the most room; none when no cap (or no row: the message is not passed).
    pub fn cap_of(&self, from: &Party, to: &Party, carries: &str) -> Option<u32> {
        let rows: Vec<&Link> = self.rows.iter().filter(|r| r.carries.iter().any(|c| c == carries) && from.is(&r.from) && to.is(&r.to)).collect();
        if rows.iter().any(|r| r.cap().is_none()) { None } else { rows.iter().filter_map(|r| r.cap()).max() }
    }
}
