"""j4n kernel: a persistent Python namespace over JSON lines on stdin/stdout. Stdlib only, no network, no key.

Request : {"id": 1, "code": "..."}
Load    : {"id": 0, "load": {"modules": {"j4n": "<source>"}, "recipes": {"name": "<source>"}}}  (J4n sends it first; the modules are
          imported by name with no file in the box)
Answer  : {"id": 1, "ok": true, "out": "<printed>", "value": "<repr of the last expression>", "error": null, "ms": 1.2, "names": ["x"]}
Host    : while a cell runs, `j4n.host(name, **payload)` writes {"call": "jev_many", "payload": {...}} (a line with no "id") and reads one
          line back, {"ok": true, "value": ...} or {"ok": false, "error": "..."}: J4n does the work, the kernel has no key and no network.
          The load line may carry "config": {"keep": 0.15}, which the library reads.

J4n starts it as `python3 -I -c <this file>` behind the task's place, and stops a cell that runs too long by killing the process (the
variables are then gone, and the agent is told). A cell's printed output and value are capped (CAP chars, head and tail): the agent
sees a summary and the data stays here. The protocol's two ends are private descriptors; fd 0 and fd 1 are /dev/null, so a cell that
writes to or reads from them cannot break the protocol, and `input()` finds the end of its input.
"""
import ast, contextlib, io, json, os, sys, time, traceback, types

CAP = 4000
AS_LIMIT = 2 << 30  # address space a cell may use: a runaway allocation is a MemoryError, not the machine's swap
try:
    import resource
    resource.setrlimit(resource.RLIMIT_AS, (AS_LIMIT, AS_LIMIT))
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
except (ImportError, ValueError, OSError):
    pass

_in = os.fdopen(os.dup(0), "r", encoding="utf-8")
_out = os.fdopen(os.dup(1), "w", encoding="utf-8")
_null = os.open(os.devnull, os.O_RDWR)
os.dup2(_null, 0)
os.dup2(_null, 1)
sys.stdin = io.StringIO("")
ns = {"__name__": "__j4n__"}


def cap(s):
    return s if len(s) <= CAP else s[: CAP // 2] + f"\n… [{len(s) - CAP} chars cut] …\n" + s[-CAP // 2 :]


def run(code, buf):
    tree = ast.parse(code, "<cell>", "exec")
    last = None
    if tree.body and isinstance(tree.body[-1], ast.Expr):
        last = ast.Expression(tree.body.pop().value)
    with contextlib.redirect_stdout(buf), contextlib.redirect_stderr(buf):
        exec(compile(tree, "<cell>", "exec"), ns)
        return eval(compile(last, "<cell>", "eval"), ns) if last else None


def host(name, payload):
    """Ask J4n for work: one line out, one line back, the reply as J4n wrote it. `j4n.host` makes it an error or a value. The
    cell waits here, and the tool's timeout covers the wait."""
    _out.write(json.dumps({"call": name, "payload": payload}, default=str) + "\n")
    _out.flush()
    raw = _in.readline()
    return json.loads(raw) if raw.strip() else {"ok": False, "error": "J4n ended the conversation"}


def load(spec):
    """Make the library importable by name from its source: a module in `sys.modules`, so `import j4n` finds it with no file."""
    for name, src in spec.get("modules", {}).items():
        mod = types.ModuleType(name)
        mod.__file__ = f"<{name}>"
        exec(compile(src, mod.__file__, "exec"), mod.__dict__)
        mod._RECIPES = spec.get("recipes", {})  # after the source ran, which starts it as None
        mod._CONFIG = spec.get("config", {})
        mod._channel = host
        if hasattr(mod, "_apply_off"):
            mod._apply_off()
        sys.modules[name] = mod


def failure(e):
    text = "".join(traceback.format_exception_only(type(e), e)).strip()
    line = [f.lineno for f in traceback.extract_tb(e.__traceback__) if f.filename == "<cell>"]
    return cap(text + (f" (cell line {line[-1]})" if line else ""))


def main():
    for raw in _in:
        if not raw.strip():
            continue
        req = json.loads(raw)
        t = time.perf_counter()
        ans = {"id": req.get("id"), "ok": True, "out": "", "value": None, "error": None}
        buf = io.StringIO()
        try:
            if "load" in req:
                load(req["load"])
                value = None
            else:
                value = run(req["code"], buf)
            if value is not None:
                ans["value"] = cap(repr(value))
        except BaseException as e:  # a cell's SystemExit or KeyboardInterrupt must not end the kernel
            ans.update(ok=False, error=failure(e))
        ans["out"] = cap(buf.getvalue())
        ans["ms"] = round(1000 * (time.perf_counter() - t), 2)
        lib = sys.modules.get("j4n")
        if lib is not None and "load" not in req and hasattr(lib, "_take_used"):
            ans["used"] = lib._take_used()  # the parts of the library this cell used; {} is a cell that used none
        ans["names"] = sorted(k for k in ns if not k.startswith("_"))[:50]
        try:
            ans["mem"] = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss  # KiB, the most this kernel has held
        except (NameError, OSError):
            pass
        _out.write(json.dumps(ans) + "\n")
        _out.flush()


if __name__ == "__main__":
    main()
