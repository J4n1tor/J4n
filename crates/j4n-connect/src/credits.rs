//! What a gateway says is left of a key's credits: OpenRouter's `GET /key`. One call, made only when `/usage` or an
//! offload asks and at most once in ten minutes (the front keeps the answer, `j4n_core::front::usage::Credits`). The key goes in a header and
//! is never in anything this says: an error is scrubbed of it.

use crate::net::Net;
use j4n_core::front::usage::Left;
use serde_json::Value;

/// The credits a `/key` reply names. `data.limit_remaining` is what is left of a limit the key has; a key with none has `remaining: None`, and
/// what it has used (`data.usage`) is still said.
pub fn parse(body: &str) -> Result<Left, String> {
    let v: Value = serde_json::from_str(body).map_err(|_| "the gateway's answer is not JSON".to_string())?;
    let d = v.get("data").ok_or("the gateway's answer has no `data`")?;
    let num = |k: &str| d.get(k).and_then(Value::as_f64);
    Ok(Left { remaining: num("limit_remaining"), used: num("usage").ok_or("the gateway's answer says nothing of what was used")?, limit: num("limit") })
}

/// Ask the gateway at `base` (`https://openrouter.ai/api/v1`) what `key` has left.
pub fn ask(net: &Net, base: &str, key: &str) -> Result<Left, String> {
    let scrub = |s: String| s.replace(key, "…");
    let reply = net.get(&format!("{}/key", base.trim_end_matches('/')), &[("Authorization", &format!("Bearer {key}"))]).map_err(|e| scrub(e.to_string()))?;
    if !reply.ok() {
        let status = reply.status;
        return Err(format!("the gateway answered {status}{}", if status == 401 { " (the key is not accepted)" } else { "" }));
    }
    parse(&reply.text().map_err(|e| scrub(e.to_string()))?)
}
