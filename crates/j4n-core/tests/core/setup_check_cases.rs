//! The pre-flight never holds the screen: Connect › Setup and Library › Models answer at once while a program's probe is out, its rows say
//! `checking…` (never "not found" or "none works"), each doer's answer lands as it comes and the next tick draws it; one look runs at a
//! time, its facts are stamped when it ends, and a program that is not there (or did not answer in time) is asked again only after a while or on `r`.

use crate::front_cases::fx_screened;
use j4n_core::clock::ManualClock;
use j4n_core::doctor::{BACKOFF_MS, CHECKING, Check, DoerFact, Facts, Report, STALE_MS};
use j4n_core::doer::Probe;
use j4n_core::front::screen::Screen;
use j4n_core::front::serve::Serve;
use j4n_core::setting::Settings;
use j4n_core::table::{Layer, LayerKind, SourceFile, Tables};
use j4n_proto::{Down, Up};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// A door a probe waits at until the test opens it; once open it stays open.
#[derive(Clone, Default)]
struct Door(Arc<(Mutex<bool>, Condvar)>);

impl Door {
    fn open(&self) {
        *self.0.0.lock().unwrap() = true;
        self.0.1.notify_all();
    }

    fn wait(&self) {
        let mut open = self.0.0.lock().unwrap();
        while !*open {
            open = self.0.1.wait(open).unwrap();
        }
    }
}

/// A check over three declared doers: claude and codex wait at their doors, gemini is not there. `looks` counts the looks started,
/// `asked` the probes each doer was asked.
struct Rig {
    check: Check,
    claude: Door,
    codex: Door,
    looks: Arc<AtomicUsize>,
    asked: Arc<Mutex<Vec<String>>>,
}

/// `clock`: the clock of the check, and how long codex's probe takes on it.
fn rig(clock: Option<(Arc<ManualClock>, u64)>) -> Rig {
    let (claude, codex, looks, asked) = (Door::default(), Door::default(), Arc::new(AtomicUsize::new(0)), Arc::new(Mutex::new(Vec::new())));
    let frame = {
        let looks = looks.clone();
        move || {
            looks.fetch_add(1, Ordering::SeqCst);
            let declared = |id: &str| DoerFact { id: id.into(), work: true, pending: true, ..DoerFact::default() };
            Facts { doers: vec![declared("claude"), declared("codex"), declared("gemini")], key: true, ..Facts::default() }
        }
    };
    let probe = {
        let (claude, codex, asked, clock) = (claude.clone(), codex.clone(), asked.clone(), clock.clone());
        move |id: &str| {
            asked.lock().unwrap().push(id.to_string());
            match id {
                "claude" => claude.wait(),
                "codex" => {
                    codex.wait();
                    if let Some((c, took)) = &clock {
                        c.advance(*took);
                    }
                }
                _ => {}
            }
            let installed = id != "gemini";
            DoerFact { id: id.into(), work: true, probe: Some(Probe { installed, authed: installed, version: Some("1.0".into()), detail: String::new() }), fix: "install it".into(), ms: Some(5), ..DoerFact::default() }
        }
    };
    let check = Check::new(frame, probe);
    let check = match clock {
        Some((c, _)) => check.with_clock(c),
        None => check,
    };
    Rig { check, claude, codex, looks, asked }
}

fn what(r: &Report, id: &str) -> String {
    r.rows.iter().find(|x| x.id == id).unwrap_or_else(|| panic!("no row {id}: {r:?}")).what.clone()
}

/// Wait, without a sleep, for what another thread lands; a test that fails says what it waited for.
fn until(what: &str, ok: impl Fn() -> bool) {
    let end = Instant::now() + Duration::from_secs(30);
    while !ok() {
        assert!(Instant::now() < end, "{what} never came");
        std::thread::yield_now();
    }
}

/// The base pack's pages and views, and a claude connector with two models, which Library › Models lists.
fn screen() -> Screen {
    let files = [
        ("pages/main.toml", include_str!("../../../../packs/base/pages/main.toml")),
        ("views/main.toml", include_str!("../../../../packs/base/views/main.toml")),
        ("commands/main.toml", include_str!("../../../../packs/base/commands/main.toml")),
        ("connectors/claude.toml", "api = 1\n[[connector]]\nid = \"claude\"\nis = \"agent\"\ndialect = \"claude-stream\"\nmodels = [\"haiku\", \"sonnet\"]\n"),
    ];
    let files = files.into_iter().map(|(p, t)| SourceFile { path: p.into(), text: t.into() }).collect();
    let (tables, _) = Tables::load(vec![Layer::new(LayerKind::Pack, "base", files)]);
    let (settings, _) = Settings::load(&tables);
    Screen::load(tables, &settings, Vec::new(), &BTreeMap::new()).0
}

/// The cell `at` of the row whose first cell is `first`, in the last rows of `bind` that `downs` hold.
fn cell(downs: &[Down], bind: &str, first: &str, at: usize) -> Option<String> {
    let rows = downs.iter().rev().find_map(|d| match d {
        Down::Rows { bind: b, rows, .. } if b == bind => Some(rows),
        _ => None,
    })?;
    rows.iter().find(|r| r.cells.first().is_some_and(|c| c.text == first)).and_then(|r| r.cells.get(at)).map(|c| c.text.clone())
}

#[test]
fn setup_and_library_answer_at_once_while_a_probe_is_out_and_the_next_tick_draws_each_answer_as_it_lands() {
    let r = rig(None);
    let f = fx_screened(Vec::new(), screen(), |c| c.setup = Some(r.check.clone()));
    let mut s = Serve::new(f.front.clone());
    // both probes wait at their doors: the pages are drawn anyway, and say that the programs are being checked
    let (downs, _) = s.up(Up::Watch { bind: "view:setup".into() });
    assert_eq!(cell(&downs, "view:setup", "claude", 1).as_deref(), Some(CHECKING), "{downs:?}");
    assert_eq!(cell(&downs, "view:setup", "codex", 1).as_deref(), Some(CHECKING));
    assert_eq!(cell(&downs, "view:setup", "default doer", 1).as_deref(), Some(CHECKING), "no default is said before an answer, and never `none works`");
    let (downs, _) = s.up(Up::Watch { bind: "view:setup-flow".into() });
    assert_eq!(cell(&downs, "view:setup-flow", "Agent", 1).as_deref(), Some(CHECKING), "the Agent step waits too: {downs:?}");
    let (downs, _) = s.up(Up::Watch { bind: "view:library".into() });
    assert_eq!(cell(&downs, "view:library", "sonnet", 2).as_deref(), Some(CHECKING), "a model of a program not checked yet is neither usable nor missing: {downs:?}");
    // claude answers: its row is drawn at the next tick, while codex still waits
    r.claude.open();
    until("claude's answer", || what(&r.check.report_for(None), "claude") != CHECKING);
    let downs = s.tick();
    assert!(cell(&downs, "view:setup", "claude", 1).is_some_and(|w| w.starts_with("found · signed in")), "{downs:?}");
    assert_eq!(cell(&downs, "view:setup", "codex", 1).as_deref(), Some(CHECKING));
    assert_eq!(cell(&downs, "view:setup", "default doer", 1).as_deref(), Some("claude"), "every doer still out sorts after claude: the default is known");
    // codex answers: the look ends, and the next tick draws it
    r.codex.open();
    let now = r.check.report_now(None);
    assert!(!now.checking, "{now:?}");
    let downs = s.tick();
    assert!(cell(&downs, "view:setup", "codex", 1).is_some_and(|w| w.starts_with("found · signed in")), "{downs:?}");
    assert_eq!(cell(&downs, "view:setup", "gemini", 1).as_deref(), Some("not found"));
    assert_eq!(r.looks.load(Ordering::SeqCst), 1, "every report and every page was served by the one look");
}

#[test]
fn a_report_while_a_look_runs_starts_no_second_look_and_one_that_waits_waits_for_it() {
    let r = rig(None);
    for _ in 0..5 {
        let report = r.check.report_for(None);
        assert!(report.checking && report.default.is_none(), "{report:?}");
    }
    // the look that runs is the one a caller that acts waits for
    let waiter = {
        let check = r.check.clone();
        std::thread::spawn(move || check.report_now(None))
    };
    r.claude.open();
    r.codex.open();
    let now = waiter.join().unwrap();
    assert_eq!((now.checking, now.default.as_deref()), (false, Some("claude")));
    r.check.report_for(None);
    assert_eq!(r.looks.load(Ordering::SeqCst), 1, "one look, however many asked");
    assert_eq!(r.asked.lock().unwrap().iter().filter(|id| *id == "codex").count(), 1);
}

#[test]
fn the_facts_are_stamped_when_the_look_ends_so_a_long_look_is_not_stale_when_it_lands() {
    let clock = Arc::new(ManualClock::at(1_000_000));
    let r = rig(Some((clock.clone(), 0)));
    r.check.report_for(None);
    // the look takes three times as long as the facts stand
    clock.advance(3 * STALE_MS);
    r.claude.open();
    r.codex.open();
    r.check.report_now(None);
    r.check.report_for(None);
    r.check.report_now(None);
    assert_eq!(r.looks.load(Ordering::SeqCst), 1, "the facts of a long look are fresh when it ends");
    clock.advance(STALE_MS);
    r.check.report_now(None);
    assert_eq!(r.looks.load(Ordering::SeqCst), 2, "they stand for their time from the end, and no longer");
}

#[test]
fn a_program_not_there_keeps_its_answer_until_its_backoff_ends_or_r_asks_again() {
    let clock = Arc::new(ManualClock::at(1_000_000));
    // codex is slow (its probe takes the whole time the facts stand) and is found: slow alone is no reason to keep an answer
    let r = rig(Some((clock.clone(), STALE_MS)));
    r.claude.open();
    r.codex.open();
    let first = r.check.report_now(None);
    assert_eq!(what(&first, "gemini"), "not found");
    let times = |id: &str| r.asked.lock().unwrap().iter().filter(|a| *a == id).count();
    // stale: the doers are asked again, but gemini was not there, so its answer stands
    clock.advance(STALE_MS);
    let second = r.check.report_now(None);
    assert_eq!(r.looks.load(Ordering::SeqCst), 2);
    assert_eq!((times("claude"), times("codex"), times("gemini")), (2, 2, 1), "{:?}", r.asked.lock().unwrap());
    assert_eq!(what(&second, "gemini"), "not found");
    // Setup's `r` asks every program again at once
    r.check.again();
    r.check.report_now(None);
    assert_eq!(times("gemini"), 2, "`r` asks again a program that was not there");
    // and the backoff ends by itself
    clock.advance(BACKOFF_MS);
    r.check.report_now(None);
    assert_eq!(times("gemini"), 3);
}

#[test]
fn a_look_whose_frame_fails_ends_says_checking_and_is_not_started_again_at_every_draw() {
    let looks = Arc::new(AtomicUsize::new(0));
    let check = {
        let looks = looks.clone();
        Check::new(
            move || {
                looks.fetch_add(1, Ordering::SeqCst);
                panic!("the frame of this case fails")
            },
            |id: &str| DoerFact { id: id.into(), ..DoerFact::default() },
        )
    };
    let now = check.report_now(None);
    assert!(now.checking && now.default.is_none(), "{now:?}");
    for _ in 0..5 {
        check.report_for(None);
    }
    assert_eq!(looks.load(Ordering::SeqCst), 1, "a look that found nothing stands for its time like one that did");
}
