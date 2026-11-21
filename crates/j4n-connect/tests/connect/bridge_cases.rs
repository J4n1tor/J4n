//! Code in the `py` kernel reaches the tools of the doer's offer
//! through one route. The kernel is the real one; the tools are a scripted set that answers the Gate as the real ones do (and, for the
//! taint, the real Gate with the pack `guard`'s rules); nothing leaves the machine and no live model takes part. What is held is
//! the Gate per call (Allow runs, Ask and Block are refused), the taint, the leak walk, the caps of 200 and 8, and the row each call leaves.

use crate::bench::base_layer;
use crate::world::Scratch;
use j4n_connect::kithost::{Host, KitSet};
use j4n_connect::manifest::Connectors;
use j4n_connect::model::doer::{Gated, ToolOut, Tools, action};
use j4n_connect::model::{Call, Tool};
use j4n_connect::py::PyTools;
use j4n_connect::tools::Builtin;
use j4n_core::gate::check::Gates;
use j4n_core::gate::grant::Grants;
use j4n_core::gate::{Gate, ReadOnly, Verb, Verdict};
use j4n_core::place::{Launch, Site};
use j4n_core::redact::Redactor;
use j4n_core::table::{Layer, LayerKind, SourceFile, Tables};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Shaped like a GitHub token, made up for the test.
const SHAPED: &str = "ghp_aB3dE6gH9jK2mN5pQ8sT1vW4yZ7bC0eF3hJ6";
const TAINT: &str = include_str!("../../../../packs/guard/gates/taint.toml");

fn set(red: &Redactor) -> KitSet {
    KitSet {
        judge: false,
        keep: 0.15,
        model: "light".into(),
        red: red.clone(),
        egress: Arc::new(|_| Err("no egress".into())),
        ..KitSet::default()
    }
}

fn gate(taint: bool) -> Gate {
    let mut layers = vec![base_layer()];
    if taint {
        layers.push(Layer::new(
            LayerKind::Pack,
            "guard",
            vec![SourceFile { path: "taint.toml".into(), text: TAINT.into() }],
        ));
    }
    let (tables, _) = Tables::load(layers);
    Gate::new(Gates::load(&tables).0, Grants::new("s1"))
}

/// The tools of the scripted offer: `echo` (allowed), `ask_me` (the Gate asks until `granted`), `no` (blocked), `boom` (fails), `put`
/// (a write, as the real Gate sees it), `page` (a page: untrusted), `nap` (takes 30 ms and counts how many run together).
struct Script {
    gate: Gate,
    gated: AtomicUsize,
    ran: Mutex<Vec<String>>,
    now: AtomicUsize,
    peak: AtomicUsize,
    granted: AtomicBool,
    nap: Duration,
}

impl Script {
    fn new(gate: Gate) -> Arc<Script> {
        Arc::new(Script {
            gate,
            gated: AtomicUsize::new(0),
            ran: Mutex::default(),
            now: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            granted: AtomicBool::new(false),
            nap: Duration::from_millis(30),
        })
    }

    fn ran(&self) -> Vec<String> {
        self.ran.lock().unwrap().clone()
    }
}

fn text(call: &Call, k: &str) -> String {
    call.args.get(k).and_then(Value::as_str).unwrap_or_default().to_string()
}

impl Tools for Script {
    fn specs(&self) -> Vec<Tool> {
        ["echo", "ask_me", "no", "boom", "put", "page", "nap"]
            .map(|n| Tool {
                name: n.into(),
                doc: format!("the tool {n}"),
                schema: json!({ "type": "object", "properties": { "text": { "type": "string" } } }),
            })
            .to_vec()
    }

    fn untrusted(&self, call: &Call) -> Option<String> {
        (call.name == "page").then(|| "web".to_string())
    }

    fn gate(&self, call: &Call, task: &str, site: &Site, ro: ReadOnly) -> Option<Gated> {
        self.gated.fetch_add(1, Ordering::SeqCst);
        let (who, verb, target) = match call.name.as_str() {
            "ask_me" if !self.granted.load(Ordering::SeqCst) => {
                return Some(Gated {
                    action: action("t", Verb::Net, "x", task, site, ro),
                    question: "go to x".into(),
                    risk: 40,
                });
            }
            "no" => {
                return Some(Gated {
                    action: action("t", Verb::Net, "x", task, site, ro),
                    question: "do the forbidden".into(),
                    risk: 100,
                });
            }
            "put" => ("builtin", Verb::Write, text(call, "path")),
            "page" => ("web", Verb::Net, text(call, "url")),
            _ => return None,
        };
        let mut a = action(who, verb, &target, task, site, ro);
        a.inside = Some(true);
        match self.gate.check(&a) {
            Verdict::Allow { .. } => None,
            Verdict::Ask { risk, .. } => Some(Gated { action: a, question: format!("{} {target}", call.name), risk }),
            Verdict::Block { .. } => Some(Gated { action: a, question: format!("{} {target}", call.name), risk: 100 }),
        }
    }

    fn run(&self, call: &Call, _: &Site, _: &(dyn Fn(Launch) -> Launch + Send + Sync)) -> ToolOut {
        self.ran.lock().unwrap().push(format!("{} {}", call.name, call.args));
        if call.name == "nap" {
            let n = self.now.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(n, Ordering::SeqCst);
            std::thread::sleep(self.nap);
            self.now.fetch_sub(1, Ordering::SeqCst);
        }
        match call.name.as_str() {
            "boom" => ToolOut { text: "it broke".into(), error: true, ..ToolOut::default() },
            "put" => ToolOut { text: "written".into(), changed: vec![text(call, "path")], ..ToolOut::default() },
            _ => ToolOut { text: format!("{}:{}", call.name, text(call, "text")), ..ToolOut::default() },
        }
    }
}

fn tools(host: Host) -> PyTools {
    PyTools::new().with_host(Arc::new(host))
}

fn cell(t: &PyTools, site: &Site, code: &str) -> ToolOut {
    run_with(t, site, json!({ "code": code }))
}

fn run_with(t: &PyTools, site: &Site, args: Value) -> ToolOut {
    t.run(&Call { id: "c1".into(), name: "py".into(), args, bad: None }, site, &|l: Launch| l)
}

fn value(out: &ToolOut) -> String {
    out.text.lines().find_map(|l| l.strip_prefix("=> ")).unwrap_or_default().to_string()
}

fn rows(out: &ToolOut) -> Vec<Value> {
    out.py.as_ref().and_then(|p| p["host"].as_array().cloned()).unwrap_or_default()
}

fn host_of(script: &Arc<Script>, red: &Redactor) -> Host {
    Host::new(set(red)).with_offer(Some(script.clone()))
}

#[test]
fn tools_lists_the_offer_and_a_command_row_added_to_the_rows_is_in_it_with_no_other_change() {
    let dir = Scratch::new("br473");
    let site = Site::new("t1", dir.path());
    let base = r#"
[[connector]]
id = "builtin"
doc = "The tools J4n carries."
is = "tool"
dialect = "builtin"

[[connector]]
id = "own"
doc = "A model."
is = "agent"
dialect = "openai"
url = "http://127.0.0.1:1/v1"
keyless = true
tools = ["builtin", "*", "ask", "py"]
models = ["m"]

[[connector]]
id = "claude"
doc = "A program with a shell of its own, naming no tools."
is = "agent"
dialect = "claude-stream"
program = "claude"
"#;
    let one = r#"
[[connector]]
id = "say"
doc = "Say a word."
is = "tool"
dialect = "text"
program = "echo"
args = [{ name = "word", required = true }]
argv = ["echo", "{word}"]
risk = "read"
"#;
    let two = one.replace("id = \"say\"", "id = \"shout\"").replace("Say a word.", "Say it loudly.");
    let load = |extra: &str| {
        let user = Layer::new(
            LayerKind::User,
            "mine",
            vec![SourceFile { path: "connectors/x.toml".into(), text: format!("api = 1\n{base}{extra}") }],
        );
        let (tables, problems) = Tables::load(vec![base_layer(), user]);
        assert!(problems.is_empty(), "{problems:?}");
        let (list, said) = Connectors::load(&tables);
        assert!(said.is_empty(), "{said:?}");
        list
    };
    let secret = |_: &str, _: &str| Err::<String, String>("no secret".into());
    let listed = |list: &Connectors, who: &str| {
        let offer = list.bridge_offer(list.get(who).expect("the row"), &secret);
        let t = tools(Host::new(set(&Redactor::new())).with_offer(Some(offer)));
        let out = cell(&t, &site, "import j4n\nsorted(j4n.tools())");
        assert!(!out.error, "{}", out.text);
        (value(&out), t)
    };
    let (before, _) = listed(&load(one), "own");
    for builtin in ["list", "read", "grep", "edit", "write", "run", "git", "say"] {
        assert!(before.contains(&format!("'{builtin}'")), "{builtin} in {before}");
    }
    for kept in ["py", "ask", "mcp"] {
        assert!(!before.contains(&format!("'{kept}'")), "{kept} is kept from code: {before}");
    }
    let (after, t) = listed(&load(&format!("{one}{two}")), "own");
    assert_eq!(
        after,
        before.replace("'say'", "'say', 'shout'"),
        "one more [[command]] row is one more tool, and the host and the library are the same"
    );
    // the kernel is the one of the cell above: the two kept names are refused by name, with the list of what there is
    for kept in ["py", "ask"] {
        let out = cell(&t, &site, &format!("import j4n\nj4n.call({kept:?})"));
        assert!(
            out.error && out.text.contains("Refused") && out.text.contains("is not a tool that code may call"),
            "{}",
            out.text
        );
    }
    // a program that names no tools has what the models of the rows have
    let (cli, _) = listed(&load(one), "claude");
    assert_eq!(cli, before);
}

#[test]
fn call_read_is_the_models_text_one_gate_check_one_row_and_a_file_it_changes_is_reported() {
    let dir = Scratch::new("br474");
    dir.put("a.txt", "one\ntwo\n");
    let site = Site::new("t1", dir.path());
    let script = Script::new(gate(false));
    let t = tools(Host::new(set(&Redactor::new())).with_offer(Some(Arc::new(Builtin::new()))));
    let model = Builtin::new().run(
        &Call { id: "m".into(), name: "read".into(), args: json!({ "path": "a.txt" }), bad: None },
        &site,
        &|l| l,
    );
    let same = cell(
        &t,
        &site,
        &format!("import j4n\nj4n.call('read', path='a.txt') == {}", serde_json::to_string(&model.text).unwrap()),
    );
    assert!(!same.error && value(&same) == "True", "{}", same.text);
    let got = same;
    let r = rows(&got);
    assert_eq!(r.len(), 1, "{r:?}");
    assert_eq!(
        (r[0]["call"].as_str(), r[0]["tool"].as_str(), r[0]["ok"].as_bool()),
        (Some("tool"), Some("read"), Some(true))
    );
    assert!(
        r[0]["args"].as_str().is_some_and(|d| d.len() == 12) && !r[0].to_string().contains("a.txt"),
        "a digest of the arguments, never the arguments: {}",
        r[0]
    );
    // one Gate question for one call, and a call that changed a file says so (the loop's list of changed paths)
    let t = tools(host_of(&script, &Redactor::new()));
    let put = cell(&t, &site, "import j4n\nj4n.call('echo', text='a')\nj4n.call('put', path='src/lib.rs')");
    assert!(!put.error, "{}", put.text);
    assert_eq!(script.gated.load(Ordering::SeqCst), 2, "one question for each call");
    assert_eq!(put.changed, ["src/lib.rs"]);
    assert_eq!(rows(&put).len(), 2);
    assert!(put.py.unwrap()["calls"].as_str().unwrap().contains("tool echo"), "the Kit page's line names the tool");
}

#[test]
fn ask_and_block_are_refused_with_the_question_a_read_only_task_may_not_write_and_a_grant_makes_it_a_run() {
    let dir = Scratch::new("br475");
    let site = Site::new("t1", dir.path());
    let script = Script::new(gate(false));
    let t = tools(host_of(&script, &Redactor::new()));
    let ask = cell(&t, &site, "import j4n\nj4n.call('ask_me')");
    assert!(
        ask.error && ask.text.contains("Refused") && ask.text.contains("asks before") && ask.text.contains("go to x"),
        "{}",
        ask.text
    );
    let block = cell(&t, &site, "import j4n\nj4n.call('no')");
    assert!(
        block.error
            && block.text.contains("Refused")
            && block.text.contains("blocks")
            && block.text.contains("do the forbidden"),
        "{}",
        block.text
    );
    assert!(script.ran().is_empty(), "nothing ran");
    let r = rows(&block);
    assert_eq!((r[0]["ok"].as_bool(), r[0]["refused"].as_bool()), (Some(false), Some(true)));
    // a refused call in a loop with errors="keep" leaves the others to run
    let kept =
        cell(&t, &site, "import j4n\nr = j4n.many('ask_me', [{}, {}], errors='keep')\n[type(x).__name__ for x in r]");
    assert_eq!(value(&kept), "['Refused', 'Refused']", "{}", kept.text);
    // the task is read-only: the Gate's own rule blocks the write, as it does a model's
    let py = Call { id: "g".into(), name: "py".into(), args: json!({ "code": "1" }), bad: None };
    t.gate(&py, "t1", &site, ReadOnly::Explicit);
    let put = cell(&t, &site, "import j4n\nj4n.call('put', path='x.txt')");
    assert!(
        put.error && put.text.contains("Refused") && put.text.contains("blocks") && put.text.contains("put x.txt"),
        "{}",
        put.text
    );
    assert!(put.changed.is_empty() && !script.ran().iter().any(|r| r.starts_with("put")), "and nothing was written");
    // a task that is not read-only may; a grant ahead of time makes what asked a run
    t.gate(&py, "t1", &site, ReadOnly::No);
    assert!(!cell(&t, &site, "import j4n\nj4n.call('put', path='x.txt')").error);
    script.granted.store(true, Ordering::SeqCst);
    assert_eq!(value(&cell(&t, &site, "import j4n\nj4n.call('ask_me', text='now')")), "'ask_me:now'");
}

#[test]
fn a_read_only_task_runs_a_bridge_cell_and_the_gate_per_call_refuses_the_write() {
    let dir = Scratch::new("br983");
    let site = Site::new("t1", dir.path());
    let script = Script::new(gate(false));
    let (tables, _) = Tables::load(vec![base_layer()]);
    let real = Gate::new(Gates::load(&tables).0, Grants::new("s1"));
    let t = PyTools::new().with_gate(real).with_host(Arc::new(host_of(&script, &Redactor::new())));
    let py = |code: &str| Call { id: "g".into(), name: "py".into(), args: json!({ "code": code }), bad: None };
    let looped = "import j4n\nr = [j4n.call('echo', text=str(i)) for i in range(3)]\nr";
    let many = "import j4n\nj4n.many('echo', [{'text': 'a'}, {'text': 'b'}])";
    let write = "import j4n\nj4n.call('put', path='x.txt')";
    // the cell is a read for the Gate (each call inside is asked on its own), in a task that may only read
    for code in [looped, many, write] {
        assert!(t.gate(&py(code), "t1", &site, ReadOnly::Explicit).is_none(), "a bridge cell is a read: {code}");
    }
    let out = cell(&t, &site, looped);
    assert_eq!(value(&out), "['echo:0', 'echo:1', 'echo:2']", "{}", out.text);
    // the write is the doer's model call: the Gate blocks it and nothing is written
    let put = cell(&t, &site, write);
    assert!(put.error && put.text.contains("Refused") && put.text.contains("put x.txt"), "{}", put.text);
    assert!(put.changed.is_empty() && !script.ran().iter().any(|r| r.starts_with("put")), "nothing was written");
    // what else reaches out from a cell stays exec: another `call`, a host call, a fetch
    for code in [
        "import subprocess\nsubprocess.call(['true'])",
        "import j4n\nj4n.host('need', name='x')",
        "import j4n\nj4n.fetch('x')",
        "from j4n import call\ncall('echo')",
        "import j4n\nx = j4n\nx.call('echo')",
        "import j4n\nobj.j4n.call('echo')",
    ] {
        assert!(t.gate(&py(code), "t1", &site, ReadOnly::Explicit).is_some(), "still exec: {code}");
    }
}

#[test]
fn the_recipe_each_over_a_file_of_10_lines_makes_10_calls_in_one_many_and_counts_the_ok_and_the_failed() {
    let dir = Scratch::new("br481");
    dir.put("items.txt", &(0..10).map(|i| format!("item-{i}\n")).collect::<String>());
    let site = Site::new("t1", dir.path());
    let script = Script::new(gate(false));
    let t = tools(host_of(&script, &Redactor::new()));
    let out = cell(
        &t,
        &site,
        "import j4n\nr = j4n.recipe('each', tool='echo', items='items.txt')\n(r['ok'], r['failed'], r['results'][3])",
    );
    assert_eq!(value(&out), "(10, 0, 'echo:item-3')", "{}", out.text);
    assert_eq!(script.ran().len(), 10, "ten calls");
    assert_eq!(rows(&out).len(), 11, "a row for each call, and one for the `tools` that found the argument's name");
    // a Gate that asks, or a tool that fails, is a failure in the count and the rest still run
    let ask = cell(
        &t,
        &site,
        "import j4n\nr = j4n.recipe('each', tool='ask_me', items='items.txt')\n(r['ok'], r['failed'], 'asks before' in r['failures'][0][1])",
    );
    assert_eq!(value(&ask), "(0, 10, True)", "{}", ask.text);
    let boom = cell(
        &t,
        &site,
        "import j4n\nr = j4n.recipe('each', tool='boom', items=['a', 'b'])\n(r['ok'], r['failed'], r['failures'][0])",
    );
    assert_eq!(value(&boom), "(0, 2, ['a', 'it broke'])", "{}", boom.text);
}

#[test]
fn a_page_taints_the_task_and_a_later_write_from_the_same_cell_is_held_as_a_models_is() {
    let dir = Scratch::new("br476");
    let site = Site::new("t1", dir.path());
    let g = gate(true);
    let script = Script::new(g.clone());
    let t = tools(host_of(&script, &Redactor::new()).with_guard(Some(g.clone())));
    let code = "import j4n\nj4n.call('page', url='https://docs.test/a')\nr = j4n.many('put', [{'path': 'src/lib.rs'}, {'path': '.env'}], errors='keep')\n[type(x).__name__ if not isinstance(x, str) else x for x in r]";
    let out = cell(&t, &site, code);
    assert_eq!(value(&out), "['written', 'Refused']", "{}", out.text);
    assert_eq!(
        script.ran(),
        [r#"page {"url":"https://docs.test/a"}"#, r#"put {"path":"src/lib.rs"}"#],
        "the secret file was not written"
    );
    assert_eq!(g.tainted("t1").as_deref(), Some("web"));
    assert!(out.text.contains("has read from web"), "the model is told, once: {}", out.text);
    let r = rows(&out);
    assert_eq!((r[0]["untrusted"].as_str(), r[0]["taint"].as_bool()), (Some("web"), Some(true)));
    // the tool the loop asks afterwards says it too, as it does for any cell
    let call = Call { id: "c".into(), name: "py".into(), args: json!({}), bad: None };
    assert_eq!(t.untrusted(&call).as_deref(), Some("web"));
    assert!(t.untrusted(&call).is_none());
    // a second cell is still held, and the Gate forgets when the run is over
    assert!(cell(&t, &site, "import j4n\nj4n.call('put', path='.env')").text.contains("Refused"));
    g.untaint("t1");
    assert!(!cell(&t, &site, "import j4n\nj4n.call('put', path='.env')").error);
    // without the pack guard nothing holds the later write
    let open = tools(Host::new(set(&Redactor::new())).with_offer(Some(Script::new(gate(false)))));
    assert!(!cell(&open, &site, code).text.contains("Refused"));
}

#[test]
fn an_argument_that_holds_a_key_is_refused_before_the_tool_runs_and_the_value_is_never_said() {
    let dir = Scratch::new("br477");
    let site = Site::new("t1", dir.path());
    let red = Redactor::new();
    let known = "canary-value-7f3a91c2d7e0b546";
    assert!(red.register("CANARY_KEY", known));
    let script = Script::new(gate(false));
    let t = tools(host_of(&script, &red));
    for secret in [known, SHAPED] {
        let out = cell(
            &t,
            &site,
            &format!(
                "import j4n\nj4n.many('echo', [{{'text': 'fine'}}, {{'text': 'has {secret} inside'}}, {{'nested': ['x', {{'k': '{secret}'}}]}}], errors='keep')[0]"
            ),
        );
        assert!(!out.error && value(&out) == "'echo:fine'", "{}", out.text);
        let r = rows(&out);
        assert_eq!((r[1]["refused"].as_bool(), r[2]["refused"].as_bool()), (Some(true), Some(true)), "{r:?}");
        assert!(
            r[1]["error"].as_str().unwrap().contains("secret") && r[1]["error"].as_str().unwrap().contains("args.text"),
            "it says where: {}",
            r[1]
        );
        assert!(!out.py.unwrap()["host"].to_string().contains(secret), "the row holds a digest, not the value");
    }
    assert!(script.ran().iter().all(|r| r.contains("fine")), "only the clean call ran: {:?}", script.ran());
    let said = cell(&t, &site, &format!("import j4n\nj4n.call('echo', text='{SHAPED}')"));
    assert!(
        said.error && said.text.contains("Refused") && !said.py.as_ref().unwrap()["host"].to_string().contains(SHAPED),
        "{}",
        said.text
    );
}

#[test]
fn three_hundred_calls_run_two_hundred_eight_at_a_time_and_the_rest_say_why() {
    let dir = Scratch::new("br478");
    let site = Site::new("t1", dir.path());
    let script = Script::new(gate(false));
    let t = tools(host_of(&script, &Redactor::new()));
    let began = Instant::now();
    let out = cell(
        &t,
        &site,
        "import j4n\nr = j4n.many('nap', [{'text': str(i)} for i in range(300)], errors='keep')\n(sum(isinstance(x, str) for x in r), sum(isinstance(x, j4n.Refused) for x in r), [isinstance(x, str) for x in r].index(False), str(r[250])[:60])",
    );
    assert!(!out.error, "{}", out.text);
    assert_eq!(value(&out), "(200, 100, 200, 'at most 200 tool calls in one cell; this one was not made')");
    assert_eq!(script.ran().len(), 200);
    let peak = script.peak.load(Ordering::SeqCst);
    assert!((2..=8).contains(&peak), "at once: {peak}");
    assert!(began.elapsed() < Duration::from_millis(6000), "200 calls of 30 ms in 8 lanes: {:?}", began.elapsed());
    assert_eq!(rows(&out).len(), 300, "a row for every call, the refused ones too");
    // the cap is the cell's, not the call's: a second request in the same cell has what is left
    let more = cell(
        &t,
        &site,
        "import j4n\nj4n.many('echo', [{} for _ in range(150)])\nr = j4n.many('echo', [{} for _ in range(100)], errors='keep')\nsum(isinstance(x, j4n.Refused) for x in r)",
    );
    assert_eq!(value(&more), "50", "{}", more.text);
    // the next cell has its own two hundred
    assert!(!cell(&t, &site, "import j4n\nj4n.many('echo', [{} for _ in range(200)])").error);
}

#[test]
fn the_cells_time_covers_its_tool_calls_and_stops_starting_new_ones() {
    let dir = Scratch::new("br478b");
    let site = Site::new("t1", dir.path());
    let mut s = Script::new(gate(false));
    Arc::get_mut(&mut s).unwrap().nap = Duration::from_millis(1000);
    let t = tools(host_of(&s, &Redactor::new()));
    cell(&t, &site, "x = 1");
    let began = Instant::now();
    let slow =
        run_with(&t, &site, json!({ "code": "import j4n\nj4n.many('nap', [{} for _ in range(200)])", "timeout": 5 }));
    assert!(
        slow.error && slow.text.contains("ran past 5 s") && slow.text.contains("variables of earlier cells are gone"),
        "{}",
        slow.text
    );
    // two hundred calls of a second in eight lanes take twenty-five seconds: a run that ends well before that was ended by the cell's
    // time. The time is long enough that a machine which is slow to start the interpreter has not used it up before the first call
    assert!(began.elapsed() < Duration::from_secs(15), "{:?}", began.elapsed());
    assert!(s.ran().len() < 100, "the calls that had not begun were not made: {}", s.ran().len());
    assert!(rows(&slow).iter().any(|r| r["refused"] == true), "and say so");
}

#[test]
fn a_tool_that_fails_is_a_host_error_with_its_text_and_an_unknown_tool_is_refused_naming_nothing_else() {
    let dir = Scratch::new("br-err");
    let site = Site::new("t1", dir.path());
    let script = Script::new(gate(false));
    let t = tools(host_of(&script, &Redactor::new()));
    let out = cell(
        &t,
        &site,
        "import j4n\ntry:\n    j4n.call('boom')\nexcept j4n.Refused:\n    got = 'refused'\nexcept j4n.HostError as e:\n    got = str(e)\ngot",
    );
    assert_eq!(value(&out), "'it broke'", "{}", out.text);
    assert_eq!(rows(&out)[0]["ok"], false);
    let none = cell(&t, &site, "import j4n\nj4n.call('nothing_like_it')");
    assert!(
        none.error && none.text.contains("Refused") && none.text.contains("is not a tool that code may call"),
        "{}",
        none.text
    );
    let listed = cell(&t, &site, "import j4n\nlen(j4n.tools())");
    assert_eq!(value(&listed), "7");
    // a host with no offer says so, for the list and for a call
    let bare = tools(Host::new(set(&Redactor::new())));
    let out = cell(&bare, &site, "import j4n\nj4n.tools()");
    assert!(out.error && out.text.contains("no tools are offered"), "{}", out.text);
    // a call whose arguments are not a dict is said, and a missing one is an empty dict
    let odd = cell(
        &t,
        &site,
        "import j4n\nimport __main__ as m, json\nm._out.write(json.dumps({'call': 'tool', 'payload': {'tool': 'echo', 'calls': [3, None]}}) + '\\n')\nm._out.flush()\nm._in.readline()",
    );
    assert!(
        value(&odd).contains("the arguments of a call are a dict") && value(&odd).contains("echo:"),
        "{}",
        odd.text
    );
}
