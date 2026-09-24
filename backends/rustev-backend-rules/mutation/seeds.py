"""Seeded defects in the rules backend's critical guarantees (spec 005,
acceptance: a bounded negative control).

Each seed rewrites exact text in a copy of the workspace, must still compile,
and must make the backend's tests fail. The script exits non-zero if a seed's
anchor is missing (the seed would silently do nothing), if a seed does not
compile, if the tests pass with the defect in place, or if the tests do not
finish: the backend never waits, so a timeout is inconclusive, not a
detection. The working tree is never modified.
"""

import os
import shutil
import signal
import subprocess
import sys
import tempfile

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
B = "backends/rustev-backend-rules/src/"
# Generous: a new test binary can wait on the OS before it starts.
TEST_TIMEOUT_S = 1800

# (name, file, old, new)
SEEDS = [
    (
        "artifact identity ignores the program",
        B + "program.rs",
        "let digest = tagged_digest(SCHEMA, &self.canonical()?);",
        "let digest = tagged_digest(SCHEMA, &{ let _ = self.canonical()?; b\"{}\".to_vec() });",
    ),
    (
        "charge differs from the declared bound",
        B + "lib.rs",
        "units: self.program.cost.units_per_call,",
        "units: 0,",
    ),
    (
        "per-call bound understated",
        B + "lib.rs",
        "max_units: self.program.cost.units_per_call,",
        "max_units: self.program.cost.units_per_call.saturating_sub(1),",
    ),
    (
        "cancellation never observed",
        B + "lib.rs",
        "self.evaluate_observed(projection, &mut |_| cancel.is_raised())",
        "self.evaluate_observed(projection, &mut |_| { let _ = cancel; false })",
    ),
    (
        "a stop claimed for a failure",
        B + "lib.rs",
        """                    result: Err((AdapterFailure::Permanent, detail)),
                    charge,
                    cancel: CancelAck::NotRequested,""",
        """                    result: Err((AdapterFailure::Permanent, detail)),
                    charge,
                    cancel: CancelAck::Stopped,""",
    ),
    (
        "descriptor advertises distributions",
        B + "lib.rs",
        "output: OutputKind::Logits,",
        "output: OutputKind::Distribution,",
    ),
    (
        "stop ignored",
        B + "eval.rs",
        "if rule.stop {",
        "if false && rule.stop {",
    ),
    (
        "no-match failure ignored",
        B + "eval.rs",
        "if !applied && task.on_no_match == NoMatch::Fail {",
        "if false && task.on_no_match == NoMatch::Fail {",
    ),
    (
        "options mismatch accepted",
        B + "eval.rs",
        "if options.as_ref() != Some(&expected)",
        "if false && options.as_ref() != Some(&expected)",
    ),
    (
        "integer field type unchecked",
        B + "eval.rs",
        "FieldType::Integer => FieldValue::Integer(v.as_i64().ok_or_else(bad)?),",
        "FieldType::Integer => FieldValue::Integer(v.as_i64().unwrap_or(0)),",
    ),
    (
        "overflow saturates",
        B + "eval.rs",
        """            .checked_add(*d)
            .map_err(|_| format!("overflow in option {:?}", task.options[*k]))?;""",
        """            .checked_add(*d)
            .unwrap_or(logits[*k]);
        let _ = task;""",
    ),
    (
        "linear rounds toward zero",
        B + "eval.rs",
        ".checked_mul(x, Rounding::HalfEven)",
        ".checked_mul(x, Rounding::TowardZero)",
    ),
    (
        "duplicate lookup key resolved by position",
        B + "program.rs",
        "if table.insert(entry.key.clone(), a).is_some() {",
        "if table.insert(entry.key.clone(), a).is_some() && false {",
    ),
    (
        "condition depth unbounded",
        B + "program.rs",
        "if depth + 1 > limits::CONDITION_DEPTH {",
        "if depth + 1 > usize::MAX - 1 {",
    ),
    (
        "plan check ignores options",
        B + "check.rs",
        "if task.options != decl.options {",
        "if false {",
    ),
    (
        "le evaluated as lt",
        B + "eval.rs",
        "CompareOp::Le => n <= *value,",
        "CompareOp::Le => n < *value,",
    ),
    (
        "not ignored",
        B + "eval.rs",
        "CCond::Not(c) => !holds(c, values),",
        "CCond::Not(c) => holds(c, values),",
    ),
    (
        "any evaluated as all",
        B + "eval.rs",
        "CCond::Any(v) => v.iter().any(|c| holds(c, values)),",
        "CCond::Any(v) => v.iter().all(|c| holds(c, values)),",
    ),
    (
        "fallback targets unchecked",
        B + "check.rs",
        "for f in d.fallbacks.iter().filter(|f| &f.backend_id == me) {",
        "for f in d.fallbacks.iter().filter(|f| &f.backend_id == me && false) {",
    ),
    (
        "field reference left out of the identity",
        B + "program.rs",
        """    #[serde(rename = "ref")]
    pub reference: String,""",
        """    #[serde(rename = "ref", skip_serializing)]
    pub reference: String,""",
    ),
    (
        "lookup on_missing left out of the identity",
        B + "program.rs",
        """        entries: Vec<LookupEntry>,
        on_missing: OnMissing,""",
        """        entries: Vec<LookupEntry>,
        #[serde(skip_serializing)]
        on_missing: OnMissing,""",
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
    work = tempfile.mkdtemp(prefix="rustev-rules-seeds-")
    env = dict(os.environ, CARGO_TARGET_DIR=os.path.join(work, "target"))
    src = os.path.join(work, "src")
    ignore = shutil.ignore_patterns("target", ".git", ".tooling", ".statecraft")
    shutil.copytree(ROOT, src, ignore=ignore)
    failures = []
    chosen = [x for x in SEEDS if not sys.argv[1:] or any(a in x[0] for a in sys.argv[1:])]
    base = ["cargo", "test", "--locked", "-q", "-p", "rustev-backend-rules"]
    try:
        for name, path, old, new in chosen:
            f = os.path.join(src, path)
            original = open(f, encoding="utf-8").read()
            if original.count(old) < 1:
                failures.append(f"{name}: anchor not found in {path}")
                print(f"BROKEN   {name}: anchor not found", flush=True)
                continue
            open(f, "w", encoding="utf-8").write(original.replace(old, new))
            try:
                if run(base + ["--no-run"], src, env) != 0:
                    failures.append(f"{name}: the seeded tree does not compile")
                    print(f"BROKEN   {name}: does not compile", flush=True)
                    continue
                rc = run(base, src, env, TEST_TIMEOUT_S)
                if rc == 0:
                    failures.append(f"{name}: survived (the tests passed with the defect)")
                    print(f"SURVIVED {name}", flush=True)
                elif rc == "timeout":
                    failures.append(f"{name}: inconclusive (the tests did not finish)")
                    print(f"TIMEOUT  {name}", flush=True)
                else:
                    print(f"DETECTED {name}", flush=True)
            finally:
                open(f, "w", encoding="utf-8").write(original)
    finally:
        shutil.rmtree(work, ignore_errors=True)
    print(f"seeds: {len(chosen) - len(failures)}/{len(chosen)} detected")
    if failures:
        for x in failures:
            print(f"  {x}")
        sys.exit(1)


if __name__ == "__main__":
    main()
