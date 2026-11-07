//! Spec 043's live provider stage (3.13.2; manual, never under cargo test):
//! a host composing this backend with `rustev_live::run` over the SYNTHETIC
//! dataset in `fixtures/live/`, reserved against the shared testing journal.
//! The definition classifies each case's synthetic text into one of three
//! fruit keys and proposes the result.
fn main() {
    #[cfg(feature = "aws-config")]
    let result = host::run();
    #[cfg(not(feature = "aws-config"))]
    let result: Result<u8, String> =
        Err("live run requires the aws-config feature; sent nothing".into());
    match result {
        Ok(code) => std::process::exit(code.into()),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}

#[cfg(feature = "aws-config")]
mod host {
    use rustev_bedrock::{
        BedrockBackend,
        adapter::Config,
        aws_config_provider::{StandardCredentials, live_args},
        binding::BedrockBinding,
        policy::ModelPolicy,
    };
    use rustev_contract::definition::{Definition, Operation, ProvenanceClass};
    use rustev_contract::ids::{ArtifactId, DatasetId};
    use rustev_contract::live::{AuthorizationReference, AuthorizationRequest};
    use rustev_contract::run::RunRecord;
    use rustev_contract::scope::{Handle, Scope};
    use rustev_contract::snapshot::{Entry, Snapshot};
    use rustev_contract::time::Timestamp;
    use rustev_contract::{Document, Identified, schema};
    use rustev_core::seams::{BoxFuture, CancelSignal, DecisionBackend, EvidenceSink};
    use rustev_live::documents::{LiveConfig, Mode};
    use rustev_live::journal::SystemClock;
    use rustev_live::probe::ProbeSnapshot;
    use rustev_live::run::{
        CaseInput, CaseOrigin, LiveCase, RunOptions, RunSpec, RunStatus, exit_code, prepare,
    };
    use rustev_runtime::{
        DispatchAnswer, LiveAuthorizer, Runtime, RuntimeConfig, SinkPolicy, TokioClock,
    };
    use serde_json::{Value, json};
    use sha2::{Digest, Sha256};
    use std::collections::BTreeMap;
    use std::sync::Arc;

    const DEFINITION: &[u8] = include_bytes!("../fixtures/live/definition.json");
    const CASES: &[u8] = include_bytes!("../fixtures/live/cases.json");
    const USAGE: &str = "usage: bedrock-live-run --binding FILE --policy FILE --materials FILE --journal FILE --out DIR --run-id ID --run-cap-units N [--resume]";

    /// Records stay in memory; `rustev_live` writes the run's documents.
    struct ReturnedRecord;
    impl EvidenceSink for ReturnedRecord {
        fn deliver<'a>(&'a self, record: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
            Box::pin(async move {
                record
                    .record_digest()
                    .map(|d| format!("live-memory:{d}"))
                    .map_err(|e| e.to_string())
            })
        }
        fn accepts(&self, schema: &str) -> bool {
            matches!(schema, schema::RUN | schema::RUN_V3)
        }
    }

    /// The host's own facts (spec 026 3.3): only `classify` on this one
    /// binding may be sent, and no single attempt may reserve more than the
    /// whole run cap. The run cap itself is held by `rustev_live`'s gate.
    struct Host {
        backend_id: String,
        artifact: ArtifactId,
        /// The run cap: one attempt never reserves more than the whole run.
        max_exposure_units: u64,
    }
    impl LiveAuthorizer for Host {
        fn authorize(&self, r: &AuthorizationRequest) -> Result<Option<DispatchAnswer>, String> {
            let reference = AuthorizationReference::new(format!(
                "SYNTHETIC host: work order 2026-10-16, {}",
                r.attempt_id
            ))
            .map_err(|e| e.to_string())?;
            Ok(Some(
                if r.backend_id == self.backend_id
                    && r.artifact == self.artifact
                    && r.operation == Operation::Classify
                    && r.exposure.reserved_units <= self.max_exposure_units
                {
                    DispatchAnswer::Allow {
                        answers: r.digest().map_err(|e| e.to_string())?,
                        reference,
                        valid_for_ms: r.remaining_ms,
                    }
                } else {
                    DispatchAnswer::Refuse {
                        reference: Some(reference),
                        detail: "outside the host's terms".into(),
                    }
                },
            ))
        }
    }

    fn snapshot(text: &Value, as_of: Timestamp) -> Result<Snapshot, String> {
        if !text.is_string() {
            return Err("case text".into());
        }
        Ok(Snapshot {
            schema: schema::SNAPSHOT.into(),
            entries: vec![Entry {
                field: "text".into(),
                value: text.clone(),
                provenance: ProvenanceClass::UserSupplied,
                as_of_ms: as_of,
                source: "synthetic".into(),
                grade: None,
                producer: None,
            }],
        })
    }

    pub fn run() -> Result<u8, String> {
        let args = live_args()?;
        let mut options = BTreeMap::new();
        let mut resume = false;
        let mut rest = args.iter();
        while let Some(flag) = rest.next() {
            if flag == "--resume" {
                resume = true;
                continue;
            }
            if !matches!(
                flag.as_str(),
                "--binding"
                    | "--policy"
                    | "--materials"
                    | "--journal"
                    | "--out"
                    | "--run-id"
                    | "--run-cap-units"
            ) {
                return Err(USAGE.into());
            }
            let value = rest.next().ok_or(USAGE)?;
            if options.insert(flag.as_str(), value.clone()).is_some() {
                return Err(USAGE.into());
            }
        }
        let value = |key: &str| options.get(key).cloned().ok_or_else(|| USAGE.to_string());
        let load = |key: &str| std::fs::read(value(key)?).map_err(|_| format!("cannot read {key}"));
        let binding = BedrockBinding::parse(&load("--binding")?)?;
        let policy = ModelPolicy::parse(&load("--policy")?)?;
        let materials = rustev_contract::bounded::parse_bounded(
            &load("--materials")?,
            &rustev_bedrock::binding::LIMITS,
        )
        .map_err(|e| e.to_string())?;
        // No default cap (spec 043 3.5.4): the host names it.
        let run_cap_units: u64 = value("--run-cap-units")?
            .parse()
            .map_err(|_| "invalid --run-cap-units")?;
        let run_id = value("--run-id")?;

        let backend = BedrockBackend::new(
            "bedrock-live".into(),
            binding,
            policy,
            materials,
            Config {
                credentials: Some(Arc::new(StandardCredentials)),
                ..Default::default()
            },
        )?;
        let descriptor = backend.descriptor().clone();
        let definition: Definition =
            serde_json::from_slice(DEFINITION).map_err(|e| e.to_string())?;
        let compiled = rustev_core::compile(&definition, std::slice::from_ref(&descriptor), &[])
            .map_err(|e| format!("{e:?}"))?;
        backend.check_plan(&compiled.plan)?;

        let cases: Value = serde_json::from_slice(CASES).map_err(|e| e.to_string())?;
        let as_of = Timestamp::from_ms(cases["as_of_ms"].as_i64().ok_or("as_of_ms")?)
            .map_err(|e| e.to_string())?;
        let probe = snapshot(&cases["probe"], as_of)?;
        let probe_bytes = probe.canonical().map_err(|e| e.to_string())?;
        let config = LiveConfig::parse(
            &serde_json::to_vec(&json!({
                "schema":"rustev.live-config/1", "budget":"testing", "run_cap_units":run_cap_units,
                "repeats":1, "evaluation_time":"source", "bindings":[{
                    "identity":{"backend_id":descriptor.backend_id,"artifact":descriptor.artifact},
                    "cost_unit":"nano_usd", "max_in_flight":1, "min_interval_ms":500,
                    "case_deadline_ms":60_000, "rate_limit_pause_max_ms":5_000
                }], "acquisitions":{},
                "probe":{"calls":1,"interval_ms":0,"snapshot":"fixtures/live/cases.json#probe",
                    "digest":probe.id().map_err(|e| e.to_string())?,"provenance":"synthetic"}
            }))
            .map_err(|e| e.to_string())?,
        )
        .map_err(|e| format!("{e:?}"))?;
        let probe_snapshot = ProbeSnapshot::from_bytes(&config.probe, &probe_bytes, &[])
            .map_err(|e| format!("{e:?}"))?;
        let mut live_cases = vec![];
        for (case_id, text) in cases["cases"].as_object().ok_or("cases")? {
            let s = snapshot(text, as_of)?;
            live_cases.push(LiveCase {
                case_id: case_id.clone(),
                input: CaseInput::Available {
                    source: CaseOrigin::DatasetCase,
                    bundle: None,
                    snapshot_bytes: s.canonical().map_err(|e| e.to_string())?,
                    snapshot_id: s.id().map_err(|e| e.to_string())?,
                    source_time: Some(as_of),
                    scope: None,
                },
            });
        }
        let handle = |s: &str| Handle::new(s).map_err(|e| e.to_string());
        let spec = RunSpec {
            acquisition_pages: vec![],
            source_comparison: None,
            reports: None,
            config,
            mode: Mode::Library,
            run_id: run_id.clone(),
            dataset_id: DatasetId::parse(&format!("sha256:{:x}", Sha256::digest(CASES)))
                .map_err(|e| e.to_string())?,
            split: "test".into(),
            candidates: BTreeMap::from([("bedrock".into(), compiled)]),
            cases: live_cases,
            probe_snapshot,
            probe_time: as_of,
            principal_handle: b"synthetic-tenant".to_vec(),
            subject: handle("synthetic-subject")?,
            scope: Scope::principal(
                handle("synthetic-tenant")?,
                handle("rev-1")?,
                handle("scope-synthetic")?,
            ),
            input_digests: BTreeMap::new(),
        };
        let prepared = prepare(
            spec,
            RunOptions {
                journal_path: value("--journal")?.into(),
                require_existing_journal: false,
                journal_clock: Arc::new(SystemClock),
                out: value("--out")?.into(),
                resume,
                rerun_interrupted: false,
                probe_only: false,
                run_deadline_ms: Some(600_000),
            },
        )
        .map_err(|e| format!("{e:?}"))?;
        let journal = prepared.open_journal().map_err(|e| format!("{e:?}"))?;
        let host = Host {
            backend_id: descriptor.backend_id.clone(),
            artifact: descriptor.artifact.clone(),
            max_exposure_units: run_cap_units,
        };
        let tokio = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        tokio.block_on(async {
            let clock = Arc::new(TokioClock::new());
            let runtime = Runtime::builder(
                clock.clone(),
                Arc::new(ReturnedRecord),
                RuntimeConfig {
                    max_in_flight: 1,
                    max_queued: 0,
                    max_parallel_requests: 1,
                    sink: SinkPolicy::FailDecision { timeout_ms: 5_000 },
                },
            )
            .backend(Arc::new(backend), 1)
            .build()
            .map_err(|e| format!("{e:?}"))?;
            let outcome = prepared
                .execute(
                    &runtime,
                    journal.clone(),
                    &host,
                    clock.as_ref(),
                    &CancelSignal::new(),
                )
                .await
                .map_err(|e| format!("{e:?}"))?;
            let status = serde_json::to_value(outcome.status).map_err(|e| e.to_string())?;
            println!(
                "{} {run_id} run_spend={} counts={}",
                status.as_str().unwrap_or("unknown"),
                journal.run_spend(&run_id),
                serde_json::to_string(&outcome.live_run.counts).map_err(|e| e.to_string())?
            );
            for (name, summary) in [
                ("journal before", &outcome.journal_before),
                ("journal after", &outcome.journal_after),
            ] {
                println!(
                    "{name}: {}",
                    serde_json::to_string(summary).map_err(|e| e.to_string())?
                );
            }
            Ok(if outcome.status == RunStatus::EvaluatedLive {
                0
            } else {
                exit_code(outcome.status)
            })
        })
    }
}
