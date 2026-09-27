//! Local synthetic measurement harness for spec 017.
//!
//! This ignored test has no performance threshold. It reports cold and warm
//! cohorts while asserting only dispatch counts and judgment equivalence.

mod common;

use std::time::Instant;

use common::*;
use rustev_contract::execution::CostPolicy;
use rustev_contract::optimization::{
    EvictionOrder, ExpiryBasis, MemoryCachePolicy, OptimizationAdmission, OptimizationPolicy,
};
use rustev_core::seams::CancelSignal;
use rustev_runtime::Completion;

const RUNS: usize = 100;

fn policy() -> rustev_contract::execution::ExecutionPolicy {
    let mut policy = execution(CostPolicy::Hard { max_units: 100 }, vec![]);
    policy.optimization = Some(OptimizationPolicy {
        key_schema_version: 1,
        cache_namespace_version: 1,
        admission: OptimizationAdmission::Refuse,
        expiry: ExpiryBasis::InjectedRuntimeTime,
        batch: None,
        memory_cache: Some(MemoryCachePolicy {
            max_bytes: 10_000_000,
            max_entries: 512,
            max_entry_bytes: 16_384,
            max_scope_bytes: 100_000,
            ttl_ms: 60_000,
            eviction: EvictionOrder::Fifo,
        }),
    });
    policy
}

fn percentile(values: &[u128], percentile: usize) -> u128 {
    let mut values = values.to_vec();
    values.sort_unstable();
    let rank = (percentile * values.len()).div_ceil(100).max(1);
    values[rank - 1]
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "local synthetic measurement without a performance threshold"]
async fn report_cold_and_warm_memory_cache_measurements() {
    let backend = answering(linear_head());
    let rig = rig(config(2, 0, 3), &[(backend.clone(), 3)]);
    let plan = rig.rt.prepare(support_compiled(Some(&policy()))).unwrap();
    let mut cold_ns = Vec::with_capacity(RUNS);
    let mut reference = None;
    let cold_started = Instant::now();
    for index in 0..RUNS {
        let mut req = request(&format!("benchmark-cold-{index}"));
        req.principal_handle = format!("tenant-cold-{index}").into_bytes();
        let started = Instant::now();
        let decided = rig
            .rt
            .decide(&plan, req, &CancelSignal::new())
            .await
            .unwrap();
        cold_ns.push(started.elapsed().as_nanos());
        assert!(matches!(decided.completion, Completion::Judged(_)));
        reference.get_or_insert(decided.completion);
    }
    let cold_total_ns = cold_started.elapsed().as_nanos();
    assert_eq!(backend.dispatched().len(), RUNS * 3);

    let mut warm_request = request("benchmark-warm");
    warm_request.principal_handle = b"tenant-warm".to_vec();
    let seed = rig
        .rt
        .decide(&plan, warm_request.clone(), &CancelSignal::new())
        .await
        .unwrap();
    assert_eq!(seed.completion, reference.clone().unwrap());
    let dispatches_after_seed = backend.dispatched().len();

    let mut warm_ns = Vec::with_capacity(RUNS);
    let warm_started = Instant::now();
    for _ in 0..RUNS {
        let started = Instant::now();
        let decided = rig
            .rt
            .decide(&plan, warm_request.clone(), &CancelSignal::new())
            .await
            .unwrap();
        warm_ns.push(started.elapsed().as_nanos());
        assert_eq!(decided.completion, reference.clone().unwrap());
        assert_eq!(decided.record.core, seed.record.core);
    }
    let warm_total_ns = warm_started.elapsed().as_nanos();
    assert_eq!(backend.dispatched().len(), dispatches_after_seed);

    println!(
        "{{\"runs\":{RUNS},\"cold\":{{\"p50_us\":{},\"p95_us\":{},\"p99_us\":{},\"throughput_per_second\":{:.2}}},\"warm\":{{\"p50_us\":{},\"p95_us\":{},\"p99_us\":{},\"throughput_per_second\":{:.2}}},\"backend_dispatches\":{}}}",
        percentile(&cold_ns, 50) / 1_000,
        percentile(&cold_ns, 95) / 1_000,
        percentile(&cold_ns, 99) / 1_000,
        RUNS as f64 * 1_000_000_000.0 / cold_total_ns as f64,
        percentile(&warm_ns, 50) / 1_000,
        percentile(&warm_ns, 95) / 1_000,
        percentile(&warm_ns, 99) / 1_000,
        RUNS as f64 * 1_000_000_000.0 / warm_total_ns as f64,
        backend.dispatched().len(),
    );
}
