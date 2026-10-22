//! Plan bundle integrity documents (spec 042, 3.3). Signatures and
//! registrations describe integrity, never execution authority.
use crate::bounded::{ParseLimits, parse_bounded};
use crate::canonical::{CanonicalError, tagged_digest};
use crate::ids::{ContentDigest, ExecutionPolicyId, PlanId};
use crate::scope::Handle;
use crate::serve::{
    BUNDLE_LIMITS, Embedded, List, Maybe, SERVICE_LIMITS, Text, Validate, canonical_envelope,
    invalid, service_document,
};
use crate::{Document, DocumentError, Timestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SigningMode {
    RequireSigned,
    AllowUnsigned,
    // Extension point: spec 034 (deny_unsigned_for_profiles).
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Signer {
    Unsigned,
    Ed25519 {
        key_id: Handle,
    },
    Sigstore {
        certificate_identity: Text<1, 4096>,
        issuer: Text<1, 4096>,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct RegistrationId(pub ContentDigest);
impl RegistrationId {
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}
/// Successful output of the existing surface compile command. Output paths
/// are report observations; they never become an endpoint in a binding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompileReport {
    pub command: CompileCommand,
    pub status: CompileStatus,
    pub plan_id: PlanId,
    pub out: Text<1, 4096>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CompileCommand {
    #[serde(rename = "plan compile")]
    Compile,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum CompileStatus {
    #[serde(rename = "compiled")]
    Compiled,
}
service_document!(PlanBundle,PlanBundleSchema,"rustev.plan-bundle/1",BUNDLE_LIMITS,{
    pub definition:Embedded<crate::definition::Definition>,
    pub execution_policy:Maybe<Embedded<crate::execution::ExecutionPolicy>>,
    pub calibrations:List<Embedded<crate::calibration::CalibrationArtifact>,0,1024>,
    // Extension point: spec 034 (parameters, variants and tenant profile).
    #[schemars(extend("maxProperties" = 1024))] pub bindings:BTreeMap<Handle,Handle>,pub compile_report:CompileReport,
});
impl Validate for PlanBundle {
    fn validate(&self) -> Result<(), String> {
        if self.bindings.len() > 1024 {
            Err("at most 1024 backend bindings".into())
        } else {
            Ok(())
        }
    }
}
impl PlanBundle {
    /// `tagged_digest` of the bundle's canonical bytes (042 3.3): exactly
    /// the bytes `Document::canonical` produces, so a verifier that hashes
    /// `canonical()` reproduces it.
    pub fn digest(&self) -> Result<ContentDigest, CanonicalError> {
        Ok(ContentDigest::from_digest(tagged_digest(
            Self::SCHEMA,
            &Document::canonical(self)?,
        )))
    }
}
/// Ed25519 signature octets, exactly 64. No encoding ambiguity and no key
/// material is carried in a plan bundle.
pub type Ed25519Signature = List<u8, 64, 64>;
/// A Sigstore bundle is a third-party document: its bytes are bounded to
/// 1 MiB, must parse as a JSON object, and are kept exactly as received so
/// the verifier sees the signed bytes unchanged. This type makes no
/// verification claim.
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct SigstoreBundle(Box<serde_json::value::RawValue>);
impl PartialEq for SigstoreBundle {
    fn eq(&self, other: &Self) -> bool {
        self.0.get() == other.0.get()
    }
}
impl SigstoreBundle {
    /// The bundle's bytes exactly as received.
    pub fn bytes(&self) -> &[u8] {
        self.0.get().as_bytes()
    }
}
impl JsonSchema for SigstoreBundle {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "SigstoreBundle".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type":"object","maxProperties":10000,"description":"Opaque Sigstore JSON: at most 1 MiB, depth 32, strings 256 KiB, collections 10000, total values 100000, integer tokens 40 bytes."})
    }
}
impl<'de> Deserialize<'de> for SigstoreBundle {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = Box::<serde_json::value::RawValue>::deserialize(d)?;
        let limits = ParseLimits {
            max_bytes: 1024 * 1024,
            max_depth: 32,
            max_string_bytes: 256 * 1024,
            max_collection_len: 10000,
            max_total_values: 100000,
            max_number_bytes: 40,
            allow_fractional_numbers: false,
        };
        let value: serde_json::Value =
            parse_bounded(raw.get().as_bytes(), &limits).map_err(serde::de::Error::custom)?;
        if !value.is_object() {
            return Err(serde::de::Error::custom(
                "Sigstore bundle must be an object",
            ));
        }
        Ok(Self(raw))
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum SignatureScheme {
    Ed25519 {
        key_id: Handle,
        signature: Ed25519Signature,
    },
    Sigstore {
        bundle: SigstoreBundle,
    },
}
service_document!(PlanBundleSignature,PlanBundleSignatureSchema,"rustev.plan-bundle-signature/1",SERVICE_LIMITS,{
    pub bundle_digest:ContentDigest,pub scheme:SignatureScheme,
});
impl Validate for PlanBundleSignature {}
impl PlanBundleSignature {
    pub fn signing_input(&self) -> Vec<u8> {
        format!("{}\n{}", Self::SCHEMA, self.bundle_digest).into_bytes()
    }
}
service_document!(PlanRegistration,PlanRegistrationSchema,"rustev.plan-registration/1",SERVICE_LIMITS,{
    pub plan_id:PlanId,pub execution_policy_id:Maybe<ExecutionPolicyId>,
    // Extension point: spec 034 (profile_id).
    pub bundle_digest:ContentDigest,pub signer:Signer,pub signing_mode:SigningMode,
    pub service_build:Handle,pub configuration_digest:ContentDigest,pub registered_at_ms:Timestamp,
});
impl Validate for PlanRegistration {}
impl PlanRegistration {
    /// Registration time is an observation and does not affect identity.
    pub fn registration_id(&self) -> Result<RegistrationId, CanonicalError> {
        let mut value = serde_json::to_value(self).map_err(|e| CanonicalError::Serialize {
            message: e.to_string(),
        })?;
        value.as_object_mut().unwrap().remove("registered_at_ms");
        Ok(RegistrationId(ContentDigest::from_digest(tagged_digest(
            Self::SCHEMA,
            &crate::canonical::canonical_value_bytes(&value)?,
        ))))
    }
}
