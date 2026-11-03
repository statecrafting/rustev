"""Whether a pull request changes anything a CI job reads.

A pull request's own run uses this to leave out a job whose inputs it does
not touch; the merge queue and every push to main still run the job in full.
Any doubt (no base, a failed diff, unreadable metadata) answers "affected".

    python3 scripts/acceptance/affected.py --base SHA \
        [--package NAME]... [--all-packages] [--path PREFIX]...

Prints `true` or `false`. The inputs are the directories of the named
workspace packages and of every workspace package they depend on (any
dependency kind, path dependencies through `cargo metadata --no-deps`, so no
registry access), every path prefix given, and the workspace-wide inputs in
WORKSPACE_FILES.
"""
import argparse
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
# Files every Rust job reads, whatever the package.
WORKSPACE_FILES = ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "Makefile", ".cargo/")


def closure(metadata, roots, every):
    """Directories (relative to ROOT, with a trailing slash) of the roots and their path dependencies."""
    by_dir = {}
    for p in metadata["packages"]:
        by_dir[str(Path(p["manifest_path"]).resolve().parent)] = p
    by_name = {p["name"]: p for p in by_dir.values()}
    missing = [r for r in roots if r not in by_name]
    if missing:
        raise KeyError(f"not a workspace package: {', '.join(missing)}")
    todo = list(by_name.values()) if every else [by_name[r] for r in roots]
    seen = set()
    while todo:
        p = todo.pop()
        d = str(Path(p["manifest_path"]).resolve().parent)
        if d in seen:
            continue
        seen.add(d)
        for dep in p["dependencies"]:
            path = dep.get("path")
            if path and str(Path(path).resolve()) in by_dir:
                todo.append(by_dir[str(Path(path).resolve())])
    root = str(Path(metadata["workspace_root"]).resolve())
    return sorted(str(Path(d).relative_to(root)) + "/" for d in seen)


def affected(changed, prefixes):
    return any(f == p.rstrip("/") or f.startswith(p) for f in changed for p in prefixes)


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--base", default="")
    ap.add_argument("--package", action="append", default=[])
    ap.add_argument("--all-packages", action="store_true")
    ap.add_argument("--path", action="append", default=[])
    args = ap.parse_args(argv)
    try:
        if not args.base:
            raise ValueError("no base commit")
        meta = json.loads(subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--no-deps"],
            cwd=ROOT, check=True, capture_output=True, text=True).stdout)
        prefixes = closure(meta, args.package, args.all_packages) + list(args.path) + list(WORKSPACE_FILES)
        changed = subprocess.run(
            ["git", "diff", "--name-only", f"{args.base}...HEAD"],
            cwd=ROOT, check=True, capture_output=True, text=True).stdout.split()
    except Exception as e:  # any doubt runs the job
        print(f"affected: {e}; treating the change as affected", file=sys.stderr)
        print("true")
        return
    hit = sorted(f for f in changed if affected([f], prefixes))
    print(f"affected: {len(hit)} of {len(changed)} changed file(s) are inputs"
          + (f", first {hit[0]}" if hit else ""), file=sys.stderr)
    print("true" if hit else "false")


if __name__ == "__main__":
    main()
