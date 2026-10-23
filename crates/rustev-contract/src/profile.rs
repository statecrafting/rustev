//! Profile documents and definition-independent validation (spec 034, 3.5).
use crate::{
    Document, DocumentError, Identified,
    bounded::{ParseError, parse_bounded},
    ids::ProfileId,
    limits::DESCRIPTOR_V1,
    parameter::MAX_PARAMETERS,
    schema,
    version::{Version, VersionRange},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, fmt};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema: String,
    pub profile: String,
    pub version: String,
    pub definition: ProfileDefinition,
    pub extends: String,
    pub values: BTreeMap<String, Value>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProfileDefinition {
    pub name: String,
    pub version: String,
}

/// Names the invalid document member, for core's `invalid-profile` refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileError {
    pub member: &'static str,
}
impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid profile member {}", self.member)
    }
}
impl std::error::Error for ProfileError {}
/// Profile labels are opaque ASCII names (spec 034, 3.5.2).
pub fn is_profile_name(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.as_bytes()[0].is_ascii_alphanumeric()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
/// An absent base or an exact profile name and version (spec 034, 3.5.1).
pub fn is_profile_extends(s: &str) -> bool {
    s == "none"
        || s.split_once('@')
            .is_some_and(|(n, v)| is_profile_name(n) && v.parse::<Version>().is_ok())
}
impl Profile {
    pub fn validate(&self) -> Result<(), ProfileError> {
        for (member, valid) in [
            ("schema", self.schema == schema::PROFILE),
            ("profile", is_profile_name(&self.profile)),
            ("version", self.version.parse::<Version>().is_ok()),
            (
                "definition.version",
                self.definition.version.parse::<VersionRange>().is_ok(),
            ),
            ("extends", is_profile_extends(&self.extends)),
            ("values", self.values.len() <= MAX_PARAMETERS),
        ] {
            if !valid {
                return Err(ProfileError { member });
            }
        }
        Ok(())
    }
}
impl Document for Profile {
    const SCHEMA: &'static str = schema::PROFILE;
    const LIMITS: crate::bounded::ParseLimits = DESCRIPTOR_V1;
    fn schema(&self) -> &str {
        &self.schema
    }
    fn parse(bytes: &[u8]) -> Result<Self, DocumentError> {
        let doc: Self = parse_bounded(bytes, &Self::LIMITS).map_err(DocumentError::Parse)?;
        if doc.schema != Self::SCHEMA {
            return Err(DocumentError::Schema {
                expected: Self::SCHEMA,
                found: doc.schema.chars().take(80).collect(),
            });
        }
        doc.validate().map_err(|e| {
            DocumentError::Parse(ParseError::Schema {
                message: e.to_string(),
            })
        })?;
        Ok(doc)
    }
}
impl Identified for Profile {
    type Id = ProfileId;
    fn wrap(digest: String) -> ProfileId {
        ProfileId::from_digest(digest)
    }
}
