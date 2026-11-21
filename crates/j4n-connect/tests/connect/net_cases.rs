//! Law 13, the behaviour half: the one builder reads the proxy from the environment, lets an account's proxy replace the
//! system's, and sends loopback and `no_proxy` hosts direct (the builder's part).

use crate::mock::{Mock, Proxy as Fixture, Say};
use j4n_connect::net::{Net, NetError, Proxy};
use serde_json::json;

fn env(pairs: &[(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> + Send + Sync + 'static + use<> {
    let pairs: Vec<_> = pairs.to_vec();
    move |k| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| (*v).to_string())
}

#[test]
fn the_system_proxy_is_read_by_scheme_and_by_either_spelling() {
    let sys = |pairs: &[(&'static str, &'static str)]| Net::new().with_env(env(pairs));
    assert_eq!(sys(&[("https_proxy", "http://a:1")]).route("https://api.example.com/v1").as_deref(), Some("http://a:1"));
    assert_eq!(sys(&[("HTTPS_PROXY", "http://b:2")]).route("https://api.example.com/v1").as_deref(), Some("http://b:2"));
    assert_eq!(sys(&[("HTTPS_PROXY", "http://upper:1"), ("https_proxy", "http://lower:2")]).route("https://x.example/").as_deref(), Some("http://upper:1"));
    assert_eq!(sys(&[("ALL_PROXY", "socks5://c:3")]).route("https://api.example.com").as_deref(), Some("socks5://c:3"));
    assert_eq!(sys(&[("all_proxy", "socks5://d:4")]).route("http://api.example.com").as_deref(), Some("socks5://d:4"));
    // an https call does not use the plain-http variable, nor the other way round
    assert_eq!(sys(&[("http_proxy", "http://e:5")]).route("https://api.example.com"), None);
    assert_eq!(sys(&[("https_proxy", "http://f:6")]).route("http://api.example.com"), None);
    assert_eq!(sys(&[("http_proxy", "http://e:5")]).route("http://api.example.com").as_deref(), Some("http://e:5"));
    // an empty variable is not a proxy; none set is none
    assert_eq!(sys(&[("https_proxy", "  ")]).route("https://api.example.com"), None);
    assert_eq!(sys(&[]).route("https://api.example.com"), None);
}

#[test]
fn loopback_and_no_proxy_hosts_go_direct() {
    let net = Net::new().with_env(env(&[("https_proxy", "http://p:1"), ("http_proxy", "http://p:1"), ("no_proxy", "internal.test, .corp.example ,10.0.0.5")]));
    for url in ["http://127.0.0.1:8080/x", "http://localhost/x", "http://[::1]:9000/x", "http://me:secret@localhost:11434/v1"] {
        assert_eq!(net.route(url), None, "{url}");
    }
    for url in ["https://internal.test/v1", "https://a.corp.example/v1", "https://corp.example/v1", "http://10.0.0.5/x"] {
        assert_eq!(net.route(url), None, "{url}");
    }
    // a suffix needs its dot, and a host that merely ends the same way is not named
    assert_eq!(net.route("https://notcorp.example/v1").as_deref(), Some("http://p:1"));
    assert_eq!(net.route("https://internal.test.evil.example/v1").as_deref(), Some("http://p:1"));
    let all = Net::new().with_env(env(&[("https_proxy", "http://p:1"), ("NO_PROXY", "*")]));
    assert_eq!(all.route("https://anything.example"), None);
}

#[test]
fn an_accounts_proxy_replaces_the_system_one_and_still_spares_loopback() {
    let net = Net::new().with_env(env(&[("https_proxy", "http://system:1"), ("no_proxy", "spared.test")])).proxied(Proxy::Url("socks5://account:9".into()));
    assert_eq!(net.route("https://api.example.com/v1").as_deref(), Some("socks5://account:9"));
    assert_eq!(net.route("http://api.example.com/v1").as_deref(), Some("socks5://account:9"), "the account's has no scheme");
    assert_eq!(net.route("http://127.0.0.1:1/v1"), None);
    assert_eq!(net.route("https://spared.test/v1"), None);
    // `system` is the system's, and a missing system proxy is none
    let sys = Net::new().with_env(env(&[])).proxied(Proxy::System);
    assert_eq!(sys.route("https://api.example.com"), None);
}

#[test]
fn a_proxy_word_is_system_or_a_url_with_a_scheme_and_a_host() {
    assert_eq!(Proxy::parse("system"), Ok(Proxy::System));
    assert_eq!(Proxy::parse(" system "), Ok(Proxy::System));
    assert_eq!(Proxy::parse(""), Ok(Proxy::System));
    assert_eq!(Proxy::parse("http://127.0.0.1:2080"), Ok(Proxy::Url("http://127.0.0.1:2080".into())));
    assert_eq!(Proxy::parse("socks5://127.0.0.1:1080"), Ok(Proxy::Url("socks5://127.0.0.1:1080".into())));
    assert_eq!(Proxy::parse("socks5h://u:p@h:1"), Ok(Proxy::Url("socks5h://u:p@h:1".into())));
    for bad in ["127.0.0.1:2080", "ftp://h:1", "http://", "none"] {
        assert!(Proxy::parse(bad).is_err(), "{bad}");
    }
}

#[test]
fn a_call_leaves_by_the_accounts_proxy_and_the_system_one_sees_none() {
    let gateway = Mock::start(|_, _| Say::Json(200, json!({"ok": true})));
    let account = Fixture::start(gateway.addr);
    let system = Fixture::start(gateway.addr);
    // the host is not loopback, so the proxy has to carry the call: it reaches the mock whatever it is asked for
    let url = format!("http://gateway.test:{}/v1/ping", gateway.addr.port());
    let sys_url = system.url();
    let net = Net::new().with_env(move |k| (k == "http_proxy").then(|| sys_url.clone())).proxied(Proxy::Url(account.url()));
    let reply = net.get(&url, &[]).unwrap();
    assert_eq!(reply.status, 200);
    assert_eq!(account.calls(), [format!("http://gateway.test:{}/v1/ping", gateway.addr.port())]);
    assert!(system.calls().is_empty(), "the system proxy was not used: {:?}", system.calls());
    // without an account's proxy the system one carries it
    let sys_url = system.url();
    let plain = Net::new().with_env(move |k| (k == "http_proxy").then(|| sys_url.clone()));
    assert_eq!(plain.get(&url, &[]).unwrap().status, 200);
    assert_eq!(system.calls().len(), 1);
    assert_eq!(account.calls().len(), 1);
}

#[test]
fn loopback_is_not_sent_through_a_proxy() {
    let gateway = Mock::start(|_, _| Say::Json(200, json!({})));
    let proxy = Fixture::start(gateway.addr);
    let net = Net::new().with_env(env(&[])).proxied(Proxy::Url(proxy.url()));
    assert_eq!(net.get(&format!("{}/x", gateway.url()), &[]).unwrap().status, 200);
    assert!(proxy.calls().is_empty());
    assert_eq!(gateway.seen().len(), 1);
}

#[test]
fn a_status_of_400_or_more_is_an_answer_with_its_body() {
    let gateway = Mock::start(|_, n| if n == 0 { Say::Json(429, json!({"error": {"message": "slow down"}})) } else { Say::Text(503, "down".into()) });
    let net = Net::new().with_env(env(&[]));
    let r = net.post_json(&format!("{}/v1/chat", gateway.url()), &[("Authorization", "Bearer k")], &json!({"a": 1})).unwrap();
    assert_eq!((r.status, r.ok()), (429, false));
    assert!(r.text().unwrap().contains("slow down"));
    let r = net.get(&format!("{}/again", gateway.url()), &[]).unwrap();
    assert_eq!((r.status, r.text().unwrap().as_str()), (503, "down"));
    let seen = gateway.seen();
    assert_eq!(seen[0].method, "POST");
    assert_eq!(seen[0].headers.get("authorization").map(String::as_str), Some("Bearer k"));
    assert_eq!(seen[0].headers.get("content-type").map(String::as_str), Some("application/json"));
    assert_eq!(seen[0].json(), json!({"a": 1}));
    assert_eq!(seen[1].method, "GET");
}

#[test]
fn a_stream_is_read_line_by_line_as_it_comes() {
    let gateway = Mock::start(|_, _| Say::Sse(vec!["one".into(), "two".into(), "[DONE]".into()]));
    let r = Net::new().with_env(env(&[])).get(&gateway.url(), &[]).unwrap();
    assert_eq!(r.headers.get("content-type").map(String::as_str), Some("text/event-stream"));
    let lines: Vec<String> = r.lines().map(Result::unwrap).filter(|l| !l.is_empty()).collect();
    assert_eq!(lines, ["data: one", "data: two", "data: [DONE]"]);
}

#[test]
fn no_answer_at_all_is_an_error_that_does_not_carry_a_password() {
    let dead = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = dead.local_addr().unwrap().port();
    drop(dead);
    let e: NetError = Net::new().with_env(env(&[])).get(&format!("http://127.0.0.1:{port}/"), &[]).err().unwrap();
    assert!(!e.0.is_empty());
    let mock = Mock::start(|_, _| Say::Drop);
    assert!(Net::new().with_env(env(&[])).get(&mock.url(), &[]).is_err());
    // a proxy that cannot be used is named without its password
    let bad = Net::new().with_env(env(&[])).proxied(Proxy::Url("bogus://user:hunter2@proxy.test:1".into()));
    let e = bad.get("http://api.example.com/", &[]).err().unwrap();
    assert!(!e.0.contains("hunter2"), "{e}");
}

#[test]
fn a_body_goes_through_a_proxy_too() {
    let gateway = Mock::start(|s, _| Say::Json(200, json!({ "got": s.json() })));
    let proxy = Fixture::start(gateway.addr);
    let url = format!("http://gateway.test:{}/v1/echo", gateway.addr.port());
    let net = Net::new().with_env(env(&[])).proxied(Proxy::Url(proxy.url()));
    let r = net.post_json(&url, &[("Authorization", "Bearer k")], &json!({ "a": [1, 2, 3] })).unwrap();
    assert_eq!(r.status, 200);
    assert_eq!(r.text().unwrap(), json!({ "got": { "a": [1, 2, 3] } }).to_string());
    assert_eq!(proxy.calls(), [url]);
}

#[test]
fn a_read_that_hears_nothing_for_the_idle_time_fails_and_one_that_is_answered_in_time_does_not() {
    use std::time::Duration;
    let slow = Mock::start(|_, _| Say::After(Duration::from_millis(4000), Box::new(Say::Json(200, json!({ "late": true })))));
    let started = std::time::Instant::now();
    let e = Net::new().with_env(env(&[])).idle(Duration::from_millis(200)).get(&slow.url(), &[]);
    let e = match e {
        Err(e) => e,
        Ok(r) => r.text().expect_err("the body did not come in time"),
    };
    assert!(!e.0.is_empty());
    assert!(started.elapsed() < Duration::from_millis(3000), "it gave up at the idle time, not when the answer came: {:?}", started.elapsed());
    let quick = Mock::start(|_, _| Say::After(Duration::from_millis(100), Box::new(Say::Json(200, json!({ "ok": true })))));
    assert_eq!(Net::new().with_env(env(&[])).idle(Duration::from_millis(600)).get(&quick.url(), &[]).unwrap().status, 200);
}

#[test]
fn only_a_2xx_status_is_ok() {
    for (code, ok) in [(200, true), (204, true), (304, false), (404, false), (500, false)] {
        let mock = Mock::start(move |_, _| Say::Text(code, String::new()));
        assert_eq!(Net::new().with_env(env(&[])).get(&mock.url(), &[]).unwrap().ok(), ok, "{code}");
    }
}

// ---- what a gateway key has left ----

#[test]
fn what_a_key_has_left_is_read_from_the_key_endpoint_and_the_key_is_never_in_what_is_said() {
    use j4n_connect::credits::{ask, parse};
    let key = "sk-or-v1-not-for-printing-0123456789";
    let body = serde_json::json!({ "data": { "label": "j4n", "usage": 4.25, "limit": 10.0, "limit_remaining": 5.75, "is_free_tier": false } });
    let g = Mock::start(move |_, _| Say::Json(200, body.clone()));
    let left = ask(&j4n_connect::net::Net::new().with_env(|_| None), &g.url(), key).expect("a reading");
    assert_eq!((left.remaining, left.used, left.limit), (Some(5.75), 4.25, Some(10.0)));
    let seen = g.seen();
    assert_eq!((seen[0].method.as_str(), seen[0].path.as_str()), ("GET", "/key"), "the one call");
    assert_eq!(seen[0].headers.get("authorization").map(String::as_str), Some(&*format!("Bearer {key}")), "the key goes in a header and nowhere else");
    // a key with no limit says what it used, and nothing left
    let open = parse(r#"{"data":{"usage":1.5,"limit":null,"limit_remaining":null}}"#).unwrap();
    assert_eq!((open.remaining, open.used, open.limit), (None, 1.5, None));
    assert!(parse("not json").is_err() && parse(r#"{"data":{}}"#).is_err() && parse(r#"{"error":"x"}"#).is_err());
    // a refusal and a dead gateway say so without the key
    let refusing = Mock::start(|_, _| Say::Json(401, serde_json::json!({ "error": { "message": "bad key" } })));
    let err = ask(&j4n_connect::net::Net::new().with_env(|_| None), &refusing.url(), key).unwrap_err();
    assert!(err.contains("401") && err.contains("key is not accepted") && !err.contains(key), "{err}");
    let dead = ask(&j4n_connect::net::Net::new().with_env(|_| None), "http://127.0.0.1:9", key).unwrap_err();
    assert!(!dead.contains(key), "{dead}");
}
