//! The one place a network client is built (the charter's one-builder law): every call J4n makes by itself —
//! a model, a decider, a feed — goes out through [`Net`]. It does what `ureq` does not do on its own:
//!
//! - the proxy is the **account's** when the account has one (`Proxy::Url`), else the system's, read from
//!   `https_proxy` / `HTTPS_PROXY` (for `https://`), `http_proxy` / `HTTP_PROXY` (for `http://`) and `ALL_PROXY` / `all_proxy`;
//!   an account's proxy beats the system's, which is not tried then (a proxy behind a proxy is not a thing);
//! - loopback and every host named by `no_proxy` / `NO_PROXY` go direct, so a local mock or a local model never detours;
//! - connecting is bounded on its own, and a read that hears nothing for [`IDLE`] fails, so a dead endpoint is told soon and
//!   a long answer is not cut;
//! - a status of 400 or more is a [`Reply`] like any other, not an error: the caller decides what 429 means.
//!
//! The environment is read through a function that can be replaced ([`Net::with_env`]), so tests need not touch the
//! process's own.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read};
use std::sync::Arc;
use std::time::Duration;

/// How long connecting may take.
pub const CONNECT: Duration = Duration::from_secs(10);
/// How long a read may hear nothing before it fails.
pub const IDLE: Duration = Duration::from_secs(120);
/// The most of a body that is read as text.
pub const BODY_MAX: usize = 8 << 20;

type Env = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// Where a call leaves from.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Proxy {
    /// The system's (`https_proxy` …); none if none is set.
    #[default]
    System,
    /// This one, an account's: `http://host:port`, `https://…`, `socks5://…` (`socks5h`, `socks4`).
    Url(String),
}

impl Proxy {
    /// `system`, or a proxy URL. Anything else is refused by name.
    pub fn parse(word: &str) -> Result<Proxy, String> {
        let word = word.trim();
        if word == "system" || word.is_empty() {
            return Ok(Proxy::System);
        }
        let (scheme, rest) = word.split_once("://").ok_or_else(|| format!("`{word}` is neither `system` nor a proxy URL like http://host:port"))?;
        if !["http", "https", "socks5", "socks5h", "socks4"].contains(&scheme) || rest.is_empty() {
            return Err(format!("`{word}` is not a proxy URL: http://host:port or socks5://host:port"));
        }
        Ok(Proxy::Url(word.into()))
    }

    /// Whether a proxy URL accepts a connection now, within `secs`: the host and port are what is tried, a password is never
    /// said. `Ok` for the system's proxy, which is nobody's to try here.
    pub fn answers(&self, secs: u64) -> Result<(), String> {
        use std::net::{TcpStream, ToSocketAddrs};
        let Proxy::Url(url) = self else { return Ok(()) };
        let rest = url.split_once("://").map_or(url.as_str(), |(_, r)| r);
        let host = rest.rsplit('@').next().unwrap_or(rest).split('/').next().unwrap_or(rest);
        let host = if host.contains(':') { host.to_string() } else { format!("{host}:{}", if url.starts_with("socks") { 1080 } else { 80 }) };
        let addr = host.to_socket_addrs().ok().and_then(|mut a| a.next()).ok_or_else(|| format!("the proxy ({host}) is not an address that can be found"))?;
        TcpStream::connect_timeout(&addr, Duration::from_secs(secs)).map(drop).map_err(|e| format!("the proxy ({host}) does not accept a connection: {e}"))
    }
}

/// A call that did not get an answer (the connection, a timeout, a body that broke). A status is not one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetError(pub String);

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for NetError {}

/// What came back: the status, the headers (names in lower case) and the body, still to be read.
pub struct Reply {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    body: Box<dyn Read + Send + Sync>,
}

impl Reply {
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// The body as text, up to [`BODY_MAX`], a broken character made harmless.
    pub fn text(mut self) -> Result<String, NetError> {
        let mut raw = Vec::new();
        (&mut self.body).take(BODY_MAX as u64).read_to_end(&mut raw).map_err(|e| NetError(format!("the body broke off: {e}")))?;
        Ok(String::from_utf8_lossy(&raw).into_owned())
    }

    /// The body line by line, as it comes.
    pub fn lines(self) -> impl Iterator<Item = Result<String, NetError>> {
        BufReader::new(self.body).lines().map(|l| l.map_err(|e| NetError(format!("the stream broke off: {e}"))))
    }
}

/// The builder.
#[derive(Clone)]
pub struct Net {
    proxy: Proxy,
    env: Env,
    idle: Duration,
    /// How many redirects a call follows by itself; zero hands the 3xx to the caller.
    redirects: u32,
}

impl Default for Net {
    fn default() -> Net {
        Net::new()
    }
}

impl Net {
    /// Behind the system's proxy, read from this process's environment.
    pub fn new() -> Net {
        Net { proxy: Proxy::System, env: Arc::new(|k| std::env::var(k).ok()), idle: IDLE, redirects: 5 }
    }

    /// Behind this proxy (an account's) instead.
    pub fn proxied(mut self, proxy: Proxy) -> Net {
        self.proxy = proxy;
        self
    }

    /// A read that hears nothing for this long fails (a decider is quick; a model that thinks is given [`IDLE`]).
    pub fn idle(mut self, idle: Duration) -> Net {
        self.idle = idle;
        self
    }

    /// Follow no redirect: a 3xx comes back as the reply, for a caller that looks at where it leads.
    pub fn no_redirects(mut self) -> Net {
        self.redirects = 0;
        self
    }

    /// Read the environment through `env` instead of the process's.
    pub fn with_env(mut self, env: impl Fn(&str) -> Option<String> + Send + Sync + 'static) -> Net {
        self.env = Arc::new(env);
        self
    }

    fn var(&self, names: &[&str]) -> Option<String> {
        names.iter().find_map(|n| (self.env)(n).filter(|v| !v.trim().is_empty()))
    }

    /// The proxy a call to `url` goes through, if any.
    pub fn route(&self, url: &str) -> Option<String> {
        if self.bypass(&host_of(url)) {
            return None;
        }
        match &self.proxy {
            Proxy::Url(u) => Some(u.clone()),
            Proxy::System if url.starts_with("https://") => self.var(&["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"]),
            Proxy::System => self.var(&["HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"]),
        }
    }

    /// Loopback and the hosts of `no_proxy` (a suffix after a dot, `*`, or the host itself) are reached directly.
    fn bypass(&self, host: &str) -> bool {
        if matches!(host, "localhost" | "127.0.0.1" | "::1") {
            return true;
        }
        let Some(no) = self.var(&["NO_PROXY", "no_proxy"]) else { return false };
        no.split(',').map(|e| e.trim().trim_start_matches('.').to_ascii_lowercase()).any(|e| e == "*" || (!e.is_empty() && (host == e || host.ends_with(&format!(".{e}")))))
    }

    fn agent(&self, url: &str) -> Result<ureq::Agent, NetError> {
        let mut b = ureq::AgentBuilder::new().timeout_connect(CONNECT).timeout_read(self.idle).user_agent("j4n").redirects(self.redirects);
        if let Some(p) = self.route(url) {
            b = b.proxy(ureq::Proxy::new(&p).map_err(|e| NetError(format!("the proxy `{}` is not usable: {e}", redact(&p))))?);
        }
        Ok(b.build())
    }

    /// `GET url` with these headers.
    pub fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<Reply, NetError> {
        send(self.agent(url)?.get(url), headers, None)
    }

    /// Any method, with a body when there is one (`content_type` says what it is; JSON when left out).
    pub fn request(&self, method: &str, url: &str, headers: &[(&str, &str)], body: Option<Vec<u8>>) -> Result<Reply, NetError> {
        send(self.agent(url)?.request(method, url), headers, body)
    }

    /// `POST url` with a JSON body and these headers.
    pub fn post_json(&self, url: &str, headers: &[(&str, &str)], body: &serde_json::Value) -> Result<Reply, NetError> {
        let bytes = serde_json::to_vec(body).map_err(|e| NetError(format!("the request is not JSON: {e}")))?;
        send(self.agent(url)?.post(url), headers, Some(bytes))
    }
}

fn send(mut req: ureq::Request, headers: &[(&str, &str)], body: Option<Vec<u8>>) -> Result<Reply, NetError> {
    for (k, v) in headers {
        req = req.set(k, v);
    }
    let got = match body {
        Some(b) => req.set("Content-Type", "application/json").send_bytes(&b),
        None => req.call(),
    };
    let resp = match got {
        Ok(r) | Err(ureq::Error::Status(_, r)) => r,
        Err(ureq::Error::Transport(t)) => return Err(NetError(redact(&t.to_string()))),
    };
    let headers = resp.headers_names().into_iter().filter_map(|n| resp.header(&n).map(|v| (n.to_ascii_lowercase(), v.to_string()))).collect();
    Ok(Reply { status: resp.status(), headers, body: resp.into_reader() })
}

/// A proxy URL without the password a user might have put in it.
fn redact(s: &str) -> String {
    match (s.find("://"), s.find('@')) {
        (Some(a), Some(b)) if b > a => format!("{}***@{}", &s[..a + 3], &s[b + 1..]),
        _ => s.to_string(),
    }
}

fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map_or("", |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let hostport = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = match hostport.strip_prefix('[') {
        Some(inner) => inner.split(']').next().unwrap_or_default(),
        None => hostport.rsplit_once(':').map_or(hostport, |(h, _)| h),
    };
    host.to_ascii_lowercase()
}
