//! Law 14, clean agent text. No model-bound template of a pack words a budget or claims a missing capability: budget is
//! enforced outside the text (routing, caps, parking) and what an agent can do is the tools offered this turn. Model-bound text is every file in a `personas`, `skills` or `prompts` directory of a pack, and every string
//! under a model-bound key in the pack's other TOML files. A file of either kind that is not UTF-8 text is a violation.

use crate::support::{assert_clean, root, walk};
use std::fs;
use std::path::Path;

/// Directories of a pack whose files are all sent to a model.
const MODEL_DIRS: [&str; 3] = ["personas", "skills", "prompts"];

/// Keys whose strings reach a model, at any depth of a pack's TOML: a task's `goal` (a flow step),
/// the `prompt`, `system` and `template` of a connector or a Brief section, the text of a `brief`, a
/// `persona`, a seat's `personality` and `reports`, or a `skill`, `instruction`(s) and a Rule's `steer`. A compound key is bound when one of its words,
/// split at `_` and `-`, is here: `system_prompt`, `prompt_template`, `user_prompt`. Not here, deliberately: a Rule's
/// `notice` goes to the operator, not to the model, and so do `why`, `question` and `summary`, which cards and menus
/// show (the soft-cap menu asks the operator about money). Strings under these keys in a table or an array count too,
/// except under an `OPERATOR_KEYS` key.
const MODEL_BOUND_KEYS: [&str; 13] = [
    "brief",
    "goal",
    "instruction",
    "instructions",
    "persona",
    "personality",
    "prompt",
    "reports",
    "skill",
    "skills",
    "steer",
    "system",
    "template",
];

/// Keys that name or label a thing for the operator rather than word it for the model. Strings under them are not
/// scanned even inside a model-bound table or array: a `[[persona]]` holds an `id`, a `summary` and a `why` beside its
/// text. Outside such a table they are not model-bound anyway.
const OPERATOR_KEYS: [&str; 6] = ["id", "label", "notice", "question", "summary", "why"];

/// Budget words and phrases, as whole words in the form `normalise` leaves them.
const BUDGET: [&str; 44] = [
    "budget",
    "budgets",
    "budgeted",
    "budgeting",
    "quota",
    "quotas",
    "cost",
    "costs",
    "costly",
    "costed",
    "costing",
    "cheap",
    "cheaper",
    "cheapest",
    "cheaply",
    "expensive",
    "inexpensive",
    "spend",
    "spends",
    "spending",
    "spent",
    "overspend",
    "overspending",
    "overspent",
    "usd",
    "dollar",
    "dollars",
    "tokens left",
    "tokens remaining",
    "remaining tokens",
    "out of tokens",
    "token limit",
    "token limits",
    "context window",
    "running out",
    "running low",
    "save tokens",
    "saving tokens",
    "save your tokens",
    "conserve tokens",
    "waste tokens",
    "wasting tokens",
    "wasted tokens",
    "waste your tokens",
];

/// Negative capability claims, as whole-word phrases in the form `normalise` leaves them.
const NEGATIVE: [&str; 28] = [
    "you cannot",
    "you can not",
    "you can't",
    "you are unable",
    "you're unable",
    "you are not able",
    "you're not able",
    "you aren't able",
    "you won't be able",
    "you will not be able",
    "will be unable",
    "you do not have",
    "you don't have",
    "you have no",
    "you lack",
    "no access to",
    "no network access",
    "no internet access",
    "no shell access",
    "has no access",
    "have no access",
    "cannot access",
    "can't access",
    "without access",
    "do not have access",
    "don't have access",
    "does not have access",
    "doesn't have access",
];

/// Adverbs that pad a claim without changing it: "you do not currently have" says "you do not have".
const FILLER: [&str; 7] = ["currently", "really", "simply", "actually", "presently", "now", "just"];

/// Words that say what an agent tells the operator; a `never`, `do not` or `don't` before one forbids the claim after it.
const SPEECH: [&str; 7] = ["say", "tell", "claim", "pretend", "state", "reply", "write"];

/// The most words between that speech verb and the claim: "never tell the operator you cannot" has two.
const PROHIBITION_REACH: usize = 3;

/// A noun that "you have no … to" frees an obligation with: "you have no need to ask".
const FREED: [&str; 4] = ["need", "obligation", "reason", "choice"];

const BUDGET_RULE: &str = "budget wording — budget is enforced outside the text (routing, caps, parking)";
const CLAIM_RULE: &str = "a negative capability claim — capability is the tools offered this turn";

/// Lowercase words joined by single spaces: curly apostrophes made straight, everything but letters, digits and the
/// apostrophes inside a word turned into a gap, and a possessive `'s` dropped ("the budget's gone" is "the budget
/// gone"). A plural possessive ends in the apostrophe, which is a gap already.
fn normalise(text: &str) -> String {
    let lowered: String = text
        .chars()
        .flat_map(char::to_lowercase)
        .map(|c| if matches!(c, '\u{2018}' | '\u{2019}' | '\u{02bc}') { '\'' } else { c })
        .collect();
    let words: Vec<&str> = lowered
        .split(|c: char| !c.is_alphanumeric() && c != '\'')
        .map(|word| word.trim_matches('\''))
        .map(|word| word.strip_suffix("'s").unwrap_or(word))
        .filter(|word| !word.is_empty())
        .collect();
    words.join(" ")
}

/// One sentence as the matcher reads it: normalised, filler adverbs dropped, a space round every word.
fn plain(sentence: &str) -> String {
    let normalised = normalise(sentence);
    let words: Vec<&str> = normalised.split(' ').filter(|word| !FILLER.contains(word)).collect();
    format!(" {} ", words.join(" "))
}

/// `phrase` frees an obligation and claims no missing capability when `rest` (the text after it) goes on with "to":
/// "you do not have to ask", "you have no need to ask".
fn frees_an_obligation(phrase: &str, rest: &str) -> bool {
    if phrase.ends_with(" have") {
        return rest.starts_with(" to ");
    }
    phrase.ends_with(" have no") && FREED.iter().any(|noun| rest.starts_with(&format!(" {noun} to ")))
}

/// What comes `before` the phrase forbids it: a `never`, `do not` or `don't`, a speech verb, and at most
/// `PROHIBITION_REACH` words, as in "don't tell the user you can't".
fn is_forbidden(before: &str) -> bool {
    let words: Vec<&str> = before.split(' ').filter(|word| !word.is_empty()).collect();
    (words.len().saturating_sub(PROHIBITION_REACH + 1)..words.len()).any(|at| {
        let head = &words[..at];
        SPEECH.contains(&words[at])
            && (head.ends_with(&["never"]) || head.ends_with(&["don't"]) || head.ends_with(&["do", "not"]))
    })
}

/// The space-padded sentence holds `phrase` as whole words, and it is a claim: not an obligation freed, not forbidden.
fn says(padded: &str, phrase: &str) -> bool {
    let start = format!(" {phrase}");
    padded.match_indices(&start).any(|(at, _)| {
        let rest = &padded[at + start.len()..];
        rest.starts_with(' ') && !frees_an_obligation(phrase, rest) && !is_forbidden(&padded[..at])
    })
}

/// The violations in one piece of model-bound `text`, found in `file` under `key` (`"file"` for the whole file). It is
/// read a sentence at a time, so a prohibition or an obligation never reaches over a full stop into the next one.
fn text_violations(file: &str, key: &str, text: &str) -> Vec<String> {
    let sentences: Vec<String> = text.split(['.', '!', '?', ';']).map(plain).collect();
    let mut found = Vec::new();
    for (phrases, rule) in [(&BUDGET[..], BUDGET_RULE), (&NEGATIVE[..], CLAIM_RULE)] {
        for phrase in phrases.iter().filter(|phrase| sentences.iter().any(|sentence| says(sentence, phrase))) {
            found.push(format!("{file} ({key}): \"{phrase}\" is {rule}"));
        }
    }
    found
}

/// `key`, or one of its words split at `_` and `-`, is in `MODEL_BOUND_KEYS`.
fn is_model_bound(key: &str) -> bool {
    key.split(['_', '-']).any(|word| MODEL_BOUND_KEYS.contains(&word))
}

/// The strings under a model-bound key of `table`, each with its key path (`flow[0].steps[1].goal`).
fn bound_strings<'a>(table: &'a toml::Table, prefix: &str, bound: bool, out: &mut Vec<(String, &'a str)>) {
    for (key, value) in table {
        if bound && OPERATOR_KEYS.contains(&key.as_str()) {
            continue;
        }
        let path = if prefix.is_empty() { key.clone() } else { format!("{prefix}.{key}") };
        collect(value, &path, bound || is_model_bound(key), out);
    }
}

fn collect<'a>(value: &'a toml::Value, path: &str, bound: bool, out: &mut Vec<(String, &'a str)>) {
    match value {
        toml::Value::String(text) if bound => out.push((path.to_owned(), text)),
        toml::Value::Array(items) => {
            for (n, item) in items.iter().enumerate() {
                collect(item, &format!("{path}[{n}]"), bound, out);
            }
        }
        toml::Value::Table(table) => bound_strings(table, path, bound, out),
        _ => {}
    }
}

/// The violations in the model-bound keys of a TOML file; a file that does not parse cannot be shown clean.
fn toml_violations(file: &str, text: &str) -> Vec<String> {
    let table: toml::Table = match text.parse() {
        Ok(table) => table,
        Err(e) => return vec![format!("{file}: not TOML, so its model-bound keys cannot be read: {}", e.message())],
    };
    let mut strings = Vec::new();
    bound_strings(&table, "", false, &mut strings);
    strings.iter().flat_map(|(key, text)| text_violations(file, key, text)).collect()
}

/// `file` (relative to the repository root) is inside a `personas`, `skills` or `prompts` directory of its pack.
fn in_model_dir(file: &str) -> bool {
    let mut dirs: Vec<&str> = file.split('/').skip(2).collect();
    dirs.pop();
    dirs.iter().any(|dir| MODEL_DIRS.contains(dir))
}

/// The violations in one file of `packs/`: all of it when it sits in a model directory, only its model-bound keys
/// when it is another TOML file, nothing otherwise. A file of either kind that is not UTF-8 cannot be shown clean.
fn file_violations(file: &str, bytes: &[u8]) -> Vec<String> {
    let whole_file = in_model_dir(file);
    if !whole_file && !file.ends_with(".toml") {
        return Vec::new();
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        return vec![format!("{file}: not UTF-8 text, so it cannot be shown clean")];
    };
    // the frontmatter of a skill (`name`, `when`, `cost`) names it for the operator and the picker; the words under it are what a model reads
    let text = if file.ends_with("/SKILL.md") { text.strip_prefix("---\n").and_then(|t| t.split_once("\n---")).map_or(text, |(_, body)| body) } else { text };
    if whole_file { text_violations(file, "file", text) } else { toml_violations(file, text) }
}

/// `path` relative to `base`, with `/` separators.
fn relative(base: &Path, path: &Path) -> String {
    path.strip_prefix(base).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

/// The violations in every file under `base/packs`, each file named relative to `base`.
fn scan(base: &Path) -> Vec<String> {
    walk(&base.join("packs"))
        .iter()
        .flat_map(|path| {
            let file = relative(base, path);
            file_violations(&file, &fs::read(path).unwrap_or_else(|e| panic!("read {file}: {e}")))
        })
        .collect()
}

#[test]
fn model_bound_text_in_packs_is_clean() {
    assert_clean("law 14 (clean agent text)", &scan(&root()));
}

fn flagged(text: &str) -> Vec<String> {
    text_violations("packs/x/personas/p.md", "file", text)
}

fn assert_flags(text: &str, phrase: &str) {
    let found = flagged(text);
    let wanted = format!("\"{phrase}\" is ");
    assert!(found.iter().any(|message| message.contains(&wanted)), "expected {wanted} in {found:?} for: {text}");
}

#[test]
fn every_listed_phrase_is_written_the_way_normalise_leaves_text() {
    for phrase in BUDGET.iter().chain(NEGATIVE.iter()) {
        assert_eq!(normalise(phrase), *phrase);
    }
}

#[test]
fn normalise_lowercases_straightens_apostrophes_and_collapses_gaps() {
    assert_eq!(normalise("  You  CAN’T\n\tpush, ‘ever’!  "), "you can't push ever");
    assert_eq!(normalise("a**b**_c {d.e}"), "a b c d e");
}

#[test]
fn every_budget_word_is_caught() {
    for phrase in BUDGET {
        assert_flags(&format!("Please mind the {phrase} here."), phrase);
    }
}

#[test]
fn every_negative_capability_claim_is_caught() {
    for phrase in NEGATIVE {
        assert_flags(&format!("Note that {phrase} the network."), phrase);
    }
}

#[test]
fn realistic_sentences_are_caught() {
    assert_flags("You have 500 tokens left, so keep it short.", "tokens left");
    assert_flags("Stay within the token limit.", "token limit");
    assert_flags("Do not spend more than you need.", "spend");
    assert_flags("This path is cheap and that one is expensive.", "cheap");
    assert_flags("The context window is nearly full; we are running low.", "running low");
    assert_flags("You cannot push to the remote.", "you cannot");
    assert_flags("You don't have write access.", "you don't have");
    assert_flags("The sandbox has no access to the network.", "no access to");
    assert_flags("Pretend you lack a shell.", "you lack");
}

#[test]
fn case_curly_apostrophes_markdown_and_line_breaks_do_not_hide_a_phrase() {
    assert_flags("YOU\n   CANNOT push", "you cannot");
    assert_flags("You can\u{2019}t push", "you can't");
    assert_flags("You **cannot** push", "you cannot");
    assert_flags("the 'budget' is gone", "budget");
    assert_flags("Report {fold.spend.today} to the model", "spend");
    assert_flags("Be careful with token\nlimits", "token limits");
}

#[test]
fn a_possessive_does_not_hide_a_budget_word() {
    assert_flags("Once the budget's used up, stop.", "budget");
    assert_flags("The quota\u{2019}s nearly gone.", "quota");
    assert_flags("the cost\u{02bc}s too high", "cost");
    assert_flags("Add up the budgets' total.", "budgets");
}

#[test]
fn an_inflected_budget_word_is_caught() {
    for (text, phrase) in [
        ("The task is budgeted at ten steps.", "budgeted"),
        ("We are budgeting for this.", "budgeting"),
        ("Costed at ten.", "costed"),
        ("It is costing us.", "costing"),
        ("Cheaply done.", "cheaply"),
        ("Prefer inexpensive options.", "inexpensive"),
        ("Do not overspend.", "overspend"),
        ("Avoid overspending.", "overspending"),
        ("He overspent.", "overspent"),
        ("Avoid wasting tokens.", "wasting tokens"),
        ("He wasted tokens.", "wasted tokens"),
        ("Do not waste your tokens.", "waste your tokens"),
        ("Conserve tokens.", "conserve tokens"),
    ] {
        assert_flags(text, phrase);
    }
}

#[test]
fn instructions_and_permissions_are_not_capability_claims() {
    for text in [
        "Do not push to the remote.",
        "Never write outside the worktree.",
        "Ask the operator before you push.",
        "You may read any file under the project root.",
        "Use only the tools offered this turn.",
        "You do not have to ask before reading.",
        "You don't have to repeat the question.",
        "You do not currently have to ask.",
        "You are not allowed to run commands.",
        "Reply with the word ready.",
    ] {
        assert_eq!(flagged(text), Vec::<String>::new(), "for: {text}");
    }
}

#[test]
fn a_phrase_inside_a_longer_word_is_no_match() {
    for text in ["A costume.", "A quotation.", "The usdz file.", "Respent hours.", "Cheapskate.", "Visit Costa Rica."] {
        assert_eq!(flagged(text), Vec::<String>::new(), "for: {text}");
    }
}

#[test]
fn a_free_obligation_does_not_hide_a_real_claim_after_it() {
    let found = flagged("You do not have to ask. You do not have write access.");
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("\"you do not have\""), "{found:?}");
}

#[test]
fn a_filler_word_or_a_future_modal_does_not_hide_a_negative_claim() {
    for (text, phrase) in [
        ("You do not currently have a shell.", "you do not have"),
        ("You don't currently have a shell.", "you don't have"),
        ("You currently cannot browse.", "you cannot"),
        ("You really can't push.", "you can't"),
        ("You are not currently able to push.", "you are not able"),
        ("You won't be able to run commands.", "you won't be able"),
        ("You will not be able to run commands.", "you will not be able"),
        ("The worker has no network access.", "no network access"),
        ("There is no network access.", "no network access"),
        ("The agent has no access.", "has no access"),
    ] {
        assert_flags(text, phrase);
    }
}

#[test]
fn a_free_obligation_with_no_need_reason_choice_or_duty_claims_nothing() {
    for text in [
        "You have no need to ask permission before reading files.",
        "You have no obligation to explain.",
        "You have no reason to wait.",
        "You have no choice to make here.",
    ] {
        assert_eq!(flagged(text), Vec::<String>::new(), "for: {text}");
    }
}

#[test]
fn a_claim_that_a_prohibition_forbids_is_no_claim() {
    for text in [
        "Never say you cannot push.",
        "Never tell the operator you cannot do something the offered tools can do.",
        "Don't tell the user you can't see their screen.",
        "Do not claim you have no shell.",
        "Do not say you cannot push.",
    ] {
        assert_eq!(flagged(text), Vec::<String>::new(), "for: {text}");
    }
}

#[test]
fn a_prohibition_does_not_hide_a_real_claim_in_the_next_sentence_or_far_after_it() {
    for (text, phrase) in [
        ("Never say yes. You cannot push.", "you cannot"),
        ("Never say yes; you cannot push.", "you cannot"),
        ("Do not say much more than needed because you cannot push.", "you cannot"),
        ("You have no need to ask. You have no write access.", "you have no"),
    ] {
        let found = flagged(text);
        assert_eq!(found.len(), 1, "{found:?} for: {text}");
        assert!(found[0].contains(&format!("\"{phrase}\" is ")), "{found:?} for: {text}");
    }
}

#[test]
fn a_violation_names_the_file_the_key_the_phrase_and_the_rule() {
    let claim = concat!(
        "packs/x/personas/p.md (file): \"you cannot\" is a negative capability claim — ",
        "capability is the tools offered this turn"
    );
    assert_eq!(text_violations("packs/x/personas/p.md", "file", "You cannot push."), [claim]);
    let budget = concat!(
        "packs/x/flows.toml (flow[0].goal): \"budget\" is budget wording — ",
        "budget is enforced outside the text (routing, caps, parking)"
    );
    assert_eq!(text_violations("packs/x/flows.toml", "flow[0].goal", "Mind the budget."), [budget]);
}

#[test]
fn a_model_bound_key_is_found_at_any_depth() {
    let text = "api = 1\n[[flow]]\nid = \"w\"\nsteps = [{ id = \"plan\", goal = \"You cannot push.\" }]\n";
    let found = toml_violations("packs/x/flows.toml", text);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].starts_with("packs/x/flows.toml (flow[0].steps[0].goal): \"you cannot\""), "{found:?}");
}

#[test]
fn strings_under_a_model_bound_table_or_array_are_scanned() {
    let text = "persona = [\"Be brief.\", \"Mind the budget.\"]\n[prompt]\nuser = \"You lack a shell.\"\n";
    let found = toml_violations("packs/x/a.toml", text);
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found.iter().any(|m| m.contains("(persona[1])") && m.contains("\"budget\"")), "{found:?}");
    assert!(found.iter().any(|m| m.contains("(prompt.user)") && m.contains("\"you lack\"")), "{found:?}");
}

#[test]
fn every_model_bound_key_is_scanned() {
    for key in MODEL_BOUND_KEYS {
        let found = toml_violations("packs/x/a.toml", &format!("[[thing]]\n{key} = \"Mind the budget.\"\n"));
        assert_eq!(found.len(), 1, "{key}: {found:?}");
        assert!(found[0].contains(&format!("(thing[0].{key})")), "{found:?}");
    }
}

#[test]
fn a_compound_key_is_model_bound_when_one_of_its_words_is() {
    for key in ["system_prompt", "prompt_template", "user_prompt", "persona-text", "first_goal"] {
        let found = toml_violations("packs/x/a.toml", &format!("[[thing]]\n{key} = \"Mind the budget.\"\n"));
        assert_eq!(found.len(), 1, "{key}: {found:?}");
        assert!(found[0].contains(&format!("(thing[0].{key})")), "{found:?}");
    }
    for key in ["context", "description", "systematic", "templates"] {
        let found = toml_violations("packs/x/a.toml", &format!("[[thing]]\n{key} = \"Mind the budget.\"\n"));
        assert_eq!(found, Vec::<String>::new(), "{key}");
    }
}

#[test]
fn a_skill_is_model_bound_like_a_persona() {
    let found = toml_violations(
        "packs/x/a.toml",
        "[[skill]]\nname = \"Mind the budget.\"\n[skills]\ngit = \"You lack a shell.\"\n",
    );
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found.iter().any(|m| m.contains("(skill[0].name)")), "{found:?}");
    assert!(found.iter().any(|m| m.contains("(skills.git)")), "{found:?}");
}

#[test]
fn the_operators_text_beside_the_models_in_a_bound_table_is_not_scanned() {
    for key in OPERATOR_KEYS {
        let text =
            format!("[[persona]]\n{key} = \"A cheap pass: you cannot spend more.\"\ntext = \"Review the diff.\"\n");
        assert_eq!(toml_violations("packs/x/a.toml", &text), Vec::<String>::new(), "key {key}");
    }
    let text = "[[persona]]\nid = \"cost-reviewer\"\nsummary = \"A cheap pass\"\ntext = \"Mind the budget.\"\n";
    let found = toml_violations("packs/x/a.toml", text);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("(persona[0].text)"), "{found:?}");
}

#[test]
fn text_for_the_operator_is_not_model_bound() {
    for key in OPERATOR_KEYS.into_iter().chain(["description"]) {
        let text = format!("{key} = \"Over the budget: you cannot spend more. Raise the cap?\"\n");
        assert_eq!(toml_violations("packs/x/a.toml", &text), Vec::<String>::new(), "key {key}");
    }
}

#[test]
fn a_steer_is_scanned_while_the_notice_beside_it_is_not() {
    let text = "[[rule]]\nthen = \"steer\"\nsteer = \"Keep going.\"\nnotice = \"The budget is used up.\"\n";
    assert_eq!(toml_violations("packs/x/rules.toml", text), Vec::<String>::new());
    let text = "[[rule]]\nthen = \"steer\"\nsteer = \"Keep going; the budget is used up.\"\nnotice = \"Steered.\"\n";
    assert_eq!(toml_violations("packs/x/rules.toml", text).len(), 1);
}

#[test]
fn values_that_are_not_strings_are_ignored_and_a_file_that_is_not_toml_is_a_violation() {
    assert_eq!(toml_violations("packs/x/a.toml", "goal = 5\nprompt = [1, 2.5, true]\n"), Vec::<String>::new());
    let found = toml_violations("packs/x/a.toml", "goal = \n");
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].starts_with("packs/x/a.toml: not TOML"), "{found:?}");
}

#[test]
fn model_directories_are_recognised_inside_a_pack_only() {
    for file in [
        "packs/code/personas/reviewer.md",
        "packs/code/skills/git/SKILL.md",
        "packs/agent/prompts/system.txt",
        "packs/code/tests/prompts/case.toml",
    ] {
        assert!(in_model_dir(file), "{file}");
    }
    for file in [
        "packs/code/pack.toml",
        "packs/skills/pack.toml",
        "packs/code/flows/personas",
        "packs/code/bin/prompts.sh",
        "packs/code/review.toml",
    ] {
        assert!(!in_model_dir(file), "{file}");
    }
}

#[test]
fn a_file_in_a_model_directory_is_scanned_whole_once() {
    let found =
        file_violations("packs/x/prompts/a.toml", b"body = \"Mind the budget.\"\ngoal = \"You cannot push.\"\n");
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found.iter().all(|m| m.contains("(file)")), "{found:?}");
    assert_eq!(file_violations("packs/x/skills/git/SKILL.md", b"Mind the budget.").len(), 1);
    // the frontmatter of a skill is not read by a model: its `cost` is a number of tokens; the words under it are
    assert!(file_violations("packs/x/skills/git/SKILL.md", b"---\nname: git\nwhen: commit\ncost: 90\n---\nCommit small.\n").is_empty());
    assert_eq!(file_violations("packs/x/skills/git/SKILL.md", b"---\nname: git\nwhen: commit\ncost: 90\n---\nMind the budget.\n").len(), 1);
}

#[test]
fn a_toml_file_elsewhere_is_scanned_by_key_and_other_files_not_at_all() {
    assert_eq!(file_violations("packs/x/flows.toml", b"summary = \"cost\"\nbody = \"budget\"\n"), Vec::<String>::new());
    assert_eq!(file_violations("packs/x/flows.toml", b"template = \"cost\"\n").len(), 1);
    assert_eq!(file_violations("packs/x/README.md", b"You cannot mind the budget."), Vec::<String>::new());
}

/// Build `tree` (file name relative to a fresh directory, bytes) in the temp directory, `scan` it, remove it again.
fn scanned(name: &str, tree: &[(&str, &[u8])]) -> Vec<String> {
    let base = std::env::temp_dir().join(format!("j4n-clean-text-{}-{name}", std::process::id()));
    for (file, bytes) in tree {
        let path = base.join(file);
        fs::create_dir_all(path.parent().expect("file below the tree")).expect("temp tree");
        fs::write(&path, bytes).expect("temp file");
    }
    let found = scan(&base);
    fs::remove_dir_all(&base).expect("remove temp tree");
    found
}

#[test]
fn the_scan_reports_model_bound_files_and_keys_of_a_tree_by_their_path_below_it() {
    let found = scanned(
        "tree",
        &[
            ("packs/x/pack.toml", b"api = 1\nid = \"x\"\nsummary = \"Over the budget\"\n"),
            ("packs/x/personas/p.md", b"You cannot push. Mind the budget.\n"),
            ("packs/x/flows.toml", b"[[flow]]\nsteps = [{ id = \"a\", goal = \"Spend less.\" }]\n"),
            ("packs/x/README.md", b"You cannot mind the budget.\n"),
            ("packs/x/skills/git/SKILL.md", b"Be brief.\n"),
            ("other/personas/p.md", b"You cannot push.\n"),
        ],
    );
    let claim = "a negative capability claim — capability is the tools offered this turn";
    let budget = "budget wording — budget is enforced outside the text (routing, caps, parking)";
    assert_eq!(
        found,
        [
            format!("packs/x/flows.toml (flow[0].steps[0].goal): \"spend\" is {budget}"),
            format!("packs/x/personas/p.md (file): \"budget\" is {budget}"),
            format!("packs/x/personas/p.md (file): \"you cannot\" is {claim}"),
        ]
    );
}

#[test]
fn a_file_that_is_not_utf8_in_a_model_directory_or_a_toml_file_is_a_violation() {
    let found = scanned(
        "bytes",
        &[
            ("packs/x/personas/p.md", b"Mind the budget.\xff\n"),
            ("packs/x/rules.toml", b"api = 1\n[[rule]]\nsteer = \"Mind the budget.\"\n\xff\n"),
            ("packs/x/bin/tool", b"\xff\xfe"),
            ("packs/x/README.md", b"\xff\xfe"),
        ],
    );
    assert_eq!(
        found,
        [
            "packs/x/personas/p.md: not UTF-8 text, so it cannot be shown clean",
            "packs/x/rules.toml: not UTF-8 text, so it cannot be shown clean",
        ]
    );
}

#[test]
fn a_seats_own_words_and_its_report_line_are_model_bound() {
    for (key, text, phrase) in [("personality", "Stay inside the budget.", "budget"), ("personality", "You cannot run the tests.", "you cannot"), ("reports", "Say what you spent.", "spent")] {
        let found = toml_violations("packs/x/seats/s.toml", &format!("[[seat]]\nid = \"s\"\n{key} = \"{text}\"\n"));
        assert_eq!(found.len(), 1, "{key}: {found:?}");
        assert!(found[0].contains(&format!("(seat[0].{key})")) && found[0].contains(&format!("\"{phrase}\"")), "{found:?}");
    }
}

#[test]
fn the_shipped_seats_are_read_word_for_word_and_are_clean() {
    let path = root().join("packs/crew/seats/seats.toml");
    let text = fs::read_to_string(&path).expect("the shipped seats");
    assert_eq!(toml_violations("packs/crew/seats/seats.toml", &text), Vec::<String>::new());
    // six seats, each with its own words and its report line, all of them scanned
    let table: toml::Table = text.parse().unwrap();
    let mut strings = Vec::new();
    bound_strings(&table, "", false, &mut strings);
    let keys: Vec<&str> = strings.iter().map(|(k, _)| k.rsplit('.').next().unwrap()).collect();
    assert_eq!((keys.iter().filter(|k| **k == "personality").count(), keys.iter().filter(|k| **k == "reports").count()), (6, 5), "{keys:?}");
    assert!(strings.iter().filter(|(k, _)| k.ends_with(".personality")).all(|(_, t)| t.chars().count() <= 1200));
}
