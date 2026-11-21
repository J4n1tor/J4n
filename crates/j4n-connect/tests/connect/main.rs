//! The process runner, the session pool, Claude Code as a doer (dialect `claude-stream`) and the scripted
//! `test` doer, through the public API only. A child that plays recorded frames stands in for `claude` (the hermetic runner keeps the real one off the PATH), so
//! what is tested is the real runner, the real session and the real loop; `J4N_LIVE=1` runs the same turn against the
//! installed CLI.

mod account_cases;
mod acp_cases;
mod bench;
mod boxed_cases;
mod check_cases;
mod command_cases;
mod connect_cases;
mod claude_cases;
mod decide_cases;
mod decode_cases;
mod frames_cases;
mod hosts_cases;
mod httpjson_cases;
mod limits_cases;
mod live;
mod jsonmap_cases;
mod bridge_cases;
mod kit_cases;
mod kituse_cases;
mod librarian_cases;
mod tell_cases;
mod chat_tool_cases;
mod kitpkg_cases;
mod kitsock_cases;
mod login_cases;
mod manifest_cases;
mod mock;
mod model_cases;
mod model_doer_cases;
mod models_of_cases;
mod net_cases;
mod oneshot_cases;
mod permit_cases;
mod pi_cases;
mod places_loop;
mod pool_cases;
mod proc_cases;
mod py_cases;
mod template_cases;
mod worktree_cases;
mod test_cases;
mod log_receipt_cases;
mod taint_cases;
mod tools_cases;
mod relevance_canary_cases;
mod digest_cases;
mod source_kinds_cases;
mod view_cases;
mod watch_cases;
mod world;
