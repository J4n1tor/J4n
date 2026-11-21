//! The access of the chats of a split: a chat that writes is given read-only copies of what it reads of another part and of the folder's build files, by the
//! plan's rung; it asks for more with the tool `access`, which the Deciders, then Main (`access <id>: yes`), answer; the guard refuses a secret whatever is said,
//! and no one is asked about it.

use crate::branch_validate::{CLEAN, Held, Rig3, checker, doer};
use crate::front_cases::{Fx, base, chatty, fx_placed};
use crate::world::Scratch;
use j4n_core::answer::point::points;
use j4n_core::answer::{Answerer, Offer, Question};
use j4n_core::ask::{Answer, AskShape};
use j4n_core::branch::Switch;
use j4n_core::front::Cfg;
use j4n_core::front::access::{TOOL_CLI, WORDS, access_rule, access_words, guard};
use serde_json::{Value, json};

const ONE: &str = r#"{"decision":"one","chats":[],"features":[],"why":"a question"}"#;
const LINE: &str = "build the store app, its store and its command line";

fn settings(c: &mut Cfg) {
    c.doer = Some("claude".into());
    c.lanes = Switch::Orchestrate;
    c.chats.screen = true;
    c.orchestrate.look = true;
    c.orchestrate.ask = "never".into();
    c.orchestrate.doer = "zcheap".into();
    c.orchestrate.access = true;
}

fn look() -> String {
    json!({ "decision": "split", "chats": [{ "goal": "the store", "writes": true, "doer": "pi", "model": "" }, { "goal": "the command line", "writes": true, "doer": "pi", "model": "" }], "features": [], "why": "two parts" }).to_string()
}

/// The store (c1) and the command line that calls it (c2, which reads `src/store.py`).
fn briefs() -> String {
    let brief = |id: &str, reads: &[&str], writes: &[&str]| json!({ "id": id, "goal": format!("write the {id}"), "paths": reads, "writes": writes, "done": "it runs", "doer": "pi", "model": "", "mode": "write", "features": [], "context": "brief", "checks": [] });
    json!({ "chats": [brief("store", &[], &["src/store.py"]), brief("cli", &["src/store.py"], &["src/cli.py"])], "main": "" }).to_string()
}

const REPORT: &str = r#"{"answer":"it is done","found":[],"changed":"","ran":[],"sure":"high","open":[]}"#;

/// The project's folder: a repository with one commit, which a chat that writes is cut from.
fn folder() -> Scratch {
    let s = Scratch::new("access-root");
    let git = |args: &[&str]| assert!(std::process::Command::new("git").arg("-C").arg(s.path()).args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]).args(args).output().unwrap().status.success());
    git(&["init", "-q"]);
    git(&["commit", "-q", "--allow-empty", "-m", "base"]);
    s
}

/// The store (c1) and the command line (c2), whose goal names the store's file and asks for a stand-in, and whose `paths` are empty: as a planner may
/// write them.
fn briefs_named() -> String {
    let brief = |id: &str, goal: &str, writes: &[&str]| json!({ "id": id, "goal": goal, "paths": [], "writes": writes, "done": "it runs", "doer": "pi", "model": "", "mode": "write", "features": [], "context": "brief", "checks": [] });
    let store = brief("store", "write the store and its tests; the command line in cli calls it", &["src/store.py", "src/test_store.py"]);
    let cli = brief("cli", "write the command line over the store (src/store.py, written in parallel by another chat; do not write it). Test it with a small fake Store (do not import store).", &["src/cli.py"]);
    json!({ "chats": [store, cli], "main": "" }).to_string()
}

/// Main answers the planning turn, then `main` on every turn; the decider answers `pick` to what it is asked; `access` is the switch.
fn rig_in(main: &str, pick: Option<&'static str>, access: bool) -> (Rig3, Scratch) {
    rig_of(&briefs(), main, pick, access)
}

/// The same with the briefs `plan`.
fn rig_of(plan: &str, main: &str, pick: Option<&'static str>, access: bool) -> (Rig3, Scratch) {
    let (held, root) = (Held::new(), folder());
    let doers = vec![doer("claude", &[plan, main, main, main, main, main, main]), doer("pi", &[REPORT; 6]), doer("own", &[CLEAN; 8]), checker(8), chatty("zcheap", &[&look(), ONE])];
    let f: Fx = fx_placed(doers, pick, vec![held.place()], |c| {
        settings(c);
        c.root = root.path().to_path_buf();
        c.orchestrate.access = access;
    });
    (Rig3 { f, held }, root)
}

fn rig(main: &str, pick: Option<&'static str>) -> (Rig3, Scratch) {
    rig_in(main, pick, true)
}

fn rows(r: &Rig3, kind: &str) -> Vec<Value> {
    r.f.rig.of_kind(kind).into_iter().map(|e| e.data).collect()
}

/// `(chat, path, rung)` of each row of `kind`.
fn said(r: &Rig3, kind: &str) -> Vec<(String, String, String)> {
    rows(r, kind).iter().map(|d| (d["chat"].as_str().unwrap_or_default().into(), d["path"].as_str().unwrap_or_default().into(), d["rung"].as_str().unwrap_or_default().into())).collect()
}

fn started(r: &Rig3) {
    r.f.front.say("main", LINE);
    r.until("both chats started", |r| r.f.tasks().iter().filter(|t| t.spec.input.get("chat").is_some()).count() >= 2);
}

/// The task of chat `id`'s first turn.
fn task_of(r: &Rig3, id: &str) -> j4n_core::task::Row {
    r.f.tasks().into_iter().find(|t| t.spec.input["chat"] == id).unwrap_or_else(|| panic!("{}", r.f.rig.dump()))
}

#[test]
fn a_part_that_reads_another_s_file_is_given_it_by_the_plan_with_the_build_files_and_its_brief_names_them() {
    let (r, root) = rig("main worked", Some("pi"));
    // no part is laid in: a part laid in is done and its shares are kept no longer, and its check and review would otherwise race the steps below
    *r.held.refuse_all.lock().unwrap() = Some("kept open while its shares are refreshed".into());
    root.put("package.json", "{}\n");
    started(&r);
    let given = said(&r, "access.given");
    for want in [("c2", "src/store.py"), ("c1", "package.json"), ("c2", "package.json")] {
        assert!(given.iter().any(|(c, p, rung)| (c.as_str(), p.as_str()) == want && rung == "rule:access"), "{want:?}: {given:?}");
    }
    assert!(!given.iter().any(|(c, p, _)| c == "c1" && p == "src/cli.py"), "c1 reads nothing of c2's: {given:?}");
    assert!(rows(&r, "access.asked").iter().all(|d| !(d["chat"] == "c2" && d["path"] == "src/cli.py")), "its own file is never asked for");
    let (one, two) = (task_of(&r, "c1"), task_of(&r, "c2"));
    assert!(two.spec.goal.contains("Shared with you, read-only") && two.spec.goal.contains("src/store.py (not written yet)") && two.spec.goal.contains("package.json (the folder's)"), "{}", two.spec.goal);
    assert!(two.spec.goal.contains(TOOL_CLI) && !two.spec.goal.contains("stand-in in your own files"), "{}", two.spec.goal);
    assert!(one.spec.goal.contains("package.json (the folder's)"), "{}", one.spec.goal);
    // the folder's build file is copied in before the chat starts; the store is not written yet
    let shares: Vec<&str> = two.spec.input["shares"].as_array().unwrap().iter().filter_map(|s| s["path"].as_str()).collect();
    assert_eq!(shares, ["package.json"], "{}", two.spec.input);
    // c1 writes its store: at the next step c2's copy is made, and when it changes again c2 is told
    let site = r.held.site(&one.id);
    std::fs::create_dir_all(site.join("src")).unwrap();
    std::fs::write(site.join("src/store.py"), "def add_item(): pass\n").unwrap();
    r.f.front.plan_step("main");
    assert_eq!(std::fs::read_to_string(r.held.site(&two.id).join("src/store.py")).unwrap(), "def add_item(): pass\n", "{}", r.f.rig.dump());
    std::fs::write(site.join("src/store.py"), "def add_item(x): return x\n").unwrap();
    r.f.front.plan_step("main");
    let changes: Vec<Value> = rows(&r, "access.shared").into_iter().filter(|d| d["chat"] == "c2" && d["path"] == "src/store.py").collect();
    assert_eq!(changes.iter().map(|d| d["change"].as_str().unwrap()).collect::<Vec<_>>(), ["placed", "changed"], "{changes:?}");
    let listed = r.f.front.access_call(&two.id, "list", "").unwrap();
    assert!(listed.contains("c1's src/store.py changed"), "{listed}");
}

/// What a part whose goal names another part's file is told of its copy and of the stand-in its brief asks for.
const OVERRIDE: &str = "Your brief names src/store.py, which another part writes, so you read its copy: where your brief says to use a stand-in for that part or not to import its module, use the copy instead.";

#[test]
fn a_part_whose_goal_names_another_part_s_file_reads_it_through_a_share_and_is_told_the_copy_stands_over_its_stand_in() {
    let (r, _root) = rig_of(&briefs_named(), "main worked", Some("pi"), true);
    *r.held.refuse_all.lock().unwrap() = Some("kept open while its shares are refreshed".into());
    started(&r);
    let given = said(&r, "access.given");
    assert!(given.iter().any(|(c, p, rung)| c == "c2" && p == "src/store.py" && rung == "rule:access"), "{given:?}");
    // the store's own test file is not named, and the store's goal names `cli` with no file: nothing is given to either for that
    assert!(!given.iter().any(|(c, p, _)| (c == "c2" && p == "src/test_store.py") || (c == "c1" && p == "src/cli.py")), "{given:?}");
    let briefed = rows(&r, "chat.briefed");
    let of = |id: &str| briefed.iter().find(|d| d["id"] == id).cloned().unwrap_or_else(|| panic!("{briefed:?}"));
    assert_eq!((of("cli")["paths"].clone(), of("cli")["named"].clone()), (json!(["src/store.py"]), json!(["src/store.py"])), "the Log says the read came from the goal");
    assert_eq!((of("store")["paths"].clone(), of("store").get("named").cloned()), (json!([]), None));
    let (one, two) = (task_of(&r, "c1"), task_of(&r, "c2"));
    assert!(two.spec.goal.contains("You read: src/store.py.") && two.spec.goal.contains("Use another part's module through its copy") && two.spec.goal.contains(OVERRIDE), "{}", two.spec.goal);
    assert!(two.spec.goal.contains("src/store.py (not written yet)") && two.spec.goal.contains("Test it with a small fake Store (do not import store)."), "the brief's own words stay: {}", two.spec.goal);
    assert!(!one.spec.goal.contains("Your brief names"), "{}", one.spec.goal);
    // c1 writes its store: at the next step c2's copy is placed
    let site = r.held.site(&one.id);
    std::fs::create_dir_all(site.join("src")).unwrap();
    std::fs::write(site.join("src/store.py"), "def step(): pass\n").unwrap();
    r.f.front.plan_step("main");
    let shared: Vec<Value> = rows(&r, "access.shared").into_iter().filter(|d| d["chat"] == "c2" && d["path"] == "src/store.py").collect();
    assert_eq!(shared.iter().map(|d| d["change"].as_str().unwrap_or_default()).collect::<Vec<_>>(), ["placed"], "{}", r.f.rig.dump());
    assert_eq!(std::fs::read_to_string(r.held.site(&two.id).join("src/store.py")).unwrap(), "def step(): pass\n");
}

#[test]
fn with_the_access_off_a_goal_that_names_another_part_s_file_changes_nothing_of_the_brief() {
    let (r, _root) = rig_of(&briefs_named(), "main worked", Some("pi"), false);
    started(&r);
    let two = task_of(&r, "c2");
    assert!(two.spec.goal.contains("You read: none.") && two.spec.goal.contains("stand-in in your own files") && !two.spec.goal.contains("Your brief names"), "{}", two.spec.goal);
    assert!(rows(&r, "chat.briefed").iter().all(|d| d["paths"] == json!([]) && d.get("named").is_none()));
    assert!(said(&r, "access.given").is_empty());
}

#[test]
fn a_goal_names_a_file_as_a_whole_path_with_an_extension_never_a_folder_a_bare_module_or_part_of_a_longer_name() {
    let paths: Vec<String> = ["store.py", "src/board.rs", "cli", "ui/", "pieces.py"].iter().map(|s| s.to_string()).collect();
    let named = |goal: &str| j4n_core::front::split::named_files(goal, &paths);
    for goal in ["it uses store.py, written by another chat", "it uses (store.py)", "it uses `store.py`", "it imports store.py.", "store.py"] {
        assert_eq!(named(goal), ["store.py"], "{goal}");
    }
    for goal in ["test_store.py", "src/store.py", "store.pyc", "store.py.bak", "my-store.py", ".store.py", "the store interface", "import store", "the cli and the ui", "a fruits list"] {
        assert!(named(goal).is_empty(), "{goal}: {:?}", named(goal));
    }
    assert_eq!(named("board.rs and src/board.rs, then pieces.py and store.py"), ["store.py", "src/board.rs", "pieces.py"], "each once, in the order of the paths");
}

#[test]
fn the_guard_refuses_a_secret_whatever_a_rung_would_say_and_no_one_is_asked_and_the_chat_s_own_file_is_never_a_share() {
    // the decider says yes to everything it is asked
    let (r, _root) = rig("main worked", Some("yes"));
    started(&r);
    let two = task_of(&r, "c2");
    let asked = r.f.asked.lock().unwrap().iter().filter(|q| q.point == "access").count();
    for want in ["folder:.env", "folder:keys/server.pem", "c1:.git/config", "folder:../outside.py", "folder:.claude/projects/x.jsonl"] {
        let said = r.f.front.access_call(&two.id, want, "my tests need it").unwrap();
        assert!(said.starts_with("refused by guard"), "{want}: {said}");
    }
    assert_eq!(r.f.asked.lock().unwrap().iter().filter(|q| q.point == "access").count(), asked, "a refused path reaches no Decider");
    assert!(said(&r, "access.refused").iter().filter(|(_, _, rung)| rung == "guard").count() >= 5);
    let own = r.f.front.access_call(&two.id, "c2:src/cli.py", "to see it").unwrap();
    assert!(own.starts_with("refused by rule:access") && own.contains("its own file"), "{own}");
}

#[test]
fn a_request_the_plan_does_not_name_is_settled_by_a_decider_that_is_sure() {
    let (r, root) = rig("main worked", Some("yes"));
    root.put("docs/rules.md", "the rules\n");
    started(&r);
    let two = task_of(&r, "c2");
    let said_now = r.f.front.access_call(&two.id, "folder:docs/rules.md", "the command line follows the rules of the store").unwrap();
    assert!(said_now.starts_with("given by decider:jev"), "{said_now}");
    assert_eq!(std::fs::read_to_string(r.held.site(&two.id).join("docs/rules.md")).unwrap(), "the rules\n");
    let q = r.f.asked.lock().unwrap().iter().rev().find(|q| q.point == "access").cloned().unwrap();
    assert!(q.text.contains("chat c2 asks to read docs/rules.md, from folder") && q.text.contains("\nown: no\n") && q.text.contains("\nheld: yes\n"), "{}", q.text);
}

#[test]
fn a_folder_given_is_its_plain_files_below_it_without_secrets_links_build_output_or_the_chat_s_own_files_and_at_most_the_cap() {
    use j4n_core::front::access::FILES;
    let (r, root) = rig("main worked", Some("yes"));
    root.put("docs/rules.md", "the rules\n");
    root.put("docs/deep/more.md", "more\n");
    root.put("docs/.env", "SECRET=x\n");
    root.put("docs/node_modules/x.js", "x\n");
    std::os::unix::fs::symlink(root.path().join("docs/rules.md"), root.path().join("docs/link.md")).unwrap();
    root.put("src/store.py", "def add_item(): pass\n");
    root.put("src/cli.py", "the folder's old command line\n");
    for i in 0..FILES + 5 {
        root.put(&format!("bulk/f{i:03}.txt"), "x\n");
    }
    started(&r);
    let two = task_of(&r, "c2");
    let site = r.held.site(&two.id);
    let said_now = r.f.front.access_call(&two.id, "folder:docs", "the command line follows the rules").unwrap();
    assert!(said_now.starts_with("given by"), "{said_now}");
    assert_eq!(std::fs::read_to_string(site.join("docs/rules.md")).unwrap(), "the rules\n");
    assert_eq!(std::fs::read_to_string(site.join("docs/deep/more.md")).unwrap(), "more\n");
    for not in ["docs/.env", "docs/node_modules/x.js", "docs/link.md"] {
        assert!(!site.join(not).exists(), "{not} is never shared");
    }
    // a folder that holds the chat's own file: the others come, its own does not
    let src = r.f.front.access_call(&two.id, "folder:src", "the command line calls the store").unwrap();
    assert!(src.starts_with("given by"), "{src}");
    assert!(site.join("src/store.py").exists() && !site.join("src/cli.py").exists(), "{}", r.f.rig.dump());
    r.f.front.access_call(&two.id, "folder:bulk", "the data").unwrap();
    assert_eq!(std::fs::read_dir(site.join("bulk")).unwrap().count(), FILES, "a folder is at most the cap");
    let listed = r.f.front.access_call(&two.id, "list", "").unwrap();
    assert!(listed.contains("bulk"), "{listed}");
}

#[test]
fn what_no_rung_settles_is_main_s_and_its_word_is_read_once_and_any_other_form_is_a_no() {
    let (r, root) = rig("main worked\naccess a1: yes\naccess a2: maybe", Some("pi"));
    root.put("docs/rules.md", "the rules\n");
    root.put("docs/art.md", "the art\n");
    started(&r);
    let two = task_of(&r, "c2");
    for want in ["folder:docs/rules.md", "folder:docs/art.md"] {
        let said = r.f.front.access_call(&two.id, want, "the command line needs it").unwrap();
        assert!(said.contains("Main is asked"), "{said}");
    }
    r.until("Main decided both", |r| rows(r, "access.given").iter().any(|d| d["rung"] == "main") && rows(r, "access.refused").iter().any(|d| d["rung"] == "main"));
    assert!(r.f.tasks().iter().any(|t| t.spec.doer == "claude" && t.spec.goal.contains(WORDS)), "Main was woken with the words to answer: {}", r.f.rig.dump());
    let given = said(&r, "access.given");
    assert!(given.contains(&("c2".into(), "docs/rules.md".into(), "main".into())), "{given:?}");
    let refused: Vec<Value> = rows(&r, "access.refused").into_iter().filter(|d| d["rung"] == "main").collect();
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert!(refused[0]["path"] == "docs/art.md" && refused[0]["why"].as_str().unwrap().contains("`maybe`"), "{}", refused[0]);
    // read once: more turns of Main decide nothing again
    for _ in 0..3 {
        r.step();
    }
    assert_eq!(rows(&r, "access.given").iter().filter(|d| d["rung"] == "main").count(), 1);
    assert_eq!(rows(&r, "access.refused").iter().filter(|d| d["rung"] == "main").count(), 1);
}

#[test]
fn with_the_access_off_a_chat_is_given_nothing_and_told_to_use_a_stand_in() {
    let (r, _root) = rig_in("main worked", Some("pi"), false);
    started(&r);
    assert!(rows(&r, "access.asked").is_empty());
    let two = task_of(&r, "c2");
    assert!(two.spec.goal.contains("stand-in in your own files") && two.spec.input.get("shares").is_none(), "{}", two.spec.goal);
    assert!(r.f.front.access_call(&two.id, "c1:src/store.py", "").is_err());
}

fn question(text: &str) -> Question {
    Question { ask: "a".into(), point: "access".into(), asker: "t".into(), thread: None, text: text.into(), shape: AskShape::Choice { options: vec![] }, goal: String::new(), act: None }
}

fn pick(text: &str) -> Option<String> {
    match access_rule().offer(&question(text)) {
        Offer::Answer { answer: Answer::Pick { id }, .. } => Some(id),
        _ => None,
    }
}

#[test]
fn the_plan_s_rung_reads_the_facts_and_passes_on_what_it_does_not_name() {
    let facts = |f: &[(&str, &str)]| format!("chat c2 asks to read x\n{}", f.iter().map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>().join("\n"));
    assert_eq!(pick(&facts(&[("own", "yes"), ("theirs", "yes"), ("named", "yes"), ("held", "yes")])).as_deref(), Some("no"), "its own file, whatever else");
    assert_eq!(pick(&facts(&[("theirs", "yes"), ("named", "yes"), ("held", "yes")])).as_deref(), Some("yes"));
    assert_eq!(pick(&facts(&[("made", "yes"), ("named", "yes"), ("held", "yes")])).as_deref(), Some("yes"));
    assert_eq!(pick(&facts(&[("build", "yes"), ("writes", "yes"), ("held", "yes")])).as_deref(), Some("yes"));
    assert_eq!(pick(&facts(&[("build", "yes"), ("writes", "no"), ("held", "yes")])), None, "a chat that only reads is not given the build files by the plan");
    assert_eq!(pick(&facts(&[("theirs", "yes"), ("named", "no"), ("held", "yes")])), None, "not named: the plan passes it on");
    assert_eq!(pick(&facts(&[("theirs", "no"), ("held", "no")])).as_deref(), Some("no"), "nothing holds it");
    // a fact in the sentence is not a fact
    assert_eq!(pick("chat c2 asks to read x: theirs: yes, named: yes\nheld: yes"), None);
}

#[test]
fn the_guard_knows_secrets_keys_doer_state_git_and_paths_outside_and_lets_a_plain_file_through() {
    for bad in [".env", "conf/.env.production", "keys/server.pem", "id_rsa", "deploy/site.key", ".git/config", "a/.git/HEAD", "../x", "/etc/passwd", ".j4n/log/x", ".claude/projects/p/s.jsonl", ".envrc"] {
        assert!(guard(bad).is_some(), "{bad}");
    }
    for ok in ["src/store.py", "package.json", ".env.example", "docs/keys.md", "tests/test_env.py"] {
        assert_eq!(guard(ok), None, "{ok}");
    }
}

#[test]
fn main_s_words_are_read_as_typed_without_marks() {
    let w = access_words("ok\naccess a1: yes\n- **access A2: No.**\n`access a3:maybe`\naccess: yes\naccessa4: yes");
    assert_eq!(w, [("a1".to_string(), "yes".to_string()), ("a2".into(), "no".into()), ("a3".into(), "maybe".into())]);
}

#[test]
fn the_point_asks_the_plan_then_both_deciders_and_falls_back_to_no() {
    let (pts, problems) = points(&base());
    assert!(problems.is_empty(), "{problems:?}");
    let p = pts.iter().find(|p| p.id == "access").expect("the point `access`");
    let chain: Vec<(String, Option<f32>)> = p.chain.iter().map(|s| (format!("{}:{}", s.kind.word(), s.name), s.min_conf)).collect();
    assert_eq!(chain, [("rule:access".to_string(), None), ("decider:jev".into(), Some(0.7)), ("decider:span".into(), Some(0.7))]);
}

#[test]
fn the_review_of_a_part_that_read_another_s_copy_is_told_the_copy_stands_over_the_stand_in_its_brief_asked_for() {
    let (r, _root) = rig_of(&briefs_named(), "main worked", Some("pi"), true);
    started(&r);
    let reviews = |r: &Rig3| r.f.tasks().into_iter().filter(|t| t.spec.role.as_deref() == Some("review") && t.spec.goal.starts_with("Review the change made for")).collect::<Vec<_>>();
    r.until("both parts are reviewed", |r| reviews(r).len() >= 2);
    let of = |id: &str| reviews(&r).into_iter().find(|t| t.spec.goal.contains(&format!("`{id}`"))).unwrap_or_else(|| panic!("{}", r.f.rig.dump()));
    let told = "This part was given read-only copies of `src/store.py`";
    assert!(of("c2").spec.goal.contains(told) && of("c2").spec.goal.contains("using the copy instead is what it was told to do, not a fault"), "{}", of("c2").spec.goal);
    assert!(!of("c1").spec.goal.contains("read-only copies"), "{}", of("c1").spec.goal);
}
