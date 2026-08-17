#!/bin/sh
set -e

REPO="divyo-argha/claude-user"
BIN_DIR="${CUSER_INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${CUSER_VERSION:-latest}"

os=$(uname -s)
arch=$(uname -m)

case "$os" in
    Linux) platform="unknown-linux-gnu" ;;
    Darwin) platform="apple-darwin" ;;
    *)
        echo "error: unsupported OS: $os (this installer supports Linux and macOS)" >&2
        exit 1
        ;;
esac

case "$arch" in
    x86_64 | amd64) cpu="x86_64" ;;
    arm64 | aarch64) cpu="aarch64" ;;
    *)
        echo "error: unsupported architecture: $arch" >&2
        exit 1
        ;;
esac

target="${cpu}-${platform}"
asset="claude-user-${target}.tar.gz"

if [ "$VERSION" = "latest" ]; then
    url="https://github.com/${REPO}/releases/latest/download/${asset}"
else
    url="https://github.com/${REPO}/releases/download/${VERSION}/${asset}"
fi

tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT

echo "Downloading ${asset}..."
curl -fsSL "$url" -o "$tmpdir/$asset"
tar -xzf "$tmpdir/$asset" -C "$tmpdir"

mkdir -p "$BIN_DIR"
install -m 755 "$tmpdir/claude-user" "$BIN_DIR/claude-user"
install -m 755 "$tmpdir/cuser" "$BIN_DIR/cuser"

ORANGE='\033[38;5;208m'
SLATE='\033[38;5;244m'
GREEN='\033[1;32m'
BOLD='\033[1m'
NC='\033[0m'

printf "\n"
printf "${ORANGE}░█▀▀░█░░░█▀█░█░█░█▀▄░█▀▀${SLATE}░░░░░█░█░█▀▀░█▀▀░█▀▄${NC}\n"
printf "${ORANGE}░█░░░█░░░█▀█░█░█░█░█░█▀▀${SLATE}░▄▄▄░█░█░▀▀█░█▀▀░█▀▄${NC}\n"
printf "${ORANGE}░▀▀▀░▀▀▀░▀░▀░▀▀▀░▀▀░░▀▀▀${SLATE}░░░░░▀▀▀░▀▀▀░▀▀▀░▀░▀${NC}\n\n"

VERSION_STR=$("$BIN_DIR/cuser" --version 2>/dev/null | awk '{print $2}')
if [ -z "$VERSION_STR" ]; then
    VERSION_STR="0.1.0"
fi
printf "${GREEN}✓ Installed claude-user v${VERSION_STR} successfully!${NC}\n"
printf "  Location: ${BOLD}$BIN_DIR${NC}\n\n"

case ":$PATH:" in
    *":$BIN_DIR:"*)
        printf "Run ${BOLD}cuser${NC} to get started.\n"
        ;;
    *)
        printf "Note: ${BOLD}$BIN_DIR${NC} is not on your PATH.\n"
        printf "Please add it to your shell profile to run ${BOLD}cuser${NC}.\n"
        ;;
esac
