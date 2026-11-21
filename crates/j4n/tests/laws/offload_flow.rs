//! The parts that need the binary's wiring: the pack `offload` is on as shipped (the switch on, the cap $2, the classes
//! loaded), `:offload off` and `:setup doer` are proposals written to the operator's folder that the next start reads and `:undo` takes back, and
//! `j4n doctor` says how long each doer's probe took. Tier 0: the doers are scripts of the test's own, no network, no paid call.

use crate::doctor_flow::world;
use crate::screen_drive::Operator;
use crate::run_cases::{Dirs, j4n};
use j4n_core::gate::grant::here_dir;
use std::fs;

fn held(d: &Dirs, file: &str) -> String {
    fs::read_to_string(here_dir(&d.j4n_home(), &d.project()).join(file)).unwrap_or_default()
}

#[test]
fn offload_is_on_as_shipped_with_the_cap_the_order_and_its_classes_and_off_by_a_proposal_that_undo_takes_back() {
    let d = Dirs::new("offload-switch");
    let w = world(&d, true);
    let o = w.front.cfg().offload.clone();
    assert!(o.on && w.front.offload_on(), "on when nothing was said");
    assert_eq!((o.cap_usd, o.busy_at, o.order.join(" "), o.model.as_str()), (2.0, 0.8, "haiku pro own".to_string(), "haiku"));
    let ids: Vec<&str> = o.chores.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["build", "digest", "lint", "review", "search", "tests"], "the classes of the pack, as it ships");
    assert!(w.notices.iter().all(|n| !n.contains("offload") && !n.contains("chore")), "{:?}", w.notices);
    let mut op = Operator::sit(&w);
    op.type_line(":offload");
    op.until("the state said", |s| s.contains("offload on") && s.contains("of $2.00 spent today") && s.contains("review (asks)") && s.contains("order haiku pro own"));
    op.type_line(":offload off");
    op.until("said back", |s| s.contains("offload: off"));
    assert!(!w.front.offload_on());
    let file = held(&d, "offload.toml");
    assert!(file.contains("id = \"offload.on\"") && file.contains("value = false"), "{file}");
    // the next start reads it from the file
    drop(op);
    w.close();
    drop(w);
    let again = world(&d, true);
    assert!(!again.front.cfg().offload.on, "the file says off");
    let mut op = Operator::sit(&again);
    op.type_line(":undo");
    op.until("taken back", |s| s.contains("took back 1 change"));
    assert!(!held(&d, "offload.toml").contains("value = false"), "{}", held(&d, "offload.toml"));
    op.type_line(":offload maybe");
    op.until("a word that is none", |s| s.contains("maybe is not a word of offload"));
    drop(op);
    again.close();
}

#[test]
fn the_operator_picks_the_main_doer_in_setup_and_the_default_row_and_the_next_start_follow_it() {
    let d = Dirs::new("offload-pick");
    let w = world(&d, true);
    assert_eq!(w.front.main_pick(), None);
    // said as the screen says it: the pick is said in the chat, which stays open (the Setup page is gone)
    assert_eq!(w.front.say("main", ":setup doer codex"), j4n_core::front::Said::Command("setup".into()), "{:?}", w.front.log().read(0).filter(|e| e.kind == "chat.note").map(|e| e.data["text"].clone()).collect::<Vec<_>>());
    assert_eq!(w.front.main_pick().as_deref(), Some("codex"));
    let file = held(&d, "lane.toml");
    assert!(file.contains("id = \"lane.doer\"") && file.contains("value = \"codex\""), "{file}");
    assert!(matches!(w.front.say("main", ":setup doer nobody"), j4n_core::front::Said::Refused(m) if m.contains("`nobody` is not ready for work")));
    assert_eq!(w.front.main_pick().as_deref(), Some("codex"), "a refusal changes nothing");
    // the pre-flight's default row follows it: `view:setup`, which no page shows now, still says it
    {
        use j4n_core::front::serve::Serve;
        // the programs have answered, as they have by the time an operator reads the rows
        if let Some(c) = w.front.cfg().setup.as_ref() {
            c.report_now(w.front.main_pick());
        }
        let (downs, _) = Serve::new(w.front.clone()).up(j4n_proto::Up::Watch { bind: "view:setup".into() });
        let said = format!("{downs:?}");
        assert!(said.contains("default doer") && said.contains("codex") && said.contains("your pick") && !said.contains("not ready"), "{said}");
    }
    w.close();
    drop(w);
    let again = world(&d, true);
    assert_eq!(again.front.cfg().main.doer.as_deref(), Some("codex"), "the next start reads it from the file");
    assert_eq!(again.front.main_pick().as_deref(), Some("codex"));
    assert_eq!(again.front.say("main", ":undo"), j4n_core::front::Said::Command("undo".into()));
    assert_eq!(again.front.main_pick().as_deref(), None, "taken back at once: the file no longer says it");
    assert!(!held(&d, "lane.toml").contains("value = \"codex\""), "{}", held(&d, "lane.toml"));
    again.close();
}

#[test]
fn doctor_says_how_long_each_doers_probe_took_and_calls_nothing_that_costs() {
    let d = Dirs::new("offload-doctor");
    world(&d, true).close();
    let bin = d.dir.join("bin").display().to_string();
    let program = d.dir.join("bin/claude").display().to_string();
    let o = j4n(&d, &["doctor"], None, &[("PATH", bin.as_str()), ("J4N_CLAUDE", program.as_str())]);
    assert_eq!(o.rc, 0, "{}{}", o.out, o.err);
    let times: Vec<&str> = o.out.lines().filter(|l| l.starts_with("doer")).collect();
    assert_eq!(times.len(), 7, "{}", o.out);
    for l in times.iter().filter(|l| !l.contains(" — ")) {
        assert!(l.contains(" ms"), "a doer that is ready has its time: {l}");
    }
    assert!(o.out.lines().any(|l| l.starts_with("doer") && l.contains("claude") && l.contains("found · signed in · 2.1.0") && l.contains(" ms")), "{}", o.out);
    assert!(o.out.lines().any(|l| l.starts_with("default") && l.contains("claude") && l.contains("Agents › Setup › Agent: ⏎ on one uses it for the main lane")), "{}", o.out);
}

#[test]
fn the_pack_passes_its_own_check_with_the_cases_it_ships() {
    // `packs/offload/tests/offload.toml` is run here (the gate's pack-test stage names it as run by the tests stage)
    let d = Dirs::new("offload-pack-check");
    let pack = crate::support::root().join("packs/offload");
    let o = j4n(&d, &["pack", "check", &pack.display().to_string()], None, &[]);
    assert_eq!(o.rc, 0, "{}{}", o.out, o.err);
    assert!(o.out.contains("pack offload: ok (5 files") && o.out.contains("6 cases"), "{}", o.out);
}
