//! The no-shell walk (charter law 11). The real binary on a real pty (the tty harness of
//! `tty_cases`), in a HOME and a project that are empty, driven with keys only through the `walk` rows: chats on (`:chats` while off
//! leads to the switch), a Features switch, two of All and the undo of one, the theme, a plan's price, an account added from
//! Connect › Add account and signed in, a login already on this machine named with `u`, two packs on from Grow › Packs and the restart their rows ask
//! for, an approval card answered over another tab, a grant taken back in Settings › Gates, and a proposal J4n drafted accepted in Grow.
//!
//! What it holds to: (1) the test writes no file into HOME, the project or the J4n folder: only J4n does (the stand-in `claude` the test puts
//! on the PATH lives outside HOME, and its login is the program's own answer, not a file); (2) Grow holds no row for any operator edit, only
//! the one J4n drafted; (3) nothing waited for a restart unless its row said `⟳`, and the restart card's key restarted it; (4) every refusal
//! met on the way carried the key that leads on. What the walk does not drive is named in the `walk` rows' note.
//!
//! And law 11's own check: every `[[command]]` of every pack resolves to a row of the palette.
//!
//! The setup walk is the second walk: from an empty HOME to a proven chat with Connect › Setup's keys, every frame looked at for a shell
//! command, a path or a file, and for the key it types (its own doc says how).

use crate::deciders_flow::{Cases, gateway, learn};
use crate::key_canary::{Canary, VALUES, in_files, in_requests, in_world, keys, needles, to_chat, until};
use crate::run_cases::Dirs;
use crate::screen_drive::{Operator, flat, wait_idle};
use crate::setup_flow::{run_test, setup_page, to_row};
use crate::tty_cases::{Tty, no_pty};
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use j4n_core::task::Mode;
use j4n_tui::headless::{KeyCode, press, typed};
use serde_json::Value as Json;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

/// F1–F9: the n-th tab of the strip, as it stands (a pack that is on adds its tab, and the ones after it move up one).
const F: [&[u8]; 9] = [b"\x1bOP", b"\x1bOQ", b"\x1bOR", b"\x1bOS", b"\x1b[15~", b"\x1b[17~", b"\x1b[18~", b"\x1b[19~", b"\x1b[20~"];
/// The tabs there can be, in their order.
const TABS: [&str; 9] = ["Chat", "Tasks", "Library", "Connect", "Spend", "Quality", "Grow", "Log", "Settings"];
const RIGHT: &[u8] = b"\x1b[1;3C";
const DOWN: &[u8] = b"\x1b[B";
const ESC: &[u8] = b"\x1b";
const ENTER: &[u8] = b"\r";
/// What the restart card asks (`j4n-tui/src/restart.rs`).
const CARD: &str = "restart now?";

/// A `claude` that stands in for the real one, outside HOME: its own login (no `CLAUDE_CONFIG_DIR`) is signed in as the operator, the way a
/// `~/.claude` already set up is, with no file for it; an account's login (`CLAUDE_CONFIG_DIR` set, the account's folder) is signed in once
/// `auth login` has run there. It runs no turn: the walk's doer is `test`.
fn stand_in_claude(bin: &Path) -> PathBuf {
    let path = bin.join("claude");
    let body = r#"#!/bin/sh
case "$1 $2" in
  "--version "*) echo "2.1.310 (Claude Code)"; exit 0 ;;
  "auth status")
    if [ -z "$CLAUDE_CONFIG_DIR" ]; then echo '{"loggedIn":true,"authMethod":"claude.ai","email":"me@example.org","subscriptionType":"max"}'
    elif [ -f "$CLAUDE_CONFIG_DIR/.walk-login" ]; then echo '{"loggedIn":true,"authMethod":"claude.ai","email":"walk@example.org","subscriptionType":"pro"}'
    else echo '{"loggedIn":false}'; fi
    exit 0 ;;
  "auth login") [ -n "$CLAUDE_CONFIG_DIR" ] && : > "$CLAUDE_CONFIG_DIR/.walk-login" && exit 0; exit 1 ;;
esac
exit 1
"#;
    fs::write(&path, body).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// Every file under `dir`, by its path.
fn files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = fs::read_dir(dir) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            out.extend(files(&p));
        } else {
            out.push(p);
        }
    }
    out
}

/// The events of the project's Log, read as the text they are (opening the Log would lock it, and a lock is a file).
fn events(project: &Path) -> Vec<Json> {
    let mut segs: Vec<PathBuf> = fs::read_dir(project.join(".j4n/log")).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "jsonl")).collect()).unwrap_or_default();
    segs.sort();
    segs.iter().flat_map(|p| fs::read_to_string(p).unwrap_or_default().lines().filter_map(|l| serde_json::from_str::<Json>(l).ok()).collect::<Vec<_>>()).collect()
}

fn kind<'a>(evs: &'a [Json], k: &str) -> Vec<&'a Json> {
    evs.iter().filter(|e| e["kind"] == k).collect()
}

/// Whether the Log holds the operator's edit of `what` within `wait`.
fn applied_within(project: &Path, what: &str, wait: Duration) -> bool {
    let end = Instant::now() + wait;
    loop {
        if kind(&events(project), "edit.applied").iter().any(|e| e["data"]["what"][0] == what) {
            return true;
        }
        if Instant::now() >= end {
            return false;
        }
        thread::sleep(Duration::from_millis(50));
    }
}

/// Wait until the Log holds the operator's edit of `what` (`setting theme.palette`): the key was taken, not only the screen drawn again.
fn edited(project: &Path, what: &str, w: &mut Walk) {
    let end = Instant::now() + crate::support::PATIENCE;
    while !kind(&events(project), "edit.applied").iter().any(|e| e["data"]["what"][0] == what) {
        if Instant::now() >= end {
            let evs = events(project);
            let tail: Vec<String> = evs.iter().rev().take(12).rev().map(|e| format!("{} {}", e["kind"], e["data"])).collect();
            panic!("no edit of {what}:\n{}\nthe Log ends:\n{}", w.t.screen(), tail.join("\n"));
        }
        thread::sleep(Duration::from_millis(50));
    }
}

/// The walk: the screen, and what it met on the way.
struct Walk {
    t: Tty,
    /// The restart cards that came up, and the rows that said `⟳` before them.
    cards: usize,
}

impl Walk {
    /// Wait for the screen to satisfy `ready`.
    fn see(&mut self, what: &str, ready: impl Fn(&str) -> bool) -> String {
        self.t.until_screen(what, ready)
    }

    /// Keys, then a moment for the screen to take them.
    fn keys(&mut self, bytes: &[u8]) {
        self.t.send(bytes);
        thread::sleep(Duration::from_millis(120));
    }

    /// Go to the tab called `name` with its F key, by its place in the strip as it is now.
    fn tab(&mut self, name: &str) {
        let s = self.t.screen();
        let strip = s.lines().find(|l| l.contains("Chat") && l.contains("Settings")).unwrap_or_else(|| panic!("no tab strip:\n{s}")).to_string();
        let names: Vec<&str> = strip.split_whitespace().filter(|w| TABS.contains(w)).collect();
        let at = names.iter().position(|n| *n == name).unwrap_or_else(|| panic!("no tab {name} in {names:?}"));
        self.keys(F[at]);
        self.see(&format!("the tab {name}"), |s| s.lines().nth(at_strip(s) + 1).is_some_and(|rule| under_at(s, at_strip(s), name, rule)));
    }

    /// Step to the sub-tab called `name` with `⌥→`.
    fn sub(&mut self, name: &str) {
        for _ in 0..12 {
            if sub_is(&self.t.screen(), name) {
                return;
            }
            self.keys(RIGHT);
        }
        let s = self.t.screen();
        panic!("never reached the sub-tab {name}:\n{s}");
    }

    /// Find the row `text` in the list that is open (`/`): the rows that match are the ones left, and the selection is on one.
    fn find(&mut self, text: &str) {
        self.keys(b"/");
        self.keys(text.as_bytes());
        self.keys(ENTER);
        self.see(&format!("the row {text} found"), |s| s.contains("found") && s.contains(text));
    }

    /// The line of the screen that holds `text`, as it is now.
    fn line(&mut self, text: &str) -> String {
        self.t.screen().lines().find(|l| l.contains(text)).unwrap_or_default().to_string()
    }

    /// No restart card is up and none came: the edit just made was live.
    fn no_card(&mut self, after: &str) {
        thread::sleep(Duration::from_millis(300));
        let s = self.t.screen();
        assert!(!s.contains(CARD), "{after} asked for a restart, and its row did not say ⟳:\n{s}");
    }
}

/// The row of the strip of the top tabs.
fn at_strip(s: &str) -> usize {
    s.lines().position(|l| l.contains("Chat") && l.contains("Settings")).unwrap_or(0)
}

/// Whether `word` on row `row` has the thick rule under it on the row after (`rule`): the tab or sub-tab that is open.
fn under_at(s: &str, row: usize, word: &str, rule: &str) -> bool {
    let Some(line) = s.lines().nth(row) else { return false };
    // the word alone, not inside another (Grow the tab, Grow the sub-tab)
    let Some(b) = line.match_indices(word).map(|(b, _)| b).find(|b| line[b + word.len()..].starts_with(' ') || b + word.len() == line.len()) else { return false };
    let col = line[..b].chars().count();
    rule.chars().nth(col) == Some('━')
}

/// Whether the sub-tab that is open is `name`: the strip of the sub-tabs is the second row under the top tabs'.
fn sub_is(s: &str, name: &str) -> bool {
    let row = at_strip(s) + 2;
    s.lines().nth(row + 1).is_some_and(|rule| under_at(s, row, name, rule))
}

#[test]
fn the_operators_walk_is_keys_only_in_a_fresh_home_and_nothing_of_it_is_a_file_by_hand() {
    assert!(!no_pty(), "util-linux `script` gives the binary a pty; it is not on this machine");
    let d = Dirs::bare("no-shell-walk-keys");
    let claude = stand_in_claude(&d.dir.join("bin"));
    // (1) before J4n starts, HOME and the project hold nothing at all: every file found in them later is J4n's
    assert!(files(&d.home()).is_empty() && files(&d.project()).is_empty(), "a fresh HOME and project");
    assert!(!claude.starts_with(d.home()) && !claude.starts_with(d.project()));
    let path = format!("{}:{}", d.dir.join("bin").display(), std::env::var("PATH").unwrap_or_default());
    let t = Tty::sized(&d, "--doer test", &[("PATH", path)], (140, 44));
    let mut w = Walk { t, cards: 0 };
    w.see("the first frame", |s| s.contains("⌥1–9 tabs"));
    // a fresh J4n home: the Setup card asks (the Decider is off in J4n basic), and its first option opens Connect › Setup and answers it for
    // the home, so the start after the restart below asks nothing
    w.see("the Setup card", |s| s.contains("Set up J4n?"));
    w.keys(b"1");
    w.see("Connect › Setup from the card", |s| sub_is(s, "Setup") && !s.contains("Set up J4n?"));
    w.tab("Chat");
    // the first start in a new folder says what is missing in one row, and its ⏎ opens Connect › Setup too
    w.see("the first-start row", |s| flat(s).contains("⏎ opens Connect › Setup"));
    w.keys(ENTER);
    w.see("Connect from the first-start row", |s| sub_is(s, "Setup"));
    w.tab("Chat");

    // `:chats` while they are off is refused with the key that turns them on; ⏎ on the empty composer does it, and the note
    // that answers says it waits for the next start, with ⏎ to restart (`⟳`). It is not pressed: the pack below asks for the same.
    w.keys(b":chats\r");
    w.see("the refusal that leads", |s| flat(s).contains("the chats are off") && flat(s).contains("⏎ turns them on"));
    w.keys(ENTER);
    w.see("chats on, from the next start", |s| flat(s).contains("chats.on is true") && flat(s).contains("⏎ restarts now"));

    // Settings › Features, the switches of J4n basic: a word of three (session.mode), the row says the new word, and no restart is asked
    w.tab("Settings");
    w.sub("Features");
    w.see("the Features rows", |s| s.contains("space flip"));
    w.find("session.mode");
    let before = w.line("session.mode");
    w.keys(b" ");
    w.see("session.mode changed", |s| s.lines().any(|l| l.contains("session.mode") && l != before));
    w.no_card("session.mode");
    assert!(!w.line("session.mode").contains("next start"), "session.mode is live: {}", w.line("session.mode"));
    w.keys(ESC);

    // Settings › Look, the palette's next word with `n`: live (`Down::Theme`), a Change and no Grow row
    w.sub("Look");
    w.find("theme.palette");
    thread::sleep(Duration::from_millis(300));
    let word = |l: &str| l.split_whitespace().nth(2).unwrap_or_default().to_string();
    let before = word(&w.line("theme.palette"));
    w.keys(b"n");
    // a key may be lost under load (no Change, no line said, while the screen shows the row found); it is sent once more, only when
    // nothing was applied and nothing was said for it.
    if !applied_within(&d.project(), "setting theme.palette", Duration::from_secs(8)) {
        let said = kind(&events(&d.project()), "chat.say").iter().any(|e| e["data"]["text"].as_str().is_some_and(|t| t.starts_with(":look")));
        assert!(!said, "`n` was said and applied nothing:\n{}", w.t.screen());
        eprintln!("`n` on theme.palette reached nothing in 8 s; sent once more");
        w.keys(b"n");
    }
    edited(&d.project(), "setting theme.palette", &mut w);
    w.see("the palette's next word", |s| s.lines().any(|l| l.contains("theme.palette") && word(l) != before));
    w.no_card("theme.palette");
    w.keys(ESC);

    // Settings › All (the switches J4n basic does not show): the keys row (a bool, live), `space` hides it at once and `u` takes
    // it back; lanes, a word, whose row says the new word and asks for no restart
    w.sub("All");
    w.see("the All rows", |s| s.contains("space flip"));
    w.find("look.keys_row");
    w.keys(b" ");
    w.see("the keys row gone at once", |s| !s.contains("⌥1–9 tabs"));
    w.no_card("look.keys_row");
    w.keys(b"u");
    w.see("the keys row back with `u`", |s| s.contains("⌥1–9 tabs"));
    w.keys(ESC);
    w.find("acts on its own");
    w.keys(b" ");
    w.see("lanes auto", |s| s.lines().any(|l| l.contains("lanes") && l.contains("auto")));
    w.no_card("lanes");
    w.keys(ESC);

    // Library › Plans, a price in place
    w.tab("Library");
    w.sub("Plans");
    w.see("the plan rows", |s| s.contains("⏎ set price"));
    w.keys(ENTER);
    w.keys(b"24\r");
    w.see("the price on its row", |s| s.contains("$24"));
    w.no_card("a plan's price");

    // Grow › Packs, `space` on qc: it acts from the next start, so the card asks, and its ⏎ restarts J4n
    w.tab("Grow");
    w.sub("Packs");
    w.see("the packs", |s| s.contains("a pack acts from the next start"));
    // the pack accounts (Connect › Add account and what follows): it adds connectors, so its card names what it widens, `y`; the restart card is put off with esc,
    // and the restart comes with qc below
    w.find("accounts");
    w.keys(b" ");
    w.see("the card that names what accounts widens", |s| flat(s).contains("widens what J4n may do") && s.contains("y yes · n no"));
    w.keys(b"y");
    w.see("the row says ⟳ and the restart card is up", |s| s.contains(CARD) && s.lines().any(|l| l.contains("accounts") && l.contains("on ⟳")));
    w.keys(ESC);
    // a second esc clears the find that is still on the list
    w.keys(ESC);
    w.find("qc");
    w.keys(b" ");
    // qc widens what J4n may do: the card names it, and `y` is the whole of it, with no Grow row
    w.see("the card that names what qc widens", |s| flat(s).contains("widens what J4n may do") && s.contains("y yes · n no"));
    w.keys(b"y");
    let s = w.see("the row says ⟳ and the restart card is up", |s| s.contains(CARD) && s.lines().any(|l| l.contains("qc") && l.contains("on ⟳")));
    assert!(s.contains("qc · ⟳ next start"), "the card names the row that waits: {s}");
    w.cards += 1;
    w.keys(ENTER);
    // the second J4n: the chats are on (the list is pinned) and the pack qc is its tab, Quality
    w.see("J4n started again, with the chats and qc", |s| s.contains("Quality") && (s.contains("no chats yet") || s.contains("chats")));

    // the pack accounts is on: Connect › Add account and what follows
    w.tab("Connect");
    // Connect › Add account, the form filled in with the next free name; ⏎ on its last row adds it, the sign-in card asks, `y`
    w.sub("Add account");
    w.see("the form", |s| s.contains("add account") && s.contains("claude-1"));
    for _ in 0..4 {
        w.keys(DOWN);
    }
    w.keys(ENTER);
    w.see("the sign-in card", |s| s.contains("sign in account `claude-1`") && s.contains("y yes · n no"));
    w.keys(b"y");
    // the login of the stand-in's own (as `~/.claude` already set up) is found, and `u` names it as an account
    w.see("the login found on this machine", |s| s.contains("claude · yours") && s.contains("found on this machine"));
    for _ in 0..12 {
        w.keys(DOWN);
    }
    w.keys(b"u");
    w.see("the login named", |s| s.contains("named claude-2"));
    // Connect › Accounts lists both: claude-1 signed in through its own folder, claude-2 your own login
    w.sub("Accounts");
    w.see("claude-1 signed in", |s| s.lines().any(|l| l.contains("claude-1") && l.contains("signed in")));
    w.see("claude-2 named", |s| s.lines().any(|l| l.contains("claude-2")));
    // `c` checks claude-1; `x` removes it, through the card
    w.find("claude-1");
    let checks = |p: &Path| kind(&events(p), "account.checked").iter().filter(|e| e["data"]["account"] == "claude-1").count();
    let before = checks(&d.project());
    w.keys(b"c");
    let end = Instant::now() + crate::support::PATIENCE;
    while checks(&d.project()) == before {
        assert!(Instant::now() < end, "`c` checked nothing:\n{}", w.t.screen());
        thread::sleep(Duration::from_millis(50));
    }
    w.keys(b"x");
    w.see("the card that asks to remove claude-1", |s| s.contains("claude-1") && s.contains("y yes · n no"));
    w.keys(b"y");
    w.keys(ESC);
    w.see("claude-1 removed", |s| !s.lines().any(|l| l.contains("claude-1") && l.contains("signed in")) && s.lines().any(|l| l.contains("claude-2")));


    // an approval card over the Settings tab. The doer waits, then asks to run `make deploy`; `3` allows it for the session
    w.tab("Chat");
    w.keys(b"wait 1500; run make deploy\r");
    w.tab("Settings");
    w.see("the approval card over Settings", |s| s.contains("make deploy") && s.contains("y yes"));
    let card = w.t.screen();
    let digit = card.lines().find(|l| l.contains("this session")).and_then(|l| l.split_whitespace().find(|t| t.len() == 1 && t.chars().all(|c| c.is_ascii_digit()))).map(str::to_string);
    let digit = digit.unwrap_or_else(|| panic!("the card offers no session grant:\n{card}"));
    // a grant widens what J4n may do: the card says so on the option, and its key is pressed a second time
    w.keys(digit.as_bytes());
    w.see("the second press asked", |s| flat(s).contains(&format!("{digit} again")) || !s.contains("y yes"));
    if w.t.screen().contains("y yes") {
        w.keys(digit.as_bytes());
    }
    w.see("the card gone", |s| !s.contains("y yes"));
    // Settings › Gates lists the grant; `x` takes it back
    w.sub("Gates");
    w.see("the grant on Gates", |s| s.contains("make deploy") && s.lines().any(|l| l.contains("shell") && l.contains("this session")));
    w.find("this session");
    w.keys(b"x");
    if w.t.screen().contains("y yes") {
        w.keys(b"y");
    }
    w.see("the grant taken back", |s| !s.contains("make deploy"));
    w.keys(ESC);

    // the same yes three times is a proposal J4n drafts; Grow › Grow, ⏎ accepts it through the card
    w.tab("Chat");
    w.keys(b"run make build; run make build; run make build\r");
    // the line is none of c1's and no Decider is on in a fresh home: the question of which chat takes it is asked, and `2` is a new chat
    w.see("the question of which chat takes the line", |s| s.contains("which chat takes this?"));
    w.keys(b"2");
    // in the Chat the approval is drawn above the composer, and `1` is its yes
    let answered = |p: &Path| kind(&events(p), "ask.answered").len();
    for n in 0..3 {
        let before = answered(&d.project());
        w.see(&format!("approval {}", n + 1), |s| s.contains("run make build?") && s.contains("1–"));
        w.keys(b"1");
        let end = Instant::now() + crate::support::PATIENCE;
        while answered(&d.project()) == before {
            assert!(Instant::now() < end, "approval {} was never answered:\n{}", n + 1, w.t.screen());
            thread::sleep(Duration::from_millis(50));
        }
    }
    w.tab("Grow");
    w.sub("Grow");
    w.see("the proposal J4n drafted", |s| s.contains("Allow \"make build\" here without asking"));
    w.find("make build");
    w.keys(ENTER);
    w.see("the accept card", |s| s.contains("accept p") && s.contains("y yes · n no"));
    w.keys(b"y");
    w.see("accepted", |s| s.lines().any(|l| l.split_whitespace().take(3).any(|t| t == "accepted")));
    w.keys(ESC);

    // (2) Grow holds the one row J4n drafted and none for the operator's edits: the screen says one, and so does the Log below
    let grow = w.t.screen();
    w.t.send(&[0x03]);
    let end = Instant::now() + crate::support::PATIENCE;
    while w.t.child.try_wait().ok().flatten().is_none() {
        assert!(Instant::now() < end, "J4n did not end on ⌃c");
        thread::sleep(Duration::from_millis(50));
    }
    let evs = events(&d.project());
    let made = kind(&evs, "proposal.made");
    assert_eq!(made.len(), 1, "one proposal, J4n's, and none for an operator edit: {made:?}\n{grow}");
    assert_ne!(made[0]["by"], "operator", "{}", made[0]);
    let edits = kind(&evs, "edit.applied");
    let what: Vec<&str> = edits.iter().filter(|e| e["by"] == "operator").flat_map(|e| e["data"]["what"].as_array().into_iter().flatten().filter_map(Json::as_str)).collect();
    for want in ["setting chats.on", "setting look.keys_row", "setting session.mode", "setting lanes", "setting theme.palette", "plan claude", "account claude-1", "account claude-2", "enable accounts", "enable qc"] {
        assert!(what.contains(&want), "{want} is an operator edit, a Change with an undo: {what:?}");
    }
    // the pack that widens was said yes to on the card, and the edit keeps what it widened
    assert!(edits.iter().any(|e| e["data"]["what"][0] == "enable qc" && e["data"]["widens"].is_array()), "{edits:?}");
    assert_eq!(kind(&evs, "proposal.accepted").len(), 1, "the proposal J4n drafted was accepted");
    // (3) the one restart came from the card's key, after a row said ⟳; the second J4n started
    assert_eq!(w.cards, 1);
    // (4) every refusal met carried its key: a note of warning said while the walk went on has an act; the notices of the start are no refusal
    let refusals: Vec<&Json> = kind(&evs, "chat.note").into_iter().filter(|e| e["data"]["tone"] == "warn" && !e["data"]["text"].as_str().unwrap_or_default().starts_with("j4n: ")).collect();
    assert!(!refusals.is_empty(), "the walk met the refusal of :chats at least");
    for r in &refusals {
        assert!(r["data"].get("act").is_some(), "a refusal with no key that leads on: {}", r["data"]);
    }
}

// ---- the setup walk: from an empty HOME to a proven chat ----

/// The made-up key the walk types on Setup, the Decider's: of no vendor's shape, with `+ / =`, `"` and `\`, so its four forms are four strings.
const SETUP_KEY: Canary = Canary { name: "the key typed on Setup", value: r#"Jc4wSetupKy+Pn6/Dh\Xm"8r=Tz"# };

/// The world of the walk's project, as `j4n` starts it in a terminal: no pin (`doer` none, a line's doer is elected), or `--doer test`.
fn open_walk(d: &Dirs, doer: Option<&str>) -> World {
    let opts = Opts { doer: doer.map(String::from), model: None, approval: Some(Approval::Ask), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false };
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: d.dir.join("bin/no-claude").display().to_string() };
    crate::support::opened(|| wire::open(setup.clone(), &opts, true, "main"))
}

/// What the screen does at each start before the operator's first key (`j4n`'s `screen`): the Setup card while it is due, then, in a
/// folder J4n has not worked in, the one row that says what is missing.
fn started(w: &World, new_folder: bool) {
    w.front.setup_card_open();
    if let Some(n) = j4n::first_start_note(&w.front, new_folder, "new folder project · write here · a checkpoint before work") {
        w.front.first_note("main", &n);
    }
}

/// The words no frame may show (a shell command, a path, a file), lower case: Setup's list and the walk's, and the test's own folders, where every path J4n has is.
fn r1_words(d: &Dirs) -> Vec<String> {
    let mut words: Vec<String> = crate::setup_flow::OUT.iter().chain(BY_HAND.iter()).map(|w| w.to_string()).collect();
    words.push(d.dir.display().to_string().to_lowercase());
    words.sort();
    words.dedup();
    words
}

/// Connect › Setup on the screen: its steps.
fn on_setup(s: &str) -> bool {
    s.contains("Agent") && s.contains("Decider") && s.contains("Custom")
}

fn count(w: &World, kind: &str) -> usize {
    crate::deciders_flow::events(w).iter().filter(|(_, k, _)| k == kind).count()
}

/// The steps of Connect › Setup that hang from no other (Agent, Model, Decider, Test, Custom), each with whether it is done.
fn done_steps(w: &World) -> Vec<(String, bool)> {
    use j4n_core::value::Value;
    let word = |v: &Value, f: &str| match v.pointer(f) {
        Value::Str(s) => s,
        _ => String::new(),
    };
    w.front.setup_steps(None).iter().filter(|v| word(v, "/parent").is_empty()).map(|v| (word(v, "/id"), v.pointer("/done") == Value::Bool(true))).collect()
}

/// The setup walk, keys only, in a HOME and a project that are empty. The first start has no agent and no pin: the Setup card says
/// `Agent: none works` and its `1` opens Connect › Setup; the first-start row says what is missing; a chat line is refused with the key
/// that opens Setup, and that key opens it. The `test` doer is no agent the Agent step lists (it is not one of the doctor's), so the second
/// start is pinned to it, as `j4n --doer test` is: the step names it, in use. On Setup the Decider is turned on (`o`, the card's `y`), the
/// row's `r` starts J4n again, the made-up key is typed on `Jev · System One`'s row in its masked editor and the Decider is made
/// live by it; `⏎` on Test makes the proof call, which passes for the doer and the Decider, and every step is done. Then a chat line is
/// said and answered. The one file the test writes into the HOME is Jev's address on the loopback (`deciders_flow`'s gateway, the test
/// answerer), at the restart, before the key is typed, so the key goes nowhere else.
///
/// Every frame drawn, the waits' too, is looked at for a shell command, a path or a file, and for the key; and the key canary's sinks are
/// scanned for the key: the composer's history, the Log, the folds, the notices, every request the gateway was sent, and every file under
/// the HOME and the project but the key files, which hold it alone, 0600 in a folder of 0700.
#[test]
fn setup_from_an_empty_home_to_a_proven_chat_is_keys_only_and_no_frame_shows_a_shell_a_path_a_file_or_the_key() {
    let d = Dirs::bare("no-shell-walk-setup");
    assert!(files(&d.home()).is_empty() && files(&d.project()).is_empty(), "a fresh HOME and project");
    let values: Vec<&Canary> = VALUES.iter().copied().chain([&SETUP_KEY]).collect();
    let never: Vec<(String, String)> = values.iter().flat_map(|c| needles(c)).collect();
    let words = r1_words(&d);
    let mut shown: Vec<String> = Vec::new();
    let mut hits: Vec<String> = Vec::new();
    let mut drawn = 0;

    // (1) the first start: no agent works and none is pinned
    let w = open_walk(&d, None);
    let mut op = Operator::scanning(&w, never.clone(), words.clone());
    started(&w, true);
    // (2) the Setup card, answered `open Setup`
    let s = op.until("the Setup card", |s| s.contains("Set up J4n?"));
    assert!(flat(&s).contains("Agent: none works"), "the card says no agent works:\n{s}");
    keys(&mut op, vec![press(KeyCode::Char('1'))]);
    // (3) Connect › Setup, its Agent step not done
    op.until("Connect › Setup from the card", |s| on_setup(s) && s.contains("none works") && !s.contains("Set up J4n?"));
    // the first-start row and a line no agent can take each lead to it
    to_chat(&mut op);
    op.until("the first-start row", |s| flat(s).contains("⏎ opens Connect › Setup"));
    keys(&mut op, typed("make the tests pass", true));
    op.until("the refusal that leads", |s| flat(s).contains("no agent works") && flat(s).contains("⏎ opens Connect › Setup"));
    assert_eq!(count(&w, "task.queued"), 0, "no task was started");
    keys(&mut op, vec![press(KeyCode::Enter)]);
    op.until("Connect › Setup from the refusal", |s| on_setup(s) && s.contains("none works"));
    hits.extend(in_world(&op, &w, &values));
    shown.append(&mut op.shown);
    drawn += op.drawn;
    drop(op);
    w.close();
    drop(w);

    // (4) the second start, pinned to the test doer: the card was answered for this home, so it is not asked again
    let w = open_walk(&d, Some("test"));
    let mut op = Operator::scanning(&w, never.clone(), words.clone());
    started(&w, false);
    assert!(w.front.welcome_asks().is_empty(), "answered once for the home");
    let s = setup_page(&mut op);
    assert!(s.lines().any(|l| l.contains("Agent") && l.contains("test · ready · pinned at start")), "the Agent step names the doer:\n{s}");
    to_row(&mut op, "Agent");
    keys(&mut op, vec![press(KeyCode::Enter)]);
    op.until("the test doer in use", |s| s.lines().any(|l| l.contains("test") && l.contains("in use · pinned at start")));
    // the Decider: `o` on Jev's row asks on the card, which names what it widens; `y` turns it on from the next start
    to_row(&mut op, "Jev · System One");
    keys(&mut op, vec![press(KeyCode::Char('o'))]);
    op.until("the card that names what the Decider widens", |s| flat(s).contains("widens what J4n may do") && s.contains("y yes"));
    keys(&mut op, vec![press(KeyCode::Char('y'))]);
    op.until("the step says it waits for the next start", |s| s.contains("on from the next start · ⟳"));
    // the row that waits has the key that restarts now, on the page itself
    op.until("the key that restarts", |s| s.lines().next_back().is_some_and(|l| l.contains("r restart now")));
    keys(&mut op, vec![press(KeyCode::Char('r'))]);
    until(&mut op, &w, "the restart asked", || w.front.restart_asked());
    hits.extend(in_world(&op, &w, &values));
    shown.append(&mut op.shown);
    drawn += op.drawn;
    drop(op);
    w.close();
    drop(w);

    // the test answerer: Jev on the loopback, taking the key the walk types. Its address is the one file the test writes into the HOME, while
    // J4n is down and before the key is typed, so the key goes nowhere else; written earlier, a folder's connector `jev` would have turned
    // the Decider's pack on by itself and there would be nothing to turn on
    let cases: Cases = std::sync::Arc::default();
    let jev = gateway(&cases, SETUP_KEY.value);
    fs::create_dir_all(d.j4n_home().join("connectors")).unwrap();
    fs::write(d.j4n_home().join("connectors/jev.toml"), format!("api = 1\n[[connector]]\nid = \"jev\"\nurl = \"{}\"\n", jev.url())).unwrap();

    // J4n started again over the same folders, as `:restart` does: the Decider is loaded, its key not typed yet
    let w = open_walk(&d, Some("test"));
    learn(&w, &cases);
    let mut op = Operator::scanning(&w, never.clone(), words.clone());
    started(&w, false);
    let s = setup_page(&mut op);
    assert!(s.contains("on · its key is not typed yet"), "the Decider is on and waits for its key:\n{s}");
    // the key, typed on Jev's row in its masked editor: the Decider is made live by it and passes its probe cases
    let sets = count(&w, "key.set");
    to_row(&mut op, "Jev · System One");
    keys(&mut op, vec![press(KeyCode::Enter)]);
    op.until("the masked editor", |s| s.contains("paste or type the key"));
    keys(&mut op, typed(SETUP_KEY.value, false));
    keys(&mut op, vec![press(KeyCode::Enter)]);
    until(&mut op, &w, "the key kept", || count(&w, "key.set") > sets);
    until(&mut op, &w, "the Decider's probes passed", || crate::deciders_flow::jev_probes(&w).iter().any(|(_, p)| p["passed"] == true));
    op.until("the Decider answers", |s| s.contains("on · Jev · System One answers"));
    // (5) ⏎ on Test: the proof call passes for the doer and the Decider
    let asked = jev.seen().len();
    let t = run_test(&mut op, &w, "not run");
    assert_eq!(t["passed"], true, "{t:#}");
    let parts: Vec<(&str, &str, bool)> = t["parts"].as_array().unwrap().iter().map(|p| (p["part"].as_str().unwrap_or(""), p["id"].as_str().unwrap_or(""), p["ok"] == true)).collect();
    assert_eq!(parts, [("agent", "test", true), ("decider", "decider:jev", true)], "the doer's turn and the Decider's cases, both ok");
    assert!(jev.seen().len() > asked, "the test asked the Decider");
    // (6) done: the test passed for what is set now, and every step is done
    op.until("the test passed", |s| s.lines().any(|l| l.contains("Test") && l.contains("passed ·")));
    let steps = done_steps(&w);
    for id in ["Agent", "Model", "Decider", "Test"] {
        assert!(steps.iter().any(|(s, done)| s == id && *done), "{id} is done: {steps:?}");
    }
    // (7) a chat line, said and answered
    to_chat(&mut op);
    let (settled, asked) = (count(&w, "task.settled"), jev.seen().len());
    keys(&mut op, typed("say the walk is over", true));
    until(&mut op, &w, "the line's turn ended", || count(&w, "task.settled") > settled);
    wait_idle(&w);
    // the line's own turn: the test doer's, on the main chat, ended well with the words it was given
    let rows = w.front.tasks().book().rows();
    let turn = rows.iter().find(|r| r.spec.goal == "say the walk is over").unwrap_or_else(|| panic!("no turn of the line: {:?}", rows.iter().map(|r| &r.spec.goal).collect::<Vec<_>>()));
    assert!(turn.spec.doer == "test" && turn.state == j4n_core::task::State::Done { ok: true } && turn.answer.as_deref() == Some("the walk is over"), "{turn:?}");
    // the Decider, live, was put the line's intake, and the line went on to the doer
    assert!(jev.seen().len() > asked, "the line's intake asked the Decider");
    op.until("the answer", |s| s.lines().filter(|l| l.contains("the walk is over")).count() >= 2);
    hits.extend(in_world(&op, &w, &values));
    shown.append(&mut op.shown);
    drawn += op.drawn;
    drop(op);
    w.close();
    drop(w);

    // no frame showed a shell command, a path or a file
    assert!(drawn > 60, "the walk drew its frames: {drawn}");
    let (r1, keys_shown): (Vec<String>, Vec<String>) = shown.into_iter().partition(|h| h.starts_with('`'));
    assert!(r1.is_empty(), "texts shown on the walk that send the operator out of the TUI:\n{}", r1.join("\n"));
    // the canary: the key was in no frame, no request body, no file but its own
    hits.extend(keys_shown);
    let auth = format!("Bearer {}", SETUP_KEY.value);
    assert!(jev.seen().iter().any(|r| r.headers.get("authorization") == Some(&auth)), "the Decider was given its key");
    hits.extend(in_requests("the Decider's gateway", &jev.seen(), SETUP_KEY.value, &values));
    let kept = d.j4n_home().join("keys");
    hits.extend(in_files(&[d.home(), d.project()], &kept, &values));
    assert_eq!(fs::metadata(&kept).unwrap().permissions().mode() & 0o777, 0o700, "the keys' folder");
    let mut held = Vec::new();
    for f in files(&kept) {
        assert_eq!(fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o600, "{}", f.display());
        assert_eq!(fs::read_to_string(&f).unwrap(), SETUP_KEY.value, "{} holds the key typed and nothing else", f.display());
        held.push(f.file_name().unwrap().to_string_lossy().into_owned());
    }
    assert!(!held.is_empty() && held.iter().all(|n| ["typesafe", "jev"].contains(&n.as_str())), "TypeSafe's key: {held:?}");
    assert!(hits.is_empty(), "the key typed on Setup is where it never goes:\n{}", hits.join("\n"));
}

/// CHARTER law 11's own check: every `[[command]]` of every pack, all of them on, resolves to a row of the palette, the row the jump and
/// `⌃k` read.
#[test]
fn every_command_of_every_pack_resolves_to_a_row_of_the_palette() {
    let d = Dirs::new("no-shell-walk-palette");
    // every pack on: the ones that ship off too (bench, qc, shape, terminal, test)
    let mut harness = String::from("api = 1\n");
    for id in ["bench", "qc", "shape", "terminal", "test", "crew"] {
        harness.push_str(&format!("[[enable]]\nid = \"{id}\"\n"));
    }
    fs::write(d.j4n_home().join("harness.toml"), harness).unwrap();
    let world = crate::ui_keys::open(&d);
    let palette: Vec<String> = world.front.screen().palette().into_iter().map(|r| r.key).collect();
    let root = crate::support::root();
    let mut found = 0;
    for pack in fs::read_dir(root.join("packs")).unwrap().flatten() {
        for file in fs::read_dir(pack.path().join("commands")).into_iter().flatten().flatten() {
            let text = fs::read_to_string(file.path()).unwrap();
            let table: toml::Table = toml::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", file.path().display()));
            for c in table.get("command").and_then(toml::Value::as_array).into_iter().flatten() {
                let id = c.get("id").and_then(toml::Value::as_str).expect("a command has an id");
                assert!(palette.iter().any(|k| k == id), "[[command]] {id} ({}) is no row of the palette: {palette:?}", file.path().display());
                found += 1;
            }
        }
    }
    assert!(found >= 40, "the packs' commands were read: {found}");
    world.close();
}

/// What tells the operator to run something by hand: a shell command, a path in the J4n folder, a file written by
/// hand, a restart by hand. Lower case; a text is matched lower-cased.
const BY_HAND: [&str; 13] = [
    "start j4n again",
    "by hand",
    "~/.j4n",
    "j4n doctor",
    "j4n pack ",
    "j4n watch",
    "j4n report",
    "claude /login",
    "run it once in a terminal",
    "put it on your path",
    "install chromium",
    "export openrouter",
    "export anthropic",
];

/// The texts the scan leaves, each with why: the CLI before or after the TUI, the pack author's CLI, a file J4n writes and
/// nobody is shown, and one left open, with its reason. An exemption that no longer matches anything fails too, so the list stays true.
const EXEMPT: [(&str, &str, &str); 5] = [
    ("crates/j4n/src/cli.rs", "", "the CLI's own help: before or after the TUI"),
    ("crates/j4n/src/author.rs", "j4n pack ", "the pack author's CLI"),
    ("crates/j4n-core/src/pack/check.rs", "j4n pack ", "the README and tests `j4n pack new` writes for the pack author"),
    ("crates/j4n-core/src/front/lookfile.rs", "~/.j4n", "the head of the file the look's pass writes, never shown"),
    ("crates/j4n-connect/src/connect.rs", "by hand", "a program no sniff row fits has no connector form yet"),
];

/// Every text of the packs (TOML, comments left out) and of the crates' code (string lines outside comments and tests) that the TUI can
/// show, as (file, line, text).
fn shown_texts(root: &Path) -> Vec<(String, usize, String)> {
    fn walk(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
        for e in fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, ext, out);
            } else if p.extension().is_some_and(|x| x == ext) {
                out.push(p);
            }
        }
    }
    let mut files = Vec::new();
    walk(&root.join("packs"), "toml", &mut files);
    for c in ["j4n-core", "j4n-connect", "j4n-tui", "j4n"] {
        walk(&root.join("crates").join(c).join("src"), "rs", &mut files);
    }
    let mut out = Vec::new();
    for f in files {
        let rel = f.strip_prefix(root).unwrap().display().to_string();
        let rust = rel.ends_with(".rs");
        for (n, line) in fs::read_to_string(&f).unwrap_or_default().lines().enumerate() {
            let t = line.trim_start();
            if rust && t.starts_with("#[cfg(test)]") {
                break;
            }
            if t.starts_with('#') && !rust || t.starts_with("//") || (rust && !t.contains('"')) {
                continue;
            }
            out.push((rel.clone(), n + 1, line.to_lowercase()));
        }
    }
    out
}

/// no text the TUI can show tells the operator to run a shell command, write a file by hand, or start J4n again, but those the
/// list above exempts with a reason.
#[test]
fn no_text_the_tui_shows_sends_the_operator_to_a_shell_or_a_file() {
    let root = crate::support::root();
    let mut found = Vec::new();
    let mut used = [false; EXEMPT.len()];
    for (file, n, text) in shown_texts(&root) {
        for pat in BY_HAND.iter().filter(|p| text.contains(**p)) {
            match EXEMPT.iter().position(|(f, p, _)| file == *f && (p.is_empty() || p == pat)) {
                Some(i) => used[i] = true,
                None => found.push(format!("{file}:{n} says `{pat}`: {}", text.trim())),
            }
        }
    }
    assert!(found.is_empty(), "texts that send the operator out of the TUI:\n{}", found.join("\n"));
    for (i, (f, p, why)) in EXEMPT.iter().enumerate() {
        assert!(used[i], "the exemption of {f} (`{p}`, {why}) matches nothing now: take it out");
    }
}
