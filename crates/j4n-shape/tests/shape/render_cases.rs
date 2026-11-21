use j4n_shape::check::Item;
use j4n_shape::render::{Template, fill, section};

fn template() -> Template {
    let toml = r#"
id = "list"
head = "Notes on this line:"
ref = "- \"{phrase}\" is turn {turn}: \"{quote}\""
files = "- Likely relevant: {paths}"
done = "- Done when `{cmd}` passes"
note = "- {text}"
question = "- {text} {answer}"
"#;
    toml::from_str(toml).unwrap()
}

fn item(kind: &'static str, text: &str, parts: &[(&'static str, &str)]) -> Item {
    Item { key: kind.into(), kind, text: text.into(), source: "t1".into(), proof: "quote", options: vec!["the commas".into(), "the quotes".into()], parts: parts.iter().map(|(k, v)| (*k, v.to_string())).collect() }
}

#[test]
fn a_hole_is_filled_wherever_it_stands_and_a_word_in_braces_that_is_not_one_is_left() {
    assert_eq!(fill("{a} and {a} and {b} {c}", &[("a", "x"), ("b", "y")]), "x and x and y {c}");
}

#[test]
fn the_kept_rows_are_worded_by_the_template_in_the_order_ref_files_done_notes_question() {
    let t = template();
    let items = [
        item("question", "Which one?", &[]),
        item("note", "the lexer ends tokens at commas", &[]),
        item("done", "cargo test -p cli", &[]),
        item("file", "a.rs", &[]),
        item("file", "b.rs", &[]),
        item("ref", "x", &[("phrase", "the parser thing"), ("turn", "t3"), ("quote", "trailing commas")]),
    ];
    let refs: Vec<&Item> = items.iter().collect();
    let text = section(&t, &refs, Some(1), "fix it").unwrap();
    assert_eq!(
        text,
        "Notes on this line:\n- \"the parser thing\" is turn t3: \"trailing commas\"\n- Likely relevant: a.rs, b.rs\n- Done when `cargo test -p cli` passes\n- the lexer ends tokens at commas\n- Which one? the quotes"
    );
}

#[test]
fn a_question_nobody_answered_is_left_out_and_with_nothing_kept_there_is_no_section() {
    let t = template();
    let q = item("question", "Which one?", &[]);
    assert_eq!(section(&t, &[&q], None, "x"), None);
    assert_eq!(section(&t, &[&q], Some(9), "x"), None, "an option that is not there is no answer");
    assert_eq!(section(&t, &[], None, "x"), None);
    let n = item("note", "a note", &[]);
    assert_eq!(section(&t, &[&q, &n], None, "x").unwrap(), "Notes on this line:\n- a note");
}

#[test]
fn a_frame_goes_round_the_rows_and_the_line_has_a_hole_in_the_head() {
    let mut t = template();
    t.frame = "<context>\n{rows}\n</context>".into();
    t.head = "As typed: {line}".into();
    let n = item("note", "a note", &[]);
    assert_eq!(section(&t, &[&n], None, "fix it").unwrap(), "As typed: fix it\n<context>\n- a note\n</context>");
    t.head.clear();
    assert_eq!(section(&t, &[&n], None, "fix it").unwrap(), "<context>\n- a note\n</context>", "an empty head leaves no blank line");
}
