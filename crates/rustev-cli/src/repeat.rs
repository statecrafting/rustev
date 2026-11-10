//! CLI arguments and filesystem publication for repeat commands.
use crate::args::{Parsed, Usage, usage};
use crate::io::{self, Reads, join};
use crate::out::{Done, Res};
use rustev_contract::Document;
use rustev_contract::eval_report::Split;
use rustev_contract::scope::Handle;
use rustev_eval::comparison;
use rustev_eval::repeat::RepeatReport;
use rustev_surface::adapter::TaskAdapterDoc;
pub fn validate(p: &Parsed) -> Result<usize, Usage> {
    let split = crate::eval::split(p)?;
    let extra = [
        "repeat",
        "purpose",
        "lineage",
        "ledger",
        "ledger-new",
        "ledger-out",
    ];
    if !p.has("repeat-config") {
        p.forbid(&extra, "needs --repeat-config")?;
        if p.many("bundles").len() != 1 {
            return usage("several --bundles need --repeat-config");
        }
        return Ok(1);
    }
    let purpose = p.one("purpose").unwrap_or("measurement");
    if !matches!(purpose, "measurement" | "optimization") {
        return usage("--purpose is measurement or optimization");
    }
    if split == Split::FinalTest {
        if purpose == "optimization" {
            return usage("optimization cannot use final_test");
        }
        p.need(&["lineage", "ledger-out"], "for held-out evaluation")?;
        if p.has("ledger") == p.has("ledger-new") {
            return usage("give exactly one of --ledger and --ledger-new");
        }
        Handle::new(p.req("lineage")).map_err(|e| Usage(e.to_string()))?;
    } else {
        p.forbid(
            &["lineage", "ledger", "ledger-new", "ledger-out"],
            "is only for final_test",
        )?;
    }
    if !p.has("candidate-plan") {
        p.forbid(
            &["descriptor", "rules", "calibration"],
            "needs --candidate-plan",
        )?;
    }
    let n = p.uint("repeat", 1, 64)?.map(|n| n as usize);
    let dirs = p.many("bundles").len();
    if dirs == 1 {
        n.ok_or_else(|| Usage("one --bundles needs --repeat".into()))
    } else if dirs > 64 || n.is_some_and(|n| n != dirs) {
        usage("captured runs need 2 to 64 directories and an equal --repeat, if given")
    } else {
        Ok(dirs)
    }
}

pub fn eval(p: &Parsed) -> Result<Res, Usage> {
    let n = validate(p)?;
    let split = crate::eval::split(p)?;
    let replay = crate::replay::replay_config(p)?;
    if let Err(d) = crate::outputs::check(p) {
        return Ok(Err(d));
    }
    let reads = Reads::default();
    let resolver = crate::replay::resolver(p);
    let mut rules = vec![];
    let args = rustev_surface::repeat::EvalArgs {
        n,
        split,
        replay: &replay,
        dataset: p.req("dataset"),
        config: p.req("config"),
        adapter: p.req("adapter"),
        repeat_config: p.req("repeat-config"),
        bundles: p.many("bundles"),
        candidate_plan: p.one("candidate-plan"),
        deps: crate::deps::args(p),
        ledger: p.one("ledger"),
        lineage: p.one("lineage"),
        ledger_out: p.one("ledger-out"),
        out: p.req("out"),
    };
    let mut hook = crate::deps::rules_hook(&mut rules);
    Ok(if p.spec.path == ["eval", "candidates"] {
        rustev_surface::repeat::candidates(
            &p.name(),
            &reads,
            args,
            p.req("candidates"),
            resolver.as_ref(),
            &mut hook,
        )
    } else {
        rustev_surface::repeat::eval(&p.name(), &reads, args, resolver.as_ref(), &mut hook)
    })
}

pub fn baseline(p: &Parsed) -> Result<Res, Usage> {
    let now = crate::host::now(p)?.as_ms();
    if let Err(d) = crate::outputs::check(p) {
        return Ok(Err(d));
    }
    Ok(rustev_surface::repeat::baseline(
        &p.name(),
        &Reads::default(),
        rustev_surface::repeat::BaselineArgs {
            name: p.req("name"),
            eval: p.req("eval"),
            out: p.req("out"),
            now_ms: now,
        },
    ))
}

pub fn compare(p: &Parsed) -> Res {
    crate::outputs::check(p)?;
    let args = rustev_surface::repeat::CompareArgs {
        baseline: p.req("baseline"),
        baseline_eval: p.req("baseline-eval"),
        run: p.req("run"),
    };
    if p.spec.path == ["eval", "diff"] {
        rustev_surface::repeat::diff(&p.name(), &Reads::default(), args, p.req("out"))
    } else {
        rustev_surface::repeat::reproducibility(&p.name(), &Reads::default(), args)
    }
}

pub fn select(p: &Parsed) -> Res {
    crate::outputs::check(p)?;
    let mut done = rustev_surface::repeat::select(
        &p.name(),
        &Reads::default(),
        rustev_surface::repeat::SelectArgs {
            selection_gate: p.req("selection-gate"),
            baseline: p.req("baseline"),
            baseline_eval: p.req("baseline-eval"),
            candidates: p.req("candidates"),
            out: p.req("out"),
        },
    )?;
    // Keep legacy stdout bytes untouched. Only integrity-void verdicts gain
    // the instruction to repeat the run; the clause entries carry its evidence.
    if done.code == 12 {
        let mut value: serde_json::Value =
            serde_json::from_slice(&done.out.bytes()).expect("command output is JSON");
        if let Some(candidates) = value.get_mut("candidates").and_then(|v| v.as_array_mut()) {
            let mut changed = false;
            for candidate in candidates.iter_mut() {
                if candidate
                    .pointer("/verdict/unknown/clause")
                    .and_then(|v| v.as_str())
                    .is_some_and(|clause| clause.ends_with(".integrity"))
                {
                    candidate["notice"] = serde_json::json!("void: repeat the run");
                    changed = true;
                }
            }
            if changed {
                done.out = done.out.set("candidates", candidates);
            }
        }
    }
    Ok(done)
}

/// `rustev eval repeat import` (spec 061, 3.2).
pub fn import(p: &Parsed) -> Result<Res, Usage> {
    if p.many("run").len() > 64 {
        return usage("--run is given 1 to 64 times");
    }
    if let Err(d) = crate::outputs::check(p) {
        return Ok(Err(d));
    }
    Ok(rustev_surface::repeat::import(
        &p.name(),
        &Reads::default(),
        rustev_surface::repeat::ImportArgs {
            runs: p.many("run"),
            repeat_config: p.req("repeat-config"),
            config: p.req("config"),
            out: p.req("out"),
        },
    ))
}

/// Render the host-owned parts of live repeat sets after engine publication.
pub(crate) fn publish_live_outputs(
    command: &str,
    out: &str,
    adapter: &TaskAdapterDoc,
    reports: &std::collections::BTreeMap<String, RepeatReport>,
    comparison: &comparison::Comparison,
) -> Result<(), Done> {
    let bytes = adapter
        .canonical()
        .map_err(|e| Done::invalid(command, "adapter", e.to_string()))?;
    let mut files: Vec<_> = reports
        .keys()
        .map(|name| (join(&join(out, name), "adapter.json"), bytes.clone()))
        .collect();
    files.push((
        join(out, "comparison.md"),
        comparison::render_markdown(comparison).into_bytes(),
    ));
    // Resume accepts identical published files, but never overwrites conflicting evidence.
    let reads = Reads::default();
    let mut pending = Vec::new();
    for (path, bytes) in files {
        match reads.read_classified(&path, bytes.len().saturating_add(1)) {
            Ok(Some(existing)) if existing == bytes => {}
            Ok(None) => pending.push((path, bytes)),
            Ok(Some(_)) => {
                return Err(Done::io(
                    command,
                    io::IoFail {
                        path,
                        detail: "output already exists with different bytes".into(),
                    },
                ));
            }
            Err(e) => {
                return Err(Done::io(
                    command,
                    match e {
                        rustev_surface::host::ReadFail::File(e)
                        | rustev_surface::host::ReadFail::Budget(e) => e,
                    },
                ));
            }
        }
    }
    publish_outputs(&pending, &[], |temp, path| std::fs::hard_link(temp, path))
        .map_err(|e| Done::io(command, e))
}

// Stage the entire set before publishing. Hard links provide the no-clobber
// publication required by spec 006, including when a path appears after preflight.
pub(crate) fn publish_outputs(
    files: &[(String, Vec<u8>)],
    dirs: &[String],
    mut publish: impl FnMut(&std::path::Path, &str) -> std::io::Result<()>,
) -> Result<(), io::IoFail> {
    use std::io::Write;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let mut temporary = vec![];
    let mut placed = vec![];
    let mut created = vec![];
    let fail = |path: &str, e: std::io::Error| io::IoFail {
        path: path.into(),
        detail: e.to_string(),
    };
    let result = (|| {
        for dir in dirs {
            std::fs::create_dir(dir).map_err(|e| fail(dir, e))?;
            created.push(dir.clone());
        }
        for (path, bytes) in files {
            let temp = std::path::Path::new(path).with_file_name(format!(
                ".rustev-repeat-{}-{}.tmp",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)
                .map_err(|e| fail(path, e))?;
            temporary.push(temp);
            file.write_all(bytes)
                .and_then(|()| file.sync_all())
                .map_err(|e| fail(path, e))?;
        }
        for ((path, _), temp) in files.iter().zip(&temporary) {
            publish(temp, path).map_err(|e| fail(path, e))?;
            placed.push(std::path::PathBuf::from(path));
        }
        Ok(())
    })();
    if let Err(mut error) = result {
        rollback_outputs(&mut error, &placed, &temporary, &created);
        return Err(error);
    }
    // Every output is published. A temporary file that cannot be removed now
    // is reported, never a reason to withdraw the published results.
    for temp in &temporary {
        if let Err(e) = std::fs::remove_file(temp) {
            eprintln!(
                "rustev: warning: outputs published, but temporary file {} was not removed: {e}",
                temp.display()
            );
        }
    }
    Ok(())
}

fn rollback_outputs(
    error: &mut io::IoFail,
    placed: &[std::path::PathBuf],
    temporary: &[std::path::PathBuf],
    created: &[String],
) {
    let mut report = |path: &std::path::Path, result: std::io::Result<()>| {
        if let Err(e) = result {
            error
                .detail
                .push_str(&format!("; cleanup failed for {}: {e}", path.display()));
        }
    };
    for path in placed.iter().rev().chain(temporary.iter().rev()) {
        report(path, std::fs::remove_file(path));
    }
    for dir in created.iter().rev() {
        report(std::path::Path::new(dir), std::fs::remove_dir_all(dir));
    }
}

#[cfg(test)]
mod publication_tests {
    use super::*;

    #[test]
    fn comparison_second_staging_failure_leaves_no_outputs() {
        let root =
            std::env::temp_dir().join(format!("rustev-compare-staging-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let files: Vec<_> = ["diff.json", "missing/diff.md"]
            .iter()
            .map(|name| {
                (
                    root.join(name).to_str().unwrap().to_owned(),
                    b"output".to_vec(),
                )
            })
            .collect();
        let command = "eval diff";
        // The first file can be staged, but the second has no parent directory.
        let result = publish_outputs(&files, &[], |temp, path| std::fs::hard_link(temp, path));
        let remaining = std::fs::read_dir(&root).unwrap().count();
        std::fs::remove_dir_all(&root).unwrap();
        let error = result.unwrap_err();
        let error = Done::io(command, error);
        assert_eq!(error.code, 3);
        assert_eq!(error.out.status(), "io_error");
        assert_eq!(
            remaining, 0,
            "published and temporary files must be removed"
        );
    }

    #[test]
    fn comparison_second_publication_failure_leaves_no_outputs() {
        let root =
            std::env::temp_dir().join(format!("rustev-compare-rollback-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let files: Vec<_> = ["diff.json", "diff.md"]
            .iter()
            .map(|name| {
                (
                    root.join(name).to_str().unwrap().to_owned(),
                    b"output".to_vec(),
                )
            })
            .collect();
        let command = "eval diff";
        let mut count = 0;
        let result = publish_outputs(&files, &[], |temp, path| {
            count += 1;
            if count == 2 {
                assert!(root.join("diff.json").is_file());
                return Err(std::io::Error::other("second publication failed"));
            }
            std::fs::hard_link(temp, path)
        });
        let remaining = std::fs::read_dir(&root).unwrap().count();
        std::fs::remove_dir_all(&root).unwrap();
        let error = result.unwrap_err();
        assert_eq!(count, 2);
        let error = Done::io(command, error);
        assert_eq!(error.code, 3);
        assert_eq!(error.out.status(), "io_error");
        assert_eq!(
            remaining, 0,
            "published and temporary files must be removed"
        );
    }

    #[test]
    fn repeat_publication_failure_removes_staged_and_placed_files() {
        let root =
            std::env::temp_dir().join(format!("rustev-repeat-rollback-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let candidate = root.join("candidate").to_str().unwrap().to_owned();
        let paths = [
            root.join("first.json"),
            root.join("candidate/second.json"),
            root.join("last.json"),
        ];
        let files: Vec<_> = paths
            .iter()
            .map(|p| (p.to_str().unwrap().to_owned(), b"output".to_vec()))
            .collect();
        let mut count = 0;
        let error = publish_outputs(&files, &[candidate], |temp, path| {
            count += 1;
            if count == 3 {
                // Simulate a competing writer after staging and two publications.
                std::fs::write(path, b"keep")?;
            }
            std::fs::hard_link(temp, path)
        })
        .unwrap_err();
        assert_eq!(count, 3);
        assert_eq!(error.path, files[2].0);
        let done = Done::io("eval", error);
        assert_eq!(done.code, 3);
        assert_eq!(done.out.status(), "io_error");
        assert!(!paths[0].exists());
        assert!(!root.join("candidate").exists());
        assert_eq!(std::fs::read(&paths[2]).unwrap(), b"keep");
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn repeat_rollback_reports_cleanup_failure_and_continues() {
        let root =
            std::env::temp_dir().join(format!("rustev-repeat-cleanup-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let good = root.join("file");
        std::fs::write(&good, b"output").unwrap();
        let mut error = io::IoFail {
            path: "original".into(),
            detail: "original failure".into(),
        };
        // A directory cannot be removed as a file. The other removal must still run.
        rollback_outputs(&mut error, &[good.clone(), root.clone()], &[], &[]);
        assert_eq!(error.path, "original");
        assert!(
            error
                .detail
                .starts_with("original failure; cleanup failed for ")
        );
        assert!(error.detail.contains(root.to_str().unwrap()));
        assert!(!good.exists());
        std::fs::remove_dir(root).unwrap();
    }
}
