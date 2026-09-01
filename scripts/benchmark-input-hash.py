#!/usr/bin/env python3
import argparse
import hashlib
import subprocess
from pathlib import Path

PATHS = (
    "Cargo.toml",
    "Cargo.lock",
    "benches/stream.rs",
    "src",
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
)


def git(root: Path, *args: str) -> bytes:
    return subprocess.run(
        ["git", "-C", str(root), *args],
        check=True,
        stdout=subprocess.PIPE,
    ).stdout


def tracked_files(root: Path) -> list[Path]:
    raw = git(root, "ls-files", "-z", "--", *PATHS)
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


def hash_worktree(root: Path) -> str:
    digest = hashlib.sha256()
    digest.update(b"streamdown-benchmark-worktree-v2\0")
    for path in tracked_files(root):
        add_file(digest, root, path)
    return digest.hexdigest()


def hash_commit(root: Path, commit: str) -> str:
    # ls-tree contains each tracked path, file mode, object type, and content-addressed
    # blob id. Hashing this record stream is enough to detect every benchmark input
    # change without checking the commit out into a second worktree.
    tree = git(root, "ls-tree", "-r", "-z", "--full-tree", commit, "--", *PATHS)
    if not tree:
        raise SystemExit(f"no benchmark inputs found in commit {commit}")
    digest = hashlib.sha256()
    digest.update(b"streamdown-benchmark-commit-v1\0")
    digest.update(tree)
    return digest.hexdigest()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("root", nargs="?", default=".", type=Path)
    parser.add_argument("--commit", help="hash tracked benchmark inputs from a Git commit")
    args = parser.parse_args()
    root = args.root.resolve()
    print(hash_commit(root, args.commit) if args.commit else hash_worktree(root))


if __name__ == "__main__":
    main()
