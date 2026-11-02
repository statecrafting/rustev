//! Plan bundle integrity documents (spec 042, 3.3). Signatures and
//! registrations describe integrity, never execution authority.
use crate::bounded::{ParseLimits, parse_bounded};
use crate::canonical::{CanonicalError, tagged_digest};
use crate::ids::{ContentDigest, ExecutionPolicyId, PlanId, ProfileId};
use crate::scope::Handle;
use crate::serve::{
    BUNDLE_LIMITS, Embedded, List, Maybe, SERVICE_LIMITS, Text, Validate, canonical_envelope,
    invalid, service_document,
};
use crate::{Document, DocumentError, Timestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum SigningMode {
    RequireSigned,
    AllowUnsigned,
    /// Admits unsigned bundles except those whose resolved profile chain,
    /// leaf or base, carries a listed `ProfileId` (3.3.6, 3.3.8). The list
    /// holds 1 to 1024 ids (an engineering bound; an empty list would be
    /// `allow_unsigned` under another name).
    DenyUnsignedForProfiles(List<ProfileId, 1, 1024>),
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
/// A bundle's definition (3.3.1, 3.3.8): a definition document, or a spec
/// 034 template, a `rustev.definition/2` document declaring `parameters` or
/// `variants`. A concrete definition is checked by its own parser; a
/// template is bounded here and checked by instantiation (034 3.9 stage 1),
/// whose refusal is `compile_refused`, so anything else is refused here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BundleDefinition(serde_json::Value);
impl Document for BundleDefinition {
    // The definition's own schema string: `Embedded` names it in the
    // bundle's JSON Schema, which is unchanged.
    const SCHEMA: &'static str = <crate::definition::Definition as Document>::SCHEMA;
    const LIMITS: ParseLimits = crate::limits::DEFINITION_V1;
    fn schema(&self) -> &str {
        self.0["schema"].as_str().unwrap_or_default()
    }
    fn parse(bytes: &[u8]) -> Result<Self, DocumentError> {
        if crate::definition::Definition::parse(bytes).is_ok() {
            return Ok(Self(
                parse_bounded(bytes, &Self::LIMITS).map_err(DocumentError::Parse)?,
            ));
        }
        let value: serde_json::Value =
            parse_bounded(bytes, &Self::LIMITS).map_err(DocumentError::Parse)?;
        let template = value["schema"] == crate::schema::DEFINITION_V2
            && (value.get("parameters").is_some() || value.get("variants").is_some());
        if !template {
            // The concrete parser's own refusal.
            crate::definition::Definition::parse(bytes)?;
        }
        Ok(Self(value))
    }
}
/// The variant a bundle is instantiated for (3.3.8): `{"named": <name>}`,
/// the name matching `[a-z][a-z0-9_]{0,63}` (034 3.1.3). The variant `none`
/// is spelled by omitting the member.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BundleVariant {
    #[schemars(regex(pattern = r"^[a-z][a-z0-9_]{0,63}$"))]
    pub named: String,
}
impl BundleVariant {
    fn valid(&self) -> bool {
        let mut chars = self.named.chars();
        self.named.len() <= 64
            && chars.next().is_some_and(|c| c.is_ascii_lowercase())
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    }
}
service_document!(PlanBundle,PlanBundleSchema,"rustev.plan-bundle/1",BUNDLE_LIMITS,{
    pub definition:Embedded<BundleDefinition>,
    pub execution_policy:Maybe<Embedded<crate::execution::ExecutionPolicy>>,
    pub calibrations:List<Embedded<crate::calibration::CalibrationArtifact>,0,1024>,
    /// The tenant profile, the leaf of the chain (spec 034); omitted for
    /// none, so a bundle without one keeps its bytes (3.3.8).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile:Option<Embedded<crate::profile::Profile>>,
    /// The variant the bundle is instantiated for; omitted for `none`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant:Option<BundleVariant>,
    /// 1 to 64 profiles available to the leaf's `extends` chain; omitted
    /// for none, and only with a profile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_bases:Option<List<Embedded<crate::profile::Profile>,1,64>>,
    #[schemars(extend("maxProperties" = 1024))] pub bindings:BTreeMap<Handle,Handle>,pub compile_report:CompileReport,
});
impl Validate for PlanBundle {
    fn validate(&self) -> Result<(), String> {
        if self.bindings.len() > 1024 {
            return Err("at most 1024 backend bindings".into());
        }
        if self.variant.as_ref().is_some_and(|v| !v.valid()) {
            return Err("variant names match [a-z][a-z0-9_]{0,63}".into());
        }
        if self.profile_bases.is_some() && self.profile.is_none() {
            return Err("profile_bases requires a profile".into());
        }
        Ok(())
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
    /// The leaf profile's `ProfileId` (3.3.3, 3.3.8); omitted for none, so
    /// a registration without a profile keeps its `RegistrationId`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id:Option<ProfileId>,
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
