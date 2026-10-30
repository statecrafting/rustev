//! Explicit host-constructed decision backend. No retry, batching or authority.
use crate::{
    binding::{BedrockBinding, Wire},
    budget::Budget,
    cancel::{cancel_outcome, cancelled_report},
    exchange::{
        DiscardExchanges, ExchangeSink, Exchanges, RetainedBytes, blank_record, output_digest,
    },
    mapping::{self, Materials},
    net::{Reply, Transport, host_header, model_path},
    policy::ModelPolicy,
    signing::{Clock, CredentialProvider, SystemClock, sign_request},
    taxonomy::{RemoteFailure, code_of_detail},
    wire::{SentTracker, digest_bytes},
};
use rustev_contract::Identified;
use rustev_contract::{
    definition::{Determinism, Operation},
    descriptor::{BackendDescriptor, InputExcess, InputLimit, OperationSupport, OutputKind},
    remote::{
        CancelSupport, Disclosure, ExchangeMember, MemberOutcome, RemoteCode, ServedIdentity,
    },
    run::{Charge, CostBound, CostModel},
    schema,
};
use rustev_core::seams::{
    AttemptCall, AttemptReport, BoxFuture, CancelAck, DecisionBackend, RemoteEnd,
};
use std::sync::{Arc, RwLock};

pub struct Config {
    pub endpoint: Option<String>,
    pub credentials: Option<Arc<dyn CredentialProvider>>,
    pub clock: Arc<dyn Clock>,
    pub sink: Arc<dyn ExchangeSink>,
    pub retain_bytes: bool,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            endpoint: None,
            credentials: None,
            clock: Arc::new(SystemClock),
            sink: Arc::new(DiscardExchanges),
            retain_bytes: false,
        }
    }
}
pub struct BedrockBackend {
    binding: BedrockBinding,
    descriptor: BackendDescriptor,
    materials: Materials,
    policy: RwLock<ModelPolicy>,
    credentials: Arc<dyn CredentialProvider>,
    clock: Arc<dyn Clock>,
    transport: Transport,
    exchanges: Exchanges,
}
impl BedrockBackend {
    /// A host policy is mandatory; there is no default admission policy.
    /// ```compile_fail,E0061
    /// use rustev_bedrock::{BedrockBackend, adapter::Config, binding::BedrockBinding, mapping::Materials};
    /// fn without_policy(binding: BedrockBinding, materials: Materials) {
    ///     BedrockBackend::new("host".into(), binding, materials, Config::default());
    /// }
    /// ```
    pub fn new(
        backend_id: String,
        binding: BedrockBinding,
        policy: ModelPolicy,
        materials: Materials,
        config: Config,
    ) -> Result<Self, String> {
        binding.validate()?;
        policy.evaluate(&binding)?;
        materials.validate(&binding)?;
        let credentials = config.credentials.ok_or("credential_provider_required")?;
        let endpoint = config
            .endpoint
            .unwrap_or_else(|| format!("https://bedrock-runtime.{}.amazonaws.com", binding.region));
        let transport = Transport::new(&endpoint)?;
        let descriptor = BackendDescriptor {
            ranking: None,
            schema: schema::BACKEND.into(),
            backend_id,
            artifact: binding.artifact()?,
            operations: binding
                .operations
                .iter()
                .map(|o| OperationSupport {
                    isolation: None,
                    screen: None,
                    operation: match o.operation.as_str() {
                        "classify" => Operation::Classify,
                        "proposition" => Operation::Proposition,
                        _ => Operation::Rubric,
                    },
                    output: OutputKind::Label,
                    max_options: o.max_options,
                })
                .collect(),
            input_limit: InputLimit {
                max_bytes: binding.request_limits.max_projection_bytes,
                on_excess: InputExcess::Refuse,
            },
            determinism: Determinism::Unspecified,
        };
        Ok(Self {
            binding,
            descriptor,
            materials,
            policy: RwLock::new(policy),
            credentials,
            clock: config.clock,
            transport,
            exchanges: Exchanges::new(config.sink, config.retain_bytes),
        })
    }
    pub fn binding(&self) -> &BedrockBinding {
        &self.binding
    }
    pub fn exchange_stats(&self) -> crate::exchange::ExchangeStats {
        self.exchanges.stats()
    }
    /// Installs even a denying policy; subsequent attempts fail unsent.
    pub fn replace_policy(&self, policy: ModelPolicy) -> Result<(), String> {
        let result = policy.evaluate(&self.binding);
        *self.policy.write().map_err(|_| "policy_lock")? = policy;
        result
    }
    pub fn check_plan(&self, plan: &rustev_contract::plan::Plan) -> Result<(), String> {
        self.binding.validate()?;
        self.policy
            .read()
            .map_err(|_| "policy_lock")?
            .evaluate(&self.binding)?;
        let id = self.descriptor.id().map_err(|e| e.to_string())?;
        for step in &plan.steps {
            if let rustev_contract::plan::PlanStepDetail::Semantic(b) = &step.detail
                && b.backend_id == self.descriptor.backend_id
                && b.descriptor != id
            {
                return Err("descriptor_mismatch".into());
            }
        }
        if let rustev_contract::plan::PlanExecution::Declared(d) = &plan.execution {
            for f in &d.fallbacks {
                if f.backend_id == self.descriptor.backend_id && f.descriptor != id {
                    return Err("descriptor_mismatch".into());
                }
            }
        }
        for step in &plan.definition.steps {
            let rustev_contract::definition::StepBody::Semantic(declaration) = &step.body else {
                continue;
            };
            let bound = plan.steps.iter().any(|s| s.id == step.id && matches!(&s.detail, rustev_contract::plan::PlanStepDetail::Semantic(b) if b.backend_id == self.descriptor.backend_id));
            let fallback = matches!(&plan.execution, rustev_contract::plan::PlanExecution::Declared(d) if d.fallbacks.iter().any(|f| f.step == step.id && f.backend_id == self.descriptor.backend_id));
            if bound || fallback {
                let support = self
                    .descriptor
                    .operations
                    .iter()
                    .find(|o| o.operation == declaration.operation)
                    .ok_or("unsupported_operation")?;
                if declaration.options.len() < 2
                    || declaration.options.len() as u64 > support.max_options
                    || declaration
                        .options
                        .iter()
                        .any(|key| key.is_empty() || key.len() > 1024)
                    || declaration
                        .options
                        .iter()
                        .collect::<std::collections::BTreeSet<_>>()
                        .len()
                        != declaration.options.len()
                    || (declaration.operation == Operation::Proposition
                        && declaration.options != ["false", "true"])
                {
                    return Err("invalid_options".into());
                }
            }
        }
        Ok(())
    }
    async fn attempt(&self, call: AttemptCall<'_>) -> AttemptReport {
        let budget = Budget::start(call.cx.remaining_ms);
        let sent = SentTracker::new();
        let policy = self
            .policy
            .read()
            .map(|p| p.clone())
            .map_err(|_| "policy_lock".to_owned());
        let policy_digest = policy.as_ref().ok().and_then(|p| p.digest().ok());
        let mut request_bytes = None;
        let mut reply = None;
        let run = async {
            self.binding
                .validate()
                .and_then(|()| policy.and_then(|p| p.evaluate(&self.binding)))
                .map_err(|e| fail(RemoteCode::Capability, &e, false))?;
            let body =
                mapping::request(&self.binding, &self.materials, call.projection).map_err(|e| {
                    fail(
                        if e == "invalid_projection" {
                            RemoteCode::MalformedRequest
                        } else {
                            RemoteCode::Capability
                        },
                        &e,
                        false,
                    )
                })?;
            request_bytes = Some(body.clone());
            let credentials = self
                .credentials
                .credentials(budget, &call.cancel)
                .await
                .map_err(|_| fail(RemoteCode::TransportUnsent, "credential_provider", false))?;
            if credentials
                .expiry()
                .is_some_and(|expiry| expiry <= self.clock.now())
            {
                return Err(fail(
                    RemoteCode::TransportUnsent,
                    "expired_credentials",
                    false,
                ));
            }
            let path = match self.binding.wire {
                Wire::Converse => model_path(&self.binding.model_id),
                Wire::Responses => "/openai/v1/responses".into(),
            };
            let url = self.transport.url(&path);
            let uri: hyper::Uri = url
                .parse()
                .map_err(|_| fail(RemoteCode::TransportUnsent, "endpoint", false))?;
            let authority = host_header(&uri)
                .ok_or_else(|| fail(RemoteCode::TransportUnsent, "endpoint", false))?;
            let mut request = hyper::Request::post(uri)
                .header("host", authority)
                .header("content-type", "application/json")
                .body(body)
                .map_err(|_| fail(RemoteCode::TransportUnsent, "request", false))?;
            sign_request(
                &mut request,
                credentials,
                &self.binding.region,
                self.clock.now(),
            )
            .map_err(|_| fail(RemoteCode::TransportUnsent, "signing", false))?;
            let r = self
                .transport
                .send(
                    request,
                    self.binding.request_limits.max_response_bytes as usize,
                    &sent,
                )
                .await
                .map_err(|e| {
                    let code = if matches!(e, crate::http::TransportError::TooLarge { .. }) {
                        RemoteCode::MalformedResponse
                    } else if sent.may_have_sent() {
                        RemoteCode::TransportInterrupted
                    } else {
                        RemoteCode::TransportUnsent
                    };
                    fail(code, "transport", sent.may_have_sent())
                })?;
            reply = Some(r);
            let r = reply.as_ref().expect("reply just assigned");
            if !(200..300).contains(&r.status) {
                let kind = error_type(r);
                let code = mapping::error_code(r.status, &kind);
                let usage_charge = if code == RemoteCode::Capability {
                    rustev_contract::bounded::parse_bounded::<serde_json::Value>(
                        &r.body,
                        &crate::binding::LIMITS,
                    )
                    .ok()
                    .map(|raw| mapping::charge(&self.binding, &raw))
                } else {
                    None
                };
                return Err(RemoteFailure::new(
                    code,
                    format!("bedrock:{kind}"),
                    usage_charge,
                    true,
                    true,
                )
                .report());
            }
            let mapped = mapping::response(&self.binding, &r.body)
                .map_err(|_| fail(RemoteCode::MalformedResponse, "response", true))?;
            match mapped.output {
                Ok(output) => Ok(AttemptReport {
                    result: Ok(output),
                    charge: mapped.charge,
                    cancel: CancelAck::NotRequested,
                    remote: RemoteEnd::Finished,
                }),
                Err(code) => Err(RemoteFailure::new(
                    code,
                    format!(
                        "bedrock:{}",
                        mapped.raw["stopReason"]
                            .as_str()
                            .or_else(|| mapped.raw["status"].as_str())
                            .unwrap_or("invalid_answer")
                    ),
                    Some(mapped.charge),
                    true,
                    true,
                )
                .report()),
            }
        };
        let report = tokio::select! { biased;
            ()=call.cancel.raised()=>{ let unsent=sent.try_abort(); cancelled_report(cancel_outcome(!unsent,CancelSupport::None,None),"bedrock:cancelled") },
            ()=budget.expiry()=>{ let unsent=sent.try_abort(); fail(if unsent{RemoteCode::TransportUnsent}else{RemoteCode::RemoteDeadline},"deadline",!unsent) },
            result=run=>match result {Ok(report)|Err(report)=>report}
        };
        self.record(
            call.attempt_id,
            &report,
            request_bytes,
            reply,
            sent.may_have_sent(),
            policy_digest,
        );
        report
    }
    fn record(
        &self,
        attempt: &str,
        report: &AttemptReport,
        request: Option<Vec<u8>>,
        reply: Option<Reply>,
        sent: bool,
        policy: Option<String>,
    ) {
        let outcome = match &report.result {
            // An output whose canonical bytes cannot be digested is recorded
            // as a failure, never as an output with an empty digest.
            Ok(output) => match output_digest(output) {
                Some(digest) => MemberOutcome::Output { digest },
                None => MemberOutcome::Failure {
                    code: RemoteCode::MalformedResponse,
                    detail: "bedrock:output_not_canonical".into(),
                },
            },
            Err((_, detail)) => MemberOutcome::Failure {
                code: code_of_detail(detail).unwrap_or(RemoteCode::RemoteError),
                detail: detail.clone(),
            },
        };
        let mut record = blank_record(
            "rustev-bedrock-mapping/1",
            self.descriptor.artifact.clone(),
            vec![ExchangeMember {
                attempt_id: attempt.into(),
                outcome,
                charge: report.charge,
            }],
        );
        record.bytes_sent = sent;
        record.exchange_charge = report.charge;
        record.requested_model = Some(self.binding.model_id.clone());
        record.request_digest = request.as_deref().map(digest_bytes);
        if let Some(p) = policy {
            record.provider_extras.insert("policy_digest".into(), p);
        }
        for (key, value) in [
            ("region", serde_json::json!(self.binding.region)),
            (
                "inference_profile",
                serde_json::json!(self.binding.inference_profile),
            ),
            (
                "destination_regions",
                serde_json::json!(self.binding.destination_regions),
            ),
        ] {
            record.provider_extras.insert(key.into(), value.to_string());
        }
        record.provider_extras.insert(
            "served_region".into(),
            "unknown:not_disclosed_by_wire".into(),
        );
        record
            .provider_extras
            .insert("served_reason".into(), "not_disclosed_by_wire".into());
        if let Some(r) = &reply {
            record.status = Some(r.status);
            record.response_digest = Some(digest_bytes(&r.body));
            record.retry_after = r
                .headers
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            if let Ok(raw) = rustev_contract::bounded::parse_bounded::<serde_json::Value>(
                &r.body,
                &crate::binding::LIMITS,
            ) {
                if let Some(usage) = raw.get("usage") {
                    record
                        .provider_extras
                        .insert("raw_usage".into(), usage.to_string());
                    if let Some(u) = usage.as_object() {
                        for (k, v) in u {
                            if let Some(n) = v.as_u64() {
                                record.usage.insert(k.clone(), n);
                            }
                        }
                    }
                }
                // Bounded by the shared exchange writer. Provider fields never enter output types.
                let extras = mapping::reported_extras(&self.binding, &raw);
                // Provider extras take only the room the policy, residency and
                // usage entries already recorded leave, so the map never
                // exceeds `MAX_ENTRIES` without `truncated` saying so.
                let maximum = rustev_contract::remote::MAX_ENTRIES
                    .saturating_sub(record.provider_extras.len());
                record.truncated |= extras.len() > maximum;
                record
                    .provider_extras
                    .extend(extras.into_iter().take(maximum));
                if self.binding.wire == Wire::Responses {
                    record.generation_id = raw["id"].as_str().map(str::to_owned);
                    if let Some(model) = raw["model"].as_str().filter(|model| !model.is_empty()) {
                        if model == self.binding.model_id {
                            record
                                .provider_extras
                                .insert("served_reason".into(), "echo_of_requested".into());
                        } else {
                            record.served = ServedIdentity {
                                identity: Some(model.into()),
                                disclosure: Disclosure::Reported,
                            };
                            record.provider_extras.remove("served_reason");
                        }
                    }
                }
            }
            if self.binding.wire == Wire::Converse {
                record.generation_id = r
                    .headers
                    .get("x-amzn-requestid")
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_owned);
            }
        }
        let retained = request.map(|request| RetainedBytes {
            request,
            response: reply.map(|r| r.body),
        });
        self.exchanges.deliver(record, retained);
    }
}
fn fail(code: RemoteCode, kind: &str, sent: bool) -> AttemptReport {
    RemoteFailure::new(
        code,
        format!("bedrock:{kind}"),
        if sent {
            None
        } else {
            Some(Charge::Observed { units: 0 })
        },
        // `refusals_charged` (not "received"): a refusal is never assumed
        // free (030 3.9.4); an unsent attempt carries its own zero charge above.
        true,
        sent,
    )
    .report()
}
fn error_type(r: &Reply) -> String {
    let header = r
        .headers
        .get("x-amzn-errortype")
        .and_then(|v| v.to_str().ok());
    let raw: Option<serde_json::Value> =
        rustev_contract::bounded::parse_bounded(&r.body, &crate::binding::LIMITS).ok();
    let value = header
        .or_else(|| {
            raw.as_ref()
                .and_then(|v| {
                    v.pointer("/error/type")
                        .or_else(|| v.pointer("/error/code"))
                        .or_else(|| v.get("__type"))
                        .or_else(|| v.get("type"))
                })
                .and_then(serde_json::Value::as_str)
        })
        .unwrap_or("unknown");
    value
        .rsplit('#')
        .next()
        .unwrap_or("unknown")
        .split(':')
        .next()
        .unwrap_or("unknown")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .take(128)
        .collect()
}
impl DecisionBackend for BedrockBackend {
    fn descriptor(&self) -> &BackendDescriptor {
        &self.descriptor
    }
    fn cost_model(&self) -> CostModel {
        CostModel::Estimated
    }
    fn cost_bound(&self, projection: &[u8]) -> CostBound {
        // Version one uses one UTF-8 byte per estimated input token, including question material.
        mapping::request(&self.binding, &self.materials, projection)
            .ok()
            .and_then(|b| {
                mapping::priced(
                    &self.binding,
                    b.len() as u64,
                    self.binding.parameters.max_tokens,
                    0,
                    0,
                )
            })
            .map_or(CostBound::Unknown, |units| CostBound::Estimated { units })
    }
    fn infer<'a>(&'a self, call: AttemptCall<'a>) -> BoxFuture<'a, AttemptReport> {
        Box::pin(self.attempt(call))
    }
}
