//! The arguments of a connector, from the words of its manifest.

use j4n_connect::template::{Vars, names, render};

fn v(pairs: &[(&str, &str)]) -> Vars {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn t(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| s.to_string()).collect()
}

#[test]
fn names_are_found_in_order() {
    assert_eq!(names("--x={a}{b} {c"), ["a", "b"]);
    assert!(names("{} {a b}").is_empty());
}

#[test]
fn a_value_fills_its_place_and_a_missing_one_takes_its_flag_with_it() {
    let tpl = t(&["run", "--model", "{model}", "{prompt}"]);
    assert_eq!(render(&tpl, &v(&[("prompt", "hi"), ("model", "m1")])).unwrap(), ["run", "--model", "m1", "hi"]);
    assert_eq!(render(&tpl, &v(&[("prompt", "hi")])).unwrap(), ["run", "hi"]);
}

#[test]
fn a_value_that_would_be_a_flag_is_refused_but_the_prompt_is_only_spaced() {
    let tpl = t(&["--model", "{model}", "{prompt}"]);
    assert!(render(&tpl, &v(&[("model", "--evil"), ("prompt", "x")])).is_err());
    assert_eq!(render(&tpl, &v(&[("model", "m"), ("prompt", "- one")])).unwrap(), ["--model", "m", " - one"]);
    assert_eq!(render(&t(&["--m={model}"]), &v(&[("model", "-x")])).unwrap(), ["--m=-x"], "inside an argument it is no flag");
}
