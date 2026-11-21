//! The base pack the binary carries is the base pack on disk: nothing added to `packs/base` is forgotten, nothing is in the
//! binary that the directory does not have. The kinds are the core's own and the manifest is not data the binary reads.

use crate::support::{read, root, walk_ext};
use j4n::pack::{BASE, base};
use j4n_core::answer::point::points;
use j4n_core::front::phrase::Phrases;
use j4n_core::gate::check::Gates;
use j4n_core::rank::specs;
use j4n_core::table::Tables;
use std::collections::BTreeSet;

fn on_disk() -> BTreeSet<String> {
    let dir = root().join("packs/base");
    walk_ext(&dir, "toml")
        .into_iter()
        .map(|p| p.strip_prefix(&dir).unwrap().to_string_lossy().into_owned())
        .filter(|p| !p.starts_with("kinds/") && !p.starts_with("tests/") && p != "pack.toml")
        .collect()
}

#[test]
fn the_binary_carries_every_data_file_of_the_base_pack_and_nothing_else() {
    let carried: BTreeSet<String> = BASE.iter().map(|(p, _)| p.to_string()).collect();
    assert_eq!(carried, on_disk());
}

#[test]
fn what_it_carries_is_what_is_on_disk_byte_for_byte() {
    for (path, text) in BASE {
        assert_eq!(text, read(&root().join("packs/base").join(path)), "{path}");
    }
}

#[test]
fn the_carried_pack_loads_clean_and_gives_gates_points_phrases_and_ranks() {
    let (tables, problems) = Tables::load(vec![base()]);
    assert!(problems.is_empty(), "{problems:?}");
    let (gates, found) = Gates::load(&tables);
    assert!(found.is_empty(), "{found:?}");
    let _ = gates;
    let (pts, found) = points(&tables);
    assert!(found.is_empty() && pts.iter().any(|p| p.id == "intake") && pts.iter().any(|p| p.id == "approval"), "{found:?}");
    let (phrases, found) = Phrases::load(&tables);
    assert!(found.is_empty() && phrases.parse(":fresh").is_some(), "{found:?}");
    let (ranks, found) = specs(&tables);
    assert!(found.is_empty() && ranks.contains_key("default"), "{found:?}");
}

#[test]
fn the_base_pack_describes_a_wall_seven_checks_the_rules_that_run_them_and_the_switches() {
    use j4n_connect::check::Check;
    use j4n_connect::place::boxed;
    use j4n_core::rule::Rules;
    use j4n_core::setting::Settings;
    use j4n_core::value::Value;
    let (tables, problems) = Tables::load(vec![base()]);
    assert!(problems.is_empty(), "{problems:?}");
    let (walls, found) = boxed::rows(&tables);
    assert!(found.is_empty() && walls.iter().map(|w| w.id.as_str()).collect::<Vec<_>>() == ["bwrap"], "{found:?}");
    let (check, found) = Check::load(&tables);
    assert!(found.is_empty(), "{found:?}");
    let dir = std::env::temp_dir().join(format!("j4n-pack-check-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("go.mod"), "module x\n").unwrap();
    assert_eq!(check.row_for(&dir).map(|r| r.id.clone()), Some("2-go".to_string()));
    let _ = std::fs::remove_dir_all(&dir);
    let (rules, found) = Rules::load(&tables);
    assert!(found.is_empty(), "{found:?}");
    assert!(rules.ids().contains(&"lane-check") && rules.ids().contains(&"lane-check-applied"));
    let (settings, found) = Settings::load(&tables);
    assert!(found.is_empty(), "{found:?}");
    assert_eq!(
        (settings.word("work.apply", "?"), settings.word("box.use", "?"), settings.word("box.unavailable", "?"), settings.get("box.net"), settings.get("check.after")),
        ("auto", "auto", "refuse", Some(&Value::Bool(true)), Some(&Value::Bool(true))),
        "a worktree lays its change down, a wall that cannot run refuses, the network is on, and the check is on"
    );
}

// ---- the packs `agent` and `code` ----

fn on_disk_of(pack: &str) -> BTreeSet<String> {
    let dir = root().join("packs").join(pack);
    walk_ext(&dir, "toml").into_iter().map(|p| p.strip_prefix(&dir).unwrap().to_string_lossy().into_owned()).filter(|p| p != "pack.toml").collect()
}

#[test]
fn the_binary_carries_every_data_file_of_the_agent_and_code_packs_byte_for_byte() {
    use j4n::pack::{AGENT, CODE};
    for (pack, carried) in [("agent", &AGENT[..]), ("code", &CODE[..])] {
        let names: BTreeSet<String> = carried.iter().map(|(p, _)| p.to_string()).collect();
        assert_eq!(names, on_disk_of(pack), "{pack}");
        for (path, text) in carried {
            assert_eq!(*text, read(&root().join("packs").join(pack).join(path)), "{pack}/{path}");
        }
    }
}

fn harness() -> (Tables, j4n_core::flow::Flows) {
    let (tables, problems) = Tables::load(vec![base(), j4n::pack::agent(), j4n::pack::code()]);
    assert!(problems.is_empty(), "{problems:?}");
    let (flows, found) = j4n_core::flow::Flows::load(&tables);
    assert!(found.is_empty(), "the flows of the packs pass their own lint: {found:?}");
    (tables, flows)
}

#[test]
fn the_flows_the_packs_carry_load_clean_and_the_phrases_that_start_them_name_flows_that_are_there() {
    let (tables, flows) = harness();
    let ids: Vec<&str> = flows.all().iter().map(|f| f.id()).collect();
    assert_eq!(ids.iter().copied().collect::<BTreeSet<_>>(), BTreeSet::from(["design", "plan", "plan-work-validate", "react", "recall", "remember", "signin", "work"]));
    let (phrases, found) = Phrases::load(&tables);
    assert!(found.is_empty(), "{found:?}");
    assert_eq!(phrases.flows(), ["design", "plan", "plan-work-validate"]);
    for flow in phrases.flows() {
        assert!(flows.get(&flow).is_some(), "{flow}");
    }
    let cmd = phrases.parse("/plan add a --verbose flag").unwrap();
    assert_eq!((cmd.name.as_str(), cmd.args.join(" ")), ("flow", "plan add a --verbose flag".to_string()));
    assert_eq!(phrases.parse(":build x").unwrap().args[0], "plan-work-validate");
    // the own agent is a connector with the tools J4n carries, and the tools are a connector too
    let (connectors, found) = j4n_connect::manifest::Connectors::load(&tables);
    assert!(found.is_empty(), "{found:?}");
    assert_eq!(connectors.get("own").map(|c| c.tools.clone()), Some(["builtin", "*", "web", "ask"].map(String::from).to_vec()));
    assert!(connectors.get("builtin").is_some());
}

#[test]
fn a_plan_and_a_design_only_read_and_the_own_agents_loops_are_pinned_to_the_turn_limits_l8() {
    let (_, flows) = harness();
    for id in ["plan", "design"] {
        let f = flows.get(id).unwrap();
        let works: Vec<_> = f.row.steps.iter().filter(|s| s.doer.is_some()).collect();
        assert!(!works.is_empty() && works.iter().all(|s| s.mode == Some(j4n_core::task::Mode::Read)), "{id}: every step that does something is told to only read");
    }
    // the loop of a lane that works is a hundred turns, and one that only reads is ten; the constant is the same number
    let turns = |id: &str| flows.get(id).unwrap().row.steps[0].turns;
    assert_eq!((turns("react"), turns("work")), (Some(10), Some(100)));
    assert_eq!(j4n_connect::model::doer::TURNS_MAX, 100);
    assert_eq!(flows.get("react").unwrap().row.steps[0].mode, Some(j4n_core::task::Mode::Read));
    // plan, work in a worktree that is held, check where it was done, validate by another, ask before it is applied
    let pwv = flows.get("plan-work-validate").unwrap();
    let work = &pwv.row.steps[pwv.step("work").unwrap()];
    assert_eq!((work.place.as_deref(), work.hold, work.max_visits), (Some("worktree"), true, None));
    let again = &pwv.row.steps[pwv.step("again").unwrap()];
    assert_eq!(again.max_visits, Some(3));
    assert_eq!(pwv.row.steps[pwv.step("apply").unwrap()].apply.as_deref(), Some("work"));
    assert_eq!(pwv.row.steps[pwv.step("check").unwrap()].place.as_deref(), Some("@work"));
    assert!(pwv.row.steps[pwv.step("validate").unwrap()].exclude.iter().any(|e| e.contains("steps.work.doer")));
}
