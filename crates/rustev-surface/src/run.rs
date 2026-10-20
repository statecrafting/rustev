//! The result envelope of `run` (spec 006, 3.5): how a decision's outcome
//! is printed, whichever host ran it. Running the decision is the host's
//! (the runtime is not a dependency of this crate); the host maps its
//! runtime's results onto these.

use rustev_contract::Document;
use rustev_contract::ids::PlanId;
use rustev_contract::judgment::Judgment;
use rustev_contract::replay::CaptureStatus;
use rustev_contract::run::{RunRecord, Termination};
use serde_json::json;

use crate::out::{Done, Out, code};

/// The fields every decided or rejected `run` prints.
pub fn base(command: &str, decision_id: &str, plan_id: &PlanId) -> Out {
    Out::new(command, "")
        .set("decision_id", decision_id)
        .set("plan_id", plan_id)
}

/// A decision rejected before admission: its kind and detail.
pub fn rejected(base: Out, kind: &str, detail: &str) -> Done {
    Done::new(
        code::REJECTED,
        base.set("status", "rejected")
            .set("rejection", kind)
            .set("detail", detail),
    )
}

/// A delivered decision: where its record went and the sink's receipt.
pub fn delivered(base: Out, record: &str, receipt: &str) -> Out {
    base.set("record", record).set("receipt", receipt)
}

/// A judged decision (`out` from [`delivered`]).
pub fn judged(out: Out, j: &Judgment) -> (Out, u8) {
    (
        addressing(
            out.set("status", "judged")
                .set("completion", "judged")
                .raw("judgment", judgment_bytes(j)),
            j,
        ),
        code::OK,
    )
}

/// A cancelled decision (`out` from [`delivered`]).
pub fn cancelled(out: Out) -> (Out, u8) {
    (
        out.set("status", "cancelled")
            .set("completion", "cancelled"),
        code::CANCELLED,
    )
}

/// A decision whose evidence was not delivered: the record is evidence, not
/// a result to act on.
pub fn not_delivered(base: Out, record: &RunRecord, detail: &str, uncertain: bool) -> (Out, u8) {
    let completion = match record.termination {
        Termination::Judged => "judged",
        Termination::Cancelled => "cancelled",
    };
    (
        base.set("status", "evidence_not_delivered")
            .set("completion", completion)
            .set("detail", detail)
            .set("uncertain", &uncertain)
            .raw("run_record", record_bytes(record)),
        code::EVIDENCE_NOT_DELIVERED,
    )
}

/// For a `rustev.judgment/2`, the outcome's `addressed_to` and `transient`
/// beside the judgment (spec 033, 3.11.3): an unresolved or `needs_input`
/// outcome's action party and transience, a refusal's combined party and
/// `false`, and `null` for a proposal or an escalation (`escalate`, which
/// carries no action). A version 1 judgment's output keeps its bytes. The
/// status stays `judged` and the exit code 0 for every outcome.
fn addressing(out: Out, j: &Judgment) -> Out {
    use rustev_contract::judgment::Outcome;
    if j.schema != rustev_contract::schema::JUDGMENT_V2 {
        return out;
    }
    let (to, transient) = match &j.outcome {
        Outcome::UnresolvedAction { action, .. } => {
            (json!(action.addressed_to), json!(action.transient))
        }
        Outcome::NeedsInput(n) => (json!(n.action.addressed_to), json!(n.action.transient)),
        Outcome::Refused { addressed_to, .. } => (json!(addressed_to), json!(false)),
        _ => (json!(null), json!(null)),
    };
    out.set("addressed_to", &to).set("transient", &transient)
}

/// A judgment's record canonical bytes.
pub fn judgment_bytes(j: &Judgment) -> Vec<u8> {
    // Every judgment the core finishes has a record form; a failure here is
    // a defect in the core's number handling.
    j.record_canonical()
        .expect("a judgment has a record canonical form")
}

/// A run record's record canonical bytes.
pub fn record_bytes(r: &RunRecord) -> Vec<u8> {
    r.record_canonical()
        .expect("a run record has a record canonical form")
}

/// The name of a capture's status.
pub fn capture_status(s: CaptureStatus) -> &'static str {
    match s {
        CaptureStatus::Complete => "complete",
        CaptureStatus::Disabled => "disabled",
        CaptureStatus::LimitExceeded => "limit_exceeded",
        CaptureStatus::Cancelled => "cancelled",
        CaptureStatus::Abandoned => "abandoned",
    }
}
