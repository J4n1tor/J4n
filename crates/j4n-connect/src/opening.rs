//! A tool call that is still being written: what the arguments say of it so far, read from a JSON
//! text that is cut short. The dialects share it: pi's `toolcall_delta`, the models' `tool_calls` and `input_json_delta`.

/// The keys that say what a call is about, in the order they are preferred.
const KEYS: [&str; 8] = ["file_path", "path", "file", "notebook_path", "pattern", "command", "url", "query"];

/// What the arguments written so far are about: the first of [`KEYS`] whose string has been closed. Empty while none has.
pub fn target_in(partial: &str) -> String {
    for k in KEYS {
        let mut from = 0;
        while let Some(at) = partial[from..].find(&format!("\"{k}\"")) {
            let rest = partial[from + at + k.len() + 2..].trim_start();
            from += at + k.len() + 2;
            let Some(rest) = rest.strip_prefix(':').map(str::trim_start).and_then(|r| r.strip_prefix('"')) else { continue };
            let mut escaped = false;
            for (i, c) in rest.char_indices() {
                match c {
                    _ if escaped => escaped = false,
                    '\\' => escaped = true,
                    '"' => return serde_json::from_str::<String>(&format!("\"{}\"", &rest[..i])).unwrap_or_default(),
                    _ => {}
                }
            }
        }
    }
    String::new()
}

/// The top-level keys of the arguments written so far (a string that is still open is not a key yet), in the order they came.
pub fn keys_in(partial: &str) -> Vec<String> {
    let (mut keys, mut depth, mut inside, mut escaped, mut word) = (Vec::new(), 0u32, false, false, String::new());
    let mut closed = None;
    for c in partial.chars() {
        if inside {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => (inside, closed) = (false, (depth == 1).then(|| std::mem::take(&mut word))),
                _ if depth == 1 => word.push(c),
                _ => {}
            }
            continue;
        }
        match c {
            '"' => (inside, word) = (true, String::new()),
            ':' => keys.extend(closed.take()),
            '{' | '[' => depth += 1,
            '}' | ']' => depth = depth.saturating_sub(1),
            c if !c.is_whitespace() => closed = None,
            _ => {}
        }
    }
    keys
}

/// What tool the arguments written so far are of, from the keys alone: the claude CLI streams a call's arguments with no name (the name comes
/// with the settled call), so the shape of what is written says it. The names are the ones the settled call has. Empty while no key says (a
/// `file_path` alone is a read as well as a write).
pub fn name_in(partial: &str) -> &'static str {
    let keys = keys_in(partial);
    let has = |k: &str| keys.iter().any(|x| x == k);
    match () {
        _ if has("old_string") => "Edit",
        _ if has("edits") => "MultiEdit",
        _ if has("new_source") => "NotebookEdit",
        _ if has("content") && (has("file_path") || has("path")) => "Write",
        _ if has("command") => "Bash",
        _ if has("pattern") => "Grep",
        _ if has("url") => "WebFetch",
        _ if has("query") => "WebSearch",
        _ if has("todos") => "TodoWrite",
        _ if has("subagent_type") => "Agent",
        _ => "",
    }
}
