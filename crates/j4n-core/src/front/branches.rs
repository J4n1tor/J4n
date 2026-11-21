//! The branches, in the front: a branch's answer told to the main lane as a checked report, and the
//! rows of the Tasks page that show what its branches took. The ledger itself is the fold [`Lanes`]; the Brief's `branches` section is
//! made in `briefed`.

use super::Front;
use crate::lanes::{Lanes, Row, Totals};
use crate::report::Report;
use j4n_proto::{Act, Cell, Intent, Row as PRow, Tone};

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max { s.to_string() } else { format!("{}…", s.chars().take(max.saturating_sub(1)).collect::<String>()) }
}

fn count(n: u64) -> String {
    if n >= 1000 { format!("{:.1}k", n as f64 / 1000.0) } else { n.to_string() }
}

/// What a set of branches took, in a cell: dollars, seconds, tokens in/out.
fn took(t: &Totals) -> String {
    format!("${:.4} {:.1}s {}/{}", t.micros as f64 / 1e6, t.wall_ms as f64 / 1000.0, count(t.tokens_in), count(t.tokens_out))
}

fn tone(outcome: &str) -> Tone {
    match outcome {
        "used" | "merged" => Tone::Good,
        "blocked" => Tone::Warn,
        "killed" => Tone::Bad,
        "ignored" => Tone::Dim,
        _ => Tone::Accent,
    }
}

impl Front {
    /// The same front, keeping the branches of each thread in `lanes` (the fold the Folds host feeds). Without one a thread has none.
    pub fn with_lanes(self, lanes: Lanes) -> Front {
        match std::sync::Arc::try_unwrap(self.0) {
            Ok(mut core) => {
                core.lanes = Some(lanes);
                Front(std::sync::Arc::new(core))
            }
            Err(shared) => Front(shared),
        }
    }

    pub fn lanes(&self) -> Option<&Lanes> {
        self.0.lanes.as_ref()
    }

    /// A finished branch's answer for the main lane of `thread`: checked, written as `report.made`, and told in the next Brief's
    /// `branches` section, once. `None`: nothing here keeps lanes, or the task is not known.
    pub fn report(&self, task: &str) -> Option<Report> {
        let lanes = self.0.lanes.as_ref()?;
        let row = self.0.tasks.book().get(task)?;
        let answer = row.answer.clone().unwrap_or_default();
        // the report of a chat Main started is Main's to read as a digest, not the chat's own next turn's
        Some(lanes.settle(&self.0.log, &row, &self.0.cfg().root, &answer, row.spec.input.get("chat").is_none()))
    }

    /// What the one switch turns on, in words, as it stands now (the look, the crew and the envoys as the live settings hold them), with the needs that are unmet.
    pub fn switch_says(&self, thread: &str) -> String {
        let c = self.0.cfg();
        self.screen().switch_words(self.needs_count(thread), [c.orchestrate.look, c.crew.on, c.crew.envoys])
    }

    /// The one switch as the first row of the Tasks page while it is not `single`: its word, what it turns on, the needs and the crew's ready switch; `⏎` turns
    /// it off, `g` turns the crew's ready switch, `u` takes the last change of the word back.
    pub fn switch_row(&self, thread: &str) -> Option<PRow> {
        let word = self.lanes_now();
        if word == crate::branch::Switch::Single {
            return None;
        }
        let orchestrate = word == crate::branch::Switch::Orchestrate;
        let (to, label) = if orchestrate { ("single", "turn orchestrate off") } else { ("orchestrate", "turn orchestrate on") };
        let set = Act { key: "enter".into(), label: label.into(), intent: Intent::Set { kind: "setting".into(), id: "lanes".into(), field: "value".into(), value: to.into() }, risk: 0 };
        let mut actions = vec![set];
        // the crew's ready switch is beside the one switch: the same key as on the Packs row
        if let Some(on) = self.screen().crew_ready() {
            let to = if on { "off" } else { "on" };
            actions.push(Act { key: "g".into(), label: format!("crew ready to go {to}"), intent: Intent::Run { command: "pack".into(), args: vec!["ready".into(), to.into(), "crew".into()] }, risk: 0 });
        }
        if let Some(last) = self.proposer().and_then(|p| p.last_edit("setting", "lanes")).filter(|l| !l.is_empty()) {
            actions.push(Act { key: "u".into(), label: "undo".into(), intent: Intent::Run { command: "undo".into(), args: vec![last] }, risk: 0 });
        }
        let cells = vec![Cell::new("switch", Tone::Dim), Cell::new(word.word(), if orchestrate { Tone::Accent } else { Tone::Plain }), Cell::new("", Tone::Plain), Cell::new(self.switch_says(thread), Tone::Dim), Cell::new("", Tone::Dim)];
        Some(PRow { key: "switch:lanes".into(), cells, tone: Tone::Accent, actions, ..PRow::default() })
    }

    /// The rows of the thread's branches for the Tasks page: a root with the thread's totals, a row for each line that has branches
    /// (with the totals of its branches), and a row for each branch under its line, drawn with the pipes of a tree.
    pub fn lane_rows(&self, thread: &str) -> Vec<PRow> {
        let Some(lanes) = &self.0.lanes else { return Vec::new() };
        let lane = lanes.lane(thread);
        if lane.lines.is_empty() {
            return Vec::new();
        }
        let root = format!("lanes:{thread}");
        let head = vec![
            Cell::new("lanes", Tone::Dim),
            Cell::new(format!("{} branches", lane.total.branches), Tone::Accent),
            Cell::new("", Tone::Plain),
            Cell::new(format!("{} lines", lane.lines.len()), Tone::Plain),
            Cell::new(took(&lane.total), Tone::Dim),
        ];
        let mut rows = vec![PRow { key: root.clone(), cells: head, tone: Tone::Accent, ..PRow::default() }];
        let branch = |rows: &mut Vec<PRow>, r: &Row, pipe: &str| {
            let (what, who) = (if r.outcome.is_empty() { "running" } else { r.outcome }, [r.class.as_str(), crate::task::who(&r.doer, &r.model, "").as_str()].iter().filter(|w| !w.is_empty()).copied().collect::<Vec<_>>().join(" "));
            let own = Totals { branches: 1, micros: r.sub.micros, wall_ms: r.sub.wall_ms, tokens_in: r.sub.tokens_in, tokens_out: r.sub.tokens_out, cached: r.sub.cached };
            let cells = vec![Cell::new(format!("{pipe}{}", r.id), Tone::Dim), Cell::new(what, tone(r.outcome)), Cell::new(who, Tone::Plain), Cell::new(clip(&r.goal, 50), Tone::Plain), Cell::new(took(&own), Tone::Dim)];
            let detail = [r.host.as_str()].iter().filter(|w| !w.is_empty()).map(|w| Cell::new(*w, Tone::Dim)).collect();
            rows.push(PRow { key: format!("lanes:{thread}:{}", r.id), cells, tone: tone(r.outcome), detail, parent: Some(root.clone()), ..PRow::default() });
        };
        for line in &lane.lines {
            let cells = vec![Cell::new(&line.root.id, Tone::Dim), Cell::new(format!("{} branches", line.total.branches), Tone::Accent), Cell::new("", Tone::Plain), Cell::new(clip(&line.root.goal, 50), Tone::Plain), Cell::new(took(&line.total), Tone::Dim)];
            rows.push(PRow { key: format!("lanes:{thread}:{}", line.root.id), cells, parent: Some(root.clone()), ..PRow::default() });
            for (n, r) in line.rows.iter().enumerate() {
                // the last one at its depth ends its pipe
                let last = !line.rows[n + 1..].iter().take_while(|x| x.depth >= r.depth).any(|x| x.depth == r.depth);
                let pipe = format!("{}{} ", "│ ".repeat(r.depth.saturating_sub(1)), if last { "└─" } else { "├─" });
                branch(&mut rows, r, &pipe);
            }
        }
        rows
    }
}
