//! J4n's own secrets are credentials: its key store, the redactor's salt, the browser profiles of the accounts, and the environment or memory of
//! a process under `/proc`. A doer's or a model's file-tool read of one is a block, which no grant opens, even where the row that lets a doer read
//! its own `.j4n` would say yes; the shell matcher and the doers' own deny lists name the same paths.

use crate::world::{layer, refused};
use j4n_core::change::Actor;
use j4n_core::gate::check::{Gates, check};
use j4n_core::gate::danger::danger;
use j4n_core::gate::foreign::Own;
use j4n_core::gate::{Action, Covers, Verb, Verdict};
use j4n_core::table::{LayerKind, Tables};
use std::path::Path;

const SITE: &str = "/home/me/Projects/app";

fn gates() -> Gates {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/base/gates");
    let files: Vec<(String, String)> = ["modes.toml", "paths.toml", "shell.toml", "egress.toml"].iter().map(|f| (f.to_string(), std::fs::read_to_string(dir.join(f)).unwrap())).collect();
    let refs: Vec<(&str, &str)> = files.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let (t, problems) = Tables::load(vec![layer(LayerKind::Pack, "gates", &refs)]);
    assert!(refused(&problems).is_empty(), "{problems:?}");
    Gates::load(&t).0
}

/// A grant for everything: a block is still a block.
struct Every;

impl Covers for Every {
    fn covers(&self, _: &Action) -> Option<String> {
        Some("every".into())
    }
}

/// A read by `who` working at `site`, the project being `root`, with every grant there is.
fn read(site: &str, root: &str, who: &Actor, path: &str) -> Verdict {
    let mut a = Action::new(Verb::Read, who.clone(), path);
    a.site = Some(site.into());
    a.state = Own { user: Some("/home/me".into()), j4n: Some("/home/me/.j4n".into()), root: Some(root.into()) };
    check(&gates(), &Every, &a)
}

#[test]
fn a_doers_or_a_models_read_of_j4ns_keys_salt_or_a_process_environment_is_a_block_no_grant_opens() {
    let rows = [
        ("/home/me/.j4n/keys/x", "j4n-keys"),
        ("/home/me/.j4n/keys", "j4n-keys"),
        ("/home/me/.j4n/secrets/typesafe", "j4n-keys"),
        ("/home/me/.j4n/accounts/work/browser/Default/Cookies", "j4n-keys"),
        ("/home/me/.j4n/guard.salt", "j4n-salt"),
        ("/proc/1/environ", "proc-of-others"),
        ("/proc/self/environ", "proc-of-others"),
        ("/proc/4242/mem", "proc-of-others"),
    ];
    for who in [Actor::Doer("claude".into()), Actor::Model("m".into())] {
        for (path, row) in rows {
            let v = read(SITE, SITE, &who, path);
            assert!(matches!(&v, Verdict::Block { by, .. } if by == row), "{who:?} {path}: {v:?}");
        }
        // J4n's own agent names a path from the project's folder: with the project in the home folder, `.j4n` is J4n's home
        for (path, row) in [(".j4n/keys/x", "j4n-keys"), (".j4n/keys", "j4n-keys"), (".j4n/guard.salt", "j4n-salt")] {
            let v = read("/home/me", "/home/me", &who, path);
            assert!(matches!(&v, Verdict::Block { by, .. } if by == row), "{who:?} {path}: {v:?}");
        }
    }
}

#[test]
fn the_own_state_a_doer_reads_stays_read_and_an_accounts_home_is_not_blocked_whole() {
    let doer = Actor::Doer("claude".into());
    for path in ["/home/me/Projects/app/.j4n/log/0001.jsonl", ".j4n/log/0001.jsonl", "/home/me/.j4n/accounts/work/home/settings.json"] {
        assert_eq!(read(SITE, SITE, &doer, path), Verdict::Allow { by: "own-state-is-read".into() }, "{path}");
    }
    // a file whose name only starts like the salt's is not it
    assert!(!matches!(read(SITE, SITE, &doer, "/home/me/.j4n/guard.salt.md"), Verdict::Block { .. }));
    // the operator reads what they like
    assert!(!matches!(read(SITE, SITE, &Actor::Operator, "/home/me/.j4n/keys/x"), Verdict::Block { .. }));
}

#[test]
fn the_shell_matcher_and_the_doors_name_the_keys_the_salt_and_every_process_environment() {
    let lines = ["cat ~/.j4n/guard.salt", "cat ~/.j4n/keys/own", "cat /proc/1/environ", "head -c 64 /proc/4242/mem", "cat /proc/1/environ{,}", "cat ${X:-/proc/1/environ}", "cat /proc/self/mem*"];
    for line in lines {
        assert_eq!(danger(line).class, "secret", "{line}");
    }
    assert_ne!(danger("cat /proc/meminfo").class, "secret");
    let denied = j4n_core::gate::door::denied_reads();
    for rule in ["Read(~/.j4n/keys/**)", "Read(~/.j4n/secrets/**)", "Read(~/.j4n/guard.salt)", "Read(//proc/**/environ)", "Read(//proc/**/mem)"] {
        assert!(denied.iter().any(|d| d == rule), "{rule} is denied: {denied:?}");
    }
}

/// A path is matched cleaned, by its text alone: a `..` or a doubled `/` takes no row's path past it, and a `..` inside the project comes to
/// the file it names.
#[test]
fn a_path_through_dot_dot_is_judged_where_it_comes_to() {
    let home = "/home/me";
    for who in [Actor::Doer("claude".into()), Actor::Model("m".into())] {
        for (site, path, row) in [
            (SITE, "/home/me/.j4n/log/../keys/x", "j4n-keys"),
            (SITE, "~/.j4n/log/../keys/x", "j4n-keys"),
            (SITE, "/home/me/.j4n//guard.salt", "j4n-salt"),
            (SITE, "/home/me/.j4n/skills/../guard.salt", "j4n-salt"),
            (home, "src/../.j4n/keys/x", "j4n-keys"),
            (home, ".j4n/log/./../guard.salt", "j4n-salt"),
            (SITE, "/proc/1/task/../environ", "proc-of-others"),
        ] {
            let v = read(site, site, &who, path);
            assert!(matches!(&v, Verdict::Block { by, .. } if by == row), "{who:?} {path}: {v:?}");
        }
    }
    let doer = Actor::Doer("claude".into());
    // a `..` inside the project is the file it comes to: no secret's name on the way counts, and the own Log is still the own Log
    assert!(!matches!(read(SITE, SITE, &doer, "/home/me/Projects/app/.env.d/../src/main.rs"), Verdict::Block { .. }));
    assert_eq!(read(SITE, SITE, &doer, "/home/me/Projects/app/src/../.j4n/log/0001.jsonl"), Verdict::Allow { by: "own-state-is-read".into() });
    for line in ["cat ~/.j4n/log/../keys/x", "cat ~/.j4n//guard.salt", "cat /home/me/.j4n/skills/../guard.salt"] {
        assert_eq!(danger(line).class, "secret", "{line}");
    }
}

#[test]
fn a_path_is_cleaned_by_its_text_alone() {
    use j4n_core::gate::clean_path;
    for (raw, clean) in [
        ("/a/b/../c", "/a/c"),
        ("/a//b/./c/", "/a/b/c"),
        ("/../a", "/a"),
        ("a/../../b", "../b"),
        ("../x/.j4n", "../x/.j4n"),
        ("~/../x", "~/../x"),
        ("~/a/../b", "~/b"),
        ("a/..", "."),
        ("/", "/"),
    ] {
        assert_eq!(clean_path(raw), clean, "{raw}");
    }
}
