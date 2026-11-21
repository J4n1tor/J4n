//! The permission config J4n makes for a program that has rules of its own: opencode in run mode answers its own asks by
//! rejecting them, and with a bare `--auto` it allows everything, `bash -c` and `find -delete` among it. So each task is given a config in which
//! **everything is denied but what J4n's own gate calls safe**, and `--auto` only answers what the config leaves to ask.
//!
//! The rules are an object whose **order is the meaning**: the last rule that matches a line wins, and opencode matches each command of a compound line on
//! its own. So the order is: `*` denied; what the gate's tables allow (the readers, the test runners, the read verbs of git); what the row adds
//! (`--approval full`: the repository's own writes, a file an interpreter runs); then, last, what is refused (the forms of a reader that are more than a
//! reader, the inline forms of an interpreter, `rm`, a push). The lists come from `j4n_core::gate::danger` where the gate has them, so the two do not drift
//! (a law checks each against the matcher), and from `[connector.guard]` of the pack where only the connector can say.

use crate::manifest::GuardSpec;
use j4n_core::gate::danger::{GIT_READ, REFUSED_FORMS, SAFE_RUNNERS, read_programs, secret_paths};
use std::path::{Path, PathBuf};

/// One rule: a glob over a whole command line, and what it says.
pub type Rule = (String, &'static str);

/// The flags git takes in front of a verb that a model uses and that change nothing about what the verb does here.
const GIT_FLAGS: [&str; 3] = ["--no-pager", "-c core.pager=cat", "-C *"];
/// The one more a `checkout` or a `switch` takes: a loop over commits says it so that git does not talk of a detached HEAD each time. A `-c` of a
/// key that changes what git runs (`core.*`, `alias.*`) is not this one, and is not made a rule.
const GIT_ADVICE: &str = "-c advice.detachedHead=false";

/// `pattern`, and for a rule on `git <verb>` the same with each of the global flags in front of the verb.
fn flagged(pattern: &str) -> Vec<String> {
    let mut out = vec![pattern.to_string()];
    if let Some(rest) = pattern.strip_prefix("git ").filter(|r| !r.starts_with(['-', '*'])) {
        out.extend(GIT_FLAGS.iter().map(|f| format!("git {f} {rest}")));
        if rest.starts_with("checkout") || rest.starts_with("switch") {
            out.push(format!("git {GIT_ADVICE} {rest}"));
        }
    }
    out
}

/// `git ...` rules with a leading `VAR=value` word or `env VAR=value` in front, which a model uses for the author of a commit (`--approval full`).
fn with_env(words: &[String]) -> Vec<String> {
    let plain = |w: &&String| w.starts_with("git ") && !GIT_FLAGS.iter().chain([&GIT_ADVICE]).any(|f| w.starts_with(&format!("git {f} ")));
    words.iter().filter(plain).flat_map(|w| [format!("GIT_*=* {w}"), format!("env *=* {w}")]).collect()
}

/// A rule on a program, which covers the program with arguments and without: `ls *`. opencode matches a pattern that ends in ` *` also against the line
/// without the arguments (its matcher turns the ending ` .*` into `( .*)?`, read in 1.19.4), so `ls` as a rule of its own says nothing more and was one
/// more rule in the error text of every refusal. [`verdict`] reads a rule as opencode does.
fn with_args(word: &str) -> Vec<String> {
    if word.ends_with('*') { vec![word.to_string()] } else { vec![format!("{word} *")] }
}

/// The rules for `bash`, in the order that matters. `full`: `--approval full`.
pub fn bash(g: &GuardSpec, full: bool) -> Vec<Rule> {
    bash_walled(g, full, false)
}

/// [`bash`] for a task that runs behind the box (`walled`): the OS holds every write and delete to the project and the program's own state, so the
/// patterns that stood for that at the level where the gate asks (the redirect denials) are not made, and at `full` the deletes of `boxed_allow` are.
pub fn bash_walled(g: &GuardSpec, full: bool, walled: bool) -> Vec<Rule> {
    let mut rules: Vec<Rule> = vec![("*".into(), "deny")];
    let mut allow = |words: Vec<String>| {
        let made: Vec<String> = words.iter().flat_map(|w| flagged(w)).flat_map(|w| with_args(&w)).collect();
        let prefixed = if full { with_env(&made) } else { Vec::new() };
        rules.extend(made.into_iter().chain(prefixed).map(|w| (w, "allow")));
    };
    allow(read_programs().into_iter().map(String::from).collect());
    allow(GIT_READ.split(' ').map(|v| format!("git {v}")).collect());
    allow(SAFE_RUNNERS.iter().map(|s| s.to_string()).collect());
    allow(g.allow.clone());
    if full {
        allow(g.full_allow.clone());
        // as written: a rule on the command and on nothing after it
        rules.extend(g.full_exact.iter().map(|w| (w.clone(), "allow")));
    }
    let mut deny = |words: Vec<String>| {
        let made: Vec<String> = words.iter().flat_map(|w| flagged(w)).collect();
        let prefixed = if full { with_env(&made) } else { Vec::new() };
        rules.extend(made.into_iter().chain(prefixed).map(|w| (w, "deny")));
    };
    deny(REFUSED_FORMS.iter().map(|(p, _)| p.to_string()).collect());
    deny(g.deny.clone());
    if full {
        deny(g.full_deny.clone());
        if walled {
            // after everything, so they stand over `rm*` and the refused forms of a reader
            let made: Vec<String> = g.boxed_allow.iter().flat_map(|w| with_args(w)).collect();
            rules.extend(made.into_iter().map(|w| (w, "allow")));
        }
    } else if !walled {
        // last, so nothing above can allow what these deny
        rules.extend(redirect_denials().into_iter().map(|p| (p, "deny")));
    }
    rules
}

/// The folders a redirect may not name at the level where the gate would ask: the places a user can write outside the project, and the ones a program writes
/// to do harm (`/dev/tcp/HOST/PORT` opens a connection). opencode asks `external_directory` for the arguments of a short list of commands and **never for
/// the target of a redirect**, and names a command by the text of the whole statement, so `cat *`, `echo *` and the other readers allow `> /etc/x`.
/// Not `/dev/null`: it is the one target a model uses all the time, and a pattern cannot say "any but".
const REDIRECT_ROOTS: [&str; 14] = ["/home", "/tmp", "/var", "/run", "/mnt", "/media", "/srv", "/opt", "/root", "/etc", "/usr", "/boot", "/dev/tcp", "/dev/udp"];

/// The patterns of the statements that redirect output (`>`, `>>`, `2>`, `&>`, `>|`: all hold a `>`) to a place the folder of the task does not hold: a path that starts with a root of [`REDIRECT_ROOTS`], `~`, `$`, `..`, a quote and then `/`, or goes up through one. Each is made in the
/// two spellings of a space after the `>`. A pattern reads the whole statement, a here-document's body too: a body that has `>/home` in it is refused as well.
fn redirect_denials() -> Vec<String> {
    let mut targets: Vec<String> = REDIRECT_ROOTS.iter().map(ToString::to_string).collect();
    targets.extend(["~", "$", "..", "\"/", "'/"].map(String::from));
    let mut out: Vec<String> = targets.iter().flat_map(|t| [format!("*>{t}*"), format!("*> {t}*")]).collect();
    // a way up from inside: `> sub/../../x`
    out.push("*>*/../*".into());
    out
}

/// [`bash`] and the rules that let `cd` go to the task's own folder `dir`: what the config is made of.
pub fn project_bash(g: &GuardSpec, full: bool, dir: &Path) -> Vec<Rule> {
    project_bash_walled(g, full, dir, false)
}

/// [`project_bash`] for a task behind the box or not ([`bash_walled`]).
pub fn project_bash_walled(g: &GuardSpec, full: bool, dir: &Path, walled: bool) -> Vec<Rule> {
    let mut rules = bash_walled(g, full, walled);
    rules.extend(cd_rules(dir));
    rules
}

/// What a rule list says of one command: the last rule whose glob matches wins; none matching, `ask` (opencode's own default).
pub fn verdict(rules: &[Rule], line: &str) -> &'static str {
    rules.iter().rev().find(|(p, _)| matches(p, line)).map_or("ask", |(_, a)| a)
}

/// opencode's match of a rule: a glob over the whole line, `*` any characters, `?` one; and a pattern that ends in ` *` also matches the line without the
/// arguments.
fn matches(pattern: &str, line: &str) -> bool {
    j4n_core::eval::glob_match(pattern, line) || pattern.strip_suffix(" *").is_some_and(|bare| j4n_core::eval::glob_match(bare, line))
}

/// A JSON object written in the order given: the order of the rules is their meaning, which a map would sort away.
fn object(rules: &[(String, &str)]) -> String {
    let rows: Vec<String> = rules.iter().map(|(k, v)| format!("{}:{}", json(k), json(v))).collect();
    format!("{{{}}}", rows.join(","))
}

fn json(s: &str) -> String {
    serde_json::Value::String(s.to_string()).to_string()
}

/// The folders a task may reach: its own, and those beside it that its place says it must write.
fn folders(dir: &Path, also: &[PathBuf]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for d in std::iter::once(dir).chain(also.iter().map(PathBuf::as_path)) {
        for d in spellings(d) {
            out.extend([d.clone(), format!("{d}/*")]);
        }
    }
    out
}

/// A folder as it may be written: as given, and as the system spells it when that is another way (a link on the way to it). The program is started in
/// the folder as given and reports where it is as the system does, so a model that writes its own folder out writes the second.
fn spellings(d: &Path) -> Vec<String> {
    let given = d.display().to_string();
    let real = d.canonicalize().map(|r| r.display().to_string()).ok().filter(|r| *r != given);
    std::iter::once(given).chain(real).collect()
}

/// The rules that let `cd` go to the task's own folder, last: the rules before them refuse a `cd` to any absolute path, and the project's own is
/// one. The folder itself, written plainly or quoted, and below it; what is below it may not climb out again.
fn cd_rules(dir: &Path) -> Vec<Rule> {
    let mut rules: Vec<Rule> = Vec::new();
    let mut climb: Vec<Rule> = Vec::new();
    for d in spellings(dir) {
        for q in ["", "\"", "'"] {
            rules.push((format!("cd {q}{d}{q}"), "allow"));
            rules.push((format!("cd {q}{d}/*{q}"), "allow"));
            climb.push((format!("cd {q}{d}/*..*{q}"), "deny"));
        }
    }
    rules.extend(climb);
    rules
}

/// What a run says of a call its program's rules refused, in place of the rules themselves: one sentence, with the line (clipped) and the kind
/// of thing it was when the words say. The text opencode gives is every rule of the config, about 30 000 characters each time.
pub fn refusal(what: &str) -> String {
    let one: String = what.split_whitespace().collect::<Vec<_>>().join(" ");
    let shown: String = if one.chars().count() > 200 { one.chars().take(200).collect::<String>() + "…" } else { one };
    match refused_group(what) {
        Some(group) => format!("J4n's guard refused `{shown}`: {group}."),
        None => format!("J4n's guard refused `{shown}`: it is not a command this task may run."),
    }
}

/// Which of the guard's groups a refused line is in, by its words: the code it wraps, what it deletes, what it sends, where it goes.
fn refused_group(line: &str) -> Option<&'static str> {
    let words: Vec<&str> = line.split(|c: char| c.is_whitespace() || ";&|()`\"'".contains(c)).filter(|w| !w.is_empty()).collect();
    let has = |w: &str| words.contains(&w);
    let any = |list: &[&str]| list.iter().any(|w| has(w));
    let short_c = words.iter().any(|w| *w == "-c" || (w.starts_with('-') && !w.starts_with("--") && w.ends_with('c')));
    let inline = (any(&["bash", "sh", "zsh", "dash"]) || any(&["python", "python3", "perl", "ruby"])) && short_c;
    if inline || (has("node") && (has("-e") || has("--eval"))) || any(&["eval", "xargs", "env"]) {
        return Some("it wraps another command or runs inline code");
    }
    let git = has("git");
    if any(&["rm", "rmdir", "unlink", "shred", "-delete"]) || (git && (any(&["clean", "reset", "-D"]) || (has("stash") && any(&["drop", "clear"])) || (has("tag") && has("-d")))) {
        return Some("it deletes");
    }
    if any(&["curl", "wget", "ssh", "scp", "rsync", "nc"]) || (git && any(&["push", "pull", "fetch", "clone"])) {
        return Some("it uses the network");
    }
    if (has("cd") && words.iter().any(|w| w.starts_with(['/', '~']) || w.starts_with("..") || w.starts_with("$HOME"))) || words.iter().any(|w| w.contains("../")) {
        return Some("it leaves the project");
    }
    None
}

/// Where opencode 1.19.4 keeps its own scratch (`opencode debug paths`: `tmp`): `opencode` under the temporary folder of the process, `$TMPDIR` or `/tmp`.
/// It writes there itself (a tool's output that was too long to show), and without a word for it the fence refuses opencode its own folder.
pub fn opencode_scratch(tmpdir: Option<&str>) -> PathBuf {
    Path::new(tmpdir.filter(|t| !t.is_empty()).unwrap_or("/tmp")).join("opencode")
}

/// The config of opencode for one task: `bash` as [`bash`] says it, `edit` allowed unless the task may only read, `external_directory` only the task's
/// own folders and opencode's own scratch folder `scratch`, a credential file never read, and the web denied.
pub fn opencode(g: &GuardSpec, full: bool, read_only: bool, dir: &Path, also: &[PathBuf], scratch: &Path) -> String {
    opencode_walled(g, full, read_only, dir, also, scratch, false)
}

/// [`opencode`] for a task behind the box or not ([`bash_walled`]).
pub fn opencode_walled(g: &GuardSpec, full: bool, read_only: bool, dir: &Path, also: &[PathBuf], scratch: &Path, walled: bool) -> String {
    let mut outside: Vec<(String, &str)> = vec![("*".into(), "deny")];
    outside.extend(folders(dir, also).into_iter().map(|f| (f, "allow")));
    outside.extend(folders(scratch, &[]).into_iter().map(|f| (f, "allow")));
    let mut read: Vec<(String, &str)> = vec![("*".into(), "allow"), ("*.env".into(), "deny"), ("*.env.*".into(), "deny")];
    read.extend(secret_paths().iter().map(|p| (format!("*{p}*"), "deny")));
    let edit = if read_only { "deny" } else { "allow" };
    format!(
        "{{\"$schema\":\"https://opencode.ai/config.json\",\"permission\":{{\"bash\":{},\"edit\":\"{edit}\",\"read\":{},\"external_directory\":{},\"webfetch\":\"deny\",\"websearch\":\"deny\"}}}}",
        object(&project_bash_walled(g, full, dir, walled)),
        object(&read),
        object(&outside)
    )
}

/// The config of opencode for a task whose asks J4n's gate answers: what [`opencode_walled`] says, but a command or a folder the rules would
/// have denied is **asked**, so `opencode acp` hands it to J4n (`session/request_permission`) instead of refusing it itself. What the rules allowed stays allowed
/// (the same lists, from the same matcher), so the config only ever refuses less; the credential files are still never read and the web is asked, not denied.
/// `model` is the model the task asks for, set as the default of the session.
#[allow(clippy::too_many_arguments)]
pub fn opencode_asking(g: &GuardSpec, full: bool, read_only: bool, dir: &Path, also: &[PathBuf], scratch: &Path, walled: bool, model: Option<&str>) -> String {
    let asked = |rules: Vec<Rule>| -> Vec<(String, &'static str)> { rules.into_iter().map(|(p, v)| (p, if v == "deny" { "ask" } else { v })).collect() };
    let mut outside: Vec<(String, &str)> = vec![("*".into(), "ask")];
    outside.extend(folders(dir, also).into_iter().map(|f| (f, "allow")));
    outside.extend(folders(scratch, &[]).into_iter().map(|f| (f, "allow")));
    let mut read: Vec<(String, &str)> = vec![("*".into(), "allow"), ("*.env".into(), "deny"), ("*.env.*".into(), "deny")];
    read.extend(secret_paths().iter().map(|p| (format!("*{p}*"), "deny")));
    let edit = if read_only { "deny" } else { "allow" };
    let model = model.filter(|m| !m.is_empty()).map(|m| format!(",\"model\":{}", json(m))).unwrap_or_default();
    format!(
        "{{\"$schema\":\"https://opencode.ai/config.json\"{model},\"permission\":{{\"bash\":{},\"edit\":\"{edit}\",\"read\":{},\"external_directory\":{},\"webfetch\":\"ask\",\"websearch\":\"ask\"}}}}",
        object(&asked(project_bash_walled(g, full, dir, walled))),
        object(&read),
        object(&outside)
    )
}
