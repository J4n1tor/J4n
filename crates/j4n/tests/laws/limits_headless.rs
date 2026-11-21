//! A run with no screen goes on as a login that is open. The real binary, piped, with a `claude` that is told not now when it is run as the
//! operator's own login and works as the account `claude-1`: a screen moves such a run to another login, a piped run waits out the limit.

use crate::run_cases::{Dirs, j4n, kinds, log_events};
use std::fs;
use std::os::unix::fs::PermissionsExt;

/// A `claude` for two logins: as `claude-1` it answers `ok`; as anyone else its window says `rejected` for two hours.
const CLAUDE: &str = r#"#!/bin/sh
case "$1" in
  --version) echo "2.1.323 (Claude Code)"; exit 0;;
  auth) echo '{"loggedIn":true,"email":"a@b.c","subscriptionType":"max","authMethod":"claude.ai"}'; exit 0;;
esac
now=$(date +%s)
while IFS= read -r line; do
  case "$line" in
    *'"type":"user"'*)
      echo '{"type":"system","subtype":"init","cwd":"/work","session_id":"00000000-0000-4000-8000-0000000000c1","model":"claude-haiku-5-5","permissionMode":"default","tools":[]}'
      case "$CLAUDE_CONFIG_DIR" in
        *claude-1*)
          echo '{"type":"rate_limit_event","rate_limit_info":{"status":"allowed","resetsAt":'$((now+3600))',"rateLimitType":"five_hour","unifiedWindows":{"five_hour":{"status":"allowed","utilization":0.1,"resetsAt":'$((now+3600))'}}}}'
          echo '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"ok"}]},"session_id":"00000000-0000-4000-8000-0000000000c1"}'
          echo '{"type":"result","subtype":"success","is_error":false,"result":"ok","session_id":"00000000-0000-4000-8000-0000000000c1","total_cost_usd":0.001,"usage":{"input_tokens":3,"output_tokens":1}}';;
        *)
          echo '{"type":"rate_limit_event","rate_limit_info":{"status":"rejected","resetsAt":'$((now+7200))',"rateLimitType":"five_hour","unifiedWindows":{"five_hour":{"status":"rejected","utilization":1.0,"resetsAt":'$((now+7200))'}}}}';;
      esac;;
  esac
done
"#;

#[test]
fn a_piped_run_whose_own_login_is_limited_goes_on_as_the_account() {
    let d = Dirs::new("limits-headless-piped-run-whose");
    let program = d.dir.join("bin/claude");
    fs::write(&program, CLAUDE).unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    // the account is a row of the operator's, its folder is made, and `claude auth status` says it is signed in as it
    fs::create_dir_all(d.j4n_home().join("account")).unwrap();
    fs::write(d.j4n_home().join("account/claude-1.toml"), "api = 1\n[[account]]\nid = \"claude-1\"\nconnector = \"claude\"\n").unwrap();
    for sub in ["home", "bin", "browser"] {
        fs::create_dir_all(d.j4n_home().join("accounts/claude-1").join(sub)).unwrap();
    }
    let path = format!("{}:{}", d.dir.join("bin").display(), std::env::var("PATH").unwrap_or_default());
    let env = [("J4N_CLAUDE", program.to_str().unwrap()), ("PATH", path.as_str())];
    let o = j4n(&d, &["--doer", "claude", "--model", "haiku", "--approval", "full", "--wall", "40s"], Some(":account check claude-1\nReply with the single word ok.\n"), &env);
    let events = log_events(&d);
    assert_eq!(o.rc, 0, "{}\n{}\n{:?}", o.out, o.err, kinds(&events));
    assert!(o.out.contains("ok") && o.secs < 30.0, "{}\n{}\n{:?}", o.out, o.err, kinds(&events));
    let moved: Vec<_> = events.iter().filter(|e| e["kind"] == "task.moved").collect();
    assert_eq!((moved.len(), moved[0]["data"]["to"].as_str()), (1, Some("claude-1")), "{:?}", kinds(&events));
    assert!(moved[0]["data"]["why"].as_str().unwrap().starts_with("default is limited, back in"), "{}", moved[0]["data"]);
    assert!(events.iter().any(|e| e["kind"] == "account.routed" && e["data"]["moved"] == true));
}
