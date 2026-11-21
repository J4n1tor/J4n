//! `j4n answer`: one question to a point's answerers, no task. A line is the shape of a probe case
//! (`id`, `point`, `slices`, `offered`); it is put as a headless run puts it ([`Who::ask`]), and what came of the walk is one JSON
//! line. The line `{id, gate: {cmd}}` asks the Gate's matcher what a command is. A line that cannot be taken is `{id, error}`;
//! the second of each pair is false when the text was no JSON at all.

use super::who::Who;
use crate::answer::chain::Walk;
use crate::answer::probe::ProbeRow;
use crate::answer::{ShapeKind, described};
use crate::gate::danger::danger;
use serde_json::{Map, Value as Json, json};

type Obj = Map<String, Json>;

fn fail(id: &Json, why: &str) -> Json {
    json!({ "id": id, "error": why })
}

/// The line as an object, else the error line to print.
fn object(text: &str) -> Result<(Obj, Json), (Json, bool)> {
    match serde_json::from_str::<Json>(text) {
        Ok(Json::Object(o)) => {
            let id = o.get("id").filter(|v| v.is_string() || v.is_number()).cloned().unwrap_or(Json::Null);
            Ok((o, id))
        }
        Ok(_) => Err((fail(&Json::Null, "a line is a JSON object"), true)),
        Err(e) => Err((fail(&Json::Null, &format!("not JSON: {e}")), false)),
    }
}

/// The matcher's word on `{id, gate: {cmd}}`, as a line of JSON.
pub fn gate(text: &str) -> (String, bool) {
    let (json, ok) = gate_of(text);
    (json.to_string(), ok)
}

fn gate_of(text: &str) -> (Json, bool) {
    let (o, id) = match object(text) {
        Ok(x) => x,
        Err(e) => return e,
    };
    let Some(cmd) = o.get("gate").and_then(|g| g.get("cmd")).and_then(Json::as_str) else { return (fail(&id, "`gate.cmd` is a string"), true) };
    let d = danger(cmd);
    let detail = json!({ "program": d.program, "writes": d.writes, "compound": d.compound, "broad": d.broad, "classes": d.classes });
    (json!({ "id": id, "class": d.class, "detail": detail }), true)
}

/// The answer to a probe-shaped line, from the chain of its point (`by`: from that answerer of the chain only, with no fallback),
/// as a line of JSON.
pub fn answer(who: &Who, by: Option<&str>, text: &str) -> (String, bool) {
    let (json, ok) = answer_of(who, by, text);
    (json.to_string(), ok)
}

fn answer_of(who: &Who, by: Option<&str>, text: &str) -> (Json, bool) {
    let (o, id) = match object(text) {
        Ok(x) => x,
        Err(e) => return e,
    };
    (put(who, by, &o, &id).unwrap_or_else(|why| fail(&id, &why)), true)
}

fn put(who: &Who, by: Option<&str>, o: &Obj, id: &Json) -> Result<Json, String> {
    let point = o.get("point").and_then(Json::as_str).ok_or("`point` is a string")?;
    let ap = who.point(point).ok_or_else(|| format!("there is no ask point `{point}`"))?;
    if !matches!(ap.shape, ShapeKind::YesNo | ShapeKind::Choice) {
        return Err(format!("`{point}` takes a {}, which is not asked this way", ap.shape.word()));
    }
    let offered: Vec<String> = match o.get("offered") {
        None => Vec::new(),
        Some(Json::Array(a)) => a.iter().map(|v| v.as_str().map(String::from).ok_or("`offered` holds strings")).collect::<Result<_, _>>()?,
        Some(_) => return Err("`offered` is a list of strings".into()),
    };
    if offered.is_empty() != (ap.shape == ShapeKind::YesNo) {
        return Err(if offered.is_empty() { format!("`{point}` is a choice: `offered` lists the options") } else { format!("`{point}` is a yes/no: `offered` is empty") });
    }
    if let Some(by) = by.filter(|by| !ap.chain.iter().any(|s| s.id() == *by)) {
        return Err(format!("`{by}` is not in the chain of `{point}`"));
    }
    let slices = match o.get("slices") {
        None => Default::default(),
        Some(Json::Object(m)) => m.iter().map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string())).ok_or_else(|| format!("`slices.{k}` is a string"))).collect::<Result<_, _>>()?,
        Some(_) => return Err("`slices` is an object of strings".into()),
    };
    let name = id.as_str().map_or_else(|| id.to_string(), String::from);
    let row = ProbeRow { id: name.clone(), doc: None, point: point.into(), slices, offered, expect: String::new(), pair: String::new() };
    let mut q = row.question();
    (q.ask, q.asker) = (format!("answer:{name}"), "answer".into());
    q.shape = described(q.shape, who.says(), point);
    let asked = who.ask(point, None, &q, by).ok_or("the point went away")?;
    // the point's fallback stands for the chain, not for one answerer of it
    let standing = match asked.walk {
        Walk::Answered(out) => Some(out),
        Walk::Fallback(out) if by.is_none() => Some(out),
        _ => None,
    };
    let steps: Vec<Json> = asked.notes.into_iter().filter(|n| n.kind == "ask.step").map(|mut n| {
        n.data.as_object_mut().map(|d| d.remove("ask"));
        n.data
    }).collect();
    let round = |c: f32| (f64::from(c) * 1000.0).round() / 1000.0;
    // a rule, a rank and a fallback cost nothing; what a decider or a model cost is what it reported, else `null`: unknown
    let usd = asked.spent.usd;
    Ok(json!({
        "id": id,
        "answer": standing.as_ref().map(|out| out.answer.render()),
        "by": standing.as_ref().map(|out| &out.by),
        "confidence": standing.as_ref().and_then(|out| out.conf).map(round),
        "ms": asked.ms,
        "usd": usd,
        "tokens_in": asked.spent.tokens_in,
        "tokens_out": asked.spent.tokens_out,
        "why": standing.as_ref().map_or("it gave no answer", |out| out.why.as_str()),
        "waited_ms": asked.waited,
        "steps": steps,
    }))
}
