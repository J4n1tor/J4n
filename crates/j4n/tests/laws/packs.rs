//! The pack stubs are well-formed: each `packs/<id>/pack.toml` holds exactly the keys the Table accepts,
//! `api = 1`, an `id` equal to its directory and links to other packs that do not loop. `stable` is refused: no pack is
//! marked stable yet. `packs/` holds exactly the built-in packs.

use crate::support::{assert_clean, read, root};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;

/// The keys a `pack.toml` may hold; the Table refuses any other.
const KEYS: [&str; 9] = ["api", "id", "version", "summary", "use_when", "extends", "needs", "stability", "ready"];
/// The keys every `pack.toml` must hold; a pack with no `extends` or `needs` extends and needs nothing.
const REQUIRED: [&str; 5] = ["api", "id", "version", "summary", "stability"];
/// The keys that name other packs.
const LINKS: [&str; 2] = ["extends", "needs"];
/// The built-in packs, compiled into the product: `packs/` holds exactly these directories. A tenth pack, or
/// a rename, is a deliberate edit of this list that says why, as for `CRATES` in `layout.rs`. `terminal` is the
/// tmux tools of a model: they are connectors, and a model is given every connected tool, so they are a pack the operator turns on
/// rather than a cost on every turn of everyone's.
/// `lanes` is the main lane's branch rows. `guard` is the switch of redactor v2: a manifest and no files.
// `offload`: the chores a line may be, the switch and its cap, and the rows a lint or build chore names are data
// `crew`: the kind `seat`, the six seats and the switch `crew.on`, all data
// `reflexes`: the shipped reflexes, their cases, `:reflex` and `:reflexes`, the page and the settings, all data; the switch is `reflexes.on`, off
const BUILTIN: [&str; 24] = ["base", "agent", "code", "connect", "jev", "grow", "terminal", "accounts", "bench", "shape", "qc", "contribute", "inbox", "ledger", "parts", "habits", "lanes", "crew", "guard", "kit", "offload", "skills", "reflexes", "test"];
const STABILITIES: [&str; 3] = ["experimental", "beta", "stable"];
const STABLE_REFUSED: &str = concat!(
    "`stability = \"stable\"` is refused — `stable` comes only from a confirmed list, ",
    "and no such list exists yet"
);

/// `major.minor.patch`, each part digits.
fn is_dotted_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3 && parts.iter().all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

/// What is wrong with the `pack.toml` of directory `dir`; `others` are the other directories under `packs/`.
fn manifest_violations(dir: &str, text: &str, others: &BTreeSet<String>) -> Vec<String> {
    let at = format!("packs/{dir}/pack.toml");
    let table: toml::Table = match text.parse() {
        Ok(table) => table,
        Err(e) => return vec![format!("{at}: not TOML: {}", e.message())],
    };
    let mut found = Vec::new();
    let mut bad = |what: String| found.push(format!("{at}: {what}"));
    for key in table.keys().filter(|key| !KEYS.contains(&key.as_str())) {
        bad(format!("unknown key `{key}` — the Table accepts {}", KEYS.join(", ")));
    }
    for key in REQUIRED.iter().filter(|key| !table.contains_key(**key)) {
        bad(format!("missing key `{key}`"));
    }
    if let Some(api) = table.get("api")
        && api.as_integer() != Some(1)
    {
        bad("`api` must be the integer 1".to_owned());
    }
    if let Some(id) = table.get("id")
        && id.as_str() != Some(dir)
    {
        bad(format!("`id` must be the string \"{dir}\", the directory name"));
    }
    if let Some(version) = table.get("version")
        && !version.as_str().is_some_and(is_dotted_version)
    {
        bad("`version` must be a string like \"0.1.0\" (major.minor.patch)".to_owned());
    }
    if let Some(summary) = table.get("summary")
        && !summary.as_str().is_some_and(|s| !s.trim().is_empty())
    {
        bad("`summary` must be a non-empty string".to_owned());
    }
    for key in LINKS {
        let Some(value) = table.get(key) else { continue };
        let Some(items) = value.as_array() else {
            bad(format!("`{key}` must be an array of pack ids"));
            continue;
        };
        for item in items {
            match item.as_str() {
                None => bad(format!("`{key}` holds {item}, which is not a pack id string")),
                Some(name) if name == dir => bad(format!("`{key}` names the pack itself")),
                Some(name) if !others.contains(name) => {
                    bad(format!("`{key}` names `{name}`, which is no directory under packs/"));
                }
                Some(_) => {}
            }
        }
    }
    if let Some(stability) = table.get("stability") {
        match stability.as_str() {
            Some("stable") => bad(STABLE_REFUSED.to_owned()),
            Some(name) if STABILITIES.contains(&name) => {}
            _ => bad(format!("`stability` must be one of {}", STABILITIES.join(", "))),
        }
    }
    found
}

/// The pack ids a `pack.toml` names in `extends` and `needs`; whatever is malformed is `manifest_violations`'s.
fn links(text: &str) -> Vec<String> {
    let Ok(table) = text.parse::<toml::Table>() else { return Vec::new() };
    LINKS
        .iter()
        .filter_map(|key| table.get(*key)?.as_array())
        .flatten()
        .filter_map(|item| item.as_str())
        .map(str::to_owned)
        .collect()
}

/// Every loop through two or more packs in `edges` (pack id → the ids it extends or needs), as `a -> b -> a`. A pack
/// naming itself is `manifest_violations`'s.
fn cycles(edges: &BTreeMap<String, Vec<String>>) -> Vec<String> {
    let mut found = Vec::new();
    let mut done = BTreeSet::new();
    for start in edges.keys() {
        visit(start, edges, &mut Vec::new(), &mut done, &mut found);
    }
    found
}

fn visit<'a>(
    node: &'a str,
    edges: &'a BTreeMap<String, Vec<String>>,
    path: &mut Vec<&'a str>,
    done: &mut BTreeSet<&'a str>,
    found: &mut Vec<String>,
) {
    if done.contains(node) {
        return;
    }
    if let Some(at) = path.iter().position(|seen| *seen == node) {
        let mut cycle = path[at..].to_vec();
        cycle.push(node);
        found.push(format!("packs/: needs/extends loop {}", cycle.join(" -> ")));
        return;
    }
    path.push(node);
    for next in edges.get(node).into_iter().flatten().filter(|next| *next != node) {
        visit(next, edges, path, done, found);
    }
    path.pop();
    done.insert(node);
}

/// What is wrong with `packs/`: `dirs` maps each directory to the text of its `pack.toml` (`None` when it has
/// none), `files` are the stray files.
fn violations(dirs: &BTreeMap<String, Option<String>>, files: &[String]) -> Vec<String> {
    let mut found: Vec<String> =
        files.iter().map(|file| format!("packs/{file}: a file — packs/ holds only pack directories")).collect();
    if dirs.is_empty() {
        found.push("packs/: no pack directory".to_owned());
    }
    let mut edges = BTreeMap::new();
    for (dir, manifest) in dirs {
        let Some(text) = manifest else {
            found.push(format!("packs/{dir}: no pack.toml"));
            continue;
        };
        let others: BTreeSet<String> = dirs.keys().filter(|name| *name != dir).cloned().collect();
        found.extend(manifest_violations(dir, text, &others));
        edges.insert(dir.clone(), links(text));
    }
    found.extend(cycles(&edges));
    found
}

/// What is wrong with the roster of `packs/`: each `builtin` pack that has no directory in `dirs`, and each directory
/// that is none of them.
fn roster_violations(dirs: &BTreeSet<String>, builtin: &[&str]) -> Vec<String> {
    let missing =
        builtin.iter().filter(|id| !dirs.contains(**id)).map(|id| format!("packs/{id}: built-in pack missing"));
    let extra = dirs.iter().filter(|dir| !builtin.contains(&dir.as_str())).map(|dir| {
        format!("packs/{dir}: not a built-in pack — a new pack is an edit of BUILTIN that says why")
    });
    missing.chain(extra).collect()
}

/// `packs/` as it is on disk: directories with their `pack.toml` text, and the stray files.
fn packs_on_disk() -> (BTreeMap<String, Option<String>>, Vec<String>) {
    let mut dirs = BTreeMap::new();
    let mut files = Vec::new();
    for entry in fs::read_dir(root().join("packs")).expect("packs/").flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if path.is_dir() {
            let manifest = path.join("pack.toml");
            dirs.insert(name, manifest.is_file().then(|| read(&manifest)));
        } else {
            files.push(name);
        }
    }
    files.sort();
    (dirs, files)
}

#[test]
fn every_pack_stub_is_well_formed() {
    let (dirs, files) = packs_on_disk();
    assert_clean("pack stubs", &violations(&dirs, &files));
}

#[test]
fn packs_holds_the_built_in_packs_and_no_others() {
    let (dirs, _) = packs_on_disk();
    assert_clean("pack roster", &roster_violations(&dirs.into_keys().collect(), &BUILTIN));
}

const GOOD: &str = "api = 1\nid = \"demo\"\nversion = \"0.1.2\"\nsummary = \"a demo pack\"\nstability = \"beta\"\n";

fn names(list: &[&str]) -> BTreeSet<String> {
    list.iter().map(|name| (*name).to_owned()).collect()
}

fn check(text: &str) -> Vec<String> {
    manifest_violations("demo", text, &names(&["base", "agent"]))
}

/// The violations of `text` hold `needle`, or the test fails showing what was found.
fn assert_caught(text: &str, needle: &str) {
    let found = check(text);
    assert!(found.iter().any(|message| message.contains(needle)), "expected `{needle}` in {found:?} for:\n{text}");
}

#[test]
fn a_complete_manifest_passes() {
    assert_eq!(check(GOOD), Vec::<String>::new());
    let linked = format!("{GOOD}extends = [\"base\"]\nneeds = [\"base\", \"agent\"]\n");
    assert_eq!(check(&linked), Vec::<String>::new());
    assert_eq!(check(&GOOD.replace("beta", "experimental")), Vec::<String>::new());
}

#[test]
fn text_that_is_not_toml_is_refused() {
    assert_caught("api = 1\nid = \n", "not TOML");
}

#[test]
fn a_key_the_table_does_not_know_is_refused() {
    assert_caught(&format!("{GOOD}author = \"x\"\n"), "unknown key `author`");
    assert_caught(&format!("{GOOD}Needs = []\n"), "unknown key `Needs`");
}

#[test]
fn every_required_key_is_named_when_missing() {
    for key in REQUIRED {
        let without: String =
            GOOD.lines().filter(|line| !line.starts_with(key)).map(|line| format!("{line}\n")).collect();
        assert_caught(&without, &format!("missing key `{key}`"));
    }
}

#[test]
fn api_must_be_the_integer_one() {
    for api in ["api = 2", "api = 0", "api = \"1\"", "api = 1.0"] {
        assert_caught(&GOOD.replace("api = 1", api), "`api` must be the integer 1");
    }
}

#[test]
fn id_must_equal_the_directory_name() {
    assert_caught(&GOOD.replace("\"demo\"", "\"other\""), "`id` must be the string \"demo\"");
    assert_caught(&GOOD.replace("id = \"demo\"", "id = 7"), "`id` must be the string \"demo\"");
}

#[test]
fn a_version_is_three_dotted_numbers() {
    for version in ["1.0", "1.0.0.0", "1.0.x", "v1.0.0", "", "1..0", "1.0.-1", " 1.0.0"] {
        assert_caught(&GOOD.replace("0.1.2", version), "`version` must be a string like");
    }
    assert_caught(&GOOD.replace("\"0.1.2\"", "1.2"), "`version` must be a string like");
    assert!(is_dotted_version("0.0.0") && is_dotted_version("10.20.30"));
}

#[test]
fn a_summary_is_a_non_empty_string() {
    for summary in ["\"\"", "\"   \"", "3"] {
        assert_caught(&GOOD.replace("\"a demo pack\"", summary), "`summary` must be a non-empty string");
    }
}

#[test]
fn links_name_other_packs_in_an_array() {
    assert_caught(&format!("{GOOD}needs = [\"nowhere\"]\n"), "`needs` names `nowhere`, which is no directory");
    assert_caught(&format!("{GOOD}extends = [\"nowhere\"]\n"), "`extends` names `nowhere`");
    assert_caught(&format!("{GOOD}needs = \"base\"\n"), "`needs` must be an array");
    assert_caught(&format!("{GOOD}needs = [\"base\", 3]\n"), "`needs` holds 3, which is not a pack id string");
}

#[test]
fn a_pack_may_not_name_itself() {
    assert_caught(&format!("{GOOD}needs = [\"demo\"]\n"), "`needs` names the pack itself");
    assert_caught(&format!("{GOOD}extends = [\"demo\"]\n"), "`extends` names the pack itself");
}

#[test]
fn stable_is_refused_because_no_confirmed_list_exists() {
    assert_caught(&GOOD.replace("beta", "stable"), "confirmed list");
}

#[test]
fn stability_is_one_of_the_three_names() {
    for stability in ["\"alpha\"", "\"Beta\"", "\"\"", "1"] {
        assert_caught(&GOOD.replace("\"beta\"", stability), "`stability` must be one of experimental, beta, stable");
    }
}

fn graph(edges: &[(&str, &[&str])]) -> BTreeMap<String, Vec<String>> {
    edges.iter().map(|(from, to)| ((*from).to_owned(), to.iter().map(|n| (*n).to_owned()).collect())).collect()
}

#[test]
fn a_loop_of_two_or_three_packs_is_found() {
    let two = cycles(&graph(&[("a", &["b"]), ("b", &["a"])]));
    assert_eq!(two, ["packs/: needs/extends loop a -> b -> a"]);
    let three = cycles(&graph(&[("a", &["b"]), ("b", &["c"]), ("c", &["a"]), ("d", &["a"])]));
    assert_eq!(three, ["packs/: needs/extends loop a -> b -> c -> a"]);
}

#[test]
fn a_shared_dependency_is_not_a_loop() {
    let diamond = graph(&[("top", &["left", "right"]), ("left", &["base"]), ("right", &["base"]), ("base", &[])]);
    assert_eq!(cycles(&diamond), Vec::<String>::new());
}

#[test]
fn a_pack_naming_itself_is_not_also_reported_as_a_loop() {
    assert_eq!(cycles(&graph(&[("a", &["a"])])), Vec::<String>::new());
}

#[test]
fn links_are_read_from_extends_and_needs_and_nothing_else() {
    assert_eq!(links("needs = [\"a\", \"b\"]\nextends = [\"c\"]\nsummary = \"d\"\n"), ["c", "a", "b"]);
    assert_eq!(links("needs = \"a\"\nextends = [1]\n"), Vec::<String>::new());
    assert_eq!(links("not toml ="), Vec::<String>::new());
}

fn demo_dirs(manifests: &[(&str, Option<&str>)]) -> BTreeMap<String, Option<String>> {
    manifests.iter().map(|(dir, text)| ((*dir).to_owned(), text.map(str::to_owned))).collect()
}

fn pack(id: &str, links: &str) -> String {
    format!("api = 1\nid = \"{id}\"\nversion = \"0.0.0\"\nsummary = \"s\"\nstability = \"experimental\"\n{links}")
}

#[test]
fn a_set_of_good_packs_passes() {
    let dirs = demo_dirs(&[
        ("base", Some(&pack("base", "needs = []\n"))),
        ("code", Some(&pack("code", "needs = [\"base\"]\n"))),
    ]);
    assert_eq!(violations(&dirs, &[]), Vec::<String>::new());
}

#[test]
fn a_stray_file_a_missing_manifest_and_an_empty_packs_directory_are_caught() {
    let good = pack("base", "");
    let found = violations(&demo_dirs(&[("base", Some(&good)), ("empty", None)]), &["notes.md".to_owned()]);
    assert_eq!(found, ["packs/notes.md: a file — packs/ holds only pack directories", "packs/empty: no pack.toml"]);
    assert_eq!(violations(&demo_dirs(&[]), &[]), ["packs/: no pack directory"]);
}

#[test]
fn a_loop_across_manifests_is_found_and_so_is_a_bad_manifest_among_good_ones() {
    let a = pack("a", "needs = [\"b\"]\n");
    let b = pack("b", "extends = [\"a\"]\n");
    let c = pack("c", "").replace("experimental", "stable");
    let found = violations(&demo_dirs(&[("a", Some(&a)), ("b", Some(&b)), ("c", Some(&c))]), &[]);
    assert!(found.iter().any(|m| m == "packs/: needs/extends loop a -> b -> a"), "{found:?}");
    assert!(found.iter().any(|m| m.starts_with("packs/c/pack.toml: `stability = \"stable\"` is refused")), "{found:?}");
    assert_eq!(found.len(), 2, "{found:?}");
}

#[test]
fn a_roster_of_exactly_the_built_in_packs_passes() {
    assert_eq!(roster_violations(&names(&["b", "a", "c"]), &["a", "b", "c"]), Vec::<String>::new());
    assert_eq!(roster_violations(&names(&BUILTIN), &BUILTIN), Vec::<String>::new());
}

#[test]
fn a_missing_a_renamed_and_an_extra_pack_are_each_named() {
    let builtin = ["a", "b", "c"];
    assert_eq!(roster_violations(&names(&["a", "c"]), &builtin), ["packs/b: built-in pack missing"]);
    let renamed = roster_violations(&names(&["a", "bee", "c"]), &builtin);
    assert_eq!(renamed.len(), 2, "{renamed:?}");
    assert_eq!(renamed[0], "packs/b: built-in pack missing");
    assert!(renamed[1].starts_with("packs/bee: not a built-in pack"), "{renamed:?}");
    let extra = roster_violations(&names(&["a", "b", "c", "my pack"]), &builtin);
    assert_eq!(extra.len(), 1, "{extra:?}");
    assert!(extra[0].starts_with("packs/my pack: not a built-in pack"), "{extra:?}");
}

#[test]
fn no_pack_directory_at_all_misses_every_built_in_pack() {
    assert_eq!(roster_violations(&names(&[]), &["a", "b"]).len(), 2);
}
