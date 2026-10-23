//! Sequential synthetic reachability checks. The caller owns live gating and files.
use crate::{BedrockBackend, binding::BedrockBinding, spend::SpendJournal, taxonomy};
use rustev_contract::{
    remote::RemoteCode,
    run::{Charge, CostBound},
    time::DurationMs,
};
use rustev_core::seams::{AttemptCall, CallContext, CancelSignal, DecisionBackend};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ATTEMPT: AtomicU64 = AtomicU64::new(0);

/// Validated limits, shared by the manual smoke and multi-call probe.
#[derive(Debug, Clone)]
pub struct ProbePlan {
    count: usize,
    interval_ms: u64,
    budget_ms: u64,
    prefix: String,
}
impl ProbePlan {
    /// `smoke` permits one call; a reachability probe requires at least two.
    /// The prefix should distinguish caller sessions sharing a persistent journal.
    pub fn new(
        smoke: bool,
        count: usize,
        interval_ms: u64,
        budget_ms: u64,
        prefix: String,
    ) -> Result<Self, String> {
        if !(if smoke { 1 } else { 2 }..=5).contains(&count)
            || interval_ms > 60_000
            || !(1..=120_000).contains(&budget_ms)
        {
            return Err("probe limits exceeded".into());
        }
        Ok(Self {
            count,
            interval_ms,
            budget_ms,
            prefix,
        })
    }
    pub fn synthetic_projection(binding: &BedrockBinding) -> Result<Vec<u8>, String> {
        let operation = &binding
            .operations
            .first()
            .ok_or("no bound operation")?
            .operation;
        let options = if operation == "proposition" {
            vec!["false", "true"]
        } else {
            vec!["apple", "pear"]
        };
        serde_json::to_vec(&serde_json::json!({
            "operation":operation, "question":"The supplied word is apple. Choose the matching key.",
            "options":options,"instance":{"text":"apple"},"values":{}
        })).map_err(|_| "synthetic projection".into())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ProbeReport {
    Reachable {
        calls: usize,
    },
    NotReachable {
        call: usize,
        code: RemoteCode,
        detail: String,
    },
}

/// Reserve before each dispatch, settle every returned report, and stop at the
/// first failure. This does not establish model quality or account qualification.
pub async fn probe(
    backend: &BedrockBackend,
    journal: &SpendJournal,
    plan: ProbePlan,
) -> ProbeReport {
    let refused = |call, detail: String| ProbeReport::NotReachable {
        call,
        code: RemoteCode::TransportUnsent,
        detail,
    };
    let projection = match ProbePlan::synthetic_projection(backend.binding()) {
        Ok(p) => p,
        Err(e) => return refused(1, e),
    };
    let unit = backend.binding().pricing.unit_nano_usd;
    for i in 0..plan.count {
        let units = match backend.cost_bound(&projection) {
            CostBound::Estimated { units } => match units.checked_mul(unit) {
                Some(units) => units,
                None => return refused(i + 1, "cost overflow".into()),
            },
            _ => return refused(i + 1, "unknown reservation".into()),
        };
        let attempt = format!(
            "{}-{}",
            plan.prefix,
            NEXT_ATTEMPT.fetch_add(1, Ordering::Relaxed)
        );
        let reservation = match journal.reserve(&attempt, units) {
            Ok(r) => r,
            Err(e) => return refused(i + 1, e.to_string()),
        };
        let cx = CallContext {
            remaining_ms: DurationMs(plan.budget_ms),
            trace_id: String::new(),
            principal_handle: vec![],
        };
        let report = backend
            .infer(AttemptCall {
                projection: &projection,
                attempt_id: &attempt,
                cancel: CancelSignal::new(),
                cx: &cx,
            })
            .await;
        let charge = match report.charge {
            Charge::Observed { units } => units
                .checked_mul(unit)
                .map_or(Charge::Unknown, |units| Charge::Observed { units }),
            Charge::Estimated { units } => units
                .checked_mul(unit)
                .map_or(Charge::Unknown, |units| Charge::Estimated { units }),
            Charge::Unknown => Charge::Unknown,
        };
        journal.settle(reservation, charge);
        if let Err((_, detail)) = report.result {
            return ProbeReport::NotReachable {
                call: i + 1,
                code: taxonomy::code_of_detail(&detail)
                    .expect("Bedrock failures carry a taxonomy code"),
                detail,
            };
        }
        if i + 1 < plan.count {
            tokio::time::sleep(std::time::Duration::from_millis(plan.interval_ms)).await;
        }
    }
    ProbeReport::Reachable { calls: plan.count }
}
