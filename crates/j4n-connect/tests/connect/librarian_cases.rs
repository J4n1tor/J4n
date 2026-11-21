//! The tool `context` is offered to a model that has tools, and to the cells of a doer through the bridge, only while the connectors hold the
//! librarian's shelf (`librarian.on`); what it is called with reaches the librarian as the task, the want and the reason, and what it gives back is the
//! librarian's answer or why not.

use crate::world::Scratch;
use j4n_connect::manifest::Connectors;
use j4n_connect::model::Call;
use j4n_core::front::librarian::{More, Shelf};
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

[[connector]]
id = "bare"
doc = "A model with no tools."
is = "agent"
dialect = "openai"
url = "http://127.0.0.1:1/v1"
keyless = true
models = ["m"]

[[connector]]
id = "claude"
doc = "A program with a shell of its own."
is = "agent"
dialect = "claude-stream"
program = "claude"
"#;

/// What the fake librarian was asked, as `task/want/why`.
#[derive(Default)]
struct Fake(Mutex<Vec<String>>);

impl More for Fake {
    fn menu(&self, _: &str) -> String {
        "menu".into()
    }

    fn add(&self, task: &str, want: &str, why: &str) -> Result<String, String> {
        self.0.lock().unwrap().push(format!("{task}/{want}/{why}"));
        if want == "nothing" { Err("there is nothing called `nothing`".into()) } else { Ok(format!("the text of {want}")) }
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
fn context_is_in_the_offer_only_with_the_shelf_and_reaches_the_librarian_with_the_task_the_want_and_the_why() {
    let dir = Scratch::new("cx7");
    let site = Site::new("t-7", dir.path());
    let secret = |_: &str, _: &str| Err::<String, String>("no secret".into());
    let names = |list: &Connectors| -> Vec<String> { list.bridge_offer(list.get("own").unwrap(), &secret).specs().into_iter().map(|t| t.name).collect() };
    // off: no shelf, the tool is in no offer and nobody can ask
    let mut list = load();
    assert!(!names(&list).contains(&"context".to_string()));
    assert!(list.context_doers().is_empty());
    // on: the models that have tools can ask; a model with no tools and a program with no bridge (the kit is off) cannot
    let shelf = Arc::new(Shelf::default());
    list.set_librarian(Some(shelf.clone()));
    assert!(names(&list).contains(&"context".to_string()), "code in the cells of a doer reaches it through the bridge's offer: {:?}", names(&list));
    assert_eq!(list.context_doers(), ["own"]);
    // a program of its own can ask when the kit gives it the bridge (`kit.mode` on, and a python3 to run it), and not otherwise
    list.set_kit(true);
    let kit = if j4n_connect::py::python_here() { vec!["claude", "own"] } else { vec!["own"] };
    assert_eq!(list.context_doers(), kit);
    list.set_kit(false);
    list.set_librarian(None);
    assert!(list.context_doers().is_empty(), "no shelf, nobody asks, kit or no kit");
    list.set_librarian(Some(shelf.clone()));
    // called before the front has put the librarian on the shelf, it says so; then it reaches the librarian
    let offer = list.bridge_offer(list.get("own").unwrap(), &secret);
    let call = |want: &str| Call { id: "c1".into(), name: "context".into(), args: json!({ "want": want, "why": "to name the module" }), bad: None };
    let out = offer.run(&call("project"), &site, &|l| l);
    assert!(out.error && out.text.contains("not there yet"), "{}", out.text);
    let fake = Arc::new(Fake::default());
    shelf.put(fake.clone());
    let out = offer.run(&call("project"), &site, &|l| l);
    assert!(!out.error && out.text == "the text of project", "{}", out.text);
    assert_eq!(fake.0.lock().unwrap().as_slice(), ["t-7/project/to name the module"]);
    let out = offer.run(&call("nothing"), &site, &|l| l);
    assert!(out.error && out.text.contains("nothing called"), "{}", out.text);
    // and it asks the Gate nothing: it reads the project's own facts
    assert!(offer.gate(&call("project"), "t-7", &site, j4n_core::gate::ReadOnly::No).is_none());
}
