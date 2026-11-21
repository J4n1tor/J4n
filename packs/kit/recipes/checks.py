"""Run the project's check on every file the task changed, in one `j4n.many`, and say which files fail.

run(cmd, files=None, tool="run", arg="cmd", status="git") -> the result of `each`, with "files" added
`cmd` is the check with `{}` where the file goes (`"python3 -m py_compile {}"`). `files` defaults to the files the working tree shows as
changed or new (`git status`, through the bridge). A check that exits non-zero is a failure.
"""
import shlex


def _changed(status):
    out = []
    for line in j4n.call(status, op="status").splitlines():
        if line.startswith("##") or len(line) < 4:
            continue
        path = line[3:].split(" -> ")[-1].strip().strip('"')
        if path and not path.endswith("/"):
            out.append(path)
    return out


def run(cmd, files=None, tool="run", arg="cmd", status="git"):
    files = list(files) if files is not None else _changed(status)
    if not files:
        return {"ok": 0, "failed": 0, "results": [], "failures": [], "files": []}
    # the file is quoted for the shell, so a name with a space or a quote is one word
    template = cmd.replace("{}", "{q}")
    made = [template.replace("{q}", shlex.quote(f)) for f in files]
    got = j4n.recipe("each", tool=tool, items=made, arg=arg)
    got["failures"] = [[files[made.index(c)], why] for c, why in got["failures"]]
    return {**got, "files": files}
