//! Dataset manifests, `rustev.dataset/1` (spec 004, 3.5.1).
//!
//! A manifest names each case once, with its source and snapshot identity,
//! one split, a label or an explicit missing label, and its subgroups. Its
//! identity is content-derived. Training, model-selection, calibration and
//! final-test membership are disjoint by construction, and a source or
//! snapshot identity repeated across splits is refused as leakage. Semantic
//! duplicates the manifest cannot identify (two sources with the same
//! content) remain a stated limit: Rustev does not detect them.
//!
//! `rustev.dataset/2` (spec 041, 3.1.1) is version 1 with an optional gold
//! record per case. A version 1 manifest is a version 2 manifest whose every
//! case has no gold, and keeps its bytes and identity; each version's
//! identity is tagged with its own schema.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use rustev_contract::bounded::ParseLimits;
use rustev_contract::canonical::{CanonicalError, tagged_digest};
use rustev_contract::decimal::Decimal;
use rustev_contract::eval_report::{DatasetProvenance, Split};
use rustev_contract::ids::{ContentDigest, DatasetId, SnapshotId};
use rustev_contract::limits::REPLAY_V1;
use rustev_contract::{Document, DocumentError, Identified};
use serde::{Deserialize, Serialize};

use crate::doc::eval_document;
use crate::gold::Gold;

pub const DATASET: &str = "rustev.dataset/1";
/// Version 2: per-case gold records (spec 041, 3.1.1).
pub const DATASET_V2: &str = "rustev.dataset/2";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatasetManifest {
    pub schema: String,
    pub name: String,
    /// Synthetic provenance always prevents empirical quality claims (R-04).
    pub provenance: DatasetProvenance,
    /// The task adapter whose label shape the cases use.
    pub adapter: AdapterRef,
    pub cases: Vec<DatasetCase>,
}

/// Either version parses; `SCHEMA` names version 1 only because the trait
/// holds one constant. The identity is tagged with the document's own
/// schema, so a version 1 identity is what it always was.
impl Document for DatasetManifest {
    const SCHEMA: &'static str = DATASET;
    const LIMITS: ParseLimits = REPLAY_V1;
    fn schema(&self) -> &str {
        &self.schema
    }

    fn parse(bytes: &[u8]) -> Result<Self, DocumentError> {
        let doc: Self = rustev_contract::bounded::parse_bounded(bytes, &Self::LIMITS)
            .map_err(DocumentError::Parse)?;
        if doc.schema != DATASET && doc.schema != DATASET_V2 {
            return Err(DocumentError::Schema {
                expected: DATASET_V2,
                found: doc.schema.chars().take(80).collect(),
            });
        }
        Ok(doc)
    }

    fn record_digest(&self) -> Result<ContentDigest, CanonicalError> {
        let raw = self.record_canonical()?;
        Ok(ContentDigest::parse(&tagged_digest(&self.schema, &raw))
            .expect("a tagged digest is a content digest"))
    }
}

impl Identified for DatasetManifest {
    type Id = DatasetId;
    fn wrap(digest: String) -> DatasetId {
        DatasetId::parse(&digest).expect("a tagged digest is an identity")
    }

    fn id(&self) -> Result<DatasetId, CanonicalError> {
        Ok(Self::wrap(tagged_digest(&self.schema, &self.canonical()?)))
    }
}

/// A task adapter, by name and version.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterRef {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatasetCase {
    pub id: String,
    /// Opaque identity of where the case came from.
    pub source: String,
    pub snapshot: SnapshotId,
    pub split: Split,
    pub label: Label,
    pub subgroups: Vec<String>,
    /// The case's gold record; version 2 only (spec 041, 3.3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gold: Option<Gold>,
}

/// A label, or an explicit statement that there is none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Label {
    Missing,
    Value(String),
}

impl Label {
    pub fn value(&self) -> Option<&str> {
        match self {
            Label::Value(v) => Some(v),
            Label::Missing => None,
        }
    }
}

/// Why a manifest was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DatasetError {
    Schema,
    Empty,
    /// Real provenance needs source, license, labeling method and limits.
    Provenance,
    DuplicateCase {
        case: String,
    },
    /// The same source or snapshot appears in more than one split.
    Leakage {
        case: String,
        identity: String,
    },
    /// The adapter refused the label's shape.
    Label {
        case: String,
        detail: String,
    },
    /// The manifest names another adapter than the one checking it.
    Adapter,
    /// A malformed gold record, or gold in a version 1 manifest (spec 041,
    /// 3.3.2).
    Gold {
        case: String,
        detail: String,
    },
}

impl fmt::Display for DatasetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for DatasetError {}

impl DatasetManifest {
    /// Validate everything but label shape: schema, cases, provenance,
    /// unique ids and leakage across splits.
    pub fn check_structure(&self) -> Result<(), DatasetError> {
        let adapter = self.adapter.clone();
        self.check(&adapter, |_| Ok(()))
    }

    /// Validate the manifest; `check_label` is the named adapter's label
    /// shape check.
    pub fn check(
        &self,
        adapter: &AdapterRef,
        check_label: impl Fn(&str) -> Result<(), String>,
    ) -> Result<(), DatasetError> {
        if self.schema != DATASET && self.schema != DATASET_V2 {
            return Err(DatasetError::Schema);
        }
        if &self.adapter != adapter {
            return Err(DatasetError::Adapter);
        }
        if self.cases.is_empty() {
            return Err(DatasetError::Empty);
        }
        if let DatasetProvenance::Real {
            source,
            license,
            labeling,
            limitations,
        } = &self.provenance
            && [source, license, labeling, limitations]
                .iter()
                .any(|s| s.trim().is_empty())
        {
            return Err(DatasetError::Provenance);
        }
        let mut ids = BTreeSet::new();
        let mut split_of: BTreeMap<String, Split> = BTreeMap::new();
        for c in &self.cases {
            if !ids.insert(&c.id) {
                return Err(DatasetError::DuplicateCase { case: c.id.clone() });
            }
            for identity in [
                format!("source:{}", c.source),
                format!("snapshot:{}", c.snapshot),
            ] {
                match split_of.get(&identity) {
                    Some(s) if *s != c.split => {
                        return Err(DatasetError::Leakage {
                            case: c.id.clone(),
                            identity,
                        });
                    }
                    _ => {
                        split_of.insert(identity, c.split);
                    }
                }
            }
            if let Some(gold) = &c.gold {
                let detail = if self.schema == DATASET {
                    Err("a version 1 manifest holds no gold".to_owned())
                } else {
                    gold.check()
                };
                detail.map_err(|detail| DatasetError::Gold {
                    case: c.id.clone(),
                    detail,
                })?;
            }
            if let Label::Value(v) = &c.label {
                check_label(v).map_err(|detail| DatasetError::Label {
                    case: c.id.clone(),
                    detail,
                })?;
            }
        }
        Ok(())
    }

    /// The cases of one split, in manifest order.
    pub fn split(&self, split: Split) -> impl Iterator<Item = &DatasetCase> {
        self.cases.iter().filter(move |c| c.split == split)
    }
}

// ---------------------------------------------------------------------------
// Dataset admission (spec 023, 3.1 and 3.2).
// ---------------------------------------------------------------------------

/// Schema of an owner-reviewed dataset admission record (spec 023, 3.1).
pub const DATASET_ADMISSION: &str = "rustev.dataset-admission/1";

/// The record that admits one dataset manifest as qualification input
/// (spec 023, 3.1). The owner writes and reviews it before any case is
/// evaluated; Rustev checks that it is complete and agrees with the
/// manifest, never that its statements are true. Its digest is the
/// admission identity a qualification and every claim name. It holds no
/// binary64 number: an agreement statistic is a Decimal string. The record
/// and the data stay with the host, outside this repository (spec 023, 6.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmissionRecord {
    pub schema: String,
    /// The admitted manifest's identity.
    pub dataset: DatasetId,
    pub source: AdmissionSource,
    pub license: LicenseTerms,
    pub privacy: PrivacyBasis,
    pub labeling: LabelingMethod,
    /// The models whose quality this dataset may qualify. Label
    /// independence is judged against them (3.1.4).
    pub systems_under_test: Vec<ModelRef>,
    pub task_mapping: TaskMapping,
    pub splits: SplitAssignment,
    pub population: Population,
    pub exposure: Exposure,
    pub review: OwnerReview,
}

eval_document!(AdmissionRecord, DATASET_ADMISSION, REPLAY_V1, ContentDigest);

/// Where the data came from (3.1.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum AdmissionSource {
    /// A public dataset: publisher, exact version or retrieval date, and the
    /// digest of the content admitted.
    Public {
        publisher: String,
        version: String,
        content_digest: ContentDigest,
    },
    /// A commissioned set: who labeled it and under what instruction.
    Commissioned {
        labeled_by: String,
        instruction: String,
        content_digest: ContentDigest,
    },
}

/// The license and what it permits (3.1.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LicenseTerms {
    /// The identity of the license text.
    pub license: String,
    pub may_commit: bool,
    pub may_retain_outside_git: bool,
    pub may_send_to_provider: bool,
    pub may_redistribute_in_report: bool,
}

/// The privacy basis (3.1.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrivacyBasis {
    pub personal_data: bool,
    /// Derived from real user data: refused without the owner's separate
    /// privacy decision (spec 012, 3.1.3).
    pub derived_from_real_user_data: bool,
    /// A reference to that decision.
    pub privacy_decision: Option<String>,
}

/// How labels were produced (3.1.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LabelingMethod {
    pub guidelines: String,
    pub annotators_per_case: u64,
    pub agreement: AgreementStatistic,
    pub adjudication: String,
    /// Everyone and everything that produced a label.
    pub producers: Vec<LabelProducer>,
}

/// An inter-annotator agreement statistic with its denominator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgreementStatistic {
    pub statistic: String,
    pub value: Decimal,
    pub denominator: u64,
}

/// A label producer. A model under test, or a model of the same family, is
/// never an independent labeler.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum LabelProducer {
    Human { description: String },
    Model(ModelRef),
}

/// A model, by family and exact identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelRef {
    pub family: String,
    pub identity: String,
}

impl ModelRef {
    /// The same model, or one of the same family.
    pub fn related(&self, other: &ModelRef) -> bool {
        self.family == other.family || self.identity == other.identity
    }
}

/// How the dataset's labels map to the task adapter's (3.1.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskMapping {
    pub adapter: AdapterRef,
    /// The adapter's rules digest (spec 014): the correctness every
    /// qualified report is computed under.
    pub rules: ContentDigest,
    /// Source label to adapter label.
    pub labels: Vec<LabelMapping>,
    /// Cases whose source label maps to no adapter label: kept in the
    /// manifest with a missing label, counted, never dropped.
    pub unmapped: u64,
    /// Cases the source left unlabeled.
    pub unlabeled: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LabelMapping {
    pub from: String,
    pub to: String,
}

/// How cases were assigned to splits (3.1.6). Disjointness by source and
/// snapshot is the manifest's own check (spec 004, 3.5.1); the counts here
/// must equal the manifest's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SplitAssignment {
    pub method: String,
    pub seed: String,
    pub training: u64,
    pub model_selection: u64,
    pub calibration: u64,
    pub final_test: u64,
    /// The stated limit on near-duplicates the manifest cannot identify.
    pub near_duplicates: String,
}

/// Who and what the data represents (3.1.7). A claim may name this
/// population and no other (3.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Population {
    /// A short, exact name a claim must repeat.
    pub id: String,
    pub description: String,
    pub limitations: String,
    /// Declared slices: manifest subgroups, with their whole-manifest size.
    pub slices: Vec<SliceSize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SliceSize {
    pub name: String,
    pub cases: u64,
}

/// Contamination and exposure (3.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exposure {
    /// Whether a backend's training data may include this dataset: a stated
    /// limit Rustev cannot verify, required for a public dataset.
    pub training_data: String,
    /// Final-test cases whose content reached a provider before admission:
    /// reported separately and excluded from the headline figures.
    pub exposed_final_test: Vec<String>,
    /// For a ranking task, spec 008 4.2's caveat as it applies here.
    pub ranking_caveat: Option<String>,
}

/// The owner's review of this record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerReview {
    pub reviewer: String,
    pub date: String,
}

/// Why a dataset was not admitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdmissionError {
    Schema,
    /// The manifest itself is refused (spec 004, 3.5.1).
    Dataset(DatasetError),
    /// Synthetic or generated data is never independent evidence of
    /// semantic quality (R-04).
    Synthetic,
    /// The record admits another manifest.
    WrongDataset,
    /// A required statement is empty.
    Incomplete(&'static str),
    /// Derived from real user data without the owner's privacy decision.
    PrivacyDecisionMissing,
    /// A label producer is a system under test or of the same family.
    LabelsNotIndependent {
        producer: ModelRef,
    },
    /// The mapping names another adapter than the manifest.
    Adapter,
    /// A source label is mapped twice.
    DuplicateMapping {
        from: String,
    },
    /// Unmapped plus unlabeled cases differ from the manifest's missing
    /// labels: some case was dropped or invented.
    MissingLabels {
        declared: u64,
        manifest: u64,
    },
    /// A split count differs from the manifest's.
    SplitCount {
        split: &'static str,
    },
    /// A slice is declared twice, is not a manifest subgroup, or its size
    /// differs from the manifest's.
    Slice {
        name: String,
    },
    /// An exposed case is not a final-test case of the manifest, or is
    /// listed twice.
    Exposure {
        case: String,
    },
}

impl fmt::Display for AdmissionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for AdmissionError {}

/// A manifest with a checked admission record. Only [`admit`] makes one, so
/// holding it is the statement that the checks of 3.1 ran.
#[derive(Debug, Clone)]
pub struct Admitted {
    record: AdmissionRecord,
    id: ContentDigest,
}

impl Admitted {
    pub fn record(&self) -> &AdmissionRecord {
        &self.record
    }
    /// The admission record's digest.
    pub fn id(&self) -> &ContentDigest {
        &self.id
    }
    pub fn dataset(&self) -> &DatasetId {
        &self.record.dataset
    }
    /// A model label producer that is `model` or of its family, if any:
    /// `model`'s outputs would then not meet independent labels.
    pub fn related_labeler(&self, model: &ModelRef) -> Option<&ModelRef> {
        self.record.labeling.producers.iter().find_map(|p| match p {
            LabelProducer::Model(m) if m.related(model) => Some(m),
            _ => None,
        })
    }
    /// Whether the admission judged independence against this model.
    pub fn declares_system(&self, model: &ModelRef) -> bool {
        self.record.systems_under_test.contains(model)
    }
}

/// Admit a manifest under an owner-reviewed record (spec 023, 3.1, 3.2).
/// The record must be complete and agree with the manifest; labels must be
/// independent of every system under test; data derived from real user data
/// needs the owner's privacy decision. A synthetic manifest is never
/// admitted.
pub fn admit(
    manifest: &DatasetManifest,
    record: &AdmissionRecord,
) -> Result<Admitted, AdmissionError> {
    use AdmissionError as E;
    if record.schema != DATASET_ADMISSION {
        return Err(E::Schema);
    }
    manifest.check_structure().map_err(E::Dataset)?;
    if matches!(manifest.provenance, DatasetProvenance::Synthetic { .. }) {
        return Err(E::Synthetic);
    }
    if manifest.id().ok().as_ref() != Some(&record.dataset) {
        return Err(E::WrongDataset);
    }
    let blank = |s: &str| s.trim().is_empty();
    let need = |ok: bool, what: &'static str| {
        if ok { Ok(()) } else { Err(E::Incomplete(what)) }
    };
    match &record.source {
        AdmissionSource::Public {
            publisher, version, ..
        } => {
            need(!blank(publisher) && !blank(version), "source")?;
            need(
                !blank(&record.exposure.training_data),
                "training-data exposure",
            )?;
        }
        AdmissionSource::Commissioned {
            labeled_by,
            instruction,
            ..
        } => need(!blank(labeled_by) && !blank(instruction), "source")?,
    }
    need(!blank(&record.license.license), "license")?;
    if record.privacy.derived_from_real_user_data
        && record.privacy.privacy_decision.as_deref().is_none_or(blank)
    {
        return Err(E::PrivacyDecisionMissing);
    }
    let l = &record.labeling;
    need(
        !blank(&l.guidelines)
            && !blank(&l.adjudication)
            && l.annotators_per_case > 0
            && !l.producers.is_empty(),
        "labeling method",
    )?;
    need(
        !blank(&l.agreement.statistic)
            && l.agreement.denominator > 0
            && !l.agreement.value.is_negative(),
        "agreement statistic",
    )?;
    for p in &l.producers {
        let ok = match p {
            LabelProducer::Human { description } => !blank(description),
            LabelProducer::Model(m) => !blank(&m.family) && !blank(&m.identity),
        };
        need(ok, "label producer")?;
    }
    need(!record.systems_under_test.is_empty(), "systems under test")?;
    for s in &record.systems_under_test {
        need(
            !blank(&s.family) && !blank(&s.identity),
            "systems under test",
        )?;
        for p in &l.producers {
            if let LabelProducer::Model(m) = p
                && m.related(s)
            {
                return Err(E::LabelsNotIndependent {
                    producer: m.clone(),
                });
            }
        }
    }
    let m = &record.task_mapping;
    if m.adapter != manifest.adapter {
        return Err(E::Adapter);
    }
    let mut froms = BTreeSet::new();
    for x in &m.labels {
        need(!blank(&x.from) && !blank(&x.to), "label mapping")?;
        if !froms.insert(&x.from) {
            return Err(E::DuplicateMapping {
                from: x.from.clone(),
            });
        }
    }
    let missing = manifest
        .cases
        .iter()
        .filter(|c| c.label == Label::Missing)
        .count() as u64;
    let declared = m.unmapped.saturating_add(m.unlabeled);
    if declared != missing {
        return Err(E::MissingLabels {
            declared,
            manifest: missing,
        });
    }
    let s = &record.splits;
    need(
        !blank(&s.method) && !blank(&s.seed) && !blank(&s.near_duplicates),
        "split assignment",
    )?;
    for (split, n, name) in [
        (Split::Training, s.training, "training"),
        (Split::ModelSelection, s.model_selection, "model_selection"),
        (Split::Calibration, s.calibration, "calibration"),
        (Split::FinalTest, s.final_test, "final_test"),
    ] {
        if manifest.split(split).count() as u64 != n {
            return Err(E::SplitCount { split: name });
        }
    }
    let p = &record.population;
    need(
        !blank(&p.id) && !blank(&p.description) && !blank(&p.limitations),
        "population",
    )?;
    let mut names = BTreeSet::new();
    for slice in &p.slices {
        let size = manifest
            .cases
            .iter()
            .filter(|c| c.subgroups.contains(&slice.name))
            .count() as u64;
        if !names.insert(&slice.name) || size == 0 || size != slice.cases {
            return Err(E::Slice {
                name: slice.name.clone(),
            });
        }
    }
    let mut exposed = BTreeSet::new();
    for case in &record.exposure.exposed_final_test {
        let ok = manifest.split(Split::FinalTest).any(|c| &c.id == case);
        if !ok || !exposed.insert(case) {
            return Err(E::Exposure { case: case.clone() });
        }
    }
    if record.exposure.ranking_caveat.as_deref().is_some_and(blank) {
        return Err(E::Incomplete("ranking caveat"));
    }
    need(
        !blank(&record.review.reviewer) && !blank(&record.review.date),
        "owner review",
    )?;
    let id = record.id().map_err(|_| E::Incomplete("canonical form"))?;
    Ok(Admitted {
        record: record.clone(),
        id,
    })
}
