//! What a tool of an MCP server can do, for the Gate: from what the server says of it (`annotations.readOnlyHint`,
//! `destructiveHint`), else from the words of its name. A tool nobody can place is a write. The class decides the verb the Gate is
//! asked about (a read is a read; a write or a run is a write, which asks and which a read-only step refuses).

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Read,
    Write,
    /// Runs something, or destroys something.
    Exec,
}

impl Class {
    pub fn word(self) -> &'static str {
        match self {
            Class::Read => "read",
            Class::Write => "write",
            Class::Exec => "exec",
        }
    }

    /// How much the operator is warned when a call is asked about.
    pub fn risk(self) -> u8 {
        match self {
            Class::Read => 10,
            Class::Write => 40,
            Class::Exec => 70,
        }
    }
}

const READS: [&str; 14] = ["get", "list", "read", "search", "find", "fetch", "query", "describe", "show", "view", "lookup", "check", "status", "stat"];
const EXECS: [&str; 12] = ["exec", "run", "shell", "eval", "command", "spawn", "kill", "delete", "remove", "drop", "destroy", "reset"];

/// The words of a name: `list_files`, `listFiles`, `list-files` are `list`, `files`.
fn words(name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut last_lower = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            if c.is_ascii_uppercase() && last_lower && !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            last_lower = c.is_ascii_lowercase();
            cur.push(c.to_ascii_lowercase());
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
            last_lower = false;
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

pub fn classify(name: &str, annotations: &Value) -> Class {
    let flag = |k: &str| annotations.get(k).and_then(Value::as_bool);
    if flag("destructiveHint") == Some(true) {
        return Class::Exec;
    }
    let w = words(name);
    if w.iter().any(|x| EXECS.contains(&x.as_str())) {
        return Class::Exec;
    }
    if flag("readOnlyHint") == Some(true) {
        return Class::Read;
    }
    // the first word is what the tool does (`get_file`), and a server's claim of read-only is taken, but a name is not forgiven
    if w.first().is_some_and(|x| READS.contains(&x.as_str())) { Class::Read } else { Class::Write }
}
