#!/bin/sh
# install.sh: install a J4n release binary.
#
#   curl -fsSL https://raw.githubusercontent.com/J4n1tor/J4n/main/install.sh | sh
#
# Linux x86_64 and aarch64 (one static binary for every distribution, glibc or musl) and macOS arm64 and x86_64.
# On Windows, run it inside WSL2. Plain POSIX sh: dash, busybox ash, bash, zsh and macOS's /bin/sh all run it.
#
# Environment:
#   J4N_VERSION       a tag such as v0.1.0 (default: the latest release)
#   J4N_BIN_DIR       where the binary goes (default: ~/.local/bin)
#   J4N_MODIFY_PATH   1: add J4N_BIN_DIR to your shell's start file when it is not on PATH (default: print the line only)
#   J4N_DOWNLOAD_URL  a folder holding j4n-<target>.tar.gz and j4n-<target>.tar.gz.sha256, used instead of the GitHub
#                     release: https://, http:// or file:// (a mirror, or archives built by CI or by hand)
#   J4N_REPO          the GitHub repository the releases come from (default: J4n1tor/J4n), for a fork
#
# What it does: picks the archive for this OS and CPU, downloads it and its SHA-256 over https, checks the sum, checks that
# the new binary runs here, then moves it into place in one rename (a running j4n is not disturbed). It changes no other file
# unless J4N_MODIFY_PATH=1. The whole script is one function called on the last line, so a cut-off download runs nothing.
set -eu

repo="${J4N_REPO:-J4n1tor/J4n}"
github="${J4N_GITHUB:-https://github.com}"      # for tests: where release downloads are looked up
api="${J4N_GITHUB_API:-https://api.github.com}" # for tests: where the release list is read
from_source="cargo install --git https://github.com/$repo --locked j4n"
tmp=""
part=""

say() { printf 'j4n-install: %s\n' "$*"; }
warn() { printf 'j4n-install: %s\n' "$*" >&2; }
die() {
  printf 'j4n-install: error: %s\n' "$*" >&2
  exit 1
}
has() { command -v "$1" >/dev/null 2>&1; }
cleanup() {
  [ -z "$part" ] || rm -f "$part"
  [ -z "$tmp" ] || rm -rf "$tmp"
}

# fetch URL FILE: 0 when FILE holds the body, else non-zero. https only, unless the URL came from a J4N_* override.
fetch() {
  case "$1" in
    file://*)
      cp "${1#file://}" "$2" 2>/dev/null
      return
      ;;
    https://*) secure=1 ;;
    http://*) secure=0 ;;
    *) die "not a URL this script fetches: $1" ;;
  esac
  if has curl; then
    if [ "$secure" = 1 ]; then
      curl --proto '=https' --tlsv1.2 -fsSL --retry 3 --connect-timeout 20 -o "$2" "$1"
    else
      curl -fsSL --retry 3 --connect-timeout 20 -o "$2" "$1"
    fi
  elif has wget; then
    # GNU wget can be told https only; busybox wget takes few options, so it gets the plain form and a retry loop
    if wget --version 2>/dev/null | grep -q 'GNU Wget'; then
      if [ "$secure" = 1 ]; then
        wget -q --https-only --secure-protocol=TLSv1_2 --tries=3 --timeout=20 -O "$2" "$1"
      else
        wget -q --tries=3 --timeout=20 -O "$2" "$1"
      fi
    else
      n=0
      until wget -q -T 20 -O "$2" "$1" 2>"$tmp/wget.err"; do
        n=$((n + 1))
        # a missing file stays missing: only a failed connection is tried again
        if [ "$n" -ge 3 ] || grep -q ' 404 ' "$tmp/wget.err"; then
          cat "$tmp/wget.err" >&2
          return 1
        fi
        sleep 2
      done
    fi
  else
    die "needs curl or wget to download (install one of them, e.g. with your package manager)"
  fi
}

# sha256 FILE: prints the lower-case hex sum
sha256() {
  if has sha256sum; then
    sha256sum "$1" | cut -d' ' -f1
  elif has shasum; then
    shasum -a 256 "$1" | cut -d' ' -f1
  elif has openssl; then
    openssl dgst -sha256 "$1" | sed 's/^.*= *//'
  else
    return 1
  fi
}

# The newest release's tag, pre-releases included: releases/latest skips those, the release list does not.
newest_tag() {
  fetch "$api/repos/$repo/releases?per_page=1" "$tmp/releases.json" 2>/dev/null || return 2
  sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$tmp/releases.json" | head -n 1
}

no_release() {
  die "$1
  Build from source instead (a Rust toolchain is needed):
    $from_source
  or point J4N_DOWNLOAD_URL at a folder that holds $asset and $asset.sha256."
}

target_triple() {
  os=$(uname -s)
  arch=$(uname -m)
  case "$os" in
    Linux) os_part="unknown-linux-musl" ;;
    Darwin) os_part="apple-darwin" ;;
    MINGW* | MSYS* | CYGWIN* | Windows_NT) die "native Windows is not supported: install WSL2 and run this inside it" ;;
    *) die "there is no prebuilt J4n for $os. Build it from source (a Rust toolchain is needed): $from_source" ;;
  esac
  case "$arch" in
    x86_64 | amd64) arch_part="x86_64" ;;
    aarch64 | arm64) arch_part="aarch64" ;;
    *) die "there is no prebuilt J4n for $os on $arch. Build it from source (a Rust toolchain is needed): $from_source" ;;
  esac
  # a shell under Rosetta 2 on Apple silicon says x86_64; the native build is the one to run
  if [ "$os" = Darwin ] && [ "$arch_part" = x86_64 ] && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || true)" = 1 ]; then
    say "this shell runs under Rosetta 2: installing the native arm64 build" >&2
    arch_part="aarch64"
  fi
  echo "$arch_part-$os_part"
}

# path_advice DIR: how to put DIR on PATH for the user's login shell, or do it when J4N_MODIFY_PATH=1
path_advice() {
  dir="$1"
  case ":${PATH:-}:" in *":$dir:"*) return 0 ;; esac
  if [ -z "${HOME:-}" ]; then
    say "$dir is not on your PATH; add it: export PATH=\"$dir:\$PATH\""
    return 0
  fi
  shown="$dir"
  case "$dir" in "$HOME"/*) shown="\$HOME/${dir#"$HOME"/}" ;; esac
  shell_name=$(basename "${SHELL:-sh}")
  line="export PATH=\"$shown:\$PATH\""
  case "$shell_name" in
    zsh) rc="${ZDOTDIR:-$HOME}/.zshrc" ;;
    bash)
      # Terminal on macOS starts login shells, which read ~/.bash_profile and not ~/.bashrc
      if [ "$(uname -s)" = Darwin ]; then rc="$HOME/.bash_profile"; else rc="$HOME/.bashrc"; fi
      ;;
    fish)
      rc=""
      line="fish_add_path $shown"
      ;;
    *) rc="$HOME/.profile" ;;
  esac
  if [ "${J4N_MODIFY_PATH:-0}" = 1 ]; then
    if [ -z "$rc" ]; then
      if has fish && fish -c "fish_add_path '$dir'" </dev/null; then
        say "added $dir to fish's PATH (fish_add_path); open a new terminal to use it"
        return 0
      fi
      warn "could not run fish_add_path; run it yourself: $line"
      return 0
    fi
    if [ -f "$rc" ] && grep -qF "$line" "$rc"; then
      say "$rc already puts $dir on PATH; open a new terminal to use it"
    elif printf '\n# J4n\n%s\n' "$line" >>"$rc"; then
      say "added $dir to PATH in $rc; open a new terminal to use it"
    else
      warn "could not write $rc; add this line yourself: $line"
    fi
    return 0
  fi
  say "$dir is not on your PATH. For $shell_name, run this once, then open a new terminal:"
  if [ -z "$rc" ]; then
    printf '    %s\n' "$line"
  else
    rc_shown="$rc"
    # shellcheck disable=SC2088 # the ~ is shown to the reader, not expanded
    case "$rc" in "$HOME"/*) rc_shown="~/${rc#"$HOME"/}" ;; esac
    printf '    echo '\''%s'\'' >> %s\n' "$line" "$rc_shown"
  fi
  say "(or run the installer again with J4N_MODIFY_PATH=1 to have it added)"
}

# What J4n runs that this machine lacks: a report, never a failure (docs/INSTALL.md has the list and the packages).
report_needs() {
  os=$(uname -s)
  git_ok=0
  if has git; then
    git_ok=1
    # macOS has a /usr/bin/git stub that only asks to install the Command Line Tools
    if [ "$os" = Darwin ] && [ "$(command -v git)" = /usr/bin/git ] && ! xcode-select -p >/dev/null 2>&1; then git_ok=0; fi
  fi
  [ "$git_ok" = 1 ] || warn "missing: git (checkpoints, worktrees, lanes): $(git_hint "$os")"
  agents=""
  for a in claude codex gemini opencode pi; do
    if has "$a"; then agents="$agents $a"; fi
  done
  if [ -n "$agents" ]; then
    say "agent programs found:$agents"
  else
    warn "no agent program found (claude, codex, gemini, opencode or pi): install one, or give J4n an OpenRouter key on its Setup page"
  fi
  optional=""
  has tmux || optional="$optional tmux (terminal pack);"
  has python3 || optional="$optional python3 (kit pack);"
  if [ "$os" = Linux ]; then has bwrap || optional="$optional bubblewrap (sandboxed tasks, --place box);"; fi
  [ -z "$optional" ] || say "optional, not found:${optional%;}"
}

git_hint() {
  if [ "$1" = Darwin ]; then
    echo "xcode-select --install"
  elif has apt-get; then
    echo "sudo apt install git"
  elif has dnf; then
    echo "sudo dnf install git"
  elif has pacman; then
    echo "sudo pacman -S git"
  elif has apk; then
    echo "sudo apk add git"
  elif has zypper; then
    echo "sudo zypper install git"
  else
    echo "install it with your package manager"
  fi
}

main() {
  if [ -z "${J4N_BIN_DIR:-}" ] && [ -z "${HOME:-}" ]; then die "HOME is not set; set J4N_BIN_DIR to the folder the binary goes in"; fi
  bin_dir="${J4N_BIN_DIR:-$HOME/.local/bin}"
  version="${J4N_VERSION:-latest}"
  case "$version" in latest | v*) ;; *) version="v$version" ;; esac
  has uname || die "needs uname"
  has tar || die "needs tar to unpack the download"
  sha256 /dev/null >/dev/null 2>&1 || die "needs sha256sum, shasum or openssl to check the download"

  target=$(target_triple) || exit 1
  asset="j4n-$target.tar.gz"
  if [ -n "${J4N_DOWNLOAD_URL:-}" ]; then
    base="${J4N_DOWNLOAD_URL%/}"
    case "$base" in http://*) warn "J4N_DOWNLOAD_URL is plain http: the archive and its checksum are only as safe as that server" ;; esac
  elif [ "$version" = latest ]; then
    base="$github/$repo/releases/latest/download"
  else
    base="$github/$repo/releases/download/$version"
  fi

  tmp=$(mktemp -d 2>/dev/null || mktemp -d -t j4n-install)
  trap cleanup EXIT
  trap 'cleanup; exit 130' INT
  trap 'cleanup; exit 143' TERM

  say "downloading $asset ($version) from $base"
  if ! fetch "$base/$asset" "$tmp/$asset"; then
    [ -z "${J4N_DOWNLOAD_URL:-}" ] || die "$base/$asset could not be downloaded (is it there?)"
    [ "$version" = latest ] || die "release $version has no $asset, or $repo cannot be reached: see $github/$repo/releases"
    # releases/latest serves full releases only: the newest one may be a pre-release, or lack this target
    tag=$(newest_tag) || no_release "no full release of $repo has $asset, and the release list cannot be read from here: the
  repository is private or does not exist, or GitHub is not reachable (offline, a proxy, or the API's hourly limit)"
    [ -n "$tag" ] || no_release "$repo has no release yet ($github/$repo/releases)"
    base="$github/$repo/releases/download/$tag"
    version="$tag"
    say "the latest full release has no $asset; trying the newest release, $tag"
    fetch "$base/$asset" "$tmp/$asset" || die "release $tag has no $asset: see $github/$repo/releases"
  fi
  fetch "$base/$asset.sha256" "$tmp/$asset.sha256" || die "$base/$asset.sha256 could not be downloaded, so the archive cannot be checked"

  want=$(sed -n '1s/^\([0-9A-Fa-f]\{64\}\).*/\1/p' "$tmp/$asset.sha256" | tr 'A-F' 'a-f')
  [ -n "$want" ] || die "$asset.sha256 does not hold a SHA-256 sum"
  got=$(sha256 "$tmp/$asset" | tr 'A-F' 'a-f')
  [ "$want" = "$got" ] || die "checksum mismatch for $asset: expected $want, got $got. Nothing was installed."

  mkdir "$tmp/x"
  tar -xzf "$tmp/$asset" -C "$tmp/x" j4n 2>/dev/null || tar -xzf "$tmp/$asset" -C "$tmp/x" || die "could not unpack $asset (tar with gzip support is needed)"
  [ -f "$tmp/x/j4n" ] || die "$asset holds no j4n binary"

  mkdir -p "$bin_dir" || die "could not create $bin_dir"
  old=""
  [ ! -x "$bin_dir/j4n" ] || old=$("$bin_dir/j4n" --version </dev/null 2>/dev/null || true)
  # copy beside the target, then rename over it: a running j4n keeps its old file, where cp would fail ("Text file busy")
  # on Linux, and on macOS would leave a binary whose cached signature no longer matches
  part="$bin_dir/.j4n.part.$$"
  { cp "$tmp/x/j4n" "$part" && chmod 0755 "$part"; } || die "could not write in $bin_dir (set J4N_BIN_DIR to a folder you can write)"
  # run where it will live (a temp folder may be mounted noexec); an old binary stays when the new one does not run
  new=$("$part" --version </dev/null 2>&1) || die "the downloaded j4n does not run on this machine ($target): $new"
  mv -f "$part" "$bin_dir/j4n" || die "could not replace $bin_dir/j4n"
  part=""
  if [ -n "$old" ] && [ "$old" != "$new" ]; then
    say "updated $bin_dir/j4n: $old -> $new"
  else
    say "installed $bin_dir/j4n: $new"
  fi

  path_advice "$bin_dir"
  first=$(command -v j4n 2>/dev/null || true)
  if [ -n "$first" ] && [ "$first" != "$bin_dir/j4n" ]; then
    warn "another j4n comes first on your PATH: $first ($("$first" --version </dev/null 2>/dev/null || echo "?")); remove it or put $bin_dir before it"
  fi
  report_needs
  say "next: cd your-project && j4n doctor"
}

main "$@"
