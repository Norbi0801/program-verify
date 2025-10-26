#!/usr/bin/env bash
set -euo pipefail

BIN_DIR="$HOME/.local/bin"
TARGET_BIN="$(pwd)/target/release/program-verify"
PROFILE="$HOME/.bashrc"
EXPORT_LINE='export PATH="$HOME/.local/bin:$PATH"'

if [ ! -x "$TARGET_BIN" ]; then
  echo "Build the project first: cargo build --release"
  exit 1
fi

mkdir -p "$BIN_DIR"
ln -sf "$TARGET_BIN" "$BIN_DIR/program-verify"

if [ -f "$PROFILE" ] && grep -Fqx "$EXPORT_LINE" "$PROFILE"; then
  echo "~/.local/bin is already present in $PROFILE"
else
  printf '\n%s\n' "$EXPORT_LINE" >> "$PROFILE"
  echo "Added PATH entry to $PROFILE"
fi

echo "Run: source \"$PROFILE\" or open a new shell to refresh PATH."
echo "After that you can call: program-verify --help"
