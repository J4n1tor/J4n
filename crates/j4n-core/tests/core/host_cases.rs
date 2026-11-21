//! The hosts that bind a child to themselves by environment variables are rows of kind `host`, and what they name is taken out of
//! every child's environment; the operator's own variables are not named and stay, but for those named like a key, which go too.

use j4n_core::host::{HostRow, Scrub, key_like};
use j4n_core::table::{Layer, LayerKind, SourceFile, Tables};

#[test]
fn the_base_rows_name_the_variables_that_bound_the_child_in_the_log_of_2026_11_15() {
    let s = Scrub::base();
    for name in [
        "CLAUDE_CODE_MESSAGING_SOCKET", "CLAUDE_CODE_MESSAGING_TOKEN", "CLAUDECODE", "CLAUDE_CODE_SESSION_ID", "CLAUDE_CODE_HOST_SESSION_ID", "CLAUDE_CODE_CHILD_SESSION", "CLAUDE_CODE_ENTRYPOINT",
        "CLAUDE_CODE_USER_EMAIL", "CLAUDE_CODE_ACCOUNT_UUID", "CLAUDE_CODE_ORGANIZATION_UUID", "CLAUDE_PID", "CLAUDE_AGENT_SDK_VERSION", "CLAUDE_DESKTOP_STARTED_PATH", "CLAUDE_EFFORT",
        "CODEX_THREAD_ID", "OPENCODE_PID", "OPENCODE_SERVER_PASSWORD", "PI_SESSION_ID", "GEMINI_CLI_IDE_AUTH_TOKEN", "HERDR_SOCKET_PATH", "HERDR_PANE_ID",
    ] {
        assert!(s.drops(name), "{name} is taken out");
    }
}

#[test]
fn what_the_operator_set_for_their_own_config_is_kept() {
    let s = Scrub::base();
    for name in [
        "PATH", "HOME", "LANG", "TERM", "ANTHROPIC_BASE_URL", "CLAUDE_CONFIG_DIR", "CLAUDE_CODE_USE_BEDROCK", "CLAUDE_CODE_ENABLE_TODO_TOOLS",
        "OPENCODE_CONFIG_CONTENT", "OPENCODE_CONFIG", "CODEX_HOME", "MCP_TIMEOUT", "J4N_HOME", "J4N_GUARD", "KEYBOARD", "MONKEY", "TOKENIZERS_PARALLELISM",
    ] {
        assert!(!s.drops(name) && !key_like(name), "{name} stays");
    }
}

#[test]
fn the_operators_keys_are_taken_by_their_names_ending_and_no_host_row_names_them() {
    let s = Scrub::base();
    for name in ["ANTHROPIC_API_KEY", "OPENROUTER_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN", "GEMINI_API_KEY", "GITHUB_TOKEN", "AWS_SECRET", "DB_PASSWORD", "SMTP_PASSWD"] {
        assert!(key_like(name) && !s.drops(name), "{name} is a key, taken out by its name and given back where a connector names it");
    }
    // what a J4n that J4n starts is given back: its keys, and never a host's token that a key's ending would let through
    let vars = [("OPENROUTER_API_KEY", "a"), ("CLAUDE_CODE_MESSAGING_TOKEN", "b"), ("OPENCODE_SERVER_PASSWORD", "c"), ("GEMINI_CLI_IDE_AUTH_TOKEN", "d"), ("PATH", "e")];
    let back = s.keys_of(vars.map(|(k, v)| (k.to_string(), v.to_string())));
    assert_eq!(back, [("OPENROUTER_API_KEY".to_string(), "a".to_string())], "{back:?}");
}

#[test]
fn a_row_of_the_operators_adds_a_host_and_a_prefix_of_nothing_takes_nothing() {
    let mine = Layer::new(LayerKind::User, "user", vec![SourceFile { path: "hosts/mine.toml".into(), text: "api = 1\n[[host]]\nid = \"acme\"\ndrop = [\"ACME_SESSION_ID\"]\ndrop_prefix = [\"ACME_SOCKET_\", \"\", \"A\"]\nsource = \"acme --help\"\n".into() }]);
    let (t, problems) = Tables::load(vec![mine]);
    assert!(problems.is_empty(), "{problems:?}");
    let s = Scrub::from_tables(&t);
    assert!(s.drops("ACME_SESSION_ID") && s.drops("ACME_SOCKET_PATH") && !s.drops("ACME_OTHER") && !s.drops("PATH") && !s.drops("A"), "a prefix of fewer than 3 characters is not one");
    let rows: Vec<HostRow> = t.list().unwrap();
    assert!(rows.iter().all(|r| !r.source.is_empty()), "every row says where its names were read");
}
