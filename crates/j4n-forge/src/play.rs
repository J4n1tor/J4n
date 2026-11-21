//! The harness itself: `j4n-forge play SPEC [ARGS…]`. It answers by script, not by a model: the canary word back, `write <text> to
//! <file>` and `read <file>` on disk, tool events, and it spends its usage limit. What it does is what its spec says on each axis.

use crate::spec::{Axis, Spec};
use serde_json::{Value as Json, json};
use std::collections::BTreeMap;
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};

fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

fn words(s: &str) -> usize {
    s.split_whitespace().count()
}

// ---- what a turn says ----

pub enum Ev {
    Session(String),
    Say(String),
    Tool { start: bool, name: String, detail: String },
    Usage { text: String, data: Json },
}

/// Writes the invalid byte where a private-use character stands, so a harness can print what is not UTF-8.
struct Raw(Box<dyn Write>);

impl Write for Raw {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut out = Vec::with_capacity(buf.len());
        let mut i = 0;
        while i < buf.len() {
            if buf[i..].starts_with(&[0xEE, 0x80, 0x80]) {
                out.push(0xFF);
                i += 3;
            } else {
                out.push(buf[i]);
                i += 1;
            }
        }
        self.0.write_all(&out)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

/// The shape of the output (axis B): events in, bytes out.
pub struct Sink {
    kind: String,
    names: BTreeMap<String, String>,
    depth: usize,
    out: Raw,
    said: Vec<String>,
    tools: Vec<Json>,
    session: Option<String>,
    usage: Option<Json>,
    started: bool,
}

impl Sink {
    pub fn new(kind: &str, names: &BTreeMap<String, String>, depth: usize, out: Box<dyn Write>) -> Sink {
        Sink { kind: kind.into(), names: names.clone(), depth, out: Raw(out), said: vec![], tools: vec![], session: None, usage: None, started: false }
    }

    fn n(&self, key: &str) -> String {
        self.names.get(key).cloned().unwrap_or_else(|| key.to_string())
    }

    /// One record: the kind's word, and the fields under their names, wrapped `depth - 1` times.
    fn rec(&self, kind: &str, fields: Vec<(&str, Json)>) -> (String, serde_json::Map<String, Json>, Json) {
        let word = self.n(kind);
        let mut inner = serde_json::Map::new();
        inner.insert(self.n("kind"), Json::String(word.clone()));
        for (k, v) in fields {
            inner.insert(self.n(k), v);
        }
        let mut wrapped = Json::Object(inner.clone());
        for level in (0..self.depth.saturating_sub(1)).rev() {
            wrapped = json!({ self.n(&format!("wrap{}", level.min(1))): wrapped });
        }
        (word, inner, wrapped)
    }

    pub fn event(&mut self, ev: Ev) {
        let (kind, fields): (&str, Vec<(&str, Json)>) = match &ev {
            Ev::Session(id) => ("session", vec![("id", json!(id))]),
            Ev::Say(t) => ("say", vec![("body", json!(t))]),
            Ev::Tool { start: true, name, detail } => ("tool", vec![("name", json!(name)), ("arg", json!(detail))]),
            Ev::Tool { start: false, name, detail } => ("result", vec![("name", json!(name)), ("body", json!(detail))]),
            Ev::Usage { data, .. } => ("usage", vec![("body", data.clone())]),
        };
        let colour = self.kind == "ansi";
        match self.kind.as_str() {
            "plain" | "ansi" => {
                if colour && !self.started {
                    let _ = write!(self.out, "\r\u{2819} thinking\u{2026}\r\u{2803} thinking\u{2026}\r\x1b[K");
                }
                let line = match &ev {
                    Ev::Session(id) => format!("session: {id}"),
                    Ev::Say(t) => t.clone(),
                    Ev::Tool { start: true, name, detail } => format!("* {name} {detail}"),
                    Ev::Tool { name, detail, .. } => format!("= {name}: {detail}"),
                    Ev::Usage { text, .. } => text.clone(),
                };
                let _ = if colour { writeln!(self.out, "\x1b[1;32m{line}\x1b[0m") } else { writeln!(self.out, "{line}") };
            }
            "json" | "http" => {
                match &ev {
                    Ev::Session(id) => self.session = Some(id.clone()),
                    Ev::Say(t) => self.said.push(t.clone()),
                    Ev::Tool { start, name, detail } => self.tools.push(json!({ "phase": if *start { "begin" } else { "end" }, "name": name, "text": detail })),
                    Ev::Usage { data, .. } => self.usage = Some(data.clone()),
                }
                self.started = true;
                return;
            }
            other => {
                let (word, inner, wrapped) = self.rec(kind, fields);
                let line = match other {
                    "jsonl" => format!("{wrapped}\n"),
                    "sse" => format!("event: {word}\ndata: {wrapped}\n\n"),
                    "frames" => {
                        let payload = wrapped.to_string();
                        format!("{:06}{payload}\n", payload.len())
                    }
                    "xml" => {
                        let attrs: String = inner.iter().filter(|(k, _)| **k != self.names["kind"] && **k != self.names["body"]).map(|(k, v)| format!(" {k}=\"{}\"", esc(&text_of(v)))).collect();
                        match inner.get(&self.names["body"]) {
                            Some(b) => format!("<{word}{attrs}>{}</{word}>\n", esc(&text_of(b))),
                            None => format!("<{word}{attrs}/>\n"),
                        }
                    }
                    "logfmt" => {
                        let pairs: String = inner.iter().filter(|(k, _)| **k != self.names["kind"]).map(|(k, v)| format!(" {k}={}", quote(&text_of(v)))).collect();
                        format!("lvl=info evt={word}{pairs}\n")
                    }
                    _ => {
                        let params: serde_json::Map<String, Json> = inner.into_iter().filter(|(k, _)| *k != self.names["kind"]).collect();
                        format!("{}\n", json!({ "jsonrpc": "2.0", "method": word, "params": params }))
                    }
                };
                let _ = self.out.write_all(line.as_bytes());
            }
        }
        self.started = true;
        let _ = self.out.flush();
    }

    pub fn end(&mut self) {
        let end = self.rec("end", vec![]);
        let line = match self.kind.as_str() {
            "json" | "http" => {
                let mut obj = serde_json::Map::new();
                obj.insert(self.n("say"), json!(self.said.join("\n")));
                if let Some(id) = &self.session {
                    obj.insert(self.n("session"), json!(id));
                }
                if !self.tools.is_empty() {
                    obj.insert("tools".into(), Json::Array(std::mem::take(&mut self.tools)));
                }
                if let Some(u) = &self.usage {
                    obj.insert(self.n("usage"), u.clone());
                }
                self.said.clear();
                format!("{}\n", Json::Object(obj))
            }
            "sse" => format!("event: {}\ndata: [DONE]\n\n", end.0),
            "jsonl" => format!("{}\n", end.2),
            "frames" => {
                let payload = end.2.to_string();
                format!("{:06}{payload}\n", payload.len())
            }
            "xml" => format!("<{}/>\n", end.0),
            "logfmt" => format!("lvl=info evt={}\n", end.0),
            "jsonrpc" => format!("{}\n", json!({ "jsonrpc": "2.0", "id": 1, "result": { "done": true } })),
            _ => String::new(),
        };
        let _ = self.out.write_all(line.as_bytes());
        let _ = self.out.flush();
        self.started = false;
    }
}

fn text_of(v: &Json) -> String {
    v.as_str().map_or_else(|| v.to_string(), str::to_string)
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn quote(s: &str) -> String {
    if s.chars().any(|c| c.is_whitespace() || c == '"' || c == '=') || s.is_empty() { format!("{s:?}") } else { s.to_string() }
}

// ---- the limit (axis E) ----

pub struct Quota {
    kind: String,
    unit: String,
    cap: f64,
    window: f64,
    refill_hour: f64,
    t0: f64,
    used: f64,
    bal: f64,
    t_last: f64,
    path: PathBuf,
}

impl Quota {
    fn load(e: &Axis, dir: &Path) -> Quota {
        let path = dir.join("state.json");
        let saved: Json = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Json::Null);
        let f = |k: &str, d: f64| saved.get(k).and_then(Json::as_f64).unwrap_or(d);
        let cap = e.num("cap");
        Quota { kind: e.kind.clone(), unit: e.get("unit").into(), cap, window: e.num("window"), refill_hour: e.num("refill_hour"), t0: f("t0", now()), used: f("used", 0.0), bal: f("bal", cap), t_last: f("t_last", now()), path }
    }

    fn beans(&self) -> bool {
        self.kind == "beans"
    }

    /// Windows end, beans decay (3 % a minute) and refill at their hour.
    fn roll(&mut self, at: f64) {
        if self.beans() {
            self.bal *= 0.97f64.powf((at - self.t_last).max(0.0) / 60.0);
            if self.next_refill(self.t_last) <= at {
                self.bal = self.cap;
            }
            self.t_last = at;
        } else if at - self.t0 >= self.window {
            self.used = 0.0;
            self.t0 = at;
        }
    }

    fn next_refill(&self, from: f64) -> f64 {
        let day = 86_400.0;
        let mut t = (from / day).floor() * day + self.refill_hour * 3600.0;
        if t <= from {
            t += day;
        }
        t
    }

    fn left(&self) -> f64 {
        if self.beans() { self.bal } else { self.cap - self.used }
    }

    fn reset_s(&self, at: f64) -> f64 {
        if self.beans() { self.next_refill(at) - at } else { (self.t0 + self.window - at).max(0.0) }
    }

    fn save(&self, at: f64, limited: bool) {
        let state = json!({ "t0": self.t0, "used": self.used, "bal": self.bal, "t_last": self.t_last });
        let truth = json!({ "unit": self.unit, "cap": self.cap, "used": self.cap - self.left(), "left": self.left(), "reset_s": self.reset_s(at), "limited": limited, "at": at });
        let _ = std::fs::write(&self.path, state.to_string());
        let _ = std::fs::write(self.path.with_file_name("limit.json"), truth.to_string());
    }

    fn data(&self, at: f64) -> Json {
        json!({ "used": round(self.cap - self.left()), "left": round(self.left()), "cap": self.cap, "unit": self.unit, "reset_s": self.reset_s(at).round() })
    }
}

fn round(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

fn retry_in(secs: f64) -> String {
    let s = secs.round() as u64;
    if s >= 60 { format!("{}m{}s", s / 60, s % 60) } else { format!("{s}s") }
}

// ---- the harness ----

pub struct Harness {
    spec: Spec,
    dir: PathBuf,
    names: BTreeMap<String, String>,
}

/// What a turn came to: its exit status, and the seconds to wait when it was refused for the limit.
pub struct Outcome {
    pub code: i32,
    pub limited: Option<f64>,
}

enum Intent {
    Canary(String),
    Write(String, String),
    Read(String),
    Recall,
    Trigger,
    Other,
}

fn intent(prompt: &str) -> Intent {
    let low = prompt.to_lowercase();
    let t = prompt.trim();
    if let Some(i) = low.find("reply with the single word:") {
        let word = t[i + "reply with the single word:".len()..].trim().trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_');
        return Intent::Canary(word.to_string());
    }
    if low.starts_with("write ")
        && let Some(i) = t.rfind(" to ")
    {
        return Intent::Write(t[6..i].trim().to_string(), t[i + 4..].trim().trim_end_matches('.').to_string());
    }
    if low.starts_with("read ") {
        return Intent::Read(t[5..].trim().trim_end_matches('.').to_string());
    }
    if low.contains("which word") || low.contains("earlier word") {
        return Intent::Recall;
    }
    if low.contains("trigger failure") {
        return Intent::Trigger;
    }
    Intent::Other
}

fn safe(path: &str) -> bool {
    !path.is_empty() && !path.starts_with('/') && !path.contains("..")
}

impl Harness {
    fn state_dir(&self) -> PathBuf {
        let d = PathBuf::from(format!(".{}", self.spec.name));
        let _ = std::fs::create_dir_all(d.join("sessions"));
        d
    }

    fn sink(&self, out: Box<dyn Write>) -> Sink {
        Sink::new(&self.spec.b.kind, &self.names, self.spec.b.num("depth") as usize, out)
    }

    /// Whether the history of a session (the prompts so far) is found, by the kind of session (axis C).
    fn history(&self, id: Option<&str>, fresh: bool) -> (Vec<String>, String, bool) {
        let dir = self.state_dir();
        let (path, new_id) = match (self.spec.c.kind.as_str(), id) {
            ("flag" | "expire", Some(id)) => (dir.join("sessions").join(format!("{id}.json")), id.to_string()),
            ("flag" | "expire", None) => {
                let id = format!("s{:x}", (now() * 1000.0) as u64 % 0xFF_FFFF);
                (dir.join("sessions").join(format!("{id}.json")), id)
            }
            ("file", _) => (PathBuf::from(format!(".{}-session.json", self.spec.name)), String::new()),
            ("server", _) => (dir.join("server.json"), String::new()),
            _ => return (vec![], String::new(), false),
        };
        let mut hist: Vec<String> = if fresh { vec![] } else { std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default() };
        let mut expired = false;
        if self.spec.c.kind == "expire" && hist.len() as f64 >= self.spec.c.num("k") {
            hist.clear();
            expired = true;
        }
        (hist, new_id, expired)
    }

    fn keep(&self, id: &str, hist: &[String]) {
        let path = match self.spec.c.kind.as_str() {
            "flag" | "expire" => self.state_dir().join("sessions").join(format!("{id}.json")),
            "file" => PathBuf::from(format!(".{}-session.json", self.spec.name)),
            "server" => self.state_dir().join("server.json"),
            _ => return,
        };
        let _ = std::fs::write(path, serde_json::to_string(hist).unwrap_or_default());
    }

    fn turn(&self, prompt: &str, sess: Option<&str>, fresh: bool, sink: &mut Sink, stdin: &mut dyn BufRead) -> Outcome {
        let name = &self.spec.name;
        let g = self.spec.g.kind.as_str();
        if g == "login" {
            if self.spec.b.kind != "http" {
                println!("Please sign in to continue: https://auth.{name}.example/login?code=K7Q2-XB9D");
            }
            return Outcome { code: 3, limited: None };
        }
        if g == "slow_start" {
            std::thread::sleep(std::time::Duration::from_secs(self.spec.g.num("delay") as u64));
        }
        let at = now();
        let e = &self.spec.e;
        let mut quota = (e.kind != "none").then(|| Quota::load(e, &self.state_dir()));
        let spend = if e.kind == "tokens" || e.kind == "trailer" || e.kind == "usagecmd" { 4.0 * words(prompt) as f64 + 16.0 } else { e.num("cost").max(1.0) };
        if let Some(q) = quota.as_mut() {
            q.roll(at);
            if q.left() < spend {
                q.save(at, true);
                return self.refuse(q, at, sink);
            }
            q.used += if q.beans() { 0.0 } else { spend };
            if q.beans() {
                q.bal -= spend;
            }
            q.save(at, false);
        }
        let (mut hist, mut id, expired) = self.history(sess, fresh);
        if matches!(self.spec.c.kind.as_str(), "flag" | "expire") {
            if sess.is_some() && hist.is_empty() && !expired && !self.state_dir().join("sessions").join(format!("{id}.json")).exists() {
                eprintln!("error: no such session: {id}");
                return Outcome { code: 4, limited: None };
            }
            if expired {
                eprintln!("note: session {id} expired, a new one was started");
                id = format!("s{:x}", (now() * 1000.0) as u64 % 0xFF_FFFF + 1);
            }
            sink.event(Ev::Session(id.clone()));
        } else if self.spec.c.kind == "server" {
            eprintln!("(session kept by the {name} server)");
        }
        let d = self.spec.d.kind.as_str();
        match intent(prompt) {
            Intent::Canary(w) => sink.event(Ev::Say(w)),
            Intent::Recall => {
                let before = hist.iter().rev().find_map(|p| match intent(p) {
                    Intent::Canary(w) => Some(w),
                    _ => None,
                });
                sink.event(Ev::Say(before.unwrap_or_else(|| "I have no earlier word.".into())));
            }
            Intent::Write(text, file) => self.write(d, &text, &file, sink, stdin),
            Intent::Read(file) => match d {
                "none" => sink.event(Ev::Say("I have no tools to read files.".into())),
                _ if !safe(&file) => sink.event(Ev::Say("That path is not allowed.".into())),
                _ => {
                    let loud = d != "silent";
                    if loud {
                        sink.event(Ev::Tool { start: true, name: "read".into(), detail: file.clone() });
                    }
                    let body = std::fs::read_to_string(&file).unwrap_or_else(|_| "(no such file)".into());
                    if loud {
                        sink.event(Ev::Tool { start: false, name: "read".into(), detail: body.clone() });
                    }
                    sink.event(Ev::Say(body));
                }
            },
            Intent::Trigger => self.fail(sink),
            Intent::Other => sink.event(Ev::Say("ok".into())),
        }
        hist.push(prompt.to_string());
        self.keep(&id, &hist);
        let mut code = 0;
        if let Some(q) = &quota {
            code = self.announce(q, at, sink);
        }
        sink.end();
        Outcome { code, limited: None }
    }

    fn write(&self, d: &str, text: &str, file: &str, sink: &mut Sink, stdin: &mut dyn BufRead) {
        if d == "none" {
            return sink.event(Ev::Say("I have no tools to write files.".into()));
        }
        if !safe(file) {
            return sink.event(Ev::Say("That path is not allowed.".into()));
        }
        let loud = d != "silent";
        if loud {
            sink.event(Ev::Tool { start: true, name: "write".into(), detail: file.into() });
        }
        let mut line = String::new();
        let allowed = match d {
            "ask_stdin" => {
                print!("Allow write to {file}? [y/n] ");
                let _ = std::io::stdout().flush();
                let _ = stdin.read_line(&mut line);
                line.trim().to_lowercase().starts_with('y')
            }
            "ask_json" => {
                println!("{}", json!({ "request": "permission", "tool": "write", "path": file, "id": 1 }));
                let _ = std::io::stdout().flush();
                let _ = stdin.read_line(&mut line);
                serde_json::from_str::<Json>(line.trim()).ok().and_then(|v| v.get("allow").and_then(Json::as_bool)).unwrap_or(false)
            }
            _ => true,
        };
        if allowed {
            let _ = std::fs::write(file, text);
        }
        if loud {
            sink.event(Ev::Tool { start: false, name: "write".into(), detail: if allowed { format!("wrote {} bytes", text.len()) } else { "denied".into() } });
        }
        sink.event(Ev::Say(if allowed { format!("Done: wrote {file}") } else { "Not allowed: nothing was written.".into() }));
    }

    /// What the harness does on the failing turn (axis G): the lab says "trigger failure".
    fn fail(&self, sink: &mut Sink) {
        match self.spec.g.kind.as_str() {
            "stall" => {
                sink.event(Ev::Say("partial answer".into()));
                std::thread::sleep(std::time::Duration::from_secs(3600));
            }
            "crash_half" => {
                sink.event(Ev::Say("half an ans".into()));
                // killed by a signal that leaves no core (as an out-of-memory kill): `abort` made the system keep a core and notify each time
                let _ = std::process::Command::new("kill").env_clear().env("PATH", std::env::var_os("PATH").unwrap_or_default()).args(["-KILL", &std::process::id().to_string()]).status();
                std::process::exit(137);
            }
            "bad_utf8" => sink.event(Ev::Say("ok \u{E000}\u{E000} done".into())),
            "giant_line" => {
                sink.event(Ev::Say("x".repeat(1 << 20)));
                sink.event(Ev::Say("done".into()));
            }
            "stderr_mix" => {
                for i in 0..3 {
                    eprintln!("debug: step {i} noise");
                }
                sink.event(Ev::Say("done".into()));
            }
            _ => sink.event(Ev::Say("done".into())),
        }
    }

    /// The limit said after a successful turn; the status.
    fn announce(&self, q: &Quota, at: f64, sink: &mut Sink) -> i32 {
        let (used, left, reset) = (q.cap - q.left(), q.left(), q.reset_s(at));
        let status = format!("[{} {}/{} · resets in {}]", q.unit, round(used), q.cap, retry_in(reset));
        match self.spec.e.get("where") {
            "footer" => sink.event(Ev::Usage { text: status, data: q.data(at) }),
            "trailer" => sink.event(Ev::Usage { text: q.data(at).to_string(), data: q.data(at) }),
            "stderr" if q.kind == "softhard" => {
                if used / q.cap >= 0.8 {
                    eprintln!("warning: {}% of your quota is used", (used / q.cap * 100.0).round());
                }
            }
            "stderr" => eprintln!("--- quota {}/{} {}, window resets in {} ---", round(used), q.cap, q.unit, retry_in(reset)),
            "file" => {
                let body = json!({ "remaining": round(left), "limit": q.cap, "unit": q.unit, "resets_at": (at + reset).round() });
                let _ = std::fs::write(self.state_dir().join("quota.json"), body.to_string());
            }
            _ => {}
        }
        0
    }

    fn refuse(&self, q: &Quota, at: f64, sink: &mut Sink) -> Outcome {
        let (used, reset) = (q.cap - q.left(), q.reset_s(at));
        let code = match self.spec.e.get("where") {
            "stderr" if q.kind == "softhard" => {
                println!("hard limit reached: no more requests until the window resets");
                1
            }
            "stderr" => {
                eprintln!("=== QUOTA EXCEEDED {}/{} {} - window resets in {} ===", round(used), q.cap, q.unit, retry_in(reset));
                1
            }
            "trailer" => {
                let mut data = q.data(at);
                data["exceeded"] = json!(true);
                sink.event(Ev::Usage { text: data.to_string(), data });
                sink.end();
                1
            }
            "exit" => {
                eprintln!("rate limited: retry in {}", retry_in(reset));
                self.spec.e.num("code") as i32
            }
            "http" => 1,
            "footer" => {
                println!("limit reached: {} {}/{}; resets in {}", q.unit, round(used), q.cap, retry_in(reset));
                1
            }
            _ => {
                println!("error: quota exhausted");
                1
            }
        };
        if self.spec.e.get("where") == "file" {
            let _ = std::fs::write(self.state_dir().join("quota.json"), json!({ "remaining": 0, "limit": q.cap, "unit": q.unit, "resets_at": (at + reset).round() }).to_string());
        }
        Outcome { code, limited: Some(reset) }
    }
}

// ---- how it describes itself (axis F) ----

fn output_note(s: &Spec) -> &'static str {
    match s.b.kind.as_str() {
        "plain" => "plain text lines",
        "ansi" => "text in colour, with a spinner that is redrawn in place",
        "json" => "one JSON object when the turn is over",
        "jsonl" => "one JSON object per line",
        "sse" => "server-sent events: `event:` and `data:` lines",
        "frames" => "frames, each a six-digit length and then that many bytes of JSON",
        "xml" => "XML-like tags, one per line",
        "logfmt" => "logfmt lines (key=value)",
        "jsonrpc" => "JSON-RPC 2.0 notifications, one per line, and a result at the end",
        _ => "a JSON object in the body of each HTTP reply",
    }
}

fn sample(s: &Spec) -> String {
    let buf = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    struct Shared(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
    impl Write for Shared {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().map_or(Ok(b.len()), |mut v| v.write(b))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut sink = Sink::new(&s.b.kind, &s.names, s.b.num("depth") as usize, Box::new(Shared(buf.clone())));
    if s.c.kind == "flag" || s.c.kind == "expire" {
        sink.event(Ev::Session("s1f2e".into()));
    }
    sink.event(Ev::Say("hello".into()));
    sink.end();
    let bytes = buf.lock().map(|b| b.clone()).unwrap_or_default();
    String::from_utf8_lossy(&bytes).replace(['\r', '\u{1b}'], "")
}

pub fn help_text(s: &Spec) -> String {
    let name = &s.name;
    let usage = match s.a.kind.as_str() {
        "positional" => format!("{name} [options] PROMPT"),
        "flag" => format!("{name} {} TEXT", s.a.get("flag")),
        "stdin" => format!("{name} [options]      (the prompt is read from standard input)"),
        "file" => format!("{name} {} PATH   (the prompt is read from the file at PATH)", s.a.get("flag")),
        "subcommand" if s.b.kind == "http" => format!("{name} serve      (a local HTTP server: POST /ask with {{\"prompt\": TEXT}}; it prints its port)"),
        "subcommand" => format!("{name} {} PROMPT", s.a.get("sub")),
        "env" => format!("{name}      (the prompt is read from the environment variable {})", s.a.get("env")),
        _ => format!("{name}      (an interactive loop: it prints {} and reads a line; end of input quits)", s.a.get("marker")),
    };
    let session = match s.c.kind.as_str() {
        "flag" => format!("  {} ID    carry on in the session ID, which the first answer names\n", s.c.get("flag")),
        "expire" => format!("  {} ID    carry on in the session ID; a session ends after {} turns\n", s.c.get("flag"), s.c.get("k")),
        "file" => format!("  --new          start afresh: the session is kept in ./.{name}-session.json\n"),
        "server" => "  --reset        start afresh: a local server keeps the session\n".into(),
        _ => String::new(),
    };
    let tools = match s.d.kind.as_str() {
        "none" => String::new(),
        "events" => "Tools: ask it `write TEXT to FILE` or `read FILE`; it reports each tool use in its output.\n".into(),
        "ask_stdin" => "Tools: `write TEXT to FILE` and `read FILE`; before a write it asks `[y/n]` on standard input.\n".into(),
        "ask_json" => "Tools: `write TEXT to FILE` and `read FILE`; before a write it prints a JSON permission request and reads a JSON reply on standard input.\n".into(),
        _ => "Tools: `write TEXT to FILE` and `read FILE`.\n".into(),
    };
    let usage_cmd = if s.e.kind == "usagecmd" { "  usage          show how much of your quota is used\n" } else { "" };
    let lying = if s.f.kind == "lying" { "  --json-output  print the answer as JSON\n" } else { "" };
    format!(
        "{name} - a small scripted harness\n\nUsage:\n  {usage}\n\nOptions:\n  --version      print the version\n{session}{usage_cmd}{lying}\n{tools}\nOutput: {}.\nExample output:\n{}",
        output_note(s),
        sample(s)
    )
}

fn help(s: &Spec) -> (String, i32) {
    let name = &s.name;
    match s.f.kind.as_str() {
        "none" => ("error: unknown option\n".to_string(), 2),
        "man_only" => (format!("{name}: no built-in help; read the manual page: man {name}\n"), 2),
        "schema" => {
            let args = json!([{ "usage": help_text(s).lines().nth(3).unwrap_or("").trim() }, { "output": output_note(s), "example": sample(s) }]);
            (json!({ "$schema": "https://json-schema.org/draft/2020-12/schema", "title": name, "type": "object", "description": help_text(s), "args": args }).to_string() + "\n", 0)
        }
        _ => (help_text(s), 0),
    }
}

// ---- running ----

pub fn play(spec_path: &Path, args: Vec<String>) -> i32 {
    let Ok(text) = std::fs::read_to_string(spec_path) else {
        eprintln!("j4n-forge: cannot read {}", spec_path.display());
        return 2;
    };
    if text.starts_with("family = \"tool\"") {
        return crate::tool::play(spec_path, args);
    }
    let spec: Spec = match toml::from_str(&text) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("j4n-forge: bad spec: {e}");
            return 2;
        }
    };
    let upgraded = spec.g.kind == "upgrade" && spec_path.with_file_name("upgraded").exists();
    let names = if upgraded { spec.upgraded.clone() } else { spec.names.clone() };
    let h = Harness { spec, dir: spec_path.to_path_buf(), names };
    let _ = &h.dir;
    run(&h, args)
}

fn run(h: &Harness, args: Vec<String>) -> i32 {
    let s = &h.spec;
    let stdin = std::io::stdin();
    let mut stdin = stdin.lock();
    let first = args.first().map(String::as_str);
    if matches!(first, Some("--version" | "-V")) {
        println!("{} 0.{}.{}", s.name, s.seed % 9, s.seed % 7 + 1);
        return 0;
    }
    let helps_by = match s.f.kind.as_str() {
        "help_sub" => matches!(first, Some("help")),
        "full_help" | "lying" | "schema" => matches!(first, Some("--help" | "-h" | "help")),
        _ => matches!(first, Some("--help" | "-h" | "help")),
    };
    if helps_by {
        let (text, code) = help(s);
        print!("{text}");
        return code;
    }
    if matches!(first, Some("--help" | "-h")) {
        eprintln!("error: unknown option '{}'", first.unwrap_or(""));
        return 2;
    }
    if first == Some("usage") && s.e.kind == "usagecmd" {
        let mut q = Quota::load(&s.e, &h.state_dir());
        q.roll(now());
        println!("usage: {} of {} {} used in the current window; resets in {}", round(q.cap - q.left()), q.cap, q.unit, retry_in(q.reset_s(now())));
        return 0;
    }
    if args.iter().any(|a| a == "--json-output") && s.f.kind == "lying" {
        eprintln!("error: unknown option '--json-output'");
        return 2;
    }
    // the session flag, `--new`, `--reset`, and what is left
    let (mut sess, mut fresh, mut rest) = (None, false, Vec::new());
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        if matches!(s.c.kind.as_str(), "flag" | "expire") && a == s.c.get("flag") {
            sess = it.next();
        } else if a == "--new" || a == "--reset" {
            fresh = true;
        } else {
            rest.push(a);
        }
    }
    if s.b.kind == "http" {
        return serve(h, &rest);
    }
    let mut sink = h.sink(Box::new(std::io::stdout()));
    let missing = |why: &str| {
        eprintln!("error: {why}\nTry `{} --help`.", s.name);
        2
    };
    let prompt = match s.a.kind.as_str() {
        "positional" => (!rest.is_empty()).then(|| rest.join(" ")),
        "flag" | "file" => rest.iter().position(|a| a == s.a.get("flag")).and_then(|i| rest.get(i + 1)).cloned(),
        "stdin" => {
            let mut t = String::new();
            let _ = stdin.read_to_string(&mut t);
            Some(t.trim().to_string()).filter(|t| !t.is_empty())
        }
        "subcommand" => {
            let sub: Vec<&str> = s.a.get("sub").split(' ').collect();
            (rest.len() > sub.len() && rest.iter().zip(&sub).all(|(a, b)| a == b)).then(|| rest[sub.len()..].join(" "))
        }
        "env" => std::env::var(s.a.get("env")).ok().filter(|t| !t.is_empty()),
        _ => {
            return repl(h, sess.as_deref(), fresh, &mut sink, &mut stdin);
        }
    };
    let prompt = match (s.a.kind.as_str(), prompt) {
        ("file", Some(path)) => match std::fs::read_to_string(&path) {
            Ok(t) => t.trim().to_string(),
            Err(e) => return missing(&format!("cannot read {path}: {e}")),
        },
        (_, Some(p)) => p,
        (_, None) => return missing("no prompt given"),
    };
    h.turn(&prompt, sess.as_deref(), fresh, &mut sink, &mut stdin).code
}

fn repl(h: &Harness, sess: Option<&str>, fresh: bool, sink: &mut Sink, stdin: &mut dyn BufRead) -> i32 {
    let marker = h.spec.a.get("marker").to_string();
    let mut sess = sess.map(String::from);
    let mut status = 0;
    println!("{} interactive. One line is one prompt; end of input quits.", h.spec.name);
    loop {
        print!("{marker} ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        if !matches!(stdin.read_line(&mut line), Ok(n) if n > 0) {
            println!();
            return status;
        }
        if line.trim().is_empty() {
            continue;
        }
        let out = h.turn(line.trim(), sess.as_deref(), fresh && sess.is_none(), sink, stdin);
        status = out.code;
        if out.code == 3 {
            return 3;
        }
        if sess.is_none() && matches!(h.spec.c.kind.as_str(), "flag" | "expire") {
            // the id it made is the newest session file
            sess = newest(&h.state_dir().join("sessions"));
        }
    }
}

fn newest(dir: &Path) -> Option<String> {
    let mut found: Vec<_> = std::fs::read_dir(dir).ok()?.flatten().filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path()))).collect();
    found.sort();
    found.last().and_then(|(_, p)| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
}

/// Axis B `http`: a local server whose port it prints; each POST is one turn, answered as a JSON object (and 429 with `Retry-After` when the limit says so).
fn serve(h: &Harness, rest: &[String]) -> i32 {
    use std::net::TcpListener;
    if rest.first().map(String::as_str) != Some("serve") {
        eprintln!("error: no command given\nTry `{} --help`.", h.spec.name);
        return 2;
    }
    let port = h.spec.b.num("port") as u16;
    let listener = TcpListener::bind(("127.0.0.1", port)).or_else(|_| TcpListener::bind(("127.0.0.1", 0)));
    let Ok(listener) = listener else {
        eprintln!("error: cannot listen");
        return 1;
    };
    let _ = listener.set_nonblocking(true);
    println!("listening on http://127.0.0.1:{}", listener.local_addr().map_or(port, |a| a.port()));
    let _ = std::io::stdout().flush();
    let mut idle = std::time::Instant::now();
    while idle.elapsed().as_secs() < 30 {
        let Ok((mut conn, _)) = listener.accept() else {
            std::thread::sleep(std::time::Duration::from_millis(40));
            continue;
        };
        idle = std::time::Instant::now();
        let _ = conn.set_nonblocking(false);
        let _ = conn.set_read_timeout(Some(std::time::Duration::from_secs(5)));
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        let body = loop {
            let n = conn.read(&mut chunk).unwrap_or(0);
            buf.extend_from_slice(&chunk[..n]);
            let text = String::from_utf8_lossy(&buf).to_string();
            if let Some(i) = text.find("\r\n\r\n") {
                let len = text[..i].lines().find_map(|l| l.to_lowercase().strip_prefix("content-length:").and_then(|v| v.trim().parse::<usize>().ok())).unwrap_or(0);
                if buf.len() >= i + 4 + len || n == 0 {
                    break text[i + 4..].to_string();
                }
            } else if n == 0 {
                break String::new();
            }
        };
        let req: Json = serde_json::from_str(&body).unwrap_or(Json::Null);
        let prompt = req.get("prompt").and_then(Json::as_str).unwrap_or(body.trim()).to_string();
        let sess = req.get("session").and_then(Json::as_str).map(String::from);
        let out = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        struct Shared(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
        impl Write for Shared {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                self.0.lock().map_or(Ok(b.len()), |mut v| v.write(b))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut sink = h.sink(Box::new(Shared(out.clone())));
        let o = h.turn(&prompt, sess.as_deref(), false, &mut sink, &mut std::io::empty());
        let bytes = out.lock().map(|b| b.clone()).unwrap_or_default();
        let (status, extra) = match (o.code, o.limited) {
            (_, Some(wait)) => ("429 Too Many Requests", format!("Retry-After: {}\r\n", wait.ceil() as u64)),
            (3, _) => ("401 Unauthorized", String::new()),
            (0, _) => ("200 OK", String::new()),
            _ => ("500 Internal Server Error", String::new()),
        };
        let _ = write!(conn, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n", bytes.len());
        let _ = conn.write_all(&bytes);
    }
    0
}
