//! Words for time, from the screen's own clock: how long ago a row's event was, and how long until a window resets. The core says the
//! absolute time (`data.at_ms`, an entry's `resets_ms`) and a text made when it was sent; the screen puts its own words in place of that text on each
//! frame, and wakes only for the soonest moment one of the words on screen changes.

use crate::app::App;
use j4n_proto::{Json, Row};
use std::borrow::Cow;
use std::time::{Duration, Instant};

/// `now`, `59s`, `3m`, `2h1m`, `1d3h`, `12d`: how long ago, in about the width of `00:00`. A time in the future is `now`.
pub fn ago(secs: i64) -> String {
    let s = secs.max(0);
    let (d, h, m) = (s / 86_400, s % 86_400 / 3600, s % 3600 / 60);
    match s {
        0 => "now".into(),
        1..=59 => format!("{s}s"),
        60..=3599 => format!("{m}m"),
        3600..=86_399 if m == 0 => format!("{h}h"),
        3600..=86_399 => format!("{h}h{m}m"),
        _ if d >= 10 || h == 0 => format!("{d}d"),
        _ => format!("{d}d{h}h"),
    }
}

/// Milliseconds from now to the moment [`ago`] says another word, for an event `since_ms` ago (negative: it is still to come). Never 0.
pub fn ago_wait(since_ms: i64) -> u64 {
    if since_ms < 0 {
        return since_ms.unsigned_abs() + 1;
    }
    let unit: i64 = match since_ms / 1000 {
        0..=59 => 1000,
        60..=86_399 => 60_000,
        s if s < 864_000 => 3_600_000,
        _ => 86_400_000,
    };
    (unit - since_ms % unit) as u64
}

/// `1:40`, or `3d 4h` from two days: the time left to a reset. The same words the core says in `/usage`.
pub fn until(ms: u64) -> String {
    let m = ms / 60_000;
    if m >= 2 * 1440 { format!("{}d {}h", m / 1440, m % 1440 / 60) } else { format!("{}:{:02}", m / 60, m % 60) }
}

/// Milliseconds until [`until`] says another word, with `left_ms` left; none once it is spent.
pub fn until_wait(left_ms: u64) -> Option<u64> {
    let unit = if left_ms >= 2 * 1440 * 60_000 { 3_600_000 } else { 60_000 };
    (left_ms > 0).then(|| left_ms % unit + 1)
}

impl App {
    /// Tell the screen what time it is: `epoch_ms` at the instant `at`. Until it is told a row says the time of day the core sent, as a replay does.
    pub fn set_wall(&mut self, epoch_ms: u64, at: Instant) {
        self.wall = Some((epoch_ms, at));
        self.now_ms = Some(epoch_ms);
    }

    /// The clock moved on to `now`: the words of time are made again when one of them has changed.
    pub(crate) fn age_tick(&mut self, now: Instant) {
        let Some((ms, at)) = self.wall else { return };
        let ms = ms + u64::try_from(now.saturating_duration_since(at).as_millis()).unwrap_or(u64::MAX);
        self.now_ms = Some(ms);
        self.redraw |= self.age_due.get().is_some_and(|due| due <= ms);
    }

    /// How long until a word of time on the screen changes; none when the frame drew none, and then nothing wakes the screen for the clock.
    pub(crate) fn age_wait(&self, now: Instant) -> Option<Duration> {
        let (ms, at) = self.wall?;
        let due = self.age_due.get()?;
        let at_now = ms + u64::try_from(now.saturating_duration_since(at).as_millis()).unwrap_or(u64::MAX);
        Some(Duration::from_millis(due.saturating_sub(at_now)))
    }

    /// The frame that is about to be drawn has seen no word of time yet.
    pub(crate) fn age_reset(&self) {
        self.age_due.set(None);
    }

    /// A word on the screen changes in `wait_ms`.
    fn age_note(&self, now: u64, wait_ms: u64) {
        let due = now + wait_ms;
        self.age_due.set(Some(self.age_due.get().map_or(due, |d| d.min(due))));
    }

    /// The rows as the screen says them now: a row with `data.at_ms` ends in how long ago it was (`look.time`: not with `clock`), and the entries of a held
    /// view with a `resets_ms` count down. The rows come back as they are, borrowed, when there is nothing to say or no clock to say it by.
    pub(crate) fn aged<'a>(&self, rows: &'a [Row]) -> Cow<'a, [Row]> {
        let Some(now) = self.now_ms else { return Cow::Borrowed(rows) };
        let ago = self.time_ago;
        let live = |r: &Row| r.data.as_ref().is_some_and(|d| (ago && d.get("at_ms").is_some_and(Json::is_u64)) || d["view"]["value"].is_array());
        if !rows.iter().any(live) {
            return Cow::Borrowed(rows);
        }
        let mut out = rows.to_vec();
        for r in out.iter_mut().filter(|r| live(r)) {
            let at = r.data.as_ref().and_then(|d| d["at_ms"].as_u64()).filter(|_| ago);
            if let (Some(at), Some(c)) = (at, r.cells.last_mut()) {
                let since = now as i64 - at as i64;
                c.text = ago_of(since);
                self.age_note(now, ago_wait(since));
            }
            let Some(entries) = r.data.as_mut().and_then(|d| d.pointer_mut("/view/value")).and_then(Json::as_array_mut) else { continue };
            for e in entries {
                let (Some(at), Some(left)) = (e["resets_ms"].as_u64(), e["left"].as_str().map(str::to_string)) else { continue };
                let rest = at.saturating_sub(now);
                if let Some(note) = e["note"].as_str() {
                    e["note"] = Json::String(note.replacen(&format!("resets in {left}"), &format!("resets in {}", until(rest)), 1));
                }
                if let Some(wait) = until_wait(rest) {
                    self.age_note(now, wait);
                }
            }
        }
        Cow::Owned(out)
    }
}

fn ago_of(since_ms: i64) -> String {
    ago(since_ms.div_euclid(1000))
}
