//! The second tier against a scripted Jev: one call for each part carrying every question that applies, 24 questions at most to
//! a call, a failed call an error and never a clean part, and the state cut in the order the spec gives.

use crate::{ready, seeds};
use j4n_qc::change::{Change, git};
use j4n_qc::judge::{self, Jev, Rubric};
use j4n_qc::part::{self, Goal};
use j4n_qc::pattern::Ready;
use serde_json::{Value as Json, json};
use std::sync::Mutex;
use std::time::Duration;

type Reply = Box<dyn Fn(&Json) -> Result<Json, String> + Send + Sync>;

struct Script {
    calls: Mutex<Vec<(Json, Json)>>,
    reply: Reply,
}

impl Jev for Script {
    fn model(&self) -> String {
        "jev-test".into()
    }
    fn zone(&self) -> String {
        "any".into()
    }
    fn batch(&self, state: &Json, questions: &Json) -> Result<Json, String> {
        self.calls.lock().unwrap().push((state.clone(), questions.clone()));
        (self.reply)(questions)
    }
}

fn script(reply: impl Fn(&Json) -> Result<Json, String> + Send + Sync + 'static) -> Script {
    Script { calls: Mutex::new(Vec::new()), reply: Box::new(reply) }
}

fn rubrics() -> Vec<Rubric> {
    [("fit", 4), ("scope", 3), ("consistency", 3)].into_iter().map(|(id, n)| Rubric { id: id.into(), doc: String::new(), instructions: format!("How is {id}?"), levels: (0..n).map(|i| format!("level {i}")).collect() }).collect()
}

fn jev_patterns() -> Vec<Ready> {
    seeds().into_iter().map(|p| Ready::new(p).unwrap()).collect()
}

fn repo_with(files: &[(&str, String)]) -> std::path::PathBuf {
    let d = crate::dir("judge");
    git(&d, &["init", "-q", "-b", "main"]).unwrap();
    for (p, t) in files {
        let full = d.join(p);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, t).unwrap();
    }
    git(&d, &["add", "."]).unwrap();
    git(&d, &["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-qm", "first"]).unwrap();
    d
}

fn answers(qs: &Json) -> Json {
    let mut out = serde_json::Map::new();
    for (id, q) in qs.as_object().unwrap() {
        let a = match q["type"].as_str().unwrap() {
            "noul" => json!({ "type": "noul", "noul": if id == "p:swallowed-error" { 0.9 } else { 0.1 } }),
            "choice" => json!({ "type": "choice", "choice": "none", "confidence": 0.9 }),
            _ => json!({ "type": "score", "score": 1.0 }),
        };
        out.insert(id.clone(), a);
    }
    json!({ "answers": out, "usage": { "input_tokens": 100, "output_tokens": 5 } })
}

fn parts_of(d: &std::path::Path, path: &str) -> (Change, Vec<(part::Part, Json)>) {
    let c = Change::of_path(d, path).unwrap();
    let states = part::cut(&c).into_iter().map(|p| { let s = part::state(&c, &p, &Goal { line: "add a parser".into(), ..Goal::default() }, &[]); (p, s) }).collect();
    (c, states)
}

#[test]
fn one_call_for_a_part_carries_every_question_that_applies_and_the_marks_and_rubrics_come_back() {
    let d = repo_with(&[("src/util.rs", "pub fn split_fields(s: &str) -> Vec<String> { vec![] }\n".into()), ("src/a.rs", "pub fn split_csv_line(s: &str) -> Vec<String> {\n    vec![]\n}\n".into())]);
    let (_c, states) = parts_of(&d, "src/a.rs");
    assert_eq!(states.len(), 1);
    let jev = script(|qs| Ok(answers(qs)));
    let (judged, errors) = judge::judge(&jev, &jev_patterns(), &rubrics(), &states, Duration::from_secs(5));
    assert!(errors.is_empty(), "{errors:?}");
    let calls = jev.calls.lock().unwrap();
    assert_eq!(calls.len(), 1, "one call for one part");
    let qs = calls[0].1.as_object().unwrap();
    // 12 yes/no questions (13 jev patterns less the choice) + the choice (the state names a look-alike) + 3 rubrics
    assert_eq!(qs.keys().filter(|k| k.starts_with("p:")).count(), 12);
    assert_eq!(qs.keys().filter(|k| k.starts_with("c:")).count(), 1);
    assert_eq!(qs.keys().filter(|k| k.starts_with("r:")).count(), 3);
    assert_eq!(qs["r:fit"]["criteria"], json!(["level 0", "level 1", "level 2", "level 3"]));
    assert!(qs["c:duplicate-helper"]["criteria"].as_object().unwrap().contains_key("none"));
    assert!(qs["c:duplicate-helper"]["criteria"].as_object().unwrap().contains_key("split_fields"), "{}", qs["c:duplicate-helper"]);
    // what came back
    let j = &judged[0];
    assert_eq!(j.marks.len(), 1);
    assert_eq!((j.marks[0].pattern.as_str(), j.marks[0].tier, j.marks[0].p), ("swallowed-error", 1, 0.9));
    assert_eq!(j.rubrics.len(), 3);
    assert_eq!(j.tokens, 105);
}

#[test]
fn a_choice_that_names_a_helper_is_a_mark_that_says_which_and_none_is_no_mark() {
    let d = repo_with(&[("src/util.rs", "pub fn split_fields(s: &str) -> Vec<String> { vec![] }\n".into()), ("src/a.rs", "pub fn split_csv_line(s: &str) -> Vec<String> {\n    vec![]\n}\n".into())]);
    let (_c, states) = parts_of(&d, "src/a.rs");
    let jev = script(|qs| {
        let mut r = answers(qs);
        r["answers"]["c:duplicate-helper"] = json!({ "type": "choice", "choice": "split_fields", "confidence": 0.8 });
        Ok(r)
    });
    let (judged, _) = judge::judge(&jev, &jev_patterns(), &rubrics(), &states, Duration::from_secs(5));
    let m = judged[0].marks.iter().find(|m| m.pattern == "duplicate-helper").unwrap();
    assert_eq!((m.answer.as_str(), m.p), ("split_fields", 0.8));
    assert_eq!(ready("duplicate-helper").steer_at("src/a.rs", 1, &m.answer), "The code at src/a.rs:1 repeats split_fields. Call it instead.");
}

#[test]
fn no_look_alike_means_no_choice_question() {
    let d = repo_with(&[("src/a.rs", "pub fn unique_thing() {}\n".into())]);
    let (_c, states) = parts_of(&d, "src/a.rs");
    assert!(judge::questions(&jev_patterns(), &rubrics(), &states[0].0, &states[0].1).iter().all(|(id, _)| !id.starts_with("c:")));
}

#[test]
fn more_than_twenty_four_questions_are_two_calls_and_a_failed_call_is_an_error_for_its_part() {
    let d = repo_with(&[("src/a.rs", "pub fn one() {}\n".into()), ("src/b.rs", "pub fn two() {}\n".into())]);
    let c = Change::of_path(&d, "src").unwrap();
    let states: Vec<(part::Part, Json)> = part::cut(&c).into_iter().map(|p| { let s = part::state(&c, &p, &Goal::default(), &[]); (p, s) }).collect();
    assert_eq!(states.len(), 2);
    // 12 patterns + 3 rubrics = 15 questions a part: one call each, and the second part's call fails
    let jev = script(|qs| if qs.as_object().unwrap().len() > 14 && qs.to_string().len() % 2 == 0 { Err("HTTP 500".into()) } else { Ok(answers(qs)) });
    let (judged, errors) = judge::judge(&jev, &jev_patterns(), &rubrics(), &states, Duration::from_secs(5));
    assert_eq!(judged.len(), 2);
    assert_eq!(judged.iter().map(|j| j.calls).sum::<usize>(), 2);
    assert!(errors.len() <= 2);
    // a pile of look-alike questions: 30 patterns of one kind need two calls for one part
    let many: Vec<Ready> = (0..30).map(|i| { let mut p = seeds().into_iter().find(|p| p.id == "swallowed-error").unwrap(); p.id = format!("q{i}"); Ready::new(p).unwrap() }).collect();
    let jev2 = script(|qs| Ok(answers(qs)));
    judge::judge(&jev2, &many, &[], &states[..1], Duration::from_secs(5));
    let sizes: Vec<usize> = jev2.calls.lock().unwrap().iter().map(|(_, q)| q.as_object().unwrap().len()).collect();
    assert_eq!(sizes.iter().sum::<usize>(), 30);
    assert!(sizes.iter().all(|n| *n <= 24) && sizes.len() == 2, "{sizes:?}");
}

#[test]
fn a_call_that_does_not_answer_in_time_is_an_error_not_a_clean_part() {
    let d = repo_with(&[("src/a.rs", "pub fn one() {}\n".into())]);
    let (_c, states) = parts_of(&d, "src/a.rs");
    let jev = script(|qs| { std::thread::sleep(Duration::from_millis(300)); Ok(answers(qs)) });
    let (judged, errors) = judge::judge(&jev, &jev_patterns(), &rubrics(), &states, Duration::from_millis(50));
    assert_eq!(errors.len(), 1);
    assert!(errors[0].1.contains("did not answer in 50 ms"), "{errors:?}");
    assert!(judged[0].marks.is_empty() && judged[0].rubrics.is_empty());
}

#[test]
fn a_part_is_sixty_changed_lines_of_one_file_and_a_bigger_hunk_is_cut_into_windows() {
    let body: String = (1..=150).map(|i| format!("let v{i} = {i};\n")).collect();
    let d = repo_with(&[("big.rs", body)]);
    let c = Change::of_path(&d, "big.rs").unwrap();
    let parts = part::cut(&c);
    assert_eq!(parts.iter().map(|p| p.added.len()).collect::<Vec<_>>(), [60, 60, 30]);
    // two hunks of 20 lines each in one file are one part
    let d2 = repo_with(&[("m.rs", (1..=100).map(|i| format!("line {i}\n")).collect())]);
    let edited: String = (1..=100).map(|i| if i <= 12 || (61..=72).contains(&i) { format!("changed {i}\n") } else { format!("line {i}\n") }).collect();
    std::fs::write(d2.join("m.rs"), edited).unwrap();
    let c2 = Change::of_task(&d2, None, &["m.rs".to_string()]);
    assert_eq!(c2.files[0].hunks.len(), 2);
    assert_eq!(part::cut(&c2).len(), 1);
}

/// A project of 120 small modules and a file of 200 lines of `width` characters, 30 of them changed: its part's state and the size of it.
fn big_state(width: usize) -> (Json, usize) {
    let mut files: Vec<(String, String)> = (0..120).map(|i| (format!("src/mod{i}/lib.rs"), format!("pub fn parse_thing_{i}() {{}}\n"))).collect();
    let line = |i: usize, tag: &str| format!("pub fn parse_item_{i}_{tag}() {{ let a = \"{}\"; }}\n", "x".repeat(width));
    files.push(("src/a.rs".into(), (1..=200).map(|i| line(i, "old")).collect()));
    let refs: Vec<(&str, String)> = files.iter().map(|(p, t)| (p.as_str(), t.clone())).collect();
    let d = repo_with(&refs);
    let edited: String = (1..=200).map(|i| if (85..115).contains(&i) { line(i, "new") } else { line(i, "old") }).collect();
    std::fs::write(d.join("src/a.rs"), edited).unwrap();
    let c = Change::of_task(&d, None, &["src/a.rs".to_string()]);
    let p = part::cut(&c).remove(0);
    let s = part::state(&c, &p, &Goal::default(), &[]);
    let n = s.to_string().len();
    (s, n)
}

#[test]
fn a_part_over_24_kb_is_cut_the_file_map_first_then_the_look_alikes_then_the_code_round() {
    // small: nothing is cut
    let (s, n) = big_state(20);
    assert!(n < part::STATE_BYTES && s["repo"]["map"].as_array().unwrap().len() == 100, "{n}");
    // find a width where the whole is over the size but the part without the file map is not
    let (mut only_map, mut all) = (None, None);
    for w in (40..400).step_by(10) {
        let (s, n) = big_state(w);
        assert!(n <= part::STATE_BYTES || s["around"]["function"].as_str().unwrap().lines().count() <= 71, "width {w}: {n} bytes left in a state that was cut as far as it goes");
        let map_cut = s["repo"]["map"].as_array().unwrap().is_empty();
        let around_lines = s["around"]["function"].as_str().unwrap().lines().count();
        if map_cut && around_lines > 71 && only_map.is_none() {
            only_map = Some((w, n));
        }
        if map_cut && around_lines <= 71 && all.is_none() {
            all = Some((w, n));
        }
    }
    let (w1, n1) = only_map.expect("a size at which only the file map goes");
    assert!(n1 <= part::STATE_BYTES, "width {w1}");
    let (w2, n2) = all.expect("a size at which the code round is cut too");
    assert!(w2 > w1 && n2 <= part::STATE_BYTES, "{w1} {w2}");
}

#[test]
fn a_chance_of_exactly_half_is_a_mark() {
    let d = repo_with(&[("src/a.rs", "pub fn one() {}\n".into())]);
    let (_c, states) = parts_of(&d, "src/a.rs");
    let jev = script(|qs| {
        let mut r = answers(qs);
        r["answers"]["p:swallowed-error"]["noul"] = json!(0.5);
        r["answers"]["p:silent-fallback"]["noul"] = json!(0.49);
        Ok(r)
    });
    let (judged, _) = judge::judge(&jev, &jev_patterns(), &rubrics(), &states, Duration::from_secs(5));
    assert_eq!(judged[0].marks.iter().map(|m| m.pattern.as_str()).collect::<Vec<_>>(), ["swallowed-error"]);
}

#[test]
fn the_parts_are_cut_at_sixty_exactly_and_the_callers_leave_out_the_part_itself() {
    // a hunk of 60 changed lines (30 replaced) is one part, and two hunks that make 60 together are one part
    let old: String = (1..=100).map(|i| format!("line {i}\n")).collect();
    let d = repo_with(&[("m.rs", old.clone())]);
    let edit = |a: std::ops::RangeInclusive<usize>| -> String { (1..=100).map(|i| if a.contains(&i) { format!("changed {i}\n") } else { format!("line {i}\n") }).collect() };
    std::fs::write(d.join("m.rs"), edit(1..=30)).unwrap();
    let c = Change::of_task(&d, None, &["m.rs".to_string()]);
    assert_eq!(c.files[0].hunks[0].changed(), 60);
    assert_eq!(part::cut(&c).iter().map(|p| p.added.len()).collect::<Vec<_>>(), [30], "60 changed lines are not cut");
    assert!(part::cut(&c)[0].diff.starts_with("@@"), "and the part is the hunk as git wrote it");
    std::fs::write(d.join("m.rs"), (1..=100).map(|i| if i <= 15 || (61..=75).contains(&i) { format!("changed {i}\n") } else { format!("line {i}\n") }).collect::<String>()).unwrap();
    let c = Change::of_task(&d, None, &["m.rs".to_string()]);
    assert_eq!((c.files[0].hunks.len(), part::cut(&c).len()), (2, 1), "30 and 30 changed lines are one part");
    // the callers of what a part defines are elsewhere
    let d = repo_with(&[("src/a.rs", "pub fn compute_total() -> u32 { 1 }\n".into()), ("src/b.rs", "fn user() { compute_total(); }\n".into())]);
    let c = Change::of_path(&d, "src/a.rs").unwrap();
    let p = part::cut(&c).remove(0);
    let s = part::state(&c, &p, &Goal::default(), &[]);
    let callers: Vec<&str> = s["around"]["callers"].as_array().unwrap().iter().map(|c| c.as_str().unwrap()).collect();
    assert_eq!(callers.len(), 1, "{callers:?}");
    assert!(callers[0].starts_with("src/b.rs:1:"), "{callers:?}");
}

#[test]
fn the_look_alikes_are_eight_at_most() {
    let mut files: Vec<(String, String)> = (0..12).map(|i| (format!("src/m{i}.rs"), format!("pub fn parse_number_{i}() {{}}\n"))).collect();
    files.push(("src/new.rs".into(), "pub fn parse_number() {}\n".into()));
    let refs: Vec<(&str, String)> = files.iter().map(|(p, t)| (p.as_str(), t.clone())).collect();
    let d = repo_with(&refs);
    let c = Change::of_path(&d, "src/new.rs").unwrap();
    let p = part::cut(&c).remove(0);
    let s = part::state(&c, &p, &Goal::default(), &[]);
    assert_eq!(s["similar"].as_array().unwrap().len(), 8);
}

#[test]
fn a_state_says_its_edges_are_lexical_only_when_tagged_and_the_entries_are_not_changed() {
    let d = repo_with(&[("src/util.rs", "pub fn split_fields(s: &str) -> Vec<String> { vec![] }\n".into()), ("src/a.rs", "pub fn split_csv_line(s: &str) -> Vec<String> {\n    vec![]\n}\n".into())]);
    let (_c, states) = parts_of(&d, "src/a.rs");
    let plain = states[0].1.clone();
    assert!(plain.get("basis").is_none(), "an untagged state is what it always was: {plain}");
    let mut tagged = plain.clone();
    part::tag(&mut tagged);
    for edge in ["callers", "tests", "similar", "map"] {
        assert_eq!(tagged["basis"][edge], "~lexical", "{edge}");
    }
    assert!(tagged["basis"]["note"].as_str().unwrap().contains("not by reading the code"));
    tagged.as_object_mut().unwrap().remove("basis");
    assert_eq!(tagged, plain, "only `basis` is added");
    assert!(plain["similar"].as_array().is_some_and(|s| !s.is_empty()), "the state had a look-alike to be sure about: {plain}");
}
