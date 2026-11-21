//! Rules (the kind `rule`) without a loop: what loads and what is refused, what a rule says to an event, and what keeps it
//! from saying it again and again — limits, cooldowns, waits that start over, the depth of a chain, the rate.

use crate::fold_cases::event;
use crate::scripted::base_tables;
use crate::world::*;
use j4n_core::fold::Folds;
use j4n_core::log::Ev;
use j4n_core::rule::{Act, DEPTH_MAX, Do, Due, KEYS_MAX, RATE_MAX, RATE_WINDOW_MS, RuleEnv, Rules, State, TEXT_MAX};
use j4n_core::table::{LayerKind, Tables};
use j4n_core::value::Value;
use serde_json::{Value as Json, json};

const SETTLED: &str = "task.settled";

fn tables(body: &str) -> Tables {
    let (t, problems) = load(vec![pack("p", &format!("api = 1\n{body}"))]);
    assert!(problems.is_empty(), "{problems:?}");
    t
}

/// The tables, and what the Table itself said of them.
fn tables_said(body: &str) -> (Tables, Vec<String>) {
    let (t, problems) = load(vec![pack("p", &format!("api = 1\n{body}"))]);
    (t, problems.iter().map(ToString::to_string).collect())
}

fn rules(body: &str) -> Rules {
    let (r, problems) = Rules::load(&tables(body));
    assert!(problems.is_empty(), "{problems:?}");
    r
}

/// What loading a rule says, from the Table's schema or from the rule's own check, joined; empty when it loads.
fn refused(body: &str) -> String {
    let (t, mut said) = tables_said(body);
    let (r, problems) = Rules::load(&t);
    said.extend(problems.iter().map(ToString::to_string));
    assert!(said.is_empty() || r.is_empty(), "a rule that is refused is not loaded: {said:?}");
    said.join(" | ")
}

fn rule(fields: &str) -> String {
    rule_on("task.*", fields)
}

fn rule_on(on: &str, fields: &str) -> String {
    format!("[[rule]]\nid = \"r\"\non = \"{on}\"\n{fields}\n")
}

fn settled(seq: u64, task: &str, data: Json) -> Ev {
    event(seq, seq * 10, SETTLED, Some(task), data)
}

/// The rules' answer to one event, with no folds and the given settings tree.
struct World {
    folds: Folds,
    set: Value,
    _scratch: Scratch,
}

impl World {
    fn new() -> World {
        let scratch = Scratch::new("rules");
        let folds = Folds::new(&scratch.join("folds"));
        World { folds, set: Value::Map(Default::default()), _scratch: scratch }
    }

    fn react(&self, rules: &Rules, st: &mut State, ev: &Ev, now: u64) -> Vec<Act> {
        let env = RuleEnv::new(ev, &self.folds, &self.set, now);
        rules.react(st, ev, &env, now)
    }
}

fn once(rules: &Rules, ev: &Ev) -> Vec<Act> {
    World::new().react(rules, &mut State::default(), ev, ev.at_ms)
}

#[test]
fn the_base_packs_rules_all_load() {
    let (r, problems) = Rules::load(&base_tables(""));
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(r.ids(), ["cool-on-limit", "followups", "followups-compact", "quiet", "rabbit", "soft-cap", "stuck"], "by id, as the Table lists them");
}

#[test]
fn a_rule_says_what_it_is_told_to_say_with_its_holes_filled() {
    let r = rules(&rule("when = \"ev.data.todo_left > 0\"\nthen = \"notice\"\ntext = \"{ev.data.todo_left} left on {task} ({kind})\""));
    let acts = once(&r, &settled(7, "t3", json!({ "todo_left": 2 })));
    assert_eq!(acts.len(), 1);
    assert_eq!((acts[0].rule.as_str(), acts[0].cause, acts[0].task.as_deref(), acts[0].depth), ("r", 7, Some("t3"), 0));
    assert_eq!(acts[0].what, Do::Notice("2 left on t3 (task.settled)".into()));
    assert!(once(&r, &settled(8, "t3", json!({ "todo_left": 0 }))).is_empty(), "when it does not hold, nothing");
    assert!(once(&r, &event(9, 90, "run.message", Some("t3"), json!({ "todo_left": 5 }))).is_empty(), "another kind, nothing");
}

#[test]
fn an_event_is_matched_by_the_glob_and_every_rule_that_matches_answers() {
    let r = rules("[[rule]]\nid = \"a\"\non = \"task.*\"\nthen = \"notice\"\ntext = \"a\"\n[[rule]]\nid = \"b\"\non = \"task.settled\"\nthen = \"notice\"\ntext = \"b\"\n[[rule]]\nid = \"c\"\non = \"run.*\"\nthen = \"notice\"\ntext = \"c\"\n");
    let said: Vec<String> = once(&r, &settled(1, "t1", json!({}))).into_iter().map(|a| a.rule).collect();
    assert_eq!(said, ["a", "b"], "in the order they are loaded");
    assert!(r.wants("task.settled") && r.wants("run.shell") && !r.wants("ask.answered") && !r.wants("tasks"));
}

#[test]
fn what_a_rule_asks_for_is_one_of_five_things() {
    let one = |fields: &str| -> Do { once(&rules(&rule(fields)), &settled(1, "t1", json!({ "ms": 90_000 }))).remove(0).what };
    assert_eq!(
        one("then = \"start\"\nfollow = true\ncompact = true\nkeep = [\"  a\\n b \", \"c\"]\ntext = \"go on\""),
        Do::Start { text: "go on".into(), follow: true, doer: None, place: None, detached: false, compact: true, keep: vec!["a\n b".into(), "c".into()] }
    );
    assert_eq!(one("then = \"start\"\ndoer = \"checker\"\ntext = \"check\""), Do::Start { text: "check".into(), follow: false, doer: Some("checker".into()), place: None, detached: false, compact: false, keep: vec![] });
    assert_eq!(one("then = \"steer\"\ntext = \"hurry\""), Do::Steer("hurry".into()));
    assert_eq!(one("then = \"control\"\ncontrol = \"park\""), Do::Park);
    assert_eq!(one("then = \"cool\"\ncool_for = \"5m\""), Do::Cool { ms: 300_000 });
    assert_eq!(one("then = \"cool\"\ncool_for = \"ev.data.ms\""), Do::Cool { ms: 90_000 }, "a number is milliseconds");
    for (fields, why) in [("then = \"cool\"\ncool_for = \"ev.data.nothing\"", "gave no time"), ("then = \"cool\"\ncool_for = \"0s\"", "gave no time"), ("then = \"cool\"\ncool_for = \"'soon'\"", "gave no time")] {
        let what = once(&rules(&rule(fields)), &settled(1, "t1", json!({}))).remove(0).what;
        assert!(matches!(&what, Do::Dropped(m) if m.contains(why)), "{fields}: {what:?}");
    }
}

#[test]
fn words_have_no_control_characters_a_bounded_length_and_are_never_empty() {
    let r = rules(&rule("then = \"notice\"\ntext = \"{ev.data.n}\""));
    let say = |v: &str| once(&r, &settled(1, "t1", json!({ "n": v }))).remove(0).what;
    assert_eq!(say("a\u{1b}[31mb\u{7}\tc\nd"), Do::Notice("a\u{fffd}[31mb\u{fffd}\tc\nd".into()), "a hole shows a control character as a mark; tab and line end stay");
    let literal = rules(&rule("then = \"notice\"\ntext = \"a\\u0007b\\u001bc\""));
    assert_eq!(once(&literal, &settled(1, "t1", json!({}))).remove(0).what, Do::Notice("abc".into()), "and the rule's own words are cleaned too");
    let Do::Notice(long) = say(&"z".repeat(TEXT_MAX + 500)) else { panic!() };
    assert_eq!(long.chars().count(), TEXT_MAX + 1);
    assert!(long.ends_with('…'));
    assert!(matches!(say("   "), Do::Dropped(m) if m.contains("empty")), "white space is nothing to say");
    let controls = rules(&rule("then = \"notice\"\ntext = \"\\u0007\\u0008\""));
    assert!(matches!(once(&controls, &settled(1, "t1", json!({}))).remove(0).what, Do::Dropped(_)), "and neither is what is left of nothing but controls");
}

#[test]
fn a_rule_that_asks_is_the_fronts_to_read_at_a_boundary_and_not_the_loops() {
    let body = rule_on("task.settled", "when = \"ctx.held > 0\"\nthen = \"ask\"\ntext = \"much junk\"\ndo = \":context\"\nlimit = { per = \"thread\", max = 1, within = \"1s\" }");
    let t = tables(&body);
    // the loop's rules leave it out, so a boundary is read once, and by the front
    let (loaded, problems) = Rules::load(&t);
    assert!(loaded.is_empty() && problems.is_empty(), "{problems:?}");
    // the rule engine reads the row when the front asks it to: `ctx` is a root only the front fills, a loop's env has none
    let (w, mut st) = (World::new(), State::default());
    let one = t.get::<j4n_core::rule::RuleRow>("r").unwrap().rules().unwrap();
    let react = |st: &mut State, at: u64, ctx: Option<Json>| {
        let ev = event(1, at, SETTLED, None, json!({}));
        let env = RuleEnv::new(&ev, &w.folds, &w.set, at).with_ctx(ctx.map_or(Value::Missing, |c| Value::from_json(&c)));
        one.react(st, &ev, &env, at)
    };
    assert!(react(&mut st, 0, None).is_empty(), "no `ctx` is a check that does not hold");
    assert!(react(&mut st, 0, Some(json!({ "held": 0 }))).is_empty());
    let acts = react(&mut st, 0, Some(json!({ "held": 5 })));
    assert!(matches!(acts.as_slice(), [a] if a.what == Do::Ask { text: "much junk".into(), run: ":context".into() }), "{acts:?}");
    assert!(react(&mut st, 500, Some(json!({ "held": 5 }))).is_empty(), "the limit: once in a second");
    assert_eq!(react(&mut st, 1_500, Some(json!({ "held": 5 }))).len(), 1);
}

#[test]
fn a_rule_that_is_off_does_nothing_and_ctx_is_for_a_rule_that_asks() {
    assert!(rules(&rule("then = \"notice\"\ntext = \"x\"\noff = true")).is_empty(), "off is for every rule, not only one that asks");
    assert_eq!(rules(&rule("then = \"notice\"\ntext = \"x\"")).len(), 1);
    assert!(refused(&rule("then = \"notice\"\ntext = \"x\"\nwhen = \"ctx.held > 0\"")).contains("`ctx` is read by a rule that asks only"));
    // a rule that asks and is wrong is a problem when the rules load, though the loop does not read it
    assert!(refused(&rule_on("task.settled", "then = \"ask\"\ntext = \"x\"\ndo = \":context\"")).contains("needs a condition"));
}

#[test]
fn a_rule_that_is_wrong_is_refused_and_says_where() {
    let cases = [
        ("then = \"start\"", "needs `text`"),
        ("then = \"steer\"", "needs `text`"),
        ("then = \"notice\"", "needs `text`"),
        ("then = \"control\"", "needs `control = \"park\"`"),
        ("then = \"cool\"", "needs `cool_for`"),
        ("then = \"notice\"\ntext = \"x\"\ndo = \":context\"", "`do` is for `ask`"),
        ("then = \"propose\"", "`propose` needs"),
        ("then = \"notice\"\ntext = \"x\"\npropose = { key = \"k\", title = \"t\", why = \"w\", kind = \"setting\", id = \"a\", remove = true }", "`propose` is for `propose`"),
        ("then = \"steer\"\ntext = \"x\"\nfollow = true", "`follow` is for `start`"),
        ("then = \"notice\"\ntext = \"x\"\ndoer = \"d\"", "`doer` is for `start`"),
        ("then = \"start\"\ntext = \"x\"\nfollow = true\ndoer = \"d\"", "a task that is not carried on"),
        ("then = \"start\"\ntext = \"x\"\ncompact = true", "`compact` is for `start` with `follow`"),
        ("then = \"start\"\ntext = \"x\"\nfollow = true\nkeep = [\"n\"]", "`keep` is for `compact`"),
        ("then = \"notice\"\ntext = \"x\"\ncontrol = \"park\"", "`control` is for `control`"),
        ("then = \"notice\"\ntext = \"x\"\ncool_for = \"5m\"", "`cool_for` is for `cool`"),
        ("then = \"control\"\ncontrol = \"park\"\ntext = \"x\"", "`text` is for `start`, `steer`, `notice` and `ask`"),
        ("then = \"notice\"\ntext = \"x\"\nwhen = \"(\"", "when:"),
        ("then = \"notice\"\ntext = \"x\"\nwhen = \"nosuch.x\"", "`nosuch` is not a name here"),
        ("then = \"notice\"\ntext = \"{\"", "text: a `{` is never closed"),
        ("then = \"notice\"\ntext = \"{nosuch}\"", "text: `nosuch` is not a name here"),
        ("then = \"cool\"\ncool_for = \"nosuch\"", "cool_for: `nosuch` is not a name here"),
        ("then = \"notice\"\ntext = \"x\"\nafter = \"0s\"", "`after` is a duration more than zero"),
        ("then = \"notice\"\ntext = \"x\"\ncooldown = \"0s\"", "`cooldown` is a duration more than zero"),
        ("then = \"notice\"\ntext = \"x\"\nlimit = { max = 2, within = \"0s\" }", "`limit.within`"),
        ("then = \"start\"\ntext = \"x\"\nfollow = true\ncompact = true\nkeep = [\"1\",\"2\",\"3\",\"4\",\"5\",\"6\",\"7\"]", "at most 6 notes"),
    ];
    let mut wrong = Vec::new();
    for (fields, want) in cases {
        let said = refused(&rule(fields));
        if !said.contains(want) || !(said.contains("rule r") || said.contains("[[rule]] r")) {
            wrong.push(format!("{fields}\n  wanted `{want}` and the rule's name, got `{said}`"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    let said = refused("[[rule]]\nid = \"r\"\non = \"x\"\nthen = \"notice\"\ntext = \"x\"\nlimit = { max = 0 }\n");
    assert!(said.contains("max"), "the schema wants at least one: {said}");
    let said = refused("[[rule]]\nid = \"r\"\non = \"x\"\nthen = \"notice\"\ntext = \"x\"\nnonsense = 1\n");
    assert!(said.contains("nonsense"), "{said}");
    // every fault is told, not only the first
    let said = refused(&rule("then = \"start\"\nafter = \"0s\"\ncooldown = \"0s\""));
    assert!(said.contains("needs `text`") && said.contains("`after`") && said.contains("`cooldown`"), "{said}");
}

#[test]
fn words_a_doer_or_a_model_wrote_are_not_said_bare() {
    let fmt = |text: &str| refused(&rule(&format!("then = \"notice\"\ntext = \"{text}\"")));
    for bare in [
        "{ev.data.text}",
        "{data.answer}",
        "{ev.data.cmd}",
        "{ev.data.out}",
        "{ev.data.detail}",
        "{ev.data.reason}",
        "{ev.data.error}",
        "{fold.tasks[task].answer}",
        "{fold.tasks[task].reason}",
        "{fold.tasks[task]}",
        "{fold.tasks}",
        "{ev.data}",
        "{data}",
        "{ev}",
        "{fold}",
        "{set}",
        "{lower(ev.data.text)}",
        "{len(ev.data.text)} bytes and {ev.data.message}",
        "{last(ev.data.paths)}",
        // a fold read by a doer's key is let through for the key only; a fold that holds words is refused by its own path
        "{fold.tasks[ev.data.text].answer}",
        "{fold.answer[ev.data.text]}",
        "{fold.tasks[lower(ev.data.text)].reason}",
        "{fold.tasks[task].answer} after {len(ev.data.out)}",
    ] {
        let said = fmt(bare);
        assert!(said.contains("wrote, bare") && said.contains("quote(…)"), "{bare}: {said:?}");
    }
    for fine in [
        "{quote(ev.data.text)}",
        "{quote(fold.tasks[task].answer)}",
        // a number carries no words: how long, how many, a fold read by a key a doer wrote
        "{len(ev.data.out)} characters",
        "{count(ev.data.paths)} files",
        "{fold.habit_lines[lower(ev.data.text)]} times",
        "{fold.habit_reads[ev.data.detail]} reads",
        "{ev.data.todo_left} left on {task} by {by} ({kind}, {seq})",
        "{fold.tasks[task].origin}",
        "{fold.tasks[task].spent:usd}",
        "{set.loop.followups}",
        "{fold.explored[task]}",
        "no holes at all",
    ] {
        assert_eq!(fmt(fine), "", "{fine}");
    }
    // a condition only compares; it is not said
    assert_eq!(refused(&rule("then = \"notice\"\ntext = \"x\"\nwhen = \"contains(ev.data.text, 'stop') and fold.tasks[task].answer\"")), "");
    // notes ride the same rule
    let said = refused(&rule("then = \"start\"\nfollow = true\ncompact = true\ntext = \"x\"\nkeep = [\"{ev.data.text}\"]"));
    assert!(said.contains("`keep`") && said.contains("wrote, bare"), "{said}");
}

#[test]
fn a_rule_in_a_project_never_runs_and_one_in_the_users_own_files_does() {
    let file = "api = 1\n[[rule]]\nid = \"mine\"\non = \"task.settled\"\nthen = \"notice\"\ntext = \"x\"\n";
    let layers = |kind| vec![layer(kind, "l", &[("rules.toml", file)])];
    let (t, problems) = Tables::load(layers(LayerKind::Project));
    assert!(Rules::load(&t).0.is_empty(), "{problems:?}");
    let (t, _) = Tables::load(layers(LayerKind::User));
    assert_eq!(Rules::load(&t).0.ids(), ["mine"]);
}

#[test]
fn a_rule_that_answers_a_kind_that_is_only_flashed_makes_the_loop_listen_for_flashes() {
    let flashes = "[[event]]\nid = \"draft.typed\"\ndoc = \"d\"\nephemeral = true\n";
    let on = |glob: &str| format!("{flashes}[[rule]]\nid = \"r\"\non = \"{glob}\"\nthen = \"notice\"\ntext = \"x\"\n");
    assert!(rules(&on("draft.*")).wants_flash(), "a pack's own flashed kind");
    assert!(rules(&on("run.delta")).wants_flash(), "the core's");
    assert!(rules(&on("*")).wants_flash(), "a glob that takes in one");
    assert!(!rules(&on("task.*")).wants_flash(), "written kinds only");
    assert!(!rules("").wants_flash());
}

#[test]
fn a_limit_keeps_a_rule_from_saying_it_again_for_that_task_only() {
    let r = rules(&rule("then = \"notice\"\ntext = \"x\"\nlimit = { max = 2 }"));
    let (w, mut st) = (World::new(), State::default());
    let fired = |st: &mut State, seq: u64, task: &str| w.react(&r, st, &settled(seq, task, json!({})), seq * 10).len();
    assert_eq!((fired(&mut st, 1, "t1"), fired(&mut st, 2, "t1"), fired(&mut st, 3, "t1")), (1, 1, 0), "two, then quiet: not a drop, a limit is normal");
    assert_eq!(fired(&mut st, 4, "t2"), 1, "the count is per task");
    // a task that is over keeps its count: what is answered after it ended is still about it
    assert!(st.forget("t1").is_empty());
    assert_eq!(fired(&mut st, 5, "t1"), 0);
}

#[test]
fn a_limit_can_be_per_thread_or_for_all_and_can_look_back_only_so_far() {
    let (mut st, w) = (State::default(), World::new());
    let r = rules(&rule("then = \"notice\"\ntext = \"x\"\nlimit = { per = \"thread\", max = 1 }"));
    let hit = |st: &mut State, seq: u64, task: &str, thread: &str| {
        let mut e = settled(seq, task, json!({}));
        e.thread = Some(thread.into());
        w.react(&r, st, &e, seq).len()
    };
    assert_eq!((hit(&mut st, 1, "t1", "main"), hit(&mut st, 2, "t2", "main"), hit(&mut st, 3, "t3", "other")), (1, 0, 1), "two tasks of one thread are one");
    let all = rules(&rule("then = \"notice\"\ntext = \"x\"\nlimit = { per = \"all\", max = 2 }"));
    let mut st = State::default();
    let n: usize = (1..=5).map(|i| w.react(&all, &mut st, &settled(i, &format!("t{i}"), json!({})), i).len()).sum();
    assert_eq!(n, 2, "two in all");
    // within: the ones that are old enough do not count
    let within = rules(&rule("then = \"notice\"\ntext = \"x\"\nlimit = { max = 2, within = \"1m\" }"));
    let mut st = State::default();
    let at = |st: &mut State, seq: u64, now: u64| w.react(&within, st, &settled(seq, "t1", json!({})), now).len();
    assert_eq!((at(&mut st, 1, 0), at(&mut st, 2, 10_000), at(&mut st, 3, 59_999), at(&mut st, 4, 60_001)), (1, 1, 0, 1), "the first fell out of the minute");
}

#[test]
fn a_cooldown_is_a_least_time_between_two_sayings() {
    let r = rules(&rule("then = \"notice\"\ntext = \"x\"\ncooldown = \"10s\""));
    let (w, mut st) = (World::new(), State::default());
    let at = |st: &mut State, seq: u64, now: u64| w.react(&r, st, &settled(seq, "t1", json!({})), now).len();
    assert_eq!((at(&mut st, 1, 1_000), at(&mut st, 2, 10_999), at(&mut st, 3, 11_000), at(&mut st, 4, 11_001)), (1, 0, 1, 0));
    assert_eq!(w.react(&r, &mut st, &settled(5, "t2", json!({})), 11_001).len(), 1, "for each task");
}

#[test]
fn a_wait_is_counted_from_the_newest_match_and_a_stream_of_matches_keeps_one_timer() {
    let r = rules(&rule_on("run.*", "then = \"notice\"\ntext = \"quiet since {seq}\"\nafter = \"5m\""));
    let (w, mut st) = (World::new(), State::default());
    let ev = |seq: u64, at: u64| event(seq, at, "run.message", Some("t1"), json!({}));
    let first = w.react(&r, &mut st, &ev(1, 1_000), 1_000);
    let Do::Arm { rule, key, ms, ev: e } = first[0].what.clone() else { panic!("{:?}", first[0]) };
    assert_eq!((rule, key.as_str(), ms, e.seq), (0, "t1", 300_000, 1));
    assert!(!st.renew(rule, &key, &e), "nothing waits yet");
    st.arm(rule, &key, *e, 11, ms);
    assert_eq!(st.waiting(), 1);
    // later matches renew the wait; they do not make another
    let later = w.react(&r, &mut st, &ev(2, 100_000), 100_000);
    let Do::Arm { ev: e, .. } = later[0].what.clone() else { panic!() };
    assert!(st.renew(rule, &key, &e));
    assert_eq!(st.waiting(), 1);
    // a timer that is not the one held is stale, and so is a key that is not waiting
    assert_eq!(st.take_due(rule, &key, 99, 301_000), Due::Stale);
    assert_eq!(st.take_due(rule, "t2", 11, 301_000), Due::Stale);
    // the timer set for the first match fires: the quiet has not lasted since the second
    assert_eq!(st.take_due(rule, &key, 11, 301_000), Due::Again(99_000), "100,000 + 300,000, less now");
    st.rearm(rule, &key, 12);
    assert_eq!(st.take_due(rule, &key, 11, 400_000), Due::Stale, "the old timer is not the one now");
    assert_eq!(st.take_due(rule, &key, 12, 399_999), Due::Again(1));
    let Due::Fire(got) = st.take_due(rule, &key, 12, 400_000) else { panic!() };
    assert_eq!(got.seq, 2, "the newest event is the one it acts on");
    assert_eq!(st.waiting(), 0);
    assert_eq!(st.take_due(rule, &key, 12, 400_001), Due::Stale, "once");
    let env = RuleEnv::new(&got, &w.folds, &w.set, 400_000);
    let act = r.due(&mut st, 0, &got, &env, 400_000).expect("still holds");
    assert_eq!(act.what, Do::Notice("quiet since 2".into()), "the words are made when it acts, from the event it waited on");
    assert_eq!(act.cause, 2);
}

#[test]
fn a_task_that_ends_takes_its_waits_with_it_and_a_wait_asks_again_whether_it_still_holds() {
    let r = rules(&rule_on("run.*", "when = \"data.live\"\nthen = \"notice\"\ntext = \"x\"\nafter = \"5m\""));
    let (w, mut st) = (World::new(), State::default());
    let e = event(1, 10, "run.message", Some("t1"), json!({ "live": true }));
    let Do::Arm { rule, key, ev, ms, .. } = w.react(&r, &mut st, &e, 10).remove(0).what else { panic!() };
    st.arm(rule, &key, *ev, 77, ms);
    assert_eq!(st.forget("t1"), vec![77], "the timer to cancel");
    assert_eq!(st.waiting(), 0);
    assert_eq!(st.take_due(0, "t1", 77, 400_000), Due::Stale);
    // a wait whose condition no longer holds says nothing: here the event it kept is the one that is asked again
    let Do::Arm { rule, key, ev, ms, .. } = w.react(&r, &mut st, &e, 10).remove(0).what else { panic!() };
    st.arm(rule, &key, *ev, 78, ms);
    let Due::Fire(got) = st.take_due(0, "t1", 78, 400_000) else { panic!() };
    let got = *got;
    let gone = Ev { data: json!({ "live": false }), ..got };
    let env = RuleEnv::new(&gone, &w.folds, &w.set, 20);
    assert!(r.due(&mut st, 0, &gone, &env, 20).is_none());
    assert!(r.due(&mut st, 9, &gone, &env, 20).is_none(), "a rule that is not there");
}

#[test]
fn a_wait_has_its_own_limit_and_too_many_waiting_are_dropped() {
    let r = rules(&rule_on("run.*", "then = \"notice\"\ntext = \"x\"\nafter = \"5m\""));
    let (w, mut st) = (World::new(), State::default());
    for i in 0..KEYS_MAX {
        let acts = w.react(&r, &mut st, &event(1, 1, "run.message", Some(&format!("k{i}")), json!({})), 1);
        let Do::Arm { rule, key, ev, .. } = acts[0].what.clone() else { panic!("{i}: {:?}", acts[0]) };
        st.arm(rule, &key, *ev, i as u64 + 1, 300_000);
    }
    st.drop_wait(0, "k1");
    assert_eq!(st.waiting(), KEYS_MAX - 1, "a wait that cannot go on is given up");
    st.arm(0, "k1", event(9, 9, "run.message", Some("k1"), json!({})), 9, 300_000);
    let over = w.react(&r, &mut st, &event(2, 2, "run.message", Some("one-more"), json!({})), 2);
    assert!(matches!(&over[0].what, Do::Dropped(m) if m.contains("too many reactions are waiting")), "{:?}", over[0]);
    let again = w.react(&r, &mut st, &event(3, 3, "run.message", Some("k0"), json!({})), 3);
    assert!(matches!(again[0].what, Do::Arm { .. }), "one that is waiting already starts again: it takes no more room");
}

#[test]
fn a_chain_of_rules_answering_rules_ends_at_a_depth_with_a_notice() {
    let r = rules(&rule("then = \"notice\"\ntext = \"x\""));
    let (w, mut st) = (World::new(), State::default());
    let e = settled(5, "t1", json!({}));
    assert_eq!(st.depth(&e), 0);
    st.mark(5, DEPTH_MAX - 1);
    let acts = w.react(&r, &mut st, &e, 50);
    assert_eq!((acts[0].depth, matches!(acts[0].what, Do::Notice(_))), (DEPTH_MAX - 1, true), "one below the end it may still act");
    st.mark(5, DEPTH_MAX);
    let acts = w.react(&r, &mut st, &e, 51);
    assert!(matches!(&acts[0].what, Do::Dropped(m) if m.contains("a chain of 8 reactions")), "{acts:?}");
    // the end of a chain is said once for a rule and a task, not for each event of the task
    let again = w.react(&r, &mut st, &e, 52);
    assert!(again.is_empty(), "{again:?}");
    assert!(matches!(w.react(&r, &mut st, &settled(5, "t2", json!({})), 52)[0].what, Do::Dropped(_)), "another task has its own");
    st.forget("t1");
    assert!(w.react(&r, &mut st, &e, 53).is_empty(), "and it stays said when the task is over");
    // a task a rule began carries the depth of the chain into everything it does
    let mut st = State::default();
    st.mark_task("t9", DEPTH_MAX);
    let acts = w.react(&r, &mut st, &settled(6, "t9", json!({})), 60);
    assert!(matches!(acts[0].what, Do::Dropped(_)), "{acts:?}");
    assert!(matches!(w.react(&r, &mut st, &settled(7, "t8", json!({})), 60)[0].what, Do::Notice(_)), "another task is not in the chain");
    // the deeper of the two counts
    st.mark_task("t8", 3);
    st.mark(8, 6);
    assert_eq!(st.depth(&settled(8, "t8", json!({}))), 6);
    assert_eq!(st.depth(&settled(9, "t8", json!({}))), 3);
    // a task's depth is a floor under the events that are not marked and under those marked shallower
    st.mark_task("t7", 5);
    st.mark(10, 2);
    assert_eq!(st.depth(&settled(10, "t7", json!({}))), 5);
    // the table of depths does not grow without bound
    for seq in 1..=20_000 {
        st.mark(seq, 1);
    }
    assert_eq!(st.depth(&settled(1, "tx", json!({}))), 0, "the oldest are forgotten");
    assert_eq!(st.depth(&settled(20_000, "tx", json!({}))), 1);
    for n in 100..20_200 {
        st.mark_task(&format!("t{n}"), 2);
    }
    assert_eq!((st.depth(&settled(1, "t100", json!({}))), st.depth(&settled(1, "t20199", json!({})))), (0, 2), "and neither is the table of tasks");
    st.mark_task("not-a-task", 4);
    assert_eq!(st.depth(&settled(1, "not-a-task", json!({}))), 0, "a name that is no task's has no depth");
}

#[test]
fn too_many_sayings_in_a_short_time_are_dropped_and_said_once() {
    let r = rules(&rule("then = \"notice\"\ntext = \"x\""));
    let (w, mut st) = (World::new(), State::default());
    let mut said = 0;
    let mut dropped = Vec::new();
    for i in 0..(RATE_MAX as u64 + 40) {
        for a in w.react(&r, &mut st, &settled(i + 1, &format!("t{i}"), json!({})), 1_000 + i) {
            match a.what {
                Do::Notice(_) => said += 1,
                Do::Dropped(m) => dropped.push(m),
                other => panic!("{other:?}"),
            }
        }
    }
    assert_eq!(said, RATE_MAX);
    assert_eq!(dropped.len(), 1, "once, or the notice would be the storm: {dropped:?}");
    assert!(dropped[0].contains("more than 120 reactions in 10 seconds"), "{dropped:?}");
    // the window passes and it speaks again
    let later = 1_000 + RATE_WINDOW_MS + 200;
    let acts = w.react(&r, &mut st, &settled(900, "t900", json!({})), later);
    assert!(matches!(acts[0].what, Do::Notice(_)), "{acts:?}");
}

#[test]
fn the_flashed_and_the_written_are_answered_alike() {
    let r = rules("[[rule]]\nid = \"r\"\non = \"draft.*\"\nthen = \"notice\"\ntext = \"typing\"\n");
    let flash = event(0, 5, "draft.typed", None, json!({}));
    assert_eq!(flash.seq, 0);
    let acts = once(&r, &flash);
    assert_eq!((acts.len(), acts[0].cause, acts[0].task.clone()), (1, 0, None));
}

const PROPOSE: &str = "then = \"propose\"\npropose = { key = \"gate:{ev.data.id}\", title = \"allow {ev.data.id}\", why = \"{ev.data.n} times\", evidence = \"ev.data.n\", kind = \"thing\", id = \"{ev.data.id}\", set = { size = \"=ev.data.n\", name = \"by rule {ev.data.n}\", tags = ['a', 'b'] } }";

#[test]
fn a_rule_that_proposes_says_the_whole_change_with_its_numbers_kept_as_numbers() {
    let r = rules(&rule_on("x.seen", PROPOSE));
    let acts = once(&r, &event(5, 50, "x.seen", None, json!({ "id": "abc", "n": 4 })));
    let [act] = acts.as_slice() else { panic!("{acts:?}") };
    let Do::Propose(d) = &act.what else { panic!("{:?}", act.what) };
    assert_eq!((d.key.as_str(), d.title.as_str(), d.why.as_str(), d.evidence), ("gate:abc", "allow abc", "4 times", 4));
    let [step] = d.steps.as_slice() else { panic!() };
    assert_eq!((step.kind.as_str(), step.id.as_str(), step.to), ("thing", "abc", j4n_core::change::Where::User));
    let j4n_core::proposal::StepOp::Set(t) = &step.op else { panic!() };
    assert_eq!(t.get("size").and_then(|v| v.as_integer()), Some(4), "a number is a number in the file");
    assert_eq!(t.get("name").and_then(|v| v.as_str()), Some("by rule 4"), "text with holes is a template");
    assert_eq!(t.get("tags").and_then(|v| v.as_array()).map(|a| a.len()), Some(2), "a list stays a list");
}

#[test]
fn a_proposal_with_no_evidence_yet_is_not_made_and_says_so() {
    let r = rules(&rule_on("x.seen", PROPOSE));
    let acts = once(&r, &event(5, 50, "x.seen", None, json!({ "id": "abc" })));
    assert!(matches!(&acts[0].what, Do::Dropped(why) if why.contains("no evidence")), "{acts:?}");
}

#[test]
fn what_a_proposal_writes_cannot_be_what_a_doer_wrote_bare() {
    let bad = |fields: &str| refused(&rule_on("x.seen", &format!("then = \"propose\"\npropose = {{ key = \"k\", title = \"t\", why = \"w\", kind = \"thing\", id = \"a\", {fields} }}")));
    assert!(bad("set = { name = \"=ev.data.text\" }").contains("wrote, bare"), "{}", bad("set = { name = \"=ev.data.text\" }"));
    assert!(bad("set = { name = \"{ev.data.text}\" }").contains("wrote, bare"));
    assert!(bad("set = { name = \"=quote(ev.data.text)\" }").is_empty(), "quote makes it inert");
    assert!(bad("remove = true, set = { a = 1 }").contains("stands alone"));
    assert!(bad("to = \"world\", remove = true").contains("home"), "the schema names the two layers");
    let none = refused(&rule_on("x.seen", "then = \"propose\"\npropose = { key = \"k\", title = \"t\", why = \"w\", kind = \"thing\", id = \"a\" }"));
    assert!(none.contains("`set`, `unset` or `remove`"), "{none}");
}
