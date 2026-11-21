//! The arguments of a connector, from the words a manifest wrote: `{prompt}`, `{model}`, `{session}`, `{cwd}` and the names of
//! a tool's `args` stand for what the run brings. An argument whose value is not there is left out together with the flag
//! right before it (`--model {model}` with no model is nothing), so one `start` serves a run with a model and one without.
//! A value never becomes a flag: one that would stand alone as an argument and starts with a dash is refused, except the prompt,
//! which is given a space in front (a prompt of "- fix it" is still a prompt).

use std::collections::BTreeMap;

/// Values by name.
pub type Vars = BTreeMap<String, String>;

/// The names written as `{name}` in `text`, in order.
pub fn names(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) if is_name(&after[..close]) => {
                out.push(&after[..close]);
                rest = &after[close + 1..];
            }
            _ => rest = after,
        }
    }
    out
}

fn is_name(s: &str) -> bool {
    !s.is_empty() && s.len() <= 32 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `arg` with every `{name}` replaced; None when one of them has no value.
fn fill(arg: &str, vars: &Vars) -> Option<String> {
    let mut out = String::new();
    let mut rest = arg;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}').filter(|c| is_name(&after[..*c])) {
            Some(close) => {
                out.push_str(vars.get(&after[..close])?);
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    Some(out)
}

/// The arguments for a run. An error names the value that would have been read as a flag.
pub fn render(template: &[String], vars: &Vars) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    // whether the last argument kept was a flag that has not met its value yet
    let mut flag_at: Option<usize> = None;
    for arg in template {
        let wants = names(arg);
        match fill(arg, vars) {
            Some(mut text) => {
                if wants.len() == 1 && arg == &format!("{{{}}}", wants[0]) && text.starts_with('-') {
                    if wants[0] == "prompt" {
                        text.insert(0, ' ');
                    } else {
                        return Err(format!("`{}` would be read as a flag: put `--` before `{{{}}}` in the manifest, or change the value", text, wants[0]));
                    }
                }
                if wants.is_empty() && arg.starts_with('-') {
                    flag_at = Some(out.len());
                } else {
                    flag_at = None;
                }
                out.push(text);
            }
            None => {
                if let Some(at) = flag_at.take() {
                    out.truncate(at);
                }
            }
        }
    }
    Ok(out)
}

/// `text` with every `{name}` replaced by what `put` makes of its value (an escape for the place it is going to); a name with
/// no value is an error that names it.
pub fn fill_with(text: &str, vars: &Vars, put: &dyn Fn(&str) -> String) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}').filter(|c| is_name(&after[..*c])) {
            Some(close) => {
                let name = &after[..close];
                out.push_str(&put(vars.get(name).ok_or_else(|| format!("`{{{name}}}` has no value"))?));
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    Ok(out)
}

/// A value as one part of a URL: everything but the unreserved characters is `%`-escaped.
pub fn url_part(v: &str) -> String {
    v.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// A value as the inside of a JSON string.
pub fn json_part(v: &str) -> String {
    let quoted = serde_json::Value::String(v.to_string()).to_string();
    quoted[1..quoted.len() - 1].to_string()
}
