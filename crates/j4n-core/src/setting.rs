//! Settings (the kind `setting`, `packs/base/kinds/setting.toml`): the named numbers and switches the loop and the rules read.
//! A pack declares one with its type and its default, a layer above says its `value`. A rule reads them as `set.loop.followups`:
//! the dots of an id are the steps of the path, so `loop.followups` and `loop.visits` are two keys of `set.loop`.

use crate::expr::Expr;
use crate::table::{Kind, Problem, Tables};
use crate::value::Value;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Ty {
    Int,
    Num,
    Bool,
    Dur,
    Str,
}

/// An entry of kind `setting`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingRow {
    pub id: String,
    pub doc: Option<String>,
    pub ty: Ty,
    pub default: toml::Value,
    pub value: Option<toml::Value>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// For `str`: the words it may hold, so an editor can offer them and an edit is checked against them; none: any words.
    pub words: Option<Vec<String>>,
    /// False: it decides what is wired at the start, so an edit waits for the next start (`⟳ next start`); left out, it is live.
    pub live: Option<bool>,
    /// True for a setting J4n basic shows first (Settings › Features); the rest are under All.
    pub basic: Option<bool>,
    /// What the setting holds under `packs.full`: the pack's own switch for its feature, turned on. A layer above the packs (the operator's)
    /// still wins. Left out: `packs.full` does not touch it.
    pub full: Option<toml::Value>,
    /// For a `str` row whose words are `auto`, `on` and `off`: `setting=word`, the one switch `auto` follows. `auto` holds what that setting says when it is
    /// that word (`lanes=orchestrate`: on while the word is `orchestrate`, off else), `on` and `off` are the operator's own say and win, and a `true` or `false`
    /// that an older file holds reads as `on` and `off`. The row is read as a switch (`flag`) after the settings are loaded.
    pub auto: Option<String>,
}

impl SettingRow {
    /// Whether an edit takes effect while J4n runs (`live`, true when left out).
    pub fn is_live(&self) -> bool {
        self.live.unwrap_or(true)
    }

    /// The setting and the word `auto` follows (`lanes`, `orchestrate`), for a row that has the field.
    pub fn follows(&self) -> Option<(&str, &str)> {
        self.auto.as_deref()?.split_once('=')
    }

    /// What `v` would hold, checked as an edit is: the type, `min` and `max`, and for words the list of `words`.
    pub fn check(&self, v: &toml::Value) -> Result<Value, String> {
        let held = read(self, v)?;
        if let (Some(words), toml::Value::String(w)) = (&self.words, v)
            && !words.iter().any(|x| x == w)
        {
            return Err(format!("`{w}` is not a word of {}: it is one of {}", self.id, words.join(", ")));
        }
        Ok(held)
    }

    /// `v` (as a screen sends it, JSON) as the Table holds a value of this type: a number for `int`, words for `dur`.
    pub fn value_of(&self, v: &serde_json::Value) -> Result<toml::Value, String> {
        use serde_json::Value as J;
        let parsed = match (self.ty, v) {
            (Ty::Bool, J::Bool(b)) => toml::Value::Boolean(*b),
            (Ty::Bool, J::String(s)) if matches!(s.as_str(), "true" | "on") => toml::Value::Boolean(true),
            (Ty::Bool, J::String(s)) if matches!(s.as_str(), "false" | "off") => toml::Value::Boolean(false),
            (Ty::Int, J::Number(n)) if n.as_i64().is_some() => toml::Value::Integer(n.as_i64().unwrap_or_default()),
            (Ty::Int, J::String(s)) if s.trim().parse::<i64>().is_ok() => toml::Value::Integer(s.trim().parse().unwrap_or_default()),
            (Ty::Num, J::Number(n)) => toml::Value::Float(n.as_f64().unwrap_or_default()),
            (Ty::Num, J::String(s)) if s.trim().parse::<f64>().is_ok() => toml::Value::Float(s.trim().parse().unwrap_or_default()),
            // a switch with `auto` is words: a yes or a no says `on` or `off`
            (Ty::Str, J::Bool(b)) if self.auto.is_some() => toml::Value::String(if *b { "on" } else { "off" }.into()),
            (Ty::Str, J::String(s)) if self.auto.is_some() && matches!(s.as_str(), "true" | "false") => toml::Value::String(if s == "true" { "on" } else { "off" }.into()),
            (Ty::Dur | Ty::Str, J::String(s)) => toml::Value::String(s.trim().to_string()),
            (ty, other) => return Err(format!("{other} is not {ty:?}")),
        };
        self.check(&parsed).map(|_| parsed)
    }
}

impl Kind for SettingRow {
    const NAME: &'static str = "setting";
}

/// What a setting holds, if `v` fits it.
fn read(row: &SettingRow, v: &toml::Value) -> Result<Value, String> {
    let in_range = |n: f64| match (row.min, row.max) {
        (Some(lo), _) if n < lo => Err(format!("{n} is below {lo}")),
        (_, Some(hi)) if n > hi => Err(format!("{n} is above {hi}")),
        _ => Ok(Value::num(n)),
    };
    match (row.ty, v) {
        (Ty::Int, toml::Value::Integer(i)) => in_range(*i as f64),
        (Ty::Num, toml::Value::Integer(i)) => in_range(*i as f64),
        (Ty::Num, toml::Value::Float(f)) if f.is_finite() => in_range(*f),
        (Ty::Bool, toml::Value::Boolean(b)) => Ok(Value::Bool(*b)),
        (Ty::Dur, toml::Value::String(s)) => Expr::dur(s).map(Value::Dur).ok_or_else(|| format!("`{s}` is not a duration such as \"30m\"")),
        (Ty::Str, toml::Value::String(s)) => Ok(Value::text(s)),
        // a row that was a switch before it had `auto`: the `true` or `false` of an older file stands as `on` or `off`
        (Ty::Str, toml::Value::Boolean(b)) if row.auto.is_some() => Ok(Value::text(if *b { "on" } else { "off" })),
        (ty, other) => Err(format!("{} is not {ty:?}", other.type_str())),
    }
}

/// The settings, by id and as the tree `set` reads.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Settings {
    flat: BTreeMap<String, Value>,
}

impl Settings {
    /// The Table's settings. A value that does not fit is a problem and the default stands; a default that does not fit is a
    /// problem and the setting is not there.
    pub fn load(tables: &Tables) -> (Settings, Vec<Problem>) {
        let (mut flat, mut problems) = (BTreeMap::new(), Vec::new());
        let (mut rows, mut autos) = (Vec::new(), Vec::new());
        for id in tables.ids(SettingRow::NAME) {
            match tables.get::<SettingRow>(id) {
                Ok(r) => rows.push(r),
                Err(p) => problems.push(p),
            }
        }
        for row in rows {
            let at = format!("setting {}", row.id);
            let steps: Vec<&str> = row.id.split('.').collect();
            if steps.iter().any(|s| s.is_empty()) {
                problems.push(Problem::refused(at, "an id is words joined by single dots"));
                continue;
            }
            let default = match read(&row, &row.default) {
                Ok(v) => v,
                Err(m) => {
                    problems.push(Problem::refused(at, format!("its default does not fit: {m}")));
                    continue;
                }
            };
            let value = match row.value.as_ref().map(|v| read(&row, v)) {
                Some(Ok(v)) => v,
                Some(Err(m)) => {
                    let file = tables.said_in(SettingRow::NAME, &row.id, "value").unwrap_or_default();
                    problems.push(Problem::warn(format!("{file} [[setting]] {}", row.id), format!("its value does not fit, so the default stands: {m}")));
                    default
                }
                None => default,
            };
            // `a` and `a.b` cannot both be set: the tree has one thing at a place
            let clash = flat.keys().any(|k: &String| k.strip_prefix(row.id.as_str()).is_some_and(|r| r.starts_with('.')) || row.id.strip_prefix(k.as_str()).is_some_and(|r| r.starts_with('.')));
            if clash {
                problems.push(Problem::refused(at, "another setting is a part of this one, or this one a part of it"));
                continue;
            }
            if row.auto.is_some() {
                autos.push(row.clone());
            }
            flat.insert(row.id, value);
        }
        // a row with `auto` is read as a switch from here on: the operator's `on` or `off`, else what the setting it follows says
        for row in autos {
            let Some((id, word)) = row.follows() else {
                problems.push(Problem::refused(format!("setting {}", row.id), "its `auto` is `setting=word`"));
                continue;
            };
            let on = match flat.get(&row.id) {
                Some(Value::Str(w)) if w == "on" => true,
                Some(Value::Str(w)) if w == "off" => false,
                _ => matches!(flat.get(id), Some(Value::Str(w)) if w == word),
            };
            flat.insert(row.id.clone(), Value::Bool(on));
        }
        (Settings { flat }, problems)
    }

    /// Settings made in code, for a caller that has no Table.
    pub fn from_pairs(pairs: impl IntoIterator<Item = (&'static str, Value)>) -> Settings {
        Settings { flat: pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect() }
    }

    pub fn get(&self, id: &str) -> Option<&Value> {
        self.flat.get(id)
    }

    /// A setting that holds words (`ty = "str"`).
    pub fn word<'a>(&'a self, id: &str, or: &'a str) -> &'a str {
        match self.get(id) {
            Some(Value::Str(w)) => w,
            _ => or,
        }
    }

    /// A switch (`ty = "bool"`).
    pub fn flag(&self, id: &str, or: bool) -> bool {
        match self.get(id) {
            Some(Value::Bool(b)) => *b,
            _ => or,
        }
    }

    pub fn num(&self, id: &str, or: f64) -> f64 {
        match self.get(id) {
            Some(Value::Num(n)) => *n,
            _ => or,
        }
    }

    pub fn dur_ms(&self, id: &str, or: u64) -> u64 {
        match self.get(id) {
            Some(Value::Dur(ms)) => *ms,
            _ => or,
        }
    }

    /// `set` as a rule reads it: a map of maps.
    pub fn tree(&self) -> Value {
        fn put(into: &mut BTreeMap<String, Value>, steps: &[&str], v: &Value) {
            match steps {
                [] => {}
                [last] => {
                    into.insert((*last).to_string(), v.clone());
                }
                [head, rest @ ..] => {
                    if let Value::Map(m) = into.entry((*head).to_string()).or_insert_with(|| Value::Map(BTreeMap::new())) {
                        put(m, rest, v);
                    }
                }
            }
        }
        let mut root = BTreeMap::new();
        for (id, v) in &self.flat {
            put(&mut root, &id.split('.').collect::<Vec<_>>(), v);
        }
        Value::Map(root)
    }
}

/// The settings and the tree a rule reads, as one value that is swapped whole.
#[derive(Debug, Default)]
pub struct Snap {
    pub settings: Settings,
    pub tree: Value,
}

/// The one live handle on the settings: every reader keeps one of
/// these, and an edit swaps what they all read. A clone is a second handle on the same settings.
#[derive(Debug, Clone, Default)]
pub struct Live(Arc<RwLock<Arc<Snap>>>);

impl From<Settings> for Live {
    fn from(s: Settings) -> Live {
        Live::new(s)
    }
}

impl From<&Settings> for Live {
    fn from(s: &Settings) -> Live {
        Live::new(s.clone())
    }
}

impl From<&Live> for Live {
    fn from(l: &Live) -> Live {
        l.clone()
    }
}

impl Live {
    pub fn new(settings: Settings) -> Live {
        let tree = settings.tree();
        Live(Arc::new(RwLock::new(Arc::new(Snap { settings, tree }))))
    }

    /// The settings as they are now.
    pub fn now(&self) -> Arc<Snap> {
        self.0.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Put `settings` in place of what every holder reads.
    pub fn set(&self, settings: Settings) {
        let tree = settings.tree();
        *self.0.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(Snap { settings, tree });
    }

    pub fn get(&self, id: &str) -> Option<Value> {
        self.now().settings.get(id).cloned()
    }

    pub fn word(&self, id: &str, or: &str) -> String {
        self.now().settings.word(id, or).to_string()
    }

    pub fn flag(&self, id: &str, or: bool) -> bool {
        self.now().settings.flag(id, or)
    }

    pub fn num(&self, id: &str, or: f64) -> f64 {
        self.now().settings.num(id, or)
    }

    pub fn dur_ms(&self, id: &str, or: u64) -> u64 {
        self.now().settings.dur_ms(id, or)
    }
}

/// What holds a copy of some settings and takes new ones while J4n runs (a pack's own numbers): told after every edit.
pub trait Tune: Send + Sync {
    fn tune(&self, settings: &Settings);
}
