//! What goes into the Log's lines and blobs: declared event kinds, blobs, and — the gate — no secret on disk.

use crate::world::*;
use j4n_core::change::Actor;
use j4n_core::clock::ManualClock;
use j4n_core::log::{BLOB_MAX, Log, New, Opts, Scope, is_blob_id};
use j4n_core::redact::Redactor;
use j4n_core::schema::DataClass;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::sync::Arc;

const EVENTS: &str = r#"
api = 1
[[event]]
id = "lamp.lit"
doc = "A lamp was lit."
fields = { name = { ty = "str", required = true }, brightness = { ty = "int", min = 1, max = 9 } }
[[event]]
id = "lamp.flicker"
doc = "A lamp flickers; seen, never kept."
class = "public"
ephemeral = true
[[event]]
id = "lamp.diary"
doc = "A keeper's private note."
class = "private"
"#;

fn declared(s: &Scratch) -> Log {
    let (log, _) = open_log(s);
    let (tables, problems) = load(vec![pack("lighthouse", EVENTS)]);
    assert!(problems.is_empty(), "{problems:?}");
    assert!(log.declare(&tables).is_empty());
    log
}

#[test]
fn a_declared_kind_has_its_data_checked_and_its_class_applied() {
    let s = Scratch::new("data-declared");
    let log = declared(&s);
    let ok = log.append(ev("lamp.lit", json!({"name": "north", "brightness": 4}))).unwrap();
    assert_eq!(ok.class, DataClass::Project);
    for bad in [
        json!({"brightness": 4}),
        json!({"name": "x", "brightness": 11}),
        json!({"name": "x", "colour": "red"}),
        json!({"name": 7}),
        json!(["name"]),
        json!({"name": "x", "brightness": null}),
    ] {
        let e = log.append(ev("lamp.lit", bad.clone())).unwrap_err();
        assert!(e.to_string().contains("event lamp.lit"), "{bad}: {e}");
    }
    assert_eq!(log.head(), 1, "none of those was written");
    let diary = log.append(ev("lamp.diary", json!({"anything": [1, 2]}))).unwrap();
    assert_eq!(diary.class, DataClass::Private, "the kind's class");
    let said = log.append(ev("lamp.diary", json!(1)).class(DataClass::Secret)).unwrap();
    assert_eq!(said.class, DataClass::Secret, "the caller's class wins");
    assert!(log.append(ev("other.kind", json!({"whatever": true}))).is_ok(), "an undeclared kind is not checked");
}

#[test]
fn a_declared_ephemeral_kind_is_flashed_and_a_declared_written_kind_is_not() {
    let s = Scratch::new("data-ephemeral");
    let log = declared(&s);
    assert!(log.append(ev("lamp.flicker", json!({}))).is_err());
    let f = log.flash(ev("lamp.flicker", json!({}))).unwrap();
    assert_eq!((f.seq, f.class), (0, DataClass::Public));
    assert!(log.flash(ev("lamp.lit", json!({"name": "x"}))).is_err());
    assert!(log.flash(ev("other.kind", json!({}))).is_ok(), "an undeclared kind may be flashed");
    assert_eq!(log.head(), 0);
}

#[test]
fn declaring_again_replaces_and_a_bad_declaration_is_reported() {
    let s = Scratch::new("data-redeclare");
    let log = declared(&s);
    let (none, _) = load(vec![]);
    assert!(log.declare(&none).is_empty());
    assert!(log.append(ev("lamp.flicker", json!({}))).is_ok(), "no longer declared as ephemeral");
    let (t, _) = load(vec![pack("p", "api = 1\n[[event]]\nid = \"Bad.Kind\"\ndoc = \"x\"\n")]);
    let problems = log.declare(&t);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].msg.contains("not an event kind"));
}

// ---- blobs ----

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn a_blob_is_stored_once_under_the_hash_of_its_bytes() {
    let s = Scratch::new("blob-basic");
    let (log, _) = open_log(&s);
    let blobs = log.blobs();
    let id = blobs.put(b"hello blob").unwrap();
    assert_eq!(id, sha(b"hello blob"));
    assert!(is_blob_id(&id) && blobs.has(&id));
    assert_eq!(blobs.put(b"hello blob").unwrap(), id);
    assert_ne!(blobs.put(b"another").unwrap(), id);
    assert_eq!(blobs.get(&id).unwrap(), b"hello blob");
    assert_eq!(blobs.get(&blobs.put(b"").unwrap()).unwrap(), b"");
    let files: Vec<String> = walk(&s.join(".j4n/blobs"));
    assert_eq!(files.len(), 3, "{files:?}");
    assert!(files.iter().all(|f| !f.contains(".tmp")), "no temporary file is left: {files:?}");
}

fn walk(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for e in fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p.display().to_string());
        }
    }
    out.sort();
    out
}

#[test]
fn only_a_hash_names_a_blob() {
    let s = Scratch::new("blob-ids");
    let (log, _) = open_log(&s);
    let id = log.blobs().put(b"x").unwrap();
    for bad in ["", "../../etc/passwd", &id.to_uppercase(), &id[..63], &format!("{id}0"), "zz".repeat(32).as_str()] {
        assert!(log.blobs().get(bad).is_err(), "{bad:?}");
        assert!(!log.blobs().has(bad));
    }
    assert!(log.blobs().get(&"0".repeat(64)).unwrap_err().to_string().contains("cannot read"));
}

#[test]
fn a_blob_id_is_sixty_four_lowercase_hex_digits() {
    let id = sha(b"x");
    assert!(is_blob_id(&id));
    assert!(!is_blob_id(&format!("{id}0")) && !is_blob_id(&id[..63]) && !is_blob_id(""));
    assert!(!is_blob_id(&"z".repeat(64)) && !is_blob_id(&"g".repeat(64)) && !is_blob_id(&"A".repeat(64)));
    assert!(is_blob_id(&"f".repeat(64)) && is_blob_id(&"0".repeat(64)) && is_blob_id(&"a".repeat(64)));
}

#[test]
fn the_same_bytes_put_again_are_left_where_they_are() {
    use std::os::unix::fs::MetadataExt;
    let s = Scratch::new("blob-again");
    let (log, _) = open_log(&s);
    let id = log.blobs().put(b"once").unwrap();
    let path = s.join(&format!(".j4n/blobs/{}/{}", &id[..2], &id[2..]));
    let before = fs::metadata(&path).unwrap().ino();
    assert_eq!(log.blobs().put(b"once").unwrap(), id);
    assert_eq!(fs::metadata(&path).unwrap().ino(), before, "not written again");
}

#[test]
fn a_blob_changed_on_disk_is_an_error_not_data() {
    let s = Scratch::new("blob-corrupt");
    let (log, _) = open_log(&s);
    let id = log.blobs().put(b"the original").unwrap();
    fs::write(s.join(&format!(".j4n/blobs/{}/{}", &id[..2], &id[2..])), b"the forgery").unwrap();
    assert!(log.blobs().get(&id).unwrap_err().to_string().contains("no longer match"));
}

#[test]
fn a_blob_over_the_limit_is_refused() {
    let s = Scratch::new("blob-limit");
    let (log, _) = open_log(&s);
    assert!(log.blobs().put(&vec![0u8; BLOB_MAX]).is_ok());
    let e = log.blobs().put(&vec![0u8; BLOB_MAX + 1]).unwrap_err();
    assert!(e.to_string().contains("more than a blob holds"), "{e}");
}

#[test]
fn the_same_blob_put_by_many_threads_is_one_file() {
    let s = Scratch::new("blob-threads");
    let (log, _) = open_log(&s);
    let hs: Vec<_> = (0..8)
        .map(|_| {
            let log = log.clone();
            std::thread::spawn(move || (0..20).map(|_| log.blobs().put(&vec![7u8; 100_000]).unwrap()).collect::<Vec<_>>())
        })
        .collect();
    let ids: Vec<String> = hs.into_iter().flat_map(|h| h.join().unwrap()).collect();
    assert!(ids.iter().all(|i| *i == ids[0]));
    assert_eq!(walk(&s.join(".j4n/blobs")).len(), 1);
}

// ---- the gate: secrets never on disk ----

fn all_bytes(root: &Path) -> Vec<(String, Vec<u8>)> {
    walk(root).into_iter().map(|p| (p.clone(), fs::read(&p).unwrap())).collect()
}

fn contains(hay: &[u8], needle: &str) -> bool {
    !needle.is_empty() && hay.windows(needle.len()).any(|w| w == needle.as_bytes())
}

#[test]
fn a_registered_secret_is_nowhere_on_disk_whatever_it_looks_like_and_wherever_it_was_put() {
    let s = Scratch::new("gate-secrets");
    let clock = Arc::new(ManualClock::at(1));
    let o = Opts { max_line: 2000, ..opts(&clock) };
    let red = Redactor::new();
    let secrets = ["plain-secret-value", "has \"quotes\" and \\slashes\\", "multi\nline\nsecret", "ünïcödé-sécret-ключ", "a/b+c=d&e?f"];
    for (i, v) in secrets.iter().enumerate() {
        assert!(red.register(&format!("S{i}"), v));
    }
    let (log, _) = Log::open(&s.join(".j4n"), Scope::Project, o, red.clone()).unwrap();
    let sub = log.subscribe("*", 100);
    for (i, v) in secrets.iter().enumerate() {
        let big = format!("{}{v}{}", "p".repeat(3000), "q".repeat(100));
        let tasks = format!("task-{v}");
        log.append(ev("a.b", json!({"here": v, "list": ["x", [v]], "map": {"k": v, v.to_string(): 1}, "text": format!("before {v} after")}))).unwrap();
        log.append(New::of("a.b", Actor::Rule(v.to_string()), json!({"big": big}))).unwrap();
        log.append(ev("a.b", json!(i)).task(&tasks).thread(&format!("th-{v}"))).unwrap();
        log.blobs().put(format!("blob with {v} inside").as_bytes()).unwrap();
        log.blobs().put(&[&[0xff, 0xfe][..], v.as_bytes()].concat()).unwrap();
        let flashed = log.flash(ev("run.delta", json!({"text": v}))).unwrap();
        assert!(!flashed.data.to_string().contains(*v), "a flash is scrubbed too");
    }
    while let Some(e) = sub.try_recv() {
        let text = serde_json::to_string(&*e).unwrap();
        for v in secrets {
            assert!(!text.contains(v) && !text.contains(&serde_json::to_string(v).unwrap().trim_matches('"').to_string()), "{text}");
        }
    }
    // A fold that keeps what the events said: its snapshot is on disk too.
    let (t, _) = load(vec![pack("p", "api = 1\n[[fold]]\nid = \"kept\"\non = \"a.b\"\nreduce = \"set\"\nof = \"data\"\n")]);
    let (mut folds, _) = j4n_core::fold::declared(&t, Scope::Project);
    let host = j4n_core::fold::Folds::new(&s.join(".j4n/folds"));
    host.add(folds.pop().unwrap(), 0);
    host.attach(&log).unwrap();
    log.sync();
    assert!(host.state("kept").unwrap().to_string().len() > 100, "the fold holds the events");
    assert!(s.join(".j4n/folds").is_dir());
    drop(log);
    let mut checked = 0;
    for (path, bytes) in all_bytes(&s.join(".j4n")) {
        for v in secrets {
            let json_form = serde_json::to_string(v).unwrap();
            let json_form = json_form.trim_matches('"');
            for form in [v, json_form] {
                assert!(!contains(&bytes, form), "{form:?} is in {path}");
            }
        }
        checked += 1;
    }
    assert!(checked >= 13, "files were looked at: {checked}");
}

#[test]
fn a_key_shaped_thing_is_not_on_disk_either_and_the_line_says_so() {
    let s = Scratch::new("gate-shapes");
    let (log, _) = open_log(&s);
    let e = log.append(ev("run.shell", json!({"out": "export OPENAI_API_KEY=sk-proj-abcdefghijklmnopqrstuvwx\nok"}))).unwrap();
    let out = e.data["out"].as_str().unwrap();
    assert!(out.starts_with("export OPENAI_API_KEY=«REDACTED openai_key starts=sk-proj- length=32 charset=alnum same-as-id=") && out.ends_with("»\nok"), "{out}");
    let on_disk = s.read(".j4n/log/0001.jsonl");
    assert!(!on_disk.contains("sk-proj-abc") && on_disk.contains("REDACTED openai_key"));
}

#[test]
fn a_secret_registered_later_is_kept_out_of_what_comes_after() {
    let s = Scratch::new("gate-later");
    let (log, _) = open_log(&s);
    log.append(ev("a.b", json!("late-secret-value"))).unwrap();
    assert!(log.redactor().register("LATE", "late-secret-value"));
    let e = log.append(ev("a.b", json!("late-secret-value"))).unwrap();
    assert_eq!(e.data, json!("«secret:LATE»"));
}
