//! Redaction: registered secrets in every form they take, the shapes of keys (the common key patterns
//! tests), and the ways a hostile line could stall or break it.

use j4n_core::redact::{MIN_SECRET, Redactor, resolve, resolve_in};
use serde_json::json;
use std::time::Duration;

fn plain() -> Redactor {
    Redactor::new()
}

/// The redactor with the pack `guard` off: the shape rules, as they were.
fn legacy() -> Redactor {
    let r = Redactor::new();
    r.set_guard(false);
    r
}

#[test]
fn a_registered_secret_is_replaced_wherever_it_appears() {
    let r = plain();
    assert!(r.register("ANTHROPIC_KEY", "hunter2-hunter2"));
    assert_eq!(r.scrub("use hunter2-hunter2 twice: hunter2-hunter2."), "use «secret:ANTHROPIC_KEY» twice: «secret:ANTHROPIC_KEY».");
    assert_eq!(r.scrub("nothing here"), "nothing here");
}

#[test]
fn a_secret_is_found_as_json_writes_it_and_as_a_url_writes_it() {
    let r = plain();
    assert!(r.register("TOKEN", "pa\"ss\\w/rd é 1"));
    let json_form = serde_json::to_string("pa\"ss\\w/rd é 1").unwrap();
    assert!(r.scrub(&json_form).contains("«secret:TOKEN»") && !r.scrub(&json_form).contains("ss\\\\w"));
    assert_eq!(r.scrub("?k=pa%22ss%5Cw%2Frd%20%C3%A9%201&x=1"), "?k=«secret:TOKEN»&x=1");
    assert_eq!(r.scrub("?k=pa%22ss%5cw%2frd%20%c3%a9%201"), "?k=«secret:TOKEN»");
    assert_eq!(r.scrub("raw: pa\"ss\\w/rd é 1 end"), "raw: «secret:TOKEN» end");
}

#[test]
fn a_secret_too_short_to_tell_from_text_is_refused() {
    let r = plain();
    assert!(!r.register("PIN", &"x".repeat(MIN_SECRET - 1)));
    assert!(r.register("PIN", &"x".repeat(MIN_SECRET)));
    assert!(!r.register("", "longenoughvalue"));
    assert!(!r.register("bad name", "longenoughvalue"));
    assert!(!r.register(&"n".repeat(65), "longenoughvalue"));
    assert_eq!(r.scrub("xxxxx"), "xxxxx", "the short one was never registered");
}

#[test]
fn the_longer_secret_wins_when_one_holds_the_other() {
    let r = plain();
    assert!(r.register("SHORT", "abcdef"));
    assert!(r.register("LONG", "abcdef-and-more"));
    assert_eq!(r.scrub("a abcdef-and-more b abcdef c"), "a «secret:LONG» b «secret:SHORT» c");
}

#[test]
fn clones_share_one_set_of_secrets() {
    let a = plain();
    let b = a.clone();
    assert!(b.register("K", "sharedsecret"));
    assert_eq!(a.scrub("sharedsecret"), "«secret:K»");
    assert!(format!("{a:?}").contains("forms") && !format!("{a:?}").contains("sharedsecret"));
}

#[test]
fn scrubbing_json_reaches_every_string_and_every_key_and_cuts_what_is_too_deep() {
    let r = plain();
    r.register("K", "sharedsecret");
    let mut v = json!({"a": ["x sharedsecret", {"sharedsecret": 1, "n": 2}], "n": 3, "b": null});
    r.scrub_json(&mut v);
    assert_eq!(v, json!({"a": ["x «secret:K»", {"«secret:K»": 1, "n": 2}], "n": 3, "b": null}));
    let mut deep = json!("sharedsecret");
    for _ in 0..200 {
        deep = json!([deep]);
    }
    r.scrub_json(&mut deep);
    let text = deep.to_string();
    assert!(!text.contains("sharedsecret") && text.contains("nested too deep"));
}

#[test]
fn data_is_kept_down_to_the_depth_limit_and_not_below_it() {
    let r = plain();
    let nest = |levels: usize| {
        let mut v = json!("leaf");
        for _ in 0..levels {
            v = json!([v]);
        }
        v
    };
    let mut at_limit = nest(j4n_core::redact::MAX_DEPTH);
    r.scrub_json(&mut at_limit);
    assert_eq!(at_limit, nest(j4n_core::redact::MAX_DEPTH), "as deep as allowed: untouched");
    let mut over = nest(j4n_core::redact::MAX_DEPTH + 1);
    r.scrub_json(&mut over);
    assert!(over.to_string().contains("nested too deep") && !over.to_string().contains("leaf"));
}

#[test]
fn bytes_that_are_not_text_lose_the_registered_values_too() {
    let r = plain();
    r.register("K", "sharedsecret");
    let mut b = vec![0xff, 0xfe];
    b.extend(b"..sharedsecret..");
    let out = r.scrub_bytes(&b);
    assert_eq!(&out[..2], &[0xff, 0xfe]);
    assert_eq!(&out[2..], "..«secret:K»..".as_bytes());
    assert_eq!(r.scrub_bytes(b"text sharedsecret"), "text «secret:K»".as_bytes());
}

// ---- the shapes (the common cases): what runs with the pack `guard` off, byte for byte as before ----

#[test]
fn sk_keys_are_redacted() {
    let r = legacy();
    assert_eq!(r.scrub("export OPENAI_API_KEY=sk-proj-abcdefghijklmnopqrstuvwx"), "export OPENAI_API_KEY=$$REDACTED:sk$$");
    assert_eq!(r.scrub("key: sk-abc123def456ghi789"), "key: $$REDACTED:sk$$");
    assert_eq!(r.scrub("the sk-abc prefix"), "the sk-abc prefix", "too short: not a key");
    assert_eq!(r.scrub("it is sk-abc123def456ghi789."), "it is $$REDACTED:sk$$.", "a full stop after a key is not part of it");
}

#[test]
fn github_slack_gitlab_and_aws_tokens_are_redacted() {
    let r = legacy();
    assert_eq!(r.scrub("ghp_0123456789abcdefghijklmnopqrstuvwxyz12"), "$$REDACTED:github$$");
    assert_eq!(r.scrub("github_pat_AAAABBBBCCCCDDDDEEEEFFFF"), "$$REDACTED:github$$");
    assert_eq!(r.scrub("xoxb-1234567890-abcdefghij"), "$$REDACTED:slack$$");
    assert_eq!(r.scrub("glpat-abcdefghijklmnopqrstuvwxyz"), "$$REDACTED:gitlab$$");
    assert_eq!(r.scrub("AKIAIOSFODNN7EXAMPLE is a test"), "$$REDACTED:aws$$ is a test");
    assert_eq!(r.scrub("AKIA short"), "AKIA short");
}

#[test]
fn a_shape_is_a_key_from_its_length_and_not_before() {
    let r = legacy();
    let is_key = |text: String| r.scrub(&text) != text;
    for (what, make, least) in [
        ("sk-", Box::new(|n: usize| format!("sk-{}", "a".repeat(n - 3))) as Box<dyn Fn(usize) -> String>, 20),
        ("ghp_", Box::new(|n| format!("ghp_{}", "a".repeat(n - 4))), 39),
        ("github_pat_", Box::new(|n| format!("github_pat_{}", "a".repeat(n - 11))), 30),
        ("gho_", Box::new(|n| format!("gho_{}", "a".repeat(n - 4))), 39),
        ("xoxb-", Box::new(|n| format!("xoxb-{}", "a".repeat(n - 5))), 14),
        ("glpat-", Box::new(|n| format!("glpat-{}", "a".repeat(n - 6))), 26),
    ] {
        assert!(!is_key(make(least - 1)), "{what}: one short of {least} is not a key");
        assert!(is_key(make(least)), "{what}: {least} is");
        assert!(is_key(make(least + 7)), "{what}: longer is");
    }
    let aws = |n: usize| format!("AKIA{}", "A".repeat(n - 4));
    assert!(!is_key(aws(19)) && is_key(aws(20)) && !is_key(aws(21)), "an AWS key id is exactly twenty");
    assert!(is_key(format!("ASIA{}", "9".repeat(16))));
    assert!(!is_key(format!("AKIA{}", "a".repeat(16))), "lowercase is not an AWS key id");
    let jwt = |parts: usize| (0..parts).map(|_| "eyJabc").collect::<Vec<_>>().join(".");
    assert!(!is_key(jwt(2)) && is_key(jwt(3)) && !is_key(jwt(4)), "a JWT has three parts");
}

#[test]
fn an_assignment_value_is_a_credential_from_twelve_characters() {
    let r = legacy();
    for key in ["token", "secret", "password", "passwd", "api_key", "apikey", "api-key"] {
        assert_eq!(r.scrub(&format!("{key}={}", "a".repeat(11))), format!("{key}={}", "a".repeat(11)), "{key}: eleven");
        assert_eq!(r.scrub(&format!("{key}={}", "a".repeat(12))), format!("{key}=$$REDACTED:assign$$"), "{key}: twelve");
        assert_eq!(r.scrub(&format!("{}: {}", key.to_uppercase(), "A1".repeat(8))), format!("{}: $$REDACTED:assign$$", key.to_uppercase()), "{key}: any case");
    }
    assert_eq!(r.scrub(&format!("token=\"{}\"", "a".repeat(12))), "token=\"$$REDACTED:assign$$\"");
    assert_eq!(r.scrub(&format!("token = '{}'", "a".repeat(12))), "token = '$$REDACTED:assign$$'");
    assert_eq!(r.scrub(&format!("token  \t=\t {}", "a+/~=_.-b".repeat(2))), "token  \t=\t $$REDACTED:assign$$", "the value characters");
    assert_eq!(r.scrub(&format!("token={}.", "a".repeat(12))), "token=$$REDACTED:assign$$", "the dot is a value character");
    assert_eq!(r.scrub("token=aaaaaaaaaaaa bbbbbbbbbbbb"), "token=$$REDACTED:assign$$ bbbbbbbbbbbb", "up to the space");
    assert_eq!(r.scrub(&format!("tokens: {}", "a".repeat(12))), "tokens: aaaaaaaaaaaa", "`tokens` is not `token` followed by a separator");
}

#[test]
fn a_jwt_is_redacted() {
    let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0a3lzGqB8ZhrEbDhY";
    assert_eq!(legacy().scrub(&format!("Bearer {jwt}")), "Bearer $$REDACTED:jwt$$");
    assert_eq!(legacy().scrub("eyJhbGciOiJIUzI1NiJ9.only.two.parts.here"), "eyJhbGciOiJIUzI1NiJ9.only.two.parts.here");
}

#[test]
fn the_value_of_a_key_assignment_is_redacted_and_placeholders_survive() {
    let r = legacy();
    assert_eq!(r.scrub("typesafe_api_key = Abcd1234Efgh5678"), "typesafe_api_key = $$REDACTED:assign$$");
    assert_eq!(r.scrub("password: \"Abcd1234Efgh5678\""), "password: \"$$REDACTED:assign$$\"");
    assert_eq!(r.scrub(r#"{"password":"Abcd1234Efgh5678","user":"bo"}"#), r#"{"password":"$$REDACTED:assign$$","user":"bo"}"#);
    assert_eq!(r.scrub("api_key = <your-key-here>"), "api_key = <your-key-here>");
    assert_eq!(r.scrub("api_key = $$CREDENTIAL_TYPESAFE$$"), "api_key = $$CREDENTIAL_TYPESAFE$$");
    assert_eq!(r.scrub("token = your_token_goes_here_ok"), "token = your_token_goes_here_ok");
    assert_eq!(r.scrub("token = my_example_token_value"), "token = my_example_token_value");
    assert_eq!(r.scrub("token = already-REDACTED-value"), "token = already-REDACTED-value");
    assert_eq!(r.scrub("token = <Abcd1234Efgh5678>"), "token = <Abcd1234Efgh5678>", "an angle bracket is not part of a value");
    assert_eq!(r.scrub("the token count was high"), "the token count was high");
    assert_eq!(r.scrub("token=short"), "token=short", "under twelve characters");
    assert_eq!(r.scrub("a token=Abcd1234Efgh5678 and --other"), "a token=$$REDACTED:assign$$ and --other", "the value is a token, not the rest of the line");
    assert_eq!(
        r.scrub("token=Abcd1234Efgh5678 secret=Zyxw9876Vuts5432"),
        "token=$$REDACTED:assign$$ secret=$$REDACTED:assign$$",
        "more than one in a line"
    );
}

#[test]
fn the_markers_are_not_scrubbed_again() {
    let r = legacy();
    r.register("ANTHROPIC_API_KEY", "sharedsecret");
    let once = r.scrub("x sharedsecret y secret=Abcd1234Efgh5678");
    assert_eq!(once, "x «secret:ANTHROPIC_API_KEY» y secret=$$REDACTED:assign$$");
    assert_eq!(r.scrub(&once), once);
    assert_eq!(r.scrub("key = «key:TYPESAFE_LONG_NAME_HERE» stuff"), "key = «key:TYPESAFE_LONG_NAME_HERE» stuff");
}

#[test]
fn text_with_multibyte_characters_is_scrubbed_in_the_right_places() {
    let r = legacy();
    assert_eq!(r.scrub("ключ sk-abc123def456ghi789 конец"), "ключ $$REDACTED:sk$$ конец");
    // 'İ' lowercases to two characters: a copy lowered that way would no longer line up with the line.
    assert_eq!(r.scrub("İİİİ token=Abcd1234Efgh5678 İİİ"), "İİİİ token=$$REDACTED:assign$$ İİİ");
    assert_eq!(r.scrub("ÉPASSWORD: Abcd1234Efgh5678É"), "ÉPASSWORD: $$REDACTED:assign$$É");
}

#[test]
fn clean_lines_and_newlines_are_untouched() {
    let r = legacy();
    assert_eq!(r.scrub("plain output line 42\nexit code 0\r\n\n"), "plain output line 42\nexit code 0\r\n\n");
    assert_eq!(r.scrub(""), "");
    assert_eq!(r.scrub("a\nsk-abc123def456ghi789\nb"), "a\n$$REDACTED:sk$$\nb");
}

#[test]
fn scrubbing_is_fast_on_lines_made_to_be_slow() {
    let started = crate::cpu::start();
    for r in [plain(), legacy()] {
    r.register("K", "sharedsecret");
    for line in [
        "token=".repeat(150_000),
        "token token token ".repeat(60_000),
        "a".repeat(1 << 20),
        "sk-".repeat(300_000),
        "password=\"".repeat(100_000),
        "İ".repeat(500_000),
        format!("{}token=abcdefghijklmnop", "=".repeat(1 << 20)),
    ] {
        let _ = r.scrub(&line);
    }
    }
    crate::cpu::assert_within(Duration::from_secs(20), "the scrub of hostile lines", || started.elapsed());
}

#[test]
fn scrubbing_any_text_never_panics_and_is_idempotent() {
    for r in [plain(), legacy()] {
    r.register("K", "sharedsecret");
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    let pieces = [
        "token", "=", ":", " ", "\"", "'", "sk-", "ghp_", "AKIA", "eyJ", ".", "-", "_", "abcdefghijklmnop", "İ", "é", "ключ", "\n",
        "«", "»", "$$", "REDACTED", "sharedsecret", "secret", "password", "xox", "glpat-", "<", "your", "0123456789ab",
        "://", "@", "Bearer ", "--token ", "-----BEGIN PRIVATE KEY-----", "\\n", "Password=", "sig=", "DB_PASSWORD=", "\"", "export ",
    ];
    for _ in 0..3000 {
        let mut s = String::new();
        for _ in 0..(1 + seed % 14) {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            s.push_str(pieces[(seed % pieces.len() as u64) as usize]);
        }
        let once = r.scrub(&s);
        assert_eq!(r.scrub(&once), once, "not idempotent on {s:?}");
        assert!(!once.contains("sharedsecret"), "left a secret in {s:?}");
    }
    }
}

// ---- resolving ----

#[test]
fn resolving_a_secret_registers_it() {
    let r = plain();
    // HOME is set by the runner (and by any login), and is long: it stands for a secret in the environment.
    let home = std::env::var("HOME").unwrap();
    assert_eq!(resolve("env:HOME", &r).unwrap(), home);
    assert_eq!(r.scrub(&format!("x {home} y")), "x «secret:HOME» y");
    assert!(resolve("env:J4N_TEST_NOT_SET_ANYWHERE", &r).unwrap_err().contains("not set"));
    assert!(resolve("env:", &r).is_err() && resolve("plain", &r).is_err() && resolve("file:relative", &r).is_err());
}

#[test]
fn a_secret_too_short_to_hide_is_not_handed_out() {
    use std::os::unix::fs::PermissionsExt;
    let s = crate::world::Scratch::new("redact-short");
    s.put("pin", "1234\n");
    std::fs::set_permissions(s.join("pin"), std::fs::Permissions::from_mode(0o600)).unwrap();
    let r = plain();
    let err = resolve(&format!("file:{}", s.join("pin").display()), &r).unwrap_err();
    assert!(err.contains("shorter than"), "{err}");
    assert_eq!(r.forms(), 0);
    s.put("empty", "\n");
    std::fs::set_permissions(s.join("empty"), std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(resolve(&format!("file:{}", s.join("empty").display()), &r).is_err());
}

#[test]
fn a_secret_file_must_be_the_owners_alone() {
    use std::os::unix::fs::PermissionsExt;
    let s = crate::world::Scratch::new("redact-file");
    s.put("key", "file-secret-value\r\n");
    let path = s.join("key");
    let r = plain();
    for mode in [0o644, 0o640, 0o604, 0o660, 0o666, 0o610, 0o601] {
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        let err = resolve(&format!("file:{}", path.display()), &r).unwrap_err();
        assert!(err.contains("readable by others") && err.contains("chmod 600") && err.contains(&format!("{mode:o}")), "{mode:o}: {err}");
    }
    for mode in [0o600, 0o400] {
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        assert!(resolve(&format!("file:{}", path.display()), &r).is_ok(), "{mode:o}");
    }
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(resolve(&format!("file:{}", path.display()), &r).unwrap(), "file-secret-value");
    assert_eq!(r.scrub("a file-secret-value b"), "a «secret:key» b");
    assert!(resolve("file:/nonexistent/j4n/key", &r).is_err());
    // `~/` is the home it is given.
    s.put("home/.j4n/secrets/tok", "home-secret-value");
    std::fs::set_permissions(s.join("home/.j4n/secrets/tok"), std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(resolve_in("file:~/.j4n/secrets/tok", &r, Some(&s.join("home"))).unwrap(), "home-secret-value");
    assert!(resolve_in("file:~/.j4n/secrets/tok", &r, None).unwrap_err().contains("no home directory"));
}

// ---- redactor v2. Every secret below is made up. ----

/// `n` made-up letters and digits.
fn alnum(n: usize) -> String {
    "Zq8Lm2Vx9Rt4Ys6W".repeat(n / 16 + 1)[..n].to_string()
}

/// `same-as-id=xxxx` is a keyed hash of the value: the tests that compare a whole placeholder blank it.
fn blank_ids(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find("same-as-id=") {
        out.push_str(&rest[..i + 11]);
        out.push_str("ID");
        rest = &rest[i + 11 + 4..];
    }
    out + rest
}

struct Rng(u64);

impl Rng {
    fn pick(&mut self, chars: &str, n: usize) -> String {
        (0..n)
            .map(|_| {
                self.0 ^= self.0 << 13;
                self.0 ^= self.0 >> 7;
                self.0 ^= self.0 << 17;
                chars.as_bytes()[(self.0 % chars.len() as u64) as usize] as char
            })
            .collect()
    }
}

const LOWER: &str = "abcdefghijklmnopqrstuvwxyz";
const ALPHA: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const AN: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
const HEX: &str = "0123456789abcdef";
const UPPER_DIGIT: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

fn b64url(bytes: &[u8]) -> String {
    let table = AN.to_string() + "-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for k in 0..=chunk.len() {
            out.push(table.as_bytes()[((n >> (18 - 6 * k)) & 63) as usize] as char);
        }
    }
    out
}

/// A secret, where it sits, and how many of its leading characters are the vendor's public prefix.
struct Case {
    fmt: &'static str,
    ctx: &'static str,
    path: &'static str,
    secret: String,
    prefix: usize,
    text: String,
}

/// (format, the variable it usually sits in): 29 formats, in 13 contexts below.
const FORMATS: [(&str, &str); 29] = [
    ("anthropic", "ANTHROPIC_API_KEY"), ("openai_proj", "OPENAI_API_KEY"), ("openai_legacy", "OPENAI_API_KEY"), ("openrouter", "OPENROUTER_API_KEY"),
    ("stripe_live", "STRIPE_SECRET_KEY"), ("stripe_test", "STRIPE_SECRET_KEY"), ("github_pat_classic", "GITHUB_TOKEN"), ("github_pat_fine", "GH_TOKEN"),
    ("gitlab", "GITLAB_TOKEN"), ("slack_bot", "SLACK_BOT_TOKEN"), ("slack_webhook", "SLACK_WEBHOOK_URL"), ("aws_key_id", "AWS_ACCESS_KEY_ID"),
    ("aws_secret", "AWS_SECRET_ACCESS_KEY"), ("google", "GOOGLE_API_KEY"), ("hf", "HF_TOKEN"), ("npm", "NPM_TOKEN"), ("sendgrid", "SENDGRID_API_KEY"),
    ("telegram", "TELEGRAM_BOT_TOKEN"), ("groq", "GROQ_API_KEY"), ("xai", "XAI_API_KEY"), ("do", "DIGITALOCEAN_TOKEN"), ("age", "AGE_SECRET_KEY"),
    ("password_long", "DB_PASSWORD"), ("password_short", "DB_PASSWORD"), ("password_words", "ADMIN_PASSWORD"), ("generic_secret", "SESSION_SECRET"),
    ("client_secret", "OAUTH_CLIENT_SECRET"), ("jwt", "SUPABASE_SERVICE_ROLE_KEY"), ("webhook_secret", "WEBHOOK_SECRET"),
];

/// (context, a file name in it, the line or lines): `{N}` is the variable, `{n}` its lower case, `{V}` the secret.
const CONTEXTS: [(&str, &str, &str); 13] = [
    ("env", ".env", "{N}={V}\n"),
    ("env_quoted", ".env.local", "{N}=\"{V}\"\n"),
    ("export", "setup.sh", "export {N}=\"{V}\"\n"),
    ("yaml", "config.yaml", "service:\n  {n}: {V}\n"),
    ("yaml_q", "docker-compose.yml", "services:\n  app:\n    environment:\n      {N}: \"{V}\"\n"),
    ("json", "settings.json", "{\n  \"{n}\": \"{V}\"\n}\n"),
    ("toml", "config.toml", "[auth]\n{n} = \"{V}\"\n"),
    ("python", "app.py", "client = Client({n}=\"{V}\")\n"),
    ("js", "index.js", "const {n} = '{V}';\n"),
    ("rust", "main.rs", "let {n} = \"{V}\";\n"),
    ("curl_bearer", "notes.md", "curl -H \"Authorization: Bearer {V}\" https://api.example.com/v1\n"),
    ("log", "app.log", "2026-10-24T10:00:00Z ERROR auth failed key={V} user=bob\n"),
    ("cli_flag", "run.sh", "mytool deploy --token {V} --region eu\n"),
];

/// A value of a format and the length of its public prefix.
fn value_of(fmt: &str, r: &mut Rng) -> (String, usize) {
    let b64u = |r: &mut Rng, n| r.pick(&format!("{AN}_-"), n);
    let b64 = |r: &mut Rng, n| r.pick(&format!("{AN}+/"), n);
    match fmt {
        "anthropic" => (format!("sk-ant-api03-{}AA", b64u(r, 93)), 13),
        "openai_proj" => (format!("sk-proj-{}", b64u(r, 156)), 8),
        "openai_legacy" => (format!("sk-{}", r.pick(AN, 48)), 3),
        "openrouter" => (format!("sk-or-v1-{}", r.pick(HEX, 64)), 9),
        "stripe_live" => (format!("sk_live_{}", r.pick(AN, 99)), 8),
        "stripe_test" => (format!("sk_test_{}", r.pick(AN, 99)), 8),
        "github_pat_classic" => (format!("ghp_{}", r.pick(AN, 36)), 4),
        "github_pat_fine" => (format!("github_pat_{}_{}", r.pick(AN, 22), r.pick(AN, 59)), 11),
        "gitlab" => (format!("glpat-{}", b64u(r, 20)), 6),
        "slack_bot" => (format!("xoxb-{}-{}-{}", r.pick("0123456789", 12), r.pick("0123456789", 13), r.pick(AN, 24)), 5),
        "slack_webhook" => (format!("https://hooks.slack.com/services/T{}/B{}/{}", r.pick(UPPER_DIGIT, 10), r.pick(UPPER_DIGIT, 10), r.pick(AN, 24)), 33),
        "aws_key_id" => (format!("AKIA{}", r.pick(UPPER_DIGIT, 16)), 4),
        "aws_secret" => (b64(r, 40), 0),
        "google" => (format!("AIza{}", b64u(r, 35)), 4),
        "hf" => (format!("hf_{}", r.pick(ALPHA, 34)), 3),
        "npm" => (format!("npm_{}", r.pick(AN, 36)), 4),
        "sendgrid" => (format!("SG.{}.{}", b64u(r, 22), b64u(r, 43)), 3),
        "telegram" => (format!("{}:AA{}", r.pick("0123456789", 10), b64u(r, 33)), 0),
        "groq" => (format!("gsk_{}", r.pick(AN, 52)), 4),
        "xai" => (format!("xai-{}", r.pick(AN, 80)), 4),
        "do" => (format!("dop_v1_{}", r.pick(HEX, 64)), 7),
        "age" => (format!("AGE-SECRET-KEY-1{}", r.pick(UPPER_DIGIT, 58)), 16),
        "password_long" => (r.pick(&format!("{AN}!@%^*"), 20), 0),
        "password_short" => (r.pick(AN, 7), 0),
        "password_words" => (format!("correct-horse-{}", r.pick(LOWER, 6)), 0),
        "generic_secret" => (r.pick(HEX, 64), 0),
        "client_secret" => (b64u(r, 40), 0),
        "jwt" => {
            let header = b64url(br#"{"alg":"HS256","typ":"JWT"}"#);
            let claims = b64url(br#"{"iss":"supabase","role":"service_role","iat":1700000000,"exp":2000000000}"#);
            (format!("{header}.{claims}.{}", b64u(r, 43)), 0)
        }
        _ => (format!("whsec_{}", b64(r, 32)), 0),
    }
}

/// 381 cases: 29 formats × 13
/// contexts (no password in a bearer header), a PEM key, a service-account JSON, and 5 connection strings. Same shapes, a fixed seed.
fn secret_cases() -> Vec<Case> {
    let mut r = Rng(0x9E37_79B9_7F4A_7C15);
    let mut cases = Vec::new();
    for (fmt, name) in FORMATS {
        for (ctx, path, template) in CONTEXTS {
            if fmt.starts_with("password") && ctx == "curl_bearer" {
                continue;
            }
            let (secret, prefix) = value_of(fmt, &mut r);
            let text = template.replace("{N}", name).replace("{n}", &name.to_lowercase()).replace("{V}", &secret);
            cases.push(Case { fmt, ctx, path, secret, prefix, text });
        }
    }
    let lines: Vec<String> = (0..25).map(|_| r.pick(&format!("{AN}+/"), 64)).collect();
    let body = lines.join("\n");
    let pem = format!("-----BEGIN RSA PRIVATE KEY-----\n{body}\n-----END RSA PRIVATE KEY-----\n");
    cases.push(Case { fmt: "pem_rsa", ctx: "file", path: "id_rsa", secret: body, prefix: 0, text: pem });
    let lines: Vec<String> = (0..26).map(|_| r.pick(&format!("{AN}+/"), 64)).collect();
    let body = lines.join("\\n");
    let text = format!("{{\n  \"type\": \"service_account\",\n  \"private_key\": \"-----BEGIN PRIVATE KEY-----\\n{body}\\n-----END PRIVATE KEY-----\\n\",\n  \"client_email\": \"bot@proj.iam.gserviceaccount.com\"\n}}\n");
    cases.push(Case { fmt: "gcp_sa_json", ctx: "json", path: "sa.json", secret: body, prefix: 0, text });
    for (ctx, path, template) in [
        ("env", ".env", "DATABASE_URL=postgres://app:{P}@db.internal:5432/app\n"),
        ("yaml", "config.yaml", "redis: redis://default:{P}@cache:6379/0\n"),
        ("log", "app.log", "connecting to mongodb+srv://admin:{P}@cluster0.abcd.mongodb.net/test\n"),
        ("azure", "appsettings.json", "\"Storage\": \"DefaultEndpointsProtocol=https;AccountName=acme;AccountKey={P};EndpointSuffix=core.windows.net\"\n"),
        ("mssql", "appsettings.json", "\"Db\": \"Server=db;Database=app;User Id=sa;Password={P};\"\n"),
    ] {
        let secret = if ctx == "azure" { format!("{}==", r.pick(&format!("{AN}+/"), 86)) } else { r.pick(AN, 18) };
        cases.push(Case { fmt: "conn", ctx, path, text: template.replace("{P}", &secret), secret, prefix: 0 });
    }
    cases
}

/// Whether a run of `n` characters of the secret's random part is still in `out`.
fn survives(out: &str, secret: &str, prefix: usize, n: usize) -> bool {
    let core = &secret[prefix..];
    if core.len() < n {
        return core.len() >= 4 && out.contains(core);
    }
    (0..=core.len() - n).any(|i| out.contains(&core[i..i + n]))
}

/// The cases whose secret the detector cannot tell from a word: a lowercase passphrase, a short password, or one with
/// punctuation, under a name that is no password name (`key=` in a log, `--token`). A known limit.
const KNOWN_MISSES: [(&str, &str); 4] = [("password_long", "log"), ("password_short", "log"), ("password_words", "log"), ("password_words", "cli_flag")];

#[test]
fn every_secret_is_hidden_with_none_of_its_random_part_left() {
    let r = plain();
    let cases = secret_cases();
    assert_eq!(cases.len(), 381);
    let (mut missed, mut leaked) = (Vec::new(), Vec::new());
    for c in &cases {
        let (out, spans) = r.view(&c.text, c.path);
        if survives(&out, &c.secret, c.prefix, 6) {
            if spans.is_empty() { missed.push((c.fmt, c.ctx)) } else { leaked.push(format!("{} {}: {out}", c.fmt, c.ctx)) }
        } else {
            assert!(!spans.is_empty(), "{} {}: changed with no span: {out}", c.fmt, c.ctx);
        }
    }
    assert!(leaked.is_empty(), "partly left: {leaked:#?}");
    assert_eq!(missed, KNOWN_MISSES, "the misses are the known four and no other");
    // the Log's redactor has no path: it finds the same but for a bare name (config files only) and an unquoted lower-case value (366 of 381)
    let hidden = cases.iter().filter(|c| !survives(&r.scrub(&c.text), &c.secret, c.prefix, 6)).count();
    assert!(hidden >= 360, "the Log hides {hidden} of 381");
}

#[test]
fn the_old_rules_missed_what_the_new_detector_hides() {
    let (old, new) = (legacy(), plain());
    let cases = secret_cases();
    let hidden = |r: &Redactor| cases.iter().filter(|c| !survives(&r.view(&c.text, c.path).0, &c.secret, c.prefix, 6)).count();
    let (before, after) = (hidden(&old), hidden(&new));
    assert!(before < 300 && after == 377, "the shape rules hid {before}, the detector {after} of 381");
}

#[test]
fn the_placeholder_keeps_the_schema_and_nothing_random() {
    let r = plain();
    let key = format!("sk-proj-{}", "aB3dE5gH7jK9mN1pQ3sT5vW7".repeat(2));
    let (out, spans) = r.view(&format!("OPENAI_API_KEY={key}\n"), ".env");
    let ph = format!("«REDACTED openai_key starts=sk-proj- length={} charset=alnum same-as-id={}»", key.len(), spans[0].id);
    assert_eq!(out, format!("OPENAI_API_KEY={ph}\n"));
    assert_eq!(&out[spans[0].at.clone()], ph, "a span says where its placeholder is");
    assert_eq!((spans[0].kind, spans[0].length, spans[0].charset), ("openai_key", key.len(), "alnum"));
    assert_eq!(spans[0].id.len(), 4);
    let user = r.view("DATABASE_URL=postgres://app:Zq8Lm2Vx9Rt4Ys6W@db.internal:5432/app\n", ".env").0;
    assert_eq!(blank_ids(&user), "DATABASE_URL=postgres://app:«REDACTED password length=16 charset=alnum same-as-id=ID»@db.internal:5432/app\n");
    let pem = "-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEAx7\nQ2lKm9Rt4Ys6Wz8Lm2V\n-----END RSA PRIVATE KEY-----\n";
    assert_eq!(blank_ids(&r.view(pem, "id_rsa").0), "-----BEGIN RSA PRIVATE KEY-----\n«REDACTED private_key length=38 charset=text same-as-id=ID» (2 lines)\n-----END RSA PRIVATE KEY-----\n");
    let jwt = format!("{}.{}.{}", b64url(br#"{"alg":"HS256"}"#), b64url(br#"{"role":"admin","exp":1,"iat":2}"#), "Zq8Lm2Vx9Rt4Ys6Wz8Lm2Vx9Rt4Ys6W");
    let out = blank_ids(&r.view(&format!("token={jwt}"), "a.txt").0);
    assert_eq!(out, format!("token=«REDACTED jwt starts=alg=HS256;claims=exp,iat,role length={} charset=text same-as-id=ID»", jwt.len()));
    assert_eq!(blank_ids(&r.view(&format!("export GITHUB_TOKEN=ghp_{}", alnum(36)), "a.sh").0), "export GITHUB_TOKEN=«REDACTED github_token starts=ghp_ length=40 charset=alnum same-as-id=ID»");
}

#[test]
fn json_stays_json_and_other_lines_stay_as_they_were() {
    let r = plain();
    for c in secret_cases().iter().filter(|c| c.path.ends_with(".json") && c.fmt != "conn") {
        let out = r.view(&c.text, c.path).0;
        serde_json::from_str::<serde_json::Value>(&out).unwrap_or_else(|e| panic!("{} {}: {e}: {out}", c.fmt, c.ctx));
    }
    let text = "# the app\nAPP_ENV=production\nPORT=8080\nSTRIPE_SECRET_KEY=sk_test_Zq8Lm2Vx9Rt4Ys6Wz8Lm2Vx9Rt4\nLOG_LEVEL=debug\n";
    let out = r.view(text, ".env").0;
    let (a, b): (Vec<&str>, Vec<&str>) = (text.lines().collect(), out.lines().collect());
    assert_eq!(a.len(), b.len());
    assert_eq!(a.iter().zip(&b).filter(|(x, y)| x != y).count(), 1, "one line changed: {out}");
}

#[test]
fn the_same_secret_has_the_same_id_in_every_file_and_a_salt_of_its_own() {
    let s = crate::world::Scratch::new("redact-salt");
    let (a, b) = (plain(), plain());
    assert!(a.keep_salt(&s.join("home-a")).is_ok() && b.keep_salt(&s.join("home-b")).is_ok());
    let id = |r: &Redactor, v: &str, path: &str| r.view(&format!("API_KEY={v}\n"), path).1[0].id.clone();
    let values: Vec<String> = (0..12).map(|i| format!("Zq8Lm2Vx9Rt4Ys6W{i:04}")).collect();
    for v in &values {
        assert_eq!(id(&a, v, ".env"), id(&a, v, "other/.env.local"), "the same value, the same id");
    }
    let ids: std::collections::BTreeSet<_> = values.iter().map(|v| id(&a, v, ".env")).collect();
    assert!(ids.len() >= 6, "different values, different ids: {ids:?}");
    assert!(values.iter().any(|v| id(&a, v, ".env") != id(&b, v, ".env")), "another machine's salt gives other ids");
    // the salt is kept: a new Redactor on the same home reads it back
    let again = plain();
    again.keep_salt(&s.join("home-a")).unwrap();
    assert!(values.iter().all(|v| id(&a, v, ".env") == id(&again, v, ".env")));
    let file = s.join("home-a/guard.salt");
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(&file).unwrap().len(), 32);
    assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
    // a known salt gives a known id: HMAC-SHA256(salt = 32 × 0x07, "hunter2hunter2"), first two bytes
    std::fs::write(&file, [7u8; 32]).unwrap();
    let known = plain();
    known.keep_salt(&s.join("home-a")).unwrap();
    assert_eq!(id(&known, "hunter2hunter2", ".env"), "e4b8");
}

#[test]
fn a_salt_others_can_read_is_narrowed_to_its_owner_and_kept_as_it_is() {
    use std::os::unix::fs::PermissionsExt;
    let s = crate::world::Scratch::new("redact-widesalt");
    let home = s.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let file = home.join("guard.salt");
    let mode = |f: &std::path::Path| std::fs::metadata(f).unwrap().permissions().mode() & 0o777;
    // a good salt of a wider mode: kept, and narrowed
    std::fs::write(&file, [7u8; 32]).unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    let r = plain();
    r.keep_salt(&home).unwrap();
    assert_eq!(mode(&file), 0o600);
    assert_eq!(std::fs::read(&file).unwrap(), [7u8; 32], "the salt itself is kept");
    assert_eq!(r.view("API_KEY=hunter2hunter2\n", ".env").1[0].id, "e4b8");
    // a broken one of a wider mode: written again, and narrowed too
    std::fs::write(&file, b"short").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o664)).unwrap();
    plain().keep_salt(&home).unwrap();
    assert_eq!((mode(&file), std::fs::metadata(&file).unwrap().len()), (0o600, 32));
}

#[test]
fn a_salt_that_cannot_be_kept_is_said_and_the_ids_still_work() {
    let s = crate::world::Scratch::new("redact-nosalt");
    s.put("file", "x");
    let r = plain();
    let err = r.keep_salt(&s.join("file/home")).unwrap_err();
    assert!(err.contains("guard.salt") && err.contains("is not kept") && err.contains("ids differ between runs"), "{err}");
    let (out, spans) = r.view("API_KEY=Zq8Lm2Vx9Rt4Ys6Wz8Lm\n", ".env");
    assert!(out.contains("same-as-id=") && spans.len() == 1);
    assert_eq!(r.view("API_KEY=Zq8Lm2Vx9Rt4Ys6Wz8Lm\n", ".env").0, out, "the salt of the process is the same all through");
}

/// 42 look-alikes: none of them is a secret, so none may change.
const LOOK_ALIKES: [(&str, &str); 42] = [
    ("Cargo.lock", "checksum = \"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\"\n"),
    ("package-lock.json", "      \"integrity\": \"sha512-2kNeYq4gGkBuWm8YyZ1hL0tdqXbJgR8AYuVy3qFfJ9YQ1vRL3k+OLa0Wb4bUB0fT9VKhQd1ZXxwYHzS9X6s0Bg==\",\n"),
    ("x.md", "Fixed in commit 5a0c15d2e8b7f1c3a9d4e6f0b2c8a1d3e5f7b9c0.\n"),
    ("x.rs", "let id = Uuid::parse_str(\"67e55044-10b1-426f-9247-bb680e5fe0c8\")?;\n"),
    ("x.rs", "let token = Token![;](span);\n"),
    ("x.rs", "semi_token: input.parse()?,\n"),
    ("x.rs", "let password = Zeroizing::new(password)?;\n"),
    ("x.py", "api_key = os.environ[\"OPENAI_API_KEY\"]\n"),
    ("x.js", "const apiKey = process.env.OPENAI_API_KEY;\n"),
    (".env.example", "OPENAI_API_KEY=your-openai-key-here\n"),
    (".env.example", "OPENAI_API_KEY=\n"),
    (".env", "OPENAI_API_KEY=${OPENAI_API_KEY}\n"),
    ("compose.yml", "      DB_PASSWORD: ${DB_PASSWORD:?missing}\n"),
    ("connector.toml", "secrets = { ANTHROPIC_API_KEY = \"env:ANTHROPIC_API_KEY\" }\n"),
    (".env", "MAX_TOKENS=4096\n"),
    (".env", "TOKEN_LIMIT=200000\n"),
    ("config.yaml", "max_tokens: 1000\n"),
    ("config.yaml", "token_url: https://auth.example.com/oauth/token\n"),
    ("config.yaml", "password_min_length: 12\n"),
    (".env", "DATABASE_URL=postgres://app@db.internal:5432/app\n"),
    (".env", "STRIPE_PUBLISHABLE_KEY=pk_live_51Hx8KjL2mNq3rT5vW7yZ9bC1dF3gH5jK7lM9nP1qR3sT5\n"),
    ("x.md", "Set `GITHUB_TOKEN` in your repository secrets, then run `gh auth login`.\n"),
    ("x.md", "The risk-management-framework-document-v2 is linked below.\n"),
    ("x.html", "<img src=\"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==\">\n"),
    ("x.rs", "pub const IFLA_INET6_TOKEN: _bindgen_ty_7 = _bindgen_ty_7::IFLA_INET6_TOKEN;\n"),
    ("x.ts", "type Token = { kind: TokenKind; text: string };\n"),
    ("x.py", "tokenizer = AutoTokenizer.from_pretrained('bert-base-uncased')\n"),
    ("x.md", "J4n writes «secret:NAME» into the Log in place of the value.\n"),
    ("x.go", "secret := mustGetenv(\"SESSION_SECRET\")\n"),
    ("config.yaml", "auth:\n  type: oauth2\n  token_endpoint: /oauth/token\n"),
    (".env", "APP_ENV=production\nLOG_LEVEL=debug\nPORT=8080\n"),
    ("x.sh", "git clone https://github.com/example/project.git\n"),
    ("x.md", "AWS docs use AKIAIOSFODNN7EXAMPLE as the sample key id.\n"),
    ("Cargo.toml", "trivially_copy_pass_by_ref = \"allow\"\n"),
    (".circleci/config.yml", "      key: v1-cargo-cache-{{ arch }}-{{ checksum \"Cargo.lock\" }}\n"),
    ("t.rs", "assert!(lines.contains(\"PWD=/home/u/proj\"));\n"),
    (".env", "PWD=/home/u/proj\nOLDPWD=/home/u\n"),
    ("t.sh", "ok \"clean tests pass: empty HOME, scrubbed environment\"\n"),
    ("PLAN.md", "env = { CLAUDE_CODE_OAUTH_TOKEN = \"{secret:claude-main}\" }\n"),
    ("t.rs", "error: unexpected token: \"while parsing\"\n"),
    ("t.json", "\"input\": \"http://user:pass@example.com/\"\n"),
    ("t.rs", "let key = \"hello world\";\n"),
];

#[test]
fn a_look_alike_is_left_alone_by_the_view_and_by_the_log() {
    let r = plain();
    for (path, text) in LOOK_ALIKES {
        assert_eq!(r.view(text, path), (text.to_string(), Vec::new()), "{path}: {text}");
        assert_eq!(r.scrub(text), text, "the Log, {path}");
    }
    // the same lines through the shape rules: the `key = value` rule changes some
    let changed: Vec<&str> = LOOK_ALIKES.iter().filter(|(_, t)| legacy().scrub(t) != *t).map(|(_, t)| *t).collect();
    assert!(changed.len() >= 3, "the old rules change {} of the 42", changed.len());
}

#[test]
fn the_lines_of_real_code_that_the_old_rule_changed_are_not_changed() {
    let (new, old) = (plain(), legacy());
    for code in [
        "pub const IFLA_INET6_TOKEN: _bindgen_ty_7 = _bindgen_ty_7::IFLA_INET6_TOKEN;",
        "    semi_token: Token![;],",
        "let password = Zeroizing::new(password);",
        "pub token_endpoint: Option<String>,",
        "fn secret_name(name: &str) -> bool {",
        "let secret = self.secret_store.lookup_secret_value(name)?;",
    ] {
        assert_eq!(new.scrub(code), code, "{code}");
        assert_eq!(new.view(code, "src/lib.rs").0, code, "{code}");
    }
    assert_ne!(old.scrub("pub const IFLA_INET6_TOKEN: _bindgen_ty_7 = _bindgen_ty_7::IFLA_INET6_TOKEN;"), "pub const IFLA_INET6_TOKEN: _bindgen_ty_7 = _bindgen_ty_7::IFLA_INET6_TOKEN;");
}

#[test]
fn the_log_reads_a_line_as_a_log_is_read() {
    let r = plain();
    let key = "Zq8Lm2Vx9Rt4Ys6Wz8Lm2Vx9R";
    // a quoted literal under a secret name, an env-style `NAME=value`, a vendor key anywhere
    assert_eq!(blank_ids(&r.scrub(&format!("api_key = \"{key}\""))), "api_key = \"«REDACTED secret length=25 charset=alnum same-as-id=ID»\"");
    assert_eq!(blank_ids(&r.scrub("DB_PASSWORD=hunter2hunter2")), "DB_PASSWORD=«REDACTED password length=14 charset=alnum same-as-id=ID»");
    assert_eq!(blank_ids(&r.scrub(&format!("curl -H 'Authorization: Bearer {key}'"))), "curl -H 'Authorization: Bearer «REDACTED bearer_credential length=25 charset=alnum same-as-id=ID»'");
    // a lower-case unquoted assignment is code or prose until a path says it is a config file
    assert_eq!(r.scrub(&format!("api_key = {key}")), format!("api_key = {key}"));
    assert_eq!(blank_ids(&r.view(&format!("api_key = {key}"), "app.ini").0), "api_key = «REDACTED secret length=25 charset=alnum same-as-id=ID»");
    assert_eq!(r.view(&format!("api_key = {key}"), "app.py").0, format!("api_key = {key}"), "in code, an unquoted right side is an expression");
}

#[test]
fn a_registered_secret_keeps_its_name_and_the_detector_leaves_the_marker_alone() {
    let r = plain();
    r.register("OPENAI_API_KEY", "sk-proj-Zq8Lm2Vx9Rt4Ys6Wz8Lm2Vx9Rt4");
    let (out, spans) = r.view("OPENAI_API_KEY=sk-proj-Zq8Lm2Vx9Rt4Ys6Wz8Lm2Vx9Rt4\n", ".env");
    assert_eq!((out.as_str(), spans.len()), ("OPENAI_API_KEY=«secret:OPENAI_API_KEY»\n", 0));
    let both = r.scrub(&format!("a sk-proj-Zq8Lm2Vx9Rt4Ys6Wz8Lm2Vx9Rt4 b ghp_{} c", alnum(36)));
    assert_eq!(blank_ids(&both), "a «secret:OPENAI_API_KEY» b «REDACTED github_token starts=ghp_ length=40 charset=alnum same-as-id=ID» c");
    assert_eq!(r.scrub(&both), both, "idempotent");
}

#[test]
fn with_the_switch_off_the_view_is_the_old_redactor_and_says_no_span() {
    let (off, on) = (legacy(), plain());
    let line = "export OPENAI_API_KEY=sk-proj-abcdefghijklmnopqrstuvwx";
    assert_eq!(off.scrub(line), "export OPENAI_API_KEY=$$REDACTED:sk$$");
    assert_eq!(off.view(line, ".env"), ("export OPENAI_API_KEY=$$REDACTED:sk$$".to_string(), Vec::new()));
    assert!(on.scrub(line).contains("«REDACTED openai_key"));
    // the switch is shared by the clones (the Log holds one) and can be turned back on
    let shared = on.clone();
    on.set_guard(false);
    assert_eq!(shared.scrub(line), "export OPENAI_API_KEY=$$REDACTED:sk$$");
    on.set_guard(true);
    assert!(shared.scrub(line).contains("«REDACTED openai_key"));
}

#[test]
fn a_context_free_secret_is_found_the_same_by_the_log_and_by_a_view() {
    let r = plain();
    for c in secret_cases().iter().filter(|c| !c.fmt.starts_with("password") && !matches!(c.fmt, "generic_secret" | "client_secret" | "aws_secret" | "webhook_secret" | "conn" | "pem_rsa" | "gcp_sa_json")) {
        assert_eq!(blank_ids(&r.scrub(&c.text)), blank_ids(&r.view(&c.text, "x.txt").0), "{} {}", c.fmt, c.ctx);
    }
}

#[test]
fn a_typical_env_file_is_viewed_within_a_millisecond_of_cpu() {
    let r = plain();
    let mut env = String::from("# the app\n");
    for i in 0..45 {
        env.push_str(&format!("SETTING_{i}=value-{i}\n"));
    }
    env.push_str("OPENAI_API_KEY=sk-proj-Zq8Lm2Vx9Rt4Ys6Wz8Lm2Vx9Rt4Zq8Lm2Vx9Rt4Ys6Wz8Lm\nSTRIPE_SECRET_KEY=sk_test_Zq8Lm2Vx9Rt4Ys6Wz8Lm2Vx9Rt4\n");
    env.push_str("DATABASE_URL=postgres://app:Zq8Lm2Vx9Rt4Ys6W@db.internal:5432/app\nDB_PASSWORD=\"Zq8Lm2Vx9Rt4Ys6W\"\nSESSION_SECRET=0123456789abcdef0123456789abcdef\n");
    env.push_str(&format!("GITHUB_TOKEN=ghp_{}\n", alnum(36)));
    let spans = r.view(&env, ".env").1;
    assert!(env.len() > 1_000 && spans.len() == 6, "{} bytes, {:?}", env.len(), spans.iter().map(|s| s.kind).collect::<Vec<_>>());
    // the guard's document says 1 ms; the tests are not optimized (about 8 times slower)
    let limit = Duration::from_millis(if cfg!(debug_assertions) { 8 } else { 1 });
    crate::cpu::assert_within(limit, "a typical .env", || {
        crate::cpu::within(limit, 20, || {
            std::hint::black_box(r.view(std::hint::black_box(&env), ".env"));
        })
    });
}

/// The CPU of the scan of code, in units of a plain walk over its bytes: 7 to 9 measured, in the unoptimized test build, alone and on a
/// machine at twice its cores in load; the old 500 ms was 3.3 times the 150 ms it took alone, and 24 is as much.
const WALK_LIMIT: f64 = 24.0;

#[test]
fn code_with_no_secret_and_hostile_lines_are_scanned_in_linear_time() {
    let r = plain();
    let line = "fn main() { let token = lexer.next_token(); println!(\"{}\", password_hint); }\n";
    let code = line.repeat(15_000);
    // the guard's document says 500 ms for 1 MiB of code, alone (about 150 ms in the unoptimized test build). A number of milliseconds fails in
    // a gate that runs eight test threads (910 ms of CPU, the least of three), so the cost is held in units of a plain walk over
    // the same bytes that is timed beside it, each pair back to back on this thread (`cpu::ratio_within`), and 1/15 of the code against
    // all of it holds the scaling: 15 times the code takes about 15 times the CPU (measured 14.9) and a scan that squares the time takes 225 times.
    let walk = |text: &str| {
        let mut sum = 0u64;
        for b in std::hint::black_box(text).bytes() {
            sum = sum.wrapping_add(u64::from(b.is_ascii_alphanumeric()) + u64::from(b == b'\n') * 3);
        }
        std::hint::black_box(sum);
    };
    let ratio = crate::cpu::ratio_within(WALK_LIMIT, 6, || walk(&code), || drop(std::hint::black_box(r.view(&code, "main.rs"))));
    assert!(ratio <= WALK_LIMIT, "1 MiB of code took {ratio:.1} times the CPU of a plain walk over it, the limit is {WALK_LIMIT}");
    let (few, all) = (line.repeat(1_000), &code);
    let scaling = crate::cpu::ratio_within(40.0, 6, || drop(std::hint::black_box(r.view(&few, "main.rs"))), || drop(std::hint::black_box(r.view(all, "main.rs"))));
    assert!(scaling <= 40.0, "15 times the code took {scaling:.1} times the CPU, not about 15 times");
    let (small, big) = ("key=\"".repeat(2_000), "key=\"".repeat(16_000));
    let linear = crate::cpu::ratio_within(40.0, 6, || drop(r.scrub(&small)), || drop(r.scrub(&big)));
    assert!(linear <= 40.0, "8 times the line took {linear:.1} times the CPU, not about 8 times");
}

#[test]
fn a_secret_cut_by_the_end_of_a_chunk_or_not_a_whole_token_is_not_taken_for_one() {
    let r = plain();
    for text in ["sk-proj-short", "xsk-proj-Zq8Lm2Vx9Rt4Ys6Wz8Lm2Vx9Rt4", "ghp_Zq8Lm2Vx9Rt4Ys6Wz8Lm2Vx9Rt4Ys6Wz8Lm2Vx9R9", "-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBg", "AKIAIOSFODNN7EXAMPLE", "password=your-password-here", "eyJhbGciOiJIUzI1NiJ9.only.two",
        "-----BEGIN CERTIFICATE-----\nMIIDdzCCAl+gAwIBAgIEAgAAuTANBgkqhkiG9w0BAQUFADBaMQswCQYDVQQGEwJJ\n-----END CERTIFICATE-----\n",
        "-----BEGIN PUBLIC KEY-----\nMFwwDQYJKoZIhvcNAQEBBQADSwAwSAJBAMsH3x9bQ0e7oZ2T\n-----END PUBLIC KEY-----\n",
    ] {
        assert_eq!(r.scrub(text), text, "{text}");
    }
}

/// The measurement on text that is not in the tree: `J4N_REDACT_CORPUS` names a file that lists the paths
/// (NUL-separated) and `J4N_REDACT_OUT` where to write the lines that change, one `path<TAB>line` each, then a count line.
/// Run it with `--ignored`; the counts are in the guard's document.
#[test]
#[ignore = "a measurement over a corpus named by J4N_REDACT_CORPUS"]
fn measure_a_corpus_of_real_text() {
    let (Ok(list), Ok(out)) = (std::env::var("J4N_REDACT_CORPUS"), std::env::var("J4N_REDACT_OUT")) else { return };
    let r = plain();
    let (mut files, mut changed, mut spans, mut report) = (0, 0, 0, String::new());
    let started = crate::cpu::start();
    for path in std::fs::read_to_string(list).unwrap().split('\0').filter(|p| !p.is_empty()) {
        let Ok(text) = std::fs::read_to_string(path) else { continue };
        files += 1;
        let (view, found) = r.view(&text, path);
        if found.is_empty() {
            continue;
        }
        changed += 1;
        spans += found.len();
        for (a, _) in text.lines().zip(view.lines()).filter(|(a, b)| a != b) {
            report.push_str(&format!("{path}\t{}\n", a.trim().chars().take(160).collect::<String>()));
        }
    }
    report.push_str(&format!("#files {files} changed {changed} spans {spans} cpu {:?}\n", started.elapsed()));
    std::fs::write(out, report).unwrap();
}

#[test]
fn a_secret_word_has_to_end_the_name() {
    let r = plain();
    let value = "Zq8Lm2Vx9Rt4Ys6W1";
    for name in ["TOKEN_BUCKET_SIZE", "SECRET_NAME", "api_key_file", "PASSWORD_FILE", "token_endpoint", "keyboard", "primary_key_id"] {
        let line = format!("{name}: {value}");
        assert_eq!(r.view(&line, "app.yaml").0, line, "{name}");
        assert_eq!(r.scrub(&line.replace(": ", "=").to_uppercase()), line.replace(": ", "=").to_uppercase(), "{name}");
    }
    for name in ["x_api_key", "clientSecret", "X-Api-Key", "DB_PASSPHRASE", "session.secret_key", "STRIPE_SIGNING_KEY"] {
        let line = format!("{name}: {value}");
        assert!(r.view(&line, "app.yaml").0.contains("«REDACTED"), "{name}");
    }
}

/// What a remote server prints is read as a config file's text (`view(text, ".env")`), so the usual lines of an env file and the same assignments inside a
/// sentence of the server's (`warning: API_TOKEN=… was used`, which the scan used to step over as the value of `warning:`) lose their values.
#[test]
fn what_a_remote_env_file_holds_is_hidden_alone_and_inside_a_sentence() {
    let r = plain();
    let (tok, pass, key) = ("Zq8Lm2Vx9Rt4Ys6Wb3Nc7Hd5Jk1Pf0Ae", "Wp4Rt7Yu2Io9Lk3S", "ghp_Zq8Lm2Vx9Rt4Ys6Wb3Nc7Hd5Jk1Pf0Ae12");
    for (line, secret) in [
        (format!("APP_TOKEN={tok}"), tok),
        (format!("SERVICE_API_KEY={tok}"), tok),
        (format!("SESSION_SECRET={tok}"), tok),
        (format!("PASSWORD={pass}"), pass),
        (format!("DB_PASSWORD='{pass}'"), pass),
        (format!("DATABASE_URL=postgres://app:{pass}@db.internal:5432/app"), pass),
        (format!("export GITHUB_TOKEN={key}"), key),
        (format!("warning: APP_TOKEN={tok} was used"), tok),
        (format!("2026-11-14 12:00 error: retry with SERVICE_API_KEY={tok}, again"), tok),
        (format!("Environment=APP_TOKEN={tok} PORT=8088"), tok),
        (format!("curl -H 'Authorization: Bearer {tok}' http://127.0.0.1/health"), tok),
    ] {
        let out = r.view(&line, "remote.env").0;
        assert!(!out.contains(secret), "`{line}` -> `{out}`");
        assert!(out.contains("REDACTED"), "`{line}` -> `{out}`");
    }
    let pem = "-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZWQyNTUxOQAAACD0Xk3Q\n-----END OPENSSH PRIVATE KEY-----\n";
    assert!(!r.view(pem, "remote.env").0.contains("b3BlbnNz"));
    // what is not a secret stays
    for line in ["PORT=8088", "the service is active", "warning: disk at 91% on /var", "LOG_LEVEL=debug", "Environment=PORT=8088 MODE=prod"] {
        assert_eq!(r.view(line, "remote.env").0, line);
    }
}
