//! The sweep. The real front and the real TUI, wired as the binary wires them, drawn into memory at 60, 80,
//! 120 and 160 columns, in the palettes `jjj`, `paper` and `mono`, in the tiers `icons` and `unicode`. Every page and sub-tab of the tree,
//! the chat in its states, and the overlays (the `?` sheet, the `⌃k` jump, a question) are written as text and as ANSI, so a reader can
//! go through them pass by pass.
//!
//! `J4N_SWEEP=<dir> scripts/t test -p j4n --test laws the_sweep` writes `<dir>/<palette>-<tier>/<cols>/<name>.{txt,ansi}` and an `index.txt`;
//! without the variable it does nothing (the matrix is 24 sessions and is never committed). `J4N_SWEEP_ONLY=jjj-icons` runs one look.
//! What is committed is small: `the_pages_at_160_columns_are_their_goldens` (one golden per page, jjj/icons) and one golden per finding.

use crate::screen_drive::{Operator, asked};
use crate::run_cases::Dirs;
use crate::support::{read_text, root};
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use j4n_core::task::Mode;
use j4n_tui::headless::{Headless, KeyCode, alt_key, ctrl, press, typed};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const SIZES: [(u16, u16); 4] = [(60, 24), (80, 24), (120, 36), (160, 44)];
const PALETTES: [&str; 3] = ["jjj", "paper", "mono"];
const TIERS: [&str; 2] = ["icons", "unicode"];

fn opts() -> Opts {
    Opts { doer: Some("test".into()), model: None, approval: Some(Approval::Ask), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false }
}

/// A hermetic project and home with the look `palette`/`tier` and the packs that wait for a switch turned on, so their pages are there.
fn home(palette: &str, tier: &str, tag: &str) -> Dirs {
    let d = Dirs::new(tag);
    let ids = [("theme.palette", format!("\"{palette}\"")), ("theme.marks", format!("\"{tier}\"")), ("inbox.mode", "\"on\"".into()), ("habits.mode", "\"on\"".into()), ("ledger.mode", "\"on\"".into()), ("kit.mode", "\"on\"".into())];
    let rows: String = ids.iter().map(|(id, v)| format!("[[setting]]\nid = \"{id}\"\nvalue = {v}\n")).collect();
    fs::write(d.j4n_home().join("settings/sweep.toml"), format!("api = 1\n{rows}")).unwrap();
    d
}

fn open(d: &Dirs) -> World {
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: "claude".into() };
    crate::support::opened(|| wire::open(setup.clone(), &opts(), true, "main"))
}

/// One frame of the screen as the operator has it, after what the core sends has been taken in and the picture has held still.
pub(crate) struct Sitting {
    pub(crate) op: Operator,
    cols: u16,
    rows: u16,
}

impl Sitting {
    pub(crate) fn new(world: &World, (cols, rows): (u16, u16)) -> Sitting {
        let mut op = Operator::sit(world);
        op.screen = Headless::new(cols, rows);
        op.up.send(j4n_proto::Up::Resize { cols, rows }).unwrap();
        let mut s = Sitting { op, cols, rows };
        s.settle();
        s
    }

    /// Take in what the core sends until two looks, 60 ms apart, are the same. A screen that has not held still when the generous
    /// deadline passes (a loaded machine is slow, not stuck) is a failure with the screen in it, never a picture taken half-way.
    pub(crate) fn settle(&mut self) -> String {
        let (mut last, mut held, end) = (String::new(), 0, Instant::now() + crate::support::PATIENCE);
        loop {
            while let Ok(d) = self.op.down.try_recv() {
                for u in self.op.app.down(d) {
                    self.op.up.send(u).unwrap();
                }
            }
            for u in self.op.app.tick(Instant::now()) {
                self.op.up.send(u).unwrap();
            }
            let now = self.op.screen.show(&mut self.op.app);
            held = if now == last { held + 1 } else { 0 };
            if held >= 2 {
                return now;
            }
            assert!(Instant::now() < end, "the screen at {}x{} never held still; it is\n{now}", self.cols, self.rows);
            last = now;
            std::thread::sleep(Duration::from_millis(60));
        }
    }

    pub(crate) fn keys(&mut self, keys: Vec<j4n_tui::headless::KeyEvent>) -> String {
        self.op.press(keys);
        self.settle()
    }

    pub(crate) fn line(&mut self, text: &str) -> String {
        self.keys(typed(text, true))
    }

    /// Until `ready` holds of the settled screen (a turn takes time).
    pub(crate) fn until(&mut self, what: &str, ready: impl Fn(&str) -> bool) -> String {
        let end = Instant::now() + crate::support::PATIENCE;
        loop {
            let s = self.settle();
            if ready(&s) {
                return s;
            }
            assert!(Instant::now() < end, "never saw {what}; the screen is\n{s}");
        }
    }

    /// The terminal is made `cols` by `rows`: the core is told, the screen is drawn at the new size.
    fn resize(&mut self, cols: u16, rows: u16) -> String {
        self.op.screen = Headless::new(cols, rows);
        self.op.up.send(j4n_proto::Up::Resize { cols, rows }).unwrap();
        self.cols = cols;
        self.rows = rows;
        self.settle()
    }

    fn snap(&mut self) -> j4n_tui::headless::Snap {
        self.op.screen.snap(&mut self.op.app)
    }
}

struct Out {
    dir: PathBuf,
    index: Vec<String>,
    /// The goldens the pages are held to while they are taken (the committed run): a page is waited for until it is its golden.
    want: Option<PathBuf>,
    size: (u16, u16),
}

impl Out {
    fn write(&self, name: &str, snap: &j4n_tui::headless::Snap) {
        fs::create_dir_all(&self.dir).unwrap();
        fs::write(self.dir.join(format!("{name}.txt")), format!("{}\n", snap.text)).unwrap();
        fs::write(self.dir.join(format!("{name}.ansi")), format!("{}\n", snap.ansi)).unwrap();
    }

    fn put(&mut self, s: &mut Sitting, name: &str) {
        let mut snap = s.snap();
        self.write(name, &snap);
        // a page of the Usage tab is marked, for the length of its meters
        let app = &s.op.app;
        if app.top().and_then(|t| app.open_of(t)).is_some_and(|t| t.title == "Usage") {
            fs::write(self.dir.join(format!("{name}.usage")), "").unwrap();
        }
        // A page whose rows come from the core a moment after its frame (a probe of the machine, which a loaded one is slow at) holds
        // still before it is whole. Its golden is what whole is: the page is taken again until it is its golden, and one that never is
        // goes on to be reported with its difference, once the patience is out.
        if let Some(want) = self.want.clone().filter(|_| !UNSTEADY.iter().any(|u| name.starts_with(u))) {
            let golden = read_text(&want.join(format!("{name}.{}x{}.txt", self.size.0, self.size.1)));
            let end = Instant::now() + crate::support::PATIENCE;
            while golden.is_some() && read_text(&self.dir.join(format!("{name}.txt"))).map(|t| steady(&t)) != golden && Instant::now() < end {
                std::thread::sleep(Duration::from_millis(100));
                s.settle();
                snap = s.snap();
                self.write(name, &snap);
            }
        }
        let strip: Vec<&str> = snap.text.lines().take(4).map(str::trim_end).collect();
        self.index.push(format!("{name}  |  {}", strip.join(" ¦ ")));
    }
}

const RIGHT: fn() -> j4n_tui::headless::KeyEvent = || alt_key(KeyCode::Right);

/// Whether turn `tN` is over: its `→ tN ✓` row on the screen, or, on a screen narrower than 120 columns where that row can be pushed off by
/// the cards under it, `n` tasks ended in the book and the screen held still after (the sweep reaches every width again).
fn turn_over(world: &World, cols: u16, screen: &str, row: &str, n: usize) -> bool {
    let ended = || world.front.tasks().book().rows().iter().filter(|r| r.state.is_terminal()).count() >= n;
    screen.contains("⌥1–9 tabs") && (screen.contains(row) || (cols < 120 && ended()))
}

/// The chat in its states, then the overlays, then every tab and sub-tab, as one operator at one size.
fn sit(world: &World, size: (u16, u16), dir: &Path, want: Option<&Path>) -> Vec<String> {
    let mut o = Out { dir: dir.to_path_buf(), index: Vec::new(), want: want.map(Path::to_path_buf), size };
    let mut s = Sitting::new(world, size);
    o.put(&mut s, "chat-fresh");
    // a draft, long enough to wrap at every width
    let long = "a long line of the operator's own words that goes past the end of the composer at sixty columns and on, so that it wraps";
    s.keys(typed(long, false));
    o.put(&mut s, "chat-draft");
    s.keys(vec![ctrl('u')]);
    // the sheet and the jump, on an empty chat
    s.keys(typed("?", false));
    o.put(&mut s, "overlay-sheet-chat");
    s.keys(vec![press(KeyCode::Esc)]);
    s.keys(vec![ctrl('k')]);
    o.put(&mut s, "overlay-jump");
    s.keys(typed("pack", false));
    o.put(&mut s, "overlay-jump-query");
    s.keys(vec![press(KeyCode::Esc)]);
    // a turn with an answer that is markdown, codes, a limit notice and a spend
    s.line("say **Release work** (`docs/TESTING.md` §5): open loose ends X-101, X-1020–X-1022, X-101/X-102 and B12.; tool Read src/lib.rs; sh cargo test => 0; spend 0.12; limits 0.5 0.92");
    let cols = size.0;
    s.until("the turn done", |t| turn_over(world, cols, t, "→ t1 ✓", 1));
    o.put(&mut s, "chat-turn");
    s.line("say - one\n- two");
    s.until("the second turn", |t| turn_over(world, cols, t, "→ t2 ✓", 2));
    s.line("who");
    s.until("the third turn", |t| turn_over(world, cols, t, "→ t3 ✓", 3));
    o.put(&mut s, "chat-turns");
    // scroll: up by lines, then half a screen
    s.keys(vec![j4n_tui::scene::key("shift+up").unwrap(), j4n_tui::scene::key("shift+up").unwrap()]);
    o.put(&mut s, "chat-shift-up");
    s.keys(vec![press(KeyCode::PageUp)]);
    s.keys(vec![press(KeyCode::PageUp)]);
    o.put(&mut s, "chat-pageup");
    s.keys(vec![press(KeyCode::PageDown), press(KeyCode::PageDown), press(KeyCode::PageDown)]);
    // a question: an approval and an open one
    s.line("write notes.txt hello");
    s.until("the approval", |t| asked(t) && t.contains("write notes.txt?") && t.contains("esc later"));
    o.put(&mut s, "overlay-ask-approval");
    s.keys(typed("?", false));
    o.put(&mut s, "overlay-ask-why");
    s.keys(typed("2", false));
    s.until("idle again", |t| !asked(t) && turn_over(world, cols, t, "→ t4 ✗", 4));
    s.line("ask Which of the two colours do you want the border to be, and should it change with the palette?");
    s.until("the question", |t| asked(t) && t.contains("type the answer and send it"));
    o.put(&mut s, "overlay-ask-open");
    s.keys(vec![press(KeyCode::Esc)]);
    s.until("the question put away", |t| !asked(t) && t.contains("waits for you"));
    // the tabs: F1 is Chat; F2.. the others; the F key of an open tab and alt-right step its sub-tabs (Connect has thirteen)
    for n in 2..=9u8 {
        let first = s.keys(vec![press(KeyCode::F(n))]);
        let mut seen = vec![first];
        o.put(&mut s, &format!("page-t{n}-s1"));
        for k in 2..=13 {
            let now = s.keys(vec![RIGHT()]);
            if seen.contains(&now) {
                break;
            }
            seen.push(now);
            o.put(&mut s, &format!("page-t{n}-s{k}"));
        }
        // the sheet of the page
        s.keys(typed("?", false));
        o.put(&mut s, &format!("sheet-t{n}"));
        s.keys(vec![press(KeyCode::Esc)]);
    }
    // an event of the Log opened into its tree
    s.keys(vec![press(KeyCode::F(6))]);
    s.keys(vec![press(KeyCode::Enter)]);
    o.put(&mut s, "log-open");
    s.keys(vec![press(KeyCode::Esc)]);
    // a resize chain on the chat: smaller, then larger than it began, then back
    s.keys(vec![press(KeyCode::F(1))]);
    for (c, r) in [(60, 24), (160, 44), (size.0, size.1)] {
        s.resize(c, r);
        o.put(&mut s, &format!("resize-{c}"));
    }
    o.index
}

#[test]
fn the_sweep() {
    let Some(out) = std::env::var_os("J4N_SWEEP").filter(|v| !v.is_empty()).map(PathBuf::from) else { return };
    let only = std::env::var("J4N_SWEEP_ONLY").unwrap_or_default();
    for palette in PALETTES {
        for tier in TIERS {
            let look = format!("{palette}-{tier}");
            if !only.is_empty() && !only.split(',').any(|o| o == look) {
                continue;
            }
            let mut index = Vec::new();
            for size in SIZES {
                let d = home(palette, tier, &format!("sweep-{look}-{}", size.0));
                let world = open(&d);
                let dir = out.join(&look).join(size.0.to_string());
                let got = sit(&world, size, &dir, None);
                index.extend(got.into_iter().map(|l| format!("{look} {}x{}  {l}", size.0, size.1)));
                world.close();
            }
            fs::create_dir_all(out.join(&look)).unwrap();
            fs::write(out.join(&look).join("index.txt"), index.join("\n") + "\n").unwrap();
        }
    }
}

/// What differs from one run to the next and is left out of the committed set: the Log, the sixth tab (its rows hold the clock and the
/// sequence numbers of events that come when they come), Usage, the third (the ledger's reports stacked under it are named by sequence
/// number), the opened event and the Models chart (its dot is how long a task took; Agents › Models, the seventh sub-tab of the fourth tab).
const UNSTEADY: [&str; 5] = ["page-t6", "sheet-t6", "log-open", "page-t3-s1", "page-t4-s7"];

/// A screen without its clocks: `17:16` and `17:16:28` are `HH:MM`; the seconds a task took (`3s`, in the column at the end of its
/// line) and the seconds a question has waited (`󰔛 0:07`) are `0s` and `0:00`, with the blanks that kept them at the right edge. Both
/// count real time, and a loaded machine reads the screen a second later than a quiet one; the goldens keep the quiet reading.
pub(crate) fn steady(screen: &str) -> String {
    let clocks = chart_seconds(&clock_digits(screen));
    let screen = clocks.as_str();
    let mut out = String::new();
    let chars: Vec<char> = screen.chars().collect();
    let digit = |i: usize| chars.get(i).is_some_and(char::is_ascii_digit);
    let mut i = 0;
    while i < chars.len() {
        if digit(i) && digit(i + 1) && chars.get(i + 2) == Some(&':') && digit(i + 3) && digit(i + 4) && !(i > 0 && digit(i - 1)) {
            out.push_str("HH:MM");
            i += 5;
            if chars.get(i) == Some(&':') && digit(i + 1) && digit(i + 2) {
                i += 3;
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// The rows that differ between a golden and a screen: `want` and `got` of each, so a failure names what moved.
fn row_diff(want: &str, got: &str) -> String {
    let (w, g): (Vec<&str>, Vec<&str>) = (want.lines().collect(), got.lines().collect());
    let mut out = Vec::new();
    for i in 0..w.len().max(g.len()) {
        let (a, b) = (w.get(i).copied().unwrap_or("<no row>"), g.get(i).copied().unwrap_or("<no row>"));
        if a != b {
            out.push(format!("  row {}:\n    want |{}\n    got  |{}", i + 1, a.trim_end(), b.trim_end()));
        }
    }
    out.truncate(24);
    out.join("\n")
}

/// `s` with the Models chart's legend values (`• 0.6s  slowest`, `• 0s  median`: how long the sweep's own tasks took) as `• 0s`.
/// The chart's page is left out (`page-t3-s1`), but the keys sheet over the third tab draws over it (Models is its
/// first sub-tab), and a task of the sweep takes 0.6 s or 0.7 s as the machine goes.
fn chart_seconds(s: &str) -> String {
    let lines: Vec<String> = s
        .lines()
        .map(|line| {
            // the word may be cut off by the edge of a sheet drawn over the chart (`• 0.5s│`)
            for word in ["  median", "  slowest", "│"] {
                let Some(end) = line.find(word) else { continue };
                let Some(start) = line[..end].rfind("• ") else { continue };
                let value = &line[start + "• ".len()..end];
                if !value.is_empty() && value.ends_with('s') && value[..value.len() - 1].chars().all(|c| c.is_ascii_digit() || c == '.') {
                    return format!("{}• 0s{}", &line[..start], &line[end..]);
                }
            }
            line.to_string()
        })
        .collect();
    lines.join("\n") + if s.ends_with('\n') { "\n" } else { "" }
}

/// `s` with the seconds counted in it as a quiet machine counts them (see `steady`).
fn clock_digits(s: &str) -> String {
    const TIMER: char = '\u{F051B}';
    let lines: Vec<String> = s
        .lines()
        .map(|line| {
            let c: Vec<char> = line.chars().collect();
            let mut out = String::new();
            let mut i = 0;
            while i < c.len() {
                // `󰔛 0:07`, `󰔛 1:02:03`: a question's wait
                if c[i] == TIMER && c.get(i + 1) == Some(&' ') && c.get(i + 2).is_some_and(char::is_ascii_digit) {
                    let n = c[i + 2..].iter().take_while(|ch| ch.is_ascii_digit() || **ch == ':').count();
                    out.push_str(&" ".repeat(n.saturating_sub("0:00".len())));
                    out.push(TIMER);
                    out.push_str(" 0:00");
                    i += 2 + n;
                    continue;
                }
                // `  3s  `: the seconds of a task, between blanks at the end of its line
                if c[i].is_ascii_digit() && i >= 2 && c[i - 1] == ' ' && c[i - 2] == ' ' {
                    let n = c[i..].iter().take_while(|ch| ch.is_ascii_digit()).count();
                    let after = c.get(i + n + 1..).unwrap_or(&[]);
                    if n <= 3 && c.get(i + n) == Some(&'s') && (after.is_empty() || after.starts_with(&[' ', ' '])) {
                        out.push_str(&" ".repeat(n - 1));
                        out.push_str("0s");
                        i += n + 1;
                        continue;
                    }
                }
                out.push(c[i]);
                i += 1;
            }
            out
        })
        .collect();
    lines.join("\n") + if s.ends_with('\n') { "\n" } else { "" }
}

/// The longest meter on a screen, in cells: a thin line (a run of `━` and `─` with its percent after it) or the inside of a bar
/// between `▕` and `▏`. A rule, a frame, the line under the tabs or a heading is not a meter.
fn longest_meter(screen: &str) -> usize {
    let mut longest = 0;
    for line in screen.lines() {
        let chars: Vec<char> = line.chars().collect();
        let mut at = 0;
        while at < chars.len() {
            if !matches!(chars[at], '━' | '╸' | '─') {
                at += 1;
                continue;
            }
            let end = (at..chars.len()).find(|i| !matches!(chars[*i], '━' | '╸' | '─')).unwrap_or(chars.len());
            let run = &chars[at..end];
            let after: String = chars[end..].iter().skip_while(|c| **c == ' ').take_while(|c| c.is_ascii_digit() || **c == '%').collect();
            if run.len() >= 2 && after.ends_with('%') && after.len() > 1 {
                longest = longest.max(run.len());
            }
            at = end;
        }
        for at in (0..chars.len()).filter(|i| chars[*i] == '▕') {
            if let Some(end) = chars[at..].iter().position(|c| *c == '▏') {
                longest = longest.max(end.saturating_sub(1));
            }
        }
    }
    longest
}

/// The committed part: one golden per page and overlay at 80, 120 and 160 columns, in jjj/icons (the other sizes have goldens of their own scenes).
fn pages_at((cols, rows): (u16, u16), tag: &str) {
    let d = home("jjj", "icons", &format!("sweep-{tag}"));
    let world = open(&d);
    let dir = std::env::temp_dir().join(format!("j4n-sweep-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let want = root().join("crates/j4n/tests/sweep");
    let suffix = format!(".{cols}x{rows}.txt");
    let bless = std::env::var_os("J4N_BLESS").is_some_and(|v| !v.is_empty());
    sit(&world, (cols, rows), &dir, (!bless).then_some(want.as_path()));
    world.close();
    let (mut found, mut made) = (Vec::new(), Vec::new());
    fs::create_dir_all(&want).unwrap();
    if bless {
        for stale in fs::read_dir(&want).unwrap().flatten().map(|e| e.path()).filter(|p| p.to_string_lossy().ends_with(&suffix)) {
            fs::remove_file(stale).unwrap();
        }
    }
    let mut names: Vec<String> = fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".txt")).collect();
    names.sort();
    // the pages and the chat at 80 columns hold no meter longer than eight cells: the long bar is for the page about that number
    if cols <= 80 {
        for name in &names {
            let screen = read_text(&dir.join(name)).unwrap();
            // the Usage tab is the page about those numbers: its usage view keeps the long bar the chat's `/usage` has
            if dir.join(name.replace(".txt", ".usage")).exists() {
                continue;
            }
            let long = longest_meter(&screen);
            if long > 8 {
                found.push(format!("{name}: a meter {long} cells long at {cols} columns; outside Usage one is at most 8"));
            }
        }
    }
    for name in names.iter().filter(|n| !UNSTEADY.iter().any(|u| n.starts_with(u))) {
        let got = steady(&read_text(&dir.join(name)).unwrap());
        let path = want.join(format!("{}{suffix}", name.trim_end_matches(".txt")));
        made.push(path.clone());
        if bless {
            fs::write(&path, got).unwrap();
        } else if read_text(&path).as_deref() != Some(got.as_str()) {
            let want = read_text(&path).unwrap_or_default();
            found.push(format!("{}: not its golden; J4N_BLESS=1 writes it\n{}", path.display(), row_diff(&want, &got)));
        }
    }
    // a golden that no page makes any more is stale: it is deleted, never left to look checked
    for golden in fs::read_dir(&want).unwrap().flatten().map(|e| e.path()).filter(|p| p.to_string_lossy().ends_with(&suffix) && !made.contains(p)) {
        found.push(format!("{}: a golden no page makes; delete it", golden.display()));
    }
    let _ = fs::remove_dir_all(&dir);
    crate::support::assert_clean(&format!("the pages at {cols} columns"), &found);
}

#[test]
fn the_pages_at_80_columns_are_their_goldens_and_no_meter_is_longer_than_eight_cells() {
    pages_at((80, 24), "80");
}

#[test]
fn the_pages_at_120_columns_are_their_goldens() {
    pages_at((120, 36), "120");
}

#[test]
fn the_pages_at_160_columns_are_their_goldens() {
    pages_at((160, 44), "160");
}
