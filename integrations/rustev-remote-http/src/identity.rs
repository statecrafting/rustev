//! Adapter binding identity (spec 009, 3.2.3) and served-model identity
//! (3.7, I-2).

use std::collections::BTreeMap;

use rustev_contract::canonical::{CanonicalError, canonical_bytes, tagged_digest};
use rustev_contract::ids::{ArtifactId, DescriptorId};
use rustev_contract::remote::{Disclosure, ServedIdentity, ServedTerms};
use serde::Serialize;

use crate::phases::TransportPhases;
use crate::pool::PoolBinding;
use crate::principal::PrincipalWire;

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
    /// The transport phase policy (spec 020, 3.1 and 3.4). `disabled` is
    /// not written, so a binding with every spec 020 refinement disabled
    /// keeps its spec 009 bytes and artifact.
    #[serde(skip_serializing_if = "TransportPhases::is_disabled")]
    pub transport_phases: TransportPhases,
    /// The principal wire posture, mapping version and scope (spec 020,
    /// 3.1 and 3.6); never a reference. Not written when omitted.
    #[serde(skip_serializing_if = "PrincipalWire::is_omitted")]
    pub principal_wire: PrincipalWire,
    /// The run-record schema the evidence sink accepts, when it is
    /// `rustev.run/2` (spec 020, 3.1 and 3.2). Not written otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_record_schema: Option<String>,
    /// The most members of a runtime-planned batch this binding accepts
    /// (spec 020, 3.1 and 3.3), when that dispatch is enabled. Not written
    /// when disabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub batch_dispatch_max_members: Option<u32>,
    /// The connection-pool policy and isolation-key schema (spec 020, 3.1
    /// and 3.5). Not written when disabled.
    #[serde(skip_serializing_if = "PoolBinding::is_disabled")]
    pub connection_pool: PoolBinding,
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
    fn a_binding_with_every_refinement_disabled_keeps_its_spec_009_bytes() {
        // Spec 020, 2: disabled refinements are not written, so the binding
        // and the artifact of every existing adapter are unchanged.
        let b = AdapterBinding {
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
            transport_phases: TransportPhases::Disabled,
            principal_wire: PrincipalWire::Omitted,
            run_record_schema: None,
            batch_dispatch_max_members: None,
            connection_pool: PoolBinding::Disabled,
        };
        assert_eq!(
            String::from_utf8(canonical_bytes(&b).unwrap()).unwrap(),
            concat!(
                r#"{"endpoint_class":"synthetic-loopback","mapping_tables":{},"#,
                r#""mapping_version":"identity/1","max_request_bytes":1,"#,
                r#""max_response_bytes":1,"model_pin":null,"protocol":"rustev.remote/1","#,
                r#""remote_artifact":null,"remote_descriptor":null,"requested_model":null,"#,
                r#""routing_privacy":{}}"#
            )
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
            transport_phases: TransportPhases::Disabled,
            principal_wire: PrincipalWire::Omitted,
            run_record_schema: None,
            batch_dispatch_max_members: None,
            connection_pool: PoolBinding::Disabled,
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
            AdapterBinding {
                transport_phases: TransportPhases::Enabled(crate::phases::PhasePolicy {
                    resolution_ms: 1,
                    connect_ms: 1,
                    secure_handshake_ms: 1,
                    request_write_ms: 1,
                    first_response_byte_ms: 1,
                    response_body_ms: 1,
                    cancellation_ack_ms: 1,
                }),
                ..base.clone()
            },
            AdapterBinding {
                principal_wire: PrincipalWire::HostOperated {
                    mapping_version: "m/1".into(),
                    scope: "s".into(),
                },
                ..base.clone()
            },
            AdapterBinding {
                run_record_schema: Some("rustev.run/2".into()),
                ..base.clone()
            },
            AdapterBinding {
                batch_dispatch_max_members: Some(4),
                ..base.clone()
            },
            AdapterBinding {
                connection_pool: PoolBinding::Enabled {
                    policy: crate::pool::PoolPolicy {
                        max_connections: 1,
                        max_per_origin: 1,
                        max_streams: 1,
                        max_idle: 1,
                        idle_lifetime_ms: 1,
                        max_lifetime_ms: 1,
                        max_uses: 1,
                        on_exhaustion: crate::pool::OnExhaustion::Refuse,
                    },
                    key_schema: crate::pool::POOL_KEY_SCHEMA.into(),
                },
                ..base.clone()
            },
            AdapterBinding {
                principal_wire: PrincipalWire::HostOperated {
                    mapping_version: "m/2".into(),
                    scope: "s".into(),
                },
                ..base.clone()
            },
        ];
        for v in variants {
            assert_ne!(v.artifact().unwrap(), a, "{v:?}");
        }
    }
}
