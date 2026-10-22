//! The two store operations the sink needs, behind a trait so the failure
//! mapping of binding record item 5 is tested against a scripted store as
//! well as a real one. [`StoreHandle`] is the implementation a host uses.

use rahi_store::receipt::{Classification, ContentDigest, ReceiptKey, Receipts};
use rahi_store::{Statement, StoreHandle};
use rahi_types::Error;
use rustev_core::seams::BoxFuture;

/// What Rahi's receipts already hold for one identity, reduced to what the
/// sink decides on (Rahi's `Classification`, `receipt.rs:256-283`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seen {
    /// Nothing is recorded for this decision id.
    First,
    /// A record with this digest is recorded (`Redelivered`).
    Same,
    /// The decision id is recorded with another digest (`Changed`, or a
    /// recorded collision), with the accepted digest's hex when Rahi
    /// reports it.
    Other(Option<String>),
    /// The decision id was erased; nothing is accepted for it again.
    Erased,
}

/// The accepted digest a `Changed` classification reports, or `None` when
/// it reports none: Rahi fills it from the head row with
/// `accepted_digest.unwrap_or_default()`, so a head without an accepted
/// digest reads as the empty string, which is no digest.
fn reported(head_digest: &str) -> Option<String> {
    (!head_digest.is_empty()).then(|| head_digest.to_owned())
}

/// A consistent read of the receipt state and an all-or-nothing commit.
pub trait ReceiptPort: Send + Sync {
    /// Classify `digest` under `key`, reading through the leader.
    fn classify<'a>(
        &'a self,
        key: &'a ReceiptKey,
        digest: &'a ContentDigest,
    ) -> BoxFuture<'a, Result<Seen, Error>>;

    /// Submit `statements` as one transaction: every statement lands or
    /// none does.
    fn commit(&self, statements: Vec<Statement>) -> BoxFuture<'_, Result<(), Error>>;
}

impl ReceiptPort for StoreHandle {
    fn classify<'a>(
        &'a self,
        key: &'a ReceiptKey,
        digest: &'a ContentDigest,
    ) -> BoxFuture<'a, Result<Seen, Error>> {
        Box::pin(async move {
            Ok(match Receipts::classify(self, key, digest).await? {
                Classification::FirstSeen => Seen::First,
                Classification::Redelivered { .. } => Seen::Same,
                Classification::Changed(changed) => Seen::Other(reported(changed.head_digest())),
                Classification::CollisionRedelivered { .. } => Seen::Other(None),
                Classification::Erased { .. } => Seen::Erased,
            })
        })
    }

    fn commit(&self, statements: Vec<Statement>) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async move { self.txn(statements).await.map(|_| ()) })
    }
}

#[cfg(test)]
mod tests {
    use super::reported;

    #[test]
    fn an_empty_head_digest_is_no_digest() {
        assert_eq!(reported(""), None);
        assert_eq!(reported("ab12"), Some("ab12".to_owned()));
    }
}
