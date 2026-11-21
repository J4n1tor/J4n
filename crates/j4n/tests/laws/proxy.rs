//! Law 13, proxy always: every network client is built by the one builder that honours `https_proxy`, which lives in
//! `j4n-connect` (`crates/j4n-connect/src/net.rs` or `net/`). Which client crate sits behind it is open; this law fixes only where a client may be built. Outside the builder no source names an HTTP client or a
//! raw socket type, and only `j4n-connect` has an HTTP client crate as a dependency.
//!
//! The behaviour test, that the builder really reads `https_proxy`, `HTTPS_PROXY` and `no_proxy`, cannot exist before
//! the builder. It is written with the builder; until then this law is the source half only.

use crate::support::{
    assert_clean, code_only, crate_manifests, dependency_names, positions, read, rel, tokens, workspace_manifest,
    workspace_sources,
};

/// The crate that owns the builder: the only one that may depend on an HTTP client crate.
const CONNECT: &str = "j4n-connect";

/// HTTP client crates, as dependencies of a crate.
const CLIENT_CRATES: [&str; 6] = ["ureq", "reqwest", "hyper", "attohttpc", "isahc", "curl"];

/// What code outside the builder never names: the client crates (`curl` only as a path, the word is common) and the
/// socket types a client is made of.
const CLIENTS_IN_CODE: [&str; 8] =
    ["ureq", "reqwest", "hyper", "attohttpc", "isahc", "curl::", "TcpStream", "UdpSocket"];

/// Whether the repo-relative `path` is the builder, the only place a client may be built.
fn is_builder(path: &str) -> bool {
    path == "crates/j4n-connect/src/net.rs" || path.starts_with("crates/j4n-connect/src/net/")
}

/// Where code names something in `CLIENTS_IN_CODE`, as (line, what), sorted.
fn client_hits(src: &str) -> Vec<(usize, &'static str)> {
    let code = code_only(src);
    let stream = tokens(&code);
    let mut hits = Vec::new();
    for name in CLIENTS_IN_CODE {
        hits.extend(positions(&stream, name).into_iter().map(|i| (stream[i].0, name)));
    }
    hits.sort_unstable();
    hits
}

/// Dependencies on an HTTP client crate in a manifest of a crate other than `j4n-connect`.
fn client_dependency_violations(crate_name: &str, manifest: &str, workspace: &str) -> Vec<String> {
    if crate_name == CONNECT {
        return Vec::new();
    }
    dependency_names(manifest, workspace)
        .into_iter()
        .filter(|name| CLIENT_CRATES.contains(&name.as_str()))
        .map(|name| format!("{crate_name} depends on `{name}`; only {CONNECT} may, behind the one proxy-aware builder"))
        .collect()
}

#[test]
fn no_http_client_or_socket_is_named_outside_the_builder() {
    let mut violations = Vec::new();
    for path in workspace_sources() {
        let name = rel(&path);
        if is_builder(&name) {
            continue;
        }
        for (line, what) in client_hits(&read(&path)) {
            violations.push(format!(
                "{name}:{line}: names `{what}` — a client is built only by the builder in crates/j4n-connect/src/net.rs"
            ));
        }
    }
    assert_clean("law 13 (proxy always)", &violations);
}

#[test]
fn only_the_connect_crate_depends_on_an_http_client_crate() {
    let workspace = workspace_manifest();
    let violations: Vec<String> = crate_manifests()
        .iter()
        .flat_map(|(name, manifest)| client_dependency_violations(name, manifest, &workspace))
        .collect();
    assert_clean("law 13 (proxy always)", &violations);
}

#[test]
fn the_builder_is_allowed_by_path_and_nothing_near_it() {
    assert!(is_builder("crates/j4n-connect/src/net.rs"));
    assert!(is_builder("crates/j4n-connect/src/net/agent.rs"));
    assert!(!is_builder("crates/j4n-connect/src/network.rs"));
    assert!(!is_builder("crates/j4n-connect/src/net_tests.rs"));
    assert!(!is_builder("crates/j4n-connect/src/dialect/net.rs"));
    assert!(!is_builder("crates/j4n-core/src/net.rs"));
    assert!(!is_builder("crates/j4n-connect/src/lib.rs"));
}

#[test]
fn every_http_client_and_socket_is_caught_with_its_line() {
    let src = "use ureq::Agent;\nlet c = reqwest::blocking::Client::new();\nlet h = hyper::Client::new();\nlet a = attohttpc::get(u);\nlet i = isahc::get(u);\nlet e = curl::easy::Easy::new();\nlet t = std::net::TcpStream::connect(a);\nlet u = UdpSocket::bind(a);\n";
    assert_eq!(
        client_hits(src),
        [
            (1, "ureq"),
            (2, "reqwest"),
            (3, "hyper"),
            (4, "attohttpc"),
            (5, "isahc"),
            (6, "curl::"),
            (7, "TcpStream"),
            (8, "UdpSocket"),
        ]
    );
    assert_eq!(client_hits("let a = ureq\n    ::agent();\n"), [(1, "ureq")]);
}

#[test]
fn similar_names_are_not_clients() {
    let src = "let ureqs = 1;\nlet my_hyper = 2;\nstruct Hypertext;\nlet curl = which();\nlet x = curly::y();\nstruct TcpStreamLike;\nlet z = UdpSockets;\nlet w = reqwest_like::z();\n";
    assert!(client_hits(src).is_empty());
}

#[test]
fn comments_and_string_bodies_do_not_count() {
    let src = r##"
// build it with ureq::agent()
/* reqwest::Client */
/// hyper, attohttpc and isahc
const NOTE: &str = "TcpStream UdpSocket curl::easy";
const RAW: &str = r#"ureq"#;
fn f() {}
"##;
    assert!(client_hits(src).is_empty());
}

#[test]
fn an_http_client_dependency_is_caught_outside_the_connect_crate_in_every_table() {
    let manifest = r#"
[dependencies]
j4n-proto.workspace = true
ureq = "2"

[dev-dependencies]
reqwest = { version = "0.12", features = ["blocking"] }

[build-dependencies.hyper]
version = "1"

[target.'cfg(unix)'.dependencies]
curl = "0.4"

[target.'cfg(windows)'.dev-dependencies]
attohttpc = "0.28"
isahc = "1"
"#;
    for name in ["j4n-core", "j4n-reach", "j4n-tui", "j4n-proto", "j4n"] {
        assert_eq!(client_dependency_violations(name, manifest, "").len(), 6, "{name}");
    }
    assert!(client_dependency_violations(CONNECT, manifest, "").is_empty());
    let first = &client_dependency_violations("j4n-core", manifest, "")[0];
    assert!(first.contains("j4n-core depends on `ureq`") && first.contains("j4n-connect"));
}

#[test]
fn a_renamed_http_client_dependency_is_judged_by_its_package() {
    let manifest = "[dependencies]\nhttp = { package = \"ureq\", version = \"2\" }\nagent = { package = \"j4n-core\", path = \"../j4n-core\" }\n";
    assert_eq!(client_dependency_violations("j4n-reach", manifest, "").len(), 1);
    let workspace = "[workspace.dependencies]\nnet = { package = \"reqwest\", version = \"0.12\" }\n";
    assert_eq!(client_dependency_violations("j4n-reach", "[dependencies]\nnet.workspace = true\n", workspace).len(), 1);
}

#[test]
fn a_manifest_without_an_http_client_passes() {
    let manifest =
        "[dependencies]\nj4n-core.workspace = true\nserde = \"1\"\nhyperlocal-notes = \"1\"\nureq-like = \"1\"\n";
    assert!(client_dependency_violations("j4n-reach", manifest, "").is_empty());
    assert!(client_dependency_violations("j4n-reach", "[package]\nname = \"j4n-reach\"\n", "").is_empty());
}
