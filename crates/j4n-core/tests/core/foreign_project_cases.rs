//! A doer in one project does not read another project's state (its transcripts, its `.j4n`, J4n's folders of it), and its own stays
//! readable. Each place says it: the path rule, the shell matcher with the row on its class, the Gate that is told where the doer works, and the door.
//! A line the matcher cannot see into (a script the doer writes, a link, `find /`) is not here: the guard's document says what stays open.

use crate::world::{layer, refused};
use j4n_core::change::Actor;
use j4n_core::gate::check::{Gates, check};
use j4n_core::gate::danger::{Scope, danger_scoped};
use j4n_core::gate::foreign::{Own, encode, foreign};
use j4n_core::gate::grant::Grants;
use j4n_core::gate::{Action, Gate, NoGrants, Verb, Verdict};
use j4n_core::state::project_key;
use j4n_core::table::{LayerKind, Tables};
use std::path::Path;

const SITE: &str = "/home/me/Projects/app";

fn own() -> Own {
    Own { user: Some("/home/me".into()), j4n: Some("/home/me/.j4n".into()), root: Some(SITE.into()) }
}

fn key() -> String {
    project_key(Path::new(SITE))
}

fn gates() -> Gates {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/base/gates");
    let files: Vec<(String, String)> = ["modes.toml", "paths.toml", "shell.toml", "egress.toml"].iter().map(|f| (f.to_string(), std::fs::read_to_string(dir.join(f)).unwrap())).collect();
    let refs: Vec<(&str, &str)> = files.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let (t, problems) = Tables::load(vec![layer(LayerKind::Pack, "gates", &refs)]);
    assert!(refused(&problems).is_empty(), "{problems:?}");
    Gates::load(&t).0
}

fn told(mut a: Action, site: &str) -> Action {
    a.site = Some(site.into());
    a.state = own();
    a
}

fn read_at(site: &str, who: Actor, path: &str) -> Verdict {
    check(&gates(), &NoGrants, &told(Action::new(Verb::Read, who, path), site))
}

fn read(path: &str) -> Verdict {
    read_at(SITE, Actor::Doer("claude".into()), path)
}

fn line_at(site: &str, who: Actor, cmd: &str) -> Verdict {
    check(&gates(), &NoGrants, &told(Action::shell(who, cmd), site))
}

fn line(cmd: &str) -> Verdict {
    line_at(SITE, Actor::Doer("claude".into()), cmd)
}

fn blocked_by(v: &Verdict, row: &str) -> bool {
    matches!(v, Verdict::Block { by, .. } if by == row)
}

#[test]
fn the_cli_folder_of_a_working_directory_is_every_character_that_is_no_letter_or_digit_made_a_dash() {
    assert_eq!(encode("/home/me/Projects/app"), "-home-me-Projects-app");
    assert_eq!(encode("/home/me/.j4n/worktrees/ab12/t1"), "-home-me--j4n-worktrees-ab12-t1");
}

/// The path rule: another project's transcript is blocked, the doer's own folder (its memory, its big tool results) is read, and only a doer is judged.
#[test]
fn a_read_of_another_projects_transcript_is_blocked_and_the_own_folder_is_read() {
    for path in [
        "/home/me/.claude/projects/-home-me-Projects-other/9c1.jsonl",
        "/home/me/.claude/projects/-home-me-Projects-other",
        "/home/me/.claude/projects",
        // a project whose name starts with this one's is another project: no prefix is own
        "/home/me/.claude/projects/-home-me-Projects-app-tools/9c1.jsonl",
        "/home/me/.claude/projects/-home-me-Projects-app/../-home-me-Projects-other/9c1.jsonl",
        "/home/me/.claude/projects/*/9c1.jsonl",
    ] {
        assert!(blocked_by(&read(path), "other-projects-state"), "{path}: {:?}", read(path));
    }
    for path in [
        "/home/me/.claude/projects/-home-me-Projects-app/memory/MEMORY.md",
        "/home/me/.claude/projects/-home-me-Projects-app/7f/tool-results/1.txt",
        // the sources of another project are not its state
        "/home/me/Projects/other/src/main.rs",
        "/home/me/Projects/app/src/main.rs",
    ] {
        assert!(!matches!(read(path), Verdict::Block { .. }), "{path}: {:?}", read(path));
    }
    // the operator reads what they like: the rule is for a doer and a model
    // the doer's own folder is said yes to by a row, so that no card is asked for it (an allow by no rule is a question for the operator)
    assert_eq!(read("/home/me/.claude/projects/-home-me-Projects-app/memory/MEMORY.md"), Verdict::Allow { by: "own-state-is-read".into() });
    assert_eq!(read("/home/me/Projects/app/.j4n/log/0001.jsonl"), Verdict::Allow { by: "own-state-is-read".into() });
    let theirs = "/home/me/.claude/projects/-home-me-Projects-other/9c1.jsonl";
    assert!(!matches!(read_at(SITE, Actor::Operator, theirs), Verdict::Block { .. }));
    assert!(blocked_by(&read_at(SITE, Actor::Model("m".into()), theirs), "other-projects-state"));
}

/// A project's `.j4n` (its Log, its memory, its folds) and J4n's folders of a project under its home are that project's; the global parts of J4n's home are not.
#[test]
fn another_projects_log_and_the_homes_folders_of_it_are_blocked_and_the_own_ones_are_read() {
    let key = key();
    for path in [
        "/home/me/Projects/other/.j4n/log/0001.jsonl".to_string(),
        "/home/me/Projects/other/.j4n/memory.db".to_string(),
        "/home/me/.j4n/projects/0123456789abcdef/gates.toml".to_string(),
        "/home/me/.j4n/checkpoints/0123456789abcdef/x".to_string(),
        "/home/me/.j4n/checkpoints".to_string(),
        "/home/me/.j4n/log/0001.jsonl".to_string(),
        "/home/me/.j4n/memory.db".to_string(),
        "../other/.j4n/log/0001.jsonl".to_string(),
    ] {
        assert!(blocked_by(&read(&path), "other-projects-state"), "{path}: {:?}", read(&path));
    }
    for path in [
        "/home/me/Projects/app/.j4n/log/0001.jsonl".to_string(),
        ".j4n/log/0001.jsonl".to_string(),
        format!("/home/me/.j4n/projects/{key}/gates.toml"),
        format!("/home/me/.j4n/checkpoints/{key}/x"),
        // the library, the cache and the rest of J4n's home are the operator's own and global
        "/home/me/.j4n/cache/models.json".to_string(),
        "/home/me/.j4n/skills/x/SKILL.md".to_string(),
    ] {
        assert!(!matches!(read(&path), Verdict::Block { .. }), "{path}: {:?}", read(&path));
    }
}

/// A lane works in a worktree below J4n's home, so its folder is not the project's root: the lane's own transcripts, its own folders, and the project's
/// `.j4n` are its own; another project's are not.
#[test]
fn a_lane_of_the_same_project_is_own_and_a_lane_of_another_is_not() {
    let key = key();
    let lane = format!("/home/me/.j4n/worktrees/{key}/t1");
    let at = |p: &str| read_at(&lane, Actor::Doer("claude".into()), p);
    for path in [
        format!("{lane}/src/lib.rs"),
        // the worktrees of another project are its sources, not its state
        "/home/me/.j4n/worktrees/0123456789abcdef/t1/src/lib.rs".to_string(),
        format!("/home/me/.j4n/worktrees/{key}/t2/src/lib.rs"),
        "/home/me/Projects/app/.j4n/log/0001.jsonl".to_string(),
        format!("/home/me/.claude/projects/{}/memory/MEMORY.md", encode(&lane)),
        "/home/me/.claude/projects/-home-me-Projects-app/memory/MEMORY.md".to_string(),
    ] {
        assert!(!matches!(at(&path), Verdict::Block { .. }), "{path}: {:?}", at(&path));
    }
    for path in [
        "/home/me/Projects/other/.j4n/log/0001.jsonl".to_string(),
        "/home/me/.claude/projects/-home-me-Projects-other/9c1.jsonl".to_string(),
    ] {
        assert!(blocked_by(&at(&path), "other-projects-state"), "{path}: {:?}", at(&path));
    }
}

/// The shell matcher: every word of the line is looked at, so a lister, a recursive search and a `cd` name it too.
#[test]
fn a_shell_line_that_names_another_projects_state_is_blocked() {
    for cmd in [
        "ls ~/.claude/projects/-home-me-Projects-other/",
        "grep -r foo ~/.claude/projects",
        "cat /home/me/Projects/other/.j4n/log/0001.jsonl",
        "cat ../other/.j4n/log/0001.jsonl",
        "ls $HOME/.claude/projects",
        "cd ~/.claude/projects && ls",
        "head -c 200 ${HOME}/.claude/projects/-home-me-Projects-other/9c1.jsonl",
        "ls /home/me/.j4n/checkpoints /home/me/.j4n/log",
        "rg token /home/me/.claude/projects/-home-me-Projects-app-tools",
        "echo done; cat $(ls -d ~/.claude/projects/*)",
        "python3 -c \"print(open('/home/me/Projects/other/.j4n/log/0001.jsonl').read())\"",
    ] {
        assert!(blocked_by(&line(cmd), "other-projects-state-shell"), "`{cmd}`: {:?}", line(cmd));
    }
    for cmd in [
        "cat ~/.claude/projects/-home-me-Projects-app/memory/MEMORY.md",
        "ls .j4n/log",
        "cat /home/me/Projects/app/.j4n/log/0001.jsonl",
        "grep -rn foo --exclude-dir=.j4n .",
        "ls ../other/src",
        "cargo test",
    ] {
        assert!(!blocked_by(&line(cmd), "other-projects-state-shell"), "`{cmd}`: {:?}", line(cmd));
    }
    let theirs = "ls ~/.claude/projects";
    assert!(!blocked_by(&line_at(SITE, Actor::Operator, theirs), "other-projects-state-shell"));
}

/// The class itself, and the line that reads and writes at once.
#[test]
fn the_matcher_says_foreign_and_a_foreign_line_only_reads() {
    let scope = Scope { site: Some(SITE.into()), state: own(), ..Scope::default() };
    let d = danger_scoped("cat ~/.claude/projects/-home-me-Projects-other/9c1.jsonl", &scope);
    assert_eq!((d.class.as_str(), d.writes), ("foreign", false), "{d:?}");
    let d = danger_scoped("cat ~/.claude/projects/-home-me-Projects-app/memory/MEMORY.md", &scope);
    assert_ne!(d.class, "foreign", "{d:?}");
    // a line whose most dangerous class is more dangerous keeps it: a credentials file is a `secret` first
    assert_eq!(danger_scoped("cat ~/.claude/history.jsonl ~/.claude/projects", &scope).class, "secret");
}

/// The Gate is told where the doers work once; an action that arrives without it is judged from what the Gate holds.
#[test]
fn the_gate_that_is_told_where_the_doers_work_judges_a_read_with_no_state() {
    let gate = Gate::new(gates(), Grants::new("s1")).with_state(own());
    let mut a = Action::new(Verb::Read, Actor::Doer("claude".into()), "/home/me/.claude/projects/-home-me-Projects-other/9c1.jsonl");
    a.site = Some(SITE.into());
    assert!(blocked_by(&gate.check(&a), "other-projects-state"), "{:?}", gate.check(&a));
    a.target = "/home/me/.claude/projects/-home-me-Projects-app/memory/MEMORY.md".into();
    assert!(!matches!(gate.check(&a), Verdict::Block { .. }), "{:?}", gate.check(&a));
    // a Gate that was not told still has the folder the action names: the doer's own transcripts are read, another's are not; a project's `.j4n` it cannot place
    // (no folder named) it leaves alone
    let blank = Gate::new(gates(), Grants::new("s2"));
    assert!(!matches!(blank.check(&a), Verdict::Block { .. }));
    a.target = "/home/me/.claude/projects/-home-me-Projects-other/9c1.jsonl".into();
    assert!(blocked_by(&blank.check(&a), "other-projects-state"));
    a.target = "/home/me/Projects/other/.j4n/log/0001.jsonl".into();
    a.site = None;
    assert!(!matches!(blank.check(&a), Verdict::Block { .. }));
}

#[test]
fn the_judgement_by_whole_components_with_a_folder_that_holds_a_glob_never_own() {
    let o = own();
    let f = |p: &str| foreign(p, Some(SITE), &o, true);
    assert!(!f("~/.claude/projects/-home-me-Projects-app/x"));
    assert!(f("~/.claude/projects/-home-me-Projects-app*/x"));
    assert!(f("~/.claude/projects/-home-me-Projects-app-/x"));
    assert!(f("/home/me/other-config/projects/-home-me-Projects-other/x"));
    assert!(!f("/home/me/other-config/projects/-home-me-Projects-app/x"));
    // nothing is known: nothing is own, but a relative path with no folder is not placed
    assert!(foreign("/h/.claude/projects/-h-p/x", None, &Own::default(), true));
    assert!(!foreign(".j4n/log", None, &Own::default(), true));
}

/// The door: the CLI is told to ask the Gate about a read below these, because a deny rule cannot leave the doer's own folder out.
#[test]
fn the_door_asks_the_gate_about_the_state_folders() {
    let settings: serde_json::Value = serde_json::from_str(&j4n_core::gate::door::claude_settings()).unwrap();
    let asks: Vec<&str> = settings["permissions"]["ask"].as_array().unwrap().iter().filter_map(|v| v.as_str()).collect();
    for want in ["Read(~/.claude/projects/**)", "Read(~/.j4n/projects/**)", "Read(~/.j4n/log/**)", "Read(//**/.j4n/log/**)", "Read(//**/.j4n/memory.db)"] {
        assert!(asks.contains(&want), "{want}: {asks:?}");
    }
    // a lane's whole tree is below the worktrees folder of J4n's home: not a round trip for each file
    assert!(!asks.iter().any(|a| a.contains("worktrees") || *a == "Read(~/.j4n/**)" || *a == "Read(//**/.j4n/**)"), "{asks:?}");
    // the operator's own config folder is asked too
    let own: serde_json::Value = serde_json::from_str(&j4n_core::gate::door::claude_settings_in(Some("/srv/cfg/"))).unwrap();
    assert!(own["permissions"]["ask"].as_array().unwrap().iter().any(|a| a == "Read(//srv/cfg/projects/**)"), "{own}");
    // not denied: a deny beats an allow, and the doer's own folder is readable
    let denied = j4n_core::gate::door::denied_reads();
    assert!(!denied.iter().any(|d| d.contains(".claude/projects")), "{denied:?}");
}

/// The CLI's folder name is `replace(/[^a-zA-Z0-9]/g, "-")`, cut at 200 with a hash after: a folder with an underscore or a long path is still the doer's own, a
/// glob never is, and a folder with a space in its name is one place and not two pieces of a word.
#[test]
fn the_own_folder_is_found_by_the_clis_name_whole_and_a_folder_with_a_space_is_one_place() {
    let o = own();
    assert_eq!(encode("/home/me/my_app v2"), "-home-me-my-app-v2");
    assert!(!foreign("~/.claude/projects/-home-me-my-app-v2/memory/M.md", Some("/home/me/my_app v2"), &o, true));
    assert!(foreign("~/.claude/projects/-home-me-my-app-v2-tools/x", Some("/home/me/my_app v2"), &o, true));
    let long = format!("/home/me/{}", "deep/".repeat(60));
    let name = format!("{}-ab12cd", &encode(&long)[..200]);
    assert!(!foreign(&format!("~/.claude/projects/{name}/memory/M.md"), Some(&long), &o, true));
    assert!(foreign(&format!("~/.claude/projects/{}x/y", &name[..150]), Some(&long), &o, true));
    // the doer's own `.j4n`, in a folder with a space: read as a word, as a path, and in a line
    let site = "/home/me/My Projects/app";
    let own = Own { root: Some(site.into()), ..own() };
    assert!(!foreign_in_word("/home/me/My Projects/app/.j4n/log/0001.jsonl", site, &own));
    assert!(foreign_in_word("/home/me/My Projects/other/.j4n/log/0001.jsonl", site, &own));
    let scope = Scope { site: Some(site.into()), state: own.clone(), ..Scope::default() };
    assert_ne!(danger_scoped("cat '/home/me/My Projects/app/.j4n/log/0001.jsonl'", &scope).class, "foreign");
    assert_eq!(danger_scoped("cat '/home/me/My Projects/other/.j4n/log/0001.jsonl'", &scope).class, "foreign");
}

fn foreign_in_word(text: &str, site: &str, own: &Own) -> bool {
    j4n_core::gate::foreign::foreign_in(text, Some(site), own, true)
}
