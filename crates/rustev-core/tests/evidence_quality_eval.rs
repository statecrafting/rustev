//! Plan 3 evaluation (spec 032, increment 3): admission and the conflict
//! policy, presence, joins and merges, corroboration, grade requirements,
//! propagation and evidence 2. One test per row of section 5 in scope.
//! SYNTHETIC definitions, sources and snapshots only.

mod common;
mod eq;

use eq::*;
use rustev_contract::Document;
use rustev_contract::evidence::{
    CheckResult, CorroborationOutcome, Disposition, EvidenceRecord, InputEvidence, MatchStatus,
    MergedField, ResolutionRule, StepEvidence, UsedEntry,
};
use rustev_contract::grade::{AbsentReason, Basis, Grade, MergeSide, Presence, ValueGrade};
use rustev_contract::judgment::{Judgment, Unresolved};
use rustev_contract::output::RawOutput;
use rustev_contract::schema;
use rustev_contract::snapshot::Snapshot;
use rustev_contract::time::Timestamp;
use rustev_core::evaluate::StartError;
use rustev_core::value::Value as V;
use rustev_core::{Compiled, Evaluation, Supplied, compile_with_sources};
use serde_json::{Value, json};

const NOW: i64 = 2_000_000_000_000;

fn compiled(v: &Value) -> Compiled {
    compile_with_sources(&def(v), &common::descriptors(), &[], &sources(), None)
        .unwrap_or_else(|r| panic!("{r}"))
}

fn entry(field: &str, value: Value, provenance: &str, age: i64, source: &str) -> Value {
    json!({"field": field, "value": value, "provenance": provenance,
        "as_of_ms": NOW - age, "source": source})
}

fn claimed(mut e: Value, grade: &str) -> Value {
    e["grade"] = json!(grade);
    e
}

fn hotels() -> Value {
    entry(
        "hotels",
        json!([
            {"id": "h1", "price": "150", "description": "near"},
            {"id": "h2", "price": "120", "description": "far"},
            {"id": "h3", "price": "90", "description": "old"}
        ]),
        "system-of-record",
        5_000,
        "catalogue",
    )
}

fn quotes() -> Value {
    entry(
        "quotes",
        json!({"h1": {"price": "180", "unit_price": null}, "h2": {"price": null}}),
        "third-party",
        1_000,
        "live-quote",
    )
}

fn district(name: &str) -> Value {
    entry(
        "district",
        json!({"name": name, "kind": "centre"}),
        "third-party",
        1_000,
        "geocoder",
    )
}

fn subareas() -> Value {
    entry(
        "centre_subareas",
        json!(["old-town", "harbour"]),
        "third-party",
        1_000,
        "geocoder",
    )
}

fn all_entries() -> Vec<Value> {
    vec![hotels(), quotes(), district("old-town"), subareas()]
}

fn snapshot(schema: &str, entries: Vec<Value>) -> Snapshot {
    Snapshot::parse(
        json!({"schema": schema, "entries": entries})
            .to_string()
            .as_bytes(),
    )
    .unwrap()
}

/// Evaluate with no semantic requests outstanding.
fn finish(c: &Compiled, s: &Snapshot) -> (Judgment, EvidenceRecord) {
    Evaluation::start(c, s, Timestamp::from_ms(NOW).unwrap())
        .unwrap()
        .finish("d")
        .unwrap()
}

fn run(v: &Value, entries: Vec<Value>) -> (Judgment, EvidenceRecord) {
    finish(&compiled(v), &snapshot(schema::SNAPSHOT_V2, entries))
}

fn step<'e>(ev: &'e EvidenceRecord, id: &str) -> &'e StepEvidence {
    ev.steps.iter().find(|s| s.step == id).expect(id)
}

fn input<'e>(ev: &'e EvidenceRecord, name: &str) -> &'e InputEvidence {
    ev.inputs.iter().find(|i| i.field == name).expect(name)
}

fn unresolved(s: &StepEvidence) -> &Unresolved {
    match &s.status {
        rustev_contract::evidence::StepStatus::Unresolved(u) => u,
        other => panic!("{} resolved: {other:?}", s.step),
    }
}

fn push_step(v: &mut Value, step: Value) {
    v["steps"].as_array_mut().unwrap().push(step);
}

fn compare_step(id: &str, predicate: Value) -> Value {
    json!({"id": id, "body": {"exact": {"op": "compare", "version": 1,
        "args": {"predicate": predicate}}}})
}

fn corroborate(upgrade_to: &str, on_contradiction: &str) -> Value {
    json!({"id": "district_ok", "body": {"corroborate": {
        "claim": "input:district", "upgrade_to": upgrade_to,
        "by": [{"predicate": {"member_ref": {"value": {"ref": "input:district/name"},
            "set": "input:centre_subareas"}}, "on_false": "contradicts"}],
        "mode": "all", "on_contradiction": on_contradiction}}})
}

fn is_centre() -> Value {
    compare_step(
        "is_centre",
        json!({"cmp": {"left": {"ref": "step:district_ok/kind"}, "op": "eq",
            "right": {"lit": {"enum": "centre"}}}}),
    )
}

/// Filter over the merged records, reading the merged price.
fn merged_filter() -> Value {
    json!({"id": "merged_ok", "body": {"exact": {"op": "filter_with_reasons", "version": 1,
        "args": {"list": "step:merged", "id_field": "id", "rules": [
            {"name": "under_budget", "predicate": decimal_le("item:price", "200")}]}}}})
}

#[test]
fn joins_presence_and_merges() {
    let c = compiled(&{
        let mut v = base();
        push_step(&mut v, merged_filter());
        v
    });
    let s = snapshot(schema::SNAPSHOT_V2, all_entries());
    let e = Evaluation::start(&c, &s, Timestamp::from_ms(NOW).unwrap()).unwrap();
    // A quote whose prices are both null is matched_absent; no entry is
    // unmatched; both are excluded with their reason.
    let Some(V::Filtered(f)) = e.exact_value("affordable") else {
        panic!("affordable")
    };
    assert_eq!(f.eligible, ["h1"]);
    let reasons: Vec<(&str, &[String])> = f
        .excluded
        .iter()
        .map(|x| (x.id.as_str(), x.reasons.as_slice()))
        .collect();
    assert_eq!(
        reasons,
        [
            ("h2", &["predicate_failed".to_string()][..]),
            ("h3", &["unmatched".to_string()][..])
        ]
    );
    // An overlay_only price with no overlay is absent, never the cached
    // base price, and a rule reading it excludes the item missing:<path>.
    let Some(V::List(merged)) = e.exact_value("merged") else {
        panic!("merged")
    };
    let price = |i: usize| match &merged[i] {
        V::Record(r) => r["price"].clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(
        price(0),
        V::Decimal(rustev_contract::Decimal::parse("180").unwrap())
    );
    for i in [1, 2] {
        assert!(
            matches!(price(i), V::Absent(ref a) if a.reason == AbsentReason::OverlayMissing),
            "item {i}"
        );
    }
    let Some(V::Filtered(mf)) = e.exact_value("merged_ok") else {
        panic!("merged_ok")
    };
    assert_eq!(mf.eligible, ["h1"]);
    assert!(mf.excluded.iter().all(|x| x.reasons == ["missing:price"]));
    let (_, ev) = e.finish("d").unwrap();
    assert_eq!(ev.schema, schema::EVIDENCE_V2);
    let join = step(&ev, "joined").join.as_ref().unwrap();
    let statuses: Vec<MatchStatus> = join.iter().map(|j| j.status).collect();
    assert_eq!(
        statuses,
        [
            MatchStatus::Matched,
            MatchStatus::MatchedAbsent,
            MatchStatus::Unmatched
        ]
    );
    // Per-field grades, sides and as-of in the step value and evidence 2.
    let m = step(&ev, "merged");
    let first = &m.meta.as_ref().unwrap().items.as_ref().unwrap()[0];
    let fields = first.fields.as_ref().unwrap();
    assert_eq!(fields["price"].grade, ValueGrade::Authoritative);
    assert_eq!(
        fields["price"].basis,
        Basis::Merged {
            step: "merged".into(),
            side: MergeSide::Overlay
        }
    );
    assert_eq!(fields["description"].grade, ValueGrade::Cached);
    assert_eq!(
        fields["description"].as_of_ms,
        rustev_contract::grade::AsOf::Ms(Timestamp::from_ms(NOW - 5_000).unwrap())
    );
    let records = m.merge.as_ref().unwrap();
    let h1_price = records
        .iter()
        .find(|r| r.item == "h1" && r.field == "price")
        .unwrap();
    assert_eq!(
        h1_price.result,
        MergedField::Taken {
            side: MergeSide::Overlay,
            grade: ValueGrade::Authoritative
        }
    );
    let h3_price = records
        .iter()
        .find(|r| r.item == "h3" && r.field == "price")
        .unwrap();
    assert_eq!(
        h3_price.result,
        MergedField::Absent {
            reason: AbsentReason::OverlayMissing
        }
    );
    // Evidence carries values only as digests.
    let quotes = input(&ev, "quotes");
    assert_eq!(quotes.presence, Some(Presence::Present));
    let entries = quotes.entries.as_ref().unwrap();
    assert!(entries[0].digest.starts_with("sha256:"));
    assert_eq!(entries[0].disposition, Disposition::Used);
}

#[test]
fn merge_grade_rules() {
    // overlay_else_base_if_grade_at_least{authoritative} with a cached base
    // and no overlay: absent overlay_missing_base_below_grade.
    let mut v = base();
    v["steps"][1]["body"]["exact"]["args"]["rules"][1] = json!({"field": "price",
        "rule": {"overlay_else_base_if_grade_at_least": {"grade": "authoritative"}}});
    let (_, ev) = run(&v, all_entries());
    let rec = |item: &str| {
        step(&ev, "merged")
            .merge
            .as_ref()
            .unwrap()
            .iter()
            .find(|r| r.item == item && r.field == "price")
            .unwrap()
            .result
            .clone()
    };
    assert_eq!(
        rec("h3"),
        MergedField::Absent {
            reason: AbsentReason::OverlayMissingBaseBelowGrade
        }
    );
    // At cached, the cached base fills it.
    v["steps"][1]["body"]["exact"]["args"]["rules"][1] = json!({"field": "price",
        "rule": {"overlay_else_base_if_grade_at_least": {"grade": "cached"}}});
    let (_, ev) = run(&v, all_entries());
    let taken = step(&ev, "merged")
        .merge
        .as_ref()
        .unwrap()
        .iter()
        .find(|r| r.item == "h3" && r.field == "price")
        .unwrap()
        .result
        .clone();
    assert_eq!(
        taken,
        MergedField::Taken {
            side: MergeSide::Base,
            grade: ValueGrade::Cached
        }
    );
    // A base record that fails its presence predicate yields
    // overlay_and_base_missing, never a grade reason.
    let mut v = base();
    v["inputs"][0]["type"]["list"]["item"]["record"]["fields"][1]["type"] =
        json!({"nullable": {"type": "decimal"}});
    v["inputs"][0]["type"]["list"]["item"]["record"]["present_when"] =
        json!({"all_non_null": ["price"]});
    v["steps"][1]["body"]["exact"]["args"]["rules"][1] = json!({"field": "price",
        "rule": {"overlay_else_base_if_grade_at_least": {"grade": "cached"}}});
    let mut es = all_entries();
    es[0]["value"][2]["price"] = json!(null);
    let (_, ev) = run(&v, es);
    // The absent base item has no readable id, so it is named by position.
    let r = step(&ev, "merged")
        .merge
        .as_ref()
        .unwrap()
        .iter()
        .find(|r| r.item == "#2" && r.field == "price")
        .unwrap()
        .result
        .clone();
    assert_eq!(
        r,
        MergedField::Absent {
            reason: AbsentReason::OverlayAndBaseMissing
        }
    );
}

#[test]
fn a_nested_presence_predicate_records_its_canonical_tree() {
    let mut v = base();
    v["inputs"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name": "contact",
        "type": {"record": {"fields": [
            {"name": "c", "type": {"nullable": {"type": "integer"}}},
            {"name": "b", "type": {"nullable": {"type": "integer"}}},
            {"name": "a", "type": {"nullable": {"type": "integer"}}}],
            "present_when": {"all": [{"any_non_null": ["b", "a"]}, {"all_non_null": ["c"]}]}}},
        "provenance": ["user-supplied"], "freshness": "not_required"}));
    let mut es = all_entries();
    es.push(entry(
        "contact",
        json!({"a": null, "b": 1, "c": null}),
        "user-supplied",
        0,
        "form",
    ));
    let (_, ev) = run(&v, es);
    let contact = input(&ev, "contact");
    assert!(matches!(
        &contact.status,
        rustev_contract::evidence::InputStatus::Unresolved(Unresolved::MissingEvidence { fields })
            if fields == &["contact".to_string()]
    ));
    let Some(Presence::Absent { reason, predicate }) = &contact.presence else {
        panic!("{:?}", contact.presence)
    };
    assert_eq!(*reason, AbsentReason::PredicateFailed);
    assert_eq!(
        serde_json::to_value(predicate).unwrap(),
        json!({"all": [{"any_non_null": ["a", "b"]}, {"all_non_null": ["c"]}]})
    );
}

#[test]
fn corroboration_outcomes() {
    // A label gates once corroborated by sub-area membership.
    let mut v = base();
    push_step(&mut v, corroborate("authoritative", "conflict"));
    push_step(&mut v, is_centre());
    let (_, ev) = run(&v, all_entries());
    let s = step(&ev, "district_ok");
    let rec = s.corroboration.as_ref().unwrap();
    assert_eq!(rec.outcome, CorroborationOutcome::Corroborated);
    assert_eq!(rec.grade_before, ValueGrade::Hint);
    assert_eq!(rec.grade_after, ValueGrade::Authoritative);
    assert_eq!(rec.checks[0].result, CheckResult::Supports);
    assert!(matches!(
        step(&ev, "is_centre").status,
        rustev_contract::evidence::StepStatus::Resolved { .. }
    ));
    // Contradicted, on_contradiction: conflict names the claim and the
    // contradicting check's reads; evidence holds both digests and grades.
    let mut es = all_entries();
    es[2] = district("suburbia");
    let (_, ev) = run(&v, es.clone());
    let s = step(&ev, "district_ok");
    assert_eq!(
        unresolved(s),
        &Unresolved::conflict([
            "input:district".to_string(),
            "input:district/name".to_string(),
            "input:centre_subareas".to_string()
        ])
    );
    let check = &s.corroboration.as_ref().unwrap().checks[0];
    assert_eq!(check.result, CheckResult::Contradicts);
    assert!(check.reads.iter().all(|r| r.digest.is_some()));
    assert_eq!(check.reads[1].grade, ValueGrade::Authoritative);
    // treat_as_hint keeps the claim at hint, so the gate read is short.
    let mut hint = base();
    push_step(&mut hint, corroborate("authoritative", "treat_as_hint"));
    push_step(&mut hint, is_centre());
    let (_, ev) = run(&hint, es.clone());
    assert_eq!(
        step(&ev, "district_ok").meta.as_ref().unwrap().grade,
        ValueGrade::Hint
    );
    assert_eq!(
        unresolved(step(&ev, "is_centre")),
        &Unresolved::insufficient(["step:district_ok/kind".to_string()])
    );
    // abstain.
    let mut abstain = base();
    push_step(&mut abstain, corroborate("authoritative", "abstain"));
    let (_, ev) = run(&abstain, es);
    assert_eq!(
        unresolved(step(&ev, "district_ok")),
        &Unresolved::Abstained {
            rule: "district_ok".into()
        }
    );
    // A check reading an unresolved set is no_evidence; the claim stays
    // uncorroborated at hint and the gate read is insufficient_grade.
    let (_, ev) = run(&v, vec![hotels(), quotes(), district("old-town")]);
    let rec = step(&ev, "district_ok").corroboration.clone().unwrap();
    assert_eq!(rec.checks[0].result, CheckResult::NoEvidence);
    assert_eq!(rec.outcome, CorroborationOutcome::Uncorroborated);
    assert_eq!(rec.grade_after, ValueGrade::Hint);
    assert_eq!(
        unresolved(step(&ev, "is_centre")),
        &Unresolved::insufficient(["step:district_ok/kind".to_string()])
    );
}

#[test]
fn an_uncorroborated_estimated_claim_keeps_its_grade_and_basis() {
    let mut v = base();
    v["inputs"][2]["evidence"]["graded"]["caps"] = json!([{"path": "", "max_grade": "estimated"}]);
    v["inputs"][2]["evidence"]["graded"]["sources"] = json!(["geocoder"]);
    // Make the label reliable through the cap only: the claim is estimated.
    let mut srcs = sources();
    srcs[2].facts.as_mut().unwrap()[0].grades.pop();
    push_step(&mut v, corroborate("cached", "conflict"));
    let c = compile_with_sources(&def(&v), &[], &[], &srcs, None).unwrap();
    let (_, ev) = finish(
        &c,
        &snapshot(
            schema::SNAPSHOT_V2,
            vec![hotels(), quotes(), district("old-town")],
        ),
    );
    let s = step(&ev, "district_ok");
    let rec = s.corroboration.as_ref().unwrap();
    assert_eq!(rec.outcome, CorroborationOutcome::Uncorroborated);
    let meta = s.meta.as_ref().unwrap();
    assert_eq!(meta.grade, ValueGrade::Estimated);
    assert_eq!(
        meta.basis,
        Basis::Declared,
        "the claim's basis is unchanged"
    );
}

#[test]
fn requirements_freshness_and_claims() {
    let gate = |age_limit: u64| {
        compare_step(
            "has_hotels",
            json!({"cmp": {"left": {"len": {"graded": {"ref": "input:hotels",
                "require": {"min": "cached", "max_age_ms": {"ms": age_limit}}}}},
                "op": "gt", "right": {"lit": {"integer": 0}}}}),
        )
    };
    let mut v = base();
    push_step(&mut v, gate(600_000));
    let aged = |age: i64| {
        let mut h = hotels();
        h["as_of_ms"] = json!(NOW - age);
        vec![h, quotes(), district("old-town"), subareas()]
    };
    let (_, ev) = run(&v, aged(600_001));
    assert_eq!(
        unresolved(step(&ev, "has_hotels")),
        &Unresolved::stale(["input:hotels".to_string()])
    );
    let (_, ev) = run(&v, aged(600_000));
    assert!(matches!(
        step(&ev, "has_hotels").status,
        rustev_contract::evidence::StepStatus::Resolved { .. }
    ));
    // A snapshot 2 claim of hint on an authoritative path, read by a gate.
    let mut es = all_entries();
    es[1] = claimed(quotes(), "hint");
    let (_, ev) = run(&base(), es);
    assert_eq!(
        unresolved(step(&ev, "affordable")),
        &Unresolved::insufficient(["step:joined/*/match/price".to_string()])
    );
    let entry = &input(&ev, "quotes").entries.as_ref().unwrap()[0];
    assert_eq!(entry.claim, Some(Grade::Hint));
    assert_eq!(entry.ceiling, ValueGrade::Authoritative);
    assert_eq!(entry.effective, ValueGrade::Hint);
}

#[test]
fn propagation_takes_the_minimum_and_ungraded_never_raises() {
    let mut v = base();
    push_step(
        &mut v,
        json!({"id": "total", "body": {"exact": {"op": "arith", "version": 1,
        "args": {"expr": {"add": {"left": {"len": {"ref": "input:centre_subareas"}},
            "right": {"len": {"ref": "input:hotels"}}}}}}}}),
    );
    let mut es = all_entries();
    es[0] = claimed(hotels(), "hint");
    let (_, ev) = run(&v, es);
    assert_eq!(
        step(&ev, "total").meta.as_ref().unwrap().grade,
        ValueGrade::Hint,
        "authoritative plus hint is hint"
    );
    let (_, ev) = run(&v, all_entries());
    let m = step(&ev, "total").meta.clone().unwrap();
    assert_eq!(m.grade, ValueGrade::Cached);
    assert_eq!(m.sources, ["catalogue", "geocoder"]);
}

#[test]
fn semantic_steps_are_at_most_estimated() {
    let semantic = |project: &str| {
        json!({"id": "tone", "body": {"semantic": {"operation": "classify",
            "task": "synthetic.tone", "question": "Which tone?", "options": ["calm", "busy"],
            "candidates": "none", "requires": "label", "project": [project], "for_each": [],
            "backend": {"min_determinism": "unspecified", "artifact": "any",
                "binding": {"backend": "synthetic-label-only"}},
            "calibration": "none", "fallback": "none"}}})
    };
    let with = |project: &[&str]| {
        let mut v = base();
        v["inputs"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name": "note",
            "type": {"text": {"max_bytes": 64}}, "provenance": ["user-supplied"],
            "freshness": "not_required"}));
        let mut s = semantic(project[0]);
        s["body"]["semantic"]["project"] = json!(project);
        push_step(&mut v, s);
        v["limits"]["max_semantic_requests"] = json!(1);
        v["limits"]["max_projection_bytes"] = json!(100_000);
        v
    };
    let eval = |v: &Value| {
        let c = compiled(v);
        let mut es = all_entries();
        es.push(entry("note", json!("hello"), "user-supplied", 0, "form"));
        let s = snapshot(schema::SNAPSHOT_V2, es);
        let mut e = Evaluation::start(&c, &s, Timestamp::from_ms(NOW).unwrap()).unwrap();
        e.supply(
            "tone",
            &[],
            Supplied::Output(RawOutput::Label("calm".into())),
        )
        .unwrap();
        e.finish("d").unwrap().1
    };
    let ev = eval(&with(&["input:note"]));
    assert_eq!(
        step(&ev, "tone").meta.as_ref().unwrap().grade,
        ValueGrade::Ungraded
    );
    let ev = eval(&with(&["input:note", "input:centre_subareas"]));
    assert_eq!(
        step(&ev, "tone").meta.as_ref().unwrap().grade,
        ValueGrade::Estimated
    );
}

#[test]
fn admission_and_the_conflict_policy() {
    // An entry from a source the input does not name is ignored; with no
    // other entry the input is missing.
    let mut es = all_entries();
    es[3] = entry(
        "centre_subareas",
        json!(["x"]),
        "third-party",
        0,
        "catalogue",
    );
    let (_, ev) = run(&base(), es);
    let s = input(&ev, "centre_subareas");
    assert!(matches!(
        &s.status,
        rustev_contract::evidence::InputStatus::Unresolved(Unresolved::MissingEvidence { .. })
    ));
    assert_eq!(
        s.entries.as_ref().unwrap()[0].disposition,
        Disposition::Ignored {
            reason: "unbound_source".into()
        }
    );
    // A provenance different from the source's fact is invalid.
    let mut es = all_entries();
    es[3] = entry(
        "centre_subareas",
        json!(["x"]),
        "user-supplied",
        0,
        "geocoder",
    );
    let mut v = base();
    v["inputs"][3]["provenance"] = json!(["third-party", "user-supplied"]);
    let (_, ev) = run(&v, es);
    assert!(matches!(
        &input(&ev, "centre_subareas").status,
        rustev_contract::evidence::InputStatus::Unresolved(Unresolved::InvalidInput { detail, .. })
            if detail == "source-provenance-mismatch"
    ));
    // Two sources disagree.
    let mut srcs = sources();
    let second = json!({"schema": "rustev.source/1", "name": "atlas", "version": "1.0.0",
        "facts": [{"name": "centre_subareas", "provenance": "third-party", "grades": [
            {"path": "", "max_grade": "cached", "reliability": "reliable"}]}]});
    srcs.push(
        rustev_contract::source::SourceDescriptor::parse(second.to_string().as_bytes()).unwrap(),
    );
    let with_rule = |rule: Value| {
        let mut v = base();
        v["inputs"][3]["evidence"]["graded"]["sources"] = json!(["atlas", "geocoder"]);
        v["inputs"][3]["on_disagreement"] = rule;
        compile_with_sources(&def(&v), &[], &[], &srcs, None).unwrap()
    };
    let disagreeing = |extra: Value| {
        let mut es = all_entries();
        es.push(extra);
        snapshot(schema::SNAPSHOT_V2, es)
    };
    let atlas = entry(
        "centre_subareas",
        json!(["harbour"]),
        "third-party",
        2_000,
        "atlas",
    );
    // prefer: the first listed source's value is used; both are recorded.
    let c = with_rule(json!({"prefer": {"sources": ["geocoder", "atlas"]}}));
    let (_, ev) = finish(&c, &disagreeing(atlas.clone()));
    let s = input(&ev, "centre_subareas");
    let r = s.resolution.as_ref().unwrap();
    assert_eq!(r.rule, ResolutionRule::Prefer);
    let entries = s.entries.as_ref().unwrap();
    let UsedEntry::Entry(i) = r.used else {
        panic!("{r:?}")
    };
    assert_eq!(entries[i as usize].source, "geocoder");
    assert_eq!(entries.len(), 2);
    assert_ne!(entries[0].digest, entries[1].digest);
    assert_eq!(
        s.meta.as_ref().unwrap().basis,
        Basis::Resolved {
            input: "centre_subareas".into()
        }
    );
    // treat_as_hint: the used value is lowered to hint.
    let c = with_rule(json!({"treat_as_hint": {"sources": ["atlas", "geocoder"]}}));
    let (_, ev) = finish(&c, &disagreeing(atlas.clone()));
    let s = input(&ev, "centre_subareas");
    assert_eq!(s.meta.as_ref().unwrap().grade, ValueGrade::Hint);
    assert_eq!(s.meta.as_ref().unwrap().sources, ["atlas"]);
    // abstain and require_agreement.
    let c = with_rule(json!("abstain"));
    let (_, ev) = finish(&c, &disagreeing(atlas.clone()));
    assert!(matches!(
        &input(&ev, "centre_subareas").status,
        rustev_contract::evidence::InputStatus::Unresolved(Unresolved::Abstained { rule })
            if rule == "disagreement:centre_subareas"
    ));
    let c = with_rule(json!("require_agreement"));
    let (_, ev) = finish(&c, &disagreeing(atlas));
    assert!(matches!(
        &input(&ev, "centre_subareas").status,
        rustev_contract::evidence::InputStatus::Unresolved(Unresolved::Conflict { .. })
    ));
    // One source with two differing entries is a conflict under prefer.
    let c = with_rule(json!({"prefer": {"sources": ["geocoder", "atlas"]}}));
    let twice = entry(
        "centre_subareas",
        json!(["elsewhere"]),
        "third-party",
        3_000,
        "geocoder",
    );
    let (_, ev) = finish(&c, &disagreeing(twice));
    assert!(matches!(
        &input(&ev, "centre_subareas").status,
        rustev_contract::evidence::InputStatus::Unresolved(Unresolved::Conflict { .. })
    ));
}

#[test]
fn snapshot_versions_follow_the_plan() {
    // A snapshot 2 to a plan 2 is refused, so no claim is dropped.
    let plan2 = rustev_core::compile(
        &common::support_routing(),
        &common::descriptors(),
        &[common::topic_calibration("1.5")],
    )
    .unwrap();
    let s2 = snapshot(schema::SNAPSHOT_V2, vec![]);
    match Evaluation::start(&plan2, &s2, Timestamp::from_ms(NOW).unwrap()) {
        Err(StartError::Schema(d)) => assert!(d.starts_with("snapshot_version"), "{d}"),
        other => panic!("{:?}", other.err()),
    }
    // A plan 3 accepts a snapshot 1, which claims nothing.
    let (_, ev) = finish(
        &compiled(&base()),
        &snapshot(schema::SNAPSHOT, all_entries()),
    );
    assert_eq!(
        input(&ev, "quotes").entries.as_ref().unwrap()[0].claim,
        None
    );
}
