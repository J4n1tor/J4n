//! Table: everything declared, loaded from layers, merged field by field, checked against schemas that are themselves
//! entries.
//!
//! # What a file holds
//!
//! A `.toml` file holds `api = 1` and arrays of tables named for kinds — `[[lamp]]` — and nothing else; a file may hold
//! any kinds and any number of entries, so one file per thing is just a file with one entry. Every entry has an `id`
//! ([`is_id`]); an id is unique within its kind and its layer. A key a kind does not declare is refused (the entry is
//! dropped and reported), a newer `api` than this J4n reads is refused with its own message, and a file that is not
//! TOML is refused whole. Nothing here fails: [`Tables::load`] gives what could be read and every [`Problem`].
//!
//! # Layers
//!
//! Low to high: the core's own kinds, packs (built in, then installed), the user's `~/.j4n/`, a project's `.j4n/`, the
//! session (in memory). The same `(kind, id)` in several layers is **one** entry whose fields come from the highest
//! layer that sets each (`Merged::from` says which), then the defaults; a field that is a `floor` is the lowest layer's,
//! and a higher layer's say is ignored with a warning. A table or list value is replaced whole, never merged. Required
//! fields are checked on the merged entry, so a higher layer can override one field of an entry it does not repeat.
//!
//! # Kinds are things
//!
//! A kind is declared by a `[[kind]]` entry (the schema of that kind is `packs/base/kinds/kind.toml`, embedded in this
//! binary). A kind is closed once declared: the lowest layer to declare a name owns it, a second declaration is refused.
//! Its `examples` are checked against its own schema when it is declared, so a kind whose examples do not validate does
//! not exist.
//!
//! # Trust
//!
//! The project layer is the one layer a stranger can write (a repository the operator cloned), so it is never trusted: an entry
//! of a kind with `untrusted = "ignore"` (the default) is ignored with a warning, and an entry of a kind with
//! `untrusted = "tighten"` counts only if none of its fields widens what J4n may do ([`Schema::widening`]).
//!
//! # Writing
//!
//! [`Tables::plan`] turns a [`Write`] into a [`Change`] (and says which fields widen); [`Writer`](crate::write::Writer)
//! applies a `Change` to the files; [`Tables::apply_session`] to the session layer.

use crate::change::{Actor, ActorKind, Cause, Change, Target, Where, digest};
use crate::schema::{Mode, Schema, is_id, is_name, sane};
use crate::value::Value;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::Path;
use toml::{Table, Value as Toml};

/// The longest file read, and the most files and entries.
pub const MAX_FILE: usize = 1 << 20;
pub const MAX_FILES: usize = 1_000;
pub const MAX_ENTRIES: usize = 20_000;
/// The most directories a layer is searched through, and entries read from one directory.
pub const MAX_DIRS: usize = 500;
pub const MAX_DIR_ENTRIES: usize = 10_000;
/// Directories of a user or project layer that hold state, not declarations: never read for `.toml`.
pub const STATE_DIRS: [&str; 17] = [
    "log",
    "folds",
    "blobs",
    "packs",
    "projects",
    "secrets",
    "tests",
    "bin",
    "skills",
    "personas",
    "checkpoints",
    "accounts",
    "cache",
    "tmp",
    "bench",
    "worktrees",
    "lab",
];

/// The core's own kinds, embedded: each step that adds a core kind adds its file here.
const CORE: [(&str, &str); 35] = [
    ("kinds/kind.toml", include_str!("../../../packs/base/kinds/kind.toml")),
    ("kinds/event.toml", include_str!("../../../packs/base/kinds/event.toml")),
    ("kinds/fold.toml", include_str!("../../../packs/base/kinds/fold.toml")),
    ("kinds/gate.toml", include_str!("../../../packs/base/kinds/gate.toml")),
    ("kinds/ask_point.toml", include_str!("../../../packs/base/kinds/ask_point.toml")),
    ("kinds/rank.toml", include_str!("../../../packs/base/kinds/rank.toml")),
    ("kinds/phrase.toml", include_str!("../../../packs/base/kinds/phrase.toml")),
    ("kinds/connector.toml", include_str!("../../../packs/base/kinds/connector.toml")),
    ("kinds/rule.toml", include_str!("../../../packs/base/kinds/rule.toml")),
    ("kinds/setting.toml", include_str!("../../../packs/base/kinds/setting.toml")),
    ("kinds/failure.toml", include_str!("../../../packs/base/kinds/failure.toml")),
    ("kinds/box.toml", include_str!("../../../packs/base/kinds/box.toml")),
    ("kinds/check.toml", include_str!("../../../packs/base/kinds/check.toml")),
    ("kinds/probe.toml", include_str!("../../../packs/base/kinds/probe.toml")),
    ("kinds/model.toml", include_str!("../../../packs/base/kinds/model.toml")),
    ("kinds/flow.toml", include_str!("../../../packs/base/kinds/flow.toml")),
    ("kinds/view.toml", include_str!("../../../packs/base/kinds/view.toml")),
    ("kinds/page.toml", include_str!("../../../packs/base/kinds/page.toml")),
    ("kinds/template.toml", include_str!("../../../packs/base/kinds/template.toml")),
    ("kinds/key.toml", include_str!("../../../packs/base/kinds/key.toml")),
    ("kinds/hint.toml", include_str!("../../../packs/base/kinds/hint.toml")),
    ("kinds/look.toml", include_str!("../../../packs/base/kinds/look.toml")),
    ("kinds/command.toml", include_str!("../../../packs/base/kinds/command.toml")),
    ("kinds/compact_mode.toml", include_str!("../../../packs/base/kinds/compact_mode.toml")),
    ("kinds/reflex.toml", include_str!("../../../packs/base/kinds/reflex.toml")),
    ("kinds/reflex_case.toml", include_str!("../../../packs/base/kinds/reflex_case.toml")),
    ("kinds/enable.toml", include_str!("../../../packs/base/kinds/enable.toml")),
    ("kinds/plan.toml", include_str!("../../../packs/base/kinds/plan.toml")),
    ("kinds/cost_source.toml", include_str!("../../../packs/base/kinds/cost_source.toml")),
    ("kinds/source.toml", include_str!("../../../packs/base/kinds/source.toml")),
    ("kinds/todo.toml", include_str!("../../../packs/base/kinds/todo.toml")),
    ("kinds/incident.toml", include_str!("../../../packs/base/kinds/incident.toml")),
    ("kinds/provider.toml", include_str!("../../../packs/base/kinds/provider.toml")),
    ("kinds/host.toml", include_str!("../../../packs/base/kinds/host.toml")),
    ("kinds/need.toml", include_str!("../../../packs/base/kinds/need.toml")),
];
/// The longest kind name.
const MAX_KIND: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// The thing is not loaded, or not written.
    Refused,
    /// The thing is loaded, and something is off.
    Warn,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub level: Level,
    pub at: String,
    pub msg: String,
}

impl Problem {
    pub fn refused(at: impl Into<String>, msg: impl Into<String>) -> Problem {
        Problem { level: Level::Refused, at: at.into(), msg: msg.into() }
    }

    pub fn warn(at: impl Into<String>, msg: impl Into<String>) -> Problem {
        Problem { level: Level::Warn, at: at.into(), msg: msg.into() }
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.at, self.msg)
    }
}

impl std::error::Error for Problem {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerKind {
    Pack,
    User,
    Project,
    Session,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    /// Relative to the layer, `/`-separated.
    pub path: String,
    pub text: String,
}

/// The files of one layer, as text. [`Layer::from_dir`] reads a directory; a test or an embedded pack builds one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layer {
    pub kind: LayerKind,
    /// A pack's id, or `home` or `project`: what [`Merged::from`] and a [`Problem`] call it.
    pub name: String,
    pub files: Vec<SourceFile>,
    /// The folder the files were read from, for a layer that was: what `{pack}` is in a pack's files. A built-in pack has none.
    pub dir: Option<std::path::PathBuf>,
}

/// The word of a pack's file that is its own folder: in a connector's `argv`, a check's `argv`, a box, any text the pack's files say.
pub const PACK_DIR: &str = "{pack}";

impl Layer {
    pub fn new(kind: LayerKind, name: &str, files: Vec<SourceFile>) -> Layer {
        Layer { kind, name: name.to_string(), files, dir: None }
    }

    /// `file` as the Table reads it: in a pack read from a folder, `{pack}` is that folder. In a layer that is not a pack the word has no
    /// meaning, and the file is refused whole, with the way out; a built-in pack has no folder and is read as it is. A comment line is not read.
    fn with_folder(&self, file: &SourceFile, problems: &mut Vec<Problem>) -> Option<SourceFile> {
        let said = file.text.lines().any(|l| !l.trim_start().starts_with('#') && l.contains(PACK_DIR));
        if !said {
            return Some(file.clone());
        }
        match (self.kind, &self.dir) {
            (LayerKind::Pack, Some(dir)) => {
                let path = dir.display().to_string().replace('\\', "\\\\").replace('"', "\\\"");
                Some(SourceFile { path: file.path.clone(), text: file.text.replace(PACK_DIR, &path) })
            }
            (LayerKind::Pack, None) => Some(file.clone()),
            _ => {
                problems.push(Problem::refused(
                    format!("{}:{}", self.name, file.path),
                    format!("`{PACK_DIR}` is the folder of the pack whose file says it, and this layer is not a pack: write the path of the script here, or put the script and the row in a pack; the file is left out"),
                ));
                None
            }
        }
    }

    /// Every `.toml` under `dir` that is not state (see [`STATE_DIRS`], hidden names, links and, in a pack, its
    /// `pack.toml`). A directory that is not there is an empty layer.
    pub fn from_dir(kind: LayerKind, name: &str, dir: &Path) -> (Layer, Vec<Problem>) {
        let mut layer = Layer::new(kind, name, Vec::new());
        layer.dir = Some(dir.to_path_buf());
        let mut problems = Vec::new();
        walk(dir, "", 0, &mut layer, &mut 0, &mut problems);
        layer.files.sort_by(|a, b| a.path.cmp(&b.path));
        (layer, problems)
    }

    /// The hash of the layer's files: the same files with the same text give the same hash.
    pub fn hash(&self) -> String {
        let mut files: Vec<&SourceFile> = self.files.iter().collect();
        files.sort_by(|a, b| a.path.cmp(&b.path));
        digest(&files.iter().map(|f| format!("{}\0{}\0", f.path, f.text)).collect::<String>())
    }
}

fn walk(dir: &Path, rel: &str, depth: usize, layer: &mut Layer, dirs: &mut usize, problems: &mut Vec<Problem>) {
    *dirs += 1;
    if *dirs > MAX_DIRS {
        problems.push(Problem::refused(
            format!("{}:{rel}", layer.name),
            format!("more than {MAX_DIRS} directories in a layer"),
        ));
        return;
    }
    let Ok(read) = fs::read_dir(dir) else { return };
    let mut names: Vec<_> = read.filter_map(Result::ok).take(MAX_DIR_ENTRIES + 1).collect();
    if names.len() > MAX_DIR_ENTRIES {
        names.truncate(MAX_DIR_ENTRIES);
        problems.push(Problem::refused(
            format!("{}:{rel}", layer.name),
            format!("more than {MAX_DIR_ENTRIES} entries — the rest are not read"),
        ));
    }
    names.sort_by_key(|e| e.file_name());
    for entry in names {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
        let at = format!("{}:{path}", layer.name);
        let Ok(kind) = entry.file_type() else { continue };
        if name.starts_with('.') {
            continue;
        }
        if kind.is_symlink() {
            problems.push(Problem::warn(at, "a link is not followed"));
        } else if kind.is_dir() {
            if !STATE_DIRS.contains(&name.as_str()) && depth < 6 {
                walk(&entry.path(), &path, depth + 1, layer, dirs, problems);
            }
        } else if name.ends_with(".toml") && !kind.is_file() {
            problems.push(Problem::warn(at, "not a regular file — not read"));
        } else if name.ends_with(".toml") && !(layer.kind == LayerKind::Pack && path == "pack.toml") {
            if layer.files.len() >= MAX_FILES {
                problems.push(Problem::refused(at, format!("more than {MAX_FILES} files in a layer")));
                return;
            }
            match fs::metadata(entry.path()).map(|m| m.len()) {
                Ok(n) if n > MAX_FILE as u64 => {
                    problems.push(Problem::refused(at, format!("larger than {MAX_FILE} bytes")))
                }
                _ => match fs::read_to_string(entry.path()) {
                    Ok(text) => layer.files.push(SourceFile { path, text }),
                    Err(e) => problems.push(Problem::refused(at, format!("cannot read: {e}"))),
                },
            }
        }
    }
}

/// Core kinds have a Rust struct that reads an entry; the struct and the kind's schema must agree (law 4).
pub trait Kind: serde::de::DeserializeOwned {
    /// The kind's name, as in `[[name]]`.
    const NAME: &'static str;
}

/// What a project that is not trusted may do with a kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Untrusted {
    #[default]
    Ignore,
    Tighten,
}

/// A `[[kind]]` entry as the core reads it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KindDecl {
    pub id: String,
    pub doc: String,
    #[serde(default)]
    pub untrusted: Untrusted,
    pub fields: Table,
    #[serde(default)]
    pub examples: Vec<Toml>,
}

impl Kind for KindDecl {
    const NAME: &'static str = "kind";
}

/// A declared kind.
#[derive(Debug, Clone)]
pub struct KindInfo {
    pub schema: Schema,
    pub untrusted: Untrusted,
    pub doc: String,
    /// The layer that declared it.
    pub layer: String,
}

/// An entry as the layers say it: the value, and which layer each field came from (`default` for a default).
#[derive(Debug, Clone, PartialEq)]
pub struct Merged {
    pub fields: Table,
    pub from: BTreeMap<String, String>,
}

/// One layer's say about one entry.
#[derive(Debug, Clone, PartialEq)]
struct Raw {
    file: String,
    fields: Table,
}

struct Entry {
    file: String,
    kind: String,
    id: String,
    fields: Table,
}

struct Merge {
    merged: Merged,
    floors: Vec<(usize, String)>,
    errors: Vec<String>,
    dangling: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// Set these fields of the entry in this layer (making the entry if it is not there).
    Set(Table),
    /// Take these fields out of the entry in this layer, so a lower layer shows through.
    Unset(Vec<String>),
    /// Take the entry out of this layer.
    Remove,
}

/// One edit asked for.
#[derive(Debug, Clone, PartialEq)]
pub struct Write {
    pub to: Where,
    /// The file a new entry goes into (default `<kind>.toml`); an entry that is there stays in its file.
    pub file: Option<String>,
    pub kind: String,
    pub id: String,
    pub op: Op,
    pub by: Actor,
    pub cause: Option<Cause>,
    pub at_ms: u64,
}

/// A change that can be applied, and what it widens (the Gate asks for two presses when it is not empty).
#[derive(Debug, Clone, PartialEq)]
pub struct Planned {
    pub change: Change,
    pub widens: Vec<String>,
    /// The hash of the file as loaded, for the writer's compare-and-swap; none for a new file.
    pub file_hash: Option<String>,
}

/// All of it, loaded.
#[derive(Debug, Clone)]
pub struct Tables {
    layers: Vec<(LayerKind, String)>,
    kinds: BTreeMap<String, KindInfo>,
    say: Vec<BTreeMap<(String, String), Raw>>,
    merged: BTreeMap<String, BTreeMap<String, Merged>>,
    hashes: BTreeMap<(usize, String), String>,
}

fn meta_schema() -> Result<Schema, String> {
    let (path, text) = CORE[0];
    let table: Table = text.parse().map_err(|e: toml::de::Error| format!("{path}: {}", e.message()))?;
    let fields = table
        .get("kind")
        .and_then(Toml::as_array)
        .and_then(|a| a.first())
        .and_then(|k| k.get("fields"))
        .and_then(Toml::as_table);
    Schema::parse(fields.ok_or_else(|| format!("{path}: the kind of kinds is not declared"))?)
        .map_err(|e| format!("{path}: {}", e.join("; ")))
}

/// Whether this file is one of the core's own, as the binary embeds it: `packs/base` holds them too, and `pack check` reads them once.
pub fn is_core(path: &str, text: &str) -> bool {
    CORE.iter().any(|(p, t)| *p == path && *t == text)
}

fn builtin() -> Layer {
    let files = CORE.iter().map(|(path, text)| SourceFile { path: path.to_string(), text: text.to_string() }).collect();
    Layer::new(LayerKind::Pack, "base", files)
}

/// A file name inside a layer: `/`-separated, no hidden, empty or `..` part, no control or path-special character, not in a
/// state directory, ending `.toml`, at most 6 deep.
pub fn is_file_name(f: &str) -> bool {
    let parts: Vec<&str> = f.split('/').collect();
    f.len() <= 200
        && f.ends_with(".toml")
        && parts.len() <= 6
        && parts.iter().all(|p| {
            !p.is_empty() && !p.starts_with('.') && p.chars().all(|c| !c.is_control() && !"\\:*?\"<>|".contains(c))
        })
        && !parts.iter().any(|p| STATE_DIRS.contains(p))
        && f != "pack.toml"
}

fn parse_file(layer: &Layer, file: &SourceFile, problems: &mut Vec<Problem>) -> Vec<Entry> {
    let at = format!("{}:{}", layer.name, file.path);
    let table: Table = match file.text.parse() {
        Ok(t) => t,
        Err(e) => {
            let line = e.span().and_then(|s| file.text.get(..s.start)).map_or(1, |head| head.matches('\n').count() + 1);
            problems.push(Problem::refused(at, format!("not TOML (line {line}): {}", e.message())));
            return Vec::new();
        }
    };
    match table.get("api") {
        Some(Toml::Integer(n)) if *n == i64::from(crate::API) => {}
        Some(Toml::Integer(n)) if *n > i64::from(crate::API) => {
            problems.push(Problem::refused(
                at,
                format!("api = {n}: this J4n reads api {} — update J4n, or lower the file", crate::API),
            ));
            return Vec::new();
        }
        // the operator's own files are read as this api when they say none (a file dropped for it was said only under the
        // alternate screen); a pack's are not, a pack is published and its api is part of what it promises
        None if matches!(layer.kind, LayerKind::User | LayerKind::Project) => problems.push(Problem::warn(
            at.clone(),
            format!("has no `api`, read as `api = {api}` — put `api = {api}` at the top", api = crate::API),
        )),
        other => {
            let has = other.map_or_else(|| "no `api`".to_string(), |v| format!("`api = {v}`"));
            problems.push(Problem::refused(at, format!("needs `api = {}` at the top (it has {has}), so it is left out", crate::API)));
            return Vec::new();
        }
    }
    let mut out = Vec::new();
    for (kind, value) in &table {
        if kind == "api" {
            continue;
        }
        let items = match value {
            Toml::Array(a) if a.iter().all(Toml::is_table) && is_name(kind) && kind.len() <= MAX_KIND => a,
            _ => {
                problems.push(Problem::refused(
                    at.clone(),
                    format!(
                        "`{kind}` is not `[[kind]]` entries (a file holds `api` and arrays of tables named for kinds)"
                    ),
                ));
                continue;
            }
        };
        for item in items {
            let Toml::Table(fields) = item else { continue };
            let mut fields = fields.clone();
            let here = |id: &str| format!("{at} [[{kind}]] {id}");
            let id = match fields.remove("id") {
                Some(Toml::String(id)) if is_id(&id) => id,
                Some(other) => {
                    problems.push(Problem::refused(
                        here("?"),
                        format!("`id` {other} is not an id (letters, digits, `_ - . : /`)"),
                    ));
                    continue;
                }
                None => {
                    problems.push(Problem::refused(here("?"), "needs an `id`"));
                    continue;
                }
            };
            if let Some(e) = fields.values().find_map(|v| sane(v, 0).err()) {
                problems.push(Problem::refused(here(&id), e));
                continue;
            }
            out.push(Entry { file: file.path.clone(), kind: kind.clone(), id, fields });
        }
    }
    out
}

impl Tables {
    /// Load the core's kinds and then `layers`, lowest first; a layer of kind [`LayerKind::Project`] only tightens. Never fails:
    /// what could not be read is in the problems.
    pub fn load(layers: Vec<Layer>) -> (Tables, Vec<Problem>) {
        let mut problems = Vec::new();
        let mut files = vec![builtin()];
        for layer in layers {
            if layer.kind == LayerKind::Session {
                problems.push(Problem::warn(layer.name, "the session layer is made by the Tables, not given"));
            } else {
                files.push(layer);
            }
        }
        let mut names: Vec<(LayerKind, String)> = files.iter().map(|l| (l.kind, l.name.clone())).collect();
        names.push((LayerKind::Session, "session".to_string()));
        let mut t = Tables {
            say: vec![BTreeMap::new(); names.len()],
            layers: names,
            kinds: BTreeMap::new(),
            merged: BTreeMap::new(),
            hashes: BTreeMap::new(),
        };
        let mut parsed: Vec<Vec<Entry>> = Vec::new();
        let mut count = 0;
        for (li, layer) in files.iter().enumerate() {
            let mut entries = Vec::new();
            for f in &layer.files {
                t.hashes.insert((li, f.path.clone()), digest(&f.text));
                if let Some(read) = layer.with_folder(f, &mut problems) {
                    entries.extend(parse_file(layer, &read, &mut problems));
                }
            }
            count += entries.len();
            if count > MAX_ENTRIES {
                problems.push(Problem::refused(layer.name.clone(), format!("more than {MAX_ENTRIES} entries in all")));
                entries.clear();
            }
            parsed.push(entries);
        }
        match meta_schema() {
            Ok(meta) => {
                t.declare(&parsed, &meta, &mut problems);
                t.admit(parsed, &mut problems);
                t.merge_all(&mut problems);
            }
            Err(e) => problems.push(Problem::refused("base", e)),
        }
        (t, problems)
    }

    fn keep(&self, li: usize) -> bool {
        self.layers[li].0 != LayerKind::Project
    }

    fn declare(&mut self, parsed: &[Vec<Entry>], meta: &Schema, problems: &mut Vec<Problem>) {
        for (li, entries) in parsed.iter().enumerate() {
            for e in entries.iter().filter(|e| e.kind == "kind") {
                let at = format!("{}:{} [[kind]] {}", self.layers[li].1, e.file, e.id);
                if !self.keep(li) {
                    problems.push(Problem::warn(at, "ignored: a project that is not trusted cannot declare a kind"));
                    continue;
                }
                let mut refuse = |msg: String| problems.push(Problem::refused(at.clone(), msg));
                let checked = meta.check(&e.fields, Mode::Whole, &|_, _| true);
                if !checked.errors.is_empty() {
                    refuse(checked.errors.join("; "));
                    continue;
                }
                if !is_name(&e.id) || e.id.len() > MAX_KIND || e.id == "api" {
                    refuse(format!(
                        "`{}` is not a kind name (a-z, digits, `_`, starting with a letter, at most {MAX_KIND}; not `api`)",
                        e.id
                    ));
                    continue;
                }
                if let Some(owner) = self.kinds.get(&e.id) {
                    refuse(format!("the kind is already declared by {} — a kind is closed once declared", owner.layer));
                    continue;
                }
                let spec = e.fields.get("fields").and_then(Toml::as_table).cloned().unwrap_or_default();
                let schema = match Schema::parse(&spec) {
                    Ok(s) => s,
                    Err(all) => {
                        refuse(all.join("; "));
                        continue;
                    }
                };
                let bad: Vec<String> = e
                    .fields
                    .get("examples")
                    .and_then(Toml::as_array)
                    .into_iter()
                    .flatten()
                    .enumerate()
                    .flat_map(|(i, ex)| match example(&schema, ex) {
                        Ok(()) => Vec::new(),
                        Err(all) => all.into_iter().map(|m| format!("example {}: {m}", i + 1)).collect(),
                    })
                    .collect();
                if !bad.is_empty() {
                    refuse(bad.join("; "));
                    continue;
                }
                let mut whole = e.fields.clone();
                meta.fill(&mut whole);
                let untrusted = if whole.get("untrusted").and_then(Toml::as_str) == Some("tighten") {
                    Untrusted::Tighten
                } else {
                    Untrusted::Ignore
                };
                let doc = whole.get("doc").and_then(Toml::as_str).unwrap_or_default().to_string();
                self.kinds.insert(e.id.clone(), KindInfo { schema, untrusted, doc, layer: self.layers[li].1.clone() });
                self.say[li]
                    .insert(("kind".to_string(), e.id.clone()), Raw { file: e.file.clone(), fields: e.fields.clone() });
            }
        }
        let declared: BTreeSet<&str> = self.kinds.keys().map(String::as_str).collect();
        for (name, info) in &self.kinds {
            let mut refs = BTreeSet::new();
            info.schema.ref_kinds(&mut refs);
            for r in refs.into_iter().filter(|r| !declared.contains(r.as_str())) {
                problems.push(Problem::warn(
                    format!("{} [[kind]] {name}", info.layer),
                    format!("refers to the kind `{r}`, which nothing declares"),
                ));
            }
        }
    }

    fn admit(&mut self, parsed: Vec<Vec<Entry>>, problems: &mut Vec<Problem>) {
        for (li, entries) in parsed.into_iter().enumerate() {
            for e in entries.into_iter().filter(|e| e.kind != "kind") {
                let at = format!("{}:{} [[{}]] {}", self.layers[li].1, e.file, e.kind, e.id);
                let Some(info) = self.kinds.get(&e.kind) else {
                    problems
                        .push(Problem::refused(at, format!("no kind `{}` is declared by any layer in use", e.kind)));
                    continue;
                };
                let checked = info.schema.check(&e.fields, Mode::Partial, &|_, _| true);
                if !checked.errors.is_empty() {
                    problems.push(Problem::refused(at, checked.errors.join("; ")));
                    continue;
                }
                if !self.keep(li)
                    && let Err(why) = self.admits(info, &e.fields)
                {
                    problems.push(Problem::warn(at, format!("ignored: {why}")));
                    continue;
                }
                let key = (e.kind.clone(), e.id.clone());
                if self.say[li].contains_key(&key) {
                    problems.push(Problem::refused(at, "declared twice in this layer"));
                    continue;
                }
                self.say[li].insert(key, Raw { file: e.file, fields: e.fields });
            }
        }
    }

    /// What a project that is not trusted may bring of this kind.
    fn admits(&self, info: &KindInfo, fields: &Table) -> Result<(), String> {
        match info.untrusted {
            Untrusted::Ignore => Err("this project is not trusted, and this kind is not one it may add to".to_string()),
            Untrusted::Tighten => match info.schema.widening(fields).as_slice() {
                [] => Ok(()),
                wide => {
                    Err(format!("this project is not trusted, and {} would widen what J4n may do", wide.join(", ")))
                }
            },
        }
    }

    fn exists(&self, kind: &str, id: &str) -> bool {
        let key = (kind.to_string(), id.to_string());
        self.say.iter().any(|layer| layer.contains_key(&key))
    }

    /// The layers' say about `(kind, id)` laid over each other; `over` puts another say in place of one layer's.
    fn merge(&self, kind: &str, id: &str, over: Option<(usize, Option<&Table>)>) -> Option<Merge> {
        let info = self.kinds.get(kind)?;
        let key = (kind.to_string(), id.to_string());
        let says: Vec<(usize, &Table)> = (0..self.layers.len())
            .filter_map(|li| match over {
                Some((o, replacement)) if o == li => replacement.map(|t| (li, t)),
                _ => self.say[li].get(&key).map(|r| (li, &r.fields)),
            })
            .collect();
        if says.is_empty() {
            return None;
        }
        let (mut fields, mut from, mut floors) = (Table::new(), BTreeMap::new(), Vec::new());
        for (li, say) in says {
            for (name, value) in say {
                if info.schema.fields.get(name).is_some_and(|f| f.floor) && fields.contains_key(name) {
                    floors.push((li, name.clone()));
                    continue;
                }
                fields.insert(name.clone(), value.clone());
                from.insert(name.clone(), self.layers[li].1.clone());
            }
        }
        let held: BTreeSet<String> = fields.keys().cloned().collect();
        info.schema.fill(&mut fields);
        for name in fields.keys().filter(|k| !held.contains(*k)) {
            from.insert(name.clone(), "default".to_string());
        }
        let known = |k: &str, i: &str| self.exists(k, i) || (k == kind && i == id);
        let checked = info.schema.check(&fields, Mode::Whole, &known);
        Some(Merge { merged: Merged { fields, from }, floors, errors: checked.errors, dangling: checked.dangling })
    }

    fn merge_all(&mut self, problems: &mut Vec<Problem>) {
        let keys: BTreeSet<(String, String)> = self.say.iter().flat_map(|l| l.keys().cloned()).collect();
        for (kind, id) in keys {
            let Some(m) = self.merge(&kind, &id, None) else { continue };
            let at = format!("[[{kind}]] {id}");
            for (li, field) in &m.floors {
                problems.push(Problem::warn(
                    &at,
                    format!(
                        "{} sets `{field}`, which is a floor an earlier layer already set — ignored",
                        self.layers[*li].1
                    ),
                ));
            }
            problems.extend(m.dangling.iter().map(|d| Problem::warn(&at, d)));
            if m.errors.is_empty() {
                self.merged.entry(kind).or_default().insert(id, m.merged);
            } else {
                let (file, none) = (self.said_in(&kind, &id, "").unwrap_or_default(), m.errors.iter().all(|e| e.ends_with(": required")));
                problems.push(Problem::refused(format!("{file} {at}"), m.errors.join("; ") + if none { ": no layer defines it" } else { "" }));
            }
        }
    }

    // ---- reading ----

    pub fn kinds(&self) -> impl Iterator<Item = (&str, &KindInfo)> {
        self.kinds.iter().map(|(k, v)| (k.as_str(), v))
    }

    pub fn schema(&self, kind: &str) -> Option<&Schema> {
        self.kinds.get(kind).map(|k| &k.schema)
    }

    pub fn ids(&self, kind: &str) -> Vec<&str> {
        self.merged.get(kind).into_iter().flatten().map(|(id, _)| id.as_str()).collect()
    }

    /// The entry as merged, and which layer each field came from.
    pub fn raw(&self, kind: &str, id: &str) -> Option<&Merged> {
        self.merged.get(kind)?.get(id)
    }

    /// The entry read into its Rust struct. A struct that cannot read an entry its schema passed is a bug in the pair,
    /// which the round-trip law (`tests/`) exists to find.
    pub fn get<T: Kind>(&self, id: &str) -> Result<T, Problem> {
        let m = self
            .raw(T::NAME, id)
            .ok_or_else(|| Problem::refused(format!("[[{}]] {id}", T::NAME), "there is no such thing"))?;
        decode(id, m)
    }

    /// Every entry of the kind, by id.
    pub fn list<T: Kind>(&self) -> Result<Vec<T>, Problem> {
        self.merged.get(T::NAME).into_iter().flatten().map(|(id, m)| decode(id, m)).collect()
    }

    /// The entries as rows for a query (`table:<kind>`): the fields in the language of expressions, and `id`.
    pub fn rows(&self, kind: &str) -> Vec<Value> {
        let Some(info) = self.kinds.get(kind) else { return Vec::new() };
        let rows = self.merged.get(kind).into_iter().flatten();
        rows.filter_map(|(id, m)| match info.schema.value(&m.fields) {
            Value::Map(mut row) => {
                row.insert("id".to_string(), Value::Str(id.clone()));
                Some(Value::Map(row))
            }
            _ => None,
        })
        .collect()
    }

    fn layer_index(&self, w: Where) -> Option<usize> {
        let want = match w {
            Where::User => LayerKind::User,
            Where::Project => LayerKind::Project,
            Where::Session => LayerKind::Session,
            // the home's own layer, below the project's folder of the operator's rules, which is also of kind `User`
            Where::Home => return self.layers.iter().position(|(k, n)| *k == LayerKind::User && n == "home"),
        };
        self.layers.iter().rposition(|(k, _)| *k == want)
    }

    /// Whether the tables were loaded with a layer of this kind.
    pub fn has_layer(&self, w: Where) -> bool {
        self.layer_index(w).is_some()
    }

    /// What one layer says of an entry, without the others.
    pub fn layer_entry(&self, w: Where, kind: &str, id: &str) -> Option<&Table> {
        let li = self.layer_index(w)?;
        self.say[li].get(&(kind.to_string(), id.to_string())).map(|r| &r.fields)
    }

    /// `layer:file` of the highest layer that says `field` of an entry (any field when `field` is empty).
    pub fn said_in(&self, kind: &str, id: &str, field: &str) -> Option<String> {
        let key = (kind.to_string(), id.to_string());
        let says = |li: &usize| self.say[*li].get(&key).is_some_and(|r| field.is_empty() || r.fields.contains_key(field));
        (0..self.layers.len()).rev().find(says).map(|li| format!("{}:{}", self.layers[li].1, self.say[li][&key].file))
    }

    /// What the layers below `w`'s layer say of one field of an entry: the last of them that says it.
    pub fn field_below(&self, w: Where, kind: &str, id: &str, field: &str) -> Option<&toml::Value> {
        let li = self.layer_index(w)?;
        let key = (kind.to_string(), id.to_string());
        (0..li).rev().find_map(|i| self.say[i].get(&key).and_then(|r| r.fields.get(field)))
    }

    // ---- writing ----

    /// Everything that makes a change refused except that the layer still holds `before` (the writer checks that
    /// under its lock): the kind and the layer, the file name, the schema, trust, floors, the required fields of the
    /// merged entry, and `who_may` for every field it touches.
    pub fn vet(&self, c: &Change) -> Result<(), Problem> {
        let t = &c.target;
        let at = format!("{} [[{}]] {}", t.layer.word(), t.kind, t.id);
        let no = |msg: String| Err(Problem::refused(at.clone(), msg));
        let Some(info) = self.kinds.get(&t.kind) else { return no(format!("no kind `{}` is declared", t.kind)) };
        let Some(li) = self.layer_index(t.layer) else { return no(format!("there is no {} layer", t.layer.word())) };
        if !is_id(&t.id) {
            return no(format!("{:?} is not an id", t.id));
        }
        if (t.layer == Where::Session) != t.file.is_empty() || (t.layer != Where::Session && !is_file_name(&t.file)) {
            return no(format!("{:?} is not a file name in this layer", t.file));
        }
        if t.kind == "kind" && c.after.is_some() && t.layer == Where::Session {
            return no("a kind is declared in a file, not in the session".to_string());
        }
        if let Some(after) = &c.after {
            if let Some(e) = after.values().find_map(|v| sane(v, 0).err()) {
                return no(e);
            }
            let checked = info.schema.check(after, Mode::Partial, &|_, _| true);
            if !checked.errors.is_empty() {
                return no(checked.errors.join("; "));
            }
            if self.layers[li].0 == LayerKind::Project
                && let Err(why) = self.admits(info, after)
            {
                return no(why);
            }
        }
        let empty = Table::new();
        let (before, after) = (c.before.as_ref().unwrap_or(&empty), c.after.as_ref().unwrap_or(&empty));
        let touched: Vec<&String> =
            before.keys().chain(after.keys()).filter(|k| before.get(*k) != after.get(*k)).collect();
        let actor = c.by.kind();
        let may_edit = matches!(actor, ActorKind::Operator | ActorKind::Core);
        if touched.is_empty() && !may_edit && c.before.is_some() != c.after.is_some() {
            return no(format!("{} may not make or remove an entry", actor.word()));
        }
        if let Some(name) = touched.iter().find(|n| !info.schema.may_write(n, actor)) {
            return no(format!(
                "`{name}` may not be written by {} (it is for {})",
                actor.word(),
                who(&info.schema, name)
            ));
        }
        let m = self.merge(&t.kind, &t.id, Some((li, c.after.as_ref())));
        if let Some(m) = m {
            if let Some((_, f)) = m.floors.iter().find(|(l, _)| *l == li) {
                return no(format!("`{f}` is a floor that a lower layer already set; it cannot be changed from here"));
            }
            if !m.errors.is_empty() {
                return no(m.errors.join("; "));
            }
        }
        Ok(())
    }

    /// Turn a [`Write`] into a [`Change`]: what the entry is now in that layer, what it will be, checked by [`vet`](Self::vet).
    pub fn plan(&self, w: &Write) -> Result<Planned, Problem> {
        let at = format!("{} [[{}]] {}", w.to.word(), w.kind, w.id);
        let li = self
            .layer_index(w.to)
            .ok_or_else(|| Problem::refused(at.clone(), format!("there is no {} layer", w.to.word())))?;
        let current = self.say[li].get(&(w.kind.clone(), w.id.clone()));
        let file = match (current, w.to) {
            (_, Where::Session) => String::new(),
            (Some(r), _) => r.file.clone(),
            (None, _) => w.file.clone().unwrap_or_else(|| format!("{}.toml", w.kind)),
        };
        let before = current.map(|r| r.fields.clone());
        let after = match &w.op {
            Op::Set(set) => {
                let mut t = before.clone().unwrap_or_default();
                t.extend(set.iter().map(|(k, v)| (k.clone(), v.clone())));
                Some(t)
            }
            Op::Unset(names) => {
                let mut t =
                    before.clone().ok_or_else(|| Problem::refused(at.clone(), "the layer has no such entry"))?;
                if let Some(n) = names.iter().find(|n| t.remove(*n).is_none()) {
                    return Err(Problem::refused(at, format!("`{n}` is not set in this layer")));
                }
                Some(t)
            }
            Op::Remove => {
                if before.is_none() {
                    return Err(Problem::refused(at, "the layer has no such entry"));
                }
                None
            }
        };
        if before == after {
            return Err(Problem::warn(at, "it is already so"));
        }
        let change = Change::sealed(
            Target { layer: w.to, file, kind: w.kind.clone(), id: w.id.clone() },
            before,
            after,
            w.by.clone(),
            w.cause.clone(),
            w.at_ms,
        );
        self.vet(&change)?;
        let schema = &self.kinds[&w.kind].schema;
        let was = change.before.as_ref().map(|b| schema.widening(b)).unwrap_or_default();
        let widens = change.after.as_ref().map(|a| schema.widening(a)).unwrap_or_default();
        let top = |n: &String| n.split('.').next().unwrap_or_default().to_string();
        let widens = widens
            .into_iter()
            .filter(|n| {
                !was.contains(n)
                    || change.before.as_ref().map(|b| b.get(&top(n))) != change.after.as_ref().map(|a| a.get(&top(n)))
            })
            .collect();
        let file_hash = self.hashes.get(&(li, change.target.file.clone())).cloned();
        Ok(Planned { change, widens, file_hash })
    }

    /// Apply a change to the session layer (the files are the [`Writer`](crate::write::Writer)'s). The layer must still
    /// hold `before`.
    pub fn apply_session(&mut self, c: &Change) -> Result<(), Problem> {
        let at = format!("session [[{}]] {}", c.target.kind, c.target.id);
        if c.target.layer != Where::Session {
            return Err(Problem::refused(at, "only a session change is applied to the Tables"));
        }
        self.vet(c)?;
        let li = self.layers.len() - 1;
        let key = (c.target.kind.clone(), c.target.id.clone());
        if self.say[li].get(&key).map(|r| &r.fields) != c.before.as_ref() {
            return Err(Problem::refused(at, "it changed since this was planned"));
        }
        match &c.after {
            Some(fields) => self.say[li].insert(key, Raw { file: String::new(), fields: fields.clone() }),
            None => self.say[li].remove(&key),
        };
        match self.merge(&c.target.kind, &c.target.id, None) {
            Some(m) if m.errors.is_empty() => {
                self.merged.entry(c.target.kind.clone()).or_default().insert(c.target.id.clone(), m.merged);
            }
            _ => {
                if let Some(k) = self.merged.get_mut(&c.target.kind) {
                    k.remove(&c.target.id);
                }
            }
        }
        Ok(())
    }
}

fn who(schema: &Schema, name: &str) -> String {
    let list = schema.fields.get(name).and_then(|f| f.who_may.clone());
    let kinds = list.unwrap_or_else(|| vec![ActorKind::Operator, ActorKind::Core]);
    kinds.iter().map(|k| k.word()).collect::<Vec<_>>().join(", ")
}

fn decode<T: Kind>(id: &str, m: &Merged) -> Result<T, Problem> {
    let mut fields = m.fields.clone();
    fields.insert("id".to_string(), Toml::String(id.to_string()));
    Toml::Table(fields)
        .try_into()
        .map_err(|e: toml::de::Error| Problem::refused(format!("[[{}]] {id}", T::NAME), e.message().to_string()))
}

/// An example is an entry with its id; it is checked as a whole entry.
fn example(schema: &Schema, v: &Toml) -> Result<(), Vec<String>> {
    let Some(t) = v.as_table() else { return Err(vec!["is not a table".to_string()]) };
    let mut t = t.clone();
    match t.remove("id") {
        Some(Toml::String(id)) if is_id(&id) => {}
        _ => return Err(vec!["needs an `id`".to_string()]),
    }
    if let Some(e) = t.values().find_map(|v| sane(v, 0).err()) {
        return Err(vec![e]);
    }
    let checked = schema.check(&t, Mode::Whole, &|_, _| true);
    if checked.errors.is_empty() { Ok(()) } else { Err(checked.errors) }
}
