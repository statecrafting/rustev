"""Seeded defects in replay, comparison, capture, metrics and fitting (spec
004, acceptance: a bounded negative control).

Each seed rewrites exact text in a copy of the workspace, must still compile,
and must make the named packages' tests fail. The script exits non-zero if a
seed's anchor is missing (the seed would silently do nothing), if a seed does
not compile, if the tests pass with the defect in place, or if the tests do
not finish: a timeout is inconclusive, not a detection. The working tree is
never modified.
"""

import os
import shutil
import signal
import subprocess
import sys
import tempfile

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
E = "crates/rustev-eval/src/"
C = "crates/rustev-core/src/"
R = "crates/rustev-runtime/src/"
EVAL = ["rustev-eval"]
# Spec 004, section 6: every one needs at least one detected seed.
REQUIRED = [
    "capture completeness",
    "denominator accounting",
    "expiry",
    "fallback identity",
    "output equivalence",
    "scope isolation",
    "split leakage",
]
# Generous: a new test binary can wait on the OS before it starts.
TEST_TIMEOUT_S = 1800

# (category, name, file, old, new, packages)
SEEDS = [
    (
        "scope isolation",
        "only the tenant is compared",
        E + "replay.rs",
        "if config.scope != bundle.scope {",
        "if config.scope.tenant() != bundle.scope.tenant() {",
        EVAL,
    ),
    (
        "scope isolation",
        "request identity leaves out the scope",
        C + "evaluate.rs",
        "            scope: scope.clone(),\n            backend_id: backend_id.clone(),",
        "            scope: ReplayScope::tenant_only(\n                rustev_contract::scope::Handle::new(\"x\").unwrap(),\n                rustev_contract::scope::Handle::new(\"x\").unwrap(),\n            ),\n            backend_id: backend_id.clone(),",
        ["rustev-core"] + EVAL,
    ),
    (
        "expiry",
        "an item is live at its expiry",
        E + "resolve.rs",
        "if now >= item.retention.expires_at() {",
        "if now > item.retention.expires_at() {",
        EVAL,
    ),
    (
        "expiry",
        "erasure outranks expiry",
        E + "resolve.rs",
        "    if now >= item.retention.expires_at() {\n        return unavailable(Availability::Expired);\n    }\n    let bytes = match &item.retention {",
        "    let expired = now >= item.retention.expires_at();\n    let bytes = match &item.retention {\n        _ if expired && !matches!(item.retention, Retention::External { .. }) => {\n            return unavailable(Availability::Expired);\n        }",
        EVAL,
    ),
    (
        "output equivalence",
        "disagreeing duplicates are reused",
        E + "compare.rs",
        "            Some(_) => return false,",
        "            Some(_) => {}",
        EVAL,
    ),
    (
        "output equivalence",
        "a historical failure is treated as a mismatch",
        E + "compare.rs",
        "return Err(if retained.is_some_and(|v| !v.is_empty()) {",
        "return Err(if false && retained.is_some_and(|v| !v.is_empty()) {",
        EVAL,
    ),
    (
        "fallback identity",
        "a fallback is identified as the primary",
        C + "evaluate.rs",
        "let (backend_id, artifact, descriptor) = if target == 0 {",
        "let (backend_id, artifact, descriptor) = if target <= 1 {",
        ["rustev-core"] + EVAL,
    ),
    (
        "fallback identity",
        "an output's artifact is not checked against its target",
        E + "replay.rs",
        "                    || bound.as_ref() != Some(&o.artifact)\n",
        "",
        EVAL,
    ),
    (
        "capture completeness",
        "a partial capture replays",
        E + "replay.rs",
        "if bundle.capture != CaptureStatus::Complete {",
        "if bundle.capture == CaptureStatus::Disabled {",
        EVAL,
    ),
    (
        "capture completeness",
        "a missing supply is ignored",
        E + "replay.rs",
        "    if let Some(p) = ev.pending().first() {",
        "    if let Some(p) = ev.pending().first().filter(|_| false) {",
        EVAL,
    ),
    (
        "capture completeness",
        "an overrun under cancellation is reported cancelled",
        R + "capture.rs",
        "        let status = if self.exceeded {\n            CaptureStatus::LimitExceeded\n        } else if cancelled {\n            CaptureStatus::Cancelled",
        "        let status = if cancelled {\n            CaptureStatus::Cancelled\n        } else if self.exceeded {\n            CaptureStatus::LimitExceeded",
        ["rustev-runtime"],
    ),
    (
        "denominator accounting",
        "a zero denominator measures zero",
        E + "metrics.rs",
        "        if den == 0 {\n            Measure::Unknown {",
        "        if den == 0 && num > 0 {\n            Measure::Unknown {",
        EVAL,
    ),
    (
        "denominator accounting",
        "abstentions leave the acceptance denominator",
        E + "report.rs",
        "            numerator: proposals,\n            denominator: comparable,",
        "            numerator: proposals,\n            denominator: proposals,",
        EVAL,
    ),
    (
        "denominator accounting",
        "unlabeled proposals count as correct",
        E + "report.rs",
        "                            None => bump(&mut unlabeled, \"unlabeled\"),",
        "                            None => {\n                                labeled += 1;\n                                bump(&mut unlabeled, \"unlabeled\");\n                            }",
        EVAL,
    ),
    (
        "split leakage",
        "a source repeated across splits is accepted",
        E + "dataset.rs",
        "Some(s) if *s != c.split => {",
        "Some(s) if false && *s != c.split => {",
        EVAL,
    ),
    (
        "split leakage",
        "an artifact qualifies on its own fitting split",
        E + "fit.rs",
        "    if same_dataset && split == fit.split {",
        "    if false && same_dataset && split == fit.split {",
        EVAL,
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
    work = tempfile.mkdtemp(prefix="rustev-eval-seeds-")
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
            f = os.path.join(src, path)
            original = open(f, encoding="utf-8").read()
            if original.count(old) != 1:
                failures.append(f"{label}: anchor found {original.count(old)} times in {path}")
                print(f"BROKEN   {label}: anchor", flush=True)
                continue
            open(f, "w", encoding="utf-8").write(original.replace(old, new))
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
                open(f, "w", encoding="utf-8").write(original)
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
