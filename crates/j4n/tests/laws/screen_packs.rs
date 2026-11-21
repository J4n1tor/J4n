//! A pack adds a kind, a view, a page and a command with files only, and the screen — the real front and the
//! real TUI, wired as the binary wires them — shows them with no change to either. The pack is installed in the operator's folder;
//! it is loaded only while the operator's `enable` row pins the files it has, and a project cannot turn one on.

use crate::screen_drive::Operator;
use crate::run_cases::Dirs;
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use j4n_core::front::serve::Serve;
use j4n_core::pack::packs_dir;
use j4n_core::task::Mode;
use j4n_proto::{Down, Intent, Up};
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

const MANIFEST: &str = "api = 1\nid = \"widgets\"\nversion = \"0.2.0\"\nsummary = \"things on a shelf\"\nneeds = [\"base\"]\nstability = \"experimental\"\n";

const KIND: &str = r#"api = 1
[[kind]]
id = "widget"
doc = "A thing on the shelf."
examples = [{ id = "bolt", name = "bolt", size = 3 }]

[kind.fields]
name = { ty = "str", required = true, doc = "What it is called." }
size = { ty = "int", default = 1, doc = "How big." }
"#;

const SHELF: &str = "api = 1\n[[widget]]\nid = \"bolt\"\nname = \"bolt\"\nsize = 3\n[[widget]]\nid = \"nut\"\nname = \"nut\"\n";

const VIEWS: &str = r#"api = 1
[[view]]
id = "shelf"
doc = "The widgets."
from = { from = "table:widget", order = "name" }
cells = ["{name}", "size {size}"]
[[view]]
id = "mine"
doc = "The tasks of the thread."
from = { from = "tasks", order = "-id" }
cells = ["{id}", "{state}", "{goal}"]
"#;

const PAGES: &str = r#"api = 1
[[page]]
id = "shelf"
parent = "home"
title = "Shelf"
order = 90
field = "list"
view = "shelf"
[[page]]
id = "mine"
parent = "home"
group = "shelf"
title = "Mine"
order = 91
field = "list"
view = "mine"
"#;

const COMMANDS: &str = "api = 1\n[[command]]\nid = \"shelf\"\ndoc = \"Show the shelf.\"\nopen = \"shelf\"\n[[command]]\nid = \"mine\"\nopen = \"mine\"\n";

fn put(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}

pub fn install(d: &Dirs) {
    let pack = packs_dir(&d.j4n_home());
    put(&pack, "widgets/pack.toml", MANIFEST);
    for (rel, text) in [("kinds/widget.toml", KIND), ("widgets/shelf.toml", SHELF), ("views/v.toml", VIEWS), ("pages/p.toml", PAGES), ("commands/c.toml", COMMANDS)] {
        put(&pack, &format!("widgets/{rel}"), text);
    }
}

/// The hash the operator accepts when turning the installed pack on.
pub fn pin(d: &Dirs) -> String {
    let (catalog, found) = j4n::pack::catalog().with_installed(&packs_dir(&d.j4n_home()));
    assert!(found.is_empty(), "{found:?}");
    catalog.get("widgets").unwrap().pin()
}

fn turn_on(at: &Path, hash: &str) {
    put(at, "harness.toml", &format!("api = 1\n[[enable]]\nid = \"widgets\"\nhash = \"{hash}\"\n"));
}

fn opts() -> Opts {
    Opts { doer: Some("test".into()), model: None, approval: Some(Approval::Deny), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false }
}

fn open(d: &Dirs) -> World {
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: "claude".into() };
    crate::support::opened(|| wire::open(setup.clone(), &opts(), true, "main"))
}

#[test]
fn a_pinned_pack_adds_a_page_a_view_a_kind_and_a_command_and_the_screen_shows_them_with_no_code_change() {
    let d = Dirs::new("pack-on");
    install(&d);
    turn_on(&d.j4n_home(), &pin(&d));
    let world = open(&d);
    assert!(world.notices.is_empty(), "{:?}", world.notices);
    let mut op = Operator::sit(&world);
    // the page is a tab of its own, the other is a sub-tab of it (the top row holds nine tabs), and the pack's own words open them
    let s = op.until("the new tabs", |s| s.lines().nth(1).is_some_and(|l| l.contains("Shelf")));
    assert!(s.lines().nth(1).unwrap().contains("Settings"), "{s}");
    let s = op.go(":shelf", "the shelf", |s| s.contains("bolt") && s.contains("nut"));
    assert!(s.contains("size 3") && s.contains("size 1"), "the rows are the entries of the pack's kind, with its default: {s}");
    assert!(s.lines().nth(3).is_some_and(|l| l.contains("Shelf") && l.contains("Mine")), "the page that joined the tab is a sub-tab of it, after the one the tab drew: {s}");
    // the rows of a view over the tasks follow the tasks
    op.press(vec![j4n_tui::headless::press(j4n_tui::headless::KeyCode::F(1))]);
    op.type_line("say it is done");
    op.until("the answer", |s| s.contains("it is done"));
    let s = op.go(":mine", "the task in the view", |s| s.contains("t1") && s.contains("say it is done"));
    assert!(s.contains("done"), "{s}");
    // the pack is listed with what it adds, and is marked as not settled
    op.press(vec![j4n_tui::headless::press(j4n_tui::headless::KeyCode::F(1))]);
    op.type_line(":pack");
    let s = op.until("the packs", |s| s.contains("widgets 0.2.0"));
    assert!(s.contains("1 kind") && s.contains("2 widgets"), "{s}");
    assert!(s.contains("[experimental]"), "{s}");
    world.close();
}

#[test]
fn a_view_over_the_tasks_sends_its_rows_again_when_they_change_and_not_when_nothing_did() {
    let d = Dirs::new("pack-views");
    install(&d);
    turn_on(&d.j4n_home(), &pin(&d));
    let world = open(&d);
    let mut serve = Serve::new(world.front.clone());
    let rows_of = |downs: &[Down]| downs.iter().filter_map(|x| if let Down::Rows { bind, rows, .. } = x { (bind == "view:mine").then_some(rows.len()) } else { None }).collect::<Vec<_>>();
    let (downs, _) = serve.up(Up::Watch { bind: "view:mine".into() });
    assert_eq!(rows_of(&downs), [0]);
    let (_, _) = serve.up(Up::Intent { intent: Intent::Say { text: "say hi".into(), with: Vec::new() } });
    let end = Instant::now() + crate::support::PATIENCE;
    let mut sent = Vec::new();
    let mut from = 0;
    while Instant::now() < end && !sent.contains(&1) {
        let events: Vec<_> = world.front.log().read(from).collect();
        from = events.last().map_or(from, |e| e.seq + 1);
        for e in events {
            sent.extend(rows_of(&serve.ev(&e)));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(sent.contains(&1), "the view got its row: {sent:?}");
    let again: Vec<_> = world.front.log().read(0).flat_map(|e| rows_of(&serve.ev(&e))).collect();
    assert!(again.len() <= 1, "events that change nothing in the view send it once at most: {again:?}");
    world.close();
}

#[test]
fn a_pack_that_is_installed_but_not_pinned_or_changed_since_is_left_out_and_the_screen_is_the_base_one() {
    for (tag, how) in [("nopin", 0), ("stale", 1), ("project", 2)] {
        let d = Dirs::new(&format!("pack-{tag}"));
        install(&d);
        match how {
            0 => put(&d.j4n_home(), "harness.toml", "api = 1\n[[enable]]\nid = \"widgets\"\n"),
            1 => {
                turn_on(&d.j4n_home(), &pin(&d));
                put(&packs_dir(&d.j4n_home()), "widgets/pages/p.toml", &format!("{PAGES}\n[[page]]\nid = \"sneak\"\nparent = \"home\"\ntitle = \"Sneak\"\nfield = \"log\"\nbind = \"log:all\"\n"));
            }
            // a project that was cloned from a stranger says which packs it wants: nobody asked it
            _ => {
                put(&d.project(), ".j4n/harness.toml", &format!("api = 1\n[[enable]]\nid = \"widgets\"\nhash = \"{}\"\n", pin(&d)));
            }
        }
        let world = open(&d);
        if how != 2 {
            assert!(world.notices.iter().any(|n| n.contains("pack widgets is left out")), "{tag}: {:?}", world.notices);
        }
        let mut op = Operator::sit(&world);
        let s = op.look();
        assert!(!s.lines().nth(1).unwrap().contains("Shelf") && !s.lines().nth(1).unwrap().contains("Sneak"), "{tag}: {s}");
        op.type_line(":shelf");
        std::thread::sleep(Duration::from_millis(100));
        op.type_line(":pack");
        let s = op.until("the packs", |s| s.contains("widgets 0.2.0"));
        assert!(s.contains("widgets 0.2.0") && !s.contains("widgets 0.2.0  on"), "{tag}: it is listed, and not on: {s}");
        world.close();
    }
}

#[test]
fn a_pack_may_not_take_the_id_of_a_built_in_one_and_the_built_in_packs_are_listed_whether_on_or_not() {
    let d = Dirs::new("pack-ids");
    put(&packs_dir(&d.j4n_home()), "code/pack.toml", &MANIFEST.replace("widgets", "code"));
    let world = open(&d);
    assert!(world.notices.iter().any(|n| n.contains("built-in pack's id")), "{:?}", world.notices);
    let mut op = Operator::sit(&world);
    op.type_line(":pack");
    let s = op.until("the packs", |s| s.contains("connect 0.0.0"));
    // the screen holds as many rows as it has, the longest summaries wrap and push the first rows off it: the whole list is what the command prints
    world.close();
    drop(op);
    drop(world);
    let o = crate::run_cases::j4n(&d, &["pack"], None, &[]);
    let listed = format!("{}{}", o.out, o.err);
    for id in ["agent", "bench", "code", "connect", "grow", "test"] {
        assert!(listed.lines().any(|l| l.starts_with(&format!("{id} "))), "{id}: {listed}");
    }
    assert!(s.contains("connect"), "{s}");
}

#[test]
fn a_command_of_a_pack_is_a_word_on_the_command_line_and_a_word_nobody_has_is_refused() {
    use crate::run_cases::j4n;
    let d = Dirs::new("pack-cli");
    install(&d);
    turn_on(&d.j4n_home(), &pin(&d));
    let o = j4n(&d, &["pack"], None, &[]);
    assert_eq!((o.rc, o.out.contains("widgets 0.2.0")), (0, true), "{}\n{}", o.out, o.err);
    let o = j4n(&d, &["--doer", "test", ":shelf"], None, &[]);
    assert_eq!(o.rc, 0, "a word may come with its lead: {}", o.err);
    let o = j4n(&d, &["dance"], None, &[]);
    assert_eq!(o.rc, 2);
    assert!(o.err.contains("`dance` is not a command"), "{}", o.err);
    // a pack that is not on gives no word
    let off = Dirs::new("pack-cli-off");
    install(&off);
    let o = j4n(&off, &["shelf"], None, &[]);
    assert_eq!(o.rc, 2, "{}", o.err);
}
