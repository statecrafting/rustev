//! The host-endpoint principal reference (spec 020, 3.6, I-3).
//!
//! Omitted by default. A host may enable it only for an endpoint it
//! classifies [`EndpointOperator::HostOperated`], only over
//! `rustev.remote/2`, with a mapping version and a declared scope. Per
//! attempt, the host's [`PrincipalSource`] supplies the opaque reference
//! (it may derive it from `CallContext`'s principal handle); Rustev never
//! interprets, authenticates, authorizes or expands it. The reference is
//! routing and isolation context only: not a credential, bearer token,
//! capability, tenant grant, policy fact, input value or action permission,
//! and not proof that the endpoint enforced isolation. It is sent in the
//! identified request field and nowhere else; evidence holds only its
//! domain-separated digest and mapping version.

use std::fmt;
use std::sync::Arc;

use rustev_contract::remote::WirePrincipal;
use serde::Serialize;

/// Who operates the endpoint, as the host classifies it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EndpointOperator {
    /// The default: an endpoint a third party operates. A principal
    /// reference is never sent to it.
    #[default]
    ThirdParty,
    /// An endpoint the host operates.
    HostOperated,
}

/// Supplies the opaque principal reference of one attempt.
pub trait PrincipalSource: Send + Sync {
    /// The reference for the attempt whose `CallContext` carries
    /// `principal_handle`, or `None` when there is none; the attempt is
    /// then refused before any request byte is sent.
    fn reference(&self, principal_handle: &[u8]) -> Option<Vec<u8>>;
}

/// Sends the principal handle itself as the reference. The handle is
/// already opaque to Rustev; a host that wants another reference supplies
/// its own [`PrincipalSource`].
pub struct HandleAsReference;

impl PrincipalSource for HandleAsReference {
    fn reference(&self, principal_handle: &[u8]) -> Option<Vec<u8>> {
        (!principal_handle.is_empty()).then(|| principal_handle.to_vec())
    }
}

/// The principal wire posture of a client.
#[derive(Clone, Default)]
pub enum PrincipalPosture {
    /// The default: no principal byte is ever sent.
    #[default]
    Omitted,
    /// Send the reference in the identified request field.
    Sent {
        mapping_version: String,
        scope: String,
        source: Arc<dyn PrincipalSource>,
    },
}

impl fmt::Debug for PrincipalPosture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PrincipalPosture::Omitted => f.write_str("Omitted"),
            PrincipalPosture::Sent {
                mapping_version,
                scope,
                ..
            } => f
                .debug_struct("Sent")
                .field("mapping_version", mapping_version)
                .field("scope", scope)
                .finish_non_exhaustive(),
        }
    }
}

/// The principal posture as the adapter binding holds it: posture, mapping
/// version and scope, never a reference. Changing any of them changes the
/// binding identity; rotating the reference does not.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalWire {
    #[default]
    Omitted,
    HostOperated {
        mapping_version: String,
        scope: String,
    },
}

impl PrincipalWire {
    pub fn is_omitted(&self) -> bool {
        matches!(self, PrincipalWire::Omitted)
    }
}

/// Why a principal configuration is refused at construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrincipalConfigError {
    /// Principal forwarding to an endpoint the host does not operate.
    ThirdPartyEndpoint,
    /// The reference is a `rustev.remote/2` member.
    NeedsProtocolV2,
    /// A mapping version or scope that is empty, too long or not printable.
    BadLabel,
}

impl fmt::Display for PrincipalConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            PrincipalConfigError::ThirdPartyEndpoint => {
                "a principal reference is never sent to a third-party endpoint"
            }
            PrincipalConfigError::NeedsProtocolV2 => "a principal reference needs rustev.remote/2",
            PrincipalConfigError::BadLabel => {
                "the principal mapping version and scope must be 1 to 64 printable ASCII bytes"
            }
        })
    }
}

impl std::error::Error for PrincipalConfigError {}

impl PrincipalPosture {
    /// Check the posture against the endpoint and protocol, and give the
    /// binding's view of it.
    pub fn validate(
        &self,
        operator: EndpointOperator,
        protocol_major: u32,
    ) -> Result<PrincipalWire, PrincipalConfigError> {
        match self {
            PrincipalPosture::Omitted => Ok(PrincipalWire::Omitted),
            PrincipalPosture::Sent {
                mapping_version,
                scope,
                ..
            } => {
                if operator != EndpointOperator::HostOperated {
                    return Err(PrincipalConfigError::ThirdPartyEndpoint);
                }
                if protocol_major != rustev_contract::remote::PROTOCOL_MAJOR_V2 {
                    return Err(PrincipalConfigError::NeedsProtocolV2);
                }
                // A one-byte reference checks the labels' bounds.
                if WirePrincipal::encode(&[0], mapping_version, scope).is_none() {
                    return Err(PrincipalConfigError::BadLabel);
                }
                Ok(PrincipalWire::HostOperated {
                    mapping_version: mapping_version.clone(),
                    scope: scope.clone(),
                })
            }
        }
    }

    /// The reference to send for an attempt with `principal_handle`:
    /// `Ok(None)` when omitted, `Err(())` when the posture requires one and
    /// the source has none within its bounds (refused before send).
    #[allow(clippy::result_unit_err)]
    pub fn for_attempt(&self, principal_handle: &[u8]) -> Result<Option<WirePrincipal>, ()> {
        match self {
            PrincipalPosture::Omitted => Ok(None),
            PrincipalPosture::Sent {
                mapping_version,
                scope,
                source,
            } => source
                .reference(principal_handle)
                .and_then(|r| WirePrincipal::encode(&r, mapping_version, scope))
                .map(Some)
                .ok_or(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustev_contract::remote::MAX_PRINCIPAL_BYTES;

    fn sent() -> PrincipalPosture {
        PrincipalPosture::Sent {
            mapping_version: "synthetic-map/1".into(),
            scope: "synthetic-tenant-routing".into(),
            source: Arc::new(HandleAsReference),
        }
    }

    #[test]
    fn only_a_host_operated_endpoint_over_version_2_may_receive_it() {
        assert_eq!(
            PrincipalPosture::Omitted.validate(EndpointOperator::ThirdParty, 1),
            Ok(PrincipalWire::Omitted)
        );
        assert_eq!(
            sent().validate(EndpointOperator::ThirdParty, 2),
            Err(PrincipalConfigError::ThirdPartyEndpoint)
        );
        assert_eq!(
            sent().validate(EndpointOperator::HostOperated, 1),
            Err(PrincipalConfigError::NeedsProtocolV2)
        );
        assert!(sent().validate(EndpointOperator::HostOperated, 2).is_ok());
        for (m, s) in [
            ("", "s"),
            ("m", ""),
            ("m m", "s"),
            (&"x".repeat(65)[..], "s"),
        ] {
            let p = PrincipalPosture::Sent {
                mapping_version: m.into(),
                scope: s.into(),
                source: Arc::new(HandleAsReference),
            };
            assert_eq!(
                p.validate(EndpointOperator::HostOperated, 2),
                Err(PrincipalConfigError::BadLabel)
            );
        }
    }

    #[test]
    fn a_missing_or_oversized_reference_is_refused_and_nothing_prints_it() {
        assert_eq!(PrincipalPosture::Omitted.for_attempt(b"h"), Ok(None));
        assert_eq!(sent().for_attempt(b""), Err(()));
        assert_eq!(sent().for_attempt(&[7; MAX_PRINCIPAL_BYTES + 1]), Err(()));
        let p = sent()
            .for_attempt(&[7; MAX_PRINCIPAL_BYTES])
            .unwrap()
            .unwrap();
        assert_eq!(p.reference_bytes().unwrap(), vec![7; MAX_PRINCIPAL_BYTES]);
        let w = sent().for_attempt(b"SYNTHETIC-principal").unwrap().unwrap();
        assert!(!format!("{w:?}").contains(&w.reference));
        assert!(!format!("{:?}", sent()).contains("SYNTHETIC"));
    }
}
