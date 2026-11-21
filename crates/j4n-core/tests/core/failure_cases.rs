//! Failure words (the kind `failure`): what a run's last words say, from the base pack's table and from tables of the
//! operator's own; the classic failure cases first, then data, waits and refusals.

use crate::scripted::base_tables;
use crate::world::*;
use j4n_core::failure::{Class, Failures};

fn base() -> Failures {
    let (f, problems) = Failures::load(&base_tables(""));
    assert!(problems.is_empty(), "{problems:?}");
    f
}

fn class(message: &str) -> Class {
    base().classify(message, 0).class
}

#[test]
fn a_run_that_ended_on_a_refused_or_failed_tool_is_neither_transient_nor_a_limit() {
    let refused = "the run ended on a tool that failed: The user rejected permission to use this specific tool call. · permission requested: external_directory (/tmp/opencode/*); auto-rejecting";
    assert_eq!(class(refused), Class::Refused);
    assert_eq!(class("the run ended on a tool that failed: The user has specified a rule which prevents you from using this specific tool call"), Class::Refused);
    // a tool that errored or was aborted is a class of its own, and words in its text that read as a limit or a missing file do not change that
    assert_eq!(class("the run ended on a tool that failed: Tool execution aborted"), Class::Tool);
    assert_eq!(class("the run ended on a tool that failed: HTTP 429 from the page it fetched, no such file"), Class::Tool);
    assert_eq!(Class::Refused.word(), "refused");
    assert_eq!(Class::Tool.word(), "tool");
    // a failure with nothing of this is as it was
    assert_eq!(class("connection reset by peer"), Class::Transient);
}

#[test]
fn classes_follow_the_words() {
    assert_eq!(class("HTTP 429 Too Many Requests"), Class::RateLimit);
    assert_eq!(class("Claude usage limit reached, resets 3pm"), Class::RateLimit);
    assert_eq!(class("You've hit your limit · resets 3pm"), Class::RateLimit, "as claude says it in a result");
    assert_eq!(class("model not found: opus-9"), Class::Permanent);
    assert_eq!(class("Not logged in · Please run /login"), Class::Permanent);
    assert_eq!(class("error_max_budget_usd"), Class::Permanent);
    assert_eq!(class("connection reset by peer"), Class::Transient);
    assert_eq!(class(""), Class::Transient, "what nothing knows keeps work going");
}

/// A status code is a whole token, not a run of digits inside another number; and the agent's own "you spent your
/// turns/budget" codes win over a "limit reached" in their text.
#[test]
fn a_status_code_is_a_token_and_an_agents_own_stop_code_is_permanent() {
    for transient in ["connection reset after 4290ms", "port 14290 refused", "pid 4030 exited", "took 1403 ms"] {
        assert_eq!(class(transient), Class::Transient, "{transient}");
    }
    assert_eq!(class("HTTP 429"), Class::RateLimit);
    assert_eq!(class("status=429, retry later"), Class::RateLimit);
    assert_eq!(class("(401)"), Class::Permanent);
    assert_eq!(class("403 Forbidden"), Class::Permanent);
    assert_eq!(class("error_max_turns: tool call limit reached"), Class::Permanent);
    assert_eq!(class("error_max_budget_usd: usage limit reached"), Class::Permanent);
}

#[test]
fn rate_limit_beats_permanent() {
    assert_eq!(class("429: quota exceeded for unsupported plan"), Class::RateLimit);
}

#[test]
fn words_are_compared_without_case() {
    assert_eq!(class("RATE LIMIT hit"), Class::RateLimit);
    assert_eq!(class("Overloaded_error"), Class::RateLimit);
}

#[test]
fn a_rate_limit_waits_until_it_says_it_ends_or_for_the_rows_own_time() {
    let f = base();
    let none = f.classify("You've hit your usage limit", 5_000);
    assert_eq!((none.row.as_deref(), none.wait_ms), (Some("limit"), Some(30 * 60_000)), "the row's wait");
    let overloaded = f.classify("API Error: overloaded_error", 5_000);
    assert_eq!((overloaded.row.as_deref(), overloaded.wait_ms), (Some("overloaded"), Some(120_000)));
    // the words name the time: unix seconds, a little after it so the window has opened
    let now = 1_700_000_000_000;
    let named = f.classify("usage limit reached (resets at 1700000600)", now);
    assert_eq!(named.wait_ms, Some(600_000 + 2_000));
    let past = f.classify("usage limit reached (resets at 1699999000)", now);
    assert_eq!(past.wait_ms, Some(2_000), "a time already gone waits only the margin");
    // a number that is not a time since 1970 is not believed
    let odd = f.classify("usage limit reached (resets at 12)", now);
    assert_eq!(odd.wait_ms, Some(30 * 60_000));
    // the last mention counts
    let two = f.classify("usage limit (resets at 1700000100), then (resets at 1700000300)", now);
    assert_eq!(two.wait_ms, Some(300_000 + 2_000));
    let permanent = f.classify("not logged in (resets at 1700000600)", now);
    assert_eq!(permanent.wait_ms, None, "only a rate limit waits");
    assert_eq!(f.classify("connection reset", now).row, None);
}

#[test]
fn rows_are_read_by_order_then_id_and_a_row_can_be_replaced_by_a_layer() {
    let (t, problems) = load(vec![pack(
        "p",
        r#"api = 1
[[failure]]
id = "b"
class = "permanent"
any = ["boom"]
order = 5
[[failure]]
id = "a"
class = "rate-limit"
any = ["boom", "slow down"]
order = 5
wait = "10s"
[[failure]]
id = "z"
class = "permanent"
any = ["slow down"]
"#,
    )]);
    assert!(problems.is_empty(), "{problems:?}");
    let (f, problems) = Failures::load(&t);
    assert!(problems.is_empty(), "{problems:?}");
    let s = f.classify("Boom!", 0);
    assert_eq!((s.row.as_deref(), s.class, s.wait_ms), (Some("a"), Class::RateLimit, Some(10_000)), "same order: by id");
    assert_eq!(f.classify("please slow down", 0).row.as_deref(), Some("a"), "order 5 before 100");
    // the operator's file changes one field of the base row and leaves the rest
    let mine = "[[failure]]\nid = \"limit\"\nwait = \"1m\"\n";
    let (f, problems) = Failures::load(&base_tables(mine));
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(f.classify("usage limit", 0).wait_ms, Some(60_000));
    assert_eq!(f.classify("429", 0).class, Class::RateLimit, "its words are still there");
}

#[test]
fn a_row_that_cannot_be_used_is_a_problem_and_no_row() {
    let bad = |body: &str| {
        let (t, problems) = load(vec![pack("p", &format!("api = 1\n[[failure]]\nid = \"x\"\n{body}\n"))]);
        let (f, more) = Failures::load(&t);
        (f, problems.iter().chain(&more).map(ToString::to_string).collect::<Vec<_>>().join(" | "))
    };
    let (f, said) = bad("class = \"transient\"\nany = []");
    assert!(f.is_empty() && said.contains("1 to 64 characters"), "{said}");
    let (f, said) = bad("class = \"transient\"\nany = [\"  \"]");
    assert!(f.is_empty() && said.contains("1 to 64 characters"), "{said}");
    let (f, said) = bad(&format!("class = \"transient\"\nany = [\"{}\"]", "x".repeat(65)));
    assert!(f.is_empty() && said.contains("1 to 64 characters"), "{said}");
    let (f, said) = bad("class = \"permanent\"\nany = [\"x\"]\nwait = \"5m\"");
    assert!(f.is_empty() && said.contains("only a rate limit has one"), "{said}");
    let (f, said) = bad("class = \"rate-limit\"\nany = [\"x\"]\nwait = \"soon\"");
    assert!(f.is_empty() && said.contains("expected a duration"), "the schema reads a wait first: {said}");
    let (f, said) = bad("class = \"rate-limit\"\nany = [\"x\"]\nwait = \"0s\"");
    assert!(f.is_empty() && said.contains("more than zero"), "{said}");
    let (f, said) = bad("class = \"nonsense\"\nany = [\"x\"]");
    assert!(f.is_empty() && !said.is_empty(), "an unknown class is refused by the schema");
}

#[test]
fn a_flow_that_reports_which_step_failed_is_permanent_whatever_the_step_quoted() {
    use j4n_core::failure::{Class, Failures};
    let tables = crate::front_cases::base();
    let (failures, problems) = Failures::load(&tables);
    assert!(problems.is_empty(), "{problems:?}");
    // the step was a task of its own: what it could wait out it waited out, and a quote of a limit is not the flow's limit
    let step = "flow step `work` (own, t4) did not finish: 429 rate limit (resets at 1700000000): slow down";
    assert_eq!(failures.classify(step, 0).class, Class::Permanent, "{:?}", failures.classify(step, 0));
    // the same words without the flow's prefix are a limit
    assert_eq!(failures.classify("429 rate limit: slow down", 0).class, Class::RateLimit);
}

#[test]
fn a_missing_key_is_permanent_like_a_401() {
    assert_eq!(class("Gemini API key is missing or not configured. (the agent offered no way to sign in)"), Class::Permanent);
    assert_eq!(class("unexpected status 401 Unauthorized: Missing bearer or basic authentication"), Class::Permanent);
    assert_eq!(class("could not reach the model: Network Error: peer closed connection"), Class::Transient);
}
