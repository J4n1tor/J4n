//! The claim check of the paragraph `mixed` asks for (a rule, against the facts instead of the
//! files): every path, sha, number and test name the paragraph says must be in the facts J4n wrote. A number rounded to the places the
//! paragraph gives counts; number words are not read.

/// What is around a word and no part of it.
const AROUND: &[char] = &['"', '\'', '“', '”', '‘', '’', '(', ')', '[', ']', '{', '}', '<', '>', ',', ';', ':', '!', '?', '`', '*'];

fn is_sha(w: &str) -> bool {
    (7..=40).contains(&w.len()) && w.chars().all(|c| c.is_ascii_hexdigit()) && w.chars().any(|c| c.is_ascii_digit()) && w.chars().any(|c| c.is_ascii_alphabetic())
}

/// A word that names a file: it holds a `/`, or a stem of two or more and an extension of letters.
fn is_path(w: &str) -> bool {
    w.contains('/') || w.rsplit_once('.').is_some_and(|(stem, ext)| stem.chars().count() >= 2 && (1..=5).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphabetic()))
}

fn is_test(w: &str) -> bool {
    w.contains("::") || (w.contains('_') && w.chars().all(|c| c.is_alphanumeric() || c == '_'))
}

/// The number a word says, without `$`, `#`, `,`, `%` or a sign.
fn number(w: &str) -> Option<f64> {
    let n: String = w.chars().filter(|c| !matches!(c, '$' | '#' | ',' | '%' | '+')).collect();
    n.parse::<f64>().ok().filter(|n| n.is_finite())
}

/// Every number the words of a text say.
fn numbers(text: &str) -> Vec<f64> {
    text.split_whitespace().filter_map(|w| number(w.trim_matches(AROUND).trim_end_matches('.').trim_matches(AROUND))).collect()
}

/// Whether `said` is a number of the facts, as it stands or rounded to as many places as `said` gives.
fn holds_number(word: &str, said: f64, facts: &[f64]) -> bool {
    let places = word.rsplit_once('.').map_or(0, |(_, p)| p.chars().filter(char::is_ascii_digit).count()) as i32;
    let round = |f: f64| (f * 10f64.powi(places)).round() / 10f64.powi(places);
    facts.iter().any(|f| (*f - said).abs() < 1e-9 || (round(*f) - said).abs() < 1e-9)
}

/// The words of `paragraph` that claim what the facts do not hold.
pub fn unknown(paragraph: &str, facts: &str) -> Vec<String> {
    let nums = numbers(facts);
    let mut out: Vec<String> = Vec::new();
    for raw in paragraph.split_whitespace() {
        let w = raw.trim_matches(AROUND).trim_end_matches('.').trim_matches(AROUND);
        if w.is_empty() {
            continue;
        }
        let held = if is_sha(w) {
            facts.to_lowercase().contains(&w.to_lowercase())
        } else if is_path(w) || is_test(w) {
            facts.contains(w)
        } else if w.chars().any(|c| c.is_ascii_digit()) {
            match number(w) {
                Some(n) => holds_number(w, n, &nums),
                None => facts.contains(w),
            }
        } else {
            true
        };
        if !held && !out.iter().any(|o| o == w) {
            out.push(w.to_string());
        }
    }
    out
}
