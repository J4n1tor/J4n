//! Agents › Setup: the steps to a working agent, a model and a Decider, as the rows of `fold:setup` that the view `setup-flow` draws
//! as a tree (the doctor's lines follow them in the fold, for `view:setup`). What the page knows before anything is typed is read here:
//! the doctor's probes (each program found, signed in or not), the key store (whether a key is there and where it comes from, never
//! the value), the packs and the settings. Nothing is sent anywhere.
//!
//! - **Agent**: a row per agent the doctor names, the small model left out, the one the election falls to marked `recommended`. `⏎`
//!   uses one (`lane.doer`, an operator's edit that `u` takes back). Under the one in use: its sign-in (run in the program's own
//!   folder, with no account), an API key for a program J4n can give one (`kept:` in its row) and the row that gives it (`doer.keys`,
//!   read at the start), or the key of J4n's own agent.
//! - **Model**: a pick of `default` and the models of the agent in use (`lane.model`; `default` keeps it empty).
//! - **Decider**: Span through OpenRouter, Jev · System One, none. The one whose key is there is recommended, else Span, as one key
//!   sets up three parts. `o` turns the pack on for every project (the home's own layer), the row `this project only` for this one.
//! - **Test**: `⏎` makes the proof call: one turn of the agent a line would get, on its model, asked a closed question, and the
//!   Decider's probe cases at one point. Each part's row says ok or failed, its time and its cost, read back from the Log (`setup.tested`,
//!   written once both are in); a failed part's row is the row that mends it (its key, its sign-in). The step is done only from a pass.
//! - **Custom** opens Agents › Overview.

use super::{Front, Said};
use crate::change::Actor;
use crate::doctor::{CHECKING, Report};
use crate::value::Value;
use serde_json::{Value as Json, json};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// The pack the Decider is in.
const DECIDER: &str = "jev";
/// The thread of the proof call's turn: no chat's, so it joins no chat's session and the operator's lines go elsewhere.
const PROOF: &str = "setup-test";
/// The closed question the agent is asked: one word back, so the call is short.
const QUESTION: &str = "Reply with the one word ok and nothing else.";
/// How long the agent is given to answer, and the Decider's cases to be free of another run.
const AGENT_MS: u64 = 120_000;
const CASES_WAIT: Duration = Duration::from_secs(60);

/// What the Log said of the proof calls, read on from where it was read last, and whether one is being made now.
#[derive(Default)]
pub struct Proof {
    upto: u64,
    last: Option<Json>,
    running: bool,
}

type Fields = BTreeMap<String, Value>;

fn text(s: impl AsRef<str>) -> Value {
    Value::text(s.as_ref())
}

fn word(m: &Fields, k: &str) -> String {
    match m.get(k) {
        Some(Value::Str(s)) => s.clone(),
        _ => String::new(),
    }
}

/// A row of the steps: the key the screen names it by, its kind, the row it hangs from, its mark (a glyph slot) and its words.
fn row(id: &str, kind: &str, parent: &str, mark: &str, words: [&str; 3]) -> Fields {
    let [label, now, note] = words;
    BTreeMap::from([
        ("step".to_string(), Value::Bool(true)),
        ("id".into(), text(id)),
        ("kind".into(), text(kind)),
        ("parent".into(), text(parent)),
        ("mark".into(), text(mark)),
        ("label".into(), text(label)),
        ("now".into(), text(now)),
        ("note".into(), text(note)),
        ("tone".into(), text("plain")),
        ("undo".into(), text("")),
        ("done".into(), Value::Bool(false)),
        ("restart".into(), Value::Bool(false)),
    ])
}

/// A key row of a provider (`fold:providers`): its secret editor and where the key stands (`set · J4n file`, `not set`, `not used · …`).
fn keyed(mut m: Fields, p: Option<&Fields>) -> Fields {
    if let Some(p) = p {
        m.insert("keyid".into(), text(word(p, "keyid")));
        m.insert("now".into(), text(word(p, "key")));
        if let Some(e) = p.get("edit") {
            m.insert("edit".into(), e.clone());
        }
    }
    m
}

/// Whether a key's place holds one J4n uses (`set · env`, `set · J4n file`).
/// What the `enable` row of a pack says now, if there is one: a row without `on` turns it on.
fn pack_said(screen: &super::Screen, pack: &str) -> Option<bool> {
    screen.tables.raw("enable", pack).map(|m| m.fields.get("on").and_then(toml::Value::as_bool).unwrap_or(true))
}

fn is_set(state: &str) -> bool {
    state.starts_with("set")
}

/// The provider whose key a keyed agent reads (`through` on its Providers row).
fn through(providers: &[Value], id: &str) -> Option<String> {
    providers.iter().find_map(|p| match p {
        Value::Map(m) if word(m, "through").split(", ").any(|c| c == id) => Some(word(m, "id")),
        _ => None,
    })
}

fn secs(ms: u64) -> String {
    format!("{:.1} s", ms as f64 / 1000.0)
}

/// The row of Agents › Providers that is `id`.
fn provider(providers: &[Value], id: &str) -> Option<Fields> {
    providers.iter().find_map(|r| match r {
        Value::Map(m) if word(m, "id") == id => Some(m.clone()),
        _ => None,
    })
}

impl Front {
    /// The rows of the steps, in the order the page shows them; `report` is the doctor's, none where no pre-flight is wired.
    pub fn setup_steps(&self, report: Option<&Report>) -> Vec<Value> {
        let providers = self.provider_rows();
        let mut out = self.setup_agents(report, &providers);
        out.push(self.setup_model(report));
        out.extend(self.setup_decider(&providers));
        out.extend(self.setup_proof(report, &providers));
        out.push(row("Custom", "custom", "", "", ["Custom", "any other program, a key of your own, every setting", "⏎ opens Agents › Overview · Agents › Providers and Settings › All hold every key and setting"]));
        out.into_iter().map(Value::Map).collect()
    }

    /// `:setup again`: the programs are asked again at the next look (one was installed or signed in), and the page is opened.
    pub(super) fn setup_again(&self, thread: &str) -> super::Said {
        if let Some(c) = self.cfg().setup.as_ref() {
            c.again();
        }
        self.note(thread, "the programs are asked again", "dim");
        super::Said::Open("setup".into())
    }

    fn setup_undo(&self, id: &str) -> String {
        self.proposer().and_then(|p| p.last_edit("setting", id)).unwrap_or_default()
    }


    /// The agent lines run on: the launch's pin, else the operator's pick, else the one the election falls to.
    fn setup_doer(&self, report: Option<&Report>) -> Option<String> {
        self.cfg().doer.clone().or_else(|| Some(self.0.live.word("lane.doer", "")).filter(|d| !d.is_empty())).or_else(|| report.and_then(|r| r.default.clone()))
    }

    fn setup_agents(&self, report: Option<&Report>, providers: &[Value]) -> Vec<Fields> {
        let screen = self.screen();
        let pick = self.0.live.word("lane.doer", "");
        // a pin of the launch is what every line runs on: its row is the one in use, and the pick waits for a start without it
        let pin = self.cfg().doer.clone();
        let runs = pin.clone().or_else(|| Some(pick.clone()).filter(|p| !p.is_empty()));
        let mut doers: Vec<crate::doctor::Row> = report.map(|r| r.rows.iter().filter(|r| r.kind == "doer" && r.id != "light").cloned().collect()).unwrap_or_default();
        // a pinned doer the doctor does not name (the scripted one, a connector of the operator's own) has a row from its own probe
        if let Some(p) = pin.as_ref().filter(|p| !doers.iter().any(|r| r.id == **p)) {
            let ready = self.0.who.probe_of(p).is_some_and(|x| x.installed && x.authed);
            doers.push(crate::doctor::Row { kind: "doer", id: p.clone(), tone: if ready { "good" } else { "warn" }, what: if ready { "ready" } else { "not ready" }.into(), fix: String::new(), account: false, ms: None });
        }
        let default = match &pin {
            Some(p) => doers.iter().find(|r| r.id == *p && r.tone == "good").map(|r| r.id.clone()),
            None => report.and_then(|r| r.default.clone()),
        };
        // the one in use first, then the one the election falls to; then those that work, those to mend, those that are not there
        let rank = |r: &crate::doctor::Row| (runs.as_deref() != Some(r.id.as_str()), default.as_deref() != Some(r.id.as_str()), match r.tone { "good" => 0, "warn" => 1, _ => 2 });
        doers.sort_by_key(|r| rank(r));
        let pinned = if pin.is_some() { " · pinned at start" } else { "" };
        // while a program's probe has not answered, no agent is known to work or not: the step says so, not `none works`
        let checking = default.is_none() && report.is_some_and(|r| r.checking);
        let state = default.as_ref().and_then(|d| doers.iter().find(|r| r.id == *d)).map_or(if checking { CHECKING.to_string() } else { "none works".to_string() }, |r| format!("{} · {}{pinned}", r.id, match r.what.as_str() { "key set" => "its key is set", "ready" => "ready", _ => "signed in" }));
        let (mark, tone) = match (default.is_some(), checking) {
            (true, _) => ("yes", "plain"),
            (false, true) => ("queued", "dim"),
            (false, false) => ("no", "bad"),
        };
        let mut step = row("Agent", "step", "", mark, ["Agent", &state, if default.is_some() { "⏎ change" } else { "⏎ set up" }]);
        step.insert("done".into(), Value::Bool(default.is_some()));
        step.insert("tone".into(), text(tone));
        let mut out = vec![step];
        let keys = self.0.live.word("doer.keys", "");
        let keys_start = match screen.start.pointer("/doer/keys") {
            Value::Str(s) => s,
            _ => String::new(),
        };
        for r in &doers {
            let id = r.id.as_str();
            let fields = screen.tables.raw("connector", id).map(|m| m.fields.clone()).unwrap_or_default();
            let signin = fields.get("signin").and_then(|s| s.get("argv")).and_then(toml::Value::as_array).is_some_and(|a| !a.is_empty());
            // the provider whose key J4n can give this program (`kept:` in its row's secrets), and the one a keyed agent reads
            let gives = fields.get("secrets").and_then(toml::Value::as_table).and_then(|s| s.values().find_map(|v| v.as_str().and_then(|r| r.strip_prefix("kept:")).map(String::from)));
            let (declared, ready, using, picked) = (r.what != "not declared", r.tone == "good", runs.as_deref() == Some(id), pick == id);
            // its pack turned on while J4n runs: it is made at the next start, and the row says so in place of the key that turns it on
            let waits = !declared && pack_said(&screen, crate::doctor::pack_of(id)) == Some(true);
            let (now, note) = match r.what.as_str() {
                "not declared" if waits => ("its pack is on from the next start · ⟳".to_string(), String::new()),
                "not declared" => ("its pack is off".to_string(), "⏎ turns it on".to_string()),
                "not found" => ("not on this machine".into(), "r looks again".into()),
                "no key" => ("no key".into(), if using { "its key is typed below".into() } else { "⏎ uses it, then its key is typed".into() }),
                "key set" => ("its key is set".into(), String::new()),
                "ready" => ("ready".into(), String::new()),
                "not ready" => ("not ready".into(), "r looks again".into()),
                CHECKING => (CHECKING.into(), String::new()),
                w if w.contains("not signed in") && signin => ("not signed in".into(), if using { "s signs in".into() } else { "s signs in · ⏎ uses it".into() }),
                w if w.contains("not signed in") => ("not signed in".into(), format!("sign in to {id} its own way · r looks again")),
                _ => ("signed in".into(), String::new()),
            };
            let note = match (using, ready, note.is_empty()) {
                (true, _, _) if !note.is_empty() => format!("in use{pinned} · {note}"),
                (true, ..) => format!("in use{pinned}"),
                (false, ..) if picked => "your pick · it takes effect when J4n starts without the pin".into(),
                (false, true, _) if pick.is_empty() && default.as_deref() == Some(id) => "recommended · ⏎ uses it".into(),
                (false, true, _) => "⏎ uses it".into(),
                _ => note,
            };
            let mut m = row(id, "agent", "Agent", if using { "radio-on" } else { "radio-off" }, [id, &now, &note]);
            m.insert("doer".into(), text(id));
            m.insert("pack".into(), text(crate::doctor::pack_of(id)));
            m.insert("declared".into(), Value::Bool(declared));
            m.insert("waits".into(), Value::Bool(waits));
            m.insert("ready".into(), Value::Bool(ready));
            m.insert("using".into(), Value::Bool(using));
            m.insert("picked".into(), Value::Bool(picked));
            m.insert("signin".into(), Value::Bool(signin && declared && r.what != "not found"));
            m.insert("tone".into(), text(if ready { "plain" } else { "dim" }));
            if picked {
                m.insert("undo".into(), text(self.setup_undo("lane.doer")));
            }
            out.push(m);
            if !using || !declared {
                continue;
            }
            // under the agent in use: what makes it work
            if r.what.contains("not signed in") {
                let mut s = row(&format!("{id} · sign in"), "signin", id, "queued", ["sign in", "not signed in", ""]);
                if signin {
                    s.insert("note".into(), text("⏎ starts its sign-in and shows the link it gives"));
                    s.insert("kind".into(), text("signin"));
                } else {
                    s.insert("note".into(), text(format!("sign in to {id} its own way · ⏎ looks again")));
                    s.insert("kind".into(), text("look"));
                }
                s.insert("doer".into(), text(id));
                out.push(s);
            }
            let keyed_by = if r.what == "no key" || r.what == "key set" { through(providers, id) } else { None };
            if let Some(p) = keyed_by {
                let label = format!("{} key", if p == "openrouter" { "OpenRouter".to_string() } else { p.clone() });
                out.push(keyed(row(&format!("{id} · key"), "key", id, "", [&label, "", "⏎ types it · J4n shows it to no one"]), provider(providers, &p).as_ref()));
            }
            if let Some(p) = gives {
                let k = keyed(row(&format!("{id} · API key"), "key", id, "", ["or use an API key", "", "⏎ types it · J4n shows it to no one"]), provider(providers, &p).as_ref());
                let kept = is_set(&word(&k, "now"));
                out.push(k);
                let bound = keys.split_whitespace().any(|k| k == id);
                if kept || bound {
                    let others: Vec<&str> = keys.split_whitespace().filter(|k| *k != id).collect();
                    let to = if bound { others.join(" ") } else { others.iter().copied().chain([id]).collect::<Vec<_>>().join(" ") };
                    let waits = keys_start.split_whitespace().any(|k| k == id) != bound;
                    let now = format!("{}{}", if bound { "it is given the API key" } else { "it uses its own sign-in" }, if waits { " · ⟳ next start" } else { "" });
                    let note = if bound { "⏎ takes the key back, for its sign-in" } else { "⏎ gives it the API key, from the next start" };
                    let mut b = row(&format!("{id} · give the key"), "bind", id, if bound { "tick-on" } else { "tick-off" }, ["give it the key", &now, note]);
                    b.insert("keys".into(), text(to));
                    b.insert("undo".into(), text(self.setup_undo("doer.keys")));
                    out.push(b);
                }
            }
        }
        out
    }

    fn setup_model(&self, report: Option<&Report>) -> Fields {
        let model = self.0.live.word("lane.model", "");
        let doer = self.setup_doer(report);
        let (models, own) = self.setup_models(doer.as_deref());
        let mut options = vec![text("default")];
        options.extend(models.iter().map(text));
        if !model.is_empty() && !models.contains(&model) {
            options.push(text(&model));
        }
        let (runs, pinned) = self.setup_line_model(doer.as_deref(), own);
        let now = format!("{}{}", if runs.is_empty() { "default · the agent picks" } else { runs.as_str() }, if pinned { " · pinned at start" } else { "" });
        // under a pin of the launch (its agent or its `--model`) the pick waits for a start without it, as the Agent step's pick does
        let note = match (&doer, pinned || self.cfg().doer.is_some()) {
            _ if model.is_empty() || model == runs => "⏎ pick".into(),
            (_, true) => format!("⏎ pick · your pick {model} · it takes effect when J4n starts without the pin"),
            (Some(d), false) => format!("⏎ pick · {model} is not a model of {d} · it runs on its default"),
            (None, false) => "⏎ pick".into(),
        };
        let mut m = row("Model", "model", "", "yes", ["Model", &now, &note]);
        m.insert("done".into(), Value::Bool(true));
        m.insert("undo".into(), text(self.setup_undo("lane.model")));
        let edit = BTreeMap::from([
            ("kind".to_string(), text("enum")),
            ("bind".into(), text("setting:lane.model.value")),
            ("value".into(), text(if model.is_empty() { "default" } else { model.as_str() })),
            ("options".into(), Value::List(options)),
        ]);
        m.insert("edit".into(), Value::Map(edit));
        m
    }

    fn setup_decider(&self, providers: &[Value]) -> Vec<Fields> {
        let screen = self.screen();
        let info = screen.packs.iter().find(|p| p.id == DECIDER);
        let loaded = info.is_some_and(|p| p.on);
        // what the files say now: a switch said while J4n runs is read at the next start
        let said = pack_said(&screen, DECIDER).unwrap_or(loaded);
        let (span, jev) = (provider(providers, "openrouter"), provider(providers, "typesafe"));
        let set = |p: &Option<Fields>| p.as_ref().is_some_and(|p| is_set(&word(p, "key")));
        let (span_key, jev_key) = (set(&span), set(&jev));
        let answers = if !said { "" } else if jev_key { "Jev · System One" } else if span_key { "Span through OpenRouter" } else { "" };
        let done = loaded && said && !answers.is_empty();
        let now = match (said, loaded) {
            (true, true) if !answers.is_empty() => format!("on · {answers} answers"),
            (true, true) => "on · its key is not typed yet".to_string(),
            (true, false) => "on from the next start · ⟳".to_string(),
            (false, true) => "off from the next start · ⟳".to_string(),
            (false, false) => "off · answers the small questions so you are asked less".to_string(),
        };
        let mut step = row("Decider", "step", "", if done { "yes" } else { "queued" }, ["Decider", &now, if done { "⏎ change" } else { "⏎ set up" }]);
        step.insert("done".into(), Value::Bool(done));
        step.insert("on".into(), Value::Bool(said));
        let mut out = vec![step];
        // a switch said while J4n runs acts from the next start: each row of the step has the key that restarts now, as Settings › Packs has
        let restart = said != loaded;
        // the one whose key is there is recommended; with neither, Span: one key sets up three parts
        let recommend = if jev_key && !span_key { "Jev · System One" } else { "Span through OpenRouter" };
        let off_note = if said { "" } else { " · o turns it on for every project" };
        for (label, p, what) in [("Span through OpenRouter", &span, "one key: also the small model and J4n's own agent"), ("Jev · System One", &jev, "needs a TypeSafe key")] {
            let note = format!("{what}{}{off_note}", if label == recommend { " · recommended" } else { "" });
            let mut m = keyed(row(label, "decider", "Decider", if answers == label { "radio-on" } else { "radio-off" }, [label, "not set", &note]), p.as_ref());
            m.insert("on".into(), Value::Bool(said));
            out.push(m);
        }
        let mut none = row("none", "none", "Decider", if said { "radio-off" } else { "radio-on" }, ["none", "", if said { "every question keeps its fixed pick or comes to you · ⏎ turns the Decider off" } else { "every question keeps its fixed pick or comes to you" }]);
        none.insert("on".into(), Value::Bool(said));
        out.push(none);
        if !said {
            out.push(row("this project only", "here", "Decider", "", ["this project only", "", "⏎ turns the Decider on here and in no other project"]));
        }
        for m in &mut out {
            m.insert("restart".into(), Value::Bool(restart));
        }
        out
    }

    // ---- the test: the proof call ----

    /// The agent and the model word the proof call takes, as a line's election would: the launch's pin, else the operator's pick or the
    /// doer the doctor's election falls to, with `lane.model`.
    fn setup_agent(&self, report: Option<&Report>) -> (Option<String>, String) {
        let doer = self.setup_doer(report);
        let model = self.setup_line_model(doer.as_deref(), self.setup_models(doer.as_deref()).1).0;
        (doer, model)
    }

    /// The models of `doer`'s row, and the list the election's agent is held to: those and the ones a read of it found, as kept (a page's
    /// draw never reads a list).
    fn setup_models(&self, doer: Option<&str>) -> (Vec<String>, Vec<String>) {
        let Some(d) = doer else { return Default::default() };
        let row: Vec<String> = self.screen().tables.raw("connector", d).and_then(|m| m.fields.get("models").and_then(toml::Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())).unwrap_or_default();
        let own = row.iter().cloned().chain(self.cfg().models.kept(d)).collect();
        (row, own)
    }

    /// The model word lines run on, and whether it is the launch's `--model`, as `elect_by` binds: under a pin of the launch `--model` or the
    /// agent's own default, whatever `lane.model` says; beside the operator's `lane.doer`, `lane.model` then `--model`; for the election's doer
    /// (`doer`, its list `own`), `--model`, then `lane.model` when it is one of its models.
    fn setup_line_model(&self, doer: Option<&str>, own: Vec<String>) -> (String, bool) {
        let (cfg, model) = (self.cfg(), self.0.live.word("lane.model", ""));
        let picked = !self.0.live.word("lane.doer", "").is_empty();
        match (&cfg.doer, &cfg.model) {
            (None, _) if picked && !model.is_empty() => (model, false),
            (None, None) if doer.is_some() && !model.is_empty() => (super::main_model_in(&model, Some(own)).unwrap_or_default(), false),
            (None, None) => (model, false),
            (_, m) => (m.clone().unwrap_or_default(), m.is_some()),
        }
    }

    /// The Decider the proof call asks: `(answerer, its row's label, the provider of its key)`, Jev where its key is there, else Span;
    /// Span's while neither has one (the part fails, and its row types the key). None while the pack is not loaded: the part is skipped.
    fn setup_answerer(&self, providers: &[Value]) -> Option<(String, &'static str, String)> {
        if !self.screen().packs.iter().any(|p| p.id == DECIDER && p.on) {
            return None;
        }
        let set = |id: &str| provider(providers, id).is_some_and(|p| is_set(&word(&p, "key")));
        Some(if set("typesafe") {
            ("decider:jev".into(), "Jev · System One", "typesafe".into())
        } else {
            ("decider:span".into(), "Span through OpenRouter", "openrouter".into())
        })
    }

    /// The last proof call the Log holds, and whether one is being made now.
    fn proof_last(&self) -> (bool, Option<Json>) {
        let mut p = self.0.connect.proof.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let from = p.upto;
        for ev in self.log().read(from).filter(|e| e.seq > from) {
            p.upto = p.upto.max(ev.seq);
            if ev.kind == "setup.tested" {
                p.last = Some(ev.data.clone());
            }
        }
        (p.running, p.last.clone())
    }

    /// `:setup test`: the proof call, off the line's thread. The doctor's look is asked again first, so the parts are those of now.
    pub(super) fn setup_test(&self, thread: &str) -> Said {
        {
            let mut p = self.0.connect.proof.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if p.running {
                self.note(thread, "the test is running · Agents › Setup shows each part when it ends", "dim");
                return Said::Open("setup".into());
            }
            p.running = true;
        }
        if let Some(c) = self.cfg().setup.as_ref() {
            c.again();
        }
        let decider = self.setup_answerer(&self.provider_rows());
        let (front, thread) = (self.clone(), thread.to_string());
        self.note(&thread, "the test runs: one short call to each part that is set", "dim");
        std::thread::spawn(move || {
            /// The test is over however it ends, a panic too.
            struct Over(Front);
            impl Drop for Over {
                fn drop(&mut self) {
                    self.0.0.connect.proof.lock().unwrap_or_else(std::sync::PoisonError::into_inner).running = false;
                }
            }
            let over = Over(front.clone());
            // the agent the test calls is the one the programs' answers of now elect: they are waited for here, off the line's way
            let report = front.cfg().setup.as_ref().map(|c| c.report_now(front.main_pick()));
            let (doer, model) = front.setup_agent(report.as_ref());
            let agent = front.proof_agent(doer.as_deref(), &model);
            let asked = decider.as_ref().map(|(id, label, key)| front.proof_decider(id, label, key));
            let ok = |p: &Json| p["ok"].as_bool() == Some(true);
            let passed = ok(&agent) && asked.as_ref().is_none_or(ok);
            let parts: Vec<Json> = [agent].into_iter().chain(asked).collect();
            let data = json!({ "agent": doer.unwrap_or_default(), "model": model, "decider": decider.map(|d| d.0).unwrap_or_default(), "passed": passed, "parts": parts });
            drop(over);
            front.write(PROOF, "setup.tested", Actor::Core, data);
            let said = if passed { "Setup's test passed · Agents › Setup shows each part" } else { "Setup's test did not pass · Agents › Setup: ⏎ on the part that failed mends it" };
            front.note(&thread, said, if passed { "dim" } else { "warn" });
        });
        Said::Open("setup".into())
    }

    /// The agent's part: one turn, in read mode, on a thread of its own; ok when it ended with an answer. Its time and cost are the turn's
    /// own, from its `run.metrics`; a turn that said no cost has none.
    fn proof_agent(&self, doer: Option<&str>, model: &str) -> Json {
        let part = |ok: bool, ms: u64, usd: Option<f64>, why: &str, task: &str| json!({ "part": "agent", "id": doer.unwrap_or_default(), "model": model, "ok": ok, "ms": ms, "usd": usd, "why": crate::text::line(why, 80), "task": task });
        let Some(doer) = doer else { return part(false, 0, None, "no agent works", "") };
        // the word as a line's binding takes the operator's pick: as it is, to the host's election
        let (b, _) = self.bind_to(doer.to_string(), Some(model).filter(|m| !m.is_empty()), "setup");
        let mut spec = match self.spec(PROOF, QUESTION, doer) {
            Ok(s) => s,
            Err(why) => return part(false, 0, None, &why, ""),
        };
        (spec.tune.model, spec.tune.endpoint, spec.tune.mode) = (b.model, b.endpoint, crate::task::Mode::Read);
        spec.limits.wall_ms = Some(spec.limits.wall_ms.map_or(AGENT_MS, |w| w.min(AGENT_MS)));
        let from = self.log().head();
        let task = match self.0.tasks.spawn(spec, Actor::Operator) {
            Ok(t) => t,
            Err(p) => return part(false, 0, None, &p.msg, ""),
        };
        let Some(row) = self.0.tasks.book().wait(&task, |r| r.state.is_terminal(), Duration::from_millis(AGENT_MS + 10_000)) else {
            let _ = self.0.tasks.ctl(&task, crate::task::Ctl::Abort, Actor::Core);
            return part(false, AGENT_MS, None, "it did not answer in time", &task);
        };
        let ms = row.ended_ms.unwrap_or(row.queued_ms).saturating_sub(row.started_ms.unwrap_or(row.queued_ms));
        self.log().sync();
        let paid: Vec<Option<f64>> = self.log().read(from).filter(|e| e.kind == "run.metrics" && e.task.as_deref() == Some(task.as_str())).map(|e| e.data["usd"].as_f64()).collect();
        let usd = if paid.is_empty() { None } else { paid.into_iter().sum::<Option<f64>>() };
        let answered = row.answer.as_deref().is_some_and(|a| !a.trim().is_empty());
        let ok = matches!(row.state, crate::task::State::Done { ok: true }) && answered;
        let why = if ok { "it answered" } else { row.reason.as_deref().filter(|r| !r.is_empty()).unwrap_or("it ended with no answer") };
        part(ok, ms, usd, why, &task)
    }

    /// The Decider's part: its probe cases at one point (`intake` where it answers there), put as `:probe` puts them; ok when it passed.
    fn proof_decider(&self, id: &str, label: &str, key: &str) -> Json {
        let part = |ok: bool, ms: u64, usd: Option<f64>, why: &str| json!({ "part": "decider", "id": id, "label": label, "key": key, "ok": ok, "ms": ms, "usd": usd, "why": crate::text::line(why, 80) });
        let Some(prober) = self.0.asks.prober.clone() else { return part(false, 0, None, "no Decider is made to ask") };
        let targets = prober.targets(Some(id));
        let Some(pair) = targets.iter().find(|(_, p)| p == "intake").or(targets.first()).cloned() else { return part(false, 0, None, "its key is not typed yet") };
        // a pair another run has in hand is left to it: the test waits until it is free, then puts the cases itself
        let end = Instant::now() + CASES_WAIT;
        let report = loop {
            if let Some(r) = prober.run(std::slice::from_ref(&pair)).into_iter().next() {
                break r;
            }
            if Instant::now() >= end {
                return part(false, 0, None, "its probe cases are being put already · ⏎ on run again tries it later");
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        let why = match (&report.why, report.passed) {
            (Some(why), _) => why.clone(),
            (None, true) => "it answered its probe cases".into(),
            (None, false) => format!("it missed {} of its probe cases", report.missed.len()),
        };
        part(report.passed && report.why.is_none(), report.ms, report.usd, &why)
    }

    /// The Test step and, once a test ran, a row per part and the row that runs it again. A part that failed is the row that mends it.
    fn setup_proof(&self, report: Option<&Report>, providers: &[Value]) -> Vec<Fields> {
        let (running, last) = self.proof_last();
        let (agent, model) = self.setup_agent(report);
        let decider = self.setup_answerer(providers).map(|d| d.0).unwrap_or_default();
        // while the programs' probes are out the agent is not known, and a test that was made is neither stale nor failed
        let checking = agent.is_none() && report.is_some_and(|r| r.checking);
        // a test of another agent, model or Decider proves nothing of what is set now
        let fresh = last.as_ref().is_some_and(|l| l["agent"] == agent.unwrap_or_default().as_str() && l["model"] == model.as_str() && l["decider"] == decider.as_str());
        let passed = fresh && last.as_ref().is_some_and(|l| l["passed"] == true);
        let parts: Vec<Json> = last.as_ref().and_then(|l| l["parts"].as_array().cloned()).unwrap_or_default();
        let ms: u64 = parts.iter().filter_map(|p| p["ms"].as_u64()).sum();
        let usd: Option<f64> = parts.iter().map(|p| p["usd"].as_f64()).sum();
        let spent = |ms: u64, usd: Option<f64>| format!("{}{}", secs(ms), usd.map(|u| format!(" · {}", super::context::usd(u))).unwrap_or_default());
        let failed: Vec<&str> = parts.iter().filter(|p| p["ok"] != true).map(|p| if p["part"] == "agent" { "Agent" } else { "Decider" }).collect();
        let (mark, now, note) = match (running, &last) {
            (true, _) => ("queued", "running · one short call to each part".to_string(), String::new()),
            (false, None) => ("queued", "not run · one short call to each part above".into(), "⏎ runs it".into()),
            (false, Some(_)) if checking => ("queued", CHECKING.into(), String::new()),
            (false, Some(_)) if !fresh => ("queued", "not run for what is set now".into(), "the last test was of another agent, model or Decider".into()),
            (false, Some(_)) if passed => ("yes", format!("passed · {}", spent(ms, usd)), "⏎ shows each part".into()),
            (false, Some(_)) => ("no", format!("failed · {}", failed.join(", ")), "⏎ on the part that failed mends it".into()),
        };
        let mut step = row("Test", "test", "", mark, ["Test", &now, &note]);
        step.insert("done".into(), Value::Bool(passed && !running));
        step.insert("running".into(), Value::Bool(running));
        step.insert("tone".into(), text(if mark == "no" { "bad" } else if mark == "yes" { "plain" } else { "dim" }));
        let mut out = vec![step];
        if running || last.is_none() {
            return out;
        }
        for p in &parts {
            let (ok, why) = (p["ok"] == true, p["why"].as_str().unwrap_or_default());
            let state = format!("{} · {}", if ok { "ok" } else { "failed" }, spent(p["ms"].as_u64().unwrap_or(0), p["usd"].as_f64()));
            let id = p["id"].as_str().unwrap_or_default();
            let m = if p["part"] == "agent" {
                let model = p["model"].as_str().filter(|m| !m.is_empty()).map(|m| format!(" · {m}")).unwrap_or_default();
                let label = format!("{}{model}", if id.is_empty() { "Agent" } else { id });
                let mut m = row("Test · agent", "result", "Test", if ok { "yes" } else { "no" }, [&label, &state, why]);
                // what mends it: its sign-in where J4n can start one, its key where it reads one, else the program is asked again
                let doctor = report.and_then(|r| r.rows.iter().find(|r| r.kind == "doer" && r.id == id));
                let signin = self.screen().tables.raw("connector", id).and_then(|c| c.fields.get("signin").cloned()).is_some();
                let mend = if ok {
                    None
                } else if doctor.is_some_and(|r| r.what.contains("not signed in")) && signin {
                    m.insert("doer".into(), text(id));
                    Some(("signin", "⏎ signs it in"))
                } else if let Some(k) = through(providers, id) {
                    m = keyed(m, provider(providers, &k).as_ref());
                    Some(("key", "⏎ types its key"))
                } else {
                    Some(("look", "⏎ asks the program again"))
                };
                if let Some((kind, key)) = mend {
                    m.insert("kind".into(), text(kind));
                    m.insert("note".into(), text(format!("{why} · {key}")));
                }
                m.insert("now".into(), text(&state));
                m
            } else {
                let label = p["label"].as_str().unwrap_or("Decider");
                let mut m = row("Test · decider", "result", "Test", if ok { "yes" } else { "no" }, [label, &state, why]);
                if !ok {
                    m = keyed(m, provider(providers, p["key"].as_str().unwrap_or_default()).as_ref());
                    m.insert("kind".into(), text("key"));
                    m.insert("note".into(), text(format!("{why} · ⏎ types its key again")));
                }
                m.insert("now".into(), text(&state));
                m
            };
            out.push(m);
        }
        if !parts.iter().any(|p| p["part"] == "decider") {
            let mut m = row("Test · decider", "result", "Test", "", ["Decider", "skipped", "none is set · it is optional"]);
            m.insert("tone".into(), text("dim"));
            out.push(m);
        }
        let mut again = row("Test · again", "test", "Test", "", ["run again", "", "⏎ runs the test again"]);
        again.insert("running".into(), Value::Bool(false));
        out.push(again);
        out
    }
}
