//! Ask points as data: the kind `ask_point`, the base points file, the two text parsers and the lint.

use crate::world::*;
use j4n_core::answer::Impact::{Gate, Low, Mid};
use j4n_core::answer::ShapeKind::{Approval, Choice, Text, YesNo};
use j4n_core::answer::point::{lint, options, parse_fallback, parse_step, points};
use j4n_core::answer::{AskPoint, Fallback, Impact, Kind, ShapeKind, Step};
use j4n_core::ask::{Answer, AskShape, Opt};
use j4n_core::schema::DataClass;
use j4n_core::table::{Layer, LayerKind, Level, Problem, Tables};
use std::path::Path;

fn base_asks() -> Layer {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/base/asks");
    let (layer, read) = Layer::from_dir(LayerKind::Pack, "base", &dir);
    assert!(read.is_empty(), "{read:?}");
    layer
}

fn base() -> Tables {
    let (t, problems) = load(vec![base_asks()]);
    assert!(problems.is_empty(), "{problems:?}");
    t
}

/// The base points and one more file in the operator's layer.
fn with_home(text: &str) -> (Tables, Vec<Problem>) {
    load(vec![base_asks(), home(&[("mine.toml", text)])])
}

/// One user-layer entry of the kind, the fields as given after `id`.
fn entry(id: &str, fields: &str) -> String {
    format!("api = 1\n[[ask_point]]\nid = \"{id}\"\n{fields}\n")
}

fn texts(problems: &[Problem]) -> String {
    problems.iter().map(|p| format!("{p}")).collect::<Vec<_>>().join("\n")
}

fn step(s: &str) -> Step {
    parse_step(s).unwrap_or_else(|e| panic!("{s}: {e}"))
}

fn pt(shape: ShapeKind, impact: Impact, chain: &[&str], fallback: &str) -> AskPoint {
    AskPoint {
        id: "p".into(),
        doc: String::new(),
        shape,
        impact,
        data: DataClass::Project,
        chain: chain.iter().map(|s| step(s)).collect(),
        fallback: parse_fallback(fallback).unwrap(),
        rank: None,
        breaker: true,
    }
}

/// What the lint says of a point, as one text ("" when clean).
fn says(p: &AskPoint) -> String {
    texts(&lint(std::slice::from_ref(p)))
}

fn clean(p: &AskPoint) {
    let found = says(p);
    assert!(found.is_empty(), "{found}");
}

fn refused_with(p: &AskPoint, word: &str) {
    let found = says(p);
    assert!(found.contains(word), "wanted `{word}` in: {found:?}");
}

// ---- the kind and the file ----

#[test]
fn the_kind_is_declared_and_its_examples_load() {
    let (t, problems) = load(vec![]);
    assert!(problems.is_empty(), "{problems:?}");
    let s = t.schema("ask_point").unwrap();
    for f in ["doc", "shape", "impact", "data", "chain", "fallback", "rank", "options"] {
        assert!(s.fields.contains_key(f), "{f}");
    }
    assert!(s.fields["fallback"].required && s.fields["shape"].required && s.fields["impact"].required);
    assert!(!s.fields["chain"].required && !s.fields["rank"].required && !s.fields["options"].required);
    for f in ["shape", "impact", "data", "options"] {
        assert!(s.fields[f].floor, "{f} is a floor");
    }
}

#[test]
fn the_points_file_loads_clean_with_the_core_kinds_and_every_point_lints_clean() {
    let t = base();
    let (ps, problems) = points(&t);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(ps.len(), t.ids("ask_point").len(), "no point is dropped");
    let ids: Vec<&str> = ps.iter().map(|p| p.id.as_str()).collect();
    for want in [
        "intake",
        "who:work",
        "who:plan",
        "who:review",
        "who:validate",
        "pick",
        "retry",
        "approval",
        "question",
    ] {
        assert!(ids.contains(&want), "{want} is declared");
    }
    assert!(lint(&ps).is_empty());
    assert!(ps.iter().all(|p| !p.doc.is_empty()));
}

#[test]
fn the_base_points_hold_their_chains_and_impacts() {
    let t = base();
    let (ps, _) = points(&t);
    let get = |id: &str| ps.iter().find(|p| p.id == id).unwrap();
    let chain = |id: &str| get(id).chain.iter().map(|s| s.to_string()).collect::<Vec<_>>();

    let intake = get("intake");
    assert_eq!((intake.shape, intake.impact), (Choice, Mid));
    assert_eq!(chain("intake"), ["rule:intake", "decider:jev?min_conf=0.6", "human"]);
    assert_eq!(intake.fallback, Fallback::Pick("task".into()));
    assert_eq!(options(&t, "intake"), ["answer", "command", "setting", "task", "flow"]);

    for role in ["work", "plan", "review", "validate"] {
        let p = get(&format!("who:{role}"));
        assert_eq!((p.shape, p.fallback.clone()), (Choice, Fallback::Rank), "who:{role}");
        let named = role != "validate";
        let want: &[&str] = if named { &["rule:named", "decider:jev?min_conf=0.6", "rank", "human?after=60s"] } else { &["rank", "human?after=60s"] };
        assert_eq!(chain(&p.id), want, "who:{role}");
        assert_eq!(p.chain.last().unwrap().after_ms, Some(60_000));
    }

    let approval = get("approval");
    assert_eq!((approval.shape, approval.impact, approval.fallback.clone()), (Approval, Gate, Fallback::Safe));
    assert_eq!(chain("approval"), ["rule:gate", "human"]);
    assert!(approval.chain.iter().all(|s| !s.kind.is_model()), "an approval never reaches a model");

    assert_eq!(chain("question"), ["human"]);
    assert!(matches!(get("question").fallback, Fallback::Text(_)));
    assert_eq!(get("retry").fallback, Fallback::Pick("stop".into()));
    assert_eq!(options(&t, "retry"), ["retry", "escalate", "stop"]);
    assert_eq!(options(&t, "who:work"), Vec::<String>::new());
    assert_eq!(options(&t, "nothing-like-it"), Vec::<String>::new());
    assert!(chain("pick").starts_with(&["rule:recommended".to_string()]));
    assert!(get("pick").chain.last().unwrap().kind == Kind::Human);
}

/// What a keyless run answers: the fallback alone, against a question of the point's shape.
fn keyless(t: &Tables, p: &AskPoint) -> (AskShape, Answer) {
    let declared = options(t, &p.id);
    let ids: Vec<String> = if declared.is_empty() { vec!["alpha".into(), "none".into()] } else { declared };
    let opts: Vec<Opt> = ids.iter().map(|id| Opt { id: id.clone(), label: id.clone(), recommended: false }).collect();
    let shape = match p.shape {
        YesNo => AskShape::YesNo,
        Choice => AskShape::Choice { options: opts },
        Approval => AskShape::Approval { action: "an effect".into(), risk: 50 },
        Text => AskShape::Text,
    };
    let answer = match (&p.fallback, &shape) {
        (Fallback::Rank, AskShape::Choice { options }) => Answer::Pick { id: options[0].id.clone() },
        (Fallback::Safe, AskShape::Choice { .. }) => Answer::Pick { id: "safe".into() },
        (Fallback::Safe, _) => Answer::No,
        (Fallback::Pick(id), _) => Answer::Pick { id: id.clone() },
        (Fallback::Text(line), _) => Answer::Text { text: line.clone() },
        (Fallback::Rank, _) => unreachable!("the lint keeps rank to choices"),
    };
    (shape, answer)
}

#[test]
fn a_keyless_run_still_has_an_answer_for_every_point() {
    let t = base();
    let (ps, _) = points(&t);
    for p in &ps {
        let (shape, answer) = keyless(&t, p);
        assert_eq!(shape.check(&answer), Ok(()), "{}: {answer:?}", p.id);
    }
}

// ---- parse_step ----

#[test]
fn a_step_reads_each_form_the_plan_writes() {
    let s = step("rule:route");
    assert_eq!((s.kind, s.name.as_str(), s.min_conf, s.after_ms), (Kind::Rule, "route", None, None));
    let s = step("rank");
    assert_eq!((s.kind, s.name.as_str(), s.id().as_str()), (Kind::Rank, "", "rank"));
    let s = step("decider:jev?min_conf=0.6");
    assert_eq!(
        (s.kind, s.name.as_str(), s.min_conf, s.id().as_str()),
        (Kind::Decider, "jev", Some(0.6), "decider:jev")
    );
    let s = step("model:light");
    assert_eq!((s.kind, s.name.as_str()), (Kind::Model, "light"));
    let s = step("human?after=60s");
    assert_eq!((s.kind, s.after_ms), (Kind::Human, Some(60_000)));
    assert_eq!(step("human").after_ms, None);
    assert_eq!(step("human?after=1h30m").after_ms, Some(5_400_000));
    assert_eq!(step("human?after=800ms").after_ms, Some(800));
    let s = step("model:x/y.z-1_2:3?after=2s&min_conf=1");
    assert_eq!((s.name.as_str(), s.after_ms, s.min_conf), ("x/y.z-1_2:3", Some(2000), Some(1.0)));
}

#[test]
fn a_step_that_is_not_one_is_refused_with_a_reason() {
    for bad in [
        "",
        "?",
        ":",
        "oracle:x",
        "Rule:x",
        "RANK",
        " rank",
        "rank ",
        "rule",
        "rule:",
        "rule:?min_conf=0.5",
        "rule:a b",
        "rule:-a",
        "rank:x",
        "rank:",
        "human:me",
        "human?",
        "human?after",
        "human?after=",
        "human?after=60",
        "human?after=0s",
        "human?after=-1s",
        "human?after=5 s",
        "human?after=1y",
        "human?after=99999999999999999999h",
        "human?after=1s&after=2s",
        "human?bogus=1",
        "human?after=1s&",
        "human?&after=1s",
        "decider:x?min_conf=",
        "decider:x?min_conf=abc",
        "decider:x?min_conf=nan",
        "decider:x?min_conf=inf",
        "decider:x?min_conf=-inf",
        "decider:x?min_conf=0.5&min_conf=0.6",
        "decider:x?min_conf=0.5?after=1s",
        "decider:x?Min_conf=0.5",
        "decider:x?min_conf=0,5",
        "decider:x?min_conf=1_0",
        "rule:a\u{0}b",
        "rule:é",
        "rule:a\nb",
        "\u{0}",
    ] {
        let e = parse_step(bad).expect_err(bad);
        assert!(!e.is_empty() && e.chars().count() < 200, "{bad:?}: {e}");
    }
}

#[test]
fn reading_a_step_does_not_judge_it_the_lint_does() {
    assert_eq!(step("decider:x?min_conf=7").min_conf, Some(7.0));
    assert_eq!(step("decider:x?min_conf=0").min_conf, Some(0.0));
    assert_eq!(step("rank?after=5s").after_ms, Some(5000));
}

// ---- parse_fallback ----

#[test]
fn a_fallback_reads_each_form_the_plan_writes() {
    assert_eq!(parse_fallback("rank"), Ok(Fallback::Rank));
    assert_eq!(parse_fallback("safe"), Ok(Fallback::Safe));
    assert_eq!(parse_fallback("pick:task"), Ok(Fallback::Pick("task".into())));
    assert_eq!(parse_fallback("pick:a:b"), Ok(Fallback::Pick("a:b".into())));
    assert_eq!(parse_fallback("pick:é日"), Ok(Fallback::Pick("é日".into())));
    assert_eq!(parse_fallback("text:No one answered."), Ok(Fallback::Text("No one answered.".into())));
    assert_eq!(parse_fallback("text:  trimmed  "), Ok(Fallback::Text("trimmed".into())));
    assert_eq!(parse_fallback("text:a: b ? c"), Ok(Fallback::Text("a: b ? c".into())));
}

#[test]
fn a_fallback_that_is_not_one_is_refused_with_a_reason() {
    let long = format!("text:{}", "x".repeat(241));
    let long_id = format!("pick:{}", "i".repeat(65));
    for bad in [
        "",
        "none",
        "Rank",
        "SAFE",
        " safe",
        "safe ",
        "rank:x",
        "safe:x",
        "pick",
        "pick:",
        "pick: a",
        "pick:a b",
        "pick:a\tb",
        "pick:a\u{0}",
        "pick:\n",
        "text",
        "text:",
        "text:   ",
        "text:two\nlines",
        "text:tab\there",
        "text:nul\u{0}",
        "text:bell\u{7}",
        "oracle:x",
        ":",
        "::",
        &long,
        &long_id,
    ] {
        let e = parse_fallback(bad).expect_err(bad);
        assert!(!e.is_empty() && e.chars().count() < 200, "{bad:?}: {e}");
    }
    assert!(parse_fallback(&format!("text:{}", "x".repeat(240))).is_ok(), "240 characters is the limit");
    assert!(parse_fallback(&format!("pick:{}", "i".repeat(64))).is_ok());
}

#[test]
fn the_text_forms_round_trip() {
    for s in [
        "rule:route",
        "rank",
        "decider:jev?min_conf=0.6",
        "model:light",
        "human",
        "human?after=60s",
        "human?after=1500ms",
        "decider:a/b?min_conf=0.05&after=2s",
        "model:x?min_conf=1",
    ] {
        let parsed = step(s);
        assert_eq!(step(&parsed.to_string()), parsed, "{s}");
    }
    assert_eq!(step("human?after=1m").to_string(), "human?after=60s");
    assert_eq!(step("human?after=1500ms").to_string(), "human?after=1500ms");
    for s in ["rank", "safe", "pick:task", "text:No one answered: go on."] {
        let parsed = parse_fallback(s).unwrap();
        assert_eq!(parsed.to_string(), s);
        assert_eq!(parse_fallback(&parsed.to_string()), Ok(parsed));
    }
    // every point of the file, written out and read back, is the same point
    let (ps, _) = points(&base());
    for p in &ps {
        for s in &p.chain {
            assert_eq!(step(&s.to_string()), *s);
        }
        assert_eq!(parse_fallback(&p.fallback.to_string()), Ok(p.fallback.clone()));
    }
}

#[test]
fn whatever_the_reader_accepts_it_reads_back_the_same() {
    // a small generator of strings near the grammar: if the reader takes one, writing and reading again is the identity
    let parts = [
        "rule", "rank", "decider", "model", "human", ":", "?", "&", "=", "min_conf", "after", "0.5", "1", "60s", "x",
        "-", "é", " ", "\u{0}", "pick", "text", "safe",
    ];
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    let (mut steps, mut fallbacks) = (0, 0);
    for _ in 0..40_000 {
        let mut s = String::new();
        for _ in 0..(1 + seed % 7) {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            s.push_str(parts[(seed >> 33) as usize % parts.len()]);
        }
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        if let Ok(parsed) = parse_step(&s) {
            steps += 1;
            assert_eq!(parse_step(&parsed.to_string()), Ok(parsed), "{s:?}");
        }
        if let Ok(parsed) = parse_fallback(&s) {
            fallbacks += 1;
            assert_eq!(parse_fallback(&parsed.to_string()), Ok(parsed), "{s:?}");
        }
    }
    assert!(steps > 100 && fallbacks > 100, "the generator reaches the grammar ({steps}, {fallbacks})");
}

#[test]
fn hostile_text_is_refused_fast_and_never_echoed_whole() {
    let started = crate::cpu::start();
    let big = "a".repeat(1_000_000);
    let huge = [
        format!("rule:{big}"),
        format!("{big}:x"),
        format!("decider:x?min_conf={big}"),
        format!("human?after={big}"),
        format!("human?{}", "&".repeat(1_000_000)),
        format!("human?{}", "after=1s&".repeat(100_000)),
        format!("rank{}", "?".repeat(1_000_000)),
        "\u{0}".repeat(100_000),
        "é".repeat(100_000),
        format!("decider:x?min_conf={}", "9".repeat(100_000)),
    ];
    for s in &huge {
        let e = parse_step(s).expect_err("refused");
        assert!(e.chars().count() < 300, "an error does not repeat the input");
        assert!(parse_fallback(s).is_err());
    }
    for f in [format!("pick:{big}"), format!("text:{big}"), format!("text:\n{big}"), big.clone()] {
        let e = parse_fallback(&f).expect_err("refused");
        assert!(e.chars().count() < 300);
    }
    // a control character is shown escaped, not carried into the message
    let e = parse_step("bad\u{1b}[31mkind").unwrap_err();
    assert!(!e.contains('\u{1b}'), "{e:?}");
    crate::cpu::assert_within(std::time::Duration::from_secs(5), "linear time", || started.elapsed());
}

// ---- the lint, one failing fixture for every rule ----

#[test]
fn a_clean_point_of_every_shape_passes() {
    clean(&pt(YesNo, Low, &["rule:x", "decider:d?min_conf=0.5", "human"], "safe"));
    clean(&pt(Choice, Mid, &["rank", "model:m?min_conf=1", "human?after=1s"], "rank"));
    clean(&pt(Choice, Low, &[], "pick:a"));
    clean(&pt(Text, Low, &["human"], "text:go on"));
    clean(&pt(Approval, Gate, &["rule:gate", "human"], "safe"));
    clean(&pt(Choice, Gate, &["rule:a", "decider:b", "human"], "safe"));
    clean(&pt(YesNo, Low, &["rule:x"], "safe"));
}

#[test]
fn the_lint_refuses_human_that_is_not_last_and_a_step_after_human() {
    refused_with(&pt(Choice, Low, &["human", "rank"], "rank"), "last step");
    refused_with(&pt(Choice, Low, &["human", "rule:x"], "rank"), "last step");
    refused_with(&pt(Choice, Low, &["rank", "human?after=1s", "human"], "rank"), "last step");
    refused_with(&pt(YesNo, Low, &["rule:r", "human", "model:m"], "safe"), "last step");
    clean(&pt(Choice, Low, &["human"], "rank"));
}

#[test]
fn the_lint_refuses_a_model_in_front_of_an_approval() {
    for model in ["decider:jev", "model:light", "decider:jev?min_conf=0.9", "model:x?min_conf=1"] {
        refused_with(&pt(Approval, Gate, &["rule:gate", model, "human"], "safe"), "never reaches a model");
        refused_with(&pt(Approval, Low, &[model], "safe"), "never reaches a model");
    }
    refused_with(&pt(Approval, Gate, &["rule:gate", "rank", "human"], "safe"), "orders the options");
    clean(&pt(Approval, Low, &["rule:gate", "rule:grant"], "safe"));
    clean(&pt(YesNo, Low, &["decider:jev"], "safe"));
}

#[test]
fn the_lint_refuses_an_approval_whose_fallback_is_not_safe() {
    for f in ["rank", "pick:yes", "text:ok"] {
        refused_with(&pt(Approval, Low, &["human"], f), "approval");
        refused_with(&pt(Approval, Gate, &["rule:g", "human"], f), "approval");
    }
}

#[test]
fn the_lint_refuses_a_gate_point_that_does_not_end_with_human_or_fall_back_to_safe() {
    refused_with(&pt(Choice, Gate, &["rule:a", "decider:b", "human"], "rank"), "gate-impact");
    refused_with(&pt(Choice, Gate, &["rule:a", "human"], "pick:a"), "gate-impact");
    refused_with(&pt(YesNo, Gate, &["rule:a", "decider:b"], "safe"), "gate-impact");
    refused_with(&pt(YesNo, Gate, &[], "safe"), "gate-impact");
    refused_with(&pt(YesNo, Gate, &["human", "rule:a"], "safe"), "gate-impact");
    refused_with(&pt(Text, Gate, &["human"], "text:x"), "gate-impact");
    clean(&pt(YesNo, Gate, &["rule:a", "human?after=30s"], "safe"));
    clean(&pt(YesNo, Mid, &["rule:a", "decider:b"], "safe"));
}

#[test]
fn the_lint_refuses_min_conf_outside_zero_to_one() {
    for bad in ["0", "0.0", "-0.5", "1.0001", "2", "100", "-1"] {
        refused_with(&pt(YesNo, Low, &[&format!("decider:x?min_conf={bad}")], "safe"), "min_conf");
    }
    for ok in ["0.0001", "0.5", "1", "1.0"] {
        clean(&pt(YesNo, Low, &[&format!("decider:x?min_conf={ok}")], "safe"));
    }
    // a step built by hand with a value the reader would never produce
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -0.0] {
        let mut p = pt(YesNo, Low, &["rule:x"], "safe");
        p.chain[0].min_conf = Some(bad);
        refused_with(&p, "min_conf");
    }
}

#[test]
fn the_lint_refuses_after_on_a_step_that_is_not_human() {
    for s in ["rule:x?after=5s", "rank?after=5s", "decider:x?after=5s", "model:x?after=1s&min_conf=0.5"] {
        refused_with(&pt(Choice, Low, &[s], "rank"), "only `human` has `after`");
    }
    clean(&pt(Choice, Low, &["human?after=5s"], "rank"));
}

#[test]
fn the_lint_refuses_a_fallback_that_cannot_answer_the_shape() {
    for shape in [YesNo, Approval, Text] {
        refused_with(&pt(shape, Low, &["human"], "rank"), "cannot answer");
        refused_with(&pt(shape, Low, &["human"], "pick:a"), "cannot answer");
    }
    for shape in [YesNo, Approval, Choice] {
        refused_with(&pt(shape, Low, &["human"], "text:hello"), "cannot answer");
    }
    refused_with(&pt(Text, Low, &["human"], "safe"), "cannot answer");
    refused_with(&pt(YesNo, Low, &["rank"], "safe"), "orders the options");
    refused_with(&pt(Text, Low, &["rank"], "text:x"), "orders the options");
    clean(&pt(Choice, Low, &["rank"], "safe"));
}

#[test]
fn the_lint_reports_each_point_by_id_as_refused_and_says_every_reason() {
    let mut a = pt(Approval, Gate, &["human", "model:m?after=1s&min_conf=3"], "rank");
    a.id = "bad:one".into();
    let b = pt(YesNo, Low, &["rule:x"], "safe");
    let found = lint(&[a, b]);
    assert!(found.len() >= 5, "{found:?}");
    assert!(found.iter().all(|p| p.level == Level::Refused && p.at == "[[ask_point]] bad:one"));
    assert!(lint(&[]).is_empty());
}

// ---- through the Table: refusals, options, layers ----

#[test]
fn a_point_without_a_fallback_is_not_loaded() {
    let (t, problems) = with_home(&entry("loose", "shape = \"yesno\"\nimpact = \"low\"\nchain = [\"human\"]"));
    assert!(texts(&problems).contains("fallback"), "{problems:?}");
    let (ps, found) = points(&t);
    assert!(ps.iter().all(|p| p.id != "loose") && found.is_empty());
    // an empty fallback is the same thing
    let (t, _) = with_home(&entry("empty", "shape = \"yesno\"\nimpact = \"low\"\nfallback = \"\""));
    let (ps, found) = points(&t);
    assert!(ps.iter().all(|p| p.id != "empty"));
    assert!(texts(&found).contains("fallback"), "{found:?}");
}

#[test]
fn an_unknown_step_kind_or_a_bad_step_leaves_the_point_out_and_names_the_step() {
    let (t, problems) = with_home(&entry(
        "odd",
        "shape = \"yesno\"\nimpact = \"low\"\nfallback = \"safe\"\nchain = [\"rule:ok\", \"oracle:x\", \"human?after=zzz\"]",
    ));
    assert!(problems.is_empty(), "the Table holds strings: {problems:?}");
    let (ps, found) = points(&t);
    assert!(ps.iter().all(|p| p.id != "odd"));
    let text = texts(&found);
    assert!(text.contains("chain step 2") && text.contains("chain step 3") && !text.contains("chain step 1"), "{text}");
    assert!(found.iter().all(|p| p.level == Level::Refused && p.at == "[[ask_point]] odd"));
    assert_eq!(ps.len(), 25, "the others still load");
}

#[test]
fn says_gives_each_declared_option_one_line_and_a_line_for_another_option_leaves_the_point_out() {
    let fields = |says: &str| format!("shape = \"choice\"\nimpact = \"low\"\nfallback = \"pick:a\"\noptions = [\"a\", \"b\"]\nsays = {says}");
    let loads = |says: &str| {
        let (t, _) = with_home(&entry("c", &fields(says)));
        let (ps, found) = points(&t);
        (ps.iter().any(|p| p.id == "c"), texts(&found))
    };
    assert!(loads("{ a = \"the first\", b = \"the second\" }").0);
    let (ok, why) = loads("{ a = \"the first\", z = \"no such option\" }");
    assert!(!ok && why.contains("`says` describes `z`, which is not one of the options"), "{why}");
    let (ok, why) = loads(&format!("{{ a = \"{}\" }}", "long ".repeat(60)));
    assert!(!ok && why.contains("`says.a` is one short line"), "{why}");
    let (ok, why) = loads("{ a = \"  \" }");
    assert!(!ok && why.contains("`says.a` is one short line"), "{why}");
    let (t, _) = with_home(&entry("c", &fields("{ a = \"the first\" }")));
    assert_eq!(j4n_core::answer::point::says(&t)["c"]["a"], "the first");
}

#[test]
fn an_options_list_is_checked_against_the_fallback() {
    let fields =
        |opts: &str, fb: &str| format!("shape = \"choice\"\nimpact = \"low\"\nfallback = \"{fb}\"\noptions = {opts}");
    let loads = |opts: &str, fb: &str| {
        let (t, _) = with_home(&entry("c", &fields(opts, fb)));
        let (ps, found) = points(&t);
        (ps.iter().any(|p| p.id == "c"), texts(&found))
    };
    assert!(loads("[\"a\", \"b\"]", "pick:a").0);
    assert!(loads("[\"a\", \"safe\"]", "safe").0);
    assert!(loads("[\"a\", \"b\"]", "rank").0);
    let (ok, why) = loads("[\"a\", \"b\"]", "pick:c");
    assert!(!ok && why.contains("`pick:c` is not one of the options"), "{why}");
    let (ok, why) = loads("[\"a\", \"b\"]", "safe");
    assert!(!ok && why.contains("`safe` needs an option"), "{why}");
    for dup in ["[\"a\", \"a\"]", "[\"a\", \"\"]", "[\"a\\tb\"]"] {
        let (ok, why) = loads(dup, "pick:a");
        assert!(!ok && why.contains("`options`"), "{dup}: {why}");
    }
    let many = format!("[{}]", (0..13).map(|i| format!("\"o{i}\"")).collect::<Vec<_>>().join(", "));
    assert!(!loads(&many, "rank").0, "more than the most one question offers");
    // options on a shape that is not a choice
    let (t, _) = with_home(&entry("y", "shape = \"yesno\"\nimpact = \"low\"\nfallback = \"safe\"\noptions = [\"a\"]"));
    assert!(points(&t).0.iter().all(|p| p.id != "y"));
    // no options declared: any pick is the asker's to offer
    let (t, _) = with_home(&entry("free", "shape = \"choice\"\nimpact = \"low\"\nfallback = \"pick:whatever\""));
    assert!(points(&t).0.iter().any(|p| p.id == "free"));
}

#[test]
fn a_layer_cannot_loosen_a_floor_of_a_point() {
    let (t, problems) = with_home(&entry("approval", "impact = \"low\"\nshape = \"yesno\"\ndata = \"public\""));
    assert!(problems.iter().all(|p| p.level == Level::Warn) && problems.len() == 3, "{problems:?}");
    let (ps, found) = points(&t);
    assert!(found.is_empty());
    let a = ps.iter().find(|p| p.id == "approval").unwrap();
    assert_eq!((a.impact, a.shape, a.data), (Gate, Approval, DataClass::Private));
}

#[test]
fn a_layer_that_lets_a_model_see_an_approval_removes_the_point_not_the_lint() {
    let (t, _) = with_home(&entry("approval", "chain = [\"rule:gate\", \"decider:jev\", \"human\"]"));
    let (ps, found) = points(&t);
    assert!(ps.iter().all(|p| p.id != "approval"), "a refused point is not used");
    assert!(texts(&found).contains("never reaches a model"), "{found:?}");
    let (t, _) = with_home(&entry("approval", "fallback = \"rank\""));
    let (ps, found) = points(&t);
    assert!(ps.iter().all(|p| p.id != "approval") && !found.is_empty());
    let (t, _) = with_home(&entry("approval", "chain = [\"rule:gate\"]"));
    assert!(points(&t).0.iter().all(|p| p.id != "approval"), "a gate point without a human at the end");
}

#[test]
fn a_layer_may_change_the_chain_and_fallback_where_the_lint_allows() {
    let (t, problems) = with_home(&entry("who:work", "chain = [\"rank\"]\nfallback = \"safe\""));
    assert!(problems.is_empty(), "{problems:?}");
    let (ps, found) = points(&t);
    assert!(found.is_empty());
    let w = ps.iter().find(|p| p.id == "who:work").unwrap();
    assert_eq!((w.chain.len(), &w.fallback), (1, &Fallback::Safe));
    let (t, _) =
        with_home(&entry("mine", "shape = \"text\"\nimpact = \"low\"\nfallback = \"text:later\"\nrank = \"nothing\""));
    let (ps, _) = points(&t);
    assert_eq!(ps.iter().find(|p| p.id == "mine").unwrap().rank.as_deref(), Some("nothing"));
}

#[test]
fn a_project_never_declares_or_changes_a_point() {
    let mine = entry("who:work", "chain = [\"human\"]\nfallback = \"safe\"");
    let extra = entry("sneaky", "shape = \"yesno\"\nimpact = \"low\"\nfallback = \"safe\"");
    let layers = vec![base_asks(), project(&[("a.toml", &mine), ("b.toml", &extra)])];
    let (t, problems) = Tables::load(layers);
    assert!(problems.iter().all(|p| p.level == Level::Warn) && !problems.is_empty());
    let (ps, found) = points(&t);
    assert!(found.is_empty() && ps.iter().all(|p| p.id != "sneaky"));
    assert_eq!(ps.iter().find(|p| p.id == "who:work").unwrap().chain.len(), 4, "the base chain stands");
}

#[test]
fn hostile_text_in_every_field_of_an_entry_never_panics_and_is_reported() {
    use toml::Value as V;
    let long = "x".repeat(300);
    let hostile = [
        "\u{0}",
        "\u{1b}[2J",
        "a\nb",
        "é日本\u{202e}語",
        "'; drop table",
        "{{x}}",
        "$(rm -rf /)",
        "../../etc/passwd",
        &long,
        "",
        "human?after=1s&min_conf=0.5",
        "pick:a b",
    ];
    let s = |x: &str| V::String(x.to_string());
    let mut rows = Vec::new();
    for (i, h) in hostile.iter().enumerate() {
        let mut row = toml::Table::new();
        row.insert("id".into(), s(&format!("h{i}")));
        row.insert("doc".into(), s(h));
        row.insert("shape".into(), s("choice"));
        row.insert("impact".into(), s("low"));
        row.insert("data".into(), s("public"));
        row.insert("chain".into(), V::Array(vec![s(h), s(&format!("rule:{h}"))]));
        row.insert("fallback".into(), s(h));
        row.insert("options".into(), V::Array(vec![s(h)]));
        rows.push(V::Table(row));
    }
    let mut file = toml::Table::new();
    file.insert("api".into(), V::Integer(1));
    file.insert("ask_point".into(), V::Array(rows));
    let text = toml::to_string(&file).unwrap();
    let (t, table) = load(vec![base_asks(), home(&[("h.toml", &text)])]);
    assert!(!table.is_empty(), "the Table already refuses the ones with control characters");
    let (ps, found) = points(&t);
    assert!(ps.iter().all(|p| !p.id.starts_with('h')), "none of them is a point");
    assert_eq!(ps.len(), 25);
    assert!(!found.is_empty(), "the ones the Table let through are reported here");
    for p in &found {
        assert!(p.msg.chars().count() < 400, "{}", p.msg);
        assert!(!p.msg.contains('\u{1b}') && !p.at.contains('\u{0}'));
    }
    for id in t.ids("ask_point") {
        let _ = options(&t, id);
    }
}

#[test]
fn a_thousand_points_load_and_lint_quickly() {
    let mut text = String::from("api = 1\n");
    for i in 0..1000 {
        text.push_str(&format!(
            "[[ask_point]]\nid = \"p{i}\"\nshape = \"choice\"\nimpact = \"mid\"\nchain = [\"rule:a\", \"decider:b?min_conf=0.5\", \"human?after=9s\"]\nfallback = \"rank\"\n"
        ));
    }
    let (t, problems) = load(vec![home(&[("many.toml", &text)])]);
    assert!(problems.is_empty(), "{problems:?}");
    let started = crate::cpu::start();
    let (ps, found) = points(&t);
    assert_eq!((ps.len(), found.len()), (1000, 0));
    crate::cpu::assert_within(std::time::Duration::from_millis(1500), "a thousand points", || started.elapsed());
}
