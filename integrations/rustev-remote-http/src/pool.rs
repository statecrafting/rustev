//! Bounded, isolation-keyed connection reuse (spec 020, 3.5, I-2).
//!
//! Off by default. A host that enables it constructs one
//! [`ConnectionPool`] under a [`PoolPolicy`] and hands it to the clients
//! that may share it. Every bound is hard: total connections, connections
//! per origin, concurrent streams per connection (one: HTTP/1.1 carries a
//! single request at a time, so nothing is multiplexed), idle connections,
//! idle lifetime, absolute lifetime and uses per connection. Exhaustion
//! queues or refuses as the policy declares, never past the attempt budget.
//!
//! A connection is reused only under the exact [`PoolKey`] it was opened
//! under: the digest of every isolation member of [`KeyMaterial`]
//! (endpoint class and authority, protocol, proxy route, TLS posture,
//! credential identity digest, privacy options, principal posture, scope
//! and reference digest, key schema and adapter binding). Unknown or
//! noncanonical key material makes a key that never reuses. Pool state is
//! runtime state only: never core state, never replay input. Connections
//! are never moved between keys; a stale connection is evicted, never
//! transparently replaced mid-exchange.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rustev_contract::canonical::{canonical_bytes, tagged_digest};
use rustev_contract::ids::ArtifactId;
use rustev_contract::remote::{
    AgeBucket, ConnectionObservation, ConnectionReuse, EvictionReason, Multiplex,
};
use serde::Serialize;
use tokio::sync::Notify;
use tokio::time::Instant;

use crate::budget::Budget;

/// The schema of the isolation key's canonical material.
pub const POOL_KEY_SCHEMA: &str = "rustev.pool-key/1";

/// What a policy declares when the pool has no room.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OnExhaustion {
    /// Refuse at once: `transport_unsent`, nothing sent.
    Refuse,
    /// Wait for room at most this long, and never past the attempt budget.
    Queue { max_wait_ms: u64 },
}

/// The hard bounds of a pool (spec 020, 3.5). Every bound is positive;
/// `max_streams` is 1 because the HTTP/1.1 binding cannot multiplex.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PoolPolicy {
    pub max_connections: u32,
    pub max_per_origin: u32,
    pub max_streams: u32,
    pub max_idle: u32,
    pub idle_lifetime_ms: u64,
    pub max_lifetime_ms: u64,
    pub max_uses: u64,
    pub on_exhaustion: OnExhaustion,
}

impl PoolPolicy {
    /// Refuse an incomplete or unsatisfiable policy (spec 020, 3.1).
    pub fn validate(&self) -> Result<(), String> {
        let positive = [
            ("max_connections", self.max_connections as u64),
            ("max_per_origin", self.max_per_origin as u64),
            ("max_streams", self.max_streams as u64),
            ("max_idle", self.max_idle as u64),
            ("idle_lifetime_ms", self.idle_lifetime_ms),
            ("max_lifetime_ms", self.max_lifetime_ms),
            ("max_uses", self.max_uses),
        ];
        if let Some((name, _)) = positive.iter().find(|(_, v)| *v == 0) {
            return Err(format!("pool bound `{name}` must be positive"));
        }
        if self.max_streams != 1 {
            return Err("the HTTP/1.1 binding carries one stream per connection".into());
        }
        if self.max_per_origin > self.max_connections || self.max_idle > self.max_connections {
            return Err("per-origin and idle bounds cannot exceed the total".into());
        }
        if let OnExhaustion::Queue { max_wait_ms: 0 } = self.on_exhaustion {
            return Err("a queue needs a positive wait".into());
        }
        Ok(())
    }
}

/// The pool posture of an adapter binding: the policy and the key schema
/// it isolates by. `disabled`, the default, opens a fresh connection per
/// exchange, as spec 009 does.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PoolBinding {
    #[default]
    Disabled,
    Enabled {
        policy: PoolPolicy,
        key_schema: String,
    },
}

impl PoolBinding {
    pub fn is_disabled(&self) -> bool {
        matches!(self, PoolBinding::Disabled)
    }
}

/// Every isolation member of a pooled connection (spec 020, 3.5). Only
/// digests of secrets: the credential identity is a digest, the principal
/// reference is its evidence digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KeyMaterial {
    pub schema: String,
    pub endpoint_class: String,
    /// `scheme://host:port`.
    pub authority: String,
    /// The wire protocol and the HTTP version.
    pub protocol: String,
    /// `direct`: the binding supports no proxy.
    pub proxy_route: String,
    /// `plaintext`, `webpki-roots`, or the host's label for its own TLS
    /// configuration; `None` (unknown) disables reuse.
    pub tls: Option<String>,
    /// `none`, or the digest of the credential's identity.
    pub credential: String,
    /// Privacy and provider-routing options.
    pub privacy: BTreeMap<String, String>,
    /// `omitted`, or `host_operated` with mapping version and scope.
    pub principal_posture: String,
    /// The principal reference's evidence digest, per exchange.
    pub principal: Option<String>,
    pub binding: Option<ArtifactId>,
}

/// The digest of a credential's identity: never the credential.
pub fn credential_identity(secret: Option<&str>) -> String {
    match secret {
        None => "none".into(),
        Some(s) => tagged_digest("rustev.credential-identity/1", s.as_bytes()),
    }
}

/// A connection's isolation key and origin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolKey {
    /// `None` when the key material is unknown or noncanonical: such a
    /// connection is never reused.
    pub digest: Option<String>,
    pub origin: String,
}

impl KeyMaterial {
    /// The key: the tagged digest of the canonical material, or no reuse
    /// when any member is unknown or the material has no canonical form.
    pub fn key(&self) -> PoolKey {
        let known = self.tls.is_some() && self.binding.is_some() && self.schema == POOL_KEY_SCHEMA;
        let digest = known
            .then(|| canonical_bytes(self).ok())
            .flatten()
            .map(|b| tagged_digest(POOL_KEY_SCHEMA, &b));
        PoolKey {
            digest,
            origin: self.authority.clone(),
        }
    }
}

/// The pool could not provide a connection within the policy and budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exhausted;

struct Meta {
    id: u64,
    key: Option<String>,
    origin: String,
    created: Instant,
    uses: u64,
    last_used: Instant,
}

struct Idle<C> {
    meta: Meta,
    conn: C,
}

struct State<C> {
    idle: Vec<Idle<C>>,
    /// Connections open (checked out, reserved or idle) per origin.
    per_origin: BTreeMap<String, u32>,
    open: u32,
    evictions: BTreeMap<String, u64>,
}

/// Pool counters, for hosts and tests.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PoolStats {
    pub open: u32,
    pub idle: u32,
    pub opened: u64,
    pub reused: u64,
    pub evictions: BTreeMap<String, u64>,
}

/// A connection the pool can tell is closed.
pub trait Pooled: Send + 'static {
    fn is_closed(&self) -> bool;
}

/// Bounded connections shared by every client given the same pool.
pub struct ConnectionPool<C> {
    policy: PoolPolicy,
    state: Mutex<State<C>>,
    room: Notify,
    next_id: AtomicU64,
    opened: AtomicU64,
    reused: AtomicU64,
}

impl<C> fmt::Debug for ConnectionPool<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectionPool")
            .field("policy", &self.policy)
            .finish_non_exhaustive()
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn reason_name(r: EvictionReason) -> String {
    serde_json::to_value(r)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn ms(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

/// A checked-out connection, or a reserved place for a fresh one. Dropped
/// without [`ConnectionPool::release`], it closes and frees its place.
pub struct Lease<C: Pooled> {
    pool: Arc<ConnectionPool<C>>,
    meta: Option<Meta>,
    conn: Option<C>,
    reused: bool,
}

impl<C: Pooled> Lease<C> {
    /// The connection, once there is one.
    pub fn conn(&self) -> Option<&C> {
        self.conn.as_ref()
    }

    pub fn is_reused(&self) -> bool {
        self.reused
    }

    pub fn conn_mut(&mut self) -> Option<&mut C> {
        self.conn.as_mut()
    }

    /// Put the fresh connection in the reserved place.
    pub fn fill(&mut self, conn: C) {
        self.conn = Some(conn);
    }

    /// What evidence states about the connection, before release.
    pub fn observation(&self, key_digest: &str) -> Option<ConnectionObservation> {
        let m = self.meta.as_ref()?;
        Some(ConnectionObservation {
            reuse: if self.reused {
                ConnectionReuse::Reused
            } else {
                ConnectionReuse::Fresh
            },
            instance_id: format!("conn-{}", m.id),
            age_bucket: AgeBucket::of_ms(ms(Instant::now().saturating_duration_since(m.created))),
            use_count: m.uses,
            multiplex: Multiplex::None,
            key_digest: key_digest.to_string(),
            eviction: None,
        })
    }
}

impl<C: Pooled> Drop for Lease<C> {
    fn drop(&mut self) {
        if let Some(m) = self.meta.take() {
            self.pool.close(&m, EvictionReason::Failed);
        }
    }
}

impl<C: Pooled> ConnectionPool<C> {
    /// A pool under `policy`, which must validate.
    pub fn new(policy: PoolPolicy) -> Result<Arc<Self>, String> {
        policy.validate()?;
        Ok(Arc::new(ConnectionPool {
            policy,
            state: Mutex::new(State {
                idle: vec![],
                per_origin: BTreeMap::new(),
                open: 0,
                evictions: BTreeMap::new(),
            }),
            room: Notify::new(),
            next_id: AtomicU64::new(1),
            opened: AtomicU64::new(0),
            reused: AtomicU64::new(0),
        }))
    }

    pub fn policy(&self) -> &PoolPolicy {
        &self.policy
    }

    /// The binding view of this pool.
    pub fn binding(&self) -> PoolBinding {
        PoolBinding::Enabled {
            policy: self.policy,
            key_schema: POOL_KEY_SCHEMA.into(),
        }
    }

    pub fn stats(&self) -> PoolStats {
        let st = lock(&self.state);
        PoolStats {
            open: st.open,
            idle: st.idle.len() as u32,
            opened: self.opened.load(Ordering::SeqCst),
            reused: self.reused.load(Ordering::SeqCst),
            evictions: st.evictions.clone(),
        }
    }

    fn count(st: &mut State<C>, reason: EvictionReason) {
        *st.evictions.entry(reason_name(reason)).or_default() += 1;
    }

    fn close(&self, m: &Meta, reason: EvictionReason) {
        let mut st = lock(&self.state);
        Self::forget(&mut st, m);
        Self::count(&mut st, reason);
        drop(st);
        self.room.notify_waiters();
    }

    fn forget(st: &mut State<C>, m: &Meta) {
        st.open = st.open.saturating_sub(1);
        if let Some(n) = st.per_origin.get_mut(&m.origin) {
            *n = n.saturating_sub(1);
            if *n == 0 {
                st.per_origin.remove(&m.origin);
            }
        }
    }

    /// Drop every idle connection past its idle or absolute lifetime, or
    /// closed. Deterministic: in idle order.
    fn sweep(&self, st: &mut State<C>, now: Instant) {
        let idle_life = Duration::from_millis(self.policy.idle_lifetime_ms);
        let life = Duration::from_millis(self.policy.max_lifetime_ms);
        let mut keep = Vec::with_capacity(st.idle.len());
        for i in std::mem::take(&mut st.idle) {
            let reason = if i.conn.is_closed() {
                Some(EvictionReason::Closed)
            } else if now.saturating_duration_since(i.meta.created) >= life {
                Some(EvictionReason::LifetimeExpired)
            } else if now.saturating_duration_since(i.meta.last_used) >= idle_life {
                Some(EvictionReason::IdleExpired)
            } else {
                None
            };
            match reason {
                Some(r) => {
                    Self::forget(st, &i.meta);
                    Self::count(st, r);
                }
                None => keep.push(i),
            }
        }
        st.idle = keep;
    }

    fn try_acquire(self: &Arc<Self>, key: &PoolKey) -> Option<Lease<C>> {
        let now = Instant::now();
        let mut st = lock(&self.state);
        self.sweep(&mut st, now);
        // Reuse: the exact key only, never a connection of another key.
        if let Some(digest) = &key.digest
            && let Some(at) = st
                .idle
                .iter()
                .position(|i| i.meta.key.as_deref() == Some(digest.as_str()))
        {
            let Idle { mut meta, conn } = st.idle.remove(at);
            meta.uses += 1;
            meta.last_used = now;
            self.reused.fetch_add(1, Ordering::SeqCst);
            return Some(Lease {
                pool: self.clone(),
                meta: Some(meta),
                conn: Some(conn),
                reused: true,
            });
        }
        let origin_open = st.per_origin.get(&key.origin).copied().unwrap_or(0);
        if origin_open >= self.policy.max_per_origin {
            // Make room only from this origin's idle connections.
            let at = st.idle.iter().position(|i| i.meta.origin == key.origin)?;
            let old = st.idle.remove(at);
            Self::forget(&mut st, &old.meta);
            Self::count(&mut st, EvictionReason::Capacity);
        }
        if st.open >= self.policy.max_connections {
            // The least recently used idle connection of any key closes to
            // make room; it moves nowhere.
            let at = (0..st.idle.len())
                .min_by_key(|&i| (st.idle[i].meta.last_used, st.idle[i].meta.id))?;
            let old = st.idle.remove(at);
            Self::forget(&mut st, &old.meta);
            Self::count(&mut st, EvictionReason::Capacity);
        }
        st.open += 1;
        *st.per_origin.entry(key.origin.clone()).or_default() += 1;
        self.opened.fetch_add(1, Ordering::SeqCst);
        Some(Lease {
            pool: self.clone(),
            meta: Some(Meta {
                id: self.next_id.fetch_add(1, Ordering::SeqCst),
                key: key.digest.clone(),
                origin: key.origin.clone(),
                created: now,
                uses: 1,
                last_used: now,
            }),
            conn: None,
            reused: false,
        })
    }

    /// An idle connection under exactly `key`, or a reserved place for a
    /// fresh one. On exhaustion, refuse or queue as the policy declares,
    /// never past `budget`.
    pub async fn acquire(
        self: &Arc<Self>,
        key: &PoolKey,
        budget: &Budget,
    ) -> Result<Lease<C>, Exhausted> {
        let wait = match self.policy.on_exhaustion {
            OnExhaustion::Refuse => None,
            OnExhaustion::Queue { max_wait_ms } => Some(Budget::from_ms(max_wait_ms).min(*budget)),
        };
        loop {
            let room = self.room.notified();
            tokio::pin!(room);
            room.as_mut().enable();
            if let Some(l) = self.try_acquire(key) {
                return Ok(l);
            }
            let Some(w) = wait else {
                return Err(Exhausted);
            };
            tokio::select! {
                _ = &mut room => {}
                _ = w.expiry() => return Err(Exhausted),
            }
        }
    }

    /// Return a connection after its exchange. `reusable` is false when the
    /// exchange failed or did not complete its response. Returns why the
    /// connection left the pool, if it did.
    pub fn release(&self, mut lease: Lease<C>, reusable: bool) -> Option<EvictionReason> {
        let meta = lease.meta.take()?;
        let conn = lease.conn.take();
        let now = Instant::now();
        let reason = match &conn {
            None => Some(EvictionReason::Failed),
            Some(_) if !reusable => Some(EvictionReason::Failed),
            Some(c) if c.is_closed() => Some(EvictionReason::Closed),
            Some(_) if meta.key.is_none() => Some(EvictionReason::Closed),
            Some(_) if meta.uses >= self.policy.max_uses => Some(EvictionReason::MaxUses),
            Some(_)
                if now.saturating_duration_since(meta.created)
                    >= Duration::from_millis(self.policy.max_lifetime_ms) =>
            {
                Some(EvictionReason::LifetimeExpired)
            }
            Some(_) => None,
        };
        let mut st = lock(&self.state);
        match (reason, conn) {
            (Some(r), _) => {
                Self::forget(&mut st, &meta);
                Self::count(&mut st, r);
                drop(st);
                self.room.notify_waiters();
                Some(r)
            }
            (None, Some(conn)) => {
                if st.idle.len() as u32 >= self.policy.max_idle {
                    // The oldest idle connection gives way.
                    let at = (0..st.idle.len())
                        .min_by_key(|&i| (st.idle[i].meta.last_used, st.idle[i].meta.id))
                        .expect("max_idle is positive");
                    let old = st.idle.remove(at);
                    Self::forget(&mut st, &old.meta);
                    Self::count(&mut st, EvictionReason::IdleCapacity);
                }
                let mut meta = meta;
                meta.last_used = now;
                st.idle.push(Idle { meta, conn });
                drop(st);
                self.room.notify_waiters();
                None
            }
            (None, None) => unreachable!("no connection is always a reason"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    /// A SYNTHETIC connection whose closure a test controls.
    #[derive(Clone, Default)]
    struct Fake(Arc<AtomicBool>);

    impl Pooled for Fake {
        fn is_closed(&self) -> bool {
            self.0.load(Ordering::SeqCst)
        }
    }

    fn policy() -> PoolPolicy {
        PoolPolicy {
            max_connections: 3,
            max_per_origin: 2,
            max_streams: 1,
            max_idle: 2,
            idle_lifetime_ms: 100,
            max_lifetime_ms: 1_000,
            max_uses: 3,
            on_exhaustion: OnExhaustion::Refuse,
        }
    }

    fn key(d: &str, origin: &str) -> PoolKey {
        PoolKey {
            digest: Some(d.into()),
            origin: origin.into(),
        }
    }

    async fn take(p: &Arc<ConnectionPool<Fake>>, k: &PoolKey) -> Lease<Fake> {
        let mut l = p.acquire(k, &Budget::from_ms(10_000)).await.unwrap();
        if l.conn().is_none() {
            l.fill(Fake::default());
        }
        l
    }

    #[test]
    fn every_bound_must_be_positive_and_consistent() {
        assert!(policy().validate().is_ok());
        let cases: Vec<fn(&mut PoolPolicy)> = vec![
            |p| p.max_connections = 0,
            |p| p.max_per_origin = 0,
            |p| p.max_streams = 0,
            |p| p.max_streams = 2,
            |p| p.max_idle = 0,
            |p| p.idle_lifetime_ms = 0,
            |p| p.max_lifetime_ms = 0,
            |p| p.max_uses = 0,
            |p| p.max_per_origin = 4,
            |p| p.max_idle = 4,
            |p| p.on_exhaustion = OnExhaustion::Queue { max_wait_ms: 0 },
        ];
        for c in cases {
            let mut p = policy();
            c(&mut p);
            assert!(p.validate().is_err(), "{p:?}");
            assert!(ConnectionPool::<Fake>::new(p).is_err());
        }
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn a_connection_is_reused_only_under_its_exact_key() {
        let p = ConnectionPool::<Fake>::new(policy()).unwrap();
        let a = key("A", "o");
        let l = take(&p, &a).await;
        assert!(!l.is_reused());
        assert_eq!(p.release(l, true), None);
        // Another key at the same origin never takes it.
        let b = take(&p, &key("B", "o")).await;
        assert!(!b.is_reused());
        let again = take(&p, &a).await;
        assert!(again.is_reused());
        assert_eq!(again.observation("A").unwrap().use_count, 2);
        // A key without a digest (unknown material) never reuses.
        let unknown = PoolKey {
            digest: None,
            origin: "u".into(),
        };
        let u = take(&p, &unknown).await;
        assert_eq!(p.release(u, true), Some(EvictionReason::Closed));
        let stats = p.stats();
        assert_eq!((stats.opened, stats.reused), (3, 1));
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn lifetimes_uses_and_closure_evict() {
        let p = ConnectionPool::<Fake>::new(policy()).unwrap();
        let a = key("A", "o");
        // Idle lifetime.
        let l = take(&p, &a).await;
        p.release(l, true);
        tokio::time::advance(Duration::from_millis(100)).await;
        assert!(!take(&p, &a).await.is_reused());
        assert_eq!(p.stats().evictions["idle_expired"], 1);
        // Uses: the third use is the last.
        let p = ConnectionPool::<Fake>::new(policy()).unwrap();
        let mut last = None;
        for n in 1..=3 {
            let l = take(&p, &a).await;
            assert_eq!(l.is_reused(), n > 1);
            last = p.release(l, true);
        }
        assert_eq!(last, Some(EvictionReason::MaxUses));
        // Absolute lifetime, even while in use.
        let p = ConnectionPool::<Fake>::new(policy()).unwrap();
        let l = take(&p, &a).await;
        tokio::time::advance(Duration::from_millis(1_000)).await;
        assert_eq!(p.release(l, true), Some(EvictionReason::LifetimeExpired));
        // Absolute lifetime while idle.
        let l = take(&p, &a).await;
        p.release(l, true);
        for _ in 0..11 {
            tokio::time::advance(Duration::from_millis(95)).await;
            let l = take(&p, &a).await;
            if !l.is_reused() {
                break;
            }
            p.release(l, true);
        }
        assert!(p.stats().evictions.contains_key("lifetime_expired"));
        // A closed idle connection is evicted at checkout, not reused.
        let p = ConnectionPool::<Fake>::new(policy()).unwrap();
        let l = take(&p, &a).await;
        let flag = l.conn().unwrap().0.clone();
        p.release(l, true);
        flag.store(true, Ordering::SeqCst);
        assert!(!take(&p, &a).await.is_reused());
        assert_eq!(p.stats().evictions["closed"], 1);
        // A failed exchange never returns its connection.
        let l = take(&p, &key("F", "o")).await;
        assert_eq!(p.release(l, false), Some(EvictionReason::Failed));
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn idle_total_and_origin_bounds_hold_and_exhaustion_refuses_or_queues() {
        let p = ConnectionPool::<Fake>::new(policy()).unwrap();
        // Idle bound: a third idle connection pushes the oldest out.
        let (x, y, z) = (
            take(&p, &key("X", "o1")).await,
            take(&p, &key("Y", "o1")).await,
            take(&p, &key("Z", "o2")).await,
        );
        p.release(x, true);
        tokio::time::advance(Duration::from_millis(1)).await;
        p.release(y, true);
        p.release(z, true);
        assert_eq!(p.stats().idle, 2);
        assert_eq!(p.stats().evictions["idle_capacity"], 1);
        // Per-origin bound: two checked out at o1, a third is refused.
        let p = ConnectionPool::<Fake>::new(policy()).unwrap();
        let _a = take(&p, &key("A", "o1")).await;
        let _b = take(&p, &key("B", "o1")).await;
        assert!(
            p.acquire(&key("C", "o1"), &Budget::from_ms(10))
                .await
                .is_err()
        );
        // Total bound: one more at o2 fits, then nothing.
        let _c = take(&p, &key("C", "o2")).await;
        assert!(
            p.acquire(&key("D", "o3"), &Budget::from_ms(10))
                .await
                .is_err()
        );
        // An idle connection of another key gives way (closed, not moved).
        let p = ConnectionPool::<Fake>::new(policy()).unwrap();
        let a = take(&p, &key("A", "o1")).await;
        let _b = take(&p, &key("B", "o2")).await;
        let _c = take(&p, &key("C", "o3")).await;
        p.release(a, true);
        let d = take(&p, &key("D", "o4")).await;
        assert!(!d.is_reused());
        assert_eq!(p.stats().evictions["capacity"], 1);

        // Queue: wait for room within the declared wait and the budget.
        let mut q = policy();
        q.max_connections = 1;
        q.max_per_origin = 1;
        q.max_idle = 1;
        q.on_exhaustion = OnExhaustion::Queue { max_wait_ms: 50 };
        let p = ConnectionPool::<Fake>::new(q).unwrap();
        let held = take(&p, &key("A", "o")).await;
        let t0 = Instant::now();
        assert!(
            p.acquire(&key("B", "o"), &Budget::from_ms(10_000))
                .await
                .is_err()
        );
        assert_eq!(t0.elapsed(), Duration::from_millis(50));
        let t0 = Instant::now();
        assert!(
            p.acquire(&key("B", "o"), &Budget::from_ms(20))
                .await
                .is_err()
        );
        assert_eq!(
            t0.elapsed(),
            Duration::from_millis(20),
            "never past the budget"
        );
        let p2 = p.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            drop(held);
        });
        let got = p2.acquire(&key("B", "o"), &Budget::from_ms(10_000)).await;
        assert!(got.is_ok());
        let _ = p;
    }

    #[test]
    fn every_key_member_changes_the_key_and_unknown_material_disables_reuse() {
        let base = KeyMaterial {
            schema: POOL_KEY_SCHEMA.into(),
            endpoint_class: "c".into(),
            authority: "http://127.0.0.1:1".into(),
            protocol: "rustev.remote/1 http/1.1".into(),
            proxy_route: "direct".into(),
            tls: Some("plaintext".into()),
            credential: credential_identity(Some("SYNTHETIC-A")),
            privacy: BTreeMap::new(),
            principal_posture: "omitted".into(),
            principal: None,
            binding: Some(
                ArtifactId::parse(
                    "sha256:00000000000000000000000000000000000000000000000000000000000000aa",
                )
                .unwrap(),
            ),
        };
        let k = base.key().digest.unwrap();
        let variants: Vec<KeyMaterial> =
            vec![
            KeyMaterial { endpoint_class: "x".into(), ..base.clone() },
            KeyMaterial { authority: "http://127.0.0.1:2".into(), ..base.clone() },
            KeyMaterial { protocol: "rustev.remote/2 http/1.1".into(), ..base.clone() },
            KeyMaterial { proxy_route: "via".into(), ..base.clone() },
            KeyMaterial { tls: Some("webpki-roots".into()), ..base.clone() },
            KeyMaterial { credential: credential_identity(Some("SYNTHETIC-B")), ..base.clone() },
            KeyMaterial { credential: credential_identity(None), ..base.clone() },
            KeyMaterial { privacy: BTreeMap::from([("k".into(), "v".into())]), ..base.clone() },
            KeyMaterial { principal_posture: "host_operated m/1 s".into(), ..base.clone() },
            KeyMaterial { principal: Some("d".into()), ..base.clone() },
            KeyMaterial {
                binding: Some(
                    ArtifactId::parse(
                        "sha256:00000000000000000000000000000000000000000000000000000000000000bb",
                    )
                    .unwrap(),
                ),
                ..base.clone()
            },
        ];
        for v in variants {
            assert_ne!(v.key().digest.unwrap(), k, "{v:?}");
        }
        assert!(!credential_identity(Some("SYNTHETIC-A")).contains("SYNTHETIC"));
        for unknown in [
            KeyMaterial {
                tls: None,
                ..base.clone()
            },
            KeyMaterial {
                binding: None,
                ..base.clone()
            },
            KeyMaterial {
                schema: "rustev.pool-key/0".into(),
                ..base.clone()
            },
        ] {
            assert_eq!(unknown.key().digest, None);
        }
    }
}
