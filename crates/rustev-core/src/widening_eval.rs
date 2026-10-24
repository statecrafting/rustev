//! Ordered, transactional widening tiers. Only the caller supplies pages.
use super::*;
use rustev_contract::acquisition_step::{OnTierFailure, Widen, WidenStop};
use rustev_contract::definition::Expr;

pub(crate) struct State {
    decl: Widen,
    compiled: crate::acquisition::Acquisition,
    pub(super) pending: Option<PendingAcquisition>,
    pub(super) evidence: AcquisitionEvidence,
    tier: usize,
    stop: Option<u64>,
    values: Vec<Value>,
    keys: BTreeSet<Vec<u8>>,
    tier_start: usize,
    skipped_guard: Option<String>,
}

impl Evaluation<'_> {
    pub(super) fn start_widening(
        &mut self,
        id: &str,
        decl: &Widen,
        compiled: &crate::acquisition::Acquisition,
    ) {
        // One evidence entry per declared tier, index for index: later code
        // indexes `evidence.tiers` by the tier's position. A tier whose source
        // index has no binding (refused at compile, category 20) makes the
        // whole step invalid rather than shifting every later tier.
        let tiers: Option<Vec<TierEvidence>> = decl
            .tiers
            .iter()
            .map(|t| {
                let binding = compiled.plan.sources.get(t.source_index as usize)?;
                Some(TierEvidence {
                    name: t.name.clone(),
                    outcome: TierOutcome::NotNeeded,
                    relaxation: t.relax.len() as u64,
                    source: binding.source.name.clone(),
                    operation: binding.operation.clone(),
                    request_constructed: false,
                    items_received: 0,
                    unique_items_added: 0,
                    duplicates: 0,
                    pages: vec![],
                })
            })
            .collect();
        let tiers_bound = tiers.is_some();
        let tiers = tiers.unwrap_or_default();
        let mut state = State {
            decl: decl.clone(),
            compiled: compiled.clone(),
            pending: None,
            tier: 0,
            stop: None,
            values: vec![],
            keys: BTreeSet::new(),
            tier_start: 0,
            skipped_guard: None,
            evidence: AcquisitionEvidence {
                step: id.into(),
                kind: AcquisitionKind::Widen,
                outcome: AcquisitionOutcome::Resolved,
                stopped_by: AcquisitionStop::None,
                truncated: 0,
                request_constructed: false,
                tiers,
                items: vec![],
            },
        };
        self.derivation
            .insert(format!("step:{id}"), compiled.plan.derivation);
        // The shared count is evaluated once, before any request is dispatched.
        let count = match &decl.stop {
            _ if !tiers_bound => Err(Unresolved::invalid(
                [],
                "tier source_index outside the bound sources",
            )),
            WidenStop::None => Ok(None),
            WidenStop::UniqueAtLeast(e) => self.widening_expressions([e]).and_then(|skip| {
                if let Some(guard) = skip {
                    state.skipped_guard = Some(guard);
                    return Ok(None);
                }
                match crate::expr::eval_expr(e, &View::scope(self)).map_err(fault)? {
                    Value::Integer(n) if n > 0 => Ok(Some(n as u64)),
                    _ => Err(Unresolved::invalid([], "stop count must be positive")),
                }
            }),
        };
        match count {
            Err(u) => {
                state.evidence.outcome = AcquisitionOutcome::Unresolved(u.clone());
                self.exact.insert(id.into(), Err(u));
            }
            Ok(n) => {
                state.stop = n;
                if let Some(guard) = &state.skipped_guard {
                    for t in &mut state.evidence.tiers {
                        t.outcome = TierOutcome::Skipped;
                    }
                    self.skipped.insert(id.into(), guard.clone());
                    state.evidence.outcome = AcquisitionOutcome::Skipped {
                        guard: guard.clone(),
                    };
                } else {
                    self.next_widening_tier(id, &mut state);
                }
            }
        }
        self.widenings.insert(id.into(), state);
    }

    fn widening_expressions<'a>(
        &self,
        exprs: impl IntoIterator<Item = &'a Expr>,
    ) -> Result<Option<String>, Unresolved> {
        let mut refs = vec![];
        let mut reads = vec![];
        for e in exprs {
            crate::expr::expr_refs(e, &mut refs);
            quality::expr_reads(e, None, false, &mut reads);
        }
        let skip = self.acquisition_refs(&refs)?;
        if skip.is_none() {
            self.acquisition_quality(&reads)?;
        }
        Ok(skip)
    }

    fn build_widening_request(
        &self,
        id: &str,
        s: &mut State,
    ) -> Result<Option<String>, Unresolved> {
        let tier = s
            .decl
            .tiers
            .get(s.tier)
            .ok_or_else(|| bad_page("missing tier"))?;
        if let Guard::Predicate(p) = &tier.when {
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
        let mut request = s.decl.base_request.clone();
        request.extend(tier.request_patch.clone());
        for group in &tier.relax {
            if let Some(fields) = s.decl.groups.get(group) {
                for field in fields {
                    request.remove(field);
                }
            }
        }
        if let Some(guard) = self.widening_expressions(request.values())? {
            return Ok(Some(guard));
        }
        // Protected fields have the same hard-gate requirement as their compile-time reads.
        for group in &s.decl.protected {
            if let Some(fields) = s.decl.groups.get(group) {
                for field in fields {
                    if let Some(e) = request.get(field) {
                        let mut reads = vec![];
                        quality::expr_reads(e, None, true, &mut reads);
                        self.acquisition_quality(&reads)?;
                    }
                }
            }
        }
        let index = tier.source_index as usize;
        let op = s
            .compiled
            .operations
            .get(index)
            .ok_or_else(|| bad_page("missing operation"))?;
        let binding = s
            .compiled
            .plan
            .sources
            .get(index)
            .ok_or_else(|| bad_page("missing binding"))?;
        let mut constructed = false;
        let mut fields = vec![];
        for f in &op.request {
            fields.push(match request.get(&f.name) {
                None => RequestValue::Unset,
                Some(e) => {
                    let mut refs = vec![];
                    crate::expr::expr_refs(e, &mut refs);
                    constructed |= refs
                        .iter()
                        .any(|r| self.constructed.contains(&crate::expr::base_ref_v2(r)));
                    RequestValue::Set {
                        value: crate::expr::eval_expr(e, &View::scope(self))
                            .map_err(fault)?
                            .to_json(),
                    }
                }
            });
        }
        let mut pending = make_pending(
            id,
            0,
            AcquisitionRequest {
                schema: schema::ACQUISITION_REQUEST.into(),
                source: binding.source.id.clone(),
                operation: op.name.clone(),
                fields,
                page: RequestPage::First,
            },
            op.max_request_bytes,
            &binding.source.name,
            s.decl.limits.deadline_share,
        )?;
        pending.tier_index = s.tier as u32;
        s.pending = Some(pending);
        s.evidence.request_constructed |= constructed;
        if let Some(t) = s.evidence.tiers.get_mut(s.tier) {
            t.request_constructed = constructed;
        }
        Ok(None)
    }

    fn next_widening_tier(&mut self, id: &str, s: &mut State) {
        while s.tier < s.decl.tiers.len() {
            s.tier_start = s.values.len();
            match self.build_widening_request(id, s) {
                Ok(None) => return,
                Ok(Some(guard)) => {
                    s.skipped_guard.get_or_insert(guard);
                    if let Some(t) = s.evidence.tiers.get_mut(s.tier) {
                        t.outcome = TierOutcome::Skipped;
                    }
                }
                Err(u) => {
                    self.rollback_widening_tier(s, u);
                    if s.decl.on_tier_failure == OnTierFailure::Stop {
                        self.finish_widening(id, s, AcquisitionStop::TierFailure);
                        return;
                    }
                }
            }
            s.tier += 1;
        }
        self.finish_widening(id, s, AcquisitionStop::TiersExhausted);
    }

    fn rollback_widening_tier(&self, s: &mut State, reason: Unresolved) {
        if let Some(t) = s.evidence.tiers.get_mut(s.tier) {
            if let Some(p) = s.pending.take()
                && t.pages.len() == p.page_index as usize
            {
                t.pages.push(PageEvidence {
                    request: p.request_id,
                    digest: PageDigest::None,
                    items: 0,
                    unique_items_added: 0,
                });
            }
            t.outcome = TierOutcome::Failed { reason };
            t.unique_items_added = 0;
            t.duplicates = 0;
            for p in &mut t.pages {
                p.unique_items_added = 0;
            }
        }
        s.values.truncate(s.tier_start);
        s.evidence.items.truncate(s.tier_start);
        s.keys = s
            .values
            .iter()
            .filter_map(|v| pool_key(v, &s.decl.dedupe_key))
            .collect();
    }

    fn fail_widening_tier(&mut self, id: &str, s: &mut State, reason: Unresolved) {
        self.rollback_widening_tier(s, reason);
        if s.decl.on_tier_failure == OnTierFailure::Stop {
            self.finish_widening(id, s, AcquisitionStop::TierFailure);
        } else {
            s.tier += 1;
            self.next_widening_tier(id, s);
        }
    }

    pub(super) fn supply_widening_page(
        &mut self,
        id: &str,
        tier: u32,
        page: u32,
        bytes: &[u8],
    ) -> Result<(), SupplyError> {
        let pending = self.widening_pending(id, tier, page)?;
        let Some(mut s) = self.widenings.remove(id) else {
            return Err(not_pending(id));
        };
        if let Err(u) = self.accept_widening_page(id, &mut s, &pending, bytes) {
            self.fail_widening_tier(id, &mut s, u);
        }
        self.widenings.insert(id.into(), s);
        self.advance();
        Ok(())
    }

    fn widening_pending(
        &self,
        id: &str,
        tier: u32,
        page: u32,
    ) -> Result<PendingAcquisition, SupplyError> {
        self.widenings
            .get(id)
            .and_then(|s| s.pending.clone())
            .filter(|p| p.tier_index == tier && p.page_index == page)
            .ok_or_else(|| not_pending(id))
    }

    pub(super) fn supply_widening_failure(
        &mut self,
        id: &str,
        tier: u32,
        page: u32,
        reason: Unresolved,
    ) -> Result<(), SupplyError> {
        let pending = self.widening_pending(id, tier, page)?;
        let valid = match &reason {
            Unresolved::MissingEvidence { fields } => fields == &[id.to_string()],
            Unresolved::BackendUnavailable { detail } => detail
                .strip_prefix(&format!("source:{}", pending.source))
                .is_some_and(|tail| tail.is_empty() || tail.starts_with([':', ' '])),
            Unresolved::InvalidBackendOutput { .. } | Unresolved::DeadlineExceeded => true,
            Unresolved::BudgetExhausted { resource } => {
                matches!(resource.as_str(), "acquisition_calls" | "cost")
            }
            _ => false,
        };
        if !valid {
            return Err(SupplyError::NotARuntimeReason(reason.kind()));
        }
        let Some(mut s) = self.widenings.remove(id) else {
            return Err(not_pending(id));
        };
        self.fail_widening_tier(id, &mut s, reason);
        self.widenings.insert(id.into(), s);
        self.advance();
        Ok(())
    }

    fn accept_widening_page(
        &mut self,
        id: &str,
        s: &mut State,
        pending: &PendingAcquisition,
        bytes: &[u8],
    ) -> Result<(), Unresolved> {
        let tier = s
            .decl
            .tiers
            .get(s.tier)
            .ok_or_else(|| bad_page("missing tier"))?;
        let op = s
            .compiled
            .operations
            .get(tier.source_index as usize)
            .ok_or_else(|| bad_page("missing operation"))?;
        let binding = s
            .compiled
            .plan
            .sources
            .get(tier.source_index as usize)
            .ok_or_else(|| bad_page("missing binding"))?;
        let page = AcquisitionPage::parse(bytes).map_err(|e| bad_page(e.to_string()))?;
        validate_page(
            &page,
            &pending.request,
            op,
            self.now,
            DurationMs(s.decl.limits.as_of_skew_ms),
        )
        .map_err(|e| bad_page(format!("{e:?}")))?;
        let digest = ContentDigest::parse(&rustev_contract::canonical::tagged_digest(
            schema::ACQUISITION_PAGE,
            &page.canonical().map_err(|e| bad_page(e.to_string()))?,
        ))
        .map_err(|e| bad_page(e.to_string()))?;
        let t = s
            .evidence
            .tiers
            .get_mut(s.tier)
            .ok_or_else(|| bad_page("missing tier evidence"))?;
        t.items_received += page.items.len() as u64;
        t.pages.push(PageEvidence {
            request: pending.request_id.clone(),
            digest: PageDigest::Sha256(digest),
            items: page.items.len() as u64,
            unique_items_added: 0,
        });
        let age = self
            .now
            .as_ms()
            .saturating_sub(page.as_of_ms.as_ms())
            .max(0) as u64;
        let stale: Vec<_> = binding.map.iter().filter_map(|(pool, response)| {
            let f = op.item.iter().find(|f| &f.name == response)?;
            matches!(f.freshness, rustev_contract::acquisition::Freshness::MaxAgeMs { value } if age > value).then(|| format!("{id}/{pool}"))
        }).collect();
        if !stale.is_empty() {
            return Err(Unresolved::stale(stale));
        }
        let basis = Basis::Acquired { step: id.into() };
        for (rank, item) in page.items.iter().enumerate() {
            let mut fields = BTreeMap::new();
            let mut values = BTreeMap::new();
            for (pool, response) in &binding.map {
                let f = op
                    .item
                    .iter()
                    .find(|f| &f.name == response)
                    .ok_or_else(|| bad_page("missing mapped field"))?;
                let raw = item
                    .get(response)
                    .ok_or_else(|| bad_page("missing item field"))?;
                values.insert(
                    pool.clone(),
                    crate::value::decode_v2(&f.ty, raw).map_err(bad_page)?,
                );
                let mut meta = ValueMeta::ungraded(basis.clone());
                meta.grade = f.limit().into();
                meta.provenance = vec![f.provenance];
                meta.sources = vec![binding.source.name.clone()];
                meta.as_of_ms = AsOf::Ms(page.as_of_ms);
                fields.insert(pool.clone(), meta);
            }
            let key = values
                .get(&s.decl.dedupe_key)
                .ok_or_else(|| bad_page("missing dedupe key"))?;
            let key = canonical_value_bytes(&key.to_json()).map_err(|e| bad_page(e.to_string()))?;
            if s.keys.contains(&key) {
                t.duplicates += 1;
                continue;
            }
            if s.values.len() as u64 >= s.decl.limits.max_items {
                s.evidence.truncated += 1;
                continue;
            }
            s.keys.insert(key);
            let origin = ItemOrigin {
                tier: tier.name.clone(),
                tier_index: s.tier as u32,
                relaxation: tier.relax.len() as u64,
                source: binding.source.name.clone(),
                operation: op.name.clone(),
                page: pending.page_index,
                rank: rank as u64,
                as_of_ms: page.as_of_ms,
            };
            let mut item_meta = ValueMeta::combination(fields.values(), basis.clone());
            item_meta.fields = Some(fields);
            let mut meta = item_meta.clone();
            meta.fields = Some(BTreeMap::from([("item".into(), item_meta)]));
            let value = record([
                ("item", Value::Record(values)),
                ("origin", origin_value(&origin)),
            ]);
            let digest = ContentDigest::parse(&grading::digest(&value.to_json()))
                .map_err(|e| bad_page(e.to_string()))?;
            s.evidence.items.push(AcquiredItemEvidence {
                digest,
                meta,
                origin: Some(origin),
            });
            s.values.push(value);
            t.unique_items_added += 1;
            if let Some(p) = t.pages.last_mut() {
                p.unique_items_added += 1;
            }
        }
        t.outcome = if t.items_received == 0 {
            TierOutcome::Empty
        } else {
            TierOutcome::Ok
        };
        if s.stop.is_some_and(|n| s.values.len() as u64 >= n) {
            self.finish_widening(id, s, AcquisitionStop::StopCondition);
        } else if s.values.len() as u64 >= s.decl.limits.max_items {
            self.finish_widening(id, s, AcquisitionStop::ItemLimit);
        } else if page.next == NextPage::None
            || u64::from(pending.page_index) + 1 >= s.decl.limits.max_pages
        {
            s.pending = None;
            s.tier += 1;
            self.next_widening_tier(id, s);
        } else {
            let mut request = pending.request.clone();
            request.page = match page.next {
                NextPage::Cursor { text } => RequestPage::Cursor { text },
                NextPage::Offset { integer } => RequestPage::Offset { integer },
                NextPage::None => RequestPage::First,
            };
            let mut p = make_pending(
                id,
                pending.page_index + 1,
                request,
                op.max_request_bytes,
                &binding.source.name,
                s.decl.limits.deadline_share,
            )?;
            p.tier_index = s.tier as u32;
            s.pending = Some(p);
        }
        Ok(())
    }

    fn finish_widening(&mut self, id: &str, s: &mut State, stop: AcquisitionStop) {
        s.pending = None;
        s.evidence.stopped_by = stop;
        if s.evidence
            .tiers
            .iter()
            .all(|t| t.outcome == TierOutcome::Skipped)
        {
            let guard = s.skipped_guard.clone().unwrap_or_else(|| id.into());
            self.skipped.insert(id.into(), guard.clone());
            s.evidence.outcome = AcquisitionOutcome::Skipped { guard };
            return;
        }
        if s.evidence.truncated > 0 {
            self.notices.push(Notice {
                kind: NoticeKind::Truncation,
                subject: format!("step:{id}"),
                reason: format!("{} acquisition items not retained", s.evidence.truncated),
                action: None,
                screening: None,
            });
        }
        let failures: Vec<_> = s
            .evidence
            .tiers
            .iter()
            .filter_map(|t| match &t.outcome {
                TierOutcome::Failed { reason } => Some((t.name.clone(), reason.clone())),
                _ => None,
            })
            .collect();
        if stop != AcquisitionStop::StopCondition
            && let Some((_, reason)) = failures.first()
        {
            s.evidence.outcome = AcquisitionOutcome::Unresolved(reason.clone());
            self.exact.insert(id.into(), Err(reason.clone()));
            return;
        }
        for (name, _) in failures {
            self.notices.push(Notice {
                kind: NoticeKind::Fallback,
                subject: format!("step:{id}"),
                reason: format!("failed tier {name}"),
                action: None,
                screening: None,
            });
        }
        let tiers = s
            .evidence
            .tiers
            .iter()
            .map(|t| {
                record([
                    ("name", Value::Text(t.name.clone())),
                    (
                        "outcome",
                        Value::Text(
                            match t.outcome {
                                TierOutcome::Ok => "ok",
                                TierOutcome::Empty => "empty",
                                TierOutcome::Failed { .. } => "failed",
                                TierOutcome::Skipped => "skipped",
                                TierOutcome::NotNeeded => "not_needed",
                            }
                            .into(),
                        ),
                    ),
                    ("items_received", Value::Integer(t.items_received as i64)),
                    (
                        "unique_items_added",
                        Value::Integer(t.unique_items_added as i64),
                    ),
                    ("duplicates", Value::Integer(t.duplicates as i64)),
                    ("request_constructed", Value::Bool(t.request_constructed)),
                ])
            })
            .collect();
        let value = record([
            ("items", Value::List(s.values.clone())),
            ("tiers", Value::List(tiers)),
            (
                "stopped_by",
                Value::Enum(
                    match stop {
                        AcquisitionStop::StopCondition => "stop_condition",
                        AcquisitionStop::ItemLimit => "item_limit",
                        AcquisitionStop::TierFailure => "tier_failure",
                        _ => "tiers_exhausted",
                    }
                    .into(),
                ),
            ),
            ("truncated", Value::Integer(s.evidence.truncated as i64)),
            (
                "request_constructed",
                Value::Bool(s.evidence.request_constructed),
            ),
        ]);
        let mut list = ValueMeta::combination(
            s.evidence.items.iter().map(|i| &i.meta),
            Basis::Acquired { step: id.into() },
        );
        list.items = Some(s.evidence.items.iter().map(|i| i.meta.clone()).collect());
        let mut whole = list.clone();
        whole.items = None;
        whole.fields = Some(BTreeMap::from([("items".into(), list)]));
        self.metas.insert(
            format!("step:{id}"),
            Meta {
                node: whole.grade,
                whole,
            },
        );
        self.exact_values.insert(id.into(), value.clone());
        self.exact.insert(id.into(), Ok(value));
    }
}

fn record<const N: usize>(fields: [(&str, Value); N]) -> Value {
    Value::Record(fields.into_iter().map(|(k, v)| (k.into(), v)).collect())
}
fn origin_value(o: &ItemOrigin) -> Value {
    record([
        ("tier", Value::Text(o.tier.clone())),
        ("tier_index", Value::Integer(i64::from(o.tier_index))),
        ("relaxation", Value::Integer(o.relaxation as i64)),
        ("source", Value::Text(o.source.clone())),
        ("operation", Value::Text(o.operation.clone())),
        ("page", Value::Integer(i64::from(o.page))),
        ("rank", Value::Integer(o.rank as i64)),
        ("as_of_ms", Value::Timestamp(o.as_of_ms)),
    ])
}
fn pool_key(value: &Value, key: &str) -> Option<Vec<u8>> {
    let Value::Record(wrapper) = value else {
        return None;
    };
    let Value::Record(item) = wrapper.get("item")? else {
        return None;
    };
    canonical_value_bytes(&item.get(key)?.to_json()).ok()
}
