//! Binding live preconditions (spec 067, increment 1).

use std::collections::BTreeMap;

use rustev_contract::Identified;
use rustev_contract::live::{AuthorizationReference, AuthorizationRequest};
use rustev_contract::run::CostModel;
use rustev_core::Compiled;
use rustev_live::documents::{Candidates, LiveConfig, LiveError, Mode};
use rustev_runtime::{DispatchAnswer, LiveAuthorizer, Runtime};

/// Spec 067, 3.2.4: live refusals are outcome values, not exceptions.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct LiveRefusal {
    pub code: u8,
    pub subject: String,
    pub detail: String,
}

impl LiveRefusal {
    fn invalid(subject: &str, detail: impl Into<String>) -> Self {
        Self {
            code: 4,
            subject: subject.into(),
            detail: detail.into(),
        }
    }
}

/// Spec 067, 3.2.4: preserve the CLI's invalid-input versus I/O split.
impl From<LiveError> for LiveRefusal {
    fn from(error: LiveError) -> Self {
        Self {
            code: if error.status == "io_error" { 3 } else { 4 },
            subject: error.subject.into(),
            detail: error.detail,
        }
    }
}

/// Spec 067, 3.5.2: the binding requires rules and all CLI configuration checks.
pub(crate) fn check_config(
    config: &LiveConfig,
    candidates: &Candidates,
) -> Result<(), LiveRefusal> {
    config.check(Mode::Cli, candidates).map_err(Into::into)
}

/// Spec 067, 3.5 and 2.1: rules decide one internal dispatch, never external authority.
pub(crate) struct RuleAuthorizer {
    config: LiveConfig,
    id: String,
}

impl RuleAuthorizer {
    /// Spec 067, 3.5.1: references bind the exact configuration's identity.
    pub(crate) fn new(config: LiveConfig) -> Result<Self, LiveRefusal> {
        let id = config
            .id()
            .map_err(|e| LiveRefusal::invalid("live-config", e.to_string()))?;
        Ok(Self {
            config,
            id: id.to_string(),
        })
    }
}

impl LiveAuthorizer for RuleAuthorizer {
    fn authorize(&self, request: &AuthorizationRequest) -> Result<Option<DispatchAnswer>, String> {
        let matched = self
            .config
            .authorize
            .as_deref()
            .unwrap_or_default()
            .iter()
            .enumerate()
            .find(|(_, r)| r.backend_id == request.backend_id && r.artifact == request.artifact);
        let Some((index, rule)) = matched else {
            return Ok(Some(DispatchAnswer::Refuse {
                reference: None,
                detail: "authorization_refused".into(),
            }));
        };
        let reference = AuthorizationReference::new(format!("{}#{index}", self.id))
            .map_err(|e| e.to_string())?;
        Ok(Some(
            if rule.operations.contains(&request.operation)
                && request.exposure.reserved_units <= rule.max_exposure_units
            {
                DispatchAnswer::Allow {
                    answers: request.digest().map_err(|e| e.to_string())?,
                    reference,
                    valid_for_ms: request.remaining_ms,
                }
            } else {
                DispatchAnswer::Refuse {
                    reference: Some(reference),
                    detail: "authorization_refused".into(),
                }
            },
        ))
    }
}

/// Spec 067, 3.3.3, 3.3.4 and I-1: static checks before opening any journal.
/// Order: configuration binding order, registration, unknown cost, identity;
/// then CLI configuration checks including equality with the reached union.
/// No projection bound is queried, and `cost_unit: none` remains accepted.
pub(crate) fn check_backends(
    runtime: &Runtime,
    config: &LiveConfig,
    plans: &BTreeMap<String, Compiled>,
) -> Result<(), LiveRefusal> {
    for binding in &config.bindings {
        let identity = &binding.identity;
        let Some((descriptor, model)) = runtime.backend_terms(&identity.backend_id) else {
            return Err(LiveRefusal::invalid(
                "live-config",
                format!("{} is not registered", identity.backend_id),
            ));
        };
        if model == CostModel::Unknown {
            return Err(LiveRefusal::invalid(
                "spend-unit",
                format!("{} discloses an unknown cost model", identity.backend_id),
            ));
        }
        if descriptor.backend_id != identity.backend_id || descriptor.artifact != identity.artifact
        {
            return Err(LiveRefusal::invalid(
                "live-config",
                format!("{} descriptor identity differs", identity.backend_id),
            ));
        }
    }
    let candidates = plans
        .iter()
        .map(|(name, plan)| {
            (
                name.clone(),
                rustev_live::probe::reached(plan).into_iter().collect(),
            )
        })
        .collect();
    check_config(config, &candidates)
}

#[derive(Clone, Copy)]
pub(crate) enum HostKind {
    Filesystem,
    Memory,
}

/// Spec 067, 3.4.2: memory is a thrown usage refusal, not a live outcome.
pub(crate) fn check_durable_paths(host: HostKind) -> napi::Result<()> {
    match host {
        HostKind::Filesystem => Ok(()),
        HostKind::Memory => Err(crate::errors::fail(
            "InvalidDocument",
            "durable-paths-required",
            "live runs require a filesystem host",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustev_contract::definition::*;
    use rustev_contract::descriptor::*;
    use rustev_contract::ids::{ArtifactId, RequestId};
    use rustev_contract::live::{AttemptRole, Exposure};
    use rustev_contract::run::{CostBound, RunRecord};
    use rustev_contract::scope::{Handle, Scope};
    use rustev_core::builder::{DefinitionBuilder, out, ty};
    use rustev_core::seams::{
        AttemptCall, AttemptReport, BoxFuture, DecisionBackend, EvidenceSink,
    };
    use rustev_live::documents::{AuthorizationRule, CostUnit};
    use rustev_runtime::{ManualClock, RuntimeConfig, SinkPolicy};
    use serde_json::json;
    use std::sync::Arc;

    fn digest(c: char) -> String {
        format!("sha256:{}", c.to_string().repeat(64))
    }

    fn descriptor() -> BackendDescriptor {
        BackendDescriptor {
            schema: rustev_contract::schema::BACKEND.into(),
            backend_id: "synthetic".into(),
            artifact: ArtifactId::parse(&digest('a')).unwrap(),
            operations: vec![OperationSupport {
                operation: Operation::Classify,
                output: OutputKind::Label,
                max_options: 2,
                isolation: None,
                screen: None,
            }],
            input_limit: InputLimit {
                max_bytes: 65_536,
                on_excess: InputExcess::Refuse,
            },
            determinism: Determinism::Unspecified,
            ranking: None,
        }
    }

    fn plans() -> BTreeMap<String, Compiled> {
        let definition = DefinitionBuilder::new("synthetic", "1.0.0", "synthetic")
            .input(
                "text",
                ty::text(32),
                &[ProvenanceClass::UserSupplied],
                Freshness::NotRequired,
            )
            .semantic(
                "label",
                SemanticDecl {
                    operation: Operation::Classify,
                    task: "synthetic".into(),
                    question: "Choose a label".into(),
                    options: vec!["a".into(), "b".into()],
                    candidates: rustev_contract::definition::Candidates::None,
                    requires: RequiredKind::Label,
                    project: vec!["input:text".into()],
                    for_each: vec![],
                    backend: BackendReq {
                        min_determinism: Determinism::Unspecified,
                        artifact: ArtifactPin::Any,
                        binding: BindingChoice::Auto,
                    },
                    calibration: CalibrationRef::None,
                    fallback: Fallback::None,
                },
            )
            .on_unresolved("step:label", ReasonSet::Any, HandlerAction::Propagate)
            .rule("ok", Cond::Always, out::propose("ok", &[]))
            .limits(LimitsDecl {
                acquisition: None,
                max_semantic_requests: 1,
                on_excess: ExcessPolicy::Refuse,
                max_projection_bytes: 65_536,
                distribution_tolerance: rustev_contract::decimal::Decimal::parse("0.01").unwrap(),
                deadline_ms: 1000,
            })
            .build()
            .unwrap();
        BTreeMap::from([(
            "candidate".into(),
            rustev_core::compile(&definition, &[descriptor()], &[]).unwrap(),
        )])
    }

    fn config() -> LiveConfig {
        LiveConfig::parse(&serde_json::to_vec(&json!({
            "schema":"rustev.live-config/1", "budget":"testing", "run_cap_units":100,
            "repeats":1, "evaluation_time":"source", "bindings":[{
                "identity":{"backend_id":"synthetic","artifact":digest('a')}, "cost_unit":"none",
                "max_in_flight":1,"min_interval_ms":0,"case_deadline_ms":1000,"rate_limit_pause_max_ms":0
            }], "acquisitions":{}, "probe":{"calls":1,"interval_ms":0,"snapshot":"synthetic",
                "digest":digest('b'),"provenance":"synthetic"},
            "authorize":[{"backend_id":"synthetic","artifact":digest('a'),"operations":["classify"],"max_exposure_units":10}]
        })).unwrap()).unwrap()
    }

    struct Scripted {
        descriptor: BackendDescriptor,
        model: CostModel,
    }
    impl DecisionBackend for Scripted {
        fn descriptor(&self) -> &BackendDescriptor {
            &self.descriptor
        }
        fn cost_model(&self) -> CostModel {
            self.model
        }
        fn cost_bound(&self, _: &[u8]) -> CostBound {
            match self.model {
                CostModel::Bounded => CostBound::Bounded { max_units: 10 },
                CostModel::Estimated => CostBound::Estimated { units: 10 },
                CostModel::Unknown => CostBound::Unknown,
            }
        }
        fn infer<'a>(&'a self, _: AttemptCall<'a>) -> BoxFuture<'a, AttemptReport> {
            panic!("preconditions must never dispatch")
        }
    }
    struct Sink;
    impl EvidenceSink for Sink {
        fn deliver<'a>(&'a self, _: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
            panic!("preconditions must never deliver")
        }
    }
    fn runtime(backends: Vec<(BackendDescriptor, CostModel)>) -> Runtime {
        let mut builder = Runtime::builder(
            Arc::new(ManualClock::new()),
            Arc::new(Sink),
            RuntimeConfig {
                max_in_flight: 1,
                max_queued: 1,
                max_parallel_requests: 1,
                sink: SinkPolicy::FailDecision { timeout_ms: 1000 },
            },
        );
        for (descriptor, model) in backends {
            builder = builder.backend(Arc::new(Scripted { descriptor, model }), 1);
        }
        builder.build().unwrap()
    }
    fn refused<T>(result: Result<T, LiveRefusal>, code: u8, subject: &str) -> LiveRefusal {
        let error = result.err().expect("refusal");
        assert_eq!(error.code, code);
        assert_eq!(error.subject, subject);
        error
    }
    struct Dir(std::path::PathBuf);
    impl Dir {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "rustev-node-live-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn unknown_cost_precedes_identity_and_reachability_without_opening_journal() {
        let dir = Dir::new();
        let path = dir.0.join("journal");
        let mut c = config();
        c.bindings[0].identity.artifact = ArtifactId::parse(&digest('c')).unwrap();
        refused(
            check_backends(
                &runtime(vec![(descriptor(), CostModel::Unknown)]),
                &c,
                &plans(),
            ),
            4,
            "spend-unit",
        );
        assert!(!path.exists());
        assert!(!dir.0.join("journal.lock").exists());
    }

    #[test]
    fn backend_refusals_and_configuration_order() {
        let rt = runtime(vec![(descriptor(), CostModel::Bounded)]);
        let mut c = config();
        c.bindings.clear();
        refused(check_backends(&rt, &c, &plans()), 4, "live-config");
        refused(
            check_backends(&rt, &config(), &BTreeMap::new()),
            4,
            "live-config",
        );
        refused(
            check_backends(&runtime(vec![]), &config(), &plans()),
            4,
            "live-config",
        );
        let mut changed = descriptor();
        changed.artifact = ArtifactId::parse(&digest('c')).unwrap();
        let error = refused(
            check_backends(
                &runtime(vec![(changed, CostModel::Bounded)]),
                &config(),
                &plans(),
            ),
            4,
            "live-config",
        );
        assert!(error.detail.contains("descriptor identity"));
        let rt = runtime(vec![(descriptor(), CostModel::Unknown)]);
        let mut c = config();
        let mut absent = c.bindings[0].clone();
        absent.identity.backend_id = "absent".into();
        c.bindings.insert(0, absent);
        refused(check_backends(&rt, &c, &plans()), 4, "live-config");
        c.bindings.swap(0, 1);
        refused(check_backends(&rt, &c, &plans()), 4, "spend-unit");
    }

    #[test]
    fn declared_no_spend_and_configuration_ranges() {
        let mut c = config();
        assert_eq!(c.bindings[0].cost_unit, CostUnit::None);
        for model in [CostModel::Bounded, CostModel::Estimated] {
            check_backends(&runtime(vec![(descriptor(), model)]), &c, &plans()).unwrap();
        }
        c.authorize = None;
        refused(
            check_backends(
                &runtime(vec![(descriptor(), CostModel::Bounded)]),
                &c,
                &plans(),
            ),
            4,
            "live-config",
        );
        c = config();
        c.bindings[0].max_in_flight = 0;
        refused(
            check_backends(
                &runtime(vec![(descriptor(), CostModel::Bounded)]),
                &c,
                &plans(),
            ),
            4,
            "live-config",
        );
    }

    fn request() -> AuthorizationRequest {
        AuthorizationRequest {
            decision_id: "synthetic".into(),
            subject: Handle::new("subject").unwrap(),
            scope: Scope::principal(
                Handle::new("tenant").unwrap(),
                Handle::new("revision").unwrap(),
                Handle::new("scope").unwrap(),
            ),
            step: "label".into(),
            instance: vec![],
            operation: Operation::Classify,
            target: 0,
            role: AttemptRole::Primary,
            backend_id: "synthetic".into(),
            artifact: descriptor().artifact,
            request: RequestId::parse(&digest('b')).unwrap(),
            attempt_id: "synthetic/1".into(),
            exposure: Exposure {
                bound: CostBound::Bounded { max_units: 10 },
                reserved_units: 10,
            },
            remaining_ms: 123,
        }
    }
    fn assert_refused(
        auth: &RuleAuthorizer,
        request: &AuthorizationRequest,
        reference: Option<String>,
    ) {
        let Some(DispatchAnswer::Refuse {
            reference: actual,
            detail,
        }) = auth.authorize(request).unwrap()
        else {
            panic!("expected refusal")
        };
        assert_eq!(actual.map(|r| r.as_str().to_owned()), reference);
        assert_eq!(detail, "authorization_refused");
    }

    #[test]
    fn rules_match_cli_answers_references_and_first_match() {
        let mut c = config();
        let mut unrelated: AuthorizationRule = c.authorize.as_ref().unwrap()[0].clone();
        unrelated.artifact = ArtifactId::parse(&digest('c')).unwrap();
        c.authorize.as_mut().unwrap().insert(0, unrelated);
        let reference = format!("{}#1", c.id().unwrap());
        let auth = RuleAuthorizer::new(c.clone()).unwrap();
        let mut r = request();
        for units in [0, 9, 10] {
            r.exposure.reserved_units = units;
            let Some(DispatchAnswer::Allow {
                answers,
                reference: actual,
                valid_for_ms,
            }) = auth.authorize(&r).unwrap()
            else {
                panic!("expected allow")
            };
            assert_eq!(answers, r.digest().unwrap());
            assert_eq!(actual.as_str(), reference);
            assert_eq!(valid_for_ms, r.remaining_ms);
        }
        r.exposure.reserved_units = 11;
        assert_refused(&auth, &r, Some(reference.clone()));
        r.exposure.reserved_units = 0;
        r.operation = Operation::Rubric;
        assert_refused(&auth, &r, Some(reference.clone()));
        r = request();
        r.backend_id = "other".into();
        assert_refused(&auth, &r, None);
        r = request();
        r.artifact = ArtifactId::parse(&digest('d')).unwrap();
        assert_refused(&auth, &r, None);
        let mut later = c.authorize.as_ref().unwrap()[1].clone();
        later.max_exposure_units = 100;
        c.authorize.as_mut().unwrap().push(later);
        let first_ref = format!("{}#1", c.id().unwrap());
        r = request();
        r.exposure.reserved_units = 11;
        assert_refused(&RuleAuthorizer::new(c).unwrap(), &r, Some(first_ref));
        c = config();
        c.authorize = Some(vec![]);
        let auth = RuleAuthorizer::new(c).unwrap();
        for units in [0, 10, u64::MAX] {
            r = request();
            r.exposure.reserved_units = units;
            assert_refused(&auth, &r, None);
        }
    }

    #[test]
    fn outcome_mapping_and_durable_host_usage_error() {
        for (status, code) in [("invalid_input", 4), ("io_error", 3)] {
            let error = LiveRefusal::from(LiveError {
                status,
                subject: "checkpoint",
                detail: "synthetic".into(),
            });
            assert_eq!(
                error,
                LiveRefusal {
                    code,
                    subject: "checkpoint".into(),
                    detail: "synthetic".into()
                }
            );
        }
        check_durable_paths(HostKind::Filesystem).unwrap();
        let error = check_durable_paths(HostKind::Memory).unwrap_err();
        let value: serde_json::Value = serde_json::from_str(&error.reason).unwrap();
        assert_eq!(value["class"], "InvalidDocument");
        assert_eq!(value["code"], "durable-paths-required");
    }
}
