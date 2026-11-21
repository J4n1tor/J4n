//! Packs: the container of a harness. A pack is a folder — a `pack.toml` that says what it is, and `.toml` files
//! of the kinds it declares or fills — and a harness is the packs that are on. Built-in packs are compiled into the binary and have the
//! same shape as the ones the operator installs (`~/.j4n/packs/<id>/`).
//!
//! - `needs` are packs that must be on for this one to work; `extends` are packs this one builds on, which are loaded below it even
//!   when nobody turned them on. A pack whose `needs` are not on, or that is part of a loop, is left out and says why.
//! - The packs that are on come from the defaults the binary gives and the `enable` rows of the operator's own folders
//!   ([`EnableRow`]); a project's own `.j4n/` is never asked, so a repository cannot turn a pack on. An installed pack is loaded only
//!   while its files hash to the pin the operator accepted when turning it on.
//! - A pack says how settled it is (`stability`). `stable` is only through the operator's dated list ([`CONFIRMED`]); a pack that claims
//!   it without being on the list is shown `beta`.

pub mod check;

use crate::table::{Kind, Layer, LayerKind, Level, Problem, SourceFile, Tables};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The operator's dated confirmations: `(pack id, the day it was confirmed)`. Empty until the operator confirms a pack.
pub const CONFIRMED: [(&str, &str); 0] = [];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stability {
    Experimental,
    Beta,
    Stable,
}

impl Stability {
    pub fn word(self) -> &'static str {
        match self {
            Stability::Experimental => "experimental",
            Stability::Beta => "beta",
            Stability::Stable => "stable",
        }
    }

    /// What the palette, a page and the list of packs put beside a pack that is not settled.
    pub fn tag(self) -> &'static str {
        match self {
            Stability::Experimental => "experimental",
            Stability::Beta => "beta",
            Stability::Stable => "",
        }
    }
}

/// How settled `id` is, given what its manifest claims: `stable` only when the operator confirmed it, else `beta`.
pub fn effective(id: &str, claimed: Stability) -> Stability {
    match claimed {
        Stability::Stable if !CONFIRMED.iter().any(|(p, _)| *p == id) => Stability::Beta,
        other => other,
    }
}

/// A `pack.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub api: u32,
    pub id: String,
    pub version: String,
    pub summary: String,
    /// When to reach for the pack, in plain words for a model that chooses: what a line looks like that this pack serves.
    #[serde(default)]
    pub use_when: String,
    #[serde(default)]
    pub extends: Vec<String>,
    #[serde(default)]
    pub needs: Vec<String>,
    pub stability: Stability,
    /// What the pack can switch on for itself when the operator enables it (`[ready]`).
    #[serde(default)]
    pub ready: Option<Ready>,
}

/// A value a `[ready]` row sets: what a setting of a kind `bool`, `int` or `str` holds.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum ReadyValue {
    Bool(bool),
    Int(i64),
    Text(String),
}

impl ReadyValue {
    pub fn value(&self) -> toml::Value {
        match self {
            ReadyValue::Bool(b) => toml::Value::Boolean(*b),
            ReadyValue::Int(i) => toml::Value::Integer(*i),
            ReadyValue::Text(s) => toml::Value::String(s.clone()),
        }
    }
}

/// `[ready]` of a manifest: the settings the pack turns on when it is enabled with ready to go on, and whether that is on for a pack the operator enables.
/// A pack that is on without the operator's say (J4n basic) never fires it by itself.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ready {
    #[serde(default = "yes")]
    pub default: bool,
    pub set: BTreeMap<String, ReadyValue>,
}

impl Ready {
    /// The faults of the row against the settings there are: each setting exists and takes its value, and one edit can hold them (a step for each and one for the row).
    pub fn problems(&self, pack: &str, tables: &Tables) -> Vec<Problem> {
        let at = format!("pack {pack} [ready]");
        if self.set.is_empty() || self.set.len() >= crate::proposal::STEPS_MAX {
            return vec![Problem::refused(at, format!("`set` names one to {} settings", crate::proposal::STEPS_MAX - 1))];
        }
        self.set
            .iter()
            .filter_map(|(id, v)| match tables.get::<crate::setting::SettingRow>(id) {
                Err(_) => Some(Problem::refused(at.clone(), format!("`{id}` is not a setting"))),
                Ok(row) => row.check(&v.value()).err().map(|e| Problem::refused(at.clone(), format!("`{id}`: {e}"))),
            })
            .collect()
    }
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Manifest, String> {
        let m: Manifest = toml::from_str(text).map_err(|e| e.message().to_string())?;
        if m.api != crate::API {
            return Err(format!("`api = {}`: this J4n reads api {}", m.api, crate::API));
        }
        if !crate::schema::is_id(&m.id) || m.id.contains(['/', ':']) {
            return Err(format!("`{}` is not a pack id (letters, digits, `_ - .`)", m.id));
        }
        if m.summary.trim().is_empty() {
            return Err("`summary` says what the pack is for".into());
        }
        Ok(m)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// Compiled into the binary.
    Builtin,
    /// Installed under `~/.j4n/packs/`.
    Installed,
}

#[derive(Debug, Clone)]
pub struct Pack {
    pub manifest: Manifest,
    manifest_text: String,
    pub layer: Layer,
    pub origin: Origin,
    /// The digest of the folder's other files, scripts and all; empty for a built-in pack, or one that holds only `.toml` files.
    rest: String,
}

impl Pack {
    /// A built-in pack from the texts of its files.
    pub fn builtin(manifest: &str, files: &[(&str, &str)]) -> Result<Pack, String> {
        let parsed = Manifest::parse(manifest)?;
        let files = files.iter().map(|(path, text)| SourceFile { path: (*path).into(), text: (*text).into() }).collect();
        Ok(Pack { layer: Layer::new(LayerKind::Pack, &parsed.id, files), manifest: parsed, manifest_text: manifest.into(), origin: Origin::Builtin, rest: String::new() })
    }

    /// What an operator's `hash` pins: every file of the pack and the manifest, which says what the pack needs and extends. A pack
    /// of `.toml` files alone pins as it did before the others were counted.
    pub fn pin(&self) -> String {
        let mut layer = self.layer.clone();
        layer.files.push(SourceFile { path: "pack.toml".into(), text: self.manifest_text.clone() });
        if !self.rest.is_empty() {
            layer.files.push(SourceFile { path: ".files".into(), text: self.rest.clone() });
        }
        layer.hash()
    }

    /// What it fills, kind by kind: the number of `[[kind]]` entries each file holds.
    pub fn adds(&self) -> BTreeMap<String, usize> {
        let mut out = BTreeMap::new();
        for f in &self.layer.files {
            let Ok(t) = f.text.parse::<toml::Table>() else { continue };
            for (kind, v) in t.iter().filter(|(k, _)| *k != "api") {
                *out.entry(kind.clone()).or_default() += v.as_array().map_or(1, Vec::len);
            }
        }
        out
    }

    /// The words its `[[command]]` and `[[phrase]]` rows say, those that start with a colon.
    pub fn words(&self) -> Vec<String> {
        let mut out = Vec::new();
        for f in &self.layer.files {
            let Ok(t) = f.text.parse::<toml::Table>() else { continue };
            let rows = ["command", "phrase"].into_iter().filter_map(|k| t.get(k).and_then(toml::Value::as_array)).flatten();
            let said = rows.filter_map(|r| r.get("words").and_then(toml::Value::as_array)).flatten().filter_map(toml::Value::as_str);
            out.extend(said.filter(|w| w.starts_with(':')).map(String::from));
        }
        out
    }

    /// Its `[[connector]]` rows that read a key: the id, and the key's reference.
    pub fn keyed(&self) -> Vec<(String, String)> {
        let rows: Vec<toml::Value> = self.layer.files.iter().filter_map(|f| f.text.parse::<toml::Table>().ok()?.get("connector")?.as_array().cloned()).flatten().collect();
        rows.iter().filter_map(|r| Some((r.get("id")?.as_str()?.to_string(), r.get("key")?.as_str()?.to_string()))).collect()
    }

    pub fn tag(&self) -> &'static str {
        effective(&self.manifest.id, self.manifest.stability).tag()
    }
}

/// The kinds whose entries change what J4n may run, reach or do: a pack that adds one is turned on by a proposal, not by itself.
pub const WIDENING: [&str; 6] = ["connector", "rule", "gate", "check", "box", "model"];

/// A pack turned on or off by the operator.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnableRow {
    pub id: String,
    #[serde(default)]
    pub doc: String,
    #[serde(default = "yes")]
    pub on: bool,
    #[serde(default)]
    pub hash: Option<String>,
    /// Whether the pack's `[ready]` settings are written when it is enabled; left out: the manifest's own default.
    #[serde(default)]
    pub ready: Option<bool>,
    /// The settings its `[ready]` wrote and that `:pack off` or turning ready off takes back (unless changed since).
    #[serde(default)]
    pub wrote: Vec<String>,
}

fn yes() -> bool {
    true
}

impl Kind for EnableRow {
    const NAME: &'static str = "enable";
}

/// The `enable` rows of these folders (the operator's, not the project's), later ones over earlier.
pub fn enables(layers: Vec<Layer>) -> (Vec<EnableRow>, Vec<Problem>) {
    let (tables, mut found) = Tables::load(layers);
    let mut rows: Vec<EnableRow> = Vec::new();
    for id in tables.ids(EnableRow::NAME) {
        match tables.get::<EnableRow>(id) {
            Ok(r) => rows.push(r),
            Err(p) => found.push(p),
        }
    }
    // the layers hold more than `enable` rows, other kinds half-said (a row that only the packs complete): those are the Table's to judge
    found.retain(|p| p.at.contains("[[enable]]") || p.at.starts_with("enable"));
    (rows, found)
}

/// `(kind, id, entry)` of every entry the `.toml` files of a layer say: a `[[kind]]` array of tables each with an `id`.
fn entries(layer: &Layer) -> Vec<(String, String, toml::Table)> {
    let tables = layer.files.iter().filter_map(|f| f.text.parse::<toml::Table>().ok());
    tables.flat_map(|t| t.into_iter().filter(|(k, _)| k != "api").flat_map(|(k, v)| v.as_array().into_iter().flatten().filter_map(|e| Some((k.clone(), e.get("id")?.as_str()?.to_string(), e.as_table()?.clone()))).collect::<Vec<_>>()).collect::<Vec<_>>()).collect()
}

/// The folders say `packs.full`: the last row of the setting wins.
pub fn says_full(folders: &[Layer]) -> bool {
    let said: Vec<_> = folders.iter().flat_map(entries).collect();
    said.iter().rev().find(|(k, id, _)| k == "setting" && id == "packs.full").and_then(|(.., t)| t.get("value")?.as_bool()).unwrap_or(false)
}

/// What `packs.full` turns on: every setting of a pack that is on that names a `full` value, as `(id, value)`.
pub fn full_values(layers: &[Layer]) -> Vec<(String, toml::Value)> {
    let said: Vec<_> = layers.iter().filter(|l| l.kind == LayerKind::Pack).flat_map(entries).collect();
    said.into_iter().filter(|(k, ..)| k == "setting").filter_map(|(_, id, t)| Some((id, t.get("full")?.clone()))).collect()
}

/// The layer that says them: above the packs and below the operator's folders, so a row of the operator's wins and the Table can say which value came from
/// `packs.full` (the layer is named for it). None when no pack says one.
pub fn full_layer(layers: &[Layer]) -> Option<Layer> {
    let values = full_values(layers);
    let rows: Vec<toml::Value> = values.into_iter().map(|(id, v)| toml::Value::Table([("id".to_string(), toml::Value::String(id)), ("value".to_string(), v)].into_iter().collect())).collect();
    let text = toml::to_string(&toml::Table::from_iter([("api".to_string(), toml::Value::Integer(1)), ("setting".to_string(), toml::Value::Array(rows.clone()))])).ok()?;
    (!rows.is_empty()).then(|| Layer::new(LayerKind::Pack, FULL_LAYER, vec![SourceFile { path: "full.toml".into(), text }]))
}

/// The name of the layer `packs.full` says its values in.
pub const FULL_LAYER: &str = "packs.full";

/// A pack as the operator is shown it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Info {
    pub id: String,
    pub version: String,
    pub summary: String,
    pub use_when: String,
    pub tag: &'static str,
    pub origin: Origin,
    pub on: bool,
    /// Why it is not on, when the operator or the defaults wanted it.
    pub note: String,
    /// What it fills, in words (`2 connectors, 3 pages`).
    pub adds: String,
    pub widens: bool,
    /// The hash of the files of an installed pack as they are now: what turning it on pins. Empty for a built-in pack.
    pub pin: String,
    /// The words of its commands (`:inbox`): a line that says one while the pack is off is led to turning it on.
    pub words: Vec<String>,
    /// Its `[ready]`, if it has one.
    pub ready: Option<Ready>,
    /// Its connectors that read a key, with where they read it (`jev`, `env:TYPESAFE_API_KEY`): a key typed while the pack is off names it.
    pub keyed: Vec<(String, String)>,
}

fn words(adds: &BTreeMap<String, usize>) -> String {
    // a kind is said as a reader says it: `ask point`, `branches`, `boxes`
    let plural = |k: &str| if k.ends_with(['s', 'x']) || k.ends_with("ch") || k.ends_with("sh") { "es" } else { "s" };
    let parts: Vec<String> = adds.iter().map(|(k, n)| format!("{n} {}{}", k.replace('_', " "), if *n == 1 { "" } else { plural(k) })).collect();
    if parts.is_empty() { "nothing".into() } else { parts.join(", ") }
}

/// Every pack there is, built in and installed.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    packs: BTreeMap<String, Pack>,
}

/// The harness: which packs are on, in the order they load.
#[derive(Debug, Clone, Default)]
pub struct Harness {
    pub layers: Vec<Layer>,
    pub info: Vec<Info>,
}

impl Catalog {
    pub fn new(builtin: Vec<Pack>) -> Catalog {
        Catalog { packs: builtin.into_iter().map(|p| (p.manifest.id.clone(), p)).collect() }
    }

    /// The packs installed in `dir` (`~/.j4n/packs`). One that cannot be read, or has the id of a built-in one, is left out and said.
    pub fn with_installed(mut self, dir: &Path) -> (Catalog, Vec<Problem>) {
        let mut found = Vec::new();
        let Ok(read) = std::fs::read_dir(dir) else { return (self, found) };
        let mut names: Vec<_> = read.filter_map(Result::ok).filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        for name in names.into_iter().filter(|n| !n.starts_with('.')) {
            let at = format!("pack {name}");
            match read_installed(&dir.join(&name), &name) {
                Ok((pack, more)) => {
                    found.extend(more);
                    if let Some(old) = self.packs.get(&name).filter(|p| p.origin == Origin::Builtin) {
                        found.push(Problem::refused(at, format!("{} is a built-in pack's id: the installed copy is not used", old.manifest.id)));
                    } else {
                        self.packs.insert(name, pack);
                    }
                }
                Err(why) => found.push(Problem::refused(at, why)),
            }
        }
        (self, found)
    }

    pub fn get(&self, id: &str) -> Option<&Pack> {
        self.packs.get(id)
    }

    pub fn ids(&self) -> Vec<&str> {
        self.packs.keys().map(String::as_str).collect()
    }

    /// The packs that are on when the operator has said nothing, and why one that is not `basic` is on anyway: every built-in pack
    /// but `skip` when the folders say `packs.full`; else the `basic` ones, and a pack whose feature a folder already uses (an entry of a kind it
    /// declares, or one it ships: an account, a rule, a setting) with what it needs.
    pub fn defaults(&self, basic: &[&str], skip: &[&str], folders: &[Layer]) -> (Vec<String>, BTreeMap<String, String>) {
        let said: Vec<_> = folders.iter().flat_map(entries).collect();
        let full = says_full(folders);
        let builtin = || self.packs.values().filter(|p| p.origin == Origin::Builtin);
        if full {
            return (builtin().map(|p| p.manifest.id.clone()).filter(|id| !skip.contains(&id.as_str())).collect(), BTreeMap::new());
        }
        let kinds = |p: &Pack| -> BTreeSet<String> { entries(&p.layer).into_iter().filter(|(k, ..)| k == "kind").map(|(_, id, _)| id).collect() };
        let base = self.packs.get("base").map(kinds).unwrap_or_default();
        let mut kept = BTreeMap::new();
        for p in builtin().filter(|p| !basic.contains(&p.manifest.id.as_str())) {
            let (mine, own) = (entries(&p.layer), kinds(p));
            if let Some((k, id, _)) = said.iter().find(|(k, id, _)| mine.iter().any(|(mk, mid, _)| mk == k && mid == id) || (own.contains(k) && !base.contains(k))) {
                kept.insert(p.manifest.id.clone(), format!("your folder has {} {id}", k.replace('_', " ")));
            }
        }
        let mut todo: Vec<String> = kept.keys().cloned().collect();
        while let Some(id) = todo.pop() {
            let needs: Vec<String> = self.packs[&id].manifest.needs.iter().filter(|n| !basic.contains(&n.as_str()) && !kept.contains_key(*n)).cloned().collect();
            for n in needs {
                kept.insert(n.clone(), format!("{id} needs it"));
                todo.push(n);
            }
        }
        (basic.iter().map(|s| (*s).to_string()).chain(kept.keys().cloned()).collect(), kept)
    }

    /// Which packs are on and in what order: the defaults, then the operator's rows. A pack is left out, and `note`d, when it is
    /// installed and its files are not the ones pinned, when something it `needs` is not on, or when it is part of a loop.
    pub fn harness(&self, defaults: &[&str], rows: &[EnableRow]) -> Harness {
        let mut want: BTreeSet<String> = defaults.iter().map(|s| (*s).to_string()).collect();
        let mut notes: BTreeMap<String, String> = BTreeMap::new();
        for r in rows {
            if !r.on {
                want.remove(&r.id);
                continue;
            }
            match self.packs.get(&r.id) {
                None => {
                    notes.insert(r.id.clone(), "no such pack is installed".into());
                }
                Some(p) if p.origin == Origin::Installed && r.hash.as_deref() != Some(p.pin().as_str()) => {
                    let why = if r.hash.is_none() { "it was turned on with no pin".to_string() } else { "its files are not the ones that were pinned: turn it on again to accept them".to_string() };
                    notes.insert(r.id.clone(), why);
                }
                Some(_) => {
                    want.insert(r.id.clone());
                }
            }
        }
        // what a pack extends is loaded below it, on or not
        let mut load: BTreeSet<String> = BTreeSet::new();
        let mut todo: Vec<String> = want.iter().cloned().collect();
        while let Some(id) = todo.pop() {
            let Some(p) = self.packs.get(&id) else {
                notes.insert(id, "no such pack is installed".into());
                continue;
            };
            if load.insert(id) {
                todo.extend(p.manifest.extends.iter().cloned());
            }
        }
        // a pack whose needs are not on goes, and what needed it with it
        loop {
            let gone: Vec<String> = load
                .iter()
                .filter_map(|id| {
                    let lacks = self.packs[id].manifest.needs.iter().find(|n| !load.contains(*n))?;
                    notes.insert(id.clone(), format!("it needs `{lacks}`, which is not on"));
                    Some(id.clone())
                })
                .collect();
            if gone.is_empty() {
                break;
            }
            for g in &gone {
                load.remove(g);
            }
        }
        let mut order: Vec<String> = Vec::new();
        let mut left: BTreeSet<String> = load.clone();
        while !left.is_empty() {
            let ready: Vec<String> = left
                .iter()
                .filter(|id| self.packs[*id].manifest.needs.iter().chain(&self.packs[*id].manifest.extends).all(|d| !left.contains(d) || d == *id))
                .cloned()
                .collect();
            if ready.is_empty() {
                for id in &left {
                    notes.insert(id.clone(), "it is part of a loop of packs that need or extend each other".into());
                }
                break;
            }
            for id in &ready {
                left.remove(id);
            }
            order.extend(ready);
        }
        let mut info: Vec<Info> = self
            .packs
            .values()
            .map(|p| {
                let adds = p.adds();
                let id = &p.manifest.id;
                Info {
                    id: id.clone(),
                    version: p.manifest.version.clone(),
                    summary: p.manifest.summary.clone(),
                    use_when: p.manifest.use_when.clone(),
                    tag: p.tag(),
                    origin: p.origin.clone(),
                    on: order.contains(id),
                    note: notes.get(id).cloned().unwrap_or_default(),
                    widens: WIDENING.iter().any(|k| adds.contains_key(*k)),
                    pin: if p.origin == Origin::Installed { p.pin() } else { String::new() },
                    adds: words(&adds),
                    words: p.words(),
                    ready: p.manifest.ready.clone(),
                    keyed: p.keyed(),
                }
            })
            .collect();
        info.sort_by(|a, b| (!a.on, &a.id).cmp(&(!b.on, &b.id)));
        Harness { layers: order.iter().map(|id| self.packs[id].layer.clone()).collect(), info }
    }

    /// Copy the pack in `from` into `into/<id>/`, which must not be there: installing is a copy, and turning it on is a separate act.
    pub fn install(&mut self, from: &Path, into: &Path) -> Result<Manifest, String> {
        let text = std::fs::read_to_string(from.join("pack.toml")).map_err(|e| format!("{}: cannot read pack.toml: {e}", from.display()))?;
        let manifest = Manifest::parse(&text).map_err(|e| format!("pack.toml: {e}"))?;
        if self.packs.contains_key(&manifest.id) {
            return Err(format!("a pack called `{}` is already there", manifest.id));
        }
        let (pack, problems) = read_pack(from, &manifest.id).map_err(|e| format!("{}: {e}", from.display()))?;
        if let Some(p) = problems.iter().find(|p| p.level == Level::Refused) {
            return Err(p.to_string());
        }
        let to = into.join(&manifest.id);
        std::fs::create_dir_all(&to).map_err(|e| format!("cannot make {}: {e}", to.display()))?;
        std::fs::write(to.join("pack.toml"), &text).map_err(|e| e.to_string())?;
        for f in &pack.layer.files {
            let at = to.join(&f.path);
            std::fs::create_dir_all(at.parent().unwrap_or(&to)).and_then(|()| std::fs::write(&at, &f.text)).map_err(|e| format!("cannot write {}: {e}", at.display()))?;
        }
        if let Err(e) = copy_rest(from, &to, "", 0, &mut 0) {
            let _ = std::fs::remove_dir_all(&to);
            return Err(format!("cannot copy the rest of the pack: {e}"));
        }
        // the pin is the copy's, which leaves out hidden names and links
        let pack = match read_installed(&to, &manifest.id) {
            Ok((p, _)) => p,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&to);
                return Err(format!("{}: {e}", to.display()));
            }
        };
        self.packs.insert(manifest.id.clone(), pack);
        Ok(manifest)
    }
}

/// An installed pack as it stands in its folder, every file pinned.
fn read_installed(dir: &Path, name: &str) -> Result<(Pack, Vec<Problem>), String> {
    let (mut pack, problems) = read_pack(dir, name)?;
    let held: BTreeSet<&str> = pack.layer.files.iter().map(|f| f.path.as_str()).chain(["pack.toml"]).collect();
    let (mut sha, mut seen) = (Sha256::new(), (0, 0));
    hash_rest(dir, "", &held, &mut sha, &mut seen)?;
    pack.rest = if seen.1 == 0 { String::new() } else { sha.finalize().iter().map(|b| format!("{b:02x}")).collect() };
    Ok((pack, problems))
}

/// The manifest and the `.toml` files of a folder (where a pack is installed from, or kept).
fn read_pack(dir: &Path, name: &str) -> Result<(Pack, Vec<Problem>), String> {
    let text = std::fs::read_to_string(dir.join("pack.toml")).map_err(|e| format!("cannot read pack.toml: {e}"))?;
    let manifest = Manifest::parse(&text).map_err(|e| format!("pack.toml: {e}"))?;
    if manifest.id != name {
        return Err(format!("pack.toml says `{}` and the folder is `{name}`", manifest.id));
    }
    let (layer, problems) = Layer::from_dir(LayerKind::Pack, &manifest.id, dir);
    Ok((Pack { manifest, manifest_text: text, layer, origin: Origin::Installed, rest: String::new() }, problems))
}

/// The most files of an installed pack that its pin takes in; a pack with more is not loaded.
const MAX_PIN_FILES: usize = 5000;

/// Feed `sha` every file under `root/rel` that the layer and the manifest do not hold, in a stable order: path, link target and bytes.
/// Hidden names and links are in, a connector can start any of them. `seen` counts the files looked at and the files hashed.
fn hash_rest(root: &Path, rel: &str, held: &BTreeSet<&str>, sha: &mut Sha256, seen: &mut (usize, usize)) -> Result<(), String> {
    let list = std::fs::read_dir(root.join(rel)).map_err(|e| format!("cannot read {}: {e}", root.join(rel).display()))?;
    let mut names: Vec<String> = list.filter_map(Result::ok).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    for name in names {
        let at = if rel.is_empty() { name } else { format!("{rel}/{name}") };
        let path = root.join(&at);
        let meta = std::fs::symlink_metadata(&path).map_err(|e| format!("cannot read {at}: {e}"))?;
        if meta.is_dir() {
            hash_rest(root, &at, held, sha, seen)?;
            continue;
        }
        seen.0 += 1;
        if seen.0 > MAX_PIN_FILES {
            return Err(format!("more than {MAX_PIN_FILES} files, too many to pin"));
        }
        if held.contains(at.as_str()) {
            continue;
        }
        let link = meta.is_symlink().then(|| std::fs::read_link(&path).unwrap_or_default());
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(_) if link.is_some() => Vec::new(),
            Err(e) => return Err(format!("cannot read {at}: {e}")),
        };
        sha.update(format!("{at}\0{link:?}\0{}\0", bytes.len()));
        sha.update(&bytes);
        seen.1 += 1;
    }
    Ok(())
}

/// The most the files that are not `.toml` (a pack's scripts, tests and README) may come to when a pack is installed.
const MAX_REST: u64 = 8 << 20;

/// Copy what `Layer::from_dir` did not read — scripts, tests, the README — from `from/rel` to `to/rel`: no hidden names (a clone's `.git`), no
/// links, nothing over [`MAX_REST`] in all. Once copied, the pin takes in these too.
fn copy_rest(from: &Path, to: &Path, rel: &str, depth: usize, total: &mut u64) -> std::io::Result<()> {
    for e in std::fs::read_dir(from.join(rel))?.filter_map(Result::ok) {
        let (name, kind) = (e.file_name().to_string_lossy().into_owned(), e.file_type()?);
        let at = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
        if name.starts_with('.') || kind.is_symlink() || (rel.is_empty() && name == "pack.toml") {
            continue;
        }
        if kind.is_dir() && depth < 6 {
            copy_rest(from, to, &at, depth + 1, total)?;
        } else if kind.is_file() && !to.join(&at).exists() {
            *total += e.metadata()?.len();
            if *total > MAX_REST {
                return Err(std::io::Error::other(format!("more than {MAX_REST} bytes")));
            }
            std::fs::create_dir_all(to.join(&at).parent().unwrap_or(to))?;
            std::fs::copy(e.path(), to.join(&at))?;
        }
    }
    Ok(())
}

/// Where installed packs are kept.
pub fn packs_dir(home: &Path) -> PathBuf {
    home.join("packs")
}
