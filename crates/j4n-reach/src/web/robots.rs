//! `robots.txt` (RFC 9309), the part a fetch by a program needs: the group of the product token `j4n` or else of `*`, `Allow` and
//! `Disallow` by the longest match (`Allow` wins a tie), `*` and a final `$` in a rule. A file that is not there (4xx) allows
//! everything; one that cannot be had (5xx, or no answer) allows nothing, as the RFC says.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const TOKEN: &str = "j4n";
const KEEP: Duration = Duration::from_secs(3600);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rules {
    /// `(allow, pattern)`
    list: Vec<(bool, String)>,
    /// The file could not be had: nothing is allowed.
    closed: bool,
}

/// A pattern against the start of `path`: `*` is any run, a final `$` ends the path.
fn matches(pattern: &str, path: &str) -> bool {
    let (pattern, anchored) = pattern.strip_suffix('$').map_or((pattern, false), |p| (p, true));
    let parts: Vec<&str> = pattern.split('*').collect();
    let mut at = 0usize;
    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            if !path.starts_with(part) {
                return false;
            }
            at = part.len();
        } else if let Some(found) = path[at..].find(part) {
            at += found + part.len();
        } else {
            return false;
        }
    }
    !anchored || at == path.len() || parts.last().is_some_and(|l| l.is_empty() && pattern.ends_with('*'))
}

impl Rules {
    pub fn allow_all() -> Rules {
        Rules::default()
    }

    pub fn closed() -> Rules {
        Rules { list: Vec::new(), closed: true }
    }

    /// The rules of the group that names `j4n`, else of `*`.
    pub fn parse(text: &str) -> Rules {
        let (mut ours, mut star) = (Vec::<(bool, String)>::new(), Vec::<(bool, String)>::new());
        let (mut agents, mut in_rules) = (Vec::<String>::new(), false);
        for raw in text.lines() {
            let line = raw.split('#').next().unwrap_or_default().trim();
            let Some((key, value)) = line.split_once(':') else { continue };
            let (key, value) = (key.trim().to_ascii_lowercase(), value.trim());
            match key.as_str() {
                "user-agent" => {
                    if in_rules {
                        agents.clear();
                        in_rules = false;
                    }
                    agents.push(value.to_ascii_lowercase());
                }
                "allow" | "disallow" => {
                    in_rules = true;
                    let rule = (key == "allow", value.to_string());
                    // an empty `Disallow` allows everything: it is no rule
                    if rule.1.is_empty() {
                        continue;
                    }
                    if agents.iter().any(|a| a == TOKEN) {
                        ours.push(rule.clone());
                    }
                    if agents.iter().any(|a| a == "*") {
                        star.push(rule);
                    }
                }
                _ => {}
            }
        }
        Rules { list: if ours.is_empty() { star } else { ours }, closed: false }
    }

    pub fn allows(&self, path: &str) -> bool {
        if self.closed {
            return false;
        }
        let best = self.list.iter().filter(|(_, p)| matches(p, path)).max_by_key(|(allow, p)| (p.len(), *allow));
        best.is_none_or(|(allow, _)| *allow)
    }
}

/// The rules of each origin, for an hour.
#[derive(Default)]
pub struct Cache(Mutex<BTreeMap<String, (Instant, Rules)>>);

impl Cache {
    pub fn get(&self, origin: &str) -> Option<Rules> {
        let map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        map.get(origin).filter(|(at, _)| at.elapsed() < KEEP).map(|(_, r)| r.clone())
    }

    pub fn put(&self, origin: &str, rules: Rules) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).insert(origin.to_string(), (Instant::now(), rules));
    }
}
