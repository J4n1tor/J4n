//! A web server on the loopback that answers as a script says, and writes down what it was asked.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Seen {
    pub method: String,
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub body: String,
}

/// `(status, headers, body)`.
pub type Answer = (u16, Vec<(&'static str, String)>, String);

pub fn page(body: &str) -> Answer {
    (200, vec![("Content-Type", "text/html; charset=utf-8".into())], body.into())
}

pub fn json(status: u16, body: &str) -> Answer {
    (status, vec![("Content-Type", "application/json".into())], body.into())
}

pub struct Server {
    pub addr: SocketAddr,
    seen: Arc<Mutex<Vec<Seen>>>,
    stop: Arc<AtomicBool>,
}

impl Server {
    pub fn start(script: impl Fn(&Seen) -> Answer + Send + Sync + 'static) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let (seen, stop) = (Arc::new(Mutex::new(Vec::new())), Arc::new(AtomicBool::new(false)));
        let script = Arc::new(script);
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
        Server { addr, seen, stop }
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }

    pub fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }

    pub fn paths(&self) -> Vec<String> {
        self.seen().into_iter().map(|s| s.path).collect()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn serve(mut conn: TcpStream, seen: &Mutex<Vec<Seen>>, script: &(dyn Fn(&Seen) -> Answer + Send + Sync)) {
    let Ok(clone) = conn.try_clone() else { return };
    let mut reader = BufReader::new(clone);
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let mut parts = line.split_whitespace();
    let (Some(method), Some(path)) = (parts.next(), parts.next()) else { return };
    let (method, path) = (method.to_string(), path.to_string());
    let mut headers = BTreeMap::new();
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).is_err() {
            return;
        }
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    let n: usize = headers.get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    let mut body = vec![0u8; n];
    if reader.read_exact(&mut body).is_err() {
        return;
    }
    let req = Seen { method, path, headers, body: String::from_utf8_lossy(&body).into_owned() };
    seen.lock().unwrap().push(req.clone());
    let (status, hs, text) = script(&req);
    let mut head = format!("HTTP/1.1 {status} X\r\nConnection: close\r\nContent-Length: {}\r\n", text.len());
    for (k, v) in hs {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    let _ = conn.write_all(format!("{head}\r\n{text}").as_bytes());
}
