//! Spec 006, 3.5: `rustev run` end to end on both reference plans through
//! the synthetic rules programs: one record per termination, a rejection
//! with no record, evidence not delivered with the record embedded, setup
//! refusals, and usage checks made before admission. SYNTHETIC only.

mod common;

use common::*;
use rustev_cli::{Context, execute};
use rustev_contract::Document;
use rustev_contract::run::{RunRecord, Termination};
use serde_json::Value as Json;

fn golden_run(name: &str, s: &Scratch, stdout: &Json, record: &str) {
    let mut out = stdout.clone();
    out["receipt"] = "<normalized>".into();
    cli_golden(
        &format!("{name}.run.json"),
        format!("{}\n", serde_json::to_string(&out).unwrap()).as_bytes(),
    );
    let mut rec: Json = serde_json::from_slice(&s.read(record)).unwrap();
    normalize_timing(&mut rec);
    cli_golden(
        &format!("{name}.record.json"),
        format!("{}\n", serde_json::to_string(&rec).unwrap()).as_bytes(),
    );
}

#[test]
fn both_reference_plans_are_judged_with_a_delivered_record() {
    let s = Scratch::new("run-judged");
    run_inputs(&s);
    for task in ["support", "lodging"] {
        let record = format!("{task}.record.json");
        let r = s.rustev_owned(&run_args(task, &format!("{task}-1"), &record));
        let j = r.expect(0, "judged");
        assert_eq!(j["completion"], "judged");
        // The record on disk is the record canonical run record whose digest
        // is the receipt, and its judgment is the one printed.
        let bytes = s.read(&record);
        let rec = RunRecord::parse(&bytes).unwrap();
        assert_eq!(rec.record_canonical().unwrap(), bytes);
        assert_eq!(rec.termination, Termination::Judged);
        assert_eq!(
            j["receipt"],
            format!("file:{}", rec.record_digest().unwrap())
        );
        let rustev_contract::run::CoreEvidence::Recorded(ev) = &rec.core else {
            panic!("no evidence")
        };
        assert_eq!(
            serde_json::to_value(&ev.judgment).unwrap(),
            j["judgment"],
            "{task}"
        );
        golden_run(task, &s, &j, &record);
    }
    // Support routing is billing-priority, as the rules backend's own test.
    let j: Json = serde_json::from_slice(&s.read("support.record.json")).unwrap();
    let text = j.to_string();
    assert!(text.contains("billing-priority"), "{text}");
}

#[test]
fn a_raised_cancel_signal_delivers_a_cancelled_record() {
    let s = Scratch::new("run-cancelled");
    run_inputs(&s);
    let cancel = rustev_core::seams::CancelSignal::new();
    cancel.raise();
    let mut args = run_args("support", "s-cancel", "cancel.record.json");
    args.remove(0);
    for a in args.iter_mut() {
        if a.ends_with(".json") {
            *a = s.path(a).to_string_lossy().into_owned();
        }
    }
    let mut argv = vec!["run".to_string()];
    argv.extend(args);
    let e = execute(
        &argv,
        &Context {
            cancel,
            handle_signals: false,
        },
    );
    let j: Json = serde_json::from_slice(&e.stdout).unwrap();
    assert_eq!(
        (e.code, j["status"].as_str()),
        (7, Some("cancelled")),
        "{j}"
    );
    assert!(j.get("judgment").is_none());
    let rec = RunRecord::parse(&s.read("cancel.record.json")).unwrap();
    assert_eq!(rec.termination, Termination::Cancelled);
    assert_eq!(
        j["receipt"],
        format!("file:{}", rec.record_digest().unwrap())
    );
}

#[test]
fn a_snapshot_the_plan_cannot_start_on_is_rejected_without_a_record() {
    let s = Scratch::new("run-rejected");
    run_inputs(&s);
    let mut snap = support_snapshot_default();
    snap.entries.push(entry(
        "not.declared",
        serde_json::json!("x"),
        rustev_contract::definition::ProvenanceClass::UserSupplied,
        NOW,
    ));
    s.write("support.snapshot.json", &canonical(&snap));
    let r = s.rustev_owned(&run_args("support", "s-rej", "rej.record.json"));
    let j = r.expect(6, "rejected");
    assert_eq!(j["rejection"], "invalid_request");
    assert!(!s.exists("rej.record.json"));
}

#[cfg(unix)]
#[test]
fn an_unwritable_record_directory_is_evidence_not_delivered_with_the_record_embedded() {
    use std::os::unix::fs::PermissionsExt;
    let s = Scratch::new("run-undelivered");
    run_inputs(&s);
    std::fs::create_dir(s.path("locked")).unwrap();
    std::fs::set_permissions(s.path("locked"), std::fs::Permissions::from_mode(0o555)).unwrap();
    let r = s.rustev_owned(&run_args("support", "s-lost", "locked/r.json"));
    std::fs::set_permissions(s.path("locked"), std::fs::Permissions::from_mode(0o755)).unwrap();
    // Root can write anywhere; the case only holds for an ordinary user.
    if r.code == 0 {
        eprintln!("skipped: the directory was writable (running as root?)");
        return;
    }
    let j = r.expect(8, "evidence_not_delivered");
    assert_eq!(j["completion"], "judged");
    assert_eq!(j["uncertain"], false);
    let rec: RunRecord = serde_json::from_value(j["run_record"].clone()).unwrap();
    assert_eq!(rec.decision_id, "s-lost");
    assert!(!s.exists("locked/r.json"));
}

#[test]
fn an_existing_record_path_is_refused_before_any_decision() {
    let s = Scratch::new("run-exists");
    run_inputs(&s);
    s.write("taken.json", b"keep me");
    let r = s.rustev_owned(&run_args("support", "s-x", "taken.json"));
    assert_eq!(r.expect(3, "io_error")["path"], "taken.json");
    assert_eq!(s.read("taken.json"), b"keep me");
}

#[test]
fn setup_refusals_name_their_cause() {
    let s = Scratch::new("run-setup");
    run_inputs(&s);
    // The same program twice: one backend id.
    let mut a = run_args("support", "d", "r.json");
    a.extend(["--rules".into(), "support.rules.json".into()]);
    s.rustev_owned(&a).expect(4, "invalid_input");
    // The lodging program for the support plan: its descriptor is missing.
    let mut a = run_args("support", "d", "r.json");
    let i = a.iter().position(|x| x == "support.rules.json").unwrap();
    a[i] = "lodging.rules.json".into();
    assert_eq!(
        s.rustev_owned(&a).expect(5, "refused")["load_error"],
        "missing_descriptor"
    );
    // A malformed snapshot.
    s.write("bad.snapshot.json", b"{}");
    let mut a = run_args("support", "d", "r.json");
    let i = a.iter().position(|x| x == "support.snapshot.json").unwrap();
    a[i] = "bad.snapshot.json".into();
    assert_eq!(
        s.rustev_owned(&a).expect(4, "invalid_input")["input"],
        "bad.snapshot.json"
    );
    assert!(!s.exists("r.json"));
}

#[test]
fn a_program_whose_task_differs_from_the_plan_is_refused_by_check_plan() {
    let s = Scratch::new("run-check-plan");
    reference_inputs(&s);
    // Rename one task: the descriptor (operations and outputs) still serves
    // the plan, but the program cannot answer the step.
    let text = String::from_utf8(rules_fixture("support-routing.rules.json")).unwrap();
    let renamed = text.replacen("\"support.frustration\"", "\"support.anger\"", 1);
    assert_ne!(text, renamed);
    s.write("renamed.rules.json", renamed.as_bytes());
    let b = rustev_backend_rules::RulesBackend::from_bytes(renamed.as_bytes()).unwrap();
    let cal = rules_calibration(b.artifact(), "1.5");
    s.write("renamed.calibration.json", &canonical(&cal));
    let def = support_routing_builder(&cal).build().unwrap();
    s.write("renamed.definition.json", &canonical(&def));
    s.rustev(&[
        "plan",
        "compile",
        "--definition",
        "renamed.definition.json",
        "--rules",
        "renamed.rules.json",
        "--calibration",
        "renamed.calibration.json",
        "--out",
        "renamed.plan.json",
    ])
    .expect(0, "compiled");
    s.write(
        "support.snapshot.json",
        &canonical(&support_snapshot_default()),
    );
    let r = s.rustev(&[
        "run",
        "--plan",
        "renamed.plan.json",
        "--rules",
        "renamed.rules.json",
        "--calibration",
        "renamed.calibration.json",
        "--snapshot",
        "support.snapshot.json",
        "--evaluation-time",
        NOW_ARG,
        "--decision-id",
        "d",
        "--record-out",
        "r.json",
    ]);
    let j = r.expect(5, "refused");
    assert_eq!(j["check_plan"][0]["kind"], "no_task", "{j}");
    assert_eq!(j["check_plan"][0]["step"], "frustration");
    assert!(!s.exists("r.json"));
}

#[test]
fn run_usage_is_checked_before_admission() {
    let s = Scratch::new("run-usage");
    let base = run_args("support", "d", "r.json");
    let cases: Vec<(Vec<&str>, &str)> = vec![
        (vec!["--bundle-out", "b.json"], "only with --capture-bytes"),
        (vec!["--tenant", "t"], "only with --capture-bytes"),
        (vec!["--capture-bytes", "0"], "--capture-bytes"),
        (vec!["--capture-bytes", "16777217"], "--capture-bytes"),
        (vec!["--capture-bytes", "1024"], "--bundle-out is required"),
        (
            vec![
                "--capture-bytes",
                "1024",
                "--bundle-out",
                "b",
                "--now-ms",
                "1",
                "--tenant",
                "t",
                "--context-revision",
                "r",
            ],
            "--principal-scope is required",
        ),
        (
            vec![
                "--capture-bytes",
                "1024",
                "--bundle-out",
                "b",
                "--now-ms",
                "1",
                "--scope",
                "tenant-only",
                "--tenant",
                "t",
                "--context-revision",
                "r",
                "--principal-scope",
                "p",
            ],
            "not part of a tenant-only scope",
        ),
        (
            vec![
                "--capture-bytes",
                "1024",
                "--bundle-out",
                "b",
                "--now-ms",
                "1",
                "--scope",
                "tenant-only",
                "--tenant",
                "t",
                "--context-revision",
                "r",
                "--retain",
                "external",
            ],
            "--store is required",
        ),
        (
            vec![
                "--capture-bytes",
                "1024",
                "--bundle-out",
                "b",
                "--now-ms",
                "1",
                "--scope",
                "tenant-only",
                "--tenant",
                "t",
                "--context-revision",
                "r",
                "--store",
                "st",
            ],
            "only with --retain external",
        ),
        (
            vec![
                "--capture-bytes",
                "1024",
                "--bundle-out",
                "b",
                "--now-ms",
                "1",
                "--scope",
                "tenant-only",
                "--tenant",
                "t",
                "--context-revision",
                "r",
                "--host-cap-ms",
                "1000",
            ],
            "exceeds the host cap",
        ),
        (
            vec![
                "--capture-bytes",
                "1024",
                "--bundle-out",
                "b",
                "--now-ms",
                "1",
                "--scope",
                "tenant-only",
                "--tenant",
                "",
                "--context-revision",
                "r",
            ],
            "--tenant",
        ),
        (
            vec!["--max-parallel-requests", "65"],
            "--max-parallel-requests",
        ),
        (vec!["--sink-timeout-ms", "0"], "--sink-timeout-ms"),
    ];
    for (extra, needle) in cases {
        let mut a = base.clone();
        a.extend(extra.iter().map(|x| x.to_string()));
        let r = s.rustev_owned(&a);
        assert_eq!(r.code, 2, "{extra:?}: {}", r.stdout_str());
        assert!(r.stderr.contains(needle), "{extra:?}: {}", r.stderr);
    }
    let mut a = base.clone();
    let i = a.iter().position(|x| x == "d").unwrap();
    a[i] = "x".repeat(257);
    assert_eq!(s.rustev_owned(&a).code, 2);
}

#[test]
fn too_many_rules_files_are_refused_before_any_read() {
    let s = Scratch::new("run-count");
    let mut a = run_args("support", "d", "r.json");
    for _ in 0..64 {
        a.extend(["--rules".into(), "absent.rules.json".into()]);
    }
    let j = s.rustev_owned(&a).expect(3, "io_error");
    assert_eq!(j["path"], "--rules");
    assert!(j["detail"].as_str().unwrap().contains("65 files"), "{j}");
}

#[test]
fn a_bundle_failure_after_the_decision_keeps_the_decision_fields() {
    let s = Scratch::new("run-bundle-fail");
    run_inputs(&s);
    let mut a = run_args("support", "b-1", "b.record.json");
    a.extend(
        [
            "--capture-bytes", "1048576", "--bundle-out", "missing-dir/b.json", "--now-ms",
            "1800000000000", "--tenant", "t", "--context-revision", "r", "--principal-scope", "p",
            "--retain", "embedded",
        ]
        .iter()
        .map(|x| x.to_string()),
    );
    let j = s.rustev_owned(&a).expect(3, "io_error");
    assert_eq!(j["completion"], "judged");
    assert_eq!(j["record"], "b.record.json");
    assert!(j.get("judgment").is_some());
    assert_eq!(j["bundle_error"]["path"], "missing-dir/b.json");
    assert!(s.exists("b.record.json"));
}
