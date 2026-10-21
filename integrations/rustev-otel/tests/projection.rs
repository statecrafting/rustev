//! Spec 028, 3.5 and section 5: the projection's rows beyond the goldens:
//! timing, status, unregistered plans, records it does not know, event
//! and value bounds, and canaries in default-deny fields. SYNTHETIC only.

mod common;

use std::collections::BTreeMap;

use common::*;
use rustev_contract::execution::FailureClass;
use rustev_contract::judgment::{Notice, NoticeKind, Outcome, Unresolved};
use rustev_contract::redaction::{Action, FieldOverride, RedactionPolicy, Redactor};
use rustev_contract::run::{AttemptEnd, CoreEvidence, RunRecord, Termination, Transition};
use rustev_contract::trace::{Mark, SpanKind, SpanStatus, TraceLine, TraceValue};
use rustev_otel::jsonl::{JsonlError, JsonlExporter, encode};
use rustev_otel::{Anchor, AnchorKind, Bindings, ProjectionInput, RootIds, SeededIds, project};

fn default_project(rec: &RunRecord, with_plan: bool) -> rustev_otel::Projected {
    let plan = plan("support");
    project_golden(
        rec,
        with_plan.then_some(&plan),
        &BTreeMap::new(),
        &Bindings::default(),
        RedactionPolicy::default(),
    )
}

fn attr<'a>(l: &'a TraceLine, k: &str) -> Option<&'a TraceValue> {
    l.attributes.get(k).map(|a| &a.v)
}

fn s(v: &str) -> TraceValue {
    TraceValue::Str(v.into())
}

fn evidence(rec: &mut RunRecord) -> &mut rustev_contract::evidence::EvidenceRecord {
    match &mut rec.core {
        CoreEvidence::Recorded(e) => e,
        CoreEvidence::NotProduced => panic!("no evidence"),
    }
}

#[test]
fn the_tree_has_root_steps_attempts_and_policy_last() {
    let p = default_project(&record("support"), true);
    let names: Vec<&str> = p.lines.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names[0], "rustev.decision");
    assert_eq!(*names.last().unwrap(), "rustev.policy");
    let root = &p.lines[0];
    assert!(root.resource.is_some() && root.parent_span_id.is_none());
    for l in &p.lines[1..] {
        assert!(l.resource.is_none());
        assert_eq!(l.trace_id, root.trace_id);
        if l.name == "rustev.attempt" {
            assert_ne!(l.parent_span_id.as_deref(), Some(root.span_id.as_str()));
        } else {
            assert_eq!(l.parent_span_id.as_deref(), Some(root.span_id.as_str()));
        }
    }
    // One step per evidence entry (7) and one attempt per attempt (3).
    assert_eq!(names.iter().filter(|n| **n == "rustev.step").count(), 7);
    assert_eq!(names.iter().filter(|n| **n == "rustev.attempt").count(), 3);
}

#[test]
fn attempts_and_semantic_steps_are_timed_and_the_rest_are_zero_length() {
    let mut rec = record("support");
    rec.timing.queued_ms = 2;
    rec.timing.elapsed_ms = 40;
    for (i, r) in rec.requests.iter_mut().enumerate() {
        for a in &mut r.attempts {
            a.dispatched_ms = 5 + i as u64;
            a.ended_ms = 20 + i as u64;
        }
    }
    let p = default_project(&rec, true);
    let ms = 1_000_000;
    let root = &p.lines[0];
    assert_eq!(root.start_unix_nanos, ANCHOR_NANOS + 2 * ms);
    assert_eq!(root.end_unix_nanos, ANCHOR_NANOS + 40 * ms);
    for l in &p.lines[1..] {
        let not_recorded = attr(l, "rustev.timing") == Some(&s("not_recorded"));
        let exact = attr(l, "rustev.step.kind") == Some(&s("exact"));
        if l.name == "rustev.policy" || exact {
            assert!(not_recorded, "{}", l.name);
            assert_eq!(l.start_unix_nanos, root.end_unix_nanos);
            assert_eq!(l.end_unix_nanos, root.end_unix_nanos);
        } else {
            assert!(!not_recorded);
            assert!(l.end_unix_nanos > l.start_unix_nanos);
        }
    }
    let topic_attempt = p.lines.iter().find(|l| l.name == "rustev.attempt").unwrap();
    assert_eq!(topic_attempt.start_unix_nanos, ANCHOR_NANOS + 5 * ms);
    assert_eq!(topic_attempt.end_unix_nanos, ANCHOR_NANOS + 20 * ms);
    assert_eq!(topic_attempt.kind, SpanKind::Internal);
}

#[test]
fn an_unregistered_plan_exports_unknown_step_kinds() {
    let p = default_project(&record("support"), false);
    for l in p.lines.iter().filter(|l| l.name == "rustev.step") {
        assert_eq!(attr(l, "rustev.step.kind"), Some(&s("unknown")));
        assert!(attr(l, "rustev.step.operator").is_none());
    }
}

#[test]
fn an_unresolved_outcome_is_an_answer_with_its_reason() {
    let mut rec = record("support");
    evidence(&mut rec).judgment.outcome = Outcome::Unresolved(Unresolved::invalid(
        ["ticket.message".to_string()],
        "CANARY-unresolved-detail",
    ));
    let p = default_project(&rec, true);
    let root = &p.lines[0];
    assert_eq!(root.status, SpanStatus::Unset);
    assert_eq!(attr(root, "rustev.outcome"), Some(&s("unresolved")));
    assert_eq!(
        attr(root, "rustev.outcome.unresolved_reason"),
        Some(&s("invalid_input"))
    );
    assert_eq!(
        attr(root, "rustev.outcome.unresolved_fields"),
        Some(&TraceValue::StrArray(vec!["ticket.message".into()]))
    );
    assert!(attr(root, "rustev.outcome.unresolved_detail").is_none());
    assert_eq!(
        p.facts.outcome_unresolved_reason.as_deref(),
        Some("invalid_input")
    );
    let text = String::from_utf8(encode(&p.lines).unwrap()).unwrap();
    assert!(!text.contains("CANARY"));
}

#[test]
fn a_failed_attempt_is_an_error_and_its_detail_is_withheld() {
    let mut rec = record("support");
    let r = &mut rec.requests[0];
    r.attempts[0].end = AttemptEnd::Failed {
        class: FailureClass::Overloaded,
        detail: "CANARY-failure-detail".into(),
    };
    r.transitions.push(Transition::Stop {
        after: 1,
        reason: "CANARY-stop-reason".into(),
    });
    r.transitions.push(Transition::Fallback {
        after: 1,
        class: FailureClass::Overloaded,
        to: 1,
    });
    let p = default_project(&rec, true);
    let a = p.lines.iter().find(|l| l.name == "rustev.attempt").unwrap();
    assert_eq!(a.status, SpanStatus::Error);
    assert_eq!(attr(a, "error.type"), Some(&s("overloaded")));
    assert_eq!(
        attr(a, "rustev.attempt.failure_class"),
        Some(&s("overloaded"))
    );
    assert!(attr(a, "rustev.attempt.failure_detail").is_none());
    let step = p
        .lines
        .iter()
        .find(|l| attr(l, "rustev.step.id") == Some(&s("topic")))
        .unwrap();
    assert_eq!(step.events.len(), 2);
    assert!(!step.events[0].attributes.contains_key("reason"));
    assert_eq!(p.facts.fallbacks, 1);
    let text = String::from_utf8(encode(&p.lines).unwrap()).unwrap();
    assert!(!text.contains("CANARY"), "{text}");
    assert!(text.contains("rustev.transition.reason"));
}

#[test]
fn canaries_in_default_deny_and_digest_slots_never_appear_raw() {
    let mut rec = record("support");
    rec.decision_id = "CANARY-decision".into();
    {
        let ev = evidence(&mut rec);
        if let rustev_contract::evidence::InputStatus::Resolved { source, .. } =
            &mut ev.inputs[0].status
        {
            *source = "CANARY-source".into();
        }
        if let Outcome::Propose { params, .. } = &mut ev.judgment.outcome {
            params.insert(
                "note".into(),
                rustev_contract::judgment::OutValue::Text("CANARY-param".into()),
            );
        }
    }
    let p = default_project(&rec, true);
    let text = String::from_utf8(encode(&p.lines).unwrap()).unwrap();
    assert!(!text.contains("CANARY"), "{text}");
    assert!(text.contains("hmac-sha256:"));
    assert!(text.contains("rustev.outcome.param.note"));
}

#[test]
fn a_record_it_does_not_know_is_projection_unknown() {
    let mut rec = record("support");
    rec.schema = "rustev.run/3".into();
    let redactor = Redactor::new(RedactionPolicy::default(), Some(test_key())).unwrap();
    let none = BTreeMap::new();
    let b = Bindings::default();
    let input = |rec| ProjectionInput {
        record: rec,
        plan: None,
        exchanges: &none,
        bindings: &b,
        anchor: Anchor {
            unix_nanos: ANCHOR_NANOS,
            kind: AnchorKind::Begin,
        },
        root: RootIds {
            trace_id: [1; 16],
            span_id: [2; 8],
            parent_span_id: Some([3; 8]),
        },
        value_bound: 4096,
    };
    let mut ids = SeededIds::new(b"x");
    assert!(project(&input(&rec), &redactor, &mut ids).is_err());
    // A step absent from the registered plan is unknown, not guessed.
    let mut rec = record("support");
    evidence(&mut rec).steps[0].step = "not_in_plan".into();
    let plan = plan("support");
    let mut i = input(&rec);
    i.plan = Some(&plan);
    assert!(project(&i, &redactor, &mut ids).is_err());
    // A host parent and the begin anchor are recorded.
    let rec = record("support");
    let p = project(&input(&rec), &redactor, &mut ids).unwrap();
    assert_eq!(
        p.lines[0].parent_span_id.as_deref(),
        Some("0303030303030303")
    );
    assert_eq!(attr(&p.lines[0], "rustev.time_anchor"), Some(&s("begin")));
}

#[test]
fn a_cancelled_decision_without_evidence_keeps_its_requests() {
    let mut rec = record("support");
    rec.termination = Termination::Cancelled;
    rec.core = CoreEvidence::NotProduced;
    let p = default_project(&rec, true);
    assert_eq!(
        attr(&p.lines[0], "rustev.outcome"),
        Some(&s("not_produced"))
    );
    assert_eq!(
        attr(&p.lines[0], "rustev.termination"),
        Some(&s("cancelled"))
    );
    assert_eq!(
        p.lines.iter().filter(|l| l.name == "rustev.step").count(),
        rec.requests.len()
    );
    assert_eq!(
        p.lines
            .iter()
            .filter(|l| l.name == "rustev.attempt")
            .count(),
        3
    );
}

#[test]
fn events_are_capped_at_128_per_span_and_values_at_the_bound() {
    let mut rec = record("support");
    {
        let ev = evidence(&mut rec);
        ev.judgment.notices = (0..200)
            .map(|i| Notice {
                kind: NoticeKind::Truncation,
                subject: format!("s{i}"),
                reason: "r".into(),
                action: None,
            })
            .collect();
        if let Outcome::Propose { action, .. } = &mut ev.judgment.outcome {
            *action = "é".repeat(3000);
        }
    }
    let p = default_project(&rec, true);
    let root = &p.lines[0];
    assert_eq!(root.events.len(), 128);
    // 3 inputs and 200 notices.
    assert_eq!(attr(root, "rustev.events_cut"), Some(&TraceValue::Int(75)));
    assert_eq!(p.events_cut, 75);
    let a = &root.attributes["rustev.outcome.action"];
    assert_eq!(a.r, Mark::Truncated);
    assert_eq!(a.v.as_str().unwrap().len(), 4096);
    assert_eq!(p.attributes_cut, 1);
}

#[test]
fn a_field_allowed_by_name_is_exported() {
    let mut p = RedactionPolicy::default();
    p.fields.push(FieldOverride {
        field: "rustev.outcome.param.failed_payments_30d".into(),
        action: Action::Allow,
    });
    let out = project_golden(
        &record("support"),
        None,
        &BTreeMap::new(),
        &Bindings::default(),
        p,
    );
    assert_eq!(
        attr(&out.lines[0], "rustev.outcome.param.failed_payments_30d"),
        Some(&TraceValue::Int(2))
    );
}

#[test]
fn the_jsonl_exporter_appends_privately_and_stops_at_its_bound() {
    let dir = std::env::temp_dir().join(format!("rustev-otel-jsonl-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("trace.jsonl");
    let lines = default_project(&record("support"), true).lines;
    let one = encode(&lines).unwrap().len() as u64;
    let x = JsonlExporter::open(&path, one * 2 + 1).unwrap();
    assert_eq!(x.export(&lines).unwrap(), one);
    assert_eq!(x.export(&lines).unwrap(), one);
    assert_eq!(x.export(&lines), Err(JsonlError::FileFull));
    assert_eq!(std::fs::read(&path).unwrap().len() as u64, one * 2);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    // Reopening counts the bytes already there.
    let x = JsonlExporter::open(&path, one * 2 + 1).unwrap();
    assert_eq!(x.export(&lines), Err(JsonlError::FileFull));
    let _ = std::fs::remove_dir_all(&dir);
}
