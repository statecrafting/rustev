//! The closed error taxonomy of spec 009, 3.6: each [`RemoteCode`] maps to
//! exactly one spec 003 failure class, carries a default charge, and is
//! written at the front of the failure detail as `remote:<code>` within the
//! run record's 256-byte detail.

use rustev_contract::remote::{MAX_TEXT_BYTES, RemoteCode, cut_to};
use rustev_contract::run::Charge;
use rustev_core::seams::{AdapterFailure, AttemptReport, CancelAck, RemoteEnd};

/// The one failure class a code maps to (3.6, R-2).
pub const fn failure_class(code: RemoteCode) -> AdapterFailure {
    match code {
        RemoteCode::TransportUnsent
        | RemoteCode::TransportInterrupted
        | RemoteCode::RemoteDeadline
        | RemoteCode::RemoteError => AdapterFailure::Transient,
        RemoteCode::RateLimited | RemoteCode::Overloaded => AdapterFailure::Overloaded,
        RemoteCode::Unauthorized
        | RemoteCode::Capability
        | RemoteCode::MalformedRequest
        | RemoteCode::MalformedResponse
        | RemoteCode::IdentityMismatch => AdapterFailure::Permanent,
        RemoteCode::Cancelled => AdapterFailure::Cancelled,
    }
}

/// True for the refusals whose default charge is `observed{0}` (3.6).
pub const fn is_refusal(code: RemoteCode) -> bool {
    matches!(
        code,
        RemoteCode::RateLimited
            | RemoteCode::Overloaded
            | RemoteCode::Unauthorized
            | RemoteCode::Capability
            | RemoteCode::MalformedRequest
    )
}

/// The charge a code carries when the remote side reported none (3.6).
///
/// `transport_unsent` is `observed{0}`: nothing left the process. A refusal
/// is `observed{0}` only when the remote side documents that refused
/// requests are not charged (`refusals_charged == false`); otherwise, and
/// for every other code, `unknown`. `cancelled` is decided by
/// [`crate::cancel::cancel_outcome`], never here; it is `unknown` here.
pub const fn default_charge(code: RemoteCode, refusals_charged: bool) -> Charge {
    match code {
        RemoteCode::TransportUnsent => Charge::Observed { units: 0 },
        c if is_refusal(c) && !refusals_charged => Charge::Observed { units: 0 },
        _ => Charge::Unknown,
    }
}

/// The charge to report: the reported one when there is one, else the
/// code's default. A reported `unknown` counts as not reported.
pub fn charge_for(code: RemoteCode, reported: Option<Charge>, refusals_charged: bool) -> Charge {
    match (code, reported) {
        // Nothing left the process: nothing can have been charged.
        (RemoteCode::TransportUnsent, _) => Charge::Observed { units: 0 },
        (_, Some(c)) if c != Charge::Unknown => c,
        _ => default_charge(code, refusals_charged),
    }
}

/// `remote:<code>`, then `: ` and `detail` when it is not empty, cut to 256
/// bytes on a character boundary (spec 003, 3.9.3).
pub fn failure_detail(code: RemoteCode, detail: &str) -> String {
    let mut s = format!("remote:{}", code.as_str());
    if !detail.is_empty() {
        s.push_str(": ");
        s.push_str(detail);
    }
    cut_to(&s, MAX_TEXT_BYTES)
}

/// The code a failure detail written by [`failure_detail`] begins with.
pub fn code_of_detail(detail: &str) -> Option<RemoteCode> {
    let rest = detail.strip_prefix("remote:")?;
    let name = rest.split(':').next().unwrap_or(rest);
    RemoteCode::ALL.into_iter().find(|c| c.as_str() == name)
}

/// The code for an HTTP status (3.6 and 3.11); `None` for a success.
///
/// 401 and 403 are `unauthorized`; 429 is `rate_limited`; 503 and 529 are
/// `overloaded`; every other 4xx (400 and 422 by name) is
/// `malformed_request`; everything else a success does not cover, 5xx or
/// unclassifiable, is `remote_error`.
pub const fn classify_status(status: u16) -> Option<RemoteCode> {
    match status {
        200..=299 => None,
        401 | 403 => Some(RemoteCode::Unauthorized),
        429 => Some(RemoteCode::RateLimited),
        503 | 529 => Some(RemoteCode::Overloaded),
        400..=499 => Some(RemoteCode::MalformedRequest),
        _ => Some(RemoteCode::RemoteError),
    }
}

/// Whether remote work may outlive a failure reported of the adapter's own
/// accord (spec 013, 3.1): exactly when request bytes left the process, no
/// complete answer arrived, and so the charge is `unknown`.
pub fn remote_end(code: RemoteCode, sent: bool, charge: Charge) -> RemoteEnd {
    let no_complete_answer = matches!(
        code,
        RemoteCode::TransportInterrupted | RemoteCode::MalformedResponse
    );
    if sent && no_complete_answer && charge == Charge::Unknown {
        RemoteEnd::PossiblyContinuing
    } else {
        RemoteEnd::Finished
    }
}

/// A classified remote failure, before it becomes a report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteFailure {
    pub code: RemoteCode,
    /// Free text after the `remote:<code>` prefix; never a credential.
    pub detail: String,
    pub charge: Charge,
    pub remote: RemoteEnd,
}

impl RemoteFailure {
    /// A failure with the charge chosen by [`charge_for`] and the remote end
    /// by [`remote_end`]. Not for `cancelled`, which
    /// [`crate::cancel::cancelled_report`] builds.
    pub fn new(
        code: RemoteCode,
        detail: impl Into<String>,
        reported: Option<Charge>,
        refusals_charged: bool,
        sent: bool,
    ) -> Self {
        let charge = charge_for(code, reported, refusals_charged);
        RemoteFailure {
            code,
            detail: detail.into(),
            charge,
            remote: remote_end(code, sent, charge),
        }
    }

    /// The seam's report: the code's class and a `remote:<code>` detail.
    pub fn report(&self) -> AttemptReport {
        AttemptReport {
            result: Err((
                failure_class(self.code),
                failure_detail(self.code, &self.detail),
            )),
            charge: self.charge,
            cancel: CancelAck::NotRequested,
            remote: self.remote,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_code_has_one_class_and_the_table_default_charge() {
        use AdapterFailure as F;
        use RemoteCode as C;
        let table = [
            (
                C::TransportUnsent,
                F::Transient,
                Charge::Observed { units: 0 },
            ),
            (C::TransportInterrupted, F::Transient, Charge::Unknown),
            (C::RateLimited, F::Overloaded, Charge::Observed { units: 0 }),
            (C::Overloaded, F::Overloaded, Charge::Observed { units: 0 }),
            (C::Unauthorized, F::Permanent, Charge::Observed { units: 0 }),
            (C::Capability, F::Permanent, Charge::Observed { units: 0 }),
            (
                C::MalformedRequest,
                F::Permanent,
                Charge::Observed { units: 0 },
            ),
            (C::MalformedResponse, F::Permanent, Charge::Unknown),
            (C::IdentityMismatch, F::Permanent, Charge::Unknown),
            (C::RemoteDeadline, F::Transient, Charge::Unknown),
            (C::RemoteError, F::Transient, Charge::Unknown),
            (C::Cancelled, F::Cancelled, Charge::Unknown),
        ];
        assert_eq!(table.len(), C::ALL.len());
        for (code, class, charge) in table {
            assert_eq!(failure_class(code), class, "{code:?}");
            assert_eq!(default_charge(code, false), charge, "{code:?}");
            // Where refusals are charged, only an unsent request is zero.
            let charged = default_charge(code, true);
            if code == C::TransportUnsent {
                assert_eq!(charged, Charge::Observed { units: 0 });
            } else {
                assert_eq!(charged, Charge::Unknown, "{code:?}");
            }
        }
    }

    #[test]
    fn a_reported_charge_wins_except_for_an_unsent_request() {
        let seven = Charge::Observed { units: 7 };
        assert_eq!(
            charge_for(RemoteCode::RemoteError, Some(seven), true),
            seven
        );
        assert_eq!(
            charge_for(RemoteCode::RateLimited, Some(Charge::Unknown), false),
            Charge::Observed { units: 0 }
        );
        assert_eq!(
            charge_for(RemoteCode::TransportUnsent, Some(seven), true),
            Charge::Observed { units: 0 }
        );
    }

    #[test]
    fn statuses_map_to_codes() {
        let cases = [
            (200, None),
            (204, None),
            (400, Some(RemoteCode::MalformedRequest)),
            (401, Some(RemoteCode::Unauthorized)),
            (403, Some(RemoteCode::Unauthorized)),
            (404, Some(RemoteCode::MalformedRequest)),
            (413, Some(RemoteCode::MalformedRequest)),
            (422, Some(RemoteCode::MalformedRequest)),
            (429, Some(RemoteCode::RateLimited)),
            (500, Some(RemoteCode::RemoteError)),
            (502, Some(RemoteCode::RemoteError)),
            (503, Some(RemoteCode::Overloaded)),
            (529, Some(RemoteCode::Overloaded)),
            (302, Some(RemoteCode::RemoteError)),
            (100, Some(RemoteCode::RemoteError)),
            (999, Some(RemoteCode::RemoteError)),
        ];
        for (s, c) in cases {
            assert_eq!(classify_status(s), c, "{s}");
        }
    }

    #[test]
    fn details_carry_the_code_within_256_bytes() {
        for c in RemoteCode::ALL {
            let d = failure_detail(c, &"x".repeat(1000));
            assert!(d.len() <= 256);
            assert!(d.starts_with(&format!("remote:{}", c.as_str())));
            assert_eq!(code_of_detail(&d), Some(c));
        }
        assert_eq!(
            failure_detail(RemoteCode::Capability, ""),
            "remote:capability"
        );
        assert_eq!(
            code_of_detail("remote:capability"),
            Some(RemoteCode::Capability)
        );
        assert_eq!(code_of_detail("no rule matched"), None);
    }

    #[test]
    fn possibly_continuing_only_when_sent_and_unanswered() {
        let u = Charge::Unknown;
        let pc = RemoteEnd::PossiblyContinuing;
        assert_eq!(remote_end(RemoteCode::TransportInterrupted, true, u), pc);
        assert_eq!(remote_end(RemoteCode::MalformedResponse, true, u), pc);
        assert_eq!(
            remote_end(
                RemoteCode::MalformedResponse,
                true,
                Charge::Observed { units: 1 }
            ),
            RemoteEnd::Finished
        );
        assert_eq!(
            remote_end(RemoteCode::TransportInterrupted, false, u),
            RemoteEnd::Finished
        );
        assert_eq!(
            remote_end(RemoteCode::RemoteError, true, u),
            RemoteEnd::Finished
        );
    }
}
