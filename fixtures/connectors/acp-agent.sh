#!/bin/sh
# An ACP agent in shell (the tests' stand-in for Gemini CLI, Goose and the others): JSON-RPC 2.0, one object per line, over
# stdin and stdout. It plays a scenario chosen by the words of the prompt, and logs everything it is sent, and each start with
# its arguments, to $ACP_LOG.
[ "$1" = "--version" ] && { echo "acp-agent 1.0 (fixture)"; exit 0; }
log="${ACP_LOG:-/dev/null}"
echo "start $*" >> "$log"
id_of() { printf '%s' "$1" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p'; }
say() { printf '%s\n' "$1"; }
update() { say "{\"jsonrpc\":\"2.0\",\"method\":\"session/update\",\"params\":{\"sessionId\":\"sess-1\",\"update\":$1}}"; }
chunk() { update "{\"sessionUpdate\":\"agent_message_chunk\",\"content\":{\"type\":\"text\",\"text\":\"$1\"}}"; }
finish() { say "{\"jsonrpc\":\"2.0\",\"id\":$1,\"result\":{\"stopReason\":\"$2\"}}"; }
while IFS= read -r line; do
  printf '%s\n' "$line" >> "$log"
  id=$(id_of "$line")
  case "$line" in
    *'"method":"initialize"'*)
      case "${ACP_NOLOAD:-}" in
        1) say "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"protocolVersion\":1,\"agentCapabilities\":{},\"authMethods\":[{\"id\":\"login\",\"name\":\"Sign in with a browser\"}]}}";;
        *) say "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"protocolVersion\":1,\"agentCapabilities\":{\"loadSession\":true},\"authMethods\":[]}}";;
      esac;;
    *'"method":"session/new"'*)
      if [ "${ACP_AUTH:-}" = 1 ]; then
        say "{\"jsonrpc\":\"2.0\",\"id\":$id,\"error\":{\"code\":-32000,\"message\":\"authentication required\"}}"
      else
        say "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"sessionId\":\"sess-1\"}}"
      fi;;
    *'"method":"session/load"'*)
      update '{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"an old line"}}'
      say "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{}}";;
    *'"method":"session/cancel"'*) ;;
    *'"method":"session/prompt"'*)
      case "$line" in
        *'"text":"hang'*)
          chunk "working "
          while IFS= read -r l; do
            printf '%s\n' "$l" >> "$log"
            case "$l" in *'"method":"session/cancel"'*) sleep 0.3; finish "$id" cancelled; break;; esac
          done;;
        *'"text":"die'*)
          chunk "about to "; echo "the agent fell over" >&2; exit 7;;
        *'"text":"ask'*|*'"text":"push'*)
          update '{"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"thinking it over"}}'
          chunk "Let me run it. "
          cmd="make build"; [ -n "${line##*\"text\":\"ask*}" ] && cmd="git push origin main"
          update "{\"sessionUpdate\":\"tool_call\",\"toolCallId\":\"c1\",\"title\":\"Run $cmd\",\"kind\":\"execute\",\"status\":\"pending\",\"rawInput\":{\"command\":\"$cmd\"}}"
          say "{\"jsonrpc\":\"2.0\",\"id\":900,\"method\":\"session/request_permission\",\"params\":{\"sessionId\":\"sess-1\",\"toolCall\":{\"toolCallId\":\"c1\",\"title\":\"Run $cmd\",\"kind\":\"execute\",\"rawInput\":{\"command\":\"$cmd\"}},\"options\":[{\"optionId\":\"allow\",\"name\":\"Allow once\",\"kind\":\"allow_once\"},{\"optionId\":\"deny\",\"name\":\"Reject\",\"kind\":\"reject_once\"}]}}"
          IFS= read -r resp
          printf '%s\n' "$resp" >> "$log"
          case "$resp" in
            *'"optionId":"allow"'*)
              update '{"sessionUpdate":"tool_call_update","toolCallId":"c1","status":"completed","content":[{"type":"content","content":{"type":"text","text":"built"}}]}'
              update '{"sessionUpdate":"usage_update","used":1200,"size":200000,"cost":{"amount":0.004,"currency":"USD"}}'
              update '{"sessionUpdate":"usage_update","used":2400,"size":200000,"cost":{"amount":0.010,"currency":"USD"}}'
              chunk "It built.";;
            *) chunk "It was not allowed.";;
          esac
          finish "$id" end_turn;;
        *'"text":"edit'*)
          update '{"sessionUpdate":"tool_call","toolCallId":"e1","title":"Edit a.txt","kind":"edit","status":"in_progress","locations":[{"path":"a.txt"}]}'
          update '{"sessionUpdate":"tool_call_update","toolCallId":"e1","status":"in_progress"}'
          update '{"sessionUpdate":"tool_call_update","toolCallId":"e1","status":"completed","content":[{"type":"diff","path":"a.txt","oldText":null,"newText":"x"}]}'
          update '{"sessionUpdate":"plan","entries":[{"content":"one","priority":"high","status":"completed"},{"content":"two","priority":"low","status":"pending"}]}'
          chunk "Edited."
          finish "$id" end_turn;;
        *'"text":"permedit'*)
          path=$(printf '%s' "$line" | sed -n 's/.*permedit \([^"]*\)".*/\1/p')
          say "{\"jsonrpc\":\"2.0\",\"id\":901,\"method\":\"session/request_permission\",\"params\":{\"sessionId\":\"sess-1\",\"toolCall\":{\"toolCallId\":\"p1\",\"title\":\"Edit $path\",\"kind\":\"edit\",\"locations\":[{\"path\":\"$path\"}]},\"options\":[{\"optionId\":\"allow\",\"name\":\"Allow once\",\"kind\":\"allow_once\"},{\"optionId\":\"deny\",\"name\":\"Reject\",\"kind\":\"reject_once\"}]}}"
          IFS= read -r resp; printf '%s\n' "$resp" >> "$log"
          chunk "asked"; finish "$id" end_turn;;
        *'"text":"twice'*)
          # opencode asks about one command twice under one tool call: its rule for a folder outside the project (`other`, with the command), then the command
          opts='"options":[{"optionId":"allow","name":"Allow once","kind":"allow_once"},{"optionId":"deny","name":"Reject","kind":"reject_once"}]'
          say "{\"jsonrpc\":\"2.0\",\"id\":910,\"method\":\"session/request_permission\",\"params\":{\"sessionId\":\"sess-1\",\"toolCall\":{\"toolCallId\":\"c2\",\"title\":\"ls /etc\",\"kind\":\"other\",\"rawInput\":{\"command\":\"ls /etc\",\"directories\":[\"/etc\"]}},$opts}}"
          IFS= read -r resp; printf '%s\n' "$resp" >> "$log"
          say "{\"jsonrpc\":\"2.0\",\"id\":911,\"method\":\"session/request_permission\",\"params\":{\"sessionId\":\"sess-1\",\"toolCall\":{\"toolCallId\":\"c2\",\"title\":\"ls /etc\",\"kind\":\"execute\",\"rawInput\":{\"command\":\"ls /etc\"}},$opts}}"
          IFS= read -r resp; printf '%s\n' "$resp" >> "$log"
          say "{\"jsonrpc\":\"2.0\",\"id\":912,\"method\":\"session/request_permission\",\"params\":{\"sessionId\":\"sess-1\",\"toolCall\":{\"toolCallId\":\"c2\",\"title\":\"rm -rf x\",\"kind\":\"execute\",\"rawInput\":{\"command\":\"rm -rf x\"}},$opts}}"
          IFS= read -r resp; printf '%s\n' "$resp" >> "$log"
          chunk "asked"; finish "$id" end_turn;;
        *'"text":"folder'*)
          # opencode with the tool's own permission `allow`: the folder rule is the only ask before the tool works. `folder edit|read|none PATH`: the call's own
          # `tool_call` said what the tool is (`none`: it was not seen)
          what=$(printf '%s' "$line" | sed -n 's/.*folder \([a-z]*\) .*/\1/p')
          path=$(printf '%s' "$line" | sed -n 's/.*folder [a-z]* \([^"]*\)".*/\1/p')
          dir=$(dirname "$path")
          [ "$what" = edit ] && update "{\"sessionUpdate\":\"tool_call\",\"toolCallId\":\"d1\",\"title\":\"write\",\"kind\":\"edit\",\"status\":\"pending\",\"locations\":[],\"rawInput\":{}}"
          [ "$what" = read ] && update "{\"sessionUpdate\":\"tool_call\",\"toolCallId\":\"d1\",\"title\":\"read\",\"kind\":\"read\",\"status\":\"pending\",\"locations\":[],\"rawInput\":{}}"
          say "{\"jsonrpc\":\"2.0\",\"id\":920,\"method\":\"session/request_permission\",\"params\":{\"sessionId\":\"sess-1\",\"toolCall\":{\"toolCallId\":\"d1\",\"title\":\"$dir\",\"kind\":\"other\",\"locations\":[{\"path\":\"$path\"},{\"path\":\"$dir\"}],\"rawInput\":{\"filepath\":\"$path\",\"parentDir\":\"$dir\"}},\"options\":[{\"optionId\":\"allow\",\"name\":\"Allow once\",\"kind\":\"allow_once\"},{\"optionId\":\"deny\",\"name\":\"Reject\",\"kind\":\"reject_once\"}]}}"
          IFS= read -r resp; printf '%s\n' "$resp" >> "$log"
          chunk "asked"; finish "$id" end_turn;;
        *'"text":"titled'*)
          # opencode: the call is `bash` when it begins, and the command is the title of the update that ends it
          update '{"sessionUpdate":"tool_call","toolCallId":"t1","title":"bash","kind":"execute","status":"pending","locations":[]}'
          update '{"sessionUpdate":"tool_call_update","toolCallId":"t1","status":"completed","kind":"execute","title":"ls -la","content":[{"type":"content","content":{"type":"text","text":"total 0"}}]}'
          chunk "listed"; finish "$id" end_turn;;
        *'"text":"rejectend'*)
          # opencode: a request answered no ends the turn, with nothing said
          say "{\"jsonrpc\":\"2.0\",\"id\":930,\"method\":\"session/request_permission\",\"params\":{\"sessionId\":\"sess-1\",\"toolCall\":{\"toolCallId\":\"r1\",\"title\":\"printf x > ~/x\",\"kind\":\"execute\",\"rawInput\":{\"command\":\"printf x > ~/x\"}},\"options\":[{\"optionId\":\"allow\",\"name\":\"Allow once\",\"kind\":\"allow_once\"},{\"optionId\":\"deny\",\"name\":\"Reject\",\"kind\":\"reject_once\"}]}}"
          IFS= read -r resp; printf '%s\n' "$resp" >> "$log"
          finish "$id" end_turn;;
        *'"text":"Not allowed here'*) chunk "Carried on."; finish "$id" end_turn;;
        *'"text":"fs'*)
          say '{"jsonrpc":"2.0","id":77,"method":"fs/read_text_file","params":{"path":"/x"}}'
          IFS= read -r r; printf '%s\n' "$r" >> "$log"
          chunk done; finish "$id" end_turn;;
        *'"text":"limit'*) chunk "partial"; finish "$id" max_tokens;;
        *'"text":"refuse'*) finish "$id" refusal;;
        *) chunk "Hello "; chunk "world"; finish "$id" end_turn;;
      esac;;
  esac
done
