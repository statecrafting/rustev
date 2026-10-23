//! Spec 022 production contracts. Provider mappings remain unverified until
//! owner-authorized recorded exchanges exist. Synthetic mechanics are not
//! production qualification and cannot enable external dispatch.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use rustev_contract::canonical::{canonical_bytes, tagged_digest};
use rustev_contract::ids::ArtifactId;
use rustev_contract::remote::{Disclosure, ServedIdentity};
use serde::{Deserialize, Serialize};

use crate::binding::{JevBinding, Transport};
use crate::http::{Endpoint, Scheme, Secret};
use crate::spend::{BudgetKind, SpendSummary};

pub const SCHEMA: &str = "rustev.jev-binding/2";

/// Supplied by the host; invoked at dispatch, never at construction.
pub type CredentialCallback = Arc<dyn Fn() -> Result<Secret, Unresolved> + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unresolved(pub &'static str);
impl fmt::Display for Unresolved {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Unresolved: {}", self.0)
    }
}
impl std::error::Error for Unresolved {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingUse {
    Development,
    Production,
}

/// Dated, externally reviewable fact, never inferred from missing evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fact {
    pub reference: String,
    pub recorded_on: String,
    pub valid_through: String,
}
impl Fact {
    pub fn check(&self, today: &str) -> Result<(), Unresolved> {
        if self.reference.trim().is_empty()
            || !date(&self.recorded_on)
            || !date(&self.valid_through)
            || !date(today)
            || self.recorded_on.as_str() > today
            || self.valid_through.as_str() < today
        {
            return Err(Unresolved("missing, invalid or stale fact"));
        }
        Ok(())
    }
}
fn date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    if !b
        .iter()
        .enumerate()
        .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
    {
        return false;
    }
    let year: u32 = s[..4].parse().unwrap_or(0);
    let month: u32 = s[5..7].parse().unwrap_or(0);
    let day: u32 = s[8..].parse().unwrap_or(0);
    let max = match month {
        4 | 6 | 9 | 11 => 30,
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        2 => 28,
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => 0,
    };
    year > 0 && day > 0 && day <= max
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayPinning {
    pub observation: Fact,
    pub owner_authorization: String,
    pub request_version_field: String,
    pub response_version_field: String,
    pub exact_version_honored: bool,
    pub unavailable_version_refused: bool,
    pub typesafe_only_observed: bool,
}
impl GatewayPinning {
    pub fn check(&self, today: &str) -> Result<(), Unresolved> {
        self.observation.check(today)?;
        if self.owner_authorization.trim().is_empty()
            || self.request_version_field.trim().is_empty()
            || self.response_version_field.trim().is_empty()
            || !self.exact_version_honored
            || !self.unavailable_version_refused
            || !self.typesafe_only_observed
        {
            return Err(Unresolved(
                "Gateway pinning needs all four live observations under owner authorization",
            ));
        }
        // No accepted decisions-record fact or implemented mapping exists yet.
        Err(Unresolved(
            "Gateway pinning has no accepted repository mapping",
        ))
    }
}

/// Spec 020 section 3.4 ceilings, each clipped to the one remaining budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhaseCeilings {
    pub resolution_ms: u64,
    pub connect_ms: u64,
    pub secure_handshake_ms: u64,
    pub request_write_ms: u64,
    pub first_response_byte_ms: u64,
    pub response_body_ms: u64,
    pub cancellation_ack_ms: u64,
}
impl PhaseCeilings {
    pub fn policy(&self) -> crate::phases::PhasePolicy {
        crate::phases::PhasePolicy {
            resolution_ms: self.resolution_ms,
            connect_ms: self.connect_ms,
            secure_handshake_ms: self.secure_handshake_ms,
            request_write_ms: self.request_write_ms,
            first_response_byte_ms: self.first_response_byte_ms,
            response_body_ms: self.response_body_ms,
            cancellation_ack_ms: self.cancellation_ack_ms,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransportFacts {
    pub endpoint: String,
    pub api_version: String,
    pub tls_minimum: String,
    pub root_store: String,
    pub host_certificate_pinning: bool,
    pub phases: PhaseCeilings,
    /// Reuse is unsupported: this must explicitly be false.
    pub connections_reused: bool,
    /// With reuse off, must be `one-connection-per-attempt`.
    pub isolation_key: String,
    pub retries: u32,
    pub follow_redirects: bool,
    pub privacy_terms: Fact,
    pub retention_terms: String,
    pub training_terms: String,
    /// None explicitly means the provider cannot request zero retention.
    #[serde(deserialize_with = "Option::deserialize")]
    pub zero_retention_options: Option<BTreeMap<String, String>>,
    pub price_source: Fact,
    pub input_usd_per_million: rustev_contract::Decimal,
    pub output_usd_per_million: rustev_contract::Decimal,
    pub units_per_usd: u64,
    pub per_response_charge_reported: bool,
    pub credential_custody_class: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductionBinding {
    pub schema: String,
    pub usage: BindingUse,
    /// Retained verbatim as a nested document, without altering schema 1.
    pub decision: JevBinding,
    pub mapping: String,
    pub facts: TransportFacts,
    pub accepted_versions: Vec<String>,
    pub gateway_pinning: Option<GatewayPinning>,
}

impl ProductionBinding {
    /// Bounded schema-2 parsing; required fields have no defaults.
    pub fn parse(bytes: &[u8]) -> Result<Self, Unresolved> {
        let limits = rustev_contract::bounded::ParseLimits {
            max_depth: 16,
            ..crate::binding::BINDING_LIMITS
        };
        rustev_contract::bounded::parse_bounded(bytes, &limits)
            .map_err(|_| Unresolved("invalid or incomplete schema-2 binding"))
    }

    pub fn canonical(&self) -> Result<Vec<u8>, Unresolved> {
        canonical_bytes(self).map_err(|_| Unresolved("invalid canonical binding"))
    }
    pub fn artifact(&self) -> Result<ArtifactId, Unresolved> {
        ArtifactId::parse(&tagged_digest(SCHEMA, &self.canonical()?))
            .map_err(|_| Unresolved("invalid artifact"))
    }
    /// Validate facts separately from live mapping qualification. This is
    /// not a production eligibility verdict or deployment authorization.
    pub fn check_facts(&self, today: &str, budget_ms: u64) -> Result<(), Unresolved> {
        if self.schema != SCHEMA {
            return Err(Unresolved("unsupported binding schema"));
        }
        // Gateway pins are validated by the distinct evidence path below.
        let mut legacy = self.decision.clone();
        if legacy.transport.is_gateway() {
            legacy.version_pin = None;
        }
        legacy
            .validate()
            .map_err(|_| Unresolved("invalid decision binding"))?;
        let pin = self
            .decision
            .version_pin
            .as_deref()
            .filter(|p| !p.trim().is_empty());
        if pin.is_none() {
            return Err(Unresolved("a version pin is required"));
        }
        match self.decision.transport {
            Transport::GatewayTypesafeBase => {
                return Err(Unresolved("unverified Gateway base transport"));
            }
            Transport::Gateway => self
                .gateway_pinning
                .as_ref()
                .ok_or(Unresolved("Gateway pinning evidence is missing"))?
                .check(today)?,
            Transport::Direct => {
                if self.gateway_pinning.is_some() {
                    return Err(Unresolved("conflicting transport evidence"));
                }
            }
        }
        let f = &self.facts;
        let endpoint = Endpoint::parse(&f.endpoint).map_err(|_| Unresolved("invalid endpoint"))?;
        if endpoint.scheme != Scheme::Https || endpoint.base_path.is_empty() {
            return Err(Unresolved("production requires HTTPS and an explicit path"));
        }
        if f.api_version.trim().is_empty()
            || !matches!(f.tls_minimum.as_str(), "TLS1.2" | "TLS1.3")
            || f.root_store.trim().is_empty()
            || f.credential_custody_class.trim().is_empty()
            || f.retention_terms.trim().is_empty()
            || f.training_terms.trim().is_empty()
            || self.mapping.trim().is_empty()
        {
            return Err(Unresolved("required transport facts are missing"));
        }
        f.phases
            .policy()
            .validate()
            .map_err(|_| Unresolved("every phase ceiling must be positive"))?;
        if budget_ms == 0 {
            return Err(Unresolved("no remaining attempt budget"));
        }
        if f.connections_reused
            || f.isolation_key != "one-connection-per-attempt"
            || f.retries != 0
            || f.follow_redirects
        {
            return Err(Unresolved("unsupported reuse, retry or redirect policy"));
        }
        if f.units_per_usd != crate::binding::UNITS_PER_USD
            || f.input_usd_per_million.units() < 0
            || f.output_usd_per_million.units() < 0
        {
            return Err(Unresolved("invalid price or journal unit rate"));
        }
        if f.zero_retention_options.as_ref().is_some_and(|o| {
            o.is_empty()
                || o.iter()
                    .any(|(k, v)| k.trim().is_empty() || v.trim().is_empty())
        }) {
            return Err(Unresolved("invalid zero-retention request options"));
        }
        let mut versions = std::collections::BTreeSet::new();
        for v in &self.accepted_versions {
            if v.trim().is_empty() || !versions.insert(v) {
                return Err(Unresolved("invalid served version tolerance"));
            }
        }
        f.privacy_terms.check(today)?;
        f.price_source.check(today)
    }
    /// No external direct mapping is verified in this repository. In
    /// particular, a supplied reference or vendor document cannot bypass
    /// stage 2. This refusal precedes credential lookup and dispatch.
    pub fn production_eligible(&self, today: &str, budget_ms: u64) -> Result<(), Unresolved> {
        self.check_facts(today, budget_ms)?;
        if self.usage != BindingUse::Production {
            return Err(Unresolved(
                "development evidence is not production evidence",
            ));
        }
        Err(Unresolved(
            "direct API mapping awaits owner-authorized recorded exchanges (stage 2)",
        ))
    }
    pub fn served(&self, version: Option<&str>) -> Result<ServedIdentity, Unresolved> {
        let version = version
            .filter(|v| !v.is_empty())
            .ok_or(Unresolved("identity_mismatch"))?;
        if self.decision.version_pin.as_deref() != Some(version)
            && !self.accepted_versions.iter().any(|v| v == version)
        {
            return Err(Unresolved("identity_mismatch"));
        }
        Ok(ServedIdentity {
            identity: Some(version.to_owned()),
            disclosure: Disclosure::Reported,
        })
    }
}

/// Evidence never transfers between identities, including calibration,
/// transport, mapping, options, tolerance and description-table changes.
pub fn check_evidence(
    binding: &ProductionBinding,
    artifact: &ArtifactId,
) -> Result<(), Unresolved> {
    if binding.artifact()? != *artifact {
        return Err(Unresolved(
            "binding identity differs; requalification is required",
        ));
    }
    Ok(())
}

/// Immutable comparison in journal units. Positive means billing is higher.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Reconciliation {
    pub period: String,
    pub credential_reference: String,
    pub provider_record: String,
    pub observed: u64,
    pub estimated: u64,
    pub liability: u64,
    pub outstanding: u64,
    pub billed: u64,
    pub billed_minus_observed: i128,
    pub billed_minus_committed: i128,
}
impl Reconciliation {
    /// The host supplies non-secret credential references for both records.
    /// This comparison never settles liability or rewrites journal entries.
    pub fn compare(
        summary: &SpendSummary,
        journal_credential: &str,
        billing_credential: &str,
        billing_month: &str,
        provider_record: &str,
        billed: u64,
    ) -> Result<Self, Unresolved> {
        if summary.budget != BudgetKind::Production
            || summary.period != billing_month
            || !date(&format!("{billing_month}-01"))
            || journal_credential.is_empty()
            || journal_credential != billing_credential
            || provider_record.trim().is_empty()
        {
            return Err(Unresolved(
                "billing and journal must name the same credential and calendar month",
            ));
        }
        Ok(Self {
            period: billing_month.to_owned(),
            credential_reference: journal_credential.to_owned(),
            provider_record: provider_record.to_owned(),
            observed: summary.observed,
            estimated: summary.estimated,
            liability: summary.liability,
            outstanding: summary.outstanding,
            billed,
            billed_minus_observed: i128::from(billed) - i128::from(summary.observed),
            billed_minus_committed: i128::from(billed)
                - i128::from(summary.observed)
                - i128::from(summary.estimated)
                - i128::from(summary.liability)
                - i128::from(summary.outstanding),
        })
    }
}

/// Check all primary and fallback artifacts while preparing a production
/// plan. Hosts call this before registering the plan with the runtime.
/// Missing bindings are unresolved, including development Gateway targets.
pub fn check_production_plan(
    plan: &rustev_contract::plan::Plan,
    bindings: &[ProductionBinding],
    today: &str,
    budget_ms: u64,
) -> Result<(), Unresolved> {
    use rustev_contract::plan::{PlanExecution, PlanStepDetail};
    let mut artifacts = Vec::new();
    for step in &plan.steps {
        if let PlanStepDetail::Semantic(binding) = &step.detail {
            artifacts.push(&binding.artifact);
        }
    }
    if let PlanExecution::Declared(execution) = &plan.execution {
        artifacts.extend(execution.fallbacks.iter().map(|f| &f.artifact));
    }
    for artifact in artifacts {
        let mut found = bindings
            .iter()
            .filter(|b| b.artifact().as_ref() == Ok(artifact));
        let binding = found.next().ok_or(Unresolved(
            "production plan reaches an unqualified or development binding",
        ))?;
        if found.next().is_some() {
            return Err(Unresolved("conflicting binding registry"));
        }
        binding.production_eligible(today, budget_ms)?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum QualificationDataset {
    Admitted { dataset: String, admission: Fact },
    SyntheticProvenance { dataset: String, limitation: String },
}

/// A stage-3 claim always names its exact binding, provenance and spend.
/// Recording a claim is not accepting it; validation below refuses stages
/// that cannot yet be supported by this repository's mapping evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Qualification {
    pub binding: ArtifactId,
    pub calls: Fact,
    pub owner_authorization: String,
    pub dataset: QualificationDataset,
    pub report_reference: String,
    pub spend_journal_reference: String,
    pub observed_units: u64,
    pub unresolved_liability_units: u64,
}
impl Qualification {
    pub fn check(
        &self,
        binding: &ProductionBinding,
        admitted_dataset_exists: bool,
        today: &str,
    ) -> Result<(), Unresolved> {
        check_evidence(binding, &self.binding)?;
        self.calls.check(today)?;
        if self.owner_authorization.trim().is_empty()
            || self.report_reference.trim().is_empty()
            || self.spend_journal_reference.trim().is_empty()
        {
            return Err(Unresolved(
                "qualification lacks authorization, metrics or spend evidence",
            ));
        }
        match &self.dataset {
            QualificationDataset::Admitted { dataset, admission } if !dataset.trim().is_empty() => {
                admission.check(today)?
            }
            QualificationDataset::SyntheticProvenance {
                dataset,
                limitation,
            } if !admitted_dataset_exists
                && !dataset.trim().is_empty()
                && !limitation.trim().is_empty() => {}
            _ => {
                return Err(Unresolved(
                    "qualification requires admitted data when available, otherwise explicitly limited synthetic provenance",
                ));
            }
        }
        binding.production_eligible(today, 5_000)
    }
}

/// A provider billing line settling one previously unknown attempt. This
/// is supplied by the host from the provider record, never inferred from a
/// timeout, a missing charge, or a late adapter response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BillingSettlement {
    pub credential_reference: String,
    pub provider_record: String,
    pub period: String,
    pub reservation: u64,
    pub observed_units: u64,
}
