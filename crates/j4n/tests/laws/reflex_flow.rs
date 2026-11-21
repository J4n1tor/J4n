//! The rename of the smart toggles to reflexes, the part that needs the binary's wiring: `:toggle` and `:toggles` still work as the old names of
//! `:reflex` and `:reflexes`. Tier 0: no model.

use crate::doctor_flow::world;
use crate::run_cases::Dirs;
use j4n_core::front::Said;

#[test]
fn the_old_words_toggle_and_toggles_run_the_same_commands_as_reflex_and_reflexes() {
    let d = Dirs::new("reflex-words");
    let w = world(&d, true);
    for (old, new) in [(":toggle off nothing", ":reflex off nothing"), ("/toggle off nothing", ":reflex off nothing")] {
        let (a, b) = (w.front.say("main", old), w.front.say("main", new));
        assert!(matches!(&a, Said::Refused(m) if m.contains("there is no reflex `nothing`")) && std::mem::discriminant(&a) == std::mem::discriminant(&b), "{old}: {a:?} against {new}: {b:?}");
    }
    for old in [":toggles", ":reflexes"] {
        assert_eq!(w.front.say("main", old), Said::Open("reflexes".into()), "{old}");
    }
    assert!(matches!(w.front.say("main", ":toggle"), Said::Refused(m) if m.contains(":toggle add <id> do")), "the old word names the forms it takes");
    w.close();
}
