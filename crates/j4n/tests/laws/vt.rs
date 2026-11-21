//! A terminal in miniature for `tty_cases`: it reads the bytes the real screen sends to its pty and keeps the grid they make, so a
//! test sees the screen as the operator does. The screen paints only what changed, so a word typed arrives in pieces in the bytes and is
//! whole in the grid. It knows what ratatui's crossterm backend sends — a move, a print, an erase, the colours (ignored) and the modes
//! (ignored) — and nothing else.

pub struct Vt {
    cols: usize,
    rows: usize,
    grid: Vec<Vec<char>>,
    x: usize,
    y: usize,
    /// The bytes of a character that has not all arrived.
    carry: Vec<u8>,
    state: State,
}

enum State {
    Text,
    Esc,
    /// A control sequence and the characters of it so far.
    Csi(String),
    /// A string command (`ESC ]`), up to a bell or `ESC \`; true when the last character was an escape.
    Osc(bool),
    /// An escape that takes one more character (`ESC ( B`).
    Skip,
}

/// How many columns a character takes: two for the wide ones, none for those that join the one before.
fn width(c: char) -> usize {
    match c as u32 {
        0x0300..=0x036F | 0x200B..=0x200F | 0xFE00..=0xFE0F => 0,
        0x1100..=0x115F | 0x2E80..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFE30..=0xFE6F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6 | 0x1F300..=0x1F64F | 0x1F900..=0x1F9FF | 0x20000..=0x3FFFD => 2,
        _ => 1,
    }
}

impl Vt {
    pub fn new(cols: usize, rows: usize) -> Vt {
        Vt { cols, rows, grid: vec![vec![' '; cols]; rows], x: 0, y: 0, carry: Vec::new(), state: State::Text }
    }

    /// The terminal was made another size: what was drawn stays where it was, as far as it fits.
    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.grid.resize(rows, vec![' '; cols]);
        self.grid.iter_mut().for_each(|r| r.resize(cols, ' '));
        (self.cols, self.rows) = (cols, rows);
        (self.x, self.y) = (self.x.min(cols.saturating_sub(1)), self.y.min(rows.saturating_sub(1)));
    }

    /// Where the cursor is, as (column, row) from 0.
    pub fn cursor(&self) -> (usize, usize) {
        (self.x, self.y)
    }

    /// The rows of the grid, the blanks at the end of each cut.
    pub fn lines(&self) -> Vec<String> {
        self.grid.iter().map(|r| r.iter().filter(|c| **c != '\0').collect::<String>().trim_end().to_string()).collect()
    }

    pub fn text(&self) -> String {
        self.lines().join("\n")
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.carry.extend_from_slice(bytes);
        let valid = match std::str::from_utf8(&self.carry) {
            Ok(s) => s.len(),
            Err(e) => e.valid_up_to(),
        };
        let text = String::from_utf8_lossy(&self.carry[..valid]).into_owned();
        // a sequence cut short at the end waits for the rest; bytes that can never be a character are dropped
        let keep = if self.carry.len() - valid < 4 { self.carry[valid..].to_vec() } else { Vec::new() };
        self.carry = keep;
        text.chars().for_each(|c| self.step(c));
    }

    fn step(&mut self, c: char) {
        match std::mem::replace(&mut self.state, State::Text) {
            State::Text => match c {
                '\x1b' => self.state = State::Esc,
                '\r' => self.x = 0,
                '\n' => self.y = (self.y + 1).min(self.rows - 1),
                '\x08' => self.x = self.x.saturating_sub(1),
                c if c.is_control() => {}
                c => self.print(c),
            },
            State::Esc => match c {
                '[' => self.state = State::Csi(String::new()),
                ']' => self.state = State::Osc(false),
                '(' | ')' | '#' => self.state = State::Skip,
                _ => {}
            },
            State::Skip => {}
            State::Osc(esc) => match (c, esc) {
                ('\x07', _) | ('\\', true) => {}
                (c, _) => self.state = State::Osc(c == '\x1b'),
            },
            State::Csi(mut params) => {
                if ('\u{40}'..='\u{7e}').contains(&c) {
                    self.csi(&params, c);
                } else {
                    params.push(c);
                    self.state = State::Csi(params);
                }
            }
        }
    }

    fn print(&mut self, c: char) {
        let w = width(c);
        if w == 0 {
            return;
        }
        if self.x + w > self.cols {
            self.x = 0;
            self.y = (self.y + 1).min(self.rows - 1);
        }
        self.grid[self.y][self.x] = c;
        // the cell after a wide character belongs to it
        if w == 2 {
            self.grid[self.y][self.x + 1] = '\u{0}';
        }
        self.x = (self.x + w).min(self.cols);
    }

    fn csi(&mut self, params: &str, last: char) {
        let n: Vec<usize> = params.trim_start_matches(['?', '>', '=']).split(';').map(|p| p.parse().unwrap_or(0)).collect();
        let one = |i: usize| n.get(i).copied().filter(|v| *v > 0).unwrap_or(1);
        let zero = n.first().copied().unwrap_or(0);
        if params.starts_with(['?', '>', '=']) {
            return;
        }
        match last {
            'H' | 'f' => (self.y, self.x) = ((one(0) - 1).min(self.rows - 1), (one(1) - 1).min(self.cols - 1)),
            'A' => self.y = self.y.saturating_sub(one(0)),
            'B' => self.y = (self.y + one(0)).min(self.rows - 1),
            'C' => self.x = (self.x + one(0)).min(self.cols - 1),
            'D' => self.x = self.x.saturating_sub(one(0)),
            'G' => self.x = (one(0) - 1).min(self.cols - 1),
            'd' => self.y = (one(0) - 1).min(self.rows - 1),
            'J' => match zero {
                0 => {
                    self.grid[self.y][self.x..].fill(' ');
                    self.grid[self.y + 1..].iter_mut().for_each(|r| r.fill(' '));
                }
                1 => {
                    self.grid[..self.y].iter_mut().for_each(|r| r.fill(' '));
                    self.grid[self.y][..=self.x.min(self.cols - 1)].fill(' ');
                }
                _ => self.grid.iter_mut().for_each(|r| r.fill(' ')),
            },
            'K' => match zero {
                0 => self.grid[self.y][self.x..].fill(' '),
                1 => self.grid[self.y][..=self.x.min(self.cols - 1)].fill(' '),
                _ => self.grid[self.y].fill(' '),
            },
            'X' => {
                let end = (self.x + one(0)).min(self.cols);
                self.grid[self.y][self.x..end].fill(' ');
            }
            _ => {}
        }
    }
}

#[test]
fn the_terminal_in_miniature_keeps_what_the_bytes_paint_and_nothing_else() {
    let mut t = Vt::new(10, 3);
    // a word painted in pieces, a move, colours, a mode, a wide letter, a character cut in two by the read
    t.feed(b"\x1b[?2026h\x1b[1;1H\x1b[38;2;1;2;3mwo\x1b[0m");
    t.feed(b"\x1b[1;3Hrd\x1b[?2026l\x1b[2;2H");
    let wide = "世".as_bytes();
    t.feed(&wide[..1]);
    t.feed(&wide[1..]);
    t.feed(b"x");
    assert_eq!(t.lines(), ["word", " 世x", ""]);
    assert_eq!(t.cursor(), (4, 1));
    t.feed(b"\x1b[2;1H\x1b[K\x1b]0;a title\x07\x1b[3;1Hz");
    assert_eq!(t.lines(), ["word", "", "z"]);
    t.feed(b"\x1b[2J");
    assert_eq!(t.text(), "\n\n");
    t.resize(4, 2);
    t.feed(b"\x1b[2;3Hab\x1b[2;1Hcd");
    assert_eq!(t.lines(), ["", "cdab"], "the grid is the size it was made");
    t.feed(b"\x1b[1;4Hxy");
    assert_eq!(t.lines(), ["   x", "ydab"], "a line past the edge goes on the next");
}
