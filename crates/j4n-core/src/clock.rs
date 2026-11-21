//! The time a Log, a timer or a fold reads. The real one is the system's; tests move a [`ManualClock`] by hand, so no
//! test of an hour waits an hour.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Milliseconds since 1970.
pub trait Clock: Send + Sync {
    fn now_ms(&self) -> u64;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
    }
}

/// A clock that only moves when told to.
#[derive(Debug, Default)]
pub struct ManualClock(AtomicU64);

impl ManualClock {
    pub fn at(ms: u64) -> ManualClock {
        ManualClock(AtomicU64::new(ms))
    }

    pub fn set(&self, ms: u64) {
        self.0.store(ms, Ordering::SeqCst);
    }

    pub fn advance(&self, ms: u64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

/// `struct tm` as the C library of a 64-bit unix lays it out (glibc, musl, macOS): the nine counters, then the offset east of UTC and the zone's name.
#[repr(C)]
struct Tm {
    counters: [i32; 9],
    gmtoff: i64,
    zone: *const std::ffi::c_char,
}

unsafe extern "C" {
    fn localtime_r(time: *const i64, out: *mut Tm) -> *mut Tm;
}

/// How many seconds the machine's zone is east of UTC at `epoch` (seconds since 1970); 0 when the zone cannot be read.
pub fn offset_at(epoch: i64) -> i64 {
    let mut tm = Tm { counters: [0; 9], gmtoff: 0, zone: std::ptr::null() };
    // SAFETY: `epoch` and `tm` are live for the call, and `Tm` is the layout of the C library's `struct tm`; `localtime_r` writes only into `tm`.
    let done = unsafe { localtime_r(&epoch, &mut tm) };
    if done.is_null() { 0 } else { tm.gmtoff }
}

/// The time `at` as the operator reads it, `14:20`, or `Tue 14:20` when it is not the day of `now`; both in seconds since 1970, in the zone
/// that `offset` gives (seconds east of UTC at the instant it is asked about).
pub fn when(at: i64, now: i64, offset: impl Fn(i64) -> i64) -> String {
    let (here, today) = (at + offset(at), now + offset(now));
    let (day, secs) = (here.div_euclid(86_400), here.rem_euclid(86_400));
    let clock = format!("{:02}:{:02}", secs / 3600, secs % 3600 / 60);
    if day == today.div_euclid(86_400) {
        return clock;
    }
    // 1970-01-01 was a Thursday
    format!("{} {clock}", ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"][(day + 4).rem_euclid(7) as usize])
}

/// [`when`] in the machine's zone, from the system clock.
pub fn when_local(at: i64) -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    when(at, now, offset_at)
}
