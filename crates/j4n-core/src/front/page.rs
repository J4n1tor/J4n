//! The page tree the screen is sent, made from the `page` and `template` rows of the Table. The screen
//! itself is the template `main`: its parts are fields (the questions, the status bar), the tabs of the pages under `home`, or another
//! template (`use`); a template of the operator's or a project's with the same id changes it field by field, as every row of the Table
//! is changed. A page whose `parent` is `home` is a tab, one whose parent is `main` stands under the template's parts, and one whose
//! parent is another page is a part of it (a sub-tab when that page presents its children as tabs); a page may hold parts of its own
//! (`children`) in place of pages. A pack that adds a page adds a tab; the screen draws what it is sent and is not changed for it.
//!
//! The base pack's pages (`packs/base/pages/`) are the tree a front has when nobody gave it another ([`Pages::builtin`]).

use crate::eval::Env;
use crate::expr::Expr;
use crate::schema::is_id;
use crate::table::{Kind, Layer, LayerKind, Problem, SourceFile, Tables};
use j4n_proto::{Field, FieldKind, Json, Node, Present};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

pub const ASKS: &str = "asks:open";
pub const BAR: &str = "bar:status";
pub const LOG: &str = "log:all";
pub const TASKS: &str = "tasks:all";
pub const FORM: &str = "form:thread";
pub use j4n_proto::{SHAPE_CARD, SHAPE_ON};
pub const COMMANDS: &str = j4n_proto::PALETTE;
pub const PACKS: &str = "packs:all";
pub const CATALOGUE: &str = "catalogue:main";
pub const KEYS_ROW: &str = j4n_proto::KEYS_ROW;
/// The usage view (`/usage`: each plan's windows and each gateway's spend) as a page's `meter` field: its value is the list of entries.
pub const USAGE: &str = "usage:all";
/// A field's prop: a page that lists nothing is left out of the tree, unless `tabs.all` is on.
pub const HIDE_EMPTY: &str = "hide_empty";

/// The binding of a thread's chat.
pub fn chat(thread: &str) -> String {
    format!("chat:{thread}")
}

/// One part of a template or of a page: a field, the tabs of the pages under a parent, or another template.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartRow {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub field: Option<String>,
    #[serde(default)]
    pub bind: Option<String>,
    #[serde(default)]
    pub view: Option<String>,
    #[serde(default)]
    pub props: Option<toml::Table>,
    #[serde(default)]
    pub grow: u8,
    #[serde(default)]
    pub height: u8,
    #[serde(default)]
    pub when: Option<String>,
    #[serde(default, rename = "use")]
    pub use_: Option<String>,
    #[serde(default)]
    pub tabs: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageRow {
    pub id: String,
    #[serde(default)]
    pub doc: String,
    pub parent: String,
    pub title: String,
    pub order: i64,
    pub present: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub ratio: u8,
    #[serde(default)]
    pub cols: u8,
    #[serde(default)]
    pub field: Option<String>,
    #[serde(default)]
    pub bind: Option<String>,
    #[serde(default)]
    pub view: Option<String>,
    #[serde(default)]
    pub props: Option<toml::Table>,
    #[serde(default)]
    pub children: Vec<PartRow>,
    /// For a leaf: a chart (`line` or `bars`) drawn over its field; the leaf and what joined it stay as they were.
    #[serde(default)]
    pub chart: Option<PartRow>,
    #[serde(default)]
    pub help: String,
    #[serde(default)]
    pub when: Option<String>,
    /// For a page under `home`: the tab it joins as a sub-tab, in place of being a tab of its own.
    #[serde(default)]
    pub group: Option<String>,
}

impl Kind for PageRow {
    const NAME: &'static str = "page";
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateRow {
    pub id: String,
    #[serde(default)]
    pub doc: String,
    #[serde(default)]
    pub extends: Option<String>,
    #[serde(default)]
    pub present: Option<String>,
    #[serde(default)]
    pub ratio: Option<u8>,
    #[serde(default)]
    pub cols: Option<u8>,
    #[serde(default)]
    pub children: Vec<PartRow>,
}

impl Kind for TemplateRow {
    const NAME: &'static str = "template";
}

/// What a part draws.
enum Body {
    Leaf { kind: FieldKind, bind: Bind, props: Json },
    /// The tabs of the pages whose parent is this.
    Tabs(String),
    /// Another template, in place.
    Use(String),
}

enum Bind {
    Name(String),
    View(String),
}

struct Part {
    id: Option<String>,
    title: String,
    body: Body,
    grow: u8,
    height: u8,
    when: Option<Expr>,
    at: String,
}

struct Page {
    row: PageRow,
    kind: Option<FieldKind>,
    when: Option<Expr>,
    tag: String,
    props: Json,
    parts: Vec<Part>,
    chart: Option<Part>,
}

struct Template {
    present: Present,
    ratio: u8,
    cols: u8,
    parts: Vec<Part>,
}

#[derive(Default)]
pub struct Pages {
    pages: Vec<Page>,
    templates: BTreeMap<String, Template>,
}

fn field_kind(word: &str) -> Option<FieldKind> {
    serde_json::from_value(Json::String(word.into())).ok()
}

fn present_of(word: &str) -> Present {
    match word {
        "tabs" => Present::Tabs,
        "subtabs" => Present::Subtabs,
        "sections" => Present::Sections,
        "split" => Present::Split,
        "grid" => Present::Grid,
        "auto" => Present::Auto,
        _ => Present::Stack,
    }
}

/// Why a chart's props cannot be drawn (law 8: the page is checked against what the screen draws); none for any other kind.
fn chart_check(kind: FieldKind, props: &Json) -> Option<String> {
    matches!(kind, FieldKind::Line | FieldKind::Bars).then(|| j4n_proto::chart_problem(kind, props)).flatten()
}

/// Why a field cannot stand by what it binds: a key is typed in the `secret` field only and goes by its own intent, so a `secret` is
/// never a form's draft (a form says its fields on a line the Log keeps) and a `key:` bind is never drawn by an editor that does not mask.
fn secret_check(kind: FieldKind, bind: Option<&str>) -> Option<String> {
    let bind = bind.unwrap_or_default();
    match (kind == FieldKind::Secret, bind.starts_with("draft:"), bind.starts_with("key:")) {
        (true, true, _) => Some("a `secret` field is never a form's draft: a form says its fields on a line the Log keeps".into()),
        (false, _, true) => Some("a `key:` bind is drawn by the `secret` field only: no other editor masks what is typed".into()),
        _ => None,
    }
}

fn json_of(t: &Option<toml::Table>) -> Json {
    t.as_ref().and_then(|t| serde_json::to_value(t).ok()).unwrap_or(Json::Null)
}

/// A part that can stand: it draws a field (of a `bind` or a `view`), holds tabs, or uses a template.
fn part(r: &PartRow, at: String) -> Result<Part, String> {
    let ways = usize::from(r.field.is_some()) + usize::from(r.use_.is_some()) + usize::from(r.tabs.is_some());
    if ways != 1 {
        return Err("a part draws a `field`, holds the `tabs` of a parent, or `use`s a template: one of the three".into());
    }
    if let Some(id) = r.id.as_deref().filter(|id| !is_id(id)) {
        return Err(format!("`{id}` is not an id"));
    }
    let when = r.when.as_deref().map(Expr::parse).transpose().map_err(|e| e.to_string())?;
    let body = match (&r.field, &r.use_, &r.tabs) {
        (Some(word), ..) => {
            let kind = field_kind(word).ok_or_else(|| format!("`{word}` is not a field"))?;
            let bind = match (&r.bind, &r.view) {
                (Some(b), None) => Bind::Name(b.clone()),
                (None, Some(v)) => Bind::View(v.clone()),
                _ => return Err("a field draws a `bind` or a `view`, one of the two".into()),
            };
            let props = json_of(&r.props);
            if let Some(why) = chart_check(kind, &props).or_else(|| secret_check(kind, r.bind.as_deref())) {
                return Err(why);
            }
            Body::Leaf { kind, bind, props }
        }
        (_, Some(t), _) if r.bind.is_some() || r.view.is_some() || r.props.is_some() => return Err(format!("a part that uses `{t}` draws nothing of its own")),
        (_, Some(t), _) => Body::Use(t.clone()),
        (.., Some(p)) if r.bind.is_some() || r.view.is_some() || r.props.is_some() => return Err(format!("a part that holds the tabs of `{p}` draws nothing of its own")),
        (.., Some(p)) => Body::Tabs(p.clone()),
        _ => unreachable!("one way was counted"),
    };
    Ok(Part { id: r.id.clone(), title: r.title.clone(), body, grow: r.grow, height: r.height, when, at })
}

fn parts(rows: &[PartRow], at: &str, found: &mut Vec<Problem>) -> Vec<Part> {
    let mut out = Vec::new();
    for (i, r) in rows.iter().enumerate() {
        match part(r, format!("{at} part {}", i + 1)) {
            Ok(p) => out.push(p),
            Err(why) => found.push(Problem::refused(format!("{at} part {}", i + 1), why)),
        }
    }
    out
}

const ROOTS: [&str; 2] = ["home", "main"];

/// The most tabs the top row holds: `1`–`9` reach each of them.
pub const MAX_TABS: usize = 9;

/// The tab a page joins when it would make a tenth: Grow, where what a pack adds to J4n is kept.
const OVERFLOW_GROUP: &str = "proposals";

impl Pages {
    /// The pages and templates the Table holds. `tags` says, by the name of a pack's layer, what to put beside its pages
    /// (`experimental`). A page or template that cannot stand — it is neither a group nor a leaf, its parent is nowhere, it is part of a
    /// loop — is left out and said; so is a part that cannot stand, and the rest stands.
    pub fn load(tables: &Tables, tags: &BTreeMap<String, String>) -> (Pages, Vec<Problem>) {
        let (mut rows, mut found) = (Vec::new(), Vec::new());
        for id in tables.ids(PageRow::NAME) {
            match tables.get::<PageRow>(id) {
                Ok(r) => rows.push(r),
                Err(p) => found.push(p),
            }
        }
        let ids: BTreeSet<String> = rows.iter().map(|r| r.id.clone()).collect();
        let parent_of: BTreeMap<String, String> = rows.iter().map(|r| (r.id.clone(), r.parent.clone())).collect();
        let mut pages = Vec::new();
        for r in rows {
            let at = format!("[[page]] {}", r.id);
            let leaf = r.field.is_some();
            let kind = r.field.as_deref().and_then(field_kind);
            let why = if leaf && kind.is_none() {
                Some(format!("`{}` is not a field", r.field.as_deref().unwrap_or_default()))
            } else if leaf && (r.bind.is_some() == r.view.is_some()) {
                Some("a leaf draws a `bind` or a `view`, one of the two".to_string())
            } else if !leaf && (r.bind.is_some() || r.view.is_some() || r.props.is_some()) {
                Some("a page with no `field` is a group: it has no `bind`, `view` or `props`".to_string())
            } else if let Some(why) = kind.and_then(|k| chart_check(k, &json_of(&r.props)).or_else(|| secret_check(k, r.bind.as_deref()))) {
                Some(why)
            } else if !leaf && r.chart.is_some() {
                Some("a chart stands over a leaf: a group has `children`".to_string())
            } else if leaf && !r.children.is_empty() {
                Some("a leaf has no `children`: a group does".to_string())
            } else if !ROOTS.contains(&r.parent.as_str()) && !ids.contains(&r.parent) {
                Some(format!("its parent `{}` is not a page", r.parent))
            } else if looped(&r.id, &parent_of) {
                Some("it is its own ancestor".to_string())
            } else {
                None
            };
            if let Some(why) = why {
                found.push(Problem::refused(at, why));
                continue;
            }
            let when = match r.when.as_deref().map(Expr::parse).transpose() {
                Ok(w) => w,
                Err(e) => {
                    found.push(Problem::refused(at, e.to_string()));
                    continue;
                }
            };
            let tag = tables.raw(PageRow::NAME, &r.id).and_then(|m| m.from.get("title")).and_then(|l| tags.get(l)).cloned().unwrap_or_default();
            let (props, parts) = (json_of(&r.props), parts(&r.children, &at, &mut found));
            let chart = r.chart.as_ref().and_then(|c| match part(c, format!("{at} chart")) {
                Ok(p) if matches!(&p.body, Body::Leaf { kind: FieldKind::Line | FieldKind::Bars, .. }) => Some(p),
                Ok(_) => {
                    found.push(Problem::refused(format!("{at} chart"), "a chart is a `line` or `bars` field"));
                    None
                }
                Err(why) => {
                    found.push(Problem::refused(format!("{at} chart"), why));
                    None
                }
            });
            pages.push(Page { row: r, kind, when, tag, props, parts, chart });
        }
        pages.sort_by(|a, b| (a.row.order, &a.row.id).cmp(&(b.row.order, &b.row.id)));
        let templates = templates(tables, &mut found);
        let mut all = Pages { pages, templates };
        all.place(&mut found);
        all.check_parts(&mut found);
        (all, found)
    }

    /// Settle the groups: a page that names a tab to join keeps the name only when that is a tab of `home` (else it is a tab of its own: the
    /// tab may be a pack's, and the pack off); and when more than
    /// [`MAX_TABS`] pages still make a tab each, the last by `order` join Grow (said, as a problem, so nobody finds a tab missing).
    fn place(&mut self, found: &mut Vec<Problem>) {
        let tabs: BTreeSet<String> = self.pages.iter().filter(|p| p.row.parent == "home" && p.row.group.is_none()).map(|p| p.row.id.clone()).collect();
        for p in &mut self.pages {
            let Some(g) = p.row.group.clone() else { continue };
            if p.row.parent != "home" || g == p.row.id {
                found.push(Problem::warn(format!("[[page]] {}", p.row.id), "its `group` is ignored: only a page under `home` joins another tab".to_string()));
                p.row.group = None;
            } else if !tabs.contains(&g) {
                // the tab may be a pack's, and the pack off: the page is a tab of its own, and nothing is wrong
                p.row.group = None;
            }
        }
        let mut alone: Vec<usize> = (0..self.pages.len()).filter(|i| self.pages[*i].row.parent == "home" && self.pages[*i].row.group.is_none()).collect();
        if alone.len() <= MAX_TABS || !tabs.contains(OVERFLOW_GROUP) {
            return;
        }
        alone.retain(|i| self.pages[*i].row.id != OVERFLOW_GROUP);
        // the first eight by `order` (and Grow) stay tabs; the others are sub-tabs of Grow
        for i in alone.split_off(MAX_TABS - 1) {
            found.push(Problem::warn(format!("[[page]] {}", self.pages[i].row.id), format!("there are more than {MAX_TABS} tabs: it joins `{OVERFLOW_GROUP}` as a sub-tab (give it a `group`)")));
            self.pages[i].row.group = Some(OVERFLOW_GROUP.into());
        }
    }

    /// Leave out the parts that use a template nobody has, hold the tabs of a parent nobody is, or would hold themselves.
    fn check_parts(&mut self, found: &mut Vec<Problem>) {
        let page_ids: BTreeSet<String> = self.pages.iter().map(|p| p.row.id.clone()).collect();
        let tpl_ids: BTreeSet<String> = self.templates.keys().cloned().collect();
        let edges: BTreeMap<String, Vec<String>> = self.templates.iter().map(|(id, t)| (id.clone(), t.parts.iter().filter_map(use_of).map(str::to_string).collect())).collect();
        let reaches = |from: &str, to: &str| {
            let (mut seen, mut todo) = (BTreeSet::new(), vec![from.to_string()]);
            while let Some(at) = todo.pop() {
                if at == to {
                    return true;
                }
                if seen.insert(at.clone()) {
                    todo.extend(edges.get(&at).cloned().unwrap_or_default());
                }
            }
            false
        };
        let mut keep = |owner: Option<&str>, parts: &mut Vec<Part>| {
            parts.retain(|p| {
                let why = match &p.body {
                    Body::Use(t) if !tpl_ids.contains(t) => Some(format!("it uses the template `{t}`, and there is none")),
                    Body::Use(t) if owner.is_some_and(|o| reaches(t, o)) => Some(format!("it uses `{t}`, which uses this template")),
                    Body::Tabs(t) if !ROOTS.contains(&t.as_str()) && !page_ids.contains(t) => Some(format!("it holds the tabs of `{t}`, which is not a page")),
                    _ => None,
                };
                if let Some(why) = &why {
                    found.push(Problem::refused(p.at.clone(), why.clone()));
                }
                why.is_none()
            });
        };
        for (id, t) in &mut self.templates {
            keep(Some(id), &mut t.parts);
        }
        for p in &mut self.pages {
            keep(None, &mut p.parts);
        }
    }

    pub fn has(&self, id: &str) -> bool {
        self.pages.iter().any(|p| p.row.id == id)
    }

    /// The id and the title of each page.
    pub fn ids(&self) -> impl Iterator<Item = (&str, &str)> {
        self.pages.iter().map(|p| (p.row.id.as_str(), p.row.title.as_str()))
    }

    /// How a page draws the chart `bind` (`chart:spend`): the field (`line` or `bars`) and its props, the first one a page or a template
    /// has. A chart held in the chat is drawn as the page draws it.
    pub fn chart_part(&self, bind: &str) -> Option<(FieldKind, Json)> {
        let of = |p: &Part| match &p.body {
            Body::Leaf { kind: kind @ (FieldKind::Line | FieldKind::Bars), bind: Bind::Name(b), props } if b == bind => Some((*kind, props.clone())),
            _ => None,
        };
        let on_pages = self.pages.iter().find_map(|p| p.chart.iter().chain(&p.parts).find_map(of).or_else(|| (p.row.bind.as_deref() == Some(bind)).then(|| p.kind.filter(|k| matches!(k, FieldKind::Line | FieldKind::Bars)).map(|k| (k, p.props.clone()))).flatten()));
        on_pages.or_else(|| self.templates.values().find_map(|t| t.parts.iter().find_map(of)))
    }

    /// Leave out the pages and parts that draw a view nobody has (`has` says whether one is there), and with them the pages under them.
    pub fn lint(&mut self, has: &dyn Fn(&str) -> bool) -> Vec<Problem> {
        let mut found = Vec::new();
        let mut keep = |parts: &mut Vec<Part>| {
            parts.retain(|p| match &p.body {
                Body::Leaf { bind: Bind::View(v), .. } if !has(v) => {
                    found.push(Problem::refused(p.at.clone(), format!("it draws the view `{v}`, and there is no such view")));
                    false
                }
                _ => true,
            })
        };
        self.templates.values_mut().for_each(|t| keep(&mut t.parts));
        self.pages.iter_mut().for_each(|p| keep(&mut p.parts));
        self.pages.retain(|p| match p.row.view.as_deref().filter(|v| !has(v)) {
            Some(v) => {
                found.push(Problem::refused(format!("[[page]] {}", p.row.id), format!("it draws the view `{v}`, and there is no such view")));
                false
            }
            None => true,
        });
        loop {
            let ids: BTreeSet<String> = self.pages.iter().map(|p| p.row.id.clone()).collect();
            let before = self.pages.len();
            self.pages.retain(|p| ROOTS.contains(&p.row.parent.as_str()) || ids.contains(&p.row.parent));
            if self.pages.len() == before {
                return found;
            }
        }
    }

    /// The pages of the base pack alone: the tree a front has when it was not given the Table's.
    pub fn builtin() -> Pages {
        const FILES: [(&str, &str); 2] = [("pages/main.toml", include_str!("../../../../packs/base/pages/main.toml")), ("templates/main.toml", include_str!("../../../../packs/base/templates/main.toml"))];
        let files = FILES.iter().map(|(path, text)| SourceFile { path: (*path).into(), text: (*text).into() }).collect();
        let layer = Layer::new(LayerKind::Pack, "base", files);
        Pages::load(&Tables::load(vec![layer]).0, &BTreeMap::new()).0
    }

    /// Every binding a page or a template draws, for a thread (a leaf that has a `view` draws `view:<id>`).
    pub fn binds(&self, thread: &str) -> Vec<String> {
        let own = self.pages.iter().flat_map(|p| p.chart.iter().filter_map(|c| match &c.body {
            Body::Leaf { bind, .. } => Some(bound(bind, thread)),
            _ => None,
        }).chain(bind_of(&p.row, thread)));
        let parts = self.pages.iter().flat_map(|p| &p.parts).chain(self.templates.values().flat_map(|t| &t.parts));
        own.chain(parts.filter_map(|p| match &p.body {
            Body::Leaf { bind, .. } => Some(bound(bind, thread)),
            _ => None,
        }))
        .collect()
    }

    /// The tree for a front whose thread is `thread`; `set` is the settings the `when` of a page or a part reads. The template `main`
    /// is the root, when there is one (a Table with none has the tabs of `home` alone); the pages under `main` stand after its parts.
    pub fn tree(&self, thread: &str, set: &dyn Env) -> Node {
        self.tree_lazy(thread, set, 0)
    }

    /// [`tree`](Pages::tree), where a page whose parts and pages under it make more than `lazy` nodes (0: never) is sent without them,
    /// `lazy` and with the `count` of its children; [`subtree`](Pages::subtree) is what the screen is sent when it asks for them.
    pub fn tree_lazy(&self, thread: &str, set: &dyn Env, lazy: usize) -> Node {
        let mut made = Made { thread, set, used: self.pages.iter().map(|p| p.row.id.clone()).collect(), lazy, open: None };
        made.used.insert("root".into());
        let mut root = match self.templates.get("main") {
            Some(t) => self.template_node(t, "root".into(), "main", &mut made, 0),
            None => Node { id: "root".into(), children: vec![self.tabs("home", "pages".into(), &mut made)], ..Node::default() },
        };
        root.children.extend(self.kids("main", &mut made));
        root
    }

    /// The page `id` with its children, when [`tree_lazy`](Pages::tree_lazy) leaves them out at this `lazy`; `None` for any other node.
    pub fn subtree(&self, id: &str, thread: &str, set: &dyn Env, lazy: usize) -> Option<Node> {
        let page = self.pages.iter().find(|p| p.row.id == id && Self::shown(p, set))?;
        let ids = || self.pages.iter().map(|p| p.row.id.clone()).collect();
        let cut = self.node(page, &mut Made { thread, set, used: ids(), lazy, open: None });
        cut.lazy.then(|| self.node(page, &mut Made { thread, set, used: ids(), lazy, open: Some(id) }))
    }

    fn shown(p: &Page, set: &dyn Env) -> bool {
        p.when.as_ref().is_none_or(|w| w.holds(set))
    }

    fn kids(&self, parent: &str, made: &mut Made) -> Vec<Node> {
        let mut out = Vec::new();
        for p in self.pages.iter().filter(|p| Self::under(p, parent) && Self::shown(p, made.set)) {
            out.push(self.node(p, made));
        }
        out
    }

    /// Whether `p` stands under `parent`: its parent is that, or it joined that tab with a `group` (and then is not under `home`).
    fn under(p: &Page, parent: &str) -> bool {
        match &p.row.group {
            Some(g) => g == parent,
            None => p.row.parent == parent,
        }
    }

    fn tabs(&self, parent: &str, id: String, made: &mut Made) -> Node {
        Node { id, present: Present::Tabs, children: self.kids(parent, made), ..Node::default() }
    }

    fn node(&self, p: &Page, made: &mut Made) -> Node {
        let field = p.kind.zip(bind_of(&p.row, made.thread)).map(|(kind, bind)| Field { kind, bind, props: p.props.clone() });
        let mut children = Vec::new();
        for c in &p.parts {
            children.extend(self.part_node(c, &p.row.id, made, 0));
        }
        children.extend(self.kids(&p.row.id, made));
        // a leaf that other pages joined becomes a tab with sub-tabs, the first being what it drew itself
        let joined = field.is_some() && self.pages.iter().any(|m| m.row.group.as_deref() == Some(p.row.id.as_str()) && Self::shown(m, made.set));
        let chart = p.chart.as_ref().filter(|_| field.is_some()).and_then(|c| self.part_node(c, &p.row.id, made, 0));
        let own = joined.then(|| {
            let id = made.take(format!("{}.own", p.row.id));
            over(chart.clone(), Node { id, title: p.row.title.clone(), field: field.clone(), help: p.row.help.clone(), icon: p.row.icon.clone(), ..Node::default() }, made)
        });
        let owned = own.is_some();
        let (field, help, present) = if owned { (None, String::new(), Present::Tabs) } else { (field, p.row.help.clone(), present_of(&p.row.present)) };
        children.splice(0..0, own);
        let mut node = Node {
            id: p.row.id.clone(),
            title: p.row.title.clone(),
            present,
            field,
            children,
            help,
            tag: p.tag.clone(),
            icon: p.row.icon.clone(),
            ratio: p.row.ratio,
            cols: p.row.cols,
            ..Node::default()
        };
        if !owned {
            node = over(chart, node, made);
        }
        if made.lazy > 0 && made.open != Some(p.row.id.as_str()) && weight(&node.children) > made.lazy {
            (node.count, node.lazy) = (Some(node.children.len() as u64), true);
            node.children.clear();
        }
        node
    }

    /// The node of a template, `id` being what the part that uses it (or the root) is called.
    fn template_node(&self, t: &Template, id: String, name: &str, made: &mut Made, depth: usize) -> Node {
        let mut children = Vec::new();
        for c in &t.parts {
            children.extend(self.part_node(c, name, made, depth));
        }
        Node { id, present: t.present, children, ratio: t.ratio, cols: t.cols, ..Node::default() }
    }

    /// The node of a part of `owner` (a page or a template), unless it is not shown now.
    fn part_node(&self, p: &Part, owner: &str, made: &mut Made, depth: usize) -> Option<Node> {
        if p.when.as_ref().is_some_and(|w| !w.holds(made.set)) || depth > MAX_DEPTH {
            return None;
        }
        let word = match &p.body {
            Body::Leaf { kind, .. } => format!("{kind:?}").to_lowercase(),
            Body::Tabs(_) => "tabs".into(),
            Body::Use(t) => t.clone(),
        };
        let id = made.take(p.id.clone().unwrap_or_else(|| format!("{owner}.{word}")));
        let mut node = match &p.body {
            Body::Leaf { kind, bind, props } => Node { id, field: Some(Field { kind: *kind, bind: bound(bind, made.thread), props: props.clone() }), ..Node::default() },
            Body::Tabs(parent) => self.tabs(parent, id, made),
            Body::Use(t) => self.template_node(self.templates.get(t)?, id, t, made, depth + 1),
        };
        node.title = p.title.clone();
        (node.grow, node.height) = (p.grow, p.height);
        Some(node)
    }
}

/// How deep templates may use templates: a loop is refused when the Table is read, and this is the second lock.
const MAX_DEPTH: usize = 8;

/// What a tree being made knows: whose thread it is, the settings, and the ids it has given.
struct Made<'a> {
    thread: &'a str,
    set: &'a dyn Env,
    used: BTreeSet<String>,
    /// A page with more nodes than this under it is left lazy (0: none is).
    lazy: usize,
    /// The page that is being opened, which is not left lazy.
    open: Option<&'a str>,
}

/// How many nodes there are in `nodes`, counting the ones under them.
fn weight(nodes: &[Node]) -> usize {
    nodes.iter().map(|n| 1 + weight(&n.children)).sum()
}

impl Made<'_> {
    /// `id`, or the first of `id-2`, `id-3`… that no node has.
    fn take(&mut self, id: String) -> String {
        let mut got = id.clone();
        for n in 2.. {
            if self.used.insert(got.clone()) {
                break;
            }
            got = format!("{id}-{n}");
        }
        got
    }
}

/// A leaf with a chart over it: the node becomes a stack of the chart and the leaf's field (which keeps the help and takes the room that is left).
fn over(chart: Option<Node>, mut n: Node, made: &mut Made) -> Node {
    let Some(chart) = chart.filter(|_| n.field.is_some()) else { return n };
    let inner = Node { id: made.take(format!("{}.list", n.id)), field: n.field.take(), help: std::mem::take(&mut n.help), grow: 1, ..Node::default() };
    n.present = Present::Stack;
    n.children.splice(0..0, [chart, inner]);
    n
}

fn use_of(p: &Part) -> Option<&str> {
    match &p.body {
        Body::Use(t) => Some(t),
        _ => None,
    }
}

fn bound(b: &Bind, thread: &str) -> String {
    match b {
        Bind::Name(n) => n.replace("{thread}", thread),
        Bind::View(v) => format!("view:{v}"),
    }
}

/// The templates the Table holds, each with what it extends put in, and the ones that cannot stand left out.
fn templates(tables: &Tables, found: &mut Vec<Problem>) -> BTreeMap<String, Template> {
    let mut rows = BTreeMap::new();
    for id in tables.ids(TemplateRow::NAME) {
        match tables.get::<TemplateRow>(id) {
            Ok(r) => {
                rows.insert(r.id.clone(), r);
            }
            Err(p) => found.push(p),
        }
    }
    let mut out = BTreeMap::new();
    for id in rows.keys() {
        let at = format!("[[template]] {id}");
        match inherit(id, &rows) {
            Ok((present, ratio, cols, children)) => {
                let parts = parts(&children, &at, found);
                out.insert(id.clone(), Template { present: present_of(present.as_deref().unwrap_or("stack")), ratio, cols, parts });
            }
            Err(why) => found.push(Problem::refused(at, why)),
        }
    }
    out
}

/// The `present`, `ratio`, `cols` and `children` a template has: its own, and where it has none the ones of the template it extends.
/// A template that extends its own id starts from the original in the layer below, which the Table has already done.
fn inherit(id: &str, rows: &BTreeMap<String, TemplateRow>) -> Result<(Option<String>, u8, u8, Vec<PartRow>), String> {
    let (mut present, mut ratio, mut cols, mut children) = (None, None, None, None::<Vec<PartRow>>);
    let (mut chain, mut at) = (vec![id], id);
    while let Some(r) = rows.get(at) {
        present = present.or_else(|| r.present.clone());
        ratio = ratio.or(r.ratio);
        cols = cols.or(r.cols);
        if children.is_none() && !r.children.is_empty() {
            children = Some(r.children.clone());
        }
        match r.extends.as_deref() {
            Some(next) if next != at => {
                if chain.contains(&next) {
                    return Err(format!("it extends `{next}`, which extends it"));
                }
                if !rows.contains_key(next) {
                    return Err(format!("it extends `{next}`, and there is no such template"));
                }
                chain.push(next);
                at = next;
            }
            _ => break,
        }
    }
    Ok((present, ratio.unwrap_or(0), cols.unwrap_or(0), children.unwrap_or_default()))
}

fn bind_of(r: &PageRow, thread: &str) -> Option<String> {
    match (&r.bind, &r.view) {
        (Some(b), _) => Some(b.replace("{thread}", thread)),
        (None, Some(v)) => Some(format!("view:{v}")),
        _ => None,
    }
}

fn looped(id: &str, parent_of: &BTreeMap<String, String>) -> bool {
    let mut at = id;
    for _ in 0..parent_of.len() + 1 {
        match parent_of.get(at) {
            Some(p) if p == id => return true,
            Some(p) => at = p,
            None => return false,
        }
    }
    true
}
