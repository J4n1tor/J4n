//! Memory: what the operator and the work have said, found again by its words.
//!
//! A note is an event (`note`, by `:remember`); so are the operator's lines (`chat.say`) and what a task reported (`task.settled`).
//! The Log is the memory. What is here is one **derived index** over it, in SQLite with FTS5: a file that can be deleted at any
//! time and is made again from the Log, because it holds nothing the Log does not (law 3). It is brought up to date from the
//! last event it has seen, every time it is asked.
//!
//! - [`Index`]: `catch_up` and `search`, and the class of each item kept with it, so that what is `private` stays off a remote
//!   model's screen.
//! - [`remember`] and [`recall`]: the doers of `:remember` and `:recall`, and [`tools`], the `recall` function of a model.

mod card;
mod doers;
mod recall;
mod tools;

pub use card::{CARD_TOKENS, Fact, ProjectCard};
pub use doers::{DOERS, doers};
pub use recall::{RECALL_TOKENS, Recall};
pub use tools::MemoryTools;

use j4n_core::log::{Ev, Log};
use j4n_core::schema::DataClass;
use rusqlite::{Connection, params};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

/// Bumped when what is kept, or how, changes: an index of another format is made again.
const FORMAT: &str = "2";
/// How much of an item is kept: a report longer than this is cut (the Log has all of it).
const TEXT_MAX: usize = 8_000;
/// The most items one search gives back.
pub const HITS_MAX: usize = 50;

/// One thing found.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub seq: u64,
    pub at_ms: u64,
    /// `note`, `chat.say` or `task.settled`.
    pub kind: String,
    pub thread: String,
    pub task: String,
    pub class: DataClass,
    /// Where the words matched, with `«` and `»` round them.
    pub text: String,
}

pub struct Index {
    conn: Mutex<Connection>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn sql(e: rusqlite::Error) -> String {
    format!("the memory index: {e}")
}

/// What an event adds to the memory, if anything: its words.
fn words_of(ev: &Ev) -> Option<String> {
    let text = |k: &str| ev.data.get(k).and_then(|v| v.as_str()).map(str::trim).filter(|t| !t.is_empty());
    let words = match ev.kind.as_str() {
        "note" => {
            let tags = ev.data.get("tags").and_then(|t| t.as_array()).map(|a| a.iter().filter_map(|t| t.as_str()).collect::<Vec<_>>().join(" ")).unwrap_or_default();
            format!("{} {tags}", text("text")?).trim().to_string()
        }
        // a command is not something that was said about the work: `:recall x` would find itself
        "chat.say" => text("text").filter(|t| !t.starts_with(':') && !t.starts_with('/'))?.to_string(),
        "task.settled" if ev.data.get("ok").and_then(|v| v.as_bool()) == Some(true) => text("answer")?.to_string(),
        _ => return None,
    };
    // a secret is not kept anywhere new
    (ev.class != DataClass::Secret).then(|| words.chars().take(TEXT_MAX).collect())
}

/// The query as FTS5 reads it: each word of the operator's, quoted so that no word is an operator, all of them wanted, the last
/// as the start of a word. `None` when there is nothing to look for.
fn query_of(q: &str) -> Option<String> {
    let words: Vec<String> = q.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(|w| w.to_lowercase()).collect();
    let last = words.len().checked_sub(1)?;
    Some(words.iter().enumerate().map(|(n, w)| if n == last { format!("\"{w}\"*") } else { format!("\"{w}\"") }).collect::<Vec<_>>().join(" "))
}

impl Index {
    /// The index at `path`, made if it is not there or is of another format.
    pub fn open(path: &Path) -> Result<Index, String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("the memory index: cannot make {}: {e}", dir.display()))?;
        }
        let conn = Connection::open(path).map_err(sql)?;
        Index::init(&conn)?;
        Ok(Index { conn: Mutex::new(conn) })
    }

    /// An index that lives in memory only, for a caller that has no folder.
    pub fn in_memory() -> Result<Index, String> {
        let conn = Connection::open_in_memory().map_err(sql)?;
        Index::init(&conn)?;
        Ok(Index { conn: Mutex::new(conn) })
    }

    fn init(conn: &Connection) -> Result<(), String> {
        conn.execute_batch("CREATE TABLE IF NOT EXISTS meta(k TEXT PRIMARY KEY, v TEXT NOT NULL);").map_err(sql)?;
        let format: Option<String> = conn.query_row("SELECT v FROM meta WHERE k = 'format'", [], |r| r.get(0)).ok();
        if format.as_deref() != Some(FORMAT) {
            conn.execute_batch("DROP TABLE IF EXISTS mem; DROP TABLE IF EXISTS item; DROP TABLE IF EXISTS said; DELETE FROM meta;").map_err(sql)?;
        }
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS item(seq INTEGER PRIMARY KEY, at_ms INTEGER NOT NULL, kind TEXT NOT NULL, thread TEXT NOT NULL, task TEXT NOT NULL, class TEXT NOT NULL, text TEXT NOT NULL);
             CREATE VIRTUAL TABLE IF NOT EXISTS mem USING fts5(text, content='item', content_rowid='seq', tokenize='porter unicode61');",
        )
        .map_err(sql)?;
        conn.execute_batch(recall::TABLE).map_err(sql)?;
        conn.execute("INSERT OR REPLACE INTO meta(k, v) VALUES ('format', ?1)", [FORMAT]).map_err(sql)?;
        Ok(())
    }

    /// The last event of the Log that has been looked at.
    pub fn seen(&self) -> u64 {
        lock(&self.conn).query_row("SELECT v FROM meta WHERE k = 'seen'", [], |r| r.get::<_, String>(0)).ok().and_then(|v| v.parse().ok()).unwrap_or(0)
    }

    /// Look at the events the index has not seen, and keep the ones that are memory. A Log that is shorter than what was seen
    /// (it was replaced) is read from its start. How many items were added.
    pub fn catch_up(&self, log: &Log) -> Result<usize, String> {
        log.sync();
        let head = log.head();
        let mut conn = lock(&self.conn);
        let mut seen: u64 = conn.query_row("SELECT v FROM meta WHERE k = 'seen'", [], |r| r.get::<_, String>(0)).ok().and_then(|v| v.parse().ok()).unwrap_or(0);
        let tx = conn.transaction().map_err(sql)?;
        if seen > head {
            tx.execute_batch("DELETE FROM item; DELETE FROM said; INSERT INTO mem(mem) VALUES ('delete-all');").map_err(sql)?;
            seen = 0;
        }
        let mut added = 0;
        {
            let mut put = tx.prepare("INSERT OR REPLACE INTO item(seq, at_ms, kind, thread, task, class, text) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)").map_err(sql)?;
            let mut fts = tx.prepare("INSERT INTO mem(rowid, text) VALUES (?1, ?2)").map_err(sql)?;
            for ev in log.read(seen + 1).filter(|e| e.seq <= head) {
                seen = seen.max(ev.seq);
                Index::keep_said(&tx, &ev)?;
                let Some(words) = words_of(&ev) else { continue };
                put.execute(params![ev.seq, ev.at_ms, ev.kind, ev.thread.clone().unwrap_or_default(), ev.task.clone().unwrap_or_default(), ev.class.word(), words]).map_err(sql)?;
                fts.execute(params![ev.seq, words]).map_err(sql)?;
                added += 1;
            }
        }
        tx.execute("INSERT OR REPLACE INTO meta(k, v) VALUES ('seen', ?1)", [seen.to_string()]).map_err(sql)?;
        tx.commit().map_err(sql)?;
        Ok(added)
    }

    /// What the words find, best first, up to `limit` (at most [`HITS_MAX`]). `thread` narrows it to one thread; `up_to` is the most
    /// private class that may come back (`Project`: nothing `private`). No words, no hits.
    pub fn search(&self, words: &str, thread: Option<&str>, up_to: DataClass, limit: usize) -> Result<Vec<Hit>, String> {
        let Some(q) = query_of(words) else { return Ok(Vec::new()) };
        let allowed: Vec<&str> = [DataClass::Public, DataClass::Project, DataClass::Private, DataClass::Secret].into_iter().filter(|c| *c <= up_to).map(DataClass::word).collect();
        let conn = lock(&self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT item.seq, item.at_ms, item.kind, item.thread, item.task, item.class, snippet(mem, 0, '«', '»', '…', 24)
                 FROM mem JOIN item ON item.seq = mem.rowid
                 WHERE mem MATCH ?1 AND (?2 = '' OR item.thread = ?2) ORDER BY bm25(mem), item.seq DESC LIMIT ?3",
            )
            .map_err(sql)?;
        let rows = stmt
            .query_map(params![q, thread.unwrap_or_default(), limit.clamp(1, HITS_MAX)], |r| {
                Ok(Hit {
                    seq: r.get(0)?,
                    at_ms: r.get(1)?,
                    kind: r.get(2)?,
                    thread: r.get(3)?,
                    task: r.get(4)?,
                    class: [DataClass::Public, DataClass::Project, DataClass::Private, DataClass::Secret].into_iter().find(|c| c.word() == r.get::<_, String>(5).unwrap_or_default()).unwrap_or(DataClass::Private),
                    text: r.get(6)?,
                })
            })
            .map_err(sql)?;
        let mut hits = Vec::new();
        for row in rows {
            let hit = row.map_err(sql)?;
            if allowed.contains(&hit.class.word()) {
                hits.push(hit);
            }
        }
        Ok(hits)
    }

    /// How many items are kept.
    pub fn len(&self) -> usize {
        lock(&self.conn).query_row("SELECT count(*) FROM item", [], |r| r.get::<_, i64>(0)).map_or(0, |n| n as usize)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
