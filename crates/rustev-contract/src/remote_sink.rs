//! Transport-neutral evidence delivery documents (spec 042, 3.11).
//! A receipt is a delivery observation, never authorization.

use crate::DocumentError;
use crate::ids::ContentDigest;
use crate::remote::RemoteCode;
use crate::remote_context::{checked_document, invalid};
use crate::run::RunRecord;
use crate::scope::Handle;
use serde::{Deserialize, Serialize};

pub const PROTOCOL: &str = "rustev.remote-sink/1";
pub const DESCRIBE: &str = "rustev.remote-sink-describe/1";
pub const DELIVER: &str = "rustev.remote-sink-deliver/1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DescribeRequest {
    pub schema: String,
    pub versions: Vec<u32>,
}
crate::document::document!(DescribeRequest, DESCRIBE, crate::limits::REMOTE_V1);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DescribeResponse {
    pub schema: String,
    pub version: u32,
    pub record_schemas: Vec<RecordSchema>,
    /// Sink-defined states; no durability meaning is inferred from a name.
    pub durability_states: Vec<String>,
    pub deduplication_window_ms: u64,
    pub max_record_bytes: u64,
}
crate::document::document!(DescribeResponse, DESCRIBE, crate::limits::REMOTE_V1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub enum RecordSchema {
    #[serde(rename = "rustev.run/1")]
    V1,
    #[serde(rename = "rustev.run/2")]
    V2,
    #[serde(rename = "rustev.run/3")]
    V3,
}
impl RecordSchema {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::V1 => crate::schema::RUN,
            Self::V2 => crate::schema::RUN_V2,
            Self::V3 => crate::schema::RUN_V3,
        }
    }
}

/// Registration's tagged content digest, using the existing digest validator.
pub type RegistrationId = ContentDigest;

/// The registration's signer observation (spec 042, 3.3.3 and 3.3.7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Signer {
    Unsigned,
    Ed25519 { key_id: String },
    Sigstore { identity: String, issuer: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeliverRequest {
    pub schema: String,
    pub version: u32,
    /// A non-empty delivery key, at most 256 bytes (spec 003, 3.9.2).
    pub decision_id: Handle,
    pub record_schema: RecordSchema,
    /// Exact UTF-8 canonical run-record bytes, not a re-rendered object.
    pub record: String,
    pub record_digest: ContentDigest,
    pub registration_id: RegistrationId,
    pub signer: Signer,
    pub service_build: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub traceparent: Option<String>,
}
// The envelope can contain an escaped record at its full existing byte bound.
// It does not enlarge the nested run parser's limits.
pub const DELIVER_LIMITS: crate::bounded::ParseLimits = crate::bounded::ParseLimits {
    max_bytes: 32 * 1024 * 1024,
    max_string_bytes: 24 * 1024 * 1024,
    ..crate::limits::REMOTE_V1
};
checked_document!(
    DeliverRequest,
    DELIVER,
    DELIVER_LIMITS,
    |d: &DeliverRequest| {
        crate::remote_context::validate_traceparent(d.traceparent.as_deref())?;
        let run = RunRecord::parse(d.record.as_bytes())?;
        if run.schema != d.record_schema.as_str() || run.decision_id != d.decision_id.as_str() {
            return Err(invalid(
                "record schema or decision id differs from envelope",
            ));
        }
        let canonical = run.record_canonical().map_err(|e| invalid(e.to_string()))?;
        if canonical != d.record.as_bytes() {
            return Err(invalid("run record is not canonical"));
        }
        if run.record_digest().map_err(|e| invalid(e.to_string()))? != d.record_digest {
            return Err(invalid("run record digest differs from envelope"));
        }
        Ok(())
    }
);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeliverResponse {
    pub schema: String,
    pub result: DeliveryResult,
}
crate::document::document!(DeliverResponse, DELIVER, crate::limits::REMOTE_V1);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum DeliveryResult {
    Accepted {
        durability: String,
        external_id: String,
        accepted_digest: ContentDigest,
    },
    Duplicate {
        accepted_digest: ContentDigest,
    },
    Conflict,
    Rejected {
        code: RemoteCode,
    },
}
impl DeliverResponse {
    /// The caller compares the receipt with the record it actually sent.
    pub fn validate_digest(&self, sent: &ContentDigest) -> Result<(), DocumentError> {
        match &self.result {
            DeliveryResult::Accepted {
                accepted_digest, ..
            }
            | DeliveryResult::Duplicate { accepted_digest }
                if accepted_digest != sent =>
            {
                Err(invalid("malformed_response: accepted digest differs"))
            }
            _ => Ok(()),
        }
    }
}
