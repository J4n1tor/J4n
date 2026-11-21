//! What a compaction must not lose (`MustKeep`): its order, its bounds, and the notes that may be added and never replace.

use j4n_core::compact::{KEEP_DECISIONS, KEEP_PROMPTS, MAX_KEEP_CHARS, MustKeep, clip_notes};
use j4n_core::rule::{NOTE_CHARS, NOTES_MAX};

fn keep() -> MustKeep {
    MustKeep {
        goal: "build it".into(),
        todo: Some("2 item(s) still open".into()),
        decisions: vec!["which database? → sqlite".into()],
        changed: vec!["out/report.tsv".into(), "src/a.rs".into()],
        prompts: vec!["use sqlite".into()],
        notes: vec!["never touch vendor/".into()],
        how: None,
    }
}

#[test]
fn the_block_has_the_goal_first_and_the_notes_last() {
    let text = keep().render();
    let at = |needle: &str| text.find(needle).unwrap_or_else(|| panic!("{needle} in {text}"));
    assert!(text.starts_with("MUST KEEP (re-stated after compaction):\n"), "{text}");
    assert!(at("goal: build it") < at("todo: 2 item(s)") && at("todo:") < at("decisions:") && at("decisions:") < at("changed files:"), "{text}");
    assert!(at("changed files:") < at("the operator's last words:") && at("last words") < at("also keep:") && at("also keep:") < at("never touch vendor/"), "{text}");
    assert!(text.contains("which database? → sqlite") && text.contains("out/report.tsv, src/a.rs"));
}

#[test]
fn a_part_that_is_empty_is_left_out() {
    let text = MustKeep { goal: "g".into(), ..MustKeep::default() }.render();
    assert_eq!(text, "MUST KEEP (re-stated after compaction):\n- goal: g\n");
}

#[test]
fn each_part_is_bounded_and_the_whole_is_cut_at_a_line() {
    let long = "x".repeat(5_000);
    let text = MustKeep { goal: long.clone(), todo: Some(long.clone()), ..keep() }.render();
    assert!(text.contains(&format!("goal: {}…", "x".repeat(1_200))), "the goal is clipped at 1,200");
    assert!(text.contains(&format!("todo: {}…", "x".repeat(400))));
    let many = MustKeep { changed: (0..30).map(|i| format!("dir/{i}/{}", "y".repeat(240))).collect(), ..keep() };
    let text = many.render();
    assert!(text.chars().count() <= MAX_KEEP_CHARS, "{} characters", text.chars().count());
    assert!(text.ends_with('…') && text.lines().all(|l| !l.is_empty()), "cut at a line end, and says so");
    assert!(text.contains("goal: build it") && text.contains("todo:"), "what cannot be rebuilt came first, so it survived the cut");
}

#[test]
fn files_beyond_thirty_are_counted_and_only_the_last_few_words_of_the_operator_are_kept() {
    let m = MustKeep { changed: (0..45).map(|i| format!("f{i}")).collect(), prompts: (0..9).map(|i| format!("say {i}")).collect(), ..keep() };
    let text = m.render();
    assert!(text.contains("f29") && !text.contains("f30,") && text.contains("(+15 more)"), "{text}");
    let said: Vec<&str> = text.lines().filter(|l| l.starts_with("  · say")).collect();
    assert_eq!(said.len(), KEEP_PROMPTS, "{text}");
    assert_eq!(said.last(), Some(&"  · say 8"), "the newest are the ones kept");
}

#[test]
fn only_the_latest_answers_are_kept() {
    let m = MustKeep { decisions: (0..11).map(|i| format!("q{i} → a")).collect(), ..keep() };
    let text = m.render();
    let kept: Vec<&str> = text.lines().filter(|l| l.starts_with("  · q")).collect();
    assert_eq!(kept.len(), KEEP_DECISIONS, "{text}");
    assert_eq!((kept[0], kept[KEEP_DECISIONS - 1]), ("  · q3 → a", "  · q10 → a"), "the newest are the ones kept");
}

#[test]
fn notes_are_single_lines_clipped_and_few() {
    let notes = clip_notes(vec!["  two\n lines   here ".into(), "".into(), "   ".into(), "z".repeat(NOTE_CHARS + 50)]);
    assert_eq!(notes[0], "two lines here");
    assert_eq!(notes.len(), 2, "empty ones are dropped");
    assert_eq!(notes[1].chars().count(), NOTE_CHARS + 1, "clipped, with the mark");
    let many = clip_notes((0..20).map(|i| format!("n{i}")).collect());
    assert_eq!(many.len(), NOTES_MAX);
    assert_eq!(many[0], "n0");
}
