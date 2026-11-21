//! Tier 0: the lab on eight fixed seeds with the scripted drafter ($0, no model), under a minute; the seeds
//! of the lab, its table and its scripted drafter; and the one place where the lab says yes to a Proposal.

use j4n_lab::{self as lab, CHECKS, Lab, Opts};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// The seeds of tier 0: fast ones (no 3–8 s start, no stall), and between them most ways to take a prompt and to print.
const TIER0: [u64; 8] = [0, 1, 2, 3, 4, 5, 6, 7];

fn j4n() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_j4n"))
}

fn forge() -> PathBuf {
    j4n().with_file_name("j4n-forge")
}

/// Where the harnesses are made: not under `/tmp`, which the box has of its own.
fn work() -> PathBuf {
    j4n().parent().and_then(Path::parent).map_or_else(|| scratch("work"), |t| t.join("lab-tier0"))
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("j4n-lab-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn seeds_are_a_range_up_to_the_end_an_inclusive_range_or_one_seed() {
    assert_eq!(lab::seeds("0..8"), Ok((0, 8)));
    assert_eq!(lab::seeds("3..=5"), Ok((3, 6)));
    assert_eq!(lab::seeds("7"), Ok((7, 8)));
    for bad in ["", "5..5", "8..2", "a..b", "1..2..3"] {
        assert!(lab::seeds(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn the_lab_says_yes_to_a_proposal_only_inside_the_folder_of_its_own_run() {
    let root = scratch("yes");
    let home = root.join("run-1/seed-3/home");
    std::fs::create_dir_all(&home).unwrap();
    assert!(lab::accepts_in(&home, &root.join("run-1")));
    // the run's own folder, a home that is not the lab's, a path that climbs out, and a link that leads out are all refused
    assert!(!lab::accepts_in(&root.join("run-1"), &root.join("run-1")));
    let other = scratch("yes-other");
    std::fs::create_dir_all(other.join("home")).unwrap();
    assert!(!lab::accepts_in(&other.join("home"), &root.join("run-1")));
    assert!(!lab::accepts_in(&home.join("../../../../yes-other/home"), &root.join("run-1")));
    std::fs::create_dir_all(root.join("run-1/seed-4")).unwrap();
    std::os::unix::fs::symlink(other.join("home"), root.join("run-1/seed-4/home")).unwrap();
    assert!(!lab::accepts_in(&root.join("run-1/seed-4/home"), &root.join("run-1")), "a link to a real home is not the lab's home");
    assert!(!lab::accepts_in(Path::new("/no/such/home"), &root.join("run-1")));
    assert!(!lab::accepts_in(&std::env::temp_dir(), &root.join("run-1")));
}

#[test]
fn the_scripted_drafter_reads_how_the_prompt_goes_from_the_usage_line_of_the_help() {
    let goal = |is: &str, usage: &str| {
        format!("Write a J4n connector for the program `/x/zorvik`.\n\nIt must have: `id`, `is = \"{is}\"`, `dialect = \"text\"`, `program = \"/x/zorvik\"`.\n\nIts help:\nzorvik - a harness\n\nUsage:\n  {usage}\n\nOptions:\n  --version\n\nIts manual:\n")
    };
    let agent = lab::draft(&goal("agent", "zorvik --ask TEXT"));
    assert!(agent.contains("is = \"agent\"") && agent.contains("start = [\"--ask\", \"{prompt}\"]"), "{agent}");
    let sub = lab::draft(&goal("agent", "zorvik chat send PROMPT"));
    assert!(sub.contains("start = [\"chat\", \"send\", \"{prompt}\"]"), "{sub}");
    let tool = lab::draft(&goal("tool", "zorvik [options] PROMPT"));
    assert!(tool.contains("argv = [\"/x/zorvik\", \"{text}\"]") && tool.contains("canary = { goal = \"Reply with the single word: ready\""), "{tool}");
    let none = lab::draft(&goal("agent", "zorvik"));
    assert!(none.contains("start = [\"{prompt}\"]"), "{none}");
}

#[test]
fn the_lab_runs_eight_fixed_seeds_with_the_scripted_drafter_and_scores_each_check() {
    assert!(forge().exists(), "build j4n-forge first (`cargo test --workspace` does): {}", forge().display());
    let out = scratch("tier0");
    let started = Instant::now();
    let opts = Opts { seeds: TIER0.to_vec(), drafter: "scripted".into(), jobs: 2, out: out.clone(), work: work(), run: "tier0".into(), j4n: j4n(), forge: forge(), free_mb: 600, swap_mb: 14_000, usd_cap: 0.5 };
    let done = Lab::new(opts).unwrap().run().0;
    let secs = started.elapsed().as_secs();
    // The clock here guards against a lab that has become far slower, not against a hung seed: a seed that hangs is killed at the
    // lab's own wall and shows as "still running" below. The bound is several times what the eight seeds take on a quiet
    // machine, so a loaded one does not fail it.
    assert!(secs < 240, "tier 0 took {secs} s");
    assert_eq!(done.len(), 8);
    for d in &done {
        assert_eq!(d.results.len(), CHECKS.len(), "seed {}: {:?}", d.seed, d.results);
        assert!(d.results.iter().all(|r| matches!(r.1, "pass" | "fail" | "na")), "{:?}", d.results);
        assert!(!d.results.iter().any(|r| r.2.contains("still running")), "seed {} hung: {:?}", d.seed, d.results);
        assert_eq!(d.usd, 0.0);
    }
    // a program J4n has never seen is tried and connects as an agent that chats; the one that asks for a login and the ones that cannot be told a
    // prompt are the few that do not
    let chats = done.iter().filter(|d| d.results.iter().any(|r| r.0 == "chats" && r.1 == "pass")).count();
    assert!(chats >= 5, "{chats} of 8 seeds chat: {:?}", done.iter().map(|d| (d.seed, d.results.iter().find(|r| r.0 == "chats").map(|r| r.2.clone()))).collect::<Vec<_>>());
    let rows = std::fs::read_to_string(out.join("tier0/rows.jsonl")).unwrap();
    assert!(rows.lines().filter(|l| l.contains("\"check\":\"connected\"")).count() >= 8, "{rows}");
    let table = std::fs::read_to_string(out.join("tier0/table.txt")).unwrap();
    assert!(table.contains("SEEN families") || table.contains("HELD-OUT families"), "{table}");
    assert!(table.contains("spent $0.00"), "a run of the scripted drafter costs nothing, and says so without a sign: {table}");
    // an agent that chats has a skill that says when to read it and how it is named, and the lab sees that
    for d in done.iter().filter(|d| d.results.iter().any(|r| r.0 == "chats" && r.1 == "pass")) {
        assert!(d.results.iter().any(|r| r.0 == "skill" && r.1 == "pass"), "seed {}: {:?}", d.seed, d.results);
    }
    let _ = std::fs::remove_dir_all(out);
}

#[test]
fn the_user_layer_reads_nothing_from_lab() {
    // the lab's rows live in `~/.j4n/lab/<run>/`; a seed's home with a `harness.toml` of its own must never be read as the operator's
    let home = scratch("layer");
    let seed = home.join("lab/run-1/seed-0/home");
    std::fs::create_dir_all(&seed).unwrap();
    std::fs::write(seed.join("harness.toml"), "api = 1\n[[enable]]\nid = \"connect\"\n").unwrap();
    std::fs::write(home.join("lab/run-1/seed-0/other.toml"), "api = 1\n").unwrap();
    let (layer, problems) = j4n_core::table::Layer::from_dir(j4n_core::table::LayerKind::User, "home", &home);
    assert!(layer.files.is_empty(), "{:?}", layer.files.iter().map(|f| &f.path).collect::<Vec<_>>());
    assert!(problems.is_empty(), "{problems:?}");
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn a_lab_with_its_forge_where_the_box_cannot_see_it_does_not_start_and_a_run_past_its_cap_starts_no_more_seeds() {
    let out = scratch("cap");
    let opts = |forge: PathBuf, usd_cap: f64| Opts { seeds: vec![0, 1], drafter: "scripted".into(), jobs: 1, out: out.clone(), work: work(), run: "cap".into(), j4n: j4n(), forge, free_mb: 600, swap_mb: 14_000, usd_cap };
    let why = Lab::new(opts(PathBuf::from("/tmp/j4n-lab-no-such-forge/j4n-forge"), 0.5)).err().expect("a forge under /tmp is refused");
    assert!(why.contains("the forge") && why.contains("hides"), "{why}");
    // a cap that is already reached before the first seed: nothing starts
    let done = Lab::new(opts(forge(), 0.0)).unwrap().run().0;
    assert!(done.is_empty(), "no seed started: {}", done.len());
}
