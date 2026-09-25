//! Cancellation that never claims more than is known (spec 009, 3.4.3 and
//! I-6): `stopped` only when no request byte left the process, or when the
//! remote side confirmed the stop with its final charge; `best_effort`
//! never yields `stopped`.

use rustev_contract::remote::{CancelSupport, RemoteCode};
use rustev_contract::run::Charge;
use rustev_core::seams::{AdapterFailure, AttemptReport, CancelAck, RemoteEnd};

use crate::taxonomy::failure_detail;

/// What a cancelled attempt can state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CancelOutcome {
    pub ack: CancelAck,
    pub charge: Charge,
    pub remote: RemoteEnd,
}

/// Decide the acknowledgement of a cancelled attempt.
///
/// - `sent == false` (no request byte can ever leave; see
///   [`crate::wire::SentTracker::try_abort`]): `stopped`, `observed{0}`.
/// - sent, `support == confirmed` and the remote side confirmed the stop
///   with `confirmed_charge` within the budget: `stopped` with that charge.
/// - otherwise: `unconfirmed`, charge `unknown`, possibly continuing.
pub fn cancel_outcome(
    sent: bool,
    support: CancelSupport,
    confirmed_charge: Option<Charge>,
) -> CancelOutcome {
    if !sent {
        return CancelOutcome {
            ack: CancelAck::Stopped,
            charge: Charge::Observed { units: 0 },
            remote: RemoteEnd::Finished,
        };
    }
    match (support, confirmed_charge) {
        (CancelSupport::Confirmed, Some(c)) if c != Charge::Unknown => CancelOutcome {
            ack: CancelAck::Stopped,
            charge: c,
            remote: RemoteEnd::Finished,
        },
        _ => CancelOutcome {
            ack: CancelAck::Unconfirmed,
            charge: Charge::Unknown,
            remote: RemoteEnd::PossiblyContinuing,
        },
    }
}

/// The report of a cancelled attempt, `remote:cancelled` with the outcome of
/// [`cancel_outcome`].
pub fn cancelled_report(outcome: CancelOutcome, detail: &str) -> AttemptReport {
    AttemptReport {
        result: Err((
            AdapterFailure::Cancelled,
            failure_detail(RemoteCode::Cancelled, detail),
        )),
        charge: outcome.charge,
        cancel: outcome.ack,
        remote: outcome.remote,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stopped_only_when_unsent_or_confirmed() {
        for support in [
            CancelSupport::None,
            CancelSupport::BestEffort,
            CancelSupport::Confirmed,
        ] {
            let o = cancel_outcome(false, support, None);
            assert_eq!(
                (o.ack, o.charge),
                (CancelAck::Stopped, Charge::Observed { units: 0 })
            );
            let o = cancel_outcome(true, support, None);
            assert_eq!((o.ack, o.charge), (CancelAck::Unconfirmed, Charge::Unknown));
            assert_eq!(o.remote, RemoteEnd::PossiblyContinuing);
        }
        let c = Some(Charge::Observed { units: 2 });
        assert_eq!(
            cancel_outcome(true, CancelSupport::Confirmed, c).ack,
            CancelAck::Stopped
        );
        // Best effort never yields stopped, even with a charge in hand.
        assert_eq!(
            cancel_outcome(true, CancelSupport::BestEffort, c).ack,
            CancelAck::Unconfirmed
        );
        // A confirmation without a final charge is not a confirmed stop.
        assert_eq!(
            cancel_outcome(true, CancelSupport::Confirmed, Some(Charge::Unknown)).ack,
            CancelAck::Unconfirmed
        );
    }
}
