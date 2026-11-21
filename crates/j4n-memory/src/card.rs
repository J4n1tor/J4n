//! The project card: the stable facts of the project folder, derived from its files with
//! no model, so that a doer does not research the folder again on every task. It is one section of the Brief, `project`, cut to
//! [`CARD_TOKENS`], and only behind the setting `brief.project_card` (on by default).
//!
//! Each fact keeps the hash of the file it came from. Before the card is cut every source is read and hashed again: a fact whose
//! hash changed, or whose source went away, is dropped and made again from the new file. What changes with every task (the diff,
//! the results of a test, the plan) has no source here, so it is never in the card.

use j4n_core::brief::tokens;
use j4n_core::front::ProjectFacts;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// What the card as a Brief section may take, the `## project` heading included.
pub const CARD_TOKENS: usize = 300;
/// The heading and the card's first line, which a cut keeps room for.
const HEAD: &str = "Project card, derived from the files:";
const SKIP: [&str; 9] = ["target", "node_modules", "dist", "build", "vendor", "venv", "__pycache__", "out", "bin"];
/// The files a manifest line is made from, and the targets of a `Makefile` that say how the work is run.
const MANIFESTS: [&str; 5] = ["Cargo.toml", "package.json", "pyproject.toml", "go.mod", "Makefile"];
const TARGETS: [&str; 6] = ["build", "test", "check", "lint", "all", "run"];
const READ_MAX: u64 = 256 * 1024;

/// One fact and where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fact {
    /// The file or folder it was made from, relative to the project.
    pub source: String,
    /// The first eight bytes of the SHA-256 of what the source held, in hex.
    pub hash: String,
    pub line: String,
    /// Where it stands in the card, and so what is cut last: manifests, the tree, the scripts, the docs.
    rank: u8,
}

pub struct ProjectCard {
    root: PathBuf,
    /// The settings as they are now.
    settings: j4n_core::setting::Live,
    facts: Mutex<Vec<Fact>>,
}

fn cut(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max { text.to_string() } else { format!("{}…", text.chars().take(max - 1).collect::<String>().trim_end()) }
}

/// The names in a folder, sorted, without what is hidden or built; a folder gets a `/`. Links are not followed.
fn names(dir: &Path) -> Vec<String> {
    let mut all: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let (name, kind) = (e.file_name().to_string_lossy().into_owned(), e.file_type().ok()?);
            (!name.starts_with('.') && !SKIP.contains(&name.as_str()) && !kind.is_symlink()).then(|| if kind.is_dir() { format!("{name}/") } else { name })
        })
        .collect();
    all.sort();
    all
}

fn read(path: &Path) -> Option<String> {
    std::fs::metadata(path).ok().filter(|m| m.is_file() && m.len() <= READ_MAX)?;
    Some(String::from_utf8_lossy(&std::fs::read(path).ok()?).into_owned())
}

/// The language and the commands a manifest states; nothing it does not say.
fn manifest(name: &str, text: &str) -> Option<String> {
    let short = |s: &str| cut(s, 50);
    match name {
        "Cargo.toml" => {
            let t: toml::Table = text.parse().ok()?;
            let members = t.get("workspace").and_then(|w| w.get("members")).and_then(|m| m.as_array()).map(|m| m.iter().filter_map(|s| s.as_str()).collect::<Vec<_>>().join(" "));
            let package = t.get("package").and_then(|p| p.get("name")).and_then(|n| n.as_str());
            Some(format!("rust · {}", members.map_or_else(|| format!("package {}", package.unwrap_or("?")), |m| format!("workspace of {m}"))))
        }
        "package.json" => {
            let j: serde_json::Value = serde_json::from_str(text).ok()?;
            let typed = ["dependencies", "devDependencies"].iter().any(|k| j.get(k).and_then(|d| d.get("typescript")).is_some());
            let scripts = ["build", "test", "lint", "start", "dev", "check"].iter().filter_map(|k| Some(format!("{k}: {}", short(j.get("scripts")?.get(k)?.as_str()?)))).collect::<Vec<_>>();
            Some(format!("{} · {}{}", if typed { "typescript" } else { "javascript" }, j.get("name").and_then(|n| n.as_str()).unwrap_or("?"), scripts.iter().map(|s| format!(" · {s}")).collect::<String>()))
        }
        "pyproject.toml" => {
            let t: toml::Table = text.parse().ok()?;
            let say = |path: &[&str]| path.iter().try_fold(toml::Value::Table(t.clone()), |v, k| v.get(k).cloned()).and_then(|v| v.as_str().map(String::from));
            let pytest = t.get("tool").and_then(|t| t.get("pytest")).is_some();
            Some(format!("python · {}{}{}", say(&["project", "name"]).unwrap_or_else(|| "?".into()), say(&["project", "requires-python"]).map(|r| format!(" · python {r}")).unwrap_or_default(), if pytest { " · test: pytest" } else { "" }))
        }
        "go.mod" => {
            let line = |key: &str| text.lines().find_map(|l| l.strip_prefix(key)).map(|v| v.trim().to_string());
            Some(format!("go · module {} · go {}", line("module ")?, line("go ").unwrap_or_else(|| "?".into())))
        }
        _ => {
            let found: Vec<&str> = TARGETS.iter().copied().filter(|t| text.lines().any(|l| l.starts_with(&format!("{t}:")))).collect();
            (!found.is_empty()).then(|| format!("make targets: {}", found.join(" ")))
        }
    }
}

/// The first line of a file that says what it is: a comment (not the shebang) in a script, a `# ` heading in a document.
fn said(text: &str, heading: bool) -> Option<String> {
    let line = text.lines().map(str::trim).find(|l| if heading { l.starts_with("# ") } else { l.starts_with('#') && !l.starts_with("#!") })?;
    Some(cut(line.trim_start_matches('#'), 70))
}

impl ProjectCard {
    pub fn new(root: &Path, settings: impl Into<j4n_core::setting::Live>) -> ProjectCard {
        ProjectCard { root: root.to_path_buf(), settings: settings.into(), facts: Mutex::new(Vec::new()) }
    }

    /// Every source as it is now: `(source, what it holds, rank)`, in the order the card has them.
    fn sources(&self) -> Vec<(String, String, u8)> {
        let mut out = Vec::new();
        let top = names(&self.root);
        for m in MANIFESTS.iter().filter(|m| top.iter().any(|n| n == *m)) {
            out.extend(read(&self.root.join(m)).map(|t| (m.to_string(), t, 0)));
        }
        let listing = |names: &[String]| {
            let shown = names.iter().take(6).cloned().collect::<Vec<_>>().join(" ");
            if names.len() > 6 { format!("{shown} +{}", names.len() - 6) } else { shown }
        };
        out.push((".".into(), format!("files: {}", listing(&top.iter().filter(|n| !n.ends_with('/')).cloned().collect::<Vec<_>>())), 1));
        for dir in top.iter().filter(|n| n.ends_with('/')).take(16) {
            out.push((dir.clone(), format!("{dir} {}", listing(&names(&self.root.join(dir)))), 1));
        }
        out.extend(top.iter().find(|n| *n == "README.md").and_then(|n| read(&self.root.join(n))).map(|t| ("README.md".into(), t, 3)));
        for (dir, ext, rank) in [("scripts", "", 2), ("docs", ".md", 3)] {
            let files = names(&self.root.join(dir)).into_iter().filter(|n| !n.ends_with('/') && n.ends_with(ext)).take(if rank == 2 { 12 } else { 60 });
            for n in files {
                out.extend(read(&self.root.join(dir).join(&n)).map(|t| (format!("{dir}/{n}"), t, rank)));
            }
        }
        out
    }

    /// Bring the facts up to date: those whose source still holds what it held stay; the others are made again, and a source that
    /// went away loses its facts. Gives the sources that were made again.
    pub fn refresh(&self) -> Vec<String> {
        let mut facts = self.facts.lock().unwrap_or_else(|e| e.into_inner());
        let (mut next, mut again) = (Vec::new(), Vec::new());
        for (source, held, rank) in self.sources() {
            let h = j4n_core::text::short_hash(&held);
            if let Some(f) = facts.iter().find(|f| f.source == source && f.hash == h) {
                next.push(f.clone());
                continue;
            }
            let name = source.rsplit('/').next().unwrap_or(&source);
            let line = match rank {
                0 => manifest(name, &held).map(|l| format!("{source}: {l}")),
                1 => Some(held.clone()),
                2 => said(&held, false).map(|l| format!("{source}: {l}")),
                _ => said(&held, true).map(|l| format!("{source}: {l}")),
            };
            again.extend(line.is_some().then_some(source.clone()));
            next.extend(line.map(|line| Fact { source, hash: h, line, rank }));
        }
        next.sort_by_key(|f| f.rank);
        *facts = next;
        again
    }

    pub fn facts(&self) -> Vec<Fact> {
        self.facts.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// The card as the Brief's section: the facts in their order, the last ones left out until it fits [`CARD_TOKENS`].
    pub fn card(&self) -> Option<String> {
        self.refresh();
        let mut text = HEAD.to_string();
        for f in self.facts() {
            let next = format!("{text}\n{}", f.line);
            // the heading the Brief puts over a section, `## project\n`, takes three tokens
            if tokens(&next) + 3 > CARD_TOKENS {
                break;
            }
            text = next;
        }
        (text != HEAD).then_some(text)
    }
}

impl ProjectFacts for ProjectCard {
    fn text(&self) -> Option<String> {
        self.settings.flag("brief.project_card", false).then(|| self.card()).flatten()
    }
}
