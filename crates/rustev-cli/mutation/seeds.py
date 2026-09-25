"""Seeded defects in the CLI host (spec 006, acceptance: a bounded negative
control).

Each seed rewrites exact text in a copy of the workspace, must still compile,
and must make the CLI's tests fail. The script exits non-zero if a seed's
anchor is missing (the seed would silently do nothing), if a seed does not
compile, if the tests pass with the defect in place, or if the tests do not
finish: a timeout is inconclusive, not a detection. The working tree is
never modified.
"""

import os
import shutil
import signal
import subprocess
import sys
import tempfile

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
S = "crates/rustev-cli/src/"
CLI = ["rustev-cli"]
# Spec 006, acceptance: every one needs at least one detected seed.
REQUIRED = [
    "adapter correctness",
    "byte caps",
    "exit-code mapping",
    "no-clobber",
    "scope flag handling",
    "sink acknowledgement",
    "store confinement",
    "unusable bundles",
]
# Generous: a new test binary can wait on the OS before it starts.
TEST_TIMEOUT_S = 1800

# (category, name, file, old, new, packages); `old` and `new` may be equal-
# length lists, for a defect that must remove more than one layer at once,
# and `file` then a list of the same length when the layers live in
# different files.
SEEDS = [
    (
        "unusable bundles",
        "a corrupt bundle is treated as absent (spec 015)",
        S + "eval.rs",
        "Err(e) => LoadFailure::new(BundleLoad::Corrupt, &e.to_string()).into(),",
        "Err(_) => continue,",
        CLI,
    ),
    (
        "unusable bundles",
        "an unreadable bundle stops the command (spec 015)",
        S + "eval.rs",
        "Err(ReadFail::File(e)) => LoadFailure::new(BundleLoad::Inaccessible, &e.detail).into(),",
        "Err(ReadFail::File(e)) => return Err(Done::io(command, e)),",
        CLI,
    ),
    (
        "unusable bundles",
        "an oversized bundle is reported as corrupt (spec 015)",
        S + "eval.rs",
        "                BundleLoad::Oversized,",
        "                BundleLoad::Corrupt,",
        CLI,
    ),
    (
        "unusable bundles",
        "fitting skips an unusable bundle as missing (spec 015)",
        S + "calibrate.rs",
        "bump(&mut skipped, f.kind.code());",
        'bump(&mut skipped, "bundle-missing");',
        CLI,
    ),
    (
        "scope flag handling",
        "a principal scope is accepted in tenant-only mode",
        S + "host.rs",
        'p.forbid(&["principal-scope"], "is not part of a tenant-only scope")?;',
        "",
        CLI,
    ),
    (
        "scope flag handling",
        "a missing principal scope downgrades to tenant-only",
        S + "host.rs",
        """            if !p.has("principal-scope") {
                return usage(""",
        """            if !p.has("principal-scope") {
                return Ok(Scope::tenant_only(tenant, revision));
            }
            if false {
                return usage(""",
        CLI,
    ),
    (
        "byte caps",
        "a file is read only to its limit",
        S + "io.rs",
        "self.read_bounded(path, limit.saturating_add(1))",
        "self.read_bounded(path, limit)",
        CLI,
    ),
    (
        "byte caps",
        "the aggregate budget admits one byte too many",
        S + "io.rs",
        "if bytes.len() > remaining {",
        "if bytes.len() > remaining + 1 {",
        CLI,
    ),
    (
        "byte caps",
        "one file too many is read",
        S + "deps.rs",
        "if n > max {",
        "if n > max + 1 {",
        CLI,
    ),
    (
        "no-clobber",
        "publishing renames over an existing file",
        S + "io.rs",
        "let linked = std::fs::hard_link(&temp, target);",
        "let linked = std::fs::rename(&temp, target);",
        CLI,
    ),
    (
        "no-clobber",
        "an existing output passes the pre-check",
        S + "io.rs",
        'Ok(_) => fail(path, "already exists; outputs are never overwritten"),',
        "Ok(_) => Ok(()),",
        CLI,
    ),
    (
        "exit-code mapping",
        "a rejection exits as a cancellation",
        S + "out.rs",
        "pub const REJECTED: u8 = 6;",
        "pub const REJECTED: u8 = 7;",
        CLI,
    ),
    (
        "exit-code mapping",
        "a bundle failure does not set the status",
        S + "run.rs",
        "                out = out.set(\"status\", status);\n",
        "",
        CLI,
    ),
    (
        "exit-code mapping",
        "a refusal category is misnamed",
        S + "plan.rs",
        'Category::NoCapableBackend => "no_capable_backend",',
        'Category::NoCapableBackend => "no_backend",',
        CLI,
    ),
    (
        "store confinement",
        "any reference names a store file",
        S + "replay.rs",
        "r.len() == 64 && r.bytes()",
        "!r.is_empty() || r.bytes()",
        CLI,
    ),
    (
        "store confinement",
        # The check alone is now backed by the no-follow open (3.3.1), so
        # removing only one of them is not observable.
        "a symbolic link in the store passes the check and is followed",
        S + "replay.rs",
        ["Ok(m) if m.file_type().is_file() => {}", "open_input(&path, true)"],
        ["Ok(_) => {}", "open_input(&path, false)"],
        CLI,
    ),
    (
        "adapter correctness",
        "an unmapped value is compared unmapped",
        S + "adapter.rs",
        "r.map.get(&value).cloned().or_else(|| r.otherwise.clone())",
        "r.map.get(&value).cloned().or(Some(value.clone()))",
        CLI,
    ),
    (
        "adapter correctness",
        "the last ranked candidate is selected",
        S + "adapter.rs",
        "r.entries.first().map(|e| e.candidate.clone())",
        "r.entries.last().map(|e| e.candidate.clone())",
        CLI,
    ),
    (
        "adapter correctness",
        "a gate ignores a changed adapter",
        # Two layers since spec 014: the CLI's byte comparison and the
        # library's rules precondition; removing one alone is equivalent.
        [S + "eval.rs", "crates/rustev-eval/src/report.rs"],
        [
            "let result = if badapter != cadapter {",
            '        (Some(x), Some(y)) if x != y => {\n            return unknown("the task adapter rules differ");\n        }\n',
        ],
        [
            "let result = if badapter.is_empty() && cadapter.is_empty() {",
            "",
        ],
        CLI,
    ),
    (
        "adapter correctness",
        "a fallback output is fitted as the primary's",
        S + "calibrate.rs",
        "if supply.target != 0 || *a != artifact =>",
        "if supply.target > 7 && *a != artifact =>",
        CLI,
    ),
    (
        "sink acknowledgement",
        "any existing record is acknowledged",
        S + "sink.rs",
        ".is_some_and(|existing| existing == bytes);",
        ".is_some();",
        CLI,
    ),
    (
        "sink acknowledgement",
        "the receipt names another digest",
        S + "sink.rs",
        '.map(|d| format!("file:{d}"))',
        '.map(|d| format!("file:{d}0"))',
        CLI,
    ),
    (
        "no-clobber",
        "compile skips the output pre-check",
        S + "plan.rs",
        "    io::ensure_absent(out).map_err(|e| Done::io(command, e))?;\n",
        "",
        CLI,
    ),
    (
        "no-clobber",
        "the temporary file is left behind",
        S + "io.rs",
        "    let _ = std::fs::remove_file(&temp);\n    match linked {",
        "    match linked {",
        CLI,
    ),
    (
        "byte caps",
        # The check alone is now backed by the non-blocking open (3.3.1).
        "a FIFO passes the check before opening and the open blocks",
        S + "io.rs",
        [
            "        match std::fs::metadata(path) {\n            Ok(m) if m.is_file() => {}",
            "let mut flags = libc::O_NONBLOCK;",
        ],
        ["        match std::fs::metadata(path) {\n            Ok(_) => {}", "let mut flags = 0;"],
        CLI,
    ),
    (
        "byte caps",
        "a FIFO swapped in after the check blocks the open",
        S + "io.rs",
        "let mut flags = libc::O_NONBLOCK;",
        "let mut flags = 0;",
        CLI,
    ),
    (
        "store confinement",
        "a symbolic link swapped into the store is followed",
        S + "io.rs",
        "            flags |= libc::O_NOFOLLOW;\n",
        "",
        CLI,
    ),
    (
        "exit-code mapping",
        "a missing dependency file is an input error",
        S + "plan.rs",
        "DepFail::Io(e) => Done::io(command, e),",
        "DepFail::Io(e) => Done::invalid(command, &e.path, e.detail),",
        CLI,
    ),
    (
        "exit-code mapping",
        "a recompile refusal names the wrong category",
        S + "plan.rs",
        '"category": r.category as u8,',
        '"category": r.category as u8 + 1,',
        CLI,
    ),
]


def run(cmd, cwd, env, timeout=None):
    # A session of its own, so a timeout kills the test binaries too.
    p = subprocess.Popen(
        cmd,
        cwd=cwd,
        env=env,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        start_new_session=True,
    )
    try:
        return p.wait(timeout=timeout)
    except subprocess.TimeoutExpired:
        os.killpg(p.pid, signal.SIGKILL)
        p.wait()
        return "timeout"


def main():
    work = tempfile.mkdtemp(prefix="rustev-cli-seeds-")
    env = dict(os.environ, CARGO_TARGET_DIR=os.path.join(work, "target"))
    src = os.path.join(work, "src")
    ignore = shutil.ignore_patterns("target", ".git", ".tooling", ".statecraft")
    shutil.copytree(ROOT, src, ignore=ignore)
    failures = []
    chosen = [x for x in SEEDS if not sys.argv[1:] or any(a in x[1] or a in x[0] for a in sys.argv[1:])]
    if not chosen:
        print("seeds: no seed matches the filter")
        sys.exit(1)
    try:
        for category, name, path, old, new, packages in chosen:
            label = f"{category}: {name}"
            base = ["cargo", "test", "--locked", "-q"] + [a for p in packages for a in ("-p", p)]
            # A multi-hunk seed may name one path per hunk, for a defect
            # whose layers live in different files.
            if isinstance(old, list):
                paths = path if isinstance(path, list) else [path] * len(old)
                hunks = list(zip(paths, old, new))
            else:
                hunks = [(path, old, new)]
            originals = {}
            for p in dict.fromkeys(h[0] for h in hunks):
                originals[p] = open(os.path.join(src, p), encoding="utf-8").read()
            bad = [p for p, o, _ in hunks if originals[p].count(o) != 1]
            if bad:
                failures.append(f"{label}: an anchor is not found exactly once in {bad[0]}")
                print(f"BROKEN   {label}: anchor", flush=True)
                continue
            seeded = dict(originals)
            for p, o, n in hunks:
                seeded[p] = seeded[p].replace(o, n)
            for p, text in seeded.items():
                open(os.path.join(src, p), "w", encoding="utf-8").write(text)
            try:
                if run(base + ["--no-run"], src, env) != 0:
                    failures.append(f"{label}: the seeded tree does not compile")
                    print(f"BROKEN   {label}: does not compile", flush=True)
                    continue
                rc = run(base, src, env, TEST_TIMEOUT_S)
                if rc == 0:
                    failures.append(f"{label}: survived (the tests passed with the defect)")
                    print(f"SURVIVED {label}", flush=True)
                elif rc == "timeout":
                    failures.append(f"{label}: inconclusive (the tests did not finish)")
                    print(f"TIMEOUT  {label}", flush=True)
                else:
                    print(f"DETECTED {label}", flush=True)
            finally:
                for p, text in originals.items():
                    open(os.path.join(src, p), "w", encoding="utf-8").write(text)
    finally:
        shutil.rmtree(work, ignore_errors=True)
    full = not sys.argv[1:]
    missing = [c for c in REQUIRED if full and not any(x[0] == c for x in SEEDS)]
    print(f"seeds: {len(chosen) - len(failures)}/{len(chosen)} detected")
    for c in missing:
        print(f"  no seed covers the required category {c!r}")
    if failures or missing:
        for x in failures:
            print(f"  {x}")
        sys.exit(1)


if __name__ == "__main__":
    main()
