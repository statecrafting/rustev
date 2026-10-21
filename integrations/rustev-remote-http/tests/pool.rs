//! Spec 020, 3.5: bounded, isolation-keyed connection reuse over loopback.
//! A counting TCP front between client and server shows which exchanges
//! shared a connection; it can also drop a request after its bytes arrived
//! or close every connection. Limits, lifetimes and evictions are also
//! covered deterministically by the pool's unit tests. SYNTHETIC only;
//! nothing leaves 127.0.0.1.

mod common;

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use common::*;
use rustev_contract::remote::{ConnectionReuse, EvictionReason, RemoteCode};
use rustev_contract::run::Charge;
use rustev_core::seams::{
    AttemptCall, AttemptReport, CallContext, CancelSignal, DecisionBackend, RemoteEnd,
};
use rustev_remote_http::client::{ClientConfig, RemoteClient};
use rustev_remote_http::http::{HttpPool, Secret};
use rustev_remote_http::pool::{OnExhaustion, PoolPolicy};
use rustev_remote_http::principal::{EndpointOperator, HandleAsReference, PrincipalPosture};
use rustev_remote_http::server::ServerConfig;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;

/// A TCP front: counts client connections, forwards bytes, and on request
/// drops the next client chunk (closing the connection) or closes every
/// connection.
struct Front {
    addr: SocketAddr,
    accepted: Arc<AtomicUsize>,
    drop_next: Arc<AtomicBool>,
    close_all: Arc<Notify>,
}

impl Front {
    fn url(&self) -> String {
        format!("http://{}", self.addr)
    }
    fn accepted(&self) -> usize {
        self.accepted.load(Ordering::SeqCst)
    }
}

async fn front(target: SocketAddr) -> Front {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let accepted = Arc::new(AtomicUsize::new(0));
    let drop_next = Arc::new(AtomicBool::new(false));
    let close_all = Arc::new(Notify::new());
    let (a, d, c) = (accepted.clone(), drop_next.clone(), close_all.clone());
    tokio::spawn(async move {
        loop {
            let Ok((client, _)) = listener.accept().await else {
                continue;
            };
            a.fetch_add(1, Ordering::SeqCst);
            let (d, c) = (d.clone(), c.clone());
            tokio::spawn(async move {
                let Ok(server) = TcpStream::connect(target).await else {
                    return;
                };
                let (mut cr, mut cw) = client.into_split();
                let (mut sr, mut sw) = server.into_split();
                let up = async {
                    let mut buf = vec![0u8; 65_536];
                    loop {
                        let n = match cr.read(&mut buf).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => n,
                        };
                        if d.swap(false, Ordering::SeqCst) {
                            // The request's bytes arrived; nothing answers.
                            return;
                        }
                        if sw.write_all(&buf[..n]).await.is_err() {
                            return;
                        }
                    }
                };
                let down = async {
                    let mut buf = vec![0u8; 65_536];
                    loop {
                        let n = match sr.read(&mut buf).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => n,
                        };
                        if cw.write_all(&buf[..n]).await.is_err() {
                            return;
                        }
                    }
                };
                tokio::select! {
                    _ = up => {}
                    _ = down => {}
                    _ = c.notified() => {}
                }
            });
        }
    });
    Front {
        addr,
        accepted,
        drop_next,
        close_all,
    }
}

fn policy() -> PoolPolicy {
    PoolPolicy {
        max_connections: 8,
        max_per_origin: 8,
        max_streams: 1,
        max_idle: 8,
        idle_lifetime_ms: 60_000,
        max_lifetime_ms: 600_000,
        max_uses: 100,
        on_exhaustion: OnExhaustion::Refuse,
    }
}

async fn topic_backend(
    config: ServerConfig,
) -> (
    rustev_remote_http::server::RemoteServer,
    rustev_remote_http::server::ServerHandle,
) {
    let s = scripted_head(|c| ok(c.task(), 1));
    serve(config, vec![hosted(Handle(s))]).await
}

fn pooled(url: &str, pool: &Arc<HttpPool>) -> ClientConfig {
    let mut c = client_config(url, "synthetic-linear-head");
    c.pool = Some(pool.clone());
    c
}

async fn ok_call(c: &RemoteClient, id: &str) -> AttemptReport {
    let r = call(c, id, &topic_projection(), 10_000, &CancelSignal::new()).await;
    assert!(r.result.is_ok(), "{r:?}");
    r
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_a_pool_every_exchange_opens_its_own_connection() {
    let (_s, h) = topic_backend(ServerConfig::default()).await;
    let f = front(h.addr).await;
    let (c, sink) = connect(client_config(&f.url(), "synthetic-linear-head")).await;
    ok_call(&c, "n/1").await;
    ok_call(&c, "n/2").await;
    assert_eq!(f.accepted(), 3, "describe and two exchanges");
    assert!(sink.records().iter().all(|r| r.connection.is_none()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pooled_connection_carries_later_exchanges_under_the_same_key() {
    let (_s, h) = topic_backend(ServerConfig::default()).await;
    let f = front(h.addr).await;
    let pool = HttpPool::new(policy()).unwrap();
    let (off, _) = connect(client_config(&f.url(), "synthetic-linear-head")).await;
    let (c, sink) = connect(pooled(&f.url(), &pool)).await;
    assert_ne!(c.binding(), off.binding(), "the pool policy is identity");
    let before = f.accepted();
    for n in 1..=3 {
        ok_call(&c, &format!("r/{n}")).await;
    }
    assert_eq!(
        f.accepted() - before,
        1,
        "one connection for three exchanges"
    );
    let recs = sink.until_records(3).await;
    let conns: Vec<_> = recs.iter().map(|r| r.connection.clone().unwrap()).collect();
    assert_eq!(conns[0].reuse, ConnectionReuse::Fresh);
    assert_eq!(
        conns.iter().map(|c| c.use_count).collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert!(
        conns[1..]
            .iter()
            .all(|c| c.reuse == ConnectionReuse::Reused)
    );
    assert!(conns.iter().all(|c| c.instance_id == conns[0].instance_id
        && c.key_digest == conns[0].key_digest
        && c.eviction.is_none()));
    assert!(conns[0].key_digest.starts_with("sha256:"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn max_uses_evicts_and_the_next_exchange_opens_a_fresh_connection() {
    let (_s, h) = topic_backend(ServerConfig::default()).await;
    let f = front(h.addr).await;
    let mut p = policy();
    p.max_uses = 2;
    let pool = HttpPool::new(p).unwrap();
    let (c, sink) = connect(pooled(&f.url(), &pool)).await;
    let before = f.accepted();
    for n in 1..=3 {
        ok_call(&c, &format!("u/{n}")).await;
    }
    assert_eq!(f.accepted() - before, 2);
    let recs = sink.until_records(3).await;
    assert_eq!(
        recs[1].connection.as_ref().unwrap().eviction,
        Some(EvictionReason::MaxUses)
    );
    assert_eq!(
        recs[2].connection.as_ref().unwrap().reuse,
        ConnectionReuse::Fresh
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn credentials_never_share_a_connection_and_identical_bindings_may() {
    let (_s, h) = topic_backend(ServerConfig::default()).await;
    let f = front(h.addr).await;
    let pool = HttpPool::new(policy()).unwrap();
    let with = |token: &str| {
        let mut cfg = pooled(&f.url(), &pool);
        cfg.bearer = Some(Secret::new(token));
        cfg
    };
    let (a, sa) = connect(with("SYNTHETIC-A")).await;
    let (b, sb) = connect(with("SYNTHETIC-B")).await;
    let (a2, sa2) = connect(with("SYNTHETIC-A")).await;
    let before = f.accepted();
    ok_call(&a, "k/1").await;
    ok_call(&b, "k/2").await;
    assert_eq!(
        f.accepted() - before,
        2,
        "another credential, another connection"
    );
    // Credential rotation back to A, and another client with A's exact
    // binding, reuse A's connection.
    ok_call(&a2, "k/3").await;
    ok_call(&a, "k/4").await;
    assert_eq!(f.accepted() - before, 2);
    let ka = sa.until_records(1).await[0].connection.clone().unwrap();
    let kb = sb.until_records(1).await[0].connection.clone().unwrap();
    let ka2 = sa2.until_records(1).await[0].connection.clone().unwrap();
    assert_ne!(ka.key_digest, kb.key_digest);
    assert_eq!(ka2.key_digest, ka.key_digest);
    assert_eq!(ka2.reuse, ConnectionReuse::Reused);
    // The key never holds the credential.
    assert!(!ka.key_digest.contains("SYNTHETIC"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn privacy_options_and_endpoint_class_are_key_members() {
    let terms_server = ServerConfig::default();
    let s = scripted_head(|c| ok(c.task(), 1));
    let mut hosted_b = hosted(Handle(s));
    hosted_b.privacy = vec!["no_retention".into()];
    let (_s, h) = serve(terms_server, vec![hosted_b]).await;
    let f = front(h.addr).await;
    let pool = HttpPool::new(policy()).unwrap();
    let (plain, _) = connect(pooled(&f.url(), &pool)).await;
    let mut cfg = pooled(&f.url(), &pool);
    cfg.privacy.insert("no_retention".into(), "true".into());
    let (private, _) = connect(cfg).await;
    let mut cfg = pooled(&f.url(), &pool);
    cfg.endpoint_class = "synthetic-other-class".into();
    let (other_class, _) = connect(cfg).await;
    let before = f.accepted();
    ok_call(&plain, "v/1").await;
    ok_call(&private, "v/2").await;
    ok_call(&other_class, "v/3").await;
    assert_eq!(f.accepted() - before, 3);
}

async fn call_as(c: &RemoteClient, id: &str, principal: &[u8]) -> AttemptReport {
    let cx = CallContext {
        remaining_ms: rustev_contract::time::DurationMs(10_000),
        trace_id: "SYNTHETIC-decision".into(),
        principal_handle: principal.to_vec(),
    };
    let p = topic_projection();
    c.infer(AttemptCall {
        projection: &p,
        attempt_id: id,
        cancel: CancelSignal::new(),
        cx: &cx,
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_principal_rotation_never_shares_a_connection_across_references() {
    let config = ServerConfig {
        versions: vec![1, 2],
        ..ServerConfig::default()
    };
    let (_s, h) = topic_backend(config).await;
    let f = front(h.addr).await;
    let pool = HttpPool::new(policy()).unwrap();
    let mut cfg = pooled(&f.url(), &pool);
    cfg.protocol_major = 2;
    cfg.operator = EndpointOperator::HostOperated;
    cfg.principal = PrincipalPosture::Sent {
        mapping_version: "synthetic-map/1".into(),
        scope: "synthetic-routing".into(),
        source: Arc::new(HandleAsReference),
    };
    let (c, sink) = connect(cfg).await;
    let before = f.accepted();
    assert!(call_as(&c, "q/1", b"SYNTHETIC-P1").await.result.is_ok());
    assert!(call_as(&c, "q/2", b"SYNTHETIC-P2").await.result.is_ok());
    assert!(call_as(&c, "q/3", b"SYNTHETIC-P1").await.result.is_ok());
    assert_eq!(f.accepted() - before, 2, "one connection per reference");
    let recs = sink.until_records(3).await;
    let k: Vec<_> = recs.iter().map(|r| r.connection.clone().unwrap()).collect();
    assert_ne!(k[0].key_digest, k[1].key_digest);
    assert_eq!(k[2].key_digest, k[0].key_digest);
    assert_eq!(k[2].reuse, ConnectionReuse::Reused);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_closed_idle_connection_is_evicted_at_checkout_never_used() {
    let (_s, h) = topic_backend(ServerConfig::default()).await;
    let f = front(h.addr).await;
    let pool = HttpPool::new(policy()).unwrap();
    let (c, sink) = connect(pooled(&f.url(), &pool)).await;
    ok_call(&c, "s/1").await;
    f.close_all.notify_waiters();
    // Give the connection's task a moment to observe the closed socket; the
    // pool learns of it when it next looks.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    ok_call(&c, "s/2").await;
    let recs = sink.until_records(2).await;
    assert_eq!(
        recs[1].connection.as_ref().unwrap().reuse,
        ConnectionReuse::Fresh
    );
    assert_eq!(pool.stats().evictions.get("closed"), Some(&1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stale_connection_failing_after_request_bytes_is_interrupted_and_never_retried() {
    let (s, h) = topic_backend(ServerConfig::default()).await;
    let f = front(h.addr).await;
    let pool = HttpPool::new(policy()).unwrap();
    let (c, sink) = connect(pooled(&f.url(), &pool)).await;
    ok_call(&c, "t/1").await;
    let dispatched = s.dispatched();
    let before = f.accepted();
    f.drop_next.store(true, Ordering::SeqCst);
    let r = call(&c, "t/2", &topic_projection(), 10_000, &CancelSignal::new()).await;
    let (_, detail) = r.result.clone().unwrap_err();
    assert!(
        detail.starts_with("remote:transport_interrupted"),
        "{detail}"
    );
    assert_eq!(
        (r.charge, r.remote),
        (Charge::Unknown, RemoteEnd::PossiblyContinuing)
    );
    // No transparent reconnect or replay: no new connection, nothing ran.
    assert_eq!(f.accepted(), before);
    assert_eq!(s.dispatched(), dispatched);
    let rec = sink.until_records(2).await.remove(1);
    let conn = rec.connection.unwrap();
    assert_eq!(conn.reuse, ConnectionReuse::Reused);
    assert_eq!(conn.eviction, Some(EvictionReason::Failed));
    assert!(rec.bytes_sent);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exhaustion_refuses_before_any_byte_when_the_policy_says_refuse() {
    let srv = canned_backend(linear_head(), terms(), |_| Canned::Hang).await;
    let mut p = policy();
    p.max_connections = 1;
    p.max_per_origin = 1;
    p.max_idle = 1;
    let pool = HttpPool::new(p).unwrap();
    let (c, _) = connect(pooled(&srv.url(), &pool)).await;
    let c2 = c.clone();
    let held = tokio::spawn(async move {
        call(&c2, "x/1", &topic_projection(), 3_000, &CancelSignal::new()).await
    });
    wait_until(|| srv.hits.load(Ordering::SeqCst) >= 2).await;
    let r = call(&c, "x/2", &topic_projection(), 3_000, &CancelSignal::new()).await;
    let (_, detail) = r.result.clone().unwrap_err();
    assert!(detail.starts_with("remote:transport_unsent"), "{detail}");
    assert_eq!(
        (r.charge, r.remote),
        (Charge::Observed { units: 0 }, RemoteEnd::Finished)
    );
    assert_eq!(srv.hits.load(Ordering::SeqCst), 2, "nothing more was sent");
    let _ = held.await;
    let _ = RemoteCode::TransportUnsent;
}
