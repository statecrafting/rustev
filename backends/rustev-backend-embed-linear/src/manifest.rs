//! The bundle manifest, `rustev.embed-linear/1` (spec 011, 3.4 and 3.5):
//! the document a host provisions beside the files it names, its
//! consistency checks and its identity.

use std::collections::BTreeSet;
use std::fmt;

use rustev_contract::canonical::{CanonicalError, tagged_digest};
use rustev_contract::decimal::Decimal;
use rustev_contract::definition::Operation;
use rustev_contract::ids::ArtifactId;
use rustev_contract::limits::DESCRIPTOR_V1;
use rustev_contract::{Document, DocumentError};
use serde::{Deserialize, Serialize};

/// The schema string of a bundle manifest.
pub const SCHEMA: &str = "rustev.embed-linear/1";

/// The bounds of schema version 1.
pub mod limits {
    /// A whole bundle: manifest and every named file (3.3, 3.4.1).
    pub const BUNDLE_BYTES: u64 = 150 * 1024 * 1024;
    pub const BACKEND_ID_BYTES: usize = 64;
    pub const PATH_BYTES: usize = 256;
    pub const CHECKPOINT_FILES: usize = 64;
    pub const TEXT_BYTES: usize = 256;
    pub const HIDDEN_SIZE: u64 = 4_096;
    pub const GRAPH_INPUTS: usize = 8;
    /// The selected checkpoint's position limit (3.8.4).
    pub const MAX_TOKENS: u64 = 512;
    pub const THREADS: u64 = 64;
    pub const HEADS: usize = 32;
    pub const TASK_BYTES: usize = 256;
    pub const QUESTION_BYTES: usize = 4_096;
    pub const OPTIONS_MIN: usize = 2;
    pub const OPTIONS: usize = 64;
    pub const OPTION_BYTES: usize = 256;
    pub const FIELDS: usize = 8;
    pub const LABEL_BYTES: usize = 64;
    pub const SEPARATOR_BYTES: usize = 16;
    pub const INTERPRETATION_BYTES: usize = 1_024;
    pub const MAX_PROJECTION_BYTES: u64 = 65_536;
    pub const MAX_TEXT_BYTES: u64 = 65_536;
}

/// The 3.3 numerics bounds every manifest declares (3.13.1).
pub const ABS_LOGIT: Decimal = Decimal::from_units(10_000);
pub const REL_LOGIT: Decimal = Decimal::from_units(100_000);
pub const DECISION_MARGIN: Decimal = Decimal::from_units(1_000_000);

/// A bundle manifest (3.4.2). Every member is required.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbedManifest {
    pub schema: String,
    pub backend_id: String,
    pub checkpoint: Checkpoint,
    pub encoder: Encoder,
    pub tokenizer: Tokenizer,
    pub pooling: Pooling,
    pub runtime: Runtime,
    pub heads: Vec<Head>,
    pub limits: Limits,
    pub cost: Cost,
    pub tolerance: Tolerance,
}

impl Document for EmbedManifest {
    const SCHEMA: &'static str = SCHEMA;
    const LIMITS: rustev_contract::bounded::ParseLimits = DESCRIPTOR_V1;
    fn schema(&self) -> &str {
        &self.schema
    }
}

impl EmbedManifest {
    /// The artifact identity (3.5.1): the contract's tagged digest of the
    /// manifest's canonical bytes. It covers every named file through its
    /// SHA-256.
    pub fn artifact_id(&self) -> Result<ArtifactId, CanonicalError> {
        let digest = tagged_digest(SCHEMA, &self.canonical()?);
        ArtifactId::parse(&digest).map_err(|e| CanonicalError::Serialize {
            message: e.to_string(),
        })
    }

    /// Every file the manifest names, graph first, then the tokenizer and
    /// each head's weights in head order.
    pub fn files(&self) -> Vec<FileRef> {
        let g = &self.encoder.graph;
        let t = &self.tokenizer;
        let mut files = vec![
            FileRef {
                file: g.file.clone(),
                bytes: g.bytes,
                sha256: g.sha256.clone(),
            },
            FileRef {
                file: t.file.clone(),
                bytes: t.bytes,
                sha256: t.sha256.clone(),
            },
        ];
        files.extend(self.heads.iter().map(|h| h.weights.clone()));
        files
    }

    /// The largest charge of one call (3.10.3), or `None` on overflow.
    pub fn max_units(&self) -> Option<u64> {
        self.cost
            .units_per_token
            .checked_mul(self.tokenizer.max_tokens)?
            .checked_add(self.cost.units_per_call)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    /// The checkpoint's repository, or `synthetic` for a test bundle.
    pub repository: String,
    /// The pinned revision: 40 lowercase hex digits.
    pub revision: String,
    pub files: Vec<CheckpointFile>,
    pub license: String,
    pub base_model: String,
}

/// A file of the checkpoint revision, by its path there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointFile {
    pub name: String,
    pub sha256: String,
}

/// A bundle file: a relative path, its byte length and its SHA-256.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileRef {
    pub file: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Encoder {
    pub kind: EncoderKind,
    pub graph: Graph,
    pub inputs: Vec<String>,
    pub output: String,
    pub hidden_size: u64,
    pub precision: Precision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EncoderKind {
    Onnx,
    Synthetic,
}

/// The graph file and its path in the checkpoint revision (`synthetic` for
/// the synthetic encoder).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Graph {
    pub file: String,
    pub bytes: u64,
    pub sha256: String,
    pub source: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Precision {
    F32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tokenizer {
    pub file: String,
    pub bytes: u64,
    pub sha256: String,
    /// `tokenizers@<exact version>`: identity, since it can change ids.
    pub implementation: String,
    pub max_tokens: u64,
    pub truncation: Truncation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Truncation {
    Refuse,
    TruncateEnd,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pooling {
    pub mode: PoolingMode,
    pub normalize: Normalize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PoolingMode {
    MeanMasked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Normalize {
    L2,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Runtime {
    pub onnxruntime: String,
    pub ort: String,
    pub execution_provider: ExecutionProvider,
    pub graph_optimization: GraphOptimization,
    pub intra_op_threads: u64,
    pub inter_op_threads: u64,
    pub libraries: Vec<Library>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionProvider {
    Cpu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphOptimization {
    Disabled,
    Basic,
    Extended,
    All,
}

/// The accepted runtime library for one platform.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Library {
    pub platform: Platform,
    pub sha256: String,
}

/// A platform key (3.6.3): architecture, OS and C library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Platform {
    #[serde(rename = "linux-x86_64-gnu")]
    LinuxX86_64Gnu,
    #[serde(rename = "linux-aarch64-gnu")]
    LinuxAarch64Gnu,
    #[serde(rename = "macos-aarch64")]
    MacosAarch64,
}

/// A task-specific linear head (3.9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Head {
    pub operation: Operation,
    pub task: String,
    pub question: String,
    pub options: Vec<String>,
    pub text: TextTemplate,
    pub weights: FileRef,
    pub interpretation: String,
    pub provenance: Provenance,
}

/// How a head's text is assembled from the projection (3.8.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextTemplate {
    pub fields: Vec<TextField>,
    pub separator: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextField {
    pub label: String,
    #[serde(rename = "ref")]
    pub reference: String,
}

/// Who produced a head and from what (3.9.4). Rustev never fits one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub producer: String,
    pub method: String,
    pub training_data_description: String,
    /// 64 lowercase hex digits, or `none`.
    pub training_data_sha256: String,
    pub synthetic: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub max_projection_bytes: u64,
    pub max_text_bytes: u64,
}

/// Local quota units, not money (3.11.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cost {
    pub units_per_call: u64,
    pub units_per_token: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tolerance {
    pub abs_logit: Decimal,
    pub rel_logit: Decimal,
    pub decision_margin: Decimal,
    /// The contract digest of the feasibility record that measured the
    /// bounds, or `none` for a synthetic bundle.
    pub feasibility_record: String,
}

/// Why a manifest is refused (`manifest_invalid`, 3.4.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestError {
    /// The member at fault, as a dotted path.
    pub at: String,
    pub reason: String,
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.at, self.reason)
    }
}

impl std::error::Error for ManifestError {}

impl From<DocumentError> for ManifestError {
    fn from(e: DocumentError) -> Self {
        ManifestError {
            at: String::new(),
            reason: e.to_string(),
        }
    }
}

fn fail<T>(at: impl Into<String>, reason: impl Into<String>) -> Result<T, ManifestError> {
    Err(ManifestError {
        at: at.into(),
        reason: reason.into(),
    })
}

fn ensure(ok: bool, at: impl Into<String>, reason: &str) -> Result<(), ManifestError> {
    if ok { Ok(()) } else { fail(at, reason) }
}

fn is_hex(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn bounded_text(s: &str, max: usize) -> bool {
    !s.is_empty() && s.len() <= max
}

/// An exact version: `MAJOR.MINOR.PATCH` with an optional `-pre` part.
fn exact_version(s: &str) -> bool {
    let (core, pre) = match s.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (s, None),
    };
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        && pre.is_none_or(|p| {
            !p.is_empty()
                && p.split('.')
                    .all(|i| !i.is_empty() && i.bytes().all(|b| b.is_ascii_alphanumeric()))
        })
}

/// A bundle-relative path: no root, no `..`, `.` or empty component, no
/// backslash and never the manifest itself (3.4.1).
pub fn relative_path(p: &str) -> bool {
    bounded_text(p, limits::PATH_BYTES)
        && p != "manifest.json"
        && !p.contains('\\')
        && !p.contains('\0')
        && p.split('/').all(|c| !c.is_empty() && c != "." && c != "..")
}

/// `input:<name>` or `bind:<name>`, optionally `/<member>`, as spec 005
/// 3.4 reads fields.
pub fn field_ref(r: &str) -> Option<(&str, Option<&str>)> {
    let (key, member) = match r.split_once('/') {
        Some((k, m)) => (k, Some(m)),
        None => (r, None),
    };
    let name = key
        .strip_prefix("input:")
        .or_else(|| key.strip_prefix("bind:"))?;
    let simple = |s: &str| !s.is_empty() && !s.contains('/') && !s.contains(':');
    (simple(name) && member.is_none_or(simple)).then_some((key, member))
}

/// Check a parsed manifest's members, bounds and consistency (3.4.3).
/// Nothing is read from disk and nothing reaches a runtime.
pub fn check(m: &EmbedManifest) -> Result<(), ManifestError> {
    let synthetic = m.encoder.kind == EncoderKind::Synthetic;
    ensure(
        bounded_text(&m.backend_id, limits::BACKEND_ID_BYTES)
            && m.backend_id
                .bytes()
                .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-')),
        "backend_id",
        "1 to 64 bytes of [a-z0-9._-]",
    )?;

    // Checkpoint provenance.
    let c = &m.checkpoint;
    ensure(
        bounded_text(&c.repository, limits::TEXT_BYTES),
        "checkpoint.repository",
        "1 to 256 bytes",
    )?;
    ensure(
        synthetic == (c.repository == "synthetic"),
        "checkpoint.repository",
        "`synthetic` exactly when the encoder is synthetic",
    )?;
    ensure(
        is_hex(&c.revision, 40),
        "checkpoint.revision",
        "40 lowercase hex digits",
    )?;
    ensure(
        c.files.len() <= limits::CHECKPOINT_FILES,
        "checkpoint.files",
        "at most 64 files",
    )?;
    let mut names = BTreeSet::new();
    for (i, f) in c.files.iter().enumerate() {
        let at = format!("checkpoint.files[{i}]");
        ensure(relative_path(&f.name), &at, "a relative path")?;
        ensure(is_hex(&f.sha256, 64), &at, "sha256 is 64 lowercase hex")?;
        ensure(names.insert(f.name.as_str()), &at, "duplicate file name")?;
    }
    for (at, s) in [
        ("checkpoint.license", &c.license),
        ("checkpoint.base_model", &c.base_model),
    ] {
        ensure(bounded_text(s, limits::TEXT_BYTES), at, "1 to 256 bytes")?;
    }

    // Encoder and graph.
    let e = &m.encoder;
    ensure(
        (1..=limits::HIDDEN_SIZE).contains(&e.hidden_size),
        "encoder.hidden_size",
        "1 to 4096",
    )?;
    ensure(
        !e.inputs.is_empty() && e.inputs.len() <= limits::GRAPH_INPUTS,
        "encoder.inputs",
        "1 to 8 names",
    )?;
    let mut inputs = BTreeSet::new();
    for (i, n) in e.inputs.iter().enumerate() {
        let at = format!("encoder.inputs[{i}]");
        ensure(bounded_text(n, limits::TEXT_BYTES), &at, "1 to 256 bytes")?;
        ensure(inputs.insert(n.as_str()), &at, "duplicate input")?;
    }
    ensure(
        bounded_text(&e.output, limits::TEXT_BYTES),
        "encoder.output",
        "1 to 256 bytes",
    )?;
    if synthetic {
        ensure(
            e.graph.source == "synthetic",
            "encoder.graph.source",
            "`synthetic` for the synthetic encoder",
        )?;
    } else {
        ensure(
            c.files
                .iter()
                .any(|f| f.name == e.graph.source && f.sha256 == e.graph.sha256),
            "encoder.graph.source",
            "not among checkpoint.files with the graph's sha256",
        )?;
    }

    // Tokenizer and truncation.
    let t = &m.tokenizer;
    ensure(
        t.implementation
            .strip_prefix("tokenizers@")
            .is_some_and(exact_version),
        "tokenizer.implementation",
        "`tokenizers@<exact version>`",
    )?;
    ensure(
        (1..=limits::MAX_TOKENS).contains(&t.max_tokens),
        "tokenizer.max_tokens",
        "1 to the graph's position limit (512)",
    )?;

    // Runtime settings.
    let r = &m.runtime;
    ensure(
        exact_version(&r.onnxruntime),
        "runtime.onnxruntime",
        "an exact version",
    )?;
    ensure(exact_version(&r.ort), "runtime.ort", "an exact version")?;
    for (at, n) in [
        ("runtime.intra_op_threads", r.intra_op_threads),
        ("runtime.inter_op_threads", r.inter_op_threads),
    ] {
        ensure((1..=limits::THREADS).contains(&n), at, "1 to 64")?;
    }
    let mut platforms = BTreeSet::new();
    for (i, l) in r.libraries.iter().enumerate() {
        let at = format!("runtime.libraries[{i}]");
        ensure(is_hex(&l.sha256, 64), &at, "sha256 is 64 lowercase hex")?;
        ensure(platforms.insert(l.platform), &at, "duplicate platform")?;
    }
    ensure(
        synthetic || !r.libraries.is_empty(),
        "runtime.libraries",
        "an ONNX bundle names at least one library",
    )?;

    // Limits, cost, tolerance.
    ensure(
        (1..=limits::MAX_PROJECTION_BYTES).contains(&m.limits.max_projection_bytes),
        "limits.max_projection_bytes",
        "1 to 65536",
    )?;
    ensure(
        (1..=limits::MAX_TEXT_BYTES).contains(&m.limits.max_text_bytes),
        "limits.max_text_bytes",
        "1 to 65536",
    )?;
    ensure(
        m.max_units().is_some(),
        "cost",
        "units_per_call + units_per_token * max_tokens overflows",
    )?;
    let tol = &m.tolerance;
    ensure(
        tol.abs_logit == ABS_LOGIT
            && tol.rel_logit == REL_LOGIT
            && tol.decision_margin == DECISION_MARGIN,
        "tolerance",
        "the 3.3 bounds: abs_logit 0.00001, rel_logit 0.0001, decision_margin 0.001",
    )?;
    if synthetic {
        ensure(
            tol.feasibility_record == "none",
            "tolerance.feasibility_record",
            "`none` for a synthetic bundle",
        )?;
    } else {
        ensure(
            ArtifactId::parse(&tol.feasibility_record).is_ok(),
            "tolerance.feasibility_record",
            "the record's contract digest",
        )?;
    }

    check_heads(m, synthetic)?;
    check_files(m)
}

fn check_heads(m: &EmbedManifest, synthetic: bool) -> Result<(), ManifestError> {
    ensure(
        !m.heads.is_empty() && m.heads.len() <= limits::HEADS,
        "heads",
        "1 to 32 heads",
    )?;
    let mut keys = BTreeSet::new();
    for (i, h) in m.heads.iter().enumerate() {
        let at = |member: &str| format!("heads[{i}].{member}");
        ensure(
            matches!(
                h.operation,
                Operation::Classify | Operation::Proposition | Operation::Rubric
            ),
            at("operation"),
            "classify, proposition or rubric",
        )?;
        ensure(
            bounded_text(&h.task, limits::TASK_BYTES),
            at("task"),
            "1 to 256 bytes",
        )?;
        ensure(
            bounded_text(&h.question, limits::QUESTION_BYTES),
            at("question"),
            "1 to 4096 bytes",
        )?;
        ensure(
            (limits::OPTIONS_MIN..=limits::OPTIONS).contains(&h.options.len()),
            at("options"),
            "2 to 64 options",
        )?;
        let unique: BTreeSet<&str> = h.options.iter().map(String::as_str).collect();
        ensure(
            unique.len() == h.options.len(),
            at("options"),
            "options are not unique",
        )?;
        ensure(
            h.options
                .iter()
                .all(|o| bounded_text(o, limits::OPTION_BYTES)),
            at("options"),
            "each option is 1 to 256 bytes",
        )?;
        if h.operation == Operation::Proposition {
            ensure(
                h.options == ["false", "true"],
                at("options"),
                "a proposition's options are exactly [\"false\", \"true\"]",
            )?;
        }
        ensure(
            keys.insert((h.operation, &h.task, &h.question, &h.options)),
            at("task"),
            "two heads answer the same operation, task, question and options",
        )?;
        let fields = &h.text.fields;
        ensure(
            !fields.is_empty() && fields.len() <= limits::FIELDS,
            at("text.fields"),
            "1 to 8 fields",
        )?;
        for (j, f) in fields.iter().enumerate() {
            ensure(
                f.label.len() <= limits::LABEL_BYTES,
                format!("heads[{i}].text.fields[{j}].label"),
                "at most 64 bytes",
            )?;
            ensure(
                field_ref(&f.reference).is_some(),
                format!("heads[{i}].text.fields[{j}].ref"),
                "`input:<name>` or `bind:<name>`, optionally `/<member>`",
            )?;
        }
        ensure(
            h.text.separator.len() <= limits::SEPARATOR_BYTES,
            at("text.separator"),
            "at most 16 bytes",
        )?;
        ensure(
            bounded_text(&h.interpretation, limits::INTERPRETATION_BYTES),
            at("interpretation"),
            "1 to 1024 bytes",
        )?;
        let p = &h.provenance;
        for (member, s) in [
            ("provenance.producer", &p.producer),
            ("provenance.method", &p.method),
        ] {
            ensure(
                bounded_text(s, limits::TEXT_BYTES),
                at(member),
                "1 to 256 bytes",
            )?;
        }
        ensure(
            bounded_text(&p.training_data_description, limits::INTERPRETATION_BYTES),
            at("provenance.training_data_description"),
            "1 to 1024 bytes",
        )?;
        ensure(
            p.training_data_sha256 == "none" || is_hex(&p.training_data_sha256, 64),
            at("provenance.training_data_sha256"),
            "64 lowercase hex digits or `none`",
        )?;
        if synthetic {
            ensure(
                h.interpretation.contains("SYNTHETIC") && p.synthetic,
                at("interpretation"),
                "a synthetic bundle's head says SYNTHETIC and has synthetic provenance",
            )?;
        }
    }
    Ok(())
}

/// Every named file: a relative path, a 64-hex SHA-256, and one
/// declaration per path (the bundle bound is checked at setup).
fn check_files(m: &EmbedManifest) -> Result<(), ManifestError> {
    let mut seen: Vec<FileRef> = Vec::new();
    for f in m.files() {
        ensure(
            relative_path(&f.file),
            &f.file,
            "not a bundle-relative path",
        )?;
        ensure(is_hex(&f.sha256, 64), &f.file, "sha256 is 64 lowercase hex")?;
        if let Some(prior) = seen.iter().find(|p| p.file == f.file) {
            ensure(
                prior.bytes == f.bytes && prior.sha256 == f.sha256,
                &f.file,
                "declared twice with different bytes or sha256",
            )?;
        } else {
            seen.push(f);
        }
    }
    Ok(())
}

/// The distinct files of a checked manifest and their declared total.
pub(crate) fn distinct_files(m: &EmbedManifest) -> (Vec<FileRef>, u64) {
    let mut seen: Vec<FileRef> = Vec::new();
    for f in m.files() {
        if !seen.iter().any(|p| p.file == f.file) {
            seen.push(f);
        }
    }
    let total = seen.iter().fold(0u64, |a, f| a.saturating_add(f.bytes));
    (seen, total)
}
