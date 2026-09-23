//! Documents at their bounds (spec 002, 3.2 and 3.3): duplicate keys,
//! unknown fields, every limit at and one past its value, schema checks,
//! canonical round trips. SYNTHETIC documents only.

use rustev_contract::bounded::{BoundError, ParseError};
use rustev_contract::definition::Definition;
use rustev_contract::eval_report::{EvalReport, MetricValue};
use rustev_contract::limits::{DEFINITION_V1, SNAPSHOT_V1};
use rustev_contract::output::BackendOutputDoc;
use rustev_contract::snapshot::Snapshot;
use rustev_contract::{Document, DocumentError, Identified};

const MINIMAL: &str = r#"{"schema":"rustev.definition/1","name":"m","version":"0.1.0","package":"p",
"inputs":[{"name":"x","type":{"text":{"max_bytes":8}},"provenance":["user-supplied"],"freshness":"not_required"}],
"steps":[{"id":"n","body":{"exact":{"op":"arith","version":1,"args":{"expr":{"lit":{"integer":1}}}}}}],
"policy":{"on_unresolved":[],"rules":[{"id":"r","when":"always","then":{"escalate":{"reason":"x"}}}],"adjustments":[]},
"limits":{"max_semantic_requests":0,"on_excess":"refuse","max_projection_bytes":0,"distribution_tolerance":"0.000001","deadline_ms":0}}"#;

fn bound_err(r: Result<Definition, DocumentError>) -> BoundError {
    match r {
        Err(DocumentError::Parse(ParseError::Bound(b))) => b,
        other => panic!("expected a bound refusal, got {other:?}"),
    }
}

#[test]
fn a_minimal_definition_parses_and_round_trips_canonically() {
    let d = Definition::parse(MINIMAL.as_bytes()).unwrap();
    let c = d.canonical().unwrap();
    let again = Definition::parse(&c).unwrap();
    assert_eq!(again.canonical().unwrap(), c);
    assert_eq!(d.id().unwrap(), again.id().unwrap());
    // Whitespace does not change identity; content does.
    let spaced = MINIMAL.replace(',', " , ");
    assert_eq!(
        Definition::parse(spaced.as_bytes()).unwrap().id().unwrap(),
        d.id().unwrap()
    );
    let other = MINIMAL.replace("\"name\":\"m\"", "\"name\":\"m2\"");
    assert_ne!(
        Definition::parse(other.as_bytes()).unwrap().id().unwrap(),
        d.id().unwrap()
    );
}

#[test]
fn duplicate_keys_are_refused_at_any_depth() {
    let top = MINIMAL.replacen("\"name\":\"m\"", "\"name\":\"m\",\"name\":\"m\"", 1);
    assert!(matches!(
        bound_err(Definition::parse(top.as_bytes())),
        BoundError::DuplicateKey { .. }
    ));
    let nested = MINIMAL.replacen("\"max_bytes\":8", "\"max_bytes\":8,\"max_bytes\":9", 1);
    assert!(matches!(
        bound_err(Definition::parse(nested.as_bytes())),
        BoundError::DuplicateKey { .. }
    ));
    // Escapes are decoded before comparison.
    let escaped = MINIMAL.replacen("\"name\":\"m\"", "\"name\":\"m\",\"n\\u0061me\":\"m\"", 1);
    assert!(matches!(
        bound_err(Definition::parse(escaped.as_bytes())),
        BoundError::DuplicateKey { .. }
    ));
}

#[test]
fn unknown_fields_are_refused_at_any_depth() {
    for (from, to) in [
        ("\"package\":\"p\"", "\"package\":\"p\",\"extra\":1"),
        (
            "\"freshness\":\"not_required\"",
            "\"freshness\":\"not_required\",\"extra\":1",
        ),
        ("\"max_bytes\":8", "\"max_bytes\":8,\"extra\":1"),
        ("\"deadline_ms\":0", "\"deadline_ms\":0,\"extra\":1"),
    ] {
        let doc = MINIMAL.replacen(from, to, 1);
        assert!(
            matches!(
                Definition::parse(doc.as_bytes()),
                Err(DocumentError::Parse(ParseError::Schema { .. }))
            ),
            "{to}"
        );
    }
    // A missing field is not defaulted.
    let doc = MINIMAL.replacen("\"package\":\"p\",", "", 1);
    assert!(matches!(
        Definition::parse(doc.as_bytes()),
        Err(DocumentError::Parse(ParseError::Schema { .. }))
    ));
}

#[test]
fn the_schema_string_is_checked() {
    let doc = MINIMAL.replacen("rustev.definition/1", "rustev.definition/2", 1);
    assert!(matches!(
        Definition::parse(doc.as_bytes()),
        Err(DocumentError::Schema { .. })
    ));
}

#[test]
fn the_byte_bound_holds_at_and_one_past_the_limit() {
    let max = DEFINITION_V1.max_bytes;
    let mut at = MINIMAL.as_bytes().to_vec();
    at.resize(max, b' ');
    assert!(Definition::parse(&at).is_ok());
    at.push(b' ');
    assert_eq!(
        bound_err(Definition::parse(&at)),
        BoundError::TooLarge {
            limit: max,
            actual: max + 1
        }
    );
}

#[test]
fn nesting_is_refused_by_the_scan_before_typed_parsing() {
    let depth = |n: usize| format!("{}{}", "[".repeat(n), "]".repeat(n));
    // At the limit the scan accepts and the typed parse then refuses the shape.
    let at = depth(DEFINITION_V1.max_depth);
    assert!(matches!(
        Definition::parse(at.as_bytes()),
        Err(DocumentError::Parse(ParseError::Schema { .. }))
    ));
    let past = depth(DEFINITION_V1.max_depth + 1);
    assert!(matches!(
        bound_err(Definition::parse(past.as_bytes())),
        BoundError::TooDeep { .. }
    ));
    // Hostile depth does not exhaust the stack.
    let hostile = depth(100_000);
    assert!(matches!(
        bound_err(Definition::parse(hostile.as_bytes())),
        BoundError::TooLarge { .. } | BoundError::TooDeep { .. }
    ));
}

#[test]
fn strings_and_collections_are_bounded() {
    let long = "a".repeat(DEFINITION_V1.max_string_bytes + 1);
    let doc = MINIMAL.replacen("\"name\":\"m\"", &format!("\"name\":\"{long}\""), 1);
    assert!(matches!(
        bound_err(Definition::parse(doc.as_bytes())),
        BoundError::StringTooLong { .. }
    ));
    let items = vec!["\"user-supplied\""; DEFINITION_V1.max_collection_len + 1].join(",");
    let doc = MINIMAL.replacen("[\"user-supplied\"]", &format!("[{items}]"), 1);
    assert!(matches!(
        bound_err(Definition::parse(doc.as_bytes())),
        BoundError::CollectionTooLong { .. }
    ));
}

#[test]
fn fractional_numbers_are_refused_where_identity_depends_on_them() {
    let doc = MINIMAL.replacen("\"deadline_ms\":0", "\"deadline_ms\":0.5", 1);
    assert!(matches!(
        bound_err(Definition::parse(doc.as_bytes())),
        BoundError::FractionalNumber { .. }
    ));
    let snap = br#"{"schema":"rustev.snapshot/1","entries":[{"field":"x","value":1.5,"provenance":"user-supplied","as_of_ms":0,"source":"s"}]}"#;
    assert!(matches!(
        Snapshot::parse(snap),
        Err(DocumentError::Parse(ParseError::Bound(
            BoundError::FractionalNumber { .. }
        )))
    ));
    // Backend outputs carry masses as numbers.
    let out = br#"{"schema":"rustev.backend-output/1","step":"s","instance":[],"artifact":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","output":{"distribution":{"a":0.25,"b":0.75}}}"#;
    assert!(BackendOutputDoc::parse(out).is_ok());
    let _ = SNAPSHOT_V1;
}

#[test]
fn value_counts_are_bounded_cumulatively() {
    // 70 arrays of 4000 one-element arrays: 560 070 values, each collection
    // within its bound and the bytes within theirs; the total is not.
    let inner = vec!["[1]"; 4000].join(",");
    let arrays = vec![format!("[{inner}]"); 70].join(",");
    let doc = format!(
        "{{\"schema\":\"rustev.snapshot/1\",\"entries\":[{{\"field\":\"x\",\"value\":[{arrays}],\"provenance\":\"user-supplied\",\"as_of_ms\":0,\"source\":\"s\"}}]}}"
    );
    assert!(matches!(
        Snapshot::parse(doc.as_bytes()),
        Err(DocumentError::Parse(ParseError::Bound(
            BoundError::TooManyValues { .. }
        )))
    ));
}

#[test]
fn an_unknown_metric_stays_unknown() {
    let report = br#"{"schema":"rustev.eval-report/1","plan":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","artifacts":[],"calibrations":[],
"dataset":{"id":"sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","split":"final_test","provenance":{"synthetic":{"generator":"fixture"}}},
"evaluator_config":"sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee","status":{"incomplete":{"reason":"no labels"}},
"metrics":[{"name":"macro_f1","value":{"unknown":{"reason":"no independent labels"}}}]}"#;
    let r = EvalReport::parse(report).unwrap();
    assert!(matches!(r.metrics[0].value, MetricValue::Unknown { .. }));
    // A metric cannot be written as a bare pass.
    let pass = std::str::from_utf8(report).unwrap().replace(
        "{\"unknown\":{\"reason\":\"no independent labels\"}}",
        "\"pass\"",
    );
    assert!(EvalReport::parse(pass.as_bytes()).is_err());
}
