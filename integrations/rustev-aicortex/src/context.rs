//! The context source (spec 010 3.5.1 to 3.5.3; binding record items 3 to
//! 6): a declared, bounded projection of Aicortex claims into a Rustev
//! snapshot.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use aicortex_claims::{
    AsOf, ClaimHistory, ProjectionCandidate, ProjectionPolicy, SlotState, SlotView, TxBound,
    ValidBound, project,
};
use aicortex_store::{ClaimRepo, MAX_CLAIM_PAGE_ROWS};
use aicortex_types::{ClaimValue, PredicateRef, Scope, SlotKey, SubjectRef};
use rahi_store::StoreHandle;
use rahi_types::UnixSeconds;
use rustev_contract::definition::ProvenanceClass;
use rustev_contract::schema::SNAPSHOT;
use rustev_contract::snapshot::{Entry, Snapshot};
use rustev_contract::time::Timestamp;
use rustev_core::seams::{BoxFuture, CallContext, ContextSource};
use serde_json::Value;

/// The most field bindings one context source declares.
pub const MAX_BINDINGS: usize = 64;

/// The Aicortex value kinds this adapter maps, each into one Rustev input
/// type: `text` to text, `integer` to integer, `enum` to an enum label.
/// Every other Aicortex kind is refused for the field (binding record item
/// 5), never converted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundKind {
    Text,
    Integer,
    Enum,
}

/// One declared field: which Rustev input it supplies and which Aicortex
/// slot (subject, versioned predicate, slot key) it reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldBinding {
    /// The Rustev input name.
    pub field: String,
    pub subject: SubjectRef,
    pub predicate: PredicateRef,
    /// The slot, for a predicate with cardinality `many`.
    pub slot: Option<SlotKey>,
    pub kind: BoundKind,
}

/// The read configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextConfig {
    /// 1 to [`MAX_BINDINGS`] bindings with distinct field names.
    pub bindings: Vec<FieldBinding>,
    /// The history page read per subject, 1 to Aicortex's
    /// `MAX_CLAIM_PAGE_ROWS` (500). A subject whose history fills it is
    /// refused, never read as complete.
    pub page_rows: u32,
}

impl ContextConfig {
    /// # Errors
    ///
    /// [`ConfigError`] for an empty, oversized or duplicated binding set,
    /// or an out-of-range page bound.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.bindings.is_empty() {
            return Err(ConfigError::NoBindings);
        }
        if self.bindings.len() > MAX_BINDINGS {
            return Err(ConfigError::TooManyBindings(self.bindings.len()));
        }
        let mut names = BTreeSet::new();
        for b in &self.bindings {
            if b.field.is_empty() {
                return Err(ConfigError::EmptyField);
            }
            if !names.insert(b.field.as_str()) {
                return Err(ConfigError::DuplicateField(b.field.clone()));
            }
        }
        if !(1..=MAX_CLAIM_PAGE_ROWS).contains(&self.page_rows) {
            return Err(ConfigError::PageRows(self.page_rows));
        }
        Ok(())
    }
}

/// The host's resolution of Rustev's opaque principal handle into the
/// Aicortex scope it authorizes, or `None` to refuse the read. The adapter
/// never builds a scope from anything else (3.1.2).
pub trait ScopeResolver: Send + Sync {
    fn scope(&self, principal_handle: &[u8]) -> Option<Scope>;
}

/// The host's wall clock, the read's transaction and valid-time bound;
/// `None` refuses the read.
pub type HostClock = Arc<dyn Fn() -> Option<UnixSeconds> + Send + Sync>;

/// Why a configuration is refused at construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    NoBindings,
    TooManyBindings(usize),
    EmptyField,
    DuplicateField(String),
    PageRows(u32),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoBindings => f.write_str("no field binding is declared"),
            Self::TooManyBindings(n) => write!(f, "{n} bindings, above {MAX_BINDINGS}"),
            Self::EmptyField => f.write_str("a binding has an empty field name"),
            Self::DuplicateField(n) => write!(f, "the field {n:?} is bound twice"),
            Self::PageRows(n) => write!(f, "page_rows {n} is outside 1 to {MAX_CLAIM_PAGE_ROWS}"),
        }
    }
}

impl std::error::Error for ConfigError {}

/// Why a read produced no snapshot at all. No decision runs on part of one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    /// The host's resolver refused the principal handle.
    ScopeRefused,
    /// The host clock gave no time, or one Aicortex cannot represent.
    Clock,
    /// The store failed (any `rahi_types::Error`).
    Store(String),
    /// A subject's history filled the page bound.
    Truncated { subject: String },
    /// Aicortex's projection refused (`ProjectionError`).
    Projection(String),
    /// The call's deadline passed.
    Deadline,
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ScopeRefused => f.write_str("aicortex: the host refused the principal's scope"),
            Self::Clock => f.write_str("aicortex: the host clock gave no usable time"),
            Self::Store(d) => write!(f, "aicortex: store: {d}"),
            Self::Truncated { subject } => write!(
                f,
                "aicortex: the history of {subject} fills the page bound; refused, not read as complete"
            ),
            Self::Projection(d) => write!(f, "aicortex: projection: {d}"),
            Self::Deadline => f.write_str("aicortex: the deadline passed"),
        }
    }
}

impl std::error::Error for ReadError {}

/// Why a bound field is missing from the snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Missing {
    /// No claim covers the evaluation time.
    Absent,
    /// Every claim was retracted.
    Retracted,
    /// The value is of another Aicortex kind than the binding declares.
    KindMismatch(String),
    /// Every candidate is classified hostile content.
    HostileContent,
    /// Every candidate cites an erased source.
    OriginErased,
    /// A projected claim's revision (transaction sequence and time) is not
    /// in the page read, or its time is outside Rustev's range: the value
    /// cannot be attributed, so it is not supplied (section 5).
    Unidentifiable,
}

/// What a supplied entry rests on: the claim's identity, its revision (the
/// transaction sequence and time) and its attribution, carried as data.
#[derive(Debug, Clone, PartialEq)]
pub struct Attribution {
    pub claim_id: String,
    /// Aicortex's transaction sequence of the claim: its revision.
    pub tx_seq: u64,
    pub recorded_at: UnixSeconds,
    /// Aicortex's authority level and sourcing, by name; never authority
    /// here.
    pub authority: String,
    pub sourcing: String,
    /// The producer: the claim's source reference as Aicortex renders it.
    pub source: String,
    pub value: Value,
}

/// One requested field's outcome.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldOutcome {
    /// One entry, or one per distinct value when Aicortex reports a
    /// conflict (a Rustev conflict, E-07).
    Supplied(Vec<Attribution>),
    Missing(Missing),
    /// The field is not one this source declares.
    NotBound,
}

/// A read: the snapshot, and per requested field what it rests on or why
/// it is missing.
#[derive(Debug, Clone, PartialEq)]
pub struct ContextRead {
    pub snapshot: Snapshot,
    pub fields: BTreeMap<String, FieldOutcome>,
    /// The transaction and valid-time bound the read used.
    pub as_of: UnixSeconds,
    /// Aicortex's projection policy digest.
    pub policy_digest: String,
}

/// The context source over the host's store.
pub struct AicortexContext {
    store: StoreHandle,
    config: ContextConfig,
    scopes: Arc<dyn ScopeResolver>,
    clock: HostClock,
}

impl AicortexContext {
    /// # Errors
    ///
    /// [`ConfigError`] for an empty, oversized or duplicated binding set,
    /// or an out-of-range page bound.
    pub fn new(
        store: StoreHandle,
        config: ContextConfig,
        scopes: Arc<dyn ScopeResolver>,
        clock: HostClock,
    ) -> Result<Self, ConfigError> {
        config.validate()?;
        Ok(Self {
            store,
            config,
            scopes,
            clock,
        })
    }

    /// Read the requested fields under the principal's scope, bounded at
    /// `now` on both of Aicortex's time axes.
    ///
    /// # Errors
    ///
    /// [`ReadError`]; the read is then refused whole.
    pub async fn read(
        &self,
        fields: &[String],
        principal_handle: &[u8],
        now: UnixSeconds,
    ) -> Result<ContextRead, ReadError> {
        let scope = self
            .scopes
            .scope(principal_handle)
            .ok_or(ReadError::ScopeRefused)?;
        let instant = crate::time::instant(now).ok_or(ReadError::Clock)?;
        let policy = ProjectionPolicy::v1();
        let requested: BTreeSet<&str> = fields.iter().map(String::as_str).collect();
        let wanted: Vec<&FieldBinding> = self
            .config
            .bindings
            .iter()
            .filter(|b| requested.contains(b.field.as_str()))
            .collect();

        let mut outcome: BTreeMap<String, FieldOutcome> = requested
            .iter()
            .map(|f| ((*f).to_string(), FieldOutcome::NotBound))
            .collect();
        let mut entries = Vec::new();
        let mut subjects: Vec<&SubjectRef> = Vec::new();
        for b in &wanted {
            if !subjects.contains(&&b.subject) {
                subjects.push(&b.subject);
            }
        }
        for subject in subjects {
            let page = ClaimRepo::history(
                &self.store,
                &scope,
                subject,
                None,
                &TxBound::RecordedAt(now),
                0,
                self.config.page_rows,
            )
            .await
            .map_err(|e| ReadError::Store(e.to_string()))?;
            if page.next_after.is_some() {
                return Err(ReadError::Truncated {
                    subject: subject_text(subject),
                });
            }
            let view = project(
                &page.history,
                AsOf {
                    knowledge: TxBound::RecordedAt(now),
                    valid: ValidBound::Instant(instant.clone()),
                },
                &policy,
            )
            .map_err(|e| ReadError::Projection(e.to_string()))?;
            for b in wanted.iter().filter(|b| &b.subject == subject) {
                let slot = view.slots.iter().find(|s| same_slot(s, b));
                let field = map_slot(b, slot, &page.history, &mut entries);
                outcome.insert(b.field.clone(), field);
            }
        }
        Ok(ContextRead {
            snapshot: Snapshot {
                schema: SNAPSHOT.to_string(),
                entries,
            },
            fields: outcome,
            as_of: now,
            policy_digest: policy.digest(),
        })
    }
}

fn subject_text(s: &SubjectRef) -> String {
    format!("{}:{}/{}", s.namespace, s.kind, s.key)
}

fn same_slot(view: &SlotView, b: &FieldBinding) -> bool {
    view.slot.subject_namespace == b.subject.namespace.to_string()
        && view.slot.subject_kind == b.subject.kind.to_string()
        && view.slot.subject_key == b.subject.key.to_string()
        && view.slot.predicate == b.predicate
        && view.slot.slot == b.slot
}

fn name_of<T: serde::Serialize>(v: &T) -> String {
    match serde_json::to_value(v) {
        Ok(Value::String(s)) => s,
        Ok(other) => other.to_string(),
        Err(e) => format!("unnamed ({e})"),
    }
}

/// The JSON value for `value` under `kind`, or the Aicortex kind found.
fn mapped(kind: BoundKind, value: &ClaimValue) -> Result<Value, String> {
    match (kind, value) {
        (BoundKind::Text, ClaimValue::Text(s)) => Ok(Value::String(s.clone())),
        (BoundKind::Integer, ClaimValue::Integer(i)) => Ok(Value::from(*i)),
        (BoundKind::Enum, ClaimValue::Enum(v)) => Ok(Value::String(v.to_string())),
        (_, other) => Err(name_of(&other.kind())),
    }
}

fn map_slot(
    b: &FieldBinding,
    slot: Option<&SlotView>,
    history: &ClaimHistory,
    entries: &mut Vec<Entry>,
) -> FieldOutcome {
    let (candidates, conflicted) = match slot.map(|s| &s.state) {
        None | Some(SlotState::Absent) => return FieldOutcome::Missing(Missing::Absent),
        Some(SlotState::Retracted) => return FieldOutcome::Missing(Missing::Retracted),
        Some(SlotState::Value(c)) => (c, false),
        Some(SlotState::Conflicted(c)) => (c, true),
    };
    let usable: Vec<&ProjectionCandidate> = candidates
        .iter()
        .filter(|c| !c.hostile_content && !c.origin_erased)
        .collect();
    if usable.is_empty() {
        return FieldOutcome::Missing(if candidates.iter().any(|c| c.hostile_content) {
            Missing::HostileContent
        } else {
            Missing::OriginErased
        });
    }
    // A value slot is one value, however many equal claims corroborate it;
    // a conflicted slot keeps every distinct value.
    let chosen: Vec<&ProjectionCandidate> = if conflicted {
        let mut seen: Vec<&ClaimValue> = Vec::new();
        usable
            .into_iter()
            .filter(|c| {
                if seen.contains(&&c.value) {
                    false
                } else {
                    seen.push(&c.value);
                    true
                }
            })
            .collect()
    } else {
        usable.into_iter().take(1).collect()
    };
    // Built aside and added only when every chosen candidate maps, so a
    // refused field leaves no entry behind.
    let mut supplied = Vec::new();
    let mut field_entries = Vec::new();
    for c in chosen {
        let value = match mapped(b.kind, &c.value) {
            Ok(v) => v,
            Err(found) => return FieldOutcome::Missing(Missing::KindMismatch(found)),
        };
        let Some(record) = history.claims().get(&c.claim_id) else {
            // A projected claim the page does not hold has no revision here.
            return FieldOutcome::Missing(Missing::Unidentifiable);
        };
        let Some(as_of) = record
            .tx
            .recorded_at
            .get()
            .checked_mul(1_000)
            .and_then(|ms| i64::try_from(ms).ok())
            .and_then(|ms| Timestamp::from_ms(ms).ok())
        else {
            return FieldOutcome::Missing(Missing::Unidentifiable);
        };
        supplied.push(Attribution {
            claim_id: c.claim_id.to_string(),
            tx_seq: record.tx.seq,
            recorded_at: record.tx.recorded_at,
            authority: name_of(&c.authority),
            sourcing: name_of(&c.sourcing),
            source: c.provenance.source.to_string(),
            value: value.clone(),
        });
        field_entries.push(Entry {
            field: b.field.clone(),
            value,
            provenance: ProvenanceClass::AttributedClaim,
            as_of_ms: as_of,
            source: format!("aicortex:claim/{}/tx/{}", c.claim_id, record.tx.seq),
            grade: None,
        });
    }
    entries.extend(field_entries);
    FieldOutcome::Supplied(supplied)
}

impl ContextSource for AicortexContext {
    /// The snapshot of [`AicortexContext::read`] within the call's
    /// remaining deadline; any [`ReadError`] is an error, so no decision
    /// runs on part of a snapshot. Dropping the read on the deadline is
    /// safe: it writes nothing.
    fn fetch<'a>(
        &'a self,
        fields: &'a [String],
        cx: &'a CallContext,
    ) -> BoxFuture<'a, Result<Snapshot, String>> {
        Box::pin(async move {
            let now = (self.clock)().ok_or_else(|| ReadError::Clock.to_string())?;
            let read = self.read(fields, &cx.principal_handle, now);
            match tokio::time::timeout(Duration::from_millis(cx.remaining_ms.0), read).await {
                Ok(Ok(r)) => Ok(r.snapshot),
                Ok(Err(e)) => Err(e.to_string()),
                Err(_) => Err(ReadError::Deadline.to_string()),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use aicortex_types::{Namespace, SubjectKey, SubjectKind};

    use super::*;

    fn binding(field: &str) -> FieldBinding {
        FieldBinding {
            field: field.into(),
            subject: SubjectRef {
                namespace: Namespace::new("demo").unwrap(),
                kind: SubjectKind::new("account").unwrap(),
                key: SubjectKey::new("account-1").unwrap(),
            },
            predicate: PredicateRef::parse("demo:account.note@1").unwrap(),
            slot: None,
            kind: BoundKind::Text,
        }
    }

    #[test]
    fn the_projection_is_declared_bounded_and_unambiguous() {
        let ok = ContextConfig {
            bindings: vec![binding("a"), binding("b")],
            page_rows: 500,
        };
        assert_eq!(ok.validate(), Ok(()));
        let none = ContextConfig {
            bindings: Vec::new(),
            page_rows: 10,
        };
        assert_eq!(none.validate(), Err(ConfigError::NoBindings));
        let many = ContextConfig {
            bindings: (0..=MAX_BINDINGS)
                .map(|i| binding(&format!("f{i}")))
                .collect(),
            page_rows: 10,
        };
        assert_eq!(
            many.validate(),
            Err(ConfigError::TooManyBindings(MAX_BINDINGS + 1))
        );
        let twice = ContextConfig {
            bindings: vec![binding("a"), binding("a")],
            page_rows: 10,
        };
        assert_eq!(
            twice.validate(),
            Err(ConfigError::DuplicateField("a".into()))
        );
        let empty = ContextConfig {
            bindings: vec![binding("")],
            page_rows: 10,
        };
        assert_eq!(empty.validate(), Err(ConfigError::EmptyField));
        for rows in [0, MAX_CLAIM_PAGE_ROWS + 1] {
            let c = ContextConfig {
                bindings: vec![binding("a")],
                page_rows: rows,
            };
            assert_eq!(c.validate(), Err(ConfigError::PageRows(rows)));
        }
    }

    #[test]
    fn values_map_only_into_their_bound_kind() {
        assert_eq!(
            mapped(BoundKind::Text, &ClaimValue::Text("x".into())),
            Ok(Value::from("x"))
        );
        assert_eq!(
            mapped(BoundKind::Integer, &ClaimValue::Integer(-3)),
            Ok(Value::from(-3))
        );
        assert_eq!(
            mapped(BoundKind::Integer, &ClaimValue::Text("3".into())),
            Err("text".into())
        );
        assert_eq!(
            mapped(BoundKind::Text, &ClaimValue::Integer(3)),
            Err("integer".into())
        );
    }
}
