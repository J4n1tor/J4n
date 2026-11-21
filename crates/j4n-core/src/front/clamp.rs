//! The limits code puts on a split, one function each: what Main's own model proposes is never started as it stands. Each returns what it kept and
//! why the rest stays with Main, so the Log and the row can say it.

use std::collections::BTreeSet;

/// The most chats a line may start, whatever the setting says.
pub const HARD_CHATS: usize = 4;

/// The least a chat may be given to spend, in dollars: a share below it is no chat.
const LEAST_USD: f64 = 0.02;

/// Steps that destroy, which a chat is never briefed with unless the operator's own line says them.
const DESTRUCTIVE: [&str; 13] = ["rm -rf", "rm -fr", "git push --force", "git push -f", "force-push", "force push", "git reset --hard", "git clean -f", "drop table", "drop database", "truncate table", "mkfs", "delete all"];

/// How many chats a line may start: the setting, between one and the hard cap.
pub fn chat_cap(setting: usize) -> usize {
    setting.clamp(1, HARD_CHATS)
}

/// How many more chats that write may start, with `running` writing chats already at work.
pub fn writers_room(running: usize, setting: usize) -> usize {
    setting.min(HARD_CHATS).saturating_sub(running)
}

/// Whether two paths meet: the same one, or one inside the other (`src` and `src/a.rs`).
pub fn meet(a: &str, b: &str) -> bool {
    let (a, b) = (a.trim_end_matches('/'), b.trim_end_matches('/'));
    a == b || b.strip_prefix(a).is_some_and(|r| r.starts_with('/')) || a.strip_prefix(b).is_some_and(|r| r.starts_with('/'))
}

/// The first of `writes` that meets a path in `taken`, with that path: one writer for a path.
pub fn clash(writes: &[String], taken: &BTreeSet<String>) -> Option<(String, String)> {
    writes.iter().find_map(|w| taken.iter().find(|t| meet(w, t)).map(|t| (w.clone(), t.clone())))
}

/// The destructive step a brief carries that the operator's line does not: such a step came from text that was quoted to Main (a report, a file, a page)
/// and is data, not an order.
pub fn quoted_destruction(brief: &str, line: &str) -> Option<&'static str> {
    let (brief, line) = (brief.to_lowercase(), line.to_lowercase());
    DESTRUCTIVE.iter().copied().find(|step| brief.contains(step) && !line.contains(step))
}

/// What each chat may spend: its class's cap, else an equal share of the line's, and never more than is left of the line's; `None` where what is left is
/// too little to start a chat.
pub fn shares(line_usd: f64, caps: &[Option<f64>]) -> Vec<Option<f64>> {
    let mut left = line_usd;
    let equal = line_usd / caps.len().max(1) as f64;
    caps.iter()
        .map(|cap| {
            let share = cap.unwrap_or(equal).min(left);
            (share >= LEAST_USD).then(|| {
                left -= share;
                share
            })
        })
        .collect()
}
