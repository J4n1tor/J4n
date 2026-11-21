#!/bin/sh
# An agent CLI J4n has never seen (the tests' stand-in for one): `newagent run --json [--model M] [--resume ID] PROMPT` prints one
# JSON object per line. It knows nothing of J4n; the manifest `newagent.toml` is the whole of the connection. Every start is
# logged, with its arguments, to $NEWAGENT_LOG.
[ "$1" = "--version" ] && { echo "newagent 0.3.1 (fixture)"; exit 0; }
echo "start $*" >> "${NEWAGENT_LOG:-/dev/null}"
shift 2
model=""; resume=""
while [ $# -gt 1 ]; do
  case "$1" in
    --model) model="$2"; shift 2;;
    --resume) resume="$2"; shift 2;;
    *) shift;;
  esac
done
prompt="$1"
sid="${resume:-ns-1}"
printf '%s\n' "WARN: warming up (this line is not JSON)"
case "$prompt" in
  drift*) printf '{"kind2":"hello","body2":"x"}\n{"kind2":"bye"}\n'; exit 0;;
esac
printf '{"type":"start","session_id":"%s"}\n' "$sid"
case "$prompt" in
  fail*) printf '{"type":"error","message":"the model is down"}\n'; exit 0;;
  crash*) echo "segfault" >&2; exit 139;;
esac
case "$prompt" in
  giant*) big=$(head -c 1200000 /dev/zero | tr '\0' x); printf '{"type":"text","text":"%s"}\n{"type":"result"}\n' "$big"; exit 0;;
esac
printf '{"type":"text","text":"Looking "}\n'
printf '{"type":"text","text":"around. "}\n'
printf '{"type":"tool_use","name":"read","input":{"path":"a.txt"}}\n'
printf '{"type":"tool_result","name":"read","output":"hello"}\n'
printf '{"type":"text","text":"The answer is 42 (%s, %s)."}\n' "${model:-no model}" "${resume:+resumed}"
printf '{"type":"result","usage":{"cost_usd":0.002}}\n'
