//! Spec 058 acceptance: the workflow test. `release.yml` runs on a pushed
//! tag; `approve`, `publish-crates`, `publish-npm` and `github-release`
//! follow every job of 3.2; `qualify` is the last job; no job writes
//! packages and only `github-release` writes contents, beside qualify.yml's
//! `attach` (spec 060 3.7.3), reached through the `qualify` call. Read as text (no YAML
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
                // A trailing YAML comment (a lint waiver) is not part of the target.
                "uses" => job.uses = Some(value.split(" #").next().unwrap().trim().to_string()),
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
        if job.permissions.get("packages").map(String::as_str) == Some("write") {
            // 060 3.3.1: only `publish-image`, beside what attestation needs.
            assert_eq!(name, "publish-image");
            let want: BTreeMap<String, String> = [
                ("contents", "read"),
                ("packages", "write"),
                ("id-token", "write"),
                ("attestations", "write"),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
            assert_eq!(job.permissions, want);
        }
        let contents = job.permissions.get("contents").map(String::as_str);
        if name == "qualify" {
            // 060 3.7.3: the call's ceiling may reach qualify.yml, where only
            // `attach` holds `contents: write` (checked below).
            if contents == Some("write") {
                assert!(qualify_writers().contains(&"attach".to_string()));
            }
            continue;
        }
        assert_eq!(
            contents == Some("write"),
            name == "github-release",
            "{name}: contents {contents:?}"
        );
    }
}

/// qualify.yml's jobs that hold `contents: write`.
fn qualify_writers() -> Vec<String> {
    jobs(QUALIFY)
        .into_iter()
        .filter(|(_, j)| j.permissions.get("contents").map(String::as_str) == Some("write"))
        .map(|(n, _)| n)
        .collect()
}

#[test]
fn qualify_reads_and_only_attach_may_write_contents() {
    let top = QUALIFY
        .split_once("\npermissions:\n")
        .unwrap()
        .1
        .split_once("\n\n")
        .unwrap()
        .0;
    assert_eq!(
        top.lines().map(str::trim).collect::<Vec<_>>(),
        ["contents: read", "attestations: read"]
    );
    for (name, job) in jobs(QUALIFY) {
        for (k, v) in &job.permissions {
            assert!(
                v != "write" || (name == "attach" && k == "contents"),
                "{name}: {k}: {v}"
            );
        }
    }
    // 060 3.7.3: at most the one job `attach` writes contents.
    assert!(qualify_writers().iter().all(|n| n == "attach"));
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

/// The text of job `name`, up to the next job.
fn job_text(text: &str, name: &str) -> Option<String> {
    let body = text.split_once("\njobs:\n")?.1;
    let header = format!("  {name}:");
    let mut lines = body.lines().skip_while(|l| *l != header);
    let first = lines.next()?;
    let rest =
        lines.take_while(|l| !(l.starts_with("  ") && !l.starts_with("   ") && l.ends_with(':')));
    Some(
        std::iter::once(first)
            .chain(rest)
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

#[test]
fn publish_image_runs_only_for_a_member_release_tag_after_npm() {
    let all: BTreeMap<String, Job> = jobs(WORKFLOW).into_iter().collect();
    // 060 3.3.1: until the job exists there is nothing to push.
    let Some(text) = job_text(WORKFLOW, "publish-image") else {
        return;
    };
    let by_ref: BTreeMap<String, &Job> = all.iter().map(|(n, j)| (n.clone(), j)).collect();
    let before = closure(&by_ref, "publish-image");
    for j in [
        "approve",
        "image-checks",
        "manifest",
        "publish-crates",
        "publish-npm",
    ] {
        assert!(before.contains(j), "publish-image does not follow {j}");
    }
    // Layer 8 (the GitHub release) follows layer 7.
    assert!(closure(&by_ref, "github-release").contains("publish-image"));
    // Skipped unless `tag` decided a push: a release tag (never a candidate
    // or dry run) whose release set names the image.
    assert!(text.contains("needs.tag.outputs.image == 'true'"), "{text}");
    assert!(!text.contains("latest"), "060 3.1.2: no other tag");
}

/// The `run: |` script of the step named `name` in `job`, unindented.
fn step_script(job: &str, name: &str) -> String {
    let header = format!("      - name: {name}");
    let mut lines = job.lines().skip_while(|l| !l.starts_with(&header)).skip(1);
    lines
        .by_ref()
        .find(|l| l.trim() == "run: |")
        .unwrap_or_else(|| panic!("step {name:?} has no run block"));
    lines
        .take_while(|l| l.is_empty() || l.starts_with("          "))
        .map(|l| l.get(10..).unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn publish_image_is_skipped_for_a_candidate_and_a_dry_run() {
    // 060 3.3.1 and section 5's first row: `tag` decides whether the run
    // names the image (`image_member`) and pushes it (`image`, the only
    // condition `publish-image` runs on); the decision script runs here
    // for each kind of run.
    let tag = job_text(WORKFLOW, "tag").expect("a tag job");
    assert!(
        tag.contains("image_member: ${{ steps.image.outputs.member }}"),
        "{tag}"
    );
    assert!(
        tag.contains("image: ${{ steps.image.outputs.push }}"),
        "{tag}"
    );
    let script = step_script(
        &tag,
        "Whether this run names and pushes the image, and why not (060 3.3.1)",
    );
    let root = std::env::temp_dir().join(format!("rustev-060-skip-{}", std::process::id()));
    let set = root.join("tools/rustev-release");
    std::fs::create_dir_all(&set).unwrap();
    let decide = |image: bool, event: &str, candidate: &str| -> (String, String) {
        let body = if image {
            r#"{"image":{"repository":"ghcr.io/statecrafting/rustev-serve"}}"#
        } else {
            "{}"
        };
        std::fs::write(set.join("release-set.json"), body).unwrap();
        let output = root.join("github-output");
        std::fs::write(&output, "").unwrap();
        let run = std::process::Command::new("sh")
            .arg("-c")
            .arg(&script)
            .current_dir(&root)
            .env("EVENT", event)
            .env("CANDIDATE", candidate)
            .env("GITHUB_OUTPUT", &output)
            .output()
            .expect("sh runs the decision");
        assert!(run.status.success(), "{run:?}");
        (
            std::fs::read_to_string(&output).unwrap(),
            String::from_utf8(run.stdout).unwrap(),
        )
    };
    let cases = [
        (
            true,
            "push",
            "false",
            "member=true\npush=true\n",
            "pushes it",
        ),
        (
            true,
            "push",
            "true",
            "member=false\npush=false\n",
            "a candidate never names or pushes the image",
        ),
        (
            true,
            "workflow_dispatch",
            "false",
            "member=true\npush=false\n",
            "a dry run pushes nothing",
        ),
        (
            false,
            "push",
            "false",
            "member=false\npush=false\n",
            "the release set lists the image as absent",
        ),
    ];
    for (image, event, candidate, outputs, said) in cases {
        let (got, stdout) = decide(image, event, candidate);
        assert_eq!(got, outputs, "{image} {event} {candidate}");
        assert!(
            stdout.contains(said),
            "{image} {event} {candidate}: {stdout}"
        );
    }
    std::fs::remove_dir_all(&root).unwrap();
}
