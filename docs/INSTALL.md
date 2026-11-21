# Installing J4n

J4n is one binary, `j4n`. Its packs are compiled in. Everything it writes goes under `~/.j4n/` and under `.j4n/` in each
project you open.

## 1. The binary

**Prebuilt (Linux, macOS):**

```bash
curl -fsSL https://raw.githubusercontent.com/J4n1tor/J4n/main/install.sh | sh
```

The script picks the build for your OS and CPU (`x86_64` or `aarch64`/`arm64`), checks its SHA-256 and installs it to
`~/.local/bin/j4n`. Two variables change that:

- `J4N_BIN_DIR`: the install folder;
- `J4N_VERSION`: a tag such as `v0.1.0` (the default is the latest release).

Read the script before you pipe it to a shell. It is short: [install.sh](../install.sh).

**From source (any Unix with a Rust toolchain):**

```bash
cargo install --git https://github.com/J4n1tor/J4n --locked j4n
# or, from a clone:
cargo build --release -p j4n --locked   # → target/release/j4n
```

`rust-toolchain.toml` pins the compiler. With `rustup` installed it is fetched automatically.

**Windows:** install [WSL2](https://learn.microsoft.com/windows/wsl/install) with a Linux distribution, then follow the
Linux steps inside it. Native Windows is not supported: J4n relies on Unix process groups, sockets and file modes.

## 2. An agent

J4n drives agent CLIs and needs at least one installed. You can sign it in from J4n's Setup page (section 3), or the
usual way:

| Agent | Command J4n looks for |
|---|---|
| Claude Code | `claude` |
| Codex | `codex` |
| Gemini CLI | `gemini` |
| OpenCode | `opencode` |
| Pi | `pi` |

Any other CLI can be added with `/connect <program>`. J4n never reads or stores an agent's login.

With no agent program at all, an OpenRouter key is enough: J4n's own agent uses it.

## 3. The first start: Setup

Start `j4n` in a project folder. The first time, J4n shows the **Setup card**, which says where the agent, the Decider and
the test stand. `⏎` opens **Connect › Setup**. `esc` puts it off, and the card asks again at the next start. Once you have
opened Setup from it, the card is not shown again, in this project or any other.

Setup is one page of steps, each done with keys:

1. **Agent**: the agent programs found on this machine, and whether each is signed in. `⏎` uses one. `s` starts the
   program's own sign-in and shows the link it gives. `r` looks again after you install one.
2. **Key**: where a part needs a key, you type it on its row. The key is masked while you type it, J4n keeps it in its own
   key store, and the row only ever says `set`.
3. **Model**: `⏎` picks one of the agent's models, or `default`.
4. **Decider** (optional, recommended): it answers the small questions so you are asked less. *Span through OpenRouter*
   needs one OpenRouter key, which also gives J4n's small model and its own agent. *Jev · System One* needs a TypeSafe key.
   `o` turns the Decider on for every project.
5. **Test**: `⏎` makes one short call to each part that is set. Each part's row says ok or what failed, with its time and
   its cost. A part that failed is the row that mends it.
6. **Custom**: any other program, a key of your own, and every setting.

`:setup` opens the page at any time. A line typed while no agent works, and the first row of a new folder, lead there too.

## 4. Optional tools

Each tool is needed only by the feature named next to it. `j4n doctor` says what is present.

| Tool | Feature | Arch | Debian / Ubuntu | Fedora | macOS (Homebrew) |
|---|---|---|---|---|---|
| git | checkpoints, worktrees, lanes | `pacman -S git` | `apt install git` | `dnf install git` | `xcode-select --install` |
| bubblewrap | `--place box`, sandboxed tasks | `pacman -S bubblewrap` | `apt install bubblewrap` | `dnf install bubblewrap` | not available |
| Python 3 | `kit` pack | `pacman -S python` | `apt install python3` | `dnf install python3` | `brew install python` |
| a Chromium-family browser | `accounts` pack | `pacman -S chromium` | `apt install chromium` | `dnf install chromium` | `brew install --cask chromium` (or Chrome, Brave, Edge) |
| tmux | `terminal` pack | `pacman -S tmux` | `apt install tmux` | `dnf install tmux` | `brew install tmux` |

### The browser for accounts

The `accounts` pack opens one browser window per account, each with its own profile folder and, optionally, its own
proxy. J4n does not download or bundle a browser. It uses the first one of these that it finds:

- Linux: Chromium (the real binary under `/usr/lib*/chromium*` first, then `chromium` / `chromium-browser` on the
  `PATH`), Google Chrome, Brave, Microsoft Edge;
- macOS: `Chromium.app`, `Google Chrome.app`, `Brave Browser.app` or `Microsoft Edge.app` in `/Applications`.

To use another Chromium-family browser, copy `packs/accounts/connectors/browser.toml` to
`~/.j4n/connectors/browser.toml` and put its path first in `candidates`. Firefox is not supported: the window relies on
Chromium's `--user-data-dir` and `--proxy-server` flags.

## 5. Check it

Connect › Setup is the check, on the screen. `j4n doctor` prints the same lines without opening it, each with the row of
Setup that mends it:

```bash
cd your-project
j4n doctor
```

## 6. Update and uninstall

Run the install command again to update. To uninstall:

```bash
rm ~/.local/bin/j4n     # the binary
rm -rf ~/.j4n           # settings, accounts, caches (optional)
```

Each project's `.j4n/` folder holds that project's log. Remove it with the project or keep it.
