//! What `theme.marks = auto` (the default) draws: `icons` where the icons can be drawn, `unicode` where nothing says they can. The
//! icons are those of a Nerd Font, so they can be drawn where the terminal draws them with a font of its own (ghostty, WezTerm, kitty)
//! or a Nerd Font is installed (the font folders of the OS, then fontconfig's list). An installed font is no proof that the terminal
//! draws with it, so the row of the setting says what was found and why, and a word the operator sets wins. The screen is sent a tier,
//! never `auto`.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

/// The word of `theme.marks` that is no tier: the core makes it one before the screen is sent the look.
pub const AUTO: &str = "auto";

/// What `auto` comes to, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub icons: bool,
    pub why: String,
}

impl Found {
    /// The tier the screen is sent.
    pub fn tier(&self) -> &'static str {
        if self.icons { "icons" } else { "unicode" }
    }

    /// What the row of `theme.marks` says while it is `auto`.
    pub fn shown(&self) -> String {
        format!("{AUTO} · {}: {}", self.tier(), self.why)
    }
}

/// What the row of `theme.marks` says for `word`: what `auto` comes to here and why, any other word as it is.
pub fn row_word(word: &str) -> String {
    if word == AUTO { found().shown() } else { word.to_string() }
}

/// The words the operator may say of a part of the look: the screen's, and `auto` first for the marks.
pub fn said_words(part: &str, screen: &[&'static str]) -> Vec<&'static str> {
    let auto = (part == "marks").then_some(AUTO);
    auto.into_iter().chain(screen.iter().copied()).collect()
}

/// The terminal, when it draws the icons of a Nerd Font with a font of its own, whatever is installed. `env` reads a variable.
pub fn own_icons(env: &dyn Fn(&str) -> Option<String>) -> Option<&'static str> {
    let low = |k: &str| env(k).unwrap_or_default().to_ascii_lowercase();
    let (program, term) = (low("TERM_PROGRAM"), low("TERM"));
    let named = |w: &str| program.contains(w) || term.contains(w);
    if named("ghostty") {
        Some("ghostty")
    } else if named("wezterm") {
        Some("WezTerm")
    } else if named("kitty") || env("KITTY_WINDOW_ID").is_some() {
        Some("kitty")
    } else {
        None
    }
}

/// Whether a file, folder or family name is a Nerd Font's: `Nerd Font` in any case and spacing (`JetBrainsMonoNerdFont-Regular.ttf`,
/// `Hack Nerd Font Mono`, `nerd-fonts`), or a family's short name (`JetBrainsMono NFM`).
pub fn is_nerd(name: &str) -> bool {
    let squeezed: String = name.chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_ascii_lowercase();
    squeezed.contains("nerdfont") || name.split(|c: char| !c.is_ascii_alphanumeric()).any(|w| matches!(w, "NF" | "NFM" | "NFP"))
}

/// The folders fonts are installed in on this OS: the operator's (under `home`, or `XDG_DATA_HOME`) and the system's.
pub fn folders(home: &Path, env: &dyn Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    if cfg!(target_os = "macos") {
        return [home.join("Library/Fonts"), "/Library/Fonts".into(), "/System/Library/Fonts".into(), "/opt/homebrew/share/fonts".into(), "/usr/local/share/fonts".into()].into();
    }
    let data = env("XDG_DATA_HOME").filter(|d| d.starts_with('/')).map_or_else(|| home.join(".local/share"), PathBuf::from);
    let dirs = env("XDG_DATA_DIRS").filter(|d| !d.is_empty()).unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    [data.join("fonts"), home.join(".fonts")].into_iter().chain(dirs.split(':').filter(|d| d.starts_with('/')).map(|d| Path::new(d).join("fonts"))).collect()
}

/// The first Nerd Font found in `dirs` (a font file whose name or folder says so), as its name (or `a Nerd Font`, when only the folder says); three folders deep and a few thousand
/// entries at most, so a large font folder costs a few milliseconds.
pub fn in_folders(dirs: &[PathBuf]) -> Option<String> {
    let mut left = 20_000usize;
    let mut todo: Vec<(PathBuf, u8, bool)> = dirs.iter().rev().map(|d| (d.clone(), 0, false)).collect();
    while let Some((dir, depth, nerd_dir)) = todo.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            left = left.checked_sub(1)?;
            let name = e.file_name().to_string_lossy().into_owned();
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                if depth < 3 {
                    todo.push((e.path(), depth + 1, nerd_dir || is_nerd(&name)));
                }
                continue;
            }
            let font = [".ttf", ".otf", ".ttc"].iter().any(|x| name.to_ascii_lowercase().ends_with(x));
            if font && (nerd_dir || is_nerd(&name)) {
                return Some(if is_nerd(&name) { name.split(['-', '.']).next().unwrap_or(&name).to_string() } else { "a Nerd Font".into() });
            }
        }
    }
    None
}

/// The first Nerd Font family in fontconfig's list (`fc-list : family`, a line a font, families split by commas).
pub fn in_list(list: &str) -> Option<String> {
    list.lines().flat_map(|l| l.split(',')).map(str::trim).find(|f| is_nerd(f)).map(String::from)
}

/// What `auto` comes to: `J4N_FONTS` (`icons` or `unicode`) when it says, without looking; else the terminal's own icons, then a Nerd
/// Font in the folders, then in fontconfig's list (`list`, asked only when the folders have none).
pub fn decide(env: &dyn Fn(&str) -> Option<String>, dirs: &[PathBuf], list: &dyn Fn() -> Option<String>) -> Found {
    if let Some(w) = env("J4N_FONTS").filter(|w| w == "icons" || w == "unicode") {
        return Found { icons: w == "icons", why: "J4N_FONTS".into() };
    }
    if let Some(term) = own_icons(env) {
        return Found { icons: true, why: format!("by {term}") };
    }
    // no font's name in the row: it stays short on a narrow screen
    let icons = in_folders(dirs).or_else(|| list().as_deref().and_then(in_list)).is_some();
    Found { icons, why: (if icons { "Nerd Font found" } else { "no Nerd Font" }).into() }
}

/// fontconfig's list of families, when `fc-list` is installed and answers within `wait`; one that is slower is left to end on its own. It
/// inherits no host's variables and no key, as every program J4n starts.
fn fc_list(wait: Duration) -> Option<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut cmd = std::process::Command::new("fc-list");
    crate::host::Scrub::base().strip(&mut cmd);
    let child = cmd.args([":", "family"]).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).spawn().ok()?;
    std::thread::spawn(move || tx.send(child.wait_with_output()));
    let out = rx.recv_timeout(wait).ok()?.ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// What `auto` comes to on this machine, looked for once. A test run says `J4N_FONTS=icons` (`scripts/t`), so what it draws does not
/// hang on the fonts of the machine it runs on.
pub fn found() -> &'static Found {
    static FOUND: OnceLock<Found> = OnceLock::new();
    FOUND.get_or_init(|| {
        let env = |k: &str| std::env::var(k).ok();
        let home = env("HOME").map(PathBuf::from).unwrap_or_default();
        decide(&env, &folders(&home, &env), &|| fc_list(Duration::from_millis(400)))
    })
}

/// Looks for the fonts on a thread of its own, so the first look the screen is sent seldom waits for it.
pub fn warm() {
    std::thread::spawn(|| {
        found();
    });
}
