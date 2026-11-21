//! The screen as a golden: the text of each row, and the same rows with their style as ANSI. Both come from
//! the memory buffer a frame was drawn into, so what a golden shows is what a terminal would. Reading and writing the files is the
//! tests' business: this layer reads none, and only turns a buffer into text, compares two texts and checks the spacing.

use ratatui::buffer::{Buffer, Cell};
use ratatui::style::{Color, Modifier};
use unicode_width::UnicodeWidthStr;

/// The cells of a row that are drawn: the cell after a wide letter holds a blank the terminal does not draw.
fn drawn(row: &[Cell]) -> Vec<&Cell> {
    let (mut out, mut covered) = (Vec::new(), 0);
    for cell in row {
        if covered > 0 {
            covered -= 1;
            continue;
        }
        covered = cell.symbol().width().saturating_sub(1);
        out.push(cell);
    }
    out
}

fn rows(buffer: &Buffer) -> impl Iterator<Item = &[Cell]> {
    buffer.content().chunks((buffer.area.width as usize).max(1))
}

/// One string, a line per row, the blanks at the end of each cut off.
pub fn text(buffer: &Buffer) -> String {
    rows(buffer).map(|row| drawn(row).iter().map(|c| c.symbol()).collect::<String>().trim_end().to_string()).collect::<Vec<_>>().join("\n")
}

const MODIFIERS: [(Modifier, u8); 9] = [
    (Modifier::BOLD, 1),
    (Modifier::DIM, 2),
    (Modifier::ITALIC, 3),
    (Modifier::UNDERLINED, 4),
    (Modifier::SLOW_BLINK, 5),
    (Modifier::RAPID_BLINK, 6),
    (Modifier::REVERSED, 7),
    (Modifier::HIDDEN, 8),
    (Modifier::CROSSED_OUT, 9),
];

/// The SGR parameter of a colour; `back` for the background. `None` for the terminal's own colour.
fn colour(c: Color, back: bool) -> Option<String> {
    let base: u8 = if back { 40 } else { 30 };
    let named = |n: u8| Some((base + n).to_string());
    match c {
        Color::Reset => None,
        Color::Black => named(0),
        Color::Red => named(1),
        Color::Green => named(2),
        Color::Yellow => named(3),
        Color::Blue => named(4),
        Color::Magenta => named(5),
        Color::Cyan => named(6),
        Color::Gray => named(7),
        Color::DarkGray => named(60),
        Color::LightRed => named(61),
        Color::LightGreen => named(62),
        Color::LightYellow => named(63),
        Color::LightBlue => named(64),
        Color::LightMagenta => named(65),
        Color::LightCyan => named(66),
        Color::White => named(67),
        Color::Indexed(i) => Some(format!("{};5;{i}", base + 8)),
        Color::Rgb(r, g, b) => Some(format!("{};2;{r};{g};{b}", base + 8)),
    }
}

type Look = (Color, Color, Modifier);

fn look(c: &Cell) -> Look {
    (c.fg, c.bg, c.modifier)
}

/// The escape that sets a look from a clean start.
fn set(l: Look) -> String {
    let mut params: Vec<String> = vec!["0".into()];
    params.extend(MODIFIERS.iter().filter(|(m, _)| l.2.contains(*m)).map(|(_, n)| n.to_string()));
    params.extend(colour(l.0, false));
    params.extend(colour(l.1, true));
    format!("\x1b[{}m", params.join(";"))
}

/// The rows with their style as ANSI: a look is written when it changes, a row ends with a reset if it left one on, and the
/// cells at the end of a row that are blank in the terminal's own colours are cut off, as in [`text`].
pub fn ansi(buffer: &Buffer) -> String {
    let plain: Look = (Color::Reset, Color::Reset, Modifier::empty());
    let mut out = Vec::new();
    for row in rows(buffer) {
        let mut cells = drawn(row);
        while cells.last().is_some_and(|c| c.symbol().trim().is_empty() && look(c) == plain) {
            cells.pop();
        }
        let (mut line, mut now) = (String::new(), plain);
        for c in cells {
            if look(c) != now {
                now = look(c);
                line.push_str(&set(now));
            }
            line.push_str(c.symbol());
        }
        if now != plain {
            line.push_str("\x1b[0m");
        }
        out.push(line);
    }
    out.join("\n")
}

/// Where two goldens first differ, or `None` when they are the same. The rows are shown with their numbers.
pub fn differ(expected: &str, actual: &str) -> Option<String> {
    if expected == actual {
        return None;
    }
    let (e, a): (Vec<&str>, Vec<&str>) = (expected.lines().collect(), actual.lines().collect());
    for i in 0..e.len().max(a.len()) {
        let (x, y) = (e.get(i), a.get(i));
        if x != y {
            let show = |s: Option<&&str>| s.map_or("(no row)".to_string(), |s| format!("{:?}", s.replace('\x1b', "\\e")));
            return Some(format!("row {}: the golden has {}, the screen shows {}", i + 1, show(x), show(y)));
        }
    }
    Some("the text ends differently (a final line break)".into())
}

/// Whether a letter is an icon of a Nerd font: the private use areas, where Material Design sits in the second.
fn icon(c: char) -> bool {
    matches!(c, '\u{E000}'..='\u{F8FF}' | '\u{F0000}'..='\u{FFFFD}')
}

/// The places on a screen where an icon is not followed by a blank cell. A Nerd icon advances one cell but its ink is wider, so it
/// sits in a slot of two cells: itself and a blank. One at the very end of a row has room to spill.
pub fn spacing(screen: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (r, row) in screen.lines().enumerate() {
        let letters: Vec<char> = row.chars().collect();
        let mut col = 0;
        for (i, c) in letters.iter().enumerate() {
            if icon(*c) && letters.get(i + 1).is_some_and(|n| *n != ' ') {
                found.push(format!("row {}, column {}: the icon {c:?} is not followed by a blank cell", r + 1, col + 1));
            }
            col += c.to_string().width();
        }
    }
    found
}
