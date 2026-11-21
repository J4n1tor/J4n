//! The tool `py` on its own. A kernel is one Python process for a task that
//! keeps its variables; a cell past its time is stopped and the kernel is made again, and the agent is told; behind a
//! box the network is cut and the host is read-only; every cell is one Gate call, `read` when `pyread` reads it.
//! The ones that need the real wall run where `bwrap` does; they need `python3`, which every machine that runs J4n's tests has.

use crate::bench::base_layer;
use crate::world::Scratch;
use j4n_connect::model::Call;
use j4n_connect::model::doer::{ToolOut, Tools};
use j4n_connect::place::boxed::{self, BEHIND};
use j4n_connect::place::Boxed;
use j4n_connect::py::PyTools;
use j4n_core::gate::check::Gates;
use j4n_core::gate::grant::Grants;
use j4n_core::gate::{Gate, ReadOnly};
use j4n_core::place::checkpoint::Cfg;
use j4n_core::place::{Here, Launch, Place, Prepare, Site};
use j4n_core::setting::Settings;
use j4n_core::table::{Layer, LayerKind, SourceFile, Tables};
use j4n_core::task::{Mode, TaskSpec};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn call(args: Value) -> Call {
    Call { id: "c1".into(), name: "py".into(), args, bad: None }
}

fn cell(t: &PyTools, site: &Site, code: &str) -> ToolOut {
    t.run(&call(json!({ "code": code })), site, &|l| l)
}

fn site(dir: &Scratch, task: &str) -> Site {
    Site::new(task, dir.path())
}

fn gate() -> Gate {
    let (tables, _) = Tables::load(vec![base_layer()]);
    Gate::new(Gates::load(&tables).0, Grants::new("s1"))
}

fn bwrap_works() -> bool {
    std::process::Command::new("bwrap").args(["--ro-bind", "/", "/", "true"]).output().is_ok_and(|o| o.status.success())
}

#[test]
fn a_variable_set_in_one_cell_is_read_in_the_next_and_no_second_process_starts() {
    let dir = Scratch::new("py460");
    let (t, s) = (PyTools::new(), site(&dir, "t1"));
    let first = cell(&t, &s, "x = 2");
    assert!(!first.error, "{}", first.text);
    let second = cell(&t, &s, "x * 21");
    assert!(!second.error && second.text.contains("=> 42"), "{}", second.text);
    // one process: the same pid answers, and the pool holds one live kernel
    let pid = |code: &str| cell(&t, &s, code).text.lines().find_map(|l| l.strip_prefix("=> ").map(String::from)).unwrap_or_default();
    let (a, b) = (pid("import os\nos.getpid()"), pid("import os\nos.getpid()"));
    assert!(!a.is_empty() && a == b, "{a} {b}");
    assert_eq!(t.alive(), 1);
    // another task has a kernel of its own, with no x
    let other = cell(&t, &site(&dir, "t2"), "x");
    assert!(other.error && other.text.contains("NameError"), "{}", other.text);
    assert_eq!(t.alive(), 2);
    // what the cell printed and what it came to are both there, and the Log is given the cell
    let shown = cell(&t, &s, "print('hello')\n[1, 2]");
    assert!(shown.text.contains("hello") && shown.text.contains("=> [1, 2]"), "{}", shown.text);
    let ev = shown.py.expect("a py.cell");
    assert_eq!((ev["risk"].as_str(), ev["ok"].as_bool(), ev["code"].as_str()), (Some("read"), Some(true), Some("print('hello')\n[1, 2]")));
    assert!(ev["hash"].as_str().is_some_and(|h| h.len() == 16) && ev["head"].as_str() == Some("print('hello')\n[1, 2]"), "{ev}");
}

#[test]
fn an_error_is_the_cells_result_and_the_kernel_goes_on() {
    let dir = Scratch::new("py-err");
    let (t, s) = (PyTools::new(), site(&dir, "t1"));
    cell(&t, &s, "keep = 7");
    let bad = cell(&t, &s, "print('before')\n1 / 0");
    assert!(bad.error && bad.text.contains("before") && bad.text.contains("error: ZeroDivisionError") && bad.text.contains("cell line 2"), "{}", bad.text);
    assert!(bad.py.as_ref().is_some_and(|e| e["ok"] == false));
    let syntax = cell(&t, &s, "def f(:");
    assert!(syntax.error && syntax.text.contains("SyntaxError"), "{}", syntax.text);
    let exit = cell(&t, &s, "raise SystemExit(3)");
    assert!(exit.error, "a cell's exit does not end the kernel: {}", exit.text);
    assert!(cell(&t, &s, "keep").text.contains("=> 7"));
    assert!(cell(&t, &s, "").error, "a cell with no code is said so");
}

#[test]
fn a_cell_past_its_time_is_stopped_the_kernel_restarts_and_the_agent_is_told() {
    let dir = Scratch::new("py461");
    let (t, s) = (PyTools::new(), site(&dir, "t1"));
    cell(&t, &s, "x = 1");
    let began = Instant::now();
    let slow = t.run(&call(json!({ "code": "import time\ntime.sleep(60)", "timeout": 1 })), &s, &|l| l);
    assert!(began.elapsed() < Duration::from_secs(20), "stopped at its time, not at the end of the sleep: {:?}", began.elapsed());
    assert!(slow.error && slow.text.contains("ran past 1 s") && slow.text.contains("variables of earlier cells are gone"), "{}", slow.text);
    assert_eq!(slow.py.as_ref().map(|e| e["ok"].clone()), Some(json!(false)));
    assert_eq!(t.alive(), 0, "the kernel that ran past its time is down");
    // the next cell finds a new kernel, is told so, and x is gone; the one after is plain
    let next = cell(&t, &s, "x");
    assert!(next.error && next.text.starts_with("[the kernel was restarted (it was stopped)") && next.text.contains("NameError"), "{}", next.text);
    let after = cell(&t, &s, "y = 3\ny");
    assert!(!after.text.contains("restarted") && after.text.contains("=> 3"), "{}", after.text);
    assert_eq!(t.alive(), 1);
}

#[test]
fn restart_and_a_kernel_that_ended_say_the_variables_are_gone() {
    let dir = Scratch::new("py461b");
    let (t, s) = (PyTools::new(), site(&dir, "t1"));
    cell(&t, &s, "x = 1");
    let fresh = t.run(&call(json!({ "code": "", "restart": true })), &s, &|l| l);
    assert!(!fresh.error && fresh.text.contains("restarted (you asked for it)"), "{}", fresh.text);
    assert!(cell(&t, &s, "x").text.contains("NameError"));
    // a cell that kills the kernel is answered with that, and the next one is on a new kernel
    cell(&t, &s, "z = 1");
    let dead = cell(&t, &s, "import os\nos._exit(3)");
    assert!(dead.error && dead.text.contains("the kernel ended") && dead.text.contains("variables of earlier cells are gone"), "{}", dead.text);
    let next = cell(&t, &s, "z");
    assert!(next.text.contains("[the kernel was restarted") && next.text.contains("NameError"), "{}", next.text);
}

#[test]
fn the_output_and_the_value_are_cut_and_the_data_stays_in_the_kernel() {
    let dir = Scratch::new("py-cap");
    let (t, s) = (PyTools::new(), site(&dir, "t1"));
    let big = cell(&t, &s, "data = list(range(100000))\nprint('x' * 10000)\ndata");
    assert!(big.text.contains("chars cut") && big.text.chars().count() < 10_000, "{} chars", big.text.chars().count());
    assert!(cell(&t, &s, "len(data)").text.contains("=> 100000"), "the data is still there");
}

#[test]
fn the_kernel_is_given_a_path_and_a_language_and_no_other_variable_of_j4ns() {
    let dir = Scratch::new("py-env");
    let (t, s) = (PyTools::new(), site(&dir, "t1"));
    let env = cell(&t, &s, "import os\nsorted(os.environ)");
    assert!(env.text.contains("=> ['LANG', 'PATH']"), "{}", env.text);
    // the protocol's descriptors are not the cell's: a write to fd 1 is nowhere, and a read finds the end
    let fds = cell(&t, &s, "import os\nos.write(1, b'{garbage}\\n')\n5");
    assert!(fds.text.contains("=> 5"), "{}", fds.text);
    assert!(cell(&t, &s, "input()").text.contains("EOFError"));
    assert!(cell(&t, &s, "1 + 1").text.contains("=> 2"), "the protocol is whole");
}

#[test]
fn a_kernel_idle_for_the_time_the_pool_keeps_it_is_stopped() {
    let dir = Scratch::new("py-idle");
    let (t, s) = (PyTools::new().idle(Duration::from_millis(300)), site(&dir, "t1"));
    cell(&t, &s, "x = 1");
    assert_eq!(t.alive(), 1);
    let end = Instant::now() + Duration::from_secs(20);
    while t.alive() > 0 && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(t.alive(), 0, "the reaper took it down");
    let next = cell(&t, &s, "x");
    assert!(next.text.contains("NameError"), "{}", next.text);
}

fn spec_of(t: &PyTools) -> Vec<String> {
    t.specs().iter().map(|s| s.name.clone()).collect()
}

#[test]
fn the_tool_is_py_with_a_code_a_timeout_and_a_restart() {
    let t = PyTools::new();
    assert_eq!(spec_of(&t), ["py"]);
    let schema = &t.specs()[0].schema;
    assert_eq!((schema["required"].clone(), schema["properties"].as_object().map(|p| p.keys().cloned().collect::<Vec<_>>())), (json!(["code"]), Some(vec!["code".to_string(), "restart".into(), "timeout".into()])));
}

// ---- each cell is one Gate call ----

fn gated(t: &PyTools, code: &str, read_only: ReadOnly) -> Option<(u8, String)> {
    let dir = Scratch::new("py463");
    t.gate(&call(json!({ "code": code })), "t1", &site(&dir, "t1"), read_only).map(|g| (g.risk, g.question))
}

#[test]
fn a_cell_pyread_reads_passes_a_read_only_mode_and_one_that_writes_is_asked_for() {
    let t = PyTools::new().with_gate(gate());
    let read = "import math\nvalues = [math.sqrt(n) for n in range(10)]\nsum(values)";
    let write = "open('out.txt', 'w').write('x')";
    // read: nothing is asked in a read-only task, explicit or only guessed from its words
    assert_eq!(gated(&t, read, ReadOnly::Explicit), None);
    assert_eq!(gated(&t, read, ReadOnly::Inferred), None);
    // exec: an explicit read-only step blocks it, a guessed one asks, a task that may write is not asked
    let blocked = gated(&t, write, ReadOnly::Explicit).expect("blocked");
    assert_eq!(blocked.0, 100);
    assert!(blocked.1.contains("open('out.txt'") && !blocked.1.contains("only reads"), "{}", blocked.1);
    let asked = gated(&t, write, ReadOnly::Inferred).expect("asked");
    assert!((1..100).contains(&asked.0), "{asked:?}");
    assert_eq!(gated(&t, write, ReadOnly::No), None);
    // the Gate reads the cell as the line it is: a cell that names a credentials file is refused whoever asks
    let secret = gated(&t, "open('/home/me/.ssh/id_rsa').read()", ReadOnly::No).expect("a secret is not read by a cell");
    assert_eq!(secret.0, 100);
    // with no rules loaded, everything but the cell pyread reads is asked
    let bare = PyTools::new();
    assert!(gated(&bare, write, ReadOnly::No).is_some());
    // another tool is none of its business
    let dir = Scratch::new("py463b");
    let other = Call { id: "c".into(), name: "run".into(), args: json!({ "cmd": "ls" }), bad: None };
    assert!(t.gate(&other, "t1", &site(&dir, "t1"), ReadOnly::No).is_none());
}

#[test]
fn a_cell_is_risk_read_or_exec_in_its_event() {
    let dir = Scratch::new("py463c");
    let (t, s) = (PyTools::new(), site(&dir, "t1"));
    let risk = |code: &str| cell(&t, &s, code).py.unwrap()["risk"].as_str().unwrap().to_string();
    assert_eq!(risk("sum(range(10))"), "read");
    assert_eq!(risk("open('f.txt', 'w').write('x')"), "exec");
    assert_eq!(dir.read("f.txt"), "x", "not in a box, a cell writes where the task works");
    assert_eq!(risk("import subprocess\nsubprocess.run(['true'])"), "exec");
}

// ---- behind a box ----

fn boxed_place(w: &Scratch, h: &Scratch) -> Boxed {
    let rows = Layer::new(LayerKind::Pack, "boxes", vec![SourceFile { path: "boxes/b.toml".into(), text: include_str!("../../../../packs/base/boxes/bwrap.toml").into() }]);
    let (tables, problems) = Tables::load(vec![rows]);
    assert!(problems.is_empty(), "{problems:?}");
    let (walls, problems) = boxed::rows(&tables);
    assert!(problems.is_empty(), "{problems:?}");
    // box.net is on, as it ships: the kernel's own need for no network is what cuts it
    Boxed::new("box", Arc::new(Here::new(h.path(), w.path(), Cfg::default())), walls, &Settings::from_pairs([]), h.path(), &h.join(".j4n"))
}

#[test]
fn behind_a_box_a_socket_fails_a_write_outside_the_folder_fails_and_ssh_is_empty() {
    if !bwrap_works() {
        return;
    }
    let (w, h, outside) = (Scratch::new("py462-w"), Scratch::new("py462-h"), Scratch::new("py462-o"));
    h.put(".ssh/id_ed25519", "not a real key");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let socket = format!("import socket\ntry:\n    socket.create_connection(('127.0.0.1', {port}), 3).close(); r = 'open'\nexcept OSError as e:\n    r = 'closed ' + type(e).__name__\nr");
    // not behind a box, the same cell reaches the listener: the wall is what cuts it
    let open = PyTools::new();
    let loose = cell(&open, &site(&w, "t0"), &socket);
    assert!(loose.text.contains("=> 'open'") && loose.text.contains("not behind a box"), "{}", loose.text);
    assert_eq!(loose.py.unwrap()["walled"], false);
    let place = boxed_place(&w, &h);
    let mut spec = TaskSpec::new("go", "d");
    spec.tune.mode = Mode::Write;
    let s = place.prepare(&Prepare { task: "t1", spec: &spec, root: w.path() }).expect("a site behind the wall");
    assert!(s.notes.iter().any(|n| n.starts_with(BEHIND)), "{:?}", s.notes);
    let wrap = |l: Launch| place.wrap(&s, l);
    let t = PyTools::new();
    let run = |code: &str| t.run(&call(json!({ "code": code })), &s, &wrap);
    let shut = run(&socket);
    assert!(shut.text.contains("=> 'closed ") && !shut.text.contains("not behind a box"), "{}", shut.text);
    assert_eq!(shut.py.unwrap()["walled"], true);
    // the task's folder is writable, the rest of the host is not, and the home's ~/.ssh is an empty folder
    let inside = run("open('made.txt', 'w').write('in')\n'wrote'");
    assert!(inside.text.contains("=> 'wrote'") && w.read("made.txt") == "in", "{}", inside.text);
    let away = format!("try:\n    open({:?}, 'w').write('out'); r = 'wrote'\nexcept OSError as e:\n    r = 'failed ' + type(e).__name__\nr", outside.join("x.txt").display().to_string());
    let denied = run(&away);
    assert!(denied.text.contains("=> 'failed ") && !outside.join("x.txt").exists(), "{}", denied.text);
    let ssh = run(&format!("import os\nos.listdir({:?})", h.join(".ssh").display().to_string()));
    assert!(ssh.text.contains("=> []"), "{}", ssh.text);
    // and it is still one kernel with its variables
    run("kept = 9");
    assert!(run("kept").text.contains("=> 9"));
    assert_eq!(t.alive(), 1);
}

// ---- the library `j4n` ----

#[test]
fn a_cell_imports_j4n_with_no_file_in_the_box_and_calls_its_code_layer() {
    let dir = Scratch::new("py464");
    let (t, s) = (PyTools::new(), site(&dir, "t1"));
    let first = cell(&t, &s, "import j4n\nj4n.chunks('aaaa\\n\\nbbbb\\n\\ncccc', 10)");
    assert!(!first.error && first.text.contains(r"=> ['aaaa\n\nbbbb', 'cccc']"), "{}", first.text);
    // the module is the one J4n sent: no file of it is anywhere, and its recipes are the pack's files
    let src = cell(&t, &s, "import sys\nsys.modules['j4n'].__file__");
    assert!(src.text.contains("=> '<j4n>'"), "{}", src.text);
    let mut names: Vec<String> = std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../packs/kit/recipes")).unwrap().flatten().map(|e| e.path().file_stem().unwrap().to_string_lossy().into_owned()).collect();
    names.sort();
    let listed = cell(&t, &s, "sorted(j4n.recipes())");
    assert!(listed.text.contains(&format!("=> {names:?}").replace('"', "'")), "{} vs {names:?}", listed.text);
    let ran = cell(&t, &s, "j4n.recipe('column_counts', source='a,b\\n1,x\\n1,y')");
    assert!(ran.text.contains(r#"{'rows': 2, 'columns': {'a': [['1', 2]], 'b': [['x', 1], ['y', 1]]}}"#), "{}", ran.text);
    // a kernel with no host to ask says so (`kit_cases` has the host), and goes on
    let later = cell(&t, &s, "j4n.judge(['x'], 'q')");
    assert!(later.error && later.text.contains("no host calls"), "{}", later.text);
    assert_eq!(t.alive(), 1);
}

#[test]
fn a_cell_that_only_computes_with_j4n_is_a_read_and_one_that_fetches_is_not() {
    let t = PyTools::new().with_gate(gate());
    let code = "import j4n\nrows = j4n.table(text)\nj4n.grep(rows, 'open')";
    assert_eq!(gated(&t, code, ReadOnly::Explicit), None, "a read-only task may cut and search what it was given");
    assert!(gated(&t, "import j4n\nj4n.fetch('x')", ReadOnly::Explicit).is_some());
    // the decider's questions are a read (their own switch and the egress Gate hold them), the raw channel to J4n is not
    assert_eq!(gated(&t, "import j4n\np = j4n.judge(items, 'q')\nj4n.keep(p)", ReadOnly::Explicit), None);
    assert!(gated(&t, "import j4n\nj4n.host('fetch', url='x')", ReadOnly::Explicit).is_some());
    assert!(gated(&t, "import j4n\nj4n._channel('fetch', {})", ReadOnly::Explicit).is_some());
}
