## Build
`cargo build --release`

## Run against a spec file
`./target/release/program-verify path/to/file.yml`

If you skip the `--spec-version` flag, the tool reads the `spec_version` field from the input
document and selects the matching schema from `version_map.yaml`.

- --------------------------------------------------------------------------------------------------------------------

### Use a custom schema
`./target/release/program-verify path/to/file.yml --schema custom_schema.json`

### Add the binary to PATH
The script below creates a symlink to `program-verify` and ensures `~/.local/bin` is appended
to `PATH` (by default it updates `~/.bashrc`):

```bash
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
```

Instead of copying the script, you can simply execute `./scripts/add.sh`.

If you use a different shell, replace `~/.bashrc` with the appropriate configuration file (e.g. `~/.zshrc`).
