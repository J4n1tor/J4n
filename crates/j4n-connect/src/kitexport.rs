//! `:kit export`: the cells of a task, from the Log, as `<task>.ipynb` in the project's folder. A view for
//! Jupyter, JupyterLab, VS Code or Spyder; J4n needs none of them. The notebook is plain JSON, nbformat 4: one code cell for each `py.cell`
//! event of the task, in the order they ran, with its output (what the cell printed, the value of its last expression, or its error). The code
//! of a cell is read from the blob the event names; an event whose blob is gone gives its first 200 characters.

use crate::connect::{Job, StepDoer};
use j4n_core::doer::Doer;
use j4n_core::log::{Ev, Log};
use serde_json::{Value as Json, json};
use std::path::Path;
use std::sync::Arc;

/// The doer the flow `kit` runs.
pub const DOER: &str = "kit";

pub fn doer(log: &Log, root: &Path) -> Arc<dyn Doer> {
    let (log, root) = (log.clone(), root.to_path_buf());
    Arc::new(StepDoer { id: DOER, doc: "writes the cells of a task as a notebook", work: Arc::new(move |job| run(&log, &root, job)) })
}

fn run(log: &Log, root: &Path, job: &Job) -> Result<String, String> {
    log.sync();
    let words: Vec<&str> = job.goal.split_whitespace().collect();
    let asked = match words.as_slice() {
        ["export"] => None,
        ["export", task] => Some(*task),
        _ => return Err("`:kit export` writes the cells of the newest task that ran any as <task>.ipynb; `:kit export t12` writes another task's".into()),
    };
    let cells: Vec<Ev> = log.read(0).filter(|e| e.kind == "py.cell").collect();
    let task = match asked {
        Some(t) => t.to_string(),
        None => cells.last().and_then(|e| e.task.clone()).ok_or("no task has run a cell yet")?,
    };
    if task.is_empty() || !task.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(format!("`{task}` is not a task"));
    }
    let mine: Vec<Ev> = cells.into_iter().filter(|e| e.task.as_deref() == Some(&task)).collect();
    if mine.is_empty() {
        return Err(format!("task {task} ran no cell"));
    }
    let file = root.join(format!("{task}.ipynb"));
    let text = serde_json::to_string_pretty(&notebook(log, &task, &mine)).map_err(|e| e.to_string())?;
    std::fs::write(&file, text + "\n").map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(format!("{} cells of {task} are in {}", mine.len(), file.display()))
}

fn lines(s: &str) -> Vec<&str> {
    s.split_inclusive('\n').collect()
}

fn text<'a>(d: &'a Json, k: &str) -> &'a str {
    d.get(k).and_then(Json::as_str).unwrap_or_default()
}

/// The notebook of these `py.cell` events, with their code read from `log`'s blobs.
pub fn notebook(log: &Log, task: &str, events: &[Ev]) -> Json {
    let cells: Vec<Json> = events
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let (n, d) = (i + 1, &e.data);
            let code = d.get("blob").and_then(Json::as_str).and_then(|b| log.blobs().get(b).ok()).and_then(|b| String::from_utf8(b).ok()).unwrap_or_else(|| text(d, "head").to_string());
            let mut outputs = Vec::new();
            if !text(d, "out").is_empty() {
                outputs.push(json!({ "output_type": "stream", "name": "stdout", "text": lines(text(d, "out")) }));
            }
            if !text(d, "value").is_empty() {
                outputs.push(json!({ "output_type": "execute_result", "execution_count": n, "metadata": {}, "data": { "text/plain": lines(text(d, "value")) } }));
            }
            if !text(d, "error").is_empty() {
                let (name, why) = text(d, "error").split_once(':').map_or(("Error", text(d, "error")), |(a, b)| (a, b.trim()));
                outputs.push(json!({ "output_type": "error", "ename": name, "evalue": why, "traceback": [text(d, "error")] }));
            }
            let meta = json!({ "seq": e.seq, "risk": text(d, "risk"), "ms": d.get("ms"), "via": text(d, "via"), "host": d.get("host") });
            json!({ "cell_type": "code", "execution_count": n, "metadata": { "j4n": meta }, "source": lines(&code), "outputs": outputs })
        })
        .collect();
    json!({
        "cells": cells,
        "metadata": { "kernelspec": { "display_name": "Python 3", "language": "python", "name": "python3" }, "language_info": { "name": "python" }, "j4n": { "task": task } },
        "nbformat": 4,
        "nbformat_minor": 4,
    })
}
