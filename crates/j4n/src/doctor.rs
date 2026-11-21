//! `j4n doctor` and the facts of the pre-flight. The doers are probed by their own probes, the
//! connector rows say how each is mended, and `j4n_core::doctor` decides what that adds up to. Nothing is opened: no Log, no folder, no
//! child of a task; only the probes run, as they do when a doer is elected.

use crate::wire::{Setup, secret};
use j4n_connect::hosts::Models;
use j4n_connect::manifest::{Connector, Connectors};
use j4n_core::cost::Plans;
use j4n_core::doctor::{AGENT, DOERS, DoerFact, Facts, PlanFact, SPAN};
use j4n_core::doer::Doer;
use j4n_core::gate::grant::here_layer;
use j4n_core::pack::{enables, packs_dir};
use j4n_core::redact::Redactor;
use j4n_core::table::{Layer, LayerKind, Tables};
use std::sync::Arc;

/// The line that mends a doer that is not ready: the row of Agents › Setup that does it, never a line for a terminal.
fn fix(c: &Connector, installed: bool) -> String {
    let row = format!("{AGENT} › {}", c.id);
    if c.key.is_some() && c.program.is_none() {
        // no shell and no file named: the key is typed on its row, which J4n keeps in its own key file
        return key_fix(c);
    }
    match (&c.signin, installed) {
        // installing is the machine's, not a key of J4n: the row says where it stands, and `r` looks again
        (_, false) => format!("`{}` is not installed · {row}: r looks again", c.program()),
        // the program's own sign-in, started from its row with no account of J4n's
        (Some(s), true) if !s.argv.is_empty() => format!("{row}: s signs it in"),
        // a connector with no `[connector.signin]` has no sign-in J4n can open
        (_, true) => format!("not signed in, and `{}` has no sign-in J4n can open: sign in its own way · {row}: r looks again", c.program()),
    }
}

/// Where a missing key is typed: the OpenRouter key on the Decider's row of Agents › Setup (one key for the Decider, the small model and
/// J4n's own agent); a key of another place on the connector's row in Agents › Providers. Either `⏎` is the secret field.
fn key_fix(c: &Connector) -> String {
    match c.key.as_deref() {
        Some(k) if k.contains("OPENROUTER_API_KEY") || k == "kept:openrouter" => format!("{SPAN}: ⏎ types the key"),
        _ => format!("Agents › Providers › {}: ⏎ types the key", c.id),
    }
}

/// A doer as its row says it, before or after its probe: `probe` none, it waits for one (`pending`). Its pack off, it is not declared.
fn doer_fact(rows: &[Connector], doers: &[Arc<dyn Doer>], accounts: bool, id: &str, probe: Option<(j4n_core::doer::Probe, u64)>) -> DoerFact {
    let (Some(c), true) = (rows.iter().find(|c| c.id == id), doers.iter().any(|d| d.id() == id)) else {
        // its pack is off: its row of Setup turns it on (the small model's is the Decider's pack, turned on from the Decider's row)
        let fix = if id == "light" { format!("its pack is off · {SPAN}: o turns it on for every project") } else { format!("its pack is off · {AGENT} › {id}: ⏎ turns it on") };
        return DoerFact { id: id.into(), fix, ..DoerFact::default() };
    };
    let keyed = c.key.is_some() && c.program.is_none();
    let fact = DoerFact { id: id.into(), keyed, work: c.roles.is_empty() || c.roles.iter().any(|r| r == "work"), account: accounts && c.signin.is_some(), ..DoerFact::default() };
    match probe {
        Some((p, ms)) => DoerFact { fix: fix(c, p.installed), probe: Some(p), ms: Some(ms), ..fact },
        None => DoerFact { pending: true, ..fact },
    }
}

/// The facts of the pre-flight that run no program: each doer declared waits for its probe (`pending`), the plans, the gateway key.
pub fn frame(rows: &[Connector], doers: &[Arc<dyn Doer>], plans: &Plans, secret: &dyn Fn(&str, &str) -> Result<String, String>, accounts: bool) -> Facts {
    // the gateway key: the one the agent `own` reads (the environment, else the key J4n keeps); only whether it is there is asked, never printed
    let own = rows.iter().find(|c| c.id == "own");
    let key = match own {
        Some(c) => !c.api_key(secret).is_empty(),
        None => secret("OPENROUTER_API_KEY", "env:OPENROUTER_API_KEY").is_ok(),
    };
    let key_fix = own.map_or_else(|| format!("{SPAN}: ⏎ types the key"), key_fix);
    let found = DOERS.iter().map(|id| doer_fact(rows, doers, accounts, id, None)).collect();
    Facts { doers: found, plans: plans.list().into_iter().map(|(id, price, per)| PlanFact { id, price, per }).collect(), key, key_fix, ..Facts::default() }
}

/// One doer asked by its own probe, as long as the program takes.
pub fn probe(rows: &[Connector], doers: &[Arc<dyn Doer>], accounts: bool, id: &str) -> DoerFact {
    let Some(d) = doers.iter().find(|d| d.id() == id) else { return doer_fact(rows, doers, accounts, id, None) };
    let began = std::time::Instant::now();
    let p = d.probe();
    doer_fact(rows, doers, accounts, id, Some((p, began.elapsed().as_millis() as u64)))
}

/// The facts of the pre-flight from the rows and the doers a world (or `j4n doctor`) built, every probe answered. The doers are probed side by side.
pub fn facts(rows: &[Connector], doers: &[Arc<dyn Doer>], plans: &Plans, secret: &dyn Fn(&str, &str) -> Result<String, String>, accounts: bool) -> Facts {
    let mut f = frame(rows, doers, plans, secret, accounts);
    std::thread::scope(|s| {
        for d in f.doers.iter_mut().filter(|d| d.pending) {
            // a probe that panics found nothing there, as the screen's look says it
            s.spawn(move || match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| probe(rows, doers, accounts, &d.id))) {
                Ok(found) => *d = found,
                Err(_) => (d.pending, d.probe, d.fix) = (false, Some(j4n_core::doer::Probe::default()), format!("its probe failed · {AGENT} › {}: r looks again", d.id)),
            });
        }
    });
    f
}

/// `j4n doctor`: the report on stdout; 0 when the default doer works, 1 when none does.
pub fn run(setup: &Setup) -> i32 {
    let red = Redactor::new();
    let (user, _) = Layer::from_dir(LayerKind::User, "home", &setup.home);
    let (here, _) = here_layer(&setup.home, &setup.root);
    let (catalog, _) = crate::pack::catalog().with_installed(&packs_dir(&setup.home));
    let (rows, _) = enables(vec![user.clone(), here.clone()]);
    let (harness, kept) = crate::pack::harness(&catalog, &[user.clone(), here.clone()], &rows);
    let full = j4n_core::pack::says_full(&[user.clone(), here.clone()]);
    let mut layers = harness.layers.clone();
    layers.extend(Some(full).filter(|f| *f).and_then(|_| j4n_core::pack::full_layer(&harness.layers)));
    layers.extend([user, here]);
    let (tables, _) = Tables::load(layers);
    let (mut connectors, _) = Connectors::load(&tables);
    connectors.set_program("claude", &setup.claude);
    let keys = j4n_core::keys::Keys::new(&setup.home);
    connectors.keep_keys(keys.dir(), &j4n_core::keys::provider_refs(&tables));
    let settings = j4n_core::setting::Settings::load(&tables).0;
    connectors.bind_keys(&keys, &settings.word("doer.keys", "").split_whitespace().collect::<Vec<_>>());
    // the doctor probes an agent with the keys a line's start gives it
    connectors.pass_to_agents(&settings.word("keys.pass", "GH_TOKEN GITHUB_TOKEN").split_whitespace().collect::<Vec<_>>());
    let (models, _) = Models::load(&tables);
    let read = |name: &str, reference: &str| secret(&red, name, reference);
    let (doers, _) = connectors.doers_with(&read, &models);
    let (plans, _) = Plans::load(&tables);
    let mut found = facts(connectors.all(), &doers, &plans, &read, harness.info.iter().any(|i| i.id == "accounts" && i.on));
    found.kept = crate::pack::why_on(&harness, &kept, &rows, full);
    (found.full, found.orchestrating) = (full, j4n_core::setting::Settings::load(&tables).0.word("lanes", "single") == "orchestrate");
    // what `packs.full` turned on, that the operator's own rows did not take back
    let won = |id: &str| tables.raw("setting", id).and_then(|m| m.from.get("value")).is_some_and(|l| l == j4n_core::pack::FULL_LAYER);
    found.full_sets = j4n_core::pack::full_values(&harness.layers).into_iter().filter(|(id, _)| full && won(id)).map(|(id, v)| (id, v.to_string().trim_matches('"').to_string())).collect();
    let settings = j4n_core::setting::Settings::load(&tables).0;
    found.pick = Some(settings.word("lane.doer", "").to_string()).filter(|d| !d.is_empty());
    found.needs = needs(&tables, &settings, &found, connectors.all(), &setup.root);
    let report = j4n_core::doctor::report(&found);
    print!("{}", report.text());
    doers.iter().for_each(|d| d.shutdown());
    report.code()
}

/// What a feature that is on lacks: each need whose feature is on and whose part holds no model, with what the tiers found among the doers that are ready
/// (the models their rows list; nothing is run to ask for more) and where to set it.
fn needs(tables: &Tables, settings: &j4n_core::setting::Settings, found: &Facts, rows: &[Connector], root: &std::path::Path) -> Vec<j4n_core::doctor::NeedFact> {
    use j4n_core::front::{needs::unmet, tiers};
    use j4n_core::value::Value;
    let value = |id: &str| match settings.get(id) {
        Some(Value::Str(s)) => s.clone(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    };
    let ready: Vec<(String, Vec<String>)> = found
        .doers
        .iter()
        .filter(|d| d.work && d.probe.as_ref().is_some_and(|p| p.installed && p.authed))
        .map(|d| (d.id.clone(), rows.iter().find(|c| c.id == d.id).map(|c| c.models.clone()).unwrap_or_default()))
        .collect();
    // a key or a login is what the doer's own probe said; a Decider needs the key Jev reads; nothing is run for it
    let have = |row: &j4n_core::front::needs::NeedRow| -> Option<String> {
        let probe = |id: &str| found.doers.iter().find(|d| d.id == id).and_then(|d| d.probe.clone());
        match row.kind.as_str() {
            "key" => match probe(&row.connector) {
                None => Some(format!("{} is not there: its pack is off", row.connector)),
                Some(p) => (!p.authed).then(|| format!("the key of {} is not set", row.connector)),
            },
            "login" => match probe(&row.doer) {
                None => Some(format!("{} is not there: its pack is off", row.doer)),
                Some(p) if !p.installed => Some(format!("{} is not installed", row.doer)),
                Some(p) => (!p.authed).then(|| format!("{} is not signed in", row.doer)),
            },
            "git" => (!j4n_core::place::checkpoint::has_commit(root)).then(|| "this folder has no git commit, and a chat that writes works in a worktree cut from one".to_string()),
            _ => (!found.key && !settings.word("pick.hand", "").split_whitespace().any(|w| w == row.point)).then(|| format!("no Decider is set up to answer `{}`: it needs the OpenRouter key", row.point)),
        }
    };
    let (list, _) = unmet(tables, &value, &|| ready.clone(), &tiers::parse(settings.word("offload.prices", tiers::PRICES)), &|_| Default::default(), &have);
    list.into_iter()
        .map(|u| {
            let labels: Vec<&str> = u.parts.iter().map(|p| p.label.as_str()).collect();
            let seen: Vec<String> = u.parts.iter().map(|p| format!("{}: {}", p.label, p.found.as_ref().map_or("nothing found", |f| f.says.as_str()))).collect();
            let sets: Vec<&str> = u.parts.iter().map(|p| p.model_setting.as_str()).collect();
            if u.parts.is_empty() {
                return j4n_core::doctor::NeedFact { what: format!("{} is on and {}", u.need.feature, u.why), fix: format!("{} · go on without it or turn it off: the card in Chat asks", u.need.doc), id: u.need.id };
            }
            j4n_core::doctor::NeedFact { what: format!("{} is on and no model is named for {}", u.need.feature, labels.join(" and ")), fix: format!("{} · Settings › All › {} sets it", seen.join(" · "), sets.join(", ")), id: u.need.id }
        })
        .collect()
}
