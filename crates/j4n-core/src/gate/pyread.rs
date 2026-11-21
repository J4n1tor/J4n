//! Whether a `python -c` snippet only reads: the code is read, never run. It reads when it imports only modules of
//! [`MODULES`] and names nothing that writes, runs, opens or fetches (the prefixes and names of [`NAMES`] and [`PREFIXES`]; a
//! `__` name other than `__version__`; a string with `://`). What it cannot read is not a read: an unclosed string, relative
//! imports, anything else it does not know. This is a list of what is known to read, not a sandbox: the line is still `read`, never
//! `safe`, so a mode that allows reads lets it through and the rest still ask. `j4n.call` and `j4n.many` (the library's bridge) read:
//! the tool each call names is held by the Gate on its own; `j4n.fetch`, `host` and the other host calls stay `exec`.

/// Modules a snippet may import (the first part of a dotted name); `j4n` is the kit's own library, whose code layer only computes.
const MODULES: &str = "sys math cmath json csv re statistics collections itertools functools operator datetime decimal fractions typing \
    string textwrap pprint copy numbers random time platform warnings j4n numpy pandas scipy sklearn pgmpy networkx sympy";
/// Names that run a program, open a file or a socket, change a file, or reach a name by its string.
const NAMES: &str = "open os io subprocess shutil pathlib importlib ctypes builtins eval exec compile getattr setattr delattr globals locals \
    vars input breakpoint help system popen run call check_call check_output Popen kill remove unlink rmdir rmtree removedirs rename \
    renames mkdir makedirs touch chmod chown symlink link truncate savefig tofile memmap socket urlopen urlretrieve connect attrgetter \
    methodcaller read_pickle read_clipboard read_sql read_sql_query read_sql_table read_gbq modules host _channel jev_many";
/// Names that start like one of those.
const PREFIXES: [&str; 8] = ["write", "save", "dump", "open", "spawn", "fetch", "download", "request"];
/// `to_…` writes in pandas and friends, except these, which return the value.
pub(super) const TO_READS: &str = "to_numpy to_list to_dict to_frame to_series to_string to_datetime to_numeric to_timedelta to_period to_timestamp \
    to_records to_flat_index to_pydatetime";

#[derive(Debug, PartialEq)]
enum Tok {
    Word(String),
    Dot,
    Comma,
    /// End of a statement: a newline or `;`.
    End,
    Other,
}

/// The tokens of `code` outside comments and strings; the strings with the expressions of the f-strings among the tokens. `None`
/// when a string is not closed or a string holds a URL.
fn tokens(code: &str) -> Option<Vec<Tok>> {
    let c: Vec<char> = code.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if ch == '#' {
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
        } else if ch == '\n' || ch == ';' {
            out.push(Tok::End);
            i += 1;
        } else if ch.is_alphabetic() || ch == '_' {
            // Python reads a name in other letters as the name it is the compatibility form of (`ｅｖａｌ` is `eval`): outside a string or a comment, text that is not ASCII
            // is not read; inside one it is only text
            let start = i;
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_') {
                i += 1;
            }
            let word: String = c[start..i].iter().collect();
            if !word.is_ascii() {
                return None;
            }
            let quote = c.get(i).copied().filter(|q| *q == '\'' || *q == '"');
            if quote.is_some() && word.len() <= 2 && word.chars().all(|p| "rbfuRBFU".contains(p)) {
                i = string(&c, i, word.to_lowercase().contains('f'), &mut out)?;
            } else {
                out.push(Tok::Word(word));
            }
        } else if ch == '\'' || ch == '"' {
            i = string(&c, i, false, &mut out)?;
        } else if ch.is_ascii_digit() {
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '.' || c[i] == '_') {
                i += 1;
            }
            out.push(Tok::Other);
        } else {
            match ch {
                '.' => out.push(Tok::Dot),
                ',' => out.push(Tok::Comma),
                c if c.is_whitespace() => {}
                _ => out.push(Tok::Other),
            }
            i += 1;
        }
    }
    Some(out)
}

/// The string that opens at `c[i]`: where it ends, with the expressions of an f-string pushed as code.
fn string(c: &[char], i: usize, f: bool, out: &mut Vec<Tok>) -> Option<usize> {
    let q = c[i];
    let triple = c.get(i + 1) == Some(&q) && c.get(i + 2) == Some(&q);
    let mut j = i + if triple { 3 } else { 1 };
    let mut body = String::new();
    loop {
        let ch = *c.get(j)?;
        if ch == '\\' {
            j += 2;
            continue;
        }
        if ch == q && (!triple || (c.get(j + 1) == Some(&q) && c.get(j + 2) == Some(&q))) {
            j += if triple { 3 } else { 1 };
            break;
        }
        if ch == '\n' && !triple {
            return None;
        }
        body.push(ch);
        j += 1;
    }
    if body.contains("://") {
        return None;
    }
    if f {
        let mut depth = 0;
        let mut expr = String::new();
        let chars: Vec<char> = body.chars().collect();
        let mut k = 0;
        while k < chars.len() {
            match chars[k] {
                '{' if depth == 0 && chars.get(k + 1) == Some(&'{') => k += 1,
                '}' if depth == 0 && chars.get(k + 1) == Some(&'}') => k += 1,
                '{' => {
                    depth += 1;
                    if depth > 1 {
                        expr.push('{');
                    }
                }
                '}' if depth > 0 => {
                    depth -= 1;
                    if depth == 0 {
                        out.extend(tokens(&expr)?);
                        out.push(Tok::End);
                        expr.clear();
                    } else {
                        expr.push('}');
                    }
                }
                x if depth > 0 => expr.push(x),
                _ => {}
            }
            k += 1;
        }
    }
    out.push(Tok::Other);
    Some(j)
}

fn has_word(list: &str, w: &str) -> bool {
    list.split_whitespace().any(|x| x == w)
}

/// A name that writes, runs, opens, fetches or reaches another name by its string.
fn bad(w: &str) -> bool {
    (w.starts_with("__") && w != "__version__") || has_word(NAMES, w) || PREFIXES.iter().any(|p| w.starts_with(p)) || (w.starts_with("to_") && !has_word(TO_READS, w))
}

/// Whether `code`, the text of a `python -c`, only reads.
pub fn reads(code: &str) -> bool {
    let Some(toks) = tokens(code) else { return false };
    let mut k = 0;
    while k < toks.len() {
        let Tok::Word(w) = &toks[k] else {
            k += 1;
            continue;
        };
        // the library's own `j4n.call` is the bridge: each call is its own Gate question as the doer's model call, so the cell is no `exec`
        let bridge = w == "call" && k >= 2 && toks[k - 1] == Tok::Dot && matches!(&toks[k - 2], Tok::Word(m) if m == "j4n") && !matches!(k.checked_sub(3).map(|i| &toks[i]), Some(Tok::Dot));
        if bad(w) && !bridge {
            return false;
        }
        if w != "import" && w != "from" {
            k += 1;
            continue;
        }
        // `import a.b as c, d` · `from a.b import c`: the first part of each module must be a known one
        let mut j = k + 1;
        loop {
            match toks.get(j) {
                Some(Tok::Word(m)) if has_word(MODULES, m) => {}
                _ => return false,
            }
            while let Some(t @ (Tok::Word(_) | Tok::Dot)) = toks.get(j) {
                if matches!(t, Tok::Word(x) if x == "import" || x == "as") {
                    break;
                }
                if matches!(t, Tok::Word(x) if bad(x)) {
                    return false;
                }
                j += 1;
            }
            if w == "from" {
                break;
            }
            if matches!(toks.get(j), Some(Tok::Word(x)) if x == "as") {
                j += 2;
            }
            if toks.get(j) != Some(&Tok::Comma) {
                break;
            }
            j += 1;
        }
        if w == "from" {
            if !matches!(toks.get(j), Some(Tok::Word(x)) if x == "import") {
                return false;
            }
            j += 1;
        }
        k = j;
    }
    true
}
