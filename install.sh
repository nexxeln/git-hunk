#!/bin/sh

set -eu

REPO="${GIT_HUNK_INSTALL_REPO:-nexxeln/git-hunk}"
VERSION="${GIT_HUNK_VERSION:-latest}"
BIN_DIR="${GIT_HUNK_INSTALL_DIR:-${HOME}/.local/bin}"
BIN_NAME="git-hunk"

need_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required command: $1" >&2
    exit 1
  fi
}

need_cmd curl
need_cmd tar
need_cmd mktemp

os=$(uname -s)
arch=$(uname -m)

case "$os" in
  Linux)
    case "$arch" in
      x86_64|amd64) target="x86_64-unknown-linux-gnu" ;;
      *)
        echo "unsupported Linux architecture: $arch" >&2
        exit 1
        ;;
    esac
    ;;
  Darwin)
    case "$arch" in
      arm64|aarch64) target="aarch64-apple-darwin" ;;
      x86_64) target="x86_64-apple-darwin" ;;
      *)
        echo "unsupported macOS architecture: $arch" >&2
        exit 1
        ;;
    esac
    ;;
  *)
    echo "unsupported operating system: $os" >&2
    exit 1
    ;;
esac

asset="${BIN_NAME}-${target}.tar.gz"

if [ "$VERSION" = "latest" ]; then
  url="https://github.com/${REPO}/releases/latest/download/${asset}"
else
  url="https://github.com/${REPO}/releases/download/v${VERSION}/${asset}"
fi

tmpdir=$(mktemp -d)
cleanup() {
  rm -rf "$tmpdir"
}
trap cleanup EXIT INT TERM

archive="$tmpdir/$asset"

echo "downloading $url"
curl -fsSL "$url" -o "$archive"

mkdir -p "$BIN_DIR"
tar -xzf "$archive" -C "$tmpdir"
install -m 755 "$tmpdir/$BIN_NAME" "$BIN_DIR/$BIN_NAME"

echo "installed $BIN_NAME to $BIN_DIR/$BIN_NAME"

case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *)
    echo "warning: $BIN_DIR is not on PATH" >&2
    ;;
esac
