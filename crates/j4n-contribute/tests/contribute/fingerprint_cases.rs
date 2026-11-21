//! The same bug is the same fingerprint wherever and whenever it happens, and a different bug is not.

use j4n_contribute::fingerprint::{fingerprint, skeleton};

fn fp(message: &str) -> String {
    fingerprint("floor", "turn-cap", "model", message)
}

#[test]
fn a_fingerprint_is_twelve_hex_digits() {
    let f = fp("the model went 12 turns without finishing");
    assert!(f.len() == 12 && f.chars().all(|c| c.is_ascii_hexdigit()), "{f}");
}

#[test]
fn what_belongs_to_one_run_is_not_in_it() {
    let base = fp("the model went 12 turns without finishing");
    assert_eq!(base, fp("the model went 40 turns without finishing"), "numbers");
    assert_eq!(base, fp("The model went 7 turns without finishing."), "case and the full stop");
    assert_eq!(fp("flow step `build` went back 3 times"), fp("flow step `lint-all` went back 9 times"), "quoted text and numbers");
    assert_eq!(fp("could not read /home/ana/p/src/a.rs"), fp("could not read /tmp/x/y.rs"), "paths");
    assert_eq!(fp("could not read ~/notes.txt"), fp("could not read ./other.txt"), "a home path and a relative one");
    assert_eq!(fp("task 3f9a1c2b7d failed"), fp("task 00ab34ff12 failed"), "a long hex hash");
    assert_eq!(fp("session 123e4567-e89b-12d3-a456-426614174000 lost"), fp("session aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee lost"), "a uuid");
    assert_eq!(fp("request req_8fk3j29x failed"), fp("request req_a8s7d6f5 failed"), "an id that mixes letters and digits");
    assert_eq!(fp("it said \"no such file\" and stopped"), fp("it said \"permission\" and stopped"), "double quotes");
    assert_eq!(fp("it said 'no such file' and stopped"), fp("it said 'x' and stopped"), "single quotes");
}

#[test]
fn what_makes_a_bug_that_bug_is() {
    let base = fp("the model went 12 turns without finishing");
    assert_ne!(base, fp("the model sent the same reply twice in a row"), "another message");
    assert_ne!(base, fingerprint("hang", "turn-cap", "model", "the model went 12 turns without finishing"), "another source");
    assert_ne!(base, fingerprint("floor", "same-reply", "model", "the model went 12 turns without finishing"), "another id");
    assert_ne!(base, fingerprint("floor", "turn-cap", "claude", "the model went 12 turns without finishing"), "another row");
    assert_ne!(fp("flow step went back"), fp("flow step went forward"));
}

#[test]
fn an_apostrophe_in_a_word_is_not_a_quote_and_a_quote_that_never_closes_is_left_alone() {
    assert_eq!(skeleton("it doesn't work and it didn't start"), "it doesn't work and it didn't start");
    assert_eq!(skeleton("it said 'oops and then"), "it said 'oops and then");
    assert_eq!(skeleton("the 'x' tool and the `y` tool"), "the _ tool and the _ tool");
    assert_eq!(skeleton("tool 'it's' failed"), "tool _ failed", "an apostrophe followed by a letter does not close a quote");
}

#[test]
fn the_skeleton_is_lower_case_with_single_spaces_numbers_as_hash_and_no_trailing_stop() {
    assert_eq!(skeleton("  The  Model\twent 12 turns... "), "the model went # turns");
    assert_eq!(skeleton("a1b2"), "a#b#", "short mixed words keep their letters");
    assert_eq!(skeleton("v2.1.311 is out"), "v#.#.# is out");
    assert_eq!(skeleton("see abcdef0 now"), "see _ now", "seven hex digits with a digit are a hash");
    assert_eq!(skeleton("deadbee"), "deadbee", "seven hex letters with no digit are a word");
    assert_eq!(skeleton("took 12345678 ms"), "took # ms", "a long number is a number, not a hash");
    assert_eq!(skeleton("abcdefg1"), "_", "eight characters of letters and a digit are an id");
    assert_eq!(skeleton("abcdefgh"), "abcdefgh", "letters only are a word");
    assert!(skeleton(&"word ".repeat(100)).chars().count() <= 200, "a skeleton is clipped");
    assert_eq!(skeleton(""), "");
    assert_eq!(skeleton("what happened?"), "what happened", "a question mark ends a message");
    assert_eq!(skeleton("it failed:"), "it failed", "a colon ends a message");
    assert_eq!(skeleton("abc12d"), "abc#d", "six hex digits are not a hash");
    assert_eq!(skeleton("in ~user now"), "in _ now", "a word that starts with a tilde is a path");
    assert_eq!(skeleton("read C:\\x now"), "read _ now", "a backslash makes a path");
    assert_eq!(skeleton("open / now"), "open _ now", "a lone slash is a path");
    assert_eq!(skeleton("code xyz1234 now"), "code xyz#### now".replace("####", "#"), "seven characters are not an id");
    assert_eq!(skeleton("request req-8fk3j29x failed"), "request _ failed", "an id may hold dashes");
}

#[test]
fn a_quote_opens_at_the_start_and_closes_at_the_end_but_a_possessive_opens_nothing() {
    assert_eq!(skeleton("'x' failed"), "_ failed", "a quote at the start of the text opens");
    assert_eq!(skeleton("it said 'x'"), "it said _", "a quote at the end of the text closes");
    assert_eq!(skeleton("the users' files and the users' dirs"), "the users' files and the users' dirs", "an apostrophe after a letter opens nothing");
}

#[test]
fn the_fingerprint_does_not_change() {
    // a report searches a repository for this text: if how a message is stripped changes, every issue splits in two. A change here is a change to the doc.
    assert_eq!(fingerprint("dialect", "unread:mystery_frame", "claude-stream", ""), "a126bf1f5bfd");
}
