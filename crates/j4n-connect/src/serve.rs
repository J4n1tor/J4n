//! A program that is a local server (`serve` of a `jsonl-map` connector): it is started, its address is read from a line it
//! prints, one request is posted to it and its reply is the program's output; the server is taken down with the run. Only this machine's
//! own address is spoken to, and nothing but one POST is made, through the one builder of clients (`net.rs`).

use crate::proc::{self, Child};
use j4n_core::place::Launch;
use crate::net::Net;
use std::sync::mpsc;
use std::time::Duration;

/// A reply: the status, the seconds the server asked to wait (`Retry-After`), and the body.
pub struct Reply {
    pub status: u16,
    pub retry: Option<u64>,
    pub body: String,
}

/// A running server; it is taken down when this is dropped.
pub struct Server {
    child: Child,
    host: String,
    port: u16,
}

/// The address a line of the server's output names: `http://127.0.0.1:2800`.
fn address_in(line: &str) -> Option<(String, u16)> {
    let rest = &line[line.find("http://")? + 7..];
    let end = rest.find(|c: char| c.is_whitespace() || c == '/' || c == '"' || c == '\'').unwrap_or(rest.len());
    let (host, port) = rest[..end].rsplit_once(':')?;
    Some((host.to_string(), port.parse().ok()?))
}

/// Start the server and wait, up to `secs`, for the line that holds `ready` and an address.
pub fn start(launch: &Launch, ready: &str, secs: u64) -> Result<Server, String> {
    let (tx, rx) = mpsc::channel();
    let want = ready.to_lowercase();
    let (child, _) = proc::spawn(
        launch,
        false,
        move |l| {
            if l.to_lowercase().contains(&want)
                && let Some(a) = address_in(l)
            {
                let _ = tx.send(a);
            }
        },
        |_| {},
    )
    .map_err(|e| format!("could not start it: {e}"))?;
    match rx.recv_timeout(Duration::from_secs(secs)) {
        Ok((host, port)) if ["127.0.0.1", "localhost", "[::1]"].contains(&host.as_str()) => Ok(Server { child, host, port }),
        Ok((host, _)) => {
            child.kill();
            Err(format!("it listens at `{host}`, which is not this machine: J4n speaks only to a server on this machine"))
        }
        Err(_) => {
            child.kill();
            Err(format!("it did not say where it listens within {secs} s"))
        }
    }
}

impl Server {
    /// One POST of a JSON body to `path`, through the one builder (loopback goes direct, whatever proxy is set).
    pub fn post(&self, path: &str, body: &str, secs: u64) -> Result<Reply, String> {
        let json: serde_json::Value = serde_json::from_str(body).map_err(|e| format!("the request is not JSON: {e}"))?;
        let url = format!("http://{}:{}{}", self.host, self.port, if path.starts_with('/') { path.to_string() } else { format!("/{path}") });
        let got = Net::new().idle(Duration::from_secs(secs)).post_json(&url, &[], &json).map_err(|e| format!("it does not answer: {e}"))?;
        let (status, retry) = (got.status, got.headers.get("retry-after").and_then(|v| v.trim().parse().ok()));
        Ok(Reply { status, retry, body: got.text().map_err(|e| e.to_string())? })
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.child.kill();
    }
}
