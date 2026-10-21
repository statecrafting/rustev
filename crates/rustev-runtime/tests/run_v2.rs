//! Spec 020, 3.2: the runtime emits `rustev.run/2` only to a sink that
//! selected it, with one remote evidence entry per remote attempt in
//! attempt order, and the record projects to exactly what a version 1 sink
//! receives. A rejected version 2 record is an evidence delivery failure,
//! never retried as version 1, and changes no judgment. SYNTHETIC only.

mod common;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use common::*;
use rustev_contract::Document;
use rustev_contract::descriptor::BackendDescriptor;
use rustev_contract::ids::ArtifactId;
use rustev_contract::remote::ServedIdentity;
use rustev_contract::run::{
    ChargeProvenance, CostBound, CostModel, EvidenceUnavailable, ExchangeReference, RemoteEvidence,
    RemoteState, RunRecord,
};
use rustev_contract::schema;
use rustev_core::seams::{
    AttemptCall, AttemptReport, BoxFuture, CancelSignal, DecisionBackend, EvidenceSink,
};
use rustev_runtime::{DecideError, ManualClock, Runtime};

const A: &str = "sha256:00000000000000000000000000000000000000000000000000000000000000aa";

/// A SYNTHETIC remote-looking backend: the scripted head, keeping a remote
/// evidence entry per attempt for the runtime to take once.
struct Remote {
    inner: Handle,
    kept: Mutex<BTreeMap<String, RemoteEvidence>>,
    taken: Mutex<Vec<String>>,
}

impl DecisionBackend for Remote {
    fn descriptor(&self) -> &BackendDescriptor {
        self.inner.descriptor()
    }
    fn cost_model(&self) -> CostModel {
        self.inner.cost_model()
    }
    fn cost_bound(&self, p: &[u8]) -> CostBound {
        self.inner.cost_bound(p)
    }
    fn infer<'a>(&'a self, call: AttemptCall<'a>) -> BoxFuture<'a, AttemptReport> {
        self.kept.lock().unwrap().insert(
            call.attempt_id.to_string(),
            RemoteEvidence {
                attempt_id: call.attempt_id.to_string(),
                // The runtime sets these three.
                step: "SYNTHETIC-wrong".into(),
                instance: vec!["SYNTHETIC-wrong".into()],
                remote_state: RemoteState::Stopped,
                exchange: ExchangeReference::Unavailable {
                    reason: EvidenceUnavailable::NoLocator,
                },
                requested_identity: None,
                served: ServedIdentity::unknown(),
                binding: ArtifactId::parse(A).unwrap(),
                protocol: "rustev.remote/1".into(),
                endpoint_class: "synthetic".into(),
                code: None,
                charge_provenance: ChargeProvenance::Reported,
                batch: None,
                principal_digest: None,
            },
        );
        self.inner.infer(call)
    }
    fn take_remote_evidence(&self, attempt_id: &str) -> Option<RemoteEvidence> {
        self.taken.lock().unwrap().push(attempt_id.to_string());
        self.kept.lock().unwrap().remove(attempt_id)
    }
}

/// The scripted sink, selecting `rustev.run/2`.
struct V2(SinkHandle);

impl EvidenceSink for V2 {
    fn deliver<'a>(&'a self, record: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
        self.0.deliver(record)
    }
    fn accepts(&self, schema: &str) -> bool {
        schema == schema::RUN_V2
    }
    fn run_schema(&self) -> &'static str {
        schema::RUN_V2
    }
}

fn runtime(v2: bool, answer: SinkAnswer) -> (Arc<Runtime>, Arc<ScriptedSink>, Arc<Remote>) {
    let sink = ScriptedSink::new(answer);
    let remote = Arc::new(Remote {
        inner: Handle(answering(linear_head())),
        kept: Mutex::new(BTreeMap::new()),
        taken: Mutex::new(vec![]),
    });
    let s: Arc<dyn EvidenceSink> = if v2 {
        Arc::new(V2(SinkHandle(sink.clone())))
    } else {
        Arc::new(SinkHandle(sink.clone()))
    };
    let mut c = config(8, 0, 3);
    c.sink = rustev_runtime::SinkPolicy::FailDecision { timeout_ms: 100 };
    let rt = Runtime::builder(Arc::new(ManualClock::new()), s, c)
        .backend(remote.clone(), 64)
        .build()
        .unwrap();
    (Arc::new(rt), sink, remote)
}

async fn decide(rt: &Arc<Runtime>, id: &str) -> Result<rustev_runtime::Decided, DecideError> {
    let plan = rt.prepare(support_compiled(None)).unwrap();
    rt.decide(&plan, request(id), &CancelSignal::new()).await
}

#[tokio::test(flavor = "current_thread")]
async fn a_version_1_sink_receives_version_1_and_nothing_is_taken() {
    let (rt, sink, remote) = runtime(false, SinkAnswer::Ack);
    decided(Ok(decide(&rt, "v1").await.unwrap()));
    let r = sink.received().remove(0);
    assert_eq!(r.schema, schema::RUN);
    assert!(
        !String::from_utf8(r.canonical().unwrap())
            .unwrap()
            .contains("remote_evidence")
    );
    assert!(remote.taken.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn a_version_2_sink_receives_one_entry_per_remote_attempt_in_order() {
    let (rt1, sink1, _) = runtime(false, SinkAnswer::Ack);
    let (rt2, sink2, remote) = runtime(true, SinkAnswer::Ack);
    decide(&rt1, "same").await.unwrap();
    decide(&rt2, "same").await.unwrap();
    let (v1, v2) = (sink1.received().remove(0), sink2.received().remove(0));
    assert_eq!(v2.schema, schema::RUN_V2);
    v2.check_version_members().unwrap();
    let entries = v2.remote_evidence.clone().unwrap();
    let attempts: Vec<_> = v2
        .requests
        .iter()
        .flat_map(|r| r.attempts.iter().map(move |a| (r, a)))
        .collect();
    assert_eq!(entries.len(), attempts.len());
    for (e, (r, a)) in entries.iter().zip(&attempts) {
        assert_eq!(e.attempt_id, a.attempt_id);
        assert_eq!((&e.step, &e.instance), (&r.step, &r.instance));
        assert_eq!(e.remote_state, a.remote);
    }
    assert!(remote.kept.lock().unwrap().is_empty(), "each taken once");
    // The projection is exactly what the version 1 sink received.
    assert_eq!(
        v2.project_v1().unwrap().canonical().unwrap(),
        v1.canonical().unwrap()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_rejected_version_2_record_is_never_retried_as_version_1() {
    let (rt, sink, _) = runtime(true, SinkAnswer::Fail);
    let (rt_ok, ok_sink, _) = runtime(true, SinkAnswer::Ack);
    decide(&rt_ok, "j").await.unwrap();
    match decide(&rt, "j").await {
        Err(DecideError::EvidenceNotDelivered { record, .. }) => {
            assert_eq!(record.schema, schema::RUN_V2);
            // The judgment is the one a delivered record carries.
            assert_eq!(record.core, ok_sink.received()[0].core);
        }
        other => panic!("{other:?}"),
    }
    let got = sink.received();
    assert_eq!(got.len(), 1, "delivered once");
    assert_eq!(got[0].schema, schema::RUN_V2, "never as version 1");
}
