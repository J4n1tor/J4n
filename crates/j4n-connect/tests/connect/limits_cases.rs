//! A program's own usage limit: how a line, an object, a subcommand and a refusal are read, and what a row is made of a program's output.

use j4n_connect::limits::{self, Gauge, Pattern, Row, commands, learn, objects, refusal_words, refused_message, time};
use std::sync::Arc;

fn gauge(rows: Vec<Row>) -> Gauge {
    Gauge::new(Arc::new(rows), Arc::default())
}

fn shape(line: &str) -> Row {
    Row { line: Some(line.into()), ..Row::default() }
}

fn keys(used: &str, cap: &str, reset: &str) -> Row {
    Row { used: Some(used.into()), cap: Some(cap.into()), reset: Some(reset.into()), ..Row::default() }
}

#[test]
fn a_time_is_read_in_the_words_a_program_says_it() {
    for (text, secs) in [("4m10s", 250.0), ("58s", 58.0), ("1h 2m", 3720.0), ("2 minutes", 120.0), ("90", 90.0), ("1d", 86400.0), ("800m5s", 48005.0), ("1.5h", 5400.0)] {
        assert_eq!(time(text), Some(secs), "{text}");
    }
    for bad in ["", "soon", "4x", "5%", "1/2"] {
        assert_eq!(time(bad), None, "{bad}");
    }
}

#[test]
fn a_shape_reads_the_numbers_where_its_holes_are_and_ignores_what_surrounds_the_line() {
    let p = Pattern::parse("[{unit} {used}/{cap} · resets in {reset}]").unwrap();
    let r = p.find("12:00:01 [req 3/5 · resets in 4m10s] ok").unwrap();
    assert_eq!((r.used, r.cap, r.reset_s, r.unit.as_deref()), (Some(3.0), Some(5.0), Some(250.0), Some("req")));
    let words = Pattern::parse("usage: {used} of {cap} {unit} used in the current window; resets in {reset}").unwrap();
    assert_eq!(words.find("usage: 12 of 160 tok used in the current window; resets in 3m20s").unwrap().cap, Some(160.0));
    assert!(p.find("[req three/5]").is_none());
    assert!(Pattern::parse("no holes").is_err() && Pattern::parse("{bogus} {used}").is_err() && Pattern::parse("{used").is_err());
    let skip = Pattern::parse("quota {used}/{cap} {*} resets in {reset}").unwrap();
    assert_eq!(skip.find("--- quota 3/5 req, window resets in 6m58s ---").unwrap().reset_s, Some(418.0));
}

#[test]
fn json_is_found_wherever_a_line_carries_it() {
    let obj = r#"{"used":3,"cap":5,"reset_s":250}"#;
    for line in [obj.to_string(), format!("data: {obj}"), format!("000{} {obj}", obj.len()), format!("evt=usage body=\"{}\"", obj.replace('"', "\\\"")), format!("<usage body=\"{}\"/>", obj.replace('"', "&quot;")), format!("{{\"evt\":{{\"k\":\"usage\",\"body\":{obj}}}}}")] {
        assert!(!objects(&line).is_empty(), "{line}");
    }
    let mut g = gauge(vec![keys("used", "cap", "reset_s")]);
    g.line(&format!("{{\"evt\":{{\"k\":\"usage\",\"body\":{obj}}}}}"));
    let told = g.told(None, 1_000);
    assert_eq!((told.limits[0].utilization, told.limits[0].resets_at, told.limits[0].status.as_str()), (0.6, 1_250, "allowed"));
}

#[test]
fn a_gauge_is_a_fraction_of_a_window_that_ends_and_a_full_one_is_rejected() {
    let mut g = gauge(vec![shape("[{unit} {used}/{cap} · resets in {reset}]")]);
    g.line("[beans 27/30 · resets in 14h5m3s]");
    let l = &g.told(None, 100).limits[0];
    assert_eq!((l.window.as_str(), l.utilization, l.resets_at, l.status.as_str()), ("beans", 0.9, 100 + 50_703, "allowed"));
    let mut g = gauge(vec![shape("{left} left of {cap} {unit}"), shape("{percent}% used")]);
    g.line("12 left of 160 tok");
    assert_eq!(g.told(None, 0).limits[0].utilization, 0.93);
    g.line("usage at 83% used");
    assert_eq!(g.told(None, 0).limits[0].utilization, 0.83);
    let mut g = gauge(vec![shape("{used}/{cap}")]);
    g.line("5/5");
    assert_eq!(g.told(None, 0).limits[0].status, "rejected", "nothing is left of it");
}

#[test]
fn a_line_that_is_no_row_of_the_limit_says_nothing_of_it() {
    let mut g = gauge(vec![shape("[{unit} {used}/{cap} · resets in {reset}]")]);
    g.line("step 3/10 done in 5s");
    g.line("hello");
    assert!(g.told(None, 0).limits.is_empty());
}

#[test]
fn a_run_that_fails_with_the_words_of_a_limit_is_a_refusal_with_the_wait_the_program_gave() {
    let mut g = gauge(vec![]);
    let told = g.told(Some("`x` ended with exit 75: rate limited: retry in 4m10s"), 1_000);
    assert_eq!(told.refused_until, Some(1_250));
    let l = &told.limits[0];
    assert_eq!((l.utilization, l.status.as_str(), l.resets_at), (1.0, "rejected", 1_250));
    // a refusal keeps what the turn before it said of the fraction when it names none
    let mut g = gauge(vec![shape("{used}/{cap}")]);
    g.line("4.7/5");
    g.told(None, 0);
    let again = g.told(Some("error: quota exhausted"), 0);
    assert_eq!((again.limits[0].utilization, again.refused_until), (0.94, Some(1800)), "no time said: the failure table's own wait");
    let said = g.told(Some("=== QUOTA EXCEEDED 5/5 req - window resets in 7m0s ==="), 10);
    assert_eq!((said.limits[0].utilization, said.refused_until), (1.0, Some(430)));
    assert!(gauge(vec![]).told(Some("`x` ended with exit 1: no such file"), 0).limits.is_empty(), "a failure that is no limit's is not read as one");
}

#[test]
fn a_refusal_a_program_prints_where_the_failure_does_not_carry_it_is_still_read() {
    let mut g = gauge(vec![shape("[{unit} {used}/{cap} · resets in {reset}]")]);
    g.line("[beans 57.3/63 · resets in 9h33m]");
    g.told(None, 0);
    g.line("limit reached: beans 57.3/63; resets in 9h33m");
    let told = g.told(Some("`x` ended with exit 1"), 100);
    assert_eq!((told.refused_until, told.limits[0].utilization), (Some(100 + 34_380), 0.91));
    assert_eq!(told.said.as_deref(), Some("limit reached: beans 57.3/63; resets in 9h33m"));
    // words of a limit in a run that did not fail are only words
    let mut g = gauge(vec![]);
    g.line("the quota is a lot");
    assert!(g.told(None, 0).limits.is_empty());
}

#[test]
fn a_refusal_in_an_object_is_one_whatever_words_it_has() {
    let mut g = gauge(vec![keys("used", "cap", "reset_s")]);
    g.line(r#"{"say":"","usage":{"used":5,"cap":5,"reset_s":131,"exceeded":true}}"#);
    let told = g.told(Some("`x` ended with exit 1"), 0);
    assert_eq!(told.refused_until, Some(131));
}

#[test]
fn a_refusal_is_given_to_the_failure_table_in_the_words_it_reads_with_the_time_it_ends() {
    assert_eq!(refused_message("`x` is rate limited: HTTP 429", 99), "`x` is rate limited: HTTP 429 (resets at 99)");
    let said = refused_message("`x` ended with exit 1", 99);
    assert!(said.starts_with("usage limit reached: ") && said.ends_with("(resets at 99)") && refusal_words(&said), "{said}");
    assert!(!refusal_words("no such file or directory"));
}

#[test]
fn a_row_is_learned_from_the_lines_and_objects_a_program_prints() {
    let rows = learn(&["hello\n[req 2/5 · resets in 6m58s]\n".to_string()]);
    assert_eq!(rows[0].line.as_deref(), Some("[req {used}/{cap} · resets in {reset}]"));
    assert_eq!(rows[0].unit.as_deref(), Some("req"));
    let mut g = gauge(rows);
    g.line("[req 4/5 · resets in 1m5s]");
    assert_eq!(g.told(None, 0).limits[0].utilization, 0.8);
    let rows = learn(&["--- quota 3/8 req, window resets in 4m0s ---".to_string()]);
    assert_eq!(rows[0].line.as_deref(), Some("--- quota {used}/{cap} req, window resets in {reset} ---"));
    let rows = learn(&[r#"{"say":"hi","usage":{"cap":160,"left":150,"reset_s":131,"unit":"tok","used":10}}"#.to_string()]);
    assert_eq!((rows[0].used.as_deref(), rows[0].cap.as_deref(), rows[0].reset.as_deref(), rows[0].unit.as_deref()), (Some("used"), Some("cap"), Some("reset_s"), Some("tok")));
    let rows = learn(&["usage: 12 of 160 tok used in the current window; resets in 3m20s".to_string()]);
    assert_eq!(rows[0].line.as_deref(), Some("usage: {used} of {cap} tok used in the current window; resets in {reset}"));
    assert!(learn(&["step 3/10 done".to_string(), "Reply with the single word: x".to_string(), "version 1.2.3".to_string()]).is_empty(), "no limit in what it prints");
}

#[test]
fn the_subcommands_that_report_a_limit_are_the_ones_the_help_says_do() {
    let help = "x - a tool\n\nOptions:\n  --version      print the version\n  usage          show how much of your quota is used\n  status         show the weather\n  limits         the plan\n";
    assert_eq!(commands(help), vec![vec!["usage".to_string()]]);
    assert!(commands("  usage          print this text\n").is_empty());
    let _ = limits::check;
}

#[test]
fn a_row_that_cannot_be_used_is_refused_with_why() {
    assert!(limits::check(&[Row::default()]).is_err());
    assert!(limits::check(&[Row { path: Some("../x".into()), ..keys("used", "cap", "reset") }]).is_err());
    assert!(limits::check(&[Row { path: Some("/etc/x".into()), ..keys("used", "cap", "reset") }]).is_err());
    assert!(limits::check(&[Row { command: vec!["a b".into()], ..keys("used", "cap", "reset") }]).is_err());
    assert!(limits::check(&[Row { command: vec!["usage".into()], ..shape("{used}/{cap}") }, Row { path: Some(".x/q.json".into()), ..keys("remaining", "limit", "reset_at") }]).is_ok());
}
