#!/bin/sh
set -e
cd "$(dirname "$0")/.."

if ! command -v cargo >/dev/null 2>&1; then
    echo "error: cargo not found. Install Rust first: https://rustup.rs" >&2
    exit 1
fi

cargo install --path . --force
echo "Installed claude-user and cuser. Run 'cuser' to get started."
