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
    emit_adjusted::<T>(version, part, |_| {})
}

/// [`emit`], with `adjust` applied to the derived schema before the title,
/// comment, version and limits are set: for a document whose parser holds a
/// version rule the shared type cannot express.
fn emit_adjusted<T: schemars::JsonSchema + Document>(
    version: &str,
    part: Option<&str>,
    adjust: impl FnOnce(&mut serde_json::Value),
) -> EmittedSchema {
    let generator = schemars::generate::SchemaSettings::draft2020_12().into_generator();
    let root = generator.into_root_schema_for::<T>();
    let mut v = serde_json::to_value(&root).expect("a schema is JSON");
    adjust(&mut v);
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

/// The members only `rustev.run/2` carries (spec 020, 3.2). The run
/// record's parser refuses them in every other version and requires the list
/// in version 2, so each run schema states exactly that.
const RUN_V2_MEMBERS: [&str; 2] = ["remote_evidence", "remote_evidence_truncated"];

fn run_members(version: &str) -> impl FnOnce(&mut serde_json::Value) + '_ {
    move |v| {
        if version == schema::RUN_V2 {
            // The list is required and never null: `null` reads as absent,
            // which the parser refuses in version 2.
            v["required"]
                .as_array_mut()
                .expect("the run record has required members")
                .push(RUN_V2_MEMBERS[0].into());
            let list = &mut v["properties"][RUN_V2_MEMBERS[0]];
            assert_eq!(
                list["type"],
                serde_json::json!(["array", "null"]),
                "the remote evidence list is derived as an optional array"
            );
            list["type"] = "array".into();
            return;
        }
        let props = v["properties"]
            .as_object_mut()
            .expect("the run record has properties");
        for m in RUN_V2_MEMBERS {
            props
                .remove(m)
                .unwrap_or_else(|| panic!("the run record has no {m} member"));
        }
        prune_defs(v);
    }
}

/// Removes every `$defs` entry no longer reachable from the schema's own
/// members, so a version without a member does not describe its types.
fn prune_defs(v: &mut serde_json::Value) {
    fn refs(v: &serde_json::Value, out: &mut Vec<String>) {
        match v {
            serde_json::Value::Object(m) => {
                if let Some(r) = m.get("$ref").and_then(|r| r.as_str())
                    && let Some(name) = r.strip_prefix("#/$defs/")
                {
                    out.push(name.to_string());
                }
                m.values().for_each(|x| refs(x, out));
            }
            serde_json::Value::Array(a) => a.iter().for_each(|x| refs(x, out)),
            _ => {}
        }
    }
    let Some(defs) = v.get("$defs").cloned() else {
        return;
    };
    let mut root = v.clone();
    root.as_object_mut().unwrap().remove("$defs");
    let mut pending = Vec::new();
    refs(&root, &mut pending);
    let mut reached = std::collections::BTreeSet::new();
    while let Some(name) = pending.pop() {
        if reached.insert(name.clone())
            && let Some(d) = defs.get(&name)
        {
            refs(d, &mut pending);
        }
    }
    let kept: serde_json::Map<String, serde_json::Value> = defs
        .as_object()
        .unwrap()
        .iter()
        .filter(|(k, _)| reached.contains(*k))
        .map(|(k, d)| (k.clone(), d.clone()))
        .collect();
    v["$defs"] = kept.into();
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
    // Specs 019, 026 and 028 (R-46): the dynamic-question documents, live
    // re-execution evidence, the redaction policy and the span line.
    use crate::{
        dynamic::{DynamicCapability, DynamicOutcome, DynamicQuestion},
        live::LiveReexecution,
        redaction::{REDACTION, RedactionPolicy},
        trace::{TRACE, TraceLine},
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
        emit_adjusted::<RunRecord>(schema::RUN, None, run_members(schema::RUN)),
        emit_adjusted::<RunRecord>(schema::RUN_V2, None, run_members(schema::RUN_V2)),
        emit_adjusted::<RunRecord>(schema::RUN_V3, None, run_members(schema::RUN_V3)),
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
        emit::<DynamicQuestion>(schema::DYNAMIC_QUESTION, None),
        emit::<DynamicCapability>(schema::DYNAMIC_CAPABILITY, None),
        emit::<DynamicOutcome>(schema::DYNAMIC_OUTCOME, None),
        emit::<LiveReexecution>(schema::LIVE_REEXECUTION, None),
        emit::<RedactionPolicy>(REDACTION, None),
        emit::<TraceLine>(TRACE, None),
    ];
    out.sort_by(|a, b| a.file.cmp(&b.file));
    out
}
