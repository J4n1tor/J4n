//! The reference under `docs/reference/` is made from the pack rows, never written by hand: settings, commands and packs as tables.
//! A pack change that the reference does not show fails here; `J4N_BLESS=1` writes the files again. The same rows are what J4n's
//! own help is built from, so the docs and the product say one thing.

use std::collections::BTreeMap;
use std::fs;
use toml::Value;

use super::support::root;

const HEAD: &str = "<!-- Made from the pack rows by crates/j4n/tests/laws/reference_docs.rs; do not edit. J4N_BLESS=1 writes it again. -->\n\n";

/// One pack's files of one kind (`settings`, `commands`, `phrases`), each read as TOML, sorted by file name.
fn rows(pack: &str, kind: &str, table: &str) -> Vec<Value> {
    let dir = root().join("packs").join(pack).join(kind);
    let Ok(read) = fs::read_dir(&dir) else { return Vec::new() };
    let mut files: Vec<_> = read.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "toml")).collect();
    files.sort();
    files
        .iter()
        .flat_map(|f| {
            let v = Value::Table(fs::read_to_string(f).unwrap().parse::<toml::Table>().unwrap_or_else(|e| panic!("{}: {e}", f.display())));
            v.get(table).and_then(Value::as_array).cloned().unwrap_or_default()
        })
        .collect()
}

fn packs() -> Vec<(String, Value)> {
    let mut ids: Vec<String> = fs::read_dir(root().join("packs")).unwrap().filter_map(Result::ok).filter(|e| e.path().join("pack.toml").is_file()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    ids.sort();
    ids.into_iter().map(|id| (id.clone(), Value::Table(fs::read_to_string(root().join("packs").join(&id).join("pack.toml")).unwrap().parse::<toml::Table>().unwrap()))).collect()
}

/// A cell: one line, pipes escaped.
fn cell(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").replace('|', "\\|")
}

fn text(v: Option<&Value>) -> String {
    v.and_then(Value::as_str).unwrap_or("").to_string()
}

fn shown(v: &Value) -> String {
    match v {
        Value::String(s) if s.is_empty() => "(empty)".into(),
        Value::String(s) => format!("`{s}`"),
        Value::Array(a) if a.is_empty() => "(none)".into(),
        other => format!("`{}`", other.to_string().replace('|', "\\|")),
    }
}

fn settings() -> String {
    let mut out = String::from(HEAD);
    let all: Vec<(String, Vec<Value>)> = packs().into_iter().map(|(id, _)| (id.clone(), rows(&id, "settings", "setting"))).filter(|(_, r)| !r.is_empty()).collect();
    let count: usize = all.iter().map(|(_, r)| r.len()).sum();
    out += &format!("# Settings\n\nEvery setting J4n has: {count}, from {} packs. A setting of a pack that is off does nothing. ", all.len());
    out += "*Live* no: an edit takes effect at the next start. *Shown*: under Settings › Features (else only under All). ";
    out += "*Follows*: the value `auto` means what that other setting says. Under each table, the full text of every setting whose first sentence is not all of it.\n\n";
    for (pack, rs) in &all {
        out += &format!("## {pack}\n\n| Setting | Type | Default | Words | Live | Shown | Follows | What it does |\n|---|---|---|---|---|---|---|---|\n");
        let mut rs = rs.clone();
        rs.sort_by_key(|r| text(r.get("id")));
        for r in &rs {
            let words = r.get("words").and_then(Value::as_array).map(|w| w.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")).unwrap_or_default();
            let range = match (r.get("min"), r.get("max")) {
                (Some(a), Some(b)) => format!("{a}–{b}"),
                _ => String::new(),
            };
            let doc = r.get("brief").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| first(&text(r.get("doc"))));
            out += &format!(
                "| `{}` | {} | {} | {} | {} | {} | {} | {} |\n",
                text(r.get("id")),
                text(r.get("ty")),
                r.get("default").map(shown).unwrap_or_default(),
                cell(if words.is_empty() { &range } else { &words }),
                if r.get("live").and_then(Value::as_bool) == Some(false) { "no" } else { "yes" },
                if r.get("basic").and_then(Value::as_bool) == Some(true) { "yes" } else { "" },
                text(r.get("auto")).replace('|', "\\|"),
                cell(&doc)
            );
        }
        out += "\n";
        for r in &rs {
            let doc = text(r.get("doc"));
            if doc.trim() != first(&doc) {
                out += &format!("- **`{}`**: {}\n", text(r.get("id")), doc.split_whitespace().collect::<Vec<_>>().join(" "));
            }
        }
        out += "\n";
    }
    out
}

/// The first sentence of a doc: up to the first `. ` that is not inside backticks.
fn first(doc: &str) -> String {
    let flat = doc.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut ticks = 0;
    for (i, c) in flat.char_indices() {
        match c {
            '`' => ticks += 1,
            '.' if ticks % 2 == 0 && flat[i + 1..].starts_with(' ') => return flat[..=i].to_string(),
            _ => {}
        }
    }
    flat
}

fn commands() -> String {
    let mut out = String::from(HEAD);
    out += "# Commands\n\nWhat a line can start with, per pack. A command of a pack that is off leads to turning the pack on.\n";
    for (pack, _) in packs() {
        let mut cs: Vec<Value> = rows(&pack, "phrases", "phrase");
        cs.extend(rows(&pack, "commands", "command"));
        cs.retain(|c| c.get("words").is_some() && !text(c.get("doc")).is_empty());
        if cs.is_empty() {
            continue;
        }
        out += &format!("\n## {pack}\n\n| Words | What it does |\n|---|---|\n");
        for c in &cs {
            out += &format!("| {} | {} |\n", words(c), cell(&text(c.get("doc"))));
        }
    }
    out
}

fn words(row: &Value) -> String {
    row.get("words").and_then(Value::as_array).map(|w| w.iter().filter_map(Value::as_str).map(|w| format!("`{w}`")).collect::<Vec<_>>().join(" ")).unwrap_or_default()
}

fn packs_doc() -> String {
    let on: Vec<&str> = j4n::pack::ON.to_vec();
    let mut out = String::from(HEAD);
    out += "# Packs\n\nEverything J4n does beyond its core is a pack: TOML rows for settings, rules, gates, flows, connectors, pages and commands ";
    out += "(see [PACKS-GUIDE](../PACKS-GUIDE.md)). *On* yes: on when nothing was set; the others are turned on with `:pack on <id>`.\n\n";
    out += "| Pack | On | Stability | Needs | What it adds | What it is |\n|---|---|---|---|---|---|\n";
    let mut kinds: BTreeMap<String, String> = BTreeMap::new();
    for (id, _) in packs() {
        let dir = root().join("packs").join(&id);
        let mut have: Vec<String> = fs::read_dir(&dir).unwrap().filter_map(Result::ok).filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        have.sort();
        kinds.insert(id, have.join(", "));
    }
    for (id, m) in packs() {
        let needs = m.get("needs").and_then(Value::as_array).map(|n| n.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")).unwrap_or_default();
        out += &format!(
            "| `{id}` | {} | {} | {} | {} | {} |\n",
            if on.contains(&id.as_str()) { "yes" } else { "" },
            text(m.get("stability")),
            needs,
            kinds.get(&id).cloned().unwrap_or_default(),
            cell(&text(m.get("summary")))
        );
    }
    out
}

#[test]
fn the_reference_is_the_pack_rows() {
    let dir = root().join("docs/reference");
    let made = [("settings.md", settings()), ("commands.md", commands()), ("packs.md", packs_doc())];
    if std::env::var_os("J4N_BLESS").is_some() {
        fs::create_dir_all(&dir).unwrap();
        for (name, body) in &made {
            fs::write(dir.join(name), body).unwrap();
        }
    }
    for (name, body) in &made {
        let have = fs::read_to_string(dir.join(name)).unwrap_or_default();
        assert!(have == *body, "docs/reference/{name} is not what the pack rows say (J4N_BLESS=1 writes it again)");
    }
}
