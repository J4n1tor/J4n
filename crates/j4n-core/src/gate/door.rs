//! The door a doer's own permission rules leave open: a doer that decides some calls before it asks the Gate, from rules in files
//! the operator wrote for it, makes the Gate's rules a suggestion. This is what J4n tells such a doer when it starts it, as that doer's own settings:
//! the tools that change things or reach out are *asked* (so a rule that allows them does not answer, an ask beating an allow), and every path that holds a
//! credential is *denied* for reading (a deny beats both), so a read of one stops inside the doer, before any content exists to show the model.
//!
//! What is asked is what the Gate has rules for (`Bash`, the file writers, the web tools). What is denied is made from [`super::danger::secret_paths`] and the
//! names of key and environment files, so that the shell matcher, the path rules and this list do not drift (a test holds them to each other).

use super::danger::secret_paths;

/// The tools of Claude Code that are put to the Gate whatever the operator's own rules allow.
pub const ASK_TOOLS: [&str; 7] = ["Bash", "Edit", "Write", "MultiEdit", "NotebookEdit", "WebFetch", "WebSearch"];

/// Paths of the Read tool that are asked about even when nothing denies them, so that the Gate's own rule says (it refuses a credential, and an example file is
/// not one): the environment files and keys a deny rule cannot name without naming their examples too.
pub const ASK_READS: [&str; 2] = ["**/.env.*", "**/id_*"];

/// The state of the projects. A deny rule cannot leave the doer's own folder out (a deny beats an allow), and the CLI keeps its big tool results and the
/// doer's memory in its own folder of the projects below its config folder, so these are *asked*: the Gate's rule `other-projects-state` blocks another
/// project's and allows the doer's own, with no card. Below the home folder (`~` is put before each); the worktrees of the projects are sources and are not asked.
const ASK_HOME: [&str; 7] = [
    "/.claude/projects/**", "/.j4n/projects/**", "/.j4n/checkpoints/**", "/.j4n/log/**", "/.j4n/memory.db", "/.j4n/folds/**", "/.j4n/blobs/**",
];

/// The same below any folder (`//` is the root, `**/` any depth): the `.j4n` of a project. The form is not yet confirmed against the CLI.
const ASK_ANY: [&str; 4] = ["//**/.j4n/log/**", "//**/.j4n/memory.db", "//**/.j4n/folds/**", "//**/.j4n/blobs/**"];

/// `(the entry of the shell matcher's list of credential paths, the doer's rule for it)`: a path in the doer's own syntax (`**` any depth, `//` the root). A rule that starts
/// with `/.` is below the home folder, and the doer writes that as `~` before it ([`rule`]).
const HOME: [(&str, &str); 27] = [
    ("/.aws/credentials", "/.aws/**"),
    ("/.netrc", "/.netrc"),
    ("/.git-credentials", "/.git-credentials"),
    ("/.config/gh/", "/.config/gh/**"),
    ("/.npmrc", "/.npmrc"),
    ("/.cargo/credentials", "/.cargo/credentials*"),
    ("/.docker/config.json", "/.docker/config.json"),
    ("/.kube/config", "/.kube/config"),
    ("/.ssh/id_", "/.ssh/**"),
    ("/.gnupg", "/.gnupg/**"),
    ("/id_rsa", "**/id_rsa"),
    ("/id_ed25519", "**/id_ed25519"),
    ("/id_ecdsa", "**/id_ecdsa"),
    ("/id_dsa", "**/id_dsa"),
    ("/.claude/.credentials.json", "/.claude/.credentials.json"),
    ("/.claude.json", "/.claude.json"),
    ("/.codex/auth.json", "/.codex/auth.json"),
    ("/.pypirc", "/.pypirc"),
    ("/.j4n/secrets", "/.j4n/secrets/**"),
    ("/.j4n/keys", "/.j4n/keys/**"),
    ("/.j4n/accounts", "/.j4n/accounts/**"),
    ("/.j4n/guard.salt", "/.j4n/guard.salt"),
    ("/proc/*/environ", "//proc/**/environ"),
    ("/proc/*/mem", "//proc/**/mem"),
    ("/.claude/sessions", "/.claude/sessions/**"),
    ("/.claude/daemon", "/.claude/daemon/**"),
    ("/.claude/history.jsonl", "/.claude/history.jsonl"),
];

/// Environment files and key files by name, in any folder.
const NAMES: [&str; 16] = [
    "**/.env", "**/.env.local", "**/.env.production", "**/.env.prod", "**/.env.staging", "**/.env.development", "**/.env.dev", "**/.env.test", "**/.env.*.local",
    "**/.envrc", "**/*.pem", "**/*.key", "**/*.p12", "**/*.pfx", "**/.j4n/secrets/**", "/.j4n/accounts.toml",
];

/// Whether a path of the project holds a secret, a key or a doer's own state, by the same lists the doer is denied by: never read, never shared.
pub fn secret_file(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    super::danger::secret_name(path)
        || path.split('/').any(|c| matches!(c, ".j4n" | ".claude" | ".ssh" | ".gnupg" | ".aws"))
        || NAMES.iter().filter_map(|g| g.strip_prefix("**/")).filter(|g| !g.contains('/')).any(|g| crate::eval::glob_match(g, name))
}

/// Every path the Read tool is denied, as the doer's rules (`Read(path)`).
pub fn denied_reads() -> Vec<String> {
    HOME.iter().map(|(_, r)| *r).chain(NAMES).map(|r| format!("Read({})", rule(r))).collect()
}

/// The doer's rule for a path: one below the home folder starts with `~`.
fn rule(r: &str) -> String {
    if r.starts_with("/.") { format!("~{r}") } else { r.to_string() }
}

/// The settings J4n gives Claude Code with `--settings`, as JSON text: what is asked and what is denied. Everything else the operator's own settings say is kept.
pub fn claude_settings() -> String {
    claude_settings_in(None)
}

/// [`claude_settings`] for a child whose config folder is `config` (`CLAUDE_CONFIG_DIR`, an absolute path): its projects are asked too.
pub fn claude_settings_in(config: Option<&str>) -> String {
    let own = config.map(|c| c.trim_end_matches('/')).filter(|c| c.starts_with('/') && c.len() > 1).map(|c| format!("Read(/{c}/projects/**)"));
    let asks: Vec<String> = ASK_TOOLS
        .iter()
        .map(|t| (*t).to_string())
        .chain(ASK_READS.iter().chain(&ASK_ANY).map(|p| format!("Read({p})")))
        .chain(ASK_HOME.iter().map(|p| format!("Read({})", rule(p))))
        .chain(own)
        .collect();
    serde_json::json!({ "permissions": { "ask": asks, "deny": denied_reads() } }).to_string()
}

/// The entries of [`secret_paths`] that [`HOME`] has no rule for: none, or a path was added to the matcher and not here.
pub fn uncovered() -> Vec<&'static str> {
    secret_paths().iter().copied().filter(|p| !HOME.iter().any(|(e, _)| e == p)).collect()
}
