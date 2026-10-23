//! Optional standard credential chain. Only this feature module discovers credentials.
use crate::{
    budget::Budget,
    signing::{CredentialError, CredentialProvider},
};
use aws_credential_types::provider::ProvideCredentials;
use rustev_core::seams::{BoxFuture, CancelSignal};
use std::sync::Arc;

pub struct StandardCredentials;
impl CredentialProvider for StandardCredentials {
    fn credentials<'a>(
        &'a self,
        budget: Budget,
        cancel: &'a CancelSignal,
    ) -> BoxFuture<'a, Result<aws_credential_types::Credentials, CredentialError>> {
        // Discovery runs inside the attempt's one budget and stops on cancel
        // (3.5): an expired budget or a cancel is a credential failure.
        Box::pin(async move {
            let discover = async {
                let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
                    .load()
                    .await;
                config
                    .credentials_provider()
                    .ok_or(CredentialError)?
                    .provide_credentials()
                    .await
                    .map_err(|_| CredentialError)
            };
            tokio::select! { biased;
                () = cancel.raised() => Err(CredentialError),
                r = tokio::time::timeout(budget.remaining(), discover) => r.unwrap_or(Err(CredentialError)),
            }
        })
    }
}
/// Manual synthetic smoke/probe entry point. No invocation from tests.
pub fn run_cli(smoke: bool) -> Result<(), String> {
    if std::env::var("RUSTEV_BEDROCK_LIVE").as_deref() != Ok("1") {
        return Err("RUSTEV_BEDROCK_LIVE=1 required; sent nothing".into());
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !smoke && args.first().map(String::as_str) != Some("probe") {
        return Err("usage: rustev-bedrock probe --binding FILE --policy FILE --materials FILE --journal FILE [--calls 3 --interval-ms 1000 --budget-ms 30000]".into());
    }
    let flags = if smoke { args.as_slice() } else { &args[1..] };
    let mut options = std::collections::BTreeMap::new();
    let (pairs, remainder) = flags.as_chunks::<2>();
    for pair in pairs {
        if !matches!(
            pair[0].as_str(),
            "--binding"
                | "--policy"
                | "--materials"
                | "--journal"
                | "--calls"
                | "--interval-ms"
                | "--budget-ms"
        ) || options.insert(pair[0].as_str(), pair[1].clone()).is_some()
        {
            return Err("unknown or duplicate probe option".into());
        }
    }
    if !remainder.is_empty() {
        return Err("missing probe option value".into());
    }
    let value = |key: &str| {
        options
            .get(key)
            .cloned()
            .ok_or_else(|| format!("required: {key}"))
    };
    let load = |key: &str| std::fs::read(value(key)?).map_err(|_| format!("cannot read {key}"));
    let binding = crate::binding::BedrockBinding::parse(&load("--binding")?)?;
    let policy = crate::policy::ModelPolicy::parse(&load("--policy")?)?;
    let materials =
        rustev_contract::bounded::parse_bounded(&load("--materials")?, &crate::binding::LIMITS)
            .map_err(|e| e.to_string())?;
    let count = value("--calls")
        .unwrap_or_else(|_| if smoke { "1" } else { "3" }.into())
        .parse::<usize>()
        .map_err(|_| "invalid calls")?;
    let interval = value("--interval-ms")
        .unwrap_or_else(|_| "1000".into())
        .parse::<u64>()
        .map_err(|_| "invalid interval")?;
    let budget = value("--budget-ms")
        .unwrap_or_else(|_| "30000".into())
        .parse::<u64>()
        .map_err(|_| "invalid budget")?;
    if !(if smoke { 1 } else { 2 }..=5).contains(&count)
        || interval > 60_000
        || !(1..=120_000).contains(&budget)
    {
        return Err("probe limits exceeded".into());
    }
    let operation = binding
        .operations
        .first()
        .ok_or("no bound operation")?
        .operation
        .clone();
    let options = if operation == "proposition" {
        vec!["false", "true"]
    } else {
        vec!["apple", "pear"]
    };
    let projection = serde_json::to_vec(&serde_json::json!({
        "operation":operation, "question":"The supplied word is apple. Choose the matching key.",
        "options":options,"instance":{"text":"apple"},"values":{}
    }))
    .map_err(|_| "synthetic projection")?;
    let unit = binding.pricing.unit_nano_usd;
    let journal = crate::spend::SpendJournal::open(
        value("--journal")?,
        crate::spend::BudgetKind::Testing,
        Arc::new(crate::spend::SystemClock),
    )
    .map_err(|e| e.to_string())?;
    let backend = crate::BedrockBackend::new(
        "bedrock-probe".into(),
        binding,
        policy,
        materials,
        crate::adapter::Config {
            credentials: Some(Arc::new(StandardCredentials)),
            ..Default::default()
        },
    )?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    runtime.block_on(async {
        use rustev_core::seams::{DecisionBackend,AttemptCall,CallContext};
        use rustev_contract::run::{CostBound,Charge};
        for i in 0..count {
            let units=match backend.cost_bound(&projection){CostBound::Estimated{units}=>units.checked_mul(unit).ok_or("cost overflow")?,_=>return Err("unknown reservation".into())};
            let attempt=format!("bedrock-probe-{}-{i}",std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_|"clock")?.as_nanos());
            let reservation=journal.reserve(&attempt,units).map_err(|e|e.to_string())?;
            let cx=CallContext{remaining_ms:rustev_contract::time::DurationMs(budget),trace_id:String::new(),principal_handle:vec![]};
            let report=backend.infer(AttemptCall{projection:&projection,attempt_id:&attempt,cancel:CancelSignal::new(),cx:&cx}).await;
            let charge=match report.charge { Charge::Observed{units}=>units.checked_mul(unit).map_or(Charge::Unknown,|units|Charge::Observed{units}),Charge::Estimated{units}=>units.checked_mul(unit).map_or(Charge::Unknown,|units|Charge::Estimated{units}),Charge::Unknown=>Charge::Unknown };
            journal.settle(reservation,charge);
            if let Err((_,detail))=report.result { return Err(format!("not reachable: call {}: {detail}",i+1)); }
            if i+1<count { tokio::time::sleep(std::time::Duration::from_millis(interval)).await; }
        }
        println!("reachable: {count} successful sequential calls; interval_ms={interval}; budget_ms={budget}; reachability only");
        Ok(())
    })
}
