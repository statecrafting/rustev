"""Seeded defects in evidence quality (spec 032) and in provenance-conditional
rules and actionable outcomes (spec 033): each spec's acceptance names seeds
in the core mutation harness.

Each seed rewrites exact text in a copy of the workspace, must still compile,
and must make the named tests fail. The script exits non-zero if a seed's
anchor is missing (the seed would silently do nothing), if a seed does not
compile, or if the tests pass with the defect in place. The working tree is
never modified.
"""

import os
import shutil
import signal
import subprocess
import sys
import tempfile

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
CORE = "crates/rustev-core/src/"
EVAL = ["--test", "evidence_quality_eval"]
ACT = [
    "--lib",
    "--test",
    "actionability_compile",
    "--test",
    "actionability_eval",
    "--test",
    "actionability_origin",
    "--test",
    "actionability_rows",
]

# (name, file, old, new, cargo test arguments)
SEEDS = [
    (
        "grade combination takes the maximum",
        "crates/rustev-contract/src/grade.rs",
        "(Some(a), Some(b)) => a.min(b).into(),",
        "(Some(a), Some(b)) => a.max(b).into(),",
        EVAL,
    ),
    (
        "a merge falls back to base under overlay_only",
        CORE + "grading.rs",
        "None => absent(AbsentReason::OverlayMissing),",
        "None => match b {\n                    Some(x) => taken(MergeSide::Base, x),\n                    None => absent(AbsentReason::OverlayMissing),\n                },",
        ["--lib"] + EVAL,
    ),
    (
        "key existence counted as presence",
        CORE + "value.rs",
        "let is_null = |n: &str| matches!(out.get(n), Some(Value::Null | Value::Absent(_)));",
        "let is_null = |n: &str| out.get(n).is_none();",
        ["--lib"] + EVAL,
    ),
    (
        "a cap raises a ceiling",
        CORE + "quality.rs",
        "Some(c) => g.min(c),",
        "Some(c) => g.max(c),",
        ["--lib", "--test", "evidence_quality_compile"],
    ),
    (
        "a corroboration ignores check grades",
        CORE + "evaluate.rs",
        "&& meta.grade.graded().is_none_or(|g| g < decl.upgrade_to)",
        "&& false",
        EVAL,
    ),
    (
        "prefer chooses the last entry",
        CORE + "grading.rs",
        "let first = sources\n                    .iter()\n                    .find(",
        "let first = sources\n                    .iter()\n                    .rev()\n                    .find(",
        ["--lib"] + EVAL,
    ),
    (
        "a requirement ignores max_age_ms",
        CORE + "grading.rs",
        "        if elapsed > max {\n            return Err(Unresolved::stale([name]));",
        "        if false && elapsed > max {\n            return Err(Unresolved::stale([name]));",
        ["--lib"] + EVAL,
    ),
    # Spec 033.
    (
        "party combination prefers caller",
        "crates/rustev-contract/src/actionability.rs",
        "parties.into_iter().max().unwrap_or(Party::None)",
        "{\n            let v: Vec<Party> = parties.into_iter().collect();\n            if v.contains(&Party::Caller) {\n                Party::Caller\n            } else {\n                v.into_iter().max().unwrap_or(Party::None)\n            }\n        }",
        ACT,
    ),
    (
        "an override to caller is accepted",
        CORE + "actionable.rs",
        "            if o.to == Party::Caller {",
        "            if false && o.to == Party::Caller {",
        ACT,
    ),
    (
        "accepted causes ignore a non-caller class",
        CORE + "actionable.rs",
        "Party::combine(classes.iter().map(|c| class_party(*c, caller_controlled)))",
        "Party::combine(\n                    classes\n                        .iter()\n                        .filter(|c| caller_controlled.contains(*c))\n                        .map(|c| class_party(*c, caller_controlled)),\n                )",
        ACT,
    ),
    (
        "a dropped value is replaced by its entry",
        CORE + "evaluate.rs",
        "                    self.inputs_status.insert(\n                        c.subject.clone(),\n                        Err(Unresolved::missing([c.subject.clone()])),\n                    );\n                    self.input_values.remove(&c.subject);\n                    self.present.remove(&c.subject);\n",
        "",
        ACT,
    ),
    (
        "an unevaluable check is treated as passed",
        CORE + "evaluate.rs",
        "            let passed = match tested {\n                Err(u) => {",
        "            let passed = match tested {\n                Err(_) if c.id != \"\" => true,\n                Err(u) => {",
        ACT,
    ),
    (
        "a terminal check does not stop semantic requests",
        CORE + "evaluate.rs",
        "        if ev.checks.terminal.is_none() {\n            ev.advance();\n        }",
        "        ev.advance();",
        ACT,
    ),
    (
        "transient is true for invalid_input",
        CORE + "actionable.rs",
        "U::BackendUnavailable { .. } | U::DeadlineExceeded => true,",
        "U::BackendUnavailable { .. } | U::DeadlineExceeded | U::InvalidInput { .. } => true,",
        ACT,
    ),
]


# A seeded defect can make a test wait forever (the tests never sleep on
# wall time, so nothing else ends the wait). A test run that outlives this is
# a failure the harness would also report, and counts as detected.
TEST_TIMEOUT_S = 120


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
        # The group can be gone already if the test ended just now, or be
        # refused; the child itself is still killed and reaped.
        try:
            os.killpg(p.pid, signal.SIGKILL)
        except OSError:
            p.kill()
        p.wait()
        return "timeout"


def main():
    work = tempfile.mkdtemp(prefix="rustev-seeds-")
    env = dict(os.environ, CARGO_TARGET_DIR=os.path.join(work, "target"))
    src = os.path.join(work, "src")
    ignore = shutil.ignore_patterns("target", ".git", ".tooling", ".bin", ".statecraft")
    failures = []
    # Optional arguments select seeds by a substring of their name.
    chosen = [x for x in SEEDS if not sys.argv[1:] or any(a in x[0] for a in sys.argv[1:])]
    try:
        # One snapshot of the tree; each seed is applied to it and then
        # undone, so only the seeded crate rebuilds. Inside the `try`, so an
        # interrupted copy is removed too.
        shutil.copytree(ROOT, src, ignore=ignore)
        for name, path, old, new, args in chosen:
            f = os.path.join(src, path)
            with open(f, encoding="utf-8") as fh:
                original = fh.read()
            # Exactly one match: a missing anchor seeds nothing, a repeated
            # one would seed every occurrence.
            found = original.count(old)
            if found != 1:
                failures.append(f"{name}: anchor found {found} times in {path}")
                print(f"BROKEN   {name}: anchor found {found} times", flush=True)
                continue
            with open(f, "w", encoding="utf-8") as fh:
                fh.write(original.replace(old, new))
            try:
                pkg = [] if "-p" in args else ["-p", "rustev-core"]
                base = ["cargo", "test", "--locked", "-q"] + pkg + args
                if run(base + ["--no-run"], src, env) != 0:
                    failures.append(f"{name}: the seeded tree does not compile")
                    print(f"BROKEN   {name}: does not compile", flush=True)
                else:
                    rc = run(base, src, env, TEST_TIMEOUT_S)
                    if rc == 0:
                        failures.append(f"{name}: survived (the tests passed with the defect)")
                        print(f"SURVIVED {name}", flush=True)
                    elif rc == "timeout":
                        print(f"DETECTED {name} (a test never finished)", flush=True)
                    else:
                        print(f"DETECTED {name}", flush=True)
            finally:
                with open(f, "w", encoding="utf-8") as fh:
                    fh.write(original)
    finally:
        shutil.rmtree(work, ignore_errors=True)
    print(f"seeds: {len(chosen) - len(failures)}/{len(chosen)} detected")
    if failures:
        for x in failures:
            print(f"  {x}")
        sys.exit(1)


if __name__ == "__main__":
    main()
