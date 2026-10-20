//! Spec 033, 3.3 to 3.7 and section 5's evaluation rows: checks and their
//! consequences, terminal precedence, `provenance_within`, `needs_input`,
//! and the action of every outcome and notice in `rustev.judgment/2`.
//! SYNTHETIC definitions and snapshots only.

use rustev_contract::Document;
use rustev_contract::actionability::{
    Cause, CauseProvenance, CheckStatus, Override, OverrideBasis, Party, Remediation,
};
use rustev_contract::definition::{Definition, ProvenanceClass as P};
use rustev_contract::evidence::EvidenceRecord;
use rustev_contract::judgment::{Judgment, NoticeKind, Outcome, Unresolved};
use rustev_contract::snapshot::Snapshot;
use rustev_contract::time::Timestamp;
use rustev_core::{Evaluation, compile};

const NOW: i64 = 1_800_000_000_000;
const PAST: i64 = NOW - 86_400_000;
const FUTURE: i64 = NOW + 86_400_000;

fn base() -> serde_json::Value {
    serde_json::json!({
        "schema": "rustev.definition/2",
        "name": "trip-search", "version": "1.0.0", "package": "synthetic",
        "inputs": [
            { "name": "trip.window",
              "type": { "record": { "fields": [{ "name": "start", "type": "timestamp" }] } },
              "provenance": ["user-supplied", "model-derived"], "freshness": "not_required" },
            { "name": "account.region", "type": { "text": { "max_bytes": 16 } },
              "provenance": ["system-of-record"], "freshness": { "max_age_ms": 1000 } },
            { "name": "trip.party", "type": "integer",
              "provenance": ["user-supplied", "third-party"], "freshness": "not_required" }
        ],
        "steps": [],
        "policy": {
            "on_unresolved": [
                { "target": "input:trip.window", "reasons": "any", "action": "as_unmet" }],
            "rules": [
                { "id": "dated", "when": { "exact": { "present": "input:trip.window" } },
                  "then": { "propose": { "action": "search_dated", "params": [] } } },
                { "id": "undated", "when": "always",
                  "then": { "propose": { "action": "search_undated", "params": [] } } }
            ],
            "adjustments": []
        },
        "limits": { "max_semantic_requests": 0, "on_excess": "refuse", "max_projection_bytes": 1024,
                    "distribution_tolerance": "0.01", "deadline_ms": 1000 },
        "actionability": {
            "caller_controlled": ["user-supplied"],
            "remediations": [{ "code": "send_future_window", "for": "caller", "params": [] }],
            "questions": [{ "id": "ask_window", "fields": ["trip.window"], "text": "When do you travel?" }],
            "constraint_groups": [],
            "checks": [{
                "id": "future_window", "subject": "trip.window",
                "test": { "cmp": { "left": { "ref": "input:trip.window/start" }, "op": "gt",
                                   "right": { "ref": "now" } } },
                "on_fail": {
                    "by_provenance": {
                        "user-supplied": { "refuse": { "remediation": "send_future_window" } },
                        "model-derived": { "drop_field": { "notice": "inferred_window_dropped" } } },
                    "default": "none" } }]
        }
    })
}

fn window(start: i64, class: &str) -> serde_json::Value {
    serde_json::json!({ "field": "trip.window", "value": { "start": start }, "provenance": class,
                        "as_of_ms": NOW - 10, "source": "s" })
}

fn region(age: i64) -> serde_json::Value {
    serde_json::json!({ "field": "account.region", "value": "eu", "provenance": "system-of-record",
                        "as_of_ms": NOW - age, "source": "crm" })
}

fn decide(def: &serde_json::Value, entries: Vec<serde_json::Value>) -> (Judgment, EvidenceRecord) {
    let def = Definition::parse(&serde_json::to_vec(def).unwrap()).unwrap();
    let compiled = compile(&def, &[], &[]).expect("compiles");
    let snapshot = Snapshot::parse(
        &serde_json::to_vec(
            &serde_json::json!({ "schema": "rustev.snapshot/2", "entries": entries }),
        )
        .unwrap(),
    )
    .unwrap();
    let ev = Evaluation::start(&compiled, &snapshot, Timestamp::from_ms(NOW).unwrap()).unwrap();
    assert!(ev.pending().is_empty(), "no semantic request");
    let (j, e) = ev.finish("d-1").unwrap();
    assert_eq!(j.schema, "rustev.judgment/2");
    // Both documents parse back from their canonical bytes.
    Judgment::parse(&j.record_canonical().unwrap()).unwrap();
    EvidenceRecord::parse(&e.record_canonical().unwrap()).unwrap();
    (j, e)
}

fn observed(field: &str, class: P) -> Cause {
    Cause::Input {
        field: field.into(),
        provenance: CauseProvenance::Observed { class },
    }
}

#[test]
fn a_caller_supplied_past_window_is_refused_and_addressed_to_the_caller() {
    let (j, e) = decide(&base(), vec![window(PAST, "user-supplied")]);
    let Outcome::Refused {
        refusals,
        addressed_to,
    } = &j.outcome
    else {
        panic!("{:?}", j.outcome)
    };
    assert_eq!(*addressed_to, Party::Caller);
    let r = &refusals[0];
    assert_eq!(
        (r.check.as_str(), r.subject.as_str()),
        ("future_window", "trip.window")
    );
    assert_eq!(
        (r.action.addressed_to, r.action.computed),
        (Party::Caller, Party::Caller)
    );
    assert_eq!(
        r.action.remediation,
        Remediation::code("send_future_window")
    );
    assert_eq!(
        r.action.caused_by,
        vec![observed("trip.window", P::UserSupplied)]
    );
    assert!(!r.action.transient);
    assert_eq!(r.action.override_, Override::None);
    // No step ran and the policy did not run.
    assert_eq!(j.trace, ["check:future_window"]);
    let checks = e.checks.unwrap();
    assert!(
        matches!(&checks[0].status, CheckStatus::Failed { class: P::UserSupplied, consequence } if consequence == "refuse")
    );
}

#[test]
fn a_model_derived_past_window_is_dropped_and_the_decision_continues() {
    let (j, e) = decide(&base(), vec![window(PAST, "model-derived")]);
    assert!(matches!(&j.outcome, Outcome::Propose { action, .. } if action == "search_undated"));
    let n = j
        .notices
        .iter()
        .find(|n| n.kind == NoticeKind::FieldDropped)
        .unwrap();
    assert_eq!(
        (n.subject.as_str(), n.reason.as_str()),
        ("input:trip.window", "inferred_window_dropped")
    );
    let a = n.action.as_ref().unwrap();
    assert_eq!(a.addressed_to, Party::Host);
    assert_eq!(a.caused_by, vec![observed("trip.window", P::ModelDerived)]);
    let input = e.inputs.iter().find(|i| i.field == "trip.window").unwrap();
    assert_eq!(
        input.dropped.as_ref().map(|d| d.check.as_str()),
        Some("future_window")
    );
}

#[test]
fn a_future_window_passes() {
    let (j, e) = decide(&base(), vec![window(FUTURE, "user-supplied")]);
    assert!(matches!(&j.outcome, Outcome::Propose { action, .. } if action == "search_dated"));
    assert_eq!(e.checks.unwrap()[0].status, CheckStatus::Passed);
}

#[test]
fn a_missing_subject_is_not_evaluated() {
    let (j, e) = decide(&base(), vec![]);
    assert!(matches!(&j.outcome, Outcome::Propose { action, .. } if action == "search_undated"));
    assert!(matches!(
        &e.checks.unwrap()[0].status,
        CheckStatus::NotEvaluated {
            reason: Unresolved::MissingEvidence { .. }
        }
    ));
}

#[test]
fn a_test_reading_a_stale_input_is_unevaluable_and_its_subject_stale() {
    let mut def = base();
    def["actionability"]["checks"][0]["test"] = serde_json::json!({ "all": [
        { "cmp": { "left": { "ref": "input:trip.window/start" }, "op": "gt", "right": { "ref": "now" } } },
        { "present": "input:account.region" } ] });
    def["actionability"]["remediations"][0]["for"] = serde_json::json!("upstream");
    def["policy"]["on_unresolved"] = serde_json::json!([
        { "target": "input:trip.window", "reasons": "any", "action": "propagate" }]);
    let (j, e) = decide(&def, vec![window(FUTURE, "user-supplied"), region(5000)]);
    assert!(matches!(&e.checks.unwrap()[0].status,
        CheckStatus::Unevaluable { reason: Unresolved::StaleEvidence { fields } } if fields == &["account.region"]));
    let Outcome::UnresolvedAction { reason, action } = &j.outcome else {
        panic!("{:?}", j.outcome)
    };
    assert_eq!(reason, &Unresolved::stale(["account.region".to_string()]));
    assert_eq!(action.addressed_to, Party::Upstream);
    assert_eq!(
        action.caused_by,
        vec![observed("account.region", P::SystemOfRecord)]
    );
}

#[test]
fn the_first_terminal_check_decides_and_refusals_are_all_listed() {
    let mut def = base();
    let check = |id: &str, k: serde_json::Value| {
        serde_json::json!({
        "id": id, "subject": "trip.window",
        "test": { "cmp": { "left": { "ref": "input:trip.window/start" }, "op": "gt", "right": { "ref": "now" } } },
        "on_fail": {
            "by_provenance": {
                "user-supplied": k,
                "model-derived": { "continue_with_notice": { "notice": "inferred" } } },
            "default": "none" } })
    };
    def["actionability"]["checks"] = serde_json::json!([
        check(
            "noted",
            serde_json::json!({ "continue_with_notice": { "notice": "past_window" } })
        ),
        check(
            "second",
            serde_json::json!({ "refuse": { "remediation": "send_future_window" } })
        ),
        check(
            "third",
            serde_json::json!({ "refuse": { "remediation": "send_future_window" } })
        ),
    ]);
    let (j, _) = decide(&def, vec![window(PAST, "user-supplied")]);
    let Outcome::Refused { refusals, .. } = &j.outcome else {
        panic!("{:?}", j.outcome)
    };
    let ids: Vec<&str> = refusals.iter().map(|r| r.check.as_str()).collect();
    assert_eq!(ids, ["second", "third"]);
    assert!(
        j.notices
            .iter()
            .any(|n| n.kind == NoticeKind::CheckNotice && n.reason == "past_window")
    );
}

#[test]
fn an_abstain_consequence_is_addressed_to_no_one() {
    let mut def = base();
    def["actionability"]["checks"][0]["on_fail"]["by_provenance"]["user-supplied"] =
        serde_json::json!("abstain");
    let (j, _) = decide(&def, vec![window(PAST, "user-supplied")]);
    let Outcome::UnresolvedAction { reason, action } = &j.outcome else {
        panic!("{:?}", j.outcome)
    };
    assert_eq!(
        reason,
        &Unresolved::Abstained {
            rule: "future_window".into()
        }
    );
    assert_eq!(
        action.caused_by,
        vec![Cause::Policy {
            site: "check:future_window".into()
        }]
    );
    assert_eq!(action.addressed_to, Party::None);
    assert!(!action.transient);
}

#[test]
fn a_declared_override_is_recorded_beside_the_computed_party() {
    let mut def = base();
    def["actionability"]["checks"][0]["on_fail"]["by_provenance"]["model-derived"] = serde_json::json!(
        { "drop_field": { "notice": "inferred_window_dropped",
                          "address": { "to": "upstream", "reason": "partner_outage" } } });
    let (j, _) = decide(&def, vec![window(PAST, "model-derived")]);
    let a = j
        .notices
        .iter()
        .find(|n| n.kind == NoticeKind::FieldDropped)
        .unwrap()
        .action
        .clone()
        .unwrap();
    assert_eq!((a.addressed_to, a.computed), (Party::Upstream, Party::Host));
    assert_eq!(
        a.override_,
        Override::Applied {
            to: Party::Upstream,
            basis: OverrideBasis::Declared,
            reason: "partner_outage".into()
        }
    );
}

#[test]
fn needs_input_carries_literal_questions_and_a_reproducible_continuation() {
    let mut def = base();
    def["policy"]["rules"] = serde_json::json!([
        { "id": "ask", "when": "always",
          "then": { "needs_input": { "fields": ["trip.window"], "questions": ["ask_window"] } } }]);
    let (j, _) = decide(&def, vec![]);
    let Outcome::NeedsInput(n) = &j.outcome else {
        panic!("{:?}", j.outcome)
    };
    assert_eq!(n.questions[0].text, "When do you travel?");
    assert_eq!(n.action.addressed_to, Party::Caller);
    // Recomputed independently from 3.7.4's definition.
    let bytes = rustev_contract::canonical::record_canonical_bytes(&serde_json::json!({
        "plan_id": j.plan_id.to_string(), "snapshot_id": j.snapshot_id.to_string(),
        "evaluation_time_ms": NOW, "site": "rule:ask",
        "fields": ["trip.window"], "questions": ["ask_window"] }))
    .unwrap();
    let want = rustev_contract::canonical::tagged_digest("rustev.continuation/1", &bytes);
    assert_eq!(n.continuation.to_string(), want);
}

#[test]
fn provenance_within_branches_on_the_accepted_entry() {
    let mut def = base();
    def["actionability"]["checks"] = serde_json::json!([]);
    def["policy"]["rules"] = serde_json::json!([
        { "id": "from_caller",
          "when": { "provenance_within": { "ref": "input:trip.window", "classes": ["user-supplied"] } },
          "then": { "propose": { "action": "caller_window", "params": [] } } },
        { "id": "other", "when": "always", "then": { "propose": { "action": "other", "params": [] } } }]);
    def["policy"]["on_unresolved"] = serde_json::json!([
        { "target": "input:trip.window", "reasons": "any", "action": "as_unmet" }]);
    let (j, _) = decide(&def, vec![window(FUTURE, "user-supplied")]);
    assert!(matches!(&j.outcome, Outcome::Propose { action, .. } if action == "caller_window"));
    let (j, _) = decide(&def, vec![window(FUTURE, "model-derived")]);
    assert!(matches!(&j.outcome, Outcome::Propose { action, .. } if action == "other"));
}

#[test]
fn a_remediation_parameter_reading_an_unresolved_input_is_none() {
    let mut def = base();
    def["actionability"]["remediations"] = serde_json::json!([
        { "code": "send_future_window", "for": "caller", "params": [{ "name": "party", "type": "integer" }] }]);
    def["actionability"]["checks"][0]["on_fail"]["by_provenance"]["user-supplied"] = serde_json::json!(
        { "refuse": { "remediation": { "code": "send_future_window",
                                       "params": { "party": { "ref": "input:trip.party" } } } } });
    let (j, _) = decide(&def, vec![window(PAST, "user-supplied")]);
    let Outcome::Refused { refusals, .. } = &j.outcome else {
        panic!("{:?}", j.outcome)
    };
    assert_eq!(refusals[0].action.remediation, Remediation::None);
    assert_eq!(
        refusals[0].action.addressed_to,
        Party::Caller,
        "the action is otherwise unchanged"
    );
}
