//! JSON Schema (draft 2020-12) for every public document type and version
//! (spec 027, 3.2.2), derived from the same type definitions the bounded
//! parser enforces. Emission is pure and deterministic: canonical JSON with
//! sorted keys, byte-identical across runs and targets. The schemas are
//! descriptive; the bounded parser stays the only authority on validity.

use crate::bounded::ParseLimits;
use crate::{Document, schema};

/// A tagged content digest, `sha256:` and 64 lowercase hex digits.
pub(crate) fn digest_schema() -> schemars::Schema {
    schemars::json_schema!({
        "type": "string",
        "pattern": "^sha256:[0-9a-f]{64}$"
    })
}

/// One emitted schema: the file name it is committed under and its
/// canonical bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmittedSchema {
    pub file: String,
    pub bytes: Vec<u8>,
}

/// Versions that share one wire type (judgment 1 and 2, evidence 1 and 2,
/// and the other #76 track pairs) share its schema except for `schema`: the
/// bounded parser reads both versions with that one type and accepts a later
/// version's members under the earlier `schema` string, so a stricter
/// earlier schema would describe something the parser does not enforce.
/// The schema describes the parser; it is never stricter than it (3.2.2).
///
/// The JSON Schema of `T` for the document version `version`: draft
/// 2020-12, `schema` fixed to `version`, the parse limits the bounded parser
/// enforces recorded as `x-rustev-parse-limits` (a schema cannot express
/// byte, depth or value-count bounds), canonical bytes.
/// What every emitted schema says about itself (spec 027, 3.2.2).
const COMMENT: &str = "Descriptive, derived from the types the bounded parser reads; the parser \
is the only authority on validity. Where the parser reads two versions of a document with one \
type, this schema admits what that type admits, including members the later version adds. \
Byte, depth and count bounds are recorded in x-rustev-parse-limits (a string length here counts \
characters, the parser counts bytes), and rules applied when a plan is compiled are stated in \
field descriptions, not enforced here.";

fn emit<T: schemars::JsonSchema + Document>(version: &str, part: Option<&str>) -> EmittedSchema {
    let generator = schemars::generate::SchemaSettings::draft2020_12().into_generator();
    let root = generator.into_root_schema_for::<T>();
    let mut v = serde_json::to_value(&root).expect("a schema is JSON");
    let title = match part {
        Some(p) => format!("{version} {p}"),
        None => version.to_string(),
    };
    v["title"] = title.clone().into();
    v["$comment"] = COMMENT.into();
    // Every document is an object with a `schema` member; a schema without
    // `properties` would be a defect in the derivation, never skipped. The
    // committed-bytes test emits every document, so such a defect fails it
    // naming the document, before any schema is committed or shipped.
    v.get_mut("properties")
        .and_then(|p| p.as_object_mut())
        .unwrap_or_else(|| panic!("{title}: the derived schema has no properties"))
        .insert(
            "schema".into(),
            serde_json::json!({ "type": "string", "const": version }),
        );
    v["x-rustev-parse-limits"] = limits(&T::LIMITS);
    let file = format!(
        "{}{}.schema.json",
        version.replace('/', "-"),
        part.map(|p| format!(".{p}")).unwrap_or_default()
    );
    let bytes = crate::canonical::canonical_value_bytes(&v).expect("a schema has canonical bytes");
    EmittedSchema { file, bytes }
}

fn limits(l: &ParseLimits) -> serde_json::Value {
    serde_json::json!({
        "max_bytes": l.max_bytes,
        "max_depth": l.max_depth,
        "max_string_bytes": l.max_string_bytes,
        "max_collection_len": l.max_collection_len,
        "max_total_values": l.max_total_values,
        "max_number_bytes": l.max_number_bytes,
        "allow_fractional_numbers": l.allow_fractional_numbers,
    })
}

/// Every public document type and version, sorted by file name.
pub fn document_schemas() -> Vec<EmittedSchema> {
    use crate::{
        calibration::CalibrationArtifact, definition::Definition, descriptor::BackendDescriptor,
        eval_report::EvalReport, evidence::EvidenceRecord, execution::ExecutionPolicy,
        judgment::Judgment, output::BackendOutputDoc, plan::Plan, reason::RuntimeReasonDoc, remote,
        replay::ReplayBundle, request::RequestDoc, run::RunRecord, snapshot::Snapshot,
        source::SourceDescriptor,
    };
    let mut out = vec![
        emit::<Definition>(schema::DEFINITION, None),
        emit::<Definition>(schema::DEFINITION_V2, None),
        emit::<Plan>(schema::PLAN, None),
        emit::<Plan>(schema::PLAN_V3, None),
        emit::<Snapshot>(schema::SNAPSHOT, None),
        emit::<Snapshot>(schema::SNAPSHOT_V2, None),
        emit::<EvidenceRecord>(schema::EVIDENCE, None),
        emit::<EvidenceRecord>(schema::EVIDENCE_V2, None),
        emit::<RunRecord>(schema::RUN, None),
        emit::<RunRecord>(schema::RUN_V2, None),
        emit::<RunRecord>(schema::RUN_V3, None),
        emit::<Judgment>(schema::JUDGMENT, None),
        emit::<Judgment>(schema::JUDGMENT_V2, None),
        emit::<BackendDescriptor>(schema::BACKEND, None),
        emit::<CalibrationArtifact>(schema::CALIBRATION, None),
        emit::<BackendOutputDoc>(schema::BACKEND_OUTPUT, None),
        emit::<EvalReport>(schema::EVAL_REPORT, None),
        emit::<ExecutionPolicy>(schema::EXECUTION, None),
        emit::<ReplayBundle>(schema::REPLAY, None),
        emit::<RequestDoc>(schema::REQUEST, None),
        emit::<RuntimeReasonDoc>(schema::RUNTIME_REASON, None),
        emit::<SourceDescriptor>(schema::SOURCE, None),
        emit::<remote::DescribeRequest>(schema::REMOTE_DESCRIBE, Some("request")),
        emit::<remote::DescribeResponse>(schema::REMOTE_DESCRIBE, Some("response")),
        emit::<remote::InferRequest>(schema::REMOTE_INFER, Some("request")),
        emit::<remote::InferResponse>(schema::REMOTE_INFER, Some("response")),
        emit::<remote::CancelRequest>(schema::REMOTE_CANCEL, Some("request")),
        emit::<remote::CancelResponse>(schema::REMOTE_CANCEL, Some("response")),
        emit::<remote::ExchangeRecord>(schema::REMOTE_EXCHANGE, None),
    ];
    out.sort_by(|a, b| a.file.cmp(&b.file));
    out
}
