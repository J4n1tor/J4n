//! The scenes and their goldens. A scene is a TOML list of what happens to the screen — messages
//! from the core, keys, a paste, a resize, time — run into memory by `j4n_tui::scene`; its golden is the screen as text (`.txt`) and
//! with its style as ANSI (`.ansi`), one pair for each size. The files are `crates/j4n-tui/tests/scenes/<scene>.toml` and
//! `crates/j4n-tui/tests/golden/<scene>[.<shot>].<cols>x<rows>.{txt,ansi}`. `J4N_BLESS=1 scripts/t test …` writes the goldens the
//! scenes now give; a run without it fails on any difference, on a golden no scene gives, and on an icon not followed by a blank cell.
//! The reading of the TOML is here and not in the crate because the view layer may depend on the protocol and the UI crates only.

use crate::support::{assert_clean, read, rel, root, walk_ext};
use j4n_proto::Down;
use j4n_tui::app::Preview;
use j4n_tui::golden;
use j4n_tui::theme::{Settings, Theme};
use j4n_tui::scene::{self, Scene, Shot, Step};
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

fn scenes_dir() -> PathBuf {
    root().join("crates/j4n-tui/tests/scenes")
}

fn golden_dir() -> PathBuf {
    root().join("crates/j4n-tui/tests/golden")
}

/// The keys a step may have besides a message; a step has exactly one.
const STEP_KEYS: [&str; 7] = ["key", "text", "paste", "resize", "wait", "preview", "shot"];

fn step(t: &toml::Table) -> Result<Step, String> {
    if t.contains_key("down") {
        return toml::Value::Table(t.clone()).try_into::<Down>().map(|d| Step::Down(Box::new(d))).map_err(|e| format!("a message: {e}"));
    }
    let [(k, v)] = t.iter().collect::<Vec<_>>()[..] else {
        return Err(format!("a step has one of {} or `down`; this has {:?}", STEP_KEYS.join(", "), t.keys().collect::<Vec<_>>()));
    };
    let text = || v.as_str().map(String::from).ok_or(format!("`{k}` is text"));
    match k.as_str() {
        "key" => text().and_then(|w| scene::key(&w).map(Step::Key).ok_or(format!("`{w}` is not a key"))),
        "text" => text().map(Step::Text),
        "paste" => text().map(Step::Paste),
        "shot" => text().map(Step::Shot),
        "wait" => v.as_integer().and_then(|n| u64::try_from(n).ok()).map(Step::Wait).ok_or("`wait` is milliseconds".into()),
        "resize" => size(v).map(|(c, r)| Step::Resize(c, r)),
        "preview" => match v.as_str() {
            Some("roles") => Ok(Step::Preview(Some(Preview::Roles))),
            Some("glyphs") => Ok(Step::Preview(Some(Preview::Glyphs))),
            Some("off") => Ok(Step::Preview(None)),
            _ => Err("`preview` is `roles`, `glyphs` or `off`".into()),
        },
        other => Err(format!("`{other}` is not a step; a step has one of {} or `down`", STEP_KEYS.join(", "))),
    }
}

fn size(v: &toml::Value) -> Result<(u16, u16), String> {
    let two = v.as_array().filter(|a| a.len() == 2).map(|a| a.iter().filter_map(|n| n.as_integer().and_then(|n| u16::try_from(n).ok().filter(|n| *n > 0))).collect::<Vec<_>>());
    match two.as_deref() {
        Some([c, r]) => Ok((*c, *r)),
        _ => Err("a size is [columns, rows], both above 0".into()),
    }
}

/// The `theme` of a scene: the settings of the Table by their names, the rest as the default has them. A look the theme refuses is
/// the scene's error.
fn look(v: Option<&toml::Value>) -> Result<Settings, String> {
    let mut s = Settings::default();
    let Some(v) = v else { return Ok(s) };
    let t = v.as_table().ok_or("`theme` is a table")?;
    for (k, v) in t {
        let word = || v.as_str().map(String::from).ok_or(format!("`theme.{k}` is a word"));
        let flag = || v.as_bool().ok_or(format!("`theme.{k}` is true or false"));
        match k.as_str() {
            "palette" => s.palette = word()?,
            "marks" => s.marks = word()?,
            "frame" => s.frame = word()?,
            "corners" => s.corners = word()?,
            "title_place" => s.title_place = word()?,
            "density" => s.density = word()?,
            "fill" => s.fill = word()?,
            "you" => s.you = word()?,
            "motion" => s.motion = flag()?,
            "no_color" => s.no_color = flag()?,
            "chart" => s.chart = word()?,
            "truecolor" => s.truecolor = flag()?,
            other => return Err(format!("`theme.{other}` is not a theme setting")),
        }
    }
    Theme::build(&s, &[]).map(|_| s)
}

/// The `read` of a scene: `style`, `width`, `fold_code`, `links` as the settings have them, and `caps` (`plain` or `all`), the terminal it is
/// run on. None is the calm default on a terminal that is not known.
fn reading(v: Option<&toml::Value>) -> Result<j4n_tui::read::Opts, String> {
    use j4n_tui::read::{Caps, Opts};
    let mut o = Opts::default();
    let Some(v) = v else { return Ok(o) };
    for (k, v) in v.as_table().ok_or("`read` is a table")? {
        let word = || v.as_str().ok_or(format!("`read.{k}` is a word"));
        let num = || v.as_integer().and_then(|n| usize::try_from(n).ok()).ok_or(format!("`read.{k}` is a number"));
        match k.as_str() {
            "style" => o.full = word()? == "full",
            "width" => o.width = num()?,
            "fold_code" => o.fold = num()?,
            "links" => o.osc8 = word()? != "plain",
            "caps" => o.caps = if word()? == "all" { Caps::ALL } else { Caps::PLAIN },
            "root" => o.root = Some(word()?.into()),
            other => return Err(format!("`read.{other}` is not a reading setting")),
        }
    }
    Ok(o)
}

/// A scene file read.
fn parse(name: &str, text: &str) -> Result<Scene, String> {
    let table: toml::Table = text.parse().map_err(|e| format!("not TOML: {e}"))?;
    if let Some(k) = table.keys().find(|k| !["doc", "sizes", "theme", "read", "step"].contains(&k.as_str())) {
        return Err(format!("`{k}` is not part of a scene: it has `doc`, `sizes`, `theme`, `read` and `[[step]]`"));
    }
    let theme = look(table.get("theme"))?;
    let read = reading(table.get("read"))?;
    let sizes = match table.get("sizes") {
        None => Vec::new(),
        Some(s) => s.as_array().ok_or("`sizes` is a list of [columns, rows]")?.iter().map(size).collect::<Result<_, _>>()?,
    };
    let steps = match table.get("step") {
        None => Vec::new(),
        Some(s) => {
            let list = s.as_array().ok_or("`step` is a list of tables")?;
            list.iter().enumerate().map(|(i, s)| s.as_table().ok_or("a step is a table".to_string()).and_then(step).map_err(|e| format!("step {}: {e}", i + 1))).collect::<Result<_, _>>()?
        }
    };
    Ok(Scene { name: name.into(), sizes, theme, read, steps })
}

fn bless() -> bool {
    std::env::var_os("J4N_BLESS").is_some_and(|v| !v.is_empty())
}

/// What a shot differs in from its golden files, or writes them when blessing.
fn check(shot: &Shot, wanted: &mut BTreeSet<PathBuf>) -> Vec<String> {
    let mut found = Vec::new();
    for (ext, got) in [("txt", &shot.snap.text), ("ansi", &shot.snap.ansi)] {
        let path = golden_dir().join(format!("{}.{ext}", shot.file()));
        wanted.insert(path.clone());
        let want = format!("{got}\n");
        if bless() {
            fs::write(&path, &want).unwrap_or_else(|e| panic!("write {}: {e}", rel(&path)));
        } else {
            match fs::read_to_string(&path) {
                Err(_) => found.push(format!("{}: there is no golden; J4N_BLESS=1 writes it", rel(&path))),
                Ok(have) => found.extend(golden::differ(&have, &want).map(|d| format!("{}: {d}", rel(&path)))),
            }
        }
    }
    found.extend(golden::spacing(&shot.snap.text).into_iter().map(|d| format!("{}: {d}", shot.file())));
    found
}

fn all() -> Vec<Scene> {
    walk_ext(&scenes_dir(), "toml")
        .iter()
        .map(|p| {
            let name = p.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_string();
            parse(&name, &read(p)).unwrap_or_else(|e| panic!("{}: {e}", rel(p)))
        })
        .collect()
}

#[test]
fn every_scene_gives_its_golden() {
    let scenes = all();
    assert!(!scenes.is_empty(), "crates/j4n-tui/tests/scenes has no scene");
    let (mut found, mut wanted) = (Vec::new(), BTreeSet::new());
    for scene in &scenes {
        found.extend(scene::run(scene).iter().flat_map(|s| check(s, &mut wanted)));
    }
    for path in walk_ext(&golden_dir(), "txt").into_iter().chain(walk_ext(&golden_dir(), "ansi")).filter(|p| !wanted.contains(p)) {
        if bless() {
            fs::remove_file(&path).unwrap_or_else(|e| panic!("remove {}: {e}", rel(&path)));
        } else {
            found.push(format!("{}: no scene gives this golden", rel(&path)));
        }
    }
    assert_clean("scenes and goldens", &found);
}

#[test]
fn the_suggestions_never_name_the_decider_by_its_inside_name() {
    // on screen the Decider is only `~`: no cell of the suggestion scenes, at any size or tier, holds the connector's own name
    let scenes: Vec<Scene> = all().into_iter().filter(|s| s.name.starts_with("composer-suggest")).collect();
    assert_eq!(scenes.len(), 3, "the scene and its ascii and NO_COLOR twins");
    for shot in scenes.iter().flat_map(scene::run) {
        assert!(!shot.snap.text.to_lowercase().contains("jev"), "{}: {}", shot.file(), shot.snap.text);
    }
}

#[test]
fn the_empty_scene_renders_at_each_of_its_sizes() {
    let scene = parse("none", "sizes = [[80, 24], [40, 10]]\n").expect("a scene with no step");
    let shots = scene::run(&scene);
    assert_eq!(shots.iter().map(Shot::file).collect::<Vec<_>>(), ["none.80x24", "none.40x10"]);
    for s in &shots {
        assert_eq!(s.snap.text.split('\n').count(), usize::from(s.rows), "a row on the screen for each row of the terminal");
    }
    assert_eq!(scene::run(&parse("none", "").expect("no size")).iter().map(Shot::file).collect::<Vec<_>>(), ["none.80x24"], "80×24 when it names none");
}

#[test]
fn a_scene_drives_the_screen_with_messages_keys_a_paste_and_a_resize() {
    let text = r#"
[[step]]
down = "hello"
api = 1
name = "j4n"
version = "0"
project = "demo"
[[step]]
down = "tree"
node = { id = "root", title = "", children = [{ id = "pages", title = "", present = "tabs", children = [{ id = "chat", title = "Chat", field = { kind = "chat", bind = "chat:main" } }] }] }
[[step]]
text = "hi"
[[step]]
shot = "typed"
[[step]]
paste = "a\nb"
[[step]]
key = "enter"
[[step]]
resize = [50, 12]
"#;
    let scene = parse("drive", text).expect("a scene");
    let shots = scene::run(&scene);
    assert_eq!(shots.iter().map(Shot::file).collect::<Vec<_>>(), ["drive.typed.80x24", "drive.50x12"]);
    assert!(shots[0].snap.text.contains("hi"), "the typed letters are on the screen:\n{}", shots[0].snap.text);
    assert_eq!((shots[1].cols, shots[1].rows), (50, 12), "the end is at the size the scene resized to");
    assert!(shots[1].sent.iter().any(|u| matches!(u, j4n_proto::Up::Resize { cols: 50, rows: 12 })), "a resize is told to the core: {:?}", shots[1].sent);
    let said: Vec<&str> = shots[1].sent.iter().filter_map(|u| if let j4n_proto::Up::Intent { intent: j4n_proto::Intent::Say { text, .. } } = u { Some(text.as_str()) } else { None }).collect();
    assert!(said.len() == 1 && said[0].starts_with("hi") && said[0].ends_with('b'), "the Enter sent what was typed and pasted as one line: {said:?}");
}

#[test]
fn time_passes_in_a_scene_only_when_it_says_so() {
    let text = r#"
[[step]]
down = "hello"
api = 1
name = "j4n"
version = "0"
project = "demo"
[[step]]
down = "tree"
node = { id = "root", title = "", children = [{ id = "pages", title = "", present = "tabs", children = [{ id = "chat", title = "Chat", field = { kind = "chat", bind = "chat:main" } }] }] }
[[step]]
down = "value"
bind = "shape:on"
value = true
rev = 1
[[step]]
text = "fix it"
[[step]]
wait = 10
[[step]]
wait = 700
[[step]]
shot = "early"
[[step]]
wait = 200
"#;
    let shots = scene::run(&parse("clock", text).expect("a scene"));
    let drafts = |s: &Shot| s.sent.iter().filter(|u| matches!(u, j4n_proto::Up::Draft { text, .. } if text == "fix it")).count();
    assert_eq!((drafts(&shots[0]), drafts(&shots[1])), (0, 1), "the draft goes after the pause of the composer, not before");
}

#[test]
fn a_scene_that_is_wrong_says_which_step_and_why() {
    let cases = [
        ("[[step]]\nkey = \"hyper+x\"\n", "step 1: `hyper+x` is not a key"),
        ("[[step]]\ntext = \"a\"\n[[step]]\nwait = -5\n", "step 2: `wait` is milliseconds"),
        ("[[step]]\nkey = \"a\"\nshot = \"x\"\n", "step 1: a step has one of"),
        ("[[step]]\nfly = 1\n", "`fly` is not a step"),
        ("[[step]]\nresize = [0, 5]\n", "a size is [columns, rows]"),
        ("[[step]]\ndown = \"nonsense\"\n", "step 1: a message"),
        ("colour = 1\n", "`colour` is not part of a scene"),
        ("sizes = [[80]]\n", "a size is [columns, rows]"),
        ("step = 3\n", "`step` is a list of tables"),
        ("theme = { palette = \"neon\" }\n", "theme.palette is `neon`"),
        ("theme = { marks = \"emoji\" }\n", "theme.marks is `emoji`"),
        ("theme = { colour = 1 }\n", "`theme.colour` is not a theme setting"),
        ("theme = { motion = \"yes\" }\n", "`theme.motion` is true or false"),
        ("theme = { palette = 3 }\n", "`theme.palette` is a word"),
        ("theme = 3\n", "`theme` is a table"),
        ("[[step]]\npreview = \"colours\"\n", "`preview` is `roles`, `glyphs` or `off`"),
        ("[[step\n", "not TOML"),
    ];
    for (text, why) in cases {
        let got = parse("bad", text).expect_err(text);
        assert!(got.contains(why), "{text:?} gives {got:?}, which does not say {why:?}");
    }
}

#[test]
fn keys_are_written_as_a_terminal_names_them() {
    use j4n_tui::headless::{KeyCode, alt, ctrl, press};
    assert_eq!(scene::key("enter"), Some(press(KeyCode::Enter)));
    assert_eq!(scene::key("Ctrl+K"), Some(ctrl('k')));
    assert_eq!(scene::key("alt+a"), Some(alt('a')));
    assert_eq!(scene::key("f12"), Some(press(KeyCode::F(12))));
    assert_eq!(scene::key("f13"), None);
    assert_eq!(scene::key("shift+tab").map(|k| k.code), Some(KeyCode::BackTab));
    assert_eq!(scene::key("space"), Some(press(KeyCode::Char(' '))));
    assert_eq!(scene::key("+"), Some(press(KeyCode::Char('+'))));
    assert_eq!(scene::key("ab"), None);
    assert_eq!(scene::key(""), None);
}

#[test]
fn a_scene_has_the_look_it_names_and_a_page_of_the_theme_in_place_of_the_screen() {
    let text = "theme = { palette = \"paper\", marks = \"ascii\", no_color = true }\n[[step]]\npreview = \"glyphs\"\n[[step]]\nshot = \"g\"\n[[step]]\npreview = \"off\"\n";
    let scene = parse("look", text).expect("a scene");
    assert_eq!((scene.theme.palette.as_str(), scene.theme.marks.as_str(), scene.theme.no_color), ("paper", "ascii", true));
    let shots = scene::run(&scene);
    assert!(shots[0].snap.text.contains("glyphs · ascii") && !shots[0].snap.ansi.contains("38;2"), "{}", shots[0].snap.text);
    assert!(shots[1].snap.text.contains("waiting for the core"), "off brings the screen back: {}", shots[1].snap.text);
}
