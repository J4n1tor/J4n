//! The same turns against the installed CLI. Only with `J4N_LIVE=1` and outside the hermetic runner (which keeps `claude` off
//! the PATH): it spends a little of the account it is logged in with (haiku, a few cents at most).
//!
//! ```text
//! J4N_LIVE=1 scripts/b test -p j4n-connect --test connect live -- --nocapture
//! ```

use crate::world::{Rig, Scratch};
use j4n_connect::Claude;
use j4n_connect::claude::Config;
use j4n_core::ask::Answer;
use j4n_core::change::Actor;
use j4n_core::doer::Doer;
use j4n_core::task::{Ctl, State, TaskSpec};

fn live() -> bool {
    let on = std::env::var_os("J4N_LIVE").is_some_and(|v| v == "1") && std::env::var_os("J4N_HERMETIC").is_none();
    if !on {
        eprintln!("skipped: J4N_LIVE=1 outside the hermetic runner runs it");
    }
    on
}

#[test]
fn live_the_installed_cli_is_logged_in() {
    if !live() {
        return;
    }
    let probe = Claude::new(Config::default()).probe();
    assert!(probe.installed && probe.authed, "{probe:?}");
    eprintln!("probe: {probe:?}");
}

#[test]
fn live_a_turn_a_failed_command_and_a_follow_up_in_the_same_child() {
    if !live() {
        return;
    }
    let rig = Rig::over(Scratch::new("bin"), Config::default(), |_| {});
    let task = |goal: &str| {
        let mut spec = TaskSpec::new(goal, "claude");
        spec.tune.model = Some("haiku".into());
        rig.tasks.spawn(spec, Actor::Operator).unwrap()
    };
    let first = task("Reply with the single word: ready");
    let row = rig.finished(&first);
    assert_eq!(row.state, State::Done { ok: true }, "{}", rig.dump());
    assert!(row.answer.as_deref().is_some_and(|a| a.to_lowercase().contains("ready")), "{:?}", row.answer);
    assert!(row.spent > 0 && row.tokens_out > 0);
    // a follow-up: the same session, the same child
    let second = rig.tasks.resume(&first, Some("Run exactly this one command with the Bash tool, nothing added to it: sh -c 'echo oops >&2; exit 3'"), Actor::Operator).unwrap();
    let ask = rig.waiting_on(&second);
    rig.tasks.ctl(&ask, Ctl::Answer(Answer::Yes), Actor::Operator).unwrap();
    let row = rig.finished(&second);
    assert_eq!(row.state, State::Done { ok: true }, "{}", rig.dump());
    let shells: Vec<_> = rig.events().into_iter().filter(|e| e.kind == "run.shell" && e.task.as_deref() == Some(second.as_str())).collect();
    assert_eq!(shells.len(), 1, "{}", rig.dump());
    if shells[0].data["cmd"] == "sh -c 'echo oops >&2; exit 3'" {
        assert_eq!(shells[0].data["exit"], 3, "{}", shells[0].data);
    }
    assert_eq!(rig.claude.live(), 1);
    eprintln!("answer: {:?}\nshell: {}", row.answer, shells[0].data);
}

// ---- the other agents, through the rows the packs ship ----

/// The doer a pack's row makes, and the bench around it.
fn pack_bench(id: &str, packs: &[(&str, &str)]) -> crate::bench::Bench {
    use j4n_connect::manifest::Connectors;
    use j4n_core::table::{Layer, LayerKind, Tables};
    let layers: Vec<Layer> = packs
        .iter()
        .map(|(dir, name)| {
            let (mut layer, _) = Layer::from_dir(LayerKind::Pack, name, &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs").join(dir));
            layer.files.retain(|f| !f.path.starts_with("kinds/") && f.path != "pack.toml");
            layer
        })
        .collect();
    let (tables, _) = Tables::load([vec![crate::bench::base_layer()], layers].concat());
    let (list, said) = Connectors::load(&tables);
    assert!(said.is_empty(), "{said:?}");
    let (doers, said) = list.doers(&|_, r| Err(r.to_string()));
    assert!(said.is_empty(), "{said:?}");
    assert!(doers.iter().any(|d| d.id() == id), "{id}");
    crate::bench::Bench::over(Scratch::new("bin"), doers, &tables)
}

#[test]
fn live_pi_answers_a_turn_and_keeps_its_session() {
    if !live() {
        return;
    }
    let b = pack_bench("pi", &[("base", "base-data")]);
    let row = b.run("pi", "Reply with the single word ok and nothing else.");
    eprintln!("{row:?}\n{}", b.dump());
    assert!(matches!(row.state, State::Done { ok: true }), "{row:?}");
    assert!(row.answer.as_deref().unwrap_or_default().to_lowercase().contains("ok"));
    let mut spec = TaskSpec::new("Now say it again, in capitals.", "pi");
    spec.resume = row.session.clone();
    let second = b.end(&b.tasks.spawn(spec, Actor::Operator).unwrap());
    assert!(matches!(second.state, State::Done { ok: true }), "{second:?}");
    assert!(second.answer.as_deref().unwrap_or_default().contains("OK"), "{second:?}");
}

#[test]
fn live_gemini_over_acp_answers_a_turn() {
    if !live() {
        return;
    }
    let b = pack_bench("gemini", &[("base", "base-data"), ("connect", "connect-data")]);
    let row = b.run("gemini", "Reply with the single word ok and nothing else.");
    eprintln!("{row:?}\n{}", b.dump());
    // without a login of its own the agent answers `session/new` with its words about it: the handshake worked up to there
    let login = matches!(&row.state, State::Stopped { why: j4n_core::task::Why::Failed { message } } if message.contains("sign in"));
    assert!(login || matches!(row.state, State::Done { ok: true }), "{row:?}");
}

#[test]
fn live_omp_answers_a_turn() {
    if !live() {
        return;
    }
    let b = pack_bench("omp", &[("base", "base-data")]);
    let row = b.run("omp", "Reply with the single word ok and nothing else.");
    eprintln!("{row:?}\n{}", b.dump());
    assert!(matches!(row.state, State::Done { ok: true }), "{row:?}");
}
