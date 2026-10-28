//! Spec 058 acceptance: the workflow test. `release.yml` runs on a pushed
//! tag; `approve`, `publish-crates`, `publish-npm` and `github-release`
//! follow every job of 3.2; `qualify` is the last job; no job writes
//! packages and only `github-release` writes contents. Read as text (no YAML
//! dependency): each job is a two-space key under `jobs:`, and its `needs`,
//! `environment` and `permissions` are four-space keys.

use std::collections::{BTreeMap, BTreeSet};

const WORKFLOW: &str = include_str!("../../../.github/workflows/release.yml");
const QUALIFY: &str = include_str!("../../../.github/workflows/qualify.yml");

/// The jobs of 3.2: every build and check before publication.
const BUILDS: &[&str] = &[
    "tag",
    "bindings",
    "cli",
    "rebuild",
    "crates",
    "crates-rebuild",
    "archive",
    "pack",
    "reproducibility",
    "sbom",
    "audit",
    "manifest",
];

#[derive(Debug, Default)]
struct Job {
    needs: Vec<String>,
    environment: Option<String>,
    permissions: BTreeMap<String, String>,
    uses: Option<String>,
}

/// The jobs in file order.
fn jobs(text: &str) -> Vec<(String, Job)> {
    let body = text.split_once("\njobs:\n").expect("a jobs: block").1;
    let mut out: Vec<(String, Job)> = Vec::new();
    let mut in_permissions = false;
    for line in body.lines() {
        if let Some(name) = line.strip_prefix("  ").and_then(|l| l.strip_suffix(':'))
            && !name.starts_with(' ')
        {
            out.push((name.to_string(), Job::default()));
            in_permissions = false;
            continue;
        }
        let Some((_, job)) = out.last_mut() else {
            continue;
        };
        if let Some(rest) = line.strip_prefix("    ")
            && !rest.starts_with(' ')
        {
            in_permissions = rest == "permissions:";
            let (key, value) = rest.split_once(':').unwrap_or((rest, ""));
            let value = value.trim();
            match key {
                "needs" => {
                    job.needs = value
                        .trim_matches(|c| c == '[' || c == ']')
                        .split(',')
                        .map(|n| n.trim().to_string())
                        .filter(|n| !n.is_empty())
                        .collect();
                }
                "environment" => job.environment = Some(value.to_string()),
                "uses" => job.uses = Some(value.to_string()),
                _ => {}
            }
        } else if in_permissions && let Some(rest) = line.strip_prefix("      ") {
            let (k, v) = rest.split_once(':').expect("a permission");
            job.permissions.insert(k.trim().into(), v.trim().into());
        }
    }
    out
}

fn closure(all: &BTreeMap<String, &Job>, name: &str) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut todo = vec![name.to_string()];
    while let Some(n) = todo.pop() {
        for d in &all[&n].needs {
            assert!(all.contains_key(d), "{n} needs unknown job {d}");
            if seen.insert(d.clone()) {
                todo.push(d.clone());
            }
        }
    }
    seen
}

#[test]
fn release_runs_on_a_pushed_tag_and_dispatch_is_the_dry_run() {
    let on = WORKFLOW
        .split_once("\non:\n")
        .unwrap()
        .1
        .split_once("\n\n")
        .unwrap()
        .0;
    assert!(on.contains("  push:\n    tags: [\"v*\"]"), "{on}");
    assert!(on.contains("  workflow_dispatch:"), "{on}");
    // A dry run carries no inputs: no manifest, signature or publish switch.
    assert!(!on.contains("inputs:"), "{on}");
    assert!(!WORKFLOW.contains("secrets."), "no registry secret is read");
}

#[test]
fn publication_follows_every_build_and_qualify_is_last() {
    let list = jobs(WORKFLOW);
    let all: BTreeMap<String, &Job> = list.iter().map(|(n, j)| (n.clone(), j)).collect();
    for b in BUILDS {
        assert!(all.contains_key(*b), "missing job {b}");
    }
    for p in ["approve", "publish-crates", "publish-npm", "github-release"] {
        let before = closure(&all, p);
        for b in BUILDS {
            assert!(before.contains(*b), "{p} does not follow {b}");
        }
    }
    assert!(closure(&all, "publish-crates").contains("approve"));
    assert!(closure(&all, "publish-npm").contains("publish-crates"));
    let (last, job) = list.last().unwrap();
    assert_eq!(last, "qualify");
    assert_eq!(job.uses.as_deref(), Some("./.github/workflows/qualify.yml"));
    let before = closure(&all, "qualify");
    for (n, _) in &list[..list.len() - 1] {
        assert!(before.contains(n), "qualify does not follow {n}");
    }
    assert!(QUALIFY.contains("\n  workflow_call:\n"));
}

#[test]
fn only_github_release_writes_contents_and_nothing_writes_packages() {
    let top = WORKFLOW
        .split_once("\npermissions:\n")
        .unwrap()
        .1
        .split_once("\n\n")
        .unwrap()
        .0;
    assert_eq!(top.trim(), "contents: read");
    for (name, job) in jobs(WORKFLOW) {
        assert_ne!(
            job.permissions.get("packages").map(String::as_str),
            Some("write"),
            "{name}"
        );
        let contents = job.permissions.get("contents").map(String::as_str);
        assert_eq!(
            contents == Some("write"),
            name == "github-release",
            "{name}: contents {contents:?}"
        );
    }
}

#[test]
fn publication_is_gated_by_its_environments_and_trusted_publishing() {
    let all: BTreeMap<String, Job> = jobs(WORKFLOW).into_iter().collect();
    for (job, env) in [
        ("approve", "release"),
        ("publish-crates", "crates-publish"),
        ("publish-npm", "npm-publish"),
    ] {
        assert_eq!(all[job].environment.as_deref(), Some(env), "{job}");
    }
    for job in ["publish-crates", "publish-npm"] {
        assert_eq!(
            all[job].permissions.get("id-token").map(String::as_str),
            Some("write")
        );
    }
    // Every action is pinned by its full commit.
    for line in WORKFLOW
        .lines()
        .filter(|l| l.trim_start().starts_with("uses:"))
    {
        let target = line.trim_start().trim_start_matches("uses:").trim();
        if target.starts_with("./") {
            continue;
        }
        let sha = target.split_once('@').expect("a pinned action").1;
        let sha = sha.split_whitespace().next().unwrap();
        assert!(
            sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()),
            "{line}"
        );
    }
    assert!(WORKFLOW.contains("rust-lang/crates-io-auth-action@"));
    assert!(WORKFLOW.contains("publish-npm \"$RUNNER_TEMP/release\""));
}
