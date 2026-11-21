---
name: j4n-checks
when: check, checks, gate, lint, clippy, fmt, verify, tests, test, build, Cargo.toml, *.rs, kind:write
where: crates/j4n-core/Cargo.toml
cost: 230
---
# Run this project's checks

Cargo is run only through `scripts/t`, which gives the run an empty home folder; a bare `cargo` is not used here.

- One crate: `scripts/t test -p j4n-core`. One test: `scripts/t test -p j4n-core brief_cases`.
- Goldens that a change meant to alter: `J4N_BLESS=1 scripts/t test -p j4n-tui`, then read each changed golden in `git diff`.
- The whole gate, in the background with its log in a file: `CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 scripts/check.sh`. It is green when its last line is `check: green`.
- On a shared machine, wait for a free build slot before a run: fewer than two cargo builds going, and some free memory.
- A test that fails under load is run again alone three times before it is called real.
- Say which checks ran and which did not.
