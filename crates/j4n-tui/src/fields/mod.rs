//! The value fields: one component for each type of value — a string, a number, a switch, a choice, a list,
//! a duration, a secret. A field is drawn in one of four states (at rest, focused, being edited, disabled), and with a message under
//! it when its value is not a good one. What it shows is a function of the value, the props the core gave and the state, so the
//! board of every type in every state is a table of calls (`show::rows`), and a key is a function of the state and the key (`edit`).
//!
//! The value is the core's: a change is sent as `Set { kind, id, field, value }`, the screen takes it as the new value at once, and the
//! core's own `Value` for the binding replaces it. The bind is `<kind>:<id>.<field>`.

mod edit;
pub mod parse;
mod show;

pub use edit::{Ed, How};
pub(crate) use edit::{NO_ROAD, named};

use crate::app::App;
use j4n_proto::{Field, FieldKind, Intent, Json, Node, Up};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::widgets::Paragraph;

static NULL: Json = Json::Null;

/// How a field is shown now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Idle,
    /// It has the keys.
    Focused,
    /// A value is being typed, or a list picked from.
    Editing,
    /// The Table says this one may not be changed (`props.disabled`).
    Disabled,
}

/// A field and what it was given: its kind, its label and its props.
#[derive(Clone, Copy)]
pub struct Spec<'a> {
    pub kind: FieldKind,
    pub label: &'a str,
    pub bind: &'a str,
    props: &'a Json,
}

impl<'a> Spec<'a> {
    pub fn new(label: &'a str, f: &'a Field) -> Spec<'a> {
        Spec { kind: f.kind, label, bind: &f.bind, props: &f.props }
    }

    pub fn of(n: &'a Node) -> Option<Spec<'a>> {
        n.field.as_ref().filter(|f| f.kind.is_value()).map(|f| Spec::new(&n.title, f))
    }

    pub fn get(&self, key: &str) -> &'a Json {
        self.props.get(key).unwrap_or(&NULL)
    }

    pub fn text(&self, key: &str) -> Option<&'a str> {
        self.get(key).as_str()
    }

    pub fn num(&self, key: &str) -> Option<f64> {
        self.get(key).as_f64()
    }

    pub fn flag(&self, key: &str) -> bool {
        self.get(key).as_bool().unwrap_or(false)
    }

    /// A list of words (`options`), each a string.
    pub fn words(&self, key: &str) -> Vec<&'a str> {
        self.get(key).as_array().map(|a| a.iter().filter_map(Json::as_str).collect()).unwrap_or_default()
    }

    /// The Table says this one may not be changed: `disabled` is `true`, or the reason in words.
    pub fn disabled(&self) -> bool {
        match self.get("disabled") {
            Json::Bool(b) => *b,
            Json::String(s) => !s.is_empty(),
            _ => false,
        }
    }

    /// What is wrong with the value, in the core's words.
    pub fn told(&self) -> Option<&'a str> {
        self.text("error").filter(|s| !s.is_empty())
    }
}

/// The four ways a choice is offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Radio,
    Segmented,
    Dropdown,
}

impl Spec<'_> {
    /// `style` as the props say, or by how many options there are: up to 3 segmented, up to 7 a radio list, more a dropdown.
    pub fn style(&self) -> Style {
        match self.text("style") {
            Some("radio") => Style::Radio,
            Some("segmented") => Style::Segmented,
            Some("dropdown") => Style::Dropdown,
            _ => match self.words("options").len() {
                0..=3 => Style::Segmented,
                4..=7 => Style::Radio,
                _ => Style::Dropdown,
            },
        }
    }
}

/// A row of a list being changed in place with the editor of its kind: the list's bind, the row's key, the field it is
/// edited as and the label it is drawn with. The edit itself is the app's `val`.
#[derive(Debug, Clone)]
pub struct RowEd {
    pub list: String,
    pub key: String,
    pub field: Field,
}

/// What a row of a list carries to be edited (`data.edit`, sent by the core from the setting's type): the field, its value now, and
/// the whole of `edit` (`start`, `next_start`, for the restart card).
pub fn row_field(row: &j4n_proto::Row) -> Option<(Field, Json, Json)> {
    let e = row.data.as_ref()?.get("edit")?;
    let kind = match e.get("kind")?.as_str()? {
        "bool" => FieldKind::Bool,
        "enum" => FieldKind::Enum,
        "int" => FieldKind::Int,
        "num" => FieldKind::Num,
        "usd" => FieldKind::Usd,
        "dur" => FieldKind::Dur,
        // a provider's key: typed masked, sent once, never kept on the screen
        "secret" => FieldKind::Secret,
        _ => FieldKind::Str,
    };
    let props = ["options", "min", "max"].into_iter().filter_map(|k| e.get(k).map(|v| (k.to_string(), v.clone()))).collect();
    let field = Field { kind, bind: e.get("bind")?.as_str()?.to_string(), props: Json::Object(props) };
    Some((field, e.get("value").cloned().unwrap_or(Json::Null), e.clone()))
}

/// The text a value is edited as.
pub fn text_of(value: &Json) -> String {
    match value {
        Json::String(s) => s.clone(),
        Json::Number(n) => n.as_f64().map(parse::num).unwrap_or_default(),
        Json::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

/// The strings of a list value.
pub fn strings(value: &Json) -> Vec<String> {
    value.as_array().map(|a| a.iter().filter_map(Json::as_str).map(String::from).collect()).unwrap_or_default()
}

/// The pairs of a map value, in the order the names sort.
pub fn pairs(value: &Json) -> Vec<(String, String)> {
    value.as_object().map(|o| o.iter().map(|(k, v)| (k.clone(), text_of(v))).collect()).unwrap_or_default()
}

/// The text typed, read as a value of the kind, or what is wrong with it.
pub fn draft(spec: &Spec, text: &str, items: &[String]) -> Result<Json, String> {
    let n = text.chars().count();
    let req = spec.flag("required");
    let range = |x: f64| {
        let (lo, hi) = (spec.num("min"), spec.num("max"));
        if lo.is_some_and(|l| x < l) || hi.is_some_and(|h| x > h) { Err(parse::range(lo, hi)) } else { Ok(()) }
    };
    match spec.kind {
        FieldKind::Str | FieldKind::Path => {
            let max = spec.num("max").map(|m| m as usize);
            if req && text.trim().is_empty() {
                Err("needed".into())
            } else if max.is_some_and(|m| n > m) {
                Err(format!("≤ {} chars", max.unwrap_or(0)))
            } else {
                Ok(Json::String(text.trim().into()))
            }
        }
        FieldKind::Text => {
            let max = spec.num("max").map_or(240, |m| m as usize);
            if req && text.trim().is_empty() {
                Err("needed".into())
            } else if n > max {
                Err(format!("≤ {max} chars"))
            } else {
                Ok(Json::String(text.into()))
            }
        }
        FieldKind::Markdown => if req && text.trim().is_empty() { Err("empty".into()) } else { Ok(Json::String(text.into())) },
        FieldKind::Int => {
            let x: i64 = text.trim().parse().map_err(|_| "a whole number".to_string())?;
            range(x as f64)?;
            Ok(Json::from(x))
        }
        FieldKind::Num => {
            let x: f64 = text.trim().parse().ok().filter(|x: &f64| x.is_finite()).ok_or("a number")?;
            range(x)?;
            Ok(Json::from(x))
        }
        FieldKind::Dur => parse::dur(text).map(|_| Json::String(text.trim().into())).ok_or_else(|| "like 90s · 5m · 1h".into()),
        FieldKind::Usd => {
            let x = parse::usd(text).ok_or("like $5 or 2.50")?;
            if x < 0.0 { Err("≥ $0".into()) } else { Ok(Json::from(x)) }
        }
        FieldKind::Pattern => parse::glob(text).map_or_else(|| Ok(Json::String(text.trim().into())), Err),
        FieldKind::Expr => parse::expr(text).map_or_else(|| Ok(Json::String(text.trim().into())), Err),
        // what a key would be is never made into a value: that it is there is all
        FieldKind::Secret => if text.trim().is_empty() { Err("nothing typed".into()) } else { Ok(Json::Bool(true)) },
        FieldKind::Ref => {
            let options = spec.words("options");
            let t = text.trim();
            if options.is_empty() || options.contains(&t) { Ok(Json::String(t.into())) } else { Err("not found".into()) }
        }
        FieldKind::Argv => {
            let mut all: Vec<String> = items.to_vec();
            if !text.is_empty() {
                all.push(text.into());
            }
            let known = spec.words("args");
            if !known.is_empty()
                && let Some(h) = all.iter().flat_map(|a| parse::holes(a)).find(|h| !known.contains(&h.as_str()))
            {
                return Err(format!("no arg \"{h}\""));
            }
            Ok(Json::from(all))
        }
        _ => Err("not a typed value".into()),
    }
}

/// What is wrong with the value as it stands, or `None`: the core's words first, then what the type can tell by itself.
pub fn problem(spec: &Spec, value: &Json) -> Option<String> {
    if let Some(t) = spec.told() {
        return Some(t.into());
    }
    let options = spec.words("options");
    match spec.kind {
        FieldKind::Enum => {
            let v = value.as_str().unwrap_or_default();
            (!options.is_empty() && !options.contains(&v)).then(|| "not offered".into())
        }
        FieldKind::Multi => {
            let have = strings(value);
            let min = spec.num("min").map_or(0, |m| m as usize);
            if have.len() < min {
                Some(if min == 1 { "pick one or more".into() } else { format!("pick at least {min}") })
            } else {
                have.iter().any(|h| !options.is_empty() && !options.contains(&h.as_str())).then(|| "not offered".into())
            }
        }
        FieldKind::Seq => {
            let min = spec.num("min").map_or(0, |m| m as usize);
            (strings(value).len() < min).then(|| if min == 1 { "at least 1".into() } else { format!("at least {min}") })
        }
        FieldKind::Map => pairs(value).iter().find(|(k, _)| !parse::name(k)).map(|(k, _)| format!("bad name \"{k}\"")),
        FieldKind::Argv => draft(spec, "", &strings(value)).err(),
        FieldKind::Bool | FieldKind::Secret => None,
        FieldKind::Dur if value.is_number() => None,
        FieldKind::Ref => {
            let v = value.as_str().unwrap_or_default();
            (!v.is_empty() && !options.is_empty() && !options.contains(&v)).then(|| "not found".into())
        }
        _ => {
            if value.is_null() && !spec.flag("required") {
                return None;
            }
            draft(spec, &text_of(value), &[]).err()
        }
    }
}

// ---- in the app ----

impl App {
    /// The value of a binding, or null.
    pub fn value(&self, bind: &str) -> &Json {
        self.values.get(bind).unwrap_or(&NULL)
    }

    /// A value is the new one at once, and goes to the core as a `Set`.
    pub(crate) fn set_value(&mut self, bind: &str, value: Json) -> Vec<Up> {
        self.values.insert(bind.into(), value.clone());
        // a field of a form is a draft: nothing is sent until the submit row says all of them
        if self.forms.iter().any(|f| f.holds(bind)) {
            return Vec::new();
        }
        let (kind, rest) = bind.split_once(':').unwrap_or(("", bind));
        let (id, field) = rest.rsplit_once('.').unwrap_or((rest, ""));
        vec![Up::Intent { intent: Intent::Set { kind: kind.into(), id: id.into(), field: field.into(), value } }]
    }

    fn state_of(&self, n: &Node, spec: &Spec) -> State {
        if spec.disabled() {
            State::Disabled
        } else if self.val.as_ref().is_some_and(|e| e.bind == spec.bind) {
            State::Editing
        } else if self.page().is_some_and(|p| p.id == n.id) {
            State::Focused
        } else {
            State::Idle
        }
    }

    fn rows_of(&self, n: &Node, width: u16) -> Vec<ratatui::text::Line<'static>> {
        let Some(spec) = Spec::of(n) else { return Vec::new() };
        let st = self.state_of(n, &spec);
        let ed = self.val.as_ref().filter(|e| st == State::Editing && e.bind == spec.bind);
        show::rows(&show::View { th: &self.theme, spec, value: self.value(spec.bind), st, ed, width: width as usize, last: self.tree.as_ref().is_none_or(|t| last_of(t, &n.id)) })
    }
}

/// Whether the node `id` is the last of the children of its parent (a node that stands alone is).
fn last_of(root: &Node, id: &str) -> bool {
    last_in(root, id).unwrap_or(true)
}

/// Whether `id` is the last child of its parent, looking under `node`; `None` when it is not under it.
fn last_in(node: &Node, id: &str) -> Option<bool> {
    if let Some(at) = node.children.iter().position(|c| c.id == id) {
        return Some(at + 1 == node.children.len());
    }
    node.children.iter().find_map(|c| last_in(c, id))
}

/// The rows a value field takes at this width, in the state it is in.
pub fn height(app: &App, n: &Node, width: u16) -> u16 {
    app.rows_of(n, width).len() as u16
}

pub fn draw(app: &App, n: &Node, area: Rect, f: &mut Frame) {
    f.render_widget(Paragraph::new(app.rows_of(n, area.width)), area);
}

pub use show::{View, rows};
