"""Seeded defects in the runtime's most important guarantees (spec 003,
acceptance: a bounded negative control).

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
RT = "crates/rustev-runtime/src/"

# (name, file, old, new, cargo test arguments)
SEEDS = [
    (
        "hard ledger ignores its limit",
        RT + "ledger.rs",
        "if committed.saturating_add(units) > self.limit {",
        "if false && committed.saturating_add(units) > self.limit {",
        ["--test", "budget"],
    ),
    (
        "unknown charge refunded instead of kept as liability",
        RT + "ledger.rs",
        "s.liability = s.liability.saturating_add(reserved);",
        "s.liability = s.liability.saturating_add(0);",
        ["--test", "budget", "--test", "time", "--test", "cancel"],
    ),
    (
        "dispatch allowed at or after the deadline",
        RT + "driver.rs",
        "if cx.now() >= cx.deadline {",
        "if false {",
        ["--test", "time", "--test", "admission"],
    ),
    (
        "admission queue unbounded",
        RT + "lib.rs",
        "if n >= i.config.max_queued {",
        "if false {",
        ["--test", "admission"],
    ),
    (
        "undeclared classes retried",
        RT + "driver.rs",
        "&& step.retry.on.contains(&class)",
        "&& !step.retry.on.is_empty()",
        ["--test", "retry"],
    ),
    (
        "fallback taken on a class that is not a trigger",
        RT + "driver.rs",
        "if step.fallback_on.contains(&class) && t + 1 < step.targets.len() {",
        "if t + 1 < step.targets.len() {",
        ["--test", "retry"],
    ),
    (
        "expiry wins a same-poll race with completion",
        RT + "wait.rs",
        """        if let Poll::Ready(v) = Pin::new(&mut *work).poll(cx) {
            return Poll::Ready(Raced::Done(v));
        }
        if cancel.poll_raised(cx).is_ready() {
            return Poll::Ready(Raced::Cancelled);
        }
        if timer.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Raced::Expired);
        }""",
        """        if cancel.poll_raised(cx).is_ready() {
            return Poll::Ready(Raced::Cancelled);
        }
        if timer.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Raced::Expired);
        }
        if let Poll::Ready(v) = Pin::new(&mut *work).poll(cx) {
            return Poll::Ready(Raced::Done(v));
        }""",
        ["--test", "time"],
    ),
    (
        "invalid output accepted without the core's check",
        RT + "driver.rs",
        "Ok(Ok(())) => (AttemptEnd::Output, Ok(out)),",
        "_ => (AttemptEnd::Output, Ok(out)),",
        ["--test", "retry"],
    ),
    (
        "backend permit leaked after an attempt",
        RT + "driver.rs",
        "drop(permit);",
        "std::mem::forget(permit);",
        ["--test", "admission", "--test", "cancel"],
    ),
    (
        "remote stop claimed without an acknowledgement",
        RT + "driver.rs",
        """                answer: CancelAnswer::NotObserved,
            },
            RemoteState::PossiblyContinuing,""",
        """                answer: CancelAnswer::NotObserved,
            },
            RemoteState::Stopped,""",
        ["--test", "time", "--test", "cancel"],
    ),
    (
        "a failed delivery reported as acknowledged",
        RT + "lib.rs",
        ".map(|receipt| Delivery::Acknowledged { receipt })",
        ".or_else(|_| Ok::<String, DeliveryFailure>(String::new())).map(|receipt| Delivery::Acknowledged { receipt })",
        ["--test", "sink"],
    ),
    (
        "a dropped record not counted",
        RT + "lib.rs",
        "let total = i.stats.dropped.fetch_add(1, Ordering::SeqCst) + 1;",
        "let total = i.stats.dropped.load(Ordering::SeqCst);",
        ["--test", "sink"],
    ),
    (
        "execution policy left out of plan identity",
        "crates/rustev-core/src/compile.rs",
        "execution: plan_execution,",
        "execution: {\n            let _ = plan_execution;\n            PlanExecution::None\n        },",
        ["-p", "rustev-core", "--test", "execution"],
    ),
    (
        "a panic while building an attempt not contained",
        RT + "driver.rs",
        "let work = match catch_unwind(AssertUnwindSafe(|| backend.infer(call))) {",
        "let work = match Ok::<_, Box<dyn std::any::Any + Send>>(backend.infer(call)) {",
        ["--test", "retry"],
    ),
    (
        "a sink panic before its future not contained",
        RT + "sink.rs",
        "let work = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sink.deliver(record)))",
        "let work = match Ok::<_, Box<dyn std::any::Any + Send>>(sink.deliver(record))",
        ["--test", "sink"],
    ),
    (
        "an abandoned attempt's reservation refunded",
        RT + "driver.rs",
        "        if self.armed {",
        "        if false && self.armed {",
        ["--test", "budget"],
    ),
    (
        "no deadline check between cost disclosure and dispatch",
        RT + "driver.rs",
        """            // Disclosure is adapter code: check again, right before dispatch.
            if cx.now() >= cx.deadline {""",
        """            // Disclosure is adapter code: check again, right before dispatch.
            if false {""",
        ["--test", "time"],
    ),
    (
        "the caller's long-lived signal collects this decision's wakers",
        RT + "lib.rs",
        "let cancel = &cancel.child();",
        "let cancel = cancel;",
        ["--test", "cancel"],
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
        os.killpg(p.pid, signal.SIGKILL)
        p.wait()
        return "timeout"


def main():
    work = tempfile.mkdtemp(prefix="rustev-seeds-")
    env = dict(os.environ, CARGO_TARGET_DIR=os.path.join(work, "target"))
    src = os.path.join(work, "src")
    ignore = shutil.ignore_patterns("target", ".git", ".tooling", ".statecraft")
    # One snapshot of the tree; each seed is applied to it and then undone,
    # so only the seeded crate rebuilds.
    shutil.copytree(ROOT, src, ignore=ignore)
    failures = []
    # Optional arguments select seeds by a substring of their name.
    chosen = [x for x in SEEDS if not sys.argv[1:] or any(a in x[0] for a in sys.argv[1:])]
    try:
        for name, path, old, new, args in chosen:
            f = os.path.join(src, path)
            original = open(f, encoding="utf-8").read()
            if original.count(old) < 1:
                failures.append(f"{name}: anchor not found in {path}")
                print(f"BROKEN   {name}: anchor not found", flush=True)
                continue
            open(f, "w", encoding="utf-8").write(original.replace(old, new))
            try:
                pkg = [] if "-p" in args else ["-p", "rustev-runtime"]
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
