## What this changes and why

<!-- One short paragraph. Link the issue if there is one. -->

## How it was checked

- [ ] The tests of what I touched pass (`scripts/t test -p <crate> --test <target> <filter>`)
- [ ] `scripts/check.sh` is green on the committed tree (not needed for a documentation-only change; then `scripts/t test -p j4n --test laws`)
- [ ] A bug fix has a test that fails without the fix
- [ ] A change to a screen has its goldens re-written with `J4N_BLESS=1` and I read the diff
- [ ] A crate that grew has its ceiling raised in `fixtures/size-budget.toml`, with the reason in the commit message
- [ ] Words the user reads are short and plain, with no internal names or ids
- [ ] I am happy for this to be released under the MIT license
