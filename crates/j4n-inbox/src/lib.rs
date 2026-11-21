//! The prompt inbox. A line typed while a task is not over is given a tag and a receipt instead of
//! steering the task at once: the tag is chosen by rules (`tag.rs`) from the operator's own words, the item and every change of it are
//! `inbox.item` events, the operator's re-tag is a `label` row, and a rule's precision is counted from the labels. The way
//! out is: what waits for the next turn (`due`: the lines as typed, numbered, and the pinned notes), the oldest `side` item (`side`), the receipts
//! (`sent`) and the claim that keeps two callers from delivering one item twice. With the switch off the front never calls this crate, so a
//! line steers as it always did.

pub mod tag;

use j4n_core::change::Actor;
use j4n_core::fold::Folds;
use j4n_core::front::inbox::{Due, Inbox, Kept, Over};
use j4n_core::log::{Log, New};
use j4n_core::setting::Settings;
use j4n_core::table::Tables;
use j4n_core::value::{Key, Value};
use serde_json::{Value as Json, json};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;
use tag::{Bounds, Rules, tag_word};

/// The line of a turn that carries the inbox: for a doer that takes a Brief (the lines are its section `inbox`), and for one that does not.
const CARRY: &str = "Lines the operator typed while you worked are in the section `inbox` of your brief, numbered, as typed. Take them in order, and say what you did with each.";
const CARRY_PLAIN: &str = "Lines the operator typed while you worked, numbered, as typed. Take them in order, and say what you did with each.";

const MARKS: [(&str, &str); 5] = [("stop", "■"), ("steer", "→"), ("side", "↳"), ("note", "¶"), ("hold", "…")];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Off,
    Show,
    Auto,
}

impl Mode {
    fn of(word: &str) -> Mode {
        match word.trim().to_ascii_lowercase().as_str() {
            "show" => Mode::Show,
            "auto" => Mode::Auto,
            _ => Mode::Off,
        }
    }

    fn word(self) -> &'static str {
        ["off", "show", "auto"][self as usize]
    }
}

#[derive(Default)]
struct Items {
    /// The number the next item takes, read from the fold the first time.
    next: Option<u64>,
    /// Each item as last written: the fold is a moment behind the Log, this is not.
    last: BTreeMap<String, Json>,
    /// Items a turn is being made for: no other caller takes them.
    claimed: BTreeSet<String>,
}

pub struct Desk {
    log: Log,
    folds: Folds,
    rules: Rules,
    /// `inbox.related`/`inbox.unrelated`, `inbox.steer_precision` and `inbox.min_labels`, as the settings say now (an edit tunes them).
    knobs: Mutex<(Bounds, f64, u64)>,
    mode: Mutex<Mode>,
    items: Mutex<Items>,
}

/// A fold's value as plain JSON (numbers, words, flags).
fn plain(v: &Value) -> Json {
    match v {
        Value::Num(n) => json!(n),
        Value::Str(s) => json!(s),
        Value::Bool(b) => json!(b),
        Value::Map(m) => Json::Object(m.iter().map(|(k, v)| (k.clone(), plain(v))).filter(|(_, v)| !v.is_null()).collect()),
        _ => Json::Null,
    }
}

fn text(item: &Json, key: &str) -> String {
    item[key].as_str().unwrap_or_default().to_string()
}

/// The numbers of the settings the desk reads.
fn knobs(settings: &Settings) -> (Bounds, f64, u64) {
    (Bounds { related: settings.num("inbox.related", 0.5), unrelated: settings.num("inbox.unrelated", 0.15) }, settings.num("inbox.steer_precision", 0.95), settings.num("inbox.min_labels", 40.0) as u64)
}

impl Desk {
    pub fn new(log: &Log, folds: &Folds, settings: &Settings, tables: &Tables) -> Desk {
        Desk {
            log: log.clone(),
            folds: folds.clone(),
            rules: Rules::load(tables),
            knobs: Mutex::new(knobs(settings)),
            mode: Mutex::new(Mode::of(settings.word("inbox.mode", "off"))),
            items: Mutex::default(),
        }
    }

    fn mode(&self) -> Mode {
        *self.mode.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn now(&self) -> u64 {
        self.log.clock().now_ms()
    }

    fn items(&self) -> std::sync::MutexGuard<'_, Items> {
        self.items.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn count(&self, fold: &str, rule: &str) -> f64 {
        match self.folds.read(fold, &[Key::Name(rule.into())], self.now()) {
            Value::Num(n) => n,
            _ => 0.0,
        }
    }

    /// A rule's labels: how many, and the share of them that said its tag was right.
    pub fn precision(&self, rule: &str) -> (f64, u64) {
        let (kept, wrong) = (self.count("inbox_kept", rule), self.count("inbox_wrong", rule));
        (if kept + wrong > 0.0 { kept / (kept + wrong) } else { 0.0 }, (kept + wrong) as u64)
    }

    fn put(&self, by: Actor, item: Json) -> bool {
        let ok = self.log.append(New::of("inbox.item", by, item.clone()).thread(&text(&item, "thread"))).is_ok();
        if ok {
            self.items().last.insert(text(&item, "item"), item);
        }
        ok
    }

    fn item(&self, id: &str) -> Option<Json> {
        if let Some(item) = self.items().last.get(id) {
            return Some(item.clone());
        }
        Some(plain(&self.folds.read("inbox", &[Key::Name(id.into())], self.now()))).filter(|i| i.is_object())
    }

    /// Every item, oldest first: the fold, and over it what was written since.
    fn all(&self) -> Vec<Json> {
        let mut all: BTreeMap<String, Json> = match self.folds.read("inbox", &[], self.now()) {
            Value::Map(m) => m.iter().map(|(k, v)| (k.clone(), plain(v))).collect(),
            _ => BTreeMap::new(),
        };
        all.extend(self.items().last.clone());
        let mut all: Vec<Json> = all.into_values().collect();
        all.sort_by_key(|i| text(i, "item").trim_start_matches('i').parse::<u64>().unwrap_or(0));
        all
    }

    fn next_id(&self) -> String {
        let mut items = self.items();
        let held = match self.folds.read("inbox", &[], self.now()) {
            Value::Map(m) => m.len() as u64,
            _ => 0,
        };
        let n = items.next.unwrap_or(held) + 1;
        items.next = Some(n);
        format!("i{n}")
    }

    fn retag(&self, id: &str, word: &str) -> Result<String, String> {
        let Some(tag) = tag_word(word).filter(|t| *t != "stop") else { return Err("a tag is steer, side, note or hold (to stop the task, say stop)".into()) };
        let Some(mut item) = self.item(id) else { return Err(format!("there is no item {id}: `:inbox` lists them")) };
        if !matches!(item["state"].as_str(), Some("open" | "kept")) {
            return Err(format!("{id} is over: {}", text(&item, "state")));
        }
        // only the first word of the operator on an item is a label of the rule that tagged it
        if item["told"].as_bool() != Some(true) {
            let label = if item["tag"] == tag { "kept" } else { "wrong" };
            let data = json!({ "subject_kind": "inbox_item", "subject": id, "producer": item["rule"], "model": "", "point": "", "label": label, "weight": 1.0 });
            let _ = self.log.append(New::of("label", Actor::Operator, data).thread(&text(&item, "thread")));
        }
        let mark = MARKS.iter().find(|(t, _)| *t == tag).map_or("", |m| m.1);
        item["tag"] = json!(tag);
        item["state"] = json!(if tag == "note" { "kept" } else { "open" });
        (item["told"], item["steerable"], item["why"], item["mark"], item["at_ms"]) = (json!(true), json!(false), json!("you said so"), json!(mark), json!(self.now()));
        if !self.put(Actor::Operator, item) {
            return Err("the Log does not take events".into());
        }
        Ok(format!("{id} is {tag} now"))
    }

    fn pin(&self, id: &str) -> Result<String, String> {
        let Some(mut item) = self.item(id) else { return Err(format!("there is no item {id}: `:inbox` lists them")) };
        if item["tag"] != "note" || item["state"] != "kept" {
            return Err(format!("{id} is not a note that is kept: only a note is pinned"));
        }
        (item["pinned"], item["at_ms"]) = (json!(true), json!(self.now()));
        if self.put(Actor::Operator, item) { Ok(format!("{id} is pinned: it goes with the next Brief")) } else { Err("the Log does not take events".into()) }
    }
}

impl Inbox for Desk {
    fn tune(&self, settings: &Settings) {
        *self.knobs.lock().unwrap_or_else(|e| e.into_inner()) = knobs(settings);
        *self.mode.lock().unwrap_or_else(|e| e.into_inner()) = Mode::of(settings.word("inbox.mode", "off"));
    }

    fn on(&self) -> bool {
        self.mode() != Mode::Off
    }

    fn take(&self, over: &Over) -> Option<Kept> {
        let (bounds, floor, min_labels) = *self.knobs.lock().unwrap_or_else(|e| e.into_inner());
        let t = self.rules.tag(over.text, &over.context, bounds);
        // a task that waits on an ask is waiting for its answer, not for a line to be filed: it refuses as before
        if over.state == "waiting" && t.tag != "stop" {
            return None;
        }
        let (prec, n) = self.precision(&t.rule);
        let steerable = t.tag == "steer" && n >= min_labels && prec >= floor;
        let (state, receipt) = match t.tag {
            "stop" => ("stopping", "stopping the task"),
            "note" => ("kept", "kept, and sent to no model unless you pin it"),
            "steer" => ("open", "waits for the next boundary of the task"),
            "side" => ("open", "waits to be a task of its own after this one settles"),
            _ => ("open", "held: it goes as the next turn"),
        };
        let mark = MARKS.iter().find(|(tag, _)| *tag == t.tag).map_or("", |m| m.1);
        let id = self.next_id();
        let item = json!({ "item": id, "thread": over.thread, "text": over.text, "tag": t.tag, "rule": t.rule, "why": t.why, "state": state, "task": over.task, "over": over.state, "steerable": steerable, "prec": prec, "n": n, "pinned": false, "mark": mark, "at_ms": self.now() });
        if !self.put(Actor::Core, item) {
            return None;
        }
        let keys = if t.tag == "stop" { String::new() } else { " · s steer · x side · n note · h hold".into() };
        Some(Kept { note: format!("{id} {mark} {} — {receipt}: {}{keys}", t.tag, t.why), id, tag: t.tag, now: steerable && self.mode() == Mode::Auto })
    }

    fn settled(&self, id: &str, state: &str) {
        if let Some(mut item) = self.item(id) {
            (item["state"], item["at_ms"]) = (json!(state), json!(self.now()));
            self.put(Actor::Core, item);
        }
    }

    fn due(&self, thread: &str, work: bool) -> Option<Due> {
        let all = self.all();
        let mut items = self.items();
        let mine: Vec<&Json> = all.iter().filter(|i| text(i, "thread") == thread && !items.claimed.contains(&text(i, "item"))).collect();
        let says: Vec<&Json> = mine.iter().copied().filter(|i| i["state"] == "open" && matches!(i["tag"].as_str(), Some("steer" | "hold"))).collect();
        let pins: Vec<&Json> = mine.iter().copied().filter(|i| i["tag"] == "note" && i["state"] == "kept" && i["pinned"] == true).collect();
        if says.is_empty() && (work || pins.is_empty()) {
            return None;
        }
        let mut lines: Vec<String> = says.iter().enumerate().map(|(n, i)| format!("{}. {}", n + 1, text(i, "text"))).collect();
        if !pins.is_empty() {
            lines.push("Notes the operator pinned for you:".into());
            lines.extend(pins.iter().enumerate().map(|(n, i)| format!("{}. {}", says.len() + n + 1, text(i, "text"))));
        }
        let ids: Vec<String> = says.iter().chain(pins.iter()).map(|i| text(i, "item")).collect();
        let text = lines.join("\n");
        items.claimed.extend(ids.iter().cloned());
        Some(Due { ids, line: CARRY.into(), plain: format!("{CARRY_PLAIN}\n\n{text}"), text })
    }

    fn side(&self, thread: &str) -> Option<(String, String)> {
        let all = self.all();
        let mut items = self.items();
        let next = all.iter().find(|i| text(i, "thread") == thread && i["state"] == "open" && i["tag"] == "side" && !items.claimed.contains(&text(i, "item")))?;
        items.claimed.insert(text(next, "item"));
        Some((text(next, "item"), text(next, "text")))
    }

    fn sent(&self, ids: &[String], state: &str, task: &str) {
        for id in ids {
            if let Some(mut item) = self.item(id) {
                (item["state"], item["turn"], item["at_ms"]) = (json!(state), json!(task), json!(self.now()));
                self.put(Actor::Core, item);
            }
        }
        self.back(ids);
    }

    fn back(&self, ids: &[String]) {
        self.items().claimed.retain(|c| !ids.contains(c));
    }

    fn command(&self, _thread: &str, args: &[String]) -> Result<String, String> {
        match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
            ["off" | "show" | "auto"] => {
                *self.mode.lock().unwrap_or_else(|e| e.into_inner()) = Mode::of(&args[0]);
                Ok(format!("inbox is {} until J4n ends or the setting `inbox.mode` is changed; the setting is what it starts as", args[0]))
            }
            ["tag", id, word] => self.retag(id, word),
            ["pin", id] => self.pin(id),
            [] => {
                let mode = *self.mode.lock().unwrap_or_else(|e| e.into_inner());
                let open: Vec<String> = self.all().iter().filter(|i| matches!(i["state"].as_str(), Some("open" | "kept"))).map(|i| format!("{} {} {} {}{} “{}”", text(i, "item"), text(i, "mark"), text(i, "tag"), text(i, "state"), if i["pinned"] == true { " pinned" } else { "" }, text(i, "text"))).collect();
                Ok(format!("inbox {} — {}", mode.word(), if open.is_empty() { "no line is waiting".to_string() } else { open.join("\n") }))
            }
            _ => Err("`:inbox`, `:inbox tag <id> <steer|side|note|hold>`, `:inbox pin <id>`, `:inbox show|auto|off`".into()),
        }
    }
}
