//! Configured remote context sources of the standalone service (spec 042,
//! 3.10 and 3.12): one `rustev.remote-context/1` client per entry of
//! `context_sources`, whose key is the source id the endpoint describes.
//!
//! Each client describes its endpoint once, at startup. A source whose
//! describe fails (unreachable, unknown version, undescribed source, another
//! endpoint class) is installed as unavailable: every fetch from it fails
//! `context_unavailable`, it is reported not described, and readiness
//! reflects that when the endpoint is readiness-affecting. Queue depth,
//! refusals and late discards are reported to the health table.
use crate::invalidations::Item;
use crate::{
    Service,
    context_client::{ContextClientConfig, RemoteContextClient},
    health::ComponentKind,
};
use rustev_contract::remote_context::{InvalidationKind, InvalidationScope, InvalidationSupport};
use rustev_contract::{scope::Handle, serve::ServeConfig, snapshot::Snapshot};
use rustev_core::seams::{BoxFuture, CallContext, ContextSource};
use std::{
    collections::BTreeMap,
    sync::{Arc, Weak},
    time::Duration,
};

// Engineering choices where 3.10.2 states a value without fixing it; the
// configuration document has no member for them.
/// The transit margin subtracted from the remaining budget sent to a source.
pub const TRANSIT_MARGIN_MS: u64 = 25;
/// The principal reference's mapping version: the reference is the
/// caller's principal handle, unchanged.
pub const PRINCIPAL_MAPPING_VERSION: &str = "rustev-serve/principal-handle/1";
/// The principal reference's declared scope.
pub const PRINCIPAL_SCOPE: &str = "rustev-serve";
/// The budget of each source's describe exchange at startup.
pub const DESCRIBE_BUDGET_MS: u64 = 10_000;
/// How often queue counters are reported to the health table.
pub const REPORT_INTERVAL_MS: u64 = 1_000;

/// A configured source whose describe failed at startup.
struct Unavailable;

impl ContextSource for Unavailable {
    fn fetch<'a>(
        &'a self,
        _fields: &'a [String],
        _cx: &'a CallContext,
    ) -> BoxFuture<'a, Result<Snapshot, String>> {
        Box::pin(async { Err("remote:capability: context source was not described".into()) })
    }
}

/// The installed sources and, per configured name, the client when its
/// describe succeeded.
pub struct Connected {
    pub sources: BTreeMap<String, Arc<dyn ContextSource>>,
    clients: Vec<(Handle, Option<(RemoteContextClient, u64)>)>,
}

/// Describe every configured source. Only an unreadable credential file or
/// an invalid address or bound is a startup error.
pub async fn connect(config: &ServeConfig) -> Result<Connected, String> {
    let mut connected = Connected {
        sources: BTreeMap::new(),
        clients: Vec::new(),
    };
    for (name, endpoint) in &config.context_sources {
        let transport = crate::sinks::transport(endpoint)?;
        let client = RemoteContextClient::connect(
            transport,
            ContextClientConfig {
                source_id: name.as_str().into(),
                in_flight: endpoint.max_in_flight.get() as u32,
                queue_capacity: endpoint.queue_capacity.get() as u32,
                transit_margin_ms: TRANSIT_MARGIN_MS,
                describe_budget_ms: DESCRIBE_BUDGET_MS,
                mapping_version: PRINCIPAL_MAPPING_VERSION.into(),
                principal_scope: PRINCIPAL_SCOPE.into(),
            },
            None,
        )
        .await;
        let installed: Arc<dyn ContextSource> = match &client {
            Ok(c) => Arc::new(c.clone()),
            Err(_) => Arc::new(Unavailable),
        };
        connected.sources.insert(name.as_str().into(), installed);
        connected.clients.push((
            name.clone(),
            client.ok().map(|c| (c, endpoint.max_in_flight.get())),
        ));
    }
    Ok(connected)
}

fn report(service: &Service, name: &Handle, client: &Option<(RemoteContextClient, u64)>) {
    let _ = service.update_component_health(ComponentKind::Context, name, |h| {
        h.described = client.is_some();
        if let Some((c, in_flight)) = client {
            let counters = c.counters();
            // The health table bounds queued calls, not running ones.
            h.queue_depth = counters.depth.saturating_sub(*in_flight);
            h.refusals = counters.refused;
            h.late_responses_discarded = counters.late_responses_discarded;
        }
    });
}

/// Report each source's state now, then its counters periodically while
/// the service lives.
pub fn start_reporting(service: &Arc<Service>, connected: Connected) {
    for (name, client) in &connected.clients {
        if let Some((c, _)) = client {
            let source = c.source();
            let fields = source.fields.iter().map(|f| f.name.clone()).collect();
            let _ = service.describe_context_invalidation(
                name.as_str(),
                fields,
                &source.terms.invalidation,
            );
            if let InvalidationSupport::Poll {
                interval_ms,
                max_lag_ms,
            } = source.terms.invalidation
            {
                tokio::spawn(poll(
                    Arc::downgrade(service),
                    name.as_str().to_string(),
                    c.clone(),
                    interval_ms.max(1),
                    max_lag_ms,
                ));
            }
        }
        report(service, name, client);
    }
    let weak: Weak<Service> = Arc::downgrade(service);
    let clients = connected.clients;
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_millis(REPORT_INTERVAL_MS));
        loop {
            tick.tick().await;
            let Some(service) = weak.upgrade() else {
                return;
            };
            for (name, client) in &clients {
                report(&service, name, client);
            }
        }
    });
}

/// Poll a `poll` source's invalidations every `interval_ms` while the
/// service lives (3.10.6).
async fn poll(
    service: Weak<Service>,
    name: String,
    client: RemoteContextClient,
    interval_ms: u64,
    max_lag_ms: u64,
) {
    let mut tick = tokio::time::interval(Duration::from_millis(interval_ms));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_ok = tokio::time::Instant::now();
    let mut lagging = false;
    loop {
        tick.tick().await;
        let Some(service) = service.upgrade() else {
            return;
        };
        let sequence = service.invalidation_sequence(&name);
        match client.invalidations(sequence, interval_ms.max(1_000)).await {
            Ok(answer) => {
                last_ok = tokio::time::Instant::now();
                lagging = false;
                service.invalidation_lag(&name, 0);
                match sequence {
                    None => service.invalidation_baseline(&name, answer.sequence),
                    Some(_) => {
                        let items = answer
                            .items
                            .into_iter()
                            .filter(|i| i.source_id == name)
                            .map(|i| Item {
                                sequence: i.sequence,
                                erasure: i.kind == InvalidationKind::Erasure,
                                fields: i.fields,
                                principal: match i.scope {
                                    InvalidationScope::AllScopes => None,
                                    // Only this service's own mapping is a handle.
                                    InvalidationScope::Principal(p)
                                        if p.mapping_version == PRINCIPAL_MAPPING_VERSION =>
                                    {
                                        p.reference_bytes()
                                    }
                                    InvalidationScope::Principal(_) => None,
                                },
                            })
                            .collect();
                        service.apply_invalidations(&name, items, Some(answer.sequence));
                    }
                }
            }
            Err(_) => {
                let lag = last_ok.elapsed().as_millis() as u64;
                // A failed poll is a gap once per outage.
                service.invalidation_lost(&name, lag, sequence.is_some());
            }
        }
        let lag = last_ok.elapsed().as_millis() as u64;
        if lag > max_lag_ms && !lagging {
            lagging = true;
            service.invalidation_lost(&name, lag, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::callback_server::{CallbackConfig, CallbackServer, HostedContext};
    use rustev_contract::Document;
    use rustev_contract::definition::{ProvenanceClass, TypeDecl};
    use rustev_contract::remote_context::{ContextField, Freshness, Staleness};
    use rustev_contract::time::DurationMs;

    const CONFIG: &[u8] = include_bytes!("../tests/fixtures/config.json");
    const SNAPSHOT: &[u8] =
        include_bytes!("../../rustev-otel/tests/fixtures/support.snapshot.json");

    struct Fixture;
    impl ContextSource for Fixture {
        fn fetch<'a>(
            &'a self,
            fields: &'a [String],
            _cx: &'a CallContext,
        ) -> BoxFuture<'a, Result<Snapshot, String>> {
            Box::pin(async move {
                let mut s = Snapshot::parse(SNAPSHOT).unwrap();
                s.entries.retain(|e| fields.contains(&e.field));
                Ok(s)
            })
        }
    }

    fn config(url: &str) -> ServeConfig {
        let mut value: serde_json::Value = serde_json::from_slice(CONFIG).unwrap();
        let endpoint = serde_json::json!({
            "address": url,
            "credentials_file": "none",
            "max_in_flight": 2,
            "queue_capacity": 2,
            "readiness_affecting": true
        });
        value["context_sources"] =
            serde_json::json!({ "support": endpoint.clone(), "billing": endpoint });
        ServeConfig::parse(&serde_json::to_vec(&value).unwrap()).unwrap()
    }

    #[tokio::test]
    async fn described_sources_fetch_and_undescribed_ones_are_unavailable() {
        let server = CallbackServer::new(CallbackConfig::default());
        server
            .host_context(
                "support",
                HostedContext {
                    source: Arc::new(Fixture),
                    fields: vec![ContextField {
                        name: "ticket.message".into(),
                        ty: TypeDecl::Text { max_bytes: 1024 },
                        provenance: ProvenanceClass::UserSupplied,
                        freshness: Freshness::Observation {
                            staleness: Staleness::Unknown,
                        },
                        grade: None,
                    }],
                },
            )
            .unwrap();
        let handle = server.serve_loopback().await.unwrap();
        let connected = connect(&config(&handle.url())).await.unwrap();
        let described: Vec<_> = connected
            .clients
            .iter()
            .map(|(n, c)| (n.as_str().to_string(), c.is_some()))
            .collect();
        assert_eq!(
            described,
            [
                ("billing".to_string(), false),
                ("support".to_string(), true)
            ]
        );
        let cx = CallContext {
            remaining_ms: DurationMs(2_000),
            trace_id: "d1".into(),
            principal_handle: b"user-7".to_vec(),
        };
        let want = vec!["ticket.message".to_string()];
        let snapshot = connected.sources["support"]
            .fetch(&want, &cx)
            .await
            .unwrap();
        assert_eq!(snapshot.entries.len(), 1);
        let error = connected.sources["billing"]
            .fetch(&want, &cx)
            .await
            .unwrap_err();
        assert!(error.starts_with("remote:capability"), "{error}");
    }

    fn metric(service: &Service, metric: &str) -> String {
        let body = String::from_utf8(service.metrics().body).unwrap();
        body.lines()
            .find(|l| {
                l.starts_with(&format!(
                    "rustev_{metric}{{kind=\"context\",name=\"support\"}}"
                ))
            })
            .and_then(|l| l.rsplit(' ').next())
            .unwrap_or_default()
            .to_string()
    }

    async fn until(mut cond: impl FnMut() -> bool) {
        for _ in 0..400 {
            if cond() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("condition never held");
    }

    #[tokio::test]
    async fn a_polled_source_sets_its_baseline_applies_items_and_degrades_on_failure() {
        use rustev_contract::remote_context::{InvalidationKind, InvalidationScope};
        let server = CallbackServer::new(CallbackConfig {
            invalidation: InvalidationSupport::Poll {
                interval_ms: 20,
                max_lag_ms: 60_000,
            },
            ..CallbackConfig::default()
        });
        server
            .host_context(
                "support",
                HostedContext {
                    source: Arc::new(Fixture),
                    fields: vec![],
                },
            )
            .unwrap();
        let handle = server.serve_loopback().await.unwrap();
        let mut value: serde_json::Value = serde_json::from_slice(CONFIG).unwrap();
        value["context_sources"] = serde_json::json!({ "support": {
            "address": handle.url(), "credentials_file": "none",
            "max_in_flight": 1, "queue_capacity": 1, "readiness_affecting": true
        }});
        let config = ServeConfig::parse(&serde_json::to_vec(&value).unwrap()).unwrap();
        let service = crate::sinks::standalone(config).await.unwrap();
        until(|| service.invalidation_sequence("support") == Some(0)).await;
        assert_eq!(metric(&service, "invalidation_degraded"), "0");
        for _ in 0..2 {
            server
                .publish_invalidation(
                    "support",
                    InvalidationKind::Correction,
                    vec!["ticket.message".into()],
                    InvalidationScope::AllScopes,
                )
                .unwrap();
        }
        until(|| service.invalidation_sequence("support") == Some(2)).await;
        assert_eq!(metric(&service, "invalidation_gaps_total"), "0");
        drop(handle);
        until(|| metric(&service, "invalidation_degraded") == "1").await;
        assert_eq!(metric(&service, "invalidation_gaps_total"), "1");
        assert_eq!(service.invalidation_sequence("support"), None);
    }
}
