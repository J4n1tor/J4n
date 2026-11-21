#!/bin/sh
# install-test.sh DIST: runs install.sh against the archives in DIST (j4n-<target>.tar.gz and its .sha256, named as the
# release workflow names them) through J4N_DOWNLOAD_URL, in a scratch HOME, with no GitHub release and no network:
#   - a fresh install, then the binary itself (--version, --help, doctor) with a dead proxy;
#   - an update over a j4n that is running;
#   - the refusals: a wrong checksum, a checksum file that holds none, a missing archive, no curl or wget, no sha tool;
#   - curl and wget over http, when python3 or busybox can serve DIST;
#   - the release lookup: latest missing, then the newest release from the release list (a file:// stand-in for GitHub);
#   - the target picked for Linux, macOS, Rosetta 2, an unsupported CPU and Windows (a stand-in uname);
#   - the PATH line for zsh, bash, fish and sh, and J4N_MODIFY_PATH=1 writing it once;
#   - a fresh install under every POSIX shell found here (sh, dash, bash, zsh, ksh, mksh, yash, busybox sh).
# Exit status: 0 when every check passed, 1 otherwise. POSIX sh; it writes only in a temp folder.
# A && ok || bad reads as a check here (ok never fails); $base_tools is a word list; the ~ and the $ in quotes are text to match
# shellcheck disable=SC2015,SC2086,SC2088,SC2016,SC2012
set -u

here=$(cd "$(dirname "$0")" && pwd)
installer="${INSTALL_SH:-$here/../../install.sh}"
[ $# -eq 1 ] || {
  echo "usage: install-test.sh DIST" >&2
  exit 2
}
dist=$(cd "$1" && pwd) || exit 2
w=$(mktemp -d 2>/dev/null || mktemp -d -t j4n-install-test)
trap 'chmod -R u+w "$w" 2>/dev/null; rm -rf "$w"' EXIT
pass=0
fail=0
os=$(uname -s)

ok() {
  pass=$((pass + 1))
  printf 'ok    %s\n' "$1"
}
bad() {
  fail=$((fail + 1))
  printf 'FAIL  %s\n' "$1"
  [ ! -s "$w/out" ] || sed 's/^/      | /' "$w/out"
}
skip() { printf 'skip  %s\n' "$1"; }
has() { command -v "$1" >/dev/null 2>&1; }
# check NAME: the last command's status ($rc) is 0, and $w/out holds each further argument
expect_ok() {
  name="$1"
  shift
  [ "$rc" -eq 0 ] || {
    bad "$name (exit $rc)"
    return
  }
  for want in "$@"; do grep -qF -- "$want" "$w/out" || {
    bad "$name (no \"$want\")"
    return
  }; done
  ok "$name"
}
# expect_no NAME TEXT…: the last command failed, and $w/out holds each TEXT
expect_no() {
  name="$1"
  shift
  [ "$rc" -ne 0 ] || {
    bad "$name (exit 0)"
    return
  }
  for want in "$@"; do grep -qF -- "$want" "$w/out" || {
    bad "$name (no \"$want\")"
    return
  }; done
  ok "$name"
}
# inst SHELL BIN [VAR=VALUE…]: install.sh under SHELL into BIN, output in $w/out, status in $rc
inst() {
  sh_cmd="$1"
  bin="$2"
  shift 2
  # shellcheck disable=SC2086 # the shell may be two words (busybox sh)
  env HOME="$w/home" ZDOTDIR= SHELL=/bin/sh J4N_DOWNLOAD_URL="file://$dist" J4N_BIN_DIR="$bin" J4N_VERSION= J4N_REPO= \
    J4N_MODIFY_PATH= J4N_GITHUB= J4N_GITHUB_API= "$@" $sh_cmd "$installer" >"$w/out" 2>&1 </dev/null
  rc=$?
}
# tools DIR NAME…: a PATH folder holding only these programs (those found here)
tools() {
  d="$1"
  shift
  mkdir -p "$d"
  for t in "$@"; do
    p=$(command -v "$t" 2>/dev/null) && [ -n "$p" ] && [ "${p#/}" != "$p" ] && ln -sf "$p" "$d/$t"
  done
}

mkdir -p "$w/home" "$w/proj"
ls "$dist"/j4n-*.tar.gz >/dev/null 2>&1 || {
  echo "no j4n-*.tar.gz in $dist" >&2
  exit 2
}

# --- a fresh install, and the binary itself
bin="$w/home/.local/bin"
inst sh "$bin" SHELL=/bin/zsh
expect_ok "fresh install" "installed $bin/j4n: j4n " ".zshrc" "next: cd your-project && j4n doctor"
"$bin/j4n" --version >"$w/out" 2>&1
rc=$?
expect_ok "j4n --version" "j4n "
"$bin/j4n" --help >"$w/out" 2>&1
rc=$?
expect_ok "j4n --help"
(cd "$w/proj" && env HOME="$w/home" PATH="$PATH" http_proxy=http://127.0.0.1:9 https_proxy=http://127.0.0.1:9 \
  HTTPS_PROXY=http://127.0.0.1:9 ALL_PROXY=http://127.0.0.1:9 "$bin/j4n" doctor >"$w/out" 2>&1 </dev/null)
rc=$?
# doctor's status says whether an agent is ready (none is here); a crash or a signal is >= 126
if [ "$rc" -lt 126 ] && [ -s "$w/out" ]; then ok "j4n doctor, no network (exit $rc, $(wc -l <"$w/out" | tr -d ' ') lines)"; else bad "j4n doctor (exit $rc)"; fi

# --- an update over a running binary: a rename, never a write into the busy file
cp "$bin/j4n" "$w/j4n.copy"
"$bin/j4n" relay -- sleep 30 >/dev/null 2>&1 &
busy=$!
sleep 1
if kill -0 "$busy" 2>/dev/null; then
  if [ "$os" = Linux ]; then
    if cp "$w/j4n.copy" "$bin/j4n" 2>"$w/out"; then echo "      (a plain cp onto the running binary did not fail here)"; else echo "      (a plain cp onto the running binary: $(cat "$w/out"))"; fi
  fi
  inst sh "$bin"
  expect_ok "update while j4n runs" "j4n"
  kill -0 "$busy" 2>/dev/null && ok "the running j4n kept running" || bad "the running j4n died"
  "$bin/j4n" --version >/dev/null 2>&1 && ok "the new binary runs" || bad "the new binary does not run"
  kill "$busy" 2>/dev/null
  wait "$busy" 2>/dev/null
else
  bad "j4n relay -- sleep 30 did not stay up"
fi
left=$(find "$bin" -name '.j4n.part*' | head -n 1)
[ -n "$left" ] && bad "a .j4n.part file was left in $bin" || ok "no part file left behind"

# --- refusals; each leaves the installed binary as it was
before=$("$bin/j4n" --version 2>&1)
mkdir -p "$w/badsum" "$w/nosum" "$w/empty"
for f in "$dist"/j4n-*.tar.gz; do
  cp "$f" "$w/badsum/"
  cp "$f" "$w/nosum/"
  echo "0000000000000000000000000000000000000000000000000000000000000000  ${f##*/}" >"$w/badsum/${f##*/}.sha256"
  echo "not a sum" >"$w/nosum/${f##*/}.sha256"
done
inst sh "$bin" J4N_DOWNLOAD_URL="file://$w/badsum"
expect_no "a wrong checksum is refused" "checksum mismatch" "Nothing was installed"
inst sh "$bin" J4N_DOWNLOAD_URL="file://$w/nosum"
expect_no "a checksum file without a sum is refused" "does not hold a SHA-256 sum"
inst sh "$bin" J4N_DOWNLOAD_URL="file://$w/empty"
expect_no "a missing archive is named" "could not be downloaded" "/j4n-"
[ "$("$bin/j4n" --version 2>&1)" = "$before" ] && ok "the refusals left the binary as it was" || bad "a refusal changed the binary"

# --- programs: no sha tool, then each sha tool alone; no curl or wget for an http URL
base_tools="sh uname tar gzip cut sed tr head mktemp cp mv rm chmod mkdir grep basename sleep cat env"
tools "$w/p-nosha" $base_tools
inst sh "$w/b-nosha" PATH="$w/p-nosha"
expect_no "no sha256sum, shasum or openssl: refused" "needs sha256sum, shasum or openssl"
for t in sha256sum shasum openssl; do
  if has "$t"; then
    tools "$w/p-$t" $base_tools "$t"
    inst sh "$w/b-$t" PATH="$w/p-$t"
    expect_ok "checksum with $t alone" "installed"
  else
    skip "checksum with $t alone ($t not here)"
  fi
done
tools "$w/p-nofetch" $base_tools sha256sum shasum openssl
inst sh "$w/b-nofetch" PATH="$w/p-nofetch" J4N_DOWNLOAD_URL="http://127.0.0.1:9"
expect_no "an http URL with no curl or wget: refused" "needs curl or wget"

# --- http, through curl and through wget, when DIST can be served
port=$((20000 + $$ % 20000))
server=""
if has python3; then
  python3 -m http.server --bind 127.0.0.1 --directory "$dist" "$port" >/dev/null 2>&1 &
  server=$!
elif has busybox && busybox --list 2>/dev/null | grep -qx httpd; then
  busybox httpd -f -p "127.0.0.1:$port" -h "$dist" &
  server=$!
elif has busybox-extras; then # Alpine keeps httpd in busybox-extras
  busybox-extras httpd -f -p "127.0.0.1:$port" -h "$dist" &
  server=$!
fi
if [ -n "$server" ]; then
  sleep 2
  url="http://127.0.0.1:$port"
  if has curl; then
    tools "$w/p-curl" $base_tools sha256sum shasum openssl curl
    inst sh "$w/b-curl" PATH="$w/p-curl" J4N_DOWNLOAD_URL="$url"
    expect_ok "download over http with curl" "installed" "plain http"
  else
    skip "download with curl (curl not here)"
  fi
  if has wget; then
    tools "$w/p-wget" $base_tools sha256sum shasum openssl wget
    inst sh "$w/b-wget" PATH="$w/p-wget" J4N_DOWNLOAD_URL="$url"
    expect_ok "download over http with wget ($(wget --version 2>/dev/null | grep -q 'GNU Wget' && echo GNU || echo busybox)), no curl" "installed"
    inst sh "$w/b-wget" PATH="$w/p-wget" J4N_DOWNLOAD_URL="$url/missing"
    expect_no "wget: a missing archive is named" "could not be downloaded"
  else
    skip "download with wget (wget not here)"
  fi
  kill "$server" 2>/dev/null
  wait "$server" 2>/dev/null
else
  skip "downloads over http (no python3 or busybox httpd here to serve them)"
fi

# --- the release lookup: releases/latest has no archive (a pre-release only), the release list names the newest
repo="J4n1tor/J4n"
gh="$w/gh"
mkdir -p "$gh/$repo/releases/download/v9.9.9-rc.1" "$w/api/repos/$repo"
cp "$dist"/j4n-*.tar.gz "$dist"/j4n-*.tar.gz.sha256 "$gh/$repo/releases/download/v9.9.9-rc.1/"
printf '[\n  {\n    "url": "x",\n    "tag_name": "v9.9.9-rc.1",\n    "prerelease": true\n  }\n]\n' >"$w/api/repos/$repo/releases?per_page=1"
inst sh "$w/b-gh" J4N_DOWNLOAD_URL= J4N_GITHUB="file://$gh" J4N_GITHUB_API="file://$w/api"
expect_ok "latest missing: the newest release is found in the list" "trying the newest release, v9.9.9-rc.1" "installed"
printf '[]\n' >"$w/api/repos/$repo/releases?per_page=1"
inst sh "$w/b-gh" J4N_DOWNLOAD_URL= J4N_GITHUB="file://$gh" J4N_GITHUB_API="file://$w/api"
expect_no "no release at all: said so" "has no release yet" "cargo install --git"
rm -f "$w/api/repos/$repo/releases?per_page=1"
inst sh "$w/b-gh" J4N_DOWNLOAD_URL= J4N_GITHUB="file://$gh" J4N_GITHUB_API="file://$w/api"
expect_no "the release list cannot be read (private, offline): said so" "cannot be read from here" "private" "cargo install --git"
inst sh "$w/b-gh" J4N_DOWNLOAD_URL= J4N_GITHUB="file://$gh" J4N_VERSION=0.0.1
expect_no "a pinned version that is not there: said so" "release v0.0.1 has no"

# --- the target: a stand-in uname (and sysctl) tells install.sh where it runs; the missing archive names the target
fake() { # fake DIR S M [TRANSLATED]
  mkdir -p "$1"
  printf '#!/bin/sh\ncase "$1" in -s) echo %s ;; -m) echo %s ;; *) echo %s ;; esac\n' "$2" "$3" "$2" >"$1/uname"
  printf '#!/bin/sh\necho %s\n' "${4:-0}" >"$1/sysctl"
  chmod +x "$1/uname" "$1/sysctl"
}
fake "$w/u-mac" Darwin arm64
inst sh "$w/b-u" PATH="$w/u-mac:$PATH" J4N_DOWNLOAD_URL="file://$w/empty"
expect_no "macOS arm64 asks for the aarch64-apple-darwin archive" "j4n-aarch64-apple-darwin.tar.gz"
fake "$w/u-intel" Darwin x86_64 0
inst sh "$w/b-u" PATH="$w/u-intel:$PATH" J4N_DOWNLOAD_URL="file://$w/empty"
expect_no "macOS Intel asks for the x86_64-apple-darwin archive" "j4n-x86_64-apple-darwin.tar.gz"
fake "$w/u-rosetta" Darwin x86_64 1
inst sh "$w/b-u" PATH="$w/u-rosetta:$PATH" J4N_DOWNLOAD_URL="file://$w/empty"
expect_no "a shell under Rosetta 2 gets the arm64 archive" "Rosetta 2" "j4n-aarch64-apple-darwin.tar.gz"
fake "$w/u-linux" Linux x86_64
inst sh "$w/b-u" PATH="$w/u-linux:$PATH" J4N_DOWNLOAD_URL="file://$w/empty"
expect_no "Linux x86_64 asks for the static musl archive" "j4n-x86_64-unknown-linux-musl.tar.gz"
fake "$w/u-riscv" Linux riscv64
inst sh "$w/b-u" PATH="$w/u-riscv:$PATH"
expect_no "an unsupported CPU gets the build-from-source line" "no prebuilt J4n for Linux on riscv64" "cargo install --git https://github.com/J4n1tor/J4n --locked j4n"
fake "$w/u-win" MINGW64_NT-10.0 x86_64
inst sh "$w/b-u" PATH="$w/u-win:$PATH"
expect_no "native Windows is sent to WSL2" "WSL2"

# --- the PATH line for each login shell, and J4N_MODIFY_PATH=1
fresh="$w/b-path"
inst sh "$fresh" SHELL=/usr/bin/zsh
expect_ok "PATH line for zsh" ">> ~/.zshrc" 'export PATH="'
inst sh "$fresh" SHELL=/bin/bash
if [ "$os" = Darwin ]; then expect_ok "PATH line for bash (macOS)" ">> ~/.bash_profile"; else expect_ok "PATH line for bash" ">> ~/.bashrc"; fi
inst sh "$fresh" SHELL=/usr/bin/fish
expect_ok "PATH line for fish" "fish_add_path"
inst sh "$fresh" SHELL=/bin/sh
expect_ok "PATH line for sh" ">> ~/.profile"
inst sh "$fresh" SHELL=/bin/zsh PATH="$fresh:$PATH"
grep -q "not on your PATH" "$w/out" && bad "no PATH line when the folder is on PATH" || ok "no PATH line when the folder is on PATH"
inst sh "$w/home/.local/bin" SHELL=/bin/zsh J4N_MODIFY_PATH=1
inst sh "$w/home/.local/bin" SHELL=/bin/zsh J4N_MODIFY_PATH=1
expect_ok "J4N_MODIFY_PATH=1 (run twice)" "already puts"
n=$(grep -c 'export PATH="$HOME/.local/bin:$PATH"' "$w/home/.zshrc" 2>/dev/null || echo 0)
[ "$n" = 1 ] && ok "~/.zshrc holds the line once" || bad "~/.zshrc holds the line $n times"
[ ! -e "$w/home/.profile" ] && [ ! -e "$w/home/.bashrc" ] && ok "no other start file was written" || bad "a start file was written without J4N_MODIFY_PATH"

# --- every POSIX shell found here
for s in sh dash bash zsh ksh mksh yash "busybox sh"; do
  first=${s%% *}
  if has "$first" && { [ "$first" != busybox ] || busybox sh -c true 2>/dev/null; }; then
    inst "$s" "$w/b-shell-$first"
    expect_ok "install under $s" "installed"
  else
    skip "install under $s (not here)"
  fi
done

echo "install-test: $pass passed, $fail failed ($(uname -s) $(uname -m), $(find "$dist" -name 'j4n-*.tar.gz' | wc -l | tr -d ' ') archives)"
[ "$fail" -eq 0 ]
