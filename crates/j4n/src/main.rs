//! The `j4n` binary: everything is in the library, so the tests can reach it.

fn main() {
    undumpable();
    std::process::exit(j4n::run(std::env::args().skip(1).collect()));
}

/// The keys J4n reads are in its memory and its environment. On Linux the process is made not dumpable before anything is read: it leaves
/// no core dump, and no other process of the same user (an agent J4n started among them) may read its `/proc/<pid>/mem` or `environ`, or
/// attach to it. The programs it starts are dumpable again from their own start. `J4N_DUMPABLE=1` leaves it as it was, for a debugger.
#[cfg(target_os = "linux")]
fn undumpable() {
    // SAFETY: the declaration matches libc's `prctl(2)` (integer arguments and result, `unsigned long` as `u64` on the 64-bit Linux this
    // runs on), and the process links libc.
    unsafe extern "C" {
        fn prctl(option: i32, arg2: u64, arg3: u64, arg4: u64, arg5: u64) -> i32;
    }
    const PR_SET_DUMPABLE: i32 = 4;
    if std::env::var_os("J4N_DUMPABLE").is_some_and(|v| v == "1") {
        return;
    }
    // SAFETY: one call of integer arguments, before any thread is started.
    unsafe { prctl(PR_SET_DUMPABLE, 0, 0, 0, 0) };
}

#[cfg(not(target_os = "linux"))]
fn undumpable() {}
