//! The deciders (`systemone`, `decisions`) and the model answerer against a gateway on the loopback, and the walk of a chain
//! with them in it: what is asked, what comes back, what fails, what the egress Gate keeps from leaving, and the breaker
//! with a real client behind it.

use crate::bench::base_layer;
use crate::mock::{Mock, Proxy as Fixture, Say};
use crate::world::Scratch;
use j4n_connect::decide::{Decider, DeciderConfig, DeciderWire, ModelAnswerer, ModelConfig, zone_of};
use j4n_connect::model::openai::Openai;
use j4n_connect::model::Client;
use j4n_connect::net::{Net, Proxy};
use j4n_core::answer::chain::{Ledger, Registry, Walk, walk};
use j4n_core::answer::point::points;
use j4n_core::answer::state::Answerers;
use j4n_core::answer::{Answerer, AskPoint, Offer, Paid, Question, described};
use j4n_core::ask::{Answer, AskShape, Opt};
use j4n_core::change::Actor;
use j4n_core::clock::{Clock, ManualClock};
use j4n_core::fold::Folds;
use j4n_core::front::INTAKE;
use j4n_core::gate::Gate;
use j4n_core::gate::check::Gates;
use j4n_core::gate::grant::Grants;
use j4n_core::log::{Log, New, Opts, Scope};
use j4n_core::redact::Redactor;
use j4n_core::table::Tables;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

fn net() -> Net {
    Net::new().with_env(|_| None)
}

fn decider(mock: &Mock, wire: DeciderWire) -> Decider {
    Decider::new(DeciderConfig { id: "jev".into(), wire, base: mock.url(), key: "k-secret".into(), model: "jev-latest".into(), zone: None, net: net(), backoff: vec![Duration::ZERO; 2] })
}

fn opts(ids: &[&str]) -> AskShape {
    AskShape::Choice { options: ids.iter().map(|id| Opt { id: (*id).into(), label: format!("the {id} way"), recommended: false }).collect() }
}

fn question(shape: AskShape) -> Question {
    Question { ask: "pick#1".into(), point: "pick".into(), asker: "front".into(), thread: None, text: "which one?".into(), shape, goal: "fix the build".into(), act: None }
}

fn said(o: Offer) -> (Answer, f32) {
    match o {
        Offer::Answer { answer, conf, .. } => (answer, conf),
        other => panic!("not an answer: {other:?}"),
    }
}

fn failed(o: Offer) -> String {
    match o {
        Offer::Failed { why } => why,
        other => panic!("not a failure: {other:?}"),
    }
}

fn judgment(answers: Value) -> Say {
    Say::Json(200, json!({ "model": "jev-latest", "answers": answers, "usage": { "input_tokens": 9, "output_tokens": 0 } }))
}

#[test]
fn a_yes_no_goes_to_systemone_as_a_noul_and_comes_back_as_the_likelier_side() {
    let mock = Mock::start(|_, n| judgment(json!({ "q": { "type": "noul", "noul": if n == 0 { 0.83 } else { 0.2 } } })));
    let d = decider(&mock, DeciderWire::Systemone);
    let (a, c) = said(d.offer(&question(AskShape::YesNo)));
    assert_eq!((a, (c * 100.0).round()), (Answer::Yes, 83.0));
    let (a, c) = said(d.offer(&question(AskShape::YesNo)));
    assert_eq!((a, (c * 100.0).round()), (Answer::No, 80.0));
    let half = Mock::start(|_, _| judgment(json!({ "q": { "type": "noul", "noul": 0.5 } })));
    let (a, c) = said(decider(&half, DeciderWire::Systemone).offer(&question(AskShape::YesNo)));
    assert_eq!((a, c), (Answer::Yes, 0.5), "half is yes, and as unsure as it gets");
    let seen = &mock.seen()[0];
    assert_eq!((seen.method.as_str(), seen.path.as_str()), ("POST", "/v1/systemone"));
    assert_eq!(seen.headers.get("authorization").map(String::as_str), Some("Bearer k-secret"));
    let b = seen.json();
    assert_eq!(b["model"], "jev-latest");
    assert_eq!(b["state"], json!({ "point": "pick", "question": "which one?", "goal": "fix the build" }));
    assert_eq!(b["questions"]["q"]["type"], "noul");
    assert!(b["questions"]["q"]["instructions"].as_str().unwrap().contains("which one?"));
    assert_eq!(d.id(), "decider:jev");
    assert_eq!(d.model(), "jev-latest@systemone");
}

#[test]
fn a_choice_goes_to_systemone_with_its_options_and_comes_back_as_one_of_them() {
    let mock = Mock::start(|_, n| {
        if n == 0 {
            judgment(json!({ "q": { "type": "choice", "choice": "safe", "confidence": 0.91, "probabilities": { "safe": 0.91, "risky": 0.09 } } }))
        } else {
            judgment(json!({ "q": { "type": "choice", "choice": "risky", "probabilities": { "safe": 0.3, "risky": 0.7 } } }))
        }
    });
    let d = decider(&mock, DeciderWire::Systemone);
    let q = question(opts(&["safe", "risky"]));
    let (a, c) = said(d.offer(&q));
    assert_eq!((a, (c * 100.0).round()), (Answer::Pick { id: "safe".into() }, 91.0));
    let (a, c) = said(d.offer(&q));
    assert_eq!((a, (c * 100.0).round()), (Answer::Pick { id: "risky".into() }, 70.0), "no confidence: the probability of the choice");
    let b = mock.seen()[0].json();
    assert_eq!(b["questions"]["q"]["type"], "choice");
    assert_eq!(b["questions"]["q"]["criteria"], json!({ "safe": "the safe way", "risky": "the risky way" }));
}

/// The request jev gets for the first line of a thread, built as `Front::intake` builds it (the five ids of
/// `INTAKE`, bare, the line as both the question and the goal) and sent through the real client to a gateway on the loopback.
/// The same bytes go to the decider in a benchmark run: nothing in what J4n sends may move the combo intake under its floor. A change
/// of these words, their order, the options or the state is a change of what jev is asked, and a decision of its own.
#[test]
fn the_intake_request_is_the_line_five_options_with_their_says_and_nothing_that_leans_to_task() {
    let line = "This folder holds several small projects. Do all of these:\n\nIn `p1/` (a project of its own; work from inside that folder): Answer these questions about the project, writing `answer.json`. Do not change any file.\n\nIn `p2/`: `make ci` fails; fix it with the smallest change.\n\nThen add one `- ` line for each project to CHANGELOG.md.";
    let tables = tables();
    let says = j4n_core::answer::point::says(&tables);
    let bare = AskShape::Choice { options: INTAKE.iter().map(|id| Opt { id: (*id).into(), label: (*id).into(), recommended: *id == "task" }).collect() };
    let q = Question { ask: "intake#1-0".into(), point: "intake".into(), asker: "front".into(), thread: Some("run-1".into()), text: line.into(), shape: described(bare, &says, "intake"), goal: line.into(), act: None };
    let mock = Mock::start(|_, _| judgment(json!({ "q": { "type": "choice", "choice": "flow", "confidence": 0.9 } })));
    assert!(matches!(decider(&mock, DeciderWire::Systemone).offer(&q), Offer::Answer { .. }));
    let b = mock.seen()[0].json();
    let mut keys: Vec<&str> = b.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort();
    assert_eq!(keys, ["model", "questions", "state"], "no other field rides along");
    assert_eq!(b["state"], json!({ "point": "intake", "question": line, "goal": line }), "the goal is the line itself, not a story in front of it");
    let says = [
        ("answer", "the line asks to be told something, and no work is done"),
        ("command", "the line is a command of the tool itself"),
        ("setting", "the line asks to change a setting of the tool"),
        ("task", "the line asks for one piece of work, which a doer does in one go"),
        ("flow", "the line asks for a plan first and then the work in several steps, or lists the steps"),
    ];
    let instructions: String = format!("Question: {line}\nOptions:\n") + &says.iter().map(|(id, line)| format!("- {id}: {line}\n")).collect::<String>();
    assert_eq!(b["questions"]["q"]["instructions"], instructions, "the options in the order of INTAKE, each with its line, no `What is being done`, no mark on `task`");
    assert_eq!(b["questions"]["q"]["type"], "choice");
    let criteria = b["questions"]["q"]["criteria"].as_object().unwrap();
    assert_eq!(criteria.len(), 5);
    for (id, line) in says {
        assert_eq!(criteria[id], line, "{id}");
    }
    assert!(!b.to_string().to_lowercase().contains("recommend"), "{b}");
}

#[test]
fn the_decisions_dialect_asks_one_noul_for_each_option_and_takes_the_likeliest() {
    let mock = Mock::start(|_, _| judgment(json!({ "q__c0": { "type": "noul", "noul": 0.2 }, "q__c1": { "type": "noul", "noul": 0.77 }, "q__c2": { "type": "noul", "noul": 0.1 } })));
    let d = decider(&mock, DeciderWire::Decisions);
    let (a, c) = said(d.offer(&question(opts(&["a", "b", "c"]))));
    assert_eq!((a, (c * 100.0).round()), (Answer::Pick { id: "b".into() }, 77.0));
    let seen = &mock.seen()[0];
    assert_eq!(seen.path, "/api/alpha/decisions");
    let b = seen.json();
    assert!(b["state"].is_string(), "this contract takes the state as a string");
    let fans = b["questions"].as_object().unwrap();
    assert_eq!(fans.keys().collect::<Vec<_>>(), ["q__c0", "q__c1", "q__c2"]);
    assert!(fans["q__c1"]["instructions"].as_str().unwrap().contains("under test is: b"));
    assert_eq!(fans["q__c1"]["type"], "noul");
    // a yes/no is one plain noul
    let yn = Mock::start(|_, _| judgment(json!({ "q": { "type": "noul", "noul": 0.9 } })));
    assert_eq!(said(decider(&yn, DeciderWire::Decisions).offer(&question(AskShape::YesNo))).0, Answer::Yes);
    // a tie goes to the first option offered
    let tie = Mock::start(|_, _| judgment(json!({ "q__c0": { "noul": 0.5 }, "q__c1": { "noul": 0.5 } })));
    assert_eq!(said(decider(&tie, DeciderWire::Decisions).offer(&question(opts(&["x", "y"])))).0, Answer::Pick { id: "x".into() });
}

#[test]
fn a_decider_takes_no_text_and_no_approval_and_sends_nothing_for_them() {
    let mock = Mock::start(|_, _| judgment(json!({})));
    let d = decider(&mock, DeciderWire::Systemone);
    for shape in [AskShape::Text, AskShape::Approval { action: "rm".into(), risk: 90 }, AskShape::Choice { options: vec![] }] {
        assert!(matches!(d.offer(&question(shape)), Offer::Pass { .. }));
    }
    assert!(mock.seen().is_empty());
}

#[test]
fn a_decider_that_cannot_be_read_has_failed_and_a_default_never_stands_in() {
    for (script, says) in [
        (Say::Json(401, json!({ "error": { "message": "bad key" } })), "HTTP 401"),
        (Say::Text(200, "<html>nope</html>".into()), "not JSON"),
        (Say::Json(200, json!({ "answers": {} })), "no answer for `q`"),
        (Say::Json(200, json!({ "answers": { "q": { "type": "noul" } } })), "no probability"),
    ] {
        let mock = Mock::start({
            let body = match &script {
                Say::Json(c, v) => Say::Json(*c, v.clone()),
                Say::Text(c, t) => Say::Text(*c, t.clone()),
                _ => unreachable!(),
            };
            let cell = std::sync::Mutex::new(Some(body));
            move |_, _| cell.lock().unwrap().take().unwrap_or(Say::Drop)
        });
        let why = failed(decider(&mock, DeciderWire::Systemone).offer(&question(AskShape::YesNo)));
        assert!(why.contains(says), "{why}");
        assert_eq!(mock.seen().len(), 1, "a status that will not mend is not tried again: {says}");
    }
    let choice = Mock::start(|_, _| judgment(json!({ "q": { "type": "choice" } })));
    assert!(failed(decider(&choice, DeciderWire::Systemone).offer(&question(opts(&["a"])))).contains("no choice"));
}

#[test]
fn a_limit_or_a_busy_gateway_is_tried_again_and_then_it_is_a_failure() {
    let mock = Mock::start(|_, n| if n < 2 { Say::Json(503, json!({ "error": "busy" })) } else { judgment(json!({ "q": { "type": "noul", "noul": 0.9 } })) });
    assert_eq!(said(decider(&mock, DeciderWire::Systemone).offer(&question(AskShape::YesNo))).0, Answer::Yes);
    assert_eq!(mock.seen().len(), 3);
    let down = Mock::start(|_, _| Say::Json(429, json!({ "error": { "message": "slow" } })));
    let why = failed(decider(&down, DeciderWire::Systemone).offer(&question(AskShape::YesNo)));
    assert!(why.contains("HTTP 429") && why.contains("slow"), "{why}");
    assert_eq!(down.seen().len(), 3, "three tries");
    let gone = Mock::start(|_, _| Say::Drop);
    assert!(failed(decider(&gone, DeciderWire::Systemone).offer(&question(AskShape::YesNo))).contains("could not reach it"));
    assert_eq!(gone.seen().len(), 3);
}

// ---- the model answerer ----

fn model_answerer(mock: &Mock) -> ModelAnswerer {
    let client = Client::new(Box::new(Openai), net(), &format!("{}/v1", mock.url()), "k-secret");
    ModelAnswerer::new(ModelConfig { id: "light".into(), model: "vendor/small".into(), zone: None, base: mock.url() }, client)
}

fn chat(text: &str) -> Say {
    Say::Sse(vec![json!({ "choices": [{ "delta": { "content": text } }] }).to_string(), json!({ "choices": [{ "delta": {}, "finish_reason": "stop" }] }).to_string()])
}

#[test]
fn a_model_is_asked_for_one_json_object_and_nothing_else() {
    let mock = Mock::start(|_, _| chat("Sure!\n```json\n{\"answer\": \"risky\", \"confidence\": 0.7, \"why\": \"the build is red\"}\n```"));
    let m = model_answerer(&mock);
    let (a, c) = said(m.offer(&question(opts(&["safe", "risky"]))));
    assert_eq!((a, (c * 10.0).round()), (Answer::Pick { id: "risky".into() }, 7.0));
    let req = mock.seen()[0].json();
    assert_eq!(req["max_tokens"], 512);
    assert!(req["messages"][0]["content"].as_str().unwrap().contains("only a JSON object"));
    let ask = req["messages"][1]["content"].as_str().unwrap();
    assert!(ask.contains("which one?") && ask.contains("fix the build") && ask.contains("- safe: the safe way") && ask.contains("- risky: the risky way"), "{ask}");
    assert!(req.get("tools").is_none());
    assert_eq!((m.id(), m.model().as_str()), ("model:light", "vendor/small"));
    let yes = Mock::start(|_, _| chat("{\"answer\":\"Yes\",\"confidence\":0.95,\"why\":\"x\"}"));
    assert_eq!(said(model_answerer(&yes).offer(&question(AskShape::YesNo))).0, Answer::Yes);
}

#[test]
fn a_model_that_does_not_answer_as_asked_has_failed() {
    for (reply, says) in [
        ("I think the second.", "not the JSON object"),
        ("{\"answer\":\"maybe\",\"confidence\":0.5}", "not one that was offered"),
        ("{\"answer\":\"safe\"}", "no confidence"),
        ("", "not the JSON object"),
    ] {
        let mock = Mock::start(move |_, _| chat(reply));
        let shape = if reply.contains("maybe") { AskShape::YesNo } else { opts(&["safe", "risky"]) };
        let why = failed(model_answerer(&mock).offer(&question(shape)));
        assert!(why.contains(says), "{reply}: {why}");
    }
    let down = Mock::start(|_, _| Say::Text(500, "oops".into()));
    assert!(failed(model_answerer(&down).offer(&question(AskShape::YesNo))).contains("HTTP 500"));
    let none = Mock::start(|_, _| chat("{}"));
    assert!(matches!(model_answerer(&none).offer(&question(AskShape::Text)), Offer::Pass { .. }));
    assert!(none.seen().is_empty());
}

#[test]
fn a_decider_says_what_its_reply_reported_it_cost_and_otherwise_it_is_unknown() {
    // tokens only (the shape of `judgment`): the tokens are known, the price is not, and it is not guessed
    let mock = Mock::start(|_, _| judgment(json!({ "q": { "type": "noul", "noul": 0.9 } })));
    let (offer, paid) = decider(&mock, DeciderWire::Systemone).offer_paid(&question(AskShape::YesNo));
    assert_eq!(said(offer).0, Answer::Yes);
    assert_eq!(paid, Some(Paid { usd: None, tokens_in: 9, tokens_out: 0 }));
    // a usage with a price
    let priced = Mock::start(|_, _| Say::Json(200, json!({ "answers": { "q": { "type": "noul", "noul": 0.9 } }, "usage": { "input_tokens": 120, "output_tokens": 7, "cost_usd": 0.0004 } })));
    let (_, paid) = decider(&priced, DeciderWire::Systemone).offer_paid(&question(AskShape::YesNo));
    assert_eq!(paid, Some(Paid { usd: Some(0.0004), tokens_in: 120, tokens_out: 7 }));
    // no usage at all: nothing is known of it
    let bare = Mock::start(|_, _| Say::Json(200, json!({ "answers": { "q": { "type": "noul", "noul": 0.9 } } })));
    let (_, paid) = decider(&bare, DeciderWire::Systemone).offer_paid(&question(AskShape::YesNo));
    assert_eq!(paid, Some(Paid { usd: None, tokens_in: 0, tokens_out: 0 }));
    // a reply that could not be read was still a reply that was paid for
    let odd = Mock::start(|_, _| Say::Json(200, json!({ "answers": {}, "usage": { "input_tokens": 5, "output_tokens": 1 } })));
    let (offer, paid) = decider(&odd, DeciderWire::Systemone).offer_paid(&question(AskShape::YesNo));
    failed(offer);
    assert_eq!(paid.map(|p| (p.usd, p.tokens_in)), Some((None, 5)));
    // nothing was sent, or nothing came back: nothing was billed, and that is known
    let (offer, paid) = decider(&mock, DeciderWire::Systemone).offer_paid(&question(AskShape::Text));
    assert!(matches!(offer, Offer::Pass { .. }));
    assert_eq!(paid, Some(Paid { usd: Some(0.0), tokens_in: 0, tokens_out: 0 }));
    let down = Mock::start(|_, _| Say::Text(400, "no".into()));
    let (_, paid) = decider(&down, DeciderWire::Systemone).offer_paid(&question(AskShape::YesNo));
    assert_eq!(paid.and_then(|p| p.usd), Some(0.0));
}

#[test]
fn a_model_says_what_its_usage_reported_it_cost_and_otherwise_it_is_unknown() {
    let said_cost = |usage: Value| {
        let usage = usage.clone();
        Mock::start(move |_, _| {
            let words = "{\"answer\": \"safe\", \"confidence\": 0.9, \"why\": \"x\"}";
            Say::Sse(vec![
                json!({ "choices": [{ "delta": { "content": words } }] }).to_string(),
                json!({ "choices": [{ "delta": {}, "finish_reason": "stop" }] }).to_string(),
                json!({ "choices": [], "usage": usage }).to_string(),
            ])
        })
    };
    let priced = said_cost(json!({ "prompt_tokens": 120, "completion_tokens": 7, "cost": 0.0003 }));
    let (offer, paid) = model_answerer(&priced).offer_paid(&question(opts(&["safe", "risky"])));
    assert_eq!(said(offer).0, Answer::Pick { id: "safe".into() });
    assert_eq!(paid, Some(Paid { usd: Some(0.0003), tokens_in: 120, tokens_out: 7 }));
    let unpriced = said_cost(json!({ "prompt_tokens": 120, "completion_tokens": 7 }));
    let (_, paid) = model_answerer(&unpriced).offer_paid(&question(opts(&["safe", "risky"])));
    assert_eq!(paid, Some(Paid { usd: None, tokens_in: 120, tokens_out: 7 }), "a gateway that gives no price leaves it unknown");
    let none = Mock::start(|_, _| chat("{}"));
    let (_, paid) = model_answerer(&none).offer_paid(&question(AskShape::Text));
    assert_eq!(paid, Some(Paid { usd: Some(0.0), tokens_in: 0, tokens_out: 0 }), "nothing was sent for a text");
}

// ---- the walk, with real answerers in it ----

fn tables() -> Tables {
    Tables::load(vec![base_layer()]).0
}

fn point(id: &str) -> AskPoint {
    points(&tables()).0.into_iter().find(|p| p.id == id).unwrap()
}

fn gate() -> Gate {
    Gate::new(Gates::load(&tables()).0, Grants::new("s"))
}

#[test]
fn where_a_question_is_going_decides_whether_it_goes() {
    let gateway = Mock::start(|_, _| judgment(json!({ "q": { "type": "choice", "choice": "a", "confidence": 0.9 } })));
    // a gateway on another host, reached by the proxy that stands in for the network
    let proxy = Fixture::start(gateway.addr);
    let remote = |zone: Option<&str>| {
        Arc::new(Decider::new(DeciderConfig {
            id: "jev".into(),
            wire: DeciderWire::Systemone,
            base: format!("http://gateway.test:{}", gateway.addr.port()),
            key: "k".into(),
            model: "jev-latest".into(),
            zone: zone.map(String::from),
            net: net().proxied(Proxy::Url(proxy.url())),
            backoff: vec![],
        }))
    };
    let p = point("pick");
    let mut q = question(opts(&["a", "b"]));
    q.point = "pick".into();
    let steps = |notes: &[j4n_core::answer::chain::Note], kind: &str| notes.iter().filter(|n| n.kind == kind).count();
    let state = Answerers::new();
    let mk = |reg: Registry| walk(&p, &q, &reg, &state, &Ledger::new(), 1_000, true);

    // project data may go to a remote zone: the question leaves, and the Log is told that it did
    let mut reg = Registry::new().with_egress(Arc::new(gate()));
    reg.register(remote(None));
    let (w, notes) = mk(reg);
    assert!(matches!(w, Walk::Answered(_)), "{w:?}");
    assert_eq!(steps(&notes, "ask.egress"), 1);
    let eg = notes.iter().find(|n| n.kind == "ask.egress").unwrap();
    assert_eq!((eg.data["zone"].clone(), eg.data["class"].clone(), eg.data["answerer"].clone()), (json!("any"), json!("project"), json!("decider:jev")));
    assert_eq!(proxy.calls().len(), 1);

    // private data does not go to a remote zone without a yes, which a decider cannot be given: skipped, nothing sent
    let mut private = p.clone();
    private.data = j4n_core::schema::DataClass::Private;
    let mut reg = Registry::new().with_egress(Arc::new(gate()));
    reg.register(remote(None));
    let (w, notes) = walk(&private, &q, &reg, &state, &Ledger::new(), 1_000, true);
    assert!(matches!(w, Walk::Fallback(_)), "{w:?}");
    assert_eq!((steps(&notes, "ask.egress"), proxy.calls().len()), (0, 1));
    let skipped = notes.iter().find(|n| n.data["answerer"] == "decider:jev" && n.data["outcome"] == "skipped").unwrap();
    assert!(skipped.data["why"].as_str().unwrap().contains("needs a yes"), "{skipped:?}");

    // secret data never goes anywhere
    let mut secret = p.clone();
    secret.data = j4n_core::schema::DataClass::Secret;
    let (_, notes) = walk(&secret, &q, &reg, &state, &Ledger::new(), 1_000, true);
    assert!(notes.iter().any(|n| n.data["why"].as_str().is_some_and(|w| w.contains("blocked"))));
    assert_eq!(proxy.calls().len(), 1);

    // with nothing to check it, an answerer with a zone is not called at all
    let mut bare = Registry::new();
    bare.register(remote(None));
    let (w, notes) = walk(&p, &q, &bare, &state, &Ledger::new(), 1_000, true);
    assert!(matches!(w, Walk::Fallback(_)));
    assert!(notes.iter().any(|n| n.data["why"].as_str().is_some_and(|w| w.contains("nothing checks"))));
    assert_eq!(proxy.calls().len(), 1);

    // and a gateway on this machine is the local zone, where private data may go
    let local = Arc::new(decider(&gateway, DeciderWire::Systemone));
    let mut reg = Registry::new().with_egress(Arc::new(gate()));
    reg.register(local);
    let (w, notes) = walk(&private, &q, &reg, &state, &Ledger::new(), 1_000, true);
    assert!(matches!(w, Walk::Answered(_)), "{w:?}");
    assert_eq!(notes.iter().find(|n| n.kind == "ask.egress").unwrap().data["zone"], "local");
    assert_eq!(zone_of("http://[::1]:9/x", Some("trusted")), "local");
    assert_eq!(zone_of("https://api.example.com/v1", Some("trusted")), "trusted");
    assert_eq!(zone_of("https://api.example.com/v1", None), "any");
}

#[test]
fn a_decider_that_keeps_failing_is_not_called_and_nothing_leaves_after_the_third() {
    let gateway = Mock::start(|_, _| Say::Json(500, json!({ "error": "down" })));
    let dir = Scratch::new("s10");
    let clock = Arc::new(ManualClock::at(1_000));
    let (log, problems) = Log::open(&dir.join(".j4n"), Scope::Project, Opts { segment_bytes: 8 << 20, max_line: 1 << 20, checkpoint_every: 1000, clock: clock.clone() }, Redactor::new()).unwrap();
    assert!(problems.is_empty());
    let state = Answerers::new();
    let folds = Folds::new(&dir.join("folds"));
    assert!(folds.add(Box::new(state.clone()), 0).is_empty());
    folds.attach(&log).unwrap();
    let mut reg = Registry::new().with_egress(Arc::new(gate()));
    reg.register(Arc::new(Decider::new(DeciderConfig { backoff: vec![], ..decider_cfg(&gateway) })));
    let p = point("retry");
    let q = |i: usize| Question { ask: format!("retry#{i}"), point: "retry".into(), asker: "front".into(), thread: None, text: format!("how heavy {i}"), shape: opts(&["retry", "escalate", "stop"]), goal: "x".into(), act: None };
    let ask = |i: usize| {
        let (w, notes) = walk(&p, &q(i), &reg, &state, &Ledger::new(), clock.now_ms(), true);
        for n in notes {
            log.append(New::of(&n.kind, Actor::Core, n.data)).unwrap();
        }
        log.sync();
        w
    };
    for i in 0..3 {
        clock.advance(1_000);
        assert!(matches!(ask(i), Walk::Fallback(_)), "the fallback stands in");
    }
    assert_eq!(gateway.seen().len(), 3);
    let egress = log.read(0).filter(|e| e.kind == "ask.egress").count();
    assert_eq!(egress, 3);
    clock.advance(1_000);
    assert!(matches!(ask(3), Walk::Fallback(_)));
    assert_eq!(gateway.seen().len(), 3, "the fourth ask makes no call");
    assert_eq!(log.read(0).filter(|e| e.kind == "ask.egress").count(), 3, "and no egress event");
}

fn decider_cfg(mock: &Mock) -> DeciderConfig {
    DeciderConfig { id: "jev".into(), wire: DeciderWire::Systemone, base: mock.url(), key: "k".into(), model: "jev-latest".into(), zone: None, net: net(), backoff: vec![] }
}

#[test]
fn keyless_the_answers_are_the_fallbacks() {
    // no decider and no model installed: every point still answers, by its rule, its rank or its fallback
    let reg = Registry::new().with_egress(Arc::new(gate()));
    let state = Answerers::new();
    for id in ["pick", "retry"] {
        let p = point(id);
        let q = Question { ask: format!("{id}#1"), point: id.into(), asker: "front".into(), thread: None, text: "q".into(), shape: opts(&["none", "retry", "escalate", "stop"]), goal: "g".into(), act: None };
        let (w, notes) = walk(&p, &q, &reg, &state, &Ledger::new(), 1_000, true);
        assert!(matches!(w, Walk::Fallback(_)), "{id}: {w:?}");
        assert!(notes.iter().all(|n| n.kind != "ask.egress"), "{id}: nothing left");
    }
}
