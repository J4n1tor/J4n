#!/bin/sh
# A `claude` that plays a recorded stream: for each user line on its stdin it prints the frames of $FAKE_FIXTURE up to the
# next `result` frame; at a `control_request` it waits for the response (as the real one does) and goes on. Everything it is
# sent is appended to $FAKE_LOG, and each time it is started it logs `start <arguments>`. It also answers `--version` and
# `auth status`. The tests of j4n-connect and of j4n play their recordings through it.
case "$1" in
  --version) echo "2.1.309 (Claude Code)"; exit 0;;
  auth) echo '{"loggedIn": true, "authMethod": "claude.ai"}'; exit 0;;
esac
log="${FAKE_LOG:-/dev/null}"
echo "start $*" >> "$log"
[ -n "$FAKE_ENV_LOG" ] && env | sort > "$FAKE_ENV_LOG"
exec 3< "$FAKE_FIXTURE"
while IFS= read -r line; do
  printf '%s\n' "$line" >> "$log"
  case "$line" in
    *'"type":"user"'*)
      while IFS= read -r f <&3; do
        case "$f" in '#'*|'') continue;; esac
        printf '%s\n' "$f"
        case "$f" in
          *'"type":"control_request"'*) IFS= read -r resp; printf '%s\n' "$resp" >> "$log";;
          *'"type":"result"'*) break;;
        esac
      done;;
  esac
done
