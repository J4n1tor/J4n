//! What the marks' `auto` comes to: the terminal's own icons, a Nerd Font in the font folders, one in fontconfig's list, or none; told
//! from fake folders, a fake list and a fake environment, so no case hangs on the fonts of the machine it runs on.

use j4n_core::front::fonts::{AUTO, Found, decide, in_folders, in_list, is_nerd, own_icons, said_words};
use std::cell::Cell;
use std::fs;
use std::path::PathBuf;

/// Font folders under the run's own temporary folder, one file a path (`a/b.ttf`).
fn folders(tag: &str, files: &[&str]) -> PathBuf {
    let root = std::env::temp_dir().join(format!("j4n-fonts-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    for f in files {
        let p = root.join(f);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b"").unwrap();
    }
    fs::create_dir_all(&root).unwrap();
    root
}

fn none(_: &str) -> Option<String> {
    None
}

#[test]
fn a_nerd_font_is_told_by_its_file_its_family_or_its_folder() {
    for name in ["JetBrainsMonoNerdFont-Regular.ttf", "Hack Regular Nerd Font Complete.ttf", "Hack Nerd Font Mono", "JetBrainsMono NFM", "SymbolsNerdFontMono-Regular.ttf", "nerd-fonts"] {
        assert!(is_nerd(name), "{name}");
    }
    for name in ["JetBrainsMono-Regular.ttf", "DejaVuSansMono.ttf", "Menlo.ttc", "Nerdy Sans", "INFO.ttf"] {
        assert!(!is_nerd(name), "{name}");
    }
}

#[test]
fn a_nerd_font_in_a_folder_gives_the_icons_and_fontconfig_is_not_asked() {
    let root = folders("found", &["TTF/DejaVuSansMono.ttf", "TTF/JetBrainsMonoNerdFont-Regular.ttf", "notes/NerdFont.txt"]);
    let asked = Cell::new(0);
    let list = || {
        asked.set(asked.get() + 1);
        None
    };
    let found = decide(&none, &[root.join("none-here"), root.clone()], &list);
    assert_eq!(found, Found { icons: true, why: "Nerd Font found".into() });
    assert_eq!((found.tier(), found.shown().as_str()), ("icons", "auto · icons: Nerd Font found"), "the row says what was found");
    assert_eq!(asked.get(), 0, "the folders answered: fontconfig is not started");
    // a folder that says it is a Nerd Font's is enough for the files in it; a file that is no font is not
    let dir = folders("dir", &["nerd-fonts/Regular.ttf"]);
    assert_eq!(in_folders(std::slice::from_ref(&dir)).as_deref(), Some("a Nerd Font"));
    let text = folders("text", &["notes/NerdFont.txt"]);
    assert_eq!(in_folders(std::slice::from_ref(&text)), None);
    for d in [root, dir, text] {
        let _ = fs::remove_dir_all(d);
    }
}

#[test]
fn with_no_nerd_font_in_the_folders_fontconfig_is_asked_and_without_one_there_either_it_is_unicode() {
    let root = folders("plain", &["truetype/DejaVuSansMono.ttf", "Menlo.ttc"]);
    let list = "DejaVu Sans Mono\nNoto Sans,Noto Sans Display\nHack Nerd Font Mono,Hack NFM\n";
    assert_eq!(in_list(list).as_deref(), Some("Hack Nerd Font Mono"));
    let found = decide(&none, std::slice::from_ref(&root), &|| Some(list.to_string()));
    assert_eq!(found, Found { icons: true, why: "Nerd Font found".into() }, "fontconfig knows a folder the list of folders does not");
    let found = decide(&none, std::slice::from_ref(&root), &|| Some("DejaVu Sans Mono\nMenlo\n".into()));
    assert_eq!(found, Found { icons: false, why: "no Nerd Font".into() });
    assert_eq!(found.shown(), "auto · unicode: no Nerd Font");
    let found = decide(&none, &[root.join("gone")], &|| None);
    assert_eq!(found.tier(), "unicode", "no folder and no fontconfig (a Mac out of the box): unicode");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_terminal_that_draws_the_icons_itself_gives_them_whatever_is_installed() {
    let env = |pairs: &'static [(&'static str, &'static str)]| move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string());
    assert_eq!(own_icons(&env(&[("TERM_PROGRAM", "ghostty")])), Some("ghostty"));
    assert_eq!(own_icons(&env(&[("TERM", "xterm-ghostty")])), Some("ghostty"));
    assert_eq!(own_icons(&env(&[("TERM_PROGRAM", "WezTerm")])), Some("WezTerm"));
    assert_eq!(own_icons(&env(&[("TERM", "xterm-kitty")])), Some("kitty"));
    assert_eq!(own_icons(&env(&[("TERM_PROGRAM", "Apple_Terminal"), ("TERM", "xterm-256color")])), None, "Terminal.app draws with the font it is given");
    assert_eq!(own_icons(&env(&[("TERM_PROGRAM", "iTerm.app")])), None);
    let asked = Cell::new(false);
    let found = decide(&env(&[("TERM_PROGRAM", "ghostty")]), &[PathBuf::from("/nowhere")], &|| {
        asked.set(true);
        None
    });
    assert_eq!(found.shown(), "auto · icons: by ghostty");
    assert!(!asked.get());
}

#[test]
fn j4n_fonts_says_what_auto_is_without_looking() {
    let asked = Cell::new(false);
    let list = || {
        asked.set(true);
        None
    };
    let say = |w: &'static str| move |k: &str| (k == "J4N_FONTS").then(|| w.to_string()).or_else(|| (k == "TERM").then(|| "xterm-kitty".to_string()));
    assert_eq!(decide(&say("unicode"), &[PathBuf::from("/nowhere")], &list), Found { icons: false, why: "J4N_FONTS".into() }, "it wins over a terminal with its own icons");
    assert_eq!(decide(&say("icons"), &[PathBuf::from("/nowhere")], &list).shown(), "auto · icons: J4N_FONTS");
    assert!(!asked.get());
    assert_eq!(decide(&say("emoji"), &[PathBuf::from("/nowhere")], &list).shown(), "auto · icons: by kitty", "a word it does not know is no word");
}

#[test]
fn auto_is_a_word_of_the_marks_only() {
    assert_eq!(said_words("marks", &["icons", "unicode", "ascii", "words"]), [AUTO, "icons", "unicode", "ascii", "words"]);
    assert_eq!(said_words("frame", &["pipes", "round"]), ["pipes", "round"]);
}

#[cfg(not(target_os = "macos"))]
#[test]
fn the_font_folders_are_the_operators_and_the_systems() {
    use j4n_core::front::fonts::folders as dirs;
    let home = PathBuf::from("/home/user");
    let plain = dirs(&home, &none);
    assert_eq!(plain, ["/home/user/.local/share/fonts", "/home/user/.fonts", "/usr/local/share/fonts", "/usr/share/fonts"].map(PathBuf::from));
    let env = |k: &str| match k {
        "XDG_DATA_HOME" => Some("/home/user/data".to_string()),
        "XDG_DATA_DIRS" => Some("/run/current-system/sw/share:relative".to_string()),
        _ => None,
    };
    assert_eq!(dirs(&home, &env), ["/home/user/data/fonts", "/home/user/.fonts", "/run/current-system/sw/share/fonts"].map(PathBuf::from), "a relative folder is not one");
}
