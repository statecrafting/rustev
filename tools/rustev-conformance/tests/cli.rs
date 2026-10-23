//! Spec 027, 3.12: the Rust runner runs every `cli` case through the CLI's
//! library entry point, in a fresh directory holding exactly the case's
//! pinned inputs, and compares every output with its golden under the
//! case's declared normalization. The cases run one after another in one
//! test, because a case's paths are relative to the process's directory.

use rustev_conformance::{compare, manifest, pinned};

#[test]
fn every_cli_case_produces_its_expected_bytes() {
    let m = manifest().unwrap();
    // Unique per run, and created fresh: no earlier run's files can be
    // taken for this run's outputs.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let base =
        std::env::temp_dir().join(format!("rustev-conformance-{}-{nanos}", std::process::id()));
    assert!(!base.exists(), "{} exists", base.display());
    let start = std::env::current_dir().unwrap();
    let mut failures = vec![];
    let mut ran = 0;
    for case in m
        .cases
        .iter()
        .filter(|c| c.surfaces.iter().any(|s| s == "cli"))
    {
        let dir = base.join(&case.id);
        std::fs::create_dir_all(&dir).unwrap();
        for i in &case.inputs {
            let bytes = pinned(&i.path, &i.sha256).unwrap();
            let p = dir.join(rustev_conformance::contained(&i.name).unwrap());
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, bytes).unwrap();
        }
        // The CLI resolves its arguments against the working directory, a
        // process-wide value: `Cwd` holds a lock for the call and restores
        // the directory even when the call panics.
        let exit = {
            let _cwd = Cwd::enter(&dir, &start);
            rustev_cli::execute(&case.argv, &rustev_cli::Context::default())
        };
        failures.extend(compare(case, i32::from(exit.code), &exit.stdout, &dir));
        ran += 1;
    }
    // On a divergence the outputs stay where the CLI wrote them.
    assert!(
        failures.is_empty(),
        "{failures:#?}\noutputs kept under {}",
        base.display()
    );
    let _ = std::fs::remove_dir_all(&base);
    // Every CLI golden is a case (3.12.4).
    // Both sides count `.json` goldens only; `.md` and `.stdout` renderings are
    // pinned by the CLI's own tests, not by conformance cases.
    let goldens =
        std::fs::read_dir(rustev_conformance::root().join("crates/rustev-cli/tests/golden"))
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .is_ok_and(|e| e.path().extension().is_some_and(|x| x == "json"))
            })
            .count();
    let covered: std::collections::BTreeSet<&str> = m
        .cases
        .iter()
        .flat_map(|c| c.outputs.iter().map(|o| o.golden.as_str()))
        .filter(|g| g.starts_with("crates/rustev-cli/tests/golden/") && g.ends_with(".json"))
        .collect();
    assert_eq!(covered.len(), goldens, "every CLI golden is covered");
    // A `run` case pins two goldens, its printed output and the record it
    // writes, so the records are covered though no case is named for them.
    for record in ["lodging.record.json", "support.record.json"] {
        let path = format!("crates/rustev-cli/tests/golden/{record}");
        assert!(
            covered.contains(path.as_str()),
            "{path} is a run case's output"
        );
    }
    assert!(ran >= 29, "{ran} cases ran");
}

#[test]
fn every_case_pins_its_inputs_and_compares_its_plan_id() {
    let m = manifest().unwrap();
    let mut with_plan = 0;
    for c in &m.cases {
        for i in &c.inputs {
            pinned(&i.path, &i.sha256).unwrap();
        }
        assert!(!c.outputs.is_empty(), "{}", c.id);
        // Every case asserts `plan_id` (3.12.3): where a golden prints one,
        // no declared normalization touches it, so it is compared as bytes.
        for o in &c.outputs {
            let golden = pinned(&o.golden, &o.sha256).unwrap();
            // Repeat diff and comparison also pin their Markdown rendering.
            // Those bytes are compared in full by the CLI runner above.
            if o.golden.ends_with(".md") {
                assert!(o.normalize.is_empty(), "{}", c.id);
                continue;
            }
            let v: serde_json::Value = serde_json::from_slice(&golden).unwrap();
            if let Some(id) = v.get("plan_id") {
                let normalized =
                    rustev_conformance::normalize::apply(&o.normalize, &golden).unwrap();
                let n: serde_json::Value = serde_json::from_slice(&normalized).unwrap();
                assert_eq!(n.get("plan_id"), Some(id), "{}", c.id);
                with_plan += 1;
            }
        }
    }
    assert!(with_plan >= 15, "{with_plan} outputs name a plan");
}

#[test]
fn a_differing_output_or_exit_code_is_reported() {
    let m = manifest().unwrap();
    let c = m.cases.iter().find(|c| c.id == "support.check").unwrap();
    let dir = std::env::temp_dir();
    let v = compare(c, 0, b"{}\n", &dir);
    assert!(v.iter().any(|x| x.contains("differs")), "{v:?}");
    let golden = pinned(&c.outputs[0].golden, &c.outputs[0].sha256).unwrap();
    assert!(compare(c, 0, &golden, &dir).is_empty());
    let v = compare(c, 5, &golden, &dir);
    assert!(v.iter().any(|x| x.contains("exit 5")), "{v:?}");
    // A normalization that does not apply is reported as such.
    let mut odd = c.clone();
    odd.outputs[0].normalize = vec!["receipt".into()];
    let v = compare(&odd, 0, b"not json", &dir);
    assert!(v.iter().any(|x| x.contains("do not apply")), "{v:?}");
}

/// The working directory changed for one CLI call: serialized across this
/// binary's tests and restored on drop, panic included.
struct Cwd<'a> {
    start: &'a std::path::Path,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl<'a> Cwd<'a> {
    fn enter(dir: &std::path::Path, start: &'a std::path::Path) -> Self {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let lock = LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        std::env::set_current_dir(dir).unwrap();
        Cwd { start, _lock: lock }
    }
}

impl Drop for Cwd<'_> {
    fn drop(&mut self) {
        // While a panic unwinds, a second one would abort the process: the
        // failure to restore is reported, and the first panic stands.
        if let Err(e) = std::env::set_current_dir(self.start) {
            if std::thread::panicking() {
                eprintln!("could not restore {}: {e}", self.start.display());
            } else {
                panic!("could not restore {}: {e}", self.start.display());
            }
        }
    }
}

#[test]
fn the_duplicate_key_case_is_the_parsers_refusal_of_it() {
    // `refusal-01` pins category 1 (parse): its definition carries the key
    // `name` twice on purpose, and the bounded parser refuses it rather
    // than keep the last value as serde_json alone would.
    let m = manifest().unwrap();
    let case = m.cases.iter().find(|c| c.id == "refusal-01").unwrap();
    let input = case
        .inputs
        .iter()
        .find(|i| i.name == "cat-01.definition.json")
        .unwrap();
    let text = String::from_utf8(pinned(&input.path, &input.sha256).unwrap()).unwrap();
    // The top-level `name`, first `x`, then again `support-routing`.
    assert!(
        text.starts_with(r#"{"name":"x","#),
        "the duplicate is the point"
    );
    assert!(
        text.contains(r#""name":"support-routing""#),
        "the duplicate is the point"
    );
    let golden = &case.outputs[0];
    let out = String::from_utf8(pinned(&golden.golden, &golden.sha256).unwrap()).unwrap();
    assert!(out.contains("duplicate key \\\"name\\\""), "{out}");
}
