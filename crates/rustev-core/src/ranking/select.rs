//! Pure selection and exact backfill (039, 3.10-3.11).
use rustev_contract::judgment::{Derivation, Lineage, Unresolved};
use rustev_contract::ranking::{BackendFailure, Backfill, Select};
use rustev_contract::ranking_result::*;
use std::collections::BTreeSet;

/// A resolved source's entries in source order, or its unresolved reason.
/// Later evaluation adapts semantic and composed rankings to this same seam.
pub struct Source<'a> {
    pub ids: Result<&'a [String], Unresolved>,
    pub partial: bool,
    pub derivation: Derivation,
    pub lineage: &'a Lineage,
}

/// The caller resolves the declared step reference; both classes are checked.
pub struct ExactOrdering<'a> {
    pub from: &'a str,
    pub static_class: Derivation,
    pub source: Source<'a>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionResult {
    pub value: Result<Selection, Unresolved>,
    pub derivation: Derivation,
    pub lineage: Lineage,
    /// The evaluator emits a fallback notice for its select step with this reason.
    pub fallback: Option<Unresolved>,
    /// Retained even when a needed backfill is unresolved.
    pub backfill: SelectionBackfill,
}

pub fn backend_failure(reason: &Unresolved) -> bool {
    matches!(
        reason,
        Unresolved::BackendUnavailable { .. }
            | Unresolved::InvalidBackendOutput { .. }
            | Unresolved::BudgetExhausted { .. }
            | Unresolved::DeadlineExceeded
            | Unresolved::Unsupported { .. }
    )
}

fn invalid(detail: &str) -> Unresolved {
    Unresolved::InvalidInput {
        fields: vec![],
        detail: detail.into(),
    }
}

/// The pool remains available even when the source failed. Backfill's resolved
/// value is inspected only when needed, but its declared static class is always
/// checked. A selection remains mixed-derived even with no model positions.
pub fn select(
    declaration: &Select,
    pool: &[String],
    model: &Source<'_>,
    backfill: Option<&ExactOrdering<'_>>,
) -> SelectionResult {
    let mut lineage = model.lineage.clone();
    if let Some(b) = backfill {
        lineage.inputs.extend(b.source.lineage.inputs.clone());
        lineage.steps.extend(b.source.lineage.steps.clone());
        if let Some(producers) = &b.source.lineage.producers {
            lineage
                .producers
                .get_or_insert_with(Default::default)
                .extend(producers.clone());
        }
    }
    lineage.inputs.sort();
    lineage.inputs.dedup();
    lineage.steps.sort();
    lineage.steps.dedup();
    let mut result = SelectionResult {
        value: Err(invalid("selection not evaluated")),
        derivation: Derivation::MixedDerived,
        lineage,
        fallback: None,
        backfill: SelectionBackfill::None,
    };
    result.value = assemble(
        declaration,
        pool,
        model,
        backfill,
        &mut result.fallback,
        &mut result.backfill,
    );
    result
}

fn assemble(
    d: &Select,
    pool: &[String],
    model: &Source<'_>,
    backfill: Option<&ExactOrdering<'_>>,
    fallback: &mut Option<Unresolved>,
    state: &mut SelectionBackfill,
) -> Result<Selection, Unresolved> {
    if !(1..=1024).contains(&d.k)
        || pool.len() > 1024
        || pool.iter().collect::<BTreeSet<_>>().len() != pool.len()
    {
        return Err(invalid("invalid selection bound or pool"));
    }
    match (&d.backfill, backfill) {
        (Backfill::None, None) if d.on_backend_failure != BackendFailure::BackfillAll => {}
        (
            Backfill::From {
                from,
                exclude_selected: true,
            },
            Some(b),
        ) if from == b.from
            && b.static_class == Derivation::ExactDerived
            && b.source.derivation == Derivation::ExactDerived => {}
        _ => {
            return Err(invalid(
                "backfill must match its declared exact-derived ordering",
            ));
        }
    }
    let mut entries = Vec::new();
    let mut selected = BTreeSet::new();
    let status = match &model.ids {
        Ok(ids) => {
            // Reject malformed source values, rather than laundering pool escapes
            // or duplicate positions through the model half of a selection.
            if ids.len() > pool.len()
                || ids.iter().any(|id| !pool.contains(id))
                || ids.iter().collect::<BTreeSet<_>>().len() != ids.len()
            {
                return Err(invalid("invalid ranking source ids"));
            }
            for (p, id) in ids.iter().take(d.k as usize).enumerate() {
                selected.insert(id.clone());
                entries.push(SelectionEntry {
                    position: p as u64 + 1,
                    id: id.clone(),
                    origin: SelectionOrigin::Model,
                    source_position: p as u64 + 1,
                    derivation: model.derivation,
                    lineage: model.lineage.clone(),
                });
            }
            SelectionModel::Ranked {
                entries: ids.len() as u64,
                partial: model.partial,
            }
        }
        Err(reason) => {
            if !backend_failure(reason) || d.on_backend_failure == BackendFailure::Propagate {
                return Err(reason.clone());
            }
            *fallback = Some(reason.clone());
            SelectionModel::Failed {
                reason: reason.clone(),
            }
        }
    };
    if let Some(b) = backfill {
        if entries.len() as u64 == d.k {
            *state = SelectionBackfill::NotNeeded;
        } else {
            let ids = b.source.ids.as_ref().map_err(|reason| {
                *state = SelectionBackfill::Unresolved {
                    reason: reason.clone(),
                };
                reason.clone()
            })?;
            let (mut taken, mut skipped_selected, mut skipped_outside_pool) = (0, 0, 0);
            for (p, id) in ids.iter().enumerate() {
                if entries.len() as u64 == d.k {
                    break;
                }
                if selected.contains(id) {
                    skipped_selected += 1;
                } else if !pool.contains(id) {
                    skipped_outside_pool += 1;
                } else {
                    selected.insert(id.clone());
                    taken += 1;
                    entries.push(SelectionEntry {
                        position: entries.len() as u64 + 1,
                        id: id.clone(),
                        origin: SelectionOrigin::Backfill,
                        source_position: p as u64 + 1,
                        derivation: b.source.derivation,
                        lineage: b.source.lineage.clone(),
                    });
                }
            }
            *state = SelectionBackfill::Used {
                taken,
                skipped_selected,
                skipped_outside_pool,
            };
        }
    }
    Ok(Selection {
        k: d.k,
        shortfall: d.k - entries.len() as u64,
        entries,
        model: status,
        backfill: state.clone(),
    })
}
