//! Table writes: plan a [`Write`] into a [`Change`], apply it to the files with the writer, undo it, and the same in the
//! session layer. The files are real (a scratch directory) and so is the lock.

use crate::world::*;
use j4n_core::change::{Actor, Cause, Change, Where};
use j4n_core::table::{Layer, LayerKind, Level, Op, Planned, Problem, Tables, Write};
use j4n_core::value::{Key, Value};
use j4n_core::write::{Applied, Journal, NoJournal, Writer, confirm, edited};
use std::cell::RefCell;
use std::fs;
use std::os::unix::fs::PermissionsExt;

const KINDS: &str = r#"
api = 1
[[kind]]
id = "thing"
doc = "a thing"
untrusted = "tighten"
[kind.fields]
name = { ty = "str", required = true }
size = { ty = "int", min = 1, max = 9, default = 3 }
mode = { ty = "enum:strict|loose", default = "strict", widens = ["loose"] }
locked = { ty = "int", floor = true }
shared = { ty = "str", who_may = ["operator", "rule"] }
tags = "list<str>"
meta = "any"
every = "dur"

[[kind]]
id = "note"
doc = "a note"
[kind.fields]
text = { ty = "text", required = true }

[[thing]]
id = "base"
name = "from the pack"
locked = 1
"#;

const CREW: &str = r#"api = 1

# the old crew
[[thing]]
id = "a"
name = "Ada"     # first
size = 4         # since the storm
tags = ["x", "y"]

# second
[[thing]]
id = "b"
name = "Bo"

[[note]]
id = "n1"
text = "hello"
"#;

struct Rig {
    home: Scratch,
    project: Scratch,
}

impl Rig {
    fn new(crew: Option<&str>) -> Rig {
        let rig = Rig { home: Scratch::new("home"), project: Scratch::new("project") };
        if let Some(text) = crew {
            rig.home.put("crew.toml", text);
        }
        rig
    }

    fn load(&self) -> (Tables, Vec<Problem>) {
        let (home, mut problems) = Layer::from_dir(LayerKind::User, "home", self.home.path());
        let (project, more) = Layer::from_dir(LayerKind::Project, "project", self.project.path());
        problems.extend(more);
        let (t, loaded) = Tables::load(vec![pack("p", KINDS), home, project]);
        problems.extend(loaded);
        (t, problems)
    }

    fn tables(&self) -> Tables {
        let (t, problems) = self.load();
        assert!(refused(&problems).is_empty(), "{problems:?}");
        t
    }

    fn writer(&self) -> Writer {
        Writer::new(Some(self.home.path().to_path_buf()), Some(self.project.path().to_path_buf()))
    }

    /// Plan and apply, as the operator.
    fn run(&self, w: &Write) -> Result<(Planned, Applied), Problem> {
        let t = self.tables();
        let planned = t.plan(w)?;
        let applied = self.writer().apply(&t, &planned.change, planned.file_hash.as_deref(), &mut NoJournal)?;
        Ok((planned, applied))
    }
}

fn write(to: Where, kind: &str, id: &str, op: Op) -> Write {
    Write {
        to,
        file: None,
        kind: kind.to_string(),
        id: id.to_string(),
        op,
        by: Actor::Operator,
        cause: None,
        at_ms: 1_000,
    }
}

fn set(body: &str) -> Op {
    Op::Set(body.parse().unwrap())
}

fn home_write(kind: &str, id: &str, op: Op) -> Write {
    write(Where::User, kind, id, op)
}

fn text_of(rig: &Rig) -> String {
    rig.home.read("crew.toml")
}

#[test]
fn a_set_on_a_new_entry_makes_the_file_with_its_api_line() {
    let rig = Rig::new(None);
    let (planned, applied) = rig.run(&home_write("thing", "a", set("name = \"Ada\"\nsize = 5"))).unwrap();
    assert_eq!(planned.change.target.file, "thing.toml");
    assert_eq!(planned.change.before, None);
    assert_eq!(planned.change.after.as_ref().unwrap()["size"].as_integer(), Some(5));
    assert_eq!(planned.change.id.len(), 16);
    assert!(planned.widens.is_empty() && planned.file_hash.is_none());
    assert_eq!(rig.home.read("thing.toml"), "api = 1\n\n[[thing]]\nid = \"a\"\nname = \"Ada\"\nsize = 5\n");
    assert_eq!(applied.path, rig.home.join("thing.toml"));
    let t = rig.tables();
    assert_eq!(t.raw("thing", "a").unwrap().from["size"], "home");
    assert!(rig.home.exists(".table.lock"), "the lock is a file next to what it guards");
    assert!(
        !fs::read_dir(rig.home.path()).unwrap().any(|e| e.unwrap().file_name().to_string_lossy().ends_with(".tmp"))
    );
}

#[test]
fn a_new_entry_is_appended_and_every_comment_and_blank_line_stays() {
    let rig = Rig::new(Some(CREW));
    let mut w = home_write("thing", "c", set("name = \"Cy\"\ntags = [\"z\"]"));
    w.file = Some("crew.toml".into());
    rig.run(&w).unwrap();
    let got = text_of(&rig);
    assert!(got.starts_with("api = 1\n\n# the old crew\n[[thing]]\nid = \"a\"\nname = \"Ada\"     # first\nsize = 4         # since the storm\n"), "{got}");
    assert!(got.contains("# second\n[[thing]]\nid = \"b\"\nname = \"Bo\"\n"), "{got}");
    assert!(got.contains("[[thing]]\nid = \"c\"\nname = \"Cy\"\ntags = [\"z\"]\n"), "{got}");
    assert!(got.contains("[[note]]\nid = \"n1\"\ntext = \"hello\"\n"), "{got}");
    let t = rig.tables();
    assert_eq!(t.ids("thing"), ["a", "b", "base", "c"]);
}

#[test]
fn changing_a_field_changes_that_field_and_keeps_the_comment_on_its_line() {
    let rig = Rig::new(Some(CREW));
    let (planned, _) = rig.run(&home_write("thing", "a", set("size = 7"))).unwrap();
    assert_eq!(planned.change.before.as_ref().unwrap()["size"].as_integer(), Some(4));
    assert_eq!(text_of(&rig), CREW.replace("size = 4         # since the storm", "size = 7         # since the storm"));
}

#[test]
fn adding_a_field_and_taking_one_away_touch_only_those_lines() {
    let rig = Rig::new(Some(CREW));
    rig.run(&home_write("thing", "b", set("size = 2"))).unwrap();
    assert!(text_of(&rig).contains("[[thing]]\nid = \"b\"\nname = \"Bo\"\nsize = 2\n"), "{}", text_of(&rig));
    rig.run(&home_write("thing", "a", Op::Unset(vec!["tags".into()]))).unwrap();
    assert_eq!(
        text_of(&rig),
        CREW.replace("tags = [\"x\", \"y\"]\n", "").replace("name = \"Bo\"\n", "name = \"Bo\"\nsize = 2\n")
    );
    let t = rig.tables();
    assert_eq!(t.raw("thing", "a").unwrap().from.get("tags"), None, "unset: a lower layer, or nothing, shows through");
}

#[test]
fn a_table_or_a_list_that_is_changed_is_written_inline_and_reads_back_the_same() {
    let rig = Rig::new(Some(CREW));
    rig.run(&home_write(
        "thing",
        "a",
        set("meta = { depth = { a = [1, 2.5, \"x\"], ok = true }, empty = [] }\ntags = [\"q\"]"),
    ))
    .unwrap();
    let t = rig.tables();
    let a = t.raw("thing", "a").unwrap();
    assert_eq!(a.fields["tags"].as_array().unwrap().len(), 1);
    assert_eq!(a.fields["meta"]["depth"]["a"][1].as_float(), Some(2.5));
    assert!(
        text_of(&rig).contains("meta = { depth = { a = [1, 2.5, \"x\"], ok = true }, empty = [] }"),
        "{}",
        text_of(&rig)
    );
}

#[test]
fn removing_an_entry_removes_only_it_and_the_last_of_a_kind_takes_its_array_with_it() {
    let rig = Rig::new(Some(CREW));
    rig.run(&home_write("thing", "a", Op::Remove)).unwrap();
    assert!(
        !text_of(&rig).contains("id = \"a\"") && text_of(&rig).contains("# second\n[[thing]]\nid = \"b\""),
        "{}",
        text_of(&rig)
    );
    rig.run(&home_write("thing", "b", Op::Remove)).unwrap();
    rig.run(&home_write("note", "n1", Op::Remove)).unwrap();
    assert_eq!(rig.tables().ids("thing"), ["base"]);
    assert!(!text_of(&rig).contains("[["), "{}", text_of(&rig));
    assert!(text_of(&rig).starts_with("api = 1"));
}

#[test]
fn an_entry_that_is_asked_for_as_it_already_is_changes_nothing() {
    let rig = Rig::new(Some(CREW));
    let err = rig.run(&home_write("thing", "a", set("size = 4"))).unwrap_err();
    assert_eq!(err.level, Level::Warn);
    assert!(err.msg.contains("already so"));
    assert_eq!(text_of(&rig), CREW);
}

fn refusals() -> Vec<(Write, &'static str)> {
    let model = |mut w: Write| {
        w.by = Actor::Model("m".into());
        w
    };
    let file = |name: &str| Write { file: Some(name.to_string()), ..home_write("thing", "new", set("name = \"x\"")) };
    vec![
        (home_write("nope", "a", set("x = 1")), "no kind `nope` is declared"),
        (home_write("thing", "a", set("bogus = 1")), "bogus: not a field of this"),
        (home_write("thing", "a", set("size = 99")), "size: 99 is outside 1..9"),
        (home_write("thing", "bad id", set("name = \"x\"")), "is not an id"),
        (home_write("thing", "ghost", Op::Remove), "the layer has no such entry"),
        (home_write("thing", "ghost", Op::Unset(vec!["size".into()])), "the layer has no such entry"),
        (home_write("thing", "a", Op::Unset(vec!["meta".into()])), "`meta` is not set in this layer"),
        (home_write("thing", "a", Op::Unset(vec!["name".into()])), "name: required"),
        (home_write("thing", "new", set("size = 2")), "name: required"),
        (home_write("thing", "base", set("locked = 2")), "`locked` is a floor that a lower layer already set"),
        (
            model(home_write("thing", "a", set("size = 2"))),
            "`size` may not be written by model (it is for operator, core)",
        ),
        (
            model(home_write("thing", "a", set("shared = \"x\""))),
            "`shared` may not be written by model (it is for operator, rule)",
        ),
        (file("../out.toml"), "is not a file name in this layer"),
        (file("/abs.toml"), "is not a file name in this layer"),
        (file("pack.toml"), "is not a file name in this layer"),
        (file("log/x.toml"), "is not a file name in this layer"),
        (file("sub/log/x.toml"), "is not a file name in this layer"),
        (file("sub/bin/x.toml"), "is not a file name in this layer"),
        (file(".hidden.toml"), "is not a file name in this layer"),
        (file("notes.txt"), "is not a file name in this layer"),
        (file("a//b.toml"), "is not a file name in this layer"),
        (
            write(Where::Session, "kind", "mine", set("doc = \"d\"\nfields = {}")),
            "a kind is declared in a file, not in the session",
        ),
    ]
}

#[test]
fn a_layer_that_is_not_there_cannot_be_written() {
    let (t, _) = load(vec![pack("p", KINDS)]);
    for to in [Where::User, Where::Project] {
        let err = t.plan(&write(to, "thing", "a", set("name = \"x\""))).unwrap_err();
        assert!(err.msg.contains("layer"), "{err}");
    }
    assert!(t.plan(&write(Where::Session, "thing", "a", set("name = \"x\""))).is_ok());
}

#[test]
fn a_write_that_would_not_load_is_refused_before_it_is_made() {
    let rig = Rig::new(Some(CREW));
    let t = rig.tables();
    for (w, want) in refusals() {
        let err = t.plan(&w).expect_err(want);
        assert!(err.msg.contains(want), "{w:?}: wanted `{want}`, got `{err}`");
        assert_eq!(err.level, Level::Refused);
    }
    assert_eq!(text_of(&rig), CREW);
}

#[test]
fn who_may_lets_a_rule_write_what_a_schema_gives_it_and_a_model_only_by_proposal() {
    let rig = Rig::new(Some(CREW));
    let t = rig.tables();
    let mut w = home_write("thing", "a", set("shared = \"from a rule\""));
    w.by = Actor::Rule("r1".into());
    w.cause = Some(Cause::Event(7));
    let planned = t.plan(&w).unwrap();
    assert_eq!(
        (planned.change.by.clone(), planned.change.cause.clone()),
        (Actor::Rule("r1".into()), Some(Cause::Event(7)))
    );
    // the operator accepting a model's proposal is the operator's write, with the proposal as its cause
    let mut accepted = home_write("thing", "a", set("size = 2"));
    accepted.cause = Some(Cause::Proposal("p9".into()));
    assert!(t.plan(&accepted).is_ok());
}

#[test]
fn a_write_says_which_fields_widen_what_j4n_may_do() {
    let rig = Rig::new(Some(CREW));
    let t = rig.tables();
    assert_eq!(t.plan(&home_write("thing", "a", set("mode = \"loose\""))).unwrap().widens, ["mode"]);
    assert!(t.plan(&home_write("thing", "a", set("mode = \"strict\""))).unwrap().widens.is_empty(), "strict is safe");
}

#[test]
fn a_field_that_already_widens_is_not_a_widening_again() {
    let rig = Rig::new(Some(CREW));
    rig.run(&home_write("thing", "a", set("mode = \"loose\""))).unwrap();
    let t = rig.tables();
    let again = t.plan(&home_write("thing", "a", set("size = 6"))).unwrap();
    assert!(again.widens.is_empty(), "{:?}", again.widens);
    assert_eq!(t.plan(&home_write("thing", "a", set("mode = \"strict\""))).unwrap().widens, Vec::<String>::new());
    assert_eq!(t.plan(&home_write("thing", "b", set("mode = \"loose\""))).unwrap().widens, ["mode"]);
}

#[test]
fn a_stranger_project_may_be_written_to_only_as_far_as_it_may_count() {
    let rig = Rig::new(None);
    let (t, _) = rig.load();
    let w = |body: &str| write(Where::Project, "thing", "p", set(body));
    assert!(t.plan(&w("name = \"x\"\nsize = 2")).is_ok(), "a tightening kind takes what does not widen");
    let err = t.plan(&w("name = \"x\"\nmode = \"loose\"")).unwrap_err();
    assert!(err.msg.contains("this project is not trusted, and mode would widen"), "{err}");
    let err = t.plan(&write(Where::Project, "note", "n", set("text = \"x\""))).unwrap_err();
    assert!(err.msg.contains("not trusted"), "{err}");
}

struct Tape(RefCell<Vec<String>>, Option<&'static str>);

impl Journal for &Tape {
    fn record(&mut self, c: &Change) -> Result<(), String> {
        self.0.borrow_mut().push(format!("record {}", c.id));
        self.1.map_or(Ok(()), |e| Err(e.to_string()))
    }

    fn failed(&mut self, c: &Change, why: &str) {
        self.0.borrow_mut().push(format!("failed {} {why}", c.id));
    }
}

#[test]
fn the_change_is_recorded_before_the_file_is_touched_and_a_refusal_stops_it() {
    let rig = Rig::new(Some(CREW));
    let t = rig.tables();
    let planned = t.plan(&home_write("thing", "a", set("size = 5"))).unwrap();
    let tape = Tape(RefCell::new(vec![]), Some("the log is full"));
    let err = rig.writer().apply(&t, &planned.change, None, &mut &tape).unwrap_err();
    assert!(err.msg.contains("not recorded: the log is full"), "{err}");
    assert_eq!(text_of(&rig), CREW, "a change that cannot be recorded is not made");
    assert_eq!(tape.0.borrow().len(), 1);

    let tape = Tape(RefCell::new(vec![]), None);
    rig.writer().apply(&t, &planned.change, None, &mut &tape).unwrap();
    assert_eq!(*tape.0.borrow(), [format!("record {}", planned.change.id)]);
    assert!(text_of(&rig).contains("size = 5"));
}

#[test]
fn a_write_that_fails_after_the_record_says_so_and_leaves_the_file_whole() {
    let rig = Rig::new(Some(CREW));
    let t = rig.tables();
    rig.run(&home_write("thing", "b", set("size = 2"))).unwrap(); // the lock file exists now
    let before = text_of(&rig);
    fs::set_permissions(rig.home.path(), fs::Permissions::from_mode(0o555)).unwrap();
    let writable = fs::write(rig.home.join("probe"), "x").is_ok(); // root ignores the mode
    let t = if writable { t } else { rig.tables() };
    let planned = t.plan(&home_write("thing", "b", set("size = 6"))).unwrap();
    let tape = Tape(RefCell::new(vec![]), None);
    let got = rig.writer().apply(&t, &planned.change, None, &mut &tape);
    fs::set_permissions(rig.home.path(), fs::Permissions::from_mode(0o755)).unwrap();
    if !writable {
        let err = got.unwrap_err();
        assert!(err.msg.contains("cannot write crew.toml"), "{err}");
        assert!(tape.0.borrow()[1].starts_with("failed "), "{:?}", tape.0.borrow());
        assert_eq!(text_of(&rig), before);
        assert!(
            !fs::read_dir(rig.home.path()).unwrap().any(|e| e.unwrap().file_name().to_string_lossy().ends_with(".tmp"))
        );
    }
}

#[test]
fn a_file_changed_since_it_was_loaded_is_not_written_over() {
    let rig = Rig::new(Some(CREW));
    let t = rig.tables();
    let planned = t.plan(&home_write("thing", "a", set("size = 5"))).unwrap();
    assert_eq!(planned.file_hash.as_deref().map(str::len), Some(64));
    rig.home.put("crew.toml", &format!("{CREW}\n# someone was here\n"));
    let err = rig.writer().apply(&t, &planned.change, planned.file_hash.as_deref(), &mut NoJournal).unwrap_err();
    assert!(err.msg.contains("crew.toml changed since it was loaded"), "{err}");
    assert!(text_of(&rig).ends_with("# someone was here\n") && text_of(&rig).contains("size = 4"));
    // without the file check the entry check still holds: the entry is what it was, so the write goes through
    rig.writer().apply(&t, &planned.change, None, &mut NoJournal).unwrap();
    assert!(text_of(&rig).contains("size = 5") && text_of(&rig).ends_with("# someone was here\n"));
}

#[test]
fn an_entry_changed_since_it_was_planned_is_not_written_over() {
    let rig = Rig::new(Some(CREW));
    let t = rig.tables();
    let planned = t.plan(&home_write("thing", "a", set("size = 5"))).unwrap();
    rig.run(&home_write("thing", "a", set("size = 8"))).unwrap();
    let err = rig.writer().apply(&t, &planned.change, None, &mut NoJournal).unwrap_err();
    assert!(err.msg.contains("it changed since this was planned"), "{err}");
    assert!(text_of(&rig).contains("size = 8"));
    // a new entry that another writer made first
    let planned = t.plan(&home_write("thing", "d", set("name = \"D\""))).unwrap();
    rig.run(&home_write("thing", "d", set("name = \"other\""))).unwrap();
    let err = rig.writer().apply(&t, &planned.change, None, &mut NoJournal).unwrap_err();
    assert!(err.msg.contains("changed since"), "{err}");
}

#[test]
fn a_file_that_cannot_be_trusted_is_never_overwritten() {
    for (text, want) in [
        ("[unclosed\n", "is not valid TOML"),
        ("# no api here\n[[thing]]\nid = \"a\"\nname = \"x\"\n", "has no `api = 1`"),
        ("api = 2\n[[thing]]\nid = \"a\"\nname = \"x\"\n", "has no `api = 1`"),
    ] {
        let rig = Rig::new(Some(CREW));
        let t = rig.tables();
        let planned = t.plan(&home_write("thing", "a", set("size = 5"))).unwrap();
        rig.home.put("crew.toml", text);
        let err = rig.writer().apply(&t, &planned.change, None, &mut NoJournal).unwrap_err();
        assert!(err.msg.contains(want), "{text:?}: {err}");
        assert_eq!(text_of(&rig), text);
    }
}

#[test]
fn an_entry_written_as_an_inline_array_is_left_to_the_hand_that_wrote_it() {
    let inline = "api = 1\nthing = [{ id = \"a\", name = \"Ada\" }]\n";
    let rig = Rig::new(Some(inline));
    let t = rig.tables();
    assert_eq!(t.raw("thing", "a").unwrap().fields["name"].as_str(), Some("Ada"), "it reads fine");
    let into_crew = Write { file: Some("crew.toml".into()), ..home_write("thing", "z", set("name = \"Z\"")) };
    for w in [home_write("thing", "a", set("size = 5")), into_crew] {
        let planned = t.plan(&w).unwrap();
        let err = rig.writer().apply(&t, &planned.change, None, &mut NoJournal).unwrap_err();
        assert!(err.msg.contains("written as an inline array"), "{err}");
    }
    assert_eq!(text_of(&rig), inline);
}

#[test]
fn a_change_that_was_not_planned_is_vetted_like_one_that_was() {
    let rig = Rig::new(Some(CREW));
    let t = rig.tables();
    let mut change = t.plan(&home_write("thing", "a", set("size = 5"))).unwrap().change;
    change.after.as_mut().unwrap().insert("size".into(), toml::Value::String("big".into()));
    let err = rig.writer().apply(&t, &change, None, &mut NoJournal).unwrap_err();
    assert!(err.msg.contains("size: expected a whole number"), "{err}");
    let mut change = t.plan(&home_write("thing", "a", set("size = 5"))).unwrap().change;
    change.target.file = "../escape.toml".into();
    assert!(rig.writer().apply(&t, &change, None, &mut NoJournal).is_err());
    assert!(!rig.home.path().parent().unwrap().join("escape.toml").exists());
    let mut change = t.plan(&home_write("thing", "a", set("size = 5"))).unwrap().change;
    change.by = Actor::Model("m".into());
    assert!(
        rig.writer().apply(&t, &change, None, &mut NoJournal).unwrap_err().msg.contains("may not be written by model")
    );
    assert_eq!(text_of(&rig), CREW);
}

#[test]
fn the_session_layer_is_not_a_file() {
    let rig = Rig::new(Some(CREW));
    let t = rig.tables();
    let planned = t.plan(&write(Where::Session, "thing", "a", set("size = 6"))).unwrap();
    let err = rig.writer().apply(&t, &planned.change, None, &mut NoJournal).unwrap_err();
    assert!(err.msg.contains("session layer is not written to files"), "{err}");
}

#[test]
fn a_file_keeps_its_permissions_when_it_is_replaced() {
    let rig = Rig::new(Some(CREW));
    fs::set_permissions(rig.home.join("crew.toml"), fs::Permissions::from_mode(0o600)).unwrap();
    rig.run(&home_write("thing", "a", set("size = 5"))).unwrap();
    assert_eq!(fs::metadata(rig.home.join("crew.toml")).unwrap().permissions().mode() & 0o777, 0o600);
}

#[test]
fn an_undo_is_the_inverse_change_and_it_works_across_a_restart() {
    let rig = Rig::new(Some(CREW));
    let (first, _) = rig.run(&home_write("thing", "a", set("size = 7"))).unwrap();
    let (second, _) = rig.run(&home_write("thing", "c", set("name = \"Cy\""))).unwrap();
    let (third, _) = rig.run(&home_write("thing", "b", Op::Remove)).unwrap();
    // "restart": the changes come back from the Log as JSON, the Tables are read again from the files
    let log: Vec<String> =
        [&first, &second, &third].iter().map(|p| serde_json::to_string(&p.change).unwrap()).collect();
    let changes: Vec<Change> = log.iter().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(changes[0], first.change);
    for (n, c) in changes.iter().enumerate().rev() {
        let t = rig.tables();
        let undo = c.inverse(Actor::Operator, 2_000 + n as u64);
        assert_eq!(undo.cause, Some(Cause::Undo(c.id.clone())));
        assert_eq!((undo.before.as_ref(), undo.after.as_ref()), (c.after.as_ref(), c.before.as_ref()));
        t.vet(&undo).unwrap();
        rig.writer().apply(&t, &undo, None, &mut NoJournal).unwrap();
    }
    let t = rig.tables();
    assert_eq!(t.ids("thing"), ["a", "b", "base"]);
    assert_eq!(t.raw("thing", "a").unwrap().fields["size"].as_integer(), Some(4));
    assert_eq!(t.raw("thing", "b").unwrap().fields["name"].as_str(), Some("Bo"));
    assert!(
        text_of(&rig).contains("size = 4         # since the storm"),
        "the comment is still on its line:\n{}",
        text_of(&rig)
    );
}

#[test]
fn an_undo_of_something_changed_since_is_refused_not_forced() {
    let rig = Rig::new(Some(CREW));
    let (first, _) = rig.run(&home_write("thing", "a", set("size = 7"))).unwrap();
    rig.run(&home_write("thing", "a", set("size = 8"))).unwrap();
    let t = rig.tables();
    let undo = first.change.inverse(Actor::Operator, 3);
    let err = rig.writer().apply(&t, &undo, None, &mut NoJournal).unwrap_err();
    assert!(err.msg.contains("it changed since this was planned"), "{err}");
    assert!(text_of(&rig).contains("size = 8"));
}

#[test]
fn the_same_edit_made_twice_is_two_changes() {
    let rig = Rig::new(Some(CREW));
    let t = rig.tables();
    let mut a = home_write("thing", "a", set("size = 5"));
    let one = t.plan(&a).unwrap().change;
    assert_eq!(one.id, t.plan(&a).unwrap().change.id, "the id is made of the change, so a plan is repeatable");
    a.at_ms += 1;
    assert_ne!(one.id, t.plan(&a).unwrap().change.id);
}

#[test]
fn the_session_layer_is_written_in_memory_with_the_same_checks_and_undo() {
    let rig = Rig::new(Some(CREW));
    let mut t = rig.tables();
    let planned = t.plan(&write(Where::Session, "thing", "a", set("size = 9"))).unwrap();
    assert_eq!(planned.change.target.file, "");
    assert!(planned.file_hash.is_none());
    t.apply_session(&planned.change).unwrap();
    let a = t.raw("thing", "a").unwrap();
    assert_eq!(
        (a.fields["size"].as_integer(), a.from["size"].as_str(), a.from["name"].as_str()),
        (Some(9), "session", "home")
    );
    assert_eq!(text_of(&rig), CREW, "nothing on disk");
    // a session-only entry
    let other = t.plan(&write(Where::Session, "thing", "tmp", set("name = \"temp\""))).unwrap();
    t.apply_session(&other.change).unwrap();
    assert_eq!(t.ids("thing"), ["a", "b", "base", "tmp"]);
    // checks and CAS are the same as for a file
    assert!(t.plan(&write(Where::Session, "thing", "a", set("size = 99"))).is_err());
    assert!(t.apply_session(&planned.change).unwrap_err().msg.contains("changed since"));
    let mut hand_made = other.change.clone();
    hand_made.target.layer = Where::User;
    assert!(t.apply_session(&hand_made).unwrap_err().msg.contains("only a session change"));
    // undo, and removing
    t.apply_session(&planned.change.inverse(Actor::Operator, 5)).unwrap();
    assert_eq!(t.raw("thing", "a").unwrap().fields["size"].as_integer(), Some(4));
    t.apply_session(&other.change.inverse(Actor::Operator, 6)).unwrap();
    assert_eq!(t.ids("thing"), ["a", "b", "base"]);
    let gone = t.plan(&write(Where::Session, "thing", "a", set("size = 2"))).unwrap();
    t.apply_session(&gone.change).unwrap();
    let rm = t.plan(&write(Where::Session, "thing", "a", Op::Remove)).unwrap();
    t.apply_session(&rm.change).unwrap();
    assert_eq!(
        t.raw("thing", "a").unwrap().fields["size"].as_integer(),
        Some(4),
        "the layers below show through again"
    );
}

#[test]
fn a_session_write_that_leaves_an_entry_without_a_required_field_is_refused() {
    let rig = Rig::new(Some(CREW));
    let t = rig.tables();
    let err = t.plan(&write(Where::Session, "thing", "only-here", set("size = 2"))).unwrap_err();
    assert!(err.msg.contains("name: required"), "{err}");
}

#[test]
fn rows_follow_a_session_write() {
    let rig = Rig::new(Some(CREW));
    let mut t = rig.tables();
    let planned = t.plan(&write(Where::Session, "thing", "a", set("every = \"90s\""))).unwrap();
    t.apply_session(&planned.change).unwrap();
    let rows = t.rows("thing");
    let a = rows.iter().find(|r| r.at(&[Key::Name("id".into())]) == Value::Str("a".into())).unwrap();
    assert_eq!(a.at(&[Key::Name("every".into())]), Value::Dur(90_000));
}

#[test]
fn writers_in_parallel_do_not_lose_each_others_entries() {
    let rig = Rig::new(Some(CREW));
    let t = rig.tables();
    let writer = rig.writer();
    let planned: Vec<_> = (0..8)
        .map(|n| {
            let w = Write {
                file: Some("crew.toml".into()),
                ..home_write("thing", &format!("w{n}"), set(&format!("name = \"W{n}\"")))
            };
            t.plan(&w).unwrap()
        })
        .collect();
    std::thread::scope(|s| {
        for p in &planned {
            let (t, writer) = (&t, &writer);
            s.spawn(move || {
                writer.apply(t, &p.change, None, &mut NoJournal).unwrap();
            });
        }
    });
    let t = rig.tables();
    assert_eq!(t.ids("thing").len(), 2 + 1 + 8, "a, b, base and the eight");
    assert!(t.ids("thing").contains(&"w7"));
    let text = text_of(&rig);
    assert_eq!(text.matches("[[thing]]").count(), 2 + 8, "{text}");
}

#[test]
fn a_write_to_the_project_goes_to_the_project_and_not_to_home() {
    let rig = Rig::new(Some(CREW));
    let t = rig.tables();
    let mut w = write(Where::Project, "thing", "p", set("name = \"P\""));
    w.file = Some("sub/dir/p.toml".into());
    let planned = t.plan(&w).unwrap();
    rig.writer().apply(&t, &planned.change, None, &mut NoJournal).unwrap();
    assert!(rig.project.exists("sub/dir/p.toml") && rig.project.exists(".table.lock"));
    assert_eq!(text_of(&rig), CREW);
    assert_eq!(rig.tables().raw("thing", "p").unwrap().from["name"], "project");
}

#[test]
fn a_write_is_not_made_through_a_link() {
    let rig = Rig::new(Some(CREW));
    let outside = Scratch::new("outside");
    outside.put("target.toml", "api = 1\n");
    std::os::unix::fs::symlink(outside.path(), rig.project.join("sub")).unwrap();
    std::os::unix::fs::symlink(outside.join("target.toml"), rig.home.join("linked.toml")).unwrap();
    let t = rig.tables();
    for (to, file) in [(Where::Project, "sub/x.toml"), (Where::User, "linked.toml")] {
        let w = Write { file: Some(file.into()), ..write(to, "thing", "z", set("name = \"Z\"")) };
        let planned = t.plan(&w).unwrap();
        let err = rig.writer().apply(&t, &planned.change, None, &mut NoJournal).unwrap_err();
        assert!(err.msg.contains("is a link"), "{file}: {err}");
    }
    assert!(!outside.exists("x.toml"));
    assert_eq!(outside.read("target.toml"), "api = 1\n");
}

#[test]
fn a_writer_does_not_wait_forever_for_a_lock_someone_holds() {
    let rig = Rig::new(Some(CREW));
    let t = rig.tables();
    let planned = t.plan(&home_write("thing", "a", set("size = 5"))).unwrap();
    let holder =
        std::fs::File::options().create(true).write(true).truncate(false).open(rig.home.join(".table.lock")).unwrap();
    holder.lock().unwrap();
    let writer = rig.writer().waiting(std::time::Duration::from_millis(60));
    let err = writer.apply(&t, &planned.change, None, &mut NoJournal).unwrap_err();
    assert!(err.msg.contains("cannot take the lock") && err.msg.contains("another writer holds it"), "{err}");
    assert_eq!(text_of(&rig), CREW);
    drop(holder);
    writer.apply(&t, &planned.change, None, &mut NoJournal).unwrap();
}

#[test]
fn a_file_name_with_a_space_or_an_accent_is_a_file_name() {
    let rig = Rig::new(None);
    let t = rig.tables();
    for file in ["my crew.toml", "équipe/ça.toml", "a-b_c.d.toml"] {
        let w = Write { file: Some(file.into()), ..home_write("thing", "z", set("name = \"Z\"")) };
        assert!(t.plan(&w).is_ok(), "{file}");
    }
    for file in ["a\\b.toml", "a:b.toml", "tab\t.toml", "x.toml/", "", ".toml", "a/.b.toml"] {
        let w = Write { file: Some(file.into()), ..home_write("thing", "z", set("name = \"Z\"")) };
        assert!(t.plan(&w).is_err(), "{file:?}");
    }
}

#[test]
fn a_layer_may_override_one_field_of_an_entry_a_lower_layer_made() {
    let rig = Rig::new(None);
    let (planned, _) = rig.run(&home_write("thing", "base", set("size = 5"))).unwrap();
    assert_eq!(planned.change.before, None, "the home layer had no say about it");
    assert_eq!(rig.home.read("thing.toml"), "api = 1\n\n[[thing]]\nid = \"base\"\nsize = 5\n");
    let t = rig.tables();
    let base = t.raw("thing", "base").unwrap();
    assert_eq!((base.fields["size"].as_integer(), base.from["size"].as_str()), (Some(5), "home"));
    assert_eq!((base.fields["name"].as_str(), base.from["name"].as_str()), (Some("from the pack"), "p"));
    // taking the override away lets the pack's entry show again, and the entry is gone from the file
    rig.run(&home_write("thing", "base", Op::Remove)).unwrap();
    assert_eq!(rig.tables().raw("thing", "base").unwrap().from["size"], "default");
}

#[test]
fn the_tables_can_be_shared_between_threads_and_a_change_has_a_fixed_wire_form() {
    fn shared<T: Send + Sync>() {}
    shared::<Tables>();
    shared::<Writer>();
    shared::<Change>();
    let change = Change {
        id: "0123456789abcdef".into(),
        target: j4n_core::change::Target {
            layer: Where::Project,
            file: "a.toml".into(),
            kind: "thing".into(),
            id: "x".into(),
        },
        before: None,
        after: Some("size = 3".parse().unwrap()),
        by: Actor::Rule("r1".into()),
        cause: Some(Cause::Undo("fedcba9876543210".into())),
        at_ms: 42,
    };
    let json = serde_json::to_string(&change).unwrap();
    assert_eq!(
        json,
        r#"{"id":"0123456789abcdef","target":{"layer":"project","file":"a.toml","kind":"thing","id":"x"},"before":null,"after":{"size":3},"by":{"rule":"r1"},"cause":{"undo":"fedcba9876543210"},"at_ms":42}"#
    );
    assert_eq!(serde_json::from_str::<Change>(&json).unwrap(), change);
    for (actor, wire) in
        [(Actor::Operator, r#""operator""#), (Actor::Core, r#""core""#), (Actor::Model("m".into()), r#"{"model":"m"}"#)]
    {
        assert_eq!(serde_json::to_string(&actor).unwrap(), wire);
    }
    for (cause, wire) in [(Cause::Event(7), r#"{"event":7}"#), (Cause::Proposal("p".into()), r#"{"proposal":"p"}"#)] {
        assert_eq!(serde_json::to_string(&cause).unwrap(), wire);
    }
    assert_eq!(
        serde_json::to_string(&[Where::User, Where::Project, Where::Session]).unwrap(),
        r#"["user","project","session"]"#
    );
}

#[test]
fn the_edited_text_is_what_an_approval_card_can_show_before_anything_is_written() {
    let rig = Rig::new(Some(CREW));
    let t = rig.tables();
    let change = t.plan(&home_write("thing", "a", set("size = 7"))).unwrap().change;
    let text = edited(CREW, &change).unwrap();
    assert_eq!(text, CREW.replace("size = 4         # since the storm", "size = 7         # since the storm"));
    assert_eq!(text_of(&rig), CREW, "nothing was written");
    let create = t.plan(&write(Where::User, "thing", "n", set("name = \"N\""))).unwrap().change;
    assert_eq!(edited("", &create).unwrap(), "api = 1\n\n[[thing]]\nid = \"n\"\nname = \"N\"\n");
    assert!(edited("[unclosed", &change).unwrap_err().contains("expected"), "a file that is not TOML is not edited");
}

#[test]
fn an_edit_is_confirmed_only_when_it_is_exactly_the_change() {
    let rig = Rig::new(Some(CREW));
    let t = rig.tables();
    let change = t.plan(&home_write("thing", "a", set("size = 7"))).unwrap().change;
    let good = edited(CREW, &change).unwrap();
    assert!(confirm(CREW, &good, &change).is_ok());
    for (what, bad) in [
        ("the entry unchanged", CREW.to_string()),
        ("another value", good.replace("size = 7", "size = 8")),
        ("another entry touched", good.replace("name = \"Bo\"", "name = \"Bob\"")),
        ("an entry dropped", good.replace("[[note]]\nid = \"n1\"\ntext = \"hello\"\n", "")),
        ("an entry added", format!("{good}\n[[note]]\nid = \"n2\"\ntext = \"x\"\n")),
        ("api gone", good.replace("api = 1", "")),
        ("not TOML", "[[[".to_string()),
    ] {
        let err = confirm(CREW, &bad, &change).expect_err(what);
        assert!(err.contains("did not give what was asked") || err.contains("not valid TOML"), "{what}: {err}");
    }
    assert!(confirm("[[[", &good, &change).unwrap_err().contains("the file is not valid TOML"));
    // a new file is confirmed with its api line, and not without it
    let create = t.plan(&write(Where::User, "thing", "n", set("name = \"N\""))).unwrap().change;
    let fresh = edited("", &create).unwrap();
    assert!(confirm("", &fresh, &create).is_ok());
    assert!(confirm("", &fresh.replace("api = 1\n", ""), &create).is_err());
}
