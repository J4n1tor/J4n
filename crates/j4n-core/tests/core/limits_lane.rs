//! The lane for accounts, limits and routing. Tier 0, a doer that hits its limit on cue as the login it is run as.
//! The case: the default login is at its limit, a second account is signed in, and every new chat must not still run as the
//! default, find the limit again and wait for its reset; nor may `/usage` say of the new account to run something first.

use crate::front_cases::{Fx, fx_trusting};
use crate::scripted::{Beat, Scripted, base_tables, end, say};
use j4n_core::change::Actor;
use j4n_core::clock::Clock;
use j4n_core::failure::{Class, Failures};
use j4n_core::front::acct::{Candidate, Chosen, choose};
use j4n_core::front::serve::Serve;
use j4n_core::front::usage::{Usage, plain, view_with};
use j4n_core::front::Said;
use j4n_core::log::{Ev, New};
use j4n_core::schema::DataClass;
use j4n_core::task::{Row, State, TaskSpec};
use j4n_proto::Up;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;

const H: u64 = 3_600_000;
/// The clock of every case: a real day, so a limit that says `resets at <seconds>` is read as one.
const T0: u64 = 1_800_000_000_000;

fn secs(ms: u64) -> i64 {
    (ms / 1000) as i64
}

/// A run that is told not now: the window says `rejected`, and the run fails with the words the connector gives it.
fn shut(resets_ms: u64) -> Vec<Beat> {
    vec![Beat::Session("old".into()), Beat::Shut("five_hour", secs(resets_ms)), Beat::Fail(format!("rate limit: claude window rejected (resets at {})", secs(resets_ms)))]
}

/// A run that works, and says its window has room.
fn works(account: &str) -> Vec<Beat> {
    vec![Beat::Session(format!("{account}-session")), Beat::Limits(vec![("five_hour", 0.1, secs(T0 + 5 * H))]), say("done"), end("done")]
}

/// A run that is stopped by the limit after it did some work.
fn midway(resets_ms: u64) -> Vec<Beat> {
    vec![Beat::Session("old".into()), say("half way"), Beat::Changed(vec!["a.txt".into()]), Beat::Shut("five_hour", secs(resets_ms)), Beat::Fail(format!("rate limit: claude window rejected (resets at {})", secs(resets_ms)))]
}

struct Lane {
    f: Fx,
    claude: Arc<Scripted>,
}

/// claude as several logins: `rows` are the accounts of the project, `signed` the ones the Log says are signed in (each as a MAX account).
fn lane(logins: Vec<(&str, Vec<Vec<Beat>>)>, rows: &[&str], signed: &[&str]) -> Lane {
    let claude = Scripted::by_account("claude", logins);
    let rows: Vec<(String, String)> = rows.iter().map(|a| ("claude".to_string(), a.to_string())).collect();
    let f = fx_trusting(vec![claude.clone()], None, |c| {
        c.doer = Some("claude".into());
        c.accounts = Some(Arc::new(move || rows.clone()));
    });
    f.rig.clock.set(T0);
    let lane = Lane { f, claude };
    signed.iter().for_each(|a| {
        lane.sign_in(a);
    });
    lane
}

impl Lane {
    fn sign_in(&self, account: &str) -> u64 {
        self.f.rig.log.append(New::of("account.checked", Actor::Core, json!({ "account": account, "ok": true, "plan": "max", "state": "signed in", "email": "a@b.c" }))).unwrap().seq
    }

    /// What a run said earlier: the window of `account` (empty: the doer's own login) is shut until `resets_ms`.
    fn was_shut(&self, account: &str, resets_ms: u64) {
        let mut data = json!({ "window": "five_hour", "utilization": 1.0, "resets_at": secs(resets_ms), "status": "rejected" });
        if !account.is_empty() {
            data["account"] = json!(account);
        }
        self.f.rig.log.append(New::of("run.window", Actor::Doer("claude".into()), data).task("t90").thread("old")).unwrap();
    }

    /// The logins each run was started as, in order.
    fn ran_as(&self) -> Vec<Option<String>> {
        self.claude.starts().into_iter().map(|s| s.account).collect()
    }

    fn start(&self, line: &str) -> String {
        match self.f.front.say("main", line) {
            Said::Started { task } => task,
            other => panic!("{line:?} did not start a task: {other:?}\n{}", self.f.rig.dump()),
        }
    }

    fn cooled(&self, id: &str) -> Row {
        self.f.rig.until(id, "cooled", |r| matches!(r.state, State::Cooling { .. }))
    }

    fn done(&self, id: &str) -> Row {
        self.f.rig.until(id, "done", |r| matches!(r.state, State::Done { .. }))
    }

    fn notes(&self) -> String {
        self.f.notes().join("\n")
    }
}

fn some(a: &str) -> Option<String> {
    Some(a.to_string())
}

// ---- (a) (b) (d): where a new chat goes ----

/// The case: the default login is shut and a MAX account is signed in. A new chat goes to the account and does not run as the default.
#[test]
fn a_new_chat_runs_as_the_signed_in_account_while_the_default_login_is_shut() {
    let l = lane(vec![("", vec![shut(T0 + 2 * H)]), ("claude-1", vec![works("claude-1")])], &["claude-1"], &["claude-1"]);
    l.was_shut("", T0 + 2 * H);
    let id = l.start("add a --verbose flag to main.rs");
    let row = l.done(&id);
    assert_eq!(l.ran_as(), [some("claude-1")], "the default is not tried again: {}", l.f.rig.dump());
    assert_eq!(row.spec.account.as_deref(), Some("claude-1"));
    assert!(row.who().ends_with("· claude-1"), "the task row says the login: {}", row.who());
    let routed = l.f.rig.of_kind("account.routed");
    assert_eq!((routed.len(), routed[0].data["to"].as_str(), routed[0].data["from"].is_null()), (1, Some("claude-1"), true), "{routed:?}");
    assert!(routed[0].data["why"].as_str().unwrap().contains("default limited"), "{}", routed[0].data);
    assert!(l.notes().contains("claude · claude-1: default limited"), "{}", l.notes());
}

/// Nothing is shut as far as the Log knows: the default login is tried first, as it always was (no change for one who has accounts and no limit).
#[test]
fn nothing_is_changed_while_no_login_is_shut() {
    let l = lane(vec![("", vec![works("")]), ("claude-1", vec![works("claude-1")])], &["claude-1"], &["claude-1"]);
    let id = l.start("add a --verbose flag to main.rs");
    l.done(&id);
    assert_eq!(l.ran_as(), [None], "{}", l.f.rig.dump());
    assert!(l.f.rig.of_kind("account.routed").is_empty() && !l.notes().contains("limited"));
}

/// An account that is a row and was never signed in is not a login a turn can run as: it would fail on a login, which is worse than a limit.
#[test]
fn an_account_that_was_never_signed_in_is_not_chosen() {
    let l = lane(vec![("", vec![shut(T0 + 2 * H)]), ("claude-1", vec![works("claude-1")])], &["claude-1"], &[]);
    l.was_shut("", T0 + 2 * H);
    let id = l.start("add a --verbose flag to main.rs");
    l.cooled(&id);
    assert_eq!(l.ran_as(), [None], "{}", l.f.rig.dump());
    l.f.front.limits_sweep();
    assert_eq!(l.ran_as(), [None], "the sweep has nowhere to go either");
    // one signed out later is not one either
    l.f.rig.log.append(New::of("account.removed", Actor::Core, json!({ "account": "claude-1", "state": "removed" }))).unwrap();
    l.sign_in("claude-1");
    l.f.rig.log.append(New::of("account.signin", Actor::Core, json!({ "account": "claude-1", "ok": false, "reason": "timed out" }))).unwrap();
    l.f.front.limits_sweep();
    assert_eq!(l.ran_as(), [None], "{}", l.f.rig.dump());
}

/// Three logins: the default and the first account are shut, the third has room, and the one that is fullest is passed over.
#[test]
fn three_logins_the_open_one_with_the_most_room_takes_it() {
    let l = lane(vec![("", vec![works("")]), ("a", vec![works("a")]), ("b", vec![works("b")]), ("c", vec![works("c")])], &["a", "b", "c"], &["a", "b", "c"]);
    l.was_shut("", T0 + 2 * H);
    l.was_shut("a", T0 + H);
    l.f.rig.log.append(New::of("run.window", Actor::Doer("claude".into()), json!({ "account": "b", "window": "five_hour", "utilization": 0.8, "resets_at": secs(T0 + 4 * H), "status": "allowed" }))).unwrap();
    let id = l.start("add a --verbose flag to main.rs");
    l.done(&id);
    assert_eq!(l.ran_as(), [some("c")], "{}", l.f.rig.dump());
    assert!(l.notes().contains("default and a limited"), "{}", l.notes());
}

// ---- (c) (d): the limit is the login's, and a chat that waits is moved ----

/// Nothing says the default is shut: the first chat runs as it, finds the limit, and goes on as the open account at once instead of waiting for the
/// reset; the next chat does not run as the default at all.
#[test]
fn a_chat_that_finds_the_limit_goes_on_as_the_open_account_in_a_new_session() {
    let l = lane(vec![("", vec![shut(T0 + 2 * H)]), ("claude-1", vec![works("claude-1")])], &["claude-1"], &["claude-1"]);
    let id = l.start("add a --verbose flag to main.rs");
    l.cooled(&id);
    l.f.front.limits_sweep();
    let row = l.done(&id);
    assert_eq!(l.ran_as(), [None, some("claude-1")], "{}", l.f.rig.dump());
    assert_eq!(l.claude.starts()[1].resume, None, "the old session is the old login's");
    assert_eq!(row.spec.account.as_deref(), Some("claude-1"));
    let moved = l.f.rig.of_kind("task.moved");
    assert_eq!((moved.len(), moved[0].data["to"].as_str(), moved[0].data["from"].is_null()), (1, Some("claude-1"), true), "{}", l.f.rig.dump());
    assert!(moved[0].data["why"].as_str().unwrap().starts_with("default is limited, back in 2:00"), "{}", moved[0].data);
    assert!(l.notes().contains("claude · claude-1: default is limited, back in 2:00 · a new session"), "{}", l.notes());
    // the limit says whose it is: the error row names the login that said it, so the next turn knows
    let said = l.f.rig.of_kind("run.error");
    assert!(said[0].data.get("account").is_none(), "the default login says no account: {}", said[0].data);
    // and the next chat does not find it again
    let again = l.f.front.say("main", "and a --quiet flag too");
    assert!(matches!(again, Said::Started { .. }), "{again:?}");
    assert_eq!(l.ran_as(), [None, some("claude-1"), some("claude-1")], "stays where it works, in its session: {}", l.f.rig.dump());
    assert_eq!(l.claude.starts()[2].resume.as_deref(), Some("claude-1-session"), "the next turn carries on in the session of the login it runs as");
}

/// The limit is said by the account a chat runs as, and it is that account's: the doer's own login is not shut by it.
#[test]
fn a_limit_is_the_logins_not_the_doers() {
    let l = lane(vec![("", vec![works("")]), ("claude-1", vec![shut(T0 + H)])], &["claude-1"], &["claude-1"]);
    l.was_shut("", T0 + 2 * H);
    let id = l.start("add a --verbose flag to main.rs");
    l.cooled(&id);
    let said = l.f.rig.of_kind("run.error");
    assert_eq!(said[0].data["account"], "claude-1", "{}", said[0].data);
    let mut u = Usage::default();
    l.f.rig.log.read(0).for_each(|e| u.count(&e));
    let now = l.f.rig.clock.now_ms();
    assert_eq!((u.limited_until("claude", "claude-1", "", now).is_some(), u.limited_until("claude", "", "", now)), (true, Some(T0 + 2 * H)));
    assert_eq!(u.limited_until("claude", "other", "", now), None);
}

/// Mid-task: it did some work as the default and the limit stopped it. It goes on as the account from its goal (the folder keeps what it changed).
#[test]
fn a_limit_in_the_middle_of_a_task_moves_it_with_its_goal_and_no_old_session() {
    let l = lane(vec![("", vec![midway(T0 + 2 * H)]), ("claude-1", vec![works("claude-1")])], &["claude-1"], &["claude-1"]);
    let id = l.start("add a --verbose flag to main.rs");
    l.cooled(&id);
    l.f.front.limits_sweep();
    l.done(&id);
    let starts = l.claude.starts();
    assert_eq!((starts.len(), starts[1].account.as_deref(), starts[1].resume.clone()), (2, Some("claude-1"), None));
    assert!(starts[1].goal.contains("--verbose"), "{:?}", starts[1]);
    assert!(l.f.rig.of_kind("task.moved").len() == 1 && l.f.rig.of_kind("task.cooling").len() == 1);
}

/// A limit during the retry: the account it was moved to is shut too. Every login is shut now: it waits, for the earliest reset, and nothing runs
/// in a loop.
#[test]
fn a_limit_after_the_move_waits_for_the_earliest_reset_and_nothing_loops() {
    let l = lane(vec![("", vec![shut(T0 + 2 * H)]), ("claude-1", vec![shut(T0 + H)])], &["claude-1"], &["claude-1"]);
    let id = l.start("add a --verbose flag to main.rs");
    l.cooled(&id);
    l.f.front.limits_sweep();
    let row = l.until_cooled_twice(&id);
    for _ in 0..3 {
        l.f.front.limits_sweep();
    }
    assert_eq!(l.ran_as(), [None, some("claude-1")], "no third run: {}", l.f.rig.dump());
    let State::Cooling { until_ms } = row.state else { panic!("{row:?}") };
    assert!((T0 + H..T0 + H + 60_000).contains(&until_ms), "it waits for the account that is let in first, not the default's two hours: {until_ms}");
}

impl Lane {
    fn until_cooled_twice(&self, id: &str) -> Row {
        self.f.rig.until(id, "cooled again", |r| matches!(r.state, State::Cooling { .. }) && self.f.rig.of_kind("task.cooling").len() >= 2)
    }
}

/// Every login is shut as far as the Log knows, so a new chat is sent to the one that is let in first and waits there for that reset.
#[test]
fn with_every_login_shut_a_new_chat_waits_on_the_one_that_is_let_in_first() {
    let l = lane(vec![("", vec![shut(T0 + 3 * H)]), ("a", vec![shut(T0 + 2 * H)]), ("b", vec![shut(T0 + H)])], &["a", "b"], &["a", "b"]);
    l.was_shut("", T0 + 3 * H);
    l.was_shut("a", T0 + 2 * H);
    l.was_shut("b", T0 + H);
    let id = l.start("add a --verbose flag to main.rs");
    let row = l.cooled(&id);
    assert_eq!(l.ran_as(), [some("b")], "{}", l.f.rig.dump());
    let State::Cooling { until_ms } = row.state else { panic!() };
    assert!((T0 + H..T0 + H + 60_000).contains(&until_ms), "{until_ms}");
    assert!(l.notes().contains("every claude login is limited: the first is back in 1:00"), "{}", l.notes());
}

/// An account is signed in while chats wait (the same case, through the screen's own path): the events that say it take the chats that wait.
#[test]
fn a_sign_in_while_a_chat_waits_takes_the_chat_at_once() {
    let l = lane(vec![("", vec![shut(T0 + 2 * H)]), ("claude-1", vec![works("claude-1")])], &["claude-1"], &[]);
    let mut s = Serve::new(l.f.front.clone());
    s.up(Up::Hello { api: j4n_proto::API });
    let id = l.start("add a --verbose flag to main.rs");
    l.cooled(&id);
    for e in l.f.rig.log.read(0).collect::<Vec<_>>() {
        s.ev(&e);
    }
    assert_eq!(l.ran_as(), [None], "not signed in yet: nothing to move to");
    l.sign_in("claude-1");
    for e in l.f.rig.log.read(0).filter(|e| e.kind == "account.checked").collect::<Vec<Ev>>() {
        s.ev(&e);
    }
    let row = l.done(&id);
    assert_eq!((row.spec.account.as_deref(), l.ran_as()), (Some("claude-1"), vec![None, some("claude-1")]), "{}", l.f.rig.dump());
}

/// A reset passes: what was shut is open again, and a chat that works as an account stays there while it is open (its session is there).
#[test]
fn a_reset_opens_the_login_again() {
    let mut u = Usage::default();
    fn w(u: &mut Usage, status: &str, at: u64, seq: u64, resets: u64) {
        let ev = Ev { seq, at_ms: at, kind: "run.window".into(), task: None, thread: None, by: Actor::Doer("claude".into()), cause: None, class: DataClass::default(), data: json!({ "window": "five_hour", "utilization": 1.0, "resets_at": secs(resets), "status": status }) };
        u.count(&ev);
    }
    w(&mut u, "rejected", T0, 1, T0 + 2 * H);
    assert_eq!((u.limited_until("claude", "", "", T0 + H), u.limited_until("claude", "", "", T0 + 2 * H)), (Some(T0 + 2 * H), None), "the reset time passes");
    w(&mut u, "allowed", T0 + 3 * H, 2, T0 + 7 * H);
    w(&mut u, "rejected", T0 + 3 * H, 3, T0 + 8 * H);
    assert_eq!(u.limited_until("claude", "", "", T0 + 4 * H), Some(T0 + 8 * H));
    w(&mut u, "allowed", T0 + 4 * H, 4, T0 + 9 * H);
    assert_eq!(u.limited_until("claude", "", "", T0 + 4 * H), None, "a window that says allowed again is open before its time");
}

/// The words of a limit, with the time it lasts, shut the login until then, and a reading that comes after takes it back.
#[test]
fn a_limit_said_in_words_shuts_the_login_until_a_later_reading_says_otherwise() {
    let mut u = Usage::default();
    let ev = |seq: u64, kind: &str, by: &str, data: serde_json::Value| Ev { seq, at_ms: T0 + seq, kind: kind.into(), task: None, thread: None, by: Actor::Doer(by.into()), cause: None, class: DataClass::default(), data };
    u.count(&ev(1, "run.error", "claude", json!({ "class": "rate-limit", "row": "limit", "wait_ms": 2 * H, "account": "work" })));
    assert_eq!((u.limited_until("claude", "work", "", T0 + H), u.limited_until("claude", "", "", T0 + H)), (Some(T0 + 1 + 2 * H), None));
    // the provider being busy is not the login being out
    u.count(&ev(2, "run.error", "claude", json!({ "class": "rate-limit", "row": "overloaded", "wait_ms": 2 * 60_000 })));
    assert_eq!(u.limited_until("claude", "", "", T0 + 3), None);
    u.count(&ev(3, "run.window", "claude", json!({ "account": "work", "window": "five_hour", "utilization": 0.1, "resets_at": secs(T0 + 5 * H), "status": "allowed" })));
    assert_eq!(u.limited_until("claude", "work", "", T0 + H), None, "it worked again");
}

// ---- the pin ----

/// The operator's pin is theirs: a pinned chat stays on its account when that is shut, says it waits because of the pin, and is not moved.
#[test]
fn a_pinned_account_is_kept_and_the_chat_says_it_waits_because_of_the_pin() {
    let l = lane(vec![("", vec![works("")]), ("claude-1", vec![shut(T0 + H)]), ("claude-2", vec![works("claude-2")])], &["claude-1", "claude-2"], &["claude-1", "claude-2"]);
    l.was_shut("claude-1", T0 + H);
    let said = l.f.say(":switch claude account=claude-1");
    assert!(matches!(said, Said::Command(_)), "{said:?}\n{}", l.notes());
    let id = l.start("add a --verbose flag to main.rs");
    l.cooled(&id);
    l.f.front.limits_sweep();
    assert_eq!(l.ran_as(), [some("claude-1")], "never moved off the pin: {}", l.f.rig.dump());
    assert!(l.notes().contains("is pinned to claude-1, which is limited, back in 1:00: it waits"), "{}", l.notes());
    assert!(l.f.rig.of_kind("account.routed").is_empty() && l.f.rig.of_kind("task.moved").is_empty());
}

// ---- the pick, pure ----

fn cand(account: &str, shut: Option<u64>, load: f64) -> Candidate {
    Candidate { account: account.into(), shut, load }
}

#[test]
fn the_pick_stays_on_an_open_login_else_the_doers_own_else_the_most_room_else_the_first_to_reopen() {
    let all = [cand("", None, 0.6), cand("a", None, 0.5), cand("b", None, 0.1)];
    assert_eq!(choose(&all, None), Some(Chosen::Open(String::new())), "the doer's own login first, as it always was");
    assert_eq!(choose(&all, Some("a")), Some(Chosen::Open("a".into())), "where the chat works, while it is open");
    let some_shut = [cand("", Some(9), 0.6), cand("a", None, 0.5), cand("b", None, 0.1)];
    assert_eq!(choose(&some_shut, Some("")), Some(Chosen::Open("b".into())), "its own is shut: the one with the most room");
    let none_open = [cand("", Some(30), 1.0), cand("a", Some(10), 1.0), cand("b", Some(20), 1.0)];
    assert_eq!(choose(&none_open, None), Some(Chosen::Shut("a".into(), 10)));
    assert_eq!(choose(&[], None), None);
}

// ---- (e) `/usage` ----

fn usage_of(events: &[(&str, serde_json::Value)], accounts: &[&str]) -> String {
    let mut u = Usage::default();
    for (n, (kind, data)) in events.iter().enumerate() {
        u.count(&Ev { seq: n as u64 + 1, at_ms: T0, kind: (*kind).into(), task: None, thread: None, by: Actor::Core, cause: None, class: DataClass::default(), data: data.clone() });
    }
    let (plans, problems) = j4n_core::cost::Plans::load(&base_tables(""));
    assert!(problems.is_empty(), "{problems:?}");
    let accounts: Vec<(String, String)> = accounts.iter().map(|a| ("claude".to_string(), a.to_string())).collect();
    plain(&view_with(&u, &plans, &[], &[], &accounts, &BTreeMap::new(), T0))
}

/// The operator's `/usage`: an account that has not run is a signed-in account with its plan and no runs, and nothing is told to run first.
#[test]
fn usage_shows_a_signed_in_account_with_its_plan_and_no_runs_yet() {
    let text = usage_of(&[("account.checked", json!({ "account": "claude-1", "ok": true, "plan": "max", "state": "signed in" }))], &["claude-1"]);
    assert!(text.contains("claude · claude-1 · max\n  signed in · no reading yet"), "{text}");
    assert!(!text.contains("it shows after"), "{text}");
}

#[test]
fn a_later_sign_in_without_a_plan_keeps_the_plan_and_a_failed_one_says_it_needs_a_sign_in() {
    let text = usage_of(&[("account.checked", json!({ "account": "w", "ok": true, "plan": "pro" })), ("account.signin", json!({ "account": "w", "ok": true })), ("account.signin", json!({ "account": "x", "ok": false, "reason": "timed out" }))], &["w", "x", "y"]);
    assert!(text.contains("claude · w · pro\n  signed in · no reading yet"), "{text}");
    assert!(text.contains("claude · x\n  needs sign-in · no reading yet") && text.contains("claude · y\n  no reading yet · the next task"), "{text}");
}

/// A shut login says so in its title.
#[test]
fn usage_says_which_login_is_limited_and_for_how_long() {
    let mut u = Usage::default();
    let ev = Ev { seq: 1, at_ms: T0, kind: "run.window".into(), task: None, thread: None, by: Actor::Doer("claude".into()), cause: None, class: DataClass::default(), data: json!({ "window": "five_hour", "utilization": 1.0, "resets_at": secs(T0 + 75 * 60_000), "status": "rejected" }) };
    u.count(&ev);
    let (plans, _) = j4n_core::cost::Plans::load(&base_tables(""));
    let text = plain(&view_with(&u, &plans, &[], &[], &[], &BTreeMap::new(), T0));
    assert!(text.contains("claude · limited, back in 1:15"), "{text}");
}

// ---- the words of a limit, in each form claude prints ----

/// Every form a limit comes in is a rate limit (it is waited out, never asked about or retried into); the one that names its time gives the wait.
#[test]
fn every_form_of_a_limit_text_is_a_rate_limit_and_one_that_names_its_time_says_how_long() {
    let (failures, problems) = Failures::load(&base_tables(""));
    assert!(problems.is_empty(), "{problems:?}");
    let forms = [
        "rate limit: claude window rejected (resets at 1800007200)",
        "rate limit: claude window blocked",
        "Claude AI usage limit reached|1800007200",
        "Claude usage limit reached. Your limit will reset at 3pm (UTC).",
        "5-hour limit reached ∙ resets 3pm",
        "Weekly limit reached ∙ resets Nov 20, 1pm",
        "Session limit reached ∙ resets 5pm",
        "You've hit your limit · resets 8pm (UTC)",
        "API Error: 429 {\"type\":\"error\",\"error\":{\"type\":\"rate_limit_error\",\"message\":\"This request would exceed your account's rate limit.\"}}",
        "Your organization has reached its monthly usage limit",
        "You've hit your session limit",
        "Opus weekly limit reached ∙ resets Nov 20, 1pm",
    ];
    for text in forms {
        let seen = failures.classify(text, T0);
        assert_eq!(seen.class, Class::RateLimit, "{text}: {seen:?}");
        assert!(seen.wait_ms.is_some(), "{text}: a limit is waited for a time: {seen:?}");
    }
    let named = failures.classify("rate limit: claude window rejected (resets at 1800007200)", T0);
    assert_eq!(named.wait_ms, Some(7_200_000 + 2_000), "the time it names, and a little more");
    let words = failures.classify("5-hour limit reached ∙ resets 3pm", T0);
    assert_eq!(words.wait_ms, Some(30 * 60_000), "a time in words, not in seconds, is the row's wait, tried again after it");
}


// ---- the main chat's own roles: the planner, the reviewer, a branch, a chore, a light answer ----

/// A window of one family shuts that family only (`seven_day_opus`): the planner on Opus goes to the account while the doers on Sonnet stay where they
/// are, and the other way round. A window of the whole login shuts both.
fn family(l: &Lane, window: &str, account: &str) {
    let mut data = json!({ "window": window, "utilization": 1.0, "resets_at": secs(T0 + 2 * H), "status": "rejected" });
    if !account.is_empty() {
        data["account"] = json!(account);
    }
    l.f.rig.log.append(New::of("run.window", Actor::Doer("claude".into()), data).task("t91").thread("old")).unwrap();
}

/// A task of a role of the chat's, as the front starts it: the role and the model it asked for, and the login it ends up with.
fn login_of(l: &Lane, role: &str, model: &str) -> Option<String> {
    let mut spec = TaskSpec::new("a role of the chat", "claude");
    spec.thread = Some("main".into());
    (spec.role, spec.tune.model) = (Some(role.into()), Some(model.into()));
    // as every start of the front takes it: the binding's account, which only a pin sets
    spec.account = l.f.front.binding("main").and_then(|b| b.account);
    l.f.front.login_for("main", &mut spec);
    spec.account
}

fn started(role_lane: &Lane) {
    let id = role_lane.start("add a --verbose flag to main.rs");
    role_lane.done(&id);
}

#[test]
fn the_planner_on_opus_is_limited_while_the_doers_on_sonnet_are_fine() {
    let l = lane(vec![("", vec![works("")]), ("claude-1", vec![works("claude-1")])], &["claude-1"], &["claude-1"]);
    started(&l);
    family(&l, "seven_day_opus", "");
    assert_eq!(login_of(&l, "plan", "opus-x"), some("claude-1"), "the planner moves by itself: {}", l.notes());
    assert_eq!(login_of(&l, "review", "opus-x"), some("claude-1"));
    assert_eq!(login_of(&l, "work", "sonnet-x"), None, "the doers stay on the login that has room for them");
    assert!(l.f.rig.of_kind("account.routed").iter().any(|e| e.data["role"] == "plan"), "{}", l.f.rig.dump());
}

#[test]
fn the_doers_on_sonnet_are_limited_while_the_planner_on_opus_is_fine() {
    let l = lane(vec![("", vec![works("")]), ("claude-1", vec![works("claude-1")])], &["claude-1"], &["claude-1"]);
    started(&l);
    family(&l, "seven_day_sonnet", "");
    assert_eq!((login_of(&l, "work", "sonnet-x"), login_of(&l, "chore", "sonnet-x")), (some("claude-1"), some("claude-1")));
    assert_eq!(login_of(&l, "plan", "opus-x"), None);
}

#[test]
fn when_the_whole_login_is_shut_every_role_goes_to_the_account() {
    let l = lane(vec![("", vec![works("")]), ("claude-1", vec![works("claude-1")])], &["claude-1"], &["claude-1"]);
    started(&l);
    l.was_shut("", T0 + 2 * H);
    for (role, model) in [("plan", "opus-x"), ("review", "opus-x"), ("work", "sonnet-x"), ("light", "haiku"), ("chore", "haiku"), ("piece", "sonnet-x")] {
        assert_eq!(login_of(&l, role, model), some("claude-1"), "{role}: {}", l.notes());
    }
}

#[test]
fn a_pin_holds_for_every_role_and_is_said_as_the_reason_to_wait() {
    let l = lane(vec![("", vec![works("")]), ("claude-1", vec![works("claude-1")])], &["claude-1"], &["claude-1"]);
    l.f.say(":switch claude account=claude-1");
    l.was_shut("claude-1", T0 + H);
    assert_eq!((login_of(&l, "plan", "opus-x"), login_of(&l, "work", "sonnet-x")), (some("claude-1"), some("claude-1")));
    assert!(l.notes().contains("pinned to claude-1"), "{}", l.notes());
}

/// An account is signed in while the main chat's turn and its planner both wait: the sign-in takes both.
#[test]
fn a_sign_in_while_the_turn_and_the_planner_wait_takes_both() {
    let l = lane(vec![("", vec![shut(T0 + 2 * H), shut(T0 + 2 * H)]), ("claude-1", vec![works("claude-1"), works("claude-1")])], &["claude-1"], &[]);
    let work = l.start("add a --verbose flag to main.rs");
    l.cooled(&work);
    let mut spec = TaskSpec::new("plan it", "claude");
    (spec.thread, spec.role) = (Some("main".into()), Some("plan".into()));
    let plan = l.f.rig.tasks.spawn(spec, Actor::Core).unwrap();
    l.cooled(&plan);
    l.f.front.limits_sweep();
    assert_eq!(l.ran_as(), [None, None], "nothing signed in: both wait");
    l.sign_in("claude-1");
    l.f.front.limits_sweep();
    let (a, b) = (l.done(&work), l.done(&plan));
    assert_eq!((a.spec.account.as_deref(), b.spec.account.as_deref()), (Some("claude-1"), Some("claude-1")), "{}", l.f.rig.dump());
}

/// A window of one family is not a limit of the models it does not name, and one of the whole login is a limit of all.
#[test]
fn a_limit_of_a_login_is_not_a_limit_of_a_family_it_does_not_name() {
    let mut u = Usage::default();
    let ev = |seq: u64, window: &str| Ev { seq, at_ms: T0, kind: "run.window".into(), task: None, thread: None, by: Actor::Doer("claude".into()), cause: None, class: DataClass::default(), data: json!({ "window": window, "utilization": 1.0, "resets_at": secs(T0 + H), "status": "rejected" }) };
    u.count(&ev(1, "seven_day_opus"));
    assert_eq!((u.limited_until("claude", "", "claude-opus-4", T0), u.limited_until("claude", "", "sonnet", T0), u.limited_until("claude", "", "", T0)), (Some(T0 + H), None, None));
    u.count(&ev(2, "five_hour"));
    assert_eq!(u.limited_until("claude", "", "sonnet", T0), Some(T0 + H), "the whole login");
}

/// A run with no screen asks for `/usage` twice, and the second holds what a run read in between (the screen's event path is not there to count it).
#[test]
fn a_second_usage_holds_what_was_read_since_the_first() {
    let l = lane(vec![("", vec![works("")])], &["claude-1"], &["claude-1"]);
    let first = l.f.front.view_value("usage").expect("the view").to_string();
    assert!(!first.contains("\"f\""), "no window yet: {first}");
    let data = json!({ "window": "five_hour", "utilization": 0.3, "resets_at": secs(T0 + 4 * H), "status": "allowed" });
    l.f.rig.log.append(New::of("run.window", Actor::Doer("claude".into()), data).task("t9").thread("other")).unwrap();
    let second = l.f.front.view_value("usage").expect("the view").to_string();
    assert!(second.contains("0.3"), "{second}");
}
