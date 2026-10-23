"""Landing status: what stands between open pull requests and `main`.

Read-only by default. Combines the open pull requests, their stack bases, the
latest CI rollup, runs waiting on the `statecraft-review-exception`
Environment, the latest AI review verdict and the spec dependency plan into one
ordered list of next actions, plus a few delivery measures. Approvals,
retargets and merges stay explicit; the only write is `--cancel-superseded`,
which cancels waiting runs no open pull request head still points at.
"""
import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
import re
import statistics
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
DEFAULT = "main"
AGGREGATES = {"ci-gate", "verify / verify", "review-exception"}
FAILED = {"FAILURE", "CANCELLED", "TIMED_OUT", "ACTION_REQUIRED", "STARTUP_FAILURE"}
AUTHORITY = ("scripts/statecraft/", ".github/workflows/", ".github/CODEOWNERS",
             ".statecraft/setup/", "scripts/check-authored-content.sh")
NOW = datetime.now(timezone.utc)

# Action classes, most urgent first: what unblocks the most for the least.
RANK = {"retarget": 0, "approve": 1, "fix": 2, "update": 3, "auto-merge": 4,
        "hazard": 5, "running": 6, "merging": 7, "stacked": 8, "draft": 9}
QUIET = ("running", "merging", "stacked", "draft")


def gh(*args):
    out = subprocess.run(["gh", *args], cwd=ROOT, capture_output=True, text=True)
    if out.returncode:
        raise SystemExit(f"gh {' '.join(args[:3])}: {out.stderr.strip()}")
    return json.loads(out.stdout) if out.stdout.strip() else None


def spine(*args):
    exe = ROOT / ".bin" / "spec-spine"
    out = subprocess.run([str(exe) if exe.exists() else "spec-spine", *args],
                         cwd=ROOT, capture_output=True, text=True)
    if out.returncode:
        raise SystemExit(f"spec-spine {' '.join(args)}: {out.stderr.strip()}")
    return json.loads(out.stdout)


def age(stamp):
    return (NOW - datetime.fromisoformat(stamp.replace("Z", "+00:00"))).total_seconds() / 3600


def hours(h):
    return f"{h:.0f}h" if h < 48 else f"{h / 24:.1f}d"


def unlocks(plan):
    """Spec id prefix -> how many blocked specs it unblocks, transitively."""
    direct = {}
    for b in plan.get("blocked", []):
        for dep in b.get("blockedBy", []):
            direct.setdefault(dep["id"][:3], set()).add(b["id"][:3])
    def reach(spec, seen):
        for nxt in direct.get(spec, ()):
            if nxt not in seen:
                seen.add(nxt)
                reach(nxt, seen)
        return seen
    return {spec: len(reach(spec, set())) for spec in direct}


def review_verdict(number):
    # Oldest first and no `direction` on this endpoint: read every page and
    # keep the last verdict header.
    out = subprocess.run(
        ["gh", "api", "--paginate", f"repos/{{owner}}/{{repo}}/issues/{number}/comments?per_page=100",
         "--jq", '.[].body | select(startswith("## AI review (")) | split("\n")[0]'],
        cwd=ROOT, capture_output=True, text=True)
    if out.returncode:
        raise SystemExit(f"gh api comments #{number}: {out.stderr.strip()}")
    heads = re.findall(r"## AI review \(([^)]+)\)", out.stdout)
    return heads[-1] if heads else None


def changed_paths(number):
    # Every page: the `files` field of `gh pr list` stops at 100 files, which
    # could hide an authority path in a large pull request.
    out = subprocess.run(
        ["gh", "api", "--paginate", f"repos/{{owner}}/{{repo}}/pulls/{number}/files?per_page=100",
         "--jq", ".[].filename"], cwd=ROOT, capture_output=True, text=True)
    if out.returncode:
        raise SystemExit(f"gh api files #{number}: {out.stderr.strip()}")
    return out.stdout.splitlines()


def approval_reason(number, verdict):
    paths = changed_paths(number)
    why = []
    if verdict and verdict != "no-findings":
        why.append(f"AI review: {verdict} (resolve in-PR first; approve the rerun only if they stand)")
    if any(p.startswith(AUTHORITY) for p in paths):
        why.append("authority change")
    if any(p.endswith("spec.md") for p in paths) and verdict == "no-findings":
        why.append("possible ratification (spec.md changed)")
    return ", ".join(why) or "see the run"


def classify(pr, by_head, waiting):
    base, head = pr["baseRefName"], pr["headRefOid"]
    checks = pr.get("statusCheckRollup") or []
    failed = [c["name"] for c in checks
              if c.get("conclusion") in FAILED and c["name"] not in AGGREGATES]
    running = [c["name"] for c in checks if c.get("status") not in ("COMPLETED", None)]
    auto = pr.get("autoMergeRequest") is not None
    if base != DEFAULT:
        parent = by_head.get(base)
        if parent is None:
            return "retarget", f"base `{base}` has no open PR: retarget to {DEFAULT} and rerun checks"
        if auto:
            return "hazard", f"auto-merge is on while stacked on #{parent['number']}: it would merge into `{base}`, not {DEFAULT}"
        return "stacked", f"waiting for parent #{parent['number']} (governance fails by design until retargeted)"
    run = waiting.get(head)
    if run:
        return "approve", f"owner approval waiting {hours(age(run['created_at']))}"
    if pr["isDraft"]:
        return "draft", "draft: not yet offered for merge"
    if failed:
        return "fix", "failed: " + ", ".join(failed[:4]) + (" …" if len(failed) > 4 else "")
    if running:
        return "running", f"{len(running)} check(s) running"
    if pr.get("mergeStateStatus") == "BEHIND":
        return "update", "green but behind main: update the branch"
    if not auto:
        return "auto-merge", "green and targets main: enable auto-merge"
    return "merging", "auto-merge armed"


def metrics(prs, items, waiting_runs, open_heads, specs):
    stuck = [i for i in items if i["action"] not in ("merging", "running", "draft")]
    main_heads = {p["headRefOid"] for p in prs if p["baseRefName"] == DEFAULT}
    waits = [age(r["created_at"]) for r in waiting_runs if r["head_sha"] in main_heads]
    wasted = sum(1 for r in waiting_runs if r["head_sha"] in open_heads - main_heads)
    since = (NOW.date().fromordinal(NOW.date().toordinal() - 7)).isoformat()
    merged = gh("pr", "list", "--state", "merged", "--search", f"merged:>={since}",
                "--limit", "200", "--json", "baseRefName") or []
    to_main = sum(1 for m in merged if m["baseRefName"] == DEFAULT)
    approved = [s for s in specs if s.get("status") == "approved"]
    rehearsal = gh("run", "list", "--workflow", "release.yml",
                   "--limit", "10", "--json", "conclusion,status") or []
    done = [r for r in rehearsal if r["status"] == "completed"]
    return {
        "open_prs": len(prs),
        "stacked_prs": sum(1 for p in prs if p["baseRefName"] != DEFAULT),
        "oldest_blocked_pr_hours": round(max((age(i["created"]) for i in stuck), default=0), 1),
        "approvals_waiting": len(waits),
        "approval_wait_hours_max": round(max(waits, default=0), 1),
        "approval_wait_hours_median": round(statistics.median(waits), 1) if waits else 0,
        "stacked_waiting_runs (approving is wasted)": wasted,
        "superseded_waiting_runs": sum(1 for r in waiting_runs
                                       if r["event"] == "pull_request" and r["head_sha"] not in open_heads),
        "merged_to_main_7d": to_main,
        "merged_to_intermediate_7d": len(merged) - to_main,
        "specs_complete_on_main": sum(1 for s in approved if s.get("implementation") == "complete"),
        "specs_approved": len(approved),
        "release_rehearsals_passed_last10": f"{sum(1 for r in done if r['conclusion'] == 'success')}/{len(done)}",
    }


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--json", action="store_true", help="machine-readable output")
    ap.add_argument("--cancel-superseded", action="store_true",
                    help="cancel waiting runs whose head is no open pull request's head")
    args = ap.parse_args()

    prs = gh("pr", "list", "--state", "open", "--limit", "100", "--json",
             "number,title,baseRefName,headRefName,headRefOid,autoMergeRequest,"
             "createdAt,mergeStateStatus,statusCheckRollup,isDraft,url") or []
    by_head = {p["headRefName"]: p for p in prs}
    open_heads = {p["headRefOid"] for p in prs}
    waiting_runs = []
    for page in range(1, 11):
        batch = gh("api", f"repos/{{owner}}/{{repo}}/actions/runs?status=waiting&per_page=100&page={page}")
        waiting_runs += batch["workflow_runs"]
        if len(batch["workflow_runs"]) < 100:
            break
    waiting = {}
    for r in sorted(waiting_runs, key=lambda r: r["created_at"]):
        waiting[r["head_sha"]] = r
    plan = spine("registry", "plan", "--json")
    specs = spine("registry", "list", "--json")["items"]
    weight = unlocks(plan)

    items = []
    for pr in prs:
        action, detail = classify(pr, by_head, waiting)
        spec = (re.search(r"\((\d{3})", pr["title"]) or [None, "---"])[1]
        item = {"pr": pr["number"], "spec": spec, "unlocks": weight.get(spec, 0),
                "action": action, "detail": detail, "base": pr["baseRefName"],
                "created": pr["createdAt"], "title": pr["title"], "url": pr["url"]}
        if action == "approve":
            verdict = review_verdict(pr["number"])
            run = waiting[pr["headRefOid"]]
            item.update(reason=approval_reason(pr["number"], verdict), run=run["html_url"])
        items.append(item)
    items.sort(key=lambda i: (RANK[i["action"]], -i["unlocks"], i["spec"], i["pr"]))
    report = {"actions": items, "metrics": metrics(prs, items, waiting_runs, open_heads, specs)}

    # Only pull-request runs: a release dispatch waiting on its own
    # Environment is never superseded by a pull request.
    stale = [r for r in waiting_runs
             if r["event"] == "pull_request" and r["head_sha"] not in open_heads]
    if args.cancel_superseded:
        # A run waiting on an Environment ignores a plain cancel request (it
        # is accepted and the run stays waiting); force-cancel ends it.
        failed = [r["id"] for r in stale if subprocess.run(
            ["gh", "api", "-X", "POST", f"repos/{{owner}}/{{repo}}/actions/runs/{r['id']}/force-cancel"],
            cwd=ROOT, capture_output=True, text=True).returncode]
        report["cancelled"] = len(stale) - len(failed)
        report["cancel_failed"] = failed

    if args.json:
        json.dump(report, sys.stdout, indent=2)
        print()
        return 0
    print("Next useful actions (most unblocking first):")
    n = 0
    for i in items:
        if i["action"] in QUIET:
            continue
        n += 1
        print(f"{n:3}. #{i['pr']} [{i['spec']}, unlocks {i['unlocks']}] {i['action']}: {i['detail']}")
    if not n:
        print("  (nothing actionable: every PR is running, merging or waiting on a parent)")
    print("\nIn flight:")
    for i in items:
        if i["action"] in QUIET:
            print(f"     #{i['pr']} [{i['spec']}] {i['detail']}")
    queue = [i for i in items if i["action"] == "approve"]
    if queue:
        print("\nApproval queue (statecraft-review-exception):")
        for i in queue:
            print(f"     #{i['pr']} {i['title'][:60]}\n        why: {i['reason']}\n        run: {i['run']}")
    print("\nMeasures:")
    for k, v in report["metrics"].items():
        print(f"     {k}: {v}")
    if stale and not args.cancel_superseded:
        print(f"\n{len(stale)} waiting run(s) are superseded; `--cancel-superseded` cancels them.")
    elif args.cancel_superseded:
        print(f"\ncancelled {report['cancelled']} superseded waiting run(s)")
        if report["cancel_failed"]:
            print(f"could not cancel: {report['cancel_failed']}")
            return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
