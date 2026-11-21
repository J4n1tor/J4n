//! The tools that reach out: the web (`web`) and MCP servers, through the public API only. Servers
//! are plain threads on the loopback, so what is tested is the real client and the real wire.

mod mcp_cases;
mod server;
mod web_cases;
