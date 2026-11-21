//! The version line: `0.1.0 (0123abcd 2026-01-01)`. The commit and its date are stamped by `build.rs`; a build outside git is `0.1.0`.

/// The version, then the commit and its date when the build knew them.
pub fn line() -> String {
    let number = env!("CARGO_PKG_VERSION");
    match (option_env!("J4N_COMMIT"), option_env!("J4N_DATE")) {
        (Some(commit), Some(date)) => format!("{number} ({commit} {date})"),
        _ => number.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::line;

    fn digits(s: &str, n: Option<usize>) -> bool {
        !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && n.is_none_or(|n| s.len() == n)
    }

    #[test]
    fn the_line_is_a_number_then_the_commit_and_its_date_when_known() {
        let line = line();
        let (number, stamp) = line.split_once(' ').map_or((line.as_str(), None), |(n, s)| (n, Some(s)));
        let parts: Vec<&str> = number.split('.').collect();
        assert!(parts.len() == 3 && parts.iter().all(|p| digits(p, None)), "{line}");
        let Some(stamp) = stamp else { return };
        let inner = stamp.strip_prefix('(').and_then(|s| s.strip_suffix(')')).expect(&line);
        let (commit, date) = inner.split_once(' ').expect(&line);
        assert!(commit.len() == 8 && commit.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()), "{line}");
        let d: Vec<&str> = date.split('-').collect();
        assert!(d.len() == 3 && digits(d[0], Some(4)) && digits(d[1], Some(2)) && digits(d[2], Some(2)), "{line}");
    }
}
