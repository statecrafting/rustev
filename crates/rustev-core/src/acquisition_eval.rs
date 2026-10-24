//! Pure staged acquisition evaluation. A caller supplies pages in request order.
use super::*;
#[path = "widening_eval.rs"]
pub(super) mod widening;
use rustev_contract::Document;
use rustev_contract::acquisition::{
    AcquisitionPage, AcquisitionRequest, NextPage, RequestPage, RequestValue, validate_page,
};
use rustev_contract::acquisition_record::*;
use rustev_contract::acquisition_step::{Acquire, AcquireStop, Guard};
use rustev_contract::definition::StepBody;
use rustev_contract::grade::AsOf;
use rustev_contract::ids::{AcquisitionRequestId, ContentDigest};
use rustev_contract::time::DurationMs;

/// A single pending page, with the exact identity computed by the core.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingAcquisition {
    pub step: String,
    pub tier_index: u32,
    pub page_index: u32,
    pub source: String,
    pub operation: String,
    pub deadline_share: rustev_contract::Decimal,
    pub request: AcquisitionRequest,
    pub request_id: AcquisitionRequestId,
}

pub(super) struct State {
    decl: Acquire,
    operation: rustev_contract::acquisition::Operation,
    pending: Option<PendingAcquisition>,
    stop: Option<u64>,
    values: Vec<Value>,
    evidence: AcquisitionEvidence,
    oldest: Option<Timestamp>,
}

impl Evaluation<'_> {
    /// Pending pages in definition step order. Each step has at most one.
    pub fn pending_acquisitions(&self) -> Vec<PendingAcquisition> {
        self.compiled
            .info
            .step_ids
            .iter()
            .filter_map(|id| {
                self.acquisitions
                    .get(id)
                    .and_then(|s| s.pending.clone())
                    .or_else(|| self.widenings.get(id).and_then(|s| s.pending.clone()))
            })
            .collect()
    }

    pub(super) fn acquisition_evidence(&self) -> Option<Vec<AcquisitionEvidence>> {
        let records: Vec<_> = self
            .compiled
            .info
            .step_ids
            .iter()
            .filter_map(|id| {
                self.acquisitions
                    .get(id)
                    .map(|s| s.evidence.clone())
                    .or_else(|| self.widenings.get(id).map(|s| s.evidence.clone()))
            })
            .collect();
        (!records.is_empty()).then_some(records)
    }

    fn acquisition_refs(&self, refs: &[String]) -> Result<Option<String>, Unresolved> {
        let bases: Vec<_> = refs.iter().map(|r| crate::expr::base_ref_v2(r)).collect();
        if let Some(u) = merge(bases.iter().filter_map(|r| self.status(r).err()).collect()) {
            return Err(u);
        }
        Ok(bases
            .iter()
            .find_map(|r| self.skipped.get(r.strip_prefix("step:")?).cloned()))
    }

    // A false or failed guard does not wait for request-only dependencies.
    pub(super) fn acquisition_guard_terminal(&self, id: &str) -> bool {
        let Some(StepBody::Acquire(a)) = self
            .compiled
            .plan
            .definition
            .steps
            .iter()
            .find(|s| s.id == id)
            .map(|s| &s.body)
        else {
            return false;
        };
        let Guard::Predicate(p) = &a.when else {
            return false;
        };
        let mut refs = vec![];
        crate::expr::predicate_refs(p, &mut refs);
        if !refs
            .iter()
            .all(|r| self.dep_done(&crate::expr::base_ref_v2(r)))
        {
            return false;
        }
        if self.acquisition_refs(&refs).is_err() {
            return true;
        }
        let mut values = vec![];
        crate::compile::predicate_value_refs(p, &mut values);
        if matches!(self.acquisition_refs(&values), Ok(Some(_))) {
            return true;
        }
        let mut reads = vec![];
        quality::predicate_reads(p, None, true, &mut reads);
        self.acquisition_quality(&reads).is_err()
            || !matches!(crate::expr::eval_predicate(p, &View::scope(self)), Ok(true))
    }

    fn acquisition_quality(&self, reads: &[quality::Read]) -> Result<(), Unresolved> {
        for r in reads {
            if View::skipped(self, &r.base) {
                continue;
            }
            grading::check_read(
                r,
                &quality::read_ceiling(r, &self.compiled.info.trees),
                &grading::read_meta(r, &self.metas),
                self.now,
            )?;
        }
        Ok(())
    }

    pub(super) fn start_acquisition(&mut self, id: &str, a: &crate::acquisition::Acquisition) {
        if let Some(StepBody::Widen(w)) = self
            .compiled
            .plan
            .definition
            .steps
            .iter()
            .find(|s| s.id == id)
            .map(|s| &s.body)
        {
            self.start_widening(id, w, a);
            return;
        }
        let Some(StepBody::Acquire(decl)) = self
            .compiled
            .plan
            .definition
            .steps
            .iter()
            .find(|s| s.id == id)
            .map(|s| &s.body)
        else {
            // A compiled acquisition always names an `acquire` step; record the
            // broken invariant as the step's result so evaluation completes.
            self.exact.insert(
                id.into(),
                Err(Unresolved::invalid(
                    [],
                    "acquisition step not found in the plan",
                )),
            );
            return;
        };
        // Compilation refuses an acquisition without a bound source and
        // operation (3.9.3), so this is a broken invariant, recorded like the
        // one above; acquisition evidence needs the source and operation it
        // would name, so none is written for it.
        let (Some(operation), Some(binding)) = (a.operations.first(), a.plan.sources.first())
        else {
            self.exact.insert(
                id.into(),
                Err(Unresolved::invalid(
                    [],
                    "acquisition step has no bound source",
                )),
            );
            return;
        };
        let mut state = State {
            decl: decl.clone(),
            operation: operation.clone(),
            pending: None,
            stop: None,
            values: vec![],
            oldest: None,
            evidence: AcquisitionEvidence {
                step: id.into(),
                kind: AcquisitionKind::Acquire,
                outcome: AcquisitionOutcome::Resolved,
                stopped_by: AcquisitionStop::None,
                truncated: 0,
                request_constructed: false,
                tiers: vec![TierEvidence {
                    name: id.into(),
                    outcome: TierOutcome::Empty,
                    relaxation: 0,
                    source: binding.source.name.clone(),
                    operation: operation.name.clone(),
                    request_constructed: false,
                    items_received: 0,
                    unique_items_added: 0,
                    duplicates: 0,
                    pages: vec![],
                }],
                items: vec![],
            },
        };
        self.derivation
            .insert(format!("step:{id}"), a.plan.derivation);
        let built = (|| -> Result<Option<String>, Unresolved> {
            if let Guard::Predicate(p) = &decl.when {
                let mut refs = vec![];
                crate::expr::predicate_refs(p, &mut refs);
                self.acquisition_refs(&refs)?;
                let mut values = vec![];
                crate::compile::predicate_value_refs(p, &mut values);
                if let Some(guard) = self.acquisition_refs(&values)? {
                    return Ok(Some(guard));
                }
                let mut reads = vec![];
                quality::predicate_reads(p, None, true, &mut reads);
                self.acquisition_quality(&reads)?;
                if !crate::expr::eval_predicate(p, &View::scope(self)).map_err(fault)? {
                    return Ok(Some(id.into()));
                }
            }
            let mut refs = vec![];
            for e in decl.request.values() {
                crate::expr::expr_refs(e, &mut refs);
            }
            if let AcquireStop::ItemsAtLeast { count } = &decl.stop {
                crate::expr::expr_refs(count, &mut refs);
            }
            if let Some(guard) = self.acquisition_refs(&refs)? {
                return Ok(Some(guard));
            }
            let mut reads = vec![];
            for e in decl.request.values() {
                quality::expr_reads(e, None, false, &mut reads);
            }
            if let AcquireStop::ItemsAtLeast { count } = &decl.stop {
                quality::expr_reads(count, None, false, &mut reads);
            }
            self.acquisition_quality(&reads)?;
            state.stop = match &decl.stop {
                AcquireStop::None => None,
                AcquireStop::ItemsAtLeast { count } => {
                    match crate::expr::eval_expr(count, &View::scope(self)).map_err(fault)? {
                        Value::Integer(n) if n > 0 => Some(n as u64),
                        _ => return Err(Unresolved::invalid([], "stop count must be positive")),
                    }
                }
            };
            let mut fields = vec![];
            let mut constructed = false;
            for f in &operation.request {
                fields.push(match decl.request.get(&f.name) {
                    Some(e) => {
                        let mut reads = vec![];
                        crate::expr::expr_refs(e, &mut reads);
                        constructed |= reads
                            .iter()
                            .any(|r| self.constructed.contains(&crate::expr::base_ref_v2(r)));
                        RequestValue::Set {
                            value: crate::expr::eval_expr(e, &View::scope(self))
                                .map_err(fault)?
                                .to_json(),
                        }
                    }
                    None => RequestValue::Unset,
                });
            }
            state.evidence.request_constructed = constructed;
            if let Some(tier) = state.evidence.tiers.first_mut() {
                tier.request_constructed = constructed;
            }
            let request = AcquisitionRequest {
                schema: schema::ACQUISITION_REQUEST.into(),
                source: binding.source.id.clone(),
                operation: operation.name.clone(),
                fields,
                page: RequestPage::First,
            };
            state.pending = Some(make_pending(
                id,
                0,
                request,
                operation.max_request_bytes,
                &decl.source,
                decl.limits.deadline_share,
            )?);
            Ok(None)
        })();
        match built {
            Ok(Some(guard)) => {
                self.skipped.insert(id.into(), guard.clone());
                state.evidence.outcome = AcquisitionOutcome::Skipped { guard };
                if let Some(t) = state.evidence.tiers.first_mut() {
                    t.outcome = TierOutcome::Skipped;
                }
            }
            Err(u) => self.fail_acquisition(id, &mut state, u),
            Ok(None) => {}
        }
        self.acquisitions.insert(id.into(), state);
    }

    fn fail_acquisition(&mut self, id: &str, state: &mut State, u: Unresolved) {
        if let Some(p) = state.pending.take()
            && let Some(t) = state.evidence.tiers.first_mut()
            && t.pages.len() == p.page_index as usize
        {
            t.pages.push(PageEvidence {
                request: p.request_id,
                digest: PageDigest::None,
                items: 0,
                unique_items_added: 0,
            });
        }
        state.values.clear();
        state.evidence.items.clear();
        state.evidence.outcome = AcquisitionOutcome::Unresolved(u.clone());
        if let Some(t) = state.evidence.tiers.first_mut() {
            t.outcome = TierOutcome::Failed { reason: u.clone() };
            t.unique_items_added = 0;
            for page in &mut t.pages {
                page.unique_items_added = 0;
            }
        }
        self.exact.insert(id.into(), Err(u));
    }

    /// Parse and validate page bytes against the pending request and bound contract.
    /// An invalid page completes the step as `invalid_backend_output`.
    pub fn supply_page(
        &mut self,
        step: &str,
        tier_index: u32,
        page_index: u32,
        bytes: &[u8],
    ) -> Result<(), SupplyError> {
        if self.widenings.contains_key(step) {
            return self.supply_widening_page(step, tier_index, page_index, bytes);
        }
        let pending = self
            .acquisitions
            .get(step)
            .and_then(|s| s.pending.clone())
            .filter(|p| p.tier_index == tier_index && p.page_index == page_index)
            .ok_or_else(|| not_pending(step))?;
        let Some(mut state) = self.acquisitions.remove(step) else {
            return Err(not_pending(step));
        };
        let result = (|| -> Result<(), Unresolved> {
            let page = AcquisitionPage::parse(bytes).map_err(|e| bad_page(e.to_string()))?;
            validate_page(
                &page,
                &pending.request,
                &state.operation,
                self.now,
                DurationMs(state.decl.limits.as_of_skew_ms),
            )
            .map_err(|e| bad_page(format!("{e:?}")))?;
            let digest = ContentDigest::parse(&rustev_contract::canonical::tagged_digest(
                schema::ACQUISITION_PAGE,
                &page.canonical().map_err(|e| bad_page(e.to_string()))?,
            ))
            .map_err(|e| bad_page(e.to_string()))?;
            if let Some(t) = state.evidence.tiers.first_mut() {
                t.items_received += page.items.len() as u64;
                t.pages.push(PageEvidence {
                    request: pending.request_id.clone(),
                    digest: PageDigest::Sha256(digest),
                    items: page.items.len() as u64,
                    unique_items_added: 0,
                });
            }
            let age = self
                .now
                .as_ms()
                .saturating_sub(page.as_of_ms.as_ms())
                .max(0) as u64;
            let stale: Vec<_> = state.operation.item.iter().filter(|f| matches!(f.freshness, rustev_contract::acquisition::Freshness::MaxAgeMs { value } if age > value)).map(|f| format!("{step}/{}", f.name)).collect();
            if !stale.is_empty() {
                return Err(Unresolved::stale(stale));
            }
            let room = state
                .decl
                .limits
                .max_items
                .saturating_sub(state.values.len() as u64);
            let keep = (page.items.len() as u64).min(room) as usize;
            let basis = Basis::Acquired { step: step.into() };
            let source = state
                .evidence
                .tiers
                .first()
                .map(|t| t.source.clone())
                .unwrap_or_default();
            for item in page.items.iter().take(keep) {
                let mut values = BTreeMap::new();
                let mut fields = BTreeMap::new();
                for f in &state.operation.item {
                    let raw = item
                        .get(&f.name)
                        .ok_or_else(|| bad_page("missing item field"))?;
                    values.insert(
                        f.name.clone(),
                        crate::value::decode_v2(&f.ty, raw).map_err(bad_page)?,
                    );
                    let mut meta = ValueMeta::ungraded(basis.clone());
                    meta.grade = f.limit().into();
                    meta.provenance = vec![f.provenance];
                    meta.sources = vec![source.clone()];
                    meta.as_of_ms = AsOf::Ms(page.as_of_ms);
                    fields.insert(f.name.clone(), meta);
                }
                let mut meta = ValueMeta::combination(fields.values(), basis.clone());
                meta.fields = Some(fields);
                let value = Value::Record(values);
                let digest = ContentDigest::parse(&grading::digest(&value.to_json()))
                    .map_err(|e| bad_page(e.to_string()))?;
                state.evidence.items.push(AcquiredItemEvidence {
                    digest,
                    meta,
                    origin: None,
                });
                state.values.push(value);
            }
            if keep > 0 {
                state.oldest = Some(state.oldest.map_or(page.as_of_ms, |t| t.min(page.as_of_ms)));
            }
            state.evidence.truncated += page.items.len() as u64 - keep as u64;
            if let Some(t) = state.evidence.tiers.first_mut() {
                t.unique_items_added += keep as u64;
                if let Some(p) = t.pages.last_mut() {
                    p.unique_items_added = keep as u64;
                }
                t.outcome = if state.values.is_empty() {
                    TierOutcome::Empty
                } else {
                    TierOutcome::Ok
                };
            }
            let continuation = match page.next {
                NextPage::None => Err((AcquisitionStop::Exhausted, "exhausted")),
                _ if state.stop.is_some_and(|n| state.values.len() as u64 >= n) => {
                    Err((AcquisitionStop::StopCondition, "stop_condition"))
                }
                _ if u64::from(pending.page_index) + 1 >= state.decl.limits.max_pages => {
                    Err((AcquisitionStop::PageLimit, "page_limit"))
                }
                _ if state.values.len() as u64 >= state.decl.limits.max_items => {
                    Err((AcquisitionStop::ItemLimit, "item_limit"))
                }
                NextPage::Cursor { text } => Ok(RequestPage::Cursor { text }),
                NextPage::Offset { integer } => Ok(RequestPage::Offset { integer }),
            };
            if let Err((stop, name)) = continuation {
                state.pending = None;
                state.evidence.stopped_by = stop;
                let value = Value::Record(BTreeMap::from([
                    ("items".into(), Value::List(state.values.clone())),
                    ("stopped_by".into(), Value::Enum(name.into())),
                    (
                        "truncated".into(),
                        Value::Integer(state.evidence.truncated as i64),
                    ),
                    (
                        "as_of_ms".into(),
                        Value::Timestamp(state.oldest.unwrap_or(page.as_of_ms)),
                    ),
                    (
                        "request_constructed".into(),
                        Value::Bool(state.evidence.request_constructed),
                    ),
                ]));
                let mut list = ValueMeta::combination(
                    state.evidence.items.iter().map(|i| &i.meta),
                    basis.clone(),
                );
                list.items = Some(
                    state
                        .evidence
                        .items
                        .iter()
                        .map(|i| i.meta.clone())
                        .collect(),
                );
                let mut whole = list.clone();
                whole.items = None;
                whole.fields = Some(BTreeMap::from([("items".into(), list)]));
                self.metas.insert(
                    format!("step:{step}"),
                    Meta {
                        node: whole.grade,
                        whole,
                    },
                );
                self.exact_values.insert(step.into(), value.clone());
                self.exact.insert(step.into(), Ok(value));
                if state.evidence.truncated > 0 {
                    self.notices.push(Notice {
                        kind: NoticeKind::Truncation,
                        subject: format!("step:{step}"),
                        reason: format!(
                            "{} acquisition items not retained",
                            state.evidence.truncated
                        ),
                        action: None,
                        screening: None,
                    });
                }
            } else if let Ok(page) = continuation {
                let mut request = pending.request.clone();
                request.page = page;
                state.pending = Some(make_pending(
                    step,
                    pending.page_index + 1,
                    request,
                    state.operation.max_request_bytes,
                    &state.decl.source,
                    state.decl.limits.deadline_share,
                )?);
            }
            Ok(())
        })();
        if let Err(u) = result {
            self.fail_acquisition(step, &mut state, u);
        }
        self.acquisitions.insert(step.into(), state);
        self.advance();
        Ok(())
    }

    /// Complete a pending acquisition with one of the runtime's allowed failures.
    pub fn supply_acquisition_failure(
        &mut self,
        step: &str,
        tier_index: u32,
        page_index: u32,
        reason: Unresolved,
    ) -> Result<(), SupplyError> {
        if self.widenings.contains_key(step) {
            return self.supply_widening_failure(step, tier_index, page_index, reason);
        }
        let state = self
            .acquisitions
            .get(step)
            .filter(|s| {
                s.pending
                    .as_ref()
                    .is_some_and(|p| p.tier_index == tier_index && p.page_index == page_index)
            })
            .ok_or_else(|| not_pending(step))?;
        let valid = match &reason {
            Unresolved::MissingEvidence { fields } => fields == &[step.to_string()],
            Unresolved::BackendUnavailable { detail } => {
                state.evidence.tiers.first().is_some_and(|t| {
                    detail
                        .strip_prefix(&format!("source:{}", t.source))
                        .is_some_and(|tail| tail.is_empty() || tail.starts_with([':', ' ']))
                })
            }
            Unresolved::InvalidBackendOutput { .. } | Unresolved::DeadlineExceeded => true,
            Unresolved::BudgetExhausted { resource } => {
                matches!(resource.as_str(), "acquisition_calls" | "cost")
            }
            _ => false,
        };
        if !valid {
            return Err(SupplyError::NotARuntimeReason(reason.kind()));
        }
        let Some(mut state) = self.acquisitions.remove(step) else {
            return Err(not_pending(step));
        };
        self.fail_acquisition(step, &mut state, reason);
        self.acquisitions.insert(step.into(), state);
        self.advance();
        Ok(())
    }
}
fn make_pending(
    step: &str,
    page: u32,
    request: AcquisitionRequest,
    max_bytes: u64,
    source: &str,
    deadline_share: rustev_contract::Decimal,
) -> Result<PendingAcquisition, Unresolved> {
    let bytes = request
        .canonical()
        .map_err(|e| Unresolved::invalid([], e.to_string()))?;
    if bytes.len() as u64 > max_bytes {
        return Err(Unresolved::invalid([], "request exceeds max_request_bytes"));
    }
    let request_id = request
        .id()
        .map_err(|e| Unresolved::invalid([], e.to_string()))?;
    Ok(PendingAcquisition {
        step: step.into(),
        tier_index: 0,
        page_index: page,
        source: source.into(),
        operation: request.operation.clone(),
        deadline_share,
        request,
        request_id,
    })
}
fn not_pending(step: &str) -> SupplyError {
    SupplyError::NotPending {
        step: step.into(),
        instance: vec![],
    }
}
fn bad_page(detail: impl Into<String>) -> Unresolved {
    Unresolved::InvalidBackendOutput {
        detail: detail.into(),
    }
}
fn fault(f: crate::expr::Fault) -> Unresolved {
    match f {
        crate::expr::Fault::Missing(s) | crate::expr::Fault::Absent(s) => Unresolved::missing([s]),
        crate::expr::Fault::Conflict(s) => Unresolved::conflict([s]),
        crate::expr::Fault::Invalid(s) => Unresolved::invalid([], s),
    }
}
