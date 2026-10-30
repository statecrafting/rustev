//! Optional environment credentials (3.5 item 2, A-65). Only this feature
//! module reads credentials from the environment.
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
        // Only the environment is read: the default chain builds an instance
        // metadata client, which panics without an HTTP client (A-65).
        Box::pin(async move {
            let discover = async {
                aws_config::environment::credentials::EnvironmentVariableCredentialsProvider::new()
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
        return Err("usage: rustev-bedrock probe --binding FILE --policy FILE --materials FILE --journal FILE [--out DIR --calls 3 --interval-ms 1000 --budget-ms 30000]".into());
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
                | "--out"
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
    let projection = crate::evidence::probe_projection(&operation)?;
    let unit = binding.pricing.unit_nano_usd;
    let journal = crate::spend::SpendJournal::open(
        value("--journal")?,
        crate::spend::BudgetKind::Testing,
        Arc::new(crate::spend::SystemClock),
    )
    .map_err(|e| e.to_string())?;
    println!(
        "journal before: {}",
        serde_json::to_string(&journal.summary()).map_err(|e| e.to_string())?
    );
    // `--out` retains request and response bodies for recorded fixtures
    // (3.14 item 3); without it the record stays digest-only (R-19).
    let out = value("--out").ok().map(std::path::PathBuf::from);
    let kept = Arc::new(crate::evidence::KeepExchanges::default());
    // The name prefixes each attempt id, so a recorded exchange says which
    // entry point sent it.
    let name = if smoke {
        "bedrock-smoke"
    } else {
        "bedrock-probe"
    };
    let backend = crate::BedrockBackend::new(
        name.into(),
        binding,
        policy,
        materials,
        crate::adapter::Config {
            credentials: Some(Arc::new(StandardCredentials)),
            sink: kept.clone(),
            retain_bytes: out.is_some(),
            ..Default::default()
        },
    )?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let result = runtime.block_on(async {
        use rustev_core::seams::{DecisionBackend,AttemptCall,CallContext};
        use rustev_contract::run::{CostBound,Charge};
        for i in 0..count {
            let units=match backend.cost_bound(&projection){CostBound::Estimated{units}=>units.checked_mul(unit).ok_or("cost overflow")?,_=>return Err("unknown reservation".into())};
            let attempt=format!("{name}-{}-{i}",std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_|"clock")?.as_nanos());
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
    });
    println!(
        "journal after: {}",
        serde_json::to_string(&journal.summary()).map_err(|e| e.to_string())?
    );
    // The probe's own result is reported before any evidence refusal.
    if let Err(error) = &result {
        eprintln!("{error}");
    }
    if let Some(dir) = out {
        let secrets: Vec<(&str, Vec<u8>)> = [
            "AWS_ACCESS_KEY_ID",
            "AWS_SECRET_ACCESS_KEY",
            "AWS_SESSION_TOKEN",
        ]
        .iter()
        .filter_map(|k| std::env::var(k).ok().map(|v| (*k, v.into_bytes())))
        .collect();
        let secrets: Vec<(&str, &[u8])> = secrets.iter().map(|(k, v)| (*k, v.as_slice())).collect();
        let kept = kept.0.lock().map_err(|_| "evidence sink poisoned")?;
        let provenance = if smoke {
            "live Bedrock smoke, synthetic fixture (3.14)"
        } else {
            "live Bedrock probe, synthetic fixture (3.11)"
        };
        let n = crate::evidence::write_evidence(&dir, &kept, provenance, &secrets)?;
        println!("evidence: {n} exchange(s) written");
    }
    result
}
