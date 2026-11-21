//! What a program's lines are before they are JSON: the option `decode` of a `jsonl-map` connector. Each word is one
//! small step on a raw line, in order, and the last step leaves a JSON document for the map: `ansi` takes the colour codes, the
//! redraws (`\r`) and the spinners out, `sse` keeps the `data:` of a server-sent event and drops its other lines, `frames` takes
//! the length off a length-prefixed frame, `logfmt` turns `k=v k2="a b"` into an object, `colon` turns `key: value` into
//! `{"key": "value"}` and every line into `{"line": …, "head": its first word, "tail": the rest}`, and `plain` only the last. A connector with no `colon`, `plain` or `logfmt`
//! reads what is left as JSON.

use serde_json::{Value as Json, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Ansi,
    Sse,
    Frames,
    Logfmt,
    Colon,
    Plain,
}

impl Step {
    pub const WORDS: [&'static str; 6] = ["ansi", "sse", "frames", "logfmt", "colon", "plain"];

    pub fn of(word: &str) -> Option<Step> {
        Some(match word {
            "ansi" => Step::Ansi,
            "sse" => Step::Sse,
            "frames" => Step::Frames,
            "logfmt" => Step::Logfmt,
            "colon" => Step::Colon,
            "plain" => Step::Plain,
            _ => return None,
        })
    }
}

/// The steps of a connector's `decode`, or the word that is not one.
pub fn steps(words: &[String]) -> Result<Vec<Step>, String> {
    words.iter().map(|w| Step::of(w).ok_or_else(|| format!("`decode` holds `{w}`, and the steps are {}", Step::WORDS.join(", ")))).collect()
}

/// A line without its colour codes and redraws: after the last `\r` of a line only what is drawn last stays, and an escape
/// sequence (`ESC [ … letter`, `ESC ] … BEL`) is gone.
pub fn clean(line: &str) -> String {
    let shown = line.trim_end_matches('\r').rsplit('\r').next().unwrap_or_default();
    let mut out = String::with_capacity(shown.len());
    let mut chars = shown.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('[') => {
                for n in chars.by_ref() {
                    if n.is_ascii_alphabetic() || n == '~' {
                        break;
                    }
                }
            }
            Some(']') => {
                for n in chars.by_ref() {
                    if n == '\u{7}' {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// The pairs of a `logfmt` line, in order; a value in double quotes may hold spaces and `\"`.
pub fn pairs(line: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = line.trim();
    while !rest.is_empty() {
        let Some(eq) = rest.find('=') else { break };
        let key = rest[..eq].trim();
        if key.is_empty() || key.contains(char::is_whitespace) {
            // a word with no value: skip it
            rest = rest.split_once(char::is_whitespace).map_or("", |(_, r)| r.trim_start());
            continue;
        }
        let after = &rest[eq + 1..];
        let (value, tail) = if let Some(q) = after.strip_prefix('"') {
            let mut v = String::new();
            let mut esc = false;
            let mut end = q.len();
            for (i, c) in q.char_indices() {
                if esc {
                    v.push(c);
                    esc = false;
                } else if c == '\\' {
                    esc = true;
                } else if c == '"' {
                    end = i + 1;
                    break;
                } else {
                    v.push(c);
                }
            }
            (v, q.get(end..).unwrap_or(""))
        } else {
            let at = after.find(char::is_whitespace).unwrap_or(after.len());
            (after[..at].to_string(), &after[at..])
        };
        out.push((key.to_string(), value));
        rest = tail.trim_start();
    }
    out
}

/// The `key: value` of a line: a key of a word or two that begins with a letter, and a value.
pub fn colon(line: &str) -> Option<(&str, &str)> {
    let (k, v) = line.split_once(": ")?;
    (k.len() <= 24 && k.starts_with(|c: char| c.is_ascii_alphabetic()) && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == ' ')).then_some((k, v))
}

/// The length a frame starts with, when the line is one: six or more digits and then `{` or `[`.
pub fn frame(line: &str) -> Option<&str> {
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    (digits >= 4 && line[digits..].starts_with(['{', '['])).then(|| &line[digits..])
}

/// One raw line through the steps: the document it comes to, or none when a step drops the line (an `event:` line, a blank one).
pub fn line(decode: &[Step], raw: &str) -> Option<Json> {
    let mut text = raw.to_string();
    let mut doc: Option<Json> = None;
    for step in decode {
        match step {
            Step::Ansi => text = clean(&text),
            Step::Sse => {
                let t = text.trim();
                text = t.strip_prefix("data:")?.trim_start().to_string();
                if text == "[DONE]" {
                    return None;
                }
            }
            Step::Frames => {
                if let Some(rest) = frame(text.trim()) {
                    text = rest.to_string();
                }
            }
            Step::Logfmt => doc = Some(Json::Object(pairs(&text).into_iter().map(|(k, v)| (k, Json::String(v))).collect())),
            Step::Colon => {
                let mut obj = serde_json::Map::new();
                if let Some((k, v)) = colon(text.trim()) {
                    obj.insert(k.to_string(), Json::String(v.to_string()));
                }
                let (head, tail) = text.trim().split_once(char::is_whitespace).unwrap_or((text.trim(), ""));
                obj.insert("line".into(), Json::String(text.trim().to_string()));
                obj.insert("head".into(), Json::String(head.to_string()));
                obj.insert("tail".into(), Json::String(tail.trim().to_string()));
                doc = Some(Json::Object(obj));
            }
            Step::Plain => doc = Some(json!({ "line": text.trim() })),
        }
    }
    if text.trim().is_empty() {
        return None;
    }
    doc.or_else(|| serde_json::from_str(text.trim()).ok())
}
