//! A feature says what it lacks: with `lanes = orchestrate` and no model named, one card says what is missing, what was found (a `claude` that is
//! signed in, its cheapest model haiku and its standard one sonnet, found by price words) and four answers, each a setting edit of the operator's that `:undo`
//! takes back; it is not shown again for the need; `j4n doctor` lists the unmet need. The real front, proposer and Serve, with a `claude` that is a script.

use crate::run_cases::{Dirs, j4n};
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use j4n_core::branch::Switch;
use j4n_core::front::serve::Serve;
use j4n_core::task::Mode;
use j4n_proto::{Down, Intent, Reply, Up};
use serde_json::{Value as Json, json};
use std::fs;
use std::os::unix::fs::PermissionsExt;

/// A claude that answers `--version` and `auth status` like a signed-in one; its models are the rows' (`haiku`, `sonnet`, `opus`).
fn claude(d: &Dirs) -> String {
    let p = d.dir.join("bin/claude");
    fs::write(&p, "#!/bin/sh\ncase \"$1\" in --version) echo '2.1.0 (Claude Code)';; auth) echo '{\"loggedIn\":true,\"authMethod\":\"claude.ai\"}';; esac\n").unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p.display().to_string()
}

/// The word `orchestrate`, and none of the settings the one switch stands for.
fn orchestrating(d: &Dirs) {
    d.committed();
    fs::write(d.j4n_home().join("settings/lanes.toml"), "api = 1\n[[setting]]\nid = \"lanes\"\nvalue = \"orchestrate\"\n").unwrap();
}

fn open(d: &Dirs) -> World {
    let opts = Opts { doer: Some("test".into()), model: None, approval: Some(Approval::Ask), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false };
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: claude(d) };
    crate::support::opened(|| wire::open(setup.clone(), &opts, true, "main"))
}

fn events(w: &World, kind: &str) -> Vec<Json> {
    w.front.log().sync();
    w.front.log().read(0).filter(|e| e.kind == kind).map(|e| e.data).collect()
}

fn asks(serve: &mut Serve) -> Vec<j4n_proto::AskView> {
    let (downs, _) = serve.up(Up::Watch { bind: "asks:open".into() });
    downs.into_iter().filter_map(|d| if let Down::Asks { open } = d { Some(open) } else { None }).flatten().collect()
}

fn answer(serve: &mut Serve, id: &str) {
    serve.up(Up::Intent { intent: Intent::Answer { ask: "need:1".into(), reply: Reply::Pick { id: id.into() } } });
}

fn until(what: &str, done: impl Fn() -> bool) {
    let end = std::time::Instant::now() + crate::support::PATIENCE;
    while !done() {
        assert!(std::time::Instant::now() < end, "never saw {what}");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn card(d: &Dirs) -> (World, Serve) {
    claude(d);
    orchestrating(d);
    let world = open(d);
    let mut serve = Serve::new(world.front.clone());
    serve.up(Up::Hello { api: j4n_proto::API });
    (world, serve)
}

#[test]
fn the_switch_on_with_no_model_named_shows_one_card_with_what_was_found_and_four_answers() {
    let d = Dirs::new("need-card");
    let (world, mut serve) = card(&d);
    let open = asks(&mut serve);
    assert_eq!(open.len(), 1, "{open:?}");
    let a = &open[0];
    assert_eq!((a.ask.as_str(), a.shape, a.question.as_str()), ("need:1", j4n_proto::Shape::Choice, "lanes needs the quick look and Main"));
    let ids: Vec<&str> = a.options.iter().map(|o| o.id.as_str()).collect();
    assert_eq!(ids, ["use", "pick", "without", "off"], "{a:?}");
    assert!(a.options[0].recommended && a.options[0].label.contains("haiku") && a.options[0].label.contains("sonnet"), "{:?}", a.options[0]);
    assert!(a.options[2].label.contains("lines go to Main as typed"), "what going on without it means is on the card: {:?}", a.options[2]);
    let found: Vec<&str> = a.facts.iter().map(|c| c.text.as_str()).collect();
    assert!(found.iter().any(|f| f.contains("claude is signed in; its cheapest model is haiku")) && found.iter().any(|f| f.contains("its standard model is sonnet")), "{found:?}");
    assert_eq!(events(&world, "need.asked").len(), 1);
    // no second card while one waits, and Opus is nowhere in what was found
    world.front.need_check("main");
    assert_eq!(world.front.need_asks().len(), 1);
    assert!(!found.iter().any(|f| f.contains("opus")), "{found:?}");
    world.close();
}

#[test]
fn use_the_suggestion_writes_the_models_as_edits_and_the_card_is_not_shown_again() {
    let d = Dirs::new("need-use");
    let (world, mut serve) = card(&d);
    assert_eq!(asks(&mut serve).len(), 1);
    answer(&mut serve, "use");
    until("the look's model", || world.front.cfg().orchestrate.model == "haiku");
    let cfg = world.front.cfg();
    assert_eq!((cfg.main.doer.as_deref(), cfg.main.model.as_deref(), cfg.orchestrate.doer.as_str()), (Some("claude"), Some("sonnet"), "claude"));
    assert_eq!(events(&world, "need.answered").iter().map(|e| e["answer"].clone()).collect::<Vec<_>>(), [json!("use")]);
    assert_eq!(events(&world, "edit.applied").len(), 4, "the doer and the model of each part, each its own edit: {:?}", events(&world, "edit.applied"));
    assert!(world.front.cfg().waived.is_empty() && world.front.need_asks().is_empty(), "the card is gone");
    world.front.need_check("main");
    assert!(world.front.need_asks().is_empty(), "not shown again while the need holds");
    assert!(events(&world, "chat.note").iter().any(|n| n["text"].as_str().is_some_and(|t| t.contains(":undo") && t.contains("the quick look on claude · haiku"))), "{:?}", events(&world, "chat.note"));
    world.close();
}

#[test]
fn go_on_without_it_is_a_setting_that_stops_the_tiers_and_turn_it_off_sets_the_switch_back() {
    let d = Dirs::new("need-without");
    let (world, mut serve) = card(&d);
    answer(&mut serve, "without");
    until("the need waived", || world.front.cfg().waived == ["orchestrate"]);
    assert!(world.front.need_asks().is_empty() && world.front.waived("orchestrate"));
    world.front.need_check("main");
    assert!(world.front.need_asks().is_empty(), "not shown again");
    assert_eq!(events(&world, "need.answered").iter().map(|e| e["answer"].clone()).collect::<Vec<_>>(), [json!("without")]);
    assert!(world.front.cfg().orchestrate.model.is_empty(), "nothing was pinned");
    world.close();
    let d = Dirs::new("need-off");
    let (world, mut serve) = card(&d);
    answer(&mut serve, "off");
    until("the switch off", || world.front.lanes_now() == Switch::Single);
    assert!(world.front.need_asks().is_empty());
    assert_eq!(events(&world, "need.answered").iter().map(|e| e["answer"].clone()).collect::<Vec<_>>(), [json!("off")]);
    world.close();
}

#[test]
fn pick_another_asks_the_models_of_each_part_cheapest_first_and_writes_the_ones_picked() {
    let d = Dirs::new("need-pick");
    let (world, mut serve) = card(&d);
    answer(&mut serve, "pick");
    let first = asks(&mut serve);
    assert_eq!(first[0].question, "Which model for the quick look?");
    assert_eq!(first[0].options.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(), ["haiku", "sonnet", "opus"], "the cheapest first: {:?}", first[0].options);
    answer(&mut serve, "sonnet");
    assert_eq!(asks(&mut serve)[0].question, "Which model for Main?");
    answer(&mut serve, "haiku");
    until("the picks written", || world.front.cfg().orchestrate.model == "sonnet" && world.front.cfg().main.model.as_deref() == Some("haiku"));
    assert!(world.front.need_asks().is_empty());
    assert_eq!(events(&world, "need.answered").iter().map(|e| e["answer"].clone()).collect::<Vec<_>>(), [json!("pick")]);
    world.close();
}

#[test]
fn doctor_lists_the_need_a_feature_that_is_on_has_not_met() {
    let d = Dirs::new("need-doctor");
    claude(&d);
    orchestrating(&d);
    let bin = d.dir.join("bin").display().to_string();
    let out = j4n(&d, &["doctor"], None, &[("PATH", bin.as_str())]);
    let line = out.out.lines().find(|l| l.starts_with("need ")).unwrap_or_else(|| panic!("no need line: {}", out.out));
    assert!(line.contains("need orchestrate") && line.contains("no model is named for the quick look and Main") && line.contains("claude is signed in; its cheapest model is haiku"), "{line}");
    // with the switch off the need is not one
    fs::remove_file(d.j4n_home().join("settings/lanes.toml")).unwrap();
    let out = j4n(&d, &["doctor"], None, &[("PATH", bin.as_str())]);
    assert!(!out.out.lines().any(|l| l.starts_with("need ")), "{}", out.out);
}

/// A setting of the operator's, for this project.
fn set(d: &Dirs, file: &str, id: &str, value: &str) {
    fs::write(d.j4n_home().join("settings").join(file), format!("api = 1\n[[setting]]\nid = \"{id}\"\nvalue = {value}\n")).unwrap();
}

#[test]
fn a_key_need_with_no_key_shows_its_card_before_the_feature_acts_and_never_a_key() {
    let d = Dirs::new("need-key");
    claude(&d);
    // the kit's judge is on and sends text to Jev, whose small model `light` has no key here
    set(&d, "kit.toml", "kit.judge", "\"on\"");
    let world = open(&d);
    let mut serve = Serve::new(world.front.clone());
    serve.up(Up::Hello { api: j4n_proto::API });
    let open = asks(&mut serve);
    let a = open.iter().find(|a| a.question.contains("key of light")).unwrap_or_else(|| panic!("{open:?}"));
    assert_eq!(a.options.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(), ["key", "without", "off"]);
    assert!(a.options[0].label.contains("Library › Providers › light"), "{:?}", a.options[0]);
    assert!(events(&world, "task.queued").is_empty(), "the card came before anything acted");
    // never a key: not on the card, not in the Log
    let shown = serde_json::to_string(a).unwrap();
    assert!(!shown.contains("sk-") && !events(&world, "need.asked").iter().any(|e| e.to_string().contains("sk-")), "{shown}");
    // the answer opens where keys are typed and writes nothing
    serve.up(Up::Intent { intent: Intent::Answer { ask: a.ask.clone(), reply: Reply::Pick { id: "key".into() } } });
    until("the answer", || events(&world, "need.answered").iter().any(|e| e["answer"] == "key"));
    assert!(events(&world, "edit.applied").is_empty());
    assert!(world.front.need_asks().is_empty());
    world.close();
}

#[test]
fn a_decider_need_answered_pick_by_hand_puts_the_point_in_ask_hand_and_the_row_follows_it() {
    let d = Dirs::new("need-hand");
    claude(&d);
    set(&d, "chats.toml", "chats.on", "true");
    let world = open(&d);
    let mut serve = Serve::new(world.front.clone());
    serve.up(Up::Hello { api: j4n_proto::API });
    let a = asks(&mut serve).into_iter().find(|a| a.question.contains("Decider for chats")).expect("the card");
    assert_eq!(a.options.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(), ["setup", "hand", "without", "off"]);
    let ask = a.ask.clone();
    serve.up(Up::Intent { intent: Intent::Answer { ask, reply: Reply::Pick { id: "hand".into() } } });
    until("pick.hand written", || world.front.cfg().hand == ["chats"]);
    assert!(world.front.who().hands().has("chats"), "the point is picked by hand from now on");
    world.front.need_check("main");
    assert!(world.front.need_asks().is_empty(), "the need holds");
    world.close();
}

#[test]
fn a_login_need_shows_the_doers_own_way_to_sign_in_and_waits_for_it() {
    let d = Dirs::new("need-login");
    claude(&d);
    // a row of the operator's own: the quick look's doer `pi` must be signed in for this feature, and it is not even installed here
    fs::write(d.j4n_home().join("settings/mine-need.toml"), "api = 1\n[[need]]\nid = \"mine\"\ndoc = \"pi reads for it.\"\nkind = \"login\"\ndoer = \"pi\"\nfeature = \"chats.on\"\nwithout = \"nothing reads\"\n").unwrap();
    set(&d, "chats.toml", "chats.on", "true");
    let world = open(&d);
    let mut serve = Serve::new(world.front.clone());
    serve.up(Up::Hello { api: j4n_proto::API });
    // one card at a time: the Decider's for `chats` comes first, and the answer to it brings the next
    let first = asks(&mut serve).remove(0);
    serve.up(Up::Intent { intent: Intent::Answer { ask: first.ask, reply: Reply::Pick { id: "without".into() } } });
    until("the next card", || world.front.need_asks().first().is_some_and(|a| a.question.contains("pi signed in")));
    let a = world.front.need_asks().remove(0);
    assert_eq!(a.options.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(), ["login", "without", "off"]);
    assert!(a.options[0].label.starts_with("log in: "), "{:?}", a.options[0]);
    serve.up(Up::Intent { intent: Intent::Answer { ask: a.ask.clone(), reply: Reply::Pick { id: "login".into() } } });
    until("the answer", || events(&world, "need.answered").iter().any(|e| e["answer"] == "login"));
    assert!(events(&world, "chat.note").iter().any(|n| n["text"].as_str().is_some_and(|t| t.contains("sign pi in"))), "{:?}", events(&world, "chat.note"));
    world.close();
}

#[test]
fn a_folder_with_no_git_commit_under_orchestrate_shows_the_git_card_and_its_answer_makes_a_first_commit() {
    let d = Dirs::new("need-git");
    claude(&d);
    fs::write(d.j4n_home().join("settings/lanes.toml"), "api = 1\n[[setting]]\nid = \"lanes\"\nvalue = \"orchestrate\"\n[[setting]]\nid = \"orchestrate.look_model\"\nvalue = \"haiku\"\n[[setting]]\nid = \"lane.model\"\nvalue = \"sonnet\"\n").unwrap();
    assert!(!d.project().join(".git").exists());
    let world = open(&d);
    let mut serve = Serve::new(world.front.clone());
    serve.up(Up::Hello { api: j4n_proto::API });
    let a = asks(&mut serve).remove(0);
    assert_eq!(a.question, "lanes needs a git commit in this folder", "{a:?}");
    assert_eq!(a.options.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(), ["commit", "without", "off"]);
    assert!(a.options[1].label.contains("chats that write stay with Main"), "{:?}", a.options[1]);
    assert!(a.options.iter().all(|o| o.label.chars().count() <= 80), "no label is cut at 80 columns: {a:?}");
    serve.up(Up::Intent { intent: Intent::Answer { ask: a.ask.clone(), reply: Reply::Pick { id: "commit".into() } } });
    until("the answer", || events(&world, "need.answered").iter().any(|e| e["answer"] == "commit"));
    assert!(j4n_core::place::checkpoint::has_commit(&d.project()), "the answer made a first commit");
    assert!(world.front.needs_unmet("main").iter().all(|u| u.need.id != "git"), "the need holds now");
    world.close();
}
