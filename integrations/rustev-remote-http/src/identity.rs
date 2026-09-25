//! Adapter binding identity (spec 009, 3.2.3) and served-model identity
//! (3.7, I-2).

use std::collections::BTreeMap;

use rustev_contract::canonical::{CanonicalError, canonical_bytes, tagged_digest};
use rustev_contract::ids::{ArtifactId, DescriptorId};
use rustev_contract::remote::{Disclosure, ServedIdentity, ServedTerms};
use serde::Serialize;

/// The tag of an adapter binding's digest.
pub const BINDING_SCHEMA: &str = "rustev.remote-binding/1";

/// Every output-affecting input of a remote adapter (3.2.3). Its digest is
/// the adapter's `ArtifactId`, so a change to any of them changes the
/// descriptor identity and the `PlanId` of every plan compiled against the
/// adapter. Credentials, network addresses, concurrency and cost rates are
/// configuration, never members.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AdapterBinding {
    /// The wire protocol and its version, for example `rustev.remote/1`.
    pub protocol: String,
    /// A class naming the endpoint, never a secret URL or key.
    pub endpoint_class: String,
    /// The remote backend's own artifact identity, when it reports one.
    pub remote_artifact: Option<ArtifactId>,
    /// The remote backend's descriptor identity, when it reports one.
    pub remote_descriptor: Option<DescriptorId>,
    /// Exactly the model the adapter asks for, and any version pin.
    pub requested_model: Option<String>,
    pub model_pin: Option<String>,
    /// The question mapping's version and any mapping tables.
    pub mapping_version: String,
    pub mapping_tables: BTreeMap<String, String>,
    /// Privacy options that can change routing, and so output (3.8.2).
    pub routing_privacy: BTreeMap<String, String>,
    pub max_request_bytes: u64,
    pub max_response_bytes: u64,
}

impl AdapterBinding {
    /// `sha256(BINDING_SCHEMA || 0x00 || canonical bytes)`, as an artifact.
    pub fn artifact(&self) -> Result<ArtifactId, CanonicalError> {
        let digest = tagged_digest(BINDING_SCHEMA, &canonical_bytes(self)?);
        ArtifactId::parse(&digest).map_err(|e| CanonicalError::Serialize {
            message: e.to_string(),
        })
    }
}

/// A served identity that contradicts a pinned one (3.7): the attempt fails
/// `identity_mismatch` and its output is never supplied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityMismatch {
    pub pinned: String,
    pub served: String,
}

/// The served identity to record, from the negotiated terms and what the
/// remote side reported for this exchange.
///
/// - `pinned`: the pinned identity is recorded; a reported identity that
///   differs is an [`IdentityMismatch`].
/// - `reported`: a reported identity is recorded as `reported`; none is
///   `unknown`.
/// - `unknown`: always `unknown`. A name the remote side offers anyway is
///   not a served identity under these terms; the caller keeps it among the
///   provider extras. Nothing is promoted, and the requested model is never
///   copied into the served field.
pub fn served_identity(
    terms: &ServedTerms,
    reported: Option<&str>,
) -> Result<ServedIdentity, IdentityMismatch> {
    match terms.disclosure {
        Disclosure::Pinned => {
            let pinned = terms.identity.clone().unwrap_or_default();
            match reported {
                Some(r) if r != pinned => Err(IdentityMismatch {
                    pinned,
                    served: r.to_string(),
                }),
                _ => Ok(ServedIdentity {
                    identity: Some(pinned),
                    disclosure: Disclosure::Pinned,
                }),
            }
        }
        Disclosure::Reported => Ok(match reported {
            Some(r) => ServedIdentity {
                identity: Some(r.to_string()),
                disclosure: Disclosure::Reported,
            },
            None => ServedIdentity::unknown(),
        }),
        Disclosure::Unknown => Ok(ServedIdentity::unknown()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terms(d: Disclosure, id: Option<&str>) -> ServedTerms {
        ServedTerms {
            disclosure: d,
            identity: id.map(str::to_string),
        }
    }

    #[test]
    fn a_pinned_identity_must_match() {
        let t = terms(Disclosure::Pinned, Some("m@1"));
        assert_eq!(
            served_identity(&t, Some("m@1")).unwrap().disclosure,
            Disclosure::Pinned
        );
        assert_eq!(
            served_identity(&t, None).unwrap().identity.as_deref(),
            Some("m@1")
        );
        assert!(served_identity(&t, Some("m@2")).is_err());
    }

    #[test]
    fn unknown_is_never_promoted() {
        let t = terms(Disclosure::Unknown, None);
        assert_eq!(
            served_identity(&t, Some("m@9")).unwrap(),
            ServedIdentity::unknown()
        );
        let t = terms(Disclosure::Reported, None);
        let s = served_identity(&t, Some("m@9")).unwrap();
        assert_eq!(
            (s.identity.as_deref(), s.disclosure),
            (Some("m@9"), Disclosure::Reported)
        );
        assert_eq!(
            served_identity(&t, None).unwrap(),
            ServedIdentity::unknown()
        );
    }

    #[test]
    fn every_binding_member_changes_the_artifact() {
        let base = AdapterBinding {
            protocol: "rustev.remote/1".into(),
            endpoint_class: "synthetic-loopback".into(),
            remote_artifact: None,
            remote_descriptor: None,
            requested_model: None,
            model_pin: None,
            mapping_version: "identity/1".into(),
            mapping_tables: BTreeMap::new(),
            routing_privacy: BTreeMap::new(),
            max_request_bytes: 1,
            max_response_bytes: 1,
        };
        let a = base.artifact().unwrap();
        assert_eq!(a, base.clone().artifact().unwrap());
        let variants: Vec<AdapterBinding> = vec![
            AdapterBinding {
                protocol: "x".into(),
                ..base.clone()
            },
            AdapterBinding {
                endpoint_class: "x".into(),
                ..base.clone()
            },
            AdapterBinding {
                requested_model: Some("m".into()),
                ..base.clone()
            },
            AdapterBinding {
                model_pin: Some("p".into()),
                ..base.clone()
            },
            AdapterBinding {
                mapping_version: "x".into(),
                ..base.clone()
            },
            AdapterBinding {
                mapping_tables: BTreeMap::from([("k".into(), "v".into())]),
                ..base.clone()
            },
            AdapterBinding {
                routing_privacy: BTreeMap::from([("k".into(), "v".into())]),
                ..base.clone()
            },
            AdapterBinding {
                max_request_bytes: 2,
                ..base.clone()
            },
            AdapterBinding {
                max_response_bytes: 2,
                ..base.clone()
            },
        ];
        for v in variants {
            assert_ne!(v.artifact().unwrap(), a, "{v:?}");
        }
    }
}
