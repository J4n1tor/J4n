//! The look file: how the Log shows each kind of event and each field, as entries of
//! the kind `look` (`packs/base/look/look.toml`) that the screen draws from, sent as the value `look:file`. Its layers: the packs, then
//! what the look's pass made (`~/.j4n/cache/look.generated.toml`, [`layer`]), then the operator's (`~/.j4n/ui/look.toml`), which wins and is
//! never written; a project's look file is never read.
//!
//! The front counts what the look does not cover: a kind no family has, a number or a yes or no in a field no renderer has, a key of a page with no
//! word ([`Miss`]; a sample or two of each, cut and masked by the guard's redactor). Behind `look.generate`, when the misses pass
//! `look.misses` or a day went by with any, at most once an hour and never while a task works, the ask point `look` (jev first) is
//! asked for entries for those only, picking among the families, lines and renderers that exist; the answer is checked ([`check`]) and
//! added to the generated file, or refused and the file kept. Each pass is a `look.made` (or `look.refused`) in the Log.

use super::Front;
use crate::answer::chain::{Spent, Walk};
use crate::answer::Question;
use crate::ask::{Answer, AskShape, Opt};
use crate::change::Actor;
use crate::log::{Ev, New};
use crate::table::{Kind, Layer, LayerKind, Level, SourceFile, Tables};
use crate::task::State as Task;
use crate::value::Value;
use serde::Deserialize;
use serde_json::{Value as Json, json};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;

/// The least time between two passes, and how long a few misses wait before they make one anyway.
pub const HOUR: u64 = 3_600_000;
pub const DAY: u64 = 24 * HOUR;
/// The widest a line may be on any sample of its kind.
pub const WIDE: usize = 64;
/// The most misses one pass asks about, the samples each keeps, and how long a string of a sample is kept.
const MOST: usize = 12;
const SAMPLES: usize = 2;
const CUT: usize = 120;
/// The renderers a yes or no may take: a result is the tier's tick, a switch is `on` or `off`, or it stays `true` / `false`.
const YESNO: [(&str, &str); 3] = [
    ("tick", "a result, as ✓ or ✗"),
    ("onoff", "a switch, as on or off"),
    ("text", "as it is: true or false"),
];
/// The renderers a number may take, and what each makes of one.
const NUMBERS: [(&str, &str); 5] = [
    ("text", "as it is: a count, a score"),
    ("duration", "milliseconds, as 340 ms, 1.2 s, 2 m 05 s"),
    ("time", "milliseconds since 1970, as a date and a time"),
    ("money", "dollars, as $0.0120"),
    ("bytes", "a count of bytes, as 4.0 kB, 1.2 MB"),
];

/// One entry of the look file.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LookRow {
    pub id: String,
    #[serde(default)]
    pub doc: String,
    pub glyph: Option<String>,
    pub role: Option<String>,
    pub text: Option<String>,
    pub show: Option<String>,
    pub tone: Option<String>,
}

impl Kind for LookRow {
    const NAME: &'static str = "look";
}

impl LookRow {
    /// The entry as a `[[look]]` table of a file.
    fn toml(&self) -> String {
        let fields = [("id", Some(&self.id)), ("doc", Some(&self.doc)), ("glyph", self.glyph.as_ref()), ("role", self.role.as_ref()), ("text", self.text.as_ref()), ("show", self.show.as_ref()), ("tone", self.tone.as_ref())];
        let lines: String = fields.iter().filter_map(|(k, v)| v.filter(|v| !v.is_empty()).map(|v| format!("{k} = {}\n", toml::Value::from(v.as_str())))).collect();
        format!("\n[[look]]\n{lines}")
    }
}

/// What the look missed: how often, and a sample or two, masked.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Miss {
    pub n: u32,
    pub samples: Vec<Json>,
}

/// The look's pass as it stands: what was made since the start, what is missed, and when.
#[derive(Default)]
pub struct State {
    made: Vec<LookRow>,
    misses: BTreeMap<String, Miss>,
    since: Option<u64>,
    last: Option<u64>,
    busy: bool,
    cover: Option<(Vec<String>, Vec<String>)>,
    paged: bool,
}

/// Whether a field called `name` is the field (or the `_suffix`) `pat` names.
pub fn fits(pat: &str, name: &str) -> bool {
    if pat.starts_with('_') { name.ends_with(pat) && name.len() > pat.len() } else { pat == name }
}

/// The line `tpl` makes of the event `ev`: each `{data.x}` hole filled with a value's first line; a hole with nothing is left out.
pub fn fill(tpl: &str, ev: &Json) -> String {
    let mut out = String::new();
    for part in tpl.split('{') {
        let (hole, rest) = part.split_once('}').unwrap_or(("", part));
        match (!hole.is_empty()).then(|| hole.split('.').try_fold(ev, |v, k| v.get(k))).flatten() {
            Some(Json::String(s)) => out.push_str(s.lines().next().unwrap_or("")),
            Some(v) if !v.is_object() && !v.is_array() && !v.is_null() => out.push_str(&v.to_string()),
            _ => {}
        }
        out.push_str(rest);
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The look as the screen is sent it: `{family: {prefix: {glyph, role}}, summary: {kind: text}, field: {name: show}, tone: {word: tone}}`.
pub fn json(rows: &[LookRow]) -> Json {
    let mut out = json!({ "family": {}, "summary": {}, "field": {}, "tone": {} });
    for r in rows {
        let Some((part, key)) = r.id.split_once('/') else { continue };
        let v = match part {
            "family" => json!({ "glyph": r.glyph, "role": r.role }),
            "summary" => json!(r.text),
            "field" => json!(r.show),
            "tone" => json!(r.tone),
            _ => continue,
        };
        out[part][key] = v;
    }
    out
}

/// The layer of the entries the look's pass made, from the file at `path` (none there: an empty layer). It goes above the packs and
/// below the operator's folder, so what the operator says of an entry wins.
pub fn layer(path: &Path) -> Layer {
    let files = std::fs::read_to_string(path).map(|text| vec![SourceFile { path: "look.generated.toml".into(), text }]).unwrap_or_default();
    Layer::new(LayerKind::Pack, "look.generated", files)
}

/// Whether `text` (`[[look]]` tables) may be added to the look: it parses, names only the slots, roles and renderers that exist, each
/// entry is whole, and each line fits in [`WIDE`] on every sample of its kind. The entries, or why not.
pub fn check(text: &str, samples: &BTreeMap<String, Vec<Json>>) -> Result<Vec<LookRow>, String> {
    let file = SourceFile { path: "look.generated.toml".into(), text: format!("api = 1\n{text}") };
    let (tables, found) = Tables::load(vec![Layer::new(LayerKind::Pack, "look.generated", vec![file])]);
    if let Some(p) = found.iter().find(|p| p.level == Level::Refused) {
        return Err(p.to_string());
    }
    let rows = tables.list::<LookRow>().map_err(|p| p.to_string())?;
    for r in &rows {
        let whole = match r.id.split_once('/') {
            Some(("family", _)) => r.glyph.is_some() && r.role.is_some(),
            Some(("summary", _)) => r.text.is_some(),
            Some(("field", _)) => r.show.is_some(),
            Some(("tone", _)) => r.tone.is_some(),
            _ => false,
        };
        if !whole {
            return Err(format!("`{}` is not a whole entry of the look", r.id));
        }
        if let (Some(kind), Some(tpl)) = (r.id.strip_prefix("summary/"), &r.text)
            && let Some(w) = samples.get(kind).into_iter().flatten().map(|s| fill(tpl, s).chars().count()).find(|w| *w > WIDE)
        {
            return Err(format!("the line of `{kind}` is {w} wide on a sample, wider than {WIDE}"));
        }
    }
    Ok(rows)
}

/// The sample of an event a decider may see: its strings cut, then masked.
fn sample(front: &Front, ev: &Ev) -> Json {
    fn cut(v: &mut Json) {
        match v {
            Json::String(s) if s.chars().count() > CUT => *s = format!("{}…", s.chars().take(CUT).collect::<String>()),
            Json::Array(a) => a.iter_mut().for_each(cut),
            Json::Object(m) => m.values_mut().for_each(cut),
            _ => {}
        }
    }
    let mut s = ev.shown();
    cut(&mut s);
    front.log().redactor().scrub_json(&mut s);
    s
}

/// The lines a kind might have, as `(template, what it makes of the sample)`: each of its first short fields, and the first two.
fn lines(sample: &Json) -> Vec<(String, String)> {
    let names: Vec<&String> = sample["data"].as_object().into_iter().flatten().filter(|(_, v)| v.is_string() || v.is_number()).map(|(k, _)| k).take(4).collect();
    let mut tpls: Vec<String> = names.iter().map(|k| format!("{{data.{k}}}")).collect();
    if names.len() > 1 {
        tpls.push(format!("{{data.{}}} {{data.{}}}", names[0], names[1]));
    }
    tpls.into_iter().map(|t| (fill(&t, sample), t)).filter(|(s, _)| !s.is_empty() && s.chars().count() <= WIDE).map(|(s, t)| (t, s)).collect()
}

impl Front {
    fn lookstate(&self) -> std::sync::MutexGuard<'_, State> {
        self.0.lookfile.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Every entry of the look: the Table's, then what the passes made since the start (which the Table did not have).
    pub fn look_rows(&self) -> Vec<LookRow> {
        let mut rows = self.screen().tables.list::<LookRow>().unwrap_or_default();
        rows.extend(self.lookstate().made.iter().cloned());
        rows
    }

    /// What the look misses now, by `kind:<kind>`, `field:<name>` and `page:<id>/<key>`.
    pub fn look_misses(&self) -> BTreeMap<String, Miss> {
        self.lookstate().misses.clone()
    }

    fn look_set(&self, id: &str) -> Value {
        self.screen().set.pointer(&format!("/look/{id}"))
    }

    /// Count what the look does not cover in `ev`: its kind, and its numbers; the first time, the keys of pages that have no word.
    pub fn look_see(&self, ev: &Ev) {
        if self.look_set("generate") == Value::Bool(false) || ev.kind.starts_with("look.") {
            return;
        }
        let need = {
            let st = self.lookstate();
            st.cover.is_none() || !st.paged
        };
        let rows = need.then(|| self.look_rows());
        let mut st = self.lookstate();
        if !st.paged {
            st.paged = true;
            let hinted: Vec<String> = self.screen().hints.iter().map(|h| h.key.clone()).collect();
            for k in self.screen().tables.list::<super::keys::KeyRow>().unwrap_or_default() {
                if k.id.starts_with("page:") && k.word.is_empty() && !hinted.contains(&k.id) {
                    st.misses.entry(k.id.clone()).or_default().n += 1;
                }
            }
        }
        let (families, fields) = st.cover.get_or_insert_with(|| {
            let rows = rows.unwrap_or_default();
            let part = |p: &str| rows.iter().filter_map(|r| r.id.strip_prefix(p).map(String::from)).collect::<Vec<_>>();
            (part("family/").into_iter().filter(|f| !f.is_empty()).collect(), part("field/"))
        });
        let mut found = Vec::new();
        if !families.iter().any(|f| ev.kind.starts_with(f.as_str())) {
            found.push(format!("kind:{}", ev.kind));
        }
        let values = ev.data.as_object().into_iter().flatten().filter(|(_, v)| v.is_number() || v.is_boolean());
        found.extend(values.filter(|(k, _)| !fields.iter().any(|f| fits(f, k))).map(|(k, _)| format!("field:{k}")));
        if found.is_empty() {
            return;
        }
        let now = self.log().clock().now_ms();
        st.since.get_or_insert(now);
        for key in found {
            let m = st.misses.entry(key).or_default();
            m.n += 1;
            if m.samples.len() < SAMPLES {
                m.samples.push(sample(self, ev));
            }
        }
    }

    /// Whether a pass may run now, or why not.
    pub fn look_due(&self) -> Result<(), String> {
        let st = self.lookstate();
        self.due(&st, self.log().clock().now_ms())
    }

    fn due(&self, st: &State, now: u64) -> Result<(), String> {
        let n: u32 = st.misses.iter().filter(|(k, _)| !k.starts_with("page:")).map(|(_, m)| m.n).sum();
        let least = match self.look_set("misses") {
            Value::Num(x) => x as u32,
            _ => 12,
        };
        if self.look_set("generate") == Value::Bool(false) {
            Err("look.generate is off".into())
        } else if st.busy {
            Err("a pass is running".into())
        } else if n == 0 {
            Err("the look misses nothing".into())
        } else if st.last.is_some_and(|l| now < l + HOUR) {
            Err("the last pass was less than an hour ago".into())
        } else if n < least && st.since.is_none_or(|s| now < s + DAY) {
            Err(format!("{n} misses, fewer than {least}, and none a day old"))
        } else if self.tasks().book().rows().iter().any(|r| r.state == Task::Running) {
            Err("a task works".into())
        } else {
            Ok(())
        }
    }

    /// A pass, off this thread, when one is due.
    pub fn look_soon(&self) {
        if self.look_due().is_ok() {
            let front = self.clone();
            std::thread::spawn(move || front.look_pass());
        }
    }

    /// One question to the ask point `look`: the option picked, and who picked it. What the walk cost is added to `spent`, whether or
    /// not it came to an answer.
    fn look_ask(&self, n: usize, text: String, options: Vec<Opt>, spent: &mut Spent) -> Option<(String, String)> {
        let goal = "a look, among those that exist, for what the look file does not cover yet".to_string();
        let ask = format!("look:{}:{n}", self.log().clock().now_ms());
        let q = Question { ask, point: "look".into(), asker: "look".into(), thread: None, text, shape: AskShape::Choice { options }, goal, act: None };
        let asked = self.who().ask("look", None, &q, None)?;
        spent.add(&asked.spent);
        match asked.walk {
            Walk::Answered(o) => match o.answer {
                Answer::Pick { id } => Some((id, o.by)),
                _ => None,
            },
            _ => None,
        }
    }

    /// The pass, when it is due: entries for the misses, checked, kept, and said as `look.made`; or `look.refused` and the file kept.
    /// The data of the event written, or `None` when no pass was due.
    pub fn look_pass(&self) -> Option<Json> {
        let now = self.log().clock().now_ms();
        let mut asked: Vec<(String, Miss)> = {
            let mut st = self.lookstate();
            self.due(&st, now).ok()?;
            (st.busy, st.last) = (true, Some(now));
            st.misses.iter().filter(|(k, _)| !k.starts_with("page:")).map(|(k, m)| (k.clone(), m.clone())).collect()
        };
        asked.sort_by_key(|(_, m)| std::cmp::Reverse(m.n));
        asked.truncate(MOST);
        let began = Instant::now();
        let rows = self.look_rows();
        let mut looks: Vec<(String, (&str, &str))> = Vec::new();
        for r in rows.iter().filter(|r| r.id.len() > "family/".len()) {
            if let (Some(p), Some(g), Some(c)) = (r.id.strip_prefix("family/"), r.glyph.as_deref(), r.role.as_deref()) {
                match looks.iter_mut().find(|(_, l)| *l == (g, c)) {
                    Some((ps, _)) => ps.push_str(&format!(", {p}*")),
                    None => looks.push((format!("{p}*"), (g, c))),
                }
            }
        }
        let (mut made, mut by, mut samples, mut questions) = (Vec::<LookRow>::new(), Vec::<String>::new(), BTreeMap::new(), 0);
        let mut spent = Spent::default();
        let mut put = |made: &mut Vec<LookRow>, row: LookRow, who: String| {
            by.push(who);
            made.push(row);
        };
        for (key, miss) in &asked {
            let s = miss.samples.first().cloned().unwrap_or(Json::Null);
            let shown = s.to_string();
            match key.split_once(':') {
                Some(("kind", kind)) => {
                    samples.insert(kind.to_string(), miss.samples.clone());
                    let prefix = kind.split_once('.').map_or(kind.to_string(), |(a, _)| format!("{a}."));
                    let id = format!("family/{prefix}");
                    if !made.iter().any(|r| r.id == id) {
                        let options = looks.iter().map(|(ps, (g, c))| Opt { id: format!("{g}|{c}"), label: format!("like {ps}: glyph {g}, colour {c}"), recommended: false }).collect();
                        questions += 1;
                        if let Some((pick, who)) = self.look_ask(questions, format!("Which look fits the events of kind `{kind}`? One, masked: {shown}"), options, &mut spent) {
                            let (g, c) = pick.split_once('|').unwrap_or((&pick, ""));
                            put(&mut made, LookRow { id, glyph: Some(g.into()), role: Some(c.into()), ..LookRow::default() }, who);
                        }
                    }
                    let mut options: Vec<Opt> = lines(&s).into_iter().map(|(t, l)| Opt { id: t, label: l, recommended: false }).collect();
                    options.push(Opt { id: "first".into(), label: "its first short fields as `key value`".into(), recommended: false });
                    questions += 1;
                    if let Some((pick, who)) = self.look_ask(questions, format!("Which line says best what an event of kind `{kind}` is? One, masked: {shown}"), options, &mut spent).filter(|(p, _)| p != "first") {
                        put(&mut made, LookRow { id: format!("summary/{kind}"), text: Some(pick), ..LookRow::default() }, who);
                    }
                }
                Some(("field", name)) => {
                    let value = &s["data"][name];
                    let (what, shows) = if value.is_boolean() { ("yes or no", &YESNO[..]) } else { ("number", &NUMBERS[..]) };
                    let options = shows.iter().map(|(id, l)| Opt { id: (*id).into(), label: format!("{id}: {l}"), recommended: false }).collect();
                    questions += 1;
                    if let Some((pick, who)) = self.look_ask(questions, format!("How is the {what} in the field `{name}` best shown? It was {value} in this event, masked: {shown}"), options, &mut spent) {
                        put(&mut made, LookRow { id: format!("field/{name}"), show: Some(pick), ..LookRow::default() }, who);
                    }
                }
                _ => {}
            }
        }
        let text: String = made.iter().map(|r| LookRow { doc: format!("made by {}", by.first().map_or("", String::as_str)), ..r.clone() }.toml()).collect();
        let ms = began.elapsed().as_millis() as u64;
        let missed: Vec<&String> = asked.iter().map(|(k, _)| k).collect();
        // what the asks cost, as the answerers reported it: a number, or `null` when a decider or a model did not say — unknown,
        // not nothing; the tokens as reported
        let (usd, tokens) = (json!(spent.usd), (spent.tokens_in, spent.tokens_out));
        let checked = check(&text, &samples).and_then(|rows| match &self.cfg().look_made {
            Some(path) if !rows.is_empty() => {
                // the file whole again, with the new entries after the old (only the Log appends)
                let head = "api = 1\n\n# What the look's pass made: an entry of ~/.j4n/ui/look.toml with the same id wins; delete one here to undo it.\n";
                let old = std::fs::read_to_string(path).unwrap_or_else(|_| head.into());
                path.parent().map(std::fs::create_dir_all).transpose().map_err(|e| e.to_string())?;
                std::fs::write(path, format!("{old}{text}")).map_err(|e| e.to_string())?;
                Ok(rows)
            }
            _ => Ok(rows),
        });
        let mut st = self.lookstate();
        st.busy = false;
        st.misses.retain(|k, _| !missed.contains(&k));
        st.since = (!st.misses.is_empty()).then_some(now);
        let (kind, data) = match checked {
            Ok(rows) => {
                let ids: Vec<String> = rows.iter().map(|r| r.id.clone()).collect();
                st.made.extend(rows);
                st.cover = None;
                let made_for = |k: &str| match k.split_once(':') {
                    Some(("kind", kind)) => format!("family/{}", kind.split_once('.').map_or(kind.to_string(), |(a, _)| format!("{a}."))),
                    _ => k.replacen(':', "/", 1),
                };
                let left: Vec<&String> = missed.iter().copied().filter(|k| !ids.contains(&made_for(k))).collect();
                ("look.made", json!({ "made": ids, "left": left, "asked": questions, "by": by, "ms": ms, "usd": usd, "tokens_in": tokens.0, "tokens_out": tokens.1, "file": self.cfg().look_made }))
            }
            Err(why) => ("look.refused", json!({ "why": why, "missed": missed, "asked": questions, "by": by, "ms": ms, "usd": usd, "tokens_in": tokens.0, "tokens_out": tokens.1 })),
        };
        drop(st);
        let _ = self.log().append(New::of(kind, Actor::Core, data.clone()));
        Some(data)
    }
}
