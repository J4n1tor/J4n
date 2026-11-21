//! Prompt shaping (pack `shape`): while the operator types, a shaper model reads the draft and the thread
//! and adds short notes, each pointing at a turn, a file or a check that J4n can look at without a model. A judge of another family
//! reads them; the operator keeps the rows they want with a key, and the line goes to the doer exactly as typed with those rows in
//! the Brief. Off by default (`:shape on`).
//!
//! - [`check`]: what holds of the shaper's answer, and why the rest was dropped.
//! - [`state`]: what the shaper is shown, made from the Log and the folder.
//! - [`render`]: the words round the rows that were kept (templates of the pack).
//! - [`Desk`]: the switch, the card and the kept rows, for the front.
//! - [`Shaper`]: the doer `shape` that the rule `shape-draft` starts.

pub mod check;
pub mod desk;
pub mod render;
pub mod run;
pub mod state;

pub use desk::{Cfg as DeskCfg, Desk, Outcome};
pub use render::{Format, Persona, Template};
pub use run::{Cfg, DOER, Shaper};

use j4n_connect::check::CheckRow;
use j4n_core::log::Log;
use j4n_core::setting::Settings;
use j4n_core::table::{Kind, Tables};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

/// The desk and the shaper the Table and the settings describe, and what was wrong with the rows (a row that does not read is left
/// out, and says so).
pub fn load(tables: &Tables, settings: &Settings, root: &Path, log: &Log) -> (Arc<Desk>, Shaper, Vec<String>) {
    let mut found = Vec::new();
    let rows = |kind: &str| -> Vec<String> { tables.ids(kind).into_iter().map(String::from).collect() };
    let mut templates = BTreeMap::new();
    for id in rows(Template::NAME) {
        match tables.get::<Template>(&id) {
            Ok(t) => {
                templates.insert(t.id.clone(), t);
            }
            Err(p) => found.push(p.to_string()),
        }
    }
    let mut personas = BTreeMap::new();
    for id in rows(Persona::NAME) {
        match tables.get::<Persona>(&id) {
            Ok(p) => {
                personas.insert(p.id, p.text);
            }
            Err(p) => found.push(p.to_string()),
        }
    }
    let mut formats = BTreeMap::new();
    for id in rows(Format::NAME) {
        match tables.get::<Format>(&id) {
            Ok(f) => {
                formats.insert(f.doer, f.format);
            }
            Err(p) => found.push(p.to_string()),
        }
    }
    // the commands that are known to show the work done: the project's own check rows whose files are here
    let checks = rows(CheckRow::NAME).into_iter().filter_map(|id| tables.get::<CheckRow>(&id).ok()).filter(|c| c.files.iter().all(|f| j4n_connect::check::present(root, f))).map(|c| c.argv.join(" ")).collect();
    let word = |id: &str, or: &str| settings.word(id, or).to_string();
    let desk = Desk::new(
        DeskCfg {
            on: matches!(settings.get("shape.on"), Some(j4n_core::value::Value::Bool(true))),
            skip: settings.word("shape.skip", "").split(',').map(check::fold).filter(|s| !s.is_empty()).collect(),
            templates,
            format: word("shape.format", "auto"),
            formats,
        },
        log.clone(),
    );
    let cfg = Cfg {
        model: word("shape.model", ""),
        judge: word("shape.judge", "off"),
        style: word("shape.style", "notes"),
        tone: word("shape.tone", "keep"),
        format: word("shape.format", "auto"),
        words: word("shape.words", ""),
        max_tokens: settings.num("shape.max_notes_tokens", 120.0) as usize,
        timeout_ms: settings.dur_ms("shape.timeout", 3_000),
        checks,
        personas,
    };
    let shaper = Shaper::new(cfg, desk.clone(), log.clone());
    (desk, shaper, found)
}
