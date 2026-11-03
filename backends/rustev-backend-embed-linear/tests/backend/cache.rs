//! The embedding cache wired into the backend (3.14): off by default,
//! equivalence, charges, principal isolation, eviction, `clear`,
//! `erase_principal` racing an in-flight run, and cancelled runs.
use super::*;
use rustev_backend_embed_linear::cache::{CacheLimits, CacheStats};

fn cached(b: &Bundle, max_entries: usize, max_bytes: usize) -> EmbedConfig {
    let mut c = config(b, 1, 1);
    c.embedding_cache = Some(CacheLimits {
        max_entries,
        max_bytes,
    });
    c
}
fn blocked_cached(b: &Bundle, max_entries: usize) -> (EmbedLinearBackend, Arc<Gate>) {
    let gate = Arc::new(Gate::default());
    let runner = gate.clone();
    (
        EmbedLinearBackend::open_with_synthetic_runner(
            cached(b, max_entries, usize::MAX),
            move |ids, c| runner.run(ids, c),
        )
        .unwrap(),
        gate,
    )
}
fn principal(handle: &[u8]) -> CallContext {
    CallContext {
        principal_handle: handle.to_vec(),
        ..context()
    }
}
fn other_text() -> Vec<u8> {
    String::from_utf8(projection())
        .unwrap()
        .replace("hello", "world")
        .into_bytes()
}
/// Run one attempt to its report, releasing the gate once if it starts.
fn run_through(
    backend: &EmbedLinearBackend,
    gate: &Gate,
    p: &[u8],
    cx: &CallContext,
) -> AttemptReport {
    let notice = Arc::new(Notice::default());
    let before = gate.state.lock().unwrap().started;
    let mut f = attempt(backend, p, cx, &CancelSignal::new());
    if let Poll::Ready(r) = poll(&mut f, &notice) {
        return r;
    }
    gate.started(before + 1);
    gate.release(End::Good);
    finish(&mut f, &notice)
}
fn started(gate: &Gate) -> usize {
    gate.state.lock().unwrap().started
}

#[test]
fn off_by_default_every_call_reaches_a_worker_and_counts_nothing() {
    let b = new_bundle();
    let (backend, gate) = blocked(&b, 1, 1);
    let (p, cx) = (projection(), context());
    for n in 1..=2 {
        let r = run_through(&backend, &gate, &p, &cx);
        assert!(r.result.is_ok());
        assert_charge(&r, 11);
        assert_eq!(started(&gate), n);
    }
    backend.erase_principal(b"");
    backend.clear_cache();
    assert_eq!(backend.cache_stats(), CacheStats::default());
}

#[test]
fn hit_supplies_the_miss_bits_without_a_run_and_is_charged_per_call() {
    let b = new_bundle();
    let backend = EmbedLinearBackend::open(cached(&b, 4, usize::MAX)).unwrap();
    let uncached = EmbedLinearBackend::open(config(&b, 1, 0)).unwrap();
    let (p, cx, notice) = (
        projection(),
        principal(b"alice"),
        Arc::new(Notice::default()),
    );
    let miss = finish(
        &mut attempt(&backend, &p, &cx, &CancelSignal::new()),
        &notice,
    );
    assert_charge(&miss, 11);
    // A hit completes on the caller's first poll, with no handoff.
    let hit = ready(
        &mut attempt(&backend, &p, &cx, &CancelSignal::new()),
        &notice,
    );
    assert_charge(&hit, 1);
    assert_eq!(
        (hit.cancel, hit.remote),
        (CancelAck::NotRequested, RemoteEnd::Finished)
    );
    let reference = finish(
        &mut attempt(&uncached, &p, &cx, &CancelSignal::new()),
        &notice,
    );
    let bits = |r: &AttemptReport| match r.result.as_ref().unwrap() {
        RawOutput::Logits(l) => l
            .iter()
            .map(|(k, v)| (k.clone(), v.to_bits()))
            .collect::<Vec<_>>(),
        other => panic!("{other:?}"),
    };
    assert_eq!(bits(&hit), bits(&miss));
    assert_eq!(bits(&hit), bits(&reference));
    let stats = backend.cache_stats();
    assert_eq!(
        (stats.hits, stats.misses, stats.admitted, stats.evicted),
        (1, 1, 1, 0)
    );
    assert!(stats.bytes > 0);
}

#[test]
fn hits_never_cross_principal_handles() {
    let b = new_bundle();
    let (backend, gate) = blocked_cached(&b, 4);
    let p = projection();
    let (alice, bob) = (principal(b"alice"), principal(b"bob"));
    run_through(&backend, &gate, &p, &alice);
    run_through(&backend, &gate, &p, &bob);
    assert_eq!(started(&gate), 2, "bob's first call must run");
    assert_charge(&run_through(&backend, &gate, &p, &alice), 1);
    assert_charge(&run_through(&backend, &gate, &p, &bob), 1);
    assert_eq!(started(&gate), 2);
    let stats = backend.cache_stats();
    assert_eq!((stats.hits, stats.misses, stats.admitted), (2, 2, 2));
}

#[test]
fn eviction_is_bounded_by_entries_and_least_recently_used() {
    let b = new_bundle();
    let (backend, gate) = blocked_cached(&b, 1);
    let (p, q, cx) = (projection(), other_text(), principal(b"alice"));
    run_through(&backend, &gate, &p, &cx);
    run_through(&backend, &gate, &q, &cx);
    let stats = backend.cache_stats();
    assert_eq!((stats.admitted, stats.evicted), (2, 1));
    assert_charge(&run_through(&backend, &gate, &q, &cx), 1);
    assert_charge(&run_through(&backend, &gate, &p, &cx), 11);
    assert_eq!(started(&gate), 3);
}

#[test]
fn eviction_is_bounded_by_bytes() {
    let b = new_bundle();
    let backend = EmbedLinearBackend::open(cached(&b, 8, 1)).unwrap();
    let (p, cx, notice) = (
        projection(),
        principal(b"alice"),
        Arc::new(Notice::default()),
    );
    for _ in 0..2 {
        let r = finish(
            &mut attempt(&backend, &p, &cx, &CancelSignal::new()),
            &notice,
        );
        assert_charge(&r, 11);
    }
    let stats = backend.cache_stats();
    assert_eq!((stats.admitted, stats.bytes, stats.misses), (0, 0, 2));
}

#[test]
fn clear_and_erase_principal_remove_entries_before_returning() {
    let b = new_bundle();
    let (backend, gate) = blocked_cached(&b, 4);
    let p = projection();
    let (alice, bob) = (principal(b"alice"), principal(b"bob"));
    run_through(&backend, &gate, &p, &alice);
    run_through(&backend, &gate, &p, &bob);
    backend.erase_principal(b"alice");
    assert_charge(&run_through(&backend, &gate, &p, &bob), 1);
    assert_charge(&run_through(&backend, &gate, &p, &alice), 11);
    backend.clear_cache();
    assert_eq!(backend.cache_stats().bytes, 0);
    assert_charge(&run_through(&backend, &gate, &p, &bob), 11);
    assert_eq!(started(&gate), 4);
    assert_eq!(backend.cache_stats().invalidations, 2);
}

#[test]
fn erase_principal_racing_an_in_flight_run_is_not_stored() {
    let b = new_bundle();
    let (backend, gate) = blocked_cached(&b, 4);
    let (p, cx, notice) = (
        projection(),
        principal(b"alice"),
        Arc::new(Notice::default()),
    );
    let mut f = attempt(&backend, &p, &cx, &CancelSignal::new());
    assert!(poll(&mut f, &notice).is_pending());
    gate.started(1);
    backend.erase_principal(b"alice");
    gate.release(End::Good);
    let r = finish(&mut f, &notice);
    assert!(r.result.is_ok(), "the run's own answer is still supplied");
    assert_charge(&r, 11);
    assert_eq!(backend.cache_stats().admitted, 0);
    assert_charge(&run_through(&backend, &gate, &p, &cx), 11);
    assert_eq!(started(&gate), 2);
    assert_eq!(backend.cache_stats().admitted, 1);
}

#[test]
fn cancelled_failed_and_abandoned_runs_are_not_admitted() {
    let b = new_bundle();
    let (backend, gate) = blocked_cached(&b, 4);
    let (p, cx, notice) = (
        projection(),
        principal(b"alice"),
        Arc::new(Notice::default()),
    );
    // Cancelled after handoff; the run completes before the next poll.
    let signal = CancelSignal::new();
    let mut f = attempt(&backend, &p, &cx, &signal);
    assert!(poll(&mut f, &notice).is_pending());
    gate.started(1);
    signal.raise();
    let before = notice.count();
    gate.release(End::Good);
    notice.after(before);
    assert_eq!(ready(&mut f, &notice).cancel, CancelAck::Stopped);
    // A failed run.
    let mut f = attempt(&backend, &p, &cx, &CancelSignal::new());
    assert!(poll(&mut f, &notice).is_pending());
    gate.started(2);
    gate.release(End::Fail);
    assert!(finish(&mut f, &notice).result.is_err());
    // Abandoned: the future is dropped while its run is in flight.
    let mut f = attempt(&backend, &p, &cx, &CancelSignal::new());
    assert!(poll(&mut f, &notice).is_pending());
    gate.started(3);
    drop(f);
    gate.release(End::Good);
    // The slot frees once the late run ends; the next call is still a miss.
    let r = run_through(&backend, &gate, &p, &cx);
    assert_charge(&r, 11);
    let stats = backend.cache_stats();
    assert_eq!((stats.admitted, stats.hits, stats.misses), (1, 0, 4));
}

#[test]
fn cache_configuration_is_not_identity() {
    let b = new_bundle();
    let on = EmbedLinearBackend::open(cached(&b, 4, 4096)).unwrap();
    let off = EmbedLinearBackend::open(config(&b, 1, 0)).unwrap();
    assert_eq!(on.descriptor(), off.descriptor());
    assert_eq!(on.cost_bound(b""), off.cost_bound(b""));
}
