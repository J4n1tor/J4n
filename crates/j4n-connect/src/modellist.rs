//! The models each doer has: what a model word is matched against. For a connector, its `models`, and more
//! when it can say them: the output of its `list` command (`pi --list-models`), or the `/models` of an `openai`-shaped endpoint.
//! Read once a day, and again when a word matches none: the front asks when a word is said, and starting J4n runs no program.
//!
//! The kept lists are state of this process, not a fold of the Log: what a program lists is not an event of the work, and a
//! replay must not depend on it.

use crate::manifest::{Connector, Dialect};
use crate::proc;
use j4n_core::front::models::ModelLists;
use j4n_core::listing::{Cache, ROUTER, prose};
use j4n_core::place::Launch;
use j4n_core::setting::Live;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long a list is kept before it is read again.
pub const DAY: Duration = Duration::from_secs(24 * 3600);
/// The least time between two reads of one list, so a run of words that match none does not ask the program each time.
const LEAST: Duration = Duration::from_secs(30);
const LIMIT: Duration = Duration::from_secs(30);

/// Reads a secret by its name and where it is kept.
pub type Secret = dyn Fn(&str, &str) -> Result<String, String> + Send + Sync;

type Read = dyn Fn(&Connector) -> Result<Vec<String>, String> + Send + Sync;

/// The models of every connector, kept.
pub struct ModelsOf {
    rows: Vec<Connector>,
    read: Arc<Read>,
    kept: Mutex<BTreeMap<String, (Instant, Vec<String>)>>,
    day: Duration,
    /// The lists [`Listing`] keeps: a list read within the day is taken from here, so a word said is matched without asking a program.
    cache: Option<Cache>,
}

impl ModelsOf {
    /// The models of `connectors`; `secret` reads the key an endpoint is asked with, and the environment of a listing command.
    pub fn new(connectors: &[Connector], secret: Arc<Secret>) -> ModelsOf {
        let read: Arc<Read> = Arc::new(move |c| ask(c, secret.as_ref()));
        ModelsOf::with_reader(connectors, read)
    }

    /// [`ModelsOf::new`] with another way to read a list, for a test.
    pub fn with_reader(connectors: &[Connector], read: Arc<Read>) -> ModelsOf {
        ModelsOf { rows: connectors.to_vec(), read, kept: Mutex::new(BTreeMap::new()), day: DAY, cache: None }
    }

    /// Take a list that [`Listing`] read within the day from `cache`, where none has been read in this process.
    pub fn cached(mut self, cache: Cache) -> ModelsOf {
        self.cache = Some(cache);
        self
    }

    /// The lists are good for `day` (a test's clock).
    pub fn lasting(mut self, day: Duration) -> ModelsOf {
        self.day = day;
        self
    }

    fn row(&self, doer: &str) -> Option<&Connector> {
        self.rows.iter().find(|c| c.id == doer)
    }

    /// The list of `c` now: read when none is kept, the kept one is a day old, or `force` is set and the last read is not just now.
    fn fetch(&self, c: &Connector, force: bool) -> Vec<String> {
        let mut kept = self.kept.lock().unwrap_or_else(|e| e.into_inner()).get(&c.id).cloned();
        if kept.is_none()
            && let Some(l) = self.cache.as_ref().and_then(|k| k.read().remove(&c.id))
        {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
            let age = Duration::from_millis(now.saturating_sub(l.at_ms));
            if age < self.day && !l.models.is_empty() {
                let at = Instant::now().checked_sub(age).unwrap_or_else(Instant::now);
                self.kept.lock().unwrap_or_else(|e| e.into_inner()).insert(c.id.clone(), (at, l.models.clone()));
                kept = Some((at, l.models));
            }
        }
        let due = match &kept {
            None => true,
            Some((at, _)) => at.elapsed() >= if force { LEAST } else { self.day },
        };
        if !due {
            return kept.map(|(_, l)| l).unwrap_or_default();
        }
        // a program that does not answer leaves the last list in place, and the next word tries it again
        let got = (self.read)(c).unwrap_or_else(|_| kept.as_ref().map(|(_, l)| l.clone()).unwrap_or_default());
        self.kept.lock().unwrap_or_else(|e| e.into_inner()).insert(c.id.clone(), (Instant::now(), got.clone()));
        got
    }

    fn all(&self, c: &Connector, force: bool) -> Vec<String> {
        let mut out = c.models.clone();
        if listed(c) {
            for m in self.fetch(c, force) {
                if !out.contains(&m) {
                    out.push(m);
                }
            }
        }
        out
    }
}

impl ModelLists for ModelsOf {
    fn models(&self, doer: &str) -> Option<Vec<String>> {
        Some(self.row(doer).map(|c| self.all(c, false)).unwrap_or_default())
    }

    fn refresh(&self, doer: &str) -> Option<Vec<String>> {
        Some(self.row(doer).map(|c| self.all(c, true)).unwrap_or_default())
    }

    fn kept(&self, doer: &str) -> Vec<String> {
        self.kept.lock().unwrap_or_else(|e| e.into_inner()).get(doer).map(|(_, l)| l.clone()).unwrap_or_default()
    }
}

/// Whether `c` can say more models than its `models`.
fn listed(c: &Connector) -> bool {
    c.list.as_ref().is_some_and(|l| !l.argv.is_empty()) || (c.dialect == Dialect::Openai && c.url.is_some())
}

fn ask(c: &Connector, secret: &Secret) -> Result<Vec<String>, String> {
    if let Some(l) = c.list.as_ref().filter(|l| !l.argv.is_empty()) {
        let env = c.environment(&secret)?;
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::env::temp_dir());
        let (text, exit) = proc::capture(&Launch::new(l.argv.clone(), cwd, env), LIMIT).map_err(|e| e.to_string())?;
        if exit.code != Some(0) {
            return Err(exit.how());
        }
        return Ok(from_table(&text));
    }
    let url = format!("{}/models", c.url.as_deref().unwrap_or_default().trim_end_matches('/'));
    let key = c.api_key(&secret);
    let bearer = format!("Bearer {key}");
    let headers: Vec<(&str, &str)> = if key.is_empty() { vec![] } else { vec![("Authorization", bearer.as_str())] };
    let reply = c.net().get(&url, &headers).map_err(|e| e.to_string())?;
    if !reply.ok() {
        return Err(format!("{url} answered {}", reply.status));
    }
    from_endpoint(&reply.text().map_err(|e| e.to_string())?)
}

/// The model of each line of a listing: the `model` column when the first line is a header that has one, else the first word of each
/// line that is not prose (one that ends a sentence, or starts with a path or a word like `Warning:`).
pub fn from_table(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).collect();
    let header = lines.first().and_then(|l| l.split_whitespace().position(|w| w.eq_ignore_ascii_case("model")));
    let (col, rows) = match header {
        Some(col) => (col, &lines[1..]),
        None => (0, &lines[..]),
    };
    let mut out: Vec<String> = Vec::new();
    // with no header, a line of prose (`No models available. … See:`, `Warning: …`) or a path it points at names no model
    let model = |l: &&str| header.is_some() || !(l.ends_with([':', '.']) || l.split_whitespace().next().is_some_and(prose));
    for m in rows.iter().copied().filter(model).filter_map(|l| l.split_whitespace().nth(col)) {
        if !out.iter().any(|o| o == m) {
            out.push(m.to_string());
        }
    }
    out
}

/// The ids of what an `openai`-shaped `/models` answered (`{"data":[{"id":…}]}`).
pub fn from_endpoint(body: &str) -> Result<Vec<String>, String> {
    let v: serde_json::Value = serde_json::from_str(body).map_err(|e| format!("the list is not JSON: {e}"))?;
    let data = v.get("data").and_then(|d| d.as_array()).ok_or("the list has no `data`")?;
    Ok(data.iter().filter_map(|m| m.get("id").and_then(|i| i.as_str()).map(String::from)).collect())
}

/// Where the free lists come from: a trait so a test gives it a fake and no net or program is touched.
pub trait ListSource: Send + Sync {
    /// OpenRouter's public list, which takes no key.
    fn router(&self) -> Result<Vec<String>, String>;
    /// A doer's own list: its `list` command.
    fn doer(&self, c: &Connector) -> Result<Vec<String>, String>;
    /// Whether the doer's program is there to be asked.
    fn installed(&self, c: &Connector) -> bool;
}

/// The real source: OpenRouter's `/models`, and the doer's own `list` command.
pub struct Real {
    secret: Arc<Secret>,
}

impl Real {
    pub fn new(secret: Arc<Secret>) -> Real {
        Real { secret }
    }
}

impl ListSource for Real {
    fn router(&self) -> Result<Vec<String>, String> {
        let url = "https://openrouter.ai/api/v1/models";
        let reply = crate::net::Net::new().get(url, &[]).map_err(|e| e.to_string())?;
        if !reply.ok() {
            return Err(format!("{url} answered {}", reply.status));
        }
        from_endpoint(&reply.text().map_err(|e| e.to_string())?)
    }

    fn doer(&self, c: &Connector) -> Result<Vec<String>, String> {
        ask(c, self.secret.as_ref())
    }

    fn installed(&self, c: &Connector) -> bool {
        let Some(program) = c.list.as_ref().and_then(|l| l.argv.first()) else { return false };
        if program.contains('/') {
            return std::path::Path::new(program).is_file();
        }
        std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(program).is_file()))
    }
}

/// What a pass read.
#[derive(Debug, Default, PartialEq)]
pub struct Pass {
    /// The sources asked now, and what each said (the number of models, or why not).
    pub asked: Vec<(String, Result<usize, String>)>,
}

/// The free model lists, read once a day into the library's cache: OpenRouter's public `/models` when the net is allowed, and
/// the `list` of each doer whose program is installed. Nothing runs when a page opens: [`Listing::pass`] runs on a timer of its own while J4n
/// shows a screen, and never at the start.
pub struct Listing {
    cache: Cache,
    connectors: Vec<Connector>,
    source: Arc<dyn ListSource>,
    settings: Option<Live>,
}

impl Listing {
    pub fn new(cache: Cache, connectors: &[Connector], source: Arc<dyn ListSource>, settings: Option<Live>) -> Listing {
        Listing { cache, connectors: connectors.to_vec(), source, settings }
    }

    /// Whether the net may be asked for a list: the same switch that lets J4n ask after the tools' releases.
    fn net(&self) -> bool {
        self.settings.as_ref().is_some_and(|s| s.flag("watch.upstream", false))
    }

    /// Read the lists that are a day old (or not read yet) at `now_ms`. A source that fails keeps its last list and is not asked again before a day
    /// has passed, so a program that does not answer is not asked every hour.
    pub fn pass(&self, now_ms: u64) -> Pass {
        let mut out = Pass::default();
        let kept = self.cache.read();
        let mut put = |source: &str, got: Result<Vec<String>, String>| {
            let models = got.as_ref().ok().cloned().or_else(|| kept.get(source).map(|l| l.models.clone())).unwrap_or_default();
            let wrote = self.cache.put(source, now_ms, &models);
            out.asked.push((source.to_string(), got.map(|l| l.len()).and_then(|n| wrote.map(|()| n))));
        };
        if self.net() && self.cache.due(ROUTER, now_ms) {
            put(ROUTER, self.source.router());
        }
        for c in self.connectors.iter().filter(|c| c.list.as_ref().is_some_and(|l| !l.argv.is_empty())) {
            if self.cache.due(&c.id, now_ms) && self.source.installed(c) {
                put(&c.id, self.source.doer(c));
            }
        }
        out
    }

    /// Look on a timer of its own while the process lives (it holds the listing), the first time a minute after the screen is up and then each
    /// hour; the lists are read only when a day old.
    pub fn start(this: Arc<Listing>) {
        let _ = std::thread::Builder::new().name("j4n-listing".into()).spawn(move || {
            std::thread::sleep(Duration::from_secs(60));
            loop {
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
                this.pass(now);
                std::thread::sleep(Duration::from_secs(3600));
            }
        });
    }
}
