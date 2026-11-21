//! What Main is told of J4n, on the packs the binary ships: every pack says when to use it, in plain words, and the catalogue built from all of them with
//! every pack on stays under its token cap with room to spare for the roster.

use j4n_core::front::catalogue::{Roster, build};

fn roster() -> Vec<Roster> {
    let some = |d: &str, m: &[&str]| Roster { doer: d.into(), models: m.iter().map(|m| m.to_string()).collect() };
    vec![some("claude", &["haiku", "sonnet", "opus"]), some("codex", &[]), some("gemini", &[]), some("opencode", &[]), some("pi", &[]), some("omp", &[]), some("own", &["deepseek/deepseek-v4.1-flash"])]
}

fn shipped() -> Vec<j4n_core::pack::Info> {
    let catalog = j4n::pack::catalog();
    let ids: Vec<&str> = catalog.ids();
    catalog.harness(&ids, &[]).info
}

#[test]
fn every_shipped_pack_says_when_to_use_it_in_plain_words() {
    let packs = shipped();
    assert!(packs.len() >= 23, "{}", packs.len());
    for p in &packs {
        assert!(p.use_when.trim().len() >= 20, "{}: `use_when` says what a line looks like that the pack serves", p.id);
        let low = p.use_when.to_lowercase();
        assert!(!low.contains("docs/") && !low.contains(".md") && !low.contains("`"), "{}: plain words, no path and no code: {}", p.id, p.use_when);
        assert!(!p.use_when.split(|c: char| !c.is_alphanumeric() && c != '-').any(|w| (w.starts_with("D-") || w.starts_with("J")) && w.len() > 2 && w[1..].chars().all(|c| c.is_ascii_digit() || c == '-')), "{}: no id: {}", p.id, p.use_when);
    }
}

#[test]
fn the_catalogue_of_every_shipped_pack_with_every_pack_on_is_under_1500_tokens_and_names_the_features_a_line_may_need() {
    let c = build(&shipped(), &roster(), &["debug-cause".into(), "log-hunt".into(), "narrow-reads".into()], 1500);
    assert!(c.tokens <= 1500, "{} tokens:\n{}", c.tokens, c.text);
    for id in ["kit", "reflexes", "lanes", "offload", "skills", "accounts", "guard"] {
        assert!(c.packs.iter().any(|p| p == id), "{id} is in it:\n{}", c.text);
    }
    assert!(c.text.contains("use when a goal names many items"), "{}", c.text);
    assert!(!c.packs.iter().any(|p| ["base", "test", "bench", "contribute"].contains(&p.as_str())));
    // the commands of the packs are in it while there is room for them
    assert!(c.text.contains("(:kit)") && !c.text.contains("::"), "{}", c.text);
}
