//! The UI protocol: serde types only, shared by the core and the TUI.
//!
//! The core sends [`Down`] and the TUI sends [`Up`]; today over channels in one process, later over a socket. Nothing here
//! evaluates, reads a file or names a core type: an answer is a [`Reply`], an order to a task is a [`Control`], and what the
//! TUI draws is a [`Node`] tree whose fields are bound to data the core keeps (`chat:main`, `log:all`, `tasks:open`).

use serde::{Deserialize, Serialize};

pub mod keys;

/// The loose value a binding or a `Set` carries; re-exported so the TUI needs no JSON crate of its own.
pub use serde_json::{Value as Json, json};
/// Wipes a buffer that held a key; re-exported so the screen and the core need no crate of their own for it.
pub use zeroize::Zeroize;

/// Carried by `Hello { api }`; the TUI refuses a newer one with a clear message.
pub const API: u32 = 1;

/// The two bindings of the composer: the rows of the shaper's card for the draft in it, and whether the switch is on. The screen watches
/// both whenever a chat is open.
pub const SHAPE_CARD: &str = "shape:card";
pub const SHAPE_ON: &str = "shape:on";
/// What stands in the composer's strip: the tags the draft was given, how full the context is, and the doer the draft would go to
/// (`Value`: `{tags: [{text, icon, pct}], gauge, route}`). The screen watches it whenever a chat is open.
pub const COMPOSER_STRIP: &str = "composer:strip";
/// What the open-ask binder would make of the draft (`Value`: `{text, ask, mark, label}`, or null when the draft is not one it takes). The
/// core answers a `Draft` with it whenever a question is open; `text` is the draft it was made for, `ask` the question it would answer,
/// `mark` a glyph-slot name and `label` the reply in a word (`yes`). The screen shows it after the target of the line and only while the
/// draft is still `text`.
pub const COMPOSER_BIND: &str = "composer:bind";

/// The palette: a row for each command (its words, what follows them, what it does, and the key that runs it). The screen watches it
/// whenever it holds a tree, so that the jump (`⌃k`) can list the commands beside the nodes.
pub const PALETTE: &str = "commands:all";

/// The keys row, the last row of the screen: a row for each hint (the id of the hint as its key, its keys, its words), in the order
/// they stand on the row. The screen picks the ones of the page that is open.
pub const KEYS_ROW: &str = "keys:row";
/// The project's glossary (`Value`: `{id: words}`): each id the docs define in a table row, with the first words of its
/// meaning. The screen watches it whenever a chat is open and draws a known id in prose with its words.
pub const TERMS: &str = "terms:";
/// The look file (`Value`): per kind prefix its glyph and colour, per kind its line, per field its renderer.
pub const LOOK_FILE: &str = "look:file";
/// The chats, the pinned list of the orchestrator's screen: a row for each chat, the ones that wait first, a plan's
/// branches under their chat (`parent`). Its `cells` are the state mark (`icon`), the id (`c3`), the title, `doer · model`, how full the context is,
/// what the chat cost, and the time or the word (`waits 2:10`); `data` has the same as values. Sent only while the switch `chats.on` is on.
pub const CHATS: &str = "chats:list";
/// Which chat the screen is in (`Value`: `{top, id, title, crumb: [{id, title}], state, chats, running, waits}`), sent again when it changes. The
/// binding of the chat itself (`chat:main`, as the tree names it) holds the rows of the chat in hand, so the screen has one chat field for all of
/// them and starts it over when `id` changes. Null, or never sent, while the switch is off.
pub const CHAT_HERE: &str = "chat:here";
/// What the core completes: a window with `find` = `<slot> <token>`, the slot `line` for the line up to the cursor. Rows, sent whole: `~for`
/// (`find` as asked, the slot, the token), then a row per candidate, best first (`data`: `{slot, text, score}`). A slot the screen holds is `~for` alone.
pub const COMPLETE: &str = "complete:";

/// The data of a chat row that is a message between chats: `row.data["message"]`, with the envelope as the row's icon (`message`). The row
/// of a sender says `→ c4` and then `delivered`, the row of a receiver `from c3`, the orchestrator's `c3 → c4`; `state` is `queued`, `held` or `delivered`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub from: String,
    pub to: String,
    /// Who carried it: `J4n`.
    pub via: String,
    pub text: String,
    pub state: String,
}

impl Message {
    /// The message a chat row is, when it is one.
    pub fn of(row: &Row) -> Option<Message> {
        serde_json::from_value(row.data.as_ref()?.get("message")?.clone()).ok()
    }
}

/// How a cell, a line or a toast reads: one word the theme turns into a colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tone {
    #[default]
    Plain,
    Dim,
    Good,
    Warn,
    Bad,
    Accent,
}

// ---- what is drawn ----

/// How the children of a node are laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Present {
    #[default]
    Stack,
    Tabs,
    Sections,
    /// Chosen by depth below the root: 0 tabs, 1 sub-tabs, 2 sections, deeper a stack.
    Auto,
    /// One row of titles with the open one underlined; the other children are one page at a time, as in tabs.
    Subtabs,
    /// Two or more panes side by side; `ratio` is the share of the first.
    Split,
    /// Cells in order, `cols` to a row (0: by width).
    Grid,
}

/// What a leaf shows. Each is one component of the renderer; a pack adds pages, never a kind of field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldKind {
    /// A transcript with a composer under it. Bound to a stream.
    Chat,
    /// One row of chips. Bound to rows.
    Bar,
    /// The questions that are open. Bound to `asks:open`.
    Ask,
    /// A virtualized stream of events.
    Log,
    /// Rows, one per line.
    List,
    /// Rows of `label | value`; a value with an action can be changed.
    Form,
    // ---- a value, one component for each type; the node's title is its label, `bind` holds the value
    // (`Down::Value`) and names what a change sets (`<kind>:<id>.<field>`) ----
    Str,
    Text,
    Int,
    Num,
    Bool,
    Enum,
    Multi,
    /// One of the entries of a kind of the Table, picked by typing (`props.options`).
    Ref,
    /// A list of strings.
    Seq,
    /// Names with strings for values.
    Map,
    Dur,
    Usd,
    Path,
    Argv,
    Pattern,
    Expr,
    Secret,
    Markdown,
    // ---- what is only shown: the value is a binding's `Value`, or its `Rows` ----
    /// Big numbers with a label and a change beside each (`Value`: a list of `{v, label, delta, tone}`).
    Stat,
    /// How full something is (`Value`: a fraction, 1 being all of it; `props.warn`).
    Meter,
    /// A small chart of a series (`Value`: a list of numbers, or `{points, last}`).
    Spark,
    /// A chart of lines on one canvas (`Value`: `{x: [label…], series: {name: [number|null…]}}`; props: [`chart_problem`]).
    Line,
    /// A chart of bars, vertical or horizontal, grouped by label (the same `Value` and props).
    Bars,
    /// A mark and a word in the colour of what it says (`Value`: `{icon, text, tone}` or a list of them).
    Chip,
    /// A button that sends `props.intent` (`Value`: `done` once it has been applied).
    Action,
    /// Columns (`props.columns`) of the rows of a window of a binding, sorted and found by the core.
    Table,
    /// Rows that hang from the row named in their `parent`, folded and unfolded.
    Tree,
    /// One thing: a title, what is known of it on pipes, and what can be done with it.
    Card,
    /// A grid of cards, one for each row.
    Cards,
}

impl FieldKind {
    /// Whether this kind is a value the operator can change.
    pub fn is_value(self) -> bool {
        !matches!(
            self,
            FieldKind::Chat | FieldKind::Bar | FieldKind::Ask | FieldKind::Log | FieldKind::List | FieldKind::Form | FieldKind::Stat | FieldKind::Meter | FieldKind::Spark | FieldKind::Line | FieldKind::Bars | FieldKind::Chip | FieldKind::Action | FieldKind::Table | FieldKind::Tree | FieldKind::Card | FieldKind::Cards
        )
    }
}

// ---- charts: the words of the props of `line` and `bars`, said once for the core that
// checks a page and the screen that draws it ----

/// The most series one chart holds (the reader could not tell more apart).
pub const CHART_SERIES: usize = 4;
/// `style` of a `line`, `scale` and `right_scale`, `axis`, `unit`, `orient` of `bars`, and the role colours a series may name.
pub const CHART_STYLES: [&str; 3] = ["braille", "stream", "wave"];
pub const CHART_SCALES: [&str; 2] = ["lin", "log"];
pub const CHART_AXES: [&str; 2] = ["left", "right"];
pub const CHART_UNITS: [&str; 4] = ["n", "usd", "pct", "s"];
pub const CHART_ORIENTS: [&str; 2] = ["vertical", "horizontal"];
pub const CHART_ROLES: [&str; 6] = ["accent", "run", "fetch", "warn", "ok", "error"];

/// Why the props of a chart cannot be drawn, or `None`: a `series` of one to [`CHART_SERIES`] entries `{name, axis, role, unit}` with names
/// that differ, and `style`, `scale`, `right_scale`, `orient`, `window` in their words. The core checks a page with it; the screen, what it draws.
pub fn chart_problem(kind: FieldKind, props: &Json) -> Option<String> {
    let word = |key: &str, ok: &[&str], at: &Json| match at.get(key) {
        Some(w) if !w.as_str().is_some_and(|w| ok.contains(&w)) => Some(format!("`{key}` is {w}; it is one of {}", ok.join(", "))),
        _ => None,
    };
    let list = props.get("series").and_then(Json::as_array).map_or(&[][..], Vec::as_slice);
    if list.is_empty() || list.len() > CHART_SERIES {
        return Some(format!("a chart holds one to {CHART_SERIES} series, not {}", list.len()));
    }
    let bars = kind == FieldKind::Bars;
    let words = [("style", &CHART_STYLES[..]), ("scale", &CHART_SCALES), ("right_scale", &CHART_SCALES), ("orient", &CHART_ORIENTS)];
    if let Some(why) = words.iter().find_map(|(k, ok)| word(k, ok, props)) {
        return Some(why);
    }
    if props.get("window").is_some_and(|w| w.as_u64().is_none_or(|w| w == 0)) || props.get("rows").is_some_and(|r| !r.as_u64().is_some_and(|r| (4..=24).contains(&r))) || (bars && props.get("scale").is_some_and(|s| s != "lin")) {
        return Some("`window` is a count of points, `rows` is 4 to 24, and bars are linear".into());
    }
    for (i, s) in list.iter().enumerate() {
        let name = s.get("name").and_then(Json::as_str).unwrap_or_default();
        if name.is_empty() || list[..i].iter().any(|o| o.get("name") == s.get("name")) {
            return Some(format!("series {} has no name, or one that is used already", i + 1));
        }
        for (k, ok) in [("axis", &CHART_AXES[..]), ("role", &CHART_ROLES), ("unit", &CHART_UNITS)] {
            if let Some(why) = word(k, ok, s) {
                return Some(format!("series `{name}`: {why}"));
            }
        }
    }
    None
}

/// A leaf of the tree: what to draw and where its data comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Field {
    pub kind: FieldKind,
    pub bind: String,
    /// What the type is given (`max`, `step`, `options`, `disabled`, `error`, …): a table of loose values, read by the component of the kind.
    #[serde(default, skip_serializing_if = "Json::is_null")]
    pub props: Json,
}

/// One node of the page tree. Presentation follows the depth unless `present` says otherwise.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub present: Present,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<Field>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Node>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub help: String,
    /// What stands beside the title when the page is not settled (`experimental`, `beta`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tag: String,
    /// The name of a glyph slot (`worktree`, `ask`), never a codepoint; the screen draws it in its tier.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub icon: String,
    /// A number beside the title (the open asks, the changed settings).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u64>,
    /// The children are not sent yet: the screen asks for them (`Intent::Open`) when the node is first shown, and the core answers with
    /// a `Tree` for this node.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub lazy: bool,
    /// For a split: the share of the first pane, in percent (0: half).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub ratio: u8,
    /// For a grid: cells in a row (0: by width).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub cols: u8,
    /// In a stack: its share of the room that is left, when it is not sized by what it shows (0: by what it shows).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub grow: u8,
    /// In a stack: exactly this many rows (0: by what it shows). It wins over `grow`.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub height: u8,
}

fn is_zero(n: &u8) -> bool {
    *n == 0
}

fn is_zero_u64(n: &u64) -> bool {
    *n == 0
}

// ---- what is said ----

/// Who a line of the transcript is from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    You,
    Doer,
    Reasoning,
    Tool,
    Shell,
    Notice,
    Error,
    /// A line of the log field: `seq kind task detail`.
    Event,
}

/// One line of a stream. A `partial` one is still being written: the next [`Item::Append`] with its key adds to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    pub key: String,
    pub role: Role,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub partial: bool,
    #[serde(default)]
    pub tone: Tone,
}

/// A change to a stream. `Put` replaces the line with its key, or adds it at the end.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "do", rename_all = "snake_case")]
pub enum Item {
    Put { line: Line },
    Append { key: String, text: String },
    Drop { key: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Cell {
    pub text: String,
    #[serde(default)]
    pub tone: Tone,
    /// The name of a glyph slot (`done`, `failed`) drawn before the text; the screen draws it in its tier.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub icon: String,
}

impl Cell {
    pub fn new(text: impl Into<String>, tone: Tone) -> Cell {
        Cell { text: text.into(), tone, icon: String::new() }
    }

    /// A cell that starts with the glyph of the slot `icon`.
    pub fn with_icon(icon: &str, text: impl Into<String>, tone: Tone) -> Cell {
        Cell { text: text.into(), tone, icon: icon.into() }
    }
}

/// What a row offers to do: a key to press, a label, the intent it sends, and how risky it is (0–100).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Act {
    pub key: String,
    pub label: String,
    pub intent: Intent,
    #[serde(default)]
    pub risk: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Row {
    pub key: String,
    pub cells: Vec<Cell>,
    #[serde(default)]
    pub tone: Tone,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub detail: Vec<Cell>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<Act>,
    /// The key of the row this one hangs from (a tree, a list with sections); none for a row at the top.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// Who made the words of the row (the chat marks a model's with `~`).
    #[serde(default, skip_serializing_if = "By::is_default")]
    pub by: By,
    /// The core's default for a row with rows under it or output under it; none leaves it to the screen's rules.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fold: Option<Fold>,
    /// The row may be shown as one with the rows beside it that have the same mark and may be merged.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub merge: bool,
    /// What the row stands for, as structure, for the screen to format in its own look: the Log sends each event of a window so.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Json>,
}

/// Who made the words of a row. A row made by a model starts with `~` where it is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum By {
    #[default]
    Operator,
    Core,
    Rule,
    Doer,
    Model,
}

impl By {
    fn is_default(&self) -> bool {
        *self == By::Operator
    }
}

/// How the core would have a row with rows under it shown at first; the screen keeps what the operator opened or shut.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fold {
    Open,
    Folded,
    /// Open, and not folded by a rule (a lane that failed).
    Pinned,
}

/// The kind of question that is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    #[default]
    YesNo,
    Choice,
    Approval,
    Text,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Choice {
    pub id: String,
    /// Empty for an option of an approval that only lays its hint or note over a fixed answer.
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub recommended: bool,
    /// Who made the recommendation. On an approval only the core's own word or a rule's is shown as the hint, never a model's.
    #[serde(default, skip_serializing_if = "By::is_default")]
    pub by: By,
    /// Answering with this widens what is allowed (a grant for the session or the place); the screen asks for the key twice.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub widens: bool,
    /// A few words at the right of the option (`−60%`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

/// One open question, as the operator is shown it. On an approval `options` only lay words over the fixed answers (`yes`, `no`, the
/// grants `session` and `here`, `stop`): a `Choice` with one of those ids gives its label, its note, its hint and whether it widens.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AskView {
    pub ask: String,
    /// The task that asked.
    pub asker: String,
    pub question: String,
    pub shape: Shape,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<Choice>,
    /// For an approval: the effect, in a line (`shell: make build`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub effect: String,
    /// How risky the effect is (0–100).
    #[serde(default)]
    pub risk: u8,
    /// Which grants can be given with a yes: `session`, `here`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub grants: Vec<String>,
    /// How long it has waited, as the core words it (`8:42`); the core sends the list again when it changes.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub wait: String,
    /// What it is about, each a mark and a word (a branch, a commit).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub facts: Vec<Cell>,
    /// The change an approval would make, a row for each (`[what, stats…]`); the `detail` of a row is its diff.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub change: Vec<Row>,
    /// Why it is risky or why it was asked, in words.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub why: String,
}

// ---- what is asked ----

/// What the operator answers to a question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reply", rename_all = "snake_case")]
pub enum Reply {
    Yes,
    No,
    Pick { id: String },
    Text { text: String },
    /// Yes, and remember it for `scope` (`session` or `here`).
    Grant { scope: String },
}

/// An order to a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "ctl", rename_all = "snake_case")]
pub enum Control {
    Steer { text: String },
    Abort,
    Kill,
}

/// What the TUI asks the core to do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "intent", rename_all = "snake_case")]
pub enum Intent {
    /// A typed line. The core decides what it is: an answer, a command, a steer, the next turn. `with` holds the keys of the rows
    /// the operator kept with it (the notes of the shaper, files attached); a key the core does not know is dropped with a notice.
    Say { text: String, #[serde(default, skip_serializing_if = "Vec::is_empty")] with: Vec<String> },
    Answer { ask: String, reply: Reply },
    Control { task: String, control: Control },
    /// A command by name (`fresh`, `switch`), as the palette runs it.
    Run { command: String, #[serde(default)] args: Vec<String> },
    /// Go into a chat of the list (`c3`, `c3.1`); the screen is sent that chat's rows. Empty: the orchestrator. `@next`: the next chat that
    /// waits for you, then the next that runs (`⇥1`). The core says why when there is none.
    Enter { chat: String },
    /// Go up one level: out of a branch to its chat, out of a chat to the orchestrator. At the top it does nothing.
    Leave,
    /// Change one field of a thing.
    Set { kind: String, id: String, field: String, value: Json },
    Open { node: String },
    /// A provider's key, typed in its secret editor (the bind `key:<id>.value`): the one road a key takes to J4n's key file. It is
    /// never a `Set`, a form's submit or a line, which the Log keeps.
    Key { id: String, value: Secret },
}

/// A key on its way from the secret editor to J4n's key file. Nothing prints it: `Debug`, `Display` and `Serialize` say `«secret»`;
/// it is never read from text (a prop or a line is not a road for a key); and its text is wiped when it goes.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct Secret(String);

/// What a [`Secret`] is printed and written as.
pub const SECRET_SHOWN: &str = "«secret»";

impl Secret {
    pub fn new(text: String) -> Secret {
        Secret(text)
    }

    /// The key itself, for the one place that keeps it.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(SECRET_SHOWN)
    }
}

impl std::fmt::Display for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(SECRET_SHOWN)
    }
}

impl Serialize for Secret {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(SECRET_SHOWN)
    }
}

impl<'de> Deserialize<'de> for Secret {
    fn deserialize<D: serde::Deserializer<'de>>(_: D) -> Result<Secret, D::Error> {
        Err(serde::de::Error::custom("a key is never read from text: it is typed in its editor"))
    }
}

impl Intent {
    /// An intent written as loose JSON (a prop of a field), or none when it is not one.
    pub fn from_json(v: &Json) -> Option<Intent> {
        Intent::deserialize(v).ok()
    }
}

/// Core → TUI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "down", rename_all = "snake_case")]
pub enum Down {
    Hello { api: u32, name: String, version: String, project: String },
    /// The page tree, or the part of it under a node.
    Tree { node: Node },
    /// One value of a binding.
    Value { bind: String, value: Json, rev: u64 },
    /// The rows of a binding: all of them, or (once there are many) a window of `rows.len()` of the `total`, the first being the
    /// `from`-th in the order the screen asked for (`Up::Window`).
    Rows { bind: String, rows: Vec<Row>, total: u64, #[serde(default, skip_serializing_if = "is_zero_u64")] from: u64, rev: u64 },
    /// Changes to a stream. `reset` first empties it (a replay after a reconnect).
    Stream { bind: String, reset: bool, items: Vec<Item> },
    Asks { open: Vec<AskView> },
    Toast { text: String, tone: Tone },
    Focus { node: String },
    /// The keymap: every key that is bound or freed, by context. Until it comes, the screen takes each key as it is.
    Keys { binds: Vec<Bind> },
    /// The look the operator chose (`theme.*`), sent after the keymap and again when one changes. The screen
    /// checks the words and keeps the look it has, with a toast, for one it cannot read.
    Theme { look: Look },
}

/// The words of `theme.*` that shape what is drawn, as the Table holds them. `NO_COLOR` is the terminal's and is not here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Look {
    pub palette: String,
    pub marks: String,
    pub frame: String,
    pub corners: String,
    pub title_place: String,
    pub density: String,
    pub fill: String,
    pub you: String,
    pub motion: bool,
    pub mouse: String,
    /// How prose is read (`read.*`); older senders leave it out.
    #[serde(default)]
    pub read: Read,
    /// How a row's time is said (`look.time`): `ago` (`3m`, `2h1m`, from the screen's own clock) or `clock` (`13:43`); empty is `ago`.
    #[serde(default)]
    pub time: String,
    /// How the charts are drawn (`look.chart`): `dither` (half blocks where the terminal has truecolor) or `plain`; empty is `dither`.
    #[serde(default)]
    pub chart: String,
}

/// The words of `read.*` that shape how prose is drawn: the style (`calm` or `full`), where prose wraps, after how many lines code
/// is folded, whether an address opens on a click (`osc8`) or is said (`plain`), and the big level-1 headings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Read {
    pub style: String,
    pub width: u32,
    pub fold_code: u32,
    pub links: String,
    pub big_heads: bool,
    /// `read.terms`: a known id in prose is drawn with its words.
    #[serde(default)]
    pub terms: bool,
}

impl Default for Read {
    fn default() -> Read {
        Read { style: "calm".into(), width: 76, fold_code: 12, links: "osc8".into(), big_heads: false, terms: true }
    }
}

impl Look {
    /// The parts of the look and the words each takes (`motion` takes `true` or `false`; the palettes are the six of the design). The screen
    /// checks what it draws against its own words (law 8), and a test holds the two lists the same.
    pub const PARTS: [(&'static str, &'static [&'static str]); 10] = [
        ("palette", &["jjj", "dusk", "paper", "ember", "mono", "contrast"]),
        ("marks", &["icons", "unicode", "ascii", "words"]),
        ("frame", &["pipes", "gutter", "hang", "round", "airy"]),
        ("corners", &["round", "square"]),
        ("title_place", &["inside", "border", "none"]),
        ("density", &["compact", "normal", "roomy"]),
        ("fill", &["match", "terminal", "paint"]),
        ("you", &["bracket", "band", "mark"]),
        ("motion", &["true", "false"]),
        ("mouse", &["on", "off"]),
    ];

    /// The word of a part, as the Table holds it.
    pub fn word(&self, part: &str) -> Option<String> {
        Some(match part {
            "palette" => self.palette.clone(),
            "marks" => self.marks.clone(),
            "frame" => self.frame.clone(),
            "corners" => self.corners.clone(),
            "title_place" => self.title_place.clone(),
            "density" => self.density.clone(),
            "fill" => self.fill.clone(),
            "you" => self.you.clone(),
            "motion" => self.motion.to_string(),
            "mouse" => self.mouse.clone(),
            _ => return None,
        })
    }

    /// The look with one part said again; a part or a word that is not one is left as it was.
    #[must_use]
    pub fn with(mut self, part: &str, word: &str) -> Look {
        match part {
            "palette" => self.palette = word.into(),
            "marks" => self.marks = word.into(),
            "frame" => self.frame = word.into(),
            "corners" => self.corners = word.into(),
            "title_place" => self.title_place = word.into(),
            "density" => self.density = word.into(),
            "fill" if matches!(word, "match" | "terminal" | "paint") => self.fill = word.into(),
            "you" if matches!(word, "bracket" | "band" | "mark") => self.you = word.into(),
            "motion" if matches!(word, "true" | "false") => self.motion = word == "true",
            "mouse" if matches!(word, "on" | "off") => self.mouse = word.into(),
            _ => {}
        }
        self
    }
}

/// What a key does where it is pressed. `context` is `global`, `composer`, `transcript`, `list`, `ask` or
/// `page:<id>`; `key` and `to` are names as in [`keys`]. The screen takes `key` for the key `to` (the one its own code reads), or sends
/// `run`; a bind with neither is a key the operator freed, which does nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bind {
    pub context: String,
    pub key: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub to: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<Intent>,
}

/// TUI → core.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "up", rename_all = "snake_case")]
pub enum Up {
    Hello { api: u32 },
    Watch { bind: String },
    Unwatch { bind: String },
    /// Which rows of a watched binding the screen shows: `count` of them from the `from`-th, in the order of the column `sort`
    /// (`-name` for the other way; empty: the core's own order), of those that match `find` (empty: all). The core answers with `Rows`.
    Window { bind: String, from: u64, count: u64, #[serde(default, skip_serializing_if = "String::is_empty")] sort: String, #[serde(default, skip_serializing_if = "String::is_empty")] find: String },
    Intent { intent: Intent },
    /// The composer's draft, after a pause in typing and only when it changed. `target` names the doer the line is for, or is empty
    /// for the one the thread has. Fed to the shaper, and to the route guess under the composer.
    Draft { text: String, #[serde(default, skip_serializing_if = "String::is_empty")] target: String },
    Resize { cols: u16, rows: u16 },
    Quit,
}
