//! The #76 track versions and source descriptors (spec 032, 3.1): each
//! version parses in one wire shape, identities are tagged with the
//! document's own schema, and unused optional members are omitted from
//! canonical bytes. SYNTHETIC documents only.

use rustev_contract::definition::Definition;
use rustev_contract::evidence::{EvidenceRecord, InputEvidence, InputStatus, StepEvidence};
use rustev_contract::grade::{Grade, ValueMeta};
use rustev_contract::plan::Plan;
use rustev_contract::run::RunRecord;
use rustev_contract::snapshot::Snapshot;
use rustev_contract::source::{Reliability, SourceDescriptor, valid_source_name};
use rustev_contract::{Document, DocumentError, Identified, schema};

const MINIMAL: &str = r#"{"schema":"rustev.definition/1","name":"m","version":"0.1.0","package":"p",
"inputs":[{"name":"x","type":{"text":{"max_bytes":8}},"provenance":["user-supplied"],"freshness":"not_required"}],
"steps":[{"id":"n","body":{"exact":{"op":"arith","version":1,"args":{"expr":{"lit":{"integer":1}}}}}}],
"policy":{"on_unresolved":[],"rules":[{"id":"r","when":"always","then":{"escalate":{"reason":"x"}}}],"adjustments":[]},
"limits":{"max_semantic_requests":0,"on_excess":"refuse","max_projection_bytes":0,"distribution_tolerance":"0.000001","deadline_ms":0}}"#;

const SNAPSHOT: &str = r#"{"schema":"rustev.snapshot/1","entries":[
{"field":"x","value":"a","provenance":"user-supplied","as_of_ms":5,"source":"s"}]}"#;

const SOURCE: &str = r#"{"schema":"rustev.source/1","name":"geocoder","version":"1.0.0",
"facts":[{"name":"area","provenance":"third-party","grades":[
{"path":"","max_grade":"authoritative","reliability":"reliable"},
{"path":"type","max_grade":"authoritative","reliability":{"known_unreliable":{"note":"labels drift"}}}]}]}"#;

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[test]
fn both_definition_versions_parse_and_identity_is_tagged_with_the_own_schema() {
    let v1 = Definition::parse(MINIMAL.as_bytes()).unwrap();
    let v2_text = MINIMAL.replace("rustev.definition/1", "rustev.definition/2");
    let v2 = Definition::parse(v2_text.as_bytes()).unwrap();
    assert_eq!(v2.schema, schema::DEFINITION_V2);
    // Same content under another schema is another identity, and the
    // version 1 identity is the plain tagged digest it always was.
    assert_ne!(v1.id().unwrap(), v2.id().unwrap());
    let raw =
        rustev_contract::canonical::tagged_digest(schema::DEFINITION, &v1.canonical().unwrap());
    assert_eq!(v1.id().unwrap().as_str(), raw);
    let v3 = MINIMAL.replace("rustev.definition/1", "rustev.definition/3");
    match Definition::parse(v3.as_bytes()) {
        Err(DocumentError::Schema { expected, .. }) => assert_eq!(
            expected,
            format!("{} or {}", schema::DEFINITION, schema::DEFINITION_V2),
            "the refusal names every accepted version"
        ),
        other => panic!("{other:?}"),
    }
}

#[test]
fn unused_optional_members_are_omitted_and_used_ones_round_trip() {
    let v1 = Definition::parse(MINIMAL.as_bytes()).unwrap();
    let bytes = text(&v1.canonical().unwrap());
    for member in ["evidence", "on_disagreement", "present_when", "grade"] {
        assert!(!bytes.contains(member), "{member} written while unused");
    }
    let graded = MINIMAL
        .replace("rustev.definition/1", "rustev.definition/2")
        .replace(
            r#""freshness":"not_required"}"#,
            r#""freshness":"not_required","evidence":{"graded":{"sources":["geocoder"],"caps":[{"path":"","max_grade":"cached"}]}},"on_disagreement":{"prefer":{"sources":["geocoder"]}}}"#,
        );
    let d = Definition::parse(graded.as_bytes()).unwrap();
    let c = d.canonical().unwrap();
    assert!(text(&c).contains(r#""evidence":{"graded""#));
    assert_eq!(Definition::parse(&c).unwrap(), d);
}

#[test]
fn version_2_types_and_steps_parse() {
    let doc = MINIMAL
        .replace("rustev.definition/1", "rustev.definition/2")
        .replace(
            r#""type":{"text":{"max_bytes":8}}"#,
            r#""type":{"map":{"key_max_bytes":16,"max_entries":10,"value":{"record":{"fields":[
{"name":"total","type":{"nullable":{"type":"decimal"}}},
{"name":"unit","type":{"nullable":{"type":"decimal"}}}],
"present_when":{"any_non_null":["total","unit"]}}}}}"#,
        )
        .replace(
            r#"{"id":"n","body""#,
            r#"{"id":"c","body":{"corroborate":{"claim":"input:x","upgrade_to":"authoritative",
"by":[{"predicate":{"member_ref":{"value":{"graded":{"ref":"input:x","require":{"min":"cached","max_age_ms":{"ms":600000}}}},"set":"input:x"}},"on_false":"contradicts"}],
"mode":"all","on_contradiction":"conflict"}}},{"id":"n","body""#,
        );
    let d = Definition::parse(doc.as_bytes()).unwrap();
    let c = d.canonical().unwrap();
    assert_eq!(Definition::parse(&c).unwrap(), d);
}

#[test]
fn snapshot_2_claims_are_optional_members() {
    let s1 = Snapshot::parse(SNAPSHOT.as_bytes()).unwrap();
    assert!(!text(&s1.canonical().unwrap()).contains("grade"));
    let s2_text = SNAPSHOT
        .replace("rustev.snapshot/1", "rustev.snapshot/2")
        .replace(r#""source":"s"}"#, r#""source":"s","grade":"cached"}"#);
    let s2 = Snapshot::parse(s2_text.as_bytes()).unwrap();
    assert_eq!(s2.entries[0].grade, Some(Grade::Cached));
    assert_ne!(s1.id().unwrap(), s2.id().unwrap());
}

#[test]
fn a_source_descriptor_parses_and_is_identified() {
    let s = SourceDescriptor::parse(SOURCE.as_bytes()).unwrap();
    let facts = s.facts.as_ref().unwrap();
    let label = &facts[0].grades[1];
    assert!(matches!(
        label.reliability,
        Reliability::KnownUnreliable { .. }
    ));
    assert_eq!(label.limit(), Grade::Hint, "unreliable lowers to hint");
    assert_eq!(facts[0].grades[0].limit(), Grade::Authoritative);
    let id = s.id().unwrap();
    let again = SourceDescriptor::parse(&s.canonical().unwrap()).unwrap();
    assert_eq!(again.id().unwrap(), id);
    let bare = SourceDescriptor::parse(
        br#"{"schema":"rustev.source/1","name":"geocoder","version":"1.0.0"}"#,
    )
    .unwrap();
    assert!(!text(&bare.canonical().unwrap()).contains("facts"));
    assert_ne!(bare.id().unwrap(), id);
    assert!(valid_source_name("live-quote_2"));
    assert!(!valid_source_name("Live"));
    assert!(!valid_source_name(""));
    assert!(!valid_source_name(&"a".repeat(65)));
}

#[test]
fn plan_evidence_and_run_versions_parse_without_the_new_members() {
    // Existing records parse unchanged and write no evidence 2 member.
    let input = InputEvidence {
        field: "x".into(),
        status: InputStatus::Unresolved(rustev_contract::judgment::Unresolved::missing([
            "x".to_string()
        ])),
        meta: None,
        presence: None,
        entries: None,
        resolution: None,
    };
    let json = serde_json::to_string(&input).unwrap();
    assert!(!json.contains("meta") && !json.contains("presence"));
    let step = StepEvidence::new(
        "s".into(),
        vec![],
        rustev_contract::evidence::StepStatus::Unresolved(
            rustev_contract::judgment::Unresolved::insufficient(["input:x/type".to_string()]),
        ),
    );
    let json = serde_json::to_string(&step).unwrap();
    assert!(json.contains("insufficient_grade"));
    assert!(!json.contains("join") && !json.contains("corroboration"));
    let with_meta = InputEvidence {
        meta: Some(ValueMeta::ungraded(rustev_contract::grade::Basis::Declared)),
        ..input
    };
    let json = serde_json::to_string(&with_meta).unwrap();
    assert!(json.contains(r#""meta":{"grade":"ungraded""#));
    for (doc, v) in [
        (Plan::SCHEMA, schema::PLAN_V3),
        (EvidenceRecord::SCHEMA, schema::EVIDENCE_V2),
        (RunRecord::SCHEMA, schema::RUN_V3),
    ] {
        assert_ne!(doc, v, "the trait constant names the first version");
    }
}

#[test]
fn every_versioned_refusal_names_its_accepted_versions() {
    fn expected<D: Document + std::fmt::Debug>(bytes: &str) -> &'static str {
        match D::parse(bytes.as_bytes()) {
            Err(DocumentError::Schema { expected, .. }) => expected,
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(
        expected::<Snapshot>(r#"{"schema":"rustev.unknown/9","entries":[]}"#),
        format!("{} or {}", schema::SNAPSHOT, schema::SNAPSHOT_V2)
    );
    // Plan, evidence and run records carry the same macro literal beside
    // their constants; their refusal text is pinned in their own modules.
}
