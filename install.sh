#!/bin/sh
# install.sh — install the latest J4n release binary.
#
#   curl -fsSL https://raw.githubusercontent.com/J4n1tor/J4n/main/install.sh | sh
#
# Linux (x86_64, aarch64) and macOS (x86_64, arm64). On Windows, run it inside WSL2.
# Environment:
#   J4N_VERSION   a tag such as v0.1.0 (default: the latest release)
#   J4N_BIN_DIR   where the binary goes (default: ~/.local/bin)
set -eu

repo="J4n1tor/J4n"
bin_dir="${J4N_BIN_DIR:-$HOME/.local/bin}"
version="${J4N_VERSION:-latest}"

say() { printf 'j4n-install: %s\n' "$*"; }
die() { printf 'j4n-install: %s\n' "$*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "needs \`$1\`"; }

need uname
need tar
if command -v curl >/dev/null 2>&1; then
  fetch() { curl -fsSL "$1" -o "$2"; }
elif command -v wget >/dev/null 2>&1; then
  fetch() { wget -qO "$2" "$1"; }
else
  die "needs curl or wget"
fi

os=$(uname -s)
arch=$(uname -m)
case "$os" in
  Linux) os_part="unknown-linux-musl" ;;
  Darwin) os_part="apple-darwin" ;;
  MINGW* | MSYS* | CYGWIN*) die "native Windows is not supported; install WSL2 and run this inside it" ;;
  *) die "no prebuilt binary for $os; build from source: cargo install --git https://github.com/$repo --locked j4n" ;;
esac
case "$arch" in
  x86_64 | amd64) arch_part="x86_64" ;;
  aarch64 | arm64) arch_part="aarch64" ;;
  *) die "no prebuilt binary for $arch; build from source: cargo install --git https://github.com/$repo --locked j4n" ;;
esac
target="$arch_part-$os_part"

if [ "$version" = "latest" ]; then
  base="https://github.com/$repo/releases/latest/download"
else
  base="https://github.com/$repo/releases/download/$version"
fi
asset="j4n-$target.tar.gz"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM
say "downloading $asset ($version)"
fetch "$base/$asset" "$tmp/$asset" || die "download failed: $base/$asset"
fetch "$base/$asset.sha256" "$tmp/$asset.sha256" || die "checksum download failed"

want=$(cut -d' ' -f1 < "$tmp/$asset.sha256")
if command -v sha256sum >/dev/null 2>&1; then
  got=$(sha256sum "$tmp/$asset" | cut -d' ' -f1)
else
  got=$(shasum -a 256 "$tmp/$asset" | cut -d' ' -f1)
fi
[ "$want" = "$got" ] || die "checksum mismatch for $asset"

tar -xzf "$tmp/$asset" -C "$tmp"
mkdir -p "$bin_dir"
install -m 0755 "$tmp/j4n" "$bin_dir/j4n" 2>/dev/null || { cp "$tmp/j4n" "$bin_dir/j4n"; chmod 0755 "$bin_dir/j4n"; }
say "installed $bin_dir/j4n ($("$bin_dir/j4n" --version 2>/dev/null || echo "$version"))"

case ":$PATH:" in
  *":$bin_dir:"*) ;;
  *) say "add $bin_dir to your PATH, e.g.: echo 'export PATH=\"$bin_dir:\$PATH\"' >> ~/.profile" ;;
esac
say "next: cd your-project && j4n doctor"
