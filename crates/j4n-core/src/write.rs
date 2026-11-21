//! The one writer of declaration files. A [`Change`] from
//! [`Tables::plan`] is applied like this, and in no other way:
//!
//! 1. the Tables **vet** it again (a hand-made change is checked like a planned one);
//! 2. the layer's lock (`.table.lock`, an exclusive `flock`) is taken, and under it the file is read again;
//! 3. **compare-and-swap**: the file must still hash as it did when it was loaded (when the caller says so), and the
//!    entry must still be what `before` says — so an undo, which only knows the entry, works after any other edit;
//! 4. the [`Journal`] records the change (a refusal stops the write);
//! 5. the text is edited **in place** with `toml_edit`: comments, blank lines and key order survive, and only the fields
//!    that differ change (a field that is replaced loses the comments inside it, and keeps one at the end of its line);
//! 6. the result is read back and compared — the entry must be `after` and every other entry unchanged — before
//! 7. it replaces the file atomically (a temporary file in the same directory, synced, renamed).
//!
//! A file that is not valid TOML, has no `api = 1`, or is reached through a link is never overwritten. An entry written as
//! an inline array (`lamp = [{ id = "a" }]`) is read like any other but never edited: the writer says so and leaves the
//! file as it is.

use crate::change::{Change, Where, digest};
use crate::table::{Problem, Tables};
use std::fs::{self, File, TryLockError};
use std::io::{ErrorKind, Write as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use toml::{Table, Value as Toml};
use toml_edit::{Array, ArrayOfTables, DocumentMut, InlineTable, Item, Table as EditTable, Value as EditValue};

/// How long a writer waits for the layer's lock.
const LOCK_WAIT: Duration = Duration::from_secs(5);

/// Where a change is recorded before the file is touched. A refusal stops the write; if the write then fails,
/// [`failed`](Journal::failed) says so.
pub trait Journal {
    fn record(&mut self, change: &Change) -> Result<(), String>;

    fn failed(&mut self, _change: &Change, _why: &str) {}
}

/// A journal that records nothing.
pub struct NoJournal;

impl Journal for NoJournal {
    fn record(&mut self, _change: &Change) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    pub path: PathBuf,
    /// The hash of the file as written.
    pub hash: String,
}

/// The directories of the two writable layers (`~/.j4n`, `<project>/.j4n`).
#[derive(Debug, Clone)]
pub struct Writer {
    user: Option<PathBuf>,
    project: Option<PathBuf>,
    home: Option<PathBuf>,
    wait: Duration,
}

/// The entry `(kind, id)` as the file holds it (without its `id`).
fn entry(doc: &Table, kind: &str, id: &str) -> Result<Option<Table>, String> {
    match doc.get(kind) {
        None => Ok(None),
        Some(Toml::Array(items)) => {
            Ok(items.iter().filter_map(Toml::as_table).find(|t| t.get("id").and_then(Toml::as_str) == Some(id)).map(
                |t| {
                    let mut t = t.clone();
                    t.remove("id");
                    t
                },
            ))
        }
        Some(_) => Err(format!("`{kind}` is not an array of tables")),
    }
}

fn to_edit(v: &Toml) -> EditValue {
    match v {
        Toml::String(s) => EditValue::from(s.as_str()),
        Toml::Integer(i) => EditValue::from(*i),
        Toml::Float(f) => EditValue::from(*f),
        Toml::Boolean(b) => EditValue::from(*b),
        Toml::Datetime(d) => EditValue::from(d.to_string()),
        Toml::Array(items) => {
            let mut array = Array::new();
            items.iter().for_each(|i| array.push(to_edit(i)));
            array.fmt();
            EditValue::Array(array)
        }
        Toml::Table(t) => {
            let mut inline = InlineTable::new();
            t.iter().for_each(|(k, v)| {
                inline.insert(k, to_edit(v));
            });
            inline.fmt();
            EditValue::InlineTable(inline)
        }
    }
}

/// The text with the change made in it, and nothing else different.
fn edit(text: &str, c: &Change) -> Result<String, String> {
    let mut doc: DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| e.message().to_string())?;
    let fresh = text.trim().is_empty();
    if fresh {
        doc["api"] = toml_edit::value(i64::from(crate::API));
    }
    let (kind, id) = (c.target.kind.as_str(), c.target.id.as_str());
    let index = match doc.get(kind) {
        None => None,
        Some(Item::ArrayOfTables(a)) => a.iter().position(|t| t.get("id").and_then(Item::as_str) == Some(id)),
        Some(_) => return Err(format!("`{kind}` is written as an inline array in this file, which J4n does not rewrite: the change was not written")),
    };
    let gone = || format!("`{kind}` has no {id} where it was found");
    match (index, &c.after) {
        (Some(i), None) => {
            let Some(Item::ArrayOfTables(a)) = doc.get_mut(kind) else { return Err(gone()) };
            a.remove(i);
            if a.is_empty() {
                doc.remove(kind);
            }
        }
        (None, Some(after)) => {
            let mut table = EditTable::new();
            table.insert("id", toml_edit::value(id));
            after.iter().for_each(|(k, v)| {
                table.insert(k, Item::Value(to_edit(v)));
            });
            match doc.get_mut(kind) {
                Some(Item::ArrayOfTables(a)) => a.push(table),
                _ => {
                    let mut a = ArrayOfTables::new();
                    a.push(table);
                    doc.insert(kind, Item::ArrayOfTables(a));
                }
            }
        }
        (Some(i), Some(after)) => {
            let Some(Item::ArrayOfTables(a)) = doc.get_mut(kind) else { return Err(gone()) };
            let table = a.get_mut(i).ok_or_else(gone)?;
            let before = c.before.clone().unwrap_or_default();
            for name in before.keys().filter(|k| !after.contains_key(*k)) {
                table.remove(name);
            }
            for (name, value) in after.iter().filter(|(k, v)| before.get(*k) != Some(*v)) {
                let mut new = to_edit(value);
                if let Some(old) = table.get(name).and_then(Item::as_value) {
                    new.decor_mut().set_suffix(old.decor().suffix().cloned().unwrap_or_default());
                }
                table.insert(name, Item::Value(new));
            }
        }
        (None, None) => {}
    }
    Ok(doc.to_string())
}

/// The text with the change made in it, checked: [`confirm`] has read it back. This is the whole of what
/// [`Writer::apply`] writes, so it is also what an approval card can show before anything is written.
pub fn edited(text: &str, c: &Change) -> Result<String, String> {
    let new = edit(text, c)?;
    confirm(text, &new, c)?;
    Ok(new)
}

/// Whether `new` is `old` with exactly `c` made in it: valid TOML, the entry is `c.after`, and every other entry (and
/// `api`, which a new file gets) is as it was.
pub fn confirm(old: &str, new: &str, c: &Change) -> Result<(), String> {
    let (kind, id) = (&c.target.kind, &c.target.id);
    let parse = |text: &str, what: &str| {
        text.parse::<Table>()
            .map_err(|e: toml::de::Error| format!("{what} is not valid TOML ({}) — nothing written", e.message()))
    };
    let (old_doc, new_doc) = (parse(old, "the file")?, parse(new, "the edit")?);
    let mut rest = without(old_doc, kind, id);
    if old.trim().is_empty() {
        rest.insert("api".to_string(), Toml::Integer(i64::from(crate::API)));
    }
    let landed = entry(&new_doc, kind, id).ok().flatten();
    if landed != c.after || without(new_doc, kind, id) != rest {
        return Err("the edit did not give what was asked — nothing written".to_string());
    }
    Ok(())
}

/// `doc` without the entry, and without the array it leaves empty.
fn without(mut doc: Table, kind: &str, id: &str) -> Table {
    if let Some(Toml::Array(items)) = doc.get_mut(kind) {
        items.retain(|t| t.get("id").and_then(Toml::as_str) != Some(id));
        if items.is_empty() {
            doc.remove(kind);
        }
    }
    doc
}

fn replace(path: &Path, text: &str) -> std::io::Result<()> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    let written = File::create(&tmp).and_then(|mut f| f.write_all(text.as_bytes()).and_then(|()| f.sync_all()));
    let done = written.and_then(|()| {
        if let Ok(meta) = fs::metadata(path) {
            let _ = fs::set_permissions(&tmp, meta.permissions());
        }
        fs::rename(&tmp, path)
    });
    if done.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    done
}

/// Take the layer's lock, waiting up to `wait` for another writer to finish.
fn take(lock: &File, wait: Duration) -> std::io::Result<()> {
    let start = Instant::now();
    loop {
        match lock.try_lock() {
            Ok(()) => return Ok(()),
            Err(TryLockError::WouldBlock) if start.elapsed() < wait => std::thread::sleep(Duration::from_millis(10)),
            Err(TryLockError::WouldBlock) => {
                return Err(std::io::Error::new(ErrorKind::TimedOut, "another writer holds it"));
            }
            Err(TryLockError::Error(e)) => return Err(e),
        }
    }
}

/// `file` under `dir` is reached through no link, and is not itself one: a project the operator cloned may hold a link
/// where a directory should be, and a write must not follow it out of the layer.
fn plain(dir: &Path, file: &str) -> Result<(), String> {
    let mut at = dir.to_path_buf();
    for part in file.split('/') {
        at.push(part);
        match fs::symlink_metadata(&at) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(format!("{} is a link — not written through", at.display()));
            }
            _ => {}
        }
    }
    Ok(())
}

impl Writer {
    pub fn new(user: Option<PathBuf>, project: Option<PathBuf>) -> Writer {
        Writer { user, project, home: None, wait: LOCK_WAIT }
    }

    /// The folder [`Where::Home`] is written to (`~/.j4n`); none: that layer is not written here.
    pub fn with_home(mut self, home: PathBuf) -> Writer {
        self.home = Some(home);
        self
    }

    /// Wait this long for another writer's lock instead of the default 5 seconds.
    pub fn waiting(mut self, wait: Duration) -> Writer {
        self.wait = wait;
        self
    }

    /// Apply `change` to its file. `expect_file` is the hash of the file as it was loaded ([`Planned::file_hash`](crate::table::Planned)); none skips that check.
    pub fn apply(
        &self,
        tables: &Tables,
        change: &Change,
        expect_file: Option<&str>,
        journal: &mut dyn Journal,
    ) -> Result<Applied, Problem> {
        let t = &change.target;
        let at = format!("{} [[{}]] {}", t.layer.word(), t.kind, t.id);
        let refuse = |msg: String| Problem::refused(at.clone(), msg);
        tables.vet(change)?;
        let dir = match t.layer {
            Where::User => self.user.as_deref(),
            Where::Project => self.project.as_deref(),
            Where::Session => None,
            Where::Home => self.home.as_deref(),
        };
        let dir = dir.ok_or_else(|| refuse(format!("the {} layer is not written to files here", t.layer.word())))?;
        let path = dir.join(&t.file);
        plain(dir, &t.file).map_err(&refuse)?;
        let io = |what: &str, e: std::io::Error| refuse(format!("{what}: {e}"));
        crate::state::ensure_root(dir, t.layer == Where::Project).map_err(|e| io("cannot make the directory", e))?;
        fs::create_dir_all(path.parent().unwrap_or(dir)).map_err(|e| io("cannot make the directory", e))?;
        let lock = File::options()
            .create(true)
            .write(true)
            .truncate(false)
            .open(dir.join(".table.lock"))
            .map_err(|e| io("cannot open the lock", e))?;
        take(&lock, self.wait).map_err(|e| io("cannot take the lock", e))?;

        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == ErrorKind::NotFound => String::new(),
            Err(e) => return Err(io("cannot read the file", e)),
        };
        if expect_file.is_some_and(|h| h != digest(&text)) {
            return Err(refuse(format!("{} changed since it was loaded", t.file)));
        }
        let old: Table = text.parse().map_err(|e: toml::de::Error| {
            refuse(format!("{} is not valid TOML ({}) — left as it is", t.file, e.message()))
        })?;
        if !text.trim().is_empty() && old.get("api").and_then(Toml::as_integer) != Some(i64::from(crate::API)) {
            return Err(refuse(format!("{} has no `api = {}` — left as it is", t.file, crate::API)));
        }
        let now = entry(&old, &t.kind, &t.id).map_err(&refuse)?;
        if now != change.before {
            return Err(refuse("it changed since this was planned".to_string()));
        }
        journal.record(change).map_err(|e| refuse(format!("not recorded: {e}")))?;

        let fail = |journal: &mut dyn Journal, why: String| {
            journal.failed(change, &why);
            Err(refuse(why))
        };
        let new_text = match edited(&text, change) {
            Ok(s) => s,
            Err(why) => return fail(journal, why),
        };
        match replace(&path, &new_text) {
            Ok(()) => Ok(Applied { path, hash: digest(&new_text) }),
            Err(e) => fail(journal, format!("cannot write {}: {e}", t.file)),
        }
    }
}
