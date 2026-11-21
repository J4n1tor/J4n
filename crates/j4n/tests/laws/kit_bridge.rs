//! The weight law of the kit bridge: the bridge is one host arm and one block of the library,
//! and neither names a tool. A tool added to the offer is callable from code with no line here, so a line that names one is a wrapper
//! (weight the bridge must not carry). The one exception is the alias `fetch`, which the library keeps as `call("web", url=url)`.

use crate::support::{read, root};
use j4n_connect::model::doer::Tools;
use j4n_connect::tools::Builtin;

const BEGIN: &str = "# ---- the bridge";
const END: &str = "# ---- end of the bridge";

/// Every name a tool of J4n has: the builtin set, and the ones that are made elsewhere (`py`, the memory's, the web's, the loop's `ask`, `mcp`).
fn tool_names() -> Vec<String> {
    let mut names: Vec<String> = Builtin::new().specs().into_iter().map(|t| t.name).collect();
    names.extend(["py", "ask", "web", "memory", "recall", "mcp"].map(String::from));
    names
}

/// The string literals of `src` outside comment lines: each whole, between its double quotes.
fn literals(src: &str) -> Vec<String> {
    let code: String = src
        .lines()
        .filter(|l| !l.trim_start().starts_with("//") && !l.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    let (mut out, mut cur, mut inside, mut escaped) = (Vec::new(), String::new(), false, false);
    for c in code.chars() {
        match (inside, escaped, c) {
            (true, true, _) => {
                cur.push(c);
                escaped = false;
            }
            (true, false, '\\') => {
                cur.push(c);
                escaped = true;
            }
            (true, false, '"') => {
                out.push(std::mem::take(&mut cur));
                inside = false;
            }
            (true, false, _) => cur.push(c),
            (false, _, '"') => inside = true,
            _ => {}
        }
    }
    out
}

/// Where `text` names a tool: a literal that is its name, or says it in backticks or quotes.
fn names_in(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for lit in literals(text) {
        for n in tool_names() {
            if lit == n || ["`", "'"].iter().any(|q| lit.contains(&format!("{q}{n}{q}"))) {
                found.push(format!("{n} in {lit:?}"));
            }
        }
    }
    found
}

/// The lines of the library between the bridge's markers, and a docstring's backticked words with them.
fn block(py: &str) -> String {
    let (a, b) = (py.find(BEGIN).expect("the bridge's first marker"), py.find(END).expect("the bridge's last marker"));
    assert!(a < b);
    py[a..b].to_string()
}

fn backticked(text: &str) -> Vec<String> {
    text.split('`').skip(1).step_by(2).map(String::from).collect()
}

#[test]
fn kithost_and_the_libraries_bridge_block_name_no_tool_and_fetch_is_the_one_alias() {
    let host = read(&root().join("crates/j4n-connect/src/kithost.rs"));
    let said = names_in(&host);
    let ticks: Vec<String> =
        backticked(&host.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n"))
            .into_iter()
            .filter(|w| tool_names().contains(w))
            .collect();
    assert!(
        said.is_empty() && ticks.is_empty(),
        "law (kit bridge weight): kithost.rs names a tool: {said:?} {ticks:?}"
    );
    let py = read(&root().join("packs/kit/py/j4n.py"));
    let inside = block(&py);
    let said = names_in(&inside.replace('#', "//"));
    let ticks: Vec<String> = backticked(&inside).into_iter().filter(|w| tool_names().contains(w)).collect();
    assert!(
        said.is_empty() && ticks.is_empty(),
        "law (kit bridge weight): the library's bridge block names a tool: {said:?} {ticks:?}"
    );
    // outside the block the library names exactly one: the alias
    let outside = format!("{}{}", &py[..py.find(BEGIN).unwrap()], &py[py.find(END).unwrap()..]);
    let alias: Vec<&str> = outside.lines().filter(|l| l.contains("\"web\"")).collect();
    assert_eq!(
        alias,
        ["    return call(\"web\", url=url)"],
        "fetch is `call(\"web\", url=url)` and the only line that names a tool"
    );
    assert!(
        !outside.contains("host(\"fetch\"") && !outside.contains("host('fetch'"),
        "fetch has no host call of its own"
    );
}

#[test]
fn the_detector_catches_a_wrapper_for_one_tool() {
    assert_eq!(names_in("let r = host(\"read\", path);"), ["read in \"read\""]);
    assert_eq!(
        names_in("Err(\"ask for it with the `web` tool first\".into())"),
        ["web in \"ask for it with the `web` tool first\""]
    );
    assert!(
        names_in("// \"read\" in a comment\nlet n = \"tool\";\nlet m = \"read the file\";").is_empty(),
        "comments and the words of a sentence are not names"
    );
    assert_eq!(backticked("a `py` and `b`"), ["py", "b"]);
    assert!(tool_names().contains(&"edit".to_string()) && tool_names().contains(&"py".to_string()));
}
