//! What a compaction must not lose.
//! A doer's own summary of its session is lossy, so the loop never trusts it with the task: the must-keep block is told to the
//! doer (so its summary keeps it) **and** put in front of the next prompt (so it survives even if the summary does not). A
//! compaction happens at a settled boundary and nowhere else, and any failure of it is a note and not a stop.

/// The longest block, in characters (about 1,500 tokens): the point of compacting is to make the window smaller.
pub const MAX_KEEP_CHARS: usize = 6_000;
/// The operator's last words that are kept.
pub const KEEP_PROMPTS: usize = 4;
/// The answers to questions that are kept: the latest ones.
pub const KEEP_DECISIONS: usize = 8;

/// What the loop knows must stay, from the task and the ones it carried on from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MustKeep {
    /// The goal as the operator first gave it.
    pub goal: String,
    /// What is left to do, in the doer's last words on it.
    pub todo: Option<String>,
    /// Questions answered, and how, oldest first.
    pub decisions: Vec<String>,
    /// The paths changed so far.
    pub changed: Vec<String>,
    /// The operator's last steers, oldest first.
    pub prompts: Vec<String>,
    /// What a rule's `keep` adds. It comes after everything above and never in place of it.
    pub notes: Vec<String>,
    /// How the operator's `:compact` mode says to shrink (a `compact_mode` row's words): handed to the doer's own compaction beside what
    /// must stay, never in place of it.
    pub how: Option<String>,
}

/// The longest words of a compaction mode, in characters, after the operator's own words are put into them.
pub const HOW_CHARS: usize = 400;

fn clip(s: &str, n: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= n { s.to_string() } else { s.chars().take(n).collect::<String>() + "…" }
}

/// Notes as a rule may add them: single lines of at most [`crate::rule::NOTE_CHARS`] characters, at most
/// [`crate::rule::NOTES_MAX`] of them.
pub fn clip_notes(notes: Vec<String>) -> Vec<String> {
    notes
        .into_iter()
        .map(|n| n.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|n| !n.is_empty())
        .map(|n| clip(&n, crate::rule::NOTE_CHARS))
        .take(crate::rule::NOTES_MAX)
        .collect()
}

impl MustKeep {
    /// The block. The parts that cannot be rebuilt come first, so a clip loses the least.
    pub fn render(&self) -> String {
        let mut out = String::from("MUST KEEP (re-stated after compaction):\n");
        out.push_str(&format!("- goal: {}\n", clip(&self.goal, 1_200)));
        if let Some(h) = self.how.as_deref().map(|h| clip(h, HOW_CHARS)).filter(|h| !h.is_empty()) {
            out.push_str(&format!("- how to shrink: {h}\n"));
        }
        if let Some(t) = &self.todo {
            out.push_str(&format!("- todo: {}\n", clip(t, 400)));
        }
        if !self.decisions.is_empty() {
            out.push_str("- decisions:\n");
            for d in self.decisions.iter().rev().take(KEEP_DECISIONS).rev() {
                out.push_str(&format!("  · {}\n", clip(d, 200)));
            }
        }
        if !self.changed.is_empty() {
            let shown: Vec<&str> = self.changed.iter().take(30).map(String::as_str).collect();
            let more = self.changed.len().saturating_sub(shown.len());
            let tail = if more > 0 { format!(" (+{more} more)") } else { String::new() };
            out.push_str(&format!("- changed files: {}{tail}\n", shown.join(", ")));
        }
        if !self.prompts.is_empty() {
            out.push_str("- the operator's last words:\n");
            for p in self.prompts.iter().rev().take(KEEP_PROMPTS).rev() {
                out.push_str(&format!("  · {}\n", clip(p, 200)));
            }
        }
        if !self.notes.is_empty() {
            out.push_str("- also keep:\n");
            for n in &self.notes {
                out.push_str(&format!("  · {}\n", clip(n, crate::rule::NOTE_CHARS)));
            }
        }
        clip_block(out, MAX_KEEP_CHARS)
    }
}

/// Cut at a line end, so no line is left in half.
fn clip_block(s: String, max: usize) -> String {
    if s.chars().count() <= max {
        return s;
    }
    let mut out = String::new();
    for line in s.lines() {
        if out.chars().count() + line.chars().count() + 1 > max.saturating_sub(2) {
            break;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.push('…');
    out
}
