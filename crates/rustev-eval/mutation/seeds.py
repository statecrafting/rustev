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
import threading

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
    "unusable bundles",
    "adapter rules",
]
# Seeds another spec's acceptance runs under its own filter: spec 023's
# (labelled "spec 023"), spec 026's (`live-comparison`) and spec 029's. The
# unfiltered run, spec 004's negative control, leaves them out rather than
# run them a second time in CI; `seeds.sh "spec 023"`, `live-comparison` and
# `"spec 029"` still run and require every one. A spec whose acceptance
# starts filtering its own seeds joins this list.
OWN_RUNS = ("spec 023", "live-comparison", "spec 029")
# Generous: a new test binary can wait on the OS before it starts.
TEST_TIMEOUT_S = 1800

# (category, name, file, old, new, packages)
SEEDS = [
    (
        "adapter rules",
        "the rules digest is left out of the configuration identity (spec 014)",
        E + "config.rs",
        '#[serde(default, skip_serializing_if = "Option::is_none")]\n    pub rules',
        "#[serde(default, skip_serializing)]\n    pub rules",
        EVAL,
    ),
    (
        "adapter rules",
        "the adapter-mismatch check ignores the rules (spec 014)",
        E + "report.rs",
        ".is_some_and(|r| *r != inputs.adapter.rules())",
        ".is_some_and(|_| false)",
        EVAL,
    ),
    (
        "adapter rules",
        "a gate passes over one side's unbound rules (spec 014)",
        E + "report.rs",
        "        baseline.config.bound_rules(),\n        candidate.config.bound_rules(),\n    ) {",
        "        baseline.config.bound_rules().or(candidate.config.bound_rules()),\n        candidate.config.bound_rules().or(baseline.config.bound_rules()),\n    ) {",
        EVAL,
    ),
    (
        "unusable bundles",
        "a load failure is reported as bundle-missing (spec 015)",
        E + "report.rs",
        "Some(BundleInput::Failed(f)) => return incomparable(None, f.kind.code()),",
        'Some(BundleInput::Failed(_)) => return incomparable(None, "bundle-missing"),',
        EVAL,
    ),
    (
        "unusable bundles",
        "an unusable bundle leaves the coverage denominator (spec 015)",
        E + "report.rs",
        "let results: Vec<CaseResult<'_>> = cases.iter().map(|c| evaluate_case(inputs, c)).collect();",
        "let results: Vec<CaseResult<'_>> = cases.iter().map(|c| evaluate_case(inputs, c)).filter(|r| !matches!(&r.status, Status::Incomparable(c) if c.starts_with(\"bundle-\") && c != \"bundle-missing\")).collect();",
        EVAL,
    ),
    (
        "unusable bundles",
        "an unusable bundle is counted as comparable (spec 015)",
        E + "report.rs",
        "            Status::Incomparable(code) => {\n",
        "            Status::Incomparable(code) => {\n                if code.starts_with(\"bundle-c\") {\n                    comparable += 1;\n                }\n",
        EVAL,
    ),
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
    # Spec 026, 3.8: live comparison (not a spec 004 category).
    (
        "live-comparison",
        "a changed live result is reported as agreement",
        E + "live.rs",
        """                ResultChange::Unchanged => ComparisonOutcome::Agreement,
                _ => ComparisonOutcome::LiveDiverged,""",
        """                _ => ComparisonOutcome::Agreement,""",
        EVAL,
    ),
    (
        "live-comparison",
        "a bundle other than the linked one is compared",
        E + "live.rs",
        "if HistoricalLink::of(case.bundle).ok().as_ref() != Some(link) {",
        "if false {",
        EVAL,
    ),
    (
        "qualification refusals",
        "a synthetic manifest is admitted (spec 023, I-1)",
        E + "dataset.rs",
        "    if matches!(manifest.provenance, DatasetProvenance::Synthetic { .. }) {",
        "    if false && matches!(manifest.provenance, DatasetProvenance::Synthetic { .. }) {",
        EVAL,
    ),
    (
        "qualification refusals",
        "labels from the model under test are admitted (spec 023, 5 row 2)",
        E + "dataset.rs",
        "                && m.related(s)",
        "                && false && m.related(s)",
        EVAL,
    ),
    (
        "qualification refusals",
        "a claim cites another admission (spec 023, 5 row 1)",
        E + "report.rs",
        "    if &r.dataset != admitted.dataset() || &r.admission != admitted.id() {\n"
        "        return Err(ClaimError::NotAdmitted);",
        "    if false {\n        return Err(ClaimError::NotAdmitted);",
        EVAL,
    ),
    (
        "qualification refusals",
        "a calibration without lineage or fitted on the final test freezes (spec 023, 5 row 3)",
        E + "report.rs",
        "=> return Err(void(reason)),",
        "=> drop(reason),",
        EVAL,
    ),
    (
        "qualification refusals",
        "a report under a changed configuration is measured (spec 023, 5 row 4)",
        E + "report.rs",
        "        if e.report.evaluator_config != r.config {",
        "        if false && e.report.evaluator_config != r.config {",
        EVAL,
    ),
    (
        "qualification refusals",
        "a slice below its minimum is a figure (spec 023, 5 row 5)",
        E + "report.rs",
        "            if (members.len() as u64) < min {",
        "            if false && (members.len() as u64) < min {",
        EVAL,
    ),
    (
        "qualification refusals",
        "a claim names another population (spec 023, 5 row 6)",
        E + "report.rs",
        "    if request.population != a.population.id {",
        "    if false && request.population != a.population.id {",
        EVAL,
    ),
    (
        "qualification refusals",
        "exposed cases stay in the headline (spec 023, 3.2.1)",
        E + "report.rs",
        "final_test.difference(&exposed).copied().collect();",
        "final_test.iter().copied().collect();",
        EVAL,
    ),
]

# Spec 029 Acceptance: one seed for each required library defect.
SEEDS += [
    (
        "spec 029",
        'nearest-rank quantile off by one',
        E + 'repeat_stats.rs',
        'Figure::Known(samples[rank - 1])',
        'Figure::Known(samples[rank.min(samples.len() - 1)])',
        EVAL,
    ),
    (
        "spec 029",
        'undefined resample dropped',
        E + 'repeat_stats.rs',
        'Figure::Unknown { .. } => undefined[m] += 1,',
        'Figure::Unknown { .. } => {},',
        EVAL,
    ),
    (
        "spec 029",
        'unknown band treated as zero',
        E + 'selection.rs',
        'BandDoc::Unknown(r) => Err(format!("{} band: {r}", m.name)),',
        'BandDoc::Unknown(_) => Ok(0.0),',
        EVAL,
    ),
    (
        "spec 029",
        'unknown selection qualifies',
        E + 'selection.rs',
        '            Verdict::Unknown { clause }',
        '            { let _ = clause; Verdict::Qualifies }',
        EVAL,
    ),
    (
        "spec 029",
        'diff uses only baseline band',
        E + 'diff.rs',
        'DiffBool::Value(d.abs() <= b.max(*r))',
        'DiffBool::Value(d.abs() <= *b)',
        EVAL,
    ),
    (
        "spec 029",
        'held-out count off by one',
        E + 'holdout.rs',
        'count: count(&|e| e.lineage == new.lineage),',
        'count: count(&|e| e.lineage == new.lineage) - 1,',
        EVAL,
    ),
    (
        "spec 029",
        'historical latency counted as candidate',
        E + 'repeat.rs',
        '        Observation::Historical',
        '        Observation::Candidate',
        EVAL,
    ),
    (
        "spec 029",
        'ledger chain not checked',
        E + 'holdout.rs',
        'if e.prev != prev {',
        'if false && e.prev != prev {',
        EVAL,
    ),
]


# Spec 040: explanation acceptance defects.
SEEDS += [
    ("spec 040", 'explain040-first-reason', E + 'explain/excluded.rs', 'let first = v["reasons"].as_array().and_then(|a| a.first());', 'let first = v["reasons"].as_array().and_then(|a| a.last());', EVAL),
    ("spec 040", 'explain040-tie-descending', E + 'explain/order.rs', 'a.as_bytes() < c.as_bytes()', 'a.as_bytes() > c.as_bytes()', EVAL),
    ("spec 040", 'explain040-judgment-exact', E + 'explain/facts.rs', '                class,\n                bound: DerivationBound::Judgment,', '                class: { let _ = class; Derivation::ExactDerived },\n                bound: DerivationBound::Judgment,', EVAL),
    ("spec 040", 'explain040-unknown-zero', E + 'explain/models.rs', 'group.unknown.checked_add(1)', 'group.unknown.checked_add(0)', EVAL),
    ("spec 040", 'explain040-observation-first', E + 'explain/diff.rs', 'Claim::ObservationDifference => self.observations.push(id),', 'Claim::ObservationDifference => { self.why = Some(Divergence::OutcomeChanged); self.differences.insert(0, id); self.total += 1; },', EVAL),
    ("spec 040", 'explain040-withheld-rendered', E + 'explain/render.rs', 'Availability::NotAvailable(u) => unavailable(u),', 'Availability::NotAvailable(u) => { let _ = u; json(&f.value) },', EVAL),
    ("spec 040", 'explain040-plan-mismatch', E + 'explain/evidence.rs', 'if record.as_ref().is_some_and(|r| r.plan_id != id) {', 'if false && record.as_ref().is_some_and(|r| r.plan_id != id) {', EVAL),
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


def jobs():
    # Workers, each on its own copy of the tree and target directory. One
    # seed's rebuild and test run leave cores idle (a single crate's front
    # end is serial), so two seeds at a time on a 4-core runner overlap
    # compile and test. SEEDS_JOBS overrides the default.
    if os.environ.get("SEEDS_JOBS"):
        return max(1, int(os.environ["SEEDS_JOBS"]))
    return max(1, min(4, (os.cpu_count() or 2) // 2))


def try_seed(seed, src, env):
    """The verdict line and the failure (None when detected) of one seed."""
    category, name, path, old, new, packages = seed
    label = f"{category}: {name}"
    base = ["cargo", "test", "--locked", "-q"] + [a for p in packages for a in ("-p", p)]
    f = os.path.join(src, path)
    original = open(f, encoding="utf-8").read()
    if original.count(old) != 1:
        return f"BROKEN   {label}: anchor", f"{label}: anchor found {original.count(old)} times in {path}"
    open(f, "w", encoding="utf-8").write(original.replace(old, new))
    try:
        if run(base + ["--no-run"], src, env) != 0:
            return f"BROKEN   {label}: does not compile", f"{label}: the seeded tree does not compile"
        rc = run(base, src, env, TEST_TIMEOUT_S)
        if rc == 0:
            return f"SURVIVED {label}", f"{label}: survived (the tests passed with the defect)"
        if rc == "timeout":
            return f"TIMEOUT  {label}", f"{label}: inconclusive (the tests did not finish)"
        return f"DETECTED {label}", None
    finally:
        open(f, "w", encoding="utf-8").write(original)


def main():
    if sys.argv[1:]:
        chosen = [x for x in SEEDS if any(a in x[1] or a in x[0] for a in sys.argv[1:])]
    else:
        chosen = [x for x in SEEDS if not any(o in x[0] or o in x[1] for o in OWN_RUNS)]
    if not chosen:
        print("seeds: no seed matches the filter")
        sys.exit(1)
    work = tempfile.mkdtemp(prefix="rustev-eval-seeds-")
    ignore = shutil.ignore_patterns("target", ".git", ".tooling", ".bin", ".statecraft")
    n = min(jobs(), len(chosen))
    trees = []
    for i in range(n):
        src = os.path.join(work, f"src{i}")
        shutil.copytree(ROOT, src, ignore=ignore)
        trees.append((src, dict(os.environ, CARGO_TARGET_DIR=os.path.join(work, f"target{i}"))))
    failures = []
    try:
        if n > 1:
            # Build the unseeded tree once, then give every worker a copy of
            # that target directory instead of n concurrent cold builds.
            # Cargo keys artifacts on the source path, so the copies stay
            # valid only for the dependencies; each worker rebuilds the
            # workspace crates on its first seed.
            src, env = trees[0]
            packages = sorted({p for x in chosen for p in x[5]})
            warm = subprocess.run(
                ["cargo", "test", "--locked", "-q", "--no-run"] + [a for p in packages for a in ("-p", p)],
                cwd=src, env=env, capture_output=True, text=True,
            )
            if warm.returncode != 0 or not os.path.isdir(env["CARGO_TARGET_DIR"]):
                # Only a speed-up: each worker builds cold on its first seed
                # instead, and a tree that truly does not compile is still
                # reported per seed. The failure is shown, never swallowed.
                tail = "\n".join((warm.stderr or "").strip().splitlines()[-20:])
                print(f"seeds: the warm build failed (exit {warm.returncode}); each worker builds cold\n{tail}", flush=True)
            else:
                for _, other in trees[1:]:
                    shutil.copytree(env["CARGO_TARGET_DIR"], other["CARGO_TARGET_DIR"], symlinks=True)
        queue = list(enumerate(chosen))
        results = {}
        lock = threading.Lock()

        def worker(src, env):
            while True:
                with lock:
                    if not queue:
                        return
                    i, seed = queue.pop(0)
                try:
                    line, failure = try_seed(seed, src, env)
                except Exception as e:  # a crash is never a detection
                    label = f"{seed[0]}: {seed[1]}"
                    line, failure = f"BROKEN   {label}: {e!r}", f"{label}: the harness failed: {e!r}"
                with lock:
                    results[i] = failure
                    print(line, flush=True)

        threads = [threading.Thread(target=worker, args=tree) for tree in trees]
        for t in threads:
            t.start()
        for t in threads:
            t.join()
        failures = [results[i] for i in sorted(results) if results[i]]
        # Every seed must have a verdict: one without is never counted detected.
        failures += [f"{chosen[i][0]}: {chosen[i][1]}: no verdict was recorded"
                     for i in range(len(chosen)) if i not in results]
    finally:
        shutil.rmtree(work, ignore_errors=True)
    full = not sys.argv[1:]
    missing = [c for c in REQUIRED if full and not any(x[0] == c for x in chosen)]
    print(f"seeds: {len(chosen) - len(failures)}/{len(chosen)} detected ({n} worker(s))")
    for c in missing:
        print(f"  no seed covers the required category {c!r}")
    if failures or missing:
        for x in failures:
            print(f"  {x}")
        sys.exit(1)


if __name__ == "__main__":
    main()
