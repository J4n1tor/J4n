//! `/usage` is answered from the Log with no doer and no model. The front, the loop and the screen are the
//! real ones, wired as the binary wires them: the line starts no task, the chat holds the view as a row, and a window read afterwards (as a
//! claude run writes it) is drawn in that row without another line being said.

use crate::screen_drive::Operator;
use crate::run_cases::{Dirs, log_events};
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use j4n_core::change::Actor;
use j4n_core::log::New;
use j4n_core::task::Mode;
use serde_json::json;

fn open(d: &Dirs) -> World {
    let opts = Opts { doer: Some("test".into()), model: None, approval: Some(Approval::Deny), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false };
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: "claude".into() };
    crate::support::opened(|| wire::open(setup.clone(), &opts, true, "main"))
}

fn kinds(d: &Dirs, kind: &str) -> usize {
    log_events(d).iter().filter(|e| e["kind"] == kind).count()
}

#[test]
fn usage_starts_no_task_and_its_row_follows_the_windows() {
    let d = Dirs::new("usage");
    let world = open(&d);
    assert!(world.notices.is_empty(), "{:?}", world.notices);
    let mut op = Operator::sit(&world);
    op.type_line("/usage");
    op.until("the view, with no window yet", |s| s.contains("no runs yet"));
    op.type_line(":usage");
    op.until("the second row", |_| kinds(&d, "chat.view") == 2);
    assert_eq!((kinds(&d, "task.queued"), kinds(&d, "task.started"), kinds(&d, "brief.rendered")), (0, 0, 0), "no task, no brief: nothing was asked of a doer");
    // a claude run in the operator's own login reads a window (the row is on the screen: no line is said)
    let resets = world.front.log().clock().now_ms() / 1000 + 3 * 3600;
    world.front.log().append(New::of("run.window", Actor::Doer("claude".into()), json!({ "window": "five_hour", "utilization": 0.42, "resets_at": resets, "status": "allowed" })).task("t1").thread("main")).unwrap();
    let s = op.until("the bar", |s| s.contains("42%"));
    assert!(s.contains("claude · default") && s.contains("5h") && !s.contains("no runs yet"), "{s}");
    drop(op);
    world.close();
}
