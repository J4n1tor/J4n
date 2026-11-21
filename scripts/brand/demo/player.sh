#!/bin/sh
# A paced `claude` for the header GIF: like fixtures/claude-frames/fake-claude.sh, it plays $FAKE_FIXTURE one turn per
# user line and waits for the answer at a `control_request`; a line `# wait S` sleeps S seconds first, so the screen has
# time to draw each step. No model is called.
case "$1" in
  --version) echo "2.1.309 (Claude Code)"; exit 0;;
  auth) echo '{"loggedIn": true, "authMethod": "claude.ai"}'; exit 0;;
esac
exec 3< "$FAKE_FIXTURE"
while IFS= read -r line; do
  case "$line" in
    *'"type":"user"'*)
      while IFS= read -r f <&3; do
        case "$f" in
          '# wait '*) sleep "${f#\# wait }"; continue;;
          '#'*|'') continue;;
        esac
        printf '%s\n' "$f"
        case "$f" in
          *'"type":"control_request"'*) IFS= read -r resp;;
          *'"type":"result"'*) break;;
        esac
      done;;
  esac
done
