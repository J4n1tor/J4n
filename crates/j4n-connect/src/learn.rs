//! `connect-learn`: learn a program by trying it, with no model. The probe read what the
//! program *says*; this step runs it behind the place's walls, a few ways, with a prompt that asks for one made-up word back, and
//! reads what comes back. The way that prints the word is how the program is given a prompt (an argument, a flag, a subcommand,
//! its standard input or a variable); where the word sits in what it printed says how its output is read (colour codes, server-sent
//! events, frames, logfmt, JSON by a pointer, or plain lines), and a second try asks it to read a file that is not there, to see what
//! a tool event looks like. What was learned is one `jsonl-map` row, checked as any draft is and run once as the canary: a model is
//! asked only when none of this settles it.

use crate::connect::{Job, Link, canary_of, id_ok, judge};
use crate::decode::{self, Step};
use crate::limits;
use j4n_core::text::clip;
use serde_json::{Value as Json, json};
use std::time::{Duration, Instant};

/// How long one try is given, and all of them together.
const TRY_SECS: u64 = 12;
const BUDGET: Duration = Duration::from_secs(150);
/// The most ways tried.
const MOST: usize = 36;

/// One way of giving a program a prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Way {
    /// The arguments after the program, `{prompt}` where the words go.
    args: Vec<String>,
    stdin: bool,
    /// A variable that carries the prompt.
    env: Option<String>,
}

impl Way {
    fn run(&self, job: &Job, path: &str, words: &str) -> Option<(String, Option<i32>)> {
        let mut argv = vec![path.to_string()];
        // a way that gives the prompt as the path of a file: the file is made beside the project's files for the try and taken away
        let file = self.args.iter().any(|a| a == "{promptfile}").then(|| job.cx.site.dir.join(format!(".j4n-learn-prompt-{}.txt", std::process::id())));
        if let Some(f) = &file {
            std::fs::write(f, words).ok()?;
        }
        argv.extend(self.args.iter().map(|a| a.replace("{prompt}", words).replace("{promptfile}", &file.as_ref().map(|f| f.display().to_string()).unwrap_or_default())));
        let env: Vec<(String, String)> = self.env.iter().map(|n| (n.clone(), words.to_string())).collect();
        let tried = job.trial(&argv, self.stdin.then_some(words), &env, TRY_SECS, true);
        if let Some(f) = &file {
            let _ = std::fs::remove_file(f);
        }
        tried
    }

    /// The same run in a folder of its own under the project (`sub`), which later runs find as this one left it: what a session needs.
    fn run_in(&self, job: &Job, path: &str, words: &str, sub: &str, extra: &[String]) -> Option<(String, Option<i32>)> {
        let mut argv: Vec<String> = ["sh", "-c", "mkdir -p \"$1\" && cd \"$1\" || exit 97; shift; exec \"$@\"", "sh", sub, path].map(String::from).into();
        argv.extend(extra.iter().cloned());
        argv.extend(self.args.iter().map(|a| a.replace("{prompt}", words)));
        let env: Vec<(String, String)> = self.env.iter().map(|n| (n.clone(), words.to_string())).collect();
        job.trial(&argv, self.stdin.then_some(words), &env, TRY_SECS, false)
    }

    /// The flag that carries a session, found by trying: the first run names a session, and a second run that is given its id with a flag
    /// must know what the first was asked.
    fn session_flag(&self, job: &Job, path: &str, word: &str, at: &str) -> Option<String> {
        let sub = format!(".j4n-learn-{}", std::process::id());
        let asking = format!("Reply with the single word: {word}");
        let found = (|| {
            let (out, _) = self.run_in(job, path, &asking, &sub, &[])?;
            let (decode, docs) = read(&out, word)?;
            let _ = decode;
            let id = docs.iter().find_map(|d| str_at(d, at).map(String::from))?;
            ["--session", "--resume", "--conversation", "--thread", "-s", "--session-id", "--chat", "--id"].into_iter().find(|f| {
                self.run_in(job, path, "Which word did I ask you to reply with before?", &sub, &[f.to_string(), id.clone()]).is_some_and(|(o, _)| o.contains(word))
            })
        })();
        let _ = job.trial(&["rm".into(), "-rf".into(), job.cx.site.dir.join(&sub).display().to_string()], None, &[], 5, false);
        found.map(String::from)
    }

    fn how(&self) -> String {
        match (&self.env, self.stdin, self.args.as_slice()) {
            (Some(n), ..) => format!("the variable {n}"),
            (None, true, _) => "its standard input".into(),
            (None, false, [only]) if only == "{prompt}" => "an argument".into(),
            _ => format!("`{}`", self.args.join(" ")),
        }
    }
}

fn is_placeholder(t: &str) -> bool {
    let t = t.trim_matches(|c| c == '[' || c == ']' || c == '(' || c == ')' || c == ',' || c == '.');
    (t.starts_with('<') && t.ends_with('>')) || (t.len() >= 2 && t.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'))
}

fn flag_word(t: &str) -> Option<&str> {
    let t = t.trim_end_matches(',');
    (t.starts_with('-') && t.len() >= 2 && !t.starts_with("---") && t[1..].trim_start_matches('-').chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')).then_some(t)
}

/// The ways worth trying, from what the program says about itself first and the common ones after.
fn ways(name: &str, said: &str) -> (Vec<Way>, Vec<Vec<String>>) {
    let mut out: Vec<Way> = Vec::new();
    let mut servers: Vec<Vec<String>> = Vec::new();
    let mut add = |args: &[&str], stdin: bool, env: Option<&str>| {
        let w = Way { args: args.iter().map(|a| a.to_string()).collect(), stdin, env: env.map(String::from) };
        if !out.contains(&w) {
            out.push(w);
        }
    };
    let short = std::path::Path::new(name).file_name().and_then(|n| n.to_str()).unwrap_or(name);
    for raw in said.lines() {
        let l = raw.trim();
        let ll = l.to_lowercase();
        let l = l.trim_start_matches("Usage:").trim_start_matches("usage:").trim();
        if let Some(after) = l.strip_prefix(short) {
            // `name [options] PROMPT`, `name --flag TEXT`, `name ask PROMPT`
            let (mut words, mut slot) = (Vec::<String>::new(), false);
            for t in after.split_whitespace() {
                if t.starts_with('[') || t.starts_with('(') {
                    if t.starts_with('(') {
                        break;
                    }
                    continue;
                }
                if let Some(f) = flag_word(t) {
                    words.push(f.to_string());
                } else if is_placeholder(t) {
                    slot = true;
                    break;
                } else if t.chars().all(|c| c.is_ascii_lowercase() || c == '-' || c == '_') {
                    words.push(t.to_string());
                } else {
                    break;
                }
            }
            if slot {
                // `name --flag PATH  (the prompt is read from the file at PATH)`: the prompt goes in a file, and the flag is given its path
                let file = ll.contains("file") && ll.contains("prompt") && !words.is_empty();
                let mut args: Vec<&str> = words.iter().map(String::as_str).collect();
                args.push(if file { "{promptfile}" } else { "{prompt}" });
                add(&args, false, None);
            } else if !words.is_empty() && (ll.contains("http") || ll.contains("server")) && !servers.contains(&words) {
                servers.push(words);
            }
        }
        if ll.contains("standard input") || ll.contains("stdin") {
            add(&[], true, None);
        }
        if let Some(i) = ll.find("environment variable ") {
            let v = l[i + "environment variable ".len()..].split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).next().unwrap_or_default();
            if v.len() >= 3 {
                add(&[], false, Some(v));
            }
        }
        if raw.trim_start().starts_with('-') && ["prompt", "message", "ask", "query", "question", "text", "input"].iter().any(|w| ll.contains(w)) && !ll.contains("session") && !ll.contains("version") {
            let mut toks = raw.split_whitespace();
            let flags: Vec<&str> = toks.by_ref().take_while(|t| flag_word(t).is_some()).filter_map(flag_word).collect();
            if let Some(f) = flags.last() {
                add(&[f, "{prompt}"], false, None);
            }
        }
    }
    let upper = short.to_uppercase().replace('-', "_");
    add(&["{prompt}"], false, None);
    add(&[], true, None);
    for f in ["-p", "--prompt", "-m", "--message", "-q", "--query", "-c", "-e"] {
        add(&[f, "{prompt}"], false, None);
    }
    for sub in ["ask", "chat", "run", "q", "query", "say", "send", "prompt", "message", "exec"] {
        add(&[sub, "{prompt}"], false, None);
    }
    add(&["chat", "send", "{prompt}"], false, None);
    for v in [format!("{upper}_PROMPT"), "PROMPT".to_string(), format!("{upper}_MESSAGE")] {
        add(&[], false, Some(&v));
    }
    out.truncate(MOST);
    for w in ["serve", "server", "--serve", "start", "daemon"] {
        if !servers.iter().any(|s| s.as_slice() == [w]) {
            servers.push(vec![w.to_string()]);
        }
    }
    servers.truncate(6);
    (out, servers)
}

/// The paths and the keys of a request to a server, from what it says (`POST /ask with {"prompt": TEXT}`) and the common ones.
fn requests(said: &str) -> (Vec<String>, Vec<String>) {
    let (mut paths, mut keys) = (Vec::<String>::new(), Vec::<String>::new());
    for l in said.lines().filter(|l| l.contains("POST ")) {
        let after = &l[l.find("POST ").unwrap_or(0) + 5..];
        if let Some(p) = after.split_whitespace().next().filter(|p| p.starts_with('/')) {
            paths.push(p.trim_matches(|c: char| !(c.is_ascii_alphanumeric() || "/_-".contains(c))).to_string());
        }
        if let Some(i) = after.find("{\"") {
            let k: String = after[i + 2..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
            if !k.is_empty() {
                keys.push(k);
            }
        }
    }
    paths.extend(["/", "/ask", "/chat", "/message", "/prompt", "/v1/chat"].map(String::from));
    keys.extend(["prompt", "message", "text", "input", "question"].map(String::from));
    paths.dedup();
    keys.dedup();
    (paths, keys)
}

// ---- reading what came back ----

fn ok_ptr(p: &str) -> bool {
    p.chars().all(|c| c.is_ascii_alphanumeric() || "_-.:~@$%+/".contains(c))
}

/// The text leaves of a document, by their pointer; a leaf whose pointer an expression could not name is left out.
fn leaves(v: &Json) -> Vec<(String, String)> {
    fn walk(v: &Json, at: &str, out: &mut Vec<(String, String)>) {
        match v {
            Json::String(s) if ok_ptr(at) => out.push((at.to_string(), s.clone())),
            Json::Object(m) => m.iter().for_each(|(k, x)| walk(x, &format!("{at}/{}", k.replace('~', "~0").replace('/', "~1")), out)),
            Json::Array(a) => a.iter().enumerate().for_each(|(i, x)| walk(x, &format!("{at}/{i}"), out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(v, "", &mut out);
    out
}

/// How many values (not containers) a document holds.
fn weight(v: &Json) -> usize {
    match v {
        Json::Object(m) => m.values().map(weight).sum(),
        Json::Array(a) => a.iter().map(weight).sum(),
        _ => 1,
    }
}

fn has(d: &Json, want: &str) -> Option<String> {
    leaves(d).into_iter().find(|(_, v)| v == want).map(|(p, _)| p)
}

fn str_at<'a>(d: &'a Json, p: &str) -> Option<&'a str> {
    d.pointer(p).and_then(Json::as_str)
}

/// The decodings to try on a run's output, most structured first.
fn shapes(raw: &str) -> Vec<Vec<Step>> {
    let pre: Vec<Step> = if raw.contains('\u{1b}') || raw.contains('\r') { vec![Step::Ansi] } else { vec![] };
    [vec![Step::Frames], vec![Step::Sse], vec![], vec![Step::Logfmt], vec![Step::Colon]]
        .into_iter()
        .map(|rest| pre.iter().copied().chain(rest).collect())
        .collect()
}

fn docs(decode: &[Step], raw: &str) -> Vec<Json> {
    raw.lines().filter_map(|l| decode::line(decode, l)).collect()
}

/// The decoding under which some document holds `word` as a whole leaf, and its documents.
fn read(raw: &str, word: &str) -> Option<(Vec<Step>, Vec<Json>)> {
    shapes(raw).into_iter().find_map(|d| {
        let ds = docs(&d, raw);
        ds.iter().any(|x| has(x, word).is_some()).then_some((d, ds))
    })
}

fn quotable(s: &str) -> bool {
    !s.contains('\'') && !s.contains('\\') && !s.contains('\n')
}

/// Whether a run answered with nothing but what it was given: every word of every text it printed is a word of the prompt. A filter (`cat`,
/// `sed -n p`, `tee`, `awk '{print $NF}'`, `echo`) returns its input or a part of it; an agent answers something that was not handed to it.
fn only_returns(docs: &[Json], prompt: &str) -> bool {
    let words = |t: &str| t.to_lowercase().split_whitespace().map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string()).filter(|w| !w.is_empty()).collect::<Vec<_>>();
    let given = words(prompt);
    let texts: Vec<String> = docs.iter().flat_map(|d| leaves(d).into_iter().map(|(_, v)| v)).collect();
    !texts.is_empty() && texts.iter().flat_map(|t| words(t)).all(|w| given.contains(&w))
}

/// What a run's documents say of the program's events: the rows of the map, whether it names a session, whether it shows tools.
struct Rows {
    map: toml::Table,
    session: bool,
    tools: bool,
}

fn row(pairs: &[(&str, String)]) -> toml::Value {
    toml::Value::Table(pairs.iter().map(|(k, v)| (k.to_string(), toml::Value::String(v.clone()))).collect())
}

const SESSION_WORDS: [&str; 6] = ["session", "thread", "conv", "init", "hello", "start"];

fn learn_rows(decode: &[Step], one: &[Json], word: &str, tool: &[Json], session_flag: bool) -> Option<Rows> {
    let mut map = toml::Table::new();
    let mut events: Vec<toml::Value> = Vec::new();
    let (mut session, mut tools) = (false, false);
    let say_i = one.iter().position(|d| has(d, word).is_some())?;
    if decode.contains(&Step::Colon) {
        // lines: `key: value`, `* tool`, `= result`, and the answer
        let heads: Vec<String> = one.iter().enumerate().filter(|(i, _)| *i != say_i).filter_map(|(_, d)| d.get("head").and_then(Json::as_str).map(String::from)).collect();
        let mut noise: Vec<String> = heads.into_iter().filter(|h| quotable(h)).collect();
        let tool_at = tool.iter().position(|d| str_at(d, "/line").is_some_and(|l| l.contains("nothing-here")));
        if let Some(i) = tool_at {
            let head = |j: usize| tool.get(j).and_then(|d| str_at(d, "/head")).map(String::from).filter(|h| quotable(h));
            if let Some(h) = head(i) {
                noise.push(h.clone());
                events.push(row(&[("when", format!("/head == '{h}'")), ("emit", "tool".into()), ("name", "'tool'".into()), ("detail", "/tail".into())]));
                tools = true;
                if i + 2 < tool.len()
                    && let Some(r) = head(i + 1).filter(|r| *r != h)
                {
                    noise.push(r.clone());
                    events.push(row(&[("when", format!("/head == '{r}'")), ("emit", "tool_end".into()), ("name", "'tool'".into()), ("detail", "/tail".into())]));
                }
            }
        }
        noise.sort();
        noise.dedup();
        let when = if noise.is_empty() { "/line != ''".to_string() } else { noise.iter().map(|h| format!("not (/head == '{h}')")).collect::<Vec<_>>().join(" and ") };
        events.insert(0, row(&[("when", when), ("emit", "text".into()), ("text", "/line".into())]));
        if let Some((k, _)) = one.iter().enumerate().filter(|(i, _)| *i != say_i).flat_map(|(_, d)| d.as_object().into_iter().flatten()).find(|(k, _)| !["line", "head", "tail"].contains(&k.as_str()) && ok_ptr(k) && SESSION_WORDS.iter().any(|w| k.to_lowercase().contains(w))) {
            map.insert("session".into(), toml::Value::String(format!("/{k}")));
            session = true;
        }
    } else {
        let say = &one[say_i];
        let text = has(say, word)?;
        let disc = leaves(say).into_iter().find(|(p, v)| *p != text && quotable(v) && one.iter().enumerate().any(|(i, d)| i != say_i && str_at(d, p).is_some_and(|x| x != v)));
        let when = match &disc {
            Some((p, v)) => format!("{p} == '{v}'"),
            None => format!("{text} != ''"),
        };
        events.push(row(&[("when", when), ("emit", "text".into()), ("text", text.clone())]));
        if let Some((dp, dv)) = &disc {
            // the document that names the session comes before the answer and holds one more text; the one that ends the turn holds only its kind
            // (the text that is not the same on the answer's own document: `lvl=info` is on every line)
            let said = leaves(say);
            if let Some(sd) = one.iter().take(say_i).find(|d| str_at(d, dp).is_some_and(|k| k != dv))
                && let Some((p, _)) = leaves(sd).into_iter().find(|(p, v)| p != dp && said.iter().find(|(q, _)| q == p).is_none_or(|(_, w)| w != v))
            {
                map.insert("session".into(), toml::Value::String(p));
                session = true;
            }
            if let Some(e) = one.iter().enumerate().rev().find(|(i, d)| *i > say_i && weight(d) == 1 && str_at(d, dp).is_some_and(|k| quotable(k) && k != dv)).map(|(_, d)| d)
                && let Some(k) = str_at(e, dp)
            {
                map.insert("end".into(), toml::Value::String(format!("{dp} == '{k}'")));
            }
            // a tool is two documents: one that holds the file asked for, one that holds the tool's name and what came back
            if let Some(si) = tool.iter().position(|d| has(d, "nothing-here").is_some()) {
                let s = &tool[si];
                let (arg, name) = (has(s, "nothing-here"), has(s, "read"));
                let sk = str_at(s, dp).map(String::from).filter(|k| quotable(k));
                let res = tool.iter().enumerate().find(|(i, d)| *i != si && has(d, "read").is_some() && str_at(d, dp).is_some_and(|k| k != dv && Some(k) != sk.as_deref()));
                if let (Some(arg), Some(name), Some(sk)) = (arg, name, sk) {
                    events.push(row(&[("when", format!("{dp} == '{sk}'")), ("emit", "tool".into()), ("name", name), ("detail", arg)]));
                    tools = true;
                    if let Some((_, r)) = res
                        && let (Some(rk), Some(rn), Some(body)) = (str_at(r, dp), has(r, "read"), leaves(r).into_iter().find(|(_, v)| v == "(no such file)"))
                        && quotable(rk)
                    {
                        events.push(row(&[("when", format!("{dp} == '{rk}'")), ("emit", "tool_end".into()), ("name", rn), ("detail", body.0)]));
                    }
                }
            }
        } else {
            // one document holds all: a tool is a pair of objects in a list of it (the first tool of a turn is the one shown)
            if let Some(d) = tool.iter().find(|d| has(d, "nothing-here").is_some())
                && let Some(arg) = has(d, "nothing-here")
            {
                let parent = |p: &str| p.rsplit_once('/').map_or(String::new(), |(a, _)| a.to_string());
                let same = leaves(d).into_iter().find(|(p, v)| v == "read" && parent(p) == parent(&arg)).map(|(p, _)| p);
                let end = leaves(d).into_iter().find(|(p, v)| v == "(no such file)" && *p != text && parent(p) != parent(&arg));
                if let (Some(name), Some((body, _))) = (same, end) {
                    let end_name = format!("{}/name", parent(&body));
                    events.push(row(&[("when", format!("{name} != ''")), ("emit", "tool".into()), ("name", name), ("detail", arg)]));
                    events.push(row(&[("when", format!("{end_name} != ''")), ("emit", "tool_end".into()), ("name", end_name), ("detail", body)]));
                    tools = true;
                }
            }
            if session_flag {
            // the session is the one other id-like text in it: the session is the one other id-like text in it
            let others: Vec<(String, String)> = leaves(say).into_iter().filter(|(p, v)| *p != text && v.len() >= 4 && v.len() <= 40 && !v.contains(' ')).collect();
            let named = others.iter().find(|(p, _)| SESSION_WORDS.iter().any(|w| p.to_lowercase().contains(w)));
            if let Some((p, _)) = named.or(if others.len() == 1 { others.first() } else { None }) {
                map.insert("session".into(), toml::Value::String(p.clone()));
                session = true;
            }
            }
        }
    }
    map.insert("events".into(), toml::Value::Array(events));
    Some(Rows { map, session, tools })
}

/// The flag that carries a session, from the option lines of the help.
fn session_flag(said: &str) -> Option<String> {
    said.lines().filter(|l| l.trim_start().starts_with('-')).find_map(|l| {
        let mut toks = l.split_whitespace();
        let flag = toks.next().and_then(flag_word)?;
        let rest: Vec<&str> = toks.collect();
        (rest.first().is_some_and(|t| is_placeholder(t)) && rest.join(" ").to_lowercase().contains("session")).then(|| flag.to_string())
    })
}

fn word_for(name: &str, n: u32) -> String {
    let h = name.bytes().fold(7u32, |a, b| a.wrapping_mul(31).wrapping_add(u32::from(b)));
    format!("{}{}{}", ["plum", "tarn", "wisp", "fenn"][(h % 4) as usize], h % 90 + 10, n)
}

/// A server found by trying: the arguments that start it, the path and the key of the request.
struct Serving {
    start: Vec<String>,
    path: String,
    key: String,
    /// The request carries the session in the key `session`: it was tried, and the second answer knew the first.
    resume: bool,
}

fn manifest(name: &str, way: &Way, serving: Option<&Serving>, decode: &[Step], rows: &Rows, flag: Option<&str>) -> toml::Table {
    let s = |x: &str| toml::Value::String(x.to_string());
    let list = |xs: Vec<String>| toml::Value::Array(xs.into_iter().map(toml::Value::String).collect());
    let mut t = toml::Table::new();
    let short = std::path::Path::new(name).file_name().and_then(|n| n.to_str()).unwrap_or(name);
    let id: String = short.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '-' }).collect();
    t.insert("id".into(), s(&id));
    t.insert("doc".into(), s(&format!("`{short}`, learned by trying it: it is given its prompt by {}", way.how())));

    t.insert("is".into(), s("agent"));
    t.insert("dialect".into(), s("jsonl-map"));
    // a program J4n has only tried cannot be asked before it writes: each run goes to the Gate
    t.insert("gated".into(), toml::Value::Boolean(true));
    t.insert("program".into(), s(name));
    t.insert("probe".into(), toml::Value::Table([("argv".to_string(), list(vec![name.into(), "--version".into()]))].into_iter().collect()));
    let mut argv = vec![name.to_string()];
    let resume = flag.filter(|_| rows.session && serving.is_none());
    if let Some(f) = resume {
        argv.extend([f.to_string(), "{session}".into()]);
        t.insert("resume".into(), list(vec![f.to_string(), "{session}".into()]));
    }
    argv.extend(way.args.iter().cloned());
    if let Some(sv) = serving {
        argv.extend(sv.start.iter().cloned());
        let body = format!("{{\"{}\": \"{{prompt}}\"}}", sv.key);
        let mut table: toml::Table = [("ready".to_string(), s("http://")), ("path".to_string(), s(&sv.path)), ("body".to_string(), s(&body))].into_iter().collect();
        if sv.resume {
            table.insert("resume".into(), s(&format!("{{\"{}\": \"{{prompt}}\", \"session\": \"{{session}}\"}}", sv.key)));
            t.insert("resume".into(), list(vec!["{session}".into()]));
        }
        t.insert("serve".into(), toml::Value::Table(table));
        t.insert("doc".into(), s(&format!("`{short}`, learned by trying it: a local server it starts, and the prompt is posted to {}", sv.path)));
    }
    t.insert("argv".into(), list(argv));
    if way.stdin {
        t.insert("stdin".into(), s("{prompt}"));
    }
    if let Some(v) = &way.env {
        t.insert("env".into(), toml::Value::Table([(v.clone(), s("{prompt}"))].into_iter().collect()));
    }
    let words: Vec<String> = decode.iter().map(|d| Step::WORDS[*d as usize].to_string()).collect();
    if !words.is_empty() {
        t.insert("decode".into(), list(words));
    }
    t.insert("map".into(), toml::Value::Table(rows.map.clone()));
    t.insert("canary".into(), toml::Value::Table([("goal".to_string(), s("Reply with the single word: ready")), ("expect".to_string(), s("ready"))].into_iter().collect()));
    t
}

/// What the tries said, when none of them settled it: a program that asks for a login says so in the first words it prints.
fn login(outputs: &[String]) -> Option<String> {
    const SAYS: [&str; 10] = ["authentication required", "not authenticated", "not logged in", "login first", "log in first", "sign in first", "no providers configured", "no provider configured", "please log in", "please sign in"];
    outputs.iter().find_map(|o| {
        let low = o.to_lowercase();
        // a page to sign in at, or the words a program says when it has no login or no provider: the line that says it
        let says = SAYS.iter().any(|w| low.contains(w));
        if !(says || ((low.contains("sign in") || low.contains("log in") || low.contains("login") || low.contains("authenticat")) && low.contains("http"))) {
            return None;
        }
        let line = o.lines().find(|l| {
            let l = l.to_lowercase();
            SAYS.iter().any(|w| l.contains(w)) || l.contains("sign in") || l.contains("log in") || l.contains("login")
        });
        Some(clip(line.or_else(|| o.lines().find(|l| !l.trim().is_empty())).unwrap_or_default().trim(), 200))
    })
}

fn answer(settled: bool, why: &str, manifest: &str, transcript: &str, how: &str, login: Option<&str>) -> String {
    json!({ "settled": settled, "refused": false, "why": why, "manifest": manifest, "transcript": transcript, "how": how, "login": login.is_some(), "login_text": login.unwrap_or_default(), "is": "agent", "dialect": "jsonl-map" }).to_string()
}

pub(crate) fn learn(job: &Job, link: &Link) -> Result<String, String> {
    let (name, path) = (job.with("program"), job.with("path"));
    if name.is_empty() || path.is_empty() {
        return Err("connect-learn is given the program and its path".into());
    }
    // a program that says it speaks a protocol J4n has a dialect for (`--acp`, an MCP server) is not tried: the rows were certain. One that says `stream-json`
    // is tried: it may speak what claude does and take other flags for it (a clone does), which only trying shows, and the draft is still there when it settles nothing
    if ["acp", "mcp"].contains(&job.with("dialect")) {
        return Ok(answer(false, &format!("it speaks `{}`, which J4n has a dialect for", job.with("dialect")), "", "", "", None));
    }
    let said = format!("{}\n{}", job.with("help"), job.with("man"));
    job.approve("learn", format!("try `{name}` a few ways with a one-word prompt, behind the box?"), &format!("{path} <prompt>"))?;
    let started = Instant::now();
    let word = word_for(name, 1);
    let asking = |w: &str| format!("Reply with the single word: {w}");
    let mut seen: Vec<String> = Vec::new();
    let mut found: Option<(Way, Vec<Step>, Vec<Json>)> = None;
    // what the program printed: where it says its usage limit is read from
    let mut printed: Vec<String> = Vec::new();
    let (tries, servers) = ways(name, &said);
    let mut ran_over = 0;
    for way in tries {
        if started.elapsed() > BUDGET {
            break;
        }
        let Some((out, _)) = way.run(job, path, &asking(&word)) else {
            // a try that was still running at its clock: after two, the program does not end when it is given words, and each more costs the clock again
            ran_over += 1;
            if ran_over >= 2 {
                break;
            }
            continue;
        };
        if let Some((decode, docs)) = read(&out, &word) {
            printed.push(out);
            found = Some((way, decode, docs));
            break;
        }
        seen.push(out);
    }
    let mut serving: Option<Serving> = None;
    let mut live: Option<(crate::serve::Server, String, String)> = None;
    let mut tool: Vec<Json> = Vec::new();
    if found.is_none() {
        // a program that is a local server: started, its address read from what it prints, the prompt posted
        let (paths, keys) = requests(&said);
        'servers: for start in servers {
            let mut argv = vec![path.to_string()];
            argv.extend(start.iter().cloned());
            let Ok(server) = crate::serve::start(&job.launch(&argv, &[], true), "http://", 8) else { continue };
            for p in &paths {
                for k in &keys {
                    let body = |w: &str| format!("{{\"{k}\": \"{w}\"}}");
                    let Ok(r) = server.post(p, &body(&asking(&word)), 15) else { continue };
                    if !(200..300).contains(&r.status) {
                        seen.push(if matches!(r.status, 401 | 403) { format!("Please sign in: it answered HTTP {} (see http://)", r.status) } else { r.body });
                        continue;
                    }
                    if let Some((decode, docs)) = read(&r.body, &word) {
                        printed.push(r.body.clone());
                        tool = server.post(p, &body("Read nothing-here"), 15).map(|t| self::docs(&decode, &t.body)).unwrap_or_default();
                        serving = Some(Serving { start: start.clone(), path: p.clone(), key: k.clone(), resume: false });
                        live = Some((server, p.clone(), k.clone()));
                        found = Some((Way { args: vec![], stdin: false, env: None }, decode, docs));
                        break 'servers;
                    }
                }
            }
        }
    }
    let Some((way, decode, one)) = found else {
        let login = login(&seen);
        // a program that takes no prompt and says what its commands are is a tool: its commands are learned, and no model is asked
        if login.is_none() && let Some(a) = learn_tool(job, link, name, path, &said)? {
            return Ok(a);
        }
        let why = match &login {
            Some(l) => format!("it asks for a login: {l}"),
            None if ran_over >= 2 => return Ok(refuse_tool(&format!("it did not end when it was given words, in {TRY_SECS} s, twice: a server, a watcher or a program that waits for input; J4n runs a program once and reads what it prints when it ends"))),
            None => format!("none of the ways to give it a prompt printed the word back ({} tried){}", seen.len(), seen.iter().find_map(|o| o.lines().find(|l| !l.trim().is_empty())).map_or(String::new(), |l| format!("; the first said: `{}`", clip(l.trim(), 100)))),
        };
        return Ok(answer(false, &why, "", "", "", login.as_deref()));
    };
    // a filter that hands back what it is given would have printed the word too: an agent answers a prompt that holds no word to give back
    let plain = "Name one colour.";
    let echoed = match (&serving, &live) {
        (Some(_), Some((server, p, k))) => server.post(p, &format!("{{\"{k}\": \"{plain}\"}}"), 15).map(|r| docs(&decode, &r.body)).unwrap_or_default(),
        _ => way.run(job, path, plain).map(|(o, _)| docs(&decode, &o)).unwrap_or_default(),
    };
    if only_returns(&echoed, plain) {
        return Ok(answer(false, "it only gives back what it is handed (a filter, not an agent): it printed the word of the prompt and nothing of its own", "", "", &way.how(), None));
    }
    if serving.is_none() {
        tool = way.run(job, path, "Read nothing-here").map(|(o, _)| docs(&decode, &o)).unwrap_or_default();
    }
    let mut flag = session_flag(&said);
    let Some(rows) = learn_rows(&decode, &one, &word, &tool, flag.is_some() || serving.is_some()) else {
        return Ok(answer(false, "the word was in its output, and no line of it could be read as an answer", "", "", &way.how(), None));
    };
    // a server keeps the session itself: the id of the first answer goes back with the second prompt, and the second answer must know the first
    if let (Some(sv), Some((server, p, k)), true) = (serving.as_mut(), live.as_ref(), rows.session) {
        let at = rows.map.get("session").and_then(toml::Value::as_str).unwrap_or_default();
        if let Some(id) = one.iter().find_map(|d| str_at(d, at).map(String::from)) {
            let body = format!("{{\"{k}\": \"Which word did I ask you to reply with before?\", \"session\": \"{id}\"}}");
            sv.resume = server.post(p, &body, 15).is_ok_and(|r| (200..300).contains(&r.status) && r.body.contains(&word));
        }
    }
    // no flag in what it says of itself: the usual ones are tried, in a folder it keeps its session in
    if flag.is_none() && rows.session && serving.is_none() {
        let at = rows.map.get("session").and_then(toml::Value::as_str).unwrap_or_default().to_string();
        let at = if at.is_empty() { None } else { Some(at) };
        if let Some(at) = at {
            flag = way.session_flag(job, path, &word_for(name, 3), &at);
        }
    }
    let limits = learn_limits(job, path, &said, &way, serving.as_ref().zip(live.as_ref()), printed);
    let mut table = manifest(name, &way, serving.as_ref(), &decode, &rows, flag.as_deref());
    if !limits.is_empty() {
        table.insert("limits".into(), toml::Value::Array(limits.iter().filter_map(|r| toml::Value::try_from(r).ok()).collect()));
    }
    let text = toml::to_string(&table).map_err(|e| e.to_string())?;
    let (bad, entry) = judge(&text, name, ("agent", "jsonl-map"), link);
    if !bad.is_empty() || entry.is_none() || !id_ok(table.get("id").and_then(toml::Value::as_str).unwrap_or_default()) {
        return Ok(answer(false, &format!("what was learned is not a connector J4n takes: {}", bad.join("; ")), &text, "", &way.how(), None));
    }
    let verdict: Json = serde_json::from_str(&canary_of(job, link, &text)?).map_err(|e| e.to_string())?;
    let pass = verdict.get("pass").and_then(Json::as_bool).unwrap_or(false);
    let transcript = verdict.get("transcript").and_then(Json::as_str).unwrap_or_default();
    let concerns = verdict.get("concerns").map(|c| c.to_string()).unwrap_or_default();
    let manifest = verdict.get("manifest").and_then(Json::as_str).unwrap_or(&text);
    let how = format!("{}{}{}", serving.as_ref().map_or_else(|| way.how(), |sv| format!("a server (`{}`), posted to {}", sv.start.join(" "), sv.path)), if rows.session { ", with a session" } else { "" }, if rows.tools { ", its tools shown" } else { "" });
    Ok(if pass { answer(true, "", manifest, transcript, &how, None) } else { answer(false, &format!("the canary of what was learned did not pass: {concerns}"), manifest, transcript, &how, None) })
}

/// What the program says of its own usage limit, as `limits` rows: in what a turn printed (twice more, in a folder that keeps what the first left, so
/// a count that moves shows), and in the subcommands its help says report it. Nothing is pushed to a limit: a refusal is read when it comes.
fn learn_limits(job: &Job, path: &str, said: &str, way: &Way, served: Option<(&Serving, &(crate::serve::Server, String, String))>, mut printed: Vec<String>) -> Vec<limits::Row> {
    let (again, again2) = (word_for(job.with("program"), 5), word_for(job.with("program"), 6));
    let ask = |w: &str| format!("Reply with the single word: {w}");
    match served {
        Some((_, (server, p, k))) => {
            for w in [&again, &again2] {
                if let Ok(r) = server.post(p, &format!("{{\"{k}\": \"{}\"}}", ask(w)), 15) {
                    printed.push(r.body);
                }
            }
        }
        None => {
            let sub = format!(".j4n-limit-{}", std::process::id());
            for w in [&again, &again2] {
                if let Some((out, _)) = way.run_in(job, path, &ask(w), &sub, &[]) {
                    printed.push(out);
                }
            }
            let _ = job.trial(&["rm".into(), "-rf".into(), job.cx.site.dir.join(&sub).display().to_string()], None, &[], 5, false);
        }
    }
    let mut rows = limits::learn(&printed);
    for command in limits::commands(said) {
        let mut argv = vec![path.to_string()];
        argv.extend(command.iter().cloned());
        if let Some((out, _)) = job.trial(&argv, None, &[], 8, true) {
            rows.extend(limits::learn(&[out]).into_iter().take(1).map(|r| limits::Row { command: command.clone(), ..r }));
        }
    }
    rows.truncate(3);
    rows
}

// ---- a tool: a command, not an agent ----

/// One command of a tool, as its own words give it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Command {
    name: String,
    args: Vec<String>,
    about: String,
}

fn placeholder(t: &str) -> Option<String> {
    let t = t.trim_matches(|c: char| "[](),.<>".contains(c));
    (!t.is_empty() && is_placeholder(t) || (t.len() >= 2 && t.chars().all(|c| c.is_ascii_uppercase() || c == '_'))).then(|| t.to_lowercase().replace('-', "_"))
}

fn is_word(t: &str) -> bool {
    !t.is_empty() && t.len() <= 24 && t.starts_with(|c: char| c.is_ascii_lowercase()) && t.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// The commands a help text names: `usage:` alternatives (`x show FILE | x put TEXT FILE`), and the lines of a `Commands:` section
/// (`  show FILE  prints the file`).
fn commands(name: &str, text: &str) -> Vec<Command> {
    let mut out: Vec<Command> = Vec::new();
    let mut add = |c: Command| match out.iter_mut().find(|o| o.name == c.name) {
        Some(o) => {
            if o.args.is_empty() {
                o.args = c.args;
            }
            if o.about.is_empty() {
                o.about = c.about;
            }
        }
        None => out.push(c),
    };
    let short = std::path::Path::new(name).file_name().and_then(|n| n.to_str()).unwrap_or(name);
    let mut section = false;
    for raw in text.lines() {
        let l = raw.trim();
        let lower = l.to_lowercase();
        if lower.starts_with("commands") || lower.starts_with("subcommands") || lower.starts_with("available commands") {
            section = true;
            if let Some((_, list)) = l.split_once(':').filter(|(_, r)| !r.trim().is_empty() && !r.contains("  ")) {
                list.split(|c: char| c == ',' || c.is_whitespace()).filter(|w| is_word(w)).for_each(|w| add(Command { name: w.to_string(), args: vec![], about: String::new() }));
            }
            continue;
        }
        if l.is_empty() || (!raw.starts_with(' ') && l.ends_with(':')) {
            section = false;
        }
        let usage = l.trim_start_matches("Usage:").trim_start_matches("usage:").trim();
        if let Some(after) = usage.strip_prefix(short).filter(|a| a.starts_with(' ')) {
            let _ = after;
            for alt in usage.split(" | ") {
                let toks: Vec<&str> = alt.split_whitespace().collect();
                if toks.first() == Some(&short) && toks.get(1).is_some_and(|w| is_word(w)) && !toks[1].starts_with('[') {
                    add(Command { name: toks[1].to_string(), args: toks[2..].iter().map_while(|t| placeholder(t)).collect(), about: String::new() });
                }
            }
        }
        if section && raw.starts_with(' ') {
            let (left, about) = l.split_once("  ").map_or((l, ""), |(a, b)| (a, b.trim()));
            let toks: Vec<&str> = left.split_whitespace().collect();
            if toks.first().is_some_and(|w| is_word(w)) && toks.iter().skip(1).all(|t| placeholder(t).is_some()) {
                add(Command { name: toks[0].to_string(), args: toks[1..].iter().filter_map(|t| placeholder(t)).collect(), about: about.to_string() });
            }
        }
    }
    out
}

/// What running a command can do, from what its words say of it and then from its name: `read`, `write`, `net`, or `exec` (the schema's word for the
/// most: a delete, anything that is not known).
fn risk_of(name: &str, about: &str, args: &[String]) -> &'static str {
    let (about, name) = (about.to_lowercase(), name.to_lowercase());
    let out_arg = args.iter().any(|a| ["out", "output", "dest", "destination", "target"].contains(&a.as_str()));
    let has = |words: &[&str], t: &str| words.iter().any(|w| t.split(|c: char| !c.is_alphanumeric()).any(|x| x == *w || (w.len() > 4 && x.starts_with(w))));
    const DESTROY: [&str; 12] = ["delete", "deletes", "remove", "removes", "destroy", "destroys", "drop", "purge", "wipe", "erase", "unlink", "force"];
    const NET: [&str; 10] = ["network", "download", "downloads", "upload", "uploads", "fetch", "fetches", "push", "pushes", "http"];
    const WRITE: [&str; 14] = ["write", "writes", "overwrite", "overwrites", "create", "creates", "save", "saves", "convert", "converts", "install", "installs", "modify", "modifies"];
    const READ: [&str; 12] = ["print", "prints", "show", "shows", "list", "lists", "count", "counts", "read", "reads", "display", "displays"];
    if has(&DESTROY, &about) || has(&["rm", "del", "delete", "remove", "drop", "purge", "wipe", "destroy", "clean", "kill"], &name) {
        return "exec";
    }
    if has(&NET, &about) || has(&["fetch", "get", "download", "upload", "push", "pull", "curl", "http", "publish", "sync", "send"], &name) {
        return "net";
    }
    if out_arg || has(&WRITE, &about) || has(&["put", "write", "set", "add", "create", "save", "new", "init", "convert", "move", "mv", "copy", "cp", "touch", "install", "build"], &name) {
        return "write";
    }
    if has(&READ, &about) || has(&["show", "cat", "list", "ls", "count", "wc", "get", "view", "print", "status", "info", "find", "search", "grep"], &name) {
        return "read";
    }
    "exec"
}

/// The flags that ask for JSON, when what it says offers them.
fn json_flag(text: &str) -> Vec<String> {
    let low = text.to_lowercase();
    if low.contains("-o json") {
        vec!["-o".into(), "json".into()]
    } else if low.contains("--format json") {
        vec!["--format".into(), "json".into()]
    } else if low.contains("--json") {
        vec!["--json".into()]
    } else {
        vec![]
    }
}

/// A tool learned from what it says: the commands, each a connector of its own (one Proposal writes them all), the arguments each takes, a `risk` for each,
/// the flag that asks for JSON, and a canary that has no effect (`--help`). Only what the help marks as read-only is run: `--help`, `help <command>`, the usage
/// an invocation with nothing prints, and `--version`; a command is never run, whatever its name says, and none of them is a delete.
fn learn_tool(job: &Job, link: &Link, name: &str, path: &str, said: &str) -> Result<Option<String>, String> {
    let low = said.to_lowercase();
    // an agent that was not learned is for the model to draft: a tool is a command that takes no prompt
    if low.contains("prompt") || low.contains("session") || low.contains("conversation") {
        return Ok(None);
    }
    let mut text = said.to_string();
    let run = |args: &[&str]| job.trial(&std::iter::once(path.to_string()).chain(args.iter().map(|a| a.to_string())).collect::<Vec<_>>(), None, &[], TRY_SECS, true);
    let (mut canary, mut helped) = (vec!["--version".to_string()], false);
    for (i, way) in [&["--help"][..], &["-h"], &["help"]].into_iter().enumerate() {
        let tried = run(way);
        // asked for its help and still running at the clock: it does not end by itself (a server, a watcher), and the other ways are not tried
        if tried.is_none() && i == 0 {
            return Ok(Some(refuse_tool(&format!("`{} --help` was still running after {TRY_SECS} s: it does not end by itself (a server, a watcher or a program that waits for input), and J4n runs a program once and reads what it prints when it ends", short_of(name)))));
        }
        if let Some((o, Some(0))) = tried.filter(|(o, _)| !o.trim().is_empty()) {
            text.push('\n');
            text.push_str(&o);
            canary = way.iter().map(|a| a.to_string()).collect();
            helped = true;
            break;
        }
    }
    // with nothing said, what an invocation with nothing prints (a usage) is what there is
    // never a program that does something when it is started with nothing to do (a build, a delete, the machine)
    const BARE_NOT: [&str; 14] = ["rm", "dd", "mkfs", "shutdown", "reboot", "halt", "poweroff", "kill", "killall", "make", "ninja", "cargo", "npm", "pip"];
    let base = std::path::Path::new(name).file_name().and_then(|n| n.to_str()).unwrap_or(name);
    // what it says with nothing to do is also where a program that wants a terminal says so: it is read for that
    let mut bare = None;
    if !BARE_NOT.contains(&base) && let Some((o, over, code)) = job.trial_out(&std::iter::once(path.to_string()).collect::<Vec<_>>(), TRY_SECS) {
        // still running at the clock, with an address printed: a server, which is not a command that ends; one that printed nothing waits for what it is not given
        if over && let Some(line) = o.lines().find(|l| is_listening(l)) {
            return Ok(Some(refuse_tool(&format!("it is a server (`{}`) and was still running after {TRY_SECS} s: J4n runs a program once and reads what it prints when it ends", line.trim().chars().take(100).collect::<String>()))));
        }
        if !helped && !over {
            text.push('\n');
            text.push_str(&o);
        }
        bare = (!over).then_some((o, code));
    }
    if let Some((o, _)) = bare.as_ref().filter(|(o, code)| *code != Some(0) && wants_a_terminal(o)) {
        let line = o.lines().find(|l| wants_a_terminal(l)).unwrap_or_default().trim().chars().take(120).collect::<String>();
        return Ok(Some(refuse_tool(&format!("it needs a terminal (`{line}`), and J4n runs a program with none: what it prints is read when it ends"))));
    }
    let mut cmds = commands(name, &text);
    // `help <command>` says what one takes
    {
        for c in cmds.iter_mut().filter(|c| c.args.is_empty() || c.about.is_empty()) {
            let tried = run(&["help", &c.name]);
            // one that did not end in its clock is not asked again for each command: that would be the clock times the number of commands
            if tried.is_none() {
                break;
            }
            if let Some((o, Some(0))) = tried {
                text.push('\n');
                text.push_str(&o);
                let more = commands(name, &format!("Commands:\n{}", o.lines().map(|l| if l.starts_with(' ') { l.to_string() } else { format!("  {l}") }).collect::<Vec<_>>().join("\n")));
                if let Some(m) = more.into_iter().find(|m| m.name == c.name) {
                    if !m.args.is_empty() {
                        c.args = m.args;
                    }
                    c.about = m.about;
                }
            }
        }
    }
    let filter = cmds.is_empty() && (low.contains("standard input") || low.contains("stdin"));
    if cmds.is_empty() && !filter {
        return Ok(None);
    }
    let flag = json_flag(&text);
    let short = std::path::Path::new(name).file_name().and_then(|n| n.to_str()).unwrap_or(name).to_lowercase();
    let s = |x: &str| toml::Value::String(x.to_string());
    let list = |xs: Vec<String>| toml::Value::Array(xs.into_iter().map(toml::Value::String).collect());
    let mut manifests = Vec::new();
    let named: Vec<Command> = if filter { vec![Command { name: String::new(), args: vec!["input".into()], about: "upper-cases or filters what it reads on standard input".into() }] } else { cmds };
    for c in &named {
        let mut t = toml::Table::new();
        let id: String = if named.len() == 1 && c.name.is_empty() || named.len() == 1 { short.clone() } else { format!("{short}-{}", c.name) }.chars().map(|x| if x.is_ascii_alphanumeric() || x == '_' || x == '-' { x } else { '-' }).collect();
        t.insert("id".into(), s(&id));
        t.insert("doc".into(), s(&format!("`{short}{}`: {}", if c.name.is_empty() { String::new() } else { format!(" {}", c.name) }, if c.about.is_empty() { "learned from its help" } else { &c.about })));
        t.insert("is".into(), s("tool"));
        t.insert("dialect".into(), s("text"));
        t.insert("program".into(), s(name));
        let risk = if filter { "read" } else { risk_of(&c.name, &c.about, &c.args) };
        t.insert("risk".into(), s(risk));
        t.insert("says".into(), s(risk));
        let mut argv = vec![name.to_string()];
        if !c.name.is_empty() {
            argv.push(c.name.clone());
        }
        argv.extend(flag.iter().cloned());
        argv.extend(c.args.iter().filter(|_| !filter).map(|a| format!("{{{a}}}")));
        t.insert("argv".into(), list(argv));
        if filter {
            t.insert("stdin".into(), s("{input}"));
        }
        let args: Vec<toml::Value> = c.args.iter().map(|a| toml::Value::Table([("name".to_string(), s(a)), ("required".to_string(), toml::Value::Boolean(true))].into_iter().collect())).collect();
        t.insert("args".into(), toml::Value::Array(args));
        t.insert("canary".into(), toml::Value::Table([("argv".to_string(), list(canary.clone()))].into_iter().collect()));
        t.insert("probe".into(), toml::Value::Table([("argv".to_string(), list(vec![name.to_string(), "--version".to_string()]))].into_iter().collect()));
        manifests.push(toml::to_string(&t).map_err(|e| e.to_string())?);
    }
    for m in &manifests {
        let (bad, entry) = judge(m, name, ("tool", "text"), link);
        if !bad.is_empty() || entry.is_none() {
            return Ok(Some(answer_tool(false, &format!("what was learned of its commands is not a connector J4n takes: {}", bad.join("; ")), &manifests.join("\n---\n"), "", &short)));
        }
    }
    let verdict: Json = serde_json::from_str(&canary_of(job, link, &manifests[0])?).map_err(|e| e.to_string())?;
    let pass = verdict.get("pass").and_then(Json::as_bool).unwrap_or(false);
    let transcript = verdict.get("transcript").and_then(Json::as_str).unwrap_or_default();
    let how = format!("{} command{} ({})", named.len(), if named.len() == 1 { "" } else { "s" }, named.iter().map(|c| format!("{} {} [{}] <{}>", if c.name.is_empty() { "stdin" } else { &c.name }, risk_of(&c.name, &c.about, &c.args), c.args.join(","), c.about)).collect::<Vec<_>>().join(", "));
    Ok(Some(answer_tool(pass, if pass { "" } else { "its `--help` did not answer with words and exit 0" }, &manifests.join("\n---\n"), transcript, &how)))
}

fn short_of(name: &str) -> &str {
    std::path::Path::new(name).file_name().and_then(|n| n.to_str()).unwrap_or(name)
}

/// Whether a line says the program needs a terminal to run in.
fn wants_a_terminal(line: &str) -> bool {
    let low = line.to_lowercase();
    ["needs a terminal", "requires a terminal", "needs a tty", "requires a tty", "not a tty", "not a terminal", "not a tty", "must be run in a terminal", "must be run from a terminal", "interactive terminal", "inappropriate ioctl"].iter().any(|w| low.contains(w))
}

/// Whether a line says the program is serving at an address.
fn is_listening(line: &str) -> bool {
    let low = line.to_lowercase();
    ["listening on", "listening at", "serving on", "serving at", "server started", "started server", "running on http", "running at http"].iter().any(|w| low.contains(w))
}

/// What a tool that J4n does not connect says: the flow ends with it, one line.
fn refuse_tool(why: &str) -> String {
    json!({ "settled": false, "refused": true, "why": why, "manifest": "", "transcript": "", "how": "", "login": false, "login_text": "", "is": "tool", "dialect": "text" }).to_string()
}

fn answer_tool(settled: bool, why: &str, manifest: &str, transcript: &str, how: &str) -> String {
    json!({ "settled": settled, "refused": false, "why": why, "manifest": manifest, "transcript": transcript, "how": how, "login": false, "login_text": "", "is": "tool", "dialect": "text" }).to_string()
}

