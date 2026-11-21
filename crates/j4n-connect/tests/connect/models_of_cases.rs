//! The models a doer has: a connector's own `models`, what its `list` prints and what an `openai`-shaped
//! endpoint's `/models` says, kept for a day and read again on a miss.

use crate::mock::{Mock, Say};
use j4n_connect::manifest::Connector;
use j4n_connect::modellist::{ModelsOf, from_endpoint, from_table};
use j4n_core::front::models::ModelLists;
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

fn row(toml: &str) -> Connector {
    toml::from_str(toml).unwrap()
}

const PI: &str = "provider    model                       context  max-out\nopenrouter  ~deepseek/deepseek-pro-latest  1.0M     393.2K\nopenrouter  deepseek/deepseek-v4.1-flash  1.0M     131.1K\nopenrouter  deepseek/deepseek-v4.1-flash  1.0M     131.1K\n";

#[test]
fn a_table_gives_its_model_column_and_other_output_its_first_word() {
    assert_eq!(from_table(PI), ["~deepseek/deepseek-pro-latest", "deepseek/deepseek-v4.1-flash"]);
    assert_eq!(from_table("alpha one\n\n# note\nbeta two\n"), ["alpha", "beta"]);
    assert!(from_table("  \n").is_empty());
    assert_eq!(from_endpoint(r#"{"data":[{"id":"a/b"},{"id":"c"},{"no":1}]}"#).unwrap(), ["a/b", "c"]);
    assert!(from_endpoint("<html>").is_err() && from_endpoint("{}").is_err());
}

#[test]
fn the_fixed_words_and_the_listed_models_are_one_list_read_once_a_day_and_again_on_a_miss() {
    let pi = row("id = \"pi\"\nis = \"agent\"\ndialect = \"pi-rpc\"\nmodels = [\"fixed/one\"]\nlist = { argv = [\"pi\", \"--list-models\"] }\n");
    let claude = row("id = \"claude\"\nis = \"agent\"\ndialect = \"claude-stream\"\nmodels = [\"haiku\", \"sonnet\", \"opus\"]\n");
    let reads = Arc::new(AtomicUsize::new(0));
    let counted = reads.clone();
    let read = Arc::new(move |c: &Connector| {
        let n = counted.fetch_add(1, Ordering::SeqCst);
        assert_eq!(c.id, "pi", "only a connector with a way to list is asked");
        Ok(if n == 0 { from_table(PI) } else { vec!["new/model".to_string()] })
    });
    let of = ModelsOf::with_reader(&[pi, claude], read);
    assert_eq!(of.models("claude").unwrap(), ["haiku", "sonnet", "opus"]);
    assert_eq!(reads.load(Ordering::SeqCst), 0, "claude's words are fixed");
    let first = of.models("pi").unwrap();
    assert_eq!(first, ["fixed/one", "~deepseek/deepseek-pro-latest", "deepseek/deepseek-v4.1-flash"]);
    assert_eq!(of.models("pi").unwrap(), first);
    assert_eq!(reads.load(Ordering::SeqCst), 1, "kept: the turn path reads nothing");
    assert_eq!(of.models("nobody").unwrap(), Vec::<String>::new(), "a doer with no connector lists no models");
}

#[test]
fn a_list_a_day_old_is_read_again_and_one_that_cannot_be_read_leaves_the_last_in_place() {
    let pi = row("id = \"pi\"\nis = \"agent\"\ndialect = \"pi-rpc\"\nlist = { argv = [\"pi\", \"--list-models\"] }\n");
    let reads = Arc::new(AtomicUsize::new(0));
    let counted = reads.clone();
    let read = Arc::new(move |_: &Connector| match counted.fetch_add(1, Ordering::SeqCst) {
        0 => Ok(vec!["a/one".to_string()]),
        _ => Err("pi did not answer".to_string()),
    });
    let of = ModelsOf::with_reader(&[pi], read).lasting(Duration::ZERO);
    assert_eq!(of.models("pi").unwrap(), ["a/one"]);
    assert_eq!(of.models("pi").unwrap(), ["a/one"], "read again, failed, and the last list stays");
    assert_eq!(reads.load(Ordering::SeqCst), 2);
}

#[test]
fn an_openai_shaped_endpoint_lists_its_models_from_its_models_path_with_its_key() {
    let gateway = Mock::start(|seen, _| {
        assert_eq!(seen.path, "/v1/models");
        assert_eq!(seen.headers.get("authorization").map(String::as_str), Some("Bearer sk-test"));
        Say::Json(200, json!({ "data": [{ "id": "acme/big-1" }, { "id": "acme/big-2" }] }))
    });
    let c = row(&format!("id = \"own\"\nis = \"model\"\ndialect = \"openai\"\nurl = \"{}/v1\"\nkey = \"env:OWN_KEY\"\nmodels = [\"acme/big-1\"]\n", gateway.url()));
    let of = ModelsOf::new(&[c], Arc::new(|_, _| Ok("sk-test".into())));
    assert_eq!(of.models("own").unwrap(), ["acme/big-1", "acme/big-2"]);
    assert_eq!(gateway.seen().len(), 1);
}

// ---- the free model lists ----

use j4n_connect::modellist::{ListSource, Listing};
use j4n_core::listing::{Cache, DAY_MS, ROUTER};
use j4n_core::setting::{Live, Settings};
use std::sync::Mutex;

/// A source that touches no net and no program: it says what it is told and counts what it was asked.
struct Fake {
    asked: Mutex<Vec<String>>,
    router: Result<Vec<String>, String>,
    installed: bool,
}

impl Fake {
    fn new(router: Result<Vec<String>, String>, installed: bool) -> Arc<Fake> {
        Arc::new(Fake { asked: Mutex::new(Vec::new()), router, installed })
    }
    fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }
}

impl ListSource for Fake {
    fn router(&self) -> Result<Vec<String>, String> {
        self.asked.lock().unwrap().push("router".into());
        self.router.clone()
    }
    fn doer(&self, c: &Connector) -> Result<Vec<String>, String> {
        self.asked.lock().unwrap().push(c.id.clone());
        Ok(vec![format!("{}/big", c.id)])
    }
    fn installed(&self, _: &Connector) -> bool {
        self.installed
    }
}

fn pi() -> Connector {
    row("id = \"pi\"\nis = \"agent\"\ndialect = \"pi-rpc\"\nlist = { argv = [\"pi\", \"--list-models\"] }\n")
}

fn net(on: bool) -> Live {
    let (tables, _) = j4n_core::table::Tables::load(vec![j4n_core::table::Layer::new(
        j4n_core::table::LayerKind::Pack,
        "t",
        vec![j4n_core::table::SourceFile { path: "s.toml".into(), text: format!("api = 1\n[[setting]]\nid = \"watch.upstream\"\ndoc = \"x\"\nty = \"bool\"\ndefault = {on}\n") }],
    )]);
    Live::new(Settings::load(&tables).0)
}

fn listing(home: &crate::world::Scratch, source: &Arc<Fake>, on: bool) -> Listing {
    Listing::new(Cache::new(home.path()), &[pi()], source.clone(), Some(net(on)))
}

#[test]
fn a_pass_reads_openrouters_list_only_when_the_net_is_allowed_and_a_doers_only_when_it_is_installed() {
    let home = crate::world::Scratch::new("t575-pass");
    let source = Fake::new(Ok(vec!["a/one".into(), "b/two".into()]), false);
    // the net is not allowed and pi is not installed: nothing is asked
    assert!(listing(&home, &source, false).pass(1_000).asked.is_empty());
    assert!(source.asked().is_empty() && Cache::new(home.path()).read().is_empty());
    // the net is allowed: OpenRouter's list is read; pi is still not installed
    let got = listing(&home, &source, true).pass(2_000);
    assert_eq!(got.asked, [(ROUTER.to_string(), Ok(2))]);
    assert_eq!(source.asked(), ["router"]);
    let kept = Cache::new(home.path()).read();
    assert_eq!((kept[ROUTER].at_ms, kept[ROUTER].models.clone()), (2_000, vec!["a/one".to_string(), "b/two".to_string()]));
    // pi is installed, the net is off: pi's own list is read, OpenRouter's is not asked again
    let source = Fake::new(Ok(Vec::new()), true);
    let got = listing(&home, &source, false).pass(3_000);
    assert_eq!(got.asked, [("pi".to_string(), Ok(1))]);
    assert_eq!(Cache::new(home.path()).read()["pi"].models, ["pi/big"]);
}

#[test]
fn a_list_is_read_at_most_once_a_day_and_one_that_failed_keeps_the_last_and_waits_the_day() {
    let home = crate::world::Scratch::new("t575-day");
    let ok = Fake::new(Ok(vec!["a/one".into()]), true);
    listing(&home, &ok, true).pass(1_000);
    assert_eq!(ok.asked(), ["router", "pi"]);
    // an hour later nothing is due
    assert!(listing(&home, &ok, true).pass(1_000 + 3_600_000).asked.is_empty());
    assert_eq!(ok.asked().len(), 2);
    // a day later the net fails: the list stays, and it is not asked again until another day
    let down = Fake::new(Err("no route".into()), true);
    let got = listing(&home, &down, true).pass(1_000 + DAY_MS);
    assert_eq!(got.asked[0], (ROUTER.to_string(), Err("no route".to_string())));
    assert_eq!(Cache::new(home.path()).read()[ROUTER].models, ["a/one"]);
    assert!(listing(&home, &down, true).pass(1_000 + DAY_MS + 3_600_000).asked.is_empty());
    assert_eq!(down.asked(), ["router", "pi"]);
}

#[test]
fn a_word_is_matched_against_a_list_the_cache_holds_without_asking_the_program() {
    let home = crate::world::Scratch::new("t575-seed");
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
    Cache::new(home.path()).put("pi", now, &["cached/model-9".to_string()]).unwrap();
    let reads = Arc::new(AtomicUsize::new(0));
    let counted = reads.clone();
    let read = Arc::new(move |_: &Connector| {
        counted.fetch_add(1, Ordering::SeqCst);
        Ok(vec!["fresh/model".to_string()])
    });
    let of = ModelsOf::with_reader(&[pi()], read).cached(Cache::new(home.path()));
    assert_eq!(of.models("pi").unwrap(), ["cached/model-9"]);
    assert_eq!(reads.load(Ordering::SeqCst), 0, "a list read within the day is not read again on a word");
    // a list two days old is not taken: the program is asked, as before
    Cache::new(home.path()).put("pi", now - 2 * DAY_MS, &["old/model".to_string()]).unwrap();
    let of = ModelsOf::with_reader(&[pi()], Arc::new(|_: &Connector| Ok(vec!["fresh/model".to_string()]))).cached(Cache::new(home.path()));
    assert_eq!(of.models("pi").unwrap(), ["fresh/model"]);
}

#[test]
fn a_connector_reads_its_own_key_file_and_then_the_one_kept_for_its_provider() {
    let dir = crate::world::Scratch::new("t574-read");
    let text = "api = 1\n[[connector]]\nid = \"late\"\nis = \"model\"\ndialect = \"openai\"\nurl = \"https://acme.example/v1\"\nkey = \"env:J4N_J574_CONNECT_NOBODY_SETS_KEY\"\nmodels = [\"m\"]\n";
    let file = j4n_core::table::SourceFile { path: "connectors/late.toml".into(), text: text.into() };
    let (mut list, problems) = j4n_connect::manifest::Connectors::load(&j4n_core::table::Tables::load(vec![j4n_core::table::Layer::new(j4n_core::table::LayerKind::Pack, "t", vec![file])]).0);
    assert!(problems.is_empty(), "{problems:?}");
    // `keep_keys` points the connector at its own file and at the provider's that names the same reference
    list.keep_keys(dir.path(), &[("acme".into(), "env:J4N_J574_CONNECT_NOBODY_SETS_KEY".into()), ("other".into(), "env:SOMETHING_ELSE".into())]);
    let c = list.all().iter().find(|c| c.id == "late").unwrap().clone();
    assert_eq!((c.kept.clone(), c.kept_for.clone()), (Some(dir.join("late")), Some(dir.join("acme"))));
    let secret = |_: &str, reference: &str| -> Result<String, String> { std::fs::read_to_string(reference.strip_prefix("file:").ok_or("nobody sets it")?).map(|v| v.trim().to_string()).map_err(|e| e.to_string()) };
    assert_eq!(c.api_key(&secret), "", "no key anywhere");
    dir.put("acme", "sk-for-the-provider-0001");
    assert_eq!(c.api_key(&secret), "sk-for-the-provider-0001", "the provider's key is the connector's");
    dir.put("late", "sk-its-own-0002");
    assert_eq!(c.api_key(&secret), "sk-its-own-0002", "its own file comes first");
}
