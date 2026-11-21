//! The kit through the binary's own wiring: with the setting `kit.mode` off there is no tool
//! `py`; on, the own agent is offered it after the builtin ones, a cell keeps its variables for the task, every cell is one Gate call
//! and one `py.cell` event, and a read-only task passes the cell `pyread` reads and is refused the one that writes.

use crate::models_cases::mock::{Mock, Say, Seen};
use crate::run_cases::Dirs;
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use j4n_core::front::Said;
use j4n_core::task::{Mode, Row, State};
use serde_json::{Value, json};
use std::fs;
use std::time::Duration;

fn open(d: &Dirs, gateway: &Mock, kit: &str, mode: Mode) -> World {
    let key = d.dir.join("key");
    crate::support::key_file(&key, "sk-test-own-0001\n");
    fs::create_dir_all(d.j4n_home().join("connectors")).unwrap();
    let mine = format!("api = 1\n[[connector]]\nid = \"own\"\nurl = \"{}/v1\"\nkey = \"file:{}\"\nmodels = [\"vendor/m\"]\n", gateway.url(), key.display());
    fs::write(d.j4n_home().join("connectors/mine.toml"), mine).unwrap();
    if !kit.is_empty() {
        fs::write(d.j4n_home().join("settings/kit.toml"), format!("api = 1\n[[setting]]\nid = \"kit.mode\"\nvalue = \"{kit}\"\n")).unwrap();
    }
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: "claude".into() };
    let opts = Opts { doer: Some("own".into()), model: None, approval: Some(Approval::Deny), wall: None, usd: None, mode, place: None, shape: false, anywhere: false };
    let w = wire::open(setup, &opts, false, "main").unwrap_or_else(|e| panic!("opens: {e}"));
    assert!(w.notices.is_empty(), "{:?}", w.notices);
    w
}

fn calls(list: &[(&str, &str, Value)]) -> Say {
    let frags: Vec<Value> = list.iter().enumerate().map(|(i, (id, name, args))| json!({ "index": i, "id": id, "function": { "name": name, "arguments": args.to_string() } })).collect();
    Say::Sse(vec![
        json!({ "choices": [{ "delta": { "tool_calls": frags } }] }).to_string(),
        json!({ "choices": [{ "delta": {}, "finish_reason": "tool_calls" }] }).to_string(),
        json!({ "choices": [], "usage": { "prompt_tokens": 100, "completion_tokens": 10 } }).to_string(),
    ])
}

fn answer(text: &str) -> Say {
    Say::Sse(vec![
        json!({ "choices": [{ "delta": { "content": text } }] }).to_string(),
        json!({ "choices": [{ "delta": {}, "finish_reason": "stop" }] }).to_string(),
        json!({ "choices": [], "usage": { "prompt_tokens": 100, "completion_tokens": 10 } }).to_string(),
    ])
}

fn line(w: &World, text: &str) -> Row {
    let Said::Started { task } = w.front.say("main", text) else { panic!("{text:?} did not start a task") };
    w.front.tasks().book().wait(&task, |r| r.state.is_terminal(), crate::support::PATIENCE).unwrap_or_else(|| panic!("{task} never ended\n{}", dump(w)))
}

fn dump(w: &World) -> String {
    w.front.log().read(0).map(|e| format!("{:>4} {:<16} {} {}", e.seq, e.kind, e.task.as_deref().unwrap_or("-"), e.data)).collect::<Vec<_>>().join("\n")
}

fn events(w: &World, kind: &str) -> Vec<Value> {
    w.front.log().read(0).filter(|e| e.kind == kind).map(|e| e.data).collect()
}

fn results(s: &Seen) -> Vec<String> {
    s.json()["messages"].as_array().cloned().unwrap_or_default().iter().filter(|m| m["role"] == "tool").map(|m| m["content"].as_str().unwrap_or_default().to_string()).collect()
}

fn tools_of(s: &Seen) -> Vec<String> {
    s.json()["tools"].as_array().unwrap().iter().map(|t| t["function"]["name"].as_str().unwrap().to_string()).collect()
}

fn python_here() -> bool {
    j4n_connect::py::python_here()
}

#[test]
fn with_kit_off_there_is_no_py_tool_and_a_call_of_it_finds_none() {
    let d = Dirs::new("kit-off");
    let gateway = Mock::start(|_, n| if n == 0 { calls(&[("c1", "py", json!({ "code": "1 + 1" }))]) } else { answer("done") });
    let w = open(&d, &gateway, "", Mode::Write);
    line(&w, "add");
    let seen = gateway.seen();
    assert_eq!(tools_of(&seen[0]), ["list", "read", "grep", "edit", "write", "run", "git", "ask", "web"], "the same tools as before the kit");
    assert!(results(&seen[1])[0].contains("no tool `py`") || results(&seen[1])[0].contains("not run"), "{:?}", results(&seen[1]));
    assert!(events(&w, "py.cell").is_empty() && events(&w, "run.shell").is_empty());
    // naming it in the setting as `off`, or in a word that is not `on`, is the same
    for word in ["off", "ON?"] {
        let d = Dirs::new("kit-off2");
        let gateway = Mock::start(|_, _| answer("done"));
        let w = open(&d, &gateway, word, Mode::Write);
        line(&w, "hi");
        assert!(!tools_of(&gateway.seen()[0]).contains(&"py".to_string()), "{word}");
        w.close();
    }
    w.close();
}

#[test]
fn with_kit_on_a_cell_keeps_its_variable_and_each_cell_is_one_event_in_the_log() {
    if !python_here() {
        return;
    }
    let d = Dirs::new("kit-on");
    let gateway = Mock::start(|_, n| match n {
        0 => calls(&[
            ("c1", "py", json!({ "code": "x = 2" })),
            ("c2", "py", json!({ "code": "x * 21" })),
            ("c3", "py", json!({ "code": "open('made.txt', 'w').write('from py')" })),
        ]),
        _ => answer("It is 42."),
    });
    let w = open(&d, &gateway, "on", Mode::Write);
    let row = line(&w, "compute");
    assert!(matches!(row.state, State::Done { ok: true }), "{:?}\n{}", row.state, dump(&w));
    let seen = gateway.seen();
    assert_eq!(tools_of(&seen[0]), ["list", "read", "grep", "edit", "write", "run", "git", "py", "ask", "web"]);
    let r = results(&seen[1]);
    assert_eq!(r.len(), 3);
    assert!(r[1].contains("=> 42"), "{r:?}");
    assert_eq!(fs::read_to_string(d.project().join("made.txt")).unwrap(), "from py");
    // one event for each cell, with its risk; the code is in a blob the event names
    let cells = events(&w, "py.cell");
    assert_eq!(cells.iter().map(|c| c["risk"].as_str().unwrap()).collect::<Vec<_>>(), ["read", "read", "exec"], "{cells:?}");
    assert!(cells.iter().all(|c| c["ok"] == true && c["hash"].as_str().is_some()), "{cells:?}");
    let blob = cells[2]["blob"].as_str().expect("the code is in a blob");
    assert_eq!(String::from_utf8(w.front.log().blobs().get(blob).unwrap()).unwrap(), "open('made.txt', 'w').write('from py')");
    assert!(cells[2].get("code").is_none(), "the code is not in the line: {}", cells[2]);
    w.close();
}

#[test]
fn a_read_only_task_passes_the_cell_pyread_reads_and_is_refused_the_one_that_writes() {
    if !python_here() {
        return;
    }
    let d = Dirs::new("kit-read");
    let gateway = Mock::start(|_, n| match n {
        0 => calls(&[("c1", "py", json!({ "code": "import math\nmath.sqrt(49)" })), ("c2", "py", json!({ "code": "open('no.txt', 'w').write('x')" }))]),
        _ => answer("looked"),
    });
    let w = open(&d, &gateway, "on", Mode::Read);
    line(&w, "look");
    let r = results(&gateway.seen()[1]);
    assert!(r[0].contains("=> 7.0"), "{r:?}");
    assert!(r[1].contains("was not allowed"), "{r:?}");
    assert!(!d.project().join("no.txt").exists());
    // the refused cell never reached the kernel: one event, the one that ran
    assert_eq!(events(&w, "py.cell").len(), 1);
    w.close();
}

fn system_of(s: &Seen) -> String {
    s.json()["messages"].as_array().unwrap().iter().filter(|m| m["role"] == "system").map(|m| m["content"].as_str().unwrap_or_default().to_string()).collect::<Vec<_>>().join("\n")
}

#[test]
fn the_kit_card_is_in_the_system_text_with_kit_on_and_not_with_it_off() {
    if !python_here() {
        return;
    }
    for (word, told) in [("on", true), ("off", false), ("", false)] {
        let d = Dirs::new("kit-card");
        let gateway = Mock::start(|_, _| answer("hi"));
        let w = open(&d, &gateway, word, Mode::Write);
        line(&w, "add a note");
        let system = system_of(&gateway.seen()[0]);
        assert_eq!(system.contains("import j4n"), told, "kit.mode {word:?}: {system}");
        assert_eq!(tools_of(&gateway.seen()[0]).contains(&"py".to_string()), told, "the card goes with the tool");
        if told {
            assert!(system.contains("j4n.chunks") && system.contains("j4n.recipes()"), "{system}");
        }
        w.close();
    }
}

#[test]
fn a_cell_in_a_task_imports_j4n_and_cuts_a_text() {
    if !python_here() {
        return;
    }
    let d = Dirs::new("kit-import");
    let gateway = Mock::start(|_, n| match n {
        0 => calls(&[("c1", "py", json!({ "code": "import j4n\nlen(j4n.chunks('a\\n\\nb\\n\\nc', 1))" }))]),
        _ => answer("three"),
    });
    let w = open(&d, &gateway, "on", Mode::Read);
    line(&w, "cut");
    let r = results(&gateway.seen()[1]);
    assert!(r[0].contains("=> 3"), "{r:?}");
    assert_eq!(events(&w, "py.cell")[0]["risk"], "read", "a read-only task may use the library's code layer");
    w.close();
}

// ---- a doer with a shell of its own, and the notebook ----

/// A world whose doer `shell` is a program (`script`, a shell script): the kind of doer that runs a program of its own.
fn open_shell(d: &Dirs, kit: &str, script: &str) -> World {
    let prog = d.dir.join("bin/shell.sh");
    fs::write(&prog, format!("#!/bin/sh\n{script}\n")).unwrap();
    fs::set_permissions(&prog, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    fs::create_dir_all(d.j4n_home().join("connectors")).unwrap();
    fs::write(d.j4n_home().join("connectors/shell.toml"), format!("api = 1\n[[connector]]\nid = \"shell\"\nis = \"agent\"\ndialect = \"text\"\nprogram = \"{}\"\nstart = [\"{{prompt}}\"]\n", prog.display())).unwrap();
    if !kit.is_empty() {
        fs::write(d.j4n_home().join("settings/kit.toml"), format!("api = 1\n[[setting]]\nid = \"kit.mode\"\nvalue = \"{kit}\"\n")).unwrap();
    }
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: "claude".into() };
    let opts = Opts { doer: Some("shell".into()), model: None, approval: Some(Approval::Deny), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false };
    let w = wire::open(setup, &opts, false, "main").unwrap_or_else(|e| panic!("opens: {e}"));
    assert!(w.notices.is_empty(), "{:?}", w.notices);
    w
}

#[test]
fn the_command_j4n_py_in_a_doers_shell_runs_in_the_tasks_kernel_and_keeps_its_variables() {
    if !python_here() {
        return;
    }
    let d = Dirs::new("kit-shell");
    let j4n = env!("CARGO_BIN_EXE_j4n");
    let w = open_shell(&d, "on", &format!("printf 'PROMPT<<%s>>\\n' \"$1\"\n{j4n} py -c 'x = 41'\n{j4n} py -c 'x + 1'\n{j4n} py -c 'x + 1' --task nope; echo \"exit $?\""));
    let row = line(&w, "count the lines");
    assert!(matches!(row.state, State::Done { ok: true }), "{:?}\n{}", row.state, dump(&w));
    let said = row.answer.unwrap();
    // the doer was told, and its second cell read the variable its first one set
    assert!(said.contains("PROMPT<<count the lines") && said.contains("`j4n py -c '<code>'`"), "{said}");
    assert!(said.contains("=> 42"), "{said}");
    // a task's name that is not this one's has no socket here
    assert!(said.contains("exit 1"), "{said}");
    let cells = events(&w, "py.cell");
    assert_eq!(cells.iter().map(|c| (c["head"].as_str().unwrap(), c["via"].as_str().unwrap())).collect::<Vec<_>>(), [("x = 41", "j4n py"), ("x + 1", "j4n py")], "{}", dump(&w));
    assert!(cells.iter().all(|c| c["risk"] == "read" && c["ok"] == true), "{cells:?}");
    // the socket went with the run
    let end = std::time::Instant::now() + crate::support::PATIENCE;
    while fs::read_dir(d.project().join(".j4n/run")).map_or(0, Iterator::count) > 0 && std::time::Instant::now() < end {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(fs::read_dir(d.project().join(".j4n/run")).map_or(0, Iterator::count), 0);
    w.close();
}

#[test]
fn a_doer_with_a_shell_is_told_of_the_kit_only_with_kit_on() {
    if !python_here() {
        return;
    }
    for (word, told) in [("on", true), ("off", false), ("", false)] {
        let d = Dirs::new("kit-shell-card");
        let w = open_shell(&d, word, "printf 'PROMPT<<%s>>\\n' \"$1\"");
        let said = line(&w, "add a note").answer.unwrap();
        assert!(said.starts_with("PROMPT<<add a note"), "{said}");
        assert_eq!(said.contains("j4n py -c"), told, "kit.mode {word:?}: {said}");
        assert_eq!(d.project().join(".j4n/run").exists(), told, "the socket goes with the card (kit.mode {word:?})");
        w.close();
    }
}

/// The binary itself runs `go` as the script of a doer `shell` (kit.mode on) with a `j4n` of another version first on the PATH; its output.
fn run_shell(d: &Dirs, go: &str) -> crate::run_cases::Out {
    let prog = d.dir.join("bin/shell.sh");
    fs::write(&prog, format!("#!/bin/sh\n{go}\n")).unwrap();
    fs::set_permissions(&prog, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    fs::create_dir_all(d.j4n_home().join("connectors")).unwrap();
    fs::write(d.j4n_home().join("connectors/shell.toml"), format!("api = 1\n[[connector]]\nid = \"shell\"\nis = \"agent\"\ndialect = \"text\"\nprogram = \"{}\"\nstart = [\"{{prompt}}\"]\n", prog.display())).unwrap();
    fs::write(d.j4n_home().join("settings/kit.toml"), "api = 1\n[[setting]]\nid = \"kit.mode\"\nvalue = \"on\"\n").unwrap();
    let decoy = d.dir.join("decoy");
    fs::create_dir_all(&decoy).unwrap();
    fs::write(decoy.join("j4n"), "#!/bin/sh\necho DECOY\n").unwrap();
    fs::set_permissions(decoy.join("j4n"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let path = format!("{}:{}", decoy.display(), std::env::var("PATH").unwrap_or_default());
    crate::run_cases::j4n(d, &["run", "--doer", "shell", "go"], None, &[("PATH", path.as_str())])
}

#[test]
fn a_doers_shell_finds_this_runs_j4n_over_the_socket_and_a_command_that_opens_the_log_says_what_works() {
    if !python_here() {
        return;
    }
    let d = Dirs::new("kit-path");
    let o = run_shell(&d, "command -v j4n\nj4n py -c 'print(6 * 7)'\nj4n tools 2>&1; echo \"tools exit $?\"\necho \"in task: $J4N_IN_TASK\"\nls $(dirname $(command -v j4n))");
    assert_eq!(o.rc, 0, "{}", o.err);
    // this run's own binary, first, whatever `j4n` the PATH held before; the folder holds the link and nothing else
    assert!(o.out.contains(".j4n/run/") && o.out.contains(".bin/j4n"), "{}", o.out);
    assert!(!o.out.contains("DECOY"), "an earlier j4n on the PATH must not win: {}", o.out);
    assert!(o.out.contains("42"), "j4n py ran over the socket with no install: {}", o.out);
    assert!(o.out.trim_end().ends_with("j4n"), "only the link is in its folder: {}", o.out);
    // no second writer on the Log, no notice of one; the command says what works here
    assert!(o.out.contains("tools exit 2") && o.out.contains("held by the running J4n"), "{}", o.out);
    assert!(!o.out.contains("another J4n has this log open") && !o.err.contains("another J4n"), "{}\n{}", o.out, o.err);
    assert!(o.out.contains("in task: "), "{}", o.out);
    // the link and the folder went with the run
    assert_eq!(fs::read_dir(d.project().join(".j4n/run")).map_or(0, Iterator::count), 0);
}

/// The build J4n was started from is removed while it runs, as a rebuild does: a doer's `j4n py` in the next task still runs, through a copy of
/// J4n's own image in the task's folder (another process is refused the image itself once J4n is not dumpable). The image is read through
/// `/proc/self/exe`, which only Linux has: on macOS a `j4n py` after its build is removed fails.
#[cfg(target_os = "linux")]
#[test]
fn a_doers_j4n_py_runs_after_the_build_j4n_was_started_from_is_removed() {
    use std::io::Write as _;
    if !python_here() {
        return;
    }
    let d = Dirs::new("kit-gone");
    let build = d.dir.join("build/j4n");
    fs::create_dir_all(build.parent().unwrap()).unwrap();
    fs::copy(env!("CARGO_BIN_EXE_j4n"), &build).unwrap();
    let prog = d.dir.join("bin/shell.sh");
    fs::write(&prog, format!("#!/bin/sh\ncase \"$1\" in\n*run-the-cell*) j4n py -c 'print(6 * 7)';;\n*) rm -f '{}'; echo removed;;\nesac\n", build.display())).unwrap();
    fs::set_permissions(&prog, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    fs::create_dir_all(d.j4n_home().join("connectors")).unwrap();
    fs::write(d.j4n_home().join("connectors/shell.toml"), format!("api = 1\n[[connector]]\nid = \"shell\"\nis = \"agent\"\ndialect = \"text\"\nprogram = \"{}\"\nstart = [\"{{prompt}}\"]\n", prog.display())).unwrap();
    fs::write(d.j4n_home().join("settings/kit.toml"), "api = 1\n[[setting]]\nid = \"kit.mode\"\nvalue = \"on\"\n").unwrap();
    fs::create_dir_all(d.dir.join("tmp")).unwrap();
    let mut child = std::process::Command::new(&build)
        .args(["--doer", "shell", "--approval", "deny"])
        .current_dir(d.project())
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", d.home())
        .env("J4N_HOME", d.j4n_home())
        .env("TMPDIR", d.dir.join("tmp"))
        .env("https_proxy", "http://127.0.0.1:9")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("the build starts");
    child.stdin.take().unwrap().write_all(b"remove-the-build\nrun-the-cell\n").unwrap();
    let o = child.wait_with_output().expect("it ends");
    let (out, err) = (String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    assert!(!build.exists() && out.contains("removed"), "the first task removed the build: {out}\n{err}");
    assert!(out.contains("42"), "j4n py ran over the socket with the build gone: {out}\n{err}");
    assert_eq!(fs::read_dir(d.project().join(".j4n/run")).map_or(0, Iterator::count), 0, "the copy went with its task");
}

#[test]
fn a_command_that_would_open_the_tasks_own_log_is_refused_and_any_other_folder_runs_as_before() {
    let (d, other) = (Dirs::new("kit-in-task"), Dirs::new("kit-other"));
    fs::create_dir_all(d.project().join(".j4n")).unwrap();
    let task_log = d.project().join(".j4n").canonicalize().unwrap();
    let held = [(j4n_connect::kitsock::IN_TASK, task_log.to_str().unwrap())];
    for args in [&["tools"][..], &["run", "say ok"], &["answer"]] {
        let o = crate::run_cases::j4n(&d, args, None, &held);
        assert_eq!(o.rc, 2, "{args:?}: {}", o.err);
        assert!(o.err.contains("held by the running J4n"), "{args:?}: {}", o.err);
    }
    assert!(!d.project().join(".j4n/log/.lock").exists(), "the Log was not opened");
    // what needs no Log still runs there
    assert_eq!(crate::run_cases::j4n(&d, &["--version"], None, &held).rc, 0);
    assert_eq!(crate::run_cases::j4n(&d, &["answer", "--gate"], Some(""), &held).rc, 0);
    // another project, with the same variable inherited: its own Log, it runs
    let o = crate::run_cases::j4n(&other, &["run", "say ok", "--doer", "test"], None, &held);
    assert_eq!(o.rc, 0, "{}", o.err);
    assert!(other.project().join(".j4n/log/.lock").exists());
}

#[test]
fn kit_export_writes_the_tasks_cells_as_a_notebook_json_loads() {
    if !python_here() {
        return;
    }
    let d = Dirs::new("kit-export");
    let gateway = Mock::start(|_, n| match n {
        0 => calls(&[
            ("c1", "py", json!({ "code": "x = 2\nprint('hi')" })),
            ("c2", "py", json!({ "code": "x * 21" })),
            ("c3", "py", json!({ "code": "1 / 0" })),
        ]),
        _ => answer("done"),
    });
    let w = open(&d, &gateway, "on", Mode::Write);
    let row = line(&w, "compute");
    let cells = events(&w, "py.cell");
    assert_eq!(cells.len(), 3, "{}", dump(&w));
    let Said::Started { task } = w.front.say("main", ":kit export") else { panic!(":kit export did not start\n{}", dump(&w)) };
    let done = w.front.tasks().book().wait(&task, |r| r.state.is_terminal(), crate::support::PATIENCE).unwrap_or_else(|| panic!("{task} never ended\n{}", dump(&w)));
    assert!(matches!(done.state, State::Done { ok: true }), "{:?} {:?}\n{}", done.state, done.answer, dump(&w));
    // the file is plain JSON, nbformat 4, one code cell for each cell that ran, in order, with its output
    let file = d.project().join(format!("{}.ipynb", row.id));
    let nb: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap_or_else(|e| panic!("{}: {e}", file.display()))).expect("json loads it");
    assert_eq!(nb["nbformat"], 4);
    assert_eq!(nb["metadata"]["kernelspec"]["name"], "python3");
    let list = nb["cells"].as_array().unwrap();
    assert_eq!(list.len(), cells.len());
    assert!(list.iter().all(|c| c["cell_type"] == "code"));
    let source = |c: &Value| c["source"].as_array().unwrap().iter().map(|l| l.as_str().unwrap()).collect::<String>();
    assert_eq!(list.iter().map(source).collect::<Vec<_>>(), ["x = 2\nprint('hi')", "x * 21", "1 / 0"]);
    assert_eq!(list[0]["outputs"][0]["output_type"], "stream");
    assert_eq!(list[0]["outputs"][0]["text"], json!(["hi\n"]));
    assert_eq!(list[1]["outputs"][0]["output_type"], "execute_result");
    assert_eq!(list[1]["outputs"][0]["data"]["text/plain"], json!(["42"]));
    assert_eq!((list[2]["outputs"][0]["output_type"].as_str(), list[2]["outputs"][0]["ename"].as_str()), (Some("error"), Some("ZeroDivisionError")));
    // a task that ran no cell has nothing to export
    let Said::Started { task } = w.front.say("main", ":kit export t99") else { panic!("did not start") };
    let none = w.front.tasks().book().wait(&task, |r| r.state.is_terminal(), crate::support::PATIENCE).unwrap();
    assert!(!matches!(none.state, State::Done { ok: true }), "{:?}", none.state);
    assert!(!d.project().join("t99.ipynb").exists());
    w.close();
}

#[test]
fn a_cell_of_the_own_agent_reaches_the_tools_it_is_offered_through_j4n_call_and_py_and_ask_are_not_among_them() {
    if !python_here() {
        return;
    }
    let d = Dirs::new("kit-bridge");
    fs::write(d.project().join("a.txt"), "one\ntwo\n").unwrap();
    let cell = "import j4n\nprint(sorted(j4n.tools()))\nprint(j4n.call('read', path='a.txt'))\nprint(j4n.many('grep', [{'pattern': 'two', 'path': 'a.txt'}, {'pattern': 'zzz', 'path': 'a.txt'}], errors='keep'))\nfor name in ('py', 'ask'):\n    try:\n        j4n.call(name)\n    except j4n.Refused as e:\n        print('refused', name)";
    let gateway =
        Mock::start(move |_, n| if n == 0 { calls(&[("c1", "py", json!({ "code": cell }))]) } else { answer("done") });
    let w = open(&d, &gateway, "on", Mode::Write);
    let row = line(&w, "look");
    assert!(matches!(row.state, State::Done { ok: true }), "{:?}\n{}", row.state, dump(&w));
    let r = results(&gateway.seen()[1]);
    assert!(
        r[0].contains("['edit', 'git', 'grep', 'list', 'read', 'run', 'web', 'write']"),
        "the offer less py and ask: {r:?}"
    );
    assert!(r[0].contains("two") && r[0].contains("refused py") && r[0].contains("refused ask"), "{r:?}");
    let cells = events(&w, "py.cell");
    let rows = cells[0]["host"].as_array().expect("a row for each call");
    let by: Vec<(&str, &str, bool)> =
        rows.iter().map(|x| (x["call"].as_str().unwrap(), x["tool"].as_str().unwrap_or(""), x["ok"] == true)).collect();
    assert_eq!(
        by,
        [
            ("tools", "", true),
            ("tool", "read", true),
            ("tool", "grep", true),
            ("tool", "grep", true),
            ("tool", "py", false),
            ("tool", "ask", false)
        ],
        "{rows:?}"
    );
    w.close();
}
