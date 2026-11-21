//! Packages for the kit's kernel. `uv` is a script on the test's own folder, never the
//! real one, and J4n's home is a scratch folder: no network, no real install, nothing under the real `~/.j4n`. What is tested: the Gate's Ask
//! before the cell, once, with the package; a grant and the setting answer it, a Block row wins; the package imports in the
//! same session; `temp=True` leaves nothing in the shared folder and its folder goes with the kernel; no `uv`; a name that is not a name;
//! and, where `bwrap` runs, a kernel in a box that reads the folder and cannot write it.

use crate::bench::base_layer;
use crate::world::Scratch;
use j4n_connect::kithost::{Host, KitSet};
use j4n_connect::model::Call;
use j4n_connect::model::doer::{ToolOut, Tools};
use j4n_connect::place::Boxed;
use j4n_connect::place::boxed;
use j4n_connect::py::PyTools;
use j4n_core::change::Actor;
use j4n_core::fold::Fold;
use j4n_core::gate::check::Gates;
use j4n_core::gate::grant::{Grants, Scope};
use j4n_core::gate::{Action, Gate, ReadOnly, Verb, Verdict};
use j4n_core::log::Ev;
use j4n_core::place::checkpoint::Cfg;
use j4n_core::place::{Here, Launch, Place, Prepare, Site};
use j4n_core::setting::Settings;
use j4n_core::table::{Layer, LayerKind, SourceFile, Tables};
use j4n_core::task::{Mode, TaskSpec};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

const GOOD: &str = "import j4n\nj4n.need(\"fakepkg\")\nimport fakepkg\nfakepkg.VALUE";

fn kit_layer(extra: &str) -> Vec<Layer> {
    let files = vec![SourceFile { path: "need.toml".into(), text: include_str!("../../../../packs/kit/gates/need.toml").into() }];
    let mut layers = vec![base_layer(), Layer::new(LayerKind::Pack, "kit", files)];
    if !extra.is_empty() {
        layers.push(Layer::new(LayerKind::User, "mine", vec![SourceFile { path: "gates.toml".into(), text: extra.into() }]));
    }
    layers
}

fn gate(extra: &str, grants: &Grants) -> Gate {
    let (tables, problems) = Tables::load(kit_layer(extra));
    assert!(problems.is_empty(), "{problems:?}");
    Gate::new(Gates::load(&tables).0, grants.clone())
}

/// A `uv` that makes what `uv pip install --target <dir> -- <name>` makes, and writes its arguments in `uv.log`; `FAIL` in the folder
/// makes it say why it cannot.
fn fake_uv(bin: &Scratch) -> String {
    let log = bin.join("uv.log");
    let script = format!(
        "#!/bin/sh\necho \"$@\" >> {log:?}\npwd >> {cwd:?}\nif [ -f {noisy:?} ]; then head -c 300000 /dev/zero | tr '\\0' x >&2; fi\nif [ -f {fail:?} ]; then echo 'No solution found when resolving' >&2; exit 1; fi\ntarget=''; prev=''; for a in \"$@\"; do if [ \"$prev\" = '--target' ]; then target=\"$a\"; fi; prev=\"$a\"; done\nmkdir -p \"$target/$prev\" \"$target/$prev-1.0.dist-info\"\necho 'VALUE = 42' > \"$target/$prev/__init__.py\"\n",
        log = log.display().to_string(),
        cwd = bin.join("uv.cwd").display().to_string(),
        noisy = bin.join("NOISY").display().to_string(),
        fail = bin.join("FAIL").display().to_string()
    );
    let path = bin.put("uv", &script);
    std::process::Command::new("chmod").arg("+x").arg(&path).status().unwrap();
    path.display().to_string()
}

fn calls(bin: &Scratch) -> Vec<String> {
    bin.read("uv.log").lines().map(String::from).collect()
}

struct Rig {
    home: Scratch,
    bin: Scratch,
    work: Scratch,
    tools: PyTools,
}

impl Rig {
    fn new(uv: Option<&str>, grants: &[&str], gate: Gate) -> Rig {
        let (home, bin, work) = (Scratch::new("kt4-home"), Scratch::new("kt4-bin"), Scratch::new("kt4-work"));
        let uv = uv.map_or_else(|| fake_uv(&bin), String::from);
        let set = KitSet { home: home.path().to_path_buf(), uv, grants: grants.iter().map(|g| g.to_string()).collect(), ..KitSet::default() };
        let tools = PyTools::new().with_gate(gate).with_host(Arc::new(Host::new(set)));
        Rig { home, bin, work, tools }
    }

    fn plain() -> Rig {
        Rig::new(None, &[], gate("", &Grants::new("s1")))
    }

    fn site(&self) -> Site {
        Site::new("t1", self.work.path())
    }

    fn env(&self) -> PathBuf {
        self.home.join("py/env")
    }

    fn gated(&self, code: &str, read_only: ReadOnly) -> Option<(Verb, String, u8, String)> {
        self.tools.gate(&call(code), "t1", &self.site(), read_only).map(|g| (g.action.verb, g.action.target.clone(), g.risk, g.question))
    }

    fn run(&self, code: &str) -> ToolOut {
        self.tools.run(&call(code), &self.site(), &|l: Launch| l)
    }
}

fn call(code: &str) -> Call {
    Call { id: "c1".into(), name: "py".into(), args: json!({ "code": code }), bad: None }
}

fn value(out: &ToolOut) -> String {
    out.text.lines().find_map(|l| l.strip_prefix("=> ")).unwrap_or_default().to_string()
}

fn host_rows(out: &ToolOut) -> Vec<Value> {
    out.py.as_ref().and_then(|p| p["host"].as_array().cloned()).unwrap_or_default()
}

fn ev(n: u64, e: &j4n_core::log::New) -> Ev {
    Ev { seq: n, at_ms: n, kind: e.kind.clone(), task: None, thread: None, by: e.by.clone(), cause: None, class: Default::default(), data: e.data.clone() }
}

// ---- the Gate's question before a cell ----

#[test]
fn an_ungranted_package_asks_once_before_the_cell_and_on_yes_it_imports_in_the_same_session() {
    let r = Rig::plain();
    let (verb, target, risk, question) = r.gated(GOOD, ReadOnly::No).expect("an install asks");
    assert_eq!((verb, target.as_str(), risk), (Verb::Net, "pypi:fakepkg", 40));
    assert!(question.contains("`fakepkg`") && question.contains("from the network") && question.contains("py/env"), "{question}");
    assert!(calls(&r.bin).is_empty(), "asking installs nothing");
    // the yes: the cell runs, and the package is there in the same cell
    let out = r.run(GOOD);
    assert!(!out.error && value(&out) == "42", "{}", out.text);
    let uv = calls(&r.bin);
    assert_eq!(uv.len(), 1, "{uv:?}");
    assert_eq!(uv[0], format!("pip install --quiet --no-progress --color never --python python3 --only-binary :all: --target {} -- fakepkg", r.env().display()));
    // run in the folder it installs into, never where the task works
    let cwd = std::fs::canonicalize(r.bin.read("uv.cwd").trim()).unwrap();
    assert_eq!(cwd, std::fs::canonicalize(r.env()).unwrap());
    assert_ne!(cwd, std::fs::canonicalize(r.work.path()).unwrap());
    let rows = host_rows(&out);
    assert_eq!((rows[0]["call"].as_str(), rows[0]["name"].as_str(), rows[0]["installed"].as_bool(), rows[0]["ok"].as_bool()), (Some("need"), Some("fakepkg"), Some(true), Some(true)), "{rows:?}");
    assert!(rows[0]["size"].as_u64().unwrap() > 0, "the size of what was added is told after the install");
    // later cells of the session keep it; and the next ask for it is none (it is there), and no second install is made
    assert!(value(&r.run("fakepkg.VALUE")) == "42");
    assert_eq!(r.gated(GOOD, ReadOnly::No), None);
    let again = r.run(GOOD);
    assert!(value(&again) == "42" && calls(&r.bin).len() == 1, "{}", again.text);
    assert_eq!(host_rows(&again)[0]["installed"], Value::Null, "nothing installed the second time");
    assert!(r.env().join("fakepkg/__init__.py").is_file());
    // a cell that names no package asks nothing more than a cell does
    assert_eq!(r.gated("1 + 1", ReadOnly::No), None);
}

#[test]
fn a_cell_run_without_the_yes_installs_nothing_whatever_it_calls() {
    let r = Rig::plain();
    // run without the Gate's question first: not asked about, so refused
    let out = r.run(GOOD);
    assert!(out.error && out.text.contains("was not asked about"), "{}", out.text);
    // a name the Gate could not read in the cell is not asked about either, and not installed
    let hidden = "import j4n\nname = 'fake' + 'pkg'\nj4n.need(name)";
    assert_eq!(r.gated(hidden, ReadOnly::No), None, "no literal name, nothing to ask");
    let out = r.run(hidden);
    assert!(out.error && out.text.contains("was not asked about"), "{}", out.text);
    // the raw channel is the same call
    let raw = "import __main__ as m, json\nm._out.write(json.dumps({'call': 'need', 'payload': {'name': 'fakepkg', 'temp': False}}) + '\\n')\nm._out.flush()\nm._in.readline()";
    assert!(r.run(raw).text.contains("was not asked about"));
    assert!(calls(&r.bin).is_empty() && !r.env().join("fakepkg").exists());
}

#[test]
fn an_ask_answered_by_a_grant_or_the_setting_does_not_ask_and_a_block_row_wins_over_both() {
    // the setting kit.grants: a standing yes for named packages (any case, any spelling of the separators)
    let r = Rig::new(None, &["FakePkg", "numpy"], gate("", &Grants::new("s1")));
    assert_eq!(r.gated(GOOD, ReadOnly::No), None);
    let out = r.run(GOOD);
    assert!(value(&out) == "42", "{}", out.text);
    assert!(r.gated("need(\"other\")", ReadOnly::No).is_some(), "only the named ones");
    // a grant of the Gate (the operator's yes for this session on exactly this install) answers it as it does any Ask
    let mut grants = Grants::new("s1");
    let yes = Action::new(Verb::Net, Actor::Operator, "pypi:fakepkg");
    let made = grants.event(&Actor::Operator, Scope::Session, &yes).unwrap();
    let s = Rig::new(None, &[], gate("", &grants));
    assert!(s.gated(GOOD, ReadOnly::No).is_some(), "not yet granted");
    grants.apply(&ev(1, &made));
    assert_eq!(s.gated(GOOD, ReadOnly::No), None, "the grant answered the Ask");
    assert!(value(&s.run(GOOD)) == "42");
    // a Block row wins over the setting and over the grant
    let block = "api = 1\n[[gate]]\nid = \"no-pypi\"\non = [\"net\"]\nwhen = \"action.target matches 'pypi:*'\"\nverdict = \"block\"\nwhy = \"no packages here\"\n";
    let b = Rig::new(None, &["fakepkg"], gate(block, &grants));
    let blocked = b.gated(GOOD, ReadOnly::No).expect("blocked");
    assert_eq!((blocked.0, blocked.2), (Verb::Net, 100));
    assert!(b.run(GOOD).error, "and a cell run on its own after that installs nothing");
    assert!(calls(&b.bin).is_empty());
    // a step that only reads may not add one, with or without a grant
    let g = gate("", &grants);
    let mut a = Action::new(Verb::Net, Actor::Doer("py".into()), "pypi:fakepkg");
    a.read_only = ReadOnly::Explicit;
    assert!(matches!(g.check(&a), Verdict::Block { .. }));
    let plan = Rig::new(None, &["fakepkg"], g);
    assert_eq!(plan.gated(GOOD, ReadOnly::Explicit).map(|x| x.2), Some(100));
}

#[test]
fn one_package_a_cell_is_asked_about_and_the_others_wait_for_their_own_question() {
    let r = Rig::plain();
    let two = "import j4n\nj4n.need(\"fakea\")\ntry:\n    j4n.need(\"fakeb\")\nexcept j4n.HostError as e:\n    print(e)\nimport fakea\nfakea.VALUE";
    let asked = r.gated(two, ReadOnly::No).expect("asks");
    assert_eq!(asked.1, "pypi:fakea", "the first one");
    let out = r.run(two);
    assert!(out.text.contains("`fakeb` was not asked about") && value(&out) == "42", "{}", out.text);
    assert_eq!(calls(&r.bin).len(), 1);
    assert_eq!(r.gated("need(\"fakeb\")", ReadOnly::No).map(|x| x.1), Some("pypi:fakeb".into()));
}

#[test]
fn a_name_is_a_name_and_nothing_else() {
    let r = Rig::plain();
    for bad in ["pandas==2.0", "--index-url=http://x", "https://x/y.whl", "../x", "a b", "", "x[extra]", "evil.whl", "x.tar.gz", "pkg.zip", "a.TGZ", "b.egg", "c.tar", "d.xz", "e.bz2"] {
        let out = r.run(&format!("import j4n\nj4n.need({bad:?})"));
        assert!(out.error && out.text.contains("takes a package name"), "{bad}: {}", out.text);
    }
    assert!(calls(&r.bin).is_empty(), "no uv ran for a name that is a file");
    assert!(!r.bin.join("uv.cwd").exists());
    // a file in the task's folder with such a name is no way to install it either
    r.work.put("evil.whl", "not a wheel");
    assert!(r.gated("import j4n\nj4n.need('evil.whl')", ReadOnly::No).is_none());
    assert!(r.run("import j4n\nj4n.need('evil.whl')").text.contains("takes a package name"));
    assert!(calls(&r.bin).is_empty());
    for good in ["zope.interface", "ruamel.yaml", "scikit-learn", "Pillow", "a", "pyyaml", "tomli_w", "zipp", "gzip-utils"] {
        assert!(j4n_connect::kitpkg::valid(good), "{good}");
    }
    assert!(j4n_connect::kitpkg::named("need('a b')\nneed(\"x==1\")").is_empty(), "nothing to ask about for a name that is not one");
    assert_eq!(j4n_connect::kitpkg::named("j4n.need('Scikit_Learn')\nneed(\"numpy\", temp=True)\nxneed('no')\n# need('numpy', temp=True)"), [("scikit-learn".to_string(), false), ("numpy".to_string(), true)]);
}

#[test]
fn uv_missing_says_so_plainly_and_the_kit_goes_on_without_packages() {
    let r = Rig::new(Some("/nonexistent/uv"), &[], gate("", &Grants::new("s1")));
    assert!(r.gated(GOOD, ReadOnly::No).is_some(), "it still asks: the yes is the operator's, whether or not uv is there");
    let out = r.run("import j4n\nj4n.need(\"fakepkg\")");
    assert!(out.error && out.text.contains("uv is not on this machine") && out.text.contains("pip install") && out.text.contains("standard library"), "{}", out.text);
    // no crash: the same kernel answers, and the stdlib is there
    let next = r.run("import statistics\nstatistics.mean([1, 2, 3])");
    assert!(!next.error && value(&next) == "2", "{}", next.text);
    assert_eq!(r.tools.alive(), 1);
    // the same when the name is on the PATH and the PATH has none
    let bare = Rig::new(Some("no-such-uv-anywhere"), &["fakepkg"], gate("", &Grants::new("s1")));
    let code = "import j4n\nj4n.need(\"fakepkg\")";
    assert_eq!(bare.gated(code, ReadOnly::No), None, "granted ahead");
    assert!(bare.run(code).text.contains("uv is not on this machine"));
}

#[test]
fn a_failed_install_says_why_and_leaves_the_kernel_alone() {
    let r = Rig::plain();
    r.bin.put("FAIL", "");
    assert!(r.gated(GOOD, ReadOnly::No).is_some());
    let out = r.run("x = 5\nimport j4n\nj4n.need(\"fakepkg\")");
    assert!(out.error && out.text.contains("could not install `fakepkg`") && out.text.contains("No solution found"), "{}", out.text);
    assert!(!r.env().join("fakepkg").exists());
    assert_eq!(value(&r.run("x")), "5", "the kernel kept its variables");
}

// ---- temporary packages ----

fn temp_dir(r: &Rig) -> PathBuf {
    r.home.join("py/tmp/t1")
}

const TEMP: &str = "import j4n\nj4n.need(\"fakepkg\", temp=True)\nimport fakepkg\nfakepkg.VALUE";

#[test]
fn temp_leaves_nothing_in_the_shared_folder_and_its_folder_goes_with_the_kernel() {
    let r = Rig::plain();
    assert_eq!(r.gated(TEMP, ReadOnly::No).map(|x| x.1), Some("pypi:fakepkg".into()));
    let out = r.run(TEMP);
    assert!(!out.error && value(&out) == "42", "{}", out.text);
    assert!(calls(&r.bin)[0].contains(&format!("--target {} ", temp_dir(&r).display())), "{:?}", calls(&r.bin));
    assert!(temp_dir(&r).join("fakepkg/__init__.py").is_file());
    assert!(!r.env().join("fakepkg").exists() && std::fs::read_dir(r.env()).unwrap().count() == 0, "nothing in ~/.j4n/py/env");
    // the shared folder is a different ask: the package is not there for a plain need
    assert!(r.gated("need(\"fakepkg\")", ReadOnly::No).is_some());
    // another task has no such folder and does not see it
    let other = r.tools.run(&call("import fakepkg"), &Site::new("t2", r.work.path()), &|l: Launch| l);
    assert!(other.error && other.text.contains("ModuleNotFoundError"), "{}", other.text);
    // the kernel ends (a restart): the folder goes with it
    let gone = r.tools.run(&Call { id: "c".into(), name: "py".into(), args: json!({ "code": "", "restart": true }), bad: None }, &r.site(), &|l: Launch| l);
    assert!(!gone.error, "{}", gone.text);
    assert!(!temp_dir(&r).exists(), "the temp folder went with the kernel");
    assert!(r.env().is_dir());
}

#[test]
fn the_temp_folder_goes_when_the_idle_kernel_is_reaped_and_when_the_tool_ends() {
    let (home, bin, work) = (Scratch::new("kt4-homeb"), Scratch::new("kt4-binb"), Scratch::new("kt4-workb"));
    let set = KitSet { home: home.path().to_path_buf(), uv: fake_uv(&bin), ..KitSet::default() };
    let t = PyTools::new().with_gate(gate("", &Grants::new("s1"))).with_host(Arc::new(Host::new(set))).idle(Duration::from_millis(300));
    let s = Site::new("t1", work.path());
    // the Gate is asked first, as the loop does; the yes is assumed
    let run = |code: &str| {
        assert!(t.gate(&call(code), "t1", &s, ReadOnly::No).is_some() || code == "1");
        t.run(&call(code), &s, &|l: Launch| l)
    };
    assert!(value(&run(TEMP)) == "42");
    assert!(home.join("py/tmp/t1").is_dir());
    // the pool lets a reaped kernel go before it ends it and its folder, so a loaded machine sees the count at 0 first
    let end = Instant::now() + Duration::from_secs(20);
    while (t.alive() > 0 || home.join("py/tmp/t1").exists()) && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(t.alive(), 0);
    assert!(!home.join("py/tmp/t1").exists(), "reaped with its kernel");
    // and when the tool itself is dropped with a kernel alive
    assert!(value(&run(TEMP)) == "42");
    assert!(home.join("py/tmp/t1").is_dir());
    drop(t);
    assert!(!home.join("py/tmp/t1").exists(), "dropped with its kernel");
}

#[test]
fn a_temp_folder_a_j4n_that_ended_badly_left_is_swept_when_the_next_kernel_starts() {
    let r = Rig::plain();
    let old = r.home.join("py/tmp/oldtask");
    let fresh = r.home.join("py/tmp/freshtask");
    for d in [&old, &fresh] {
        std::fs::create_dir_all(d.join("pkg")).unwrap();
    }
    let long_ago = std::time::SystemTime::now() - Duration::from_secs(5 * 24 * 3600);
    std::fs::File::open(&old).unwrap().set_modified(long_ago).unwrap();
    let out = r.run("1");
    assert!(!out.error, "{}", out.text);
    assert!(!old.exists() && fresh.exists());
}

// ---- in a box ----

fn bwrap_works() -> bool {
    std::process::Command::new("bwrap").args(["--ro-bind", "/", "/", "true"]).output().is_ok_and(|o| o.status.success())
}

#[test]
fn in_a_box_the_kernel_reads_the_packages_after_they_are_added_and_cannot_write_them() {
    if !bwrap_works() {
        return;
    }
    let (h, w, bin) = (Scratch::new("kt4-boxh"), Scratch::new("kt4-boxw"), Scratch::new("kt4-boxb"));
    let rows = Layer::new(LayerKind::Pack, "boxes", vec![SourceFile { path: "boxes/b.toml".into(), text: include_str!("../../../../packs/base/boxes/bwrap.toml").into() }]);
    let (tables, _) = Tables::load(vec![rows]);
    let (walls, problems) = boxed::rows(&tables);
    assert!(problems.is_empty(), "{problems:?}");
    let j4n_home = h.join(".j4n");
    let place = Boxed::new("box", Arc::new(Here::new(h.path(), w.path(), Cfg::default())), walls, &Settings::from_pairs([]), h.path(), &j4n_home);
    let mut spec = TaskSpec::new("go", "d");
    spec.tune.mode = Mode::Write;
    let s = place.prepare(&Prepare { task: "t1", spec: &spec, root: w.path() }).expect("a site behind the wall");
    let wrap = |l: Launch| place.wrap(&s, l);
    let set = KitSet { home: j4n_home.clone(), uv: fake_uv(&bin), ..KitSet::default() };
    let t = PyTools::new().with_gate(gate("", &Grants::new("s1"))).with_host(Arc::new(Host::new(set)));
    let run = |code: &str| t.run(&call(code), &s, &wrap);
    // the first cell starts the kernel before any package is there; the folder exists so the wall can bind it
    assert!(run("1").text.contains("=> 1"));
    assert!(t.gate(&call(GOOD), "t1", &s, ReadOnly::No).is_some());
    let out = run(GOOD);
    assert!(!out.error && value(&out) == "42", "{}", out.text);
    // the rest of J4n's home is empty in there, the packages are read-only, and the home on the host holds what the install made
    let probe = format!("import os\nr = []\nfor p in [{:?}, {:?}]:\n    try:\n        open(p, 'w').write('x'); r.append('wrote')\n    except OSError as e:\n        r.append(type(e).__name__)\nr", j4n_home.join("py/env/fakepkg/new.py").display().to_string(), j4n_home.join("py/env/owned.txt").display().to_string());
    let tried = run(&probe);
    assert!(tried.text.contains("=> ['OSError'") || tried.text.contains("PermissionError") || tried.text.contains("FileNotFoundError") || tried.text.contains("OSError"), "{}", tried.text);
    assert!(!tried.text.contains("wrote"), "{}", tried.text);
    assert!(!j4n_home.join("py/env/owned.txt").exists());
    assert!(run("import os\nos.path.exists('/nonexistent')").text.contains("False"));
    let listing = run(&format!("import os\nsorted(os.listdir({:?}))", j4n_home.display().to_string()));
    assert!(listing.text.contains("=> ['py']"), "J4n's home holds nothing else in the box: {}", listing.text);
}

#[test]
fn a_cell_that_itself_asks_is_one_question_with_its_packages_and_the_yes_covers_both() {
    let r = Rig::plain();
    // a task that only looks read-only asks before a cell that may write: the question names the package too
    let asked = r.gated(GOOD, ReadOnly::Inferred).expect("asks");
    assert_eq!(asked.0, Verb::Shell, "the cell's own action is the one asked");
    assert!(asked.3.contains("`fakepkg`") && asked.3.contains("run a Python cell"), "{}", asked.3);
    // after the yes, the cell installs it in the same run
    let out = r.run(GOOD);
    assert!(!out.error && value(&out) == "42", "{}", out.text);
    // a Block of the package ends it, whatever the cell's own question
    let block = "api = 1\n[[gate]]\nid = \"no-pypi\"\non = [\"net\"]\nwhen = \"action.target matches 'pypi:*'\"\nverdict = \"block\"\nwhy = \"no\"\n";
    let b = Rig::new(None, &[], gate(block, &Grants::new("s1")));
    assert_eq!(b.gated(GOOD, ReadOnly::Inferred).map(|x| x.2), Some(100));
}

#[test]
fn what_uv_says_in_bulk_is_read_while_it_runs_and_does_not_stall_it() {
    let r = Rig::plain();
    r.bin.put("NOISY", "");
    assert!(r.gated(GOOD, ReadOnly::No).is_some());
    let began = Instant::now();
    let out = r.tools.run(&Call { id: "c".into(), name: "py".into(), args: json!({ "code": GOOD, "timeout": 20 }), bad: None }, &r.site(), &|l: Launch| l);
    assert!(!out.error && value(&out) == "42", "{}", out.text);
    assert!(began.elapsed() < Duration::from_secs(10), "{:?}", began.elapsed());
    // and a failure keeps the end of what it said, after all that
    r.bin.put("FAIL", "");
    let _ = r.gated("import j4n\nj4n.need('fakeb')", ReadOnly::No);
    let bad = r.run("import j4n\nj4n.need('fakeb')");
    assert!(bad.text.contains("could not install `fakeb`") && bad.text.contains("No solution found"), "{}", bad.text);
}
