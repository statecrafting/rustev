//! Pushed context invalidations (spec 042, 3.10.6): `POST
//! /v1/context/invalidations` takes `rustev.serve-invalidations/1` and
//! answers `rustev.serve-invalidations-ack/1` with the highest sequence
//! number applied.
//!
//! Each item is applied before it is acknowledged, as spec 017
//! invalidation: `invalidate_scope` for a named principal, `invalidate_plan`
//! for `all_scopes`, for every registered plan reading a named field. The
//! three kinds are applied alike. A batch that is not contiguous with the
//! last applied item (and the first batch after startup, which has nothing
//! to be contiguous with) invalidates every plan reading any of the
//! source's fields first; a gap marks the source `invalidation_degraded`
//! and is counted, and the next contiguous batch clears it. Items at or
//! below the last applied sequence were applied already and are skipped. An
//! erasure applied while a response for its scope is retained is counted.
//!
//! A `poll` source is polled by [`crate::contexts`]: the first poll sets the
//! baseline; a poll failure, or lag beyond `max_lag_ms`, invalidates every
//! plan reading the source, marks it degraded and needs a fresh baseline.
use crate::{
    Reply, Service,
    health::ComponentKind,
    service::{lock, scope_key},
};
use rustev_contract::{
    Document,
    remote_context::InvalidationSupport,
    serve::{
        InvalidationItem, InvalidationKind, InvalidationScope, InvalidationsAckSchema, RefusalCode,
        ServeInvalidations, ServeInvalidationsAck,
    },
};
use std::sync::atomic::Ordering;

/// What the service knows about one configured source's invalidation.
#[derive(Debug, Clone)]
pub struct Source {
    fields: Vec<String>,
    push: bool,
    last: Option<u64>,
    degraded: bool,
    gaps: u64,
    lag_ms: u64,
}

/// One item, whichever way it arrived.
pub(crate) struct Item {
    pub sequence: u64,
    pub erasure: bool,
    pub fields: Vec<String>,
    /// The principal handle, or `None` for every scope.
    pub principal: Option<Vec<u8>>,
}

impl Service {
    /// Record a described source's fields and invalidation terms; the
    /// remote context client calls this after describe. Unknown names are
    /// refused. A `poll` source is `invalidation_degraded` until its
    /// baseline poll succeeds.
    pub fn describe_context_invalidation(
        &self,
        name: &str,
        fields: Vec<String>,
        invalidation: &InvalidationSupport,
    ) -> Result<(), &'static str> {
        if !self
            .config
            .context_sources
            .keys()
            .any(|k| k.as_str() == name)
        {
            return Err("unknown configured context source");
        }
        let source = Source {
            fields,
            push: matches!(invalidation, InvalidationSupport::Push),
            last: None,
            degraded: matches!(invalidation, InvalidationSupport::Poll { .. }),
            gaps: 0,
            lag_ms: 0,
        };
        let health = (source.degraded, 0, 0);
        lock(&self.invalidation).insert(name.into(), source);
        self.report_invalidation(name, health);
        Ok(())
    }

    /// The handler of `POST /v1/context/invalidations` (admin group).
    pub fn invalidations(&self, bytes: &[u8]) -> Reply {
        let Ok(batch) = ServeInvalidations::parse(bytes) else {
            return self.refuse(RefusalCode::InvalidDocument);
        };
        let name = batch.source_id.as_str();
        match lock(&self.invalidation).get(name) {
            None => return self.refuse(RefusalCode::ContextUnavailable),
            Some(s) if !s.push => return self.refuse(RefusalCode::InvalidRequest),
            Some(_) => {}
        }
        let items = batch
            .items
            .as_slice()
            .iter()
            .map(|i: &InvalidationItem| Item {
                sequence: i.sequence,
                erasure: i.kind == InvalidationKind::Erasure,
                fields: i
                    .fields
                    .as_slice()
                    .iter()
                    .map(|f| f.as_str().into())
                    .collect(),
                principal: match &i.scope {
                    InvalidationScope::AllScopes => None,
                    InvalidationScope::Principal(p) => Some(p.as_slice().to_vec()),
                },
            })
            .collect();
        let highest = self.apply_invalidations(name, items, None);
        Reply::document(
            200,
            &ServeInvalidationsAck {
                schema: InvalidationsAckSchema::V1,
                highest_sequence: highest,
            },
        )
    }

    /// Apply items in sequence order and return the highest sequence
    /// applied. `current`, from a poll answer, is the source's sequence
    /// after these items: beyond the last item it means items were dropped.
    pub(crate) fn apply_invalidations(
        &self,
        name: &str,
        items: Vec<Item>,
        current: Option<u64>,
    ) -> u64 {
        let mut sources = lock(&self.invalidation);
        let Some(source) = sources.get_mut(name) else {
            return 0;
        };
        let items: Vec<Item> = items
            .into_iter()
            .filter(|i| source.last.is_none_or(|l| i.sequence > l))
            .collect();
        let expected_last = items.last().map(|i| i.sequence).or(source.last);
        let dropped = current.is_some_and(|c| expected_last.is_some_and(|l| c > l));
        if let Some(first) = items.first() {
            let contiguous = source.last.is_some_and(|l| first.sequence == l + 1);
            if !contiguous {
                // Nothing to be contiguous with, or a gap: every cached value
                // from this source may be stale.
                self.invalidate_fields(&source.fields, None);
                if source.last.is_some() {
                    source.gaps += 1;
                    source.degraded = true;
                }
            } else if !dropped {
                source.degraded = false;
            }
            for item in &items {
                self.invalidate_fields(&item.fields, item.principal.as_deref());
                if item.erasure && self.retains(item.principal.as_deref()) {
                    self.erasures_while_retained.fetch_add(1, Ordering::Relaxed);
                }
                source.last = Some(item.sequence);
            }
        }
        if dropped {
            self.invalidate_fields(&source.fields, None);
            source.gaps += 1;
            source.degraded = true;
            source.last = current;
        }
        let health = (source.degraded, source.gaps, source.lag_ms);
        let highest = source.last.unwrap_or(0);
        drop(sources);
        self.report_invalidation(name, health);
        highest
    }

    /// The last applied sequence of a polled source, `None` before its
    /// baseline.
    pub(crate) fn invalidation_sequence(&self, name: &str) -> Option<u64> {
        lock(&self.invalidation).get(name).and_then(|s| s.last)
    }

    /// A baseline poll succeeded: nothing cached can be older than it, so
    /// the source is current at `sequence`.
    pub(crate) fn invalidation_baseline(&self, name: &str, sequence: u64) {
        let mut sources = lock(&self.invalidation);
        let Some(source) = sources.get_mut(name) else {
            return;
        };
        source.last = Some(sequence);
        source.degraded = false;
        let health = (source.degraded, source.gaps, source.lag_ms);
        drop(sources);
        self.report_invalidation(name, health);
    }

    /// A poll failed or lag passed `max_lag_ms`: invalidate everything from
    /// the source, mark it degraded and require a fresh baseline.
    pub(crate) fn invalidation_lost(&self, name: &str, lag_ms: u64, count: bool) {
        let mut sources = lock(&self.invalidation);
        let Some(source) = sources.get_mut(name) else {
            return;
        };
        self.invalidate_fields(&source.fields, None);
        if count {
            source.gaps += 1;
        }
        source.degraded = true;
        source.last = None;
        source.lag_ms = lag_ms;
        let health = (source.degraded, source.gaps, source.lag_ms);
        drop(sources);
        self.report_invalidation(name, health);
    }

    /// Record the time since the last successful poll.
    pub(crate) fn invalidation_lag(&self, name: &str, lag_ms: u64) {
        let mut sources = lock(&self.invalidation);
        let Some(source) = sources.get_mut(name) else {
            return;
        };
        source.lag_ms = lag_ms;
        let health = (source.degraded, source.gaps, source.lag_ms);
        drop(sources);
        self.report_invalidation(name, health);
    }

    fn report_invalidation(&self, name: &str, (degraded, gaps, lag): (bool, u64, u64)) {
        if let Ok(handle) = rustev_contract::scope::Handle::new(name) {
            let _ = self.update_component_health(ComponentKind::Context, &handle, |h| {
                h.invalidation_degraded = degraded;
                h.invalidation_gaps = gaps;
                h.invalidation_lag_ms = lag;
            });
        }
    }

    /// Invalidate, for every registered plan reading one of `fields`, the
    /// named scope or (with `None`) the plan's whole namespace.
    fn invalidate_fields(&self, fields: &[String], principal: Option<&[u8]>) {
        let plans: Vec<_> = lock(&self.state)
            .plans
            .values()
            .filter(|r| {
                r.plan
                    .compiled()
                    .plan
                    .definition
                    .inputs
                    .iter()
                    .any(|i| fields.contains(&i.name))
            })
            .cloned()
            .collect();
        for registration in plans {
            match principal {
                Some(p) => {
                    self.runtime.invalidate_scope(&registration.plan, p);
                }
                None => {
                    self.runtime.invalidate_plan(&registration.plan);
                }
            }
        }
    }

    /// Whether a response is retained for the scope (any scope for `None`).
    fn retains(&self, principal: Option<&[u8]>) -> bool {
        let state = lock(&self.state);
        match principal {
            None => !state.entries.is_empty(),
            Some(p) => {
                let (digest, _) = scope_key(p, "");
                state.entries.keys().any(|(d, _)| *d == digest)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    // The handler is exercised through the HTTP surface in
    // tests/handlers.rs; this checks the parse-level refusal only.
    use rustev_contract::Document;
    use rustev_contract::serve::ServeInvalidations;

    #[test]
    fn a_decreasing_batch_does_not_parse() {
        let body = br#"{"schema":"rustev.serve-invalidations/1","source_id":"s","items":[
            {"schema":"rustev.remote-context-invalidation/1","source_id":"s","sequence":2,"kind":"correction","fields":["f"],"scope":"all_scopes"},
            {"schema":"rustev.remote-context-invalidation/1","source_id":"s","sequence":1,"kind":"correction","fields":["f"],"scope":"all_scopes"}]}"#;
        assert!(ServeInvalidations::parse(body).is_err());
    }
}
