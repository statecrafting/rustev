//! Evidence quality at evaluation (spec 032): value metadata of inputs and
//! steps, the conflict policy, grade requirements, joins, merges and
//! corroboration. Pure: `now` is an input, nothing is read from outside.

use std::collections::BTreeMap;

use rustev_contract::canonical::canonical_value_bytes;
use rustev_contract::definition::{AgeLimit, Disagreement, InputDecl, ProvenanceClass};
use rustev_contract::evidence::{
    Disposition, EntryEvidence, MergeEvidence, MergedField, Resolution, ResolutionRule, UsedEntry,
};
use rustev_contract::grade::{
    AbsentReason, AsOf, Basis, Grade, MergeSide, Presence, ValueGrade, ValueMeta, value_digest,
};
use rustev_contract::judgment::Unresolved;
use rustev_contract::snapshot::Entry;
use rustev_contract::time::Timestamp;
use serde_json::Value as Json;

use crate::ops::{MergeFields, MergeRule};
use crate::quality::{Ceil, Grading, Mode, Read, input_tree};
use crate::value::{Absence, JoinEntry, JoinMatch, Value, decode_v2};

/// The digest of a JSON value, or of nothing when it has no canonical form.
pub(crate) fn digest(j: &Json) -> String {
    value_digest(j).unwrap_or_default()
}

/// A value's metadata: `grade` is the combination of the node and its parts
/// (spec 032, 3.2.2). `node` keeps the node's own grade, for reads that use
/// a list's items' existence only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Meta {
    pub whole: ValueMeta,
    pub node: ValueGrade,
}

fn combine_into(meta: &mut ValueMeta, parts: &[&ValueMeta]) {
    for p in parts {
        meta.grade = meta.grade.combine(p.grade);
    }
}

/// The metadata tree of a decoded input value: every node carries the
/// entry's provenance, source and as-of; its grade is the minimum of its
/// ceiling through the entry's source and the entry's claim, lowered to
/// `hint` under `treat_as_hint`.
#[allow(clippy::too_many_arguments)]
fn input_meta_tree(
    v: &Value,
    tree: &Ceil,
    limit: Option<Grade>,
    graded: bool,
    template: &ValueMeta,
) -> ValueMeta {
    let here = if graded {
        match (tree.here.graded(), limit) {
            (Some(g), Some(l)) => ValueGrade::from(g.min(l)),
            (Some(g), None) => ValueGrade::from(g),
            (None, _) => ValueGrade::Ungraded,
        }
    } else {
        ValueGrade::Ungraded
    };
    let mut m = ValueMeta {
        grade: here,
        ..template.clone()
    };
    let child = |c: Option<&Ceil>, v: &Value| {
        let leaf = Ceil::leaf(tree.here, false);
        input_meta_tree(v, c.unwrap_or(&leaf), limit, graded, template)
    };
    match v {
        Value::Record(fields) => {
            let parts: BTreeMap<String, ValueMeta> = fields
                .iter()
                .map(|(k, fv)| (k.clone(), child(tree.fields.get(k), fv)))
                .collect();
            combine_into(&mut m, &parts.values().collect::<Vec<_>>());
            m.fields = Some(parts);
        }
        Value::List(items) => {
            let parts: Vec<ValueMeta> = items
                .iter()
                .map(|iv| child(tree.item.as_deref(), iv))
                .collect();
            combine_into(&mut m, &parts.iter().collect::<Vec<_>>());
            m.items = Some(parts);
        }
        Value::Map(entries) => {
            let parts: BTreeMap<String, ValueMeta> = entries
                .iter()
                .map(|(k, ev)| (k.clone(), child(tree.item.as_deref(), ev)))
                .collect();
            combine_into(&mut m, &parts.values().collect::<Vec<_>>());
            m.entries = Some(parts);
        }
        _ => {}
    }
    m
}

/// The metadata at `path` (field names, `*` for every item or entry), as a
/// whole value: past the tree, the deepest node.
pub(crate) fn meta_at(m: &ValueMeta, path: &[String]) -> ValueMeta {
    let Some((head, rest)) = path.split_first() else {
        return m.clone();
    };
    if head == "*" {
        let parts: Vec<ValueMeta> = match (&m.items, &m.entries) {
            (Some(items), _) => items.iter().map(|i| meta_at(i, rest)).collect(),
            (None, Some(entries)) => entries.values().map(|e| meta_at(e, rest)).collect(),
            (None, None) => return strip(m),
        };
        if parts.is_empty() {
            return strip(m);
        }
        return ValueMeta::combination(parts.iter(), m.basis.clone());
    }
    match m.fields.as_ref().and_then(|f| f.get(head)) {
        Some(c) => meta_at(c, rest),
        None => strip(m),
    }
}

fn strip(m: &ValueMeta) -> ValueMeta {
    ValueMeta {
        fields: None,
        items: None,
        entries: None,
        ..m.clone()
    }
}

/// What one input's validation produced, beside its value or reason.
#[derive(Debug, Clone, Default)]
pub(crate) struct InputQuality {
    pub meta: Option<Meta>,
    pub presence: Option<Presence>,
    pub entries: Option<Vec<EntryEvidence>>,
    pub resolution: Option<Resolution>,
}

fn rule_of(d: &Disagreement) -> ResolutionRule {
    match d {
        Disagreement::RequireAgreement => ResolutionRule::RequireAgreement,
        Disagreement::Prefer { .. } => ResolutionRule::Prefer,
        Disagreement::TreatAsHint { .. } => ResolutionRule::TreatAsHint,
        Disagreement::Abstain => ResolutionRule::Abstain,
    }
}

/// Validate a plan 3 input (spec 002, 3.7.3, as spec 032 3.3.6 and 3.8
/// amend it for graded inputs): admission by source, the conflict policy,
/// then spec 002's checks on the used entry, presence, and metadata.
pub(crate) fn validate_input(
    name: &str,
    decl: &InputDecl,
    entries: &[&Entry],
    grading: Option<&Grading>,
    now: Timestamp,
) -> (Result<(Value, Derived, Entry), Unresolved>, InputQuality) {
    let field = || vec![name.to_string()];
    let mut q = InputQuality::default();
    let mut records: Vec<EntryEvidence> = Vec::new();
    // Admission (3.3.6): an entry from a source the input does not name is
    // ignored; one whose provenance differs from its source's fact makes the
    // input invalid. Ungraded inputs keep spec 002's provenance filter.
    let mut admitted: Vec<(usize, &Entry)> = Vec::new();
    let mut mismatch = false;
    for e in entries {
        let ceiling = match grading {
            Some(g) => input_tree(&decl.ty, Some(g), Some(&e.source)).own,
            None => ValueGrade::Ungraded,
        };
        let effective = match (ceiling.graded(), e.grade) {
            (Some(c), Some(claim)) => ValueGrade::from(c.min(claim)),
            _ => ceiling,
        };
        let mut rec = EntryEvidence {
            source: e.source.clone(),
            provenance: e.provenance,
            claim: e.grade,
            ceiling,
            effective,
            digest: digest(&e.value),
            as_of_ms: e.as_of_ms,
            disposition: Disposition::Outvoted,
        };
        let disposition = match grading {
            Some(g) => match g.sources.iter().find(|(n, _, _)| *n == e.source) {
                None => Some(Disposition::Ignored {
                    reason: "unbound_source".into(),
                }),
                Some((_, p, _)) if *p != e.provenance => {
                    mismatch = true;
                    Some(Disposition::Rejected {
                        reason: "source-provenance-mismatch".into(),
                    })
                }
                Some(_) => None,
            },
            None if !decl.provenance.contains(&e.provenance) => Some(Disposition::Ignored {
                reason: "provenance_not_accepted".into(),
            }),
            None => None,
        };
        match disposition {
            Some(d) => rec.disposition = d,
            None => admitted.push((records.len(), e)),
        }
        records.push(rec);
    }
    let graded = grading.is_some();
    // Entries are recorded for a graded input, and for an ungraded one only
    // when an entry carries a claim (3.3.5).
    let keep_entries = graded || entries.iter().any(|e| e.grade.is_some());
    let finish = |q: &mut InputQuality, records: Vec<EntryEvidence>| {
        if keep_entries {
            q.entries = Some(records);
        }
    };
    if mismatch {
        finish(&mut q, records);
        return (
            Err(Unresolved::invalid(field(), "source-provenance-mismatch")),
            q,
        );
    }
    // An entry with no canonical form cannot be compared or digested, so
    // it makes the input invalid rather than agreeing with another.
    let uncanonical: Vec<usize> = admitted
        .iter()
        .filter(|(_, e)| canonical_value_bytes(&e.value).is_err())
        .map(|(i, _)| *i)
        .collect();
    if !uncanonical.is_empty() {
        for i in uncanonical {
            records[i].disposition = Disposition::Rejected {
                reason: "value has no canonical form".into(),
            };
        }
        finish(&mut q, records);
        return (
            Err(Unresolved::invalid(field(), "value has no canonical form")),
            q,
        );
    }
    // The conflict policy (3.8).
    let canon = |e: &Entry| canonical_value_bytes(&e.value).ok();
    let disagree = admitted
        .first()
        .is_some_and(|(_, f)| admitted.iter().any(|(_, e)| canon(e) != canon(f)));
    let rule = grading
        .map(|g| g.disagreement.clone())
        .unwrap_or(Disagreement::RequireAgreement);
    let mut hint_only = false;
    let mut resolved = false;
    let chosen: Vec<(usize, &Entry)> = if disagree {
        // Two differing entries from one source: a preference cannot choose
        // within a source (3.8.2).
        let within_source = admitted.iter().any(|(_, a)| {
            admitted
                .iter()
                .any(|(_, b)| a.source == b.source && canon(a) != canon(b))
        });
        let outcome: Result<Vec<(usize, &Entry)>, Unresolved> = match (&rule, within_source) {
            (_, true) | (Disagreement::RequireAgreement, _) => Err(Unresolved::conflict(field())),
            (Disagreement::Abstain, _) => Err(Unresolved::Abstained {
                rule: format!("disagreement:{name}"),
            }),
            (Disagreement::Prefer { sources }, _) | (Disagreement::TreatAsHint { sources }, _) => {
                hint_only = matches!(rule, Disagreement::TreatAsHint { .. });
                let first = sources
                    .iter()
                    .find(|s| admitted.iter().any(|(_, e)| &e.source == *s));
                Ok(admitted
                    .iter()
                    .filter(|(_, e)| first.is_some_and(|s| &e.source == s))
                    .copied()
                    .collect())
            }
        };
        match outcome {
            Ok(c) => {
                resolved = true;
                c
            }
            Err(u) => {
                q.resolution = Some(Resolution {
                    rule: rule_of(&rule),
                    used: UsedEntry::None,
                });
                finish(&mut q, records);
                return (Err(u), q);
            }
        }
    } else {
        admitted.clone()
    };
    // Spec 002's checks on the chosen entries: a future-dated entry, none
    // at all, then the newest decides.
    let future: Vec<usize> = chosen
        .iter()
        .filter(|(_, e)| e.as_of_ms > now)
        .map(|(i, _)| *i)
        .collect();
    if !future.is_empty() {
        for i in future {
            records[i].disposition = Disposition::Rejected {
                reason: "as_of is after the evaluation time".into(),
            };
        }
        finish(&mut q, records);
        return (
            Err(Unresolved::invalid(
                field(),
                "as_of is after the evaluation time",
            )),
            q,
        );
    }
    let Some(&(used_index, used)) = chosen.iter().max_by_key(|(_, e)| e.as_of_ms) else {
        finish(&mut q, records);
        return (Err(Unresolved::missing(field())), q);
    };
    records[used_index].disposition = Disposition::Used;
    if resolved {
        q.resolution = Some(Resolution {
            rule: rule_of(&rule),
            used: UsedEntry::Entry(used_index as u64),
        });
    }
    let value = match decode_v2(&decl.ty, &used.value) {
        Ok(v) => v,
        Err(e) => {
            records[used_index].disposition = Disposition::Rejected { reason: e.clone() };
            finish(&mut q, records);
            return (Err(Unresolved::invalid(field(), e)), q);
        }
    };
    if let rustev_contract::definition::Freshness::MaxAgeMs(max) = decl.freshness {
        let age = now.since(used.as_of_ms).map(|d| d.0).unwrap_or(0);
        if age > max {
            records[used_index].disposition = Disposition::Rejected {
                reason: "stale".into(),
            };
            finish(&mut q, records);
            return (Err(Unresolved::stale(field())), q);
        }
    }
    // Metadata (3.2.2 and 3.3.5).
    let template = ValueMeta {
        grade: ValueGrade::Ungraded,
        provenance: vec![used.provenance],
        sources: vec![used.source.clone()],
        as_of_ms: if graded {
            AsOf::Ms(used.as_of_ms)
        } else {
            AsOf::None
        },
        basis: if resolved {
            Basis::Resolved {
                input: name.to_string(),
            }
        } else {
            Basis::Declared
        },
        fields: None,
        items: None,
        entries: None,
    };
    let tree = input_tree(&decl.ty, grading, Some(&used.source));
    let limit = match (used.grade, hint_only) {
        (_, true) => Some(Grade::Hint),
        (Some(c), false) => Some(c),
        (None, false) => None,
    };
    let whole = input_meta_tree(&value, &tree, limit, graded, &template);
    let node = if graded {
        match (tree.here.graded(), limit) {
            (Some(g), Some(l)) => g.min(l).into(),
            (Some(g), None) => g.into(),
            _ => ValueGrade::Ungraded,
        }
    } else {
        ValueGrade::Ungraded
    };
    if !graded {
        // An ungraded input's claim is recorded and its value is ungraded.
        records[used_index].effective = ValueGrade::Ungraded;
    }
    q.meta = Some(Meta { whole, node });
    finish(&mut q, records);
    // The most derived accepted entry decides, outvoted or not, as for a
    // plan 2 input (spec 002, 3.12.2).
    let derived = if admitted
        .iter()
        .any(|(_, e)| e.provenance == ProvenanceClass::ModelDerived)
    {
        Derived::Model
    } else {
        Derived::Exact
    };
    // A top value that fails its presence predicate is missing (3.4.4).
    if let Value::Absent(a) = &value {
        q.presence = Some(Presence::Absent {
            reason: a.reason,
            predicate: a.predicate.clone(),
        });
        return (Err(Unresolved::missing(field())), q);
    }
    q.presence = Some(Presence::Present);
    (Ok((value, derived, used.clone())), q)
}

/// The derivation class an input's accepted entries give it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Derived {
    Exact,
    Model,
}

/// The metadata a read sees.
pub(crate) fn read_meta(r: &Read, metas: &BTreeMap<String, Meta>) -> ValueMeta {
    let Some(m) = metas.get(&r.base) else {
        return ValueMeta::ungraded(Basis::Computed);
    };
    match r.mode {
        Mode::Whole => meta_at(&m.whole, &r.path),
        Mode::Node if r.path.is_empty() => ValueMeta {
            grade: m.node,
            ..strip(&m.whole)
        },
        Mode::Node => strip(&meta_at(&m.whole, &r.path)),
    }
}

/// The combination of every read's metadata, under `basis`.
pub(crate) fn combine_reads(
    reads: &[Read],
    metas: &BTreeMap<String, Meta>,
    basis: Basis,
) -> ValueMeta {
    let parts: Vec<ValueMeta> = reads.iter().map(|r| read_meta(r, metas)).collect();
    ValueMeta::combination(parts.iter(), basis)
}

/// The requirement a read carries, if any: explicit, or the implicit
/// `estimated` of a hard gate whose read is statically graded (3.9.2).
pub(crate) fn requirement(r: &Read, ceiling: &Ceil) -> Option<(Grade, AgeLimit)> {
    match &r.require {
        Some(req) => Some((req.min, req.max_age_ms)),
        None if r.gate && ceiling.own.graded().is_some() => {
            Some((Grade::Estimated, AgeLimit::NotRequired))
        }
        None => None,
    }
}

/// Judge one read against its requirement (3.9.1): below the minimum (or
/// ungraded) is `insufficient_grade`; a `cached` or `estimated` value older
/// than `max_age_ms` is `stale_evidence` (equal is fresh).
pub(crate) fn check_read(
    r: &Read,
    ceiling: &Ceil,
    meta: &ValueMeta,
    now: Timestamp,
) -> Result<(), Unresolved> {
    let Some((min, age)) = requirement(r, ceiling) else {
        return Ok(());
    };
    let name = r.name();
    let Some(g) = meta.grade.graded() else {
        return Err(Unresolved::insufficient([name]));
    };
    if g < min {
        return Err(Unresolved::insufficient([name]));
    }
    if let (AgeLimit::Ms(max), true, AsOf::Ms(at)) = (
        age,
        matches!(g, Grade::Cached | Grade::Estimated),
        meta.as_of_ms,
    ) {
        let elapsed = now.since(at).map(|d| d.0).unwrap_or(0);
        if elapsed > max {
            return Err(Unresolved::stale([name]));
        }
    }
    Ok(())
}

/// One merged side's record, or nothing (an unmatched or absent overlay, or
/// a base record that fails its presence predicate has no present fields).
type Side<'a> = Option<(&'a std::collections::BTreeMap<String, Value>, &'a ValueMeta)>;

fn present_field<'a>(side: &Side<'a>, f: &str) -> Option<(&'a Value, ValueMeta)> {
    let (rec, meta) = side.as_ref()?;
    match rec.get(f) {
        None | Some(Value::Null) | Some(Value::Absent(_)) => None,
        Some(v) => Some((v, meta_at(meta, &[f.to_string()]))),
    }
}

fn side_of<'a>(v: Option<&'a Value>, m: Option<&'a ValueMeta>) -> Side<'a> {
    match (v, m) {
        (Some(Value::Record(r)), Some(m)) => Some((r, m)),
        _ => None,
    }
}

/// Merge one record (spec 032, 3.6.2 and 3.6.3).
fn merge_one(
    m: &MergeFields,
    step: &str,
    item: &str,
    base: Side<'_>,
    overlay: Side<'_>,
    evidence: &mut Vec<MergeEvidence>,
) -> (Value, ValueMeta) {
    let mut fields = BTreeMap::new();
    let mut metas = BTreeMap::new();
    for r in &m.rules {
        let taken = |side: MergeSide, (v, meta): (&Value, ValueMeta)| {
            let meta = ValueMeta {
                basis: Basis::Merged {
                    step: step.to_string(),
                    side,
                },
                ..meta
            };
            (Ok((v.clone(), side)), meta)
        };
        let absent = |reason: AbsentReason| (Err(reason), ValueMeta::ungraded(Basis::Computed));
        let o = present_field(&overlay, &r.field);
        let b = present_field(&base, &r.field);
        let (result, meta) = match &r.rule {
            MergeRule::OverlayOnly => match o {
                Some(x) => taken(MergeSide::Overlay, x),
                None => absent(AbsentReason::OverlayMissing),
            },
            MergeRule::BaseOnly => match b {
                Some(x) => taken(MergeSide::Base, x),
                None => absent(AbsentReason::BaseMissing),
            },
            MergeRule::OverlayElseBaseIfGradeAtLeast { grade } => match (o, b) {
                (Some(x), _) => taken(MergeSide::Overlay, x),
                (None, Some(x)) if x.1.grade.graded().is_some_and(|g| g >= *grade) => {
                    taken(MergeSide::Base, x)
                }
                (None, Some(_)) => absent(AbsentReason::OverlayMissingBaseBelowGrade),
                (None, None) => absent(AbsentReason::OverlayAndBaseMissing),
            },
        };
        let (value, record) = match result {
            Ok((v, side)) => (
                v,
                MergedField::Taken {
                    side,
                    grade: meta.grade,
                },
            ),
            Err(reason) => (
                Value::Absent(Absence {
                    reason,
                    predicate: None,
                }),
                MergedField::Absent { reason },
            ),
        };
        evidence.push(MergeEvidence {
            item: item.to_string(),
            field: r.field.clone(),
            rule: r.rule.name().into(),
            result: record,
        });
        fields.insert(r.field.clone(), value);
        metas.insert(r.field.clone(), meta);
    }
    let mut meta = ValueMeta::combination(
        metas.values(),
        Basis::Merged {
            step: step.to_string(),
            side: MergeSide::Overlay,
        },
    );
    meta.basis = Basis::Computed;
    meta.fields = Some(metas);
    if let Some(p) = &m.present_when {
        let is_null =
            |f: &str| matches!(fields.get(f), None | Some(Value::Null | Value::Absent(_)));
        if !p.holds(&is_null) {
            return (
                Value::Absent(Absence {
                    reason: AbsentReason::PredicateFailed,
                    predicate: Some(p.canonical()),
                }),
                meta,
            );
        }
    }
    (Value::Record(fields), meta)
}

/// Evaluate `merge_fields@1` over two records, or per item over a list and
/// a join over it, matched by position in left order.
pub(crate) fn merge(
    m: &MergeFields,
    step: &str,
    base: (&Value, &ValueMeta),
    overlay: (&Value, &ValueMeta),
) -> Result<(Value, ValueMeta, Vec<MergeEvidence>), String> {
    let mut evidence = Vec::new();
    match (base.0, overlay.0) {
        (Value::List(items), Value::Joined(entries)) => {
            if items.len() != entries.len() {
                return Err("the join does not cover the base list".into());
            }
            let mut out = Vec::new();
            let mut metas = Vec::new();
            for (i, (item, entry)) in items.iter().zip(entries).enumerate() {
                let base_meta = base.1.items.as_ref().and_then(|v| v.get(i));
                let overlay_value = match &entry.matched {
                    JoinMatch::Matched(v) => Some(v.as_ref()),
                    _ => None,
                };
                let overlay_meta = overlay
                    .1
                    .items
                    .as_ref()
                    .and_then(|v| v.get(i))
                    .and_then(|m| m.fields.as_ref())
                    .and_then(|f| f.get("match"));
                let (v, meta) = merge_one(
                    m,
                    step,
                    &entry.id,
                    side_of(Some(item), base_meta),
                    side_of(overlay_value, overlay_meta),
                    &mut evidence,
                );
                out.push(v);
                metas.push(meta);
            }
            let mut meta = ValueMeta::combination(metas.iter(), Basis::Computed);
            meta.items = Some(metas);
            Ok((Value::List(out), meta, evidence))
        }
        (b, o) => {
            let (v, meta) = merge_one(
                m,
                step,
                "",
                side_of(Some(b), Some(base.1)),
                side_of(Some(o), Some(overlay.1)),
                &mut evidence,
            );
            Ok((v, meta, evidence))
        }
    }
}

/// The id of an item no id can be read from: its position.
pub(crate) fn position_id(i: usize) -> String {
    format!("#{i}")
}

/// Evaluate `join_keyed@1` (spec 032, 3.5.2 and 3.5.3).
pub(crate) fn join(
    left: (&Value, &ValueMeta),
    key: &str,
    key_path: &str,
    right: (&Value, &ValueMeta),
    here: ValueMeta,
) -> Result<(Value, ValueMeta), String> {
    let (Value::List(items), Value::Map(map)) = (left.0, right.0) else {
        return Err("join_keyed reads a list and a map".into());
    };
    let mut seen = std::collections::BTreeSet::new();
    let mut entries = Vec::new();
    let mut metas = Vec::new();
    for (i, it) in items.iter().enumerate() {
        let item_meta = left.1.items.as_ref().and_then(|v| v.get(i));
        let key_value = match (key, it) {
            ("id", Value::Text(s) | Value::Enum(s)) => Some(s.clone()),
            (k, Value::Record(r)) => match r.get(k) {
                Some(Value::Text(s) | Value::Enum(s)) => Some(s.clone()),
                _ => None,
            },
            _ => None,
        };
        let key_meta = match (key, item_meta) {
            ("id", Some(m)) => m.clone(),
            (k, Some(m)) => meta_at(m, &[k.to_string()]),
            _ => ValueMeta::ungraded(Basis::Computed),
        };
        let (entry, match_meta) = match key_value {
            None => (
                JoinEntry {
                    id: position_id(i),
                    matched: JoinMatch::Unmatched {
                        key_path: Some(key_path.to_string()),
                    },
                },
                ValueMeta::ungraded(Basis::Computed),
            ),
            Some(id) => {
                if !seen.insert(id.clone()) {
                    return Err(format!("duplicate id {id:?}"));
                }
                let em = right
                    .1
                    .entries
                    .as_ref()
                    .and_then(|e| e.get(&id))
                    .cloned()
                    .unwrap_or_else(|| ValueMeta::ungraded(Basis::Computed));
                let matched = match map.get(&id) {
                    None => JoinMatch::Unmatched { key_path: None },
                    Some(Value::Absent(a)) => JoinMatch::MatchedAbsent(a.clone()),
                    Some(v) => JoinMatch::Matched(Box::new(v.clone())),
                };
                (JoinEntry { id, matched }, em)
            }
        };
        let mut item = ValueMeta::combination([&key_meta, &match_meta], Basis::Computed);
        item.fields = Some(BTreeMap::from([
            ("id".to_string(), key_meta),
            ("match".to_string(), match_meta),
        ]));
        entries.push(entry);
        metas.push(item);
    }
    let mut meta =
        ValueMeta::combination(std::iter::once(&here).chain(metas.iter()), Basis::Computed);
    meta.items = Some(metas);
    Ok((Value::Joined(entries), meta))
}

/// A corroborated claim's metadata: every node at `max(grade, upgrade_to)`
/// (an ungraded node takes `upgrade_to`), basis `corroborated{step}`;
/// provenance, sources and derivation never change (spec 032, 3.7.5).
pub(crate) fn raise(m: &ValueMeta, g: Grade, step: &str) -> ValueMeta {
    let up = |v: ValueGrade| match v.graded() {
        Some(x) => ValueGrade::from(x.max(g)),
        None => ValueGrade::from(g),
    };
    ValueMeta {
        grade: up(m.grade),
        basis: Basis::Corroborated {
            step: step.to_string(),
        },
        fields: m.fields.as_ref().map(|f| {
            f.iter()
                .map(|(k, c)| (k.clone(), raise(c, g, step)))
                .collect()
        }),
        items: m
            .items
            .as_ref()
            .map(|i| i.iter().map(|c| raise(c, g, step)).collect()),
        entries: m.entries.as_ref().map(|e| {
            e.iter()
                .map(|(k, c)| (k.clone(), raise(c, g, step)))
                .collect()
        }),
        ..m.clone()
    }
}

/// A contradicted claim kept at grade `hint` (spec 032, 3.7.5: "the claim
/// at grade `hint`"): every node, an ungraded one included, since the rule
/// names the claim's grade, not its nodes'.
pub(crate) fn lower_to_hint(m: &ValueMeta) -> ValueMeta {
    ValueMeta {
        grade: ValueGrade::Hint,
        fields: m.fields.as_ref().map(|f| {
            f.iter()
                .map(|(k, c)| (k.clone(), lower_to_hint(c)))
                .collect()
        }),
        items: m
            .items
            .as_ref()
            .map(|i| i.iter().map(lower_to_hint).collect()),
        entries: m.entries.as_ref().map(|e| {
            e.iter()
                .map(|(k, c)| (k.clone(), lower_to_hint(c)))
                .collect()
        }),
        ..m.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustev_contract::definition::GradeRequirement;
    use serde_json::json;

    fn ts(ms: i64) -> Timestamp {
        Timestamp::from_ms(ms).unwrap()
    }

    fn meta(g: Grade, as_of: Option<i64>) -> ValueMeta {
        ValueMeta {
            grade: g.into(),
            provenance: vec![ProvenanceClass::ThirdParty],
            sources: vec!["s".into()],
            as_of_ms: as_of.map_or(AsOf::None, |m| AsOf::Ms(ts(m))),
            ..ValueMeta::ungraded(Basis::Declared)
        }
    }

    fn record_meta(fields: &[(&str, ValueMeta)]) -> ValueMeta {
        let parts: BTreeMap<String, ValueMeta> = fields
            .iter()
            .map(|(k, m)| (k.to_string(), m.clone()))
            .collect();
        let mut m = ValueMeta::combination(parts.values(), Basis::Declared);
        m.fields = Some(parts);
        m
    }

    fn read(min: Option<Grade>, age: AgeLimit, gate: bool) -> Read {
        Read {
            base: "input:x".into(),
            path: vec![],
            mode: Mode::Whole,
            require: min.map(|min| GradeRequirement {
                min,
                max_age_ms: age,
            }),
            gate,
        }
    }

    #[test]
    fn a_read_is_judged_against_its_grade_and_age() {
        let graded = Ceil::leaf(Grade::Authoritative.into(), false);
        let r = read(Some(Grade::Cached), AgeLimit::Ms(100), false);
        let now = ts(1_000);
        let ok = |m: &ValueMeta| check_read(&r, &graded, m, now);
        assert_eq!(
            ok(&meta(Grade::Cached, Some(900))),
            Ok(()),
            "equal age is fresh"
        );
        assert_eq!(
            ok(&meta(Grade::Cached, Some(899))),
            Err(Unresolved::stale(["input:x".to_string()])),
            "one millisecond past"
        );
        assert_eq!(
            ok(&meta(Grade::Authoritative, Some(0))),
            Ok(()),
            "authoritative is not aged"
        );
        assert_eq!(
            ok(&meta(Grade::Estimated, Some(1_000))),
            Err(Unresolved::insufficient(["input:x".to_string()]))
        );
        assert_eq!(
            ok(&ValueMeta::ungraded(Basis::Computed)),
            Err(Unresolved::insufficient(["input:x".to_string()])),
            "ungraded never meets a requirement"
        );
        // A hard gate over a graded value requires `estimated` implicitly; an
        // ungraded ceiling carries no requirement.
        let gate = read(None, AgeLimit::NotRequired, true);
        assert_eq!(
            check_read(&gate, &graded, &meta(Grade::Hint, None), now),
            Err(Unresolved::insufficient(["input:x".to_string()]))
        );
        let ungraded = Ceil::leaf(ValueGrade::Ungraded, false);
        assert_eq!(requirement(&gate, &ungraded), None);
    }

    #[test]
    fn every_future_dated_entry_is_recorded_rejected() {
        let decl: InputDecl = serde_json::from_value(json!({"name": "x",
            "type": {"text": {"max_bytes": 8}}, "provenance": ["user-supplied"],
            "freshness": "not_required"}))
        .unwrap();
        let entry = |source: &str| -> Entry {
            serde_json::from_value(json!({"field": "x", "value": "v",
                "provenance": "user-supplied", "as_of_ms": 2_000, "source": source,
                "grade": "cached"}))
            .unwrap()
        };
        let (a, b) = (entry("a"), entry("b"));
        let (r, q) = validate_input("x", &decl, &[&a, &b], None, ts(1_000));
        assert!(r.is_err());
        let records = q.entries.unwrap();
        assert_eq!(records.len(), 2);
        assert!(
            records
                .iter()
                .all(|e| matches!(e.disposition, Disposition::Rejected { .. }))
        );
    }

    #[test]
    fn an_entry_with_no_canonical_form_is_rejected_not_compared() {
        let decl: InputDecl = serde_json::from_value(json!({"name": "x",
            "type": {"text": {"max_bytes": 8}}, "provenance": ["user-supplied"],
            "freshness": "not_required"}))
        .unwrap();
        let entry = |value: Json| -> Entry {
            serde_json::from_value(json!({"field": "x", "value": value,
                "provenance": "user-supplied", "as_of_ms": 0, "source": "a",
                "grade": "cached"}))
            .unwrap()
        };
        let (a, b) = (entry(json!(1.5)), entry(json!(2.5)));
        let (r, q) = validate_input("x", &decl, &[&a, &b], None, ts(1_000));
        assert_eq!(
            r.unwrap_err(),
            Unresolved::invalid(["x".to_string()], "value has no canonical form")
        );
        assert!(
            q.entries
                .unwrap()
                .iter()
                .all(|e| matches!(e.disposition, Disposition::Rejected { .. })),
            "every such entry"
        );
    }

    #[test]
    fn a_stale_entry_is_recorded_rejected_not_used() {
        let decl: InputDecl = serde_json::from_value(json!({"name": "x",
            "type": {"text": {"max_bytes": 8}}, "provenance": ["user-supplied"],
            "freshness": {"max_age_ms": 100}}))
        .unwrap();
        let e: Entry = serde_json::from_value(json!({"field": "x", "value": "v",
            "provenance": "user-supplied", "as_of_ms": 0, "source": "a", "grade": "cached"}))
        .unwrap();
        let (r, q) = validate_input("x", &decl, &[&e], None, ts(101));
        assert_eq!(r.unwrap_err(), Unresolved::stale(["x".to_string()]));
        assert_eq!(
            q.entries.unwrap()[0].disposition,
            Disposition::Rejected {
                reason: "stale".into()
            }
        );
        let (r, _) = validate_input("x", &decl, &[&e], None, ts(100));
        assert!(r.is_ok(), "equal age is fresh");
    }

    #[test]
    fn metadata_is_read_at_a_path_or_across_items() {
        let rec = |price| record_meta(&[("price", meta(price, None))]);
        let mut list = ValueMeta::combination(
            [rec(Grade::Cached), rec(Grade::Hint)].iter(),
            Basis::Declared,
        );
        list.items = Some(vec![rec(Grade::Cached), rec(Grade::Hint)]);
        let at =
            |p: &[&str]| meta_at(&list, &p.iter().map(|s| s.to_string()).collect::<Vec<_>>()).grade;
        assert_eq!(at(&["*", "price"]), ValueGrade::Hint, "the weakest item");
        assert_eq!(at(&["*", "price", "deeper"]), ValueGrade::Hint);
        let first = meta_at(&list.items.as_ref().unwrap()[0], &["price".to_string()]);
        assert_eq!(first.grade, ValueGrade::Cached);
        assert_eq!(first.fields, None);
    }

    #[test]
    fn a_join_matches_by_key_and_names_what_it_cannot_read() {
        let item = |id: &str| Value::Record([("id".to_string(), Value::Text(id.into()))].into());
        let left = Value::List(vec![
            item("a"),
            item("b"),
            Value::Record(BTreeMap::new()),
            item("c"),
        ]);
        let right = Value::Map(BTreeMap::from([
            ("a".to_string(), Value::Integer(1)),
            (
                "c".to_string(),
                Value::Absent(Absence {
                    reason: AbsentReason::PredicateFailed,
                    predicate: None,
                }),
            ),
        ]));
        let here = ValueMeta::ungraded(Basis::Computed);
        let none = ValueMeta::ungraded(Basis::Declared);
        let (v, m) = join((&left, &none), "id", "id", (&right, &none), here.clone()).unwrap();
        let Value::Joined(entries) = v else {
            panic!("{v:?}")
        };
        assert_eq!(
            entries[0].matched,
            JoinMatch::Matched(Box::new(Value::Integer(1)))
        );
        assert_eq!(entries[1].matched, JoinMatch::Unmatched { key_path: None });
        assert_eq!(entries[2].id, position_id(2), "no readable key");
        assert_eq!(
            entries[2].matched,
            JoinMatch::Unmatched {
                key_path: Some("id".into())
            }
        );
        assert!(matches!(entries[3].matched, JoinMatch::MatchedAbsent(_)));
        assert_eq!(m.items.as_ref().map(Vec::len), Some(4));
        let twice = Value::List(vec![item("a"), item("a")]);
        assert!(join((&twice, &none), "id", "id", (&right, &none), here.clone()).is_err());
        assert!(join((&right, &none), "id", "id", (&left, &none), here).is_err());
    }

    fn merge_args(rules: Json, present_when: Option<Json>) -> MergeFields {
        let mut a = json!({"base": "input:b", "overlay": "input:o", "rules": rules});
        if let Some(p) = present_when {
            a["present_when"] = p;
        }
        match crate::ops::decode_v2("merge_fields", 1, &a)
            .unwrap()
            .unwrap()
        {
            crate::ops::OpArgs::MergeFields(m) => m,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_merge_takes_each_field_by_its_rule() {
        let m = merge_args(
            json!([
                {"field": "price", "rule": "overlay_only"},
                {"field": "name", "rule": "base_only"},
                {"field": "rating", "rule": {"overlay_else_base_if_grade_at_least": {"grade": "cached"}}},
                {"field": "stars", "rule": {"overlay_else_base_if_grade_at_least": {"grade": "authoritative"}}}
            ]),
            None,
        );
        let base = Value::Record(BTreeMap::from([
            ("name".to_string(), Value::Text("inn".into())),
            ("rating".to_string(), Value::Integer(4)),
            ("stars".to_string(), Value::Integer(3)),
        ]));
        let base_meta = record_meta(&[
            ("name", meta(Grade::Cached, None)),
            ("rating", meta(Grade::Cached, None)),
            ("stars", meta(Grade::Cached, None)),
        ]);
        let overlay = Value::Record(BTreeMap::from([("price".to_string(), Value::Integer(90))]));
        let overlay_meta = record_meta(&[("price", meta(Grade::Authoritative, None))]);
        let (v, meta, evidence) =
            merge(&m, "merged", (&base, &base_meta), (&overlay, &overlay_meta)).unwrap();
        let Value::Record(f) = v else { panic!("{v:?}") };
        assert_eq!(f["price"], Value::Integer(90));
        assert_eq!(f["name"], Value::Text("inn".into()));
        assert_eq!(f["rating"], Value::Integer(4), "the base meets cached");
        assert_eq!(
            f["stars"],
            Value::Absent(Absence {
                reason: AbsentReason::OverlayMissingBaseBelowGrade,
                predicate: None
            })
        );
        let fields = meta.fields.unwrap();
        assert_eq!(
            fields["price"].grade,
            ValueGrade::from(Grade::Authoritative)
        );
        assert_eq!(
            fields["price"].basis,
            Basis::Merged {
                step: "merged".into(),
                side: MergeSide::Overlay
            }
        );
        assert_eq!(evidence.len(), 4);
    }

    #[test]
    fn a_merged_record_failing_its_presence_predicate_is_absent() {
        let m = merge_args(
            json!([{"field": "price", "rule": "overlay_only"}]),
            Some(json!({"any_non_null": ["price"]})),
        );
        let empty = Value::Record(BTreeMap::new());
        let none = record_meta(&[]);
        let (v, _, _) = merge(&m, "merged", (&empty, &none), (&empty, &none)).unwrap();
        assert!(matches!(
            v,
            Value::Absent(Absence {
                reason: AbsentReason::PredicateFailed,
                ..
            })
        ));
    }

    #[test]
    fn corroboration_raises_every_node_and_contradiction_lowers_it() {
        let m = record_meta(&[
            ("a", meta(Grade::Hint, None)),
            ("b", meta(Grade::Authoritative, None)),
            ("c", ValueMeta::ungraded(Basis::Declared)),
        ]);
        let up = raise(&m, Grade::Cached, "check");
        let f = up.fields.as_ref().unwrap();
        assert_eq!(f["a"].grade, ValueGrade::from(Grade::Cached));
        assert_eq!(
            f["b"].grade,
            ValueGrade::from(Grade::Authoritative),
            "never lowered"
        );
        assert_eq!(
            f["c"].grade,
            ValueGrade::from(Grade::Cached),
            "ungraded takes upgrade_to"
        );
        assert_eq!(up.grade, ValueGrade::from(Grade::Cached));
        assert_eq!(
            up.basis,
            Basis::Corroborated {
                step: "check".into()
            }
        );
        assert_eq!(
            (up.provenance, up.sources),
            (m.provenance.clone(), m.sources.clone())
        );
        let down = lower_to_hint(&m);
        assert_eq!(down.grade, ValueGrade::Hint);
        assert!(
            down.fields
                .unwrap()
                .values()
                .all(|c| c.grade == ValueGrade::Hint)
        );
    }
}
