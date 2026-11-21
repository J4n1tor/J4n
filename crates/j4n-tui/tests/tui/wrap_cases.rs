//! Cutting text to the width of a terminal.

use j4n_tui::wrap::{fit, wrap};

#[test]
fn words_stay_whole_and_lines_are_as_full_as_they_can_be() {
    assert_eq!(wrap("one two three four", 9), ["one two", "three", "four"]);
    assert_eq!(wrap("one two three four", 13), ["one two three", "four"]);
    assert_eq!(wrap("one two", 7), ["one two"]);
    assert_eq!(wrap("one two", 6), ["one", "two"]);
}

#[test]
fn a_newline_is_a_newline_and_an_empty_text_is_one_empty_line() {
    assert_eq!(wrap("a\n\nb", 10), ["a", "", "b"]);
    assert_eq!(wrap("", 10), [""]);
    assert_eq!(wrap("a\n", 10), ["a", ""]);
}

#[test]
fn a_word_wider_than_the_line_is_cut_where_it_must_be() {
    assert_eq!(wrap("abcdefghij", 4), ["abcd", "efgh", "ij"]);
    assert_eq!(wrap("ab abcdefgh", 4), ["ab", "abcd", "efgh"]);
}

#[test]
fn width_is_what_a_terminal_shows_not_what_the_string_holds() {
    assert_eq!(wrap("世界世界世", 4), ["世界", "世界", "世"], "each of these is two columns");
    assert_eq!(wrap("世 界", 3), ["世", "界"]);
    assert_eq!(wrap("ééé ééé", 3), ["ééé", "ééé"]);
}

#[test]
fn a_width_of_zero_is_treated_as_one_and_never_loops() {
    assert_eq!(wrap("ab", 0), ["a", "b"]);
}

#[test]
fn spaces_between_words_are_kept_as_they_were_written() {
    assert_eq!(wrap("a  b", 10), ["a  b"]);
    assert_eq!(wrap("  lead", 10), ["  lead"]);
}

#[test]
fn fit_cuts_with_an_ellipsis_and_leaves_what_fits_alone() {
    assert_eq!(fit("short", 10), "short");
    assert_eq!(fit("exactly ten", 11), "exactly ten");
    assert_eq!(fit("this is longer", 8), "this is…");
    assert_eq!(fit("世界世界", 5), "世界…");
    assert_eq!(fit("anything", 0), "…");
}
