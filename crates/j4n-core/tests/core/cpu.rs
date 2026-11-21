#![allow(dead_code)] // the two test crates that include this file use a part each
//! How long a piece of work took, for a test that holds a speed (`docs/TESTING.md`): the CPU the calling thread spent, not the wall
//! clock. The wall clock of a gate that runs beside other builds and tests counts the time a thread waited for a core, which says
//! nothing of the code (250 ms for 100 ms of work, a 20 s allowance spent on 4 s of it). Other threads' work does not count either,
//! so a test measures what it runs on its own thread. The tui tests include this file by path.

use std::time::Duration;

#[repr(C)]
struct Timespec {
    sec: i64,
    nsec: i64,
}

unsafe extern "C" {
    fn clock_gettime(clock: i32, to: *mut Timespec) -> i32;
}

/// `CLOCK_THREAD_CPUTIME_ID` (Linux).
const THREAD_CPU: i32 = 3;

/// The CPU time this thread has used so far.
pub fn now() -> Duration {
    let mut t = Timespec { sec: 0, nsec: 0 };
    // SAFETY: `t` is a valid timespec for the call to fill in
    assert_eq!(unsafe { clock_gettime(THREAD_CPU, &mut t) }, 0, "the thread's CPU clock");
    Duration::new(t.sec.max(0) as u64, t.nsec.clamp(0, 999_999_999) as u32)
}

/// The CPU time `work` took on this thread.
pub fn of<T>(work: impl FnOnce() -> T) -> (T, Duration) {
    let from = now();
    let out = work();
    (out, now() - from)
}

/// The least CPU time of `runs` goes of `work`: a go can be slowed (a cold cache, the core shared with another thread's work) and cannot
/// be made faster than the code is, so the least of a few is what the code costs, and a budget that holds for it is not a matter of luck.
pub fn least_of(runs: u32, mut work: impl FnMut()) -> Duration {
    (0..runs).map(|_| of(&mut work).1).min().expect("at least one run")
}

/// The least CPU time of up to `tries` goes of `work`, stopping at the first that is within `limit`: only a slowness that every go
/// has is a slowness of the code. (The first go is the only one that costs anything when the code is as fast as it should be.)
pub fn within(limit: Duration, tries: u32, mut work: impl FnMut()) -> Duration {
    let mut least = of(&mut work).1;
    for _ in 1..tries {
        if least <= limit {
            break;
        }
        least = least.min(of(&mut work).1);
    }
    least
}

/// The least of up to `tries` ratios of `work`'s CPU to `reference`'s, stopping at the first that is at most `limit`. Each ratio times
/// the two back to back on this thread, so a load that slows the machine (a gate with eight test threads on eight cores: a go
/// took 1.8 times its CPU) slows both alike and the ratio stays what the code costs in units of the reference; a number of
/// milliseconds does not. The reference should be work of the same kind (a walk over the same bytes), so that what slows one slows
/// the other. A ratio can be pushed up by a slow go and not down below what the code is, so only a ratio every pair has fails.
pub fn ratio_within(limit: f64, tries: u32, mut reference: impl FnMut(), mut work: impl FnMut()) -> f64 {
    let mut least = f64::INFINITY;
    for _ in 0..tries {
        let (a, b) = (of(&mut reference).1, of(&mut work).1);
        least = least.min(b.as_secs_f64() / a.as_secs_f64().max(1e-9));
        if least <= limit {
            break;
        }
    }
    least
}

/// A stopwatch of the CPU this thread spends, for a test that holds a speed across several statements: [`start`], then `elapsed()`.
pub struct Watch(Duration);

pub fn start() -> Watch {
    Watch(now())
}

impl Watch {
    pub fn elapsed(&self) -> Duration {
        now() - self.0
    }
}

/// What the reference below costs on a quiet machine, in CPU: the least of forty goes, set once, on a machine that was not busy.
const QUIET: Duration = Duration::from_micros(QUIET_MICROS);
const QUIET_MICROS: u64 = 68_000;

/// A piece of work to hold a speed against: text made, sorted and read, which is what the checks that are timed here mostly do (small
/// allocations, comparisons, a walk over bytes), big enough that it does not fit in a cache, as the work that is timed does not.
fn reference() -> u64 {
    let mut v: Vec<String> = (0..100_000u32).map(|n| format!("item-{}", n.wrapping_mul(2_654_435_761) % 1_000_003)).collect();
    v.sort();
    v.iter().fold(0u64, |h, s| h.wrapping_mul(31).wrapping_add(s.len() as u64 + u64::from(s.as_bytes()[5])))
}

/// How many times slower than a quiet machine this thread is right now: the reference, timed here, against what it costs on a quiet
/// one (never below one). CPU time is not the work alone on a machine that other builds share: two threads on one core's two halves,
/// a cache that another process fills, a clock that slows when every core is busy each make the same work cost two or three times as
/// much CPU, and they do it to the reference as much as to the work.
pub fn slowness() -> f64 {
    let least = least_of(5, || {
        std::hint::black_box(reference());
    });
    (least.as_secs_f64() / QUIET.as_secs_f64()).max(1.0)
}

/// The CPU `measure` reports, held to `limit` as a quiet machine would take it: the limit is stretched by how much slower than a quiet
/// machine the thread was just before the work and just after it (the greater of the two). A work that costs more than the limit
/// allows on a quiet machine costs more than the stretched limit on any machine, so the claim is still one about the code; what a
/// busy machine adds to both is not.
pub fn assert_within(limit: Duration, what: &str, measure: impl FnOnce() -> Duration) {
    let before = slowness();
    let took = measure();
    let slow = before.max(slowness());
    assert!(took <= limit.mul_f64(slow), "{what}: {took:?} of CPU, over {limit:?} on a machine {slow:.1} times slower than a quiet one");
}
