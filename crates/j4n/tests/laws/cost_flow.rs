//! `:calibrate` says what a request has to cost to teach J4n what a window of the plan is worth, and spends only on `yes`.
//! The front, the loop and the folds are the real ones, wired as the binary wires them; the doer is the scripted one.

use crate::screen_drive::Operator;
use crate::run_cases::{Dirs, log_events};
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use j4n_core::task::Mode;

fn open(d: &Dirs) -> World {
    open_capped(d, None)
}

fn open_capped(d: &Dirs, usd: Option<f64>) -> World {
    let opts = Opts { doer: Some("test".into()), model: None, approval: Some(Approval::Deny), wall: None, usd, mode: Mode::Write, place: None, shape: false, anywhere: false };
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: "claude".into() };
    crate::support::opened(|| wire::open(setup.clone(), &opts, true, "main"))
}

fn plan(d: &Dirs) {
    std::fs::create_dir_all(d.j4n_home()).unwrap();
    let row = "[[plan]]\nid = \"test\"\nconnector = \"test\"\nwindows = [\"five_hour=5h\", \"seven_day=7d\"]\nprice = \"$24\"\nassume = \"$1000\"\n";
    std::fs::write(d.j4n_home().join("plan.toml"), format!("api = 1\n{row}")).unwrap();
}

fn started(d: &Dirs) -> usize {
    log_events(d).iter().filter(|e| e["kind"] == "task.started").count()
}

#[test]
fn calibrate_says_what_it_will_spend_and_runs_only_when_told_to() {
    let d = Dirs::new("calibrate");
    plan(&d);
    let world = open(&d);
    assert!(world.notices.is_empty(), "{:?}", world.notices);
    let mut op = Operator::sit(&world);
    op.type_line(":calibrate");
    let s = op.until("the size said", |s| s.contains("two steps"));
    assert!(s.contains("five_hour") && s.contains("assumed") && s.contains("$6.84") && s.contains("$0.137"), "{s}");
    assert!(s.contains("two requests") && s.contains("3000 numbers") && s.contains(":calibrate yes"), "{s}");
    assert_eq!(started(&d), 0, "saying what it would spend spends nothing");
    op.type_line(":calibrate yes");
    op.until("a request run", |s| s.contains("1 to 3000") || s.contains("calibration"));
    assert_eq!(started(&d), 1);
    let queued = log_events(&d).into_iter().find(|e| e["kind"] == "task.queued").map(|e| e["data"]["spec"].clone()).unwrap();
    let goal = queued["goal"].as_str().unwrap_or_default();
    assert!(goal.contains("calibration of cost") && goal.contains("1 to 3000"), "the request is the one announced: {queued}");
    assert_eq!((queued["tune"]["mode"].as_str(), queued["limits"]["usd_soft"].as_f64()), (Some("read"), None), "it only reads, and has no cap of its own: {queued}");
    drop(op);
    world.close();
}

#[test]
fn calibrate_on_a_doer_with_no_plan_says_why() {
    let d = Dirs::new("calibrate-none");
    let world = open(&d);
    let mut op = Operator::sit(&world);
    op.type_line(":calibrate");
    op.until("no plan said", |s| s.contains("no plan"));
    assert_eq!(started(&d), 0);
    drop(op);
    world.close();
}

#[test]
fn the_size_of_the_request_is_the_operators_to_set() {
    let d = Dirs::new("calibrate-size");
    plan(&d);
    let size = "[[setting]]\nid = \"cost.calibrate.size\"\ndoc = \"x\"\nty = \"int\"\ndefault = 3000\nvalue = 500\n";
    let old = std::fs::read_to_string(d.j4n_home().join("plan.toml")).unwrap();
    std::fs::write(d.j4n_home().join("plan.toml"), format!("{old}{size}")).unwrap();
    let world = open(&d);
    assert!(world.notices.is_empty(), "{:?}", world.notices);
    let mut op = Operator::sit(&world);
    op.type_line(":calibrate yes");
    op.until("the request said", |s| s.contains("500 numbers"));
    let goal = log_events(&d).into_iter().find(|e| e["kind"] == "task.queued").map(|e| e["data"]["spec"]["goal"].to_string()).unwrap();
    assert!(goal.contains("1 to 500"), "{goal}");
    drop(op);
    world.close();
}

#[test]
fn the_request_has_no_cap_of_its_own_even_when_the_operator_set_one_for_the_tasks() {
    let d = Dirs::new("calibrate-cap");
    plan(&d);
    let world = open_capped(&d, Some(0.5));
    let mut op = Operator::sit(&world);
    op.type_line(":calibrate yes");
    op.until("the request said", |s| s.contains("3000 numbers"));
    let spec = log_events(&d).into_iter().find(|e| e["kind"] == "task.queued").map(|e| e["data"]["spec"].clone()).unwrap();
    assert_eq!(spec["limits"]["usd_soft"].as_f64(), None, "a cap of dollars would stop a request meant to cost part of a window: {spec}");
    drop(op);
    world.close();
}
