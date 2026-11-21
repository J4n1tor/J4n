//! The candidates for the context gauge and the work mark, held where the build measures them. `tests/data/gauges.json` holds the
//! candidates with the widths a design tool counted. Here the same strings are measured by `unicode-width`, the crate ratatui lays
//! out with, so the two cannot drift. The state marks and the contrast are read from the real glyph table and the real palettes.

use j4n_proto::Json;
use j4n_tui::theme::glyph::{Slot, Tier};
use j4n_tui::theme::palette::{self, Role};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

fn doc() -> Json {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/gauges.json");
    std::fs::read_to_string(path).expect("tests/data/gauges.json").parse().expect("gauges.json is JSON")
}

const TIERS: [(&str, Tier); 3] = [("icons", Tier::Icons), ("unicode", Tier::Unicode), ("ascii", Tier::Ascii)];

/// Every variant with its id, whether it is a work mark, and one tier of it.
fn each(d: &Json, mut f: impl FnMut(&str, bool, &str, &Json)) {
    for v in d["variants"].as_array().expect("variants") {
        for (name, _) in TIERS {
            f(v["id"].as_str().expect("id"), v["work"].as_bool().expect("work"), name, &v["tiers"][name]);
        }
    }
}

fn strs(j: &Json) -> Vec<&str> {
    j.as_array().expect("array").iter().map(|s| s.as_str().expect("string")).collect()
}

#[test]
fn every_sample_is_as_wide_as_the_tool_counted_and_a_gauge_does_not_change_width_with_its_value() {
    let d = doc();
    let mut seen = 0;
    each(&d, |id, work, tier, t| {
        let (samples, cells) = (strs(&t["samples"]), t["cells"].as_array().expect("cells"));
        assert_eq!(samples.len(), cells.len(), "{id} {tier}");
        for (s, c) in samples.iter().zip(cells) {
            assert_eq!(s.width() as u64, c.as_u64().expect("number"), "{id} {tier}: `{s}` is not as wide as the tool counted");
            seen += 1;
        }
        if !work {
            let w: Vec<usize> = samples.iter().map(|s| s.width()).collect();
            assert!(w.windows(2).all(|p| p[0] == p[1]), "{id} {tier}: the row would shift as the context fills: {w:?}");
        }
    });
    assert!(seen > 300, "{seen} samples were measured");
}

#[test]
fn every_glyph_is_one_cell_and_an_icon_stands_in_a_slot_of_two() {
    let d = doc();
    let slot = d["slot"].as_u64().expect("slot") as usize;
    assert_eq!(slot, 2, "an icon takes a slot of two cells");
    let private = |c: char| ('\u{E000}'..='\u{F8FF}').contains(&c) || ('\u{F0000}'..='\u{FFFFD}').contains(&c);
    each(&d, |id, _, tier, t| {
        for g in strs(&t["glyphs"]) {
            for c in g.chars() {
                assert_eq!(c.width(), Some(1), "{id} {tier}: U+{:X} is not one cell wide", c as u32);
            }
        }
        for s in strs(&t["samples"]) {
            let cs: Vec<char> = s.chars().collect();
            for (i, c) in cs.iter().enumerate().filter(|(_, c)| private(**c)) {
                assert!(cs.get(i + 1) == Some(&' '), "{id} {tier}: the icon U+{:X} in `{s}` has no cell after it: its outline is wider than one cell (a Nerd Font icon is wider than its one-cell advance)", *c as u32);
            }
        }
    });
}

#[test]
fn the_ascii_tier_is_seven_bit_and_the_other_two_use_no_state_mark_for_something_else() {
    let d = doc();
    let state = |tier: &str| -> Vec<(String, String)> {
        d["state_slots"][tier].as_object().expect("state slots").iter().map(|(n, g)| (n.clone(), g.as_str().expect("glyph").to_string())).collect()
    };
    // the tool's copy of the table is the table
    for (name, t) in TIERS {
        for (slot, glyph) in state(name) {
            assert_eq!(Slot::by_name(&slot).unwrap_or_else(|| panic!("no slot {slot}")).cell(t), glyph, "{slot} in {name}");
        }
    }
    each(&d, |id, work, tier, t| {
        let glyphs = strs(&t["glyphs"]);
        if tier == "ascii" {
            assert!(glyphs.iter().all(|g| g.is_ascii()), "{id}: ascii has {glyphs:?}");
            return;
        }
        for (slot, mark) in state(tier) {
            if work && slot == "working" || id == "m1" && slot == "route" {
                continue; // the work mark is the working mark, and m1 is the route mark: on purpose, nothing new
            }
            assert!(!glyphs.contains(&mark.as_str()), "{id} {tier}: {mark} is the {slot} mark, and a chat row carries both");
        }
    });
}

#[test]
fn the_contrast_the_tool_reports_is_the_contrast_of_the_real_palettes() {
    let d = doc();
    let floors = [(Role::Dim, "dim", 4.5), (Role::Faint, "faint", 3.0), (Role::Accent, "accent", 4.5), (Role::Warn, "warn", 4.5), (Role::Error, "error", 4.5)];
    let palettes = palette::builtin();
    assert_eq!(palettes.len(), 6);
    for p in &palettes {
        let roles = p.roles();
        for (role, name, floor) in floors {
            let (got, _) = roles.worst(role);
            let said = d["contrast"][&p.id][name].as_f64().expect("contrast");
            assert!((got - said).abs() < 0.01, "{} {name}: the tool says {said}, the palette gives {got}", p.id);
            assert!(got >= floor - 1e-9, "{} {name}: {got:.2}:1 is under its floor {floor}", p.id);
        }
        // the edge role has no floor, and the old gauge's frame and track are drawn in it: this is the number that matters here
        let (edge, _) = roles.worst(Role::Edge);
        let said = d["contrast"][&p.id]["edge"].as_f64().expect("edge");
        assert!((edge - said).abs() < 0.01, "{} edge: the tool says {said}, the palette gives {edge}", p.id);
    }
}
