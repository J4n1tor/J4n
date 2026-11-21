//! A provider's key typed in Agents › Providers: `⏎` on a row that takes a key sends it once from the secret editor, as
//! `Intent::Key` (a `Secret`, which prints as `«secret»` and is wiped when it goes), and it comes here, not to the Change path: no Change,
//! no undo, no Grow row, and the Log's `key.set` names the connector and holds no value. `x` says `:key remove <connector>`, which the row
//! asks on the card first; it writes `key.removed`. No line, form or `Set` takes a key: what is typed on a line is kept in the Log.

use super::{Front, Lead, Said};
use crate::change::Actor;
use crate::keys::{self, Keys};
use crate::value::Value;
use j4n_proto::Secret;
use serde_json::json;

/// The page a key is typed on.
pub(crate) const PAGE: &str = "providers";

impl Front {
    /// The `key` of the row `id` of the kind `kind`, when it has one.
    fn key_of(&self, kind: &str, id: &str) -> Option<String> {
        self.screen().tables.rows(kind).into_iter().find_map(|r| match r {
            Value::Map(m) if matches!(m.get("id"), Some(Value::Str(i)) if i == id) => match m.get("key") {
                Some(Value::Str(k)) if !k.is_empty() => Some(k.clone()),
                _ => None,
            },
            _ => None,
        })
    }

    /// The key reference (`env:NAME`, `file:/path`) of a connector the screen knows, or of a provider that has no such connector; none when it takes no key.
    fn key_reference(&self, connector: &str) -> Option<String> {
        self.key_of("connector", connector).or_else(|| self.key_of("provider", connector))
    }

    /// The other connectors and providers that name the same key reference: the key of a provider is one key (`own` and `light` are both OpenRouter's),
    /// so what is typed on its row is kept for each of them, and for the provider itself, which a connector that is not on yet reads when it is.
    fn key_siblings(&self, connector: &str, reference: &str) -> Vec<String> {
        ["connector", "provider"]
            .into_iter()
            .flat_map(|kind| self.screen().tables.rows(kind))
            .filter_map(|r| match r {
                Value::Map(m) => match (m.get("id"), m.get("key")) {
                    (Some(Value::Str(id)), Some(Value::Str(k))) if k == reference && id != connector => Some(id.clone()),
                    _ => None,
                },
                _ => None,
            })
            .collect()
    }

    /// Whether what the Log keeps of a line is `:key …` and not the line: a `:key` line in any case, or one said to a chat
    /// (`@c4 :key …`), other than `:key` alone and `:key remove <connector>` for a connector the screen has or J4n keeps a key for. What
    /// follows the word may be a key typed where it does not go, and a key of no vendor's shape is not scrubbed.
    pub(super) fn key_masked(&self, text: &str) -> bool {
        let mut words = text.split_whitespace().peekable();
        if words.peek().is_some_and(|w| w.starts_with('@')) {
            words.next();
        }
        if !words.next().is_some_and(|w| matches!(w.to_ascii_lowercase().as_str(), ":key" | "/key")) {
            return false;
        }
        match (words.next(), words.next(), words.next()) {
            (None, ..) => false,
            (Some("remove"), Some(c), None) => self.key_reference(c).is_none() && !self.key_kept(c),
            _ => true,
        }
    }

    /// A line [`Front::key_masked`] holds back: the Log gets `:key …`, and the refusal leads to Agents › Providers without naming what was typed.
    pub(super) fn key_stray(&self, thread: &str, text: &str) -> Said {
        self.write(thread, "chat.say", Actor::Operator, json!({ "text": ":key …" }));
        if text.split_whitespace().any(|w| w == "remove") {
            return self.refuse_to(thread, "no connector by that name takes a key · ⏎ opens Agents › Providers".into(), Lead::open("type the key", PAGE));
        }
        self.key_said(thread, &[])
    }

    fn key_store(&self) -> Option<Keys> {
        self.cfg().keys.clone()
    }

    /// Where the connector's key comes from now, in its row's words (`set · env`, `set · J4n file`, `set · file`, `not set`, or `not used · …`
    /// for one that is there and refused); empty for a connector that takes no key.
    pub fn key_state(&self, connector: &str, reference: &str) -> String {
        if reference.is_empty() {
            return String::new();
        }
        let store = self.key_store();
        let own = keys::state(store.as_ref(), connector, reference);
        if own.starts_with("set") {
            return own.to_string();
        }
        // a key kept for the provider that names the same reference is the connector's too, as it reads it
        let theirs: Vec<&str> = self.screen().tables.rows("provider").into_iter().filter_map(|r| match r {
            Value::Map(m) => match (m.get("id"), m.get("key")) {
                (Some(Value::Str(id)), Some(Value::Str(k))) if k == reference && id != connector => Some(keys::state(store.as_ref(), id, "")),
                _ => None,
            },
            _ => None,
        }).collect();
        let pick = theirs.iter().find(|w| w.starts_with("set")).or_else(|| theirs.iter().find(|w| own == "not set" && **w != "not set"));
        pick.map_or(own, |w| *w).to_string()
    }

    /// Whether J4n keeps a key for the connector (its row then has `x`).
    pub fn key_kept(&self, connector: &str) -> bool {
        self.key_store().is_some_and(|k| k.has(connector))
    }

    /// Whether J4n keeps a key for the connector or for any that names the same reference, which its connector reads: the row's `x` removes them all.
    pub(super) fn key_kept_any(&self, connector: &str) -> bool {
        self.key_kept(connector) || self.key_reference(connector).is_some_and(|r| self.key_siblings(connector, &r).iter().any(|s| self.key_kept(s)))
    }

    /// `⏎` on a row of Agents › Providers, with what was typed: kept in J4n's own key file, and the Log told only that it was. The value
    /// is read from the `Secret` in place, never copied, and wiped when the intent goes. A connector beside it that names the same reference
    /// and could not be given the key is said by name. A Decider or model it makes ready is put its probe cases at once; one that is not on
    /// is said with what turns it on.
    pub(super) fn key_set(&self, thread: &str, connector: &str, value: &Secret) -> Said {
        let Some(keys) = self.key_store() else { return self.refuse(thread, "J4n keeps no keys in this run".into()) };
        let Some(reference) = self.key_reference(connector) else { return self.refuse(thread, format!("`{connector}` takes no key")) };
        let replaced = keys.has(connector);
        if let Err(why) = keys.set(self.0.log.redactor(), connector, value.expose()) {
            return self.refuse_to(thread, format!("the key of {connector} was not kept: {} · ⏎ types it again", unplaced(&why)), Lead::open("type the key", PAGE));
        }
        let missed: Vec<(String, String)> = self.key_siblings(connector, &reference).into_iter().filter_map(|other| keys.set(self.0.log.redactor(), &other, value.expose()).err().map(|why| (other, why))).collect();
        self.write(thread, keys::SET, Actor::Operator, json!({ "connector": connector }));
        let env = reference.strip_prefix("env:").filter(|name| std::env::var(name).is_ok_and(|v| !v.trim().is_empty()));
        let done = if replaced { "is replaced" } else { "is kept" };
        let given: Vec<String> = std::iter::once(connector.to_string()).chain(self.key_siblings(connector, &reference)).filter(|id| !missed.iter().any(|(m, _)| m == id)).collect();
        let live = env.is_none().then(|| self.key_live(thread, &given)).flatten();
        let (words, lead) = match (env, live, self.key_off(&given, &reference)) {
            (Some(name), ..) => (format!("the key of {connector} {done} · {name} from the environment is set as well and wins while it is"), None),
            (None, Some(live), _) => (format!("the key of {connector} {done} · {live} · x on its row removes it"), None),
            (None, None, Some((off, lead))) => (format!("the key of {connector} {done} and read at its next start · {off}"), Some(lead)),
            (None, None, None) => (format!("the key of {connector} {done} and read at its next start · x on its row removes it"), None),
        };
        match lead {
            Some(lead) => self.note_to(thread, &words, "warn", lead),
            None => self.note(thread, &words, "dim"),
        }
        if let Some((_, why)) = missed.first() {
            let them = missed.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>().join(", ");
            self.note_to(thread, &format!("the same key could not be kept for {them}: {} · ⏎ opens Agents › Providers to type it again", unplaced(why)), "warn", Lead::open("type the key", PAGE));
        }
        Said::Command("key".into())
    }

    /// The Deciders and models a key just typed makes ready, and what to say of them; none when it readies none. The cases they have not
    /// passed go to them at once, on a thread of their own (with a person at the screen no line waits for them), and each says how it did.
    fn key_live(&self, thread: &str, given: &[String]) -> Option<String> {
        let ready: Vec<String> = given.iter().flat_map(|id| [format!("decider:{id}"), format!("model:{id}")]).filter(|a| self.asks().registry.get(a).is_some()).collect();
        let labels: Vec<String> = ready.iter().map(|a| a.replacen("decider:", "the Decider ", 1).replacen("model:", "the model ", 1)).collect();
        let names = labels.join(" and ");
        let prober = self.asks().prober.clone().filter(|_| !ready.is_empty());
        let pairs: Vec<(String, String)> = prober.as_ref().map(|p| p.due(self.log().clock().now_ms()).into_iter().filter(|(a, _)| ready.contains(a)).collect()).unwrap_or_default();
        let Some(prober) = prober.filter(|_| !pairs.is_empty()) else { return (!ready.is_empty()).then(|| format!("{names} answers from the next question")) };
        let (front, thread) = (self.clone(), thread.to_string());
        std::thread::spawn(move || {
            let reports = prober.run(&pairs);
            for (a, name) in ready.iter().zip(labels) {
                let failed: Vec<&str> = reports.iter().filter(|r| r.answerer == *a && !r.passed).map(|r| r.point.as_str()).collect();
                if failed.is_empty() {
                    front.note(&thread, &format!("{name} passed its probe cases and answers from now"), "dim");
                } else {
                    front.note(&thread, &format!("{name} did not pass its probe cases for {}: it does not answer there until it does · :probe says why", failed.join(", ")), "warn");
                }
            }
        });
        Some(format!("the probe cases go to {names} now, and each answers once it has passed them"))
    }

    /// What turns on a connector the key is for that is not on: one that is off, or one whose pack is off. Either acts from the next start.
    fn key_off(&self, given: &[String], reference: &str) -> Option<(String, Lead)> {
        let screen = self.screen();
        let rows: Vec<(String, bool)> = screen.tables.rows("connector").into_iter().filter_map(|r| match r {
            Value::Map(m) => match m.get("id") {
                Some(Value::Str(id)) => Some((id.clone(), m.get("off") == Some(&Value::Bool(true)))),
                _ => None,
            },
            _ => None,
        }).collect();
        if let Some((id, _)) = rows.iter().find(|(id, off)| *off && given.contains(id)) {
            return Some((format!("{id} is off · ⏎ turns it on, from the next start"), Lead::run(&format!("turn on {id}"), "connector", &["on", id])));
        }
        let (pack, ids) = screen.packs.iter().filter(|p| !p.on).find_map(|p| {
            let ids: Vec<&str> = p.keyed.iter().filter(|(id, r)| r == reference && !rows.iter().any(|(c, _)| c == id)).map(|(id, _)| id.as_str()).collect();
            (!ids.is_empty()).then(|| (p.id.clone(), format!("{} {}", ids.join(" and "), if ids.len() == 1 { "is" } else { "are" })))
        })?;
        Some((format!("{ids} in the pack {pack}, which is off · ⏎ turns it on, from the next start"), Lead::run(&format!("turn on {pack}"), "pack", &["on", &pack])))
    }

    /// `:key …`: `remove <connector>` forgets a key J4n keeps (the row's `x`, after the card); a key itself is never taken from a line.
    pub(super) fn key_said(&self, thread: &str, args: &[String]) -> Said {
        let lead = || Lead::open("type the key", PAGE);
        match (args.first().map(String::as_str), args.get(1)) {
            (Some("remove"), Some(connector)) => {
                let Some(keys) = self.key_store() else { return self.refuse(thread, "J4n keeps no keys in this run".into()) };
                let mut any = false;
                for other in self.key_reference(connector).map(|r| self.key_siblings(connector, &r)).unwrap_or_default() {
                    any |= keys.remove(&other).unwrap_or(false);
                }
                match keys.remove(connector).map(|had| had || any) {
                    Ok(true) => {
                        self.write(thread, keys::REMOVED, Actor::Operator, json!({ "connector": connector }));
                        self.note(thread, &format!("the key J4n kept for {connector} is removed · ⏎ on its row in Agents › Providers types a new one"), "dim");
                        Said::Command("key".into())
                    }
                    Ok(false) => self.refuse_to(thread, format!("J4n keeps no key for {connector} · ⏎ opens Agents › Providers"), lead()),
                    Err(why) => self.refuse(thread, why),
                }
            }
            _ => self.refuse_to(thread, "a key is typed on its row, never on a line (a line is kept in the Log) · ⏎ opens Agents › Providers".into(), lead()),
        }
    }
}

/// Why a key was not kept, without the file it would have been kept in: the folder of J4n is not the operator's to know.
fn unplaced(why: &str) -> &str {
    why.rsplit_once(": ").filter(|(head, _)| head.contains('/')).map_or(why, |(_, tail)| tail)
}
