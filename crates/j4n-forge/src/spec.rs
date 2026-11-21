//! The spec of one harness: one value on each axis, drawn from a seed, written as `spec.toml`; the
//! `truth.toml` the lab's scorer compares against; the shim with a random name that runs the harness.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// One axis: the value's name and the words that go with it (a flag name, a marker, a limit's numbers).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Axis {
    pub kind: String,
    #[serde(default)]
    pub p: BTreeMap<String, String>,
}

impl Axis {
    pub fn get(&self, key: &str) -> &str {
        self.p.get(key).map_or("", String::as_str)
    }

    pub fn num(&self, key: &str) -> f64 {
        self.get(key).parse().unwrap_or(0.0)
    }

    fn set(mut self, key: &str, value: impl ToString) -> Axis {
        self.p.insert(key.into(), value.to_string());
        self
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Spec {
    pub name: String,
    pub seed: u64,
    pub a: Axis,
    pub b: Axis,
    pub c: Axis,
    pub d: Axis,
    pub e: Axis,
    pub f: Axis,
    pub g: Axis,
    /// The field names of structured output, and the ones it has after the "upgrade".
    pub names: BTreeMap<String, String>,
    pub upgraded: BTreeMap<String, String>,
}

/// What the lab's scorer compares against; J4n never reads it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Truth {
    pub seed: u64,
    pub name: String,
    /// Each axis's value, `a` to `g`.
    pub axes: BTreeMap<String, String>,
    /// The `axis:value` pairs of this harness that are held out; empty for a seed of the seen families.
    pub held: Vec<String>,
    /// Whether a prompt can be answered at all (a harness that needs a login cannot).
    pub connectable: bool,
    pub sessions: bool,
    pub tools_shown: bool,
    pub asks: bool,
    pub writes_unasked: bool,
    pub can_write: bool,
    pub upgrade: bool,
    pub limit: Option<Limit>,
    pub fails: String,
    /// Where it keeps the numbers it really has (relative to the folder it runs in).
    pub state_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Limit {
    pub kind: String,
    pub unit: String,
    pub cap: f64,
    pub window_s: u64,
    pub announced: String,
}

// (value, weight, held out): the draw is weighted toward the unusual, and two values of each axis are held out.
type Table = &'static [(&'static str, u32, bool)];
const A: Table = &[("positional", 1, false), ("flag", 3, false), ("stdin", 3, false), ("file", 3, true), ("subcommand", 3, false), ("env", 3, false), ("repl", 3, true)];
const B: Table = &[("plain", 1, false), ("ansi", 2, false), ("json", 2, false), ("jsonl", 3, false), ("sse", 3, false), ("frames", 3, false), ("xml", 3, true), ("logfmt", 3, false), ("jsonrpc", 3, true), ("http", 3, false)];
const C: Table = &[("none", 1, false), ("flag", 4, false), ("file", 3, false), ("server", 3, true), ("expire", 3, true)];
const D: Table = &[("none", 1, false), ("events", 3, false), ("ask_stdin", 3, false), ("ask_json", 3, true), ("silent", 3, true)];
const E: Table = &[
    ("none", 1, false),
    ("beans", 3, false),
    ("requests", 2, false),
    ("tokens", 2, false),
    ("cost", 2, false),
    ("banner", 2, false),
    ("trailer", 2, false),
    ("exitcode", 2, false),
    ("http429", 2, false),
    ("quotafile", 2, true),
    ("usagecmd", 2, false),
    ("softhard", 2, true),
];
const F: Table = &[("full_help", 2, false), ("help_sub", 3, false), ("man_only", 3, false), ("none", 3, false), ("schema", 2, true), ("lying", 2, true)];
const G: Table = &[("clean", 1, false), ("slow_start", 3, false), ("stall", 3, true), ("crash_half", 3, false), ("bad_utf8", 3, true), ("giant_line", 3, false), ("stderr_mix", 3, false), ("login", 3, false), ("upgrade", 3, false)];

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }

    fn pick<'a, T: ?Sized>(&mut self, xs: &[&'a T]) -> &'a T {
        xs[self.below(xs.len() as u64) as usize]
    }

    /// A made-up word of `syllables` syllables.
    fn word(&mut self, syllables: usize) -> String {
        const C: [&str; 12] = ["b", "d", "k", "l", "m", "n", "p", "r", "t", "v", "z", "x"];
        const V: [&str; 5] = ["a", "e", "i", "o", "u"];
        (0..syllables).map(|_| format!("{}{}", self.pick(&C), self.pick(&V))).collect()
    }

    fn weighted(&mut self, table: Table, allow: impl Fn(&str, bool) -> bool) -> &'static str {
        let open: Vec<_> = table.iter().filter(|(v, _, h)| allow(v, *h)).collect();
        let total: u64 = open.iter().map(|(_, w, _)| u64::from(*w)).sum();
        let mut at = self.below(total);
        for (v, w, _) in &open {
            if at < u64::from(*w) {
                return v;
            }
            at -= u64::from(*w);
        }
        open[0].0
    }
}

fn is_held(table: Table, value: &str) -> bool {
    table.iter().any(|(v, _, h)| *v == value && *h)
}

const STRUCTURED: [&str; 8] = ["json", "jsonl", "sse", "frames", "xml", "logfmt", "jsonrpc", "http"];

/// The harness a seed makes. Half the seeds draw only seen values; the other half carry at least one held-out value.
pub fn draw(seed: u64) -> Spec {
    let mut r = Rng(seed.wrapping_mul(0x2545_F491_4F6C_DD1D) ^ 0x5EED);
    let held_seed = r.below(2) == 1;
    let tables: [Table; 7] = [A, B, C, D, E, F, G];
    let mut v: Vec<&'static str> = tables.iter().map(|t| r.weighted(t, |_, h| held_seed || !h)).collect();
    if held_seed && !tables.iter().zip(&v).any(|(t, x)| is_held(t, x)) {
        let i = r.below(7) as usize;
        v[i] = r.weighted(tables[i], |_, h| h);
    }
    // a harness that is a server takes its prompts over HTTP; a 429 is an HTTP thing; a renamed field needs fields
    if v[4] == "http429" {
        v[1] = "http";
    }
    if v[1] == "http" {
        v[0] = "subcommand";
    }
    if v[6] == "upgrade" && !STRUCTURED.contains(&v[1]) {
        v[1] = r.weighted(B, |x, h| STRUCTURED.contains(&x) && x != "http" && (held_seed || !h));
    }
    let name = r.word(3);
    let upper = name.to_uppercase();
    let flags = ["--message", "-m", "--ask", "--say", "-p", "--query", "--input", "--text"];
    let flag = if r.below(3) == 0 { format!("--{}", r.word(2)) } else { r.pick(&flags).to_string() };
    let a = Axis { kind: v[0].into(), p: BTreeMap::new() };
    let a = match v[0] {
        "flag" => a.set("flag", &flag),
        "file" => a.set("flag", if r.below(2) == 0 { "--prompt-file".to_string() } else { format!("--{}-file", r.word(2)) }),
        "subcommand" => a.set("sub", if v[1] == "http" { "serve" } else { r.pick(&["ask", "chat send", "q"]) }),
        "env" => a.set("env", format!("{upper}_PROMPT")),
        "repl" => a.set("marker", r.pick(&["»", "ready>", "[0]:"])),
        _ => a,
    };
    let b = Axis { kind: v[1].into(), p: BTreeMap::new() }.set("depth", 1 + r.below(3)).set("port", 20_000 + r.below(20_000));
    let c = Axis { kind: v[2].into(), p: BTreeMap::new() }.set("flag", if r.below(2) == 0 { "--session".to_string() } else { format!("--{}", r.word(2)) }).set("k", 2 + r.below(2));
    let d = Axis { kind: v[3].into(), p: BTreeMap::new() };
    let e = limit_axis(&mut r, v[4]);
    let f = Axis { kind: v[5].into(), p: BTreeMap::new() };
    let g = Axis { kind: v[6].into(), p: BTreeMap::new() }.set("delay", 3 + r.below(6));
    let mut names = BTreeMap::new();
    let mut upgraded = BTreeMap::new();
    for key in ["kind", "body", "say", "tool", "result", "session", "usage", "end", "name", "arg", "id", "wrap0", "wrap1"] {
        let word = match (key, r.below(2)) {
            ("kind", 0) => "k".to_string(),
            ("wrap0" | "wrap1" | "kind", _) => r.word(2),
            (_, 0) => key.to_string(),
            _ => r.word(2),
        };
        upgraded.insert(key.to_string(), format!("{word}2"));
        names.insert(key.to_string(), word);
    }
    Spec { name, seed, a, b, c, d, e, f, g, names, upgraded }
}

fn limit_axis(r: &mut Rng, kind: &str) -> Axis {
    let e = Axis { kind: kind.into(), p: BTreeMap::new() };
    let unit = |u: &str| u.to_string();
    match kind {
        "beans" => e.set("unit", unit("beans")).set("cap", 30 + r.below(60)).set("cost", 4 + r.below(6)).set("window", 0).set("refill_hour", r.below(24)).set("where", "footer"),
        "requests" => e.set("unit", unit("req")).set("cap", 3 + r.below(4)).set("cost", 1).set("window", 420).set("where", "footer"),
        "tokens" => e.set("unit", unit("tok")).set("cap", 160 + r.below(160)).set("cost", 0).set("window", 131).set("where", "footer"),
        "cost" => e.set("unit", r.word(2)).set("cap", 5 + r.below(16)).set("cost", format!("{:.1}", 0.4 + r.below(9) as f64 / 10.0)).set("window", 86_400).set("where", "footer"),
        "banner" => e.set("unit", unit("req")).set("cap", 4 + r.below(5)).set("cost", 1).set("window", 300).set("where", "stderr"),
        "trailer" => e.set("unit", unit("tok")).set("cap", 160 + r.below(200)).set("cost", 0).set("window", 131).set("where", "trailer"),
        "exitcode" => e.set("unit", unit("req")).set("cap", 3 + r.below(3)).set("cost", 1).set("window", 300).set("where", "exit").set("code", if r.below(2) == 0 { 75 } else { 42 }),
        "http429" => e.set("unit", unit("req")).set("cap", 3 + r.below(3)).set("cost", 1).set("window", 240).set("where", "http"),
        "quotafile" => e.set("unit", unit("req")).set("cap", 4 + r.below(4)).set("cost", 1).set("window", 360).set("where", "file"),
        "usagecmd" => e.set("unit", unit("tok")).set("cap", 160 + r.below(160)).set("cost", 0).set("window", 200).set("where", "subcommand"),
        "softhard" => e.set("unit", unit("req")).set("cap", 5 + r.below(6)).set("cost", 1).set("window", 300).set("where", "stderr"),
        _ => e,
    }
}

pub fn axes_line(s: &Spec) -> String {
    [("a", &s.a), ("b", &s.b), ("c", &s.c), ("d", &s.d), ("e", &s.e), ("f", &s.f), ("g", &s.g)].iter().map(|(k, x)| format!("{k}={}", x.kind)).collect::<Vec<_>>().join(" ")
}

pub fn truth(s: &Spec) -> Truth {
    let axes: BTreeMap<String, String> = [("a", &s.a), ("b", &s.b), ("c", &s.c), ("d", &s.d), ("e", &s.e), ("f", &s.f), ("g", &s.g)].iter().map(|(k, x)| (k.to_string(), x.kind.clone())).collect();
    let tables: BTreeMap<&str, Table> = [("a", A), ("b", B), ("c", C), ("d", D), ("e", E), ("f", F), ("g", G)].into_iter().collect();
    let held = axes.iter().filter(|(k, v)| is_held(tables[k.as_str()], v)).map(|(k, v)| format!("{k}:{v}")).collect();
    let limit = (s.e.kind != "none").then(|| Limit { kind: s.e.kind.clone(), unit: s.e.get("unit").into(), cap: s.e.num("cap"), window_s: s.e.num("window") as u64, announced: s.e.get("where").into() });
    Truth {
        seed: s.seed,
        name: s.name.clone(),
        axes,
        held,
        connectable: s.g.kind != "login",
        sessions: s.c.kind != "none",
        tools_shown: matches!(s.d.kind.as_str(), "events" | "ask_stdin" | "ask_json"),
        asks: matches!(s.d.kind.as_str(), "ask_stdin" | "ask_json"),
        writes_unasked: matches!(s.d.kind.as_str(), "events" | "silent"),
        can_write: s.d.kind != "none",
        upgrade: s.g.kind == "upgrade",
        limit,
        fails: s.g.kind.clone(),
        state_dir: format!(".{}", s.name),
    }
}

/// Write the harness of `seed` into `out`: `spec.toml`, the shim (named as the harness is), `truth.toml`, and a manual page when that is
/// all the harness says about itself.
pub fn make(seed: u64, out: &Path) -> Result<Spec, String> {
    if seed >= crate::tool::TOOL_BASE {
        let t = crate::tool::make(seed, out)?;
        return Ok(Spec { name: t.name, seed, ..Spec::default() });
    }
    make_agent(seed, out)
}

/// The agent harness of any seed (`make --agent`): the seeds from `TOOL_BASE` on are tools unless this is asked for.
pub fn make_agent(seed: u64, out: &Path) -> Result<Spec, String> {
    let s = draw(seed);
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let put = |name: &str, text: String| std::fs::write(out.join(name), text).map_err(|e| format!("{name}: {e}"));
    put("spec.toml", toml::to_string(&s).map_err(|e| e.to_string())?)?;
    put("truth.toml", toml::to_string(&truth(&s)).map_err(|e| e.to_string())?)?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let spec = out.canonicalize().map_err(|e| e.to_string())?.join("spec.toml");
    let shim = out.join(&s.name);
    put(&s.name, format!("#!/bin/sh\nexec '{}' play '{}' \"$@\"\n", exe.display(), spec.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    if s.f.kind == "man_only" {
        let dir = out.join("man/man1");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let page = crate::play::help_text(&s).lines().map(|l| if l.starts_with('.') { format!("\\&{l}") } else { l.to_string() }).collect::<Vec<_>>().join("\n");
        std::fs::write(dir.join(format!("{}.1", s.name)), format!(".TH {} 1\n.SH NAME\n{} \\- a harness\n.SH DESCRIPTION\n.nf\n{page}\n.fi\n", s.name.to_uppercase(), s.name)).map_err(|e| e.to_string())?;
    }
    Ok(s)
}
