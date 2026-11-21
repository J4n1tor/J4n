//! The arguments of a call against the schema the server gave for the tool, before they are sent: the part of JSON Schema that
//! tool schemas use — `type`, `required`, `properties`, `enum`, `items`, `minimum`/`maximum`, `minLength`/`maxLength`,
//! `additionalProperties: false`. What it does not know it lets through; the server is the last word. What is refused is told in
//! words that name the argument, so the model can mend the call.

use serde_json::Value;

const DEPTH: usize = 12;

fn kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(n) if n.is_i64() || n.is_u64() => "integer",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn has_type(v: &Value, want: &str) -> bool {
    kind(v) == want || (want == "number" && kind(v) == "integer") || (want == "integer" && v.as_f64().is_some_and(|f| f.fract() == 0.0 && v.is_number()))
}

/// The first thing wrong with `value` against `schema`, at `at`.
fn check(value: &Value, schema: &Value, at: &str, depth: usize, out: &mut Vec<String>) {
    if depth > DEPTH {
        return;
    }
    if let Some(t) = schema.get("type") {
        let wants: Vec<&str> = match t {
            Value::String(s) => vec![s.as_str()],
            Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        };
        if !wants.is_empty() && !wants.iter().any(|w| has_type(value, w)) {
            out.push(format!("{at} is {}, and should be {}", kind(value), wants.join(" or ")));
            return;
        }
    }
    if let Some(options) = schema.get("enum").and_then(Value::as_array)
        && !options.contains(value)
    {
        out.push(format!("{at} is {value}, and should be one of {}", options.iter().map(Value::to_string).collect::<Vec<_>>().join(", ")));
    }
    match value {
        Value::Number(n) => {
            let f = n.as_f64().unwrap_or_default();
            if schema.get("minimum").and_then(Value::as_f64).is_some_and(|m| f < m) {
                out.push(format!("{at} is {value}, below the least, {}", schema["minimum"]));
            }
            if schema.get("maximum").and_then(Value::as_f64).is_some_and(|m| f > m) {
                out.push(format!("{at} is {value}, above the most, {}", schema["maximum"]));
            }
        }
        Value::String(s) => {
            let n = s.chars().count() as u64;
            if schema.get("minLength").and_then(Value::as_u64).is_some_and(|m| n < m) {
                out.push(format!("{at} is shorter than {}", schema["minLength"]));
            }
            if schema.get("maxLength").and_then(Value::as_u64).is_some_and(|m| n > m) {
                out.push(format!("{at} is longer than {}", schema["maxLength"]));
            }
        }
        Value::Array(items) => {
            if let Some(item) = schema.get("items").filter(|i| i.is_object()) {
                for (i, v) in items.iter().enumerate() {
                    check(v, item, &format!("{at}[{i}]"), depth + 1, out);
                }
            }
        }
        Value::Object(map) => {
            for name in schema.get("required").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
                if !map.contains_key(name) {
                    out.push(format!("{at}.{name} is needed"));
                }
            }
            let props = schema.get("properties").and_then(Value::as_object);
            for (k, v) in map {
                match props.and_then(|p| p.get(k)) {
                    Some(s) => check(v, s, &format!("{at}.{k}"), depth + 1, out),
                    None if schema.get("additionalProperties") == Some(&Value::Bool(false)) => out.push(format!("{at}.{k} is not an argument of this tool")),
                    None => {}
                }
            }
        }
        _ => {}
    }
}

/// What is wrong with `args`, one thing a line; empty when the call can go.
pub fn problems(args: &Value, schema: &Value) -> Vec<String> {
    let mut out = Vec::new();
    check(args, schema, "arguments", 0, &mut out);
    out
}
