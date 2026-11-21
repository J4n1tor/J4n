//! Tier 0, the real binary with a fake `claude`: the skills J4n carries are in the library with where each came from,
//! a task is given the few that fit (claude as a folder of its own for the task, never under `~/.claude`), the Log says which and why, the one switch and a
//! skill switched off with `:skill off` hold for the next run, and what the binary carries is what the pack holds on disk.

use crate::run_cases::{Dirs, Out, fake_claude, j4n, log_events};
use crate::support::{read, root};
use j4n::pack::{SKILL_FILES, SKILLS};
use serde_json::Value;
use std::fs;

/// A run of `claude` (the fake, playing a recording) on one line.
fn run_claude(d: &Dirs, line: &str) -> Out {
    let (program, fixture, log) = fake_claude(d, "two-turns.jsonl");
    let env = [("J4N_CLAUDE", program.display().to_string()), ("FAKE_FIXTURE", fixture.display().to_string()), ("FAKE_LOG", log.display().to_string())];
    let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    j4n(d, &["run", line, "--doer", "claude", "--approval", "deny"], None, &env)
}

/// The arguments the fake `claude` was started with, one string for each start.
fn starts(d: &Dirs) -> Vec<String> {
    read(&d.dir.join("bin/log")).lines().filter_map(|l| l.strip_prefix("start ").map(String::from)).collect()
}

fn picked(d: &Dirs) -> Vec<Value> {
    log_events(d).into_iter().filter(|e| e["kind"] == "brief.skills").collect()
}

#[test]
fn the_binary_carries_the_skills_of_the_pack_as_they_are_on_disk_and_each_reads() {
    let dir = root().join("packs/skills/skills");
    let mut on_disk: Vec<String> = fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    on_disk.sort();
    assert_eq!(SKILL_FILES.iter().map(|(_, f, _)| f.to_string()).collect::<Vec<_>>(), on_disk, "every skill of the pack is carried, and no other");
    for (pack, folder, text) in SKILL_FILES {
        assert_eq!(text, read(&dir.join(folder).join("SKILL.md")), "{folder}");
        let s = j4n_core::skill::parse(&format!("pack:{pack}"), folder, text).unwrap_or_else(|e| panic!("{folder}: {e}"));
        assert_eq!(s.id, folder, "the name is the folder's");
        assert!(s.cost > 0 && s.cost < 400 && !s.about().is_empty(), "{folder}: a short skill with a first line: {} tokens", s.cost);
    }
    let data: Vec<&str> = SKILLS.iter().map(|(p, _)| *p).collect();
    assert_eq!(data, ["commands/skills.toml", "pages/skills.toml", "settings/skills.toml", "views/skills.toml"]);
}

#[test]
fn a_task_for_claude_is_given_the_skills_that_fit_as_its_own_folder_in_its_place_and_the_log_says_which_and_why() {
    let d = Dirs::new("skills-claude");
    d.put("src/lib.rs", "fn a() {}\n");
    d.put("crates/j4n-core/Cargo.toml", "[package]\n");
    let o = run_claude(&d, "run the tests in src/lib.rs");
    assert_eq!(o.rc, 0, "{}{}", o.out, o.err);
    let args = &starts(&d)[0];
    let dir = std::path::PathBuf::from(args.split("--plugin-dir ").nth(1).unwrap_or_else(|| panic!("no plugin folder: {args}")).split(' ').next().unwrap());
    assert!(dir.starts_with(d.project().join(".j4n/skills")), "the folder is the task's own, in its place: {}", dir.display());
    assert!(!dir.starts_with(d.home()) && !dir.to_string_lossy().contains(".claude"), "never the home or `~/.claude`: {}", dir.display());
    assert!(!d.home().join(".claude").exists(), "nothing was written to `~/.claude`");
    let skill = read(&dir.join("skills/j4n-checks/SKILL.md"));
    assert!(skill.starts_with("---\nname: j4n-checks\ndescription: \"Use when: ") && skill.contains("scripts/t"), "{skill}");
    let all: Vec<String> = fs::read_dir(dir.join("skills")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert!(!all.contains(&"j4n-log".to_string()) && !all.contains(&"j4n-docs".to_string()), "a skill that did not fit is not in claude's folder: {all:?}");
    let rows = picked(&d);
    assert_eq!(rows.len(), 1, "one row for the task");
    let ids: Vec<&str> = rows[0]["data"]["skills"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert!(ids.contains(&"j4n-checks") && ids.len() <= 3, "{ids:?}");
    assert!(rows[0]["data"]["line"].as_str().unwrap().contains("said tests") && rows[0]["data"]["line"].as_str().unwrap().ends_with("as its own folder"), "{}", rows[0]["data"]["line"]);
    assert!(o.out.contains("skills · ") || o.err.contains("skills · "), "the chat says so too:\n{}{}", o.out, o.err);
}

#[test]
fn j4ns_own_skills_are_offered_only_in_j4ns_own_repository_and_a_users_own_skill_is_picked_anywhere() {
    // a project that is not J4n's own gets none of the four shipped skills for a goal that says test, and its owner's skill still fits
    let d = Dirs::new("skills-elsewhere");
    d.put("src/lib.rs", "fn a() {}\n");
    let o = run_claude(&d, "run the tests in src/lib.rs");
    assert_eq!(o.rc, 0, "{}{}", o.out, o.err);
    assert!(!starts(&d)[0].contains("--plugin-dir") && picked(&d).is_empty(), "no shipped skill outside J4n's repository: {}", starts(&d)[0]);
    fs::create_dir_all(d.j4n_home().join("skills/mine")).unwrap();
    fs::write(d.j4n_home().join("skills/mine/SKILL.md"), "---\nname: mine\nwhen: tests, pytest\ncost: 40\n---\n# My test notes\nRun pytest -q.\n").unwrap();
    let o = run_claude(&d, "run the tests in src/lib.rs");
    assert_eq!(o.rc, 0, "{}{}", o.out, o.err);
    let rows = picked(&d);
    let ids: Vec<&str> = rows[0]["data"]["skills"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["mine"], "the user's own skill is picked, the shipped ones are not");
}

#[test]
fn the_general_skills_fit_any_project_and_claude_lists_each_by_a_sentence() {
    // the four shipped skills are about J4n's own repository, so in every other project the pack gave nothing; these five fit any project
    let d = Dirs::new("skills-general-fit-any");
    d.put("src/lib.rs", "fn a() {}\n");
    for (line, want) in [
        ("fix the failing test, add a changelog line under Unreleased and bump the version", "release-chores"),
        ("find the commit that introduced the regression in src/lib.rs", "culprit-commit"),
        ("the logs in logs/ show an incident: what failed first?", "log-hunt"),
        ("a test fails with a traceback: find the cause", "debug-cause"),
        ("summarize the log of the last run", "narrow-reads"),
    ] {
        let o = run_claude(&d, line);
        assert_eq!(o.rc, 0, "{}{}", o.out, o.err);
        let last = picked(&d).pop().unwrap_or_else(|| panic!("no skill picked for `{line}`"));
        let ids: Vec<&str> = last["data"]["skills"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap()).collect();
        assert!(ids.contains(&want) && ids.iter().all(|i| !i.starts_with("j4n-")), "`{line}`: {ids:?}");
        let args = starts(&d).pop().unwrap();
        let dir = std::path::PathBuf::from(args.split("--plugin-dir ").nth(1).unwrap().split(' ').next().unwrap());
        let skill = read(&dir.join(format!("skills/{want}/SKILL.md")));
        assert!(skill.starts_with(&format!("---\nname: {want}\ndescription: \"Read ")) && !skill.contains("Use when:"), "{skill}");
    }
}

#[test]
fn a_line_that_no_skill_fits_gets_none_and_no_flag() {
    let d = Dirs::new("skills-none");
    let o = run_claude(&d, "say hello to the world");
    assert_eq!(o.rc, 0, "{}{}", o.out, o.err);
    assert!(!starts(&d)[0].contains("--plugin-dir") && picked(&d).is_empty(), "{}", starts(&d)[0]);
}

#[test]
fn the_one_switch_stops_the_pick_and_a_home_skill_is_in_the_library_beside_the_packs() {
    let d = Dirs::new("skills-switch");
    fs::write(d.j4n_home().join("settings/skills-off.toml"), "api = 1\n[[setting]]\nid = \"skills.pick\"\nvalue = false\n").unwrap();
    let o = run_claude(&d, "run the tests");
    assert_eq!(o.rc, 0, "{}{}", o.out, o.err);
    assert!(!starts(&d)[0].contains("--plugin-dir") && picked(&d).is_empty(), "the switch is off: nothing is picked");
    let d = Dirs::new("skills-home");
    fs::create_dir_all(d.j4n_home().join("skills/mine")).unwrap();
    fs::write(d.j4n_home().join("skills/mine/SKILL.md"), "---\nname: mine\nwhen: parser, grammar\ncost: 40\n---\n# My parser notes\nKeep the grammar flat.\n").unwrap();
    let o = j4n(&d, &["run", ":skill"], None, &[]);
    let said = format!("{}{}", o.out, o.err);
    assert!(said.contains("mine on  home") && said.contains("j4n-checks on  pack:skills"), "each says where it came from:\n{said}");
    let o = run_claude(&d, "fix the grammar of the parser");
    assert_eq!(o.rc, 0, "{}{}", o.out, o.err);
    assert_eq!(picked(&d)[0]["data"]["skills"][0]["id"], "mine");
}

#[test]
fn a_skill_switched_off_is_kept_for_the_next_start_and_is_never_picked_until_it_is_on_again() {
    let d = Dirs::new("skills-off");
    let o = j4n(&d, &["run", ":skill off j4n-checks"], None, &[]);
    assert_eq!(o.rc, 0, "{}{}", o.out, o.err);
    assert!(format!("{}{}", o.out, o.err).contains("skill j4n-checks is off"), "{}{}", o.out, o.err);
    let o = run_claude(&d, "run the tests");
    assert_eq!(o.rc, 0, "{}{}", o.out, o.err);
    assert!(!starts(&d)[0].contains("--plugin-dir"), "the only skill that fits is off: {}", starts(&d)[0]);
    let o = j4n(&d, &["run", ":skill"], None, &[]);
    assert!(format!("{}{}", o.out, o.err).contains("j4n-checks off"), "{}{}", o.out, o.err);
    let o = j4n(&d, &["run", ":skill on j4n-checks"], None, &[]);
    assert!(format!("{}{}", o.out, o.err).contains("skill j4n-checks is on"), "{}{}", o.out, o.err);
    d.put("crates/j4n-core/Cargo.toml", "[package]\n");
    let _ = run_claude(&d, "run the tests");
    assert!(starts(&d).iter().any(|s| s.contains("--plugin-dir")), "{:?}", starts(&d));
}

#[test]
fn skills_are_searched_read_and_a_name_that_is_not_there_is_refused() {
    let d = Dirs::new("skills-search");
    let said = |args: &str| {
        let o = j4n(&d, &["run", args], None, &[]);
        format!("{}{}", o.out, o.err)
    };
    let found = said(":skill log events");
    assert!(found.contains("j4n-log") && !found.contains("j4n-docs"), "{found}");
    assert!(said(":skill show j4n-pack").contains("A pack is a folder of data"));
    assert!(said(":skill off nope").contains("there is no skill `nope`"));
    assert!(said(":skill zzzz").contains("no skill matches `zzzz`"));
}
