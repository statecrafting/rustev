//! Fixtures shared by the rustev-otel tests: the reference run records of
//! spec 006, the plans they ran, synthetic exchange records, the fixed test
//! key, anchor and id seed of spec 028 3.5.8. SYNTHETIC only.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rustev_contract::plan::Plan;
use rustev_contract::redaction::{
    Action, DigestKey, FieldClass, FieldOverride, RedactionPolicy, Redactor,
};
use rustev_contract::remote::ExchangeRecord;
use rustev_contract::run::RunRecord;
use rustev_contract::trace_fields::{self, ClassRule};
use rustev_contract::{Document, Identified};
use rustev_otel::{
    Anchor, AnchorKind, Bindings, IdSource, Projected, ProjectionInput, RootIds, SeededIds,
    UsageKeys, project,
};

pub fn here(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Spec 006's committed run record of `task` (`support` or `lodging`).
pub fn record(task: &str) -> RunRecord {
    let p = here(&format!(
        "../../crates/rustev-cli/tests/golden/{task}.record.json"
    ));
    let mut bytes = read(&p);
    while bytes.last() == Some(&b'\n') {
        bytes.pop();
    }
    RunRecord::parse(&bytes).unwrap()
}

/// The plan that produced `task`'s record; its identity is the record's.
pub fn plan(task: &str) -> Plan {
    let p = Plan::parse(&read(&here(&format!("tests/fixtures/{task}.plan.json")))).unwrap();
    assert_eq!(p.id().unwrap(), record(task).plan_id, "{task} plan fixture");
    p
}

/// The synthetic exchange records joined with the support record, by
/// attempt id.
pub fn exchanges() -> BTreeMap<String, Arc<ExchangeRecord>> {
    let text =
        String::from_utf8(read(&here("tests/fixtures/support-remote.exchanges.jsonl"))).unwrap();
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let x = Arc::new(ExchangeRecord::parse(line.as_bytes()).unwrap());
        for m in &x.members {
            out.insert(m.attempt_id.clone(), x.clone());
        }
    }
    out
}

/// The host bindings of the remote golden: a provider name and usage keys
/// for the support backend. Rustev names no vendor.
pub fn remote_bindings() -> Bindings {
    let mut b = Bindings::default();
    b.providers
        .insert("synthetic-rules-support".into(), "example-provider".into());
    b.usage_keys.insert(
        "synthetic-rules-support".into(),
        UsageKeys {
            input: Some("input_units".into()),
            output: Some("output_units".into()),
        },
    );
    b
}

/// The fixed 32-byte test key, `key_id` `test`.
pub fn test_key() -> DigestKey {
    DigestKey::new("test", &(0u8..32).collect::<Vec<u8>>()).unwrap()
}

/// The fixed anchor: 2026-09-23T12:00:00Z.
pub const ANCHOR_NANOS: u64 = 1_790_164_800_000_000_000;

/// An all-`allow` test policy for the names `extra` adds to the catalogue:
/// every class that may be granted class-wide is, and every other field is
/// allowed by name (3.4.2 permits no class-wide grant of a deny class).
pub fn allow_all(extra: &[&str]) -> RedactionPolicy {
    let mut p = RedactionPolicy::default();
    for c in FieldClass::ALL {
        if !c.is_deny() {
            p.classes.insert(c, Action::Allow);
        }
    }
    let mut names: Vec<String> = trace_fields::CATALOGUE
        .iter()
        .filter(|e| !e.template && e.rule != ClassRule::Fixed)
        .map(|e| e.name.to_string())
        .collect();
    names.extend(extra.iter().map(|s| s.to_string()));
    p.fields = names
        .into_iter()
        .map(|field| FieldOverride {
            field,
            action: Action::Allow,
        })
        .collect();
    p.validate().unwrap();
    p
}

/// The template instances a record needs allowed by name.
pub fn instances(rec: &RunRecord, xs: &BTreeMap<String, Arc<ExchangeRecord>>) -> Vec<String> {
    let mut out = vec![];
    if let rustev_contract::run::CoreEvidence::Recorded(ev) = &rec.core
        && let rustev_contract::judgment::Outcome::Propose { params, .. } = &ev.judgment.outcome
    {
        for k in params.keys() {
            out.push(format!("{}{k}", trace_fields::PARAM_PREFIX));
        }
    }
    for x in xs.values() {
        for k in x.provider_extras.keys() {
            let n = format!("{}{k}", trace_fields::EXTRA_PREFIX);
            if !out.contains(&n) {
                out.push(n);
            }
        }
    }
    out
}

/// Project `rec` under the golden conditions: fixed key, anchor and seed.
pub fn project_golden(
    rec: &RunRecord,
    plan: Option<&Plan>,
    xs: &BTreeMap<String, Arc<ExchangeRecord>>,
    bindings: &Bindings,
    policy: RedactionPolicy,
) -> Projected {
    let redactor = Redactor::new(policy, Some(test_key())).unwrap();
    let mut ids = SeededIds::new(rec.decision_id.as_bytes());
    let root = RootIds {
        trace_id: ids.trace_id(),
        span_id: ids.span_id(),
        parent_span_id: None,
    };
    project(
        &ProjectionInput {
            record: rec,
            plan,
            exchanges: xs,
            bindings,
            anchor: Anchor {
                unix_nanos: ANCHOR_NANOS,
                kind: AnchorKind::Delivery,
            },
            root,
            value_bound: 4096,
        },
        &redactor,
        &mut ids,
    )
    .unwrap()
}

/// Compare `bytes` with the committed golden `name`; `RUSTEV_BLESS=1`
/// rewrites it.
pub fn golden(name: &str, bytes: &[u8]) {
    let path = here(&format!("tests/golden/{name}"));
    if std::env::var_os("RUSTEV_BLESS").is_some() {
        std::fs::write(&path, bytes).unwrap();
        return;
    }
    let want = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("{}: {e}; run with RUSTEV_BLESS=1", path.display()));
    assert!(
        want == bytes,
        "{name} differs from its golden; run with RUSTEV_BLESS=1 to review"
    );
}

/// The reference decision of `task` run through the runtime with the rules
/// backend, delivering to `sink` under `policy` (spec 003's sink policies).
pub mod run {
    use std::sync::Arc;

    use rustev_backend_rules::RulesBackend;
    use rustev_contract::Document;
    use rustev_contract::calibration::CalibrationArtifact;
    use rustev_contract::snapshot::Snapshot;
    use rustev_contract::time::Timestamp;
    use rustev_core::Compiled;
    use rustev_core::seams::{CancelSignal, EvidenceSink};
    use rustev_runtime::{
        DecideError, Decided, DecisionRequest, Runtime, RuntimeConfig, SinkPolicy, TokioClock,
    };

    use super::{here, plan};

    fn rules(task: &str) -> RulesBackend {
        let name = match task {
            "support" => "support-routing.rules.json",
            _ => "lodging.rules.json",
        };
        let p = here(&format!(
            "../../backends/rustev-backend-rules/tests/fixtures/{name}"
        ));
        RulesBackend::from_bytes(&std::fs::read(p).unwrap()).unwrap()
    }

    /// The runtime and prepared plan for `task`.
    pub fn runtime(
        task: &str,
        sink: Arc<dyn EvidenceSink>,
        policy: SinkPolicy,
    ) -> (Runtime, rustev_runtime::PreparedPlan) {
        let backend = rules(task);
        let calibrations = if task == "support" {
            vec![
                CalibrationArtifact::parse(
                    &std::fs::read(here("tests/fixtures/topic.calibration.json")).unwrap(),
                )
                .unwrap(),
            ]
        } else {
            vec![]
        };
        use rustev_core::seams::DecisionBackend;
        let compiled = Compiled::load_checked(
            &plan(task),
            std::slice::from_ref(backend.descriptor()),
            &calibrations,
        )
        .unwrap();
        let runtime = Runtime::builder(
            Arc::new(TokioClock::new()),
            sink,
            RuntimeConfig {
                max_in_flight: 4,
                max_queued: 16,
                max_parallel_requests: 4,
                sink: policy,
            },
        )
        .backend(Arc::new(backend), 4)
        .build()
        .unwrap();
        let prepared = runtime.prepare(compiled).unwrap();
        (runtime, prepared)
    }

    /// One decision of `task` with id `decision_id`.
    pub async fn decide(
        runtime: &Runtime,
        prepared: &rustev_runtime::PreparedPlan,
        task: &str,
        decision_id: &str,
    ) -> Result<Decided, DecideError> {
        let snapshot = Snapshot::parse(
            &std::fs::read(here(&format!("tests/fixtures/{task}.snapshot.json"))).unwrap(),
        )
        .unwrap();
        runtime
            .decide(
                prepared,
                DecisionRequest {
                    decision_id: decision_id.into(),
                    snapshot,
                    evaluation_time: Timestamp::from_ms(1_790_164_800_000).unwrap(),
                    deadline_ms: None,
                    principal_handle: vec![],
                },
                &CancelSignal::new(),
            )
            .await
    }
}
