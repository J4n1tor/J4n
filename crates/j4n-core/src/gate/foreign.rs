//! Another project's state: what a doer in one project must not read of another. The Gate allows a read of any path that is not a credential,
//! and the state folders of the other projects on the machine are not: the transcripts of every project (`~/.claude/projects/<folder>`), a project's
//! own `.j4n/` (its Log, its memory, its folds), and J4n's per-project folders under its home (`projects` and `checkpoints`, each keyed by the project). The worktrees
//! of another project are its sources, which a doer reads as it reads any folder, so they are not named.
//!
//! The doer's **own** state stays readable: the CLI keeps the big results of a tool and the doer's memory in `~/.claude/projects/<its folder>` and tells the
//! model to read them. What is own is read from where the doer works, never from a prefix: the folder of the doer's working directory (`site`) and of the
//! project's root (a lane works in a worktree below J4n's home, so the root is not the site), by whole components (`-home-me-p` is not `-home-me-p-tools`).
//!
//! The judgement is a pure function of a path and [`Own`], so the shell matcher (`danger.rs`, every word of a line) and the path rule (`paths.toml`, read
//! through `action.foreign`) say the same thing.

use crate::state::project_key;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Where the doer is, as far as the Gate knows: the operator's home, J4n's home and the project's root (the Gate is told once, when J4n starts). Empty means
/// nothing is known, and then nothing is own.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Own {
    /// The operator's home folder (`$HOME`), what `~` and `$HOME` are in a word.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// J4n's own folder (`$J4N_HOME`, else `~/.j4n`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub j4n: Option<String>,
    /// The project's root: the folder J4n was started in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
}

impl Own {
    pub fn is_empty(&self) -> bool {
        *self == Own::default()
    }
}

/// How long a folder name under `<config>/projects` may be before the CLI cuts it and adds a hash.
const LONGEST: usize = 200;

/// How the CLI names the folder of a working directory under `<config>/projects`: `replace(/[^a-zA-Z0-9]/g, "-")` of the path (`/home/me/.x` is `-home-me--x`; read
/// from the program's own text, claude 2.1.x). It counts UTF-16 units, as the program does, so a character outside the basic plane is two dashes. A name longer than
/// 200 is cut there and followed by `-` and a hash ([`own_folder`]).
pub fn encode(dir: &str) -> String {
    dir.chars().flat_map(|c| std::iter::repeat_n(if c.is_ascii_alphanumeric() { c } else { '-' }, c.len_utf16())).collect()
}

/// Is `found`, a folder name under `<config>/projects`, the one `mine` (an [`encode`]d working directory) has? Whole names only: no prefix, no glob, no variable
/// (a name that holds one is not a folder name). Two directories that differ only in characters the CLI turns into `-` share a name: its ambiguity, not ours.
fn own_folder(found: &str, mine: &str) -> bool {
    if found.contains(['*', '?', '[', ']', '{', '}', '$', '`']) {
        return false;
    }
    let found = encode(found);
    if mine.len() <= LONGEST {
        found == mine
    } else {
        // a long name is cut at 200 and a hash follows: the first 200 are the folder's
        found.len() > LONGEST && found[..LONGEST] == mine[..LONGEST]
    }
}

/// The folders of the project whose state is the doer's own: the working directory, the root, and a root's keys under J4n's home.
struct Mine {
    folders: Vec<String>,
    keys: Vec<String>,
    roots: Vec<String>,
}

fn mine(site: Option<&str>, own: &Own) -> Mine {
    let dirs: Vec<&str> = [site, own.root.as_deref()].into_iter().flatten().filter(|d| d.starts_with('/')).collect();
    let mut keys: Vec<String> = dirs.iter().map(|d| project_key(Path::new(d.trim_end_matches('/')))).collect();
    // a lane works in `<j4n>/worktrees/<key>/<task>`: its key is the project's
    if let (Some(site), Some(j4n)) = (site, own.j4n.as_deref())
        && let Some(rest) = site.strip_prefix(&format!("{}/worktrees/", j4n.trim_end_matches('/')))
    {
        keys.extend(rest.split('/').next().map(String::from));
    }
    Mine { folders: dirs.iter().map(|d| encode(d.trim_end_matches('/'))).collect(), keys, roots: dirs.iter().map(|d| d.trim_end_matches('/').to_string()).collect() }
}

/// The components of `text` as an absolute path, `~`, `$HOME` and a relative start resolved, `.` and `..` folded; `None` when it cannot be placed (a relative
/// path with no working directory, a `$` or a `~user` that is not the home).
fn components(text: &str, site: Option<&str>, own: &Own, relative: bool) -> Option<Vec<String>> {
    let user = own.user.as_deref().map(|u| u.trim_end_matches('/'));
    let placed = |mark: &str, dir: Option<&str>| text.strip_prefix(mark).filter(|r| r.is_empty() || r.starts_with('/')).and_then(|r| Some(format!("{}{r}", dir?.trim_end_matches('/'))));
    let text = if let Some(t) = placed(SITE, site).or_else(|| placed(ROOT, own.root.as_deref())) {
        t
    } else if text == "~" || text == "$HOME" || text == "${HOME}" {
        format!("{}/", user?)
    } else if let Some(rest) = text.strip_prefix("~/").or_else(|| text.strip_prefix("$HOME/")).or_else(|| text.strip_prefix("${HOME}/")) {
        format!("{}/{rest}", user?)
    } else {
        text.to_string()
    };
    let full = if text.starts_with('/') {
        text
    } else if relative {
        format!("{}/{text}", site?.trim_end_matches('/'))
    } else {
        return None;
    };
    let mut out: Vec<String> = Vec::new();
    for c in full.split('/') {
        match c {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            c => out.push(c.to_string()),
        }
    }
    Some(out)
}

/// Is `text` a path in the worktree the task works in, below J4n's home (`<j4n>/worktrees/<key>/<task>`, the site)? A lane's worktree is the project's
/// sources copied for it, so its files are the task's to write as the project's are; a `.j4n` folder inside it stays J4n's, and another task's worktree is not
/// this one's.
pub fn own_worktree(text: &str, site: Option<&str>, own: &Own) -> bool {
    let (Some(site), Some(j4n)) = (site, own.j4n.as_deref()) else { return false };
    let Some(rest) = site.trim_end_matches('/').strip_prefix(&format!("{}/worktrees/", j4n.trim_end_matches('/'))) else { return false };
    if rest.split('/').filter(|c| !c.is_empty()).count() != 2 {
        return false;
    }
    let (Some(path), Some(root)) = (components(text, Some(site), own, true), components(site, None, own, false)) else { return false };
    path.len() > root.len() && path[..root.len()] == root[..] && !path[root.len()..].iter().any(|c| c == ".j4n" || c == "..")
}

/// Does `text`, one path as a word or a tool names it, lie in the state of a project that is not the doer's own? `site` is the folder the doer works in;
/// `relative` says a path that does not start with `/` or `~` is read from there (a shell word before any `cd`, a tool's relative path).
pub fn foreign(text: &str, site: Option<&str>, own: &Own, relative: bool) -> bool {
    let Some(comps) = components(text, site, own, relative) else { return false };
    let j4n_home = own.j4n.as_deref().map(|j| j.trim_end_matches('/'));
    // most words name nothing of the kind: the keys are hashed only when a marker is there
    if !comps.iter().any(|c| c == "projects" || c == ".j4n") && !j4n_home.is_some_and(|j| text_dir(&comps, j)) {
        return false;
    }
    let me = mine(site, own);
    let at = |i: usize| comps.get(i).map(String::as_str);
    // the transcripts of the projects of a config folder: `<config>/projects/<folder>`; the config folder is `~/.claude`, an account's, or the operator's own
    for i in 0..comps.len() {
        if comps[i] != "projects" {
            continue;
        }
        let claude = i > 0 && comps[i - 1] == ".claude";
        match at(i + 1) {
            // `~/.claude/projects` itself lists every project's folder
            None if claude => return true,
            Some(x) if (claude || x.starts_with('-')) && !me.folders.iter().any(|f| own_folder(x, f)) => return true,
            _ => {}
        }
    }
    // J4n's own folders: `.j4n` under the operator's home or at `own.j4n` keeps the per-project folders and the Log of a project run in the home folder; every
    // other `.j4n` is one project's state
    if let Some(j4n) = j4n_home.filter(|j| text_dir(&comps, j)) {
        let depth = j4n.split('/').filter(|c| !c.is_empty()).count();
        return home_state(&comps[depth..], j4n, &me);
    }
    if let Some(k) = comps.iter().position(|c| c == ".j4n") {
        let parent = format!("/{}", comps[..k].join("/"));
        if own.user.as_deref().is_some_and(|u| u.trim_end_matches('/') == parent) {
            return home_state(&comps[k + 1..], &format!("{parent}/.j4n"), &me);
        }
        // a project's own `.j4n` is judged only when the doer's folder is known (a Gate that was not told it cannot say which project it is)
        let Some(site) = site.filter(|s| s.starts_with('/')) else { return false };
        let site = site.trim_end_matches('/');
        // the doer's own folder, the root's, or a folder above the doer's (a project's state is found from below it) is the project's own
        let mine = parent == site || me.roots.contains(&parent) || site.starts_with(&format!("{parent}/"));
        return !mine;
    }
    false
}

/// Is `dir` (absolute, no trailing `/`) a prefix of the components, whole component by component?
fn text_dir(comps: &[String], dir: &str) -> bool {
    let want: Vec<&str> = dir.split('/').filter(|c| !c.is_empty()).collect();
    want.len() <= comps.len() && want.iter().zip(comps).all(|(w, c)| w == c)
}

/// What lies under J4n's home folder `home`: the per-project folders are the project's own or not; the Log, the memory and the folds there belong to a project
/// run in the home folder, which is the project when the root is the parent of `home`. The rest (the library of skills, the cache, the guard) is the operator's own
/// and global, and is not another project's.
fn home_state(rest: &[String], home: &str, me: &Mine) -> bool {
    match rest.first().map(String::as_str) {
        Some("projects" | "checkpoints") => match rest.get(1) {
            None => true,
            Some(key) => !me.keys.iter().any(|k| k == key),
        },
        Some("log" | "memory.db" | "folds" | "blobs") => {
            let parent = home.rsplit_once('/').map_or("", |(p, _)| p);
            !me.roots.iter().any(|r| r == parent)
        }
        _ => false,
    }
}

/// The path-like pieces of a word: it is cut at what no path holds (a quote, `=`, `:` or `,` of a flag), so `--dir=~/.claude/projects` names the path.
pub fn pieces(word: &str) -> impl Iterator<Item = &str> {
    word.split(|c: char| !(c.is_alphanumeric() || "/._-~$%+*?[]{}@".contains(c))).filter(|p| !p.is_empty())
}

const SITE: &str = "@SITE@";
const ROOT: &str = "@ROOT@";

/// `word` with the doer's own folder and the root written as a placeholder where they stand as a whole path (followed by `/`, the end, or a character no name
/// holds): a folder with a space or a bracket in its name is one place, not several pieces. [`components`] reads the placeholder back.
fn shield(word: &str, site: Option<&str>, own: &Own) -> String {
    let mut out = word.to_string();
    for (dir, mark) in [(site, SITE), (own.root.as_deref(), ROOT)] {
        let Some(dir) = dir.map(|d| d.trim_end_matches('/')).filter(|d| d.len() > 1 && d.starts_with('/')) else { continue };
        let name = |c: char| c.is_alphanumeric() || "._-".contains(c);
        let (mut done, mut rest) = (String::new(), out.as_str());
        while let Some(at) = rest.find(dir) {
            let (before, after) = (rest[..at].chars().next_back().or(done.chars().next_back()), rest[at + dir.len()..].chars().next());
            let whole = before.is_none_or(|c| !(name(c) || c == '/')) && after.is_none_or(|c| !name(c));
            done.push_str(&rest[..at]);
            done.push_str(if whole { mark } else { dir });
            rest = &rest[at + dir.len()..];
        }
        done.push_str(rest);
        out = done;
    }
    out
}

/// Does some piece of `word` lie in another project's state? A relative piece is read from `site` when `relative` (no `cd` came before it in the line).
pub fn foreign_in(word: &str, site: Option<&str>, own: &Own, relative: bool) -> bool {
    pieces(&shield(word, site, own)).any(|p| foreign(p, site, own, relative))
}
