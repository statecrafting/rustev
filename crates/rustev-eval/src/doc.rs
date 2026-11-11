//! Implements the contract's document protocol for eval-owned documents.

/// `Document` (and optionally `Identified`) for a type with a `schema`
/// field, as the contract's own documents do.
macro_rules! eval_document {
    ($ty:ty, $schema:expr, $limits:expr) => {
        impl rustev_contract::Document for $ty {
            const SCHEMA: &'static str = $schema;
            const LIMITS: rustev_contract::bounded::ParseLimits = $limits;
            fn schema(&self) -> &str {
                &self.schema
            }
        }
    };
    ($ty:ty, $schema:expr, $limits:expr, $id:ty) => {
        $crate::doc::eval_document!($ty, $schema, $limits);
        impl rustev_contract::Identified for $ty {
            type Id = $id;
            fn wrap(digest: String) -> $id {
                <$id>::parse(&digest).expect("a tagged digest is an identity")
            }
        }
    };
}

pub(crate) use eval_document;

/// One emitted schema: the file name it is committed under and its
/// canonical bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmittedSchema {
    pub file: String,
    pub bytes: Vec<u8>,
}

/// What every emitted schema says about itself (spec 072, 3.3).
const COMMENT: &str = "Descriptive, derived from the types the bounded parser reads; the parser \
is the only authority on validity. Byte, depth and count bounds are recorded in \
x-rustev-parse-limits (a string length here counts characters, the parser counts bytes), and \
rules the document's own checks apply (ranges, names, declared metrics) are stated in field \
descriptions, not enforced here.";

/// The JSON Schema of `T` for its document version, as spec 027 3.2.2
/// emits the contract's documents: draft 2020-12, `schema` fixed to the
/// version, the parse limits the bounded parser enforces recorded as
/// `x-rustev-parse-limits`, canonical bytes.
fn emit<T: schemars::JsonSchema + rustev_contract::Document>() -> EmittedSchema {
    let generator = schemars::generate::SchemaSettings::draft2020_12().into_generator();
    let root = generator.into_root_schema_for::<T>();
    let mut v = serde_json::to_value(&root).expect("a schema is JSON");
    v["title"] = T::SCHEMA.into();
    v["$comment"] = COMMENT.into();
    // Every document is an object with a `schema` member; a schema without
    // `properties` is a defect in the derivation, which the committed-bytes
    // test reports by name.
    v.get_mut("properties")
        .and_then(|p| p.as_object_mut())
        .unwrap_or_else(|| panic!("{}: the derived schema has no properties", T::SCHEMA))
        .insert(
            "schema".into(),
            serde_json::json!({ "type": "string", "const": T::SCHEMA }),
        );
    let l = T::LIMITS;
    v["x-rustev-parse-limits"] = serde_json::json!({
        "max_bytes": l.max_bytes,
        "max_depth": l.max_depth,
        "max_string_bytes": l.max_string_bytes,
        "max_collection_len": l.max_collection_len,
        "max_total_values": l.max_total_values,
        "max_number_bytes": l.max_number_bytes,
        "allow_fractional_numbers": l.allow_fractional_numbers,
    });
    let file = format!("{}.schema.json", T::SCHEMA.replace('/', "-"));
    let bytes = rustev_contract::canonical::canonical_value_bytes(&v)
        .expect("a schema has canonical bytes");
    EmittedSchema { file, bytes }
}

/// The JSON Schemas (draft 2020-12) of spec 029's host documents,
/// `rustev.repeat-config/1`, `rustev.baseline/1`, `rustev.selection-gate/1`
/// and `rustev.selection/1` (spec 072, 3.3), sorted by file name. Emission is
/// pure and deterministic; the schemas are descriptive and the bounded
/// parsers stay the only authority on validity.
pub fn document_schemas() -> Vec<EmittedSchema> {
    let mut out = vec![
        emit::<crate::repeat_config::RepeatConfig>(),
        emit::<crate::baseline::Baseline>(),
        emit::<crate::selection::SelectionGate>(),
        emit::<crate::selection::Selection>(),
    ];
    out.sort_by(|a, b| a.file.cmp(&b.file));
    out
}
