//! `scroll`: the one rule that keeps the row the keys are on, and all its lines, in sight: the first place shown moves only as
//! far as the row needs, so the view moves only when the keys leave it.

/// The first place to show, of `take` places, so that `first..=last` are in sight: `top` when they are, else the least move (never past `first`).
pub fn keep(top: u64, first: u64, last: u64, take: u64) -> u64 {
    if first < top { first } else if last >= top + take { (last + 1 - take).min(first) } else { top }
}

/// The same for rows `height(i)` lines high: the first row to show, of `take` lines, so that rows `top..=sel` fit.
pub fn keep_by(top: u64, sel: u64, take: usize, height: impl Fn(u64) -> usize) -> u64 {
    let mut top = top.min(sel);
    let mut sum: usize = (top..=sel).map(&height).sum();
    while top < sel && sum > take {
        sum -= height(top);
        top += 1;
    }
    top
}
