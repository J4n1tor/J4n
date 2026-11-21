# Thanks

J4n stands on other people's work. These are the projects it is built with, the agents it drives, and the services it
can talk to. Thank you to everyone who makes and keeps them.

## The screen

- [ratatui](https://github.com/ratatui/ratatui): every frame J4n draws is a ratatui frame.
- [crossterm](https://github.com/crossterm-rs/crossterm): the terminal underneath, keys, mouse and raw mode.
- [unicode-width](https://github.com/unicode-rs/unicode-width): how many cells a character takes, so rows line up.
- [pulldown-cmark](https://github.com/raphlinus/pulldown-cmark): the Markdown of an agent's answer, read as it is.
- [dither-kit-rs](https://github.com/DeepanshuMishraa/dither-kit-rs): the dithered charts; J4n's half-block paint
  follows `dither-ratatui`'s.

## The core

- [serde](https://github.com/serde-rs/serde) and [serde_json](https://github.com/serde-rs/json): every event, frame and
  message between the core and the screen.
- [toml](https://github.com/toml-rs/toml) and toml_edit: the packs and settings, edited without losing your comments.
- [rusqlite](https://github.com/rusqlite/rusqlite) and [SQLite](https://sqlite.org): the memory.
- [regex](https://github.com/rust-lang/regex): the gate's rules and the redactor.
- [RustCrypto](https://github.com/RustCrypto) (`sha2`, `zeroize`): the hashes behind the Log and the redactor, and keys wiped from memory once used.
- [ureq](https://github.com/algesten/ureq) and [url](https://github.com/servo/rust-url): the few HTTP calls J4n makes.
- [Rust](https://www.rust-lang.org) itself, and every crate below these. The full list, with licenses, is in
  [THIRD-PARTY.md](THIRD-PARTY.md).

## The agents J4n works with

J4n is a layer over agents you already use. It would be nothing without them:
[Claude Code](https://github.com/anthropics/claude-code), [Codex](https://github.com/openai/codex),
[Gemini CLI](https://github.com/google-gemini/gemini-cli), [OpenCode](https://github.com/anomalyco/opencode),
[Pi](https://github.com/earendil-works/pi) and [oh-my-pi](https://github.com/can1357/oh-my-pi).

## Tools J4n runs

- [bubblewrap](https://github.com/containers/bubblewrap): the box a task can run in.
- [git](https://git-scm.com): checkpoints, worktrees and every diff J4n shows.
- [tmux](https://github.com/tmux/tmux): the terminal connector.
- [gitleaks](https://github.com/gitleaks/gitleaks): the secret scan this repository runs on every push.

## Services J4n can use

[TypeSafe](https://typesafe.ai) (the System One Decider), Respan's Span-01 and [OpenRouter](https://openrouter.ai). The
crew pack's practice tasks come from [Terminal-Bench](https://www.tbench.ai).

## The look

The logo is set in [Rubik Mono One](https://fonts.google.com/specimen/Rubik+Mono+One), and the brand pictures use
[JetBrains Mono](https://www.jetbrains.com/lp/mono/), both under the SIL Open Font License. The icons on the screen are
[Material Design Icons](https://pictogrammers.com/library/mdi/) as patched into
[Nerd Fonts](https://www.nerdfonts.com).
