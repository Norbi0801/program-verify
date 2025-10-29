## Build
`cargo build --release`

## Run against a spec file
`./target/release/program-verify path/to/file.yml`

If you skip the `--spec-version` flag, the tool reads the `spec_version` field from the input
document and selects the matching schema from `version_map.yaml`.

### Specification versions

`v3.0.0` introduces declarative phase contracts for mini-programs. Each phase can now describe its
inputs (including the explicit source of every payload: instance data, global context, or the output
port of another phase) and the outputs it produces. This allows tooling such as the `llm-compiler`
to reconstruct dependency graphs and prepare the minimal execution context for every step.

Specification `v4.0.0` introduces control-flow graphs, richer error handling, declarative output
composition, semantic metadata, and versioned artifacts. The draft schema lives in
[`schemas/v4.json`](schemas/v4.json); the feature roadmap remains documented in
[`docs/spec_v4.0.0_plan.md`](docs/spec_v4.0.0_plan.md).

Older documents can continue to target `v1.0.0`; both schemas are listed in `version_map.yaml`.

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
