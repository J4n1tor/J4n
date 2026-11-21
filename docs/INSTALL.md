# Installing J4n

J4n is one binary, `j4n`. Its packs are compiled in. Everything it writes goes under `~/.j4n/` and under `.j4n/` in each
project you open.

## 1. The binary

**Prebuilt (Linux, macOS):**

```bash
curl -fsSL https://raw.githubusercontent.com/J4n1tor/J4n/main/install.sh | sh
```

Read the script before you pipe it to a shell: [install.sh](../install.sh). It is plain POSIX `sh` and runs under dash,
busybox ash, bash, zsh and macOS's `/bin/sh`. What it does, in order:

1. It picks the archive for your OS and CPU: `j4n-x86_64-unknown-linux-musl`, `j4n-aarch64-unknown-linux-musl`,
   `j4n-x86_64-apple-darwin` or `j4n-aarch64-apple-darwin`. A shell that runs under Rosetta 2 on Apple silicon says
   `x86_64`; the script asks macOS (`sysctl.proc_translated`) and installs the native arm64 build.
2. It downloads the archive and its `.sha256` from the latest GitHub release, over https only (TLS 1.2 or later), with
   `curl`, or with `wget` when there is no `curl`. If the latest full release has no archive for you, it reads the release
   list and takes the newest release, a pre-release included.
3. It checks the SHA-256 with `sha256sum`, `shasum` or `openssl`, whichever is there. On a mismatch nothing is installed.
4. It copies the binary into the install folder under a temporary name, checks that it runs there (`j4n --version`), and
   renames it over the old one. A `j4n` that is running keeps running; the next start is the new version.
5. It prints the installed version, the `PATH` line for your shell when the folder is not on `PATH`, the programs J4n uses
   that it did not find (section 4), and `j4n doctor` as the next step.

It changes no other file unless you ask with `J4N_MODIFY_PATH=1`. The variables:

| Variable | What it does |
|---|---|
| `J4N_BIN_DIR` | the install folder (default `~/.local/bin`); `/usr/local/bin` with `sudo` for every user |
| `J4N_VERSION` | a release tag such as `v0.1.0` (default: the latest release) |
| `J4N_MODIFY_PATH` | `1`: append the `PATH` line to your shell's start file, once (default: only print it) |
| `J4N_DOWNLOAD_URL` | a folder that holds `j4n-<target>.tar.gz` and its `.sha256`, used instead of GitHub: `https://`, `http://` or `file://` (a mirror, or archives you built) |
| `J4N_REPO` | the GitHub repository the releases come from (default `J4n1tor/J4n`), for a fork |

For example, to install archives built on another machine:

```bash
J4N_DOWNLOAD_URL=file:///home/user/dist sh install.sh
```

`scripts/release/install-test.sh <folder>` runs the script against such a folder in a scratch `HOME`: a fresh install, an
update over a running `j4n`, the refusals, and each shell it finds. The release workflow runs it on Linux, macOS and in
Alpine, Debian, Fedora and Arch containers before a release is published.

**The `PATH` line.** The script reads your login shell from `$SHELL`:

| Shell | Line | File |
|---|---|---|
| zsh (macOS's default) | `export PATH="$HOME/.local/bin:$PATH"` | `~/.zshrc` |
| bash on Linux | the same | `~/.bashrc` |
| bash on macOS | the same | `~/.bash_profile` (Terminal starts login shells) |
| fish | `fish_add_path $HOME/.local/bin` | kept by fish itself |
| any other | the export line | `~/.profile` |

**Per OS.**

- *Linux:* the binary is static (musl), so one build runs on every distribution, glibc or musl (Alpine): no library to
  install. It needs `tar` and `gzip` to unpack, which every distribution has.
- *macOS:* macOS 11 (Big Sur) or later. Gatekeeper is not involved: `curl` and `wget` do not mark a download as
  quarantined, so there is no "unidentified developer" prompt. (A copy downloaded with a browser would be; run
  `xattr -d com.apple.quarantine j4n` on it.) The Apple-silicon build is signed ad hoc by the linker, as macOS requires.
- *Windows:* inside WSL2, as Linux.

**When it fails.**

| Message | What it means |
|---|---|
| `has no release yet` | the repository has published no binary yet: build from source (below) |
| `the release list cannot be read from here` | GitHub cannot be reached (offline, a proxy, the API's hourly limit for anonymous calls) or the repository is private: build from source, or use `J4N_DOWNLOAD_URL` |
| `release vX.Y.Z has no j4n-….tar.gz` | that release has no build for your OS and CPU |
| `checksum mismatch` | the download is damaged or was changed on the way: nothing was installed; try again |
| `needs curl or wget` / `needs sha256sum, shasum or openssl` | install one of them with your package manager |
| `no prebuilt J4n for …` | an OS or CPU with no build (32-bit ARM, RISC-V, a BSD): build from source |
| `the downloaded j4n does not run on this machine` | the install folder may be mounted `noexec`: set `J4N_BIN_DIR` to another |
| `another j4n comes first on your PATH` | an older copy (for example from `cargo install`) is found before the new one: remove it |
| `j4n: command not found` after a successful install | the folder is not on `PATH` yet: add the line above, then open a new terminal |

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
the test stand. `⏎` opens **Agents › Setup**. `esc` puts it off, and the card asks again at the next start. Once you have
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

### Icons and fonts

The marks on the screen (the tabs, the states of a task, the gate) can be drawn as icons from a
[Nerd Font](https://www.nerdfonts.com), as plain Unicode, as ASCII or as words. The setting `theme.marks` is `auto` out
of the box: J4n draws icons when the terminal draws them itself (Ghostty, WezTerm, kitty) or a Nerd Font is installed,
and Unicode when neither is found. **Settings › Features** shows the row `theme.marks` with what `auto` chose and why.

An installed font is not proof that the terminal draws with it. If the marks show as boxes or blanks, pick `unicode` on
that row (`⏎`, or `n` for the next word), or say `:look marks unicode`. For the icons, install a Nerd Font and choose it
as the terminal's font, then pick `icons` or `auto`:

- Linux: install a Nerd Font from your distribution (Arch: `pacman -S ttf-jetbrains-mono-nerd`) or unpack one from
  nerdfonts.com into `~/.local/share/fonts` and run `fc-cache -f`. J4n looks in `~/.local/share/fonts`, `~/.fonts`, the
  `fonts` folder of each `XDG_DATA_DIRS` entry (`/usr/share/fonts`, `/usr/local/share/fonts`), then in `fc-list`.
- macOS: `brew install --cask font-jetbrains-mono-nerd-font`, then choose *JetBrainsMono Nerd Font* in the terminal's
  profile (Terminal.app: Settings › Profiles › Text › Font). J4n looks in `~/Library/Fonts`, `/Library/Fonts`,
  `/System/Library/Fonts` and Homebrew's `share/fonts`, then in `fc-list` when fontconfig is installed. Terminal.app
  draws no icons without such a font.
- Over SSH, the font is the one of the terminal on your own machine, and J4n can only look at the fonts of the machine
  it runs on (a Ghostty or kitty terminal still says its name through `TERM`): set `theme.marks` there if `auto` is wrong.
- `J4N_FONTS=icons` or `J4N_FONTS=unicode` in the environment says what `auto` is on one machine without looking, for
  example in the shell profile of a host you reach over SSH.

## 5. Check it

Agents › Setup is the check, on the screen. `j4n doctor` prints the same lines without opening it, each with the row of
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

If `J4N_MODIFY_PATH=1` added the `PATH` line, it is the line under `# J4n` in your shell's start file.

Each project's `.j4n/` folder holds that project's log. Remove it with the project or keep it.
