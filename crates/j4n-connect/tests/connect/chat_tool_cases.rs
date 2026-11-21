//! The tool `chat` is offered to a model that has tools, and to the cells of a doer through the bridge, only while the connectors hold the post's mailbox and the start
//! said the word was `orchestrate`; a call reaches the front's post with the task and what it asked for, a call that asks to write is passed on as one, and the Gate
//! answers it as any tool: a `spawn` of `scout:<doer>`, allowed unless a rule of the operator says otherwise.

use crate::world::Scratch;
use j4n_connect::manifest::Connectors;
use j4n_connect::model::Call;
use j4n_core::front::messages::{Mailbox, Post};
use j4n_core::front::scout::Want;
use j4n_core::gate::{Gate, ReadOnly, Verb, check::Gates, grant::Grants};
use j4n_core::place::Site;
use j4n_core::table::{Layer, LayerKind, SourceFile, Tables};
use serde_json::json;
use std::sync::{Arc, Mutex};

const ROWS: &str = r#"api = 1
[[connector]]
id = "builtin"
doc = "The tools J4n carries."
is = "tool"
dialect = "builtin"

[[connector]]
id = "own"
doc = "A model with tools."
is = "agent"
dialect = "openai"
url = "http://127.0.0.1:1/v1"
keyless = true
tools = ["builtin"]
models = ["m"]
"#;

/// What the fake front was asked, as `task/goal/paths/doer/write`.
#[derive(Default)]
struct Fake(Mutex<Vec<String>>);

impl Post for Fake {
    fn reach(&self, _: &str, _: &str) -> bool {
        true
    }

    fn tell(&self, _: &str, _: &str, _: &str) -> Result<String, String> {
        Ok(String::new())
    }

    fn scout(&self, task: &str, want: &Want) -> Result<String, String> {
        self.0.lock().unwrap().push(format!("{task}/{}/{}/{}/{}", want.goal, want.paths.join(","), want.doer, want.write));
        if want.write { Err("a chat started this way only reads".into()) } else { Ok("c1 is started".into()) }
    }
}

fn load(extra: Option<&str>) -> Connectors {
    let mut layers = vec![crate::bench::base_layer(), Layer::new(LayerKind::User, "mine", vec![SourceFile { path: "connectors/x.toml".into(), text: ROWS.into() }])];
    if let Some(rule) = extra {
        layers.push(Layer::new(LayerKind::User, "rules", vec![SourceFile { path: "gates/x.toml".into(), text: rule.into() }]));
    }
    let (tables, problems) = Tables::load(layers);
    assert!(problems.is_empty(), "{problems:?}");
    let (list, said) = Connectors::load(&tables);
    assert!(said.is_empty(), "{said:?}");
    list
}

#[test]
fn chat_is_in_the_offer_only_with_the_post_and_the_word_and_a_call_reaches_the_front_as_a_read() {
    let dir = Scratch::new("om5");
    let site = Site::new("t-7", dir.path());
    let secret = |_: &str, _: &str| Err::<String, String>("no secret".into());
    let names = |list: &Connectors| -> Vec<String> { list.bridge_offer(list.get("own").unwrap(), &secret).specs().into_iter().map(|t| t.name).collect() };
    let mut list = load(None);
    let mailbox = Arc::new(Mailbox::default());
    // a post alone is `tell` alone; the word as well is `chat` too; the word alone, with no post, is nothing
    list.set_post(Some(mailbox.clone()));
    assert!(names(&list).contains(&"tell".to_string()) && !names(&list).contains(&"chat".to_string()), "{:?}", names(&list));
    assert!(list.scout_doers().is_empty());
    list.set_scouts(true);
    assert!(names(&list).contains(&"chat".to_string()), "{:?}", names(&list));
    assert!(list.scout_doers().contains(&"own".to_string()), "{:?}", list.scout_doers());
    list.set_post(None);
    assert!(!names(&list).contains(&"chat".to_string()) && list.scout_doers().is_empty());
    list.set_post(Some(mailbox.clone()));
    let offer = list.bridge_offer(list.get("own").unwrap(), &secret);
    let call = |args: serde_json::Value| Call { id: "c1".into(), name: "chat".into(), args, bad: None };
    let read = call(json!({ "goal": "how does the lexer count", "paths": ["src/lexer", " "], "doer": "pi" }));
    let out = offer.run(&read, &site, &|l| l);
    assert!(out.error && out.text.contains("not there yet"), "{}", out.text);
    let fake = Arc::new(Fake::default());
    mailbox.put(fake.clone());
    let out = offer.run(&read, &site, &|l| l);
    assert!(!out.error && out.text == "c1 is started", "{}", out.text);
    // whatever asks to write reaches the front marked as a write, and its refusal comes back as the error it is
    for args in [json!({ "goal": "g", "paths": ["a"], "writes": ["a/b.rs"] }), json!({ "goal": "g", "paths": ["a"], "mode": "write" }), json!({ "goal": "g", "paths": ["a"], "write": true })] {
        let out = offer.run(&call(args), &site, &|l| l);
        assert!(out.error && out.text.contains("only reads"), "{}", out.text);
    }
    assert_eq!(fake.0.lock().unwrap().as_slice(), ["t-7/how does the lexer count/src/lexer/pi/false", "t-7/g/a//true", "t-7/g/a//true", "t-7/g/a//true"]);
}

#[test]
fn the_gate_answers_chat_as_a_spawn_of_a_scout_and_a_rule_of_the_operator_can_ask_or_block_it() {
    let dir = Scratch::new("om5");
    let site = Site::new("t-7", dir.path());
    let secret = |_: &str, _: &str| Err::<String, String>("no secret".into());
    let rule = r#"api = 1
[[gate]]
id = "no-scouts-on-pi"
doc = "A scout is not started on pi."
on = ["spawn"]
when = "target == 'scout:pi'"
verdict = "block"
why = "no scout on pi"
[[gate]]
id = "ask-for-a-default-scout"
doc = "A scout on the default doer is asked first."
on = ["spawn"]
when = "target == 'scout:default'"
verdict = "ask"
why = "start a scout?"
risk = 30
"#;
    let mut list = load(Some(rule));
    list.set_post(Some(Arc::new(Mailbox::default())));
    list.set_scouts(true);
    let call = |doer: &str| Call { id: "c1".into(), name: "chat".into(), args: json!({ "goal": "g", "paths": ["a"], "doer": doer }), bad: None };
    let offer = |list: &Connectors| list.bridge_offer(list.get("own").unwrap(), &secret);
    assert!(offer(&list).gate(&call("pi"), "t-7", &site, ReadOnly::No).is_none(), "with no gate loaded nothing is asked");
    let (gates, problems) = Gates::load(&Tables::load(vec![crate::bench::base_layer(), Layer::new(LayerKind::User, "rules", vec![SourceFile { path: "gates/x.toml".into(), text: rule.into() }])]).0);
    assert!(problems.is_empty(), "{problems:?}");
    list.set_gate(Gate::new(gates, Grants::new("s1")));
    let blocked = offer(&list).gate(&call("pi"), "t-7", &site, ReadOnly::No).expect("a rule blocks it");
    assert_eq!((blocked.action.verb, blocked.action.target.as_str(), blocked.risk), (Verb::Spawn, "scout:pi", 100));
    let asked = offer(&list).gate(&call(""), "t-7", &site, ReadOnly::No).expect("a rule asks for the default doer");
    assert_eq!((asked.action.target.as_str(), asked.risk), ("scout:default", 30));
    assert!(offer(&list).gate(&call("claude"), "t-7", &site, ReadOnly::Explicit).is_none(), "nothing in the pack stops a scout of another doer, even from a step that only reads");
}
