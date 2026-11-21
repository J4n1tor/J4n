//! The theme: a palette of twenty-four roles, a tier of glyphs, and the options that shape what is drawn.
//! Every colour on the screen is a role and every mark a slot; `NO_COLOR` turns the roles into attributes and keeps the glyphs.
//! The values arrive as [`Settings`] (words, as the Table holds them) and are checked once, in [`Theme::build`].

pub mod color;
pub mod glyph;
pub mod palette;
pub mod sheet;

use glyph::{Slot, Tier};
use j4n_proto::Tone;
use palette::{Floors, Palette, Role, Roles};
use ratatui::style::{Color, Modifier, Style};

macro_rules! choice {
    ($(#[$m:meta])* $name:ident { $($v:ident $w:literal),+ }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name { $($v),+ }

        impl $name {
            pub const WORDS: &'static [&'static str] = &[$($w),+];

            pub fn parse(s: &str) -> Option<$name> {
                match s { $($w => Some($name::$v),)+ _ => None }
            }

            pub fn word(self) -> &'static str {
                match self { $($name::$v => $w),+ }
            }
        }
    };
}

choice!(
    /// The look of the trees and the chat's frame (`theme.frame`).
    Frame { Pipes "pipes", Gutter "gutter", Hang "hang", Round "round", Airy "airy" }
);
choice!(Corners { Round "round", Square "square" });
choice!(TitlePlace { Inside "inside", Border "border", None "none" });
choice!(Density { Compact "compact", Normal "normal", Roomy "roomy" });
choice!(
    /// What paints the background: `match` sets the terminal's own background to the palette's (OSC 11) and gives it back on the way
    /// out, `terminal` paints none, `paint` paints every cell and leaves the terminal as it is.
    Fill { Match "match", Terminal "terminal", Paint "paint" }
);
choice!(
    /// How the operator's line is drawn in the chat: `bracket` (the default) is their words plain between two caps `▌ ▐` in the accent, the
    /// head of the tree under it; `band` is their words on a raised band with the time at its right, no icon; `mark` has no band and no icon.
    You { Bracket "bracket", Band "band", Mark "mark" }
);

/// The theme as the Table holds it: words and flags, none of them checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
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
    pub no_color: bool,
    /// How the charts are drawn (`look.chart`): `dither` (half-block pixels where the terminal has truecolor) or `plain` (braille and blocks).
    pub chart: String,
    /// The terminal says it has truecolor (`COLORTERM`); the roles are drawn as such either way, but the dithered charts need it.
    pub truecolor: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { palette: "jjj".into(), marks: "icons".into(), frame: "round".into(), corners: "round".into(), title_place: "inside".into(), density: "normal".into(), fill: "match".into(), you: "bracket".into(), motion: true, mouse: "on".into(), no_color: false, chart: "dither".into(), truecolor: true }
    }
}

impl Settings {
    /// The defaults, with colour off when `NO_COLOR` is set and not empty (no-color.org).
    pub fn with_no_color_var(var: Option<&std::ffi::OsStr>) -> Settings {
        Settings { no_color: var.is_some_and(|v| !v.is_empty()), ..Settings::default() }
    }

    /// The same, with `COLORTERM` read: `truecolor` or `24bit` is a terminal that draws any colour; anything else, or none, is not.
    pub fn with_colorterm(self, var: Option<&std::ffi::OsStr>) -> Settings {
        Settings { truecolor: var.is_some_and(|v| v == "truecolor" || v == "24bit"), ..self }
    }
}

#[derive(Debug, Clone)]
pub struct Theme {
    pub palette: String,
    pub roles: Roles,
    pub floors: Floors,
    pub tier: Tier,
    pub frame: Frame,
    pub corners: Corners,
    pub title_place: TitlePlace,
    pub density: Density,
    pub fill: Fill,
    pub you: You,
    pub motion: bool,
    /// The mouse is reported to J4n (the wheel scrolls), and ⇧-drag selects.
    pub mouse: bool,
    pub no_color: bool,
    /// The charts are drawn as dithered half blocks where they can be (`look.chart`), and the terminal has truecolor.
    pub dither: bool,
    pub truecolor: bool,
}

impl Default for Theme {
    fn default() -> Theme {
        Theme::build(&Settings::default(), &[]).unwrap_or_else(|e| unreachable!("the default theme is valid: {e}"))
    }
}

fn pick<T>(what: &str, word: &str, parse: impl Fn(&str) -> Option<T>, words: &[&str]) -> Result<T, String> {
    parse(word).ok_or_else(|| format!("theme.{what} is `{word}`; it is one of {}", words.join(", ")))
}

impl Theme {
    /// The theme the settings name. `palettes` are the operator's, found before the six built in (an `id` the same replaces one).
    /// A palette whose roles do not meet its floors is refused with the roles that fall short.
    pub fn build(s: &Settings, palettes: &[Palette]) -> Result<Theme, String> {
        let builtin = palette::builtin();
        let chosen = palettes.iter().chain(builtin.iter()).find(|p| p.id == s.palette).ok_or_else(|| {
            let ids: Vec<&str> = palettes.iter().chain(builtin.iter()).map(|p| p.id.as_str()).collect();
            format!("theme.palette is `{}`; it is one of {}", s.palette, ids.join(", "))
        })?;
        if let short @ [_, ..] = &chosen.short()[..] {
            return Err(format!("palette `{}` cannot be read: {}", chosen.id, short.join("; ")));
        }
        Ok(Theme {
            palette: chosen.id.clone(),
            roles: chosen.roles(),
            floors: chosen.floors,
            tier: pick("marks", &s.marks, Tier::parse, &Tier::ALL.map(Tier::name))?,
            frame: pick("frame", &s.frame, Frame::parse, Frame::WORDS)?,
            corners: pick("corners", &s.corners, Corners::parse, Corners::WORDS)?,
            title_place: pick("title_place", &s.title_place, TitlePlace::parse, TitlePlace::WORDS)?,
            density: pick("density", &s.density, Density::parse, Density::WORDS)?,
            fill: pick("fill", &s.fill, Fill::parse, Fill::WORDS)?,
            you: pick("you", &s.you, You::parse, You::WORDS)?,
            motion: s.motion,
            mouse: pick("mouse", &s.mouse, |w| match w { "on" => Some(true), "off" => Some(false), _ => None }, &["on", "off"])?,
            no_color: s.no_color,
            dither: s.chart != "plain",
            truecolor: s.truecolor,
        })
    }

    fn rgb(&self, r: Role) -> Color {
        let c = self.roles.get(r);
        Color::Rgb(c.0, c.1, c.2)
    }

    /// A role as the colour of text. Without colour it is the attribute that keeps its meaning.
    pub fn fg(&self, r: Role) -> Style {
        if self.no_color {
            return Style::default().add_modifier(r.attributes());
        }
        let style = Style::default().fg(self.rgb(r));
        if matches!(r, Role::You | Role::Focus) { style.add_modifier(Modifier::BOLD) } else { style }
    }

    /// A role as a background: `raise`, `sel`. Without colour only the selection shows, reversed.
    pub fn back(&self, r: Role) -> Style {
        match (self.no_color, r) {
            (false, _) => Style::default().bg(self.rgb(r)),
            (true, Role::Sel) => Style::default().add_modifier(Modifier::REVERSED),
            (true, _) => Style::default(),
        }
    }

    /// A faint wash of a role over the background, for a band that is not the raised one (the notice band): the background moved a
    /// seventh of the way toward the role's colour. Without colour there is no wash.
    pub fn tint(&self, r: Role) -> Style {
        if self.no_color {
            return Style::default();
        }
        let (bg, to) = (self.roles.get(Role::Bg), self.roles.get(r));
        let c = color::Rgb(bg.0, bg.1, bg.2).mix(color::Rgb(to.0, to.1, to.2), 0.14);
        Style::default().bg(Color::Rgb(c.0, c.1, c.2))
    }

    /// What every cell starts as: the text on the background; the terminal's own colours without colour, and the terminal's own
    /// background with `fill = terminal`.
    pub fn base(&self) -> Style {
        match (self.no_color, self.fill) {
            (true, _) => Style::default(),
            (false, Fill::Terminal) => Style::default().fg(self.rgb(Role::Text)),
            (false, _) => Style::default().fg(self.rgb(Role::Text)).bg(self.rgb(Role::Bg)),
        }
    }

    /// The colour the terminal's own background is to be set to while the screen is up: the palette's `Bg` with `fill = match`, else none.
    pub fn ground(&self) -> Option<(u8, u8, u8)> {
        (!self.no_color && self.fill == Fill::Match).then(|| self.roles.get(Role::Bg)).map(|c| (c.0, c.1, c.2))
    }

    /// How a cell or a line of the protocol reads.
    pub fn tone(&self, t: Tone) -> Style {
        match t {
            Tone::Plain => Style::default(),
            Tone::Dim => self.fg(Role::Dim),
            Tone::Good => self.fg(Role::Ok),
            Tone::Warn => self.fg(Role::Warn),
            Tone::Bad => self.fg(Role::Error),
            Tone::Accent => self.fg(Role::Accent),
        }
    }

    /// The glyph of a slot in the tier, with the corner the theme wants.
    pub fn glyph(&self, slot: Slot) -> &'static str {
        let slot = if self.corners == Corners::Square { slot.squared() } else { slot };
        slot.cell(self.tier)
    }

    /// The name of the doer that is working: the accent at the full, down to its soft twin and back over eight ticks (`phase` 0–7).
    /// Without motion, or without colour, it is the accent in bold and does not change.
    pub fn breath(&self, phase: u8) -> Style {
        if self.no_color || !self.motion {
            return self.fg(Role::Accent).add_modifier(Modifier::BOLD);
        }
        let (a, s) = (self.roles.get(Role::Accent), self.roles.get(Role::Soft));
        let k = f64::from(if phase <= 4 { phase } else { 8 - phase }) / 4.0;
        let c = color::Rgb(a.0, a.1, a.2).mix(color::Rgb(s.0, s.1, s.2), k);
        Style::default().fg(Color::Rgb(c.0, c.1, c.2))
    }

    /// The background and the text swapped: what the chosen one of a few looks like. Without colour it is reversed video.
    pub fn inverse(&self) -> Style {
        if self.no_color {
            return Style::default().add_modifier(Modifier::REVERSED);
        }
        let (bg, accent) = (self.roles.get(Role::Bg), self.roles.get(Role::Accent));
        Style::default().fg(Color::Rgb(bg.0, bg.1, bg.2)).bg(Color::Rgb(accent.0, accent.1, accent.2))
    }

    /// A key that is live only now, in the row of keys (the newest note's act, the selected row's own keys): its leading mark and
    /// its style, the accent. The one place that says how a temporary hint looks, so how a temporary hint looks is changed here alone.
    pub fn temp_hint(&self) -> (&'static str, Style) {
        (self.glyph(Slot::Bullet), self.fg(Role::Accent))
    }

    /// The slot drawn in its role: its glyph, and the blank that makes an icon a slot of two cells.
    pub fn mark(&self, slot: Slot) -> (String, Style) {
        let g = self.glyph(slot);
        let icon = g.chars().next().is_some_and(|c| c >= '\u{E000}');
        (if icon { format!("{g} ") } else { g.to_string() }, self.fg(slot.role()))
    }
}
