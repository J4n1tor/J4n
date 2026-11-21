//! What a failed check names, read by code: Python's unittest and pytest, an output cut at its start, and what a part laid in broke
//! against the folder's check before it.

use crate::branch_validate::{SCREEN_AND_TURN, SCREEN_DRAW_FAILS, SCREEN_UNLOADED};
use j4n_core::failing::{Caused, caused, read};

#[test]
fn unittest_names_each_failing_test_and_the_modules_it_could_not_load() {
    let f = read(SCREEN_AND_TURN).unwrap();
    assert_eq!(f.ids, ["unittest.loader._FailedTest.test_screen", "test_engine.Moves.test_turn"]);
    assert_eq!(f.unloaded.into_iter().collect::<Vec<_>>(), ["test_screen"]);
    // before Python 3.11 the brackets name the class only
    let old = "======================================================================\nFAIL: test_turn (test_engine.Moves)\n----------------------------------------------------------------------\nAssertionError\n\nRan 3 tests in 0.001s\n\nFAILED (failures=1)";
    assert_eq!(read(old).unwrap().ids, ["test_engine.Moves.test_turn"]);
}

#[test]
fn an_output_whose_start_was_cut_or_that_no_runner_wrote_is_not_read() {
    let cut = SCREEN_UNLOADED.replace("FAILED (errors=1)", "FAILED (failures=2, errors=1)");
    assert!(read(&cut).unwrap_err().contains("cut"), "an entry cut off with the start must not pass for a test that did not fail");
    assert!(read("7-make check failed (exit 2):\nmake: *** [test] Error 1").unwrap_err().contains("not one J4n reads"));
    assert!(read("Ran 0 tests in 0.000s\n\nNO TESTS RAN").is_err(), "a failure that names no test is not read");
}

#[test]
fn pytest_names_the_lines_of_its_short_summary_and_a_run_cut_short_is_not_read() {
    let out = "..F.E\n=========================== short test summary info ============================\nFAILED tests/test_a.py::test_x - assert 1 == 2\nERROR tests/test_b.py::test_y - fixture 'db' not found\n1 failed, 3 passed, 1 error in 0.12s";
    assert_eq!(read(out).unwrap().ids, ["tests/test_a.py::test_x", "tests/test_b.py::test_y"]);
    let stopped = "=========================== short test summary info ============================\nERROR tests/test_b.py\n!!!!!!!!!!!!!!!!!!!! Interrupted: 1 error during collection !!!!!!!!!!!!!!!!!!!!\n1 error in 0.05s";
    assert!(read(stopped).unwrap_err().contains("interrupted"), "a collection error stops every test: what did not run says nothing");
}

#[test]
fn a_part_is_judged_by_what_it_broke_not_by_what_failed_before_it() {
    // the same test of the loader, though its message changed (`No module named` before, `cannot import name` after)
    let after = SCREEN_UNLOADED.replace("No module named 'chess.engine'", "cannot import name 'pieces' from 'chess'");
    assert_eq!(caused(Some(SCREEN_UNLOADED), &after), Caused::Old(vec!["unittest.loader._FailedTest.test_screen".into()]));
    // a test of a module that could not be loaded before was failing too
    assert_eq!(caused(Some(SCREEN_UNLOADED), SCREEN_DRAW_FAILS), Caused::Old(vec!["test_screen.ScreenTests.test_draw".into()]));
    assert_eq!(caused(Some(SCREEN_UNLOADED), SCREEN_AND_TURN), Caused::New(vec!["test_engine.Moves.test_turn".into()]));
    // a module that loaded before and does not now is the part's doing
    assert_eq!(caused(Some(SCREEN_DRAW_FAILS), SCREEN_UNLOADED), Caused::New(vec!["unittest.loader._FailedTest.test_screen".into()]));
    assert_eq!(caused(None, SCREEN_UNLOADED), Caused::New(vec!["unittest.loader._FailedTest.test_screen".into()]), "a green folder needs no reading of what came before");
    assert!(matches!(caused(Some("make: *** [test] Error 1"), SCREEN_UNLOADED), Caused::Unread(why) if why.contains("before")));
}
