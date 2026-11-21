//! The skills J4n picked for a task, as claude's own. Claude takes no Brief, so the picked skills are written as a
//! plugin folder of the task's own place, `<place>/.j4n/skills/<task>/` (`.claude-plugin/plugin.json` and `skills/<name>/SKILL.md`), and the child
//! is started with `--plugin-dir` on it: claude lists each skill by its description and reads one when it fits. Nothing is written to `~/.claude`,
//! and a skill that was not picked is not in the folder, so it is not in claude's context either. Older folders of other tasks are swept.

use j4n_core::state;
use j4n_core::task::TaskSpec;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// How long the folder of a task is kept: a live child may read it until it has been idle that long.
const KEEP: Duration = Duration::from_secs(24 * 3600);

/// `(name, what claude lists it by, body)` of each skill the task was given, from `input.skills`. What claude lists it by is the skill's own `description`
/// (a sentence that says what it is for), else the words of its `when` after `Use when:`.
pub fn picked(spec: &TaskSpec) -> Vec<(String, String, String)> {
    let text = |s: &serde_json::Value, k: &str| s.get(k).and_then(serde_json::Value::as_str).unwrap_or_default().to_string();
    let about = |s: &serde_json::Value| Some(text(s, "description")).filter(|d| !d.is_empty()).unwrap_or_else(|| format!("Use when: {}", text(s, "when")));
    spec.input.get("skills").and_then(serde_json::Value::as_array).into_iter().flatten().map(|s| (text(s, "id"), about(s), text(s, "body"))).filter(|(id, _, body)| !id.is_empty() && !body.is_empty()).collect()
}

/// What names the set of skills in the signature of a child (nothing when there are none): the same skills are the same child, whatever the task.
pub fn mark(spec: &TaskSpec) -> Vec<String> {
    let mut skills = picked(spec);
    skills.sort();
    let all: String = skills.iter().map(|(id, when, body)| format!("{id}\0{when}\0{body}\0")).collect();
    if all.is_empty() { Vec::new() } else { vec!["--skills".into(), j4n_core::change::digest(&all)] }
}

/// Write the folder for a task that was given skills and say where it is; none when it was given none.
pub fn folder(place: &Path, task: &str, spec: &TaskSpec) -> Result<Option<PathBuf>, String> {
    let skills = picked(spec);
    if skills.is_empty() {
        return Ok(None);
    }
    if !task.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(format!("`{task}` cannot name a folder"));
    }
    let root = place.join(".j4n/skills");
    let (dir, e) = (root.join(task), |e: std::io::Error| format!("{}: {e}", root.display()));
    state::ensure_root(&place.join(".j4n"), true).and_then(|()| state::private_dir(&root)).map_err(e)?;
    sweep(&root);
    drop(fs::remove_dir_all(&dir));
    let write = |rel: &str, text: &str| -> std::io::Result<()> {
        let at = dir.join(rel);
        fs::create_dir_all(at.parent().unwrap_or(&dir))?;
        fs::write(at, text)
    };
    write(".claude-plugin/plugin.json", "{\"name\":\"j4n-skills\",\"version\":\"0.0.0\",\"description\":\"the skills J4n picked for this task\"}\n").map_err(e)?;
    for (id, about, body) in skills {
        let about = serde_json::Value::String(about);
        write(&format!("skills/{id}/SKILL.md"), &format!("---\nname: {id}\ndescription: {about}\n---\n{body}\n")).map_err(e)?;
    }
    Ok(Some(dir))
}

fn sweep(root: &Path) {
    for e in fs::read_dir(root).into_iter().flatten().filter_map(Result::ok) {
        if e.metadata().and_then(|m| m.modified()).ok().and_then(|t| SystemTime::now().duration_since(t).ok()).is_some_and(|age| age > KEEP) {
            drop(fs::remove_dir_all(e.path()));
        }
    }
}
