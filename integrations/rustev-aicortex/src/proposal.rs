//! Observation proposals (spec 010 3.5.4 and 3.5.5; binding record item 8).
//!
//! A proposal says that something was observed after a decision, with the
//! lineage that ties the observation to the decision, plan and snapshot.
//! It is staged into the host's transaction as an Aicortex claim proposal
//! and never committed here: acceptance is the host's commit, admission is
//! Aicortex's gate, and neither makes the observation true or authorized.

use std::fmt;

use aicortex_store::ClaimProposalRepo;
use aicortex_types::{
    Actor, Claim, ClaimId, ClaimParts, ClaimProposal, ClaimValue, EpistemicStatus, Evidence,
    PredicateRef, ProposalId, Provenance, Scope, SlotKey, SourceRef, SourceSystem, Stance,
    SubjectRef,
};
use rahi_store::TxnBuilder;
use rahi_types::UnixSeconds;
use rustev_contract::ids::{PlanId, SnapshotId};
use sha2::{Digest, Sha256};

/// The source system every proposal names.
pub const SOURCE_SYSTEM: &str = "rustev";

/// Where the observed outcome came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutcomeOrigin {
    /// Observed in the world after the decision, by the observer named.
    Observed,
    /// A Rustev judgment. Refused: a judgment is evidence that Rustev
    /// produced a decision, not an observation about reality (spec 001
    /// 3.3.3).
    RustevJudgment,
}

/// One observation and its lineage. Every member is required; the host
/// supplies the scope it authorizes and the observer it attributes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservationProposal {
    pub decision_id: String,
    pub plan_id: PlanId,
    pub snapshot_id: SnapshotId,
    /// Tells apart several observations of one decision; with the decision
    /// id it fixes the proposal's identity, so a resubmission is the same
    /// proposal.
    pub observation_key: String,
    pub origin: OutcomeOrigin,
    pub scope: Scope,
    pub subject: SubjectRef,
    pub predicate: PredicateRef,
    pub slot: Option<SlotKey>,
    pub value: ClaimValue,
    /// Who observed it.
    pub observer: Actor,
    pub observed_at: UnixSeconds,
    /// The host's evidence, passed through; the adapter adds none.
    pub evidence: Vec<Evidence>,
}

/// Why a proposal was refused before anything was staged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProposalRefusal {
    JudgmentIsNotAnObservation,
    EmptyDecisionId,
    EmptyObservationKey,
    /// Aicortex refused a member (its `TypeError` or staging error).
    Aicortex(String),
}

impl fmt::Display for ProposalRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::JudgmentIsNotAnObservation => {
                f.write_str("a Rustev judgment is not an observation; refused before staging")
            }
            Self::EmptyDecisionId => f.write_str("the decision id is empty"),
            Self::EmptyObservationKey => f.write_str("the observation key is empty"),
            Self::Aicortex(d) => write!(f, "aicortex refused the proposal: {d}"),
        }
    }
}

impl std::error::Error for ProposalRefusal {}

/// The identities a staged proposal carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Staged {
    pub proposal_id: ProposalId,
    pub claim_id: ClaimId,
}

/// A UUID (version 8, RFC 9562 variant) derived from the decision id and
/// the observation key under `domain`.
fn derived(domain: &str, decision_id: &str, key: &str) -> uuid::Uuid {
    let mut h = Sha256::new();
    for part in [domain.as_bytes(), decision_id.as_bytes(), key.as_bytes()] {
        h.update((part.len() as u64).to_be_bytes());
        h.update(part);
    }
    let digest = h.finalize();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    uuid::Builder::from_custom_bytes(bytes).into_uuid()
}

/// Stage `p` into the host's transaction as an Aicortex claim proposal with
/// stance `observed` and the lineage in its source reference:
/// `system = "rustev"`, `external_id = <decision id>`,
/// `locator = "rustev:plan/<PlanId>/snapshot/<SnapshotId>"`. `now` is the
/// proposal and ingestion time. Nothing is committed.
///
/// # Errors
///
/// [`ProposalRefusal`]; nothing is staged.
pub fn stage_observation(
    txn: &mut TxnBuilder,
    p: &ObservationProposal,
    now: UnixSeconds,
) -> Result<Staged, ProposalRefusal> {
    if p.origin == OutcomeOrigin::RustevJudgment {
        return Err(ProposalRefusal::JudgmentIsNotAnObservation);
    }
    if p.decision_id.is_empty() {
        return Err(ProposalRefusal::EmptyDecisionId);
    }
    if p.observation_key.is_empty() {
        return Err(ProposalRefusal::EmptyObservationKey);
    }
    let aicortex = |e: &dyn fmt::Display| ProposalRefusal::Aicortex(e.to_string());
    let system = SourceSystem::new(SOURCE_SYSTEM).map_err(|e| aicortex(&e))?;
    let source = SourceRef::new(system)
        .with_external_id(p.decision_id.clone())
        .with_locator(format!(
            "rustev:plan/{}/snapshot/{}",
            p.plan_id, p.snapshot_id
        ));
    let epistemic = EpistemicStatus::plain(Stance::Observed).map_err(|e| aicortex(&e))?;
    let ids = Staged {
        proposal_id: ProposalId::from_uuid(derived(
            "rustev.aicortex.proposal/1",
            &p.decision_id,
            &p.observation_key,
        )),
        claim_id: ClaimId::from_uuid(derived(
            "rustev.aicortex.claim/1",
            &p.decision_id,
            &p.observation_key,
        )),
    };
    let proposal = ClaimProposal {
        id: ids.proposal_id,
        claim: Claim::new(ClaimParts {
            id: ids.claim_id,
            scope: p.scope.clone(),
            subject: p.subject.clone(),
            predicate: p.predicate.clone(),
            value: p.value.clone(),
            slot: p.slot.clone(),
            epistemic,
            provenance: Provenance::captured(source, p.observed_at, now),
        }),
        proposer: p.observer.clone(),
        evidence: p.evidence.clone(),
        relations: Vec::new(),
        proposed_at: now,
    };
    // Stage into a scratch builder first, so a refusal leaves the host's
    // transaction untouched.
    let mut staged = TxnBuilder::new();
    ClaimProposalRepo::stage_append(&mut staged, &proposal).map_err(|e| aicortex(&e))?;
    for s in staged.into_statements() {
        txn.push(s);
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use aicortex_types::{ActorId, Namespace, SubjectKey, SubjectKind};
    use rahi_types::Sub;

    use super::*;

    fn proposal(key: &str) -> ObservationProposal {
        ObservationProposal {
            decision_id: "decision-1".into(),
            plan_id: PlanId::parse(&format!("sha256:{}", "1".repeat(64))).unwrap(),
            snapshot_id: SnapshotId::parse(&format!("sha256:{}", "2".repeat(64))).unwrap(),
            observation_key: key.into(),
            origin: OutcomeOrigin::Observed,
            scope: Scope::personal(Sub::new("traveler")),
            subject: SubjectRef {
                namespace: Namespace::new("demo").unwrap(),
                kind: SubjectKind::new("account").unwrap(),
                key: SubjectKey::new("account-1").unwrap(),
            },
            predicate: PredicateRef::parse("demo:account.visits@1").unwrap(),
            slot: None,
            value: ClaimValue::Integer(1),
            observer: Actor::human(ActorId::new("front-desk").unwrap()),
            observed_at: UnixSeconds::new(1_800_000_000),
            evidence: Vec::new(),
        }
    }

    #[test]
    fn identities_derive_from_the_decision_and_the_observation_key() {
        let now = UnixSeconds::new(1_800_000_100);
        let a = stage_observation(&mut TxnBuilder::new(), &proposal("visits"), now).unwrap();
        let again = stage_observation(&mut TxnBuilder::new(), &proposal("visits"), now).unwrap();
        let other = stage_observation(&mut TxnBuilder::new(), &proposal("tier"), now).unwrap();
        assert_eq!(a, again);
        assert_ne!(a.proposal_id, other.proposal_id);
        assert_ne!(a.claim_id, other.claim_id);
        assert_ne!(a.proposal_id.to_string(), a.claim_id.to_string());
    }

    #[test]
    fn refusals_stage_nothing() {
        let now = UnixSeconds::new(1_800_000_100);
        let mut txn = TxnBuilder::new();
        let mut judgment = proposal("visits");
        judgment.origin = OutcomeOrigin::RustevJudgment;
        assert_eq!(
            stage_observation(&mut txn, &judgment, now),
            Err(ProposalRefusal::JudgmentIsNotAnObservation)
        );
        let mut no_decision = proposal("visits");
        no_decision.decision_id.clear();
        assert_eq!(
            stage_observation(&mut txn, &no_decision, now),
            Err(ProposalRefusal::EmptyDecisionId)
        );
        assert_eq!(
            stage_observation(&mut txn, &proposal(""), now),
            Err(ProposalRefusal::EmptyObservationKey)
        );
        assert!(txn.is_empty());
    }
}
