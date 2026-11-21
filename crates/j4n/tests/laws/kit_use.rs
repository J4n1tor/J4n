//! The kit use through the binary's own wiring: the fold `kit_use` counts what the cells of
//! the Log used of the kit's library, the rule `kit-part-unused` puts a part that no cell used over the last 100 cells (in 5 tasks) to the
//! operator as a Proposal, and the operator's yes writes the part into `kit.off`, which from the next start refuses it in cells and leaves it
//! out of the card, `help(j4n)` and `j4n.recipes()`. With `kit.mode` off nothing is folded and nothing is proposed.

use crate::models_cases::mock::{Mock, Say, Seen};
use crate::run_cases::Dirs;
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use j4n_core::change::Actor;
use j4n_core::front::Said;
use j4n_core::log::New;
use j4n_core::task::{Mode, State};
use j4n_core::value::{Key, Value};
use serde_json::{Value as Json, json};
use std::fs;
use std::time::{Duration, Instant};

fn opts(doer: &str) -> Opts {
    Opts { doer: Some(doer.into()), model: None, approval: Some(Approval::Full), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false }
}

/// A world with the setting file `settings/kit.toml` of the operator's folder holding `rows` (`id = value` lines).
fn open_with(d: &Dirs, doer: &str, rows: &[(&str, &str)], gateway: Option<&Mock>) -> World {
    let mut text = String::from("api = 1\n[[setting]]\nid = \"loop.followups\"\nvalue = 0\n");
    for (id, value) in rows {
        text.push_str(&format!("[[setting]]\nid = \"{id}\"\nvalue = {value}\n"));
    }
    fs::write(d.j4n_home().join("settings/kit.toml"), text).unwrap();
    open_kept(d, doer, gateway)
}

/// A world over the operator's folder as it stands (the own agent's connector is written again for the gateway).
fn open_kept(d: &Dirs, doer: &str, gateway: Option<&Mock>) -> World {
    if let Some(g) = gateway {
        let key = d.dir.join("key");
        crate::support::key_file(&key, "sk-test-own-0001\n");
        fs::create_dir_all(d.j4n_home().join("connectors")).unwrap();
        let mine = format!("api = 1\n[[connector]]\nid = \"own\"\nurl = \"{}/v1\"\nkey = \"file:{}\"\nmodels = [\"vendor/m\"]\n", g.url(), key.display());
        fs::write(d.j4n_home().join("connectors/mine.toml"), mine).unwrap();
    }
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: "claude".into() };
    let w = crate::support::opened(|| wire::open(setup.clone(), &opts(doer), true, "main"));
    assert!(w.notices.is_empty(), "{:?}", w.notices);
    w
}

fn dump(w: &World) -> String {
    w.front.log().sync();
    w.front.log().read(0).map(|e| format!("{:>4} {:<16} {} {}", e.seq, e.kind, e.task.as_deref().unwrap_or("-"), e.data)).collect::<Vec<_>>().join("\n")
}

fn kinds(w: &World, kind: &str) -> Vec<Json> {
    w.front.log().sync();
    w.front.log().read(0).filter(|e| e.kind == kind).map(|e| e.data).collect()
}

/// One cell as the tool `py` writes it, in `task`, saying what it used (`None`: a cell from before the kit counted use).
fn cell(w: &World, task: &str, used: Option<Json>, ok: bool) {
    let mut data = json!({ "head": "x", "hash": "0", "risk": "read", "ok": ok });
    if let Some(u) = used {
        data["used"] = u;
    }
    w.front.log().append(New::of("py.cell", Actor::Core, data).task(task)).unwrap();
}

fn fold(w: &World, path: &[&str]) -> Value {
    w.front.log().sync();
    let path: Vec<Key> = path.iter().map(|p| Key::Name((*p).into())).collect();
    w.front.screen().folds.as_ref().expect("the folds").read("kit_use", &path, 0)
}

fn n(w: &World, path: &[&str]) -> f64 {
    match fold(w, path) {
        Value::Num(x) => x,
        other => panic!("{path:?} is {other:?}\n{}", dump(w)),
    }
}

/// The files of the operator's folder that hold `kit.off`.
fn kept_files(d: &Dirs) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let mut todo = vec![d.j4n_home()];
    while let Some(dir) = todo.pop() {
        for e in fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = e.path();
            if path.is_dir() {
                todo.push(path);
            } else if fs::read_to_string(&path).is_ok_and(|t| t.contains("kit.off")) {
                found.push(path);
            }
        }
    }
    found
}

fn held(d: &Dirs) -> String {
    kept_files(d).iter().map(|f| fs::read_to_string(f).unwrap_or_default()).collect()
}

/// A task of the scripted doer that ends by itself; the rules have had their say when this returns.
fn settle(w: &World) {
    let Said::Started { task } = w.front.say("main", "say a") else { panic!("did not start\n{}", dump(w)) };
    w.front.tasks().book().wait(&task, |r| r.state.is_terminal(), crate::support::PATIENCE).unwrap_or_else(|| panic!("{task} never ended\n{}", dump(w)));
    w.front.tasks().sync().unwrap();
    w.front.log().sync();
}

fn proposals(w: &World) -> Vec<String> {
    kinds(w, "proposal.made").iter().map(|d| d["draft"]["key"].as_str().unwrap_or_default().to_string()).collect()
}

/// `count` cells over `tasks` tasks, round and round, each using `used`.
fn cells(w: &World, count: usize, tasks: usize, used: Json) {
    for i in 0..count {
        cell(w, &format!("k{}", i % tasks), Some(used.clone()), true);
    }
}

#[test]
fn the_fold_counts_uses_tasks_and_failed_cells_per_part_and_skips_a_cell_that_does_not_say() {
    let d = Dirs::new("kituse-counts");
    let w = open_with(&d, "test", &[("kit.mode", "\"on\"")], None);
    cell(&w, "k1", Some(json!({ "chunks": 2, "grep": 1 })), true);
    cell(&w, "k1", Some(json!({ "chunks": 1 })), false);
    cell(&w, "k2", Some(json!({ "grep": 1, "recipe": 1, "recipe:each": 1 })), true);
    cell(&w, "k3", Some(json!({})), true);
    cell(&w, "k3", None, true);
    assert_eq!((n(&w, &["cells"]), n(&w, &["tasks"])), (4.0, 3.0), "a cell from before the kit counted use is no evidence\n{}", dump(&w));
    let part = |p: &str, k: &str| n(&w, &["parts", p, k]);
    assert_eq!([part("chunks", "uses"), part("chunks", "cells"), part("chunks", "tasks"), part("chunks", "fails")], [3.0, 2.0, 1.0, 1.0]);
    assert_eq!([part("grep", "uses"), part("grep", "tasks"), part("grep", "fails")], [2.0, 2.0, 0.0]);
    assert_eq!([part("recipe:each", "uses"), part("recipe", "uses"), part("table", "uses")], [1.0, 1.0, 0.0]);
    assert_eq!(part("table", "idle"), 4.0, "never used: idle for every cell");
    assert_eq!(part("grep", "idle"), 1.0, "used in the third of four cells: one cell ago");
    assert!(matches!(fold(&w, &["next"]), Value::Missing), "four cells are far from a window");
    w.close();
}

#[test]
fn with_kit_off_nothing_is_folded_and_nothing_is_proposed() {
    let d = Dirs::new("kituse-off");
    let w = open_with(&d, "test", &[], None);
    assert!(matches!(fold(&w, &[]), Value::Missing), "no fold with the kit off");
    cells(&w, 130, 8, json!({ "chunks": 1 }));
    settle(&w);
    assert!(proposals(&w).is_empty(), "{}", dump(&w));
    // the same cells with the kit on: the only difference is the switch
    let on = Dirs::new("kituse-off-on");
    let w2 = open_with(&on, "test", &[("kit.mode", "\"on\"")], None);
    cells(&w2, 130, 8, json!({ "chunks": 1 }));
    settle(&w2);
    assert_eq!(proposals(&w2), ["kit-off:table"], "{}", dump(&w2));
    w.close();
    w2.close();
}

#[test]
fn a_part_is_proposed_only_past_the_window_and_the_tasks_and_once() {
    let d = Dirs::new("kituse-window");
    let w = open_with(&d, "test", &[("kit.mode", "\"on\"")], None);
    // 99 cells in 8 tasks: one short of the window
    cells(&w, 99, 8, json!({ "chunks": 1 }));
    settle(&w);
    assert!(proposals(&w).is_empty(), "99 cells are not 100\n{}", dump(&w));
    // the 100th: `table` is the first part nobody used; `chunks` was used in the last cell
    cell(&w, "k0", Some(json!({ "chunks": 1 })), true);
    settle(&w);
    assert_eq!(proposals(&w), ["kit-off:table"], "{}", dump(&w));
    let made = &kinds(&w, "proposal.made")[0];
    let (title, why) = (made["draft"]["title"].as_str().unwrap(), made["draft"]["why"].as_str().unwrap());
    assert!(title.contains("table"), "{title}");
    assert!(why.contains("100 cells") && why.contains("0 uses in 0 tasks") && why.contains("nothing is deleted"), "{why}");
    assert_eq!(made["draft"]["steps"][0]["id"], "kit.off", "{made}");
    // asked once: another task's end proposes the next part, never the same one again
    settle(&w);
    assert_eq!(proposals(&w), ["kit-off:table", "kit-off:grep"], "{}", dump(&w));
    // a part that is in use is not due
    let used = Dirs::new("kituse-used");
    let w2 = open_with(&used, "test", &[("kit.mode", "\"on\"")], None);
    let all = json!({ "chunks": 1, "table": 1, "grep": 1, "dedup": 1, "heads": 1, "recipes": 1, "recipe": 1, "judge": 1, "choose": 1, "score": 1, "keep": 1, "pick": 1, "model": 1, "tools": 1, "call": 1, "many": 1, "fetch": 1, "need": 1, "recipe:checks": 1, "recipe:column_counts": 1, "recipe:each": 1, "recipe:matching": 1, "recipe:near_duplicates": 1, "recipe:outline": 1 });
    cells(&w2, 120, 8, all);
    settle(&w2);
    assert!(proposals(&w2).is_empty(), "every part was used in the window\n{}", dump(&w2));
    // four tasks are too few to say a part is not used
    let few = Dirs::new("kituse-few");
    let w3 = open_with(&few, "test", &[("kit.mode", "\"on\"")], None);
    cells(&w3, 150, 4, json!({ "chunks": 1 }));
    settle(&w3);
    assert!(proposals(&w3).is_empty(), "{}", dump(&w3));
    // a part that is off already is not asked about again
    let off = Dirs::new("kituse-offalready");
    let w4 = open_with(&off, "test", &[("kit.mode", "\"on\""), ("kit.off", "\"table grep\"")], None);
    cells(&w4, 100, 8, json!({ "chunks": 1 }));
    settle(&w4);
    assert_eq!(proposals(&w4), ["kit-off:dedup"], "{}", dump(&w4));
    for w in [w, w2, w3, w4] {
        w.close();
    }
}

// ---- the yes, and what it turns off ----

fn calls(list: &[(&str, &str, Json)]) -> Say {
    let frags: Vec<Json> = list.iter().enumerate().map(|(i, (id, name, args))| json!({ "index": i, "id": id, "function": { "name": name, "arguments": args.to_string() } })).collect();
    Say::Sse(vec![
        json!({ "choices": [{ "delta": { "tool_calls": frags } }] }).to_string(),
        json!({ "choices": [{ "delta": {}, "finish_reason": "tool_calls" }] }).to_string(),
        json!({ "choices": [], "usage": { "prompt_tokens": 100, "completion_tokens": 10 } }).to_string(),
    ])
}

fn answer(text: &str) -> Say {
    Say::Sse(vec![
        json!({ "choices": [{ "delta": { "content": text } }] }).to_string(),
        json!({ "choices": [{ "delta": {}, "finish_reason": "stop" }] }).to_string(),
        json!({ "choices": [], "usage": { "prompt_tokens": 100, "completion_tokens": 10 } }).to_string(),
    ])
}

fn system_of(s: &Seen) -> String {
    s.json()["messages"].as_array().unwrap().iter().filter(|m| m["role"] == "system").map(|m| m["content"].as_str().unwrap_or_default().to_string()).collect::<Vec<_>>().join("\n")
}

fn tool_results(s: &Seen) -> Vec<String> {
    s.json()["messages"].as_array().cloned().unwrap_or_default().iter().filter(|m| m["role"] == "tool").map(|m| m["content"].as_str().unwrap_or_default().to_string()).collect()
}

fn line(w: &World, text: &str) {
    let Said::Started { task } = w.front.say("main", text) else { panic!("{text:?} did not start a task") };
    let row = w.front.tasks().book().wait(&task, |r| r.state.is_terminal(), crate::support::PATIENCE).unwrap_or_else(|| panic!("{task} never ended\n{}", dump(w)));
    assert!(matches!(row.state, State::Done { ok: true }), "{:?}\n{}", row.state, dump(w));
}

#[test]
fn the_operators_yes_turns_a_part_off_from_the_next_start_in_the_card_in_help_and_in_cells_and_taking_it_out_turns_it_on() {
    if !j4n_connect::py::python_here() {
        return;
    }
    let d = Dirs::new("kituse-accept");
    let gateway = Mock::start(|_, _| answer("done"));
    let w = open_with(&d, "own", &[("kit.mode", "\"on\"")], Some(&gateway));
    cells(&w, 100, 8, json!({ "chunks": 1 }));
    line(&w, "hi");
    // a rule answers the end of the task a moment after the task is over
    let end = Instant::now() + crate::support::PATIENCE;
    while proposals(&w).is_empty() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(proposals(&w), ["kit-off:table"], "{}", dump(&w));
    // a proposal changes nothing until the yes
    assert!(!held(&d).contains("table"), "{}", held(&d));
    assert!(matches!(w.front.say("main", ":accept"), Said::Command(_)), "{}", dump(&w));
    let file = held(&d);
    assert!(file.contains("kit.off") && file.contains("table"), "the yes writes the setting: {file:?}\n{}", dump(&w));
    w.close();
    drop(w);
    // started again: the part is off
    let gateway = Mock::start(|_, n| match n {
        0 => calls(&[
            ("c1", "py", json!({ "code": "import j4n\nj4n.table('a,b\\n1,2')" })),
            ("c2", "py", json!({ "code": "import j4n\nj4n.grep(['a'], 'a')" })),
            ("c3", "py", json!({ "code": "import j4n\nprint('table(' in (j4n.__doc__ or ''), 'table' in j4n.__all__, 'grep(' in j4n.__doc__)" })),
        ]),
        _ => answer("done"),
    });
    let w = open_kept(&d, "own", Some(&gateway));
    line(&w, "try the library");
    let seen = gateway.seen();
    let system = system_of(&seen[0]);
    assert!(system.contains("import j4n") && system.contains("a cell that calls them is refused: `j4n.table`."), "{system}");
    let r = tool_results(&seen[1]);
    assert!(r[0].contains("Refused") && r[0].contains("kit.off"), "a cell that calls the part is refused: {r:?}");
    assert!(!r[1].contains("error"), "the others work: {r:?}");
    assert!(r[2].contains("False False True"), "help(j4n) and __all__ leave it out: {r:?}");
    assert!(w.front.log().read(0).filter(|e| e.kind == "py.cell").all(|e| e.data["used"].get("table").is_none()), "a refused call is no use");
    w.close();
    drop(w);
    // taking the name out turns it on: nothing was deleted
    let file = d.j4n_home().join("settings/kit.toml");
    let text = fs::read_to_string(&file).unwrap();
    assert!(text.contains("kit.mode"), "{text}");
    for f in kept_files(&d) {
        fs::write(&f, fs::read_to_string(&f).unwrap().replace("table", "")).unwrap();
    }
    let gateway = Mock::start(|_, n| if n == 0 { calls(&[("c1", "py", json!({ "code": "import j4n\nj4n.table('a,b\\n1,2')" }))]) } else { answer("done") });
    let w = open_kept(&d, "own", Some(&gateway));
    line(&w, "again");
    let seen = gateway.seen();
    assert!(!system_of(&seen[0]).contains("Turned off"), "{}", system_of(&seen[0]));
    assert!(!tool_results(&seen[1])[0].contains("Refused"), "{:?}", tool_results(&seen[1]));
    w.close();
}

#[test]
fn the_pack_the_binary_carries_has_the_need_gate_and_the_rule_and_every_file_of_the_pack_is_listed() {
    let carried: Vec<&str> = j4n::pack::KIT.iter().map(|f| f.0).collect();
    for want in ["gates/need.toml", "rules/kit.toml", "settings/kit.toml"] {
        assert!(carried.contains(&want), "{want} is not in the files of the pack kit the binary carries: {carried:?}");
    }
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/kit");
    let mut on_disk = Vec::new();
    for sub in fs::read_dir(&dir).unwrap().flatten().filter(|e| e.path().is_dir()) {
        let name = sub.file_name().to_string_lossy().to_string();
        for f in fs::read_dir(sub.path()).unwrap().flatten().filter(|f| f.path().extension().is_some_and(|x| x == "toml")) {
            on_disk.push(format!("{name}/{}", f.file_name().to_string_lossy()));
        }
    }
    for f in on_disk {
        assert!(carried.contains(&f.as_str()), "{f} is in packs/kit but not in the list the binary carries");
    }
}

#[test]
fn a_cell_that_forges_its_account_leaves_the_fold_bounded_and_unknown_names_out() {
    let d = Dirs::new("kituse-forged");
    let w = open_with(&d, "test", &[("kit.mode", "\"on\"")], None);
    let mut forged = serde_json::Map::new();
    forged.insert("chunks".into(), json!(u64::MAX));
    forged.insert("grep".into(), json!(u64::MAX));
    forged.insert("evil".into(), json!(7));
    forged.insert("recipe:nope".into(), json!(1));
    for i in 0..5000 {
        forged.insert(format!("k{i}"), json!(1));
    }
    cell(&w, "k1", Some(Json::Object(forged.clone())), true);
    cell(&w, "k1", Some(Json::Object(forged)), true);
    let Value::Map(parts) = fold(&w, &["parts"]) else { panic!("{}", dump(&w)) };
    assert_eq!(parts.len(), 18 + 6, "only the known parts: {}", parts.len());
    assert!(!parts.contains_key("evil") && !parts.contains_key("k0"));
    assert_eq!(n(&w, &["parts", "chunks", "uses"]), 200_000.0, "two cells, each clamped to 100,000");
    assert_eq!(n(&w, &["parts", "chunks", "cells"]), 2.0);
    w.close();
}
