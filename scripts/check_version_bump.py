#!/usr/bin/env python3
"""Pre-commit hook ensuring Cargo.toml version is bumped relative to HEAD."""
from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

SEMVER_RE = re.compile(r"^(\d+)\.(\d+)\.(\d+)(?:[-+].*)?$")


def run_git(*args: str) -> str:
    try:
        result = subprocess.check_output(("git", *args), text=True)
    except subprocess.CalledProcessError as exc:
        raise SystemExit(exc.returncode)
    return result.strip()


def git_optional(*args: str) -> str | None:
    try:
        return subprocess.check_output(("git", *args), text=True)
    except subprocess.CalledProcessError:
        return None


def ensure_file_staged(path: Path, repo_root: Path) -> None:
    relative_path = path.resolve().relative_to(repo_root)
    staged = run_git("diff", "--cached", "--name-only", "--diff-filter=ACM")
    files = {line for line in staged.splitlines() if line}
    if str(relative_path) not in files:
        msg = (
            f"{path} nie jest dodany do commitu. Upewnij się, że plik jest w indeksie "
            "i zawiera podbitą wersję."
        )
        raise SystemExit(msg)


def extract_version(content: str) -> str:
    in_package = False
    for raw_line in content.splitlines():
        line = raw_line.strip()
        if line.startswith("[") and line.endswith("]"):
            in_package = line == "[package]"
            continue
        if not in_package:
            continue
        match = re.match(r"version\s*=\s*\"([^\"]+)\"", line)
        if match:
            return match.group(1)
    raise SystemExit("Nie udało się znaleźć pola version w sekcji [package] Cargo.toml")


def parse_semver(value: str) -> tuple[int, int, int]:
    match = SEMVER_RE.match(value)
    if not match:
        raise SystemExit(
            "Wartość version musi mieć format SemVer, np. 0.1.0 (opcjonalnie z sufiksem)."
        )
    return tuple(int(part) for part in match.groups())


def main() -> int:
    repo_root = Path(run_git("rev-parse", "--show-toplevel"))
    cargo_toml = repo_root / "Cargo.toml"

    ensure_file_staged(cargo_toml, repo_root)

    staged_content = git_optional("show", f":{cargo_toml.relative_to(repo_root)}")
    if staged_content is None:
        raise SystemExit("Brak staged wersji Cargo.toml. Użyj git add Cargo.toml.")
    staged_version = extract_version(staged_content)

    head_content = git_optional("show", f"HEAD:{cargo_toml.relative_to(repo_root)}")
    if head_content is None:
        # No previous commit, allow.
        return 0
    head_version = extract_version(head_content)

    new_semver = parse_semver(staged_version)
    old_semver = parse_semver(head_version)

    if new_semver <= old_semver:
        print(
            "Wersja w Cargo.toml musi zostać podbita względem HEAD. "
            f"Obecnie: {staged_version}, HEAD: {head_version}.",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
