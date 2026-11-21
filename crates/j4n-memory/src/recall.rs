//! Remembered claims: a task whose goal is the goal of an earlier one, on files that are as that
//! one left them, is shown what the earlier report claimed, as a section of the first turn's Brief, `remembered`. Behind the setting
//! `brief.recall` (off until measured). Only what a report claimed and its checks passed is kept (never its answer, its commands and exits,
//! the diff or the plan: what changes with every task); each claim keeps the hash of the file it names, and is dropped when the file's
//! hash is not that any more or the file is gone. The words say they are an old report's, not facts.

use crate::{Index, lock};
use j4n_core::brief::tokens;
use j4n_core::front::Remembered;
use j4n_core::log::{Ev, Log};
use j4n_core::report::{at_path, file_hash, goal_key};
use j4n_core::schema::DataClass;
use rusqlite::params;
use serde_json::{Value as Json, json};
use std::path::PathBuf;
use std::sync::Arc;

/// What the section may take as rendered, its `## remembered` heading included.
pub const RECALL_TOKENS: usize = 150;
const HEAD: &str = "Claims of an earlier report of this same task (same goal, and the files below are unchanged since). They are that report's claims, not facts: check one before relying on it.";
const CLAIMS: usize = 6;
/// The reports of one goal looked at, newest first, for one that still has a claim that holds.
const LOOKED: usize = 5;

pub(crate) const TABLE: &str = "CREATE TABLE IF NOT EXISTS said(seq INTEGER PRIMARY KEY, goal TEXT NOT NULL, claims TEXT NOT NULL); CREATE INDEX IF NOT EXISTS said_goal ON said(goal);";

/// What a `report.made` adds: its goal's key and its checked claims with the hash their file had then. A report made by a build
/// that did not write them, a secret, and a report with no checked claim over a file add nothing.
pub(crate) fn kept(ev: &Ev) -> Option<(String, String)> {
    if ev.kind != "report.made" || ev.class > DataClass::Project {
        return None;
    }
    let (goal, files) = (ev.data.get("goal")?.as_str()?, ev.data.get("files")?.as_object()?);
    let claims: Vec<Json> = ev.data.pointer("/report/found")?.as_array()?.iter().filter(|f| f["unchecked"] == false).filter_map(|f| {
        let (claim, at) = (f["claim"].as_str()?, f["at"].as_str()?);
        let path = at_path(at)?;
        Some(json!({ "claim": claim, "at": at, "path": path, "hash": files.get(path)?.as_str()? }))
    }).collect();
    (!claims.is_empty()).then(|| (goal.to_string(), Json::Array(claims).to_string()))
}

impl Index {
    pub(crate) fn keep_said(tx: &rusqlite::Transaction, ev: &Ev) -> Result<(), String> {
        if let Some((goal, claims)) = kept(ev) {
            tx.execute("INSERT OR REPLACE INTO said(seq, goal, claims) VALUES (?1, ?2, ?3)", params![ev.seq, goal, claims]).map_err(crate::sql)?;
        }
        Ok(())
    }

    /// The claims kept for a goal's key, those of the newest report first.
    fn said(&self, key: &str) -> Vec<String> {
        let conn = lock(&self.conn);
        let Ok(mut stmt) = conn.prepare("SELECT claims FROM said WHERE goal = ?1 ORDER BY seq DESC LIMIT ?2") else { return Vec::new() };
        stmt.query_map(params![key, LOOKED], |r| r.get::<_, String>(0)).map(|rows| rows.flatten().collect()).unwrap_or_default()
    }
}

pub struct Recall {
    index: Arc<Index>,
    log: Log,
    root: PathBuf,
    /// The settings as they are now.
    settings: j4n_core::setting::Live,
}

impl Recall {
    pub fn new(index: Arc<Index>, log: Log, root: &std::path::Path, settings: impl Into<j4n_core::setting::Live>) -> Recall {
        Recall { index, log, root: root.to_path_buf(), settings: settings.into() }
    }

    /// The claims of the newest report of `goal` that still has one whose file is as it was, cut whole claims at a time to
    /// [`RECALL_TOKENS`]. A claim is `claim @ at`, the claim cut to 120 characters.
    pub fn claims(&self, goal: &str) -> Vec<String> {
        if self.index.catch_up(&self.log).is_err() {
            return Vec::new();
        }
        for stored in self.index.said(&goal_key(goal)) {
            let Ok(Json::Array(all)) = serde_json::from_str::<Json>(&stored) else { continue };
            let mut lines: Vec<String> = Vec::new();
            for c in all {
                let (Some(claim), Some(at), Some(path), Some(hash)) = (c["claim"].as_str(), c["at"].as_str(), c["path"].as_str(), c["hash"].as_str()) else { continue };
                let claim = if claim.chars().count() > 120 { format!("{}…", claim.chars().take(119).collect::<String>().trim_end()) } else { claim.to_string() };
                let line = format!("- {claim} @ {at}");
                if file_hash(&self.root, path).as_deref() == Some(hash) && !lines.contains(&line) {
                    lines.push(line);
                }
            }
            lines.truncate(CLAIMS);
            while !lines.is_empty() && tokens(&format!("{HEAD}\n{}", lines.join("\n"))) + 3 > RECALL_TOKENS {
                lines.pop();
            }
            if !lines.is_empty() {
                return lines;
            }
        }
        Vec::new()
    }
}

impl Remembered for Recall {
    fn text(&self, goal: &str) -> Option<String> {
        if !self.settings.flag("brief.recall", false) {
            return None;
        }
        let lines = self.claims(goal);
        (!lines.is_empty()).then(|| format!("{HEAD}\n{}", lines.join("\n")))
    }
}
