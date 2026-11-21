//! What the value fields understand of what is typed: durations, dollars, numbers, globs, expressions, names. Pure functions of text;
//! a word that cannot be read is answered with the words of the message the field shows.

/// `90s`, `5m`, `1h`, `1h30m`, `1m 30s`, `2d`: seconds. A number without a unit is not a duration.
pub fn dur(s: &str) -> Option<u64> {
    let mut total = 0u64;
    let (mut digits, mut any) = (String::new(), false);
    for c in s.trim().chars().filter(|c| !c.is_whitespace()) {
        if c.is_ascii_digit() {
            digits.push(c);
            continue;
        }
        let n: u64 = digits.parse().ok()?;
        digits.clear();
        total = total.checked_add(n.checked_mul(match c {
            's' => 1,
            'm' => 60,
            'h' => 3600,
            'd' => 86400,
            _ => return None,
        })?)?;
        any = true;
    }
    (any && digits.is_empty()).then_some(total)
}

/// Seconds as words: `1m 30s`, `2h`, `45s`.
pub fn dur_words(secs: u64) -> String {
    let parts = [(secs / 86400, 'd'), (secs % 86400 / 3600, 'h'), (secs % 3600 / 60, 'm'), (secs % 60, 's')];
    let words: Vec<String> = parts.iter().filter(|(n, _)| *n > 0).map(|(n, u)| format!("{n}{u}")).collect();
    if words.is_empty() { "0s".into() } else { words.join(" ") }
}

/// Seconds as a short duration: the largest unit that holds it whole (`5m`, `90s`, `2h`).
pub fn dur_short(secs: u64) -> String {
    for (size, unit) in [(86400, 'd'), (3600, 'h'), (60, 'm')] {
        if secs >= size && secs.is_multiple_of(size) {
            return format!("{}{unit}", secs / size);
        }
    }
    format!("{secs}s")
}

/// `$5`, `5`, `$2.50`, `-1`: dollars.
pub fn usd(s: &str) -> Option<f64> {
    let t = s.trim().trim_start_matches('$').trim();
    let x: f64 = t.parse().ok()?;
    x.is_finite().then_some(x)
}

/// Dollars as the field shows them: `$5.00`, `$0.0125`. Cents, and more places only when the amount needs them.
pub fn money(x: f64) -> String {
    let cents = (x.abs() * 100.0).round() / 100.0;
    let places = if (x.abs() - cents).abs() < 1e-9 { 2 } else { 4 };
    format!("{}${:.places$}", if x < 0.0 { "-" } else { "" }, x.abs())
}

/// A number as it is read: no trailing zeros, no point after a whole number.
pub fn num(x: f64) -> String {
    let s = format!("{x:.6}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Where a glob is not closed: `open [ at 1`. Positions count from 1.
pub fn glob(s: &str) -> Option<String> {
    let mut open: Vec<(char, usize)> = Vec::new();
    let mut escaped = false;
    for (i, c) in s.chars().enumerate() {
        if std::mem::take(&mut escaped) {
            continue;
        }
        match c {
            '\\' => escaped = true,
            '[' if !open.iter().any(|(o, _)| *o == '[') => open.push((c, i + 1)),
            ']' => {
                if let Some(p) = open.iter().position(|(o, _)| *o == '[') {
                    open.remove(p);
                }
            }
            '{' => open.push((c, i + 1)),
            '}' => {
                if let Some(p) = open.iter().rposition(|(o, _)| *o == '{') {
                    open.remove(p);
                }
            }
            _ => {}
        }
    }
    open.first().map(|(c, at)| format!("open {c} at {at}"))
}

/// Where an expression has a bracket or a quote that is not closed: `( at 6 not closed`.
pub fn expr(s: &str) -> Option<String> {
    let mut stack: Vec<(char, usize)> = Vec::new();
    let mut quote: Option<(char, usize)> = None;
    for (i, c) in s.chars().enumerate() {
        if let Some((q, _)) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '\'' | '"' => quote = Some((c, i + 1)),
            '(' | '[' | '{' => stack.push((c, i + 1)),
            ')' | ']' | '}' => {
                let want = match c {
                    ')' => '(',
                    ']' => '[',
                    _ => '{',
                };
                if stack.last().map(|(o, _)| *o) != Some(want) {
                    return Some(format!("{c} at {} has no {want}", i + 1));
                }
                stack.pop();
            }
            _ => {}
        }
    }
    quote.map(|(q, at)| format!("{q} at {at} not closed")).or_else(|| stack.first().map(|(c, at)| format!("{c} at {at} not closed")))
}

/// A name for a key of a map: letters, digits, `_`, `-` and `.`.
pub fn name(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

/// The holes of an argument: `{pat}` and `{a}{b}` give `pat`, and `a` and `b`.
pub fn holes(arg: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = arg;
    while let Some(i) = rest.find('{') {
        let Some(j) = rest[i..].find('}') else { break };
        out.push(rest[i + 1..i + j].to_string());
        rest = &rest[i + j + 1..];
    }
    out
}

/// The range `lo–hi`, or the one bound that is set (`≤ 8`, `≥ 1`).
pub fn range(lo: Option<f64>, hi: Option<f64>) -> String {
    match (lo, hi) {
        (Some(a), Some(b)) => format!("{}–{}", num(a), num(b)),
        (None, Some(b)) => format!("≤ {}", num(b)),
        (Some(a), None) => format!("≥ {}", num(a)),
        (None, None) => String::new(),
    }
}
