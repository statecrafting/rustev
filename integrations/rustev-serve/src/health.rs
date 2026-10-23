//! Host-reported callback health. Reports are operator state, never evidence.
use rustev_contract::scope::Handle;
use rustev_contract::serve::{BackendKind, ReadinessFailure, ReadinessReason, ServeConfig, Sink};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ComponentKind {
    Backend,
    Context,
    Sink,
}
impl ComponentKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Backend => "backend",
            Self::Context => "context",
            Self::Sink => "sink",
        }
    }
}
/// A host adapter reports these after its describe and callback operations.
/// The service does not infer successful describe from installation of an object.
#[derive(Clone, Copy, Debug, Default)]
pub struct ComponentHealth {
    pub described: bool,
    pub degraded: bool,
    pub invalidation_degraded: bool,
    pub queue_depth: u64,
    pub refusals: u64,
    pub late_responses_discarded: u64,
    pub invalidation_lag_ms: u64,
    pub invalidation_gaps: u64,
}
pub(crate) struct Item {
    pub health: ComponentHealth,
    pub readiness_affecting: bool,
    pub queue_capacity: u64,
}
pub(crate) type Health = BTreeMap<(ComponentKind, Handle), Item>;
pub(crate) fn configured(config: &ServeConfig) -> Health {
    let mut items = Health::new();
    for (name, binding) in &config.backend_bindings {
        let (affecting, capacity, described) = match &binding.backend {
            BackendKind::Compiled { .. } => (true, 0, true),
            BackendKind::Remote { endpoint, .. } => (
                endpoint.readiness_affecting,
                endpoint.queue_capacity.get(),
                false,
            ),
        };
        items.insert(
            (ComponentKind::Backend, name.clone()),
            Item {
                health: ComponentHealth {
                    described,
                    ..Default::default()
                },
                readiness_affecting: affecting,
                queue_capacity: capacity,
            },
        );
    }
    for (name, endpoint) in &config.context_sources {
        items.insert(
            (ComponentKind::Context, name.clone()),
            Item {
                health: ComponentHealth::default(),
                readiness_affecting: endpoint.readiness_affecting,
                queue_capacity: endpoint.queue_capacity.get(),
            },
        );
    }
    for (name, sink) in &config.sinks {
        let (Sink::Remote(endpoint) | Sink::Opentelemetry { endpoint, .. }) = sink;
        items.insert(
            (ComponentKind::Sink, name.clone()),
            Item {
                health: ComponentHealth::default(),
                readiness_affecting: endpoint.readiness_affecting,
                queue_capacity: endpoint.queue_capacity.get(),
            },
        );
    }
    items
}
pub(crate) fn failures(items: &Health) -> Vec<ReadinessFailure> {
    items
        .iter()
        .filter_map(|((_, name), item)| {
            let health = item.health;
            let reason = if !health.described {
                ReadinessReason::DescribeIncomplete
            } else if health.invalidation_degraded {
                ReadinessReason::InvalidationDegraded
            } else if health.degraded && item.readiness_affecting {
                ReadinessReason::Degraded
            } else {
                return None;
            };
            Some(ReadinessFailure {
                item: name.clone(),
                reason,
            })
        })
        .collect()
}
pub(crate) fn metrics(items: &Health, out: &mut String) {
    for ((kind, name), item) in items {
        // JSON escaping also escapes the characters forbidden in label values.
        let name = serde_json::to_string(name.as_str()).unwrap();
        let h = item.health;
        for (metric, value) in [
            ("callback_described", u64::from(h.described)),
            ("callback_degraded", u64::from(h.degraded)),
            ("callback_queue_depth", h.queue_depth),
            ("callback_refusals_total", h.refusals),
            (
                "callback_late_responses_discarded_total",
                h.late_responses_discarded,
            ),
            ("invalidation_degraded", u64::from(h.invalidation_degraded)),
            ("invalidation_lag_ms", h.invalidation_lag_ms),
            ("invalidation_gaps_total", h.invalidation_gaps),
        ] {
            out.push_str(&format!(
                "rustev_{metric}{{kind=\"{}\",name={name}}} {value}\n",
                kind.label()
            ));
        }
    }
}
