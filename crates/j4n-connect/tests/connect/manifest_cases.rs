//! The `[[connector]]` rows: the kind, the struct that reads it, the rows the base pack and the pack `connect` ship, and what a
//! row that does not fit its dialect is told.

use j4n_connect::manifest::{Connectors, Dialect};
use j4n_core::table::{Layer, LayerKind, SourceFile, Tables};
use std::path::Path;

fn pack(dir: &str, name: &str) -> Layer {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs").join(dir);
    let (mut layer, read) = Layer::from_dir(LayerKind::Pack, name, &path);
    assert!(read.is_empty(), "{read:?}");
    // the kinds of the base pack are the core's own, compiled in; another pack declares its own
    layer.files.retain(|f| !(dir == "base" && f.path.starts_with("kinds/")) && f.path != "pack.toml");
    layer
}

fn rows(text: &str) -> Layer {
    Layer::new(LayerKind::User, "mine", vec![SourceFile { path: "connectors/mine.toml".into(), text: format!("api = 1\n{text}") }])
}

/// What loading `layers` said, and the connectors that came through.
fn load(layers: Vec<Layer>) -> (Connectors, Vec<String>) {
    let (tables, problems) = Tables::load(layers);
    let mut said: Vec<String> = problems.iter().map(ToString::to_string).collect();
    let (list, more) = Connectors::load(&tables);
    said.extend(more.iter().map(ToString::to_string));
    (list, said)
}

fn no_secret(_: &str, r: &str) -> Result<String, String> {
    Err(format!("no secret `{r}`"))
}

/// Law 4 at the edge, for the kinds this crate reads into a struct of its own.
#[test]
fn the_structs_read_every_example_of_their_kind_and_refuse_what_the_schema_refuses() {
    crate::world::examples_are_rows::<Connector>(2);
    crate::world::examples_are_rows::<j4n_connect::place::boxed::BoxRow>(1);
    crate::world::examples_are_rows::<j4n_connect::check::CheckRow>(1);
}

use j4n_connect::manifest::Connector;

#[test]
fn the_base_pack_connects_the_agents_it_was_built_for_and_the_test_doer() {
    let (list, said) = load(vec![pack("base", "base-data")]);
    assert!(said.is_empty(), "{said:?}");
    let ids: Vec<&str> = list.all().iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["claude", "omp", "pi", "test"]);
    let dialects: Vec<Dialect> = list.all().iter().map(|c| c.dialect).collect();
    assert_eq!(dialects, [Dialect::ClaudeStream, Dialect::PiRpc, Dialect::PiRpc, Dialect::Test]);
    let (doers, said) = list.doers(&no_secret);
    assert!(said.is_empty(), "{said:?}");
    assert_eq!(doers.iter().map(|d| d.id().to_string()).collect::<Vec<_>>(), ["claude", "omp", "pi", "test"]);
    assert!(doers.iter().find(|d| d.id() == "claude").unwrap().caps().live);
    assert!(!doers.iter().find(|d| d.id() == "test").unwrap().probe().detail.is_empty());
}

#[test]
fn the_base_packs_scripted_doer_is_good_for_no_role_an_election_asks_for() {
    let (list, said) = load(vec![pack("base", "base-data")]);
    assert!(said.is_empty(), "{said:?}");
    let test = list.all().iter().find(|c| c.id == "test").unwrap();
    // empty would mean good for any
    assert!(!test.roles.is_empty(), "a doer with no roles is elected for every role");
    let points = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/base/asks/points.toml")).unwrap();
    let asked = points.lines().filter_map(|l| l.trim().strip_prefix("id = \"who:")).filter_map(|l| l.strip_suffix('"'));
    let asked: Vec<&str> = asked.chain(["plan", "work", "scout", "review", "validate", "piece", "chore", "check"]).collect();
    assert!(test.roles.iter().all(|r| !asked.contains(&r.as_str())), "{:?} against {asked:?}", test.roles);
    // and no other connector row of the base pack leaves it out of its own election by being named as a default
    assert!(!points.contains("\"test\""), "no ask point of the base pack names the scripted doer");
}

#[test]
fn the_samples_of_the_connect_pack_load_and_make_doers() {
    let (list, said) = load(vec![pack("base", "base-data"), pack("connect", "connect-data")]);
    assert!(said.is_empty(), "{said:?}");
    for id in ["codex", "opencode", "opencode-acp", "gemini"] {
        assert!(list.get(id).is_some(), "{id}");
    }
    let (doers, said) = list.doers(&no_secret);
    assert!(said.is_empty(), "{said:?}");
    assert_eq!(doers.len(), 8);
}

#[test]
fn a_row_that_does_not_fit_its_dialect_is_left_out_and_told_why() {
    let cases: [(&str, &str); 15] = [
        ("dialect = \"text\"\nis = \"tool\"", "`program` is needed"),
        ("dialect = \"jsonl-map\"\nis = \"agent\"\nprogram = \"x\"\nstart = [\"{prompt}\"]", "`map`"),
        ("dialect = \"jsonl-map\"\nis = \"agent\"\nprogram = \"x\"\nstart = [\"{prompt}\"]\n[connector.map]\nend = \"/type ==\"", "expected a value"),
        ("dialect = \"jsonl-map\"\nis = \"agent\"\nprogram = \"x\"\nstart = [\"{prompt}\"]\n[connector.map]\nend = \"done\"", "done"),
        ("dialect = \"text\"\nis = \"agent\"\nprogram = \"x\"\nstart = [\"{nosuch}\"]", "`{nosuch}`"),
        ("dialect = \"pi-rpc\"\nis = \"agent\"\nprogram = \"x\"", "`family`"),
        ("dialect = \"mcp\"\nis = \"mcp\"\nprogram = \"x\"\nurl = \"https://x\"", "not both"),
        ("dialect = \"openai\"\nis = \"model\"", "talks to a `url`"),
        ("dialect = \"http-json\"\nis = \"tool\"", "starts with http"),
        ("dialect = \"openai\"\nis = \"model\"\nurl = \"ftp://x\"", "talks to a `url`"),
        ("dialect = \"systemone\"\nis = \"agent\"\nurl = \"https://x\"", "`decider`"),
        ("dialect = \"anthropic\"\nis = \"decider\"\nurl = \"https://x\"", "`model`"),
        ("dialect = \"openai\"\nis = \"model\"\nurl = \"https://x\"\nproxy = \"nowhere\"", "proxy URL"),
        ("dialect = \"openai\"\nis = \"model\"\nurl = \"https://x\"\nkey = \"sk-abc\"", "`env:NAME`, `file:/path`"),
        ("dialect = \"text\"\nis = \"model\"\nprogram = \"x\"\nargv = [\"x\"]", "only an `agent`, a `tool` or a `service`"),
    ];
    for (body, why) in cases {
        let (list, said) = load(vec![rows(&format!("[[connector]]\nid = \"bad\"\n{body}\n"))]);
        assert!(list.all().is_empty(), "{body}");
        assert!(said.iter().any(|s| s.contains(why)), "{body}\nexpected {why:?}: {said:?}");
    }
}

#[test]
fn one_bad_row_does_not_take_the_good_ones_down() {
    let (list, said) = load(vec![rows("[[connector]]\nid = \"bad\"\ndialect = \"text\"\nis = \"tool\"\n[[connector]]\nid = \"good\"\ndialect = \"text\"\nis = \"tool\"\nprogram = \"echo\"\nargv = [\"echo\", \"hi\"]\n")]);
    assert_eq!(list.all().iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["good"]);
    assert_eq!(said.len(), 1, "{said:?}");
}

#[test]
fn a_user_row_changes_one_field_of_a_base_row_and_leaves_the_rest() {
    let (list, said) = load(vec![pack("base", "base-data"), rows("[[connector]]\nid = \"pi\"\nprogram = \"/opt/pi/bin/pi\"\n")]);
    assert!(said.is_empty(), "{said:?}");
    let pi = list.get("pi").unwrap();
    assert_eq!((pi.program(), pi.start.clone()), ("/opt/pi/bin/pi", vec!["--mode".to_string(), "rpc".to_string()]));
}

#[test]
fn a_project_nobody_trusted_cannot_name_a_program() {
    let project = Layer::new(LayerKind::Project, "here", vec![SourceFile { path: "connectors/evil.toml".into(), text: "api = 1\n[[connector]]\nid = \"evil\"\nis = \"agent\"\ndialect = \"text\"\nprogram = \"sh\"\nstart = [\"-c\", \"{prompt}\"]\n".into() }]);
    let (tables, problems) = Tables::load(vec![pack("base", "base-data"), project]);
    let (list, _) = Connectors::load(&tables);
    assert!(list.get("evil").is_none(), "{:?}", list.all());
    assert!(problems.iter().any(|p| p.to_string().contains("connector")), "it says so: {problems:?}");
}

#[test]
fn environment_and_secrets_reach_the_child_and_a_secret_that_cannot_be_read_stops_that_connector_only() {
    let text = "[[connector]]\nid = \"keyed\"\nis = \"agent\"\ndialect = \"text\"\nprogram = \"echo\"\nstart = [\"hi\"]\nenv = { MODE = \"fast\" }\nsecrets = { MY_KEY = \"env:J4N_NO_SUCH\" }\n[[connector]]\nid = \"plain\"\nis = \"agent\"\ndialect = \"text\"\nprogram = \"echo\"\nstart = [\"hi\"]\n";
    let (list, said) = load(vec![rows(text)]);
    assert!(said.is_empty(), "{said:?}");
    let env = list.get("keyed").unwrap().environment(&|_, r| Ok(format!("value-of-{r}"))).unwrap();
    assert_eq!((env["MODE"].as_str(), env["MY_KEY"].as_str()), ("fast", "value-of-env:J4N_NO_SUCH"));
    let (doers, said) = list.doers(&no_secret);
    assert_eq!(doers.len(), 1);
    assert_eq!(said.len(), 1);
    assert!(said[0].to_string().contains("secret MY_KEY") && said[0].to_string().contains("[[connector]] keyed"), "{said:?}");
}

#[test]
fn every_dialect_the_schema_names_is_built() {
    for d in ["text", "json-last", "jsonl-map", "acp", "claude-stream", "pi-rpc", "test", "mcp", "http-json", "openai", "anthropic", "systemone", "decisions", "builtin"] {
        let (tables, problems) = Tables::load(vec![rows(&format!("[[connector]]\nid = \"x\"\nis = \"agent\"\ndialect = \"{d}\"\n"))]);
        assert!(problems.is_empty(), "{d}: the schema knows it: {problems:?}");
        let (_, said) = Connectors::load(&tables);
        assert!(!said.iter().any(|s| s.to_string().contains("is built in step")), "{d}: {said:?}");
    }
}

#[test]
fn the_probe_of_a_row_without_one_runs_the_program_with_version() {
    let (list, _) = load(vec![rows("[[connector]]\nid = \"sh\"\nis = \"agent\"\ndialect = \"text\"\nprogram = \"/no/such/program\"\nstart = [\"{prompt}\"]\n")]);
    let sh = list.get("sh").unwrap().probe(&Default::default());
    assert!(!sh.installed && sh.detail.contains("did not answer"), "{sh:?}");
    let (list, _) = load(vec![rows("[[connector]]\nid = \"ls\"\nis = \"tool\"\ndialect = \"text\"\nprogram = \"echo\"\nargv = [\"echo\"]\nprobe = { argv = [\"echo\", \"v9 extra\"], auth = [\"false\"] }\n")]);
    let p = list.get("ls").unwrap().probe(&Default::default());
    assert_eq!((p.installed, p.authed, p.version.as_deref(), p.detail.as_str()), (true, false, Some("v9"), "not logged in"));
}

#[test]
fn a_decider_or_model_with_no_key_makes_no_answerer_and_a_keyless_one_makes_one() {
    let text = "[[connector]]\nid = \"jev\"\nis = \"decider\"\ndialect = \"systemone\"\nurl = \"https://x.test\"\nkey = \"env:NOPE\"\n[[connector]]\nid = \"light\"\nis = \"model\"\ndialect = \"openai\"\nurl = \"http://127.0.0.1:1/v1\"\nkeyless = true\n[[connector]]\nid = \"big\"\nis = \"model\"\ndialect = \"anthropic\"\nurl = \"https://y.test\"\nkey = \"env:HERE\"\n";
    let (list, said) = load(vec![rows(text)]);
    assert!(said.is_empty(), "{said:?}");
    let ids = |have: &[&str]| {
        let have: Vec<String> = have.iter().map(|s| s.to_string()).collect();
        let mut v: Vec<String> = list.answerers(&move |_, r| if have.iter().any(|h| r == format!("env:{h}")) { Ok("k".into()) } else { Err("not set".into()) }).iter().map(|a| a.id().to_string()).collect();
        v.sort();
        v
    };
    assert_eq!(ids(&[]), ["model:light"], "no key: only the one that needs none");
    assert_eq!(ids(&["NOPE", "HERE"]), ["decider:jev", "model:big", "model:light"]);
    assert_eq!(ids(&["HERE"]), ["model:big", "model:light"]);
}

/// Where keys are read live, a decider or model with no key at the start is made and hidden; a key typed while J4n runs
/// (J4n's own key file) shows it to the very next question, with no new start, and removing the key hides it again.
#[test]
fn an_answerer_with_no_key_at_the_start_is_asked_once_its_key_is_typed() {
    let text = "[[connector]]\nid = \"jev\"\nis = \"decider\"\ndialect = \"systemone\"\nurl = \"https://x.test\"\nkey = \"env:J4N_J475_NOBODY_SETS\"\n[[connector]]\nid = \"light\"\nis = \"model\"\ndialect = \"openai\"\nurl = \"https://y.test/v1\"\nkey = \"env:J4N_J475_NOBODY_SETS\"\n";
    let (mut list, said) = load(vec![rows(text)]);
    assert!(said.is_empty(), "{said:?}");
    let dir = std::env::temp_dir().join(format!("j4n-t475-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    list.keep_keys(&dir, &[]);
    let read = |_: &str, r: &str| -> Result<String, String> { r.strip_prefix("file:").and_then(|p| std::fs::read_to_string(p).ok()).map(|v| v.trim().to_string()).ok_or_else(|| "not set".into()) };
    list.set_secret(std::sync::Arc::new(read));
    let mut reg = j4n_core::answer::chain::Registry::new();
    for a in list.answerers(&read) {
        reg.register(a);
    }
    for id in ["decider:jev", "model:light"] {
        assert!(reg.installed(id) && reg.get(id).is_none(), "{id}: made at the start with no key, and not asked");
    }
    std::fs::write(dir.join("jev"), "sk-t475-typed-later-77").unwrap();
    assert!(reg.get("decider:jev").is_some_and(|a| a.ready()), "the key typed while J4n runs: asked from the next question");
    assert!(reg.get("model:light").is_none(), "the other one still has no key");
    std::fs::write(dir.join("light"), "sk-t475-light-78").unwrap();
    assert!(reg.get("model:light").is_some());
    std::fs::remove_file(dir.join("jev")).unwrap();
    assert!(reg.get("decider:jev").is_none(), "removed: not asked, rather than sent with an empty key");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_probe_with_signed_is_logged_in_only_where_a_login_file_or_a_key_is_there() {
    let home = std::env::temp_dir().join(format!("j4n-signed-{}", std::process::id()));
    std::fs::create_dir_all(home.join(".agent")).unwrap();
    let (list, said) = load(vec![rows("[[connector]]\nid = \"ag\"\nis = \"tool\"\ndialect = \"text\"\nprogram = \"echo\"\nargv = [\"echo\"]\nprobe = { argv = [\"echo\", \"v1\"], signed = [\"~/.agent/creds\", \"$J4N_TEST_KEY_FOR_SIGNED\"] }\n")]);
    assert!(said.is_empty(), "{said:?}");
    let ag = list.get("ag").unwrap();
    let env = |extra: &[(&str, &str)]| -> std::collections::BTreeMap<String, String> { [("HOME", home.to_str().unwrap())].into_iter().chain(extra.iter().copied()).map(|(k, v)| (k.to_string(), v.to_string())).collect() };
    let p = ag.probe(&env(&[]));
    assert!(p.installed && !p.authed && p.detail == "not logged in", "{p:?}");
    assert!(ag.probe(&env(&[("J4N_TEST_KEY_FOR_SIGNED", "k")])).authed, "a key in the environment");
    assert!(!ag.probe(&env(&[("J4N_TEST_KEY_FOR_SIGNED", "")])).authed, "an empty key is none");
    std::fs::write(home.join(".agent/creds"), "x").unwrap();
    assert!(ag.probe(&env(&[])).authed, "a login file under the home");
    std::fs::remove_dir_all(&home).ok();
}

#[test]
fn a_connector_that_names_where_its_login_is_kept_is_logged_in_only_where_one_is() {
    let home = std::env::temp_dir().join(format!("j4n-login-{}", std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    let row = |extra: &str| format!("[[connector]]\nid = \"ag\"\nis = \"tool\"\ndialect = \"text\"\nprogram = \"echo\"\nargv = [\"echo\"]\nlogin = [\"~/.agent\", \"~/.agent.json\"]\nprobe = {{ argv = [\"echo\", \"v1\"]{extra} }}\n");
    let probe = |extra: &str, env: &[(&str, &str)]| {
        let (list, said) = load(vec![rows(&row(extra))]);
        assert!(said.is_empty(), "{said:?}");
        let env = [("HOME", home.to_str().unwrap())].into_iter().chain(env.iter().copied()).map(|(k, v)| (k.to_string(), v.to_string())).collect();
        list.get("ag").unwrap().probe(&env)
    };
    let p = probe("", &[]);
    assert!(p.installed && !p.authed, "no login in the home: {p:?}");
    // the state word, never the folders it looked in
    assert_eq!(p.detail, "not logged in", "the words name no path: {}", p.detail);
    assert!(probe(", auth = [\"echo\", \"ok\"]", &[]).authed, "a program that answers for itself is believed");
    assert!(!probe(", signed = [\"~/.other\"]", &[]).authed, "`signed` is the check when there is one");
    std::fs::write(home.join(".agent.json"), "{}").unwrap();
    assert!(probe("", &[]).authed, "any one of the login paths");
    std::fs::remove_file(home.join(".agent.json")).unwrap();
    std::fs::remove_dir_all(&home).ok();
}

#[test]
fn omp_and_pi_name_the_login_that_makes_them_candidates() {
    let (list, said) = load(vec![pack("base", "base")]);
    assert!(said.is_empty(), "{said:?}");
    for (id, login) in [("omp", "~/.omp"), ("pi", "~/.pi")] {
        let c = list.get(id).unwrap();
        assert_eq!(c.login, [login], "{id}");
        assert!(c.probe.signed.is_empty() && c.probe.auth.is_empty() && c.secrets.is_empty(), "{id}: nothing else says it is signed in, so the login path does");
    }
}

#[test]
fn codex_and_gemini_say_how_to_tell_they_are_signed_in() {
    let (list, said) = load(vec![pack("base", "base"), pack("connect", "connect")]);
    assert!(said.is_empty(), "{said:?}");
    // a program that only answers `--version` is not a doer that can work: codex asks its own `login status`, gemini looks for its login
    assert_eq!(list.get("codex").unwrap().probe.auth, ["codex", "login", "status"]);
    let signed = &list.get("gemini").unwrap().probe.signed;
    assert!(signed.iter().any(|s| s.starts_with("~/.gemini/")) && signed.iter().any(|s| s == "$GEMINI_API_KEY"), "{signed:?}");
}

// ---- a key J4n keeps, for a program that works on one (`kept:<provider>`) ----

/// A made-up key of no vendor's shape: what is proven is the kept-key path, not a pattern the redactor knows.
const KEPT: &str = "kept-canary-plain-7c41d09e5b";

/// What the wire's reader does with a reference: the redactor's own rules (a file others can read, a value too short, refused).
fn reader(red: &j4n_core::redact::Redactor) -> impl Fn(&str, &str) -> Result<String, String> + '_ {
    move |_, r| j4n_core::redact::resolve_in(r, red, None)
}

#[test]
fn a_kept_key_reaches_only_a_program_given_it_and_one_given_it_with_no_key_kept_is_refused_in_words() {
    let home = crate::world::Scratch::new("kept-env");
    let keys = j4n_core::keys::Keys::new(home.path());
    let red = j4n_core::redact::Redactor::new();
    let (mut list, said) = load(vec![pack("base", "base")]);
    assert!(said.is_empty(), "{said:?}");
    let read = reader(&red);
    // not given: no variable and no refusal, whether or not a key is kept, so a program on its sign-in is never handed one
    let env = list.get("claude").unwrap().environment(&read).unwrap();
    assert!(!env.contains_key("ANTHROPIC_API_KEY"), "{:?}", env.keys());
    // given, and no key kept: refused before the child, in words that lead to the row and name no value
    assert!(list.bind_keys(&keys, &["claude"]).is_empty());
    let why = list.get("claude").unwrap().environment(&read).unwrap_err();
    assert!(why.contains("ANTHROPIC_API_KEY") && why.contains("no key is kept for anthropic") && why.contains("Library › Providers"), "{why}");
    keys.set(&red, "anthropic", KEPT).unwrap();
    let env = list.get("claude").unwrap().environment(&read).unwrap();
    assert_eq!(env.get("ANTHROPIC_API_KEY").map(String::as_str), Some(KEPT), "given: the key J4n keeps for its provider");
    assert!(!red.scrub(&format!("it was {KEPT}")).contains(KEPT), "read through the reader, so it is registered");
    // taken back: nothing again, the key still kept
    list.bind_keys(&keys, &[]);
    assert!(!list.get("claude").unwrap().environment(&read).unwrap().contains_key("ANTHROPIC_API_KEY"));
    // an id that names no program with a kept key is said, and given nothing
    let said = list.bind_keys(&keys, &["pi", "nosuch"]);
    assert_eq!(said.len(), 2, "{said:?}");
    assert!(said.iter().all(|s| s.contains("doer.keys") && !s.contains(KEPT)), "{said:?}");
}

#[test]
fn a_kept_key_names_a_provider_by_a_file_name_and_counts_as_signed_in_only_where_it_was_given() {
    let (_, said) = load(vec![rows("[[connector]]\nid = \"ag\"\nis = \"tool\"\ndialect = \"text\"\nprogram = \"echo\"\nargv = [\"echo\"]\nsecrets = { AG_KEY = \"kept:../keys\" }\n")]);
    assert!(said.iter().any(|s| s.contains("kept:<provider>")), "a provider that is no file's name is refused: {said:?}");
    let home = std::env::temp_dir().join(format!("j4n-kept-signed-{}", std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    let (list, said) = load(vec![rows("[[connector]]\nid = \"ag\"\nis = \"tool\"\ndialect = \"text\"\nprogram = \"echo\"\nargv = [\"echo\"]\nlogin = [\"~/.agent\"]\nprobe = { argv = [\"echo\", \"v1\"] }\nsecrets = { AG_KEY = \"kept:acme\" }\n")]);
    assert!(said.is_empty(), "{said:?}");
    let ag = list.get("ag").unwrap();
    let env = |extra: &[(&str, &str)]| -> std::collections::BTreeMap<String, String> { [("HOME", home.to_str().unwrap())].into_iter().chain(extra.iter().copied()).map(|(k, v)| (k.to_string(), v.to_string())).collect() };
    assert!(!ag.probe(&env(&[])).authed, "a kept key it was not given does not sign it in, and there is no login");
    assert!(ag.probe(&env(&[("AG_KEY", KEPT)])).authed, "given, the key in its environment signs it in");
    std::fs::remove_dir_all(&home).ok();
}

#[test]
fn claude_codex_and_gemini_take_the_key_kept_for_their_provider_and_a_project_gives_none() {
    let (list, said) = load(vec![pack("base", "base"), pack("connect", "connect")]);
    assert!(said.is_empty(), "{said:?}");
    for (id, name, provider) in [("claude", "ANTHROPIC_API_KEY", "anthropic"), ("codex", "OPENAI_API_KEY", "openai"), ("gemini", "GEMINI_API_KEY", "google")] {
        let c = list.get(id).unwrap();
        assert_eq!(c.secrets.get(name).map(String::as_str), Some(format!("kept:{provider}").as_str()), "{id}");
        assert!(c.keyed.is_none(), "{id}: given to no program until the operator says so");
    }
    // a project nobody trusted cannot add a kept key to a program, nor say which programs are given one
    let project = Layer::new(LayerKind::Project, "here", vec![
        SourceFile { path: "connectors/claude.toml".into(), text: "api = 1\n[[connector]]\nid = \"claude\"\nsecrets = { OPENAI_API_KEY = \"kept:openai\" }\n".into() },
        SourceFile { path: "settings/keys.toml".into(), text: "api = 1\n[[setting]]\nid = \"doer.keys\"\nvalue = \"claude\"\n".into() },
    ]);
    let (tables, _) = Tables::load(vec![pack("base", "base-data"), project]);
    let (list, _) = Connectors::load(&tables);
    assert_eq!(list.get("claude").unwrap().secrets.keys().collect::<Vec<_>>(), ["ANTHROPIC_API_KEY"]);
    assert_eq!(j4n_core::setting::Settings::load(&tables).0.word("doer.keys", ""), "", "the project's setting is ignored");
}

/// The Decider jev reads the TypeSafe key from the environment or from J4n's key file (Library › Providers › typesafe), and an operator's
/// row of their own that names a `file:` still gives it the key from there.
#[test]
fn the_decider_jev_reads_the_typesafe_key_and_a_row_of_the_operators_own_keeps_its_file() {
    let home = crate::world::Scratch::new("jev-key");
    let keys = j4n_core::keys::Keys::new(home.path());
    let red = j4n_core::redact::Redactor::new();
    let (tables, problems) = Tables::load(vec![pack("base", "base"), pack("jev", "jev")]);
    assert!(problems.is_empty(), "{problems:?}");
    let (mut list, _) = Connectors::load(&tables);
    assert_eq!(list.get("jev").unwrap().key.as_deref(), Some("env:TYPESAFE_API_KEY"));
    let refs = j4n_core::keys::provider_refs(&tables);
    assert!(refs.contains(&("typesafe".to_string(), "env:TYPESAFE_API_KEY".to_string())), "a provider row for it: {refs:?}");
    list.keep_keys(keys.dir(), &refs);
    let read = reader(&red);
    assert_eq!(list.get("jev").unwrap().read_key(&read), Err(None), "no key anywhere is no refusal");
    // a key typed on the provider's row, while the pack was off, is jev's
    keys.set(&red, "typesafe", KEPT).unwrap();
    assert_eq!(list.get("jev").unwrap().api_key(&read), KEPT);
    // a row of the operator's own, one field over the pack's: a key file kept where the operator keeps it
    let own = home.put("elsewhere/typesafe", &format!("{KEPT}-own\n"));
    std::fs::set_permissions(&own, std::os::unix::fs::PermissionsExt::from_mode(0o600)).unwrap();
    keys.remove("typesafe").unwrap();
    let user = Layer::new(LayerKind::User, "home", vec![SourceFile { path: "connectors/jev.toml".into(), text: format!("api = 1\n[[connector]]\nid = \"jev\"\nkey = \"file:{}\"\n", own.display()) }]);
    let (tables, problems) = Tables::load(vec![pack("base", "base"), pack("jev", "jev"), user]);
    assert!(problems.is_empty(), "{problems:?}");
    let (mut list, _) = Connectors::load(&tables);
    list.keep_keys(keys.dir(), &j4n_core::keys::provider_refs(&tables));
    let jev = list.get("jev").unwrap();
    assert_eq!((jev.dialect, jev.url.as_deref()), (Dialect::Systemone, Some("https://api.typesafe.ai")), "the rest is the pack's");
    assert_eq!(jev.api_key(&read), format!("{KEPT}-own"));
}

/// A key that is there and refused says why, in the reader's words and with no value: a file others can read, a value too short.
#[test]
fn a_key_that_is_there_and_refused_says_why_and_one_that_is_nowhere_says_nothing() {
    use std::os::unix::fs::PermissionsExt;
    let dir = crate::world::Scratch::new("key-why");
    let red = j4n_core::redact::Redactor::new();
    let read = reader(&red);
    let wide = dir.put("wide", &format!("{KEPT}\n"));
    std::fs::set_permissions(&wide, std::fs::Permissions::from_mode(0o644)).unwrap();
    let short = dir.put("short", "ab12z\n");
    std::fs::set_permissions(&short, std::fs::Permissions::from_mode(0o600)).unwrap();
    let row = |path: &str| format!("[[connector]]\nid = \"m\"\nis = \"model\"\ndialect = \"openai\"\nurl = \"https://m.test/v1\"\nkey = \"file:{path}\"\n");
    for (path, said) in [(wide.display().to_string(), "readable by others"), (short.display().to_string(), "shorter than")] {
        let (list, problems) = load(vec![rows(&row(&path))]);
        assert!(problems.is_empty(), "{problems:?}");
        let why = list.get("m").unwrap().read_key(&read).unwrap_err().expect("there is a key, and it is refused");
        assert!(why.contains(said) && !why.contains(KEPT), "{why}");
    }
    let (list, _) = load(vec![rows(&row("/no/such/key/file"))]);
    assert_eq!(list.get("m").unwrap().read_key(&read), Err(None));
}
