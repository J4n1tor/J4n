//! A Decider made live by its key. The pack `jev` on and no key: `decider:jev` is made and not asked. A key typed in Agents › Providers
//! puts its probe cases at once (a person is at the screen, so no line waits for them), and once it passed them it answers the next
//! point, in the same run. A key typed for a connector that is not on says what turns it on. A gateway on the loopback is Jev, so what
//! is tested is the real wire, front, Serve and TUI; nothing of either key reaches the Log, the notes or a frame.

use crate::models_cases::mock::{Mock, Say};
use crate::run_cases::Dirs;
use crate::screen_drive::Operator;
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use j4n_core::answer::Question;
use j4n_core::answer::chain::Walk;
use j4n_core::ask::{AskShape, Opt};
use j4n_core::front::serve::Serve;
use j4n_core::task::Mode;
use j4n_proto::{Down, Row, Up};
use j4n_tui::headless::{KeyCode, press, typed};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A key the gateway refuses, then the one it takes: of no vendor's shape, so only the registered value keeps them out.
const WRONG: &str = "wrongkey-0b7d41e2c9aa";
const RIGHT: &str = "rightkey-5f2a90c3e8bb";

/// The right answer of each case, by point and question: what the gateway says to a probe.
pub type Cases = Arc<Mutex<BTreeMap<(String, String), (bool, String)>>>;

fn open(d: &Dirs) -> World {
    // a person at the screen: a line asks without waiting for probe cases, so only the key's own run proves the Decider
    let opts = Opts { doer: Some("test".into()), model: None, approval: Some(Approval::Ask), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false };
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: "claude".into() };
    crate::support::opened(|| wire::open(setup.clone(), &opts, true, "main"))
}

/// Jev on the loopback, taking the key `right`: a request with it is answered as each case wants, any other question with `answer`, each
/// answer priced as a gateway says its cost; a wrong key is
/// refused with a body that says the key back, after a pad that grows with each request, so the key comes to stand across every place
/// a reason is cut short (the excerpt of a body, the one line of a probe's miss).
pub fn gateway(cases: &Cases, right: &'static str) -> Mock {
    let cases = cases.clone();
    Mock::start(move |req, n| {
        let auth = req.headers.get("authorization").cloned().unwrap_or_default();
        if auth != format!("Bearer {right}") {
            // the key stands across the probe's one line (120 characters) on even requests, across the excerpt (300) on odd ones
            let pad = if n % 2 == 0 { 70 } else { 260 } + (n / 2) % 14;
            return Say::Json(401, json!({ "error": format!("not a key: {} {auth}", "x".repeat(pad)) }));
        }
        let body = req.json();
        let at = (body["state"]["point"].as_str().unwrap_or_default().to_string(), body["state"]["question"].as_str().unwrap_or_default().to_string());
        let answer = match cases.lock().unwrap().get(&at).cloned() {
            Some((true, expect)) => json!({ "type": "noul", "noul": if expect == "yes" { 0.97 } else { 0.03 } }),
            Some((false, expect)) => json!({ "type": "choice", "choice": expect, "confidence": 0.95 }),
            None => json!({ "type": "choice", "choice": "answer", "confidence": 0.95 }),
        };
        Say::Json(200, json!({ "model": "jev-test", "answers": { "q": answer }, "usage": { "input_tokens": 10, "output_tokens": 1, "cost_usd": 0.0001 } }))
    })
}

/// Every probe case of the run, as the gateway is asked it.
pub fn learn(world: &World, cases: &Cases) {
    let asks = world.front.asks();
    let prober = asks.prober.as_ref().expect("a prober");
    let mut out = cases.lock().unwrap();
    for point in asks.points.keys() {
        for c in prober.set().cases(point) {
            out.insert((point.clone(), c.question().text), (c.offered.is_empty(), c.expect.clone()));
        }
    }
    assert!(!out.is_empty(), "the base pack has probe cases");
}

fn providers(world: &World) -> Vec<Row> {
    let mut serve = Serve::new(world.front.clone());
    serve.up(Up::Hello { api: j4n_proto::API });
    let (downs, _) = serve.up(Up::Watch { bind: "view:providers".into() });
    downs.into_iter().filter_map(|d| if let Down::Rows { rows, .. } = d { Some(rows) } else { None }).flatten().collect()
}

fn row_text(world: &World, key: &str) -> String {
    providers(world).into_iter().find(|r| r.key == key).map(|r| r.cells.iter().map(|c| c.text.as_str()).collect::<Vec<_>>().join(" | ")).unwrap_or_else(|| panic!("no Providers row {key}"))
}

pub fn events(world: &World) -> Vec<(u64, String, Value)> {
    world.front.log().sync();
    world.front.log().read(0).map(|e| (e.seq, e.kind.clone(), e.data.clone())).collect()
}

fn log_text(world: &World) -> String {
    events(world).iter().map(|(_, k, d)| format!("{k} {d}")).collect::<Vec<_>>().join("\n")
}

/// The probe results of `decider:jev` so far.
pub fn jev_probes(world: &World) -> Vec<(u64, Value)> {
    events(world).into_iter().filter(|(_, k, d)| k == "probe.result" && d["answerer"] == "decider:jev").map(|(s, _, d)| (s, d)).collect()
}

/// Agents › Providers, `⏎` on the row `key`, the key typed masked and `⏎`.
fn type_key(op: &mut Operator, world: &World, key: &str, value: &str, frames: &mut Vec<String>) {
    if !op.look().contains("J4n keeps a key and never shows it") {
        frames.push(op.go(":providers", "Agents › Providers", |s| s.contains("Providers") && s.contains("typesafe")));
    }
    // the row as the screen draws it, one line a row
    let shown = op.look();
    let rows: Vec<&str> = shown.lines().filter(|l| l.starts_with(['├', '╰'])).collect();
    let at = rows.iter().position(|l| l.split_whitespace().any(|w| w == key)).unwrap_or_else(|| panic!("no Providers row {key}:\n{shown}"));
    op.press((0..rows.len()).map(|_| press(KeyCode::Up)).collect());
    op.press((0..at).map(|_| press(KeyCode::Down)).collect());
    op.press(vec![press(KeyCode::Enter)]);
    op.press(typed(value, false));
    frames.push(op.look());
    let sets = || events(world).iter().filter(|(_, k, _)| k == "key.set").count();
    let before = sets();
    op.press(vec![press(KeyCode::Enter)]);
    frames.push(op.look());
    until(op, world, frames, "key.set", || sets() > before);
}

fn until(op: &mut Operator, world: &World, frames: &mut Vec<String>, what: &str, ok: impl Fn() -> bool) {
    let end = Instant::now() + crate::support::PATIENCE;
    while !ok() {
        frames.push(op.look());
        assert!(Instant::now() < end, "never saw {what}; the screen is\n{}\nthe Log:\n{}", op.look(), log_text(world));
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_key_typed_for_the_decider_makes_it_answer_the_next_point_in_the_same_run_after_its_probes() {
    let d = Dirs::new("deciders-live");
    let cases: Cases = Arc::default();
    let jev = gateway(&cases, RIGHT);
    // the pack's Jev, at the loopback: one field of the operator's own over the pack's row
    std::fs::create_dir_all(d.j4n_home().join("connectors")).unwrap();
    std::fs::write(d.j4n_home().join("connectors/jev.toml"), format!("api = 1\n[[connector]]\nid = \"jev\"\nurl = \"{}\"\n", jev.url())).unwrap();
    let world = open(&d);
    learn(&world, &cases);
    let reg = &world.front.asks().registry;
    assert!(reg.installed("decider:jev") && reg.get("decider:jev").is_none(), "made with no key, and not asked");
    assert!(jev.seen().is_empty(), "nothing is sent with no key");
    let mut op = Operator::sit(&world);
    let mut frames = Vec::new();
    // a key the gateway refuses: its cases are put at once, and fail; it does not answer
    type_key(&mut op, &world, "typesafe", WRONG, &mut frames);
    until(&mut op, &world, &mut frames, "the cases put with the wrong key", || !jev_probes(&world).is_empty() && jev_probes(&world).iter().all(|(_, p)| p["passed"] == false) && op_idle(&world));
    // the right key: the cases are put again, and pass
    type_key(&mut op, &world, "typesafe", RIGHT, &mut frames);
    until(&mut op, &world, &mut frames, "the cases passed with the right key", || jev_probes(&world).iter().any(|(_, p)| p["point"] == "intake" && p["passed"] == true));
    let passed = jev_probes(&world).iter().find(|(_, p)| p["point"] == "intake" && p["passed"] == true).map(|(s, _)| *s).unwrap();
    // the completion's point is proved the same way: its cases are put as the front puts its question, and the Decider that knows them passes
    until(&mut op, &world, &mut frames, "the completion's cases passed", || jev_probes(&world).iter().any(|(_, p)| p["point"] == "complete" && p["passed"] == true));
    // the next point in the same run: answered by the Decider
    let options = ["answer", "command", "setting", "task", "flow"].map(|id| Opt { id: id.into(), label: id.into(), recommended: false }).to_vec();
    let text = "what does the readme say about the build".to_string();
    let q = Question { ask: "deciders-next".into(), point: "intake".into(), asker: "test".into(), thread: None, text: text.clone(), shape: AskShape::Choice { options }, goal: text, act: None };
    let asked = world.front.who().ask("intake", None, &q, Some("decider:jev")).expect("the point intake");
    match &asked.walk {
        Walk::Answered(o) => assert_eq!(o.by, "decider:jev", "{o:?}"),
        other => panic!("the Decider did not answer: {other:?}\n{:?}", asked.notes),
    }
    let answered = events(&world).into_iter().find(|(_, k, d)| k == "ask.answered" && d["by"] == "decider:jev").map(|(s, _, _)| s).expect("ask.answered by decider:jev");
    assert!(passed < answered, "it answered after its probes passed");
    // its Providers row says the key is kept, and waits for no start
    let row = row_text(&world, "typesafe");
    assert!(row.contains("set · J4n file") && !row.contains('⟳') && !row.contains("next start"), "{row}");
    // nothing of either key: the Log (the probe's misses, the notes), and every frame drawn
    frames.push(op.look());
    let log = log_text(&world);
    assert!(log.contains("probe.result") && log.contains("key.set"), "{log}");
    // the gateway's refusal, which said the wrong key back, is in the probe's misses, scrubbed
    assert!(log.contains("Bearer «secret"), "{log}");
    assert!(jev.seen().iter().filter(|r| r.headers.get("authorization").is_some_and(|a| a.contains(WRONG))).count() >= 8, "the pad swept the places a reason is cut");
    for key in [WRONG, RIGHT] {
        assert!(!log.contains(&key[..8]), "the Log holds part of a key:\n{log}");
        assert!(frames.iter().all(|f| !f.contains(&key[..8])), "a frame holds part of a key");
    }
    drop(op);
    world.close();
}

/// No probe run is in hand: every pair put was written.
fn op_idle(world: &World) -> bool {
    let probes = jev_probes(world);
    let points: std::collections::BTreeSet<String> = probes.iter().map(|(_, p)| p["point"].as_str().unwrap_or_default().to_string()).collect();
    let due = world.front.asks().prober.as_ref().unwrap().targets(Some("decider:jev")).len();
    points.len() >= due
}

#[test]
fn a_key_typed_for_a_connector_that_is_not_on_says_what_turns_it_on() {
    let d = Dirs::new("deciders-off");
    d.pack_off("jev");
    // a Decider of the operator's own, turned off
    std::fs::create_dir_all(d.j4n_home().join("connectors")).unwrap();
    std::fs::write(d.j4n_home().join("connectors/mine.toml"), "api = 1\n[[connector]]\nid = \"mine\"\nis = \"decider\"\ndialect = \"systemone\"\nurl = \"https://mine.example.com\"\nkey = \"env:J4N_TEST_MINE_KEY\"\nmodels = [\"m\"]\noff = true\n").unwrap();
    let world = open(&d);
    let mut op = Operator::sit(&world);
    let mut frames = Vec::new();
    // TypeSafe's key, for jev, whose pack is off
    type_key(&mut op, &world, "typesafe", RIGHT, &mut frames);
    let said = |w: &str| events(&world).into_iter().any(|(_, k, d)| k == "chat.note" && d["text"].as_str().is_some_and(|t| t.contains(w)));
    until(&mut op, &world, &mut frames, "what turns jev on", || said("in the pack jev, which is off"));
    let note = events(&world).into_iter().find(|(_, k, d)| k == "chat.note" && d["text"].as_str().is_some_and(|t| t.contains("in the pack jev"))).unwrap().2;
    assert!(note["text"].as_str().unwrap().contains("from the next start"), "{note}");
    // the Decider of the operator's own, which is off
    type_key(&mut op, &world, "mine", RIGHT, &mut frames);
    until(&mut op, &world, &mut frames, "what turns mine on", || said("mine is off"));
    assert!(std::fs::read_to_string(d.j4n_home().join("keys/mine")).is_ok_and(|k| k == RIGHT), "the key is kept all the same");
    // its row waits for its switch, not for a start
    let row = row_text(&world, "mine");
    assert!(row.contains("set · J4n file") && !row.contains('⟳'), "{row}");
    let log = log_text(&world);
    assert!(!log.contains(&RIGHT[..12]), "{log}");
    assert!(frames.iter().all(|f| !f.contains(&RIGHT[..12])));
    drop(op);
    world.close();
}
