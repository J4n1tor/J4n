//! The Log: everything that happened, one line each (law 3: every record is an event and every
//! ledger is a fold). The only module that opens a file for append.
//!
//! - **Two logs, one code**: `~/.j4n/log/` and `<project>/.j4n/log/` are both a [`Log`]; the project one writes the
//!   `.gitignore` of its directory (`*`) when it makes it ([`crate::state`]). One J4n has a log open at a time
//!   (a lock in the directory); a second `open` is refused with that said. The files would also serve several writers
//!   taking the lock per append — the on-disk form does not stand in the way — but that is a later step's to build.
//! - **One writer thread** owns the files. [`Log::append`] hands it an event and waits: when it returns the line is in the
//!   file (written, not synced), every [`Sink`] has seen the event, and the returned [`Ev`] carries its `seq`. Events are
//!   numbered 1, 2, 3 … over the whole log and their times never go backwards.
//! - **Segments** `0001.jsonl`, `0002.jsonl` … rotate by size; nothing is rewritten in place. On open, a torn last line
//!   (a crash mid-write) is cut off and said so ([`Problem`] and a `log.repaired` event); everything before it is
//!   untouched, and a line a newer binary wrote is never mistaken for a torn one. Reading skips what it cannot
//!   read and says what it skipped.
//! - **Flash**: [`Log::flash`] delivers an event to subscribers only. Streaming deltas and heartbeats are flashes
//!   ([`FLASH_KINDS`]) and are refused by `append`; the settled message, reasoning, tool calls and results and shell exits
//!   ([`SETTLED_KINDS`]) are written and refused by `flash`. A pack's `[[event]]` may declare more of either.
//! - **Redaction** on every append and every flash, before anything is serialized ([`crate::redact`]); a line over
//!   `max_line` is stored as a blob and the event keeps its id.
//! - **Durability**: a line is written with one `write` on an `O_APPEND` file, so a killed process loses nothing it
//!   acknowledged; a power cut can lose the unsynced tail, which is why the log is synced before every checkpoint, so a fold
//!   snapshot is never ahead of the disk.
//!
//! A [`Sink`] runs on the writer thread and must not call back into the Log (it is refused, not waited for).

mod blob;
mod ev;
mod seg;

pub use blob::{BLOB_MAX, Blobs, is_blob_id};
pub use ev::{Ev, FLASH_KINDS, KIND_MAX, New, SETTLED_KINDS, actor_word, is_kind};
pub use seg::{LINE_MAX, Reader, link};

/// Where the Log under `dir` stands, read without opening it (another J4n may have it open): the last segment's path and length, and the
/// `seq` of its last whole line (0: none). Only the last few KiB are read; nothing is written.
pub fn peek(dir: &std::path::Path) -> Option<(std::path::PathBuf, u64, u64)> {
    use std::io::{Read, Seek, SeekFrom};
    let (_, path) = seg::list(dir).ok()?.pop()?;
    let mut f = std::fs::File::open(&path).ok()?;
    let len = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(64 * 1024))).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    let seq = buf.split(|b| *b == b'\n').rev().find_map(|line| serde_json::from_slice::<serde_json::Value>(line).ok()?.get("seq")?.as_u64()).unwrap_or(0);
    Some((path, len, seq))
}

use crate::change::Actor;
use crate::clock::{Clock, SystemClock};
use crate::eval::glob_match;
use crate::redact::Redactor;
use crate::schema::{DataClass, Mode, Schema};
use crate::state::{ensure_root, private_dir};
use crate::table::{Kind, Problem, Tables};
use serde::Deserialize;
use serde_json::json;
use std::cell::Cell;
use std::collections::HashMap;
use std::fs::{File, TryLockError};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use toml::Table;

thread_local! {
    static ON_WRITER: Cell<bool> = const { Cell::new(false) };
}

/// Which of the two logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// `~/.j4n`: machine-wide.
    Home,
    /// `<project>/.j4n`.
    #[default]
    Project,
}

pub struct Opts {
    /// A segment is closed once it would pass this many bytes.
    pub segment_bytes: u64,
    /// A line longer than this is stored as a blob.
    pub max_line: usize,
    /// Sync the log and tell the sinks (fold snapshots) every this many events.
    pub checkpoint_every: u64,
    pub clock: Arc<dyn Clock>,
}

impl Default for Opts {
    fn default() -> Opts {
        Opts { segment_bytes: 8 << 20, max_line: 1 << 20, checkpoint_every: 1000, clock: Arc::new(SystemClock) }
    }
}

/// Whoever wants every event in order — the fold host. It runs on the writer thread, after the line is written, so it
/// must be quick, must not call the Log (it is refused), and must not keep a [`Log`] handle (the writer would then never
/// be told to stop).
pub trait Sink: Send {
    fn event(&mut self, ev: &Ev);

    /// The log is synced through `seq`: a snapshot taken now is not ahead of the disk.
    fn checkpoint(&mut self, _seq: u64) {}
}

/// A pack's `[[event]]`: what an event kind carries, and whether it is only ever flashed.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventDecl {
    pub id: String,
    pub doc: String,
    #[serde(default)]
    pub fields: Table,
    #[serde(default)]
    pub class: DataClass,
    #[serde(default)]
    pub ephemeral: bool,
}

impl Kind for EventDecl {
    const NAME: &'static str = "event";
}

struct Declared {
    ephemeral: bool,
    class: DataClass,
    schema: Option<Schema>,
}

struct Slot {
    glob: String,
    tx: SyncSender<Arc<Ev>>,
    lagged: Arc<AtomicU64>,
}

type Subs = Arc<Mutex<Vec<Slot>>>;
type Notes = Arc<Mutex<Vec<Problem>>>;

/// What a subscriber holds. A slow one loses events rather than slowing the log: [`lagged`](Sub::lagged) counts them, and
/// the log has them to read.
pub struct Sub {
    rx: Receiver<Arc<Ev>>,
    lagged: Arc<AtomicU64>,
}

impl Sub {
    pub fn try_recv(&self) -> Option<Arc<Ev>> {
        self.rx.try_recv().ok()
    }

    pub fn recv_timeout(&self, wait: Duration) -> Option<Arc<Ev>> {
        self.rx.recv_timeout(wait).ok()
    }

    /// How many events were not delivered because the buffer was full.
    pub fn lagged(&self) -> u64 {
        self.lagged.load(Ordering::SeqCst)
    }

    pub fn closed(&self) -> bool {
        matches!(self.rx.try_recv(), Err(TryRecvError::Disconnected))
    }
}

enum Cmd {
    Append(New, Sender<Result<Ev, Problem>>),
    Attach(Box<dyn Sink>, u64, Sender<Result<Vec<Problem>, Problem>>),
    Sync(Sender<()>),
    Stop,
}

struct Shared {
    dir: PathBuf,
    tx: Sender<Cmd>,
    head: Arc<AtomicU64>,
    subs: Subs,
    notes: Notes,
    red: Redactor,
    blobs: Blobs,
    clock: Arc<dyn Clock>,
    max_line: usize,
    declared: RwLock<HashMap<String, Declared>>,
    join: Mutex<Option<JoinHandle<()>>>,
    _lock: File,
}

impl Drop for Shared {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Stop);
        if let Some(handle) = self.join.lock().ok().and_then(|mut j| j.take())
            && !ON_WRITER.get()
        {
            let _ = handle.join();
        }
    }
}

/// A handle to one log. Clones share it; the writer stops when the last one is dropped.
#[derive(Clone)]
pub struct Log(Arc<Shared>);

struct Writer {
    dir: PathBuf,
    file: File,
    index: u64,
    len: u64,
    head: u64,
    last_at: u64,
    segment_bytes: u64,
    checkpoint_every: u64,
    since: u64,
    clock: Arc<dyn Clock>,
    shared_head: Arc<AtomicU64>,
    subs: Subs,
    notes: Notes,
    sinks: Vec<Box<dyn Sink>>,
    broken: Option<String>,
    /// The link of the last line written, which the next line carries as `prev`.
    last: Option<String>,
}

fn segment_file(dir: &Path, index: u64) -> std::io::Result<File> {
    File::options().create(true).append(true).mode(0o600).open(dir.join(seg::name(index)))
}

fn deliver(subs: &Subs, ev: &Arc<Ev>) {
    let Ok(mut subs) = subs.lock() else { return };
    subs.retain(|s| {
        if !glob_match(&s.glob, &ev.kind) {
            return true;
        }
        match s.tx.try_send(ev.clone()) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                s.lagged.fetch_add(1, Ordering::SeqCst);
                true
            }
            Err(TrySendError::Disconnected(_)) => false,
        }
    });
}

impl Writer {
    fn note(&self, p: Problem) {
        if let Ok(mut n) = self.notes.lock()
            && n.len() < 100
        {
            n.push(p);
        }
    }

    fn run(mut self, rx: Receiver<Cmd>) {
        ON_WRITER.set(true);
        for cmd in rx {
            match cmd {
                Cmd::Append(n, reply) => {
                    let _ = reply.send(self.append(n));
                }
                Cmd::Attach(sink, after, reply) => {
                    let _ = reply.send(self.attach(sink, after));
                }
                Cmd::Sync(reply) => {
                    self.checkpoint();
                    let _ = reply.send(());
                }
                Cmd::Stop => break,
            }
        }
        self.checkpoint();
    }

    fn checkpoint(&mut self) {
        self.since = 0;
        if self.file.sync_data().is_err() {
            return;
        }
        let head = self.head;
        self.each_sink(|s| s.checkpoint(head));
    }

    /// Run `f` over every sink; one that panics is dropped, and the panic is noted.
    fn each_sink(&mut self, mut f: impl FnMut(&mut dyn Sink)) {
        let mut dropped = false;
        self.sinks.retain_mut(|s| {
            let ok = catch_unwind(AssertUnwindSafe(|| f(s.as_mut()))).is_ok();
            dropped |= !ok;
            ok
        });
        if dropped {
            self.note(Problem::warn("log", "a sink panicked and was dropped"));
        }
    }

    fn attach(&mut self, mut sink: Box<dyn Sink>, after: u64) -> Result<Vec<Problem>, Problem> {
        let mut reader = Reader::open(&self.dir, after + 1);
        for ev in reader.by_ref() {
            if catch_unwind(AssertUnwindSafe(|| sink.event(&ev))).is_err() {
                return Err(Problem::refused("log", "the sink panicked while catching up"));
            }
        }
        self.sinks.push(sink);
        // A break of the chain nobody has told is told in the Log, once, to every sink including this one
        if let Some((after, at)) = reader.broken().first() {
            let data = json!({ "after": after, "at": at, "lines": reader.broken().len() });
            let _ = self.append(New::of("log.broken", Actor::Core, data));
        }
        Ok(reader.into_problems())
    }

    fn rotate(&mut self) -> std::io::Result<()> {
        self.file.sync_data()?;
        self.file = segment_file(&self.dir, self.index + 1)?;
        self.index += 1;
        self.len = 0;
        Ok(())
    }

    fn append(&mut self, n: New) -> Result<Ev, Problem> {
        let refuse = |m: String| Problem::refused("log", m);
        if let Some(why) = &self.broken {
            return Err(refuse(format!("the log cannot be written ({why}) — close and open it again")));
        }
        let at_ms = self.clock.now_ms().max(self.last_at);
        let ev = Ev {
            seq: self.head + 1,
            at_ms,
            kind: n.kind,
            task: n.task,
            thread: n.thread,
            by: n.by,
            cause: n.cause,
            class: n.class.unwrap_or_default(),
            data: n.data,
        };
        let mut line = serde_json::to_vec(&seg::Linked { ev: &ev, prev: self.last.as_deref() }).map_err(|e| refuse(format!("not serializable: {e}")))?;
        line.push(b'\n');
        if self.len > 0 && self.len + line.len() as u64 > self.segment_bytes {
            self.rotate().map_err(|e| refuse(format!("cannot start the next segment: {e}")))?;
        }
        if let Err(e) = self.file.write_all(&line) {
            if self.file.set_len(self.len).is_err() {
                self.broken = Some(e.to_string());
            }
            return Err(refuse(format!("cannot write: {e}")));
        }
        self.len += line.len() as u64;
        self.last = Some(link(&line[..line.len() - 1]));
        self.head = ev.seq;
        self.last_at = at_ms;
        self.shared_head.store(self.head, Ordering::SeqCst);
        self.each_sink(|s| s.event(&ev));
        deliver(&self.subs, &Arc::new(ev.clone()));
        self.since += 1;
        if self.since >= self.checkpoint_every {
            self.checkpoint();
        }
        Ok(ev)
    }
}

/// How long a held lock is waited for before it is refused. A child forked while a J4n had the lock open holds
/// the same lock until its `exec` closes the descriptor, so a log let go and opened again at once finds it held for
/// a moment, longer on a busy machine. A second J4n that is running holds it until it ends.
const LOCK_WAIT: Duration = Duration::from_millis(500);

fn take(lock: &File) -> Result<(), TryLockError> {
    let end = Instant::now() + LOCK_WAIT;
    loop {
        match lock.try_lock() {
            Err(TryLockError::WouldBlock) if Instant::now() < end => std::thread::sleep(Duration::from_millis(2)),
            r => return r,
        }
    }
}

impl Log {
    /// Open the log under `root` (`~/.j4n` or `<project>/.j4n`), making what is missing. The problems are what repair
    /// found. A log another J4n has open is refused.
    pub fn open(root: &Path, scope: Scope, opts: Opts, red: Redactor) -> Result<(Log, Vec<Problem>), Problem> {
        let refuse = |m: String| Problem::refused(format!("log {}", root.display()), m);
        ensure_root(root, scope == Scope::Project).map_err(|e| refuse(format!("cannot make the directory: {e}")))?;
        let dir = root.join("log");
        private_dir(&dir).map_err(|e| refuse(format!("cannot make the directory: {e}")))?;
        let lock = File::options()
            .create(true)
            .write(true)
            .truncate(false)
            .mode(0o600)
            .open(dir.join(".lock"))
            .map_err(|e| refuse(format!("cannot open the lock: {e}")))?;
        match take(&lock) {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Err(refuse("another J4n has this log open".into())),
            Err(TryLockError::Error(e)) => return Err(refuse(format!("cannot take the lock: {e}"))),
        }
        let segs = seg::list(&dir).map_err(|e| refuse(format!("cannot list the segments: {e}")))?;
        let mut problems = Vec::new();
        let (mut index, mut head, mut repaired, mut chain) = (1, seg::Head { seq: 0, at_ms: 0 }, None, None::<String>);
        if let Some((last, path)) = segs.last() {
            index = *last;
            let tail = seg::tail(path, true).map_err(|e| refuse(format!("{}: {e}", seg::name(*last))))?;
            if tail.dropped > 0 {
                let msg = format!("{}: a torn last line ({} bytes) was cut off", seg::name(*last), tail.dropped);
                problems.push(Problem::warn(format!("log {}", root.display()), msg));
                repaired = Some(json!({ "segment": seg::name(*last), "bytes": tail.dropped, "text": tail.dropped_text }));
            }
            let (mut found, mut link) = (tail.head, tail.last);
            for (_, earlier) in segs.iter().rev().skip(1) {
                if found.is_some() {
                    break;
                }
                let t = seg::tail(earlier, false).ok();
                (found, link) = (t.as_ref().and_then(|t| t.head), t.and_then(|t| t.last));
            }
            (head, chain) = (found.unwrap_or(head), link);
            if tail.unterminated {
                File::options().append(true).open(path).and_then(|mut f| f.write_all(b"\n")).map_err(|e| {
                    refuse(format!("{}: cannot end the last line: {e}", seg::name(*last)))
                })?;
            }
        }
        let file = segment_file(&dir, index).map_err(|e| refuse(format!("cannot open the segment: {e}")))?;
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        let (shared_head, subs, notes) = (Arc::new(AtomicU64::new(head.seq)), Subs::default(), Notes::default());
        let (tx, rx) = mpsc::channel();
        let writer = Writer {
            dir: dir.clone(),
            file,
            index,
            len,
            head: head.seq,
            last_at: head.at_ms,
            segment_bytes: opts.segment_bytes.max(1),
            checkpoint_every: opts.checkpoint_every.max(1),
            since: 0,
            clock: opts.clock.clone(),
            shared_head: shared_head.clone(),
            subs: subs.clone(),
            notes: notes.clone(),
            sinks: Vec::new(),
            broken: None,
            last: chain,
        };
        let join = std::thread::Builder::new()
            .name("j4n-log".into())
            .spawn(move || writer.run(rx))
            .map_err(|e| refuse(format!("cannot start the writer: {e}")))?;
        let log = Log(Arc::new(Shared {
            dir,
            tx,
            head: shared_head,
            subs,
            notes,
            blobs: Blobs::new(&root.join("blobs"), red.clone()),
            red,
            clock: opts.clock,
            max_line: opts.max_line,
            declared: RwLock::default(),
            join: Mutex::new(Some(join)),
            _lock: lock,
        }));
        if let Some(data) = repaired {
            log.append(New::of("log.repaired", Actor::Core, data))?;
        }
        Ok((log, problems))
    }

    /// The directory of the segments.
    pub fn dir(&self) -> &Path {
        &self.0.dir
    }

    /// The sequence number of the last event written.
    pub fn head(&self) -> u64 {
        self.0.head.load(Ordering::SeqCst)
    }

    pub fn redactor(&self) -> &Redactor {
        &self.0.red
    }

    pub fn blobs(&self) -> &Blobs {
        &self.0.blobs
    }

    pub fn clock(&self) -> Arc<dyn Clock> {
        self.0.clock.clone()
    }

    /// Things the writer noticed on its own (a sink that panicked).
    pub fn notes(&self) -> Vec<Problem> {
        self.0.notes.lock().map(|n| n.clone()).unwrap_or_default()
    }

    /// Learn the event kinds the tables declare. Replaces what was declared before.
    pub fn declare(&self, tables: &Tables) -> Vec<Problem> {
        let mut problems = Vec::new();
        let mut map = HashMap::new();
        for id in tables.ids(EventDecl::NAME) {
            let decl = match tables.get::<EventDecl>(id) {
                Ok(d) => d,
                Err(p) => {
                    problems.push(p);
                    continue;
                }
            };
            let schema = if decl.fields.is_empty() {
                None
            } else {
                match Schema::parse(&decl.fields) {
                    Ok(s) => Some(s),
                    Err(e) => {
                        problems.push(Problem::refused(format!("event {id}"), e.join("; ")));
                        continue;
                    }
                }
            };
            if !is_kind(&decl.id) {
                problems.push(Problem::refused(format!("event {id}"), "not an event kind (lowercase words joined by dots)"));
                continue;
            }
            map.insert(decl.id, Declared { ephemeral: decl.ephemeral, class: decl.class, schema });
        }
        if let Ok(mut d) = self.0.declared.write() {
            *d = map;
        }
        problems
    }

    /// Check, scrub and size a new event: what both `append` and `flash` start with.
    fn prepare(&self, mut n: New, flash: bool) -> Result<New, Problem> {
        let refuse = |m: String| Problem::refused(format!("event {}", n.kind), m);
        if ON_WRITER.get() {
            return Err(refuse("a sink runs on the writer thread and may not append — hand the event to another thread".into()));
        }
        if !is_kind(&n.kind) {
            return Err(refuse(format!("not an event kind (lowercase words joined by dots, at most {KIND_MAX})")));
        }
        let declared = self.0.declared.read().map_err(|_| refuse("declarations unreadable".into()))?;
        let decl = declared.get(&n.kind);
        let only_flash = FLASH_KINDS.contains(&n.kind.as_str()) || decl.is_some_and(|d| d.ephemeral);
        if flash && SETTLED_KINDS.contains(&n.kind.as_str()) {
            return Err(refuse("a settled message, reasoning, tool call or shell exit is written, never flashed".into()));
        }
        if flash && decl.is_some_and(|d| !d.ephemeral) {
            return Err(refuse("this kind is declared as written, not flashed".into()));
        }
        if !flash && only_flash {
            return Err(refuse("this kind is only ever flashed, never written".into()));
        }
        if let Some(schema) = decl.and_then(|d| d.schema.as_ref()) {
            let mut entry = Table::try_from(&n.data)
                .map_err(|e| refuse(format!("data must be an object without nulls ({e})")))?;
            schema.fill(&mut entry);
            let checked = schema.check(&entry, Mode::Whole, &|_, _| true);
            if !checked.errors.is_empty() {
                return Err(refuse(checked.errors.join("; ")));
            }
        }
        n.class = n.class.or(decl.map(|d| d.class));
        drop(declared);
        let red = &self.0.red;
        red.scrub_json(&mut n.data);
        for s in [&mut n.task, &mut n.thread].into_iter().flatten() {
            *s = red.scrub(s);
        }
        if let Actor::Rule(s) | Actor::Doer(s) | Actor::Model(s) | Actor::Pack(s) = &mut n.by {
            *s = red.scrub(s);
        }
        if !flash {
            let bytes = serde_json::to_vec(&n.data).map_err(|e| refuse(format!("not serializable: {e}")))?;
            if bytes.len() > self.0.max_line {
                let id = self.0.blobs.put(&bytes)?;
                n.data = json!({ "spilled": id, "bytes": bytes.len() });
            }
        }
        Ok(n)
    }

    /// Write an event. Returns once it is in the file and the sinks have it.
    pub fn append(&self, n: New) -> Result<Ev, Problem> {
        let n = self.prepare(n, false)?;
        let (reply, answer) = mpsc::channel();
        self.0.tx.send(Cmd::Append(n, reply)).map_err(|_| Problem::refused("log", "the log has stopped"))?;
        answer.recv().map_err(|_| Problem::refused("log", "the log has stopped"))?
    }

    /// Deliver an event to the subscribers and nobody else. Its `seq` is 0.
    pub fn flash(&self, n: New) -> Result<Ev, Problem> {
        let n = self.prepare(n, true)?;
        let ev = Ev {
            seq: 0,
            at_ms: self.0.clock.now_ms(),
            kind: n.kind,
            task: n.task,
            thread: n.thread,
            by: n.by,
            cause: n.cause,
            class: n.class.unwrap_or_default(),
            data: n.data,
        };
        deliver(&self.0.subs, &Arc::new(ev.clone()));
        Ok(ev)
    }

    /// How many subscribers are listening (one that was dropped is forgotten at the next event).
    pub fn subscribers(&self) -> usize {
        self.0.subs.lock().map(|s| s.len()).unwrap_or(0)
    }

    /// Events whose kind matches `glob` (`*` and `?`), written and flashed alike, up to `cap` waiting.
    pub fn subscribe(&self, glob: &str, cap: usize) -> Sub {
        let (tx, rx) = mpsc::sync_channel(cap.max(1));
        let lagged = Arc::new(AtomicU64::new(0));
        if let Ok(mut subs) = self.0.subs.lock() {
            subs.push(Slot { glob: glob.to_string(), tx, lagged: lagged.clone() });
        }
        Sub { rx, lagged }
    }

    /// Give `sink` every event after `after` that is already written, then every new one, in order, with none missed
    /// and none twice. The problems are what the catching up could not read.
    pub fn attach(&self, sink: Box<dyn Sink>, after: u64) -> Result<Vec<Problem>, Problem> {
        if ON_WRITER.get() {
            return Err(Problem::refused("log", "a sink may not attach another from the writer thread"));
        }
        let (reply, answer) = mpsc::channel();
        self.0.tx.send(Cmd::Attach(sink, after, reply)).map_err(|_| Problem::refused("log", "the log has stopped"))?;
        answer.recv().map_err(|_| Problem::refused("log", "the log has stopped"))?
    }

    /// The events from `from` on, as the files hold them now.
    pub fn read(&self, from: u64) -> Reader {
        Reader::open(&self.0.dir, from)
    }

    /// Sync the log to disk and let the sinks snapshot.
    pub fn sync(&self) {
        let (reply, answer) = mpsc::channel();
        if self.0.tx.send(Cmd::Sync(reply)).is_ok() {
            let _ = answer.recv();
        }
    }
}
