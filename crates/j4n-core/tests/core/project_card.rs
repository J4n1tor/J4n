//! The project card through the real front and a scripted doer that takes a
//! Brief. The card itself is derived in `j4n-memory` (its cases are there); here a stand-in gives the front a text.

use crate::front_cases::fx;
use crate::scripted::{Beat, Scripted, end, say};
use j4n_core::doer::Caps;
use j4n_core::front::ProjectFacts;
use j4n_core::setting::Settings;
use j4n_core::table::{Layer, LayerKind, SourceFile, Tables};
use std::sync::Arc;

struct Says(Option<&'static str>);

impl ProjectFacts for Says {
    fn text(&self) -> Option<String> {
        self.0.map(String::from)
    }
}

fn doer() -> Arc<Scripted> {
    let turns = (0..2).map(|_| vec![Beat::Session("claude-s".into()), say("ok"), end("ok")]).collect();
    Scripted::with_caps("claude", Caps { live: true, resume: true, brief: true, ..Caps::default() }, turns)
}

/// The Briefs of the first two lines of a thread.
fn briefs(card: Option<Option<&'static str>>) -> Vec<String> {
    let f = fx(vec![doer()], |c| {
        c.doer = Some("claude".into());
        c.card = card.map(|t| Arc::new(Says(t)) as Arc<dyn ProjectFacts>);
    });
    f.turn("fix the parser in src");
    f.turn("and add a test for it");
    f.tasks().iter().map(|r| r.spec.input["brief"].as_str().unwrap().to_string()).collect()
}

#[test]
fn with_the_switch_off_the_brief_is_what_it_was_and_with_it_on_the_first_turn_alone_has_the_card() {
    let none = briefs(None);
    assert_eq!(none.len(), 2);
    // the switch off (a card that says nothing) is the Brief of a build without the card, byte for byte
    assert_eq!(briefs(Some(None)), none);
    assert!(none.iter().all(|b| !b.contains("## project")));
    // on: the first turn of the session has the section, after the contract; the next turn, which the session carries on, has none
    let on = briefs(Some(Some("Project card, derived from the files:\nCargo.toml: rust")));
    assert!(on[0].find("## project") > on[0].find("## contract"), "{}", on[0]);
    assert!(on[0].contains("## project\nProject card, derived from the files:\nCargo.toml: rust"), "{}", on[0]);
    assert_eq!(on[1], none[1]);
    // the setting is in the base pack, on by default
    let file = SourceFile { path: "settings/work.toml".into(), text: include_str!("../../../../packs/base/settings/work.toml").into() };
    let (tables, problems) = Tables::load(vec![Layer::new(LayerKind::Pack, "base", vec![file])]);
    assert!(problems.is_empty(), "{problems:?}");
    let s = Settings::load(&tables).0;
    assert!(s.get("brief.project_card").is_some() && s.flag("brief.project_card", false));
}
