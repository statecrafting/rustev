//! The rules program, `rustev.rules/1` (spec 005, 3.2 and 3.3): the
//! document a host supplies, its validation and the checked form the
//! evaluator runs.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use rustev_contract::canonical::{CanonicalError, tagged_digest};
use rustev_contract::decimal::Decimal;
use rustev_contract::definition::Operation;
use rustev_contract::ids::ArtifactId;
use rustev_contract::limits::DESCRIPTOR_V1;
use rustev_contract::{Document, DocumentError};
use serde::{Deserialize, Serialize};

/// The schema string of a rules program.
pub const SCHEMA: &str = "rustev.rules/1";

/// The limits of schema version 1 (spec 005, 3.3).
pub mod limits {
    pub const BACKEND_ID_BYTES: usize = 64;
    pub const MAX_PROJECTION_BYTES: u64 = 65_536;
    pub const TASKS: usize = 32;
    pub const TASK_BYTES: usize = 256;
    pub const QUESTION_BYTES: usize = 4_096;
    pub const OPTIONS_MIN: usize = 2;
    pub const OPTIONS: usize = 64;
    pub const OPTION_BYTES: usize = 256;
    pub const INTERPRETATION_BYTES: usize = 1_024;
    pub const FIELDS: usize = 16;
    pub const NAME_BYTES: usize = 64;
    pub const RULES: usize = 128;
    pub const CONDITION_NODES: usize = 32;
    pub const CONDITION_DEPTH: usize = 4;
    pub const CONDITION_MEMBERS: usize = 16;
    pub const TERMS: usize = 32;
    pub const TERM_BYTES: usize = 64;
    pub const TERMS_PER_TASK: usize = 1_024;
    pub const CONTRIBUTIONS: usize = 8;
    pub const LOOKUP_ENTRIES: usize = 256;
    pub const LOOKUP_KEY_BYTES: usize = 256;
    pub const LOOKUP_ENTRIES_PER_TASK: usize = 1_024;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RulesProgram {
    pub schema: String,
    pub backend_id: String,
    pub limits: ProgramLimits,
    pub cost: ProgramCost,
    pub tasks: Vec<Task>,
}

impl Document for RulesProgram {
    const SCHEMA: &'static str = SCHEMA;
    const LIMITS: rustev_contract::bounded::ParseLimits = DESCRIPTOR_V1;
    fn schema(&self) -> &str {
        &self.schema
    }
}

impl RulesProgram {
    /// The program's identity, which is the artifact identity of the backend
    /// it configures: the contract's tagged digest of its canonical bytes
    /// (spec 005, 3.10).
    pub fn artifact_id(&self) -> Result<ArtifactId, CanonicalError> {
        let digest = tagged_digest(SCHEMA, &self.canonical()?);
        ArtifactId::parse(&digest).map_err(|e| CanonicalError::Serialize {
            message: e.to_string(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramLimits {
    /// Largest projection accepted, in bytes; larger ones are refused.
    pub max_projection_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramCost {
    /// Metered logical units charged per call (spec 005, 3.12). Not money.
    pub units_per_call: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    pub operation: Operation,
    pub task: String,
    pub question: String,
    pub options: Vec<String>,
    /// What the logits mean, authored (spec 005, 3.8).
    pub interpretation: String,
    pub fields: Vec<Field>,
    pub base: BTreeMap<String, Decimal>,
    pub rules: Vec<Rule>,
    pub on_no_match: NoMatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    pub name: String,
    #[serde(rename = "ref")]
    pub reference: String,
    #[serde(rename = "type")]
    pub ty: FieldType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    Text,
    Bool,
    Integer,
    Decimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoMatch {
    Base,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub name: String,
    pub when: Condition,
    pub then: Vec<Contribution>,
    pub stop: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Condition {
    Always,
    ContainsAny {
        field: String,
        terms: Vec<String>,
    },
    Equals {
        field: String,
        value: String,
    },
    Compare {
        field: String,
        op: CompareOp,
        value: Decimal,
    },
    All(Vec<Condition>),
    Any(Vec<Condition>),
    Not(Box<Condition>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompareOp {
    Lt,
    Le,
    Ge,
    Gt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Contribution {
    Add(BTreeMap<String, Decimal>),
    Linear {
        field: String,
        coefficients: BTreeMap<String, Decimal>,
    },
    Lookup {
        field: String,
        entries: Vec<LookupEntry>,
        on_missing: OnMissing,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LookupEntry {
    pub key: String,
    pub add: BTreeMap<String, Decimal>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnMissing {
    Skip,
    Fail,
}

/// Why a program was refused (spec 005, 3.3). `at` locates the item, for
/// example `tasks[0].rules[3].then[1]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramError {
    pub at: String,
    pub kind: ProgramErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgramErrorKind {
    /// The bytes are not a `rustev.rules/1` document.
    Document(DocumentError),
    /// The program has no canonical form.
    Canonical(CanonicalError),
    /// A count or length outside its bound.
    Bound {
        what: &'static str,
        min: usize,
        max: usize,
        found: usize,
    },
    BackendId,
    /// `rank`, which this backend does not answer.
    UnsupportedOperation,
    /// A proposition's options are not `["false", "true"]`.
    PropositionOptions,
    Duplicate {
        what: &'static str,
        name: String,
    },
    UnknownOption(String),
    UnknownField(String),
    /// A field `ref` is not `input:<name>` or `bind:<name>`, with at most one
    /// `/<member>`.
    Reference(String),
    /// A field of a type the condition or contribution does not accept.
    FieldType {
        field: String,
        found: FieldType,
    },
    /// An `equals` value that is not the canonical text of its field type.
    EqualsValue(String),
    /// Blank interpretation.
    Interpretation,
}

impl fmt::Display for ProgramError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {:?}", self.at, self.kind)
    }
}

impl std::error::Error for ProgramError {}

fn err(at: impl Into<String>, kind: ProgramErrorKind) -> ProgramError {
    ProgramError {
        at: at.into(),
        kind,
    }
}

fn bound(
    at: &str,
    what: &'static str,
    found: usize,
    min: usize,
    max: usize,
) -> Result<(), ProgramError> {
    if found < min || found > max {
        return Err(err(
            at,
            ProgramErrorKind::Bound {
                what,
                min,
                max,
                found,
            },
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The checked form.

/// A validated task, with names resolved to indices.
#[derive(Debug, Clone)]
pub(crate) struct CTask {
    pub operation: Operation,
    pub name: String,
    pub question: String,
    pub options: Vec<String>,
    pub fields: Vec<CField>,
    pub base: Vec<Decimal>,
    pub rules: Vec<CRule>,
    pub on_no_match: NoMatch,
}

#[derive(Debug, Clone)]
pub(crate) struct CField {
    pub name: String,
    /// The projection `values` key (`input:<name>` or `bind:<name>`).
    pub key: String,
    pub member: Option<String>,
    pub ty: FieldType,
}

#[derive(Debug, Clone)]
pub(crate) struct CRule {
    pub name: String,
    pub when: CCond,
    pub then: Vec<CContrib>,
    pub stop: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum CCond {
    Always,
    /// Terms are already ASCII lower-cased.
    ContainsAny {
        field: usize,
        terms: Vec<String>,
    },
    Equals {
        field: usize,
        value: String,
    },
    Compare {
        field: usize,
        op: CompareOp,
        value: Decimal,
    },
    All(Vec<CCond>),
    Any(Vec<CCond>),
    Not(Box<CCond>),
}

/// Option-indexed constants, in option-name order.
pub(crate) type Adds = Vec<(usize, Decimal)>;

#[derive(Debug, Clone)]
pub(crate) enum CContrib {
    Add(Adds),
    Linear {
        field: usize,
        coefficients: Adds,
    },
    Lookup {
        field: usize,
        table: BTreeMap<String, Adds>,
        on_missing: OnMissing,
    },
}

/// Validate a program into its checked form (spec 005, 3.3).
pub(crate) fn check(p: &RulesProgram) -> Result<Vec<CTask>, ProgramError> {
    if p.schema != SCHEMA {
        return Err(err(
            "schema",
            ProgramErrorKind::Document(DocumentError::Schema {
                expected: SCHEMA,
                found: p.schema.chars().take(80).collect(),
            }),
        ));
    }
    let id_ok = !p.backend_id.is_empty()
        && p.backend_id.len() <= limits::BACKEND_ID_BYTES
        && p.backend_id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b));
    if !id_ok {
        return Err(err("backend_id", ProgramErrorKind::BackendId));
    }
    let max = p.limits.max_projection_bytes;
    if max == 0 || max > limits::MAX_PROJECTION_BYTES {
        return Err(err(
            "limits.max_projection_bytes",
            ProgramErrorKind::Bound {
                what: "max_projection_bytes",
                min: 1,
                max: limits::MAX_PROJECTION_BYTES as usize,
                found: usize::try_from(max).unwrap_or(usize::MAX),
            },
        ));
    }
    bound("tasks", "tasks", p.tasks.len(), 1, limits::TASKS)?;
    let mut names = BTreeSet::new();
    let mut out = Vec::with_capacity(p.tasks.len());
    for (i, t) in p.tasks.iter().enumerate() {
        let at = format!("tasks[{i}]");
        if !names.insert(t.task.as_str()) {
            return Err(err(
                at,
                ProgramErrorKind::Duplicate {
                    what: "task",
                    name: t.task.clone(),
                },
            ));
        }
        out.push(check_task(t, &at)?);
    }
    Ok(out)
}

fn check_task(t: &Task, at: &str) -> Result<CTask, ProgramError> {
    if t.operation == Operation::Rank {
        return Err(err(
            format!("{at}.operation"),
            ProgramErrorKind::UnsupportedOperation,
        ));
    }
    bound(at, "task bytes", t.task.len(), 1, limits::TASK_BYTES)?;
    bound(
        at,
        "question bytes",
        t.question.len(),
        1,
        limits::QUESTION_BYTES,
    )?;
    bound(
        &format!("{at}.options"),
        "options",
        t.options.len(),
        limits::OPTIONS_MIN,
        limits::OPTIONS,
    )?;
    let mut options = BTreeMap::new();
    for (k, o) in t.options.iter().enumerate() {
        bound(
            &format!("{at}.options[{k}]"),
            "option bytes",
            o.len(),
            1,
            limits::OPTION_BYTES,
        )?;
        if options.insert(o.clone(), k).is_some() {
            return Err(err(
                format!("{at}.options"),
                ProgramErrorKind::Duplicate {
                    what: "option",
                    name: o.clone(),
                },
            ));
        }
    }
    if t.operation == Operation::Proposition && t.options != ["false", "true"] {
        return Err(err(
            format!("{at}.options"),
            ProgramErrorKind::PropositionOptions,
        ));
    }
    bound(
        &format!("{at}.interpretation"),
        "interpretation bytes",
        t.interpretation.len(),
        1,
        limits::INTERPRETATION_BYTES,
    )?;
    if t.interpretation.trim().is_empty() {
        return Err(err(
            format!("{at}.interpretation"),
            ProgramErrorKind::Interpretation,
        ));
    }
    bound(
        &format!("{at}.fields"),
        "fields",
        t.fields.len(),
        0,
        limits::FIELDS,
    )?;
    let mut fields = Vec::with_capacity(t.fields.len());
    let mut field_index = BTreeMap::new();
    for (k, f) in t.fields.iter().enumerate() {
        let fat = format!("{at}.fields[{k}]");
        bound(
            &fat,
            "field name bytes",
            f.name.len(),
            1,
            limits::NAME_BYTES,
        )?;
        if field_index.insert(f.name.clone(), k).is_some() {
            return Err(err(
                fat,
                ProgramErrorKind::Duplicate {
                    what: "field",
                    name: f.name.clone(),
                },
            ));
        }
        let (key, member) = parse_ref(&f.reference)
            .ok_or_else(|| err(&fat, ProgramErrorKind::Reference(f.reference.clone())))?;
        fields.push(CField {
            name: f.name.clone(),
            key,
            member,
            ty: f.ty,
        });
    }
    let base = {
        let mut v = vec![Decimal::from_i64(0); t.options.len()];
        for (k, d) in adds(&t.base, &options, &format!("{at}.base"), true)? {
            v[k] = d;
        }
        v
    };
    bound(
        &format!("{at}.rules"),
        "rules",
        t.rules.len(),
        0,
        limits::RULES,
    )?;
    let mut cx = TaskCx {
        options: &options,
        fields: &fields,
        field_index: &field_index,
        terms: 0,
        entries: 0,
    };
    let mut rule_names = BTreeSet::new();
    let mut rules = Vec::with_capacity(t.rules.len());
    for (k, r) in t.rules.iter().enumerate() {
        let rat = format!("{at}.rules[{k}]");
        bound(&rat, "rule name bytes", r.name.len(), 1, limits::NAME_BYTES)?;
        if !rule_names.insert(r.name.as_str()) {
            return Err(err(
                rat,
                ProgramErrorKind::Duplicate {
                    what: "rule",
                    name: r.name.clone(),
                },
            ));
        }
        let mut nodes = 0;
        let when = cx.condition(&r.when, &format!("{rat}.when"), 0, &mut nodes)?;
        bound(
            &format!("{rat}.when"),
            "condition nodes",
            nodes,
            1,
            limits::CONDITION_NODES,
        )?;
        bound(
            &format!("{rat}.then"),
            "contributions",
            r.then.len(),
            0,
            limits::CONTRIBUTIONS,
        )?;
        let mut then = Vec::with_capacity(r.then.len());
        for (c, contribution) in r.then.iter().enumerate() {
            then.push(cx.contribution(contribution, &format!("{rat}.then[{c}]"))?);
        }
        rules.push(CRule {
            name: r.name.clone(),
            when,
            then,
            stop: r.stop,
        });
    }
    bound(
        at,
        "contains_any terms per task",
        cx.terms,
        0,
        limits::TERMS_PER_TASK,
    )?;
    bound(
        at,
        "lookup entries per task",
        cx.entries,
        0,
        limits::LOOKUP_ENTRIES_PER_TASK,
    )?;
    Ok(CTask {
        operation: t.operation,
        name: t.task.clone(),
        question: t.question.clone(),
        options: t.options.clone(),
        fields,
        base,
        rules,
        on_no_match: t.on_no_match,
    })
}

/// `input:<name>` or `bind:<name>`, optionally `/<member>`.
fn parse_ref(r: &str) -> Option<(String, Option<String>)> {
    let (key, member) = match r.split_once('/') {
        Some((k, m)) => (k, Some(m)),
        None => (r, None),
    };
    let name = key
        .strip_prefix("input:")
        .or_else(|| key.strip_prefix("bind:"))?;
    let simple = |s: &str| !s.is_empty() && !s.contains('/') && !s.contains(':');
    if !simple(name) || member.is_some_and(|m| !simple(m)) {
        return None;
    }
    Some((key.to_string(), member.map(str::to_string)))
}

fn adds(
    m: &BTreeMap<String, Decimal>,
    options: &BTreeMap<String, usize>,
    at: &str,
    may_be_empty: bool,
) -> Result<Adds, ProgramError> {
    if m.is_empty() && !may_be_empty {
        return Err(err(
            at,
            ProgramErrorKind::Bound {
                what: "option constants",
                min: 1,
                max: limits::OPTIONS,
                found: 0,
            },
        ));
    }
    m.iter()
        .map(|(o, d)| match options.get(o) {
            Some(k) => Ok((*k, *d)),
            None => Err(err(at, ProgramErrorKind::UnknownOption(o.clone()))),
        })
        .collect()
}

struct TaskCx<'a> {
    options: &'a BTreeMap<String, usize>,
    fields: &'a [CField],
    field_index: &'a BTreeMap<String, usize>,
    terms: usize,
    entries: usize,
}

impl TaskCx<'_> {
    fn field(&self, name: &str, at: &str, accepts: &[FieldType]) -> Result<usize, ProgramError> {
        let k = *self
            .field_index
            .get(name)
            .ok_or_else(|| err(at, ProgramErrorKind::UnknownField(name.to_string())))?;
        let found = self.fields[k].ty;
        if !accepts.contains(&found) {
            return Err(err(
                at,
                ProgramErrorKind::FieldType {
                    field: name.to_string(),
                    found,
                },
            ));
        }
        Ok(k)
    }

    fn condition(
        &mut self,
        c: &Condition,
        at: &str,
        depth: usize,
        nodes: &mut usize,
    ) -> Result<CCond, ProgramError> {
        *nodes += 1;
        if *nodes > limits::CONDITION_NODES {
            return Err(err(
                at,
                ProgramErrorKind::Bound {
                    what: "condition nodes",
                    min: 1,
                    max: limits::CONDITION_NODES,
                    found: *nodes,
                },
            ));
        }
        Ok(match c {
            Condition::Always => CCond::Always,
            Condition::ContainsAny { field, terms } => {
                let field = self.field(field, at, &[FieldType::Text])?;
                bound(at, "terms", terms.len(), 1, limits::TERMS)?;
                for t in terms {
                    bound(at, "term bytes", t.len(), 1, limits::TERM_BYTES)?;
                }
                self.terms += terms.len();
                CCond::ContainsAny {
                    field,
                    terms: terms.iter().map(|t| t.to_ascii_lowercase()).collect(),
                }
            }
            Condition::Equals { field, value } => {
                let k = self.field(
                    field,
                    at,
                    &[FieldType::Text, FieldType::Bool, FieldType::Integer],
                )?;
                if !canonical_for(self.fields[k].ty, value) {
                    return Err(err(at, ProgramErrorKind::EqualsValue(value.clone())));
                }
                CCond::Equals {
                    field: k,
                    value: value.clone(),
                }
            }
            Condition::Compare { field, op, value } => CCond::Compare {
                field: self.field(field, at, &[FieldType::Integer, FieldType::Decimal])?,
                op: *op,
                value: *value,
            },
            Condition::All(members) | Condition::Any(members) => {
                if depth + 1 > limits::CONDITION_DEPTH {
                    return Err(err(
                        at,
                        ProgramErrorKind::Bound {
                            what: "condition depth",
                            min: 0,
                            max: limits::CONDITION_DEPTH,
                            found: depth + 1,
                        },
                    ));
                }
                bound(
                    at,
                    "condition members",
                    members.len(),
                    1,
                    limits::CONDITION_MEMBERS,
                )?;
                let mut v = Vec::with_capacity(members.len());
                for (k, m) in members.iter().enumerate() {
                    v.push(self.condition(m, &format!("{at}[{k}]"), depth + 1, nodes)?);
                }
                if matches!(c, Condition::All(_)) {
                    CCond::All(v)
                } else {
                    CCond::Any(v)
                }
            }
            Condition::Not(inner) => {
                if depth + 1 > limits::CONDITION_DEPTH {
                    return Err(err(
                        at,
                        ProgramErrorKind::Bound {
                            what: "condition depth",
                            min: 0,
                            max: limits::CONDITION_DEPTH,
                            found: depth + 1,
                        },
                    ));
                }
                CCond::Not(Box::new(self.condition(
                    inner,
                    &format!("{at}.not"),
                    depth + 1,
                    nodes,
                )?))
            }
        })
    }

    fn contribution(&mut self, c: &Contribution, at: &str) -> Result<CContrib, ProgramError> {
        Ok(match c {
            Contribution::Add(m) => CContrib::Add(adds(m, self.options, at, false)?),
            Contribution::Linear {
                field,
                coefficients,
            } => CContrib::Linear {
                field: self.field(field, at, &[FieldType::Integer, FieldType::Decimal])?,
                coefficients: adds(coefficients, self.options, at, false)?,
            },
            Contribution::Lookup {
                field,
                entries,
                on_missing,
            } => {
                let k = self.field(
                    field,
                    at,
                    &[FieldType::Text, FieldType::Bool, FieldType::Integer],
                )?;
                bound(
                    at,
                    "lookup entries",
                    entries.len(),
                    1,
                    limits::LOOKUP_ENTRIES,
                )?;
                self.entries += entries.len();
                let mut table = BTreeMap::new();
                for (e, entry) in entries.iter().enumerate() {
                    let eat = format!("{at}.entries[{e}]");
                    bound(
                        &eat,
                        "lookup key bytes",
                        entry.key.len(),
                        1,
                        limits::LOOKUP_KEY_BYTES,
                    )?;
                    let a = adds(&entry.add, self.options, &eat, false)?;
                    // A duplicate key is refused, never resolved by position.
                    if table.insert(entry.key.clone(), a).is_some() {
                        return Err(err(
                            eat,
                            ProgramErrorKind::Duplicate {
                                what: "lookup key",
                                name: entry.key.clone(),
                            },
                        ));
                    }
                }
                CContrib::Lookup {
                    field: k,
                    table,
                    on_missing: *on_missing,
                }
            }
        })
    }
}

/// Whether `v` is the canonical text of some value of type `ty` (3.4.3).
fn canonical_for(ty: FieldType, v: &str) -> bool {
    match ty {
        FieldType::Text => true,
        FieldType::Bool => v == "true" || v == "false",
        FieldType::Integer => v.parse::<i64>().is_ok_and(|i| i.to_string() == v),
        FieldType::Decimal => false,
    }
}
