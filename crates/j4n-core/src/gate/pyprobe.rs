//! Whether a doer's inline Python is a probe of the project: `python3 -I -c "from pkg import f; print(f(1))"`, or the same on stdin
//! (`python3 - <<'EOF'`). Such code runs the project's own code as its tests do, and is `safe` when it is static text and names nothing that writes,
//! deletes, starts a process or reaches the network. Unlike [`super::pyread`], which lists the modules that only read and says `read`, this lists what
//! is **refused** and lets the rest through, so a project's own modules can be imported; what it cannot read it refuses, and a refused snippet is
//! what it was (`write`).
//!
//! The code is tokenised, not run: comments dropped, strings kept (the f-strings' expressions among the tokens), and every word is looked at. Refused:
//! a word of [`BAD`] or [`TO_WRITES`] or one that starts like [`PREFIXES`] (the modules that run, write or fetch; the `os` and `pathlib` calls that change a file; the
//! names that reach another name by its string: `eval`, `getattr`, `importlib`), a `__` name but a few, `open` unless it opens a literal path to read,
//! a `write` that is not on `sys.stdout` or `sys.stderr`, a string that is a write mode (`'w'`, `'a'`, `'x'`, `'r+'`, `'wb'`) given to any call but a pure one (`print`, `str`, `join`, `get`, …: [`PURE`]), any character outside ASCII (Python reads `ｅｖａｌ` as `eval`), and an f-string that does not close
//! as a Python before 3.12 would read it. A cell that names `j4n`, the kit's own library (its host calls, `need`, and the bridge `call`, each held by the Gate on its own), is
//! not a probe of the project: it goes the way it went, by `pyread`. On any doubt, not safe.


/// Words that name a module or a call that runs a program, writes, deletes, reaches the network or another name by its string.
const BAD: &str = "eval exec compile importlib builtins getattr setattr delattr globals locals vars breakpoint input help \
    subprocess shutil socket socketserver ssl urllib http requests httpx aiohttp websockets paramiko ftplib smtplib telnetlib poplib imaplib xmlrpc \
    webbrowser asyncio multiprocessing concurrent ctypes cffi pty fcntl mmap resource runpy pkgutil code codeop pip ensurepip venv site sysconfig \
    inspect gc types pickle marshal shelve dbm sqlite3 tempfile zipfile tarfile gzip bz2 lzma zipimport imp pexpect plumbum pdb bdb trace timeit \
    profile cProfile doctest logging \
    system popen Popen kill killpg fork forkpty remove unlink rmdir removedirs rename renames replace makedirs mkdir mkfifo mknod chmod lchmod chown \
    lchown chflags chroot symlink symlink_to hardlink_to link truncate ftruncate utime putenv unsetenv touch write_text write_bytes environ getenv \
    expanduser expandvars home attrgetter methodcaller startfile FileHandler basicConfig tofile memmap read_pickle read_clipboard read_sql \
    read_sql_query read_sql_table read_gbq allow_pickle j4n host _channel jev_many modules connect urlopen urlretrieve FileIO wsgiref setuptools distutils zipapp compileall";
/// The `to_…` calls that write (pandas and its kin); any other `to_…` is a name of the project's (`to_seconds`).
const TO_WRITES: &str = "to_csv to_json to_excel to_parquet to_pickle to_sql to_hdf to_feather to_stata to_clipboard to_latex to_html to_markdown to_xml to_orc to_gbq to_file \
    to_zarr to_netcdf to_msgpack to_records_file to_gif to_image";
/// Names that start like one of those (`execv`, `spawnl`, `posix_spawn`, `urlopen`, `write_csv`, `savefig`, `downloads`, `requests`).
const PREFIXES: [&str; 8] = ["exec", "spawn", "posix_spawn", "write", "save", "dump_to", "fetch", "download"];
/// `__` names a probe may use.
const DUNDER: &str = "__name__ __version__ __doc__ __all__ __file__";
/// The characters of a mode that only reads.
const READ_MODE: &str = "rbtU";

#[derive(Debug, Clone, PartialEq)]
pub(super) enum T {
    Word(String),
    Str(String),
    Punct(char),
    End,
    Num,
}

fn has_word(list: &str, w: &str) -> bool {
    list.split_whitespace().any(|x| x == w)
}

/// The tokens of `code`, or `None` when it cannot be read: a string that does not close, a URL in a string, a character that is not ASCII, an f-string
/// whose braces do not close.
pub(super) fn scan(code: &str, out: &mut Vec<T>) -> Option<()> {
    if code.chars().any(|c| c.is_control() && !matches!(c, '\n' | '\t' | '\r')) {
        return None;
    }
    let c: Vec<char> = code.chars().collect();
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if ch == '#' {
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
        } else if ch == '\n' || ch == ';' {
            out.push(T::End);
            i += 1;
        } else if ch.is_ascii_alphabetic() || ch == '_' {
            let start = i;
            while i < c.len() && (c[i].is_ascii_alphanumeric() || c[i] == '_') {
                i += 1;
            }
            let word: String = c[start..i].iter().collect();
            let quote = c.get(i).copied().filter(|q| *q == '\'' || *q == '"');
            if quote.is_some() && word.len() <= 2 && word.chars().all(|p| "rbfuRBFU".contains(p)) {
                i = string(&c, i, word.contains(['f', 'F']), out)?;
            } else {
                out.push(T::Word(word));
            }
        } else if ch == '\'' || ch == '"' {
            i = string(&c, i, false, out)?;
        } else if ch.is_ascii_digit() {
            while i < c.len() && (c[i].is_ascii_alphanumeric() || c[i] == '.' || c[i] == '_') {
                i += 1;
            }
            out.push(T::Num);
        } else {
            // Python reads an identifier by its compatibility form (`ｅｖａｌ` is `eval`): outside a string or a comment nothing but ASCII is read; inside one it is only
            // text (`'Café'`)
            if !ch.is_ascii() && !ch.is_whitespace() {
                return None;
            }
            if !ch.is_whitespace() {
                out.push(T::Punct(ch));
            }
            i += 1;
        }
    }
    Some(())
}

/// The string that opens at `c[i]`: where it ends. Its body is a token; for an f-string the expressions in it are tokens too, before it.
fn string(c: &[char], i: usize, f: bool, out: &mut Vec<T>) -> Option<usize> {
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
        let (mut depth, mut expr) = (0, String::new());
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
                        scan(&expr, out)?;
                        out.push(T::End);
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
        // a field that is not closed hides what is in it: a Python of 3.12 reads a quote of the same kind inside a field, which ends this string early here
        if depth != 0 {
            return None;
        }
    }
    out.push(T::Str(body));
    Some(j)
}

/// The arguments of the call whose `(` is `toks[open]`: the tokens of each, split at the commas of the call's own level. `None` when it does not close.
pub(super) fn args(toks: &[T], open: usize) -> Option<Vec<&[T]>> {
    let (mut depth, mut from, mut out) = (0usize, open + 1, Vec::new());
    for (k, t) in toks.iter().enumerate().skip(open) {
        match t {
            T::Punct('(' | '[' | '{') => depth += 1,
            T::Punct(')' | ']' | '}') => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    if k > from || !out.is_empty() {
                        out.push(&toks[from..k]);
                    }
                    return Some(out);
                }
            }
            T::Punct(',') if depth == 1 => {
                out.push(&toks[from..k]);
                from = k + 1;
            }
            _ => {}
        }
    }
    None
}

/// A single string that is a mode that only reads (`'r'`, `'rb'`).
fn read_mode(a: &[T]) -> bool {
    matches!(a, [T::Str(s)] if !s.is_empty() && s.chars().all(|c| READ_MODE.contains(c)))
}

/// `name=value`: its name and the tokens of the value (not `a == b`).
fn keyword(a: &[T]) -> Option<(&str, &[T])> {
    match a {
        [T::Word(n), T::Punct('='), rest @ ..] if rest.first() != Some(&T::Punct('=')) => Some((n.as_str(), rest)),
        _ => None,
    }
}

/// A string that is a mode that creates, truncates or appends: up to three of the letters of a mode, one of them `w`, `a`, `x` or `+`.
fn writes_mode(s: &str) -> bool {
    s.len() <= 3 && !s.is_empty() && s.chars().all(|c| "rwaxbtU+".contains(c)) && s.contains(['w', 'a', 'x', '+'])
}

/// The calls that only compute from their arguments: a string of mode letters handed to one is text, not a mode (`print(r['job_id'], 'raw', v)`). The
/// builtins that return a value and the `str` and `dict` methods; any other call that is given such a string after its first argument is asked about.
const PURE: &str = "print format str repr len join get startswith endswith split rsplit replace count find index strip lstrip rstrip append extend add setdefault pop";

/// Is the string at `toks[k]`, an argument of a call, an argument of one of [`PURE`] (not as `mode=`)?
fn pure_argument(toks: &[T], k: usize) -> bool {
    if k >= 2 && toks[k - 1] == T::Punct('=') && toks[k - 2] == T::Word("mode".into()) {
        return false;
    }
    let open = (0..k).rev().try_fold(0usize, |d, j| match &toks[j] {
        T::Punct(')' | ']' | '}') => Ok(d + 1),
        T::Punct('(' | '[' | '{') if d == 0 => Err(j),
        T::Punct('(' | '[' | '{') => Ok(d - 1),
        _ => Ok(d),
    });
    matches!(open, Err(j) if j > 0 && matches!(&toks[j - 1], T::Word(w) if has_word(PURE, w)))
}

/// Is the call of `open` at `toks[k]` one that reads? A plain `open` takes a literal path and, at most, a read mode and the options that change only how
/// the text is read; a method called `open` (`Path.open`, `io.open`, `os.open`) only with no argument or a read mode.
fn opens_to_read(toks: &[T], k: usize) -> bool {
    if !matches!(toks.get(k + 1), Some(T::Punct('('))) {
        return false;
    }
    let Some(list) = args(toks, k + 1) else { return false };
    if k > 0 && toks[k - 1] == T::Punct('.') {
        return list.is_empty() || (list.len() == 1 && read_mode(list[0]));
    }
    let positional: Vec<&&[T]> = list.iter().filter(|a| keyword(a).is_none()).collect();
    let path_is_text = matches!(positional.first(), Some(a) if matches!(***a, [T::Str(_)]));
    let mode_ok = positional.get(1).is_none_or(|a| read_mode(a));
    let keywords_ok = list.iter().filter_map(|a| keyword(a)).all(|(n, v)| match n {
        "mode" => read_mode(v),
        "encoding" | "errors" | "newline" | "buffering" => true,
        _ => false,
    });
    path_is_text && positional.len() <= 2 && mode_ok && keywords_ok
}

/// For each token: is the innermost bracket around it the parenthesis of a call (`f(`, `a.b(`, `f(x)(`), and not a list, a tuple, a dict or a group?
fn call_context(toks: &[T]) -> Vec<bool> {
    const KEYWORDS: &str = "in is not and or if else elif while for return yield lambda assert del raise with as from import print";
    let (mut stack, mut out): (Vec<bool>, Vec<bool>) = (Vec::new(), Vec::with_capacity(toks.len()));
    for (k, t) in toks.iter().enumerate() {
        out.push(stack.last().copied().unwrap_or(false));
        match t {
            T::Punct('(') => {
                let call = k > 0 && match &toks[k - 1] {
                    T::Word(w) => !has_word(KEYWORDS, w) || w == "print",
                    T::Punct(')' | ']') => true,
                    _ => false,
                };
                stack.push(call);
            }
            T::Punct('[' | '{') => stack.push(false),
            T::Punct(')' | ']' | '}') => {
                stack.pop();
            }
            _ => {}
        }
    }
    out
}

/// Whether `code` is a probe: static text that names nothing that writes, deletes, starts a process or reaches the network. `Some` is the string
/// literals of the code, joined, for the caller to look for the names of credentials a path built from pieces would hide.
pub fn probe(code: &str) -> Option<String> {
    let mut toks = Vec::new();
    scan(code, &mut toks)?;
    let mut text = String::new();
    let in_call = call_context(&toks);
    for (k, t) in toks.iter().enumerate() {
        match t {
            T::Str(s) => {
                // a mode that creates or truncates, handed to any call (`io.FileIO('f', 'w')`, `h5py.File('f', mode='a')`): not only to `open`. It is an argument of a call
                // that is not the first (after a `,`) or is a keyword's (after a `=`); a list that holds `'x'` or a `'x' * 40` is no call
                if writes_mode(s)
                    && in_call[k]
                    && matches!(k.checked_sub(1).map(|p| &toks[p]), Some(T::Punct(',' | '=')))
                    && matches!(toks.get(k + 1), Some(T::Punct(',' | ')')))
                    && !pure_argument(&toks, k)
                {
                    return None;
                }
                text.push_str(s);
            }
            T::Word(w) => {
                let on_stdout = k >= 2 && toks[k - 1] == T::Punct('.') && matches!(&toks[k - 2], T::Word(s) if s == "stdout" || s == "stderr");
                let allowed = match w.as_str() {
                    "open" => opens_to_read(&toks, k),
                    "write" | "writelines" => on_stdout,
                    d if d.starts_with("__") => has_word(DUNDER, d),
                    _ => false,
                };
                let bad = has_word(BAD, w) || PREFIXES.iter().any(|p| w.starts_with(p)) || has_word(TO_WRITES, w) || w == "open" || w.starts_with("__");
                if bad && !allowed {
                    return None;
                }
            }
            _ => {}
        }
    }
    Some(text)
}
