//! The places that need a program, a git repository or a wall to exist: `worktree`, `box` and the check that runs
//! where the work was done. `here` is the core's. A place is registered by name in the loop's config; the names are the ones a
//! task's `place` says.

pub mod boxed;
pub mod view;
pub mod worktree;

pub use boxed::Boxed;
pub use worktree::{Apply, Worktree};
