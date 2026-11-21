//! The canary: a scripted run of a tool's newest release, in a place of its own, whose **shape** is compared
//! with the shape of a run that is known to be good. The shape is the kinds of the events and the names and types of their fields,
//! never their words, so it costs no model. It never touches the operator's install: the new version goes into a temporary folder
//! that is removed, and the run has nothing of the operator's environment.

use serde_json::Value as Json;
use std::collections::BTreeSet;
use std::io::Read;
use std::path::Path;
use std::time::{Duration, Instant};

/// The most paths a shape keeps, and the most output read from a run.
const PATHS: usize = 2000;
const OUTPUT: usize = 4 << 20;

/// The paths of a stream: `kind.field.sub:type`, an array's elements under `[]`.
pub type Shape = BTreeSet<String>;

fn kind_of(v: &Json) -> String {
    ["type", "event", "kind"].iter().find_map(|k| v.get(k).and_then(Json::as_str)).unwrap_or("-").to_string()
}

fn walk(path: &str, v: &Json, out: &mut Shape) {
    if out.len() >= PATHS {
        return;
    }
    let ty = match v {
        Json::Null => "null",
        Json::Bool(_) => "bool",
        Json::Number(_) => "num",
        Json::String(_) => "str",
        Json::Array(_) => "arr",
        Json::Object(_) => "obj",
    };
    out.insert(format!("{path}:{ty}"));
    match v {
        Json::Object(m) => m.iter().for_each(|(k, x)| walk(&format!("{path}.{k}"), x, out)),
        Json::Array(a) => a.iter().for_each(|x| walk(&format!("{path}[]"), x, out)),
        _ => {}
    }
}

/// Add what one JSON line says to a shape: a line that is not an object adds nothing.
pub fn note(out: &mut Shape, v: &Json) {
    if v.is_object() {
        walk(&kind_of(v), v, out);
    }
}

/// A short word for a text, the same for the same text.
pub fn word_of(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// A short word for a shape, the same for the same paths: what a run's health keeps of its canary.
pub fn digest(shape: &Shape) -> String {
    word_of(&shape.iter().map(String::as_str).collect::<Vec<_>>().join("\n"))
}

/// The shape of a stream of JSON lines (a line that is not JSON is not an event and is left out).
pub fn shape_of(lines: &str) -> Shape {
    let mut out = Shape::new();
    for v in lines.lines().filter_map(|l| serde_json::from_str::<Json>(l).ok()).filter(Json::is_object) {
        walk(&kind_of(&v), &v, &mut out);
    }
    out
}

/// What a run came to against the shape that is known to be good.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The same paths.
    Same,
    /// Every path of the good run is there, and these are new: a field was added, which breaks nothing that read the old ones.
    Grew(Vec<String>),
    /// Paths of the good run are missing or have another type: the tool changed what it says.
    Differs { gone: Vec<String>, new: Vec<String> },
}

pub fn compare(good: &Shape, now: &Shape) -> Verdict {
    let gone: Vec<String> = good.difference(now).cloned().collect();
    let new: Vec<String> = now.difference(good).cloned().collect();
    match (gone.is_empty(), new.is_empty()) {
        (true, true) => Verdict::Same,
        (true, false) => Verdict::Grew(new),
        _ => Verdict::Differs { gone, new },
    }
}

/// `argv` with `{version}` and `{dir}` filled in.
pub fn fill(argv: &[String], version: &str, dir: &Path) -> Vec<String> {
    argv.iter().map(|w| w.replace("{version}", version).replace("{dir}", &dir.display().to_string())).collect()
}

/// Run `argv` in `dir` with nothing of the operator's environment but `PATH`, for at most `wait`; what it said on stdout.
pub fn run(argv: &[String], dir: &Path, wait: Duration) -> Result<String, String> {
    let (program, args) = argv.split_first().ok_or("nothing to run")?;
    let mut cmd = std::process::Command::new(program);
    // every program J4n starts goes through `scrub`, whatever it keeps after (`env_clear` takes the rest)
    crate::proc::scrub(&mut cmd);
    cmd.args(args)
        .current_dir(dir)
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_else(|_| "/usr/local/bin:/usr/bin:/bin".into()))
        .env("HOME", dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let mut child = crate::proc::start(&mut cmd).map_err(|e| format!("{program} did not start: {e}"))?;
    let mut out = child.stdout.take().ok_or("no output")?;
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = (&mut out).take(OUTPUT as u64).read_to_string(&mut text);
        text
    });
    let end = Instant::now() + wait;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if Instant::now() < end => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{program} did not finish in {} s", wait.as_secs()));
            }
        }
    };
    let text = reader.join().unwrap_or_default();
    if status.success() { Ok(text) } else { Err(format!("{program} ended with {status}")) }
}
