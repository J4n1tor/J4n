//! Redaction, a floor of the core (law 9): a secret never reaches the disk, the
//! screen or a model through anything the Log or a View writes. Two passes, both without a model:
//!
//! 1. **Exact values.** Every secret J4n resolves is registered ([`Redactor::register`], or [`resolve`] which does both),
//!    and its text is replaced wherever it appears by `«secret:NAME»` — as it was given, as JSON writes it (so a blob that
//!    holds JSON text is covered), and percent-encoded (so a URL is). A value shorter than [`MIN_SECRET`] characters is
//!    refused: it cannot be told from ordinary text, so it is not used.
//! 2. **Detection** (redactor v2): one local detector finds what looks like a secret — about 25
//!    vendor shapes, PEM keys, a password in a URL, credentials in a header, a connection string or a flag, and the value
//!    of a secret *name* — and each is replaced by `«REDACTED <kind> [starts=<public prefix>] length=<n> charset=<cs>
//!    same-as-id=<4 hex>»`. The placeholder keeps what a reader may know (the kind, the vendor's fixed prefix, the length,
//!    the charset, and an id that is the same for the same value) and nothing random. [`Redactor::scrub`] is this for the
//!    Log (it has no path); [`Redactor::view`] is this for a model, told the file's path, and says where each placeholder
//!    sits. With the pack `guard` off ([`Redactor::set_guard`]) the shape rules run instead.
//!
//! Known limits: a secret that is transformed (base64, hex, split across two flashed deltas) is not found; a short or
//! lowercase password under a name that is not a password name (`key=S6ePRwo` in a log line) is not found by shape;
//! numbers in event data are not looked at. Scrubbing is idempotent: scrubbing scrubbed text changes nothing.

use crate::schema::is_secret_ref;
use serde_json::Value as Json;
use sha2::{Digest, Sha256};
use std::fmt;
use std::ops::Range;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

/// A secret shorter than this is refused.
pub const MIN_SECRET: usize = 6;
const NAME_MAX: usize = 64;
/// Data nested deeper than this is cut: nothing is read or written below it.
pub const MAX_DEPTH: usize = 64;

#[derive(Default)]
struct State {
    forms: RwLock<Vec<(String, String)>>,
    /// The pack `guard` is off: the shape rules run, not the detector.
    legacy: AtomicBool,
    /// The per-machine salt of `same-as-id` ([`Redactor::keep_salt`]); a fresh random one when none was kept.
    salt: RwLock<Option<[u8; 32]>>,
}

/// The shared list of registered secrets. Clones share it, so the Log, the blob store and every View see one set.
#[derive(Clone, Default)]
pub struct Redactor(Arc<State>);

impl fmt::Debug for Redactor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Redactor({} forms)", self.forms())
    }
}

fn percent(value: &str, upper: bool) -> String {
    let mut out = String::new();
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else if upper {
            out.push_str(&format!("%{b:02X}"));
        } else {
            out.push_str(&format!("%{b:02x}"));
        }
    }
    out
}

impl Redactor {
    pub fn new() -> Redactor {
        Redactor::default()
    }

    /// Register a secret under `name` (what the marker says). False when the name is not an identifier or the value is
    /// shorter than [`MIN_SECRET`]: such a value is not kept out of the Log and must not be used.
    pub fn register(&self, name: &str, value: &str) -> bool {
        let name_ok = !name.is_empty()
            && name.len() <= NAME_MAX
            && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':' | '/'));
        if !name_ok || value.chars().count() < MIN_SECRET {
            return false;
        }
        let json = serde_json::to_string(value).unwrap_or_default();
        let inner = json.strip_prefix('"').and_then(|j| j.strip_suffix('"')).unwrap_or_default();
        let forms = [value.to_string(), inner.to_string(), percent(value, true), percent(value, false)];
        let marker = format!("«secret:{name}»");
        let Ok(mut all) = self.0.forms.write() else { return false };
        for form in forms {
            if form.len() >= MIN_SECRET && !all.iter().any(|(f, _)| *f == form) {
                all.push((form, marker.clone()));
            }
        }
        all.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(&b.0)));
        true
    }

    /// How many forms (a secret has up to four) are registered.
    pub fn forms(&self) -> usize {
        self.0.forms.read().map(|v| v.len()).unwrap_or(0)
    }

    /// The pack `guard`'s switch: on (the default) the detector runs, off the shape rules do.
    pub fn set_guard(&self, on: bool) {
        self.0.legacy.store(!on, Ordering::Relaxed);
    }

    /// Use the salt in `<home>/guard.salt` for `same-as-id`, and make the file (32 random bytes, mode 0600) when it is not
    /// there; a kept one that others could read is narrowed to 0600. The error is a sentence for the operator: a salt that could not
    /// be made leaves the process a random one of its own (the ids then differ between runs, nothing else), and one that could not be
    /// narrowed is still used.
    pub fn keep_salt(&self, home: &Path) -> Result<(), String> {
        use std::os::unix::fs::PermissionsExt;
        let path = home.join("guard.salt");
        let kept = std::fs::read(&path).ok().and_then(|b| <[u8; 32]>::try_from(b).ok());
        let salt = kept.unwrap_or_else(random_salt);
        if let Ok(mut s) = self.0.salt.write() {
            *s = Some(salt);
        }
        if kept.is_none() {
            return write_salt(&path, &salt).map_err(|e| format!("the salt of same-as-id is not kept ({e}); ids differ between runs"));
        }
        let wide = std::fs::metadata(&path).is_ok_and(|m| m.permissions().mode() & 0o077 != 0);
        if !wide {
            return Ok(());
        }
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("the salt of same-as-id ({}) is readable by others and was not narrowed to 0600 ({e}); it is still used", path.display()))
    }

    fn salt(&self) -> [u8; 32] {
        if let Some(s) = self.0.salt.read().ok().and_then(|s| *s) {
            return s;
        }
        let fresh = random_salt();
        self.0.salt.write().map_or(fresh, |mut s| *s.get_or_insert(fresh))
    }

    /// `text` with the registered values replaced.
    fn registered(&self, text: &str) -> String {
        let mut out = text.to_string();
        if let Ok(all) = self.0.forms.read() {
            for (form, marker) in all.iter() {
                if out.contains(form.as_str()) {
                    out = out.replace(form.as_str(), marker);
                }
            }
        }
        out
    }

    /// `text` without any secret: the registered values, then the detector (the shapes with `guard` off). For the Log,
    /// which has no path: a quoted literal under a secret name counts, an unquoted one only as `NAME=value` in the env style.
    pub fn scrub(&self, text: &str) -> String {
        let out = self.registered(text);
        if self.0.legacy.load(Ordering::Relaxed) { shapes(&out) } else { self.detect(out, Ctx::Unknown).0 }
    }

    /// What a model may read of a file at `path`: [`scrub`](Redactor::scrub)'s detector with the path telling a config file
    /// (where a bare `key`, `token` or `password` counts and an unquoted value does) from code, and where each placeholder
    /// sits in the returned text. With `guard` off, the shapes and no spans.
    pub fn view(&self, text: &str, path: &str) -> (String, Vec<Span>) {
        let out = self.registered(text);
        if self.0.legacy.load(Ordering::Relaxed) {
            return (shapes(&out), Vec::new());
        }
        self.detect(out, if is_config(path) { Ctx::Config } else { Ctx::Code })
    }

    /// Every string in `v`, and every key, scrubbed in place. Data nested deeper than [`MAX_DEPTH`] is replaced.
    pub fn scrub_json(&self, v: &mut Json) {
        self.walk(v, 0);
    }

    fn walk(&self, v: &mut Json, depth: usize) {
        if depth > MAX_DEPTH {
            *v = Json::String("[nested too deep]".into());
            return;
        }
        match v {
            Json::String(s) => {
                let clean = self.scrub(s);
                if clean != *s {
                    *s = clean;
                }
            }
            Json::Array(items) => items.iter_mut().for_each(|i| self.walk(i, depth + 1)),
            Json::Object(map) => {
                let old = std::mem::take(map);
                for (k, mut item) in old {
                    self.walk(&mut item, depth + 1);
                    map.insert(self.scrub(&k), item);
                }
            }
            _ => {}
        }
    }

    /// Bytes without any secret: text is scrubbed as text; bytes that are not text only lose the registered values.
    pub fn scrub_bytes(&self, bytes: &[u8]) -> Vec<u8> {
        if let Ok(text) = std::str::from_utf8(bytes) {
            return self.scrub(text).into_bytes();
        }
        let mut out = bytes.to_vec();
        if let Ok(all) = self.0.forms.read() {
            for (form, marker) in all.iter() {
                out = replace_bytes(&out, form.as_bytes(), marker.as_bytes());
            }
        }
        out
    }
}

fn replace_bytes(hay: &[u8], needle: &[u8], with: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(hay.len());
    let mut i = 0;
    while i < hay.len() {
        if hay[i..].starts_with(needle) {
            out.extend_from_slice(with);
            i += needle.len();
        } else {
            out.push(hay[i]);
            i += 1;
        }
    }
    out
}

/// Read the secret `sref` names (`env:NAME`, `file:/path`, `file:~/path`) and register it. This is the only way a secret
/// gets into J4n, so none is used that the Log has not been told of. A file must not be readable by anyone else.
pub fn resolve(sref: &str, red: &Redactor) -> Result<String, String> {
    resolve_in(sref, red, std::env::var_os("HOME").as_deref().map(std::path::Path::new))
}

/// [`resolve`] with the home directory `~/` means given, not read from the environment.
pub fn resolve_in(sref: &str, red: &Redactor, home: Option<&std::path::Path>) -> Result<String, String> {
    if !is_secret_ref(sref) {
        return Err(format!("`{sref}` is not a secret reference (`env:NAME` or `file:/path`)"));
    }
    let (name, raw) = match sref.split_once(':') {
        Some(("env", name)) => (name.to_string(), std::env::var(name).map_err(|_| format!("{name} is not set"))?),
        Some(("kept", id)) => return Err(format!("`kept:{id}` is a key J4n keeps, which only a connector reads")),
        Some((_, path)) => {
            use std::os::unix::fs::PermissionsExt;
            let path = match path.strip_prefix("~/") {
                Some(rest) => home.ok_or("there is no home directory")?.join(rest),
                None => path.into(),
            };
            let mode = std::fs::metadata(&path).map_err(|e| format!("{}: {e}", path.display()))?.permissions().mode();
            if mode & 0o077 != 0 {
                return Err(format!("{} is readable by others (mode {:o}) — chmod 600 it", path.display(), mode & 0o777));
            }
            let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            (path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), text)
        }
        None => return Err(format!("`{sref}` is not a secret reference")),
    };
    let value = raw.trim_end_matches(['\n', '\r']).to_string();
    if !red.register(&name, &value) {
        return Err(format!("{name} is empty or shorter than {MIN_SECRET} characters — it cannot be kept out of the Log, so it is not used"));
    }
    Ok(value)
}

// ---- the shape rules: what runs with the pack `guard` off ----

const KEYS: [&str; 7] = ["api_key", "apikey", "api-key", "token", "secret", "password", "passwd"];

fn token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')
}

fn value_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"+/~=_.-".contains(&b)
}

/// The kind of key a whole token is, if it is one.
fn token_kind(t: &str) -> Option<&'static str> {
    let word = |c: u8| c.is_ascii_alphanumeric() || c == b'-' || c == b'_';
    if t.starts_with("sk-") && t.len() >= 20 && t[3..].bytes().all(word) {
        return Some("sk");
    }
    let github = (t.starts_with("ghp_") && t.len() >= 39)
        || (t.starts_with("github_pat_") && t.len() >= 30)
        || (["gho_", "ghu_", "ghs_", "ghr_"].iter().any(|p| t.starts_with(p)) && t.len() >= 39);
    if github {
        return Some("github");
    }
    if t.len() >= 14 && ["xoxb-", "xoxa-", "xoxp-", "xoxr-", "xoxs-"].iter().any(|p| t.starts_with(p)) {
        return Some("slack");
    }
    if t.starts_with("glpat-") && t.len() >= 26 {
        return Some("gitlab");
    }
    if (t.starts_with("AKIA") || t.starts_with("ASIA"))
        && t.len() == 20
        && t[4..].bytes().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    {
        return Some("aws");
    }
    if t.starts_with("eyJ") {
        let parts: Vec<&str> = t.split('.').collect();
        if parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(word)) {
            return Some("jwt");
        }
    }
    None
}

/// A value that says it is not a real one. (`<your-key>` and `$$NAME$$` never get here: neither `<` nor `$` is a value
/// character, so the value is empty.)
fn placeholder(v: &str) -> bool {
    let l = v.to_ascii_lowercase();
    l.contains("your") || l.contains("example") || l.contains("redacted")
}

type Edit = (usize, usize, &'static str);

fn tokens(line: &str, out: &mut Vec<Edit>) {
    let mut start = None;
    for (i, c) in line.char_indices().chain(std::iter::once((line.len(), ' '))) {
        match (token_char(c), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                start = None;
                let tok = line[s..i].trim_end_matches('.');
                if let Some(kind) = token_kind(tok) {
                    out.push((s, s + tok.len(), kind));
                }
            }
            _ => {}
        }
    }
}

/// `key = value`, `key: value`, `"key": "value"`: the value, when it is long enough to be a credential. Lowercasing is
/// ASCII-only so a byte position in the copy is a byte position in the line (a Unicode lowercase can change lengths).
fn assignments(line: &str, out: &mut Vec<Edit>) {
    let lower = line.to_ascii_lowercase();
    let b = line.as_bytes();
    for key in KEYS {
        let mut from = 0;
        while let Some(i) = lower[from..].find(key) {
            let at = from + i;
            let mut j = at + key.len();
            from = j;
            if line[..at].ends_with('«') {
                continue;
            }
            let skip_space = |j: &mut usize| {
                while *j < b.len() && b[*j].is_ascii_whitespace() {
                    *j += 1;
                }
            };
            if j < b.len() && (b[j] == b'"' || b[j] == b'\'') {
                j += 1;
            }
            skip_space(&mut j);
            if j >= b.len() || !matches!(b[j], b'=' | b':') {
                continue;
            }
            j += 1;
            skip_space(&mut j);
            if j < b.len() && (b[j] == b'"' || b[j] == b'\'') {
                j += 1;
            }
            let start = j;
            while j < b.len() && value_byte(b[j]) {
                j += 1;
            }
            if j - start >= 12 && !placeholder(&line[start..j]) {
                out.push((start, j, "assign"));
                from = j;
            }
        }
    }
}

fn shapes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let mut edits = Vec::new();
        tokens(line, &mut edits);
        assignments(line, &mut edits);
        edits.sort_by_key(|e| (e.0, std::cmp::Reverse(e.1)));
        let mut pos = 0;
        for (s, e, kind) in edits {
            if s < pos {
                continue;
            }
            out.push_str(&line[pos..s]);
            out.push_str(&format!("$$REDACTED:{kind}$$"));
            pos = e;
        }
        out.push_str(&line[pos..]);
    }
    out
}

// ---- redactor v2 ----

/// A placeholder in the text [`Redactor::view`] returned: where it sits (a byte range of that text) and what it says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub kind: &'static str,
    pub at: Range<usize>,
    pub length: usize,
    pub charset: &'static str,
    pub id: String,
}

/// What the text is: a config file (a bare name counts, an unquoted value counts), code (only a quoted literal counts), or
/// not known (the Log: a quoted literal, or an unquoted one under an env-style `NAME=`).
#[derive(Clone, Copy, PartialEq)]
enum Ctx {
    Config,
    Code,
    Unknown,
}

/// A secret found: the byte range of the value in the text, its kind, how many leading bytes are the vendor's public prefix,
/// what `starts=` says, and what follows the placeholder.
#[derive(Default)]
struct Hit {
    from: usize,
    to: usize,
    kind: &'static str,
    prefix: usize,
    starts: String,
    after: String,
}

impl Hit {
    fn new(from: usize, to: usize, kind: &'static str) -> Hit {
        Hit { from, to, kind, ..Hit::default() }
    }
}

impl Redactor {
    fn detect(&self, text: String, ctx: Ctx) -> (String, Vec<Span>) {
        let hits = scan(&text, ctx);
        if hits.is_empty() {
            return (text, Vec::new());
        }
        let salt = self.salt();
        let (mut out, mut spans, mut pos) = (String::with_capacity(text.len() + 64), Vec::new(), 0);
        for h in hits {
            out.push_str(&text[pos..h.from]);
            let value = &text[h.from..h.to];
            let (length, charset) = (value.chars().count(), charset(&value[h.prefix..]));
            let id = same_as_id(&salt, value);
            let at = out.len();
            let starts = if h.starts.is_empty() { String::new() } else { format!(" starts={}", h.starts) };
            out.push_str(&format!("«REDACTED {}{starts} length={length} charset={charset} same-as-id={id}»", h.kind));
            spans.push(Span { kind: h.kind, at: at..out.len(), length, charset, id });
            out.push_str(&h.after);
            pos = h.to;
        }
        out.push_str(&text[pos..]);
        (out, spans)
    }
}

fn random_salt() -> [u8; 32] {
    use std::io::Read;
    let mut salt = [0u8; 32];
    if std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut salt)).is_err() {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        salt = Sha256::digest(format!("{now}{}", std::process::id())).into();
    }
    salt
}

fn write_salt(path: &Path, salt: &[u8; 32]) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let e = |e: std::io::Error| format!("{}: {e}", path.display());
    std::fs::create_dir_all(path.parent().unwrap_or(Path::new("."))).map_err(e)?;
    // a file that was there keeps its mode when it is opened: it is narrowed before the salt is written into it
    let open = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path);
    open.and_then(|mut f| f.set_permissions(std::fs::Permissions::from_mode(0o600)).and_then(|()| f.write_all(salt))).map_err(e)
}

/// HMAC-SHA256 (RFC 2104) for a key of at most one block.
fn hmac(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let pad = |p: u8| {
        let mut block = [p; 64];
        block.iter_mut().zip(key).for_each(|(b, k)| *b ^= k);
        block
    };
    let inner = Sha256::new().chain_update(pad(0x36)).chain_update(msg).finalize();
    Sha256::new().chain_update(pad(0x5c)).chain_update(inner).finalize().into()
}

/// 4 hex characters of the HMAC of `value` under the salt: the same secret has the same id everywhere, and 16 bits say
/// nothing of the value.
fn same_as_id(salt: &[u8; 32], value: &str) -> String {
    let mac = hmac(salt, value.as_bytes());
    format!("{:02x}{:02x}", mac[0], mac[1])
}

// Byte classes: the bytes of the string, `x-y` for a range (a `-` at the end is itself).
const AN: &str = "A-Za-z0-9";
const AN_: &str = "A-Za-z0-9_";
const B64U: &str = "A-Za-z0-9_-";
const LHEX: &str = "0-9a-f";
const UD: &str = "A-Z0-9";

const fn has(class: &str, b: u8) -> bool {
    let (c, mut i) = (class.as_bytes(), 0);
    while i < c.len() {
        if i + 2 < c.len() && c[i + 1] == b'-' {
            if c[i] <= b && b <= c[i + 2] {
                return true;
            }
            i += 3;
        } else {
            if c[i] == b {
                return true;
            }
            i += 1;
        }
    }
    false
}

fn word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// The end of the run of bytes from `from` that `f` accepts.
fn run(b: &[u8], from: usize, f: impl Fn(u8) -> bool) -> usize {
    from + b[from.min(b.len())..].iter().take_while(|&&c| f(c)).count()
}

fn skip_ws(b: &[u8], from: usize) -> usize {
    run(b, from, |c| c.is_ascii_whitespace())
}

fn charset(v: &str) -> &'static str {
    let all = |class: &str| v.bytes().all(|b| has(class, b));
    match () {
        _ if v.is_empty() => "text",
        _ if all("0-9") => "digits",
        _ if all("0-9a-fA-F") => "hex",
        _ if all(AN) => "alnum",
        _ if all(B64U) => "b64url",
        _ if all("A-Za-z0-9+/=") => "b64",
        _ if all("A-Za-z0-9+/=_-") => "b64mix",
        _ => "text",
    }
}

/// How many of lowercase, uppercase and digits the value has.
fn classes(v: &str) -> usize {
    [v.bytes().any(|c| c.is_ascii_lowercase()), v.bytes().any(|c| c.is_ascii_uppercase()), v.bytes().any(|c| c.is_ascii_digit())].iter().filter(|&&p| p).count()
}

/// A value that says it is not a real one: a reference to a secret kept elsewhere, a template, a placeholder, or a word.
fn fake(v: &str) -> bool {
    const WORDS: [&str; 20] = [
        "your", "example", "changeme", "change_me", "placeholder", "dummy", "redacted", "todo", "none", "null", "nil", "true", "false", "yes", "no", "on", "off",
        "test", "secret", "password",
    ];
    let l = v.to_ascii_lowercase();
    WORDS.contains(&l.as_str())
        || l == "token"
        || (l.len() >= 3 && l.bytes().all(|c| c == b'x'))
        || (!l.is_empty() && l.bytes().all(|c| c == b'*'))
        || ["your_", "your-", "example", "placeholder", "xxxxx"].iter().any(|s| l.contains(s))
}

fn reference(v: &str) -> bool {
    const STARTS: [&str; 12] = ["${", "env:", "file:", "secret:", "{secret:", "vault:", "op://", "os.environ", "process.env", "ENV[", "getenv", "«"];
    const HAS: [&str; 8] = ["{{", "${", "<<", "«secret:", "«REDACTED", "{}", "%s", "%("];
    let b = v.as_bytes();
    let ident = |s: &[u8]| s.first().is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_');
    let braced = b.first() == Some(&b'{') && ident(&b[1..]) && b[1..].iter().find(|&&c| !(word(c) || c == b'.')) == Some(&b'}');
    STARTS.iter().any(|s| v.starts_with(s))
        || HAS.iter().any(|s| v.contains(s))
        || (b.first() == Some(&b'$') && ident(&b[1..]))
        || (b.first() == Some(&b'<') && v.contains('>'))
        || (b.first() == Some(&b'%') && ident(b[1..].strip_prefix(b"(").unwrap_or(&b[1..])))
        || braced
        || (b.len() >= 2 && b[0].is_ascii_uppercase() && b.iter().all(|&c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_'))
}

/// A secret's name is split into parts (chess, kebab, dots, camelCase); its last part, or last two, must be a secret word.
fn name_parts(name: &str) -> Vec<String> {
    let (mut parts, mut cur, mut prev) = (Vec::new(), String::new(), ' ');
    for c in name.chars() {
        let sep = matches!(c, '_' | '.' | '-');
        if (sep || (c.is_ascii_uppercase() && (prev.is_ascii_lowercase() || prev.is_ascii_digit()))) && !cur.is_empty() {
            parts.push(std::mem::take(&mut cur));
        }
        if !sep {
            cur.push(c.to_ascii_lowercase());
        }
        prev = c;
    }
    if !cur.is_empty() {
        parts.push(cur);
    }
    parts
}

/// Whether a file's name says it holds a secret: a part between its dots is a secret's name (`client_secret.json`, `db.password`), and a
/// bare word (`password.txt`, `passwd`) counts unless the file is source code or prose (`token.rs`, a lexer's).
pub fn secret_file(name: &str) -> bool {
    const CODE: [&str; 21] = ["rs", "py", "go", "js", "ts", "tsx", "jsx", "java", "kt", "c", "h", "cpp", "hpp", "cs", "rb", "php", "swift", "lua", "zig", "sh", "md"];
    let parts: Vec<&str> = name.split('.').filter(|p| !p.is_empty()).collect();
    let code = parts.len() > 1 && parts.last().is_some_and(|e| CODE.contains(&e.to_ascii_lowercase().as_str()));
    parts.iter().any(|p| secret_name(p, &name_parts(p), !code))
}

fn secret_name(name: &str, parts: &[String], config: bool) -> bool {
    const LAST_ONE: [&str; 10] = ["secret", "token", "password", "passwd", "pwd", "passphrase", "apikey", "credential", "credentials", "dsn"];
    const LAST_TWO: [(&str, &str); 11] = [
        ("api", "key"), ("private", "key"), ("access", "key"), ("secret", "key"), ("session", "key"), ("signing", "key"), ("encryption", "key"),
        ("master", "key"), ("auth", "key"), ("license", "key"), ("account", "key"),
    ];
    const BARE: [&str; 9] = ["secret", "token", "password", "passwd", "pwd", "pass", "key", "apikey", "auth"];
    let p: Vec<&str> = parts.iter().map(String::as_str).collect();
    match p.as_slice() {
        [] => false,
        // the shell's current directory
        _ if name.eq_ignore_ascii_case("pwd") || name.eq_ignore_ascii_case("oldpwd") => false,
        [one] if BARE.contains(one) => config,
        [.., prev, last] => LAST_ONE.contains(last) || LAST_TWO.contains(&(*prev, *last)),
        [last] => LAST_ONE.contains(last),
    }
}

fn scan(text: &str, ctx: Ctx) -> Vec<Hit> {
    let mut hits = Vec::new();
    if text.len() < 6 {
        return hits;
    }
    let lower = text.to_ascii_lowercase();
    pem(text, &mut hits);
    vendor(text, &mut hits);
    url_password(text, &mut hits);
    auth(text, &lower, &mut hits);
    connection(text, &lower, &mut hits);
    flags(text, &lower, &mut hits);
    let mut base = 0;
    for line in text.split_inclusive('\n') {
        let t = line.trim_start();
        if !(ctx == Ctx::Code && (t.starts_with(['#', ';']) || t.starts_with("//"))) {
            named(line, base, ctx, &mut hits);
            if ctx == Ctx::Config {
                inline(line, base, ctx, &mut hits);
            }
        }
        base += line.len();
    }
    // the first rule to find a secret keeps it: a stable sort by start, then what starts inside an earlier hit is dropped
    hits.sort_by_key(|h| h.from);
    let mut end = 0;
    hits.retain(|h| {
        let keep = h.from >= end;
        if keep {
            end = h.to;
        }
        keep
    });
    hits
}

/// `-----BEGIN … PRIVATE KEY-----`, the body, `-----END … PRIVATE KEY-----`, also with the line breaks written as `\n`.
fn pem(text: &str, hits: &mut Vec<Hit>) {
    let mut from = 0;
    while let Some(i) = text[from..].find("-----BEGIN ") {
        let label_at = from + i + 11;
        from = label_at;
        let Some(len) = text[label_at..].find("-----") else { break };
        let label = &text[label_at..label_at + len];
        if !label.ends_with("PRIVATE KEY") || !label.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b' ') {
            continue;
        }
        let rest = &text[label_at + len + 5..];
        let ws = rest.len() - rest.trim_start().len();
        let (nl, start) = match () {
            _ if rest[..ws].contains('\n') => ("\n", label_at + len + 5 + ws),
            _ if rest.starts_with("\\n") => ("\\n", label_at + len + 7),
            _ => continue,
        };
        // the END line follows a line break: the first one that does
        let mark = format!("-----END {label}-----");
        let Some(end) = text[start..].match_indices(&mark).map(|(e, _)| start + e).find(|&e| text[..e].ends_with('\n') || text[..e].ends_with("\\n")) else { continue };
        let body = text[start..end].trim_end_matches(['\r', '\n']);
        let body = body.strip_suffix("\\n").unwrap_or(body);
        if body.is_empty() || body.contains("«REDACTED") {
            continue;
        }
        let mut hit = Hit::new(start, start + body.len(), "private_key");
        hit.after = format!(" ({} lines)", body.matches(nl).count() + 1);
        hits.push(hit);
        from = end;
    }
}

enum Seg {
    L(&'static str),
    C(&'static str, usize, usize),
}
use Seg::{C, L};
const MANY: usize = usize::MAX;

/// A vendor's shape: its kind, the literals it can start with (the longest that fits is the public prefix), and the rest as
/// literals and runs of a class of bytes (`least..=most` of them). A token is bounded by a non-word byte on both sides.
type Shape = (&'static str, &'static [&'static str], &'static [Seg]);

#[rustfmt::skip]
const SHAPES: [Shape; 24] = [
    ("jwt", &["eyJ"], &[C(B64U, 8, MANY), L("."), L("eyJ"), C(B64U, 8, MANY), L("."), C(B64U, 8, MANY)]),
    ("anthropic_key", &["sk-ant-api03-", "sk-ant-admin01-"], &[C(B64U, 80, MANY)]),
    ("openrouter_key", &["sk-or-v1-"], &[C(LHEX, 64, 64)]),
    ("openai_key", &["sk-proj-", "sk-svcacct-", "sk-admin-", "sk-"], &[C(B64U, 20, MANY)]),
    ("stripe_key", &["sk_live_", "sk_test_", "rk_live_", "rk_test_"], &[C(AN, 20, MANY)]),
    ("github_token", &["ghp_", "gho_", "ghu_", "ghs_", "ghr_"], &[C(AN, 36, 36)]),
    ("github_token", &["github_pat_"], &[C(AN_, 60, MANY)]),
    ("gitlab_token", &["glpat-"], &[C(B64U, 20, 20)]),
    ("slack_token", &["xoxb-", "xoxa-", "xoxp-", "xoxr-", "xoxs-"], &[C("A-Za-z0-9-", 10, MANY)]),
    ("slack_webhook", &["https://hooks.slack.com/services/"], &[L("T"), C(UD, 1, MANY), L("/B"), C(UD, 1, MANY), L("/"), C(AN, 1, MANY)]),
    ("discord_webhook", &["https://discord.com/api/webhooks/", "https://discordapp.com/api/webhooks/"], &[C("0-9", 1, MANY), L("/"), C(B64U, 60, MANY)]),
    ("aws_key_id", &["AKIA", "ASIA"], &[C(UD, 16, 16)]),
    ("google_key", &["AIza"], &[C(B64U, 35, 35)]),
    ("hf_token", &["hf_"], &[C("A-Za-z", 34, 34)]),
    ("npm_token", &["npm_"], &[C(AN, 36, 36)]),
    ("pypi_token", &["pypi-AgEIcHlwaS5vcmc"], &[C(B64U, 50, MANY)]),
    ("sendgrid_key", &["SG."], &[C(B64U, 22, 22), L("."), C(B64U, 43, 43)]),
    ("telegram_token", &[""], &[C("0-9", 8, 10), L(":AA"), C(B64U, 33, 33)]),
    ("twilio_key", &["SK"], &[C(LHEX, 32, 32)]),
    ("do_token", &["dop_v1_"], &[C(LHEX, 64, 64)]),
    ("groq_key", &["gsk_"], &[C(AN, 52, 52)]),
    ("xai_key", &["xai-"], &[C(AN, 80, 80)]),
    ("shopify_token", &["shpat_", "shpss_", "shpca_", "shppa_"], &[C("0-9a-fA-F", 32, 32)]),
    ("age_key", &["AGE-SECRET-KEY-1"], &[C(UD, 58, 58)]),
];

// a static, not the const: a const is built again where it is used
static TABLE: [Shape; 24] = SHAPES;

/// The bytes a vendor token can start with, so the others are passed over at once.
static FIRST: [bool; 256] = {
    let (mut first, mut i) = ([false; 256], 0);
    while i < SHAPES.len() {
        let (lead, rest) = (SHAPES[i].1, SHAPES[i].2);
        let mut l = 0;
        while l < lead.len() {
            if !lead[l].is_empty() {
                first[lead[l].as_bytes()[0] as usize] = true;
            } else if let Some(C(class, ..)) = rest.first() {
                let mut b = 0;
                while b < 256 {
                    first[b] |= has(class, b as u8);
                    b += 1;
                }
            }
            l += 1;
        }
        i += 1;
    }
    first
};

/// How many bytes of `s` the segments take, when they all fit and a non-word byte (or the end) follows.
fn shape_len(s: &[u8], rest: &[Seg]) -> Option<usize> {
    let mut at = 0;
    for seg in rest {
        match *seg {
            L(lit) if s[at..].starts_with(lit.as_bytes()) => at += lit.len(),
            C(class, least, most) => {
                let n = s[at..].iter().take(most).take_while(|&&b| has(class, b)).count();
                if n < least {
                    return None;
                }
                at += n;
            }
            L(_) => return None,
        }
    }
    (!s.get(at).is_some_and(|&b| word(b))).then_some(at)
}

fn vendor(text: &str, hits: &mut Vec<Hit>) {
    let b = text.as_bytes();
    let mut i = 0;
    'token: while i < b.len() {
        if !FIRST[b[i] as usize] || (i > 0 && word(b[i - 1])) {
            i += 1;
            continue;
        }
        for (kind, lead, rest) in &TABLE {
            for l in *lead {
                let Some(n) = b[i..].starts_with(l.as_bytes()).then(|| shape_len(&b[i + l.len()..], rest)).flatten() else { continue };
                let to = i + l.len() + n;
                if !fake(&text[i..to]) {
                    let mut hit = Hit::new(i, to, kind);
                    (hit.prefix, hit.starts) = if *kind == "jwt" { (0, jwt_starts(&text[i..to])) } else { (l.len(), l.to_string()) };
                    hits.push(hit);
                }
                i = to;
                continue 'token;
            }
        }
        i += 1;
    }
}

/// Base64 (standard or URL alphabet, padding optional) to bytes.
fn b64_decode(s: &str) -> Option<Vec<u8>> {
    let (mut out, mut acc, mut bits) = (Vec::new(), 0u32, 0);
    for c in s.bytes().take_while(|&c| c != b'=') {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            _ => return None,
        };
        (acc, bits) = (acc << 6 | u32::from(v), bits + 6);
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// What of a JWT is public: the algorithm and the names of the claims, never a value.
fn jwt_starts(token: &str) -> String {
    let mut parts = token.split('.');
    let mut part = || parts.next().and_then(b64_decode).and_then(|b| serde_json::from_slice::<Json>(&b).ok());
    let (Some(head), Some(body)) = (part(), part()) else { return String::new() };
    let clean = |s: &str| s.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')).take(24).collect::<String>();
    let mut claims: Vec<String> = body.as_object().map(|o| o.keys().map(|k| clean(k)).collect()).unwrap_or_default();
    claims.truncate(12);
    format!("alg={};claims={}", head["alg"].as_str().map_or("?".into(), clean), claims.join(","))
}

/// The password of `scheme://user:password@host`.
fn url_password(text: &str, hits: &mut Vec<Hit>) {
    let b = text.as_bytes();
    let mut from = 0;
    while let Some(i) = text[from..].find("://") {
        let at = from + i + 3;
        from = at;
        let user = run(b, at, |c| !c.is_ascii_whitespace() && !b":/@\"'<>".contains(&c));
        let pass = run(b, user + 1, |c| !c.is_ascii_whitespace() && !b"@\"'<>/".contains(&c));
        let scheme = at >= 4 && (b[at - 4].is_ascii_lowercase() || b[at - 4].is_ascii_digit() || b"+.-".contains(&b[at - 4]));
        if scheme && user > at && b.get(user) == Some(&b':') && pass - user > 6 && b.get(pass) == Some(&b'@') {
            let secret = &text[user + 1..pass];
            if !reference(secret) && !fake(secret) {
                hits.push(Hit::new(user + 1, pass, "password"));
            }
        }
    }
}

/// `Authorization: Bearer X`, and `user:password` after `Authorization:`, `-u` or `--user`.
fn auth(text: &str, lower: &str, hits: &mut Vec<Hit>) {
    let b = text.as_bytes();
    for (i, _) in lower.match_indices("authorization") {
        let quote = |j: usize| j + usize::from(matches!(b.get(j), Some(b'"' | b'\'')));
        let j = skip_ws(b, quote(i + 13));
        if b.get(j) == Some(&b':') {
            credential(text, lower, quote(skip_ws(b, j + 1)), true, hits);
        }
    }
    for flag in ["-u", "--user"] {
        for (i, _) in lower.match_indices(flag) {
            if (i == 0 || b[i - 1].is_ascii_whitespace()) && b.get(i + flag.len()).is_some_and(u8::is_ascii_whitespace) {
                credential(text, lower, skip_ws(b, i + flag.len()), false, hits);
            }
        }
    }
}

fn credential(text: &str, lower: &str, at: usize, scheme: bool, hits: &mut Vec<Hit>) {
    let b = text.as_bytes();
    let scheme = ["bearer", "basic", "token"].into_iter().find(|s| scheme && lower[at..].starts_with(s) && b.get(at + s.len()).is_some_and(u8::is_ascii_whitespace));
    let from = scheme.map_or(at, |s| skip_ws(b, at + s.len()));
    let to = run(b, from, |c| c.is_ascii_alphanumeric() || b"._~+/=:@-".contains(&c));
    let value = &text[from..to];
    // without a scheme it is `user:password`
    if to - from < 12 || (scheme.is_none() && !value.find(':').is_some_and(|k| k > 0)) || reference(value) || fake(value) {
        return;
    }
    hits.push(Hit::new(from, to, match scheme {
        Some("bearer") => "bearer_credential",
        Some("basic") => "basic_credential",
        Some(_) => "token_credential",
        None => "auth_credential",
    }));
}

/// `;Password=…`, `;AccountKey=…` in a connection string, and `?sig=…`.
fn connection(text: &str, lower: &str, hits: &mut Vec<Hit>) {
    let b = text.as_bytes();
    for (key, kind, before) in [("accountkey=", "conn_secret", b';'), ("sharedaccesskey=", "conn_secret", b';'), ("password=", "password", b';'), ("pwd=", "password", b';'), ("sig=", "conn_secret", b'?')] {
        for (i, _) in lower.match_indices(key) {
            let prev = b[..i].iter().rev().find(|c| !c.is_ascii_whitespace());
            let (from, to) = (i + key.len(), run(b, i + key.len(), |c| !c.is_ascii_whitespace() && !b";&\"'".contains(&c)));
            if prev.is_some_and(|&p| p == before || (before == b'?' && p == b'&')) && to - from >= 6 && !reference(&text[from..to]) && !fake(&text[from..to]) {
                hits.push(Hit::new(from, to, kind));
            }
        }
    }
}

/// `--token VALUE`, `--password=VALUE` and the like.
fn flags(text: &str, lower: &str, hits: &mut Vec<Hit>) {
    const NAMES: [&str; 8] = ["password", "passwd", "token", "api-key", "apikey", "secret", "client-secret", "access-key"];
    let b = text.as_bytes();
    for (i, _) in lower.match_indices("--") {
        let rest = &lower[i + 2..];
        let Some(name) = NAMES.iter().find(|n| rest.starts_with(**n) && b.get(i + 2 + n.len()).is_some_and(|&c| c == b'=' || c.is_ascii_whitespace())) else { continue };
        let at = i + 2 + name.len();
        let at = if b[at] == b'=' { at + 1 } else { skip_ws(b, at) };
        let quoted = matches!(b.get(at), Some(b'"' | b'\''));
        let (from, to) = if quoted { (at + 1, b[at + 1..].iter().position(|&c| c == b[at]).map_or(at + 1, |k| at + 1 + k)) } else { (at, run(b, at, |c| !c.is_ascii_whitespace() && c != b'"' && c != b'\'')) };
        let value = &text[from..to];
        if to - from >= 6 && !reference(value) && !fake(value) && (quoted || classes(value) >= 2) {
            hits.push(Hit::new(from, to, "cli_secret"));
        }
    }
}

/// `name = value`, `name: value`, `"name": "value"`, `export NAME=value`, for every name in a line.
fn named(line: &str, base: usize, ctx: Ctx, hits: &mut Vec<Hit>) {
    let b = line.as_bytes();
    let mut start = Some(0);
    while let Some(s) = start {
        // a match cannot reach into an earlier one, and a name follows the line's start or one of ` \t{,(;`
        let from = assign_at(line, s, ctx).map_or(s, |(end, hit)| {
            hits.extend(hit.map(|(a, z, kind)| Hit::new(base + a, base + z, kind)));
            end
        });
        start = b[from..].iter().position(|&c| c.is_ascii_whitespace() || b"{,(;".contains(&c)).map(|k| from + k + 1);
    }
}

/// A longest value that [`inline`] looks at.
const INLINE_MAX: usize = 1024;

/// In a config file's text (what a remote server printed), `NAME=value` inside a sentence (`warning: API_TOKEN=abc… was used`): the scan above takes it for the
/// value of `warning:` and steps over it. Each `=` is looked at once, with the name before it and a value of at most [`INLINE_MAX`] bytes, so the work is linear.
fn inline(line: &str, base: usize, ctx: Ctx, hits: &mut Vec<Hit>) {
    let b = line.as_bytes();
    for (e, _) in line.match_indices('=') {
        let n0 = b[..e].iter().rposition(|&c| !(c.is_ascii_alphanumeric() || b"_.-".contains(&c))).map_or(0, |k| k + 1);
        if n0 == e || !(b[n0].is_ascii_alphabetic() || b[n0] == b'_') {
            continue;
        }
        let v0 = e + 1;
        let mut v1 = run(&b[..(v0 + INLINE_MAX).min(b.len())], v0, |c| !c.is_ascii_whitespace() && !b",;#}])(\"'".contains(&c));
        while !line.is_char_boundary(v1) {
            v1 -= 1;
        }
        if v1 > v0 && let Some(kind) = secret_value(&line[n0..e], &line[v0..v1], false, true, ctx) {
            hits.push(Hit::new(base + v0, base + v1, kind));
        }
    }
}

type Found = Option<(usize, usize, &'static str)>;

/// The assignment whose name starts at `s`: where it ends and, if its value is a secret, the value's range and kind.
fn assign_at(line: &str, s: usize, ctx: Ctx) -> Option<(usize, Found)> {
    let b = line.as_bytes();
    let mut j = s;
    if line[j..].starts_with("export") && b.get(j + 6).is_some_and(u8::is_ascii_whitespace) {
        j = skip_ws(b, j + 6);
    }
    let quote = b.get(j).copied().filter(|c| matches!(c, b'"' | b'\''));
    j += usize::from(quote.is_some());
    let n0 = j;
    if !b.get(j).is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_') {
        return None;
    }
    j = run(b, j, |c| c.is_ascii_alphanumeric() || b"_.-".contains(&c));
    let name = &line[n0..j];
    if quote.is_some() {
        (b.get(j) == quote.as_ref()).then_some(())?;
        j += 1;
    }
    j = skip_ws(b, j);
    let eq = b.get(j) == Some(&b'=');
    j += if line[j..].starts_with(":=") { 2 } else if matches!(b.get(j), Some(b'=' | b':')) { 1 } else { return None };
    j = skip_ws(b, j);
    let (v0, v1, end, quoted) = match *b.get(j)? {
        q @ (b'"' | b'\'') => {
            let mut k = j + 1;
            while k < b.len() && b[k] != q {
                k += if q == b'"' && b[k] == b'\\' { 2 } else { 1 };
            }
            (k < b.len()).then_some((j + 1, k, k + 1, true))?
        }
        _ => {
            let e = run(b, j, |c| !c.is_ascii_whitespace() && !b",;#}])(\"'".contains(&c));
            (e > j).then_some((j, e, e, false))?
        }
    };
    Some((end, secret_value(name, &line[v0..v1], quoted, eq, ctx).map(|kind| (v0, v1, kind))))
}

/// Whether `value` under `name` is a secret to hide, and what kind. `eq`: the sign was `=`.
fn secret_value(name: &str, value: &str, quoted: bool, eq: bool, ctx: Ctx) -> Option<&'static str> {
    // in the Log a bare `NAME=value` in the env style counts; in code only a quoted literal does
    let envish = ctx == Ctx::Unknown && eq && !name.bytes().any(|c| c.is_ascii_lowercase());
    // the name first: most lines are not about a secret, and it is the cheaper test
    let parts = name_parts(name);
    if (!quoted && ctx != Ctx::Config && !envish) || !secret_name(name, &parts, ctx == Ctx::Config) {
        return None;
    }
    if value.is_empty() || (value.len() <= 6 && value.bytes().all(|c| c.is_ascii_digit())) || reference(value) || fake(value) {
        return None;
    }
    let password = matches!(parts.last().map(String::as_str), Some("password" | "passwd" | "pwd" | "pass" | "passphrase"));
    // a token-like secret is one run of key characters; `registry` and `production` are not
    if !password && (value.len() < 8 || charset(value) == "text" || classes(value) < 2) {
        return None;
    }
    Some(if password { "password" } else { "secret" })
}

/// Whether a file at `path` is one where a bare `key`, `token` or `password` is a name of a secret and an unquoted value is a
/// literal: `.env*`, `.npmrc`, `credentials`, `config`, and the usual config extensions.
fn is_config(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    const NAMES: [&str; 5] = [".npmrc", ".netrc", ".pgpass", "credentials", "config"];
    const EXT: [&str; 12] = ["env", "ini", "cfg", "conf", "properties", "toml", "yaml", "yml", "json", "tfvars", "envrc", "log"];
    name.starts_with(".env") || NAMES.contains(&name.as_str()) || name.rsplit_once('.').is_some_and(|(_, e)| EXT.contains(&e))
}
