//! The plan's size law: a plan is as big as the request and the money left allow, and an item that one task would do in one
//! go is not a task of its own. Before the law a planner (deepseek) split a three-part request into 8-12 items, each a task that read the
//! same files again, and `lanes = auto` took 4 times single's tasks and hit the draw's cap.
//!
//! Three things, all on a plan that begins (never on one that is open: its started items are kept as they are):
//! 1. a read-only item that only one other item waits for, that waits for no other read, and that reads again what it read, is that
//!    item's first step ([`feeder`]): one task would read the files once. Reads that run side by side (independent
//!    read-only questions) stay;
//! 2. past the bound, the last item that nothing waits for goes into the item it comes after ([`tail`]), checks first;
//! 3. the bound is the request's parts, and the dollars left over what one item costs ([`bound`]).

use super::{FIELD, Given, MAX_ITEMS, PATHS};
use std::collections::BTreeSet;

/// A plan of one or two items is a plan, but the bound is never below this.
pub(crate) const MIN_ITEMS: usize = 3;

/// The parts of a request: its paragraphs (not the lead-in that ends with `:`, not a paragraph of bullets) when it has two or more, else its bulleted lines or its
/// numbered steps, whichever are most. (A line typed to `:plan` has no line breaks: its steps are `1.`, `2)` or `(3)` in the text. The
/// bullets inside a paragraph are questions of one part.)
pub fn parts(line: &str) -> usize {
    let paras: Vec<&str> = line.split("\n\n").map(str::trim).filter(|p| !p.is_empty() && !p.starts_with("- ") && !p.starts_with("* ")).collect();
    let lead = usize::from(paras.first().is_some_and(|p| p.ends_with(':')));
    let bullets = line.lines().filter(|l| l.trim_start().starts_with("- ") || l.trim_start().starts_with("* ")).count();
    let at = |n: usize| [format!("({n})"), format!("\n{n}."), format!("\n{n})"), format!(" {n}) "), format!(" {n}. ")].iter().any(|m| line.contains(m)) || line.starts_with(&format!("{n}. ")) || line.starts_with(&format!("{n}) "));
    let numbered = (1..=MAX_ITEMS).take_while(|n| at(*n)).count();
    match paras.len() - lead {
        n if n >= 2 => n,
        _ => bullets.max(numbered).max(1),
    }
}

/// How many items a plan for `line` may have: one per part, and no more than the dollars left pay for, where an item
/// costs about what the planning turn that read the project did (`left`, `unit`: dollars; `left` is none when the run has no limit).
pub fn bound(line: &str, left: Option<f64>, unit: f64) -> usize {
    let by_request = parts(line).clamp(MIN_ITEMS, MAX_ITEMS);
    match left {
        Some(left) if unit > 0.0 && left.is_finite() => by_request.min(((left / unit).floor().max(0.0) as usize).max(1)),
        _ => by_request,
    }
}

/// Fold `items` as the law says, and name each fold (`x into y`). Nothing is folded when a reference is not to an item of the plan: the
/// checks that follow say what is wrong with it.
pub(super) fn fold(items: &mut Vec<Given>, bound: usize) -> Vec<String> {
    let mut notes = Vec::new();
    let ids: BTreeSet<&str> = items.iter().map(|g| g.id.as_str()).collect();
    if ids.len() != items.len() || items.iter().any(|g| g.depends.iter().any(|d| !ids.contains(d.as_str()) || *d == g.id)) {
        return notes;
    }
    while let Some((x, w)) = feeder(items) {
        notes.push(format!("{} into {}", items[x].id, items[w].id));
        merge(items, x, w, true);
    }
    while items.len() > bound {
        let Some((y, x)) = tail(items) else { break };
        notes.push(format!("{} into {}", items[y].id, items[x].id));
        merge(items, y, x, false);
    }
    notes
}

fn waits_for(items: &[Given], i: usize) -> Vec<usize> {
    (0..items.len()).filter(|&j| items[j].depends.contains(&items[i].id)).collect()
}

/// Every item `i` comes after, directly or through others.
fn before(items: &[Given], i: usize) -> BTreeSet<usize> {
    let (mut found, mut todo) = (BTreeSet::new(), vec![i]);
    while let Some(n) = todo.pop() {
        for d in &items[n].depends {
            if let Some(k) = items.iter().position(|g| g.id == *d).filter(|k| found.insert(*k)) {
                todo.push(k);
            }
        }
    }
    found
}

/// A read-only item (not a review, not the main lane's) that exactly one item waits for, which waits for no other read-only item and reads
/// a path it read: the files would be read twice, and one task would do both in one go. `(it, that item)`.
fn feeder(items: &[Given]) -> Option<(usize, usize)> {
    (0..items.len()).find_map(|x| {
        let g = &items[x];
        let reads_only = g.writes.is_empty() && !g.main && g.place.as_deref() != Some("review");
        let only_read = |w: usize| !items.iter().any(|o| o.id != g.id && o.writes.is_empty() && items[w].depends.contains(&o.id));
        match waits_for(items, x)[..] {
            [w] if reads_only && only_read(w) && g.reads.iter().any(|p| items[w].reads.contains(p)) => Some((x, w)),
            _ => None,
        }
    })
}

/// The item to fold away past the bound and the one it goes into: a sink (nothing waits for it), a read-only one first, the last of
/// them; it goes into the last of the items it comes after that no other of them comes after (so no cycle is made).
fn tail(items: &[Given]) -> Option<(usize, usize)> {
    let sinks: Vec<usize> = (0..items.len()).filter(|&i| waits_for(items, i).is_empty()).collect();
    let y = sinks.iter().rev().find(|&&i| items[i].writes.is_empty()).or(sinks.last()).copied()?;
    let deps: Vec<usize> = items[y].depends.iter().filter_map(|d| items.iter().position(|g| g.id == *d)).collect();
    let x = match deps.iter().rev().find(|&&d| deps.iter().all(|&o| !before(items, o).contains(&d))) {
        Some(&x) => x,
        None => (0..items.len()).rev().find(|&i| i != y && deps.is_empty())?,
    };
    Some((y, x))
}

fn union(a: &mut Vec<String>, b: Vec<String>) {
    for p in b {
        if !a.contains(&p) {
            a.push(p);
        }
    }
}

/// Item `from` becomes part of item `into`: the goal says both (`into`'s words are kept first, up to the field's length), the paths and
/// dependencies are both items' (those of `from` that are `into` are gone) and whoever waited for `from` waits for `into`. `first`: `from`
/// is the read the other one starts with, and the done-when stays `into`'s; else `from` follows it and both done-whens are kept.
fn merge(items: &mut Vec<Given>, from: usize, into: usize, first: bool) {
    let g = items.remove(from);
    let into = if into > from { into - 1 } else { into };
    let (gone, kept) = (g.id.clone(), items[into].id.clone());
    for o in items.iter_mut() {
        for d in o.depends.iter_mut().filter(|d| **d == gone) {
            *d = kept.clone();
        }
    }
    let one = |s: String| s.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(FIELD).collect::<String>();
    let t = &mut items[into];
    t.goal = one(format!("{} ({}: {})", t.goal, if first { "reading first" } else { "then" }, g.goal));
    if !first {
        t.done = one(format!("{} {}", t.done, g.done));
    }
    if t.writes.is_empty() && !g.writes.is_empty() {
        t.place = g.place;
    }
    union(&mut t.reads, g.reads);
    union(&mut t.writes, g.writes);
    union(&mut t.depends, g.depends);
    t.reads.truncate(PATHS);
    t.writes.truncate(PATHS);
    t.depends.retain(|d| *d != kept && *d != gone);
    t.main |= g.main;
}

/// Two paths meet when they are the same or one is a folder the other is under.
fn meet(a: &str, b: &str) -> bool {
    let (a, b) = (a.trim_end_matches('/'), b.trim_end_matches('/'));
    a == b || a == "." || b == "." || a.starts_with(&format!("{b}/")) || b.starts_with(&format!("{a}/"))
}

/// Whether what `a` reads or writes is what `b` writes, or what `a` writes is what `b` reads: they must run in order.
fn meets(a: &Given, b: &Given) -> bool {
    a.reads.iter().chain(&a.writes).any(|p| b.writes.iter().any(|q| meet(p, q))) || a.writes.iter().any(|p| b.reads.iter().any(|q| meet(p, q)))
}

/// The planner chained parts that share no file (`p3` waited for `p1` and `p2`, three projects in three folders), and one
/// part that stops stops all. A wait on an item that writes is taken off when the waiter names what it reads and neither the item nor anything it
/// comes after meets them (writes read, read written, written twice). Kept: a wait on a read-only item (what it found is not a file), a
/// waiter that names no path (it cannot be told apart), and any wait the paths do not clearly separate. Each is named `x after y`.
pub(super) fn apart(items: &mut [Given]) -> Vec<String> {
    let ids: BTreeSet<&str> = items.iter().map(|g| g.id.as_str()).collect();
    if ids.len() != items.len() || items.iter().any(|g| g.depends.iter().any(|d| !ids.contains(d.as_str()) || *d == g.id)) {
        return Vec::new();
    }
    let was = items.to_vec();
    let mut notes = Vec::new();
    for (i, g) in items.iter_mut().enumerate() {
        // an item that reads nothing has not said what it takes in (a final item that sums up writes alone): its waits stand
        if g.reads.is_empty() {
            continue;
        }
        g.depends.retain(|d| {
            let k = was.iter().position(|o| o.id == *d).unwrap_or(i);
            let mut group = before(&was, k);
            group.insert(k);
            let keep = was[k].writes.is_empty() || group.iter().any(|&j| meets(&was[i], &was[j]));
            if !keep {
                notes.push(format!("{} after {d}", g.id));
            }
            keep
        });
    }
    notes
}
