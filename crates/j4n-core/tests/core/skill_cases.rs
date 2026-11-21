//! The skill library and the pick. A skill is a `SKILL.md` with a `name`, a `when` and a `cost`; J4n picks the few
//! that fit a task by its words, its files and its kind, locally; only those go into that doer's Brief (or a folder of the task's own); the Log and the
//! chat say which and why; the strip counts them.

use crate::front_cases::fx;
use crate::scripted::{Beat, Scripted, end, say};
use j4n_core::doer::Caps;
use j4n_core::skill::{Ask, Library, Skill, parse};
use serde_json::Value;
use std::sync::Arc;

fn skill(name: &str, when: &str, cost: usize, body: &str) -> Skill {
    parse("pack:skills", name, &format!("---\nname: {name}\nwhen: {when}\ncost: {cost}\n---\n{body}\n")).unwrap()
}

fn library() -> Library {
    Library::new(
        vec![
            skill("checks", "tests, lint, kind:write", 200, "# Run the checks\nUse scripts/t."),
            skill("rust", "cargo, *.rs", 300, "# Rust notes\nPrefer iterators."),
            skill("docs", "docs, decision, docs/", 250, "# Docs rows\nUse the given ids."),
            skill("only-kind", "kind:write", 100, "# Never picked\nThe kind alone does not pick."),
        ],
        "",
    )
}

fn ask<'a>(line: &'a str, files: &[&str], kinds: &[&str]) -> Ask<'a> {
    Ask { line, files: files.iter().map(|f| f.to_string()).collect(), kinds: kinds.iter().map(|k| k.to_string()).collect(), in_session: false, max: 3, tokens: 1200 }
}

fn ids(l: &Library, a: &Ask) -> Vec<String> {
    l.pick(a).into_iter().map(|p| p.id).collect()
}

#[test]
fn a_skill_file_has_frontmatter_with_a_name_a_when_and_a_cost_and_says_what_is_wrong_with_one_that_has_not() {
    let s = parse("home", "folder", "---\nname: j4n-log\nwhen: log, events\ncost: 220\n---\n# Read a Log\nOne event a line.\n").unwrap();
    assert_eq!((s.id.as_str(), s.when.as_str(), s.cost, s.from.as_str(), s.about()), ("j4n-log", "log, events", 220, "home", "Read a Log"));
    assert!(s.body.starts_with("# Read a Log"), "{:?}", s.body);
    // no name: the folder's; no cost: measured
    let s = parse("home", "folder", "---\nwhen: x\n---\nfour words in here\n").unwrap();
    assert_eq!((s.id.as_str(), s.cost), ("folder", 5));
    for (text, why) in [("no frontmatter", "starts with"), ("---\nname: a\n", "ends with"), ("---\nname: a\n---\nbody", "`when`"), ("---\nname: A b\nwhen: x\n---\nbody", "not a skill name")] {
        assert!(parse("home", "a", text).unwrap_err().contains(why), "{text}");
    }
}

#[test]
fn a_skill_may_say_in_a_sentence_what_it_is_for_and_one_that_does_not_has_none() {
    // the sentence is what claude lists the skill by; without one the `when` words are all it has
    let s = parse("home", "x", "---\nname: x\nwhen: log\ndescription: Read when a log must be read.\n---\nbody\n").unwrap();
    assert_eq!(s.description, "Read when a log must be read.");
    let s = parse("home", "x", "---\nname: x\nwhen: log\n---\nbody\n").unwrap();
    assert_eq!(s.description, "");
}

#[test]
fn the_pick_is_by_the_words_of_the_line_the_files_it_names_and_the_kind_of_task() {
    let l = library();
    assert_eq!(ids(&l, &ask("please run the tests", &[], &[])), ["checks"], "by a word, plural or not");
    assert_eq!(ids(&l, &ask("fix the parser", &["src/lib.rs"], &[])), ["rust"], "by a file: `*.rs`");
    let why = &l.pick(&ask("fix the parser", &["src/lib.rs"], &[]))[0].why;
    assert_eq!(why, "file *.rs");
    assert_eq!(ids(&l, &ask("update docs/PLAN.md", &["docs/PLAN.md"], &[])), ["docs"], "a folder is a file pattern too");
    // the kind ranks a skill that already hit, and is said; alone it picks nothing
    let picked = l.pick(&ask("run the lint", &[], &["write"]));
    assert_eq!((picked.len(), picked[0].id.as_str(), picked[0].why.as_str()), (1, "checks", "said lint; kind write"));
    assert!(l.pick(&ask("hello there friend", &[], &["write"])).is_empty(), "no word and no file: the kind picks nothing");
    assert!(!ids(&l, &ask("run the tests", &[], &["write"])).contains(&"only-kind".to_string()));
    // two hit: the one that hit harder is first
    assert_eq!(ids(&l, &ask("cargo tests", &["Cargo.toml"], &[])), ["checks", "rust"]);
}

#[test]
fn the_pick_has_a_small_cap_on_how_many_and_on_how_many_tokens() {
    let many = Library::new((0..6).map(|n| skill(&format!("s{n}"), "build", 100 + n * 10, "# s\nwords")).collect(), "");
    let a = ask("build it", &[], &[]);
    assert_eq!(ids(&many, &a), ["s0", "s1", "s2"], "three by default, the smaller first when all fit equally");
    assert_eq!(ids(&many, &Ask { max: 1, ..ask("build it", &[], &[]) }), ["s0"]);
    assert!(ids(&many, &Ask { max: 0, ..ask("build it", &[], &[]) }).is_empty());
    // 230 tokens hold two of ~100, a third does not fit
    assert_eq!(ids(&many, &Ask { tokens: 230, ..ask("build it", &[], &[]) }), ["s0", "s1"]);
    let lone = Library::new(vec![skill("big", "build", 5000, "# big\nwords")], "");
    assert!(ids(&lone, &a).is_empty(), "a skill over the token cap is left out");
}

#[test]
fn a_skill_that_is_switched_off_is_never_picked_and_the_state_is_the_setting_s_text() {
    let l = library();
    assert_eq!(l.switch("checks", false).unwrap(), "checks");
    assert!(!l.is_on("checks"));
    assert!(ids(&l, &ask("run the tests", &[], &[])).is_empty());
    assert_eq!(l.switch("docs", false).unwrap(), "checks,docs");
    assert_eq!(l.switch("checks", true).unwrap(), "docs");
    assert_eq!(ids(&l, &ask("run the tests", &[], &[])), ["checks"]);
    assert!(l.switch("nope", true).unwrap_err().contains("no skill `nope`"));
    // the setting at the start
    let started = Library::new(vec![skill("a", "x", 10, "# a\nb"), skill("b", "x", 10, "# b\nc")], "b");
    assert_eq!((started.is_on("a"), started.is_on("b")), (true, false));
    use j4n_core::value::Value as V;
    let rows = started.rows();
    assert_eq!((rows[0].pointer("/id"), rows[0].pointer("/state"), rows[1].pointer("/state"), rows[1].pointer("/flip")), (V::text("a"), V::text("on"), V::text("off"), V::text("on")));
}

#[test]
fn a_short_line_in_a_going_thread_picks_none_and_a_later_layer_replaces_a_skill_of_the_same_name() {
    let l = library();
    assert!(l.pick(&Ask { in_session: true, ..ask("tests", &[], &[]) }).is_empty(), "`tests` in a going thread is a line to the doer that holds the work");
    assert_eq!(ids(&l, &Ask { in_session: true, ..ask("run the tests again please", &[], &[]) }), ["checks"]);
    let both = Library::new(vec![skill("a", "x", 10, "# pack's\nb"), parse("home", "a", "---\nname: a\nwhen: x\n---\n# mine\nb\n").unwrap()], "");
    assert_eq!((both.all().len(), both.get("a").unwrap().from.as_str()), (1, "home"));
    assert_eq!(library().search("lint").iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["checks"]);
    assert_eq!(library().search("rust notes").len(), 1, "the words are the name, the `when` and the first line");
}

#[test]
fn a_skill_with_a_where_is_picked_only_in_a_project_that_has_that_path_and_the_users_own_skills_are_picked_anywhere() {
    // the shipped skills are about J4n's own repository
    let here = parse("pack:skills", "j4n-checks", "---\nname: j4n-checks\nwhen: tests, build\nwhere: crates/j4n-core/Cargo.toml\ncost: 200\n---\n# Use scripts/t\nCargo only through it.\n").unwrap();
    assert_eq!(here.here.as_deref(), Some("crates/j4n-core/Cargo.toml"));
    let mine = skill("mine", "tests", 100, "# My tests\nRun pytest.");
    let all = || vec![here.clone(), mine.clone()];
    let base = std::env::temp_dir().join(format!("j4n-dp10-where-{}", std::process::id()));
    let (j4n, other) = (base.join("j4n"), base.join("other"));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(j4n.join("crates/j4n-core")).unwrap();
    std::fs::write(j4n.join("crates/j4n-core/Cargo.toml"), "[package]\n").unwrap();
    std::fs::create_dir_all(&other).unwrap();
    let line = ask("run the tests", &[], &["write"]);
    let mut both = ids(&Library::new(all(), "").in_project(&j4n), &line);
    both.sort();
    assert_eq!(both, ["j4n-checks", "mine"], "in J4n's own repository both are picked");
    assert_eq!(ids(&Library::new(all(), "").in_project(&other), &line), ["mine"], "in another project the shipped skill is never offered, the user's own is");
    assert_eq!(ids(&Library::new(all(), ""), &line), ["mine"], "with no project known, a skill with a where is not offered either");
    assert_eq!(Library::new(all(), "").in_project(&other).all().len(), 2, "the library still lists it: `:skill` and the page show every skill");
    for bad in ["/etc/passwd", "../x", "~/x"] {
        let text = format!("---\nname: a\nwhen: x\nwhere: {bad}\n---\nb\n");
        assert!(parse("home", "a", &text).unwrap_err().contains("inside the project"), "{bad}");
    }
    let _ = std::fs::remove_dir_all(&base);
}

// ---- through the front ----

fn doer(brief: bool) -> Arc<Scripted> {
    let turns = (0..3).map(|_| vec![Beat::Session("claude-s".into()), say("ok"), end("ok")]).collect();
    Scripted::with_caps("claude", Caps { live: true, resume: true, brief, skills: !brief, ..Caps::default() }, turns)
}

fn briefs_of(f: &crate::front_cases::Fx) -> Vec<String> {
    f.tasks().iter().map(|r| r.spec.input.get("brief").and_then(Value::as_str).unwrap_or_default().to_string()).collect()
}

#[test]
fn the_brief_holds_only_the_picked_skills_and_the_log_and_the_chat_say_which_and_why_one_row_for_each_task() {
    let f = fx(vec![doer(true)], |c| {
        c.doer = Some("claude".into());
        c.skills = Some(Arc::new(library()));
    });
    f.turn("run the tests in src/lib.rs");
    let b = &briefs_of(&f)[0];
    assert!(b.contains("## skills\n### checks\n# Run the checks\nUse scripts/t.\n\n### rust\n# Rust notes"), "{b}");
    assert!(!b.contains("Docs rows") && !b.contains("Never picked"), "a skill that did not fit is not in it: {b}");
    let log = f.rig.of_kind("brief.skills");
    assert_eq!(log.len(), 1, "one row for the task");
    let d = &log[0].data;
    assert_eq!(d["doer"], "claude");
    assert_eq!(d["skills"][0]["id"], "checks");
    assert_eq!(d["skills"][0]["why"], "said tests; kind write");
    assert_eq!(d["skills"][1]["why"], "file *.rs");
    assert_eq!(d["tokens"], 500);
    let note = f.notes().into_iter().find(|n| n.starts_with("skills · ")).expect("the chat says so");
    assert_eq!(note, "skills · checks (said tests; kind write) · rust (file *.rs) · 2 of 4 · ~500 tokens");
    assert_eq!(d["line"], note.as_str(), "the Log row and the chat row are one text");
    assert!(f.rig.of_kind("brief.rendered")[0].data["sections"].as_array().unwrap().iter().any(|s| s == "skills"));
    // "continue" in the going thread picks none, and the Brief of a doer that is given one has nothing of what the line before had
    f.turn("continue");
    assert!(!briefs_of(&f)[1].contains("## skills"), "{}", briefs_of(&f)[1]);
    // a task that nothing fits says nothing and has no section
    f.turn("how are you today my friend");
    assert!(!briefs_of(&f)[2].contains("## skills"));
    assert_eq!(f.rig.of_kind("brief.skills").len(), 1);
}

#[test]
fn a_skill_that_is_off_and_a_pick_that_is_off_put_nothing_in_the_brief_and_the_caps_are_the_settings() {
    let lib = Arc::new(library());
    lib.switch("checks", false).unwrap();
    let f = fx(vec![doer(true)], |c| {
        c.doer = Some("claude".into());
        c.skills = Some(lib.clone());
    });
    f.turn("run the tests in src/lib.rs");
    let b = &briefs_of(&f)[0];
    assert!(!b.contains("### checks") && b.contains("### rust"), "{b}");
    // the one switch
    let mut screen = j4n_core::front::screen::Screen::builtin();
    screen.set = j4n_core::setting::Settings::from_pairs([("skills.pick", j4n_core::value::Value::Bool(false))]).tree();
    let off = crate::front_cases::fx_placed_on(vec![doer(true)], None, Vec::new(), screen, |c| {
        c.doer = Some("claude".into());
        c.skills = Some(Arc::new(library()));
    });
    off.turn("run the tests in src/lib.rs");
    assert!(!briefs_of(&off)[0].contains("## skills") && off.rig.of_kind("brief.skills").is_empty());
    // the count
    let mut screen = j4n_core::front::screen::Screen::builtin();
    screen.set = j4n_core::setting::Settings::from_pairs([("skills.max", j4n_core::value::Value::Num(1.0))]).tree();
    let one = crate::front_cases::fx_placed_on(vec![doer(true)], None, Vec::new(), screen, |c| {
        c.doer = Some("claude".into());
        c.skills = Some(Arc::new(library()));
    });
    one.turn("run the tests in src/lib.rs");
    assert_eq!(one.rig.of_kind("brief.skills")[0].data["skills"].as_array().unwrap().len(), 1);
}

#[test]
fn a_doer_with_no_brief_that_reads_a_folder_is_given_the_words_in_its_task() {
    let f = fx(vec![doer(false)], |c| {
        c.doer = Some("claude".into());
        c.skills = Some(Arc::new(library()));
    });
    f.turn("run the tests");
    let row = &f.tasks()[0];
    assert!(row.spec.input.get("brief").is_none(), "no Brief for it");
    assert_eq!(row.spec.input["skills"][0]["id"], "checks");
    assert!(row.spec.input["skills"][0]["body"].as_str().unwrap().contains("Use scripts/t."), "{:?}", row.spec.input);
    assert!(f.notes().iter().any(|n| n.ends_with("as its own folder")), "{:?}", f.notes());
    // the session of a doer that reads a folder keeps its skills: a line that picks none is given the same set, and says nothing; another skill is added to it, and said
    f.turn("how was that");
    let set = |n: usize| -> Vec<String> { f.tasks()[n].spec.input["skills"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap().to_string()).collect() };
    assert_eq!(set(1), set(0), "the same set, so the live child is shared");
    assert_eq!(f.rig.of_kind("brief.skills").len(), 1, "nothing new: no row");
    f.turn("update the docs and the decision rows");
    assert_eq!(set(2), ["docs", "checks"], "the new pick, then what the session had: {:?}", set(2));
    assert_eq!(f.rig.of_kind("brief.skills").len(), 2);
    assert!(f.rig.of_kind("brief.skills")[1].data["line"].as_str().unwrap().contains("checks (kept from the session)"));
}

/// A doer with neither cap (codex, gemini, opencode: no Brief, no folder of skills).
fn bare() -> Arc<Scripted> {
    Scripted::with_caps("codex", Caps { live: true, resume: true, brief: false, skills: false, ..Caps::default() }, (0..2).map(|_| vec![Beat::Session("codex-s".into()), say("ok"), end("ok")]).collect())
}

fn cli_text_of(f: &crate::front_cases::Fx, n: usize) -> String {
    f.tasks()[n].spec.input.get("cli_text").and_then(Value::as_str).unwrap_or_default().to_string()
}

/// A doer that takes no Brief and lists no skills is
/// given the best-fitting skill's words after its goal, one at most; the Log says which; a doer that reads a folder or takes a Brief is given it as before.
#[test]
fn a_doer_with_no_brief_and_no_folder_is_given_one_skill_after_its_goal_and_the_others_as_before() {
    let f = crate::front_cases::fx_placed_on(vec![bare()], None, Vec::new(), on("all"), |c| {
        c.doer = Some("codex".into());
        c.skills = Some(Arc::new(library()));
    });
    f.turn("run the tests in src/lib.rs");
    let row = &f.tasks()[0];
    assert!(row.spec.input.get("brief").is_none(), "no Brief for it");
    let text = cli_text_of(&f, 0);
    assert!(text.contains("\n\nA skill that fits this task:\n### checks\n# Run the checks\nUse scripts/t."), "the skill follows the contract, behind `all`: {text}");
    assert!(!text.contains("Rust notes"), "one skill at most, the best fit: {text}");
    assert_eq!(row.spec.input["skills"].as_array().unwrap().len(), 1);
    let log = f.rig.of_kind("brief.skills");
    assert_eq!((log.len(), log[0].data["skills"][0]["id"].as_str()), (1, Some("checks")));
    assert!(!f.notes().iter().any(|n| n.ends_with("as its own folder")), "it has no folder: {:?}", f.notes());
    // a line that nothing fits gets nothing
    f.turn("how are you today my friend");
    assert_eq!(cli_text_of(&f, 1), "");
    // the other kinds as before: a doer that reads a folder has the folder and no text, a doer with a Brief has the section
    let folder = fx(vec![doer(false)], |c| {
        c.doer = Some("claude".into());
        c.skills = Some(Arc::new(library()));
    });
    folder.turn("run the tests in src/lib.rs");
    assert_eq!(cli_text_of(&folder, 0), "");
    assert!(folder.tasks()[0].spec.input["skills"][0]["body"].as_str().is_some(), "claude's folder is made from the bodies");
    let model = fx(vec![doer(true)], |c| {
        c.doer = Some("claude".into());
        c.skills = Some(Arc::new(library()));
    });
    model.turn("run the tests in src/lib.rs");
    assert_eq!(cli_text_of(&model, 0), "");
    assert!(briefs_of(&model)[0].contains("## skills\n### checks"), "{}", briefs_of(&model)[0]);
}

struct Card;

impl j4n_core::front::ProjectFacts for Card {
    fn text(&self) -> Option<String> {
        Some("Project card, derived from the files:\nCargo.toml: rust".into())
    }
}

fn on(setting: &str) -> j4n_core::front::screen::Screen {
    let mut screen = j4n_core::front::screen::Screen::builtin();
    screen.set = j4n_core::setting::Settings::from_pairs([("brief.cli", j4n_core::value::Value::text(setting))]).tree();
    screen
}

/// `briefed` returned before it built a section for a doer with no Brief, so claude never saw the contract, the project card, the plan
/// line or the shape notes. Behind `brief.cli` it is given them as text after its goal: the card once, nothing for an empty section, the skills left to its folder.
#[test]
fn a_doer_with_no_brief_is_given_one_as_text_after_its_goal_only_behind_the_setting() {
    for (set, given) in [("off", false), ("claude", true), ("all", true)] {
        let d = doer(false);
        let f = crate::front_cases::fx_placed_on(vec![d.clone()], None, Vec::new(), on(set), |c| {
            c.doer = Some("claude".into());
            c.card = Some(Arc::new(Card));
            c.skills = Some(Arc::new(library()));
        });
        f.turn("run the tests in src/lib.rs");
        f.turn("and once more");
        let (first, second) = (cli_text_of(&f, 0), cli_text_of(&f, 1));
        assert!(f.tasks().iter().all(|r| r.spec.input.get("brief").is_none()), "it is text, not the Brief of a model doer");
        if given {
            assert!(first.starts_with("## contract\nDo what the task says"), "{first}");
            assert!(first.contains("## project\nProject card, derived from the files:\nCargo.toml: rust"), "{first}");
            assert!(!first.contains("## skills") && !first.contains("Use scripts/t."), "claude has the skills as its folder: {first}");
            assert_eq!(second, "", "the contract and the card are said once, on the first line of the session: {second}");
            let row = &f.rig.of_kind("brief.rendered")[0].data;
            assert_eq!((row["as"].as_str(), row["sections"].as_array().unwrap().len()), (Some("text"), 2), "{row}");
            // and it is what the doer is started with: the goal, a blank line, then the sections
            let started = d.starts()[0].text.clone().unwrap_or_default();
            assert!(started.starts_with("run the tests in src/lib.rs\n\n## contract\n") && started.ends_with("Cargo.toml: rust"), "{started}");
            assert_eq!(d.starts()[1].text, None, "the second line carries on in the session with nothing added");
        } else {
            assert_eq!((first.as_str(), second.as_str()), ("", ""), "off: nothing is added");
            assert!(f.rig.of_kind("brief.rendered").is_empty());
        }
    }
}

/// `brief.cli = claude` (the default) reaches the doer that reads a folder of skills and no other; `all` reaches a bare doer too.
#[test]
fn the_default_reaches_claude_and_not_a_bare_doer_and_all_reaches_both() {
    for (set, given) in [("claude", false), ("all", true)] {
        let f = crate::front_cases::fx_placed_on(vec![bare()], None, Vec::new(), on(set), |c| {
            c.doer = Some("codex".into());
            c.card = Some(Arc::new(Card));
        });
        f.turn("run the tests in src/lib.rs");
        assert_eq!(cli_text_of(&f, 0).contains("## contract"), given, "{set}: {}", cli_text_of(&f, 0));
    }
}

#[test]
fn the_strip_counts_the_skills_of_the_tasks_that_run_and_says_nothing_when_there_are_none() {
    use j4n_core::front::compose::{Strip, Work, strip};
    let one = Strip { route: "claude · sonnet 5.5", mode: "write", work: vec![Work { label: "working".into() }], skills: 2, ..Default::default() };
    assert_eq!(strip(&one)["skills"], 2);
    assert!(strip(&Strip { skills: 0, ..one }).get("skills").is_none(), "nothing running with skills: the strip is what it was");
}

fn row(text: &str) -> toml::Table {
    text.parse().unwrap()
}

#[test]
fn a_connected_agent_has_a_skill_made_from_its_row_that_says_when_to_read_it_how_its_prompt_goes_in_and_that_the_gate_is_asked() {
    let r = row("doc = \"`zorvik`, learned by trying it\"\nis = \"agent\"\ndialect = \"jsonl-map\"\nprogram = \"/opt/zorvik\"\ngated = true\nroles = [\"work\"]\nstdin = \"{prompt}\"\nresume = [\"--session\", \"{session}\"]\n[map]\nevents = [{ when = \"/k == 'tool'\" }]\n");
    let s = j4n_core::skill::of_connector("zorvik", &r).unwrap();
    assert_eq!((s.id.as_str(), s.from.as_str(), s.when.as_str()), ("connector-zorvik", "connector:zorvik", "zorvik"));
    assert!(s.description.starts_with("Read before a task") && s.description.contains("`zorvik`"), "{}", s.description);
    for line in ["- use: `--doer zorvik`", "offered for the role work", "- prompt: on its standard input", "a later line carries on in its session", "its tool calls are shown", "goes to the Gate first", "its usage limit is not read"] {
        assert!(s.body.contains(line), "{line} is not in\n{}", s.body);
    }
}

#[test]
fn a_tool_connector_has_a_skill_that_names_its_arguments_and_its_risk_and_one_that_runs_no_program_has_none() {
    let r = row("doc = \"`fmt run`: formats a file\"\nis = \"tool\"\ndialect = \"text\"\nprogram = \"fmt\"\nrisk = \"write\"\nargv = [\"fmt\", \"run\", \"{path}\"]\nargs = [{ name = \"path\", required = true }]\nnet = false\n");
    let s = j4n_core::skill::of_connector("fmt-run", &r).unwrap();
    assert_eq!(s.id, "connector-fmt-run");
    assert!(s.description.contains("`fmt`") && s.description.contains("`fmt run {path}`"), "{}", s.description);
    for line in ["`path`", "- risk: write", "a `write` run asks before it runs", "network: cut"] {
        assert!(s.body.contains(line), "{line} is not in\n{}", s.body);
    }
    for none in ["is = \"agent\"\ndialect = \"test\"\nprogram = \"x\"\n", "is = \"agent\"\ndialect = \"text\"\n", "is = \"model\"\ndialect = \"openai\"\nprogram = \"x\"\n"] {
        assert!(j4n_core::skill::of_connector("x", &row(none)).is_none(), "{none}");
    }
}

#[test]
fn the_skill_of_a_connection_is_picked_by_the_name_of_the_connector_in_the_line_and_an_operators_own_skill_of_the_same_name_replaces_it() {
    let made = j4n_core::skill::of_connector("zorvik", &row("is = \"agent\"\ndialect = \"text\"\nprogram = \"zorvik\"\nstart = [\"{prompt}\"]\n")).unwrap();
    let l = Library::new(vec![made.clone(), skill("checks", "tests, lint", 200, "# Run the checks")], "");
    assert_eq!(ids(&l, &ask("ask zorvik to count the lines", &[], &[])), ["connector-zorvik"]);
    assert!(ids(&l, &ask("fix the build", &[], &[])).is_empty());
    let own = skill("connector-zorvik", "zorvik", 50, "# My notes on zorvik");
    let l = Library::new(vec![made, own], "");
    assert_eq!(l.get("connector-zorvik").unwrap().about(), "My notes on zorvik");
}


#[test]
fn an_agent_over_a_live_protocol_is_said_to_carry_on_in_its_session_with_no_resume_in_its_row() {
    let live = row("id = \"x\"\nis = \"agent\"\ndialect = \"claude-stream\"\nprogram = \"/bin/x\"\n");
    let skill = j4n_core::skill::of_connector("x", &live).unwrap();
    assert!(skill.body.contains("a later line carries on in its session"));
    assert_eq!(skill.about(), "x: `/bin/x`, connected as an agent.", "a list of skills shows the first line");
    let text = row("id = \"x\"\nis = \"agent\"\ndialect = \"text\"\nprogram = \"/bin/x\"\nstart = [\"{prompt}\"]\n");
    assert!(j4n_core::skill::of_connector("x", &text).unwrap().body.contains("it keeps no session between lines"));
}
