#!/bin/sh
# A `pi` or `omp` in RPC mode that plays recorded frames: it answers `get_session_stats`, accepts `prompt` and then prints the
# frames of $FAKE_TURN (comments skipped) as the turn; a prompt that says `hang` starts a turn that runs until it is aborted.
# $FAKE_FAMILY=omp makes it say `ready` first; $FAKE_DIALOG=1 has it ask for an approval (omp's `select`) before the turn;
# $FAKE_REFUSE=1 makes it refuse the prompt; $FAKE_SESSION names the session; $FAKE_CTX (tokens) and $FAKE_WINDOW say how full
# its context is, $FAKE_CTX_AFTER how full once it has been told to `compact` (which $FAKE_COMPACT_FAIL=1 refuses). Everything
# it is sent is appended to $FAKE_LOG, and each start is logged with its arguments. It also answers `--version`, and in print mode (`-p`) it prints $FAKE_TURN once.
case "$1" in --version) echo "1.4.0"; exit 0;; esac
log="${FAKE_LOG:-/dev/null}"
echo "start $*" >> "$log"
for a in "$@"; do
  [ "$a" = "-p" ] && { grep -v '^#' "$FAKE_TURN"; exit "${FAKE_PRINT_EXIT:-0}"; }
done
[ "${FAKE_RPC:-1}" = 0 ] && { echo "this program does not speak rpc" >&2; exit 2; }
[ "${FAKE_FAMILY:-pi}" = omp ] && echo '{"type":"ready","protocolVersion":1}'
sid="${FAKE_SESSION:-pi-sess-1}"
compacted=0
ctx() { [ -n "$FAKE_CTX" ] || return 0; t="$FAKE_CTX"; [ "$compacted" = 1 ] && t="${FAKE_CTX_AFTER:-$FAKE_CTX}"; printf ',"contextUsage":{"tokens":%s,"contextWindow":%s}' "$t" "${FAKE_WINDOW:-1000}"; }
id_of() { printf '%s' "$1" | sed -n 's/.*"id":"\([^"]*\)".*/\1/p'; }
ok() { d="$3"; [ -z "$d" ] && d='{}'; printf '{"id":"%s","type":"response","command":"%s","success":true,"data":%s}\n' "$1" "$2" "$d"; }
# $FAKE_PACE (seconds) is the wait after each frame; the time the first `toolcall_start` goes out is written to $FAKE_T0 (nanoseconds)
play() {
  if [ -z "$FAKE_PACE" ]; then grep -v '^#' "$FAKE_TURN"; return; fi
  grep -v '^#' "$FAKE_TURN" | while IFS= read -r frame; do
    case "$frame" in *toolcall_start*) [ -n "$FAKE_T0" ] && [ ! -e "$FAKE_T0" ] && date +%s%N > "$FAKE_T0";; esac
    printf '%s\n' "$frame"
    sleep "$FAKE_PACE"
  done
}
while IFS= read -r line; do
  printf '%s\n' "$line" >> "$log"
  id=$(id_of "$line")
  case "$line" in
    *'"type":"get_session_stats"'*) ok "$id" get_session_stats "{\"sessionId\":\"$sid\"$(ctx)}";;
    *'"type":"compact"'*)
      if [ "${FAKE_COMPACT_FAIL:-0}" = 1 ]; then printf '{"id":"%s","type":"response","command":"compact","success":false,"error":"Nothing to compact"}\n' "$id"
      else compacted=1; ok "$id" compact '{"tokensBefore":900,"estimatedTokensAfter":120}'; fi;;
    *'"type":"set_event_filter"'*) ok "$id" set_event_filter;;
    *'"type":"steer"'*) ok "$id" steer;;
    *'"type":"abort"'*) ok "$id" abort; echo '{"type":"agent_end"}'; echo '{"type":"agent_settled"}'; echo '{"type":"prompt_result"}'; echo '{"type":"session_settled"}';;
    *'"type":"prompt"'*)
      if [ "${FAKE_REFUSE:-0}" = 1 ]; then
        printf '{"id":"%s","type":"response","command":"prompt","success":false,"error":"No API key found for the selected model."}\n' "$id"
        continue
      fi
      if [ "${FAKE_NOAGENT:-0}" = 1 ]; then ok "$id" prompt '{"agentInvoked":false}'; continue; fi
      ok "$id" prompt '{"disposition":"started"}'
      case "$line" in
        *'hang'*) echo '{"type":"agent_start"}';;
        *)
          if [ "${FAKE_DIALOG:-0}" = 1 ]; then
            echo '{"type":"agent_start"}'
            echo '{"type":"extension_ui_request","id":"ui1","method":"select","title":"Allow tool: bash\nCommand: make build","options":["Approve","Deny"]}'
            IFS= read -r resp; printf '%s\n' "$resp" >> "$log"
            case "$resp" in *'"value":"Approve"'*) play | grep -v '"agent_start"';; *) echo '{"type":"notice","message":"denied"}'; echo '{"type":"agent_end"}'; echo '{"type":"agent_settled"}'; echo '{"type":"prompt_result"}'; echo '{"type":"session_settled"}';; esac
          else
            play
          fi
          if [ "${FAKE_LATE:-0}" = 1 ]; then
            echo '{"type":"extension_ui_request","id":"late1","method":"select","title":"Allow tool: bash\nCommand: rm -rf x","options":["Approve","Deny"]}'
            IFS= read -r resp; printf '%s\n' "$resp" >> "$log"
          fi;;
      esac;;
    *'"type":"extension_ui_response"'*) ;;
  esac
done
