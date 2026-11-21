//! The command line: what each argument means, and that one nobody knows is an error and never ignored.

use j4n::cli::{Approval, Cmd, HELP, Opts, RUN_HELP, duration, parse};
use j4n_core::task::Mode;
use std::time::Duration;

fn args(line: &str) -> Vec<String> {
    line.split_whitespace().map(String::from).collect()
}

fn ok(line: &str) -> Cmd {
    parse(&args(line)).unwrap_or_else(|e| panic!("{line:?}: {e}"))
}

fn err(line: &str) -> String {
    parse(&args(line)).expect_err(line)
}

fn bare() -> Opts {
    Opts { doer: None, model: None, approval: None, wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false }
}

#[test]
fn nothing_at_all_is_the_screen_or_the_lines_on_stdin() {
    assert_eq!(ok(""), Cmd::Chat(bare()));
}

#[test]
fn every_option_in_both_spellings() {
    let want = Opts { doer: Some("claude".into()), model: Some("haiku".into()), approval: Some(Approval::Full), wall: Some(Duration::from_secs(90)), usd: Some(0.5), mode: Mode::Read, place: Some("worktree".into()), shape: false, anywhere: false };
    assert_eq!(ok("--doer claude --model haiku --approval full --wall 90s --usd 0.5 --mode read --place worktree"), Cmd::Chat(want.clone()));
    assert_eq!(ok("--doer=claude --model=haiku --approval=full --wall=90s --usd=0.5 --mode=read --place=worktree"), Cmd::Chat(want.clone()));
    assert_eq!(ok("run --place worktree --mode read --usd 0.5 --wall 90s --approval full --model haiku --doer claude do it"), Cmd::Run(want, vec!["do".into(), "it".into()]));
}

#[test]
fn the_approvals_and_the_modes_are_the_words_they_are() {
    for (w, a) in [("full", Approval::Full), ("ask", Approval::Ask), ("deny", Approval::Deny)] {
        assert!(matches!(ok(&format!("--approval {w}")), Cmd::Chat(o) if o.approval == Some(a)));
    }
    assert!(matches!(ok("--mode write"), Cmd::Chat(o) if o.mode == Mode::Write));
    assert!(err("--approval maybe").contains("full, ask or deny"));
    assert!(err("--mode sideways").contains("read or write"));
}

#[test]
fn run_takes_the_words_wherever_they_stand_and_a_double_dash_ends_the_options() {
    assert_eq!(ok("run say ok --doer test"), Cmd::Run(Opts { doer: Some("test".into()), ..bare() }, vec!["say".into(), "ok".into()]));
    assert_eq!(ok("run -- --not-an-option here"), Cmd::Run(bare(), vec!["--not-an-option".into(), "here".into()]));
    assert_eq!(ok("run - x"), Cmd::Run(bare(), vec!["-".into(), "x".into()]), "a lone dash is a word");
}

#[test]
fn run_without_words_is_refused() {
    assert!(err("run").contains("needs the words"));
    assert!(err("run --doer test").contains("needs the words"));
}

#[test]
fn a_word_where_only_options_go_is_a_command_for_the_packs_to_know_and_its_words_follow_it() {
    assert_eq!(ok("dance"), Cmd::Word(bare(), vec!["dance".into()]));
    assert_eq!(ok("plan --doer test add a flag"), Cmd::Word(Opts { doer: Some("test".into()), ..bare() }, vec!["plan".into(), "add".into(), "a".into(), "flag".into()]));
}

#[test]
fn an_unknown_option_is_an_error_naming_it_in_every_position() {
    for line in ["--bogus", "run --bogus x", "run x --bogus", "-z", "--doer=a --bogus=1", "run --bogus=1 x"] {
        let e = err(line);
        assert!(e.contains("unknown option") && e.contains("bogus") || e.contains("`-z`"), "{line}: {e}");
    }
}

#[test]
fn an_option_with_an_equals_sign_that_is_not_a_known_one_is_named_whole() {
    let e = err("-x=1");
    assert!(e.contains("-x=1"), "{e}");
}

#[test]
fn an_option_that_needs_a_value_and_has_none_is_refused() {
    for line in ["--doer", "--model", "--approval", "--wall", "--usd", "--mode", "--doer=", "run x --wall"] {
        assert!(err(line).contains("needs a value"), "{line}");
    }
}

#[test]
fn a_value_that_is_not_what_it_should_be_is_refused_in_words() {
    assert!(err("--wall soon").contains("not a time"));
    assert!(err("--wall 0").contains("not a time"));
    assert!(err("--wall -5").contains("unknown option") || err("--wall -5").contains("not a time"), "a negative time is none");
    assert!(err("--usd lots").contains("not an amount"));
    assert!(err("--usd 0").contains("not an amount"));
    assert!(err("--usd NaN").contains("not an amount"));
    assert!(err("--usd inf").contains("not an amount"));
}

#[test]
fn version_and_help_win_wherever_they_are_and_run_has_its_own_help() {
    assert_eq!(ok("--version"), Cmd::Version);
    assert_eq!(ok("-V"), Cmd::Version);
    assert_eq!(ok("run x --version"), Cmd::Version);
    assert_eq!(ok("--help"), Cmd::Help(HELP));
    assert_eq!(ok("-h"), Cmd::Help(HELP));
    assert_eq!(ok("run --help"), Cmd::Help(RUN_HELP));
    assert_eq!(ok("run x -h"), Cmd::Help(RUN_HELP));
    assert!(HELP.contains("stdin") && HELP.contains("--wall") && RUN_HELP.contains("--wall"), "what an outside caller looks for is there");
    assert!(HELP.contains("--approval") && HELP.contains("--doer") && HELP.contains("--model"));
}

#[test]
fn the_help_names_every_option_the_parser_takes() {
    for opt in ["--doer", "--model", "--approval", "--wall", "--usd", "--mode"] {
        assert!(HELP.contains(opt) && RUN_HELP.contains(opt), "{opt}");
    }
}

#[test]
fn times() {
    let d = |t: &str| duration(t).map(|d| d.as_millis());
    assert_eq!(d("90s"), Ok(90_000));
    assert_eq!(d("90"), Ok(90_000));
    assert_eq!(d("15m"), Ok(900_000));
    assert_eq!(d("1h"), Ok(3_600_000));
    assert_eq!(d("250ms"), Ok(250));
    assert_eq!(d("1.5s"), Ok(1_500));
    assert_eq!(d(" 2s "), Ok(2_000));
    // what the Table reads is read here too: parts of units in one, and days
    assert_eq!(d("1h30m"), Ok(5_400_000));
    assert_eq!(d("1m30s"), Ok(90_000));
    assert_eq!(d("2d"), Ok(172_800_000));
    // a fraction with a unit above seconds
    assert_eq!(d("1.5m"), Ok(90_000));
    assert_eq!(d("0.5h"), Ok(1_800_000));
    for bad in ["", "s", "h1", "1 s", "1h 30m", "200d", "-1s", "0s", "0", "1e3s", "99999999999s", "1.2.3s", "NaNs"] {
        assert!(d(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn answer_takes_by_and_gate_and_no_words() {
    assert_eq!(ok("answer"), Cmd::Answer { by: None, gate: false });
    assert_eq!(ok("answer --gate"), Cmd::Answer { by: None, gate: true });
    assert_eq!(ok("answer --by rule:intake"), Cmd::Answer { by: Some("rule:intake".into()), gate: false });
    assert_eq!(ok("answer --by=decider:jev"), Cmd::Answer { by: Some("decider:jev".into()), gate: false });
    assert_eq!(ok("answer --help"), Cmd::Help(j4n::cli::ANSWER_HELP));
    assert!(err("answer --by").contains("needs a value"));
    assert!(err("answer --doer claude").contains("--doer"));
    assert!(err("answer words").contains("takes no words"));
    assert!(err("answer --gate --by rule:intake").contains("do not go together"));
}

#[test]
fn py_takes_a_cell_from_the_argument_or_a_file_and_the_task_and_time_it_is_for() {
    use j4n::cli::PyCode;
    let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let cell = |a: &[&str]| parse(&args(a)).unwrap_or_else(|e| panic!("{a:?}: {e}"));
    assert_eq!(cell(&["py", "-c", "x + 1"]), Cmd::Py { task: None, code: PyCode::Text("x + 1".into()), timeout: None });
    assert_eq!(cell(&["py", "-f", "cells/a.py", "--task", "t3", "--timeout", "120"]), Cmd::Py { task: Some("t3".into()), code: PyCode::File("cells/a.py".into()), timeout: Some(120) });
    assert!(matches!(cell(&["py", "--help"]), Cmd::Help(h) if h.contains("j4n py")));
    for bad in [&["py"][..], &["py", "-c"], &["py", "-q", "x"], &["py", "-c", "1", "--timeout", "0"], &["py", "--task"]] {
        assert!(parse(&args(bad)).is_err(), "{bad:?}");
    }
}

#[test]
fn look_takes_the_words_of_one_line_and_two_options_and_nothing_else() {
    assert_eq!(ok("look fix the typo in the readme"), Cmd::Look { doer: None, model: None, line: "fix the typo in the readme".into(), forced: Default::default() });
    assert_eq!(ok("look --doer claude --model=haiku read docs/a.md --and more"), Cmd::Look { doer: Some("claude".into()), model: Some("haiku".into()), line: "read docs/a.md --and more".into(), forced: Default::default() });
    assert!(matches!(ok("look --help"), Cmd::Help(h) if h.contains("j4n look")));
    assert!(err("look").contains("needs the words") && err("look --doer").contains("needs a value") && err("look --wall 5 x").contains("unknown option"));
}
