//! Everything outside the core: the process runner, places, dialects, the session pool, builtin tools.
//!
//! - [`proc`]: a program as a child in a process group of its own.
//! - [`pool`]: live children kept between turns, found by their session.
//! - [`claude`]: Claude Code as a doer, dialect `claude-stream`.
//! - [`manifest`]: the `[[connector]]` rows, and a doer made of each; [`template`] fills in their arguments.
//! - [`oneshot`] (`text`, `json-last`, `jsonl-map` with [`jsonmap`]), [`acp`] and [`pi`]: the other dialects.
//! - [`tools`]: the builtin tools of a model that works on its own (`list`, `read`, `grep`, `edit`, `write`, `run`, `git`).
//! - [`py`]: the tool `py`, Python in a kernel kept for the task.
//! - [`kithost`]: what J4n does when code in the `py` kernel asks for a decider, a model or a page.
//! - [`test`]: the scripted doer that plays its goal, for anything that drives J4n from outside.

pub mod account;
pub mod acp;
pub mod canary;
pub mod check;
pub mod decide;
pub mod decode;
pub mod digest;
pub mod claude;
pub mod command;
pub mod connect;
pub mod credits;
pub mod effects;
pub mod found;
pub mod hosts;
pub mod httpjson;
pub mod jsonmap;
pub mod learn;
pub mod limits;
pub mod kitexport;
pub mod kithost;
pub mod kitpkg;
pub mod kitoffer;
pub mod contexttool;
pub mod accesstool;
pub mod chattool;
pub mod envoytool;
pub mod telltool;
pub mod kitsock;
pub mod kituse;
pub mod manifest;
pub mod model;
pub mod modellist;
pub mod net;
pub mod oneshot;
pub mod opening;
pub mod pi;
pub mod permit;
pub mod place;
pub mod pool;
pub mod proc;
pub mod py;
pub mod relevance;
pub mod serve;
pub mod sets;
pub mod template;
pub mod test;
pub mod tools;
pub mod watch;

pub use claude::Claude;
pub use test::TestDoer;
