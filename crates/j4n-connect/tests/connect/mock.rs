//! A gateway on the loopback that answers as a script says, and a proxy that writes down who came through it. Both are plain
//! threads on plain sockets, so what is tested is the real client, the real wire and the real streaming.

use serde_json::Value;
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A request as the gateway saw it.
#[derive(Debug, Clone)]
pub struct Seen {
    pub method: String,
    pub path: String,
    /// Names in lower case.
    pub headers: BTreeMap<String, String>,
    pub body: String,
}

impl Seen {
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap_or(Value::Null)
    }
}

/// What the gateway says back.
pub enum Say {
    /// A JSON body.
    Json(u16, Value),
    /// Server-sent events, one `data:` line per frame, then the connection closes.
    Sse(Vec<String>),
    /// Any status with a text body.
    Text(u16, String),
    /// Nothing at all: the connection is closed.
    Drop,
    /// A 200 that promises more than it sends: these frames, then the connection closes with the body short.
    Cut(Vec<String>),
    /// Say the other after this long.
    After(Duration, Box<Say>),
    /// A status with these headers and a text body (`Retry-After`).
    Headed(u16, Vec<(&'static str, String)>, String),
}

type Script = dyn Fn(&Seen, usize) -> Say + Send + Sync;

pub struct Mock {
    pub addr: SocketAddr,
    seen: Arc<Mutex<Vec<Seen>>>,
    stop: Arc<AtomicBool>,
}

impl Mock {
    /// `script` is given each request and its number (from 0) and says what to answer.
    pub fn start(script: impl Fn(&Seen, usize) -> Say + Send + Sync + 'static) -> Mock {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let (seen, stop) = (Arc::new(Mutex::new(Vec::new())), Arc::new(AtomicBool::new(false)));
        let script: Arc<Script> = Arc::new(script);
        let (s2, st2) = (seen.clone(), stop.clone());
        std::thread::spawn(move || {
            while !st2.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((conn, _)) => {
                        let (s, sc) = (s2.clone(), script.clone());
                        std::thread::spawn(move || serve(conn, &s, &*sc));
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(5)),
                }
            }
        });
        Mock { addr, seen, stop }
    }

    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

impl Drop for Mock {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn read_request(conn: &mut TcpStream) -> Option<Seen> {
    let mut reader = BufReader::new(conn.try_clone().ok()?);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let mut parts = line.split_whitespace();
    let (method, path) = (parts.next()?.to_string(), parts.next()?.to_string());
    let mut headers = BTreeMap::new();
    loop {
        let mut h = String::new();
        reader.read_line(&mut h).ok()?;
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        let (k, v) = h.split_once(':')?;
        headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
    }
    let n: usize = headers.get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    let mut body = vec![0u8; n];
    reader.read_exact(&mut body).ok()?;
    Some(Seen { method, path, headers, body: String::from_utf8_lossy(&body).into_owned() })
}

fn serve(mut conn: TcpStream, seen: &Mutex<Vec<Seen>>, script: &Script) {
    let Some(req) = read_request(&mut conn) else { return };
    let n = {
        let mut all = seen.lock().unwrap();
        all.push(req.clone());
        all.len() - 1
    };
    let head = |status: u16, ctype: &str| format!("HTTP/1.1 {status} X\r\nContent-Type: {ctype}\r\nConnection: close\r\n\r\n");
    let frames = |list: &[String]| list.iter().map(|f| format!("data: {f}\n\n")).collect::<String>();
    let mut said = script(&req, n);
    while let Say::After(wait, next) = said {
        std::thread::sleep(wait);
        said = *next;
    }
    match said {
        Say::After(..) => {}
        Say::Json(status, v) => {
            let _ = write!(conn, "{}{v}", head(status, "application/json"));
        }
        Say::Text(status, t) => {
            let _ = write!(conn, "{}{t}", head(status, "text/plain"));
        }
        Say::Sse(list) => {
            let _ = write!(conn, "{}{}", head(200, "text/event-stream"), frames(&list));
        }
        Say::Cut(list) => {
            let _ = write!(conn, "HTTP/1.1 200 X\r\nContent-Type: text/event-stream\r\nContent-Length: 100000\r\nConnection: close\r\n\r\n{}", frames(&list));
        }
        Say::Headed(status, headers, t) => {
            let extra: String = headers.iter().map(|(k, v)| format!("{k}: {v}\r\n")).collect();
            let _ = write!(conn, "HTTP/1.1 {status} X\r\nContent-Type: text/plain\r\n{extra}Connection: close\r\n\r\n{t}");
        }
        Say::Drop => {}
    }
    let _ = conn.flush();
}

/// A proxy that takes a call, writes the target down, and joins the caller to `to`: whatever it is asked to reach, it
/// reaches the mock. What it was asked for is what a test reads.
pub struct Proxy {
    pub addr: SocketAddr,
    calls: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
}

impl Proxy {
    pub fn start(to: SocketAddr) -> Proxy {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let (calls, stop) = (Arc::new(Mutex::new(Vec::new())), Arc::new(AtomicBool::new(false)));
        let (c2, st2) = (calls.clone(), stop.clone());
        std::thread::spawn(move || {
            while !st2.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((conn, _)) => {
                        let c = c2.clone();
                        std::thread::spawn(move || tunnel(conn, to, &c));
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(5)),
                }
            }
        });
        Proxy { addr, calls, stop }
    }

    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// What each caller asked it to reach: `host:port` for a `CONNECT`, the whole URL for a plain-http call.
    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn tunnel(mut conn: TcpStream, to: SocketAddr, calls: &Mutex<Vec<String>>) {
    let Ok(clone) = conn.try_clone() else { return };
    let mut reader = BufReader::new(clone);
    let mut first = String::new();
    let mut head = String::new();
    if reader.read_line(&mut first).is_err() {
        return;
    }
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).is_err() {
            return;
        }
        let end = h.trim().is_empty();
        head.push_str(&h);
        if end {
            break;
        }
    }
    let mut words = first.split_whitespace();
    let (method, target) = (words.next().unwrap_or_default().to_string(), words.next().unwrap_or_default().to_string());
    calls.lock().unwrap().push(target.clone());
    let Ok(mut out) = TcpStream::connect(to) else { return };
    if method == "CONNECT" {
        let _ = conn.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n");
    } else {
        // a plain-http call arrives in absolute form; the target gets it as it would from the caller itself
        let path = target.split_once("://").and_then(|(_, r)| r.find('/').map(|i| &r[i..])).unwrap_or("/");
        let _ = write!(out, "{method} {path} HTTP/1.1\r\n{head}");
    }
    let (mut up, mut down) = (out.try_clone().unwrap(), conn.try_clone().unwrap());
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut reader, &mut up);
        let _ = up.shutdown(std::net::Shutdown::Write);
    });
    let _ = std::io::copy(&mut out, &mut down);
    let _ = down.shutdown(std::net::Shutdown::Both);
}
