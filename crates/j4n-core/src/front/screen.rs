//! The screen as data: the pages, the views they draw, the commands, the packs that are on, and the collections a view reads
//! A front draws what it is given here; a pack that adds a page, a view or a command changes the screen
//! without a change to the core or the TUI.
//!
//! The base pack's own (`packs/base/{pages,views,commands}/`) are the screen a front has when nobody gave it another
//! ([`Screen::builtin`]).

use super::command::Commands;
use super::page::Pages;
use crate::cost::Show;
use crate::pack::Info;
use crate::setting::Settings;
use crate::table::{Layer, LayerKind, Problem, SourceFile, Tables};
use crate::task::Row as Task;
use crate::value::Value;
use crate::view::Views;
use j4n_proto::{Cell, Json, Node, Row, Tone};
use std::collections::BTreeMap;

#[derive(Clone)]
pub struct Screen {
    pub pages: std::sync::Arc<Pages>,
    pub views: std::sync::Arc<Views>,
    pub commands: Commands,
    pub packs: Vec<Info>,
    /// The Table as it was when the screen was made: `table:<kind>` reads it.
    pub tables: Tables,
    /// The settings as the tree `set` reads: what they are now (an edit makes a new screen with them).
    pub set: Value,
    /// The settings as J4n started with them: a row that waits for the next start is one whose value is not this.
    pub start: Value,
    /// The folds of the Log: `fold:<id>` reads the ones a pack declares, a row for each key.
    pub folds: Option<crate::fold::Folds>,
    /// What the keys do, as the screen is told (`front::keys`).
    pub keys: Vec<j4n_proto::Bind>,
    /// The hints of the keys row, in the order they stand (`keys:row`).
    pub hints: Vec<Row>,
}

impl Screen {
    /// The screen the Table describes. `tags` says, by the name of a pack's layer, what to put beside what the pack adds.
    pub fn load(tables: Tables, settings: &Settings, packs: Vec<Info>, tags: &BTreeMap<String, String>) -> (Screen, Vec<Problem>) {
        let (mut pages, mut found) = Pages::load(&tables, tags);
        let (views, more) = Views::load(&tables);
        found.extend(more);
        let (mut commands, more) = Commands::load(&tables, tags);
        found.extend(more);
        found.extend(pages.lint(&|v| views.get(v).is_some()));
        found.extend(commands.retain_pages(&|p| pages.has(p)));
        let (keys, more) = super::keys::compile(&tables, &|p| pages.has(p));
        found.extend(more);
        let (hints, more) = super::hints::rows(&tables);
        found.extend(more);
        found.extend(packs.iter().flat_map(|p| p.ready.iter().flat_map(|r| r.problems(&p.id, &tables))));
        let set = settings.tree();
        (Screen { pages: pages.into(), views: views.into(), commands, packs, tables, start: set.clone(), set, folds: None, keys, hints }, found)
    }

    /// The base pack's screen, with the settings it declares.
    pub fn builtin() -> Screen {
        const FILES: [(&str, &str); 9] = [
            ("keys/main.toml", include_str!("../../../../packs/base/keys/main.toml")),
            ("keys/hints.toml", include_str!("../../../../packs/base/keys/hints.toml")),
            ("look/look.toml", include_str!("../../../../packs/base/look/look.toml")),
            ("settings/look.toml", include_str!("../../../../packs/base/settings/look.toml")),
            ("pages/main.toml", include_str!("../../../../packs/base/pages/main.toml")),
            ("templates/main.toml", include_str!("../../../../packs/base/templates/main.toml")),
            ("views/f1.toml", include_str!("../../../../packs/base/views/f1.toml")),
            ("views/main.toml", include_str!("../../../../packs/base/views/main.toml")),
            ("commands/main.toml", include_str!("../../../../packs/base/commands/main.toml")),
        ];
        let files = FILES.iter().map(|(path, text)| SourceFile { path: (*path).into(), text: (*text).into() }).collect();
        let (tables, _) = Tables::load(vec![Layer::new(LayerKind::Pack, "base", files)]);
        let (settings, _) = Settings::load(&tables);
        Screen::load(tables, &settings, Vec::new(), &BTreeMap::new()).0
    }

    /// The same screen with the settings and the Table as they are after an edit: the pages, views and commands stay, and so does
    /// what J4n started with.
    pub fn with_settings(&self, settings: &crate::setting::Settings, tables: Tables) -> Screen {
        Screen { set: settings.tree(), tables, ..self.clone() }
    }

    /// The tree with the titles of the view each list draws in the props of its field (`titles`), unless the page gave its own: the
    /// screen draws a header over the columns.
    pub fn titled(&self, mut node: Node) -> Node {
        if let Some(f) = node.field.as_mut()
            && let Some(view) = f.bind.strip_prefix("view:").and_then(|id| self.views.get(id))
            && !view.titles().is_empty()
            && f.props.get("titles").is_none()
        {
            if !f.props.is_object() {
                f.props = Json::Object(Default::default());
            }
            f.props["titles"] = Json::from(view.titles().to_vec());
        }
        node.children = node.children.into_iter().map(|c| self.titled(c)).collect();
        node
    }

    /// The look the settings name (`theme.*`), a word the pack did not declare left to the default of the design.
    pub fn look(&self) -> j4n_proto::Look {
        let word = |id: &str, or: &str| match self.set.pointer(&format!("/theme/{id}")) {
            Value::Str(s) if !s.is_empty() => s,
            _ => or.to_string(),
        };
        let motion = !matches!(self.set.pointer("/theme/motion"), Value::Bool(false));
        let read_word = |id: &str, or: &str| match self.set.pointer(&format!("/read/{id}")) {
            Value::Str(s) if !s.is_empty() => s,
            _ => or.to_string(),
        };
        let read_num = |id: &str, or: u32| match self.set.pointer(&format!("/read/{id}")) {
            Value::Num(n) if n >= 0.0 => n as u32,
            _ => or,
        };
        j4n_proto::Look {
            palette: word("palette", "jjj"),
            marks: word("marks", super::fonts::AUTO),
            frame: word("frame", "round"),
            corners: word("corners", "round"),
            title_place: word("title_place", "inside"),
            density: word("density", "normal"),
            fill: word("fill", "match"),
            you: word("you", "bracket"),
            motion,
            read: j4n_proto::Read {
                style: read_word("style", "calm"),
                width: read_num("width", 76),
                fold_code: read_num("fold_code", 12),
                links: read_word("links", "osc8"),
                big_heads: matches!(self.set.pointer("/read/big_heads"), Value::Bool(true)),
                terms: !matches!(self.set.pointer("/read/terms"), Value::Bool(false)),
            },
            mouse: word("mouse", "on"),
            time: match self.set.pointer("/look/time") {
                Value::Str(s) if s == "clock" => s,
                _ => "ago".to_string(),
            },
            chart: match self.set.pointer("/look/chart") {
                Value::Str(s) if s == "plain" => s,
                _ => "dither".to_string(),
            },
        }
    }

    /// The rows of a declared fold: one for each key, the key as its `id` and, if what the fold kept for it is a map, its fields.
    pub fn fold_rows(&self, id: &str, now_ms: u64) -> Vec<Value> {
        let Some(Value::Map(keys)) = self.folds.as_ref().map(|f| f.read(id, &[], now_ms)) else { return Vec::new() };
        keys.into_iter()
            .map(|(key, kept)| {
                let mut row = match kept {
                    Value::Map(fields) => fields,
                    other => BTreeMap::from([("value".to_string(), other)]),
                };
                row.insert("id".into(), Value::text(&key));
                Value::Map(row)
            })
            .collect()
    }

    /// Which of the plan cost and the list cost is said (`cost.show`).
    pub fn show(&self) -> Show {
        match self.set.pointer("/cost/show") {
            Value::Str(w) => Show::word(&w),
            _ => Show::default(),
        }
    }

    /// The rows of the Usage tab: what each task cost at list price (`value`) and, for a task that ran on a plan, what it took of the
    /// plan, said as `cost.show` says in `money`.
    pub fn spend_rows(&self, now_ms: u64) -> Vec<Value> {
        let show = self.show();
        let plan: BTreeMap<String, i64> = self.fold_rows("spend_plan", now_ms)
            .into_iter()
            .filter_map(|r| match r {
                Value::Map(m) => match (m.get("id"), m.get("value")) {
                    (Some(Value::Str(id)), Some(Value::Usd(v))) => Some((id.clone(), *v)),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        let usd = |m: i64| format!("${:.4}", m as f64 / 1e6);
        let mut rows = self.fold_rows("spend", now_ms);
        for row in &mut rows {
            let Value::Map(m) = row else { continue };
            let (Some(Value::Str(id)), Some(Value::Usd(list))) = (m.get("id").cloned(), m.get("value").cloned()) else { continue };
            let money = match plan.get(&id) {
                Some(p) => show.money(true, &usd(*p), None, &usd(list)),
                None => usd(list),
            };
            m.insert("money".into(), Value::text(&money));
        }
        rows
    }

    /// The settings as rows of a view: the entries of the Table with `now`, what stands (the layer's `value`, else the default);
    /// `next_start`, whether an edit of it waits for the next start (`live = false`); and `waits`, whether one does now: what stands
    /// is not what J4n started with.
    pub fn setting_rows(&self) -> Vec<Value> {
        let mut rows = self.tables.rows("setting");
        for row in &mut rows {
            if let Value::Map(m) = row {
                let Some(Value::Str(id)) = m.get("id").cloned() else { continue };
                let at = format!("/{}", id.replace('.', "/"));
                let (mut now, mut start) = (self.set.pointer(&at), self.start.pointer(&at));
                if let Some(word) = auto_word(m) {
                    (now, start) = (word.clone(), word);
                }
                let next_start = matches!(m.get("live"), Some(Value::Bool(false)));
                m.insert("next_start".into(), Value::Bool(next_start));
                m.insert("waits".into(), Value::Bool(next_start && now != start));
                m.insert("flip".into(), Value::text(flip(m, &now)));
                m.insert("edit".into(), edit_of(m, &id, &now, &start, next_start));
                // `full` when what stands is what `packs.full` put there and the operator has not said otherwise
                let via = self.tables.raw("setting", &id).and_then(|r| r.from.get("value")).is_some_and(|l| l == crate::pack::FULL_LAYER);
                m.insert("via".into(), Value::text(if via { "full" } else { "" }));
                // the one line Settings › Features says, else the doc
                if !m.contains_key("brief") {
                    let doc = m.get("doc").cloned().unwrap_or_else(|| Value::text(""));
                    m.insert("brief".into(), doc);
                }
                // the marks' `auto` says what it comes to on this machine, and why
                let auto = id == "theme.marks" && now == Value::text(super::fonts::AUTO);
                m.insert("now".into(), if auto { Value::text(super::fonts::row_word(super::fonts::AUTO)) } else { now });
            }
        }
        rows
    }

    /// The settings that were changed since the start and wait for the next one to act: what the restart card names.
    pub fn waiting(&self) -> Vec<String> {
        self.setting_rows()
            .into_iter()
            .filter_map(|r| match r {
                Value::Map(m) if matches!(m.get("waits"), Some(Value::Bool(true))) => match m.get("id") {
                    Some(Value::Str(id)) => Some(id.clone()),
                    _ => None,
                },
                _ => None,
            })
            .collect()
    }

    /// `commands:all`: the palette.
    pub fn palette(&self) -> Vec<Row> {
        self.commands.rows()
    }

    /// `keys:row`: the hints of the keys row.
    pub fn hint_rows(&self) -> Vec<Row> {
        self.hints.clone()
    }

    /// Whether a setting holds `v` as the layers say it (the layer's `value`, else the default).
    pub fn setting_is(&self, id: &str, v: &toml::Value) -> bool {
        self.tables.raw("setting", id).is_some_and(|m| m.fields.get("value").or_else(|| m.fields.get("default")) == Some(v))
    }

    /// The ready-to-go switch of a pack as its row shows it: for a pack that is on, whether every setting of its `[ready]` holds; for one that is off, what
    /// enabling it will do (the enable row's `ready`, else the manifest's own default). None: the pack has no `[ready]`.
    fn pack_ready(&self, p: &Info, said_on: bool) -> Option<bool> {
        let r = p.ready.as_ref()?;
        Some(if said_on { r.set.iter().all(|(id, v)| self.setting_is(id, &v.value())) } else { self.tables.raw("enable", &p.id).and_then(|m| m.fields.get("ready")).and_then(toml::Value::as_bool).unwrap_or(r.default) })
    }

    /// Whether the crew's ready-to-go switch is on, as its row on the Packs page says it; none when the crew has no `[ready]`.
    pub fn crew_ready(&self) -> Option<bool> {
        self.packs.iter().find(|p| p.id == "crew").and_then(|p| self.pack_ready(p, p.on))
    }

    /// What the one switch (`lanes`) turns on as the row says it, `is_on` being the look, the crew and the envoys as they stand now: the needs that are unmet,
    /// what is on, what is off, and the ready switch of the crew where the crew has one.
    pub fn switch_words(&self, unmet: usize, is_on: [bool; 3]) -> String {
        // the needs first (a narrow row cuts from the right), then what is on, then what is off, then the crew's ready switch: `1 unmet · look crew · envoys off · crew ready`
        let all = ["look", "crew", "envoys"].into_iter().zip(is_on);
        let mut said = vec![if unmet == 0 { "needs met".to_string() } else { format!("{unmet} unmet") }];
        let on: Vec<&str> = all.clone().filter(|(_, on)| *on).map(|(n, _)| n).collect();
        said.extend(Some(on.join(" ")).filter(|w| !w.is_empty()));
        said.extend(all.filter(|(_, on)| !*on).map(|(n, _)| format!("{n} off")));
        if let Some(r) = self.crew_ready() {
            said.push(if r { "crew ready" } else { "crew ready off" }.to_string());
        }
        said.join(" · ")
    }

    /// `packs:all`: the packs there are, those that are on first, and why one that was asked for is not.
    pub fn pack_rows(&self) -> Vec<Row> {
        self.pack_rows_with(&BTreeMap::new())
    }

    /// The same, with what each pack lacks (`unmet`: a pack's id and the words of each need that is unmet): the row says how many, and its detail which.
    pub fn pack_rows_with(&self, unmet: &BTreeMap<String, Vec<String>>) -> Vec<Row> {
        // the stability most packs have: said in a row only where a pack differs
        let common = self.packs.iter().map(|p| p.tag).max_by_key(|t| self.packs.iter().filter(|p| p.tag == *t).count()).unwrap_or_default();
        self.packs
            .iter()
            .map(|p| {
                // what the files say now: a switch said while J4n runs acts from the next start, and the row says so (⟳)
                let said = self.tables.raw("enable", &p.id).and_then(|m| m.fields.get("on")).and_then(toml::Value::as_bool).filter(|_| p.note.is_empty()).unwrap_or(p.on);
                let (state, tone) = match (p.on, p.note.is_empty()) {
                    _ if said != p.on => (if said { "on ⟳" } else { "off ⟳" }, Tone::Warn),
                    (true, _) => ("on", Tone::Good),
                    (false, true) => ("off", Tone::Dim),
                    (false, false) => ("left out", Tone::Bad),
                };
                // what the pack lacks is said where the eye is, in its state, so a narrow screen that cuts the later columns keeps it
                let lacks = unmet.get(&p.id).map(Vec::as_slice).unwrap_or_default();
                let (state, state_tone) = if lacks.is_empty() { (state.to_string(), tone) } else { (format!("{state} · {} unmet", lacks.len()), Tone::Warn) };
                let ready = self.pack_ready(p, said);
                // the marks of a pack in one short column before what it adds (which is cut first on a narrow screen): its stability where it is not the one most
                // packs have (the detail says it for each), that it widens what J4n may do, and that it is ready to go
                let marks: Vec<&str> = [(!p.tag.is_empty() && p.tag != common).then_some(p.tag), p.widens.then_some("widens"), (ready == Some(true)).then_some("ready")].into_iter().flatten().collect();
                let marks_tone = if p.widens || marks.contains(&p.tag) { Tone::Warn } else { Tone::Good };
                let cells = vec![Cell::new(p.id.clone(), Tone::Plain), Cell::new(state, state_tone), Cell::new(p.version.clone(), Tone::Dim), Cell::new(marks.join(" · "), marks_tone), Cell::new(p.adds.clone(), Tone::Dim)];
                let mut detail = vec![Cell::new(p.summary.clone(), Tone::Dim)];
                if !p.tag.is_empty() {
                    detail.push(Cell::new(p.tag, Tone::Dim));
                }
                if !lacks.is_empty() {
                    detail.extend(lacks.iter().map(|w| Cell::new(format!("needs {w}"), Tone::Warn)));
                }
                if let (Some(on), Some(r)) = (ready, &p.ready) {
                    let sets: Vec<String> = r.set.iter().map(|(k, v)| format!("{k} = {}", match v.value() { toml::Value::String(s) => s, other => other.to_string() })).collect();
                    detail.push(Cell::new(format!("ready to go is {}: {} {} · g turns it {}", if on { "on" } else { "off" }, if said { "it sets" } else { "enabling it sets" }, sets.join(", "), if on { "off" } else { "on" }), Tone::Dim));
                }
                if !p.note.is_empty() {
                    detail.push(Cell::new(p.note.clone(), Tone::Bad));
                }
                // `⏎` or `space` turns a pack on or off (where a refusal of an off pack leads); `base` is never off, one left out says why
                // one that widens what J4n may do asks on the card, which names it, and its yes is the whole of it (no Grow row)
                let word = if said { "off" } else { "on" };
                let widens = !said && p.widens;
                let mut args = vec![word.to_string(), p.id.clone()];
                let (label, risk) = if widens {
                    args.push("yes".into());
                    (format!("turn on, which widens what J4n may do: {},", p.adds), 60)
                } else {
                    (format!("turn {word}"), 0)
                };
                let flip = |key: &str| j4n_proto::Act { key: key.into(), label: label.clone(), intent: j4n_proto::Intent::Run { command: "pack".into(), args: args.clone() }, risk };
                let mut actions = if p.id == "base" || !p.note.is_empty() { Vec::new() } else { vec![flip("enter"), flip(" ")] };
                if let Some(on) = ready.filter(|_| !p.id.is_empty() && p.note.is_empty()) {
                    let to = if on { "off" } else { "on" };
                    actions.push(j4n_proto::Act { key: "g".into(), label: format!("ready to go {to}"), intent: j4n_proto::Intent::Run { command: "pack".into(), args: vec!["ready".into(), to.into(), p.id.clone()] }, risk: 0 });
                }
                if said != p.on {
                    actions.push(j4n_proto::Act { key: "r".into(), label: "restart now".into(), intent: j4n_proto::Intent::Run { command: "restart".into(), args: Vec::new() }, risk: 0 });
                }
                // the screen brings up the restart card when the flip leaves the pack other than it was at the start
                let data = serde_json::json!({ "restart_after": !said != p.on });
                Row { key: p.id.clone(), cells, tone, detail, actions, parent: None, data: Some(data), ..Row::default() }
            })
            .collect()
    }

    /// The packs as rows a view can read (`fold:packs`).
    pub fn pack_values(&self) -> Vec<Value> {
        self.packs
            .iter()
            .map(|p| {
                let s = |t: &str| Value::text(t);
                Value::Map(BTreeMap::from([
                    ("id".to_string(), s(&p.id)),
                    ("version".into(), s(&p.version)),
                    ("summary".into(), s(&p.summary)),
                    ("state".into(), s(if p.on { "on" } else if p.note.is_empty() { "off" } else { "left out" })),
                    ("note".into(), s(&p.note)),
                    ("adds".into(), s(&p.adds)),
                    ("tag".into(), s(p.tag)),
                    ("widens".into(), Value::Bool(p.widens)),
                ]))
            })
            .collect()
    }
}

/// A task as a row a view reads (`tasks`).
pub fn task_value(t: &Task) -> Value {
    let s = |t: &str| Value::text(t);
    Value::Map(BTreeMap::from([
        ("id".to_string(), s(&t.id)),
        ("state".into(), s(t.state.word())),
        ("doer".into(), s(&t.spec.doer)),
        ("model".into(), s(&crate::task::model_word(&t.spec.doer, &t.model, t.spec.tune.model.as_deref().unwrap_or_default()))),
        ("goal".into(), s(&t.spec.goal)),
        ("thread".into(), s(t.spec.thread.as_deref().unwrap_or_default())),
        ("spent".into(), Value::Usd(t.spent)),
        ("reason".into(), s(t.reason.as_deref().unwrap_or_default())),
        ("answer".into(), s(t.answer.as_deref().unwrap_or_default())),
        ("capped".into(), Value::Bool(t.capped)),
        ("over".into(), Value::Bool(t.state.is_terminal())),
    ]))
}

/// The words a setting of words may hold (`words`), in order.
fn words_of(m: &BTreeMap<String, Value>) -> Vec<String> {
    match m.get("words") {
        Some(Value::List(l)) => l.iter().filter_map(|w| if let Value::Str(s) = w { Some(s.clone()) } else { None }).collect(),
        _ => Vec::new(),
    }
}

/// The word a row with `auto` holds (`auto`, `on` or `off`): the layer's `value`, else the default, a `true` or `false` of an older file as `on` or `off`. The
/// settings hold the switch it comes to; the row shows the word, so the operator sees `auto` and what it follows.
fn auto_word(m: &BTreeMap<String, Value>) -> Option<Value> {
    m.get("auto")?;
    match m.get("value").or_else(|| m.get("default"))? {
        Value::Bool(b) => Some(Value::text(if *b { "on" } else { "off" })),
        w @ Value::Str(_) => Some(w.clone()),
        _ => None,
    }
}

/// What `space` makes of a setting: a switch flipped, a word the next of its words; else what stands.
fn flip(m: &BTreeMap<String, Value>, now: &Value) -> String {
    let words = words_of(m);
    match now {
        Value::Bool(b) => (!b).to_string(),
        Value::Str(w) if !words.is_empty() => words.iter().position(|x| x == w).map_or_else(|| words[0].clone(), |i| words[(i + 1) % words.len()].clone()),
        other => crate::fmt::format_value(other, None),
    }
}

/// What the row's editor needs: the field's kind (`bool`, `enum` for words, `int`, `num`, `usd` for a number in dollars,
/// `dur`, `str`), its bind, the value now and at the start, its words and its bounds, and whether an edit waits for the next start.
fn edit_of(m: &BTreeMap<String, Value>, id: &str, now: &Value, start: &Value, next_start: bool) -> Value {
    let words = words_of(m);
    let ty = match m.get("ty") {
        Some(Value::Str(t)) => t.as_str(),
        _ => "str",
    };
    let kind = match ty {
        "bool" => "bool",
        "str" if !words.is_empty() => "enum",
        "num" if id.split(['.', '_']).any(|p| p == "usd") => "usd",
        "int" | "num" | "dur" => ty,
        _ => "str",
    };
    let mut out = BTreeMap::from([
        ("kind".to_string(), Value::text(kind)),
        ("bind".to_string(), Value::text(format!("setting:{id}.value"))),
        ("value".to_string(), now.clone()),
        ("start".to_string(), start.clone()),
        ("next_start".to_string(), Value::Bool(next_start)),
        ("waits".to_string(), Value::Bool(next_start && now != start)),
    ]);
    if !words.is_empty() {
        out.insert("options".into(), Value::List(words.into_iter().map(Value::Str).collect()));
    }
    for bound in ["min", "max"] {
        if let Some(v @ Value::Num(_)) = m.get(bound) {
            out.insert(bound.into(), v.clone());
        }
    }
    Value::Map(out)
}
