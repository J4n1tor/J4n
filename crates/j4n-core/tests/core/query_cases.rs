//! Query through the public API only. Blind tests: every expectation comes from the doc comments of `query.rs`, `value.rs`,
//! `eval.rs` and the language reference in `expr.rs`. A row marked `DECIDED` is one where the contract is silent or
//! allows two readings; the row states the reading it chose. Each theme collects every failing row and asserts once.

use j4n_core::eval::{Env, Scope};
use j4n_core::expr::Expr;
use j4n_core::query::{Coll, MAX_LIMIT, MAX_ORDER, Order, Query};
use j4n_core::value::{Key, Value};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Flip to `true` for one run to see "rows / failed" of every theme, including the passing ones.
const REPORT: bool = false;

struct Tally {
    rows: usize,
    fails: Vec<String>,
}

impl Tally {
    fn new() -> Tally {
        Tally { rows: 0, fails: Vec::new() }
    }

    fn note(note: &str) -> String {
        if note.is_empty() { String::new() } else { format!("  [{note}]") }
    }

    fn eq<T: PartialEq + Debug>(&mut self, input: String, note: &str, want: T, got: impl FnOnce() -> T) {
        self.rows += 1;
        let note = Tally::note(note);
        match catch_unwind(AssertUnwindSafe(got)) {
            Ok(g) if g == want => {}
            Ok(g) => self.fails.push(format!("{input}{note}\n      want {want:?}\n      got  {g:?}")),
            Err(_) => self.fails.push(format!("{input}{note}\n      want {want:?}\n      got  PANIC")),
        }
    }

    /// `Ok(())` passes, `Err(why)` fails with the reason.
    fn holds(&mut self, input: String, note: &str, f: impl FnOnce() -> Result<(), String>) {
        self.rows += 1;
        let note = Tally::note(note);
        match catch_unwind(AssertUnwindSafe(f)) {
            Ok(Ok(())) => {}
            Ok(Err(why)) => self.fails.push(format!("{input}{note}\n      {why}")),
            Err(_) => self.fails.push(format!("{input}{note}\n      PANIC")),
        }
    }

    fn finish(self, theme: &str) {
        let (rows, n) = (self.rows, self.fails.len());
        if REPORT || n > 0 {
            panic!("[{theme}] {n} of {rows} rows failed\n  {}", self.fails.join("\n  "));
        }
    }
}

fn n(x: f64) -> Value {
    Value::Num(x)
}

fn sv(x: &str) -> Value {
    Value::Str(x.to_string())
}

fn usd(d: f64) -> Value {
    Value::usd(d)
}

fn list(items: Vec<Value>) -> Value {
    Value::List(items)
}

fn map(pairs: &[(&str, Value)]) -> Value {
    Value::Map(pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect::<BTreeMap<_, _>>())
}

/// A row: an `id` and the other fields.
fn row(id: &str, fields: &[(&str, Value)]) -> Value {
    let mut m: BTreeMap<String, Value> = fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
    m.insert("id".to_string(), sv(id));
    Value::Map(m)
}

/// The five rows most tests use, in the order a..e.
fn base() -> Vec<Value> {
    vec![
        row("a", &[("state", sv("done")), ("n", n(3.0)), ("cost", usd(1.5)), ("data", map(&[("cmd", sv("ls"))]))]),
        row("b", &[("state", sv("open")), ("n", n(1.0)), ("cost", usd(0.25)), ("data", map(&[("cmd", sv("rm -rf"))]))]),
        row("c", &[("state", sv("done")), ("n", n(2.0)), ("cost", usd(3.0)), ("data", map(&[("cmd", sv("ls"))]))]),
        row("d", &[("state", sv("fail")), ("n", n(2.0)), ("data", map(&[]))]),
        row("e", &[("state", sv("open")), ("n", n(5.0)), ("cost", usd(0.25))]),
    ]
}

/// The base rows in the order given by `order` (letters).
fn base_in(order: &str) -> Vec<Value> {
    let all = base();
    order.chars().map(|c| all.iter().find(|r| label(r) == c.to_string()).cloned().unwrap()).collect()
}

fn outer() -> Scope {
    Scope::new().with("fold", map(&[("spend", n(2.0))])).with("set", map(&[("loop", sv("x"))]))
}

fn label(v: &Value) -> String {
    match v {
        Value::Map(m) => match m.get("id") {
            Some(Value::Str(s)) => s.clone(),
            _ => format!("{v:?}"),
        },
        Value::Num(x) => format!("{x}"),
        Value::Str(s) => s.clone(),
        other => format!("{other:?}"),
    }
}

fn ids(rows: &[Value]) -> String {
    rows.iter().map(label).collect::<Vec<_>>().join(",")
}

fn run(
    from: &str,
    filter: Option<&str>,
    order: Option<&str>,
    limit: Option<u64>,
    rows: Vec<Value>,
    env: &dyn Env,
) -> String {
    match Query::parse(from, filter, order, limit) {
        Ok(q) => ids(&q.apply(rows, env)),
        Err(e) => format!("PARSE ERROR: {e:?}"),
    }
}

fn ex(src: &str) -> Expr {
    Expr::parse(src).unwrap_or_else(|e| panic!("test expression {src:?}: {e:?}"))
}

/// An option text for a test label, cut at 30 characters.
fn cut(o: Option<&str>) -> String {
    match o {
        None => "None".to_string(),
        Some(t) if t.chars().count() > 30 => {
            format!("Some({:?}...{} chars)", t.chars().take(30).collect::<String>(), t.chars().count())
        }
        Some(t) => format!("Some({t:?})"),
    }
}

fn dg(cmd: &str) -> Value {
    Value::Bool(cmd.contains("rm"))
}

struct Spy {
    seen: RefCell<Vec<(String, Vec<Key>)>>,
}

impl Spy {
    fn new() -> Spy {
        Spy { seen: RefCell::new(Vec::new()) }
    }

    fn roots(&self) -> Vec<String> {
        self.seen.borrow().iter().map(|(r, _)| r.clone()).collect()
    }
}

impl Env for Spy {
    fn get(&self, root: &str, path: &[Key]) -> Value {
        self.seen.borrow_mut().push((root.to_string(), path.to_vec()));
        Value::Missing
    }
}

// ---------------------------------------------------------------------------------------------------- Coll

#[test]
fn coll_parse_every_form() {
    let mut t = Tally::new();
    // (text, collection, note)
    let rows: Vec<(&str, Coll, &str)> = vec![
        ("tasks", Coll::Tasks, ""),
        ("asks", Coll::Asks, ""),
        ("changes", Coll::Changes, ""),
        ("log:tool.*", Coll::Log("tool.*".into()), ""),
        ("log:*", Coll::Log("*".into()), "a glob may be only a star"),
        ("log:**", Coll::Log("**".into()), ""),
        ("log:*.*", Coll::Log("*.*".into()), ""),
        ("log:a?c", Coll::Log("a?c".into()), "? is any one character"),
        ("log:?", Coll::Log("?".into()), ""),
        ("log:user-msg_1.x", Coll::Log("user-msg_1.x".into()), "letters, digits and _ . -"),
        ("log:a.b.c", Coll::Log("a.b.c".into()), ""),
        ("log:Tool.Use", Coll::Log("Tool.Use".into()), "capital letters are letters"),
        ("log:x", Coll::Log("x".into()), ""),
        ("log:9", Coll::Log("9".into()), "digits"),
        ("log:_", Coll::Log("_".into()), "an underscore is a name character"),
        ("table:note", Coll::Table("note".into()), ""),
        ("table:a.b-c_d", Coll::Table("a.b-c_d".into()), ""),
        ("table:N1", Coll::Table("N1".into()), ""),
        ("fold:spend", Coll::Fold("spend".into()), ""),
        ("fold:a-b.c_d", Coll::Fold("a-b.c_d".into()), ""),
        ("thread:t1", Coll::Thread("t1".into()), ""),
        ("thread:abc-123_x.y", Coll::Thread("abc-123_x.y".into()), ""),
    ];
    for (text, want, note) in &rows {
        t.eq(format!("Coll::parse({text:?})"), note, Ok(want.clone()), || Coll::parse(text));
        t.eq(format!("Coll::parse({text:?}).to_string()"), note, Ok(text.to_string()), || {
            Coll::parse(text).map(|c| c.to_string())
        });
    }
    t.finish("coll_parse_every_form");
}

#[test]
fn coll_parse_every_error() {
    let mut t = Tally::new();
    // (text, note); the glob characters belong to log only
    let rows: Vec<(&str, &str)> = vec![
        ("", "empty"),
        ("task", "not a collection"),
        ("nope", "not a collection"),
        ("Tasks", "DECIDED: names are lower case words"),
        ("TASKS", "DECIDED: names are lower case words"),
        ("LOG:x", "DECIDED: names are lower case words"),
        ("tasks:x", "tasks takes no name"),
        ("tasks:", "tasks takes no name"),
        ("asks:1", "asks takes no name"),
        ("changes:*", "changes takes no name"),
        ("log", "log needs a kind glob"),
        ("log:", "log needs a kind glob"),
        ("table", "table needs a kind"),
        ("table:", "table needs a kind"),
        ("fold", "fold needs an id"),
        ("fold:", "fold needs an id"),
        ("thread", "thread needs an id"),
        ("thread:", "thread needs an id"),
        ("log:-", "a name starts with a letter, a digit or an underscore"),
        ("log:.", "a name starts with a letter, a digit or an underscore"),
        ("table:..", "a name is never a path"),
        ("fold:-x", "a name starts with a letter, a digit or an underscore"),
        ("log:a b", "a space is not a name character"),
        ("log: x", "a space is not a name character"),
        ("log:x ", "a trailing space is not a name character"),
        ("log:a/b", "a slash is not a name character"),
        ("log:a:b", "a colon is not a name character"),
        ("log:a;b", ""),
        ("log:a'b", ""),
        ("log:a,b", ""),
        ("log:a\nb", ""),
        ("log:a{b}", ""),
        ("log:a[0]", ""),
        ("log:a|b", ""),
        ("table:a*b", "a glob belongs to log only"),
        ("table:*", "a glob belongs to log only"),
        ("table:a?b", "a glob belongs to log only"),
        ("fold:a*", "a glob belongs to log only"),
        ("fold:?", "a glob belongs to log only"),
        ("thread:*", "a glob belongs to log only"),
        ("thread:a?", "a glob belongs to log only"),
        ("table:a b", ""),
        ("fold:a/b", ""),
        ("thread:a:b", ""),
        ("log:é", "DECIDED: letters; reading: ASCII letters"),
        ("table:日本", "DECIDED: letters; reading: ASCII letters"),
        (" tasks", "not trimmed"),
        ("tasks ", "not trimmed"),
        ("tasks\n", "not trimmed"),
        ("tasks,asks", "one collection"),
        ("nope:x", "not a collection"),
        ("tasks:log", ""),
        ("log:log:x", "a colon is not a name character"),
    ];
    for (text, note) in &rows {
        t.holds(format!("Coll::parse({text:?}) is an error"), note, || match Coll::parse(text) {
            Ok(c) => Err(format!("parsed as {c:?}")),
            Err(m) if m.trim().is_empty() => Err("an empty message".into()),
            Err(_) => Ok(()),
        });
    }
    t.holds(
        "an error message says what was wrong: it names the text it refused".into(),
        "DECIDED: the message is free text; reading: it quotes the input",
        || {
            let mut missing = Vec::new();
            for (text, _) in &rows {
                let base_text = text.split(':').next_back().unwrap_or(text).trim();
                if text.is_empty() || base_text.is_empty() {
                    continue;
                }
                if let Err(m) = Coll::parse(text)
                    && !m.contains(base_text)
                    && !m.contains(&format!("{text:?}"))
                {
                    missing.push(format!("{text:?} -> {m:?}"));
                }
            }
            if missing.is_empty() {
                Ok(())
            } else {
                Err(format!("messages that do not mention the input:\n        {}", missing.join("\n        ")))
            }
        },
    );
    t.finish("coll_parse_every_error");
}

#[test]
fn coll_display() {
    let mut t = Tally::new();
    // (collection, text)
    let rows: Vec<(Coll, &str)> = vec![
        (Coll::Tasks, "tasks"),
        (Coll::Asks, "asks"),
        (Coll::Changes, "changes"),
        (Coll::Log("tool.*".into()), "log:tool.*"),
        (Coll::Log("a?c".into()), "log:a?c"),
        (Coll::Log("*".into()), "log:*"),
        (Coll::Table("note".into()), "table:note"),
        (Coll::Table("a.b-c_d".into()), "table:a.b-c_d"),
        (Coll::Fold("spend".into()), "fold:spend"),
        (Coll::Thread("t1".into()), "thread:t1"),
    ];
    for (c, text) in &rows {
        t.eq(format!("{c:?}.to_string()"), "", text.to_string(), || c.to_string());
        t.eq(format!("Coll::parse({:?}) after Display", c.to_string()), "", Ok(c.clone()), || {
            Coll::parse(&c.to_string())
        });
    }
    t.finish("coll_display");
}

// --------------------------------------------------------------------------------------------- Query::parse

fn q(from: Coll, filter: Option<&str>, order: &[(&str, bool)], limit: Option<usize>) -> Query {
    Query {
        from,
        filter: filter.map(ex),
        order: order.iter().map(|(k, d)| Order { key: ex(k), desc: *d }).collect(),
        limit,
    }
}

#[test]
fn query_parse_shapes() {
    let mut t = Tally::new();
    let keys4 = "a, -b, c, -d";
    let many = (0..MAX_ORDER).map(|i| format!("k{i}")).collect::<Vec<_>>().join(", ");
    let many_want: Vec<(String, bool)> = (0..MAX_ORDER).map(|i| (format!("k{i}"), false)).collect();
    let many_want_ref: Vec<(&str, bool)> = many_want.iter().map(|(k, d)| (k.as_str(), *d)).collect();
    // (from, filter, order, limit, want, note)
    type Row<'a> = (&'a str, Option<&'a str>, Option<&'a str>, Option<u64>, Query, &'a str);
    let rows: Vec<Row> = vec![
        ("tasks", None, None, None, q(Coll::Tasks, None, &[], None), ""),
        ("asks", Some("state == 'open'"), None, None, q(Coll::Asks, Some("state == 'open'"), &[], None), ""),
        ("changes", None, Some("n"), None, q(Coll::Changes, None, &[("n", false)], None), ""),
        (
            "log:tool.*",
            Some("ok"),
            Some("-ts"),
            Some(10),
            q(Coll::Log("tool.*".into()), Some("ok"), &[("ts", true)], Some(10)),
            "",
        ),
        (
            "table:note",
            None,
            Some("-a, b"),
            Some(1),
            q(Coll::Table("note".into()), None, &[("a", true), ("b", false)], Some(1)),
            "",
        ),
        (
            "fold:spend",
            None,
            Some(keys4),
            Some(MAX_LIMIT),
            q(
                Coll::Fold("spend".into()),
                None,
                &[("a", false), ("b", true), ("c", false), ("d", true)],
                Some(MAX_LIMIT as usize),
            ),
            "four keys and the largest limit",
        ),
        (
            "thread:t1",
            None,
            Some("lower(name)"),
            None,
            q(Coll::Thread("t1".into()), None, &[("lower(name)", false)], None),
            "",
        ),
        (
            "tasks",
            None,
            Some("a,b"),
            None,
            q(Coll::Tasks, None, &[("a", false), ("b", false)], None),
            "no space after the comma",
        ),
        (
            "tasks",
            None,
            Some("a ,  b"),
            None,
            q(Coll::Tasks, None, &[("a", false), ("b", false)], None),
            "spaces around the comma",
        ),
        (
            "tasks",
            None,
            Some("-a,-b"),
            None,
            q(Coll::Tasks, None, &[("a", true), ("b", true)], None),
            "both descending",
        ),
        (
            "tasks",
            None,
            Some("  n  "),
            None,
            q(Coll::Tasks, None, &[("n", false)], None),
            "DECIDED: spaces around a key; reading: ignored",
        ),
        (
            "tasks",
            None,
            Some("- n"),
            None,
            q(Coll::Tasks, None, &[("n", true)], None),
            "DECIDED: a space after the minus; reading: still descending",
        ),
        (
            "tasks",
            None,
            Some("count(xs, 'a'), -n"),
            None,
            q(Coll::Tasks, None, &[("count(xs, 'a')", false), ("n", true)], None),
            "a comma inside a call is not a separator",
        ),
        (
            "tasks",
            None,
            Some("count(xs, len('a,b')), x"),
            None,
            q(Coll::Tasks, None, &[("count(xs, len('a,b'))", false), ("x", false)], None),
            "a comma inside nested calls and quotes",
        ),
        (
            "tasks",
            None,
            Some("'a,b'"),
            None,
            q(Coll::Tasks, None, &[("'a,b'", false)], None),
            "a comma inside quotes is not a separator",
        ),
        (
            "tasks",
            None,
            Some("\"a,b\", c"),
            None,
            q(Coll::Tasks, None, &[("\"a,b\"", false), ("c", false)], None),
            "a comma inside double quotes",
        ),
        (
            "tasks",
            None,
            Some("-'a,b', c"),
            None,
            q(Coll::Tasks, None, &[("'a,b'", true), ("c", false)], None),
            "descending text with a comma",
        ),
        (
            "tasks",
            None,
            Some("'it\\'s, ok', c"),
            None,
            q(Coll::Tasks, None, &[("'it\\'s, ok'", false), ("c", false)], None),
            "an escaped quote does not end the text",
        ),
        (
            "tasks",
            None,
            Some("count([1, 2]), x"),
            None,
            q(Coll::Tasks, None, &[("count([1, 2])", false), ("x", false)], None),
            "a comma inside brackets is not a separator",
        ),
        (
            "tasks",
            None,
            Some("xs[0], -/n"),
            None,
            q(Coll::Tasks, None, &[("xs[0]", false), ("/n", true)], None),
            "a pointer key, descending",
        ),
        ("tasks", None, Some("/n"), None, q(Coll::Tasks, None, &[("/n", false)], None), "a pointer key"),
        (
            "tasks",
            None,
            Some("-/a/b"),
            None,
            q(Coll::Tasks, None, &[("/a/b", true)], None),
            "a pointer key with slashes, descending",
        ),
        (
            "tasks",
            None,
            Some("state == 'open', -n"),
            None,
            q(Coll::Tasks, None, &[("state == 'open'", false), ("n", true)], None),
            "a comparison is a key",
        ),
        (
            "tasks",
            None,
            Some("-state == 'open'"),
            None,
            q(Coll::Tasks, None, &[("state == 'open'", true)], None),
            "descending on a comparison",
        ),
        ("tasks", None, Some(many.as_str()), None, q(Coll::Tasks, None, &many_want_ref, None), "MAX_ORDER keys"),
        ("tasks", None, None, Some(1), q(Coll::Tasks, None, &[], Some(1)), "the smallest limit"),
        (
            "tasks",
            None,
            None,
            Some(MAX_LIMIT),
            q(Coll::Tasks, None, &[], Some(MAX_LIMIT as usize)),
            "MAX_LIMIT itself is allowed",
        ),
        ("tasks", None, None, Some(50), q(Coll::Tasks, None, &[], Some(50)), ""),
        (
            "tasks",
            Some("a == 1 and (b == 2 or c)"),
            None,
            None,
            q(Coll::Tasks, Some("a == 1 and (b == 2 or c)"), &[], None),
            "a filter with parentheses",
        ),
        (
            "tasks",
            Some("/state=='done'"),
            None,
            None,
            q(Coll::Tasks, Some("/state=='done'"), &[], None),
            "a pointer and a comparison",
        ),
        (
            "tasks",
            Some("x in [1, 2, 3]"),
            None,
            None,
            q(Coll::Tasks, Some("x in [1, 2, 3]"), &[], None),
            "commas in a filter are for the expression",
        ),
    ];
    for (from, filter, order, limit, want, note) in rows {
        t.eq(format!("Query::parse({from:?}, {filter:?}, {order:?}, {limit:?})"), note, Ok(want), || {
            Query::parse(from, filter, order, limit).map_err(|e| e.join(" | "))
        });
    }
    t.finish("query_parse_shapes");
}

#[test]
fn query_parse_every_problem_at_once() {
    let mut t = Tally::new();
    let over_order = (0..=MAX_ORDER).map(|i| format!("k{i}")).collect::<Vec<_>>().join(", ");
    let long_filter = "a".repeat(2001);
    let ok_filter = "a".repeat(2000);
    // (from, filter, order, limit, fewest messages, a word the messages mention, note)
    #[allow(clippy::type_complexity)]
    let rows: Vec<(&str, Option<&str>, Option<&str>, Option<u64>, usize, &str, &str)> = vec![
        ("nope", None, None, None, 1, "", "a collection that is not one"),
        ("tasks", Some("a =="), None, None, 1, "", "a filter that does not parse"),
        ("tasks", Some("a b"), None, None, 1, "", "a filter that does not parse"),
        ("tasks", Some("foo(1)"), None, None, 1, "", "a function that is not there"),
        ("tasks", Some("1 < 2 < 3"), None, None, 1, "", "a comparison is between two things"),
        ("tasks", Some("and"), None, None, 1, "", "a word cannot be a name"),
        ("tasks", Some(long_filter.as_str()), None, None, 1, "", "longer than the longest expression"),
        ("tasks", Some(ok_filter.as_str()), None, None, 0, "", "exactly the longest expression is fine"),
        ("tasks", Some(""), None, None, 1, "", "DECIDED: an empty filter; reading: not an expression"),
        ("tasks", Some("   "), None, None, 1, "", "DECIDED: a blank filter; reading: not an expression"),
        ("tasks", None, Some("(("), None, 1, "", "an order key that does not parse"),
        ("tasks", None, Some("a =="), None, 1, "", "an order key that does not parse"),
        ("tasks", None, Some("-"), None, 1, "", "a minus and no key"),
        ("tasks", None, Some("a,"), None, 1, "", "DECIDED: a trailing comma; reading: an empty key"),
        ("tasks", None, Some(",a"), None, 1, "", "DECIDED: a leading comma; reading: an empty key"),
        ("tasks", None, Some("a,,b"), None, 1, "", "DECIDED: an empty key in the middle"),
        ("tasks", None, Some(""), None, 1, "", "DECIDED: an empty order; reading: an empty key"),
        ("tasks", None, Some("   "), None, 1, "", "DECIDED: a blank order; reading: an empty key"),
        ("tasks", None, Some(over_order.as_str()), None, 1, "", "more than MAX_ORDER keys"),
        ("tasks", None, Some("a,b,c,d,e"), None, 1, "", "five keys"),
        ("tasks", None, Some("a,b,c,d,e,f,g,h"), None, 1, "", "eight keys"),
        ("tasks", None, None, Some(0), 1, "", "DECIDED: limit 0; reading: refused (asking for no rows is a mistake)"),
        ("tasks", None, None, Some(MAX_LIMIT + 1), 1, "", "over MAX_LIMIT"),
        ("tasks", None, None, Some(MAX_LIMIT * 10), 1, "", "far over MAX_LIMIT"),
        ("tasks", None, None, Some(u64::MAX), 1, "", "no overflow"),
        ("tasks", None, None, Some(u64::MAX / 2), 1, "", "no overflow"),
        ("tasks", None, None, Some(1 << 32), 1, "", "no truncation to 32 bits"),
        ("nope", Some("a =="), None, None, 2, "", "two parts wrong: both reported"),
        ("nope", None, Some("a =="), None, 2, "", "two parts wrong: both reported"),
        ("nope", None, None, Some(MAX_LIMIT + 1), 2, "", "two parts wrong: both reported"),
        ("tasks", Some("a =="), Some("b <"), None, 2, "", "two parts wrong: both reported"),
        ("tasks", Some("a =="), None, Some(MAX_LIMIT + 1), 2, "", "two parts wrong: both reported"),
        ("tasks", None, Some("a =="), Some(MAX_LIMIT + 1), 2, "", "two parts wrong: both reported"),
        ("nope", Some("a =="), Some("b <"), Some(MAX_LIMIT + 1), 4, "", "every part wrong: every part reported"),
        ("nope", None, Some("a,b,c,d,e"), None, 2, "", "a bad collection and too many keys"),
        ("tasks", None, Some("a ==, b <"), None, 2, "", "DECIDED: two bad keys; reading: one message each"),
        ("tasks", Some("a =="), Some("a,b,c,d,e"), None, 2, "", "a bad filter and too many keys"),
        ("nope", Some("a =="), Some("b <, c >"), Some(MAX_LIMIT + 1), 4, "", "everything wrong"),
        ("nope", None, None, None, 1, "nope", "DECIDED: the message names what it refused"),
        ("tasks", Some("a =="), None, None, 1, "filter", "DECIDED: the message says which part"),
        ("tasks", None, Some("a =="), None, 1, "order", "DECIDED: the message says which part"),
        ("tasks", None, None, Some(MAX_LIMIT + 1), 1, "limit", "DECIDED: the message says which part"),
        ("nope", None, None, None, 1, "from", "DECIDED: the message says which part"),
    ];
    for (from, filter, order, limit, fewest, mention, note) in rows {
        t.holds(
            format!("Query::parse({from:?}, {}, {}, {limit:?}) gives {fewest} problem(s)", cut(filter), cut(order)),
            note,
            || match Query::parse(from, filter, order, limit) {
                Ok(_) if fewest == 0 => Ok(()),
                Ok(q) => Err(format!("parsed fine: {q:?}")),
                Err(_) if fewest == 0 => Err("refused".into()),
                Err(msgs) => {
                    if msgs.len() < fewest {
                        return Err(format!("{} message(s): {msgs:?}", msgs.len()));
                    }
                    if msgs.iter().any(|m| m.trim().is_empty()) {
                        return Err(format!("an empty message in {msgs:?}"));
                    }
                    if !mention.is_empty() && !msgs.iter().any(|m| m.to_lowercase().contains(mention)) {
                        return Err(format!("no message mentions {mention:?}: {msgs:?}"));
                    }
                    Ok(())
                }
            },
        );
    }
    t.holds("MAX_LIMIT and MAX_ORDER are what the docs of the tests assume (10000, 4)".into(), "", || {
        if MAX_LIMIT == 10_000 && MAX_ORDER == 4 {
            Ok(())
        } else {
            Err(format!("MAX_LIMIT={MAX_LIMIT} MAX_ORDER={MAX_ORDER}"))
        }
    });
    t.finish("query_parse_every_problem_at_once");
}

// --------------------------------------------------------------------------------------------- Query::apply

#[test]
fn apply_filter_by_row_fields() {
    let mut t = Tally::new();
    let env = outer();
    // (filter, want, note); base rows a..e
    let rows: &[(&str, &str, &str)] = &[
        ("state == 'done'", "a,c", ""),
        ("state != 'done'", "b,d,e", ""),
        ("state in ['open', 'fail']", "b,d,e", ""),
        ("n > 1", "a,c,d,e", ""),
        ("n >= 2", "a,c,d,e", ""),
        ("n < 2", "b", ""),
        ("n <= 2", "b,c,d", ""),
        ("n == 2", "c,d", ""),
        ("n != 2", "a,b,e", ""),
        ("n >= 2 and state == 'done'", "a,c", ""),
        ("state == 'done' or n == 5", "a,c,e", ""),
        ("n == 2 and state == 'fail'", "d", ""),
        ("data.cmd == 'ls'", "a,c", "a path into a row"),
        ("data.cmd != 'ls'", "b", "a row where it is not there is not 'not ls'"),
        ("not (data.cmd == 'ls')", "b,d,e", "the way to say 'is not ls, or is not there'"),
        ("cost", "a,b,c,e", "truth: money that is not $0 and is there"),
        ("not cost", "d", ""),
        ("cost > 1", "a,c", "money against a number counts in dollars"),
        ("cost >= $0.25", "a,b,c,e", ""),
        ("cost == 0.25", "b,e", "money against a number counts in dollars"),
        ("cost == $3", "c", ""),
        ("cost < $1", "b,e", ""),
        ("data.cmd matches 'r*'", "b", ""),
        ("data.cmd matches '?s'", "a,c", ""),
        ("data.cmd matches '*'", "a,b,c", "a missing value matches nothing"),
        ("state matches '*e*'", "a,b,c,e", ""),
        ("len(state) == 4", "a,b,c,d,e", ""),
        ("n in [1, 5]", "b,e", ""),
        ("'ls' in data.cmd", "a,c", "part in text"),
        ("'rm' in data.cmd", "b", ""),
        ("contains(data.cmd, 'ls')", "a,c", ""),
        ("id in ['a', 'c']", "a,c", ""),
        ("lower(state) == 'done'", "a,c", ""),
        ("same([n, 2])", "c,d", "a call over row fields"),
        ("sum([n, 1]) > 3", "a,e", ""),
        ("last([n, 9]) == 9", "a,b,c,d,e", ""),
        ("len(data) > 0", "a,b,c", "keys of a map; a missing map has no length"),
        ("row.state == 'done'", "a,c", "row is the whole row"),
        ("row.data.cmd == 'ls'", "a,c", ""),
        ("row.id == 'e'", "e", ""),
        ("true", "a,b,c,d,e", ""),
        ("false", "", ""),
        ("1", "a,b,c,d,e", "a number is yes"),
        ("0", "", "zero is no"),
        ("'x'", "a,b,c,d,e", "text is yes"),
        ("''", "", "empty text is no"),
    ];
    for &(filter, want, note) in rows {
        t.eq(format!("filter {filter:?} over a..e"), note, want.to_string(), || {
            run("tasks", Some(filter), None, None, base(), &env)
        });
    }
    t.finish("apply_filter_by_row_fields");
}

#[test]
fn apply_filter_by_pointer_and_by_row() {
    let mut t = Tally::new();
    let env = outer();
    // pointers read into the row
    let rows: &[(&str, &str, &str)] = &[
        ("/state == 'done'", "a,c", ""),
        ("/state=='done'", "a,c", "a pointer ends at '='"),
        ("/data/cmd == 'ls'", "a,c", ""),
        ("/data/cmd=='ls'", "a,c", ""),
        ("/n > 2", "a,e", ""),
        ("/cost", "a,b,c,e", ""),
        ("/cost > 1", "a,c", ""),
        ("/nope", "", ""),
        ("/data/nope", "", ""),
        ("not /nope", "a,b,c,d,e", ""),
        ("/data", "a,b,c", "a non-empty map is yes, an empty map or no map is no"),
        ("/id == 'b'", "b", ""),
        ("/id in ['a', 'e']", "a,e", ""),
        ("/state == 'done' and /n > 2", "a", ""),
    ];
    for &(filter, want, note) in rows {
        t.eq(format!("filter {filter:?} over a..e"), note, want.to_string(), || {
            run("tasks", Some(filter), None, None, base(), &env)
        });
    }
    // a pointer reads the row, never the outer document
    let with_doc = Scope::new().doc(map(&[("state", sv("done")), ("n", n(9.0))]));
    t.eq(
        "pointer /state with an outer document that has state, rows without".into(),
        "a pointer reads into the row",
        String::new(),
        || run("tasks", Some("/state == 'done'"), None, None, vec![row("z", &[])], &with_doc),
    );
    t.eq("pointer /n with an outer document that has n, rows with another n".into(), "", "b".to_string(), || {
        run(
            "tasks",
            Some("/n < 5"),
            None,
            None,
            vec![row("a", &[("n", n(7.0))]), row("b", &[("n", n(1.0))])],
            &with_doc,
        )
    });
    // rows that are not maps
    let nums = || vec![n(1.0), n(2.0), n(3.0), n(4.0), n(5.0)];
    let num_rows: &[(&str, &str, &str)] = &[
        ("row > 2", "3,4,5", ""),
        ("row == 3", "3", ""),
        ("row != 3", "1,2,4,5", ""),
        ("row in [1, 5]", "1,5", ""),
        ("row >= 2 and row <= 4", "2,3,4", ""),
        ("not (row > 1)", "1", ""),
        ("row", "1,2,3,4,5", "every number here is yes"),
        ("row.x", "", "a name into a number"),
        ("state == 'x'", "", "a name that is not a field of a number is missing"),
        ("/ > 2", "3,4,5", "DECIDED: a pointer `/` alone is the whole document, here the row"),
        ("/x", "", "a pointer into a number"),
    ];
    for &(filter, want, note) in num_rows {
        t.eq(format!("filter {filter:?} over rows 1..5"), note, want.to_string(), || {
            run("tasks", Some(filter), None, None, nums(), &env)
        });
    }
    t.eq("filter \"row\" over rows 0,1,2".into(), "zero is no", "1,2".to_string(), || {
        run("tasks", Some("row"), None, None, vec![n(0.0), n(1.0), n(2.0)], &env)
    });
    let words = || vec![sv("apple"), sv("Banana"), sv("cherry")];
    let word_rows: &[(&str, &str, &str)] = &[
        ("row matches 'a*'", "apple", ""),
        ("row matches '*a*'", "apple,Banana", ""),
        ("len(row) > 5", "Banana,cherry", ""),
        ("lower(row) == 'banana'", "Banana", ""),
        ("'an' in row", "Banana", ""),
        ("row == 'apple'", "apple", ""),
        ("row < 'b'", "apple,Banana", "text compares by character: B is before b"),
        ("row > 'b'", "cherry", ""),
    ];
    for &(filter, want, note) in word_rows {
        t.eq(format!("filter {filter:?} over rows apple, Banana, cherry"), note, want.to_string(), || {
            run("tasks", Some(filter), None, None, words(), &env)
        });
    }
    let mixed = || {
        vec![
            n(1.0),
            sv("x"),
            Value::Bool(true),
            usd(2.0),
            list(vec![n(1.0)]),
            map(&[("a", n(1.0))]),
            Value::Missing,
            Value::Dur(1000),
        ]
    };
    t.eq("filter \"row == 1\" over mixed kinds".into(), "", "1".to_string(), || {
        run("tasks", Some("row == 1"), None, None, mixed(), &env)
    });
    t.eq("filter \"row == $2\" over mixed kinds".into(), "money", format!("{:?}", usd(2.0)), || {
        run("tasks", Some("row == $2"), None, None, mixed(), &env)
    });
    t.eq(
        "filter \"row == 2\" over mixed kinds matches the money $2".into(),
        "a number against money counts in dollars",
        format!("{:?}", usd(2.0)),
        || run("tasks", Some("row == 2"), None, None, mixed(), &env),
    );
    t.eq("filter \"row == 1s\" over mixed kinds".into(), "", format!("{:?}", Value::Dur(1000)), || {
        run("tasks", Some("row == 1s"), None, None, mixed(), &env)
    });
    t.eq("filter \"row\" keeps every row that is yes and drops missing and the like".into(), "", 7, || {
        match Query::parse("tasks", Some("row"), None, None) {
            Ok(q) => q.apply(mixed(), &env).len(),
            Err(_) => 0,
        }
    });
    t.finish("apply_filter_by_pointer_and_by_row");
}

#[test]
fn apply_filter_through_the_outer_env() {
    let mut t = Tally::new();
    let env = outer();
    // (filter, want, note); fold.spend = 2, set.loop = 'x'
    let rows: &[(&str, &str, &str)] = &[
        ("n > fold.spend", "a,e", "a root of the outer Env"),
        ("n == fold.spend", "c,d", ""),
        ("n < fold.spend", "b", ""),
        ("cost < fold.spend", "a,b,e", "money against a number counts in dollars"),
        ("cost > fold.spend", "c", ""),
        ("set.loop == 'x'", "a,b,c,d,e", "the same for every row"),
        ("set.loop == 'y'", "", ""),
        ("state == set.loop", "", ""),
        ("fold.spend == 2", "a,b,c,d,e", ""),
        ("fold.spend", "a,b,c,d,e", ""),
        ("fold.nope == 1", "", "a path that leads nowhere"),
        ("nosuchroot.x == 1", "", "a root nobody defines"),
        ("nosuchroot", "", ""),
        ("not nosuchroot", "a,b,c,d,e", ""),
        ("n > fold.spend and state == set.loop", "", ""),
        ("n > fold.spend or state == 'fail'", "a,d,e", ""),
        ("n in [fold.spend, 5]", "c,d,e", ""),
    ];
    for &(filter, want, note) in rows {
        t.eq(format!("filter {filter:?} over a..e, fold.spend=2"), note, want.to_string(), || {
            run("tasks", Some(filter), None, None, base(), &env)
        });
    }
    // a root of the outer Env comes first (a row cannot shadow it), then a field of the row
    let shadow = vec![row("p", &[("fold", n(1.0))]), row("q", &[("fold", n(2.0))]), row("r", &[])];
    let fold1 = Scope::new().with("fold", n(1.0));
    t.eq(
        "an outer root before a row field: fold == 1 over p(fold=1) q(fold=2) r(no fold), outer fold=1".into(),
        "",
        "p,q,r".to_string(),
        || run("tasks", Some("fold == 1"), None, None, shadow.clone(), &fold1),
    );
    t.eq("an outer root before a row field: fold == 2".into(), "", String::new(), || {
        run("tasks", Some("fold == 2"), None, None, shadow.clone(), &fold1)
    });
    t.eq("an outer root before a row field: fold != 1".into(), "", String::new(), || {
        run("tasks", Some("fold != 1"), None, None, shadow.clone(), &fold1)
    });
    let deeper = vec![row("x", &[("data", map(&[]))]), row("y", &[])];
    let data_env = Scope::new().with("data", map(&[("cmd", sv("ls"))]));
    t.eq(
        "row has the first name but not the path: data.cmd, outer data.cmd='ls'".into(),
        "the outer Env has the root, so the row's own data is not read",
        "x,y".to_string(),
        || run("tasks", Some("data.cmd == 'ls'"), None, None, deeper.clone(), &data_env),
    );
    // the clock and the danger matcher of the outer Env
    let clock = Scope::new().at_ms(10_000);
    let times = || vec![row("a", &[("t", n(9000.0))]), row("b", &[("t", n(1000.0))]), row("c", &[])];
    t.eq(
        "since(t) < 5s with the outer clock at 10000".into(),
        "DECIDED: the clock is the outer Env's",
        "a".to_string(),
        || run("tasks", Some("since(t) < 5s"), None, None, times(), &clock),
    );
    t.eq(
        "since(t) > 5s with the outer clock at 10000".into(),
        "DECIDED: the clock is the outer Env's",
        "b".to_string(),
        || run("tasks", Some("since(t) > 5s"), None, None, times(), &clock),
    );
    t.eq("since(t) < 5s with no clock".into(), "no clock: since is missing", String::new(), || {
        run("tasks", Some("since(t) < 5s"), None, None, times(), &Scope::new())
    });
    t.eq(
        "order by since(t) with the outer clock".into(),
        "DECIDED: the clock is the outer Env's",
        "a,b,c".to_string(),
        || run("tasks", None, Some("since(t)"), None, times(), &clock),
    );
    let guard = Scope::new().danger_with(dg);
    t.eq(
        "danger(data.cmd) with the outer matcher".into(),
        "DECIDED: the matcher is the outer Env's",
        "b".to_string(),
        || run("tasks", Some("danger(data.cmd)"), None, None, base(), &guard),
    );
    t.eq("danger(data.cmd) with no matcher".into(), "", String::new(), || {
        run("tasks", Some("danger(data.cmd)"), None, None, base(), &Scope::new())
    });
    // the outer Env is asked first about every name, and a row field is read only when it has no such root
    t.holds("a name is asked of the outer Env before it is read as a row field".into(), "", || {
        let spy = Spy::new();
        let out = run("tasks", Some("state == 'done' and n > 1 and id != 'z'"), None, None, base(), &spy);
        let asked = spy.roots();
        if out == "a,c" && asked.iter().all(|r| ["state", "n", "id"].contains(&r.as_str())) && !asked.is_empty() {
            Ok(())
        } else {
            Err(format!("out {out:?}, asked {asked:?}"))
        }
    });
    t.holds("`row` and a pointer never reach the outer Env".into(), "", || {
        let spy = Spy::new();
        let out = run("tasks", Some("row.state == 'done' and /n > 1"), None, None, base(), &spy);
        let asked = spy.roots();
        if out == "a,c" && asked.is_empty() { Ok(()) } else { Err(format!("out {out:?}, asked {asked:?}")) }
    });
    t.holds("a name that is not a field is asked of the outer Env with its path".into(), "", || {
        let spy = Spy::new();
        let out = run("tasks", Some("fold.spend.x > 1"), None, None, vec![row("a", &[])], &spy);
        let asked = spy.seen.borrow().clone();
        if out.is_empty()
            && asked.iter().any(|(r, p)| r == "fold" && *p == vec![Key::Name("spend".into()), Key::Name("x".into())])
        {
            Ok(())
        } else {
            Err(format!("out {out:?}, asked {asked:?}"))
        }
    });
    t.holds("an order key that is not a field is asked of the outer Env".into(), "", || {
        let spy = Spy::new();
        let out = run("tasks", None, Some("weight.w"), None, base(), &spy);
        let asked = spy.seen.borrow().clone();
        if out == "a,b,c,d,e" && asked.iter().any(|(r, p)| r == "weight" && *p == vec![Key::Name("w".into())]) {
            Ok(())
        } else {
            Err(format!("out {out:?}, asked {asked:?}"))
        }
    });
    t.finish("apply_filter_through_the_outer_env");
}

#[test]
fn apply_filter_that_is_missing_everywhere() {
    let mut t = Tally::new();
    let env = outer();
    let rows: &[(&str, &str, &str)] = &[
        ("nope", "", "missing is no"),
        ("nope == 1", "", "nothing compares with nothing"),
        ("nope != 1", "", "missing != 1 is false"),
        ("nope < 1", "", ""),
        ("nope in [1]", "", ""),
        ("nope matches 'x*'", "", ""),
        ("'a' in nope", "", ""),
        ("nope.deeper.still", "", ""),
        ("nope[0]", "", ""),
        ("len(nope) > 0", "", ""),
        ("nope and true", "", ""),
        ("nope or false", "", ""),
        ("not nope", "a,b,c,d,e", "not missing is yes"),
        ("not (nope == 1)", "a,b,c,d,e", "the way to say 'is not 1, or is not there'"),
        ("nope or true", "a,b,c,d,e", ""),
        ("nope == nope", "", "missing == missing is false"),
        ("count(nope) == 0", "a,b,c,d,e", "count reads missing as an empty list"),
        ("sum(nope) == 0", "a,b,c,d,e", "nothing to add is 0"),
        ("same(nope)", "", "same of an empty list is no"),
    ];
    for &(filter, want, note) in rows {
        t.eq(format!("filter {filter:?} (no row has it)"), note, want.to_string(), || {
            run("tasks", Some(filter), None, None, base(), &env)
        });
    }
    t.eq("a filter that holds for no row, order and limit set".into(), "", String::new(), || {
        run("tasks", Some("nope"), Some("-n"), Some(3), base(), &env)
    });
    t.eq("a filter that holds for no row, on rows that are not maps".into(), "", String::new(), || {
        run("tasks", Some("nope"), None, None, vec![n(1.0), sv("x")], &env)
    });
    t.finish("apply_filter_that_is_missing_everywhere");
}

#[test]
fn apply_order_keys() {
    let mut t = Tally::new();
    let env = Scope::new().with(
        "fold",
        map(&[
            ("w", map(&[("a", n(3.0)), ("b", n(1.0)), ("c", n(2.0)), ("d", n(5.0)), ("e", n(4.0))])),
            ("spend", n(2.0)),
        ]),
    );
    // (order, want, note); base rows a..e: n=3,1,2,2,5; cost=1.5,.25,3,-,.25; state=done,open,done,fail,open
    let rows: &[(&str, &str, &str)] = &[
        ("n", "b,c,d,a,e", "ties c, d keep their place"),
        ("-n", "e,a,c,d,b", "descending: ties c, d still keep their place (not reversed)"),
        ("state, n", "c,a,d,b,e", ""),
        ("state, -n", "a,c,d,e,b", ""),
        ("-state, n", "b,e,d,c,a", ""),
        ("-state, -n", "e,b,d,a,c", ""),
        ("cost", "b,e,a,c,d", "money; a row without it last; ties b, e keep their place"),
        ("-cost", "c,a,b,e,d", "descending: the row without it is still last"),
        ("cost, n", "b,e,a,c,d", ""),
        ("-cost, -n", "c,a,e,b,d", ""),
        ("cost, -n", "e,b,a,c,d", "a tie on the first key is broken by the second"),
        ("-cost, n", "c,a,b,e,d", ""),
        ("state, cost", "a,c,d,b,e", ""),
        ("data.cmd", "a,c,b,d,e", "a path; rows without it last, in their place"),
        ("-data.cmd", "b,a,c,d,e", "rows without it last in both directions"),
        ("data.cmd, n", "c,a,b,d,e", "a tie on the first key is broken by the second; missing ones are ties"),
        ("-data.cmd, n", "b,c,a,d,e", ""),
        ("len(data.cmd)", "a,c,b,d,e", "a call as a key"),
        ("-len(data.cmd)", "b,a,c,d,e", ""),
        ("id", "a,b,c,d,e", ""),
        ("-id", "e,d,c,b,a", ""),
        ("lower(state), id", "a,c,d,b,e", ""),
        ("/n", "b,c,d,a,e", "a pointer as a key"),
        ("-/n", "e,a,c,d,b", ""),
        ("/cost", "b,e,a,c,d", ""),
        ("row.n", "b,c,d,a,e", "row is the whole row"),
        ("-row.n", "e,a,c,d,b", ""),
        ("nope", "a,b,c,d,e", "missing for every row: nobody moves"),
        ("-nope", "a,b,c,d,e", "missing for every row: nobody moves, descending too"),
        ("nope, n", "b,c,d,a,e", "the first key ties everywhere, the second decides"),
        ("n, nope", "b,c,d,a,e", ""),
        ("fold.w[id]", "b,c,a,e,d", "a key looked up in the outer Env by a field of the row"),
        ("-fold.w[id]", "d,e,a,c,b", ""),
        ("fold.spend", "a,b,c,d,e", "the same key for every row: nobody moves"),
        ("-fold.spend", "a,b,c,d,e", ""),
        ("state == 'open'", "a,c,d,b,e", "a yes/no key: no before yes"),
        ("-state == 'open'", "b,e,a,c,d", "descending: yes before no"),
        ("n > 2", "b,c,d,a,e", "DECIDED: false rows first (b,c,d), then true rows (a,e), each in place"),
        ("'a,b'", "a,b,c,d,e", "a constant text key"),
        ("count(['x', 'y'], 'x'), n", "b,c,d,a,e", "a comma inside a call and inside a list"),
        ("n, id", "b,c,d,a,e", ""),
        ("n, -id", "b,d,c,a,e", ""),
        ("-n, -id", "e,a,d,c,b", ""),
        ("state, n, id", "c,a,d,b,e", ""),
        ("state, -n, -id", "a,c,d,e,b", ""),
        ("n, state, cost, id", "b,c,d,a,e", "four keys"),
    ];
    for &(order, want, note) in rows {
        t.eq(format!("order {order:?} over a..e"), note, want.to_string(), || {
            run("tasks", None, Some(order), None, base(), &env)
        });
    }
    // ties keep the place they had in the input
    let ties: &[(&str, &str, &str, &str)] = &[
        ("edcba", "n", "b,d,c,a,e", "d before c as in the input"),
        ("edcba", "-n", "e,a,d,c,b", "d before c as in the input"),
        ("cdabe", "n", "b,c,d,a,e", ""),
        ("dcabe", "n", "b,d,c,a,e", ""),
        ("dcabe", "-n", "e,a,d,c,b", ""),
        ("bdace", "state", "a,c,d,b,e", "done: a,c in input order; open: b,e in input order"),
        ("ecabd", "-state", "e,b,d,c,a", "open: e,b in input order; done: c,a in input order"),
    ];
    for &(input, order, want, note) in ties {
        t.eq(format!("order {order:?} over rows in the order {input}"), note, want.to_string(), || {
            run("tasks", None, Some(order), None, base_in(input), &env)
        });
    }
    t.finish("apply_order_keys");
}

#[test]
fn apply_order_stability_over_many_rows() {
    let mut t = Tally::new();
    let env = Scope::new();
    let count = 300usize;
    // key = i % 7; every 11th row has no key
    let key = |i: usize| if i.is_multiple_of(11) { None } else { Some((i % 7) as f64) };
    let rows = || -> Vec<Value> {
        (0..count)
            .map(|i| match key(i) {
                Some(k) => row(&format!("{i:03}"), &[("k", n(k))]),
                None => row(&format!("{i:03}"), &[]),
            })
            .collect()
    };
    let expect = |desc: bool| -> String {
        let mut idx: Vec<usize> = (0..count).collect();
        idx.sort_by(|&a, &b| match (key(a), key(b)) {
            (Some(x), Some(y)) => {
                if desc {
                    y.partial_cmp(&x).unwrap()
                } else {
                    x.partial_cmp(&y).unwrap()
                }
            }
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        });
        idx.iter().map(|i| format!("{i:03}")).collect::<Vec<_>>().join(",")
    };
    t.eq(
        "300 rows with 7 distinct keys and holes, ascending".into(),
        "a stable sort, missing last",
        expect(false),
        || run("tasks", None, Some("k"), None, rows(), &env),
    );
    t.eq(
        "300 rows with 7 distinct keys and holes, descending".into(),
        "a stable sort (ties in input order), missing last",
        expect(true),
        || run("tasks", None, Some("-k"), None, rows(), &env),
    );
    t.eq(
        "the same with the ascending limit 25".into(),
        "",
        expect(false).split(',').take(25).collect::<Vec<_>>().join(","),
        || run("tasks", None, Some("k"), Some(25), rows(), &env),
    );
    t.eq(
        "the same with the descending limit 25".into(),
        "",
        expect(true).split(',').take(25).collect::<Vec<_>>().join(","),
        || run("tasks", None, Some("-k"), Some(25), rows(), &env),
    );
    t.holds("sorting twice gives the same answer (idempotent on its own output)".into(), "", || {
        let q = Query::parse("tasks", None, Some("-k, id"), None).map_err(|e| format!("{e:?}"))?;
        let once = q.apply(rows(), &env);
        let twice = q.apply(once.clone(), &env);
        if once == twice { Ok(()) } else { Err("a second pass moved rows".into()) }
    });
    t.holds("every row comes out exactly once".into(), "", || {
        let q = Query::parse("tasks", None, Some("-k"), None).map_err(|e| format!("{e:?}"))?;
        let mut out: Vec<String> = q.apply(rows(), &env).iter().map(label).collect();
        out.sort();
        let want: Vec<String> = (0..count).map(|i| format!("{i:03}")).collect();
        if out == want { Ok(()) } else { Err(format!("{} rows out", out.len())) }
    });
    t.holds("a thousand identical keys keep the input order, ascending and descending".into(), "", || {
        let same = || (0..1000).map(|i| row(&format!("{i:04}"), &[("k", n(1.0))])).collect::<Vec<_>>();
        let want = (0..1000).map(|i| format!("{i:04}")).collect::<Vec<_>>().join(",");
        for order in ["k", "-k"] {
            let got = run("tasks", None, Some(order), None, same(), &env);
            if got != want {
                return Err(format!("order {order:?} moved rows"));
            }
        }
        Ok(())
    });
    t.holds("5000 rows, a filter, an order and a limit".into(), "", || {
        let big = (0..5000)
            .map(|i| row(&format!("{i:04}"), &[("k", n(((i * 7919) % 1000) as f64)), ("ok", Value::Bool(i % 2 == 0))]))
            .collect::<Vec<_>>();
        let q = Query::parse("tasks", Some("ok"), Some("-k, id"), Some(10)).map_err(|e| format!("{e:?}"))?;
        let out = q.apply(big, &env);
        if out.len() != 10 {
            return Err(format!("{} rows", out.len()));
        }
        let mut last = f64::INFINITY;
        for r in &out {
            if let Value::Map(m) = r {
                if let Some(Value::Num(k)) = m.get("k") {
                    if *k > last {
                        return Err("not descending".into());
                    }
                    last = *k;
                }
                if m.get("ok") != Some(&Value::Bool(true)) {
                    return Err("a row the filter holds for no longer".into());
                }
            }
        }
        Ok(())
    });
    t.finish("apply_order_stability_over_many_rows");
}

#[test]
fn apply_order_mixed_kinds() {
    let mut t = Tally::new();
    let env = Scope::new();
    // kinds in a fixed order, then Value::order inside a kind; the rest keep their place; missing last
    let mixed = || {
        vec![
            row("n1", &[("k", n(2.0))]),
            row("s1", &[("k", sv("b"))]),
            row("n2", &[("k", n(1.0))]),
            row("s2", &[("k", sv("a"))]),
            row("m", &[]),
            row("b1", &[("k", Value::Bool(true))]),
            row("d1", &[("k", Value::Dur(1000))]),
            row("n3", &[("k", n(-5.0))]),
            row("d2", &[("k", Value::Dur(10))]),
            row("s3", &[("k", sv("A"))]),
        ]
    };
    let kind_of = |id: &str| id.chars().next().unwrap();
    t.holds("mixed kinds ascending: each kind in one run, inside a run in order, missing last".into(), "", || {
        let got = run("tasks", None, Some("k"), None, mixed(), &env);
        let order: Vec<&str> = got.split(',').collect();
        if order.len() != 10 {
            return Err(got.to_string());
        }
        if *order.last().unwrap() != "m" {
            return Err(format!("missing is not last: {got}"));
        }
        let pos = |id: &str| order.iter().position(|x| *x == id).unwrap();
        if !(pos("n3") < pos("n2") && pos("n2") < pos("n1")) {
            return Err(format!("numbers out of order: {got}"));
        }
        if !(pos("s3") < pos("s2") && pos("s2") < pos("s1")) {
            return Err(format!("text out of order (A, a, b by character): {got}"));
        }
        if !(pos("d2") < pos("d1")) {
            return Err(format!("durations out of order: {got}"));
        }
        let mut seen: Vec<char> = Vec::new();
        for id in &order[..order.len() - 1] {
            let k = kind_of(id);
            if seen.last() != Some(&k) {
                if seen.contains(&k) {
                    return Err(format!("kind {k} is split into several runs: {got}"));
                }
                seen.push(k);
            }
        }
        Ok(())
    });
    t.holds("mixed kinds descending: the ascending order reversed, missing still last".into(), "", || {
        let asc = run("tasks", None, Some("k"), None, mixed(), &env);
        let desc = run("tasks", None, Some("-k"), None, mixed(), &env);
        let mut a: Vec<&str> = asc.split(',').collect();
        let d: Vec<&str> = desc.split(',').collect();
        let m = a.pop();
        a.reverse();
        a.push(m.unwrap());
        if a == d { Ok(()) } else { Err(format!("asc {asc}\n      desc {desc}")) }
    });
    t.eq("mixed kinds: the answer is the same from any start order (distinct values)".into(), "", true, || {
        let base_rows = mixed();
        let want = run("tasks", None, Some("k"), None, base_rows.clone(), &env);
        (0..base_rows.len()).all(|r| {
            let mut v = base_rows.clone();
            v.rotate_left(r);
            run("tasks", None, Some("k"), None, v, &env) == want
        })
    });
    let lists = || {
        vec![
            row("l1", &[("k", list(vec![n(1.0)]))]),
            row("l2", &[("k", list(vec![n(0.0)]))]),
            row("l3", &[("k", list(vec![n(2.0)]))]),
        ]
    };
    t.eq(
        "lists as keys have no order: they keep their place".into(),
        "the rest keep their place",
        "l1,l2,l3".to_string(),
        || run("tasks", None, Some("k"), None, lists(), &env),
    );
    t.eq(
        "lists as keys have no order: they keep their place, descending too".into(),
        "the rest keep their place",
        "l1,l2,l3".to_string(),
        || run("tasks", None, Some("-k"), None, lists(), &env),
    );
    let maps = || vec![row("m1", &[("k", map(&[("a", n(2.0))]))]), row("m2", &[("k", map(&[("a", n(1.0))]))])];
    t.eq(
        "maps as keys have no order: they keep their place".into(),
        "the rest keep their place",
        "m1,m2".to_string(),
        || run("tasks", None, Some("k"), None, maps(), &env),
    );
    t.eq(
        "maps as keys have no order: they keep their place, descending too".into(),
        "the rest keep their place",
        "m1,m2".to_string(),
        || run("tasks", None, Some("-k"), None, maps(), &env),
    );
    let with_list = || {
        vec![row("x", &[("k", n(3.0))]), row("l", &[("k", list(vec![]))]), row("y", &[("k", n(1.0))]), row("z", &[])]
    };
    t.holds("a list among numbers: the numbers are in order, the missing one is last".into(), "", || {
        let got = run("tasks", None, Some("k"), None, with_list(), &env);
        let order: Vec<&str> = got.split(',').collect();
        let pos = |id: &str| order.iter().position(|x| *x == id).unwrap();
        if *order.last().unwrap() == "z" && pos("y") < pos("x") { Ok(()) } else { Err(got) }
    });
    let bools = || {
        vec![
            row("x", &[("ok", Value::Bool(true))]),
            row("y", &[("ok", Value::Bool(false))]),
            row("z", &[("ok", Value::Bool(true))]),
        ]
    };
    t.eq(
        "bool keys ascending".into(),
        "DECIDED: bools; reading: false before true, ties in place",
        "y,x,z".to_string(),
        || run("tasks", None, Some("ok"), None, bools(), &env),
    );
    t.eq(
        "bool keys descending".into(),
        "DECIDED: bools; reading: true before false, ties in place",
        "x,z,y".to_string(),
        || run("tasks", None, Some("-ok"), None, bools(), &env),
    );
    let durs = || {
        vec![
            row("x", &[("t", Value::Dur(2000))]),
            row("y", &[("t", Value::Dur(500))]),
            row("z", &[("t", Value::Dur(2000))]),
            row("w", &[]),
        ]
    };
    t.eq("duration keys ascending".into(), "", "y,x,z,w".to_string(), || {
        run("tasks", None, Some("t"), None, durs(), &env)
    });
    t.eq("duration keys descending".into(), "", "x,z,y,w".to_string(), || {
        run("tasks", None, Some("-t"), None, durs(), &env)
    });
    let words = || {
        vec![
            row("1", &[("w", sv("banana"))]),
            row("2", &[("w", sv("Apple"))]),
            row("3", &[("w", sv("cherry"))]),
            row("4", &[("w", sv("apple"))]),
            row("5", &[("w", sv("Banana"))]),
        ]
    };
    t.eq(
        "text keys by character, ascending".into(),
        "text compares by character: capitals first",
        "2,5,4,1,3".to_string(),
        || run("tasks", None, Some("w"), None, words(), &env),
    );
    t.eq("text keys by character, descending".into(), "", "3,1,4,5,2".to_string(), || {
        run("tasks", None, Some("-w"), None, words(), &env)
    });
    let nums = || {
        [10.0, 9.0, -1.0, 0.5, -0.5, 100.0, 2.0]
            .iter()
            .enumerate()
            .map(|(i, x)| row(&format!("{}", i + 1), &[("x", n(*x))]))
            .collect::<Vec<_>>()
    };
    t.eq("number keys are numbers, not text (9 before 10)".into(), "", "3,5,4,7,2,1,6".to_string(), || {
        run("tasks", None, Some("x"), None, nums(), &env)
    });
    t.eq("number keys descending".into(), "", "6,1,2,7,4,5,3".to_string(), || {
        run("tasks", None, Some("-x"), None, nums(), &env)
    });
    let numeric_text =
        || vec![row("a", &[("w", sv("10"))]), row("b", &[("w", sv("9"))]), row("c", &[("w", sv("100"))])];
    t.eq(
        "text that looks like numbers sorts as text".into(),
        "text compares by character",
        "a,c,b".to_string(),
        || run("tasks", None, Some("w"), None, numeric_text(), &env),
    );
    let money_mix = || {
        vec![
            row("x", &[("k", n(2.0))]),
            row("y", &[("k", usd(1.5))]),
            row("z", &[("k", n(1.0))]),
            row("w", &[("k", usd(3.0))]),
        ]
    };
    t.eq(
        "a number against money in one key".into(),
        "a number sorts before money, whatever the dollars",
        "z,x,y,w".to_string(),
        || run("tasks", None, Some("k"), None, money_mix(), &env),
    );
    let money = || vec![row("x", &[("c", usd(3.0))]), row("y", &[("c", usd(0.5))]), row("z", &[("c", usd(-1.0))])];
    t.eq("money keys ascending".into(), "", "z,y,x".to_string(), || run("tasks", None, Some("c"), None, money(), &env));
    t.eq("money keys descending".into(), "", "x,y,z".to_string(), || {
        run("tasks", None, Some("-c"), None, money(), &env)
    });
    t.finish("apply_order_mixed_kinds");
}

#[test]
fn apply_order_never_panics_on_odd_keys() {
    let mut t = Tally::new();
    let env = Scope::new();
    t.holds(
        "near-equal numbers and money in one key, 200 rows".into(),
        "DECIDED: a number finer than a millionth against money; reading: still a usable order",
        || {
            let keys = [
                n(1.5),
                usd(1.5),
                n(1.5000001),
                Value::Usd(1_500_001),
                n(1.4999999),
                Value::Usd(1_499_999),
                n(1.5000004),
                n(1.4999996),
            ];
            let rows: Vec<Value> = (0..200)
                .map(|i| row(&format!("{i:03}"), &[("k", keys[(i * 5 + i / 3) % keys.len()].clone())]))
                .collect();
            for order in ["k", "-k"] {
                let q = Query::parse("tasks", None, Some(order), None).map_err(|e| format!("{e:?}"))?;
                let out = q.apply(rows.clone(), &env);
                if out.len() != rows.len() {
                    return Err(format!("{} rows out", out.len()));
                }
            }
            Ok(())
        },
    );
    t.holds("a NaN number in a key (outside the contract: a number is finite)".into(), "robustness", || {
        let rows: Vec<Value> = (0..100)
            .map(|i| {
                let k = match i % 4 {
                    0 => Value::Num(f64::NAN),
                    1 => n(i as f64),
                    2 => Value::Num(f64::INFINITY),
                    _ => n(-(i as f64)),
                };
                row(&format!("{i:03}"), &[("k", k)])
            })
            .collect();
        for order in ["k", "-k"] {
            let q = Query::parse("tasks", None, Some(order), None).map_err(|e| format!("{e:?}"))?;
            let out = q.apply(rows.clone(), &env);
            if out.len() != rows.len() {
                return Err(format!("{} rows out", out.len()));
            }
        }
        Ok(())
    });
    t.holds("a NaN number in a filter (outside the contract)".into(), "robustness", || {
        let rows = vec![row("a", &[("k", Value::Num(f64::NAN))]), row("b", &[("k", n(1.0))])];
        let q = Query::parse("tasks", Some("k == k or k < 5 or k > 5"), None, None).map_err(|e| format!("{e:?}"))?;
        let out = q.apply(rows, &env);
        if out.len() <= 2 { Ok(()) } else { Err("more rows out than in".into()) }
    });
    t.holds("every kind of value as a row, ordered by row, nothing lost".into(), "", || {
        let rows = vec![
            n(2.0),
            sv("b"),
            Value::Bool(true),
            usd(1.0),
            Value::Dur(5),
            list(vec![]),
            map(&[]),
            Value::Missing,
            n(1.0),
            sv("a"),
        ];
        let q = Query::parse("tasks", None, Some("row"), None).map_err(|e| format!("{e:?}"))?;
        let out = q.apply(rows.clone(), &env);
        if out.len() != rows.len() {
            return Err(format!("{} rows out", out.len()));
        }
        if out.last() != Some(&Value::Missing) {
            return Err(format!("missing is not last: {out:?}"));
        }
        Ok(())
    });
    t.holds("a row that is missing is dropped by `row` as a filter and kept without a filter".into(), "", || {
        let rows = vec![Value::Missing, n(1.0)];
        let kept = Query::parse("tasks", None, None, None).map_err(|e| format!("{e:?}"))?.apply(rows.clone(), &env);
        let held = Query::parse("tasks", Some("row"), None, None).map_err(|e| format!("{e:?}"))?.apply(rows, &env);
        if kept.len() == 2 && held == vec![n(1.0)] { Ok(()) } else { Err(format!("kept {kept:?} held {held:?}")) }
    });
    t.finish("apply_order_never_panics_on_odd_keys");
}

#[test]
fn apply_limit_and_the_empty_cases() {
    let mut t = Tally::new();
    let env = outer();
    // (filter, order, limit, want, note)
    #[allow(clippy::type_complexity)]
    let rows: &[(Option<&str>, Option<&str>, Option<u64>, &str, &str)] = &[
        (None, None, None, "a,b,c,d,e", "nothing asked: everything, in place"),
        (None, None, Some(2), "a,b", "the first rows in place"),
        (None, None, Some(1), "a", ""),
        (None, None, Some(5), "a,b,c,d,e", "exactly as many as there are"),
        (None, None, Some(6), "a,b,c,d,e", "more than there are"),
        (None, None, Some(10), "a,b,c,d,e", ""),
        (None, None, Some(MAX_LIMIT), "a,b,c,d,e", ""),
        (Some("state == 'done'"), None, Some(1), "a", "the limit counts rows the filter holds for"),
        (Some("state == 'done'"), None, Some(5), "a,c", ""),
        (Some("n > 1"), Some("n"), Some(3), "c,d,a", "filter, then order, then limit"),
        (Some("n > 1"), Some("-n"), Some(2), "e,a", ""),
        (Some("n > 1"), Some("-n"), Some(3), "e,a,c", ""),
        (None, Some("-n"), Some(2), "e,a", "the top rows of the order"),
        (None, Some("n"), Some(3), "b,c,d", "ties at the edge: the earlier of the tie stays"),
        (None, Some("n"), Some(2), "b,c", ""),
        (None, Some("cost"), Some(4), "b,e,a,c", "the row without a cost is cut"),
        (None, Some("-cost"), Some(5), "c,a,b,e,d", ""),
        (None, Some("-cost"), Some(4), "c,a,b,e", ""),
        (Some("nope"), None, Some(3), "", "nothing passes the filter"),
        (Some("state == 'done'"), Some("-n"), Some(1), "a", ""),
        (Some("state == 'done'"), Some("n"), Some(1), "c", ""),
        (Some("false"), Some("n"), None, "", ""),
        (Some("true"), Some("n"), Some(2), "b,c", ""),
    ];
    for &(filter, order, limit, want, note) in rows {
        t.eq(format!("filter {filter:?} order {order:?} limit {limit:?} over a..e"), note, want.to_string(), || {
            run("tasks", filter, order, limit, base(), &env)
        });
    }
    // empty input
    let empties: &[(Option<&str>, Option<&str>, Option<u64>)] = &[
        (None, None, None),
        (Some("n > 1"), None, None),
        (None, Some("-n"), None),
        (None, None, Some(3)),
        (Some("state == 'done'"), Some("n, -id"), Some(5)),
        (Some("nope"), Some("nope"), Some(1)),
    ];
    for &(filter, order, limit) in empties {
        t.eq(format!("no rows: filter {filter:?} order {order:?} limit {limit:?}"), "", String::new(), || {
            run("tasks", filter, order, limit, vec![], &env)
        });
    }
    t.eq("no rows: the answer is an empty list".into(), "", Vec::<Value>::new(), || {
        Query::parse("tasks", Some("x"), Some("y"), Some(3))
            .map(|q| q.apply(vec![], &env))
            .unwrap_or_else(|_| vec![Value::Missing])
    });
    // the rows come back as they went in
    t.holds("a query with no filter, order or limit returns the very same rows".into(), "", || {
        let q = Query::parse("tasks", None, None, None).map_err(|e| format!("{e:?}"))?;
        let out = q.apply(base(), &env);
        if out == base() { Ok(()) } else { Err("different rows".into()) }
    });
    t.holds("rows come back whole: no field is dropped or changed by filter, order or limit".into(), "", || {
        let q = Query::parse("tasks", Some("n > 0"), Some("-n"), Some(10)).map_err(|e| format!("{e:?}"))?;
        let out = q.apply(base(), &env);
        let all = base();
        if out.len() == 5 && out.iter().all(|r| all.contains(r)) { Ok(()) } else { Err(format!("{out:?}")) }
    });
    t.holds("identical rows are both kept".into(), "", || {
        let r = row("x", &[("n", n(1.0))]);
        let q = Query::parse("tasks", Some("n == 1"), Some("n"), None).map_err(|e| format!("{e:?}"))?;
        let out = q.apply(vec![r.clone(), r.clone(), r], &env);
        if out.len() == 3 { Ok(()) } else { Err(format!("{} rows", out.len())) }
    });
    t.holds("the same Query applied twice gives the same answer".into(), "", || {
        let q = Query::parse("tasks", Some("n > 1"), Some("-n, id"), Some(3)).map_err(|e| format!("{e:?}"))?;
        let (x, y) = (q.apply(base(), &env), q.apply(base(), &env));
        if x == y { Ok(()) } else { Err("different answers".into()) }
    });
    t.holds("the collection name does not change what apply does (the resolver hands the rows in)".into(), "", || {
        let mut outs = Vec::new();
        for from in ["tasks", "asks", "changes", "log:x.*", "table:t", "fold:f", "thread:th"] {
            let q = Query::parse(from, Some("n > 1"), Some("-n"), Some(2)).map_err(|e| format!("{from}: {e:?}"))?;
            outs.push(ids(&q.apply(base(), &env)));
        }
        if outs.iter().all(|o| o == "e,a") { Ok(()) } else { Err(format!("{outs:?}")) }
    });
    // built by hand
    t.eq(
        "a Query built by hand with limit Some(0) returns no rows".into(),
        "DECIDED: parse refuses 0; reading: applied by hand it is at most 0 rows",
        String::new(),
        || {
            let q = Query { from: Coll::Tasks, filter: None, order: vec![], limit: Some(0) };
            ids(&q.apply(base(), &env))
        },
    );
    t.eq(
        "a Query built by hand with more than MAX_ORDER keys still sorts".into(),
        "DECIDED: parse refuses it; reading: applied by hand every key counts",
        "b,c,d,a,e".to_string(),
        || {
            let key = |s: &str| Order { key: ex(s), desc: false };
            let q = Query {
                from: Coll::Tasks,
                filter: None,
                order: vec![key("n"), key("nope"), key("nope"), key("nope"), key("nope"), key("nope")],
                limit: None,
            };
            ids(&q.apply(base(), &env))
        },
    );
    t.eq("a Query built by hand: one key descending".into(), "", "e,a,c,d,b".to_string(), || {
        let q = Query {
            from: Coll::Tasks,
            filter: Some(ex("true")),
            order: vec![Order { key: ex("n"), desc: true }],
            limit: None,
        };
        ids(&q.apply(base(), &env))
    });
    t.finish("apply_limit_and_the_empty_cases");
}

#[test]
fn query_check_reports_what_expr_check_finds() {
    let mut t = Tally::new();
    // (filter, order, fewest, most, note)
    #[allow(clippy::type_complexity)]
    let rows: Vec<(Option<&str>, Option<&str>, usize, usize, &str)> = vec![
        (None, None, 0, 0, "nothing to check"),
        (Some("state == 'done'"), None, 0, 0, "a name may be a row's field: no roots are checked"),
        (Some("zzz.yyy == 1"), None, 0, 0, "no roots are checked"),
        (Some("fold.spend > 1 and set.loop == 'x'"), Some("n, -zzz"), 0, 0, "no roots are checked"),
        (Some("/a/b == 1"), Some("-/c"), 0, 0, "pointers"),
        (Some("len('abc') > 1"), Some("lower(name)"), 0, 0, "fitting calls"),
        (Some("len(1)"), None, 1, 3, "a number where len takes text, a list or a map"),
        (Some("lower(2)"), None, 1, 3, "a number where lower takes text"),
        (Some("5 matches 'x'"), None, 1, 3, "matches takes text"),
        (Some("'a' matches 5"), None, 1, 3, "matches takes text"),
        (None, Some("lower(2)"), 1, 3, "the keys are checked too"),
        (None, Some("n, len(1)"), 1, 3, "the second key is checked too"),
        (None, Some("-len(1)"), 1, 3, "a descending key is checked too"),
        (Some("len(1)"), Some("lower(2)"), 2, 6, "filter and key: both reported"),
        (Some("len(1) and lower(2)"), None, 2, 6, "two problems in one filter"),
    ];
    for (filter, order, lo, hi, note) in rows {
        t.holds(format!("check() of filter {filter:?} order {order:?} gives {lo}..={hi}"), note, || {
            let q =
                Query::parse("tasks", filter, order, None).map_err(|e| format!("Query::parse refused it: {e:?}"))?;
            let msgs = q.check(None);
            if msgs.len() < lo || msgs.len() > hi {
                return Err(format!("{} messages: {msgs:?}", msgs.len()));
            }
            if msgs.iter().any(|m| m.trim().is_empty()) {
                return Err(format!("an empty message in {msgs:?}"));
            }
            Ok(())
        });
    }
    t.holds("check() agrees with Expr::check(None) on the filter".into(), "", || {
        for src in ["len(1)", "lower(2)", "5 matches 'x'", "a == 1", "zzz.yyy"] {
            let q = Query::parse("tasks", Some(src), None, None).map_err(|e| format!("{src}: {e:?}"))?;
            let want: Vec<String> = ex(src).check(None).into_iter().map(|m| format!("filter: {m}")).collect();
            if q.check(None) != want {
                return Err(format!("{src:?}: Query {:?} vs Expr {want:?}", q.check(None)));
            }
        }
        Ok(())
    });
    t.finish("query_check_reports_what_expr_check_finds");
}
