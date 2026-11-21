//! The tool `tell` is offered to a model that has tools, and to the cells of a doer through the bridge, only while the connectors hold the
//! post's mailbox (`chats.on` on a screen); a call reaches the front's post with the task, the chat and the words; the Gate answers it as any tool: a
//! `connect` to `chat:<id>`, allowed inside one account and blocked across accounts.

use crate::world::Scratch;
use j4n_connect::manifest::Connectors;
use j4n_connect::model::Call;
use j4n_core::front::messages::{Mailbox, Post};
use j4n_core::gate::{Gate, ReadOnly, check::Gates, grant::Grants};
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

/// What the fake front was told, as `task/chat/text`; chats `c4` is in the same quarantine and `c9` is not.
#[derive(Default)]
struct Fake(Mutex<Vec<String>>);

impl Post for Fake {
    fn reach(&self, _: &str, chat: &str) -> bool {
        chat != "c9"
    }

    fn tell(&self, task: &str, chat: &str, text: &str) -> Result<String, String> {
        self.0.lock().unwrap().push(format!("{task}/{chat}/{text}"));
        Ok(format!("m1 is queued for {chat}"))
    }
}

fn load() -> Connectors {
    let user = Layer::new(LayerKind::User, "mine", vec![SourceFile { path: "connectors/x.toml".into(), text: ROWS.into() }]);
    let (tables, problems) = Tables::load(vec![crate::bench::base_layer(), user]);
    assert!(problems.is_empty(), "{problems:?}");
    let (list, said) = Connectors::load(&tables);
    assert!(said.is_empty(), "{said:?}");
    list
}

#[test]
fn tell_is_in_the_offer_only_with_the_mailbox_reaches_the_post_and_the_gate_blocks_it_across_accounts() {
    let dir = Scratch::new("cx9b");
    let site = Site::new("t-7", dir.path());
    let secret = |_: &str, _: &str| Err::<String, String>("no secret".into());
    let offer = |list: &Connectors| list.bridge_offer(list.get("own").unwrap(), &secret);
    let names = |list: &Connectors| -> Vec<String> { offer(list).specs().into_iter().map(|t| t.name).collect() };
    // off: no mailbox, no doer is offered the tool
    let mut list = load();
    assert!(!names(&list).contains(&"tell".to_string()));
    // on: it is in the bridge's offer, so the bridge needed no line of its own
    let mailbox = Arc::new(Mailbox::default());
    list.set_post(Some(mailbox.clone()));
    assert!(names(&list).contains(&"tell".to_string()), "{:?}", names(&list));
    let call = |chat: &str| Call { id: "c1".into(), name: "tell".into(), args: json!({ "chat": chat, "text": "the parser's API changed" }), bad: None };
    // called before the front has put the post on the mailbox, it says so; then it reaches the post
    let out = offer(&list).run(&call("c4"), &site, &|l| l);
    assert!(out.error && out.text.contains("not there yet"), "{}", out.text);
    let fake = Arc::new(Fake::default());
    mailbox.put(fake.clone());
    let out = offer(&list).run(&call("c4"), &site, &|l| l);
    assert!(!out.error && out.text == "m1 is queued for c4", "{}", out.text);
    assert_eq!(fake.0.lock().unwrap().as_slice(), ["t-7/c4/the parser's API changed"]);
    // the Gate answers it as any tool: with no gate loaded nothing is asked; with the pack's rules, inside one account it is allowed and across accounts it is blocked
    assert!(offer(&list).gate(&call("c4"), "t-7", &site, ReadOnly::No).is_none());
    let (gates, problems) = Gates::load(&Tables::load(vec![crate::bench::base_layer()]).0);
    assert!(problems.is_empty(), "{problems:?}");
    list.set_gate(Gate::new(gates, Grants::new("s1")));
    assert!(offer(&list).gate(&call("c4"), "t-7", &site, ReadOnly::No).is_none(), "inside one project it is allowed by default");
    let blocked = offer(&list).gate(&call("c9"), "t-7", &site, ReadOnly::No).expect("another account is not allowed");
    assert_eq!((blocked.action.verb, blocked.action.target.as_str(), blocked.action.inside, blocked.risk), (j4n_core::gate::Verb::Connect, "chat:c9", Some(false), 100));
    // and the mailbox taken away again, nobody is offered it
    list.set_post(None);
    assert!(!names(&list).contains(&"tell".to_string()));
}
