//! One event of the Log in the J4n look: a line of time, glyph, kind and summary, and opened, the
//! whole event as a pipe tree. What is chosen per kind and per field is the table [`Look`], made from the look file the core sends
//! (`look:file`: `packs/base/look/look.toml` and its layers), so drawing reads only it. No JSON on the screen unless asked for (`r`).

use super::layout::{self, Laid, Piece};
use crate::theme::Theme;
use crate::theme::glyph::Slot;
use crate::theme::palette::Role as Hue;
use crate::wrap::{fit, wrap};
use j4n_proto::{Json, Tone};
use ratatui::style::Style;
use ratatui::text::Span;
use unicode_width::UnicodeWidthStr;

/// How a value is shown, by what it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Show {
    Text,
    /// milliseconds as `340 ms`, `1.2 s`, `2 m 05 s`
    Duration,
    /// milliseconds since 1970 as `2025-01-02 08:12:04.120` (UTC: the core does not know the operator's zone)
    Time,
    Money,
    /// a count of bytes as `512 B`, `4.0 kB`, `1.2 MB`
    Bytes,
    Path,
    Cmd,
    /// lines that start with `+` or `-` in the tones of a diff
    Diff,
    /// words read as Markdown (the reader); plain `Text` is shown as written, every character kept
    Prose,
    /// a yes or no as `on` / `off`, for a switch
    OnOff,
    /// a yes or no as the tier's ✓ / ✗, for a result
    Tick,
}

impl Show {
    /// The renderer a look file names (`text duration time money bytes path cmd diff prose onoff tick`).
    pub fn parse(s: &str) -> Option<Show> {
        let all = [Show::Text, Show::Duration, Show::Time, Show::Money, Show::Bytes, Show::Path, Show::Cmd, Show::Diff, Show::Prose, Show::OnOff, Show::Tick];
        ["text", "duration", "time", "money", "bytes", "path", "cmd", "diff", "prose", "onoff", "tick"].iter().position(|w| *w == s).map(|i| all[i])
    }
}

/// The glyph and the colour of the kinds that start with `prefix`; the longest prefix that fits wins.
pub struct Family {
    pub prefix: String,
    pub slot: Slot,
    pub role: Hue,
}

/// A field shown its own way: `name` is a field's name, or `_ms` for every name that ends so.
pub struct FieldShow {
    pub name: String,
    pub show: Show,
}

/// The table drawing reads: per kind its glyph, colour and summary; per field how it is shown.
pub struct Look {
    pub families: Vec<Family>,
    /// The glyph and colour of a kind no family has.
    pub other: (Slot, Hue),
    /// kind → the summary of its line, `{data.name} {data.detail}`; a kind with none shows its first short fields.
    pub summaries: Vec<(String, String)>,
    pub fields: Vec<FieldShow>,
    /// The last word of a kind that makes its line bad (`run.error`, `grant.revoked`) or a warning.
    pub bad: Vec<String>,
    pub warn: Vec<String>,
    /// The fields of the event, not of its data, shown at the end of the tree, dim.
    pub tail: Vec<String>,
    /// How many fields a summary of the first fields takes.
    pub first: usize,
}

impl Default for Look {
    /// The bare look, before the core sent the look file: every kind in the look of a kind no family has.
    fn default() -> Look {
        Look::of(&Json::Null)
    }
}

impl Look {
    /// The look file the core sends (`look:file`): `{family: {prefix: {glyph, role}}, summary: {kind: text}, field: {name: show},
    /// tone: {word: bad|warn}}`. The empty prefix is the look of a kind no other family has; a name the theme does not know is left out.
    pub fn of(v: &Json) -> Look {
        let part = |p: &str| v[p].as_object().into_iter().flatten().map(|(k, v)| (k.clone(), v)).collect::<Vec<_>>();
        let pair = |f: &Json| Some((Slot::by_name(f["glyph"].as_str()?)?, Hue::parse(f["role"].as_str()?)?));
        let fam = part("family");
        let words = |t: &str| part("tone").into_iter().filter(|(_, v)| v.as_str() == Some(t)).map(|(k, _)| k).collect();
        Look {
            families: fam.iter().filter(|(p, _)| !p.is_empty()).filter_map(|(p, f)| pair(f).map(|(slot, role)| Family { prefix: p.clone(), slot, role })).collect(),
            other: fam.iter().find(|(p, _)| p.is_empty()).and_then(|(_, f)| pair(f)).unwrap_or((Slot::Control, Hue::Faint)),
            summaries: part("summary").into_iter().filter_map(|(k, t)| Some((k, t.as_str()?.to_string()))).collect(),
            fields: part("field").into_iter().filter_map(|(name, s)| Some(FieldShow { name, show: Show::parse(s.as_str()?)? })).collect(),
            bad: words("bad"),
            warn: words("warn"),
            tail: ["task", "thread", "seq", "cause", "by", "class", "at_ms", "repeats", "last_seq", "last_at_ms"].map(String::from).to_vec(),
            first: 3,
        }
    }

    /// The glyph and colour of a kind.
    pub fn family(&self, kind: &str) -> (Slot, Hue) {
        self.families.iter().filter(|f| kind.starts_with(&f.prefix)).max_by_key(|f| f.prefix.len()).map_or(self.other, |f| (f.slot, f.role))
    }

    /// How the field called `name` is shown: by its own name, else by the longest suffix it ends with.
    pub fn show(&self, name: &str) -> Show {
        // the name itself, else the longest `_suffix` it ends with
        let fits = |p: &str| if p.starts_with('_') { name.ends_with(p) && name.len() > p.len() } else { p == name };
        self.fields.iter().filter(|f| fits(&f.name)).max_by_key(|f| (!f.name.starts_with('_'), f.name.len())).map_or(Show::Text, |f| f.show)
    }

    /// Whether the event went wrong (bad), half wrong (warn) or not: by the last word of its kind, an `error` it carries, a shell's
    /// exit, or the tone it names.
    pub fn tone(&self, ev: &Json) -> Tone {
        let kind = ev["kind"].as_str().unwrap_or("");
        let last = kind.rsplit('.').next().unwrap_or("");
        let data = &ev["data"];
        let error = data.get("error").is_some_and(|e| !e.is_null() && e != &Json::Bool(false));
        let exit = data.get("exit").and_then(Json::as_i64).is_some_and(|x| x != 0);
        let named = data.get("tone").and_then(Json::as_str).unwrap_or("");
        if self.bad.iter().any(|b| b == last) || error || exit || matches!(named, "bad" | "error") {
            Tone::Bad
        } else if self.warn.iter().any(|w| w == last) || named == "warn" {
            Tone::Warn
        } else {
            Tone::Plain
        }
    }
}

// ---- values ----

/// `340 ms`, `1.2 s`, `2 m 05 s`, `1 h 02 m`.
pub fn duration(ms: f64) -> String {
    let s = ms / 1000.0;
    match ms {
        m if m < 1000.0 => format!("{} ms", m.round()),
        _ if s < 60.0 => format!("{s:.1} s"),
        _ if s < 3600.0 => format!("{} m {:02} s", (s / 60.0).floor(), (s % 60.0).floor()),
        _ => format!("{} h {:02} m", (s / 3600.0).floor(), ((s % 3600.0) / 60.0).floor()),
    }
}

/// `08:12:04`, the time of day in UTC.
pub fn clock(ms: u64) -> String {
    let s = ms / 1000 % 86_400;
    format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

/// `2025-01-02 08:12:04.120`, in UTC.
pub fn time(ms: u64) -> String {
    // days since 1970 to a date (Howard Hinnant's civil_from_days)
    let z = (ms / 86_400_000) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (d, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y}-{m:02}-{d:02} {}.{:03}", clock(ms), ms % 1000)
}

/// `512 B`, `4.0 kB`, `1.2 MB`.
pub fn bytes(n: f64) -> String {
    match n {
        n if n < 1024.0 => format!("{n} B"),
        n if n < 1024.0 * 1024.0 => format!("{:.1} kB", n / 1024.0),
        n => format!("{:.1} MB", n / 1024.0 / 1024.0),
    }
}

/// A value that is not a map or a list, as words, and the style it is drawn in.
fn scalar(th: &Theme, show: Show, v: &Json) -> (String, Style) {
    let plain = Style::default();
    match (v, show) {
        (Json::Null, _) => ("—".into(), th.fg(Hue::Faint)),
        (Json::Bool(b), Show::OnOff) => ((if *b { "on" } else { "off" }).into(), th.fg(if *b { Hue::Ok } else { Hue::Dim })),
        (Json::Bool(b), Show::Tick) => (th.glyph(if *b { Slot::Yes } else { Slot::No }).trim_end().into(), th.fg(if *b { Hue::Ok } else { Hue::Error })),
        (Json::Bool(b), _) => (b.to_string(), th.fg(if *b { Hue::Ok } else { Hue::Dim })),
        (Json::Number(n), Show::Duration) => (n.as_f64().map_or(n.to_string(), duration), plain),
        (Json::Number(n), Show::Time) => (n.as_u64().map_or(n.to_string(), time), plain),
        (Json::Number(n), Show::Money) => (n.as_f64().map_or(n.to_string(), |x| format!("${x:.4}")), plain),
        (Json::Number(n), Show::Bytes) => (n.as_f64().map_or(n.to_string(), bytes), plain),
        (Json::Number(n), _) => (n.to_string(), plain),
        (Json::String(s), Show::Path) => (s.clone(), th.fg(Hue::Accent)),
        (Json::String(s), Show::Cmd) => (s.clone(), th.fg(Hue::Run)),
        (Json::String(s), _) => (s.clone(), plain),
        (Json::Array(a), _) => (format!("[{}]", a.len()), th.fg(Hue::Faint)),
        (Json::Object(m), _) => (format!("{{{}}}", m.len()), th.fg(Hue::Faint)),
    }
}

// ---- the line ----

/// The value at `data.a.b` of the event, as one line of words.
fn at<'a>(ev: &'a Json, path: &str) -> Option<&'a Json> {
    path.split('.').try_fold(ev, |v, k| v.get(k).or_else(|| k.parse::<usize>().ok().and_then(|i| v.get(i))))
}

fn one_line(s: &str) -> String {
    let first = s.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    if s.trim().lines().nth(1).is_some() { format!("{first} …") } else { first.to_string() }
}

/// The summary of the event: its kind's template when the look has one, else its first short fields as `key value`.
pub fn summary(th: &Theme, look: &Look, ev: &Json, tone: Tone) -> Vec<Span<'static>> {
    let val = if tone == Tone::Plain { Style::default() } else { th.tone(tone) };
    let kind = ev["kind"].as_str().unwrap_or("");
    if let Some((_, tpl)) = look.summaries.iter().find(|(k, _)| k == kind) {
        let mut out = String::new();
        for part in tpl.split('{') {
            let (hole, rest) = part.split_once('}').unwrap_or(("", part));
            if !hole.is_empty() {
                match at(ev, hole) {
                    Some(Json::String(s)) => out.push_str(&one_line(s)),
                    Some(v) if !v.is_null() && !v.is_object() && !v.is_array() => out.push_str(&scalar(th, look.show(hole.rsplit('.').next().unwrap_or(hole)), v).0),
                    _ => {}
                }
            }
            out.push_str(rest);
        }
        let out = out.split_whitespace().collect::<Vec<_>>().join(" ");
        if !out.is_empty() {
            return vec![Span::styled(out, val)];
        }
    }
    let mut spans = Vec::new();
    match &ev["data"] {
        Json::Object(m) => {
            for (k, v) in m.iter().filter(|(_, v)| !v.is_object() && !v.is_array() && !v.is_null()).take(look.first) {
                let (text, style) = scalar(th, look.show(k), v);
                if !spans.is_empty() {
                    spans.push(Span::raw("  "));
                }
                spans.push(Span::styled(format!("{k} "), th.fg(Hue::Dim)));
                spans.push(Span::styled(one_line(&text), if tone == Tone::Plain { style } else { val }));
            }
            if spans.is_empty() && !m.is_empty() {
                spans.push(Span::styled(format!("{{{}}}", m.len()), th.fg(Hue::Faint)));
            }
        }
        Json::Null => {}
        Json::String(s) => spans.push(Span::styled(one_line(s), val)),
        v => spans.push(Span::styled(scalar(th, Show::Text, v).0, val)),
    }
    spans
}

/// The spans cut to `width` columns, the last with `…` when anything was cut.
pub fn cut(spans: Vec<Span<'static>>, width: usize) -> Vec<Span<'static>> {
    let mut out = Vec::new();
    let mut used = 0;
    for s in spans {
        let w = s.content.width();
        if used + w <= width {
            used += w;
            out.push(s);
            continue;
        }
        let t = fit(&s.content, width.saturating_sub(used));
        if !t.is_empty() {
            out.push(Span::styled(t, s.style));
        }
        break;
    }
    out
}

/// The line of an event, after the mark of the row the keys are on: time, glyph, kind padded to `kind_w`, summary, in `width`.
pub fn line(th: &Theme, look: &Look, ev: &Json, kind_w: usize, width: usize) -> Vec<Span<'static>> {
    let kind = ev["kind"].as_str().unwrap_or("?");
    let (slot, role) = look.family(kind);
    let tone = look.tone(ev);
    let (glyph, _) = th.mark(slot);
    let glyph = if glyph.ends_with(' ') { glyph } else { format!("{glyph} ") };
    let kind_style = if tone == Tone::Plain { th.fg(role) } else { th.tone(tone) };
    let mut spans = vec![
        Span::styled(format!("{} ", clock(ev["at_ms"].as_u64().unwrap_or(0))), th.fg(Hue::Dim)),
        Span::styled(glyph, th.fg(if tone == Tone::Bad { Hue::Error } else { role })),
        Span::styled(format!("{kind:<kind_w$}  "), kind_style),
    ];
    spans.extend(summary(th, look, ev, tone));
    cut(spans, width)
}

// ---- the tree ----

/// How deep the tree of the event goes: 1 for an event whose fields hold no map or list.
pub fn deepest(ev: &Json) -> u8 {
    fn deep(v: &Json) -> u8 {
        match v {
            Json::Object(m) if !m.is_empty() => 1 + m.values().map(deep).max().unwrap_or(0),
            Json::Array(a) if !a.is_empty() => 1 + a.iter().map(deep).max().unwrap_or(0),
            _ => 0,
        }
    }
    let data = match &ev["data"] {
        Json::Object(m) => m.values().map(deep).max().unwrap_or(0),
        v => deep(v),
    };
    1 + data
}

/// One line of the tree, as spans.
pub type Spans = Vec<Span<'static>>;

struct Tree<'a> {
    th: &'a Theme,
    /// How the pipes are drawn: `theme.frame`.
    pipes: layout::Look<'a>,
    read: &'a crate::read::Opts,
    look: &'a Look,
    depth: u8,
    width: usize,
    out: Vec<Spans>,
}

impl Tree<'_> {
    /// The items as rows of the tree under the row at `parent` (none: the top); those from `dim_from` on (and what is under them) are dim.
    fn items(&mut self, items: &[(String, &Json)], parent: Option<&Laid>, level: u8, dim_from: usize) {
        let key_w = items.iter().map(|(k, _)| k.width()).max().unwrap_or(0).min(18);
        let edge = self.th.fg(Hue::Edge);
        for (i, (k, v)) in items.iter().enumerate() {
            let dim = i >= dim_from;
            let key_style = self.th.fg(if dim { Hue::Faint } else { Hue::Dim });
            let laid = layout::place(parent, i, items.len());
            let branch = self.pipes.prefix(&laid);
            let under = self.pipes.under(&laid).concat();
            let head = vec![Span::styled(branch.clone(), edge), Span::styled(format!("{k:<key_w$}  "), key_style)];
            let kids: Vec<(String, &Json)> = match v {
                Json::Object(m) => m.iter().map(|(k, v)| (k.clone(), v)).collect(),
                Json::Array(a) => a.iter().enumerate().map(|(i, v)| ((i + 1).to_string(), v)).collect(),
                _ => Vec::new(),
            };
            if !kids.is_empty() {
                let mut l = head;
                if level >= self.depth {
                    let (n, _) = scalar(self.th, Show::Text, v);
                    l.push(Span::styled(format!("{n} {}", self.pipes.fold(Piece::FoldClosed)), self.th.fg(Hue::Faint)));
                    self.out.push(l);
                } else {
                    self.out.push(l);
                    self.items(&kids, Some(&laid), level + 1, if dim { 0 } else { usize::MAX });
                }
                continue;
            }
            let show = self.look.show(k);
            let (text, style) = scalar(self.th, show, v);
            let style = if dim { self.th.fg(Hue::Faint) } else { style };
            let room = self.width.saturating_sub(branch.width() + key_w + 2).max(8);
            let pad = format!("{under}{}", " ".repeat(key_w + 2));
            // a field the look file calls `prose` (a reason, a summary) is read as Markdown; every other string is a receipt, shown as written
            let prose = show == Show::Prose && matches!(v, Json::String(_));
            let parts: Vec<Vec<Span<'static>>> = if prose {
                crate::read::spans(self.th, self.read, style, &text, room)
            } else {
                wrap(&text, room)
                    .into_iter()
                    .map(|part| {
                        let style = match (show, part.chars().next()) {
                            (Show::Diff, Some('+')) => self.th.fg(Hue::Add),
                            (Show::Diff, Some('-')) => self.th.fg(Hue::Del),
                            _ => style,
                        };
                        vec![Span::styled(part, style)]
                    })
                    .collect()
            };
            for (j, part) in parts.into_iter().enumerate() {
                let mut l = if j == 0 { head.clone() } else { vec![Span::styled(pad.clone(), edge)] };
                l.extend(part);
                self.out.push(l);
            }
        }
    }
}

/// The whole event as a pipe tree `width` columns wide, opened `depth` levels (a map or a list deeper is one line with its size):
/// the fields of its data, then the event's own fields from the look's tail, dim. Strings are shown whole, wrapped.
pub fn tree(th: &Theme, read: &crate::read::Opts, look: &Look, ev: &Json, depth: u8, width: usize) -> Vec<Spans> {
    let mut items: Vec<(String, &Json)> = match &ev["data"] {
        Json::Object(m) => m.iter().map(|(k, v)| (k.clone(), v)).collect(),
        Json::Null => Vec::new(),
        v => vec![("data".to_string(), v)],
    };
    let body = items.len();
    items.extend(look.tail.iter().filter_map(|k| ev.get(k).filter(|v| !v.is_null()).map(|v| (k.clone(), v))));
    let mut t = Tree { th, pipes: layout::Look::tight(th), read, look, depth, width, out: Vec::new() };
    // the event's own fields hang from the same pipe as the data's, after them and dim
    t.items(&items, None, 1, body);
    t.out
}

/// The event as JSON, the way `r` shows it and `y` copies it.
pub fn raw(ev: &Json) -> String {
    format!("{ev:#}")
}

/// What tells a terminal to put `text` on the clipboard (OSC 52).
pub fn osc52(text: &str) -> String {
    const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::from("\x1b]52;c;");
    for chunk in text.as_bytes().chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= chunk.len() { B64[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out.push('\x07');
    out
}
