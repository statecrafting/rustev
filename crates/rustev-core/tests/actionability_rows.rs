//! Spec 033, the remaining rows of section 5 and the acceptance items not
//! covered elsewhere: the computed party of every cause class (a golden
//! table), `downgrade_grade` on graded subjects (including a `hint` value),
//! `relax` and the `relaxed` status, `escalate`, mixed missing fields,
//! `provenance_within` on spec 032 step values, and the future-window
//! example as judgment 2 and evidence 2 goldens. SYNTHETIC only.

mod common;
mod eq;

use rustev_contract::Document;
use rustev_contract::actionability::{Cause, CauseProvenance, CheckStatus, Origin, Party};
use rustev_contract::definition::{Definition, ProvenanceClass as P};
use rustev_contract::evidence::EvidenceRecord;
use rustev_contract::grade::ValueGrade;
use rustev_contract::judgment::{Judgment, NoticeKind, Outcome, Unresolved};
use rustev_contract::snapshot::Snapshot;
use rustev_contract::time::Timestamp;
use rustev_core::{Evaluation, compile, compile_with_sources, computed_party};
use serde_json::{Value, json};

const CLASSES: [P; 6] = [
    P::AuthenticatedAppField,
    P::SystemOfRecord,
    P::UserSupplied,
    P::ThirdParty,
    P::AttributedClaim,
    P::ModelDerived,
];

fn input(class: P, observed: bool) -> Cause {
    Cause::Input {
        field: "f".into(),
        provenance: if observed {
            CauseProvenance::Observed { class }
        } else {
            CauseProvenance::Accepted {
                classes: vec![class],
            }
        },
    }
}

#[test]
fn the_computed_party_of_every_cause_class() {
    let mut rows = Vec::new();
    let mut row = |name: String, causes: Vec<Cause>, cc: &[P]| {
        let party = computed_party(&causes, cc);
        rows.push(
            json!({ "case": name, "caller_controlled": cc, "causes": causes, "computed": party }),
        );
        party
    };
    for class in CLASSES {
        for declared in [false, true] {
            let cc: Vec<P> = if declared { vec![class] } else { vec![] };
            for observed in [true, false] {
                let p = row(
                    format!(
                        "{} {class:?}, {}",
                        if observed { "observed" } else { "accepted" },
                        if declared {
                            "caller-controlled"
                        } else {
                            "not caller-controlled"
                        }
                    ),
                    vec![input(class, observed)],
                    &cc,
                );
                let want = match (declared, class) {
                    (true, _) => Party::Caller,
                    (false, P::ThirdParty | P::SystemOfRecord) => Party::Upstream,
                    _ => Party::Host,
                };
                assert_eq!(p, want, "{class:?} declared={declared}");
            }
        }
    }
    let cc = [P::UserSupplied];
    let accepted_mixed = Cause::Input {
        field: "f".into(),
        provenance: CauseProvenance::Accepted {
            classes: vec![P::UserSupplied, P::ModelDerived],
        },
    };
    assert_eq!(
        row(
            "accepted, caller and host classes".into(),
            vec![accepted_mixed],
            &cc
        ),
        Party::Host
    );
    for (origin, want) in [
        (Origin::Upstream, Party::Upstream),
        (Origin::Host, Party::Host),
    ] {
        let c = Cause::Backend {
            step: "s".into(),
            origin,
        };
        assert_eq!(row(format!("backend {origin:?}"), vec![c], &cc), want);
    }
    let budget = Cause::Budget {
        resource: "semantic_requests".into(),
    };
    assert_eq!(row("budget".into(), vec![budget], &cc), Party::Host);
    assert_eq!(
        row("deadline".into(), vec![Cause::Deadline], &cc),
        Party::Host
    );
    let capability = Cause::Capability { name: "s".into() };
    assert_eq!(row("capability".into(), vec![capability], &cc), Party::Host);
    let policy = Cause::Policy {
        site: "rule:r".into(),
    };
    assert_eq!(row("policy".into(), vec![policy.clone()], &cc), Party::None);
    assert_eq!(row("no cause".into(), vec![], &cc), Party::None);
    // Mixed causes combine toward upstream.
    let caller = input(P::UserSupplied, true);
    assert_eq!(
        row(
            "caller and policy".into(),
            vec![caller.clone(), policy],
            &cc
        ),
        Party::Caller
    );
    assert_eq!(
        row(
            "caller and deadline".into(),
            vec![caller.clone(), Cause::Deadline],
            &cc
        ),
        Party::Host
    );
    assert_eq!(
        row(
            "caller, host and upstream".into(),
            vec![caller, Cause::Deadline, input(P::ThirdParty, true)],
            &cc
        ),
        Party::Upstream
    );
    common::golden(
        "actionability.addressing.json",
        format!("{}\n", serde_json::to_string_pretty(&rows).unwrap()).as_bytes(),
    );
}

// ---- graded subjects (spec 032 fixtures) ----

const NOW: i64 = 2_000_000_000_000;

fn district(name: &str, grade: Option<&str>) -> Value {
    let mut e = json!({"field": "district", "value": {"name": name, "kind": "centre"},
        "provenance": "third-party", "as_of_ms": NOW - 1_000, "source": "geocoder"});
    if let Some(g) = grade {
        e["grade"] = json!(g);
    }
    e
}

/// The evidence-quality definition with a check on the geocoded district
/// whose third-party branch is `k`, and a step reading the district.
fn graded(k: Value) -> Value {
    let mut v = eq::base();
    v["steps"].as_array_mut().unwrap().push(json!({"id": "named", "body": {"exact": {
        "op": "compare", "version": 1, "args": {"predicate": {"cmp": {
            "left": {"ref": "input:district/name"}, "op": "eq", "right": {"lit": {"text": "old-town"}}}}}}}}));
    v["actionability"] = json!({
        "caller_controlled": ["user-supplied"], "remediations": [], "questions": [],
        "constraint_groups": [],
        "checks": [{ "id": "known_district", "subject": "district",
            "test": { "cmp": { "left": { "ref": "input:district/name" }, "op": "ne",
                               "right": { "lit": { "text": "nowhere" } } } },
            "on_fail": { "by_provenance": { "third-party": k }, "default": "none" } }]
    });
    v
}

fn run_graded(v: &Value, entries: Vec<Value>) -> (Judgment, EvidenceRecord) {
    let c = compile_with_sources(&eq::def(v), &[], &[], &eq::sources(), None)
        .unwrap_or_else(|r| panic!("{r}"));
    let s = Snapshot::parse(
        json!({"schema": "rustev.snapshot/2", "entries": entries})
            .to_string()
            .as_bytes(),
    )
    .unwrap();
    Evaluation::start(&c, &s, Timestamp::from_ms(NOW).unwrap())
        .unwrap()
        .finish("d")
        .unwrap()
}

fn step_grade(e: &EvidenceRecord, id: &str) -> ValueGrade {
    e.steps
        .iter()
        .find(|s| s.step == id)
        .unwrap()
        .meta
        .as_ref()
        .unwrap()
        .grade
}

#[test]
fn downgrade_grade_lowers_a_grade_and_never_raises_one() {
    let v =
        graded(json!({ "downgrade_grade": { "to": "estimated", "notice": "unknown_district" } }));
    // Authoritative (the source's ceiling) becomes estimated.
    let (j, e) = run_graded(&v, vec![district("nowhere", None)]);
    assert_eq!(step_grade(&e, "named"), ValueGrade::Estimated);
    let n = j
        .notices
        .iter()
        .find(|n| n.kind == NoticeKind::GradeDowngraded)
        .unwrap();
    assert_eq!(
        (n.subject.as_str(), n.reason.as_str()),
        ("input:district", "unknown_district")
    );
    assert_eq!(n.action.as_ref().unwrap().addressed_to, Party::Upstream);
    // A hint stays a hint (section 5): had the downgrade raised it to
    // estimated, the step would read it; a hint is too weak to read.
    let (_, e) = run_graded(&v, vec![district("nowhere", Some("hint"))]);
    let named = e.steps.iter().find(|s| s.step == "named").unwrap();
    assert_eq!(
        named.status,
        rustev_contract::evidence::StepStatus::Unresolved(Unresolved::insufficient([
            "input:district/name".to_string()
        ]))
    );
    // A passing check changes nothing.
    let (_, e) = run_graded(&v, vec![district("old-town", None)]);
    assert_eq!(step_grade(&e, "named"), ValueGrade::Authoritative);
}

#[test]
fn provenance_within_on_a_step_value_reads_its_provenance_set() {
    let mut v = graded(json!({ "continue_with_notice": { "notice": "n" } }));
    v["policy"]["rules"] = json!([
        { "id": "third", "when": { "provenance_within": { "ref": "step:named", "classes": ["third-party"] } },
          "then": { "escalate": { "reason": "third_party" } } },
        { "id": "fallback", "when": "always", "then": { "escalate": { "reason": "other" } } }]);
    v["policy"]["on_unresolved"] = json!([
        { "target": "step:named", "reasons": "any", "action": "as_unmet" }]);
    let reason = |v: &Value| match run_graded(v, vec![district("old-town", None)]).0.outcome {
        Outcome::Escalate { reason, .. } => reason,
        other => panic!("{other:?}"),
    };
    assert_eq!(reason(&v), "third_party");
    v["policy"]["rules"][0]["when"]["provenance_within"]["classes"] = json!(["user-supplied"]);
    assert_eq!(reason(&v), "other");
}

// ---- the trip-window definition ----

const T_NOW: i64 = 1_800_000_000_000;

fn trip() -> Value {
    json!({
        "schema": "rustev.definition/2", "name": "trip-search", "version": "1.0.0", "package": "synthetic",
        "inputs": [
            { "name": "trip.window",
              "type": { "record": { "fields": [{ "name": "start", "type": "timestamp" }] } },
              "provenance": ["user-supplied", "model-derived"], "freshness": "not_required" },
            { "name": "partner.code", "type": "integer",
              "provenance": ["third-party"], "freshness": "not_required" },
            { "name": "party.size", "type": "integer",
              "provenance": ["user-supplied"], "freshness": "not_required" }
        ],
        "steps": [],
        "policy": { "on_unresolved": [
                { "target": "input:trip.window", "reasons": "any", "action": "as_unmet" }],
            "rules": [
                { "id": "dated", "when": { "exact": { "present": "input:trip.window" } },
                  "then": { "propose": { "action": "search_dated", "params": [] } } },
                { "id": "undated", "when": "always",
                  "then": { "propose": { "action": "search_undated", "params": [] } } }],
            "adjustments": [] },
        "limits": { "max_semantic_requests": 0, "on_excess": "refuse", "max_projection_bytes": 1024,
                    "distribution_tolerance": "0.01", "deadline_ms": 1000 },
        "actionability": {
            "caller_controlled": ["user-supplied"],
            "remediations": [{ "code": "send_future_window", "for": "caller", "params": [] }],
            "questions": [], "constraint_groups": [],
            "checks": [{ "id": "future_window", "subject": "trip.window",
                "test": { "cmp": { "left": { "ref": "input:trip.window/start" }, "op": "gt",
                                   "right": { "ref": "now" } } },
                "on_fail": { "by_provenance": {
                    "user-supplied": { "refuse": { "remediation": "send_future_window" } },
                    "model-derived": { "drop_field": { "notice": "inferred_window_dropped" } } },
                    "default": "none" } }]
        }
    })
}

fn window(start: i64, class: &str) -> Value {
    json!({ "field": "trip.window", "value": { "start": start }, "provenance": class,
            "as_of_ms": T_NOW - 10, "source": "s" })
}

fn decide(def: &Value, entries: Vec<Value>) -> (Judgment, EvidenceRecord) {
    let def = Definition::parse(&serde_json::to_vec(def).unwrap()).unwrap();
    let compiled = compile(&def, &[], &[]).unwrap_or_else(|r| panic!("{r}"));
    let snapshot = Snapshot::parse(
        json!({ "schema": "rustev.snapshot/2", "entries": entries })
            .to_string()
            .as_bytes(),
    )
    .unwrap();
    let ev = Evaluation::start(&compiled, &snapshot, Timestamp::from_ms(T_NOW).unwrap()).unwrap();
    ev.finish("d-1").unwrap()
}

#[test]
fn the_future_window_example_as_goldens() {
    for (name, class) in [("refused", "user-supplied"), ("dropped", "model-derived")] {
        let (j, e) = decide(&trip(), vec![window(T_NOW - 86_400_000, class)]);
        common::golden(
            &format!("actionability.trip-{name}.judgment.json"),
            &j.record_canonical().unwrap(),
        );
        common::golden(
            &format!("actionability.trip-{name}.evidence.json"),
            &e.record_canonical().unwrap(),
        );
    }
}

#[test]
fn relax_marks_the_group_relaxed_and_the_decision_continues() {
    let mut def = trip();
    let check = |id: &str, k: Value| {
        json!({ "id": id, "subject": "trip.window",
        "test": { "cmp": { "left": { "ref": "input:trip.window/start" }, "op": "gt", "right": { "ref": "now" } } },
        "on_fail": { "by_provenance": { "user-supplied": k, "model-derived": "abstain" }, "default": "none" } })
    };
    def["actionability"]["constraint_groups"] = json!([{ "name": "dates", "checks": ["strict"] }]);
    def["actionability"]["checks"] = json!([
        check(
            "lenient",
            json!({ "relax": { "group": "dates", "notice": "dates_relaxed" } })
        ),
        check(
            "strict",
            json!({ "refuse": { "remediation": "send_future_window" } })
        ),
    ]);
    let (j, e) = decide(&def, vec![window(T_NOW - 86_400_000, "user-supplied")]);
    assert!(
        matches!(&j.outcome, Outcome::Propose { action, .. } if action == "search_dated"),
        "{:?}",
        j.outcome
    );
    let checks = e.checks.unwrap();
    assert!(
        matches!(&checks[1].status, CheckStatus::Relaxed { by } if by == "lenient"),
        "{checks:?}"
    );
    let n = j
        .notices
        .iter()
        .find(|n| n.kind == NoticeKind::ConstraintsRelaxed)
        .unwrap();
    assert_eq!(
        (n.subject.as_str(), n.reason.as_str()),
        ("group:dates", "dates_relaxed")
    );
    assert_eq!(n.action.as_ref().unwrap().addressed_to, Party::Caller);
}

#[test]
fn an_escalate_consequence_decides_without_the_policy() {
    let mut def = trip();
    def["actionability"]["checks"][0]["on_fail"]["by_provenance"]["user-supplied"] =
        json!({ "escalate": { "reason": "past_window" } });
    let (j, _) = decide(&def, vec![window(T_NOW - 86_400_000, "user-supplied")]);
    assert!(
        matches!(&j.outcome, Outcome::Escalate { reason, .. } if reason == "past_window"),
        "{:?}",
        j.outcome
    );
    assert_eq!(j.trace, ["check:future_window"]);
}

#[test]
fn a_third_party_and_a_caller_missing_field_are_addressed_upstream() {
    let mut def = trip();
    def["steps"] = json!([{ "id": "matched", "body": { "exact": { "op": "compare", "version": 1,
        "args": { "predicate": { "cmp": { "left": { "ref": "input:partner.code" }, "op": "eq",
                                          "right": { "ref": "input:party.size" } } } } } } }]);
    def["policy"]["on_unresolved"] = json!([
        { "target": "input:trip.window", "reasons": "any", "action": "as_unmet" },
        { "target": "step:matched", "reasons": "any", "action": "propagate" }]);
    def["policy"]["rules"] = json!([
        { "id": "matched", "when": { "exact": { "cmp": { "left": { "ref": "step:matched" }, "op": "eq",
                                                        "right": { "lit": { "bool": true } } } } },
          "then": { "propose": { "action": "go", "params": [] } } },
        { "id": "other", "when": "always", "then": { "propose": { "action": "stop", "params": [] } } }]);
    let (j, _) = decide(&def, vec![window(T_NOW + 86_400_000, "user-supplied")]);
    let Outcome::UnresolvedAction { reason, action } = &j.outcome else {
        panic!("{:?}", j.outcome)
    };
    assert!(
        matches!(reason, Unresolved::MissingEvidence { fields } if fields.len() == 2),
        "{reason:?}"
    );
    assert_eq!(action.computed, Party::Upstream);
    assert_eq!(action.addressed_to, Party::Upstream);
}

#[test]
fn a_terminal_check_issues_no_semantic_request() {
    // Support routing has three semantic steps; a refusing check on the
    // caller's message decides before any of them is requested (3.4.6).
    let mut v = serde_json::to_value(common::support_routing()).unwrap();
    v["schema"] = json!("rustev.definition/2");
    v["actionability"] = json!({
        "caller_controlled": ["user-supplied"],
        "remediations": [{ "code": "rewrite_message", "for": "caller", "params": [] }],
        "questions": [], "constraint_groups": [],
        "checks": [{ "id": "never", "subject": "ticket.message",
            "test": { "cmp": { "left": { "ref": "input:ticket.message" }, "op": "eq",
                               "right": { "lit": { "text": "never sent" } } } },
            "on_fail": { "by_provenance": {
                "user-supplied": { "refuse": { "remediation": "rewrite_message" } } },
                "default": "none" } }]
    });
    let def = Definition::parse(&serde_json::to_vec(&v).unwrap()).unwrap();
    let compiled = compile(
        &def,
        &common::descriptors(),
        &[common::topic_calibration("1.5")],
    )
    .unwrap_or_else(|r| panic!("{r}"));
    let ev = Evaluation::start(
        &compiled,
        &common::support_snapshot("pro", &[2, 5], 0),
        common::ts(common::NOW),
    )
    .unwrap();
    assert!(ev.pending().is_empty(), "no semantic request");
    let (j, _) = ev.finish("d").unwrap();
    assert!(
        matches!(j.outcome, Outcome::Refused { .. }),
        "{:?}",
        j.outcome
    );
    // Without the check the same decision requests every semantic step.
    v["actionability"]["checks"] = json!([]);
    let def = Definition::parse(&serde_json::to_vec(&v).unwrap()).unwrap();
    let compiled = compile(
        &def,
        &common::descriptors(),
        &[common::topic_calibration("1.5")],
    )
    .unwrap();
    let ev = Evaluation::start(
        &compiled,
        &common::support_snapshot("pro", &[2, 5], 0),
        common::ts(common::NOW),
    )
    .unwrap();
    assert!(!ev.pending().is_empty());
}
