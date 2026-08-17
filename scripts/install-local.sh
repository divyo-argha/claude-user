#!/bin/sh
set -e
cd "$(dirname "$0")/.."

if ! command -v cargo >/dev/null 2>&1; then
    echo "error: cargo not found. Install Rust first: https://rustup.rs" >&2
    exit 1
fi

cargo install --path . --force

ORANGE='\033[38;5;208m'
SLATE='\033[38;5;244m'
GREEN='\033[1;32m'
BOLD='\033[1m'
NC='\033[0m'

printf "${ORANGE}░█▀▀░█░░░█▀█░█░█░█▀▄░█▀▀${SLATE}░░░░░█░█░█▀▀░█▀▀░█▀▄${NC}\n"
printf "${ORANGE}░█░░░█░░░█▀█░█░█░█░█░█▀▀${SLATE}░▄▄▄░█░█░▀▀█░█▀▀░█▀▄${NC}\n"
printf "${ORANGE}░▀▀▀░▀▀▀░▀░▀░▀▀▀░▀▀░░▀▀▀${SLATE}░░░░░▀▀▀░▀▀▀░▀▀▀░▀░▀${NC}\n\n"

VERSION_STR=$(grep -m1 '^version = ' Cargo.toml | cut -d '"' -f2)
printf "${GREEN}✓ Installed claude-user v${VERSION_STR} successfully!${NC}\n"
printf "Run ${BOLD}cuser${NC} to get started.\n"
