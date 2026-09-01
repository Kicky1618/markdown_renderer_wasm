#!/usr/bin/env python3
import argparse
import hashlib
import subprocess
from pathlib import Path

SUITES = {
    "parser": (
        "Cargo.toml",
        "Cargo.lock",
        "benches/stream.rs",
        "src",
    ),
    "syntax": (
        "stream_mecab/Cargo.toml",
        "stream_mecab/src",
        "webapp/src/code.rs",
        "webapp/src/languages.rs",
        "webapp/src/japanese.rs",
        "webapp/langpacks",
        "tools/syntax-bench/Cargo.toml",
        "tools/syntax-bench/Cargo.lock",
        "tools/syntax-bench/build.rs",
        "tools/syntax-bench/src",
    ),
}


def suite_paths(suite: str) -> tuple[str, ...]:
    if suite == "all":
        return tuple(dict.fromkeys(SUITES["parser"] + SUITES["syntax"]))
    return SUITES[suite]


def git(root: Path, *args: str) -> bytes:
    return subprocess.run(
        ["git", "-C", str(root), *args],
        check=True,
        stdout=subprocess.PIPE,
    ).stdout


def tracked_files(root: Path, paths: tuple[str, ...]) -> list[Path]:
    raw = git(root, "ls-files", "-z", "--", *paths)
    names = [name for name in raw.split(b"\0") if name]
    if not names:
        raise SystemExit(f"no tracked benchmark inputs found under {root}")
    return [root / name.decode("utf-8") for name in names]


def add_file(digest, root: Path, path: Path) -> None:
    if not path.is_file():
        raise SystemExit(f"tracked benchmark input missing: {path}")
    rel = path.relative_to(root).as_posix().encode()
    data = path.read_bytes()
    digest.update(len(rel).to_bytes(4, "little"))
    digest.update(rel)
    digest.update(len(data).to_bytes(8, "little"))
    digest.update(data)


def hash_worktree(root: Path, suite: str) -> str:
    digest = hashlib.sha256()
    digest.update(b"streamdown-benchmark-worktree-v3\0")
    digest.update(suite.encode() + b"\0")
    for path in tracked_files(root, suite_paths(suite)):
        add_file(digest, root, path)
    return digest.hexdigest()


def hash_commit(root: Path, commit: str, suite: str) -> str:
    # ls-tree records path, mode, object type, and content-addressed blob id. This
    # detects every benchmark-relevant content change without a second checkout.
    tree = git(
        root,
        "ls-tree",
        "-r",
        "-z",
        "--full-tree",
        commit,
        "--",
        *suite_paths(suite),
    )
    if not tree:
        raise SystemExit(f"no {suite} benchmark inputs found in commit {commit}")
    digest = hashlib.sha256()
    digest.update(b"streamdown-benchmark-commit-v2\0")
    digest.update(suite.encode() + b"\0")
    digest.update(tree)
    return digest.hexdigest()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("root", nargs="?", default=".", type=Path)
    parser.add_argument("--commit", help="hash tracked benchmark inputs from a Git commit")
    parser.add_argument("--suite", choices=("all", "parser", "syntax"), default="all")
    args = parser.parse_args()
    root = args.root.resolve()
    if args.commit:
        print(hash_commit(root, args.commit, args.suite))
    else:
        print(hash_worktree(root, args.suite))


if __name__ == "__main__":
    main()
