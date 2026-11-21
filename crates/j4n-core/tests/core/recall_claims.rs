//! Remembered claims: the event a report is written as and the Brief, through
//! the real front and a scripted doer. What is made of the event, and the staleness rule, are in `j4n-memory`.

use crate::front_cases::{Fx, fx};
use crate::scripted::{Beat, Scripted, end, say};
use crate::world::Scratch;
use j4n_core::change::Actor;
use j4n_core::doer::Caps;
use j4n_core::front::Remembered;
use j4n_core::report::{file_hash, goal_key};
use j4n_core::setting::Settings;
use j4n_core::table::{Layer, LayerKind, SourceFile, Tables};
use j4n_core::task::TaskSpec;
use serde_json::json;
use std::sync::{Arc, Mutex};

/// A stand-in that says what it is told to, and keeps the goals it was asked about.
struct Says(Option<&'static str>, Mutex<Vec<String>>);

impl Remembered for Says {
    fn text(&self, goal: &str) -> Option<String> {
        self.1.lock().unwrap().push(goal.to_string());
        self.0.map(String::from)
    }
}

fn doer() -> Arc<Scripted> {
    let turns = (0..2).map(|_| vec![Beat::Session("claude-s".into()), say("ok"), end("ok")]).collect();
    Scripted::with_caps("claude", Caps { live: true, resume: true, brief: true, ..Caps::default() }, turns)
}

/// The Briefs of the first two lines of a thread, and the goals the stand-in was asked about.
fn briefs(recall: Option<Option<&'static str>>) -> (Vec<String>, Vec<String>) {
    let says = recall.map(|t| Arc::new(Says(t, Mutex::new(Vec::new()))));
    let f = fx(vec![doer()], |c| {
        c.doer = Some("claude".into());
        c.card = Some(Arc::new(Card));
        c.recall = says.clone().map(|s| s as Arc<dyn Remembered>);
    });
    f.turn("fix the parser in src");
    f.turn("and add a test for it");
    (f.tasks().iter().map(|r| r.spec.input["brief"].as_str().unwrap().to_string()).collect(), says.map(|s| s.1.lock().unwrap().clone()).unwrap_or_default())
}

struct Card;

impl j4n_core::front::ProjectFacts for Card {
    fn text(&self) -> Option<String> {
        Some("Project card, derived from the files:\nCargo.toml: rust".into())
    }
}

#[test]
fn with_the_switch_off_the_brief_is_what_it_was_and_with_it_on_the_first_turn_alone_has_the_claims_after_the_card() {
    let (none, asked) = briefs(None);
    assert_eq!((none.len(), asked.len()), (2, 0));
    // the switch off (a hint that says nothing) is the Brief of a build without it, byte for byte
    let (off, asked) = briefs(Some(None));
    assert_eq!(off, none);
    assert_eq!(asked, ["fix the parser in src"], "it is asked about the goal of the first turn, and the turn the session carries on is not asked");
    assert!(none.iter().all(|b| !b.contains("## remembered")));
    // on: the first turn has the section, after the card and before the rest; the next turn has none
    let (on, _) = briefs(Some(Some("Claims of an earlier report of this same task:\n- the parser is in src/p.rs @ src/p.rs:3")));
    assert!(on[0].find("## remembered") > on[0].find("## project"), "{}", on[0]);
    assert!(on[0].contains("## remembered\nClaims of an earlier report of this same task:\n- the parser is in src/p.rs @ src/p.rs:3"), "{}", on[0]);
    assert_eq!(on[1], none[1]);
    // the setting is in the base pack, and off until it is measured
    let file = SourceFile { path: "settings/work.toml".into(), text: include_str!("../../../../packs/base/settings/work.toml").into() };
    let (tables, problems) = Tables::load(vec![Layer::new(LayerKind::Pack, "base", vec![file])]);
    assert!(problems.is_empty(), "{problems:?}");
    let s = Settings::load(&tables).0;
    assert!(s.get("brief.recall").is_some() && !s.flag("brief.recall", true));
}

fn spawn(f: &Fx, doer: &str, role: &str, goal: &str, parent: Option<&str>) -> String {
    let mut spec = TaskSpec::new(goal, doer);
    spec.thread = Some("main".into());
    spec.role = Some(role.into());
    spec.parent = parent.map(String::from);
    f.rig.tasks.spawn(spec, Actor::Operator).expect("spawns")
}

#[test]
fn a_report_is_written_with_its_goals_key_and_the_hash_of_each_file_a_checked_claim_names_and_no_other() {
    let src = Scratch::new("net");
    src.put("src/net.rs", "use std::io;\n// the retry lives here\nfn fetch() { retry(3) }\n");
    src.put("src/other.rs", "fn other() { retry(1) }\n");
    let answer = json!({
        "answer": "the retry is in fetch()", "sure": "mid", "open": [], "ran": [],
        "found": [
            { "claim": "the retry is in fetch()", "at": "src/net.rs:3" },
            { "claim": "the retry is in fetch()", "at": "src/other.rs:99" },
            { "claim": "the retry is in fetch()", "at": "src/gone.rs:3" },
            { "claim": "the retry is in fetch()", "at": "https://x.dev/a:3" },
        ],
    })
    .to_string();
    let boss = Scripted::new("boss", vec![vec![Beat::Forever]]);
    let scout = Scripted::new("scout", vec![vec![end(&answer)]]);
    let f = fx(vec![boss, scout], |c| c.root = src.path().to_path_buf());
    let main = spawn(&f, "boss", "work", "main work", None);
    let branch = spawn(&f, "scout", "scout", "Find where the retry is", Some(&main));
    f.rig.finished(&branch);
    f.front.report(&branch).expect("a report");
    let made = f.rig.of_kind("report.made");
    assert_eq!(made.len(), 1);
    assert_eq!(made[0].data["goal"], goal_key("Find where the retry is"));
    assert_eq!(made[0].data["goal"], goal_key("  find where THE retry\nis "), "the same words in other letters and spacing are the goal");
    assert_ne!(made[0].data["goal"], goal_key("Find where the retry was"));
    // one file: the one a claim that holds names; the unchecked ones (a line that is not there, a file that is gone, a URL) name none
    let hash = file_hash(src.path(), "src/net.rs").unwrap();
    assert_eq!(made[0].data["files"], json!({ "src/net.rs": hash }));
    assert_eq!(hash.len(), 16);
    f.rig.tasks.ctl(&main, j4n_core::task::Ctl::Abort, Actor::Operator).unwrap();
}
