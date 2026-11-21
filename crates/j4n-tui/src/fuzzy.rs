//! Finding a node by a few of its letters (`⌃k`): a query matches a title when its letters are in it in order,
//! and the better the match the nearer the top: letters that follow each other, letters that start a word, a match at the start, a short
//! title. The search walks the tree once and keeps the best few, so it costs a pass over the titles and no index.

use j4n_proto::{Node, Row};

/// How many results a search keeps.
pub const KEEP: usize = 12;

fn lower(c: char) -> char {
    if c.is_ascii() { c.to_ascii_lowercase() } else { c.to_lowercase().next().unwrap_or(c) }
}

fn starts_word(prev: Option<char>) -> bool {
    prev.is_none_or(|p| !p.is_alphanumeric())
}

/// The score of one way of laying `query` over `text`: `lay` says where each letter goes (a position in `text`, ascending).
fn worth(text: &[char], lay: &[usize]) -> i32 {
    let mut total = 0;
    let mut run = 0;
    for (k, &i) in lay.iter().enumerate() {
        run = if k > 0 && lay[k - 1] + 1 == i { run + 1 } else { 0 };
        total += 10 + run * 15 + if starts_word(i.checked_sub(1).map(|p| text[p])) { 12 } else { 0 };
    }
    total - (text.len() as i32) / 2
}

/// The score of `query` (already lowercase) in `text`, or `None` when its letters are not all there in order. Two ways of laying the
/// letters over the title are tried — each at its first place, and each at the first place that starts a word — and the better counts.
pub fn score(query: &[char], text: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    // most titles fail here, and cost no allocation
    let mut q = query.iter().peekable();
    if text.is_ascii() {
        for b in text.bytes() {
            if q.peek().is_some_and(|w| b.to_ascii_lowercase() as char == **w) {
                q.next();
            }
        }
    } else {
        for c in text.chars() {
            if q.peek().is_some_and(|w| lower(c) == **w) {
                q.next();
            }
        }
    }
    q.peek().is_none().then_some(())?;
    let chars: Vec<char> = text.chars().collect();
    let low: Vec<char> = chars.iter().map(|c| lower(*c)).collect();
    let mut best: Option<i32> = None;
    for words in [false, true] {
        let (mut lay, mut at) = (Vec::with_capacity(query.len()), 0usize);
        for (k, w) in query.iter().enumerate() {
            let left = query.len() - k - 1;
            let limit = low.len().saturating_sub(left);
            let found = (at..limit).find(|&i| low[i] == *w && (!words || starts_word(i.checked_sub(1).map(|p| chars[p]))));
            // a letter that starts no word later is taken where it first is
            let found = found.or_else(|| (at..limit).find(|&i| low[i] == *w));
            let Some(i) = found else { break };
            lay.push(i);
            at = i + 1;
        }
        if lay.len() == query.len() {
            best = best.max(Some(worth(&chars, &lay)));
        }
    }
    best
}

/// Where the letters of `query` are in `text`, as character positions, the way [`score`] finds them.
pub fn positions(query: &[char], text: &str) -> Vec<usize> {
    let mut q = query.iter().peekable();
    let mut out = Vec::new();
    for (i, c) in text.chars().enumerate() {
        if q.peek().is_some_and(|w| lower(c) == **w) {
            q.next();
            out.push(i);
        }
    }
    out
}

/// A node that matched: where it is, and how well.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub id: String,
    pub title: String,
    /// The titles above it, joined: `Settings › look`.
    pub above: String,
    /// The slot name of the node's icon (a command has none).
    pub icon: String,
    pub score: i32,
    /// A command of the palette, and not a node: `id` is the key of its row.
    pub command: bool,
    /// A chat of the pinned list: `id` is its id, and `⏎` goes into it.
    pub chat: bool,
}

/// The best matches of `query` among the titled nodes under `root`, best first; the order of the tree breaks a tie. An empty query
/// matches every titled node in the order of the tree, so the first `KEEP` are what a search shows before anything is typed.
pub fn search(root: &Node, query: &str) -> Vec<Hit> {
    let q: Vec<char> = query.chars().filter(|c| !c.is_whitespace()).map(lower).collect();
    let mut best: Vec<(i32, usize, Vec<&Node>)> = Vec::new();
    let mut order = 0usize;
    let mut path: Vec<&Node> = Vec::new();
    fn walk<'a>(n: &'a Node, q: &[char], path: &mut Vec<&'a Node>, order: &mut usize, best: &mut Vec<(i32, usize, Vec<&'a Node>)>) {
        // a tab that is a group lists its own page under the group, once
        let owns = n.children.first().is_some_and(|c| c.id.strip_suffix(".own") == Some(n.id.as_str()));
        if !n.title.is_empty()
            && !owns
            && let Some(s) = score(q, &n.title)
        {
            let worst = best.get(KEEP - 1).map(|b| b.0);
            if best.len() < KEEP || worst.is_some_and(|w| s > w) {
                let mut p = path.clone();
                p.push(n);
                let at = best.iter().position(|b| b.0 < s).unwrap_or(best.len());
                best.insert(at, (s, *order, p));
                best.truncate(KEEP);
            }
        }
        *order += 1;
        path.push(n);
        for c in &n.children {
            walk(c, q, path, order, best);
        }
        path.pop();
    }
    walk(root, &q, &mut path, &mut order, &mut best);
    best.into_iter()
        .map(|(score, _, p)| {
            let (last, above) = p.split_last().map_or((None, &[][..]), |(l, a)| (Some(*l), a));
            Hit {
                id: last.map(|n| n.id.clone()).unwrap_or_default(),
                title: last.map(|n| n.title.clone()).unwrap_or_default(),
                // a page without a mark of its own takes the mark of the tab it is in
                icon: p.iter().rev().map(|n| n.icon.as_str()).find(|i| !i.is_empty()).unwrap_or_default().to_string(),
                above: above.iter().filter(|n| !n.title.is_empty()).map(|n| n.title.as_str()).collect::<Vec<_>>().join(" › "),
                score,
                command: false,
                chat: false,
            }
        })
        .collect()
}

/// The best matches among the nodes and the commands of the palette: a command is found by its words, and says what it does and
/// what follows its words. The nodes come before a command that scores the same.
pub fn search_with(root: &Node, palette: &[Row], query: &str) -> Vec<Hit> {
    let mut hits = search(root, query);
    let q: Vec<char> = query.chars().filter(|c| !c.is_whitespace()).map(lower).collect();
    for r in palette {
        let text = |i: usize| r.cells.get(i).map_or("", |c| c.text.as_str());
        let Some(score) = score(&q, text(0)) else { continue };
        let above = [text(1), text(2)].iter().filter(|t| !t.is_empty()).copied().collect::<Vec<_>>().join("  ");
        hits.push(Hit { id: r.key.clone(), title: text(0).to_string(), above, icon: String::new(), score, command: true, chat: false });
    }
    hits.sort_by_key(|h| std::cmp::Reverse(h.score));
    hits.truncate(KEEP);
    grouped(hits)
}

/// Where a hit is listed: the nodes under the same parent together, and the commands together.
pub fn group_of(h: &Hit) -> (bool, &str) {
    (h.command, if h.command { "" } else { &h.above })
}

/// The hits grouped, the best hit's group first and each group's hits in the order they were: the jump lists a parent once, and what the
/// arrows step through is the order it is drawn in.
fn grouped(hits: Vec<Hit>) -> Vec<Hit> {
    let mut groups: Vec<(bool, String)> = Vec::new();
    for h in &hits {
        let (c, a) = group_of(h);
        if !groups.iter().any(|(gc, ga)| *gc == c && ga == a) {
            groups.push((c, a.to_string()));
        }
    }
    groups.iter().flat_map(|(c, a)| hits.iter().filter(move |h| group_of(h) == (*c, a.as_str())).cloned()).collect()
}
