//! SYNTHETIC spec 035 runtime acceptance (R-09). All clocks are manual.
mod common;
use common::*;
use rustev_contract::{
    Document, definition::*, descriptor::*, execution::*, optimization::*, output::*, run::*,
    screening::*,
};
use rustev_core::{compile_with, seams::CancelSignal};
use rustev_runtime::{Completion, Decided, DecisionRequest};
use serde_json::json;
use std::sync::Arc;

const CANARY: &str = "CANARY-screen-é-only";

fn descriptor(per_item: bool) -> BackendDescriptor {
    let mut d = label_only();
    d.backend_id = "screen".into();
    d.operations = vec![support(Operation::Proposition, OutputKind::Screen, 2)];
    d.operations[0].isolation = per_item.then_some(Isolation::PerItem);
    d.operations[0].screen = Some(ScreenProfile {
        policy_version: "synthetic/1".into(),
        categories: vec!["injection".into()],
        max_text_bytes: 48,
        text_unit_bytes: 16,
    });
    d
}
fn definition(list: bool) -> Definition {
    let mut d: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../rustev-core/tests/golden/screening.definition.json"
    ))
    .unwrap();
    if list {
        d["screening"]["entries"] = json!([d["screening"]["entries"][2].clone()]);
        d["inputs"][1]["type"]["list"]["max_items"] = json!(10);
        d["inputs"][1]["type"]["list"]["item"]["record"]["fields"][0]["type"]["enum"]["options"] =
            json!((0..10).map(|n| format!("i{n}")).collect::<Vec<_>>());
        d["screening"]["max_text_units"] = json!(30);
    } else {
        d["screening"]["entries"][1]["on_block"] = json!("refuse");
    }
    Definition::parse(&serde_json::to_vec(&d).unwrap()).unwrap()
}
fn policy(batch: bool, cache: bool) -> ExecutionPolicy {
    let mut p = execution(CostPolicy::Hard { max_units: 100 }, vec![]);
    if batch || cache {
        p.optimization = Some(OptimizationPolicy {
            key_schema_version: 1,
            cache_namespace_version: 1,
            admission: OptimizationAdmission::Refuse,
            expiry: ExpiryBasis::InjectedRuntimeTime,
            batch: batch.then_some(BatchPolicy {
                max_members: 10,
                max_canonical_bytes: 100_000,
                max_members_per_scope: 10,
                max_queue_delay_ms: 0,
                max_backend_work: 10,
                allocation: BatchChargeAllocation::EvenRemainderByMemberIndex,
            }),
            memory_cache: cache.then_some(MemoryCachePolicy {
                max_bytes: 100_000,
                max_entries: 100,
                max_entry_bytes: 10_000,
                max_scope_bytes: 100_000,
                ttl_ms: 1000,
                eviction: EvictionOrder::Fifo,
                acquisition_pages: false,
            }),
            shared_calls: None,
            persistent_cache: None,
        });
    }
    p
}
fn req(id: &str, list: bool, text: &str) -> DecisionRequest {
    let items = if list {
        json!(
            (0..10)
                .map(|n| json!({"id":format!("i{n}"),"description":text}))
                .collect::<Vec<_>>()
        )
    } else {
        json!([{"id":"a","description":text},{"id":"b","description":text}])
    };
    DecisionRequest {
        decision_id: id.into(),
        snapshot: snapshot(vec![
            entry(
                "request",
                json!({"text":text,"behaviour_text":"behaviour"}),
                ProvenanceClass::UserSupplied,
                NOW,
            ),
            entry("candidates", items, ProvenanceClass::ThirdParty, NOW),
        ]),
        evaluation_time: ts(NOW),
        deadline_ms: None,
        principal_handle: b"tenant-a".to_vec(),
    }
}
fn answer(blocked: bool, units: ScreenUnits) -> Answer {
    Answer::Output(
        RawOutput::Screen {
            blocked,
            categories: if blocked {
                vec!["injection".into()]
            } else {
                vec![]
            },
            units,
        },
        Charge::Observed { units: 1 },
    )
}
fn assert_record(d: &Decided) {
    let bytes = d.record.record_canonical().unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains(CANARY));
    RunRecord::parse(&bytes).unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn per_item_never_enters_general_or_batch_dispatch_seam_and_shared_batches() {
    // Spec 020's batch dispatch uses the same batch_capability/infer_batch
    // seam as spec 017. Advertise it even for a per-item descriptor.
    for dispatch in [false, true] {
        for per_item in [true, false] {
            for enabled in [true, false] {
                let descriptor = descriptor(per_item);
                let backend = Scripted::new(descriptor.clone(), |_| {
                    answer(false, ScreenUnits::NotReported)
                })
                .with_batch();
                let backend = if dispatch {
                    backend.with_batch_dispatch()
                } else {
                    backend
                };
                let rig = rig(config(1, 0, 10), &[(backend.clone(), 10)]);
                let c = compile_with(
                    &definition(true),
                    &[descriptor],
                    &[],
                    &policy(enabled, false),
                )
                .unwrap();
                let plan = rig.rt.prepare(c).unwrap();
                let d = rig
                    .rt
                    .decide(&plan, req("isolation", true, CANARY), &CancelSignal::new())
                    .await
                    .unwrap();
                assert_eq!(
                    backend.text_counts(),
                    if per_item || !enabled {
                        vec![1; 10]
                    } else {
                        vec![10]
                    }
                );
                assert_eq!(d.record.requests.len(), 10);
                for r in &d.record.requests {
                    let s = r.screen.as_ref().unwrap();
                    assert_eq!(s.units.computed, Some(2));
                    assert_eq!(s.units.charged, 2);
                    assert_eq!(s.units.reported, None);
                    assert_eq!(s.units.basis, ScreenUnitBasis::Computed);
                    assert_eq!(s.cache, ScreenCache::Disabled);
                }
                assert_record(&d);
            }
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn cache_reuses_equal_text_with_source_attempt_and_never_crosses_scope() {
    let descriptor = descriptor(true);
    let backend = Scripted::new(descriptor.clone(), |_| {
        answer(false, ScreenUnits::Reported { n: 1 })
    });
    let rig = rig(config(1, 0, 1), &[(backend.clone(), 10)]);
    let plan = rig
        .rt
        .prepare(compile_with(&definition(true), &[descriptor], &[], &policy(false, true)).unwrap())
        .unwrap();
    let d = rig
        .rt
        .decide(&plan, req("cache", true, CANARY), &CancelSignal::new())
        .await
        .unwrap();
    assert_eq!(backend.text_counts(), vec![1]);
    let source = &d.record.requests[0].attempts[0].attempt_id;
    for r in &d.record.requests[1..] {
        let s = r.screen.as_ref().unwrap();
        assert_eq!(
            s.cache,
            ScreenCache::Hit {
                source_attempt: source.clone()
            }
        );
        assert_eq!(s.units.charged, 0);
        assert_eq!(s.units.basis, ScreenUnitBasis::CacheHit);
        assert_eq!(s.units.computed, None);
        assert_eq!(s.units.reported, None);
        assert!(r.attempts.is_empty());
    }
    assert_record(&d);
    let mut other = req("other", true, CANARY);
    other.principal_handle = b"tenant-b".to_vec();
    let d = rig
        .rt
        .decide(&plan, other, &CancelSignal::new())
        .await
        .unwrap();
    assert_eq!(backend.text_counts(), vec![1, 1]);
    assert_eq!(
        d.record.requests[0].screen.as_ref().unwrap().cache,
        ScreenCache::Miss
    );
    assert_record(&d);
}

#[tokio::test(flavor = "current_thread")]
async fn reported_unit_violation_and_empty_text() {
    let descriptor = descriptor(true);
    let backend = Scripted::new(descriptor.clone(), |_| {
        answer(false, ScreenUnits::Reported { n: 9 })
    });
    let rig = rig(config(1, 0, 10), &[(backend.clone(), 10)]);
    let plan = rig
        .rt
        .prepare(
            compile_with(&definition(true), &[descriptor], &[], &policy(false, false)).unwrap(),
        )
        .unwrap();
    let d = rig
        .rt
        .decide(&plan, req("units", true, "ééééééééé"), &CancelSignal::new())
        .await
        .unwrap();
    for r in &d.record.requests {
        let u = &r.screen.as_ref().unwrap().units;
        assert_eq!(u.computed, Some(2), "UTF-8 bytes, not characters");
        assert_eq!(u.reported, Some(9));
        assert_eq!(u.charged, 9);
        assert_eq!(u.basis, ScreenUnitBasis::Reported);
        assert_eq!(
            u.bound_violation,
            Some(ScreenUnitBoundViolation {
                computed: 2,
                reported: 9
            })
        );
    }
    assert_record(&d);
    let d = rig
        .rt
        .decide(&plan, req("empty", true, ""), &CancelSignal::new())
        .await
        .unwrap();
    assert!(d.record.requests.is_empty());
    assert_eq!(d.record.cost.observed, 0);
    assert_eq!(backend.text_counts().len(), 10);
    assert_record(&d);
}

#[tokio::test(flavor = "current_thread")]
async fn refusal_withdraws_queued_work_cancels_inflight_and_never_supplies_late_output() {
    for parallel in [2, 4] {
        for on_cancel in [
            OnCancel::LateScreen,
            OnCancel::Ignore,
            OnCancel::Stop(Charge::Observed { units: 1 }),
        ] {
            let descriptor = descriptor(true);
            let backend =
                Scripted::new(descriptor.clone(), |_| Answer::Gate).with_on_cancel(on_cancel);
            let rig = rig(config(1, 0, parallel), &[(backend.clone(), 2)]);
            let plan = Arc::new(
                rig.rt
                    .prepare(
                        compile_with(&definition(false), &[descriptor], &[], &policy(false, true))
                            .unwrap(),
                    )
                    .unwrap(),
            );
            let cancel = CancelSignal::new();
            let running = start_req(&rig, &plan, req("refusal", false, CANARY), &cancel);
            until(|| backend.gated().len() == 2).await;
            let second = backend
                .gated()
                .into_iter()
                .find(|s| s.contains("behaviour"))
                .unwrap();
            assert!(backend.release(&second, answer(true, ScreenUnits::NotReported)));
            let d = decided(done(running).await);
            assert!(!cancel.is_raised());
            let Completion::Judged(j) = &d.completion else {
                panic!("{:?}", d.completion)
            };
            let rustev_contract::judgment::Outcome::Refused { refusals, .. } = &j.outcome else {
                panic!("{:?}", j.outcome)
            };
            assert_eq!(refusals.len(), 1);
            assert_eq!(refusals[0].check, "screen:behaviour");
            assert_eq!(backend.text_counts(), vec![1, 1]);
            let first = d
                .record
                .requests
                .iter()
                .find(|r| r.step == "screen:caller_text:0")
                .unwrap();
            assert_eq!(first.result, RequestResult::NotSupplied);
            assert!(matches!(
                first.attempts[0].cancellation,
                Cancellation::Requested { .. }
            ));
            if on_cancel == OnCancel::LateScreen {
                assert_eq!(first.attempts[0].end, AttemptEnd::Output);
                assert_eq!(first.screen.as_ref().unwrap().units.reported, Some(7));
            }
            if on_cancel == OnCancel::Ignore {
                assert!(d.record.cost.liability > 0);
            }
            let withdrawn = d
                .record
                .requests
                .iter()
                .filter(|r| r.result == RequestResult::Withdrawn)
                .collect::<Vec<_>>();
            assert_eq!(withdrawn.len(), 2);
            for r in withdrawn {
                assert!(r.attempts.is_empty());
                assert_eq!(r.screen.as_ref().unwrap().units.charged, 0);
            }
            assert_record(&d);
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn unavailable_and_invalid_screen_outputs_are_never_cached() {
    for invalid in [true, false] {
        let descriptor = descriptor(true);
        let backend = Scripted::new(descriptor.clone(), move |_| {
            if invalid {
                Answer::Output(
                    RawOutput::Screen {
                        blocked: true,
                        categories: vec!["undeclared".into()],
                        units: ScreenUnits::Reported { n: 4 },
                    },
                    Charge::Observed { units: 1 },
                )
            } else {
                Answer::Fail(
                    rustev_core::seams::AdapterFailure::Permanent,
                    CANARY.into(),
                    Charge::Observed { units: 1 },
                )
            }
        });
        let rig = rig(config(1, 0, 1), &[(backend.clone(), 10)]);
        let plan = rig
            .rt
            .prepare(
                compile_with(&definition(true), &[descriptor], &[], &policy(false, true)).unwrap(),
            )
            .unwrap();
        let d = rig
            .rt
            .decide(
                &plan,
                req("unavailable", true, CANARY),
                &CancelSignal::new(),
            )
            .await
            .unwrap();
        assert_eq!(backend.text_counts(), vec![1; 10]);
        for r in &d.record.requests {
            assert_eq!(r.screen.as_ref().unwrap().cache, ScreenCache::Miss);
        }
        assert_record(&d);
    }
}

struct Sink3(Arc<ScriptedSink>);
impl rustev_core::seams::EvidenceSink for Sink3 {
    fn accepts(&self, schema: &str) -> bool {
        schema == rustev_contract::schema::RUN_V3
    }
    fn deliver<'a>(
        &'a self,
        record: &'a RunRecord,
    ) -> rustev_core::seams::BoxFuture<'a, Result<String, String>> {
        let handle = SinkHandle(self.0.clone());
        Box::pin(async move { handle.deliver(record).await })
    }
}
fn rig(config: rustev_runtime::RuntimeConfig, backends: &[(Arc<Scripted>, usize)]) -> Rig {
    let clock = rustev_runtime::ManualClock::new();
    let sink = ScriptedSink::new(SinkAnswer::Ack);
    let mut builder = rustev_runtime::Runtime::builder(
        Arc::new(clock.clone()),
        Arc::new(Sink3(sink.clone())),
        config,
    );
    for (backend, limit) in backends {
        builder = builder.backend(Arc::new(Handle(backend.clone())), *limit);
    }
    Rig {
        clock,
        sink,
        rt: Arc::new(builder.build().unwrap()),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_shared_batch_refusal_never_supplies_later_members() {
    let descriptor = descriptor(false);
    let backend = Scripted::new(descriptor.clone(), |call| {
        answer(
            call.attempt_id.contains("behaviour"),
            ScreenUnits::NotReported,
        )
    })
    .with_batch();
    let rig = rig(config(1, 0, 4), &[(backend.clone(), 1)]);
    let plan = rig
        .rt
        .prepare(compile_with(&definition(false), &[descriptor], &[], &policy(true, true)).unwrap())
        .unwrap();
    let d = rig
        .rt
        .decide(
            &plan,
            req("batch-refusal", false, CANARY),
            &CancelSignal::new(),
        )
        .await
        .unwrap();
    assert_eq!(backend.text_counts(), vec![4]);
    assert_eq!(
        d.record
            .requests
            .iter()
            .filter(|r| r.result == RequestResult::NotSupplied)
            .count(),
        2
    );
    let CoreEvidence::Recorded(e) = &d.record.core else {
        panic!("no evidence")
    };
    assert_eq!(e.screening.as_ref().unwrap().len(), 2);
    assert_record(&d);
}
