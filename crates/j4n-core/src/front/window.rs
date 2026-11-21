//! Windows of rows: the screen asks for `count` rows of a binding from the `from`-th, in the order of a column and
//! among those that hold a word (`Up::Window`), and is sent that and the number of rows there are in all. [`cut`] does it for any rows;
//! [`LogRows`] holds the Log as rows so that a million events are windowed, sorted and found without being read again.

use super::chat::clip;
use crate::log::{Ev, Log, Reader, actor_word};
use j4n_proto::{Cell, Json, Row, Tone};
use std::cmp::Ordering;
use std::collections::{HashMap, VecDeque};
use std::hash::{DefaultHasher, Hash, Hasher};

/// A binding of a field that shows a window is sent whole while it has no more rows than this; past it, a window.
pub const WHOLE: usize = 2_000;
/// The rows of the first window, when the screen has not asked for one.
pub const FIRST: u64 = 200;
/// The most rows a window holds, whatever was asked.
pub const MOST: u64 = 2_000;
/// The most characters of an event's data a row shows.
const DATA_MAX: usize = 200;
/// The newest events the index keeps whole, so that a window at the end of the Log is sent its events without reading the files.
const RECENT: usize = 4_096;
/// Events further apart than this in a window are read from the files in a part each, not in one read that passes over all between.
const GAP: u64 = 2_048;
/// The most parts of the files one window reads: the rows of a window sorted or found far apart past it are sent without their
/// event, and show the words of their cells.
const PARTS: usize = 16;
/// The columns of a row of the Log, by the names a sort is asked for.
pub const LOG_COLUMNS: [&str; 3] = ["seq", "kind", "task"];

/// What the screen asked for.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Ask {
    pub from: u64,
    pub count: u64,
    /// A column, with a `-` before it for the other way; empty: the order the rows come in.
    pub sort: String,
    /// A word the rows must hold, in whatever case; empty: all of them.
    pub find: String,
}

impl Ask {
    fn way(&self) -> (&str, bool) {
        match self.sort.strip_prefix('-') {
            Some(name) => (name, true),
            None => (self.sort.as_str(), false),
        }
    }

    fn needle(&self) -> String {
        self.find.trim().to_lowercase()
    }
}

/// Whether `hay` holds `needle` (in lower case) in whatever case its ASCII letters are in.
pub fn has(hay: &str, needle: &str) -> bool {
    let (h, n) = (hay.as_bytes(), needle.as_bytes());
    let Some((&first, rest)) = n.split_first() else { return true };
    if h.len() < n.len() {
        return false;
    }
    (0..=h.len() - n.len()).any(|i| h[i].to_ascii_lowercase() == first && h[i + 1..].iter().zip(rest).all(|(a, b)| a.to_ascii_lowercase() == *b))
}

fn number(s: &str) -> Option<f64> {
    s.trim().trim_start_matches('$').trim_end_matches('%').replace(',', "").parse().ok()
}

/// Two cells in order: as numbers when both are, else as words.
fn by_cell(a: &str, b: &str) -> Ordering {
    match (number(a), number(b)) {
        (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
        _ => a.to_lowercase().cmp(&b.to_lowercase()),
    }
}

/// The window `ask` makes of `rows`, whose columns are called `cols`, and how many rows match in all. The sort is stable.
pub fn cut(rows: &[Row], cols: &[String], ask: &Ask) -> (Vec<Row>, u64) {
    let needle = ask.needle();
    let mut at: Vec<usize> = (0..rows.len()).filter(|&i| needle.is_empty() || rows[i].cells.iter().any(|c| has(&c.text, &needle))).collect();
    let (name, down) = ask.way();
    if let Some(c) = cols.iter().position(|c| !name.is_empty() && c == name) {
        let text = |i: usize| rows[i].cells.get(c).map_or("", |c| c.text.as_str());
        at.sort_by(|&a, &b| if down { by_cell(text(b), text(a)) } else { by_cell(text(a), text(b)) });
    }
    let total = at.len() as u64;
    let from = ask.from.min(total) as usize;
    (at[from..].iter().take(ask.count.min(MOST) as usize).map(|&i| rows[i].clone()).collect(), total)
}

struct Entry {
    seq: u64,
    kind: u32,
    task: u32,
    /// What makes an event the same line again: its kind, task, thread, who, class, cause and whole data, not its time or number.
    sign: u64,
    text: Box<str>,
}

/// The words that are given a number, so that an event holds two numbers and not two words.
#[derive(Default)]
struct Names {
    list: Vec<String>,
    at: HashMap<String, u32>,
}

impl Names {
    fn id(&mut self, name: &str) -> u32 {
        if let Some(&id) = self.at.get(name) {
            return id;
        }
        let id = self.list.len() as u32;
        self.list.push(name.to_string());
        self.at.insert(name.to_string(), id);
        id
    }

    /// For each name, whether it holds `needle`.
    fn hits(&self, needle: &str) -> Vec<bool> {
        self.list.iter().map(|n| has(n, needle)).collect()
    }

    /// For each name, its place among the names in order.
    fn ranks(&self) -> Vec<u32> {
        let mut order: Vec<usize> = (0..self.list.len()).collect();
        order.sort_by(|&a, &b| self.list[a].cmp(&self.list[b]));
        let mut rank = vec![0; order.len()];
        for (r, i) in order.into_iter().enumerate() {
            rank[i] = r as u32;
        }
        rank
    }
}

/// What a find has found so far, and how far in the events it has looked.
struct Found {
    needle: String,
    upto: usize,
    at: Vec<u32>,
    /// The places in `at` where a run of the same line begins.
    starts: Vec<u32>,
}

/// The events in the order of a column, as far as the events go.
struct Sorted {
    name: String,
    upto: usize,
    at: Vec<u32>,
    starts: Vec<u32>,
}

/// The places in `at` (or, with none, in the events) where a run of the same line begins: one whose sign is not that of the one before it.
fn runs_of(entries: &[Entry], at: Option<&[u32]>) -> Vec<u32> {
    let sign = |p: usize| entries[at.map_or(p, |a| a[p] as usize)].sign;
    let len = at.map_or(entries.len(), <[u32]>::len);
    (0..len).filter(|&p| p == 0 || sign(p) != sign(p - 1)).map(|p| p as u32).collect()
}

/// The Log as rows, the way the screen shows it: a row for each event written (`seq`, its kind and what it carried, its task). It is
/// read from the files once, a part at a time ([`catch_up`](LogRows::catch_up)), and then kept as events are written ([`push`](LogRows::push)).
#[derive(Default)]
pub struct LogRows {
    entries: Vec<Entry>,
    kinds: Names,
    tasks: Names,
    reader: Option<Reader>,
    /// The index holds every event the files held when the reader ran out.
    caught: bool,
    /// The newest event that was heard of, whether or not the index holds it.
    heard: u64,
    found: Option<Found>,
    sorted: Option<Sorted>,
    /// The newest events whole, oldest first (at most [`RECENT`]).
    recent: VecDeque<Ev>,
    /// The events in the order written, the places where a run of the same line begins: a row of the Log is a run.
    runs: Vec<u32>,
    /// For the rows of the last window that fold more than one event: the number of its first event, how many, and the number of its last.
    folds: HashMap<u64, (u32, u64)>,
}

/// A find of the Log: `kind:<kind>` and `task:<task>` words at its start keep only the events of that kind or task, what is
/// after them is looked for as a word.
#[derive(Debug, PartialEq)]
struct Needle<'a> {
    kind: Option<&'a str>,
    task: Option<&'a str>,
    word: &'a str,
}

fn needle(n: &str) -> Needle<'_> {
    let mut out = Needle { kind: None, task: None, word: n };
    loop {
        let rest = out.word.trim_start();
        let (head, tail) = rest.split_once(' ').unwrap_or((rest, ""));
        match (head.strip_prefix("kind:"), head.strip_prefix("task:")) {
            (Some(k), _) if !k.is_empty() => out.kind = Some(k),
            (_, Some(t)) if !t.is_empty() => out.task = Some(t),
            _ => return Needle { word: rest, ..out },
        }
        out.word = tail;
    }
}

impl LogRows {
    pub fn new() -> LogRows {
        LogRows::default()
    }

    /// An index of `events` as they are, caught up with them.
    pub fn of(events: impl IntoIterator<Item = Ev>) -> LogRows {
        let mut rows = LogRows::new();
        events.into_iter().for_each(|e| rows.add(e));
        (rows.caught, rows.heard) = (true, rows.last());
        rows
    }

    fn last(&self) -> u64 {
        self.entries.last().map_or(0, |e| e.seq)
    }

    fn add(&mut self, ev: Ev) {
        let whole = if ev.data.is_null() { String::new() } else { ev.data.to_string() };
        let (kind, task) = (self.kinds.id(&ev.kind), self.tasks.id(ev.task.as_deref().unwrap_or("")));
        let mut h = DefaultHasher::new();
        (kind, task, ev.cause, ev.class as u8, &ev.thread, actor_word(&ev.by), &whole).hash(&mut h);
        let sign = h.finish();
        if self.entries.last().is_none_or(|e| e.sign != sign) {
            self.runs.push(self.entries.len() as u32);
        }
        self.entries.push(Entry { seq: ev.seq, kind, task, sign, text: clip(&whole, DATA_MAX).into() });
        if self.recent.len() == RECENT {
            self.recent.pop_front();
        }
        self.recent.push_back(ev);
    }

    /// The rows of a window are sent their events whole (`Row::data`): the newest from what the index keeps, the others read from
    /// `log`, a part of the files for each run of events close together. Only the rows of a window, so a million events cost nothing.
    pub fn fill(&self, log: Option<&Log>, rows: &mut [Row]) {
        let seq = |r: &Row| r.key.strip_prefix('e').and_then(|n| n.parse::<u64>().ok());
        let oldest = self.recent.front().map_or(u64::MAX, |e| e.seq);
        let kept = |s: u64| s.checked_sub(oldest).and_then(|i| self.recent.get(i as usize)).filter(|e| e.seq == s);
        let last = |r: &Row| seq(r).and_then(|s| self.folds.get(&s)).map(|f| f.1);
        let mut wanted: Vec<u64> = rows.iter().flat_map(|r| [seq(r), last(r)]).flatten().filter(|&s| kept(s).is_none()).collect();
        wanted.sort_unstable();
        wanted.dedup();
        let mut read: HashMap<u64, Ev> = HashMap::new();
        if let Some(log) = log {
            let (mut i, mut parts) = (0, 0);
            while i < wanted.len() && parts < PARTS {
                parts += 1;
                let mut j = i;
                while j + 1 < wanted.len() && wanted[j + 1] - wanted[j] <= GAP {
                    j += 1;
                }
                let (first, last) = (wanted[i], wanted[j]);
                let part = &wanted[i..=j];
                read.extend(log.read(first).take_while(|e| e.seq <= last).filter(|e| part.binary_search(&e.seq).is_ok()).map(|e| (e.seq, e)));
                i = j + 1;
            }
        }
        for r in rows {
            let Some(s) = seq(r) else { continue };
            let found = |s: u64| kept(s).or_else(|| read.get(&s));
            r.data = found(s).map(Ev::shown);
            // a row of a run shows its first event, with how many there are and where the last is
            if let (Some(Json::Object(m)), Some(&(n, end))) = (r.data.as_mut(), self.folds.get(&s)) {
                m.insert("repeats".into(), n.into());
                m.insert("last_seq".into(), end.into());
                if let Some(e) = found(end) {
                    m.insert("last_at_ms".into(), e.at_ms.into());
                }
            }
        }
    }

    /// How many rows the Log is in the order written, with no find: the runs of the same line.
    pub fn rows(&self) -> u64 {
        self.runs.len() as u64
    }

    /// How many events the index holds.
    pub fn len(&self) -> u64 {
        self.entries.len() as u64
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The newest event written is `head`: an index that holds less is not built.
    pub fn hear(&mut self, head: u64) {
        self.heard = self.heard.max(head);
    }

    /// Whether the index holds every event that is written.
    pub fn built(&self) -> bool {
        self.caught && self.heard <= self.last()
    }

    /// Read up to `budget` more events from the files; a call that finds nothing left marks the index built.
    pub fn catch_up(&mut self, log: &Log, budget: usize) {
        if self.built() {
            return;
        }
        let mut reader = self.reader.take().unwrap_or_else(|| log.read(self.last() + 1));
        self.caught = false;
        for _ in 0..budget {
            match reader.next() {
                Some(ev) => self.add(ev),
                None => {
                    self.caught = true;
                    return;
                }
            }
        }
        self.reader = Some(reader);
    }

    /// An event was written: held at once when the index is caught up with the one before it, else read from the files in time.
    pub fn push(&mut self, ev: &Ev) {
        self.heard = self.heard.max(ev.seq);
        if self.caught && ev.seq == self.last() + 1 {
            self.add(ev.clone());
        }
    }

    /// The row of the event `e`, which leads a run of `n` events (more than one: `×n` is its fourth cell).
    fn row(&self, e: &Entry, n: usize) -> Row {
        let (kind, task) = (&self.kinds.list[e.kind as usize], &self.tasks.list[e.task as usize]);
        let mut cells = vec![Cell::new(e.seq.to_string(), Tone::Dim), Cell::new(format!("{kind:<16} {}", e.text), Tone::Plain), Cell::new(task.clone(), Tone::Dim)];
        if n > 1 {
            cells.push(Cell::new(format!("×{n}"), Tone::Dim));
        }
        Row { key: format!("e{}", e.seq), cells, ..Row::default() }
    }

    /// Look for `needle` in the events written since the last find of it; `found` holds the places of the events that hold it.
    fn find(&mut self, needle: &str) {
        if self.found.as_ref().is_none_or(|f| f.needle != needle) {
            self.found = Some(Found { needle: needle.into(), upto: 0, at: Vec::new(), starts: Vec::new() });
        }
        let n = self::needle(needle);
        let word = n.word.trim();
        let digits = !word.is_empty() && word.bytes().all(|b| b.is_ascii_digit());
        let (kinds, tasks) = (self.kinds.hits(word), self.tasks.hits(word));
        let only = |names: &Names, want: Option<&str>| -> Option<Vec<bool>> { want.map(|w| names.list.iter().map(|n| n.eq_ignore_ascii_case(w)).collect()) };
        let (of_kind, of_task) = (only(&self.kinds, n.kind), only(&self.tasks, n.task));
        // cannot fail: the `if` above set `found` for this needle
        let f = self.found.as_mut().expect("a find was set");
        for (i, e) in self.entries.iter().enumerate().skip(f.upto) {
            let kept = of_kind.as_ref().is_none_or(|k| k[e.kind as usize]) && of_task.as_ref().is_none_or(|t| t[e.task as usize]);
            if kept && (word.is_empty() || kinds[e.kind as usize] || tasks[e.task as usize] || has(&e.text, word) || (digits && e.seq.to_string().contains(word))) {
                if f.at.last().is_none_or(|&l| self.entries[l as usize].sign != e.sign) {
                    f.starts.push(f.at.len() as u32);
                }
                f.at.push(i as u32);
            }
        }
        f.upto = self.entries.len();
    }

    /// Put every event, by its place, in the order of the column `name` (events with the same word in the order they were written).
    fn rank(&mut self, name: &str) {
        if self.sorted.as_ref().is_some_and(|s| s.name == name && s.upto == self.entries.len()) {
            return;
        }
        let kind = name == "kind";
        let rank = if kind { self.kinds.ranks() } else { self.tasks.ranks() };
        let word = |e: &Entry| rank[if kind { e.kind } else { e.task } as usize];
        let mut at: Vec<u32> = (0..self.entries.len() as u32).collect();
        at.sort_unstable_by_key(|&i| (word(&self.entries[i as usize]), i));
        let starts = runs_of(&self.entries, Some(&at));
        self.sorted = Some(Sorted { name: name.into(), upto: self.entries.len(), at, starts });
    }

    /// The window `ask` makes of the rows (a run of the same line is one), and how many rows match.
    pub fn window(&mut self, ask: &Ask) -> (Vec<Row>, u64) {
        let (needle, (name, down)) = (ask.needle(), ask.way());
        let by_word = matches!(name, "kind" | "task");
        if !needle.is_empty() {
            self.find(&needle);
        }
        if by_word {
            self.rank(name);
        }
        // the places of the events that count, in the order they are written in or the order of the column (none: all, as they are), and
        // the places among those where a run of the same line begins
        let keep: Vec<u32>;
        let kept_runs: Vec<u32>;
        let (places, starts): (Option<&[u32]>, &[u32]) = match (needle.is_empty(), by_word, &self.found, &self.sorted) {
            (true, false, ..) => (None, &self.runs),
            (false, false, Some(f), _) => (Some(&f.at), &f.starts),
            (true, true, _, Some(s)) => (Some(&s.at), &s.starts),
            (false, true, Some(f), Some(s)) => {
                let mut hit = vec![false; self.entries.len()];
                f.at.iter().for_each(|&i| hit[i as usize] = true);
                keep = s.at.iter().copied().filter(|&i| hit[i as usize]).collect();
                kept_runs = runs_of(&self.entries, Some(&keep));
                (Some(&keep), &kept_runs)
            }
            _ => (Some(&[][..]), &[][..]),
        };
        let events = places.map_or(self.entries.len(), <[u32]>::len);
        let total = starts.len() as u64;
        let (from, count) = (ask.from.min(total), ask.count.min(MOST));
        let entry = |p: usize| &self.entries[places.map_or(p, |at| at[p] as usize)];
        let mut folds = HashMap::new();
        let rows = (from..(from + count).min(total))
            .map(|r| {
                let r = (if down { total - 1 - r } else { r }) as usize;
                let (first, end) = (starts[r] as usize, starts.get(r + 1).map_or(events, |&e| e as usize));
                if end - first > 1 {
                    folds.insert(entry(first).seq, (u32::try_from(end - first).unwrap_or(u32::MAX), entry(end - 1).seq));
                }
                self.row(entry(first), end - first)
            })
            .collect();
        self.folds = folds;
        (rows, total)
    }
}
