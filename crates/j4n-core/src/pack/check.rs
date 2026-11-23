//! `j4n pack check` and `j4n pack new` (`docs/PACKS-GUIDE.md`): the validate/assert step of a pack folder, and the template
//! it starts from. [`check`] reads a folder the way an installed pack is read and says, as `file:line: why`, what is wrong with it:
//!
//! - `pack.toml` reads, and what it needs and extends is a pack that is there;
//! - every kind file loads against the kinds its dependencies declare (the Table's own check, nothing re-done here);
//! - the cases of `tests/*.toml` (`[[expect]]`, see [`CASE_KEYS`]) hold on the pack as it loads, with what it needs and extends beneath it;
//! - the goldens of those cases (`tests/golden/`) are what the pack renders now (`bless` writes them instead).
//!
//! A problem the packs below it already have is theirs and is not said again.

use super::{Catalog, Manifest};
use crate::front::phrase::{FLOW, Phrases};
use crate::table::{Layer, LayerKind, Level, Problem, Tables};
use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

/// What an `[[expect]]` of `tests/*.toml` may hold. One of `has`, `say` or `show` makes it what it is; the rest belong to one of them.
pub const CASE_KEYS: [&str; 11] = ["id", "has", "absent", "field", "is", "say", "runs", "starts", "args", "show", "golden"];

/// One thing `check` found: where (`path:line`), whether it fails the pack, and what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub level: Level,
    pub at: String,
    pub msg: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}{}", self.at, if self.level == Level::Warn { "warning: " } else { "" }, self.msg)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Report {
    pub id: String,
    pub files: usize,
    pub entries: usize,
    pub cases: usize,
    pub goldens: usize,
    pub found: Vec<Finding>,
}

impl Report {
    /// A pack passes while nothing it said is refused; a warning is for the author.
    pub fn ok(&self) -> bool {
        self.found.iter().all(|f| f.level != Level::Refused)
    }

    pub fn summary(&self) -> String {
        let bad = self.found.iter().filter(|f| f.level == Level::Refused).count();
        let what = format!("{} files, {} entries, {} cases, {} goldens", self.files, self.entries, self.cases, self.goldens);
        match bad {
            0 => format!("pack {}: ok ({what})", self.id),
            n => format!("pack {}: {n} problem{} ({what})", self.id, if n == 1 { "" } else { "s" }),
        }
    }

    fn say(&mut self, level: Level, dir: &Path, file: &str, line: usize, msg: impl Into<String>) {
        self.found.push(Finding { level, at: format!("{}:{line}", dir.join(file).display()), msg: msg.into() });
    }
}

/// Check the pack in `dir`; `catalog` is where what it needs and extends is looked for. `bless` writes the goldens that differ.
pub fn check(dir: &Path, catalog: &Catalog, bless: bool) -> Report {
    let mut r = Report::default();
    let Ok(text) = std::fs::read_to_string(dir.join("pack.toml")) else {
        r.say(Level::Refused, dir, "pack.toml", 1, "there is no readable pack.toml: a pack is a folder with one");
        return r;
    };
    let manifest = match Manifest::parse(&text) {
        Ok(m) => m,
        Err(msg) => {
            r.say(Level::Refused, dir, "pack.toml", manifest_line(&text, &msg), msg);
            return r;
        }
    };
    r.id = manifest.id.clone();
    let parts: Vec<&str> = manifest.version.split('.').collect();
    if parts.len() != 3 || !parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())) {
        r.say(Level::Refused, dir, "pack.toml", key_line(&text, "version"), format!("`version = \"{}\"` is not major.minor.patch (\"0.1.0\")", manifest.version));
    }
    if manifest.stability == super::Stability::Stable {
        r.say(Level::Warn, dir, "pack.toml", key_line(&text, "stability"), "`stable` comes only from the operator's dated list: this pack is shown `beta`");
    }
    let below = below(&manifest, catalog, &mut |key, id| r.say(Level::Refused, dir, "pack.toml", key_line(&text, key), format!("`{key}` names `{id}`, which no built-in or installed pack is")));
    let (own, walked) = Layer::from_dir(LayerKind::Pack, &manifest.id, dir);
    let (theirs_tables, theirs) = Tables::load(below.clone());
    // the kinds of the core are in the binary and in `packs/base` alike: the same file is the core's, not a second declaration
    let mut read = own.clone();
    read.files.retain(|f| !crate::table::is_core(&f.path, &f.text));
    let mut layers = below;
    layers.push(read);
    let (tables, loaded) = Tables::load(layers);
    for p in walked.into_iter().chain(loaded.into_iter().filter(|p| !theirs.contains(p))) {
        let (file, line) = locate(&p, &own);
        // a `ref` to nothing is a pack that cannot work: it fails here, though the loader of a folder only warns
        if p.level == Level::Warn && crate::schema::is_dangling(&p.msg) {
            let row = p.at.find("[[").map_or(String::new(), |i| format!("{}: ", &p.at[i..]));
            r.say(Level::Refused, dir, &file, line, format!("{row}{}", p.msg));
        } else {
            r.say(p.level, dir, &file, line, p.msg);
        }
    }
    let made = entries(&own);
    r.files = own.files.len();
    r.entries = made.len();
    // what the core reads of a kind it knows (words that do not open with a colon, a word two phrases claim) is judged too
    let (phrases, said) = Phrases::load(&tables);
    let theirs = Phrases::load(&theirs_tables).1;
    for p in said.into_iter().filter(|p| !theirs.contains(p)) {
        let (file, line) = locate(&p, &own);
        r.say(p.level, dir, &file, line, p.msg);
    }
    // a flow is checked like a program before it runs: what its lint refuses is refused here, where the pack is written
    let theirs = crate::flow::Flows::load(&theirs_tables).1;
    for p in crate::flow::Flows::load(&tables).1.into_iter().filter(|p| !theirs.contains(p)) {
        let (file, line) = locate(&p, &own);
        r.say(p.level, dir, &file, line, p.msg);
    }
    run_tests(&mut r, dir, &Rig { tables: &tables, phrases: &phrases, made: &made, bless });
    r
}

/// The layers below the pack: what it needs and extends, and what those do, in the order they load. `lack` is told of a name that is no pack.
fn below(m: &Manifest, catalog: &Catalog, lack: &mut dyn FnMut(&str, &str)) -> Vec<Layer> {
    let (mut want, mut todo) = (BTreeSet::new(), Vec::new());
    for (key, ids) in [("needs", &m.needs), ("extends", &m.extends)] {
        for id in ids {
            if catalog.get(id).is_none() {
                lack(key, id);
            } else {
                todo.push(id.clone());
            }
        }
    }
    while let Some(id) = todo.pop() {
        if want.insert(id.clone())
            && let Some(p) = catalog.get(&id)
        {
            todo.extend(p.manifest.needs.iter().chain(&p.manifest.extends).cloned());
        }
    }
    let want: Vec<&str> = want.iter().map(String::as_str).collect();
    catalog.harness(&want, &[]).layers
}

/// `(kind, id, file)` of every entry the layer's files hold.
fn entries(layer: &Layer) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for f in &layer.files {
        let Ok(t) = f.text.parse::<toml::Table>() else { continue };
        for (kind, v) in t.iter().filter(|(k, _)| *k != "api") {
            for item in v.as_array().into_iter().flatten() {
                if let Some(id) = item.get("id").and_then(toml::Value::as_str) {
                    out.push((kind.clone(), id.to_string(), f.path.clone()));
                }
            }
        }
    }
    out.sort();
    out
}

/// Which file and line a problem of the Table is about: `layer:path`, then `[[kind]] id` when it names an entry.
fn locate(p: &Problem, layer: &Layer) -> (String, usize) {
    let (head, entry) = p.at.find("[[").map_or((p.at.trim(), ""), |i| (p.at[..i].trim(), &p.at[i..]));
    let (kind, id) = entry.strip_prefix("[[").and_then(|e| e.split_once("]] ")).unwrap_or(("", ""));
    let named = head.split_once(':').map(|(_, path)| path.trim()).filter(|path| layer.files.iter().any(|f| f.path == *path));
    let file = named.or_else(|| layer.files.iter().find(|f| entries(&Layer::new(LayerKind::Pack, "", vec![(*f).clone()])).iter().any(|(k, i, _)| k == kind && i == id)).map(|f| f.path.as_str()));
    let Some(file) = file else { return (head.split_once(':').map_or(head, |(_, path)| path).to_string(), 1) };
    let text = &layer.files.iter().find(|f| f.path == file).map_or("", |f| f.text.as_str());
    let said = p.msg.split("(line ").nth(1).and_then(|r| r.split(')').next()).and_then(|n| n.parse().ok());
    (file.to_string(), said.unwrap_or_else(|| if kind.is_empty() { 1 } else { entry_line(text, kind, id) }))
}

/// The line of the `[[kind]]` that opens the entry `id` (1 when it is not written that way).
fn entry_line(text: &str, kind: &str, id: &str) -> usize {
    let (head, has) = (format!("[[{kind}]]"), [format!("id = \"{id}\""), format!("id=\"{id}\"")]);
    let mut open = None;
    for (n, l) in text.lines().enumerate().map(|(n, l)| (n + 1, l.trim())) {
        if l.starts_with("[[") {
            open = (l == head).then_some(n);
        } else if let Some(at) = open.filter(|_| has.iter().any(|h| l.starts_with(h.as_str()))) {
            return at;
        }
    }
    text.lines().position(|l| has.iter().any(|h| l.contains(h.as_str()))).map_or(1, |n| n + 1)
}

/// The line of a key at the left edge of `pack.toml` (1 when it is not there).
fn key_line(text: &str, key: &str) -> usize {
    text.lines().position(|l| l.split(['=', ' ']).next() == Some(key)).map_or(1, |n| n + 1)
}

fn manifest_line(text: &str, msg: &str) -> usize {
    let span = |e: Option<std::ops::Range<usize>>| e.and_then(|s| text.get(..s.start)).map(|h| h.matches('\n').count() + 1);
    span(text.parse::<toml::Table>().err().and_then(|e| e.span()))
        .or_else(|| span(toml::from_str::<Manifest>(text).err().and_then(|e| e.span())))
        .or_else(|| ["api", "id", "summary", "use_when", "version", "stability", "needs", "extends"].iter().find(|k| msg.contains(&format!("`{k}"))).map(|k| key_line(text, k)))
        .unwrap_or(1)
}

/// What the cases are run against.
struct Rig<'a> {
    tables: &'a Tables,
    phrases: &'a Phrases,
    made: &'a [(String, String, String)],
    bless: bool,
}

/// A case that does not hold: what is wrong, and `(file, line)` when it is a file other than the case's own that says so.
struct Miss(String, Option<(String, usize)>);

impl From<&str> for Miss {
    fn from(msg: &str) -> Miss {
        Miss(msg.to_string(), None)
    }
}

impl From<String> for Miss {
    fn from(msg: String) -> Miss {
        Miss(msg, None)
    }
}

fn run_tests(r: &mut Report, dir: &Path, rig: &Rig) {
    let Ok(read) = std::fs::read_dir(dir.join("tests")) else { return };
    let mut names: Vec<String> = read.filter_map(Result::ok).filter(|e| e.file_type().is_ok_and(|t| t.is_file())).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".toml") && !n.starts_with('.')).collect();
    names.sort();
    for name in names {
        let file = format!("tests/{name}");
        let Ok(text) = std::fs::read_to_string(dir.join(&file)) else {
            r.say(Level::Refused, dir, &file, 1, "cannot be read");
            continue;
        };
        let table = match text.parse::<toml::Table>() {
            Ok(t) => t,
            Err(e) => {
                let line = e.span().and_then(|s| text.get(..s.start)).map_or(1, |h| h.matches('\n').count() + 1);
                r.say(Level::Refused, dir, &file, line, format!("not TOML: {}", e.message()));
                continue;
            }
        };
        let Some(cases) = table.get("expect").and_then(toml::Value::as_array) else {
            r.say(Level::Warn, dir, &file, 1, "holds no [[expect]] case, so `check` does not run it");
            continue;
        };
        let at: Vec<usize> = text.lines().enumerate().filter(|(_, l)| l.trim() == "[[expect]]").map(|(n, _)| n + 1).collect();
        for (i, case) in cases.iter().enumerate() {
            let line = at.get(i).copied().unwrap_or(1);
            r.cases += 1;
            let Some(case) = case.as_table() else { continue };
            let name = case.get("id").and_then(toml::Value::as_str).map_or(format!("#{}", i + 1), |s| format!("`{s}`"));
            match one(case, rig, dir) {
                Ok(golden) => r.goldens += usize::from(golden),
                Err(Miss(msg, elsewhere)) => {
                    let (f, l) = elsewhere.unwrap_or((file.clone(), line));
                    r.say(Level::Refused, dir, &f, l, format!("expect {name}: {msg}"));
                }
            }
        }
    }
}

/// Run one `[[expect]]`; whether it was a golden.
fn one(case: &toml::Table, rig: &Rig, dir: &Path) -> Result<bool, Miss> {
    if let Some(k) = case.keys().find(|k| !CASE_KEYS.contains(&k.as_str())) {
        return Err(format!("unknown key `{k}` (an [[expect]] holds {})", CASE_KEYS.join(", ")).into());
    }
    let text = |k: &str| case.get(k).map(|v| v.as_str().map(str::to_string).ok_or(format!("`{k}` is a string")));
    let (has, say, show) = (text("has").transpose()?, text("say").transpose()?, text("show").transpose()?);
    match (has, say, show) {
        (Some(has), None, None) => has_case(case, rig, &has).map(|()| false),
        (None, Some(say), None) => say_case(case, rig, &say).map(|()| false),
        (None, None, Some(show)) => golden_case(case, rig, dir, &show).map(|()| true),
        _ => Err("one of `has` (an entry is there), `say` (a line is a command) or `show` with `golden` (the text it renders) says what to check".to_string().into()),
    }
}

fn has_case(case: &toml::Table, rig: &Rig, has: &str) -> Result<(), Miss> {
    let (kind, id) = has.split_once(':').ok_or(format!("`has = \"{has}\"` is `kind:id`"))?;
    let entry = rig.tables.raw(kind, id);
    if case.get("absent").and_then(toml::Value::as_bool) == Some(true) {
        return entry.map_or(Ok(()), |_| Err(format!("{kind}:{id} is there, and should not be")).map_err(Miss::from));
    }
    let entry = entry.ok_or(format!("{kind}:{id} is not there"))?;
    match (case.get("field").and_then(toml::Value::as_str), case.get("is")) {
        (Some(field), want) => match (entry.fields.get(field), want) {
            (None, _) => Err(format!("{kind}:{id} has no `{field}`").into()),
            (Some(have), Some(want)) if have != want => Err(format!("{kind}:{id} has {field} = {have}, expected {want}").into()),
            _ => Ok(()),
        },
        (None, Some(_)) => Err("`is` goes with a `field`".into()),
        (None, None) => Ok(()),
    }
}

fn say_case(case: &toml::Table, rig: &Rig, say: &str) -> Result<(), Miss> {
    let got = rig.phrases.parse(say).or_else(|| rig.phrases.take(say).map(|t| t.cmd));
    let word = |k: &str| case.get(k).and_then(toml::Value::as_str);
    let (name, args) = match (&got, word("runs"), word("starts")) {
        (None, None, None) => return Ok(()),
        (None, ..) => return Err(format!("`{say}` is not a phrase of the table").into()),
        (Some(c), None, None) => return Err(format!("`{say}` runs `{}`, expected no command", c.name).into()),
        (Some(c), Some(runs), None) if c.name != runs => return Err(format!("`{say}` runs `{}`, expected `{runs}`", c.name).into()),
        (Some(c), Some(_), None) => (c.name.as_str(), c.args.clone()),
        (Some(c), None, Some(flow)) if c.name != FLOW || c.args.first().map(String::as_str) != Some(flow) => return Err(format!("`{say}` does not start the flow `{flow}`").into()),
        (Some(c), None, Some(_)) => (c.name.as_str(), c.args[1..].to_vec()),
        (Some(_), Some(_), Some(_)) => return Err("`runs` and `starts` do not go together".to_string().into()),
    };
    match case.get("args").and_then(toml::Value::as_array) {
        Some(want) if want.iter().map(|v| v.as_str().unwrap_or_default().to_string()).collect::<Vec<_>>() != args => Err(format!("`{say}` gives `{name}` the arguments {args:?}, expected {want:?}").into()),
        _ => Ok(()),
    }
}

fn golden_case(case: &toml::Table, rig: &Rig, dir: &Path, show: &str) -> Result<(), Miss> {
    let name = case.get("golden").and_then(toml::Value::as_str).filter(|n| !n.is_empty() && !n.contains(['/', '\\']) && !n.starts_with('.')).ok_or("`show` needs a `golden` file name (no folders) in tests/golden/")?;
    let want = if show == "entries" {
        rig.made.iter().map(|(k, i, _)| format!("{k} {i}\n")).collect::<String>()
    } else {
        let ids: Vec<&str> = rig.made.iter().filter(|(k, ..)| k == show).map(|(_, i, _)| i.as_str()).collect();
        if ids.is_empty() {
            return Err(format!("the pack has no `{show}` entries to show (`show` is `entries` or a kind it fills)").into());
        }
        ids.iter()
            .filter_map(|id| rig.tables.raw(show, id).map(|m| (id, m)))
            .map(|(id, m)| {
                let mut fields = m.fields.clone();
                fields.insert("id".into(), toml::Value::String((*id).into()));
                format!("[[{show}]]\n{}\n", toml::to_string(&fields).unwrap_or_default())
            })
            .collect::<String>()
    };
    let (rel, path) = (format!("tests/golden/{name}"), dir.join("tests/golden").join(name));
    if rig.bless {
        return std::fs::create_dir_all(path.parent().unwrap_or(dir)).and_then(|()| std::fs::write(&path, &want)).map_err(|e| format!("cannot write {rel}: {e}").into());
    }
    let Ok(have) = std::fs::read_to_string(&path) else { return Err(Miss(format!("{rel} is not there (J4N_BLESS=1 writes it)"), None)) };
    if have == want {
        return Ok(());
    }
    let (a, b): (Vec<&str>, Vec<&str>) = (have.lines().collect(), want.lines().collect());
    let n = (0..a.len().max(b.len())).find(|&n| a.get(n) != b.get(n)).unwrap_or(0);
    let said = |v: &[&str]| v.get(n).map_or("(nothing)".to_string(), |l| format!("`{l}`"));
    Err(Miss(format!("{rel} says {}, the pack gives {} (J4N_BLESS=1 rewrites it)", said(&a), said(&b)), Some((rel, n + 1))))
}

/// The files of a new pack called `name`: a manifest, a phrase (the commonest kind), a case that holds and a golden for it, a README.
pub fn template(name: &str) -> Result<Vec<(String, String)>, String> {
    if !crate::schema::is_id(name) || name.contains(['/', ':']) || name.len() > 40 {
        return Err(format!("`{name}` is not a pack name (letters, digits, `_ - .`, starting with a letter or digit)"));
    }
    let files = [
        ("pack.toml", format!("api = 1\nid = \"{name}\"\nversion = \"0.1.0\"\nsummary = \"what {name} is for, in one line\"\nneeds = [\"base\"]\nstability = \"experimental\"\n")),
        ("phrases/hello.toml", format!("api = 1\n\n# An entry's id is shared by every pack: keep it your own (here, the pack's name). The words open with a colon.\n[[phrase]]\nid = \"{name}\"\ndoc = \"Say how things stand.\"\nwords = [\":{name}\"]\ncommand = \"status\"\n")),
        ("tests/hello.toml", format!("api = 1\n\n# `j4n pack check` runs these: an entry is there, a line is a command, and what the pack adds is what the golden says.\n[[expect]]\nhas = \"phrase:{name}\"\nfield = \"command\"\nis = \"status\"\n\n[[expect]]\nsay = \":{name}\"\nruns = \"status\"\n\n[[expect]]\nshow = \"entries\"\ngolden = \"entries.txt\"\n")),
        ("tests/golden/entries.txt", format!("phrase {name}\n")),
        ("README.md", format!("# {name}\n\nWhat {name} is for. Made by `j4n pack new`; the guide is `docs/PACKS-GUIDE.md` of J4n.\n\n    j4n pack check .\n    j4n pack install .\n\nThen turn it on in J4n with `:pack on {name}`.\n")),
    ];
    Ok(files.into_iter().map(|(p, t)| (p.to_string(), t)).collect())
}
