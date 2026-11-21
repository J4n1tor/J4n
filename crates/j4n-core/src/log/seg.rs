//! Segments: `NNNN.jsonl`, one event per line, only ever appended to. This file reads them and
//! repairs the one thing a crash can do — leave a torn last line — and writes nothing else.

use super::ev::Ev;
use crate::table::Problem;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// A line longer than this is skipped by a reader, whatever it holds.
pub const LINE_MAX: usize = 8 << 20;
/// How much of a segment's end is looked at to find the last whole line.
const WINDOW: u64 = 64 << 20;
const PROBLEMS_MAX: usize = 20;

pub(crate) fn name(index: u64) -> String {
    format!("{index:04}.jsonl")
}

/// The segments in `dir`, in order: regular files named by four or more digits and `.jsonl`.
pub(crate) fn list(dir: &Path) -> io::Result<Vec<(u64, PathBuf)>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let file = entry.file_name().to_string_lossy().into_owned();
        let index = file.strip_suffix(".jsonl").filter(|s| s.len() >= 4 && s.bytes().all(|b| b.is_ascii_digit()));
        if let (Some(stem), true) = (index, entry.file_type()?.is_file())
            && let Ok(i) = stem.parse::<u64>()
        {
            out.push((i, entry.path()));
        }
    }
    out.sort();
    Ok(out)
}

/// The link a line makes with the next one: the first 8 bytes of its SHA-256, in hex. The writer puts it in the next
/// line as `prev`, so a line that is edited, added or cut out no longer fits the one after it.
pub fn link(line: &[u8]) -> String {
    crate::text::short_hash(line)
}

/// An event as the writer puts it on a line: the event, and the link to the line before.
#[derive(Serialize)]
pub(crate) struct Linked<'a> {
    #[serde(flatten)]
    pub ev: &'a Ev,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prev: Option<&'a str>,
}

#[derive(Deserialize)]
struct Chain {
    prev: Option<String>,
}

/// The least a line must say to be an event of ours. Repair judges by this alone, so a line a newer binary wrote (an actor
/// or a class this one does not know) is never cut as if it were torn.
#[derive(Debug, Clone, Copy, Deserialize)]
pub(crate) struct Head {
    pub seq: u64,
    #[serde(default)]
    pub at_ms: u64,
}

/// What the end of a segment looked like.
#[derive(Debug, Clone, Default)]
pub(crate) struct Tail {
    /// The last whole event.
    pub head: Option<Head>,
    /// The [`link`] of that line.
    pub last: Option<String>,
    /// That line has no newline after it (the write was cut exactly there): one must be added.
    pub unterminated: bool,
    /// Bytes after the last whole line, and the start of them.
    pub dropped: u64,
    pub dropped_text: String,
}

/// Find the last whole event of `path` and, when `fix`, cut off whatever follows it: a partial line, or lines that are not
/// events. Nothing before the last whole line is touched.
pub(crate) fn tail(path: &Path, fix: bool) -> io::Result<Tail> {
    let mut f = File::options().read(true).write(fix).open(path)?;
    let len = f.metadata()?.len();
    let base = len.saturating_sub(WINDOW);
    f.seek(SeekFrom::Start(base))?;
    let mut buf = Vec::with_capacity((len - base) as usize);
    f.read_to_end(&mut buf)?;
    let (mut end, mut head, mut unterminated, mut last) = (buf.len(), None, false, None);
    while end > 0 {
        let terminated = buf[end - 1] == b'\n';
        let body = if terminated { end - 1 } else { end };
        let start = buf[..body].iter().rposition(|&b| b == b'\n').map_or(0, |p| p + 1);
        if start == 0 && base > 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "no whole line in the last 64 MiB"));
        }
        match serde_json::from_slice::<Head>(&buf[start..body]) {
            Ok(h) if h.seq > 0 => {
                (head, unterminated, last) = (Some(h), !terminated, Some(link(&buf[start..body])));
                break;
            }
            _ => end = start,
        }
    }
    let kept = base + end as u64;
    let dropped = len - kept;
    let dropped_text = String::from_utf8_lossy(&buf[end..buf.len().min(end + 2000)]).into_owned();
    if fix && dropped > 0 {
        f.set_len(kept)?;
    }
    Ok(Tail { head, last, unterminated, dropped, dropped_text })
}

enum Line {
    Eof,
    Whole,
    Partial,
    TooLong,
}

fn read_line(r: &mut BufReader<File>, buf: &mut Vec<u8>) -> io::Result<Line> {
    buf.clear();
    let n = r.by_ref().take(LINE_MAX as u64 + 1).read_until(b'\n', buf)?;
    if n == 0 {
        return Ok(Line::Eof);
    }
    if buf.last() == Some(&b'\n') {
        buf.pop();
        return Ok(Line::Whole);
    }
    if buf.len() <= LINE_MAX {
        return Ok(Line::Partial);
    }
    loop {
        buf.clear();
        let n = r.by_ref().take(1 << 16).read_until(b'\n', buf)?;
        if n == 0 || buf.last() == Some(&b'\n') {
            return Ok(Line::TooLong);
        }
    }
}

/// The events of a log from a sequence number on, in order, across segments. A line that cannot be read is skipped with a
/// [`Problem`], never silently: [`problems`](Reader::problems) says what was skipped once the reader has run out.
pub struct Reader {
    files: Vec<(String, PathBuf)>,
    next: usize,
    cur: Option<Open>,
    from: u64,
    prev: u64,
    buf: Vec<u8>,
    problems: Vec<Problem>,
    more: u64,
    chain: Links,
}

/// What the reader knows of the chain so far.
#[derive(Default)]
struct Links {
    /// The link and the `seq` of the line before.
    last: Option<(String, u64)>,
    /// A line with a `prev` was seen: one without is then a break, not a line from before the chain.
    chained: bool,
    /// The last `seq` whose successor fit it, and the breaks found: that `seq` and the line.
    sure: u64,
    breaks: Vec<(u64, String)>,
    /// The `after` of every `log.broken` met: those breaks are told.
    told: Vec<u64>,
}

struct Open {
    name: String,
    lines: BufReader<File>,
    line_no: u64,
    last: bool,
}

impl Reader {
    /// The events with `seq >= from` under `dir`.
    pub fn open(dir: &Path, from: u64) -> Reader {
        let mut r = Reader {
            files: Vec::new(),
            next: 0,
            cur: None,
            from,
            prev: 0,
            buf: Vec::new(),
            problems: Vec::new(),
            more: 0,
            chain: Links::default(),
        };
        match list(dir) {
            Ok(segs) => {
                let named: Vec<(String, PathBuf)> = segs
                    .into_iter()
                    .map(|(_, p)| (p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), p))
                    .collect();
                r.next = start(&named, from);
                r.files = named;
            }
            Err(e) => r.note(Problem::warn(dir.display().to_string(), format!("cannot list the segments: {e}"))),
        }
        r
    }

    /// What was skipped or off so far; complete once the iterator has returned `None`.
    pub fn problems(&self) -> &[Problem] {
        &self.problems
    }

    pub fn into_problems(self) -> Vec<Problem> {
        self.problems
    }

    /// The breaks of the chain no `log.broken` has told yet, as `(the last seq still trusted, the line)`: complete once the reader has
    /// run out.
    pub fn broken(&self) -> Vec<(u64, String)> {
        self.chain.breaks.iter().filter(|(after, _)| !self.chain.told.contains(after)).cloned().collect()
    }

    /// Check the line in `buf` against the one before it: its `prev` is that line's link, or it has none and none had.
    fn follow(&mut self, place: &str, ev: Option<&Ev>) {
        let prev = serde_json::from_slice::<Chain>(&self.buf).ok().and_then(|c| c.prev);
        let c = &mut self.chain;
        let ok = match (&c.last, &prev) {
            (Some((was, seq)), Some(p)) if p == was => {
                c.sure = *seq;
                true
            }
            (Some(_), None) => !c.chained,
            (Some(_), _) => false,
            _ => true,
        };
        c.chained |= prev.is_some();
        if let Some(a) = ev.filter(|e| e.kind == "log.broken").and_then(|e| e.data.get("after")).and_then(|a| a.as_u64()) {
            c.told.push(a);
        }
        c.last = Some((link(&self.buf), ev.map_or(0, |e| e.seq)));
        if !ok {
            let sure = c.sure;
            c.breaks.push((sure, place.to_string()));
            self.note(Problem::warn(place, "the chain of the log is broken here: this line does not follow the one before it, so a line was edited, added or cut out by something other than J4n"));
        }
    }

    fn note(&mut self, p: Problem) {
        if self.problems.len() < PROBLEMS_MAX {
            self.problems.push(p);
        } else {
            self.more += 1;
        }
    }

    fn finish(&mut self) {
        if self.more > 0 {
            let n = std::mem::take(&mut self.more);
            self.problems.push(Problem::warn("log", format!("and {n} more lines that could not be read")));
        }
    }
}

/// The first segment worth opening for `from`: the last one that starts at or before it.
fn start(files: &[(String, PathBuf)], from: u64) -> usize {
    for i in (0..files.len()).rev() {
        let first = File::open(&files[i].1).ok().and_then(|f| {
            let mut line = Vec::new();
            let mut r = BufReader::new(f).take(LINE_MAX as u64);
            r.read_until(b'\n', &mut line).ok()?;
            serde_json::from_slice::<Head>(&line).ok()
        });
        if first.is_some_and(|h| h.seq <= from) {
            return i;
        }
    }
    0
}

impl Iterator for Reader {
    type Item = Ev;

    fn next(&mut self) -> Option<Ev> {
        loop {
            if self.cur.is_none() {
                let Some((name, path)) = self.files.get(self.next).cloned() else {
                    self.finish();
                    return None;
                };
                self.next += 1;
                match File::open(&path) {
                    Ok(f) => {
                        self.cur = Some(Open { name, lines: BufReader::new(f), line_no: 0, last: self.next == self.files.len() })
                    }
                    Err(e) => self.note(Problem::warn(name, format!("cannot open: {e}"))),
                }
                continue;
            }
            let mut open = self.cur.take()?;
            let line = read_line(&mut open.lines, &mut self.buf);
            let at = |open: &Open| format!("{}:{}", open.name, open.line_no + 1);
            match line {
                Err(e) => {
                    self.note(Problem::warn(at(&open), format!("cannot read: {e}")));
                    continue;
                }
                Ok(Line::Eof) => continue,
                Ok(Line::Partial) => {
                    self.chain.last = None;
                    if !open.last {
                        self.note(Problem::warn(at(&open), "a torn line in a closed segment — skipped"));
                    }
                    continue;
                }
                Ok(Line::TooLong) => {
                    self.chain.last = None;
                    self.note(Problem::warn(at(&open), format!("a line over {LINE_MAX} bytes — skipped")));
                    open.line_no += 1;
                }
                Ok(Line::Whole) => {
                    let place = at(&open);
                    open.line_no += 1;
                    let parsed = serde_json::from_slice::<Ev>(&self.buf);
                    self.follow(&place, parsed.as_ref().ok());
                    match parsed {
                        Err(e) => self.note(Problem::warn(place, format!("not an event this version reads ({e}) — skipped"))),
                        Ok(ev) if ev.seq <= self.prev => self.note(Problem::warn(
                            place,
                            format!("seq {} after {} — out of order, skipped", ev.seq, self.prev),
                        )),
                        Ok(ev) => {
                            if self.prev > 0 && ev.seq > self.prev + 1 {
                                self.note(Problem::warn(place, format!("seq {} follows {}: events are missing", ev.seq, self.prev)));
                            }
                            self.prev = ev.seq;
                            if ev.seq >= self.from {
                                self.cur = Some(open);
                                return Some(ev);
                            }
                        }
                    }
                }
            }
            self.cur = Some(open);
        }
    }
}
