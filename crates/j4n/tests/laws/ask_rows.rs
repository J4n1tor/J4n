//! The question the core makes is the question the screen was built for. The recorded claude asks leave to run
//! `make build` in the real wiring; the `AskView` the core sends is the one written out by hand below, field by field, and the two go to
//! the same screen at every size. What no event carries yet (a branch, a commit, the lines of a change not yet made) is left out of both.

use crate::screen_drive::{open, setup};
use crate::run_cases::Dirs;
use j4n_proto::{AskView, By, Cell, Choice, Down, Shape, Tone, Up};
use j4n_tui::App;
use j4n_tui::headless::Headless;
use j4n_tui::theme::{Settings, Theme};
use std::time::Instant;

/// The screen a client that was sent `downs` shows, in the `unicode` tier.
fn screen(downs: &[Down], cols: u16, high: u16) -> String {
    let mut app = App::new();
    app.start();
    for d in downs {
        app.down(d.clone());
    }
    app.theme = Theme::build(&Settings { marks: "unicode".into(), ..Settings::default() }, &[]).expect("a theme");
    Headless::new(cols, high).show(&mut app)
}

#[test]
fn an_approval_the_core_makes_is_the_one_written_by_hand_and_draws_the_same_screen() {
    let d = Dirs::new("ask-rows");
    let (setup, _log) = setup(&d, "m1-session.jsonl");
    let world = open(&setup);
    let (up, down) = world.front.connect();
    // a client of its own, that watches what the screen watches
    let mut app = App::new();
    for u in app.start() {
        up.send(u).unwrap();
    }
    up.send(Up::Intent { intent: j4n_proto::Intent::Say { text: "build the thing".into(), with: Vec::new() } }).unwrap();
    let (mut seen, end) = (Vec::new(), Instant::now() + crate::support::PATIENCE);
    let core = loop {
        let d = down.recv_timeout(end.saturating_duration_since(Instant::now())).expect("the core asks leave");
        for u in app.down(d.clone()) {
            up.send(u).unwrap();
        }
        seen.push(d.clone());
        if let Down::Asks { open } = d
            && let Some(a) = open.into_iter().next()
        {
            break a;
        }
    };

    let project = d.project().file_name().unwrap().to_string_lossy().to_string();
    let grant = |id: &str| Choice { id: id.into(), by: By::Core, widens: true, ..Choice::default() };
    let hand = AskView {
        ask: core.ask.clone(),
        asker: core.asker.clone(),
        question: "allow Bash: make build?".into(),
        shape: Shape::Approval,
        options: vec![grant("task"), grant("session"), grant("here")],
        effect: "shell: make build".into(),
        risk: 50,
        grants: vec!["task".into(), "session".into(), "here".into()],
        // how long it has waited is the clock's: whatever it was, it is worded as minutes and seconds
        wait: core.wait.clone(),
        facts: vec![Cell::with_icon("folder", project, Tone::Dim)],
        change: Vec::new(),
        why: "nothing allows it yet".into(),
    };
    assert!(core.wait.len() >= 4 && core.wait.as_bytes()[core.wait.len() - 3] == b':', "{:?}", core.wait);
    assert_eq!(core, hand, "the question the core sent is the one written by hand");

    let with = |ask: &AskView| -> Vec<Down> { seen.iter().map(|d| if matches!(d, Down::Asks { .. }) { Down::Asks { open: vec![ask.clone()] } } else { d.clone() }).collect() };
    let (a, b) = (with(&core), with(&hand));
    for (cols, high) in [(120, 40), (80, 24), (60, 20)] {
        let (x, y) = (screen(&a, cols, high), screen(&b, cols, high));
        assert_eq!(x, y, "at {cols}×{high} the screens differ");
        assert!(x.contains("make build"), "{x}");
        assert!(cols < 80 || x.contains("widens"), "the answers that widen are marked: {x}");
    }
}
