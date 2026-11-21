//! Code in the `py` kernel asks J4n for work with `j4n.host`. The kernel is the
//! real one and the decider is the real client against a gateway on the loopback that answers as a script says: nothing leaves the
//! machine, and no key of the operator's is read. What is tested: the batching and the keying by item, the setting `kit.judge`
//! and the leak check, the closed set of calls and the kernel's bare environment, the cell's time covering its host
//! calls, the row each call leaves in the event, and the spend it adds up to.

use crate::mock::{Mock, Say, Seen};
use crate::world::Scratch;
use j4n_connect::decide::{Decider, DeciderConfig, DeciderWire};
use j4n_connect::kithost::{Chat, Host, KitSet, spend};
use j4n_connect::model::doer::{Gated, ToolOut, Tools, action};
use j4n_connect::model::openai::Openai;
use j4n_connect::model::{Call, Client};
use j4n_connect::net::Net;
use j4n_connect::py::PyTools;
use j4n_core::gate::{ReadOnly, Verb};
use j4n_core::place::{Launch, Site};
use j4n_core::redact::Redactor;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const JEV_KEY: &str = "k-secret-jev";
/// Shaped like a GitHub token, made up for the test.
const SHAPED: &str = "ghp_aB3dE6gH9jK2mN5pQ8sT1vW4yZ7bC0eF3hJ6";

fn net() -> Net {
    Net::new().with_env(|_| None)
}

fn decider(mock: &Mock) -> Arc<Decider> {
    Arc::new(Decider::new(DeciderConfig { id: "jev".into(), wire: DeciderWire::Systemone, base: mock.url(), key: JEV_KEY.into(), model: "jev-latest".into(), zone: None, net: net(), backoff: vec![Duration::ZERO; 2] }))
}

/// The settings of a project, with egress to `local` (the loopback) only.
fn set(judge: bool, red: &Redactor) -> KitSet {
    KitSet { judge, keep: 0.15, model: "light".into(), red: red.clone(), egress: Arc::new(|zone| if zone == "local" { Ok(()) } else { Err(format!("egress to `{zone}` is blocked")) }), ..KitSet::default() }
}

fn tools(host: Host) -> PyTools {
    PyTools::new().with_host(Arc::new(host))
}

fn cell(t: &PyTools, site: &Site, code: &str) -> ToolOut {
    t.run(&Call { id: "c1".into(), name: "py".into(), args: json!({ "code": code }), bad: None }, site, &|l: Launch| l)
}

fn site(dir: &Scratch) -> Site {
    Site::new("t1", dir.path())
}

/// A noul for every question of the request: `p<N>` is (N mod 10) / 10.
fn tenths(seen: &Seen) -> Say {
    let qs = seen.json()["questions"].as_object().cloned().unwrap_or_default();
    let answers: serde_json::Map<String, Value> = qs.keys().map(|q| (q.clone(), json!({ "type": "noul", "noul": f64::from(q[1..].parse::<u32>().unwrap_or(0) % 10) / 10.0 }))).collect();
    Say::Json(200, json!({ "model": "jev-latest", "answers": answers, "usage": { "input_tokens": 9, "output_tokens": 0 } }))
}

/// What a cell answered, as `=> value`.
fn value(out: &ToolOut) -> String {
    out.text.lines().find_map(|l| l.strip_prefix("=> ")).unwrap_or_default().to_string()
}

/// A cell that talks to J4n on the kernel's own private line, as a cell that goes around the library could.
fn raw(call: &str, payload: &str) -> String {
    format!("import __main__ as m, json\nm._out.write(json.dumps({{'call': {call:?}, 'payload': {payload}}}) + '\\n')\nm._out.flush()\nm._in.readline()")
}

#[test]
fn three_hundred_items_make_requests_of_24_or_fewer_run_at_once_and_come_back_keyed_by_item() {
    let (live, peak) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let (l, p) = (live.clone(), peak.clone());
    let mock = Mock::start(move |seen, _| {
        p.fetch_max(l.fetch_add(1, Ordering::SeqCst) + 1, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(1000));
        l.fetch_sub(1, Ordering::SeqCst);
        tenths(seen)
    });
    let dir = Scratch::new("kit465");
    let t = tools(Host::new(set(true, &Redactor::new())).with_jev(Some(decider(&mock))));
    let began = Instant::now();
    let out = cell(&t, &site(&dir), "import j4n\nitems = [f'passage {i}' for i in range(300)]\np = j4n.judge(items, 'Is it about X?')\n(len(p), all(p[i] == (i % 10) / 10 for i in range(300)), len(j4n.keep(p)))");
    assert!(!out.error, "{}", out.text);
    assert_eq!(value(&out), "(300, True, 240)", "every answer is for its own item; p of 0.0 and 0.1 are dismissed, the rest kept");
    let seen = mock.seen();
    let sizes: Vec<usize> = seen.iter().map(|s| s.json()["questions"].as_object().map_or(0, serde_json::Map::len)).collect();
    assert_eq!((sizes.iter().sum::<usize>(), sizes.iter().copied().max(), sizes.len()), (300, Some(24), 13), "{sizes:?}");
    for s in &seen {
        let b = s.json();
        assert_eq!(b["model"], "jev-latest");
        assert_eq!(s.headers.get("authorization").map(String::as_str), Some("Bearer k-secret-jev"));
        let (q, passages) = (b["questions"].as_object().unwrap(), b["state"]["passages"].as_object().unwrap());
        assert!(q.keys().eq(passages.keys()), "a request's state holds the items it asks about, and only those");
    }
    // at once: more than one in flight, never more than eight, and well under the 13 x 1000 ms of one after the other (about two rounds)
    let top = peak.load(Ordering::SeqCst);
    assert!((2..=8).contains(&top), "peak {top}");
    assert!(began.elapsed() < Duration::from_secs(9), "{:?}", began.elapsed());
    // the cell's event lists the call once, with what it cost
    let ev = out.py.expect("a py.cell");
    let rows = ev["host"].as_array().expect("host rows");
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0]["call"].as_str(), rows[0]["requests"].as_u64(), rows[0]["items"].as_u64(), rows[0]["failed"].as_u64(), rows[0]["ok"].as_bool()), (Some("jev_many"), Some(13), Some(300), Some(0), Some(true)), "{ev}");
    assert_eq!((rows[0]["tokens_in"].as_u64(), rows[0]["tokens_out"].as_u64()), (Some(13 * 9), Some(0)));
    assert!(rows[0]["usd"].is_null(), "Jev names no price: unknown, not zero");
    assert!(!ev.to_string().contains(JEV_KEY), "no key in the event");
    // the spend fold is given the tokens
    let m = spend(&ev).expect("spend");
    assert_eq!((m.tokens_in, m.tokens_out, m.usd), (117, 0, 0.0));
    assert!(spend(&json!({ "ok": true })).is_none(), "a cell with no host call adds nothing");
}

#[test]
fn with_kit_judge_off_nothing_leaves_and_the_error_names_the_setting() {
    let mock = Mock::start(|seen, _| tenths(seen));
    let dir = Scratch::new("kit466a");
    let t = tools(Host::new(set(false, &Redactor::new())).with_jev(Some(decider(&mock))));
    let s = site(&dir);
    for code in ["import j4n\nj4n.judge(['a'], 'q')", "import j4n\nj4n.choose(['a'], 'q', ['x', 'y'])", "import j4n\nj4n.score(['a'], 'q', ['lo', 'hi'])"] {
        let out = cell(&t, &s, code);
        assert!(out.error && out.text.contains("HostError") && out.text.contains("kit.judge"), "{}", out.text);
    }
    // a cell that goes around the library to the kernel's own line is refused by J4n, not by the library
    let around = cell(&t, &s, &raw("jev_many", "{'calls': [{'state': {'passages': {'p0': 'x'}}, 'questions': {'p0': {'type': 'noul', 'instructions': 'q'}}}]}"));
    assert!(!around.error && value(&around).contains("kit.judge") && value(&around).contains(r#""ok":false"#), "{}", around.text);
    assert!(mock.seen().is_empty(), "nothing left the machine: {}", mock.seen().len());
    let rows = around.py.expect("event")["host"].clone();
    assert_eq!((rows[0]["ok"].as_bool(), rows[0]["call"].as_str()), (Some(false), Some("jev_many")));
    // the other calls do not ask kit.judge: `model` rides on the egress Gate, and with no model connected it says so
    let model = cell(&t, &s, "import j4n\nj4n.model('hi')");
    assert!(model.error && model.text.contains("no chat model") && !model.text.contains("kit.judge"), "{}", model.text);
}

#[test]
fn with_it_on_a_canary_key_in_the_state_is_refused_whole_and_fail_closed() {
    let mock = Mock::start(|seen, _| tenths(seen));
    let dir = Scratch::new("kit466b");
    let red = Redactor::new();
    let known = "canary-value-7f3a91c2d7e0b546";
    assert!(red.register("CANARY_KEY", known));
    let t = tools(Host::new(set(true, &red)).with_jev(Some(decider(&mock))));
    let s = site(&dir);
    for secret in [known, SHAPED] {
        let out = cell(&t, &s, &format!("import j4n\nj4n.judge(['clean one', 'two {secret} two', 'clean three'], 'q')"));
        assert!(out.error && out.text.contains("nothing was sent") && out.text.contains("secret"), "{}", out.text);
        assert!(out.text.contains("p1") && !out.text.contains("p0") && !out.text.contains(secret), "it says where, and never the value: {}", out.text);
        // the cell's own code holds it (the Log scrubs that blob); what J4n and the kernel said of the call does not
        let ev = out.py.expect("event");
        assert!(!ev["host"].to_string().contains(secret) && !ev["error"].to_string().contains(secret), "{ev}");
    }
    // a key in the question is as bad as one in the state
    let asked = cell(&t, &s, &format!("import j4n\nj4n.judge(['a'], 'Is {SHAPED} here?')"));
    assert!(asked.error && asked.text.contains("nothing was sent"), "{}", asked.text);
    assert!(mock.seen().is_empty(), "the clean items went nowhere either: the whole call is refused");
    // text with no secret goes through the same kernel at once
    let ok = cell(&t, &s, "import j4n\nlen(j4n.judge(['clean one', 'clean three'], 'q'))");
    assert!(!ok.error && value(&ok) == "2", "{}", ok.text);
    assert_eq!(mock.seen().len(), 1);
}

#[test]
fn a_zone_the_egress_gate_refuses_gets_nothing_and_a_missing_decider_is_said() {
    let dir = Scratch::new("kit-egress");
    let mock = Mock::start(|seen, _| tenths(seen));
    let mut elsewhere = set(true, &Redactor::new());
    elsewhere.egress = Arc::new(|zone| Err(format!("egress to `{zone}` needs a yes")));
    let t = tools(Host::new(elsewhere).with_jev(Some(decider(&mock))));
    let out = cell(&t, &site(&dir), "import j4n\nj4n.judge(['a'], 'q')");
    assert!(out.error && out.text.contains("needs a yes"), "{}", out.text);
    assert!(mock.seen().is_empty());
    let none = tools(Host::new(set(true, &Redactor::new())));
    let out = cell(&none, &site(&dir), "import j4n\nj4n.judge(['a'], 'q')");
    assert!(out.error && out.text.contains("Jev is not connected"), "{}", out.text);
    // a kernel with no host at all
    let bare = PyTools::new();
    let out = cell(&bare, &site(&dir), "import j4n\nj4n.judge(['a'], 'q')");
    assert!(out.error && out.text.contains("no host calls"), "{}", out.text);
}

#[test]
fn host_names_only_the_closed_set_and_need_with_no_home_for_packages_says_so() {
    let mock = Mock::start(|seen, _| tenths(seen));
    let dir = Scratch::new("kit467a");
    let t = tools(Host::new(set(true, &Redactor::new())).with_jev(Some(decider(&mock))));
    let s = site(&dir);
    let unknown = cell(&t, &s, "import j4n\nj4n.host('shell', cmd='ls')");
    assert!(unknown.error && unknown.text.contains("jev_many, model, need, tool, tools"), "{}", unknown.text);
    assert!(unknown.py.expect("event").get("host").is_none(), "the library refused before anything was written");
    // around the library, the host refuses by itself, and says the same
    let around = cell(&t, &s, &raw("shell", "{'cmd': 'ls'}"));
    assert!(
        value(&around).contains(r#""ok":false"#)
            && value(&around).contains("is not a host call")
            && value(&around).contains("jev_many, model, need, tool, tools"),
        "{}",
        around.text
    );
    let ev = around.py.expect("event");
    assert_eq!((ev["host"][0]["ok"].as_bool(), ev["host"][0]["call"].as_str()), (Some(false), Some("shell")));
    // the kernel goes on
    assert_eq!(value(&cell(&t, &s, "1 + 1")), "2");
    let need = cell(&t, &s, "import j4n\nj4n.need('pandas')");
    assert!(need.error && need.text.contains("no folder for packages"), "{}", need.text); // the packages themselves are `kitpkg_cases`
    assert!(mock.seen().is_empty());
}

#[test]
fn the_kernel_holds_none_of_the_connectors_keys_and_none_of_j4ns_environment() {
    let mock = Mock::start(|seen, _| tenths(seen));
    let dir = Scratch::new("kit467b");
    let t = tools(Host::new(set(true, &Redactor::new())).with_jev(Some(decider(&mock))));
    let s = site(&dir);
    let names = cell(&t, &s, "import os\nsorted(os.environ)");
    let listed: Vec<String> = serde_json::from_str(&value(&names).replace('\'', "\"")).expect("a list of names");
    assert!(listed.iter().all(|n| ["PATH", "LANG", "LC_CTYPE"].contains(&n.as_str())), "{listed:?}");
    // this process has a good many more (HOME, TMPDIR, the cargo ones), and none of them is there
    assert!(std::env::vars().any(|(n, _)| n == "HOME") && !listed.iter().any(|n| n == "HOME" || n.starts_with("CARGO") || n.starts_with("J4N")));
    // the key names the pack jev's connectors ask for (`env:NAME`) are not in it, and neither is a key's value anywhere a cell can look
    let pack = include_str!("../../../../packs/jev/connectors/jev.toml");
    let wanted: Vec<&str> = pack.split("env:").skip(1).map(|r| r.split('"').next().unwrap_or_default()).collect();
    assert!(!wanted.is_empty() && wanted.iter().all(|name| !name.is_empty() && !listed.iter().any(|n| n == name)), "{wanted:?}");
    cell(&t, &s, "import j4n\nj4n.judge(['a'], 'q')");
    let all = cell(&t, &s, "import os, sys, j4n\nrepr((dict(os.environ), sys.argv, sorted(vars(j4n)), open('/proc/self/cmdline').read()))");
    assert!(!all.error && !all.text.contains(JEV_KEY), "{}", all.text);
}

#[test]
fn the_cells_time_covers_its_host_calls() {
    let mock = Mock::start(|seen, _| {
        std::thread::sleep(Duration::from_secs(14));
        tenths(seen)
    });
    let dir = Scratch::new("kit-time");
    let t = tools(Host::new(set(true, &Redactor::new())).with_jev(Some(decider(&mock))));
    let s = site(&dir);
    cell(&t, &s, "x = 1");
    let began = Instant::now();
    let slow = t.run(&Call { id: "c".into(), name: "py".into(), args: json!({ "code": "import j4n\nj4n.judge(['a'], 'q')", "timeout": 4 }), bad: None }, &s, &|l: Launch| l);
    assert!(began.elapsed() < Duration::from_secs(11), "stopped at its time, not when the decider answered: {:?}", began.elapsed());
    assert!(slow.error && slow.text.contains("ran past 4 s") && slow.text.contains("variables of earlier cells are gone"), "{}", slow.text);
    let rows = slow.py.expect("event")["host"].clone();
    assert_eq!((rows[0]["call"].as_str(), rows[0]["ok"].as_bool()), (Some("jev_many"), Some(false)), "{rows}");
    assert_eq!(t.alive(), 0);
    assert!(cell(&t, &s, "x").text.contains("NameError"), "the next cell is on a new kernel");
}

fn sse(text: &str) -> Say {
    Say::Sse(vec![
        json!({ "choices": [{ "delta": { "content": text } }] }).to_string(),
        json!({ "choices": [{ "delta": {}, "finish_reason": "stop" }] }).to_string(),
        json!({ "choices": [], "usage": { "prompt_tokens": 50, "completion_tokens": 4, "cost": 0.0002 } }).to_string(),
        "[DONE]".into(),
    ])
}

#[test]
fn model_is_asked_last_with_its_context_and_its_cost_is_in_the_row() {
    let mock = Mock::start(|seen, _| sse(if seen.body.contains("exactly these keys") { r#"Here: {"verdict": "yes", "why": "ok"}"# } else { "the summary" }));
    let dir = Scratch::new("kit-model");
    let client = Client::new(Box::new(Openai), net(), &format!("{}/v1", mock.url()), "k-model");
    let chat = Chat { client, model: "vendor/small".into(), zone: "local".into() };
    let t = tools(Host::new(set(false, &Redactor::new())).with_chat(Some(chat)));
    let s = site(&dir);
    let plain = cell(&t, &s, "import j4n\nj4n.model('Summarize.', context=['first piece', 'second piece'])");
    assert!(!plain.error && value(&plain) == "'the summary'", "{}", plain.text);
    let body = mock.seen()[0].json();
    let user = body["messages"].as_array().unwrap().iter().map(|m| m["content"].as_str().unwrap_or_default().to_string()).collect::<Vec<_>>().join("\n");
    assert!(body["model"] == "vendor/small" && user.contains("Summarize.") && user.contains("[1] first piece") && user.contains("[2] second piece"), "{body}");
    let row = plain.py.as_ref().expect("event")["host"][0].clone();
    assert_eq!((row["call"].as_str(), row["tokens_in"].as_u64(), row["tokens_out"].as_u64(), row["usd"].as_f64()), (Some("model"), Some(50), Some(4), Some(0.0002)));
    let m = spend(plain.py.as_ref().unwrap()).unwrap();
    assert_eq!((m.tokens_in, m.tokens_out, m.usd), (50, 4, 0.0002));
    let shaped = cell(&t, &s, "import j4n\nj4n.model('Is it?', shape={'verdict': 'yes or no', 'why': 'a line'})");
    assert!(!shaped.error && value(&shaped) == "{'verdict': 'yes', 'why': 'ok'}", "{}", shaped.text);
    let missing = cell(&t, &s, "import j4n\nj4n.model('Is it?', shape={'verdict': 'yes or no', 'other': 'x'})");
    assert!(missing.error && missing.text.contains("has no `other`"), "{}", missing.text);
    // a key in the prompt is refused before it leaves
    let before = mock.seen().len();
    let leak = cell(&t, &s, &format!("import j4n\nj4n.model('use {SHAPED}')"));
    assert!(leak.error && leak.text.contains("nothing was sent") && !leak.text.contains(SHAPED), "{}", leak.text);
    assert_eq!(mock.seen().len(), before);
}

/// A web tool that answers a page, or asks first.
struct Web {
    asks: bool,
    seen: std::sync::Mutex<Vec<String>>,
}

impl Tools for Web {
    fn specs(&self) -> Vec<j4n_connect::model::Tool> {
        vec![j4n_connect::model::Tool { name: "web".into(), doc: "a page".into(), schema: json!({ "type": "object" }) }]
    }

    fn untrusted(&self, call: &Call) -> Option<String> {
        (call.name == "web").then(|| "web".to_string())
    }

    fn gate(&self, call: &Call, task: &str, site: &Site, ro: ReadOnly) -> Option<Gated> {
        self.asks.then(|| Gated { action: action("web", Verb::Net, call.args["url"].as_str().unwrap_or_default(), task, site, ro), question: "fetch a page".into(), risk: 20 })
    }

    fn run(&self, call: &Call, _: &Site, _: &(dyn Fn(Launch) -> Launch + Send + Sync)) -> ToolOut {
        self.seen.lock().unwrap().push(call.args["url"].as_str().unwrap_or_default().to_string());
        ToolOut { text: "the page".into(), ..ToolOut::default() }
    }
}

#[test]
fn fetch_goes_through_the_web_tools_gate_and_taints_the_run() {
    let dir = Scratch::new("kit-fetch");
    let s = site(&dir);
    let open = Arc::new(Web { asks: false, seen: Default::default() });
    let t = tools(Host::new(set(false, &Redactor::new())).with_offer(Some(open.clone())));
    let call = Call { id: "c".into(), name: "py".into(), args: json!({}), bad: None };
    let got = cell(&t, &s, "import j4n\nj4n.fetch('https://example.org/a')");
    assert!(!got.error && value(&got) == "'the page'", "{}", got.text);
    assert_eq!(*open.seen.lock().unwrap(), ["https://example.org/a"]);
    assert!(t.untrusted(&call).is_some(), "what a page said is not the operator's");
    assert!(t.untrusted(&call).is_none(), "and it is said once");
    cell(&t, &s, "1");
    assert!(t.untrusted(&call).is_none(), "a cell that fetched nothing taints nothing");
    // where the web tool would ask, the cell is told to ask it first, and nothing is fetched
    let asking = Arc::new(Web { asks: true, seen: Default::default() });
    let t = tools(Host::new(set(false, &Redactor::new())).with_offer(Some(asking.clone())));
    let refused = cell(&t, &s, "import j4n\nj4n.fetch('https://example.org/b')");
    assert!(
        refused.error
            && refused.text.contains("Refused")
            && refused.text.contains("asks before")
            && refused.text.contains("fetch a page"),
        "{}",
        refused.text
    );
    assert!(asking.seen.lock().unwrap().is_empty() && t.untrusted(&call).is_none());
    // an address that carries a key does not go out
    let t = tools(Host::new(set(false, &Redactor::new())).with_offer(Some(open.clone())));
    let out = cell(&t, &s, &format!("import j4n\nj4n.fetch('https://example.org/?t={SHAPED}')"));
    assert!(out.error && out.text.contains("nothing was sent"), "{}", out.text);
    assert_eq!(open.seen.lock().unwrap().len(), 1);
}
