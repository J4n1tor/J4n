//! `/usage`. The bar math from fixed `run.window` events (used, reset, pace, how it ends), two accounts on two
//! plans, no data yet, a gateway's spend today, and the view as a row of the chat that is sent again when a new window reading arrives.

use crate::front_cases::fx;
use crate::scripted::{Beat, Scripted, base_tables, end, say};
use j4n_core::change::Actor;
use j4n_core::clock::Clock;
use j4n_core::cost::Plans;
use j4n_core::front::Said;
use j4n_core::front::serve::Serve;
use j4n_core::front::usage::{End, Reading, Usage, bar, hm, plain, view, view_with};
use j4n_core::log::{Ev, New};
use j4n_core::schema::DataClass;
use j4n_proto::{Down, Intent, Up};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const H: u64 = 3_600_000;
/// A fixed time: the 5-hour window began at `T0` and resets 5 hours later.
const T0: u64 = 1_800_000_000_000;

fn reading(util: f64, at: u64) -> Reading {
    Reading { util, resets_s: ((T0 + 5 * H) / 1000) as i64, at_ms: at }
}

#[test]
fn the_bar_math_is_use_over_the_time_since_the_window_began() {
    // half used two hours in: 25 % an hour, the other half takes two more hours, which is before the reset at five
    let b = bar(&reading(0.5, T0 + 2 * H), 5 * H, T0 + 2 * H);
    assert_eq!((b.used, b.reset_in, b.end), (0.5, Some(3 * H), End::In(2 * H)));
    assert!((b.per_hour.unwrap() - 0.25).abs() < 1e-9, "{b:?}");
    // a fifth used two hours in: 10 % an hour, ten hours to the end, which is past the reset
    let b = bar(&reading(0.2, T0 + 2 * H), 5 * H, T0 + 2 * H);
    assert_eq!((b.end, b.reset_in), (End::PastReset, Some(3 * H)));
    // the pace is from the reading: an hour later with nothing run, the end is an hour nearer
    let b = bar(&reading(0.5, T0 + 2 * H), 5 * H, T0 + 3 * H);
    assert_eq!((b.end, b.reset_in), (End::In(H), Some(2 * H)));
    // nothing used, all used, a window too young for a pace, a window that has reset since the reading
    assert_eq!(bar(&reading(0.0, T0 + H), 5 * H, T0 + H).end, End::Idle);
    assert_eq!(bar(&reading(1.0, T0 + 4 * H), 5 * H, T0 + 4 * H).end, End::Full);
    let young = bar(&reading(0.04, T0 + 3 * 60_000), 5 * H, T0 + 3 * 60_000);
    assert_eq!((young.end, young.per_hour), (End::Unknown, None));
    let gone = bar(&reading(0.9, T0 + 4 * H), 5 * H, T0 + 6 * H);
    assert_eq!((gone.used, gone.reset_in, gone.end), (0.0, None, End::Idle), "a reading from before the reset says nothing of the window now");
    assert_eq!((hm(100 * 60_000), hm(59 * 60_000), hm(50 * H)), ("1:40".into(), "0:59".into(), "2d 2h".into()));
}

fn window(doer: &str, account: &str, name: &str, util: f64, resets_s: u64, at: u64) -> Ev {
    let mut data = json!({ "window": name, "utilization": util, "resets_at": resets_s, "status": "allowed" });
    if !account.is_empty() {
        data["account"] = json!(account);
    }
    Ev { seq: 0, at_ms: at, kind: "run.window".into(), task: None, thread: None, by: Actor::Doer(doer.into()), cause: None, class: DataClass::default(), data }
}

fn spent(doer: &str, usd: f64, at: u64) -> Ev {
    Ev { seq: 0, at_ms: at, kind: "run.metrics".into(), task: None, thread: None, by: Actor::Doer(doer.into()), cause: None, class: DataClass::default(), data: json!({ "usd": usd }) }
}

fn plans() -> Plans {
    let (mut plans, problems) = Plans::load(&base_tables(""));
    assert!(problems.is_empty(), "{problems:?}");
    assert!(plans.name([("work".to_string(), "max5".to_string()), ("side".to_string(), "pro".to_string())]).is_empty());
    plans
}

#[test]
fn two_accounts_on_two_plans_and_the_default_login_each_show_their_own_windows() {
    let mut u = Usage::default();
    let (five, seven) = ((T0 + 5 * H) / 1000, (T0 + 4 * 24 * H) / 1000);
    // the operator's own login: half of the 5-hour window two hours in, 10 % of the week three days in
    u.count(&window("claude", "", "five_hour", 0.5, five, T0 + 2 * H));
    u.count(&window("claude", "", "seven_day", 0.1, seven, T0 + 2 * H));
    // the Max5 account is nearly out, the Pro one has not begun
    u.count(&window("claude", "work", "five_hour", 0.9, five, T0 + 2 * H));
    u.count(&window("claude", "side", "five_hour", 0.0, five, T0 + 2 * H));
    let v = view(&u, &plans(), &[], T0 + 2 * H);
    let text = plain(&v);
    let lines: Vec<&str> = text.lines().collect();
    let at = |needle: &str| lines.iter().position(|l| l.starts_with(needle)).unwrap_or_else(|| panic!("{needle}: {text}"));
    assert!(at("claude · default") < at("claude · side · pro $20/mo") && at("claude · side · pro $20/mo") < at("claude · work · max5 $100/mo"), "{text}");
    let default = &lines[at("claude · default") + 1..at("claude · side · pro $20/mo")];
    assert_eq!(default.len(), 2, "{text}");
    assert!(default[0].contains("5h") && default[0].contains("50%") && default[0].contains("resets in 3:00 (13:00 UTC)") && default[0].contains("25%/h") && default[0].contains("at this pace ends in 2:00"), "{text}");
    assert!(default[1].starts_with("  7d") && default[1].contains("10%") && default[1].contains("resets in 3d 22h") && default[1].contains("3%/day") && default[1].contains("lasts past reset"), "{text}");
    let work = lines[at("claude · work · max5 $100/mo") + 1];
    assert!(work.contains("90%") && work.contains("45%/h") && work.contains("at this pace ends in 0:13"), "{text}");
    assert!(lines[at("claude · side · pro $20/mo") + 1].contains("unused"), "{text}");
    // the entries the screen draws: the pace that ends before the reset is warned, a bar of a plan that is not known has no pace
    let entries = v.as_array().unwrap();
    assert!(entries.iter().any(|e| e["label"] == "5h" && e["tone"] == "warn" && e["f"] == 0.9), "{v}");
}

/// An entry says when its window resets (`resets_ms`) and the words it made of the time left (`left`), so the screen counts down by itself; the plain
/// text still has the words of that moment.
#[test]
fn a_window_entry_says_when_it_resets_for_the_screen_to_count_down() {
    let mut u = Usage::default();
    let five = (T0 + 5 * H) / 1000;
    u.count(&window("claude", "", "five_hour", 0.5, five, T0 + 2 * H));
    let v = view(&u, &plans(), &[], T0 + 2 * H);
    let bar = v.as_array().unwrap().iter().find(|e| e["label"] == "5h").expect("the 5h bar").clone();
    assert_eq!(bar["resets_ms"], five * 1000, "{bar}");
    assert_eq!(bar["left"], "3:00", "{bar}");
    assert!(bar["note"].as_str().unwrap().contains("resets in 3:00"), "{bar}");
}

/// Every login of a doer is a group; one with no window yet says so, whatever another has shown, and the operator's own login is one.
#[test]
fn an_account_with_no_window_yet_is_a_row_beside_one_that_has_one() {
    let mut u = Usage::default();
    let five = (T0 + 5 * H) / 1000;
    u.count(&window("claude", "work", "five_hour", 0.9, five, T0 + 2 * H));
    let accounts = vec![("claude".to_string(), "work".to_string()), ("claude".to_string(), "side".to_string())];
    let text = plain(&view_with(&u, &plans(), &[], &[], &accounts, &BTreeMap::new(), T0 + 2 * H));
    assert!(text.contains("claude · side · pro $20/mo\n  needs sign-in · no runs yet"), "{text}");
    assert!(text.contains("claude · default\n  no runs yet\n"), "{text}");
    assert!(text.contains("claude · work · max5 $100/mo\n  5h") && text.contains("90%"), "{text}");
    // with no account but the operator's own the view is what it was
    let alone = plain(&view_with(&Usage::default(), &plans(), &[], &[], &[], &BTreeMap::new(), T0));
    assert!(alone.contains("claude\n  no runs yet") && !alone.contains("default"), "{alone}");
}

#[test]
fn before_any_window_is_seen_it_says_when_it_will_be() {
    let text = plain(&view(&Usage::default(), &plans(), &[], T0));
    assert!(text.contains("claude\n  no runs yet"), "{text}");
}

#[test]
fn a_gateway_says_it_does_not_know_its_credits_and_what_it_spent_today() {
    let mut u = Usage::default();
    for (usd, at) in [(0.25, T0 + H), (0.06, T0 + 2 * H), (0.5, T0 - 24 * H)] {
        u.count(&spent("own", usd, at));
    }
    u.count(&spent("claude", 9.0, T0 + H));
    let gateways = vec![("own".to_string(), "openrouter".to_string())];
    let text = plain(&view(&u, &plans(), &gateways, T0 + 3 * H));
    assert!(text.contains("openrouter\n  credits: unknown") && text.contains("today $0.31 · 2 calls"), "{text}");
    let none = plain(&view(&Usage::default(), &plans(), &gateways, T0));
    assert!(none.contains("today: nothing spent"), "{none}");
}

// ---- the row in the chat ----

fn chat(downs: &[Down]) -> Vec<j4n_proto::Row> {
    downs.iter().rev().find_map(|d| match d {
        Down::Rows { bind, rows, .. } if bind == "chat:main" => Some(rows.clone()),
        _ => None,
    }).unwrap_or_default()
}

fn held(rows: &[j4n_proto::Row]) -> Vec<Value> {
    rows.iter().filter_map(|r| r.data.as_ref()?.get("view").cloned()).collect()
}

#[test]
fn usage_is_a_row_of_the_chat_that_holds_the_view_and_is_drawn_again_when_a_window_is_read() {
    let f = fx(vec![Scripted::new("alpha", vec![vec![Beat::Session("s".into()), say("a"), end("a")]])], |c| c.gateways = vec![("alpha".into(), "openrouter".into())]);
    let mut s = Serve::new(f.front.clone());
    s.up(Up::Hello { api: j4n_proto::API });
    let (downs, _) = s.up(Up::Watch { bind: "chat:main".into() });
    assert!(held(&chat(&downs)).is_empty());
    // the command is answered at once, and a task is not started for it
    let before = f.rig.tasks.book().rows().len();
    let (downs, _) = s.up(Up::Intent { intent: Intent::Say { text: "/usage".into(), with: Vec::new() } });
    assert!(downs.iter().all(|d| !matches!(d, Down::Toast { .. })), "{downs:?}");
    assert_eq!(f.rig.tasks.book().rows().len(), before, "no task was started");
    let log: Vec<Ev> = f.rig.log.read(0).collect();
    let made = log.iter().find(|e| e.kind == "chat.view").expect("the Log holds the row");
    assert_eq!((made.data["view"].as_str(), made.thread.as_deref()), (Some("usage"), Some("main")));
    assert!(made.data["text"].as_str().unwrap_or_default().contains("credits: unknown"), "{made:?}");
    let sent: Vec<Down> = log.iter().flat_map(|e| s.ev(e)).collect();
    let views = held(&chat(&sent));
    assert_eq!(views.len(), 1, "{sent:?}");
    assert_eq!((views[0]["id"].as_str(), views[0]["field"].as_str(), views[0]["title"].as_str()), (Some("usage"), Some("meter"), Some("usage")));
    assert!(!views[0]["value"].to_string().contains("\"f\""), "no window yet: {}", views[0]["value"]);
    // a window is read in another thread's run: the row is sent again with the bar in it
    let at = f.rig.clock.now_ms();
    f.rig.log.append(New::of("run.window", Actor::Doer("claude".into()), json!({ "window": "five_hour", "utilization": 0.3, "resets_at": (at / 1000 + 4 * 3600) as i64, "status": "allowed" })).task("t9").thread("other")).unwrap();
    let again: Vec<Down> = f.rig.log.read(made.seq + 1).flat_map(|e| s.ev(&e)).collect();
    let views = held(&chat(&again));
    assert_eq!(views.len(), 1, "{again:?}");
    assert!(views[0]["value"].as_array().unwrap().iter().any(|e| e["f"] == 0.3), "{}", views[0]["value"]);
    // and a client that comes back later is sent the row with what the Log holds
    let (replayed, _) = Serve::new(f.front.clone()).up(Up::Watch { bind: "chat:main".into() });
    assert!(held(&chat(&replayed)).first().is_some_and(|v| v["value"].to_string().contains("0.3")), "{replayed:?}");
    assert!(matches!(f.say(":usage"), Said::Command(c) if c == "usage"), "the colon word is the same command");
}
