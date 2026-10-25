//! Offline acquisition supplies, checked against the pending request and run.
use rustev_contract::Document;
use rustev_contract::acquisition::AcquisitionPage;
use rustev_contract::acquisition_record::AcquisitionResult;
use rustev_contract::ids::AcquisitionRequestId;
use rustev_contract::judgment::Unresolved;
use rustev_contract::replay::{AcquisitionSupply, ItemLocation};
use rustev_contract::run::RunRecord;
use rustev_core::evaluate::Evaluation;

use crate::replay::{Incomparable, Inconsistency};

/// An uncaptured request, with its recomputed identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingAcquisition {
    pub step: String,
    pub tier: u32,
    pub page: u32,
    pub request: AcquisitionRequestId,
}

#[derive(Debug, Clone)]
pub(crate) enum PageValue {
    Page(AcquisitionPage),
    Reason(Unresolved),
}

fn bad(index: usize, what: Inconsistency) -> Incomparable {
    Incomparable::Inconsistent {
        location: ItemLocation::AcquisitionPage(index),
        what,
    }
}

pub(crate) fn supply_page(
    ev: &mut Evaluation<'_>,
    entry: &AcquisitionSupply,
    value: &PageValue,
    run: &RunRecord,
    index: usize,
) -> Result<(), Incomparable> {
    let pending = ev
        .pending_acquisitions()
        .into_iter()
        .find(|p| p.step == entry.step && p.tier_index == entry.tier && p.page_index == entry.page)
        .ok_or_else(|| bad(index, Inconsistency::NotPending))?;
    if pending.request_id != entry.request {
        return Err(bad(index, Inconsistency::RequestIdentity));
    }
    let record = run
        .acquisitions
        .iter()
        .flatten()
        .find(|p| p.step == entry.step && p.tier == entry.tier && p.page == entry.page)
        .ok_or_else(|| bad(index, Inconsistency::NotInRun))?;
    if record.request != entry.request {
        return Err(bad(index, Inconsistency::RequestIdentity));
    }
    if record.attempts.last().map(|a| &a.attempt_id) != entry.attempt_id.as_ref() {
        return Err(bad(index, Inconsistency::Origin));
    }
    match (value, &record.result) {
        (PageValue::Page(page), AcquisitionResult::Page) => {
            let bytes = page
                .record_canonical()
                .map_err(|_| bad(index, Inconsistency::NoRecordForm))?;
            if page.request != pending.request_id {
                return Err(bad(index, Inconsistency::RequestIdentity));
            }
            ev.supply_page(&entry.step, entry.tier, entry.page, &bytes)
        }
        (PageValue::Reason(reason), AcquisitionResult::Failed(recorded)) if reason == recorded => {
            ev.supply_acquisition_failure(&entry.step, entry.tier, entry.page, reason.clone())
        }
        _ => return Err(bad(index, Inconsistency::Value)),
    }
    .map_err(|_| bad(index, Inconsistency::OutputRefused))
}

pub(crate) fn fill_missing(
    ev: &mut Evaluation<'_>,
    entries: &[AcquisitionSupply],
    missing: &mut Vec<MissingAcquisition>,
) -> Result<(), Incomparable> {
    loop {
        let absent = ev.pending_acquisitions().into_iter().find(|p| {
            !entries
                .iter()
                .any(|e| e.step == p.step && e.tier == p.tier_index && e.page == p.page_index)
        });
        let Some(p) = absent else {
            return Ok(());
        };
        ev.supply_acquisition_failure(
            &p.step,
            p.tier_index,
            p.page_index,
            Unresolved::MissingEvidence {
                fields: vec![p.step.clone()],
            },
        )
        // Unreachable for a fresh pending request: no mutation intervenes, and
        // MissingEvidence with exactly this step is accepted by acquire and widen.
        // This uncaptured page has no acquisition_pages index to report.
        .map_err(|_| bad(0, Inconsistency::NotPending))?;
        missing.push(MissingAcquisition {
            step: p.step,
            tier: p.tier_index,
            page: p.page_index,
            request: p.request_id,
        });
    }
}
