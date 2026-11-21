//! `j4n py` for an agent that has a shell of its own. A doer that runs a program is wrapped
//! (`Kitted`): it is told of the kit, and while it runs the task has a socket whose cells go to the task's kernel, the one the own agent's
//! tool `py` uses. The doers here are scripted in Rust and call the same client the command `j4n py` calls (`kitsock::ask`); the command
//! itself, through the real binary, is tested in `crates/j4n/tests/laws/kit_flow.rs`.

use crate::bench::base_layer;
use crate::world::Scratch;
use j4n_connect::kitsock::{self, Kitted};
use j4n_connect::model::Call;
use j4n_connect::model::doer::{ToolOut, Tools};
use j4n_connect::py::PyTools;
use j4n_core::change::Actor;
use j4n_core::clock::ManualClock;
use j4n_core::doer::{Caps, Doer, Probe, Role, Run, RunCtl, RunEv, StartCx};
use j4n_core::fold::Folds;
use j4n_core::gate::check::Gates;
use j4n_core::gate::grant::Grants;
use j4n_core::gate::Gate;
use j4n_core::log::{Ev, Log, Opts, Scope};
use j4n_core::place::Launch;
use j4n_core::redact::Redactor;
use j4n_core::table::{Problem, Tables};
use j4n_core::task::{Config as Loop, Mode, Row, TaskSpec, Tasks};
use j4n_core::timer::Timers;
use serde_json::{Value, json};
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn gate() -> Gate {
    let (tables, _) = Tables::load(vec![base_layer()]);
    Gate::new(Gates::load(&tables).0, Grants::new("s1"))
}

fn python_here() -> bool {
    j4n_connect::py::python_here()
}

/// What a scripted doer does with its task: `play` gets the task, its folder, and a way to run a cell as the own agent.
type Play = Arc<dyn Fn(&Cx) -> String + Send + Sync>;

/// A shell's program in the walls of the task's place: a Python client that gives `x` to the socket of its folder, as `j4n py` does.
const CLIENT: &str = "import json, socket, sys\nd, t = sys.argv[1], sys.argv[2]\nkey = open(f'{d}/.j4n/run/{t}.key').read().strip()\ns = socket.socket(socket.AF_UNIX)\ns.settimeout(20)\ns.connect(f'{d}/.j4n/run/{t}.sock')\ns.sendall((json.dumps({'task': t, 'key': key, 'code': 'x + 1'}) + '\\n').encode())\nprint(s.makefile().readline().strip())";

struct Cx {
    walled: Launch,
    /// The kernel's launch in the task's walls, which the own agent's tool uses as well.
    kernel: Launch,
    task: String,
    dir: std::path::PathBuf,
    py: Arc<PyTools>,
    text: String,
}

impl Cx {
    /// A cell of the tool `py`, as the own agent runs it.
    fn own(&self, code: &str) -> ToolOut {
        let site = j4n_core::place::Site::new(&self.task, &self.dir);
        self.py.run(&Call { id: "c".into(), name: "py".into(), args: json!({ "code": code }), bad: None }, &site, &|_| self.kernel.clone())
    }

    /// The first key file and the task of the socket beside this task's, as a client reads them.
    fn run_dir(&self) -> std::path::PathBuf {
        self.dir.join(kitsock::RUN)
    }
}

/// A doer with a program of its own that is, here, a function.
struct Shell {
    py: Arc<PyTools>,
    play: Play,
    seen: Arc<Mutex<Vec<String>>>,
    /// Whether it takes a Brief (then the core's contract carries the sentence of `brief.plain_answer`, not the wrapper).
    brief: bool,
}

struct Waiting(Sender<RunCtl>);

impl Run for Waiting {
    fn ctl(&mut self, ctl: RunCtl) -> Result<(), Problem> {
        let _ = self.0.send(ctl);
        Ok(())
    }
}

impl Doer for Shell {
    fn id(&self) -> &str {
        "shell"
    }

    fn probe(&self) -> Probe {
        Probe { installed: true, version: None, authed: true, detail: String::new() }
    }

    fn caps(&self) -> Caps {
        Caps { live: true, brief: self.brief, ..Caps::default() }
    }

    fn start(&self, cx: StartCx) -> Result<Box<dyn Run>, Problem> {
        let text = cx.text.clone().unwrap_or_else(|| cx.spec.goal.clone());
        self.seen.lock().unwrap().push(text.clone());
        let (tx, rx): (Sender<RunCtl>, Receiver<RunCtl>) = mpsc::channel();
        let walled = cx.wrap(Launch::new(["python3", "-I", "-c", CLIENT, &cx.site.dir.display().to_string(), &cx.task].map(String::from).to_vec(), cx.site.dir.clone(), Default::default()));
        let kernel = cx.wrap(j4n_connect::py::kernel_launch(&cx.site));
        let (me, play) = (Cx { walled, kernel, task: cx.task.clone(), dir: cx.site.dir.clone(), py: self.py.clone(), text }, self.play.clone());
        let out = cx.out.clone();
        std::thread::spawn(move || {
            let said = play(&me);
            out.send(RunEv::Message { role: Role::Assistant, text: said.clone() });
            out.send(RunEv::End { answer: Some(said), todo_left: 0 });
            drop(rx);
        });
        Ok(Box::new(Waiting(tx)))
    }
}

struct Rig {
    tasks: Tasks,
    log: Log,
    seen: Arc<Mutex<Vec<String>>>,
    project: Scratch,
    _home: Scratch,
    _timers: Arc<Timers>,
}

impl Rig {
    /// A loop over a project whose one doer is the scripted `shell`, wrapped as `Kitted` when `kitted`.
    fn new(kitted: bool, play: Play) -> Rig {
        Rig::placed(kitted, play, |_, _, _| {})
    }

    fn placed(kitted: bool, play: Play, tune: impl FnOnce(&mut Loop, &Scratch, &Scratch)) -> Rig {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s = seen.clone();
        Rig::over(
            move |py| {
                let inner: Arc<dyn Doer> = Arc::new(Shell { py: py.clone(), play, seen: s, brief: false });
                vec![if kitted { Arc::new(Kitted::new(inner, py.clone())) as Arc<dyn Doer> } else { inner }]
            },
            seen,
            tune,
        )
    }

    /// A loop over a project whose doers `doers` makes, given the tool `py` (the gate of the base rows).
    fn over(
        doers: impl FnOnce(&Arc<PyTools>) -> Vec<Arc<dyn Doer>>,
        seen: Arc<Mutex<Vec<String>>>,
        tune: impl FnOnce(&mut Loop, &Scratch, &Scratch),
    ) -> Rig {
        let (project, home) = (Scratch::new("ks-proj"), Scratch::new("ks-home"));
        let clock = Arc::new(ManualClock::at(1_000));
        let opts = Opts { segment_bytes: 8 << 20, max_line: 1 << 20, checkpoint_every: 1000, clock };
        let (log, problems) = Log::open(&project.join(".j4n"), Scope::Project, opts, Redactor::new()).expect("opens");
        assert!(problems.is_empty(), "{problems:?}");
        let folds = Folds::new(&project.join(".j4n/folds"));
        folds.attach(&log).expect("attaches");
        let timers = Arc::new(Timers::start(&log));
        let py = Arc::new(PyTools::new().with_gate(gate()));
        let mut lp = Loop::new(&log, &folds, &timers, project.path(), home.path());
        lp.doers = doers(&py);
        tune(&mut lp, &project, &home);
        let (tasks, problems) = Tasks::start(lp).expect("the loop starts");
        assert!(problems.iter().all(|p| p.msg.contains("left out")), "{problems:?}");
        Rig { tasks, log, seen, project, _home: home, _timers: timers }
    }

    fn spawn(&self, goal: &str, mode: Mode) -> String {
        self.spawn_in(goal, mode, "here")
    }

    fn spawn_in(&self, goal: &str, mode: Mode, place: &str) -> String {
        self.spawn_for("shell", goal, mode, place)
    }

    fn spawn_for(&self, doer: &str, goal: &str, mode: Mode, place: &str) -> String {
        let mut spec = TaskSpec::new(goal, doer);
        spec.tune.mode = mode;
        spec.place = place.into();
        self.tasks.spawn(spec, Actor::Operator).expect("spawns")
    }

    fn finished(&self, id: &str) -> Row {
        self.tasks.book().wait(id, |r| r.state.is_terminal(), Duration::from_secs(30)).unwrap_or_else(|| panic!("{id} never ended\n{}", self.dump()))
    }

    fn events(&self, kind: &str) -> Vec<Ev> {
        self.log.sync();
        self.log.read(0).filter(|e| e.kind == kind).collect()
    }

    fn dump(&self) -> String {
        self.log.read(0).map(|e| format!("{:>4} {:<16} {} {}", e.seq, e.kind, e.task.as_deref().unwrap_or("-"), e.data)).collect::<Vec<_>>().join("\n")
    }
}

fn ask(dir: &Path, task: Option<&str>, code: &str) -> Result<(bool, String), String> {
    kitsock::ask(dir, task, code, Some(20))
}

#[test]
fn a_shells_cell_reads_a_variable_the_own_agent_set_in_the_same_task_and_another_tasks_socket_is_refused() {
    if !python_here() {
        return;
    }
    // task A: the own agent sets x, then the shell's `j4n py -c 'x + 1'` reads it; A then stays until it is let go
    let hold = Arc::new(Mutex::new(false));
    let ids: Arc<Mutex<Vec<String>>> = Arc::default();
    let (h, i) = (hold.clone(), ids.clone());
    let play: Play = Arc::new(move |cx: &Cx| {
        if cx.text.starts_with("a:") {
            i.lock().unwrap().push(cx.task.clone());
            assert!(!cx.own("x = 41").error);
            let (ok, said) = ask(&cx.dir, None, "x + 1").expect("the socket answers");
            while !*h.lock().unwrap() {
                std::thread::sleep(Duration::from_millis(10));
            }
            return format!("{ok} {said}");
        }
        // task B, while A's socket is still open: two sockets are in the folder
        let wait_a = || loop {
            if let Some(a) = i.lock().unwrap().first().cloned() {
                return a;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let a = wait_a();
        let mut said = Vec::new();
        said.push(format!("ambiguous: {}", ask(&cx.dir, None, "x").unwrap_err()));
        // B's own socket is B's kernel: no x there
        let mine = ask(&cx.dir, Some(&cx.task), "x").expect("its own socket answers");
        said.push(format!("own: {} {}", mine.0, mine.1.trim()));
        // A's socket with B's task and B's key: refused, and so is A's task with B's key, and with no key
        let key = std::fs::read_to_string(cx.run_dir().join(format!("{}.key", cx.task))).unwrap();
        for (name, req) in [("task", json!({ "task": cx.task, "key": key.trim(), "code": "x" })), ("key", json!({ "task": a, "key": key.trim(), "code": "x" })), ("nokey", json!({ "task": a, "code": "x" }))] {
            let a = kitsock::send(&cx.run_dir(), &a, &req, Duration::from_secs(10)).expect("a refusal is an answer");
            said.push(format!("{name}: {} {}", a["ok"], a["error"].as_str().unwrap_or_default()));
        }
        said.join("\n")
    });
    let rig = Rig::new(true, play);
    let a = rig.spawn("a: read x", Mode::Write);
    let end = std::time::Instant::now() + Duration::from_secs(20);
    while rig.events("py.cell").is_empty() && std::time::Instant::now() < end {
        std::thread::sleep(Duration::from_millis(10));
    }
    let b = rig.spawn("b: try", Mode::Write);
    let row_b = rig.finished(&b);
    *hold.lock().unwrap() = true;
    let row_a = rig.finished(&a);
    // the shell read the own agent's variable
    let said = row_a.answer.unwrap();
    assert!(said.starts_with("true") && said.contains("=> 42"), "{said}");
    // the cells are in the Log, the shell's with its source
    let cells = rig.events("py.cell");
    let from: Vec<(&str, Option<&str>)> = cells.iter().filter(|e| e.task.as_deref() == Some(&a)).map(|e| (e.data["head"].as_str().unwrap(), e.data["via"].as_str())).collect();
    assert_eq!(from, [("x + 1", Some("j4n py"))], "the shell's cell is an event of the task; the own agent's is the loop's own: {}", rig.dump());
    // B: two sockets make the bare command ask which task; B's kernel has no x; every request at A's socket that is not A's is refused
    let said = row_b.answer.unwrap();
    assert!(said.contains("ambiguous: more than one task has a kernel here"), "{said}");
    assert!(said.contains("own: false") && said.contains("NameError"), "{said}");
    for name in ["task", "key", "nokey"] {
        let line = said.lines().find(|l| l.starts_with(&format!("{name}: "))).unwrap_or_else(|| panic!("{name} in {said}"));
        assert!(line.starts_with(&format!("{name}: false")) && line.contains("another task's"), "{line}");
    }
}

#[test]
fn the_socket_is_there_while_the_run_is_and_goes_with_it_and_its_files_are_private() {
    if !python_here() {
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    let (tx, rx) = mpsc::channel::<Value>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let release_rx = Mutex::new(release_rx);
    let tx = Mutex::new(tx);
    let play: Play = Arc::new(move |cx: &Cx| {
        let dir = cx.run_dir();
        let mode = |name: &str| std::fs::metadata(dir.join(name)).unwrap().permissions().mode() & 0o777;
        let files: Vec<String> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        tx.lock().unwrap().send(json!({ "task": cx.task, "files": files, "dir": std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, "sock": mode(&format!("{}.sock", cx.task)), "key": mode(&format!("{}.key", cx.task)) })).unwrap();
        release_rx.lock().unwrap().recv().ok();
        "done".into()
    });
    let rig = Rig::new(true, play);
    let id = rig.spawn("work", Mode::Write);
    let seen = rx.recv_timeout(Duration::from_secs(20)).expect("the run began");
    let mut files: Vec<String> = serde_json::from_value(seen["files"].clone()).unwrap();
    files.sort();
    assert_eq!(files, [format!("{id}.bin"), format!("{id}.key"), format!("{id}.sock")], "the link folder is there too");
    assert_eq!((seen["dir"].as_u64(), seen["sock"].as_u64(), seen["key"].as_u64()), (Some(0o700), Some(0o600), Some(0o600)), "{seen}");
    // the folder's state directory ignores itself, so the project's `git status` does not see the socket
    assert_eq!(std::fs::read_to_string(rig.project.join(".j4n/.gitignore")).unwrap_or_default(), "*\n");
    release_tx.send(()).unwrap();
    rig.finished(&id);
    // the task is over; the Run is dropped when the loop lets it go, and the files with it
    let end = std::time::Instant::now() + Duration::from_secs(10);
    let run = rig.project.join(".j4n/run");
    while std::fs::read_dir(&run).map_or(0, Iterator::count) > 0 && std::time::Instant::now() < end {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(std::fs::read_dir(&run).map_or(0, Iterator::count), 0, "the files of {id} are still there");
    assert!(ask(rig.project.path(), None, "1").is_err());
}

#[test]
fn a_cell_from_the_shell_is_one_gate_call_read_by_pyread_and_what_the_gate_asks_is_refused_with_its_question() {
    if !python_here() {
        return;
    }
    let play: Play = Arc::new(|cx: &Cx| {
        let mut said = Vec::new();
        for code in ["import math\nmath.sqrt(49)", "open('made.txt', 'w').write('x')"] {
            let a = ask(&cx.dir, None, code).unwrap_err_or_ok();
            said.push(a);
        }
        said.join("\n--\n")
    });
    let rig = Rig::new(true, play);
    // a task that may write: the cell that writes is the Gate's to allow, as it is for the own agent's `py`
    let id = rig.spawn("go", Mode::Write);
    let said = rig.finished(&id).answer.unwrap();
    assert!(said.contains("=> 7.0") && said.contains("ran: true => 1"), "{said}");
    assert_eq!(std::fs::read_to_string(rig.project.join("made.txt")).unwrap(), "x");
    std::fs::remove_file(rig.project.join("made.txt")).unwrap();
    let risks: Vec<String> = rig.events("py.cell").iter().map(|e| e.data["risk"].as_str().unwrap().to_string()).collect();
    assert_eq!(risks, ["read", "exec"]);
    // a task that may not: the cell `pyread` reads passes, the one that writes is refused with the Gate's question and never reaches the kernel
    let id = rig.spawn("go", Mode::Read);
    let said = rig.finished(&id).answer.unwrap();
    let (read, exec) = said.split_once("\n--\n").unwrap();
    assert!(read.starts_with("ran: ") && read.contains("=> 7.0"), "{read}");
    assert!(exec.starts_with("refused: ") && exec.contains("not run") && exec.contains("run a Python cell") && exec.contains("grant it ahead"), "{exec}");
    assert!(!rig.project.join("made.txt").exists());
    // one event for each cell that ran, with the risk the Gate read, and none for the one it stopped
    let cells: Vec<Ev> = rig.events("py.cell").into_iter().filter(|e| e.task.as_deref() == Some(&id)).collect();
    assert_eq!(cells.len(), 1, "{}", rig.dump());
    assert_eq!((cells[0].data["risk"].as_str(), cells[0].data["via"].as_str(), cells[0].data["ok"].as_bool()), (Some("read"), Some("j4n py"), Some(true)));
    assert!(cells[0].data["blob"].as_str().is_some() && cells[0].data["up"].as_u64().is_some() && cells[0].data["names"].is_array(), "{}", cells[0].data);
}

trait Shown {
    fn unwrap_err_or_ok(self) -> String;
}

impl Shown for Result<(bool, String), String> {
    fn unwrap_err_or_ok(self) -> String {
        match self {
            Ok((ok, text)) => format!("ran: {ok} {text}"),
            Err(why) => format!("refused: {why}"),
        }
    }
}

#[test]
fn the_doer_is_told_of_the_kit_on_its_first_text_of_a_session_and_not_when_it_is_not_wrapped() {
    if !python_here() {
        return;
    }
    let play: Play = Arc::new(|cx: &Cx| cx.text.clone());
    let on = Rig::new(true, play.clone());
    let id = on.spawn("count the lines", Mode::Write);
    on.finished(&id);
    let text = on.seen.lock().unwrap()[0].clone();
    assert!(text.starts_with("count the lines\n\n") && text.contains("`j4n py -c '<code>'`") && text.contains("import j4n"), "{text}");
    assert!(!text.contains("the `py` tool"), "{text}");
    let off = Rig::new(false, play);
    let id = off.spawn("count the lines", Mode::Write);
    off.finished(&id);
    assert_eq!(off.seen.lock().unwrap()[0], "count the lines");
}

#[test]
fn from_inside_a_box_the_shell_reaches_the_tasks_socket_and_the_kernel_it_shares() {
    if !std::process::Command::new("bwrap").args(["--ro-bind", "/", "/", "true"]).output().is_ok_and(|o| o.status.success()) {
        return;
    }
    use j4n_connect::place::boxed;
    use j4n_connect::place::Boxed;
    use j4n_core::place::checkpoint::Cfg;
    use j4n_core::place::Here;
    use j4n_core::setting::Settings;
    use j4n_core::table::{Layer, LayerKind, SourceFile};
    let play: Play = Arc::new(|cx: &Cx| {
        assert!(!cx.own("x = 41").error);
        let (out, exit) = j4n_connect::proc::capture(&cx.walled, Duration::from_secs(30)).expect("the client runs");
        format!("exit {:?} {}", exit.code, out.trim())
    });
    for mode in [Mode::Write, Mode::Read] {
        let rig = Rig::placed(true, play.clone(), |lp, project, home| {
            let rows = Layer::new(LayerKind::Pack, "boxes", vec![SourceFile { path: "boxes/b.toml".into(), text: include_str!("../../../../packs/base/boxes/bwrap.toml").into() }]);
            let (tables, problems) = Tables::load(vec![rows]);
            assert!(problems.is_empty(), "{problems:?}");
            let (walls, problems) = boxed::rows(&tables);
            assert!(problems.is_empty(), "{problems:?}");
            lp.places.push(Arc::new(Boxed::new("box", Arc::new(Here::new(home.path(), project.path(), Cfg::default())), walls, &Settings::from_pairs([]), home.path(), &home.join(".j4n"))));
        });
        let id = rig.spawn_in("go", mode, "box");
        let said = rig.finished(&id).answer.unwrap();
        assert!(said.starts_with("exit Some(0)") && said.contains("=> 42"), "{mode:?}: {said}\n{}", rig.dump());
    }
}

#[test]
fn a_j4n_py_cell_has_the_bridge_with_what_the_models_rows_offer_less_py_and_ask() {
    if !python_here() {
        return;
    }
    use j4n_connect::manifest::Connectors;
    use j4n_core::table::{Layer, LayerKind, SourceFile};
    let rows = r#"
[[connector]]
id = "builtin"
doc = "The tools J4n carries."
is = "tool"
dialect = "builtin"

[[connector]]
id = "say"
doc = "Say a word."
is = "tool"
dialect = "text"
program = "echo"
args = [{ name = "word", required = true }]
argv = ["echo", "{word}"]
risk = "read"

[[connector]]
id = "own"
doc = "A model."
is = "agent"
dialect = "openai"
url = "http://127.0.0.1:1/v1"
keyless = true
tools = ["builtin", "*", "ask", "py"]
models = ["m"]

[[connector]]
id = "wide"
doc = "A program with a shell of its own; its row names no tools."
is = "agent"
dialect = "text"
program = "python3"
start = ["-I", "-c", "import sys; exec(sys.argv[1].split('#END')[0])", "{prompt}"]
"#;
    let user = Layer::new(
        LayerKind::User,
        "mine",
        vec![SourceFile { path: "connectors/x.toml".into(), text: format!("api = 1\n{rows}") }],
    );
    let (tables, problems) = Tables::load(vec![base_layer(), user]);
    assert!(problems.is_empty(), "{problems:?}");
    let (mut list, said) = Connectors::load(&tables);
    assert!(said.is_empty(), "{said:?}");
    list.set_kit(true);
    list.set_gate(gate());
    let (doers, problems) = list.doers(&|_, _| Err("no secret".into()));
    assert!(problems.is_empty(), "{problems:?}");
    // the program asks the socket of its folder, as `j4n py` does, and prints what the kernel says; the code is the prompt up to `#END`,
    // so that the card J4n adds to it is not run
    let goal = "import glob, json, socket\nsock = glob.glob('.j4n/run/*.sock')[0]\nt = sock.split('/')[-1][:-5]\nkey = open(f'.j4n/run/{t}.key').read().strip()\nask = lambda c: (lambda s: (s.connect(sock), s.sendall((json.dumps({'task': t, 'key': key, 'code': c}) + '\\n').encode()), json.loads(s.makefile().readline())['text'])[2])(socket.socket(socket.AF_UNIX))\nprint(ask(\"import j4n\\nsorted(j4n.tools())\"))\nprint(ask(\"import j4n\\nj4n.call('say', word='kit')\"))\nprint(ask(\"import j4n\\nj4n.call('ask')\"))\n#END";
    let rig = Rig::over(move |_| doers, Arc::default(), |_, _, _| {});
    let said = |doer: &str| {
        let id = rig.spawn_for(doer, goal, Mode::Write, "here");
        rig.finished(&id).answer.unwrap_or_else(|| panic!("no answer from {doer}\n{}", rig.dump()))
    };
    let wide = said("wide");
    assert!(
        wide.contains("'say'")
            && wide.contains("'read'")
            && wide.contains("'run'")
            && !wide.contains("'py'")
            && !wide.contains("'ask'"),
        "a program that names no tools has what the models have, less py and ask: {wide}"
    );
    assert!(
        wide.contains("=> 'kit"),
        "and its call to a command ran (the Gate lets the read-class command through): {wide}"
    );
    assert!(wide.contains("Refused") && wide.contains("is not a tool that code may call"), "{wide}");
}

// ---- the offer line ----

fn specs() -> Vec<j4n_connect::model::Tool> {
    use j4n_connect::model::Tool;
    let t = |name: &str, doc: &str, req: &str| Tool {
        name: name.into(),
        doc: doc.into(),
        schema: json!({ "type": "object", "properties": { req: { "type": "string" } }, "required": [req] }),
    };
    vec![
        t("recall", "Search the project's memory (notes, what tasks reported) for words.", "query"),
        t("web", "Read a page (url) or search the web.", "url"),
        t("read", "Read a text file with line numbers.", "path"),
        t("list", "List a folder.", "path"),
        t("run", "Run a shell command; returns its exit code and output.", "cmd"),
    ]
}

fn lines(n: usize) -> String {
    (1..=n).map(|i| format!("item-{i}\n")).collect()
}

#[test]
fn a_goal_with_a_file_of_24_lines_and_a_per_item_word_gets_one_line_naming_the_tool_that_matches() {
    use j4n_connect::kitoffer::line;
    let dir = Scratch::new("kt8-rule");
    dir.put("bugs.txt", &lines(24));
    dir.put("one.txt", "just-this\n");
    dir.put("seven.txt", &lines(7));
    dir.put("blanks.txt", &format!("{}\n\n\n  \n", lines(5)));
    let fits = line("bugs.txt lists 24 bug ids. For each one, find in the project's memory whether it is fixed and by which commit.", dir.path(), &specs()).expect("a line");
    assert!(fits.starts_with("J4n: this task has 24 items in bugs.txt, one `recall` call each."), "{fits}");
    assert!(
        fits.contains("j4n.recipe('each', tool='recall', items='bugs.txt')") && fits.contains("j4n.many"),
        "a file of items with a tool: the recipe `each`: {fits}"
    );
    assert!(!fits.contains('\n'), "one line");
    // the per-item words: each, every, for all, which of, how many, all of
    for word in ["every one of them", "for all of them", "which of them", "how many of them", "all of them"] {
        assert!(line(&format!("bugs.txt: say something about {word}."), dir.path(), &specs()).is_some(), "{word}");
    }
    // one item, too few items (blank lines do not count), no per-item word, no file: no line
    assert_eq!(line("For each line of one.txt, find it in the memory.", dir.path(), &specs()), None);
    assert_eq!(line("For each line of seven.txt, find it in the memory.", dir.path(), &specs()), None);
    assert_eq!(line("For each line of blanks.txt, find it in the memory.", dir.path(), &specs()), None);
    assert_eq!(
        line("bugs.txt lists 24 bug ids. Find them in the memory.", dir.path(), &specs()),
        None,
        "no per-item word"
    );
    assert_eq!(line("For each bug in missing.txt, find it in the memory.", dir.path(), &specs()), None);
    assert_eq!(
        line("For each bug in ../bugs.txt or /etc/passwd, find it in the memory.", dir.path(), &specs()),
        None,
        "outside the folder is not counted"
    );
    assert_eq!(line("Rename the function in x.rs, and do it for each.", dir.path(), &specs()), None);
    // a tool is named only when its spec matches alone: none shares a stem here, and two share one in the other
    let none = line("bugs.txt holds 24 lines of feedback. How many complain about speed?", dir.path(), &specs())
        .expect("a line");
    assert!(
        none.starts_with("J4n: this task has 24 items in bugs.txt. One `py` cell") && none.contains("j4n.many(tool, "),
        "{none}"
    );
    let tied =
        line("For each of bugs.txt, do the thing with a folder or a shell.", dir.path(), &specs()).expect("a line");
    assert!(!tied.contains("call each"), "two tools share one stem: {tied}");
}

/// "does each of these" and the name of a test file made "this task has 20 items in
/// tests/test_rules.py": the lines of a source file are code, not a list.
#[test]
fn the_lines_of_a_source_file_named_in_the_goal_are_not_items_but_a_data_file_still_is() {
    use j4n_connect::kitoffer::line;
    let dir = Scratch::new("kitsock-a");
    dir.put("tests/test_rules.py", &lines(20));
    dir.put("umber/core.py", &lines(13));
    dir.put("pages.txt", &lines(20));
    let goal = "Fix the failing test in tests/test_rules.py; `CONTRIBUTING.md` says how this project does each of these.";
    assert_eq!(line(goal, dir.path(), &specs()), None, "a test file is not a list");
    assert_eq!(line("Find the commit that broke umber/core.py, and check every commit.", dir.path(), &specs()), None);
    assert!(line("For each page of pages.txt, say whether it names a rate limit.", dir.path(), &specs()).is_some(), "a data file is still a list");
}

#[test]
fn the_items_are_a_folders_files_or_a_list_in_the_goal_and_a_check_on_changed_files_names_the_recipe() {
    use j4n_connect::kitoffer::line;
    let dir = Scratch::new("kt8-items");
    for i in 0..10 {
        dir.put(&format!("src/m{i}.py"), "x = 1\n");
    }
    dir.put("src/.hidden/skip.py", "x\n");
    let many = line("Run the check on every file under src/ and answer with the failing paths.", dir.path(), &specs())
        .expect("a folder of 10 files");
    assert!(many.starts_with("J4n: this task has 10 items in src/, one `run` call each. One `py` cell can do them all: `j4n.many('run', [{...} for x in items])`"), "{many}");
    let marked = line("Do each of these:\n- a\n- b\n- c\n- d\n- e\n- f\n- g\n- h\n", dir.path(), &specs())
        .expect("a marked list");
    assert!(marked.contains("8 items in the goal"), "{marked}");
    let numbered =
        line("Do each of these:\n1. a\n2) b\n3. c\n4. d\n5. e\n6. f\n7. g\n8. h\n9. i", dir.path(), &specs())
            .expect("a numbered list");
    assert!(numbered.contains("9 items in the goal"), "{numbered}");
    let comma = line("Rename it in each of a.rs, b.rs, c.rs, d.rs, e.rs, f.rs, g.rs and h.rs.", dir.path(), &specs())
        .expect("items in a sentence");
    assert!(comma.contains("8 items in the goal"), "{comma}");
    assert_eq!(line("Rename it in each of a.rs, b.rs, c.rs and d.rs.", dir.path(), &specs()), None, "four items");
    assert_eq!(
        line(
            "For each, read the long paragraph here that goes on, then the next long paragraph that goes on and on, then another.",
            dir.path(),
            &specs()
        ),
        None,
        "long pieces are not items"
    );
    let checks = line(
        "Fix the typo in each file under src/, then run the tests on every file you changed.",
        dir.path(),
        &specs(),
    )
    .expect("a line");
    assert!(checks.contains("j4n.recipe('checks', cmd='<the check> {}')") && !checks.contains("`run`"), "{checks}");
}

/// The tools as the bridge really offers them (`tools.rs`, `j4n-reach`): `web` lists `q` and `url` and requires neither, `edit` needs three.
fn real_specs() -> Vec<j4n_connect::model::Tool> {
    use j4n_connect::model::Tool;
    let t = |name: &str, doc: &str, props: &[&str], req: &[&str]| Tool {
        name: name.into(),
        doc: doc.into(),
        schema: json!({ "type": "object", "properties": props.iter().map(|p| (p.to_string(), json!({ "type": "string" }))).collect::<serde_json::Map<_, _>>(), "required": req }),
    };
    vec![
        t("web", "Read a page (url) or search the web (q).", &["q", "url"], &[]),
        t("read", "Read a text file with line numbers (400 lines at most).", &["path", "offset", "limit"], &["path"]),
        t("edit", "Replace one exact text in a file.", &["path", "old", "new"], &["path", "old", "new"]),
        t("run", "Run a shell command; returns its exit code and output.", &["cmd", "timeout"], &["cmd"]),
        t("recall", "Search the project's memory (notes, what tasks reported) for words.", &["query"], &["query"]),
    ]
}

/// The call a printed line names, checked against `specs` as `packs/kit/recipes/each.py` and `j4n.many` would check it: the tool is there,
/// and `each` has one argument for the item (the only required one, else the only one listed) or is given `arg=`. Every form in the line is checked.
fn names_a_call_that_runs(line: &str, specs: &[j4n_connect::model::Tool]) -> Result<usize, String> {
    let find = |name: &str| specs.iter().find(|t| t.name == name).ok_or_else(|| format!("`{name}` is no tool: {line}"));
    let quoted = |from: &str, key: &str| from.split_once(key).and_then(|(_, r)| r.split('\'').next()).map(str::to_string);
    let mut forms = 0;
    for call in line.split("j4n.recipe(\'each\'").skip(1) {
        let call = call.split(')').next().unwrap_or_default();
        let tool = find(&quoted(call, "tool=\'").ok_or("each without a tool")?)?;
        let listed: Vec<&str> = tool.schema["properties"].as_object().map(|p| p.keys().map(String::as_str).collect()).unwrap_or_default();
        let required: Vec<&str> = tool.schema["required"].as_array().map(|r| r.iter().filter_map(|n| n.as_str()).collect()).unwrap_or_default();
        let need = if required.is_empty() { listed.clone() } else { required };
        match quoted(call, "arg=\'") {
            Some(arg) if listed.contains(&arg.as_str()) => {}
            Some(arg) => return Err(format!("arg `{arg}` is none of {listed:?}: {line}")),
            None if need.len() == 1 => {}
            None => return Err(format!("each on `{}` needs arg=, it takes {need:?}: {line}", tool.name)),
        }
        forms += 1;
    }
    for call in line.split("j4n.many(\'").skip(1) {
        find(call.split('\'').next().unwrap_or_default())?;
        forms += 1;
    }
    Ok(forms)
}

#[test]
fn the_line_names_each_only_for_a_tool_it_runs_as_printed_and_no_web_for_local_files() {
    use j4n_connect::kitoffer::line;
    let dir = Scratch::new("kitsock-b");
    dir.put("pages.txt", &lines(24));
    dir.put("urls.txt", &(1..=24).map(|i| format!("https://example.com/p{i}\n")).collect::<String>());
    dir.put("cmds.txt", &(1..=24).map(|i| format!("wc -l f{i}.rs\n")).collect::<String>());
    for i in 0..10 {
        dir.put(&format!("docs/p{i}.md"), "x\n");
    }
    for specs in [real_specs(), specs()] {
        // a benchmark task's goal: pages.txt is a list of local files, "pages" is no reason to fetch them
        let local = line("pages.txt lists 24 local files. For each page, find the title and report which are missing one.", dir.path(), &specs).expect("a line");
        assert!(!local.contains("`web`") && !local.contains("'web'"), "local files, no web: {local}");
        assert!(names_a_call_that_runs(&local, &specs).is_ok(), "{local}");
        // a file of commands: `run` has one argument, so `each` runs as printed
        let run = line("cmds.txt has 24 shell commands. For each one, run it and say which fail.", dir.path(), &specs).expect("a line");
        assert!(run.contains("j4n.recipe('each', tool='run', items='cmds.txt')"), "{run}");
        assert_eq!(names_a_call_that_runs(&run, &specs), Ok(1), "{run}");
        // a folder of files, a list in the goal and a recall: every form the line takes is a call that runs
        for goal in [
            "For each page under docs/, say what it is about.",
            "pages.txt: for every page, read it.",
            "For each of the 24 lines of pages.txt, find it in the memory.",
            "For each of a.md, b.md, c.md, d.md, e.md, f.md, g.md and h.md, read the page.",
            "For every file you changed under docs/, run the tests.",
        ] {
            if let Some(l) = line(goal, dir.path(), &specs) {
                assert!(names_a_call_that_runs(&l, &specs).is_ok(), "{goal}: {l}");
                assert!(!l.contains("'web'") && !l.contains("`web`"), "{goal}: {l}");
            }
        }
    }
    // a file of URLs may name `web`; the real `web` takes q or url, so `each` is not printed for it and the many form is
    let specs = real_specs();
    let web = line("urls.txt lists 24 pages. For each page, fetch it and give the title.", dir.path(), &specs).expect("a line");
    assert!(web.contains("`web`") && !web.contains("j4n.recipe('each'"), "{web}");
    assert_eq!(names_a_call_that_runs(&web, &specs), Ok(1), "{web}");
    // a tool of three required arguments gets the many form too
    let edit = line("pages.txt lists 24 local files. For each, edit the text.", dir.path(), &specs).expect("a line");
    assert!(!edit.contains("j4n.recipe('each'"), "{edit}");
    assert!(names_a_call_that_runs(&edit, &specs).is_ok(), "{edit}");
}

/// A program with a shell of its own that prints the prompt it was given (up to `#END` is its code), and a model at `mock`.
fn rows_for(mock_url: &str) -> String {
    format!(
        r#"
[[connector]]
id = "builtin"
doc = "The tools J4n carries."
is = "tool"
dialect = "builtin"

[[connector]]
id = "own"
doc = "A model."
is = "agent"
dialect = "openai"
url = "{mock_url}/v1"
keyless = true
tools = ["builtin", "*", "ask", "py"]
models = ["m"]

[[connector]]
id = "wide"
doc = "A program with a shell of its own."
is = "agent"
dialect = "text"
program = "python3"
start = ["-I", "-c", "import sys; exec(sys.argv[1].split('#END')[0])", "{{prompt}}"]
"#
    )
}

fn connectors(mock_url: &str, kit: bool) -> Vec<Arc<dyn Doer>> {
    use j4n_connect::manifest::Connectors;
    use j4n_core::table::{Layer, LayerKind, SourceFile};
    let user = Layer::new(
        LayerKind::User,
        "mine",
        vec![SourceFile { path: "connectors/x.toml".into(), text: format!("api = 1\n{}", rows_for(mock_url)) }],
    );
    let (tables, problems) = Tables::load(vec![base_layer(), user]);
    assert!(problems.is_empty(), "{problems:?}");
    let (mut list, said) = Connectors::load(&tables);
    assert!(said.is_empty(), "{said:?}");
    list.set_kit(kit);
    list.set_gate(gate());
    let (doers, problems) = list.doers(&|_, _| Err("no secret".into()));
    assert!(problems.is_empty(), "{problems:?}");
    doers
}

#[test]
fn the_line_goes_after_the_goal_for_a_doer_with_a_shell_and_for_a_model_and_not_while_the_kit_is_off() {
    use crate::mock::{Mock, Say};
    if !python_here() {
        return;
    }
    let reply = || {
        Say::Sse(vec![
            json!({ "choices": [{ "delta": { "content": "done" } }] }).to_string(),
            json!({ "choices": [{ "delta": {}, "finish_reason": "stop" }] }).to_string(),
        ])
    };
    let goal = "import sys\nprint(sys.argv[1])  # bugs.txt: for each line find it in the memory\n#END";
    let plain = "import sys\nprint(sys.argv[1])  # nothing to loop over\n#END";
    for kit in [true, false] {
        let mock = Mock::start(move |_, _| reply());
        let rig = Rig::over(|_| connectors(&mock.url(), kit), Arc::default(), |_, _, _| {});
        rig.project.put("bugs.txt", &lines(24));
        let ask = |doer: &str, goal: &str| {
            let id = rig.spawn_for(doer, goal, Mode::Write, "here");
            let row = rig.finished(&id);
            (
                row.answer.unwrap_or_default(),
                rig.events("run.notice")
                    .iter()
                    .filter_map(|e| e.data["text"].as_str().map(String::from))
                    .collect::<Vec<_>>(),
            )
        };
        let (said, notices) = ask("wide", goal);
        let (nothing, _) = ask("wide", plain);
        let _ = ask("own", "bugs.txt: for each line find it in the memory");
        let sent = mock.seen().iter().map(|s| s.body.clone()).collect::<String>();
        if kit {
            let (g, o, c) = (
                said.find("for each line").unwrap_or(usize::MAX),
                said.find("J4n: this task has 24 items in bugs.txt").unwrap_or(usize::MAX),
                said.find("import j4n").unwrap_or(usize::MAX),
            );
            assert!(g < o && o < c, "the goal, then the offer line, then the card: {said}");
            assert!(!nothing.contains("J4n: this task has"), "a goal that is no loop gets no line: {nothing}");
            assert!(
                notices.iter().any(|n| n.starts_with("kit: J4n: this task has 24 items")),
                "the Log says it: {notices:?}"
            );
            assert!(
                sent.contains("J4n: this task has 24 items in bugs.txt"),
                "and the model's first text has it: {sent}"
            );
        } else {
            assert!(!said.contains("J4n: this task has") && !said.contains("import j4n"), "kit off, no line: {said}");
            assert!(!sent.contains("J4n: this task has"), "kit off, the model gets none: {sent}");
        }
    }
}

// ---- the sentence of `brief.plain_answer` rides the wrapper, kit or no kit ----

fn plain_rig(brief: bool, kitted: bool) -> Rig {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s = seen.clone();
    Rig::over(
        move |py| {
            let inner: Arc<dyn Doer> = Arc::new(Shell { py: py.clone(), play: Arc::new(|_| "ok".into()), seen: s, brief });
            vec![if kitted { Arc::new(Kitted::new(inner, py.clone())) as Arc<dyn Doer> } else { Arc::new(Kitted::plain(inner)) }]
        },
        seen,
        |_, _, _| {},
    )
}

fn turn_marked(r: &Rig, goal: &str, marked: bool) -> String {
    let mut spec = TaskSpec::new(goal, "shell");
    spec.input = if marked { json!({ "plain_answer": true }) } else { Value::Null };
    let id = r.tasks.spawn(spec, Actor::Operator).expect("spawns");
    r.finished(&id);
    r.seen.lock().unwrap().last().cloned().unwrap_or_default()
}

#[test]
fn a_marked_answer_turn_ends_with_the_sentence_for_a_doer_that_takes_no_brief_and_other_turns_are_untouched() {
    let r = plain_rig(false, false);
    assert_eq!(turn_marked(&r, "explain the loop", true), format!("explain the loop\n\n{}", j4n_core::brief::PLAIN), "no kit is needed");
    assert_eq!(turn_marked(&r, "explain the loop", false), "explain the loop", "a turn the core did not mark is as it was");
    // a doer that takes a Brief has the sentence in its contract: the wrapper adds nothing
    let r = plain_rig(true, false);
    assert_eq!(turn_marked(&r, "explain the loop", true), "explain the loop");
}

#[test]
fn with_the_kit_the_sentence_is_a_second_part_after_the_card() {
    if !python_here() {
        return;
    }
    let r = plain_rig(false, true);
    let text = turn_marked(&r, "explain the loop", true);
    assert!(text.starts_with("explain the loop") && text.ends_with(j4n_core::brief::PLAIN), "{text}");
    assert!(text.contains("j4n py"), "the card is there as before: {text}");
    assert!(!turn_marked(&r, "explain the loop", false).contains(j4n_core::brief::PLAIN));
}
