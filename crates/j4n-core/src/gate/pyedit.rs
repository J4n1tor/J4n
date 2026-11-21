//! What a doer's Python on the line does to files, for `approval.inside`: the code is read, never run. A doer edits files with
//! `python3 - <<'EOF'` and `p='src/a.py'; s=open(p).read(); …; open(p,'w').write(s)`; such code stays inside the project when every file it opens is
//! named by a literal path inside it, and nothing else in it can reach a file, a program or the network.
//!
//! - [`Edit::Asks`]: the code names what runs a program, reaches the network, deletes or renames, reads the environment or reaches a name by its string
//!   ([`ASKS`], `os.` but `os.path`, a `__` name), opens a path outside the project (absolute elsewhere, `~`, `..`, `.git`, `.j4n`), or cannot be
//!   read (a string that does not close, a web address in a string, a character that is not ASCII outside a string). The operator is asked.
//! - [`Edit::Unsure`]: the code may stay inside, but its paths are built while it runs (an f-string in a loop, `os.path.join`, `pathlib`), it opens
//!   through a method, or it imports a module this reader does not know. Only this is put to a Decider.
//! - [`Edit::Inside`]: every `open(…)` takes a string literal, or a name every binding of which is a string literal (`p='…'`), inside the project.
//!
//! Bindings are read flow-blind: a name is a literal only when every statement that binds it is `name = 'literal'`; a name that is bound in any other
//! way anywhere (a loop, `as`, a tuple, `+=`, a parameter, an import, `global`, `del`) is not, wherever it is used. With [`MODULES`] the only
//! modules, the builtin `open` is the only door to a file.

use super::pyprobe::{T, args, scan};
use std::collections::BTreeMap;

/// What the code is, for `approval.inside`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    Inside,
    Unsure(String),
    Asks(String),
}

/// Words that run a program, reach the network, delete, rename or change a file's mode, read the environment or reach a name by its string,
/// wherever they stand. `compile` is not among them (`re.compile`): what would run what it makes is.
pub const ASKS: &str = "subprocess socket ssl urllib urllib3 http requests httpx aiohttp websockets websocket paramiko fabric ftplib smtplib telnetlib \
    poplib imaplib xmlrpc webbrowser ctypes cffi pty pexpect multiprocessing asyncio concurrent shutil pickle marshal shelve dill boto3 botocore docker \
    kubernetes eval exec importlib builtins globals locals vars getattr setattr delattr breakpoint environ getenv putenv unsetenv popen Popen fork \
    forkpty kill killpg unlink rmdir removedirs rmtree rename renames chmod chown lchown symlink symlink_to hardlink_to chroot mkfifo mknod startfile \
    urlopen urlretrieve expanduser expandvars modules";
/// Words that ask only when they are called bare, as `from os import remove` lets them be: after a `.` they are a list's or a file's (`l.remove(x)`).
pub const BARE: &str = "remove system truncate ftruncate";
/// The modules code may import and still be read: none of them opens a file, a process or a socket.
pub const MODULES: &str = "re json textwrap difflib string collections itertools functools sys typing";

/// What `code` does to files, run in the project folder `site`.
pub fn read(code: &str, site: Option<&str>) -> Edit {
    let mut toks = Vec::new();
    if scan(code, &mut toks).is_none() {
        return Edit::Asks("its code cannot be read (a string that does not close, a web address, a character that is not ASCII outside a string)".into());
    }
    let mut unsure: Option<String> = None;
    let mut doubt = |why: String| {
        unsure.get_or_insert(why);
    };
    for (k, t) in toks.iter().enumerate() {
        let next = |n: usize| toks.get(k + n);
        match t {
            T::Word(w) if has_word(ASKS, w) => return Edit::Asks(format!("its code names `{w}`")),
            T::Word(w) if has_word(BARE, w) && (k == 0 || toks[k - 1] != T::Punct('.')) => return Edit::Asks(format!("its code names `{w}`")),
            T::Word(w) if w.starts_with("__") && w != "__name__" => return Edit::Asks(format!("its code names `{w}`")),
            T::Word(w) if w == "os" => match (next(1), next(2)) {
                (Some(T::Punct('.')), Some(T::Word(m))) if m == "path" => doubt("it builds a path with `os.path`".into()),
                (Some(T::Punct('.')), Some(T::Word(m))) => return Edit::Asks(format!("its code calls `os.{m}`")),
                _ => doubt("it imports `os`".into()),
            },
            T::Word(w) if w == "Path" || w == "pathlib" => doubt("it opens files through `pathlib`".into()),
            T::Punct(':') if next(1) == Some(&T::Punct('=')) => doubt("it binds a name inside an expression (`:=`)".into()),
            _ => {}
        }
    }
    let statements = statements(&toks);
    for st in &statements {
        if let Some(m) = imported(st).into_iter().find(|m| !has_word(MODULES, m)) {
            doubt(format!("it imports `{m}`"));
        }
    }
    // a path an f-string makes of names a loop binds only to literals is each path it can make, read as literals from here on
    let toks = spelled(&toks, &bindings(&statements));
    let binds = bindings(&self::statements(&toks));
    if binds.contains_key("open") || binds.contains_key("print") {
        return Edit::Asks("its code binds `open` or `print` to something else".into());
    }
    for (k, t) in toks.iter().enumerate() {
        if *t != T::Word("open".into()) || toks.get(k + 1) != Some(&T::Punct('(')) {
            continue;
        }
        if k > 0 && toks[k - 1] == T::Punct('.') {
            doubt("it opens a file through a method".into());
            continue;
        }
        let Some(list) = args(&toks, k + 1) else { return Edit::Asks("its code cannot be read (a call that does not close)".into()) };
        let paths: Option<Vec<String>> = match list.first() {
            Some([T::Str(s)]) => Some(vec![s.clone()]),
            Some([T::Word(n)]) => binds.get(n).cloned().flatten(),
            _ => None,
        };
        let Some(paths) = paths else {
            doubt("it opens a path built while it runs".into());
            continue;
        };
        for p in paths {
            if let Err(e) = path(&p, site) {
                return e;
            }
        }
    }
    // code a Decider would be put: a string that starts like a path outside the project (a system folder, the home folder, `~`), climbs out by `..`
    // or names `.git` or `.j4n` is asked about by code, wherever it stands (the text of an f-string that builds a path is one)
    if unsure.is_some()
        && let Some(s) = toks.iter().find_map(|t| match t {
            T::Str(s) if leaves(s, site) => Some(s),
            _ => None,
        })
    {
        return Edit::Asks(format!("its code names `{}`, outside the project", s.chars().take(80).collect::<String>()));
    }
    unsure.map_or(Edit::Inside, Edit::Unsure)
}

/// The folders at the root of a system: a string that starts with one of them names a place outside the project, unless the project is below it.
const SYSTEM: &str = "etc home root usr var tmp opt proc sys dev bin sbin boot lib lib64 mnt media srv run Users";

/// Does the literal `s` look like a path that leaves the project in `site`?
fn leaves(s: &str, site: Option<&str>) -> bool {
    let site = site.unwrap_or("").trim_end_matches('/');
    let below = !site.is_empty() && (s == site || s.starts_with(&format!("{site}/")));
    let first = s.strip_prefix('/').map(|r| r.split('/').next().unwrap_or(""));
    s.starts_with('~')
        || (first.is_some_and(|f| has_word(SYSTEM, f)) && !below)
        || s.split(['/', '\\']).any(|c| c == ".." || c.eq_ignore_ascii_case(".git") || c.eq_ignore_ascii_case(".j4n"))
}

fn has_word(list: &str, w: &str) -> bool {
    list.split_whitespace().any(|x| x == w)
}

/// A literal path the code opens: inside the project, or why not.
fn path(p: &str, site: Option<&str>) -> Result<(), Edit> {
    let out = |why: &str| Err(Edit::Asks(format!("it opens `{p}`, {why}")));
    if p.is_empty() {
        return Err(Edit::Unsure("it opens an empty path".into()));
    }
    if p.starts_with('~') || p.contains(['$', '\0']) {
        return out("outside the project");
    }
    let Some(site) = site.map(|s| s.trim_end_matches('/')).filter(|s| s.starts_with('/') && s.len() > 1) else { return out("and the line has no project folder") };
    let rel = match p.strip_prefix('/') {
        Some(_) => match p.strip_prefix(site).and_then(|r| r.strip_prefix('/')) {
            Some(r) => r,
            None => return out("outside the project"),
        },
        None => p,
    };
    let parts: Vec<&str> = rel.split('/').collect();
    if parts.contains(&"..") {
        return out("which leaves the project by `..`");
    }
    if parts.iter().any(|c| c.eq_ignore_ascii_case(".git") || c.eq_ignore_ascii_case(".j4n")) {
        return out("inside `.git` or `.j4n`");
    }
    if !super::real_under(site, &format!("{site}/{rel}")) {
        return out("whose real path is outside the project");
    }
    Ok(())
}

/// The statements of the tokens: cut at an end of line or a `;` outside brackets.
fn statements(toks: &[T]) -> Vec<&[T]> {
    let (mut out, mut from, mut depth) = (Vec::new(), 0, 0usize);
    for (k, t) in toks.iter().enumerate() {
        match t {
            T::Punct('(' | '[' | '{') => depth += 1,
            T::Punct(')' | ']' | '}') => depth = depth.saturating_sub(1),
            T::End if depth == 0 => {
                if k > from {
                    out.push(&toks[from..k]);
                }
                from = k + 1;
            }
            _ => {}
        }
    }
    if toks.len() > from {
        out.push(&toks[from..]);
    }
    out
}

/// The modules a statement imports: `import a, b.c as d` names `a` and `b`, `from a.b import c` names `a`.
fn imported(st: &[T]) -> Vec<String> {
    let word = |t: &T| if let T::Word(w) = t { Some(w.clone()) } else { None };
    match st.first().and_then(word).as_deref() {
        Some("from") => st.get(1).and_then(word).into_iter().collect(),
        Some("import") => {
            let mut out = Vec::new();
            let mut take = true;
            for t in &st[1..] {
                match t {
                    T::Word(w) if take => {
                        out.push(w.clone());
                        take = false;
                    }
                    T::Punct(',') => take = true,
                    _ => {}
                }
            }
            out
        }
        _ => Vec::new(),
    }
}

/// Each name the code binds: `Some` of the literals, when every binding of it is `name = 'literal'`; `None` when any binding is anything else.
fn bindings(statements: &[&[T]]) -> BTreeMap<String, Option<Vec<String>>> {
    let mut binds: BTreeMap<String, Option<Vec<String>>> = BTreeMap::new();
    let dynamic = |binds: &mut BTreeMap<_, _>, n: &str| {
        binds.insert(n.to_string(), None);
    };
    for st in statements {
        let words = || st.iter().filter_map(|t| if let T::Word(w) = t { Some(w.as_str()) } else { None });
        // a statement that binds in a way of its own binds every name in it: a definition and its parameters, a lambda's, `global`, `del`, `case`,
        // an import
        if words().any(|w| has_word("def class lambda global nonlocal del case import from", w)) {
            words().for_each(|w| dynamic(&mut binds, w));
            continue;
        }
        if let Some(eq) = assignment(st) {
            match st {
                [T::Word(n), T::Punct('='), T::Str(s)] => {
                    let entry = binds.entry(n.clone()).or_insert_with(|| Some(Vec::new()));
                    if let Some(list) = entry {
                        list.push(s.clone());
                    }
                }
                _ => st[..eq].iter().for_each(|t| {
                    if let T::Word(w) = t {
                        dynamic(&mut binds, w);
                    }
                }),
            }
        }
        // `for a, b in …` (a comprehension's too) and `… as a`
        let mut k = 0;
        while k < st.len() {
            match &st[k] {
                T::Word(w) if w == "for" => {
                    let from = k + 1;
                    while k < st.len() && st[k] != T::Word("in".into()) {
                        k += 1;
                    }
                    let names: Vec<String> = st[from..k].iter().filter_map(|t| if let T::Word(w) = t { Some(w.clone()) } else { None }).collect();
                    // a loop over a written-out sequence of literals binds each name to the literals at its place; any other loop, dynamically
                    let sets = looped(&st[from..k], st.get(k + 1..).unwrap_or_default(), names.len());
                    for (i, n) in names.iter().enumerate() {
                        match sets.as_ref().and_then(|s| s[i].clone()) {
                            Some(vals) => {
                                if let Some(list) = binds.entry(n.clone()).or_insert_with(|| Some(Vec::new())) {
                                    list.extend(vals);
                                }
                            }
                            None => dynamic(&mut binds, n),
                        }
                    }
                }
                T::Word(w) if w == "as" => {
                    if let Some(T::Word(n)) = st.get(k + 1) {
                        dynamic(&mut binds, n);
                    }
                }
                _ => {}
            }
            k += 1;
        }
    }
    binds
}

/// The most paths an f-string is spelled out as.
const SPELLED: usize = 64;

/// `toks` with each `name = f'…{a}…'` whose every field is a bare name `binds` holds only literals for spelled out as one literal assignment for
/// each path it can make (at most [`SPELLED`]), so that the binder reads them as literals. Any other f-string is left as it is.
fn spelled(toks: &[T], binds: &BTreeMap<String, Option<Vec<String>>>) -> Vec<T> {
    let mut out = Vec::with_capacity(toks.len());
    let mut k = 0;
    while k < toks.len() {
        if let Some((name, end, paths)) = fassign(toks, k, binds) {
            for (i, p) in paths.into_iter().enumerate() {
                if i > 0 {
                    out.push(T::End);
                }
                out.extend([T::Word(name.clone()), T::Punct('='), T::Str(p)]);
            }
            k = end;
            continue;
        }
        out.push(toks[k].clone());
        k += 1;
    }
    out
}

/// The f-string assignment that starts a statement at `toks[k]`: its name, where it ends, and the paths it makes. The scan gives an f-string's fields
/// as tokens before its body, each ended: `p`, `=`, `f`, end, `'src/{f}.py'`.
fn fassign(toks: &[T], k: usize, binds: &BTreeMap<String, Option<Vec<String>>>) -> Option<(String, usize, Vec<String>)> {
    if k > 0 && toks[k - 1] != T::End {
        return None;
    }
    let T::Word(name) = &toks[k] else { return None };
    if toks.get(k + 1) != Some(&T::Punct('=')) || toks.get(k + 2) == Some(&T::Punct('=')) {
        return None;
    }
    let mut j = k + 2;
    let mut fields = Vec::new();
    while let (Some(T::Word(x)), Some(T::End)) = (toks.get(j), toks.get(j + 1)) {
        fields.push(x.as_str());
        j += 2;
    }
    let Some(T::Str(body)) = toks.get(j) else { return None };
    if fields.is_empty() || !matches!(toks.get(j + 1), None | Some(T::End)) {
        return None;
    }
    let (mut paths, mut rest, mut at) = (vec![String::new()], body.as_str(), 0);
    while let Some(open) = rest.find('{') {
        let close = open + rest[open..].find('}')?;
        let (lit, field) = (&rest[..open], &rest[open + 1..close]);
        if lit.contains('}') || fields.get(at) != Some(&field) {
            return None;
        }
        let vals = binds.get(field)?.as_ref().filter(|v| !v.is_empty())?;
        paths = paths.iter().flat_map(|p| vals.iter().map(move |v| format!("{p}{lit}{v}"))).collect();
        if paths.len() > SPELLED {
            return None;
        }
        at += 1;
        rest = &rest[close + 1..];
    }
    (at == fields.len() && !rest.contains('}')).then(|| (name.clone(), j + 1, paths.into_iter().map(|p| p + rest).collect()))
}

/// What a `for` over a written-out sequence binds, `n` names after `for` (`head`) and the tokens after `in` (`tail`): for each name, the literals it
/// takes, or `None` where one is not a string literal. `None` when the sequence is not written out to its `:`.
fn looped(head: &[T], tail: &[T], n: usize) -> Option<Vec<Option<Vec<String>>>> {
    if n == 0 || head.iter().any(|t| !matches!(t, T::Word(_) | T::Punct(',' | '(' | ')'))) {
        return None;
    }
    let close = matching(tail)?;
    if tail.get(close + 1) != Some(&T::Punct(':')) {
        return None;
    }
    let items: Vec<&[T]> = commas(&tail[1..close]).into_iter().filter(|i| !i.is_empty()).collect();
    let mut sets: Vec<Option<Vec<String>>> = vec![Some(Vec::new()); n];
    for item in items {
        let parts = if n == 1 { vec![item] } else { commas(&item[1..matching(item)?]).into_iter().filter(|p| !p.is_empty()).collect() };
        if n > 1 && (matching(item)? + 1 != item.len() || parts.len() != n) {
            return None;
        }
        for (i, part) in parts.into_iter().enumerate() {
            match (part, &mut sets[i]) {
                ([T::Str(s)], Some(list)) => list.push(s.clone()),
                (_, set) => *set = None,
            }
        }
    }
    Some(sets)
}

/// Where the bracket that opens `toks` closes.
fn matching(toks: &[T]) -> Option<usize> {
    if !matches!(toks.first(), Some(T::Punct('(' | '['))) {
        return None;
    }
    let mut depth = 0usize;
    for (k, t) in toks.iter().enumerate() {
        match t {
            T::Punct('(' | '[' | '{') => depth += 1,
            T::Punct(')' | ']' | '}') => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(k);
                }
            }
            _ => {}
        }
    }
    None
}

/// `toks` cut at its commas outside brackets.
fn commas(toks: &[T]) -> Vec<&[T]> {
    let (mut out, mut from, mut depth) = (Vec::new(), 0, 0usize);
    for (k, t) in toks.iter().enumerate() {
        match t {
            T::Punct('(' | '[' | '{') => depth += 1,
            T::Punct(')' | ']' | '}') => depth = depth.saturating_sub(1),
            T::Punct(',') if depth == 0 => {
                out.push(&toks[from..k]);
                from = k + 1;
            }
            _ => {}
        }
    }
    out.push(&toks[from..]);
    out
}

/// The place of the last `=` of a statement that assigns (an augmented one too, `+=`), outside brackets and not part of `==`, `!=`, `<=`, `>=`.
fn assignment(st: &[T]) -> Option<usize> {
    let mut depth = 0usize;
    let mut last = None;
    for (k, t) in st.iter().enumerate() {
        match t {
            T::Punct('(' | '[' | '{') => depth += 1,
            T::Punct(')' | ']' | '}') => depth = depth.saturating_sub(1),
            T::Punct('=') if depth == 0 => {
                let before = k.checked_sub(1).map(|j| &st[j]);
                let compare = matches!(before, Some(T::Punct('=' | '!' | '<' | '>'))) || st.get(k + 1) == Some(&T::Punct('='));
                if !compare {
                    last = Some(k);
                }
            }
            _ => {}
        }
    }
    last
}
