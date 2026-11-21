//! A palette: nine colours given (or six, the other three turned out of them) and the twenty-four roles made from them by one function.
//! Ratios and floors are data; the contrast pass keeps `dim`, `faint` and `soft` readable.

use super::color::{Rgb, pass, reach};
use ratatui::style::Modifier;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Bg,
    Raise,
    Sel,
    Text,
    Dim,
    Faint,
    Edge,
    Accent,
    Soft,
    Ok,
    Warn,
    Error,
    Route,
    Run,
    Fetch,
    You,
    Made,
    Add,
    Del,
    Focus,
    Strong,
    Code,
    Link,
    Head,
}

impl Role {
    pub const ALL: [Role; 24] = [
        Role::Bg, Role::Raise, Role::Sel, Role::Text, Role::Dim, Role::Faint, Role::Edge, Role::Accent, Role::Soft, Role::Ok,
        Role::Warn, Role::Error, Role::Route, Role::Run, Role::Fetch, Role::You, Role::Made, Role::Add, Role::Del, Role::Focus,
        Role::Strong, Role::Code, Role::Link, Role::Head,
    ];

    pub fn name(self) -> &'static str {
        ["bg", "raise", "sel", "text", "dim", "faint", "edge", "accent", "soft", "ok", "warn", "error", "route", "run", "fetch", "you", "made", "add", "del", "focus", "strong", "code", "link", "head"][self as usize]
    }

    pub fn parse(s: &str) -> Option<Role> {
        Role::ALL.into_iter().find(|r| r.name() == s)
    }

    /// How the role is made, in words, for the sheet.
    pub fn from(self) -> &'static str {
        [
            "input", "mix bg→accent 7%", "mix bg→accent 16%", "input", "text→bg ≤41% · ≥4.5", "text→bg ≤60% · ≥3.0", "mix text→bg 72%",
            "input · ≥4.5", "accent→bg ≤26% · ≥4.7", "input · ≥4.5", "input · ≥4.5", "input · ≥4.5", "input · else accent↻60°", "input · else warn↻−20°",
            "input · else accent⇄ok", "= accent, bold", "= faint (the ~ mark)", "= ok", "= error", "= accent, bold",
            "= text, bold", "= accent", "= fetch, underlined", "= accent, bold",
            "= text, bold", "= accent on raise", "= fetch, underlined", "= accent, bold",
        ][self as usize]
    }

    /// What the role is without colour (`NO_COLOR`): the attribute that keeps its meaning.
    pub fn attributes(self) -> Modifier {
        match self {
            Role::Sel => Modifier::REVERSED,
            Role::Accent | Role::Soft | Role::You | Role::Focus | Role::Strong | Role::Head => Modifier::BOLD,
            Role::Code => Modifier::REVERSED,
            Role::Link => Modifier::UNDERLINED,
            Role::Dim | Role::Faint | Role::Edge | Role::Made => Modifier::DIM,
            Role::Warn => Modifier::UNDERLINED,
            Role::Error => Modifier::BOLD | Modifier::UNDERLINED,
            _ => Modifier::empty(),
        }
    }

    /// Whether the role is a background (the others are colours of text).
    pub fn is_back(self) -> bool {
        matches!(self, Role::Bg | Role::Raise | Role::Sel)
    }
}

/// The colours a palette is given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inputs {
    pub bg: Rgb,
    pub text: Rgb,
    pub accent: Rgb,
    pub ok: Rgb,
    pub warn: Rgb,
    pub error: Rgb,
    pub route: Option<Rgb>,
    pub run: Option<Rgb>,
    pub fetch: Option<Rgb>,
}

/// How far each derived role is from its colours (`[palette.derive]`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Derive {
    pub dim: f64,
    pub faint: f64,
    pub edge: f64,
    pub raise: f64,
    pub sel: f64,
    pub soft: f64,
}

impl Default for Derive {
    fn default() -> Derive {
        Derive { dim: 0.41, faint: 0.60, edge: 0.72, raise: 0.07, sel: 0.16, soft: 0.26 }
    }
}

/// The contrast each role that must be read has to have on the worst of `bg`, `raise` and `sel` (`palette.floors`); `hue` is the
/// floor of the accent and the hues (`ok`, `warn`, `error`, `route`, `run`, `fetch`) that are text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Floors {
    pub dim: f64,
    pub faint: f64,
    pub soft: f64,
    pub hue: f64,
}

impl Default for Floors {
    fn default() -> Floors {
        Floors { dim: 4.5, faint: 3.0, soft: 4.7, hue: 4.5 }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Palette {
    pub id: String,
    pub inputs: Inputs,
    pub derive: Derive,
    pub floors: Floors,
    /// Roles set by hand (`[palette.roles]`), after the derivation.
    pub pins: BTreeMap<Role, Rgb>,
}

/// The twenty-four colours.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Roles([Rgb; 24]);

impl Roles {
    pub fn get(&self, r: Role) -> Rgb {
        self.0[r as usize]
    }

    /// The least contrast the role has on the three backgrounds, and the name of the one it is on.
    pub fn worst(&self, r: Role) -> (f64, &'static str) {
        let c = self.get(r);
        [Role::Bg, Role::Raise, Role::Sel].into_iter().map(|b| (c.contrast(self.get(b)), b.name())).min_by(|a, b| a.0.total_cmp(&b.0)).unwrap_or((21.0, "bg"))
    }
}

impl Palette {
    pub fn new(id: &str, inputs: Inputs) -> Palette {
        Palette { id: id.into(), inputs, derive: Derive::default(), floors: Floors::default(), pins: BTreeMap::new() }
    }

    /// The roles: one function of the inputs, the ratios and the floors.
    pub fn roles(&self) -> Roles {
        let (i, d, f) = (&self.inputs, &self.derive, &self.floors);
        let (raise, sel) = (i.bg.mix(i.accent, d.raise), i.bg.mix(i.accent, d.sel));
        let backs = [i.bg, raise, sel];
        let faint = pass(i.text, &backs, d.faint, f.faint);
        let (accent, ok, warn, error) = (reach(i.accent, i.text, &backs, f.hue), reach(i.ok, i.text, &backs, f.hue), reach(i.warn, i.text, &backs, f.hue), reach(i.error, i.text, &backs, f.hue));
        let mut c = [i.bg; 24];
        let mut set = |r: Role, v: Rgb| c[r as usize] = v;
        set(Role::Bg, i.bg);
        set(Role::Raise, raise);
        set(Role::Sel, sel);
        set(Role::Text, i.text);
        set(Role::Dim, pass(i.text, &backs, d.dim, f.dim));
        set(Role::Faint, faint);
        set(Role::Edge, i.text.mix(i.bg, d.edge));
        set(Role::Accent, accent);
        set(Role::Soft, pass(reach(i.accent, i.text, &backs, f.soft), &backs, d.soft, f.soft));
        set(Role::Ok, ok);
        set(Role::Warn, warn);
        set(Role::Error, error);
        let (route, run, fetch) = (i.route.unwrap_or_else(|| i.accent.rotate(60.0)), i.run.unwrap_or_else(|| i.warn.rotate(-20.0)), i.fetch.unwrap_or_else(|| i.accent.mix(i.ok, 0.5)));
        let (route, run, fetch) = (reach(route, i.text, &backs, f.hue), reach(run, i.text, &backs, f.hue), reach(fetch, i.text, &backs, f.hue));
        set(Role::Route, route);
        set(Role::Run, run);
        set(Role::Fetch, fetch);
        set(Role::You, accent);
        set(Role::Made, faint);
        set(Role::Add, ok);
        set(Role::Del, error);
        set(Role::Focus, accent);
        set(Role::Strong, i.text);
        set(Role::Code, accent);
        set(Role::Link, fetch);
        set(Role::Head, accent);
        for (r, v) in &self.pins {
            set(*r, *v);
        }
        Roles(c)
    }

    /// The floors the roles do not meet, as words. A palette whose text cannot be read on its background cannot meet them at all.
    pub fn short(&self) -> Vec<String> {
        let r = self.roles();
        let f = &self.floors;
        [(Role::Dim, f.dim), (Role::Faint, f.faint), (Role::Soft, f.soft), (Role::Accent, f.hue), (Role::Ok, f.hue), (Role::Warn, f.hue), (Role::Error, f.hue), (Role::Route, f.hue), (Role::Run, f.hue), (Role::Fetch, f.hue)]
            .into_iter()
            .map(|(role, floor)| (role, floor, r.worst(role)))
            .filter(|(_, floor, (got, _))| *got < *floor)
            .map(|(role, floor, (got, on))| format!("{}: {got:.2}:1 on {on}, the floor is {floor}:1", role.name()))
            .collect()
    }
}

/// The six built-in palettes: `jjj` (the default), `dusk`, `paper`, `ember`, `mono`, `contrast`.
pub fn builtin() -> Vec<Palette> {
    const P: [(&str, [&str; 9]); 6] = [
        ("jjj", ["#161a20", "#cdd2d8", "#7caede", "#83c795", "#d8bf7a", "#e07e79", "#b199d7", "#e2a373", "#7dc5c4"]),
        ("dusk", ["#1a1824", "#d4d0de", "#9aa8f0", "#8ccaa0", "#dcc07e", "#e68a8a", "#c49ae0", "#e6a77c", "#7fc6cf"]),
        ("paper", ["#f3f0e8", "#2a2e35", "#2c6aa3", "#2f7a4c", "#8a6510", "#b23c35", "#7048a8", "#a85a1c", "#1f7c7a"]),
        ("ember", ["#1b1613", "#dccfc2", "#e2a373", "#9cc58a", "#e0c070", "#e47f6f", "#c79ad8", "#e8b48a", "#86c3b8"]),
        ("mono", ["#141414", "#d0d0d0", "#ffffff", "#d0d0d0", "#d0d0d0", "#ffffff", "#d0d0d0", "#d0d0d0", "#d0d0d0"]),
        ("contrast", ["#000000", "#ffffff", "#6cc4ff", "#5ee38f", "#ffd75f", "#ff6e6e", "#d0a8ff", "#ffb070", "#5fe0e0"]),
    ];
    P.iter()
        .map(|(id, h)| {
            let c = |i: usize| Rgb::parse(h[i]).unwrap_or(Rgb(0, 0, 0));
            Palette::new(id, Inputs { bg: c(0), text: c(1), accent: c(2), ok: c(3), warn: c(4), error: c(5), route: Some(c(6)), run: Some(c(7)), fetch: Some(c(8)) })
        })
        .collect()
}
