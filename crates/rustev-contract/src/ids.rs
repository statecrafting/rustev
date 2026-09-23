//! Identities (spec 002, 3.3.2 and 3.3.3): `sha256:` and 64 lowercase hex
//! digits. Computed identities come from [`crate::Identified::id`]; supplied
//! ones (`ArtifactId`, `DatasetId`, `EvaluatorConfigId`) are validated only.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Why a string is not an identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdError {
    pub value: String,
}

impl fmt::Display for IdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "not an identity (`sha256:` and 64 lowercase hex digits): {:?}",
            self.value
        )
    }
}

impl std::error::Error for IdError {}

fn is_digest(s: &str) -> bool {
    s.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

macro_rules! identity {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            /// Validate `s` as an identity.
            pub fn parse(s: &str) -> Result<Self, IdError> {
                if is_digest(s) {
                    Ok(Self(s.to_string()))
                } else {
                    Err(IdError { value: s.chars().take(80).collect() })
                }
            }

            /// The textual form.
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Only for digests this crate computed.
            #[allow(dead_code)]
            pub(crate) fn from_digest(s: String) -> Self {
                debug_assert!(is_digest(&s));
                Self(s)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let s = String::deserialize(d)?;
                Self::parse(&s).map_err(serde::de::Error::custom)
            }
        }
    };
}

identity!(
    /// Identity of a canonical decision definition.
    DefinitionId
);
identity!(
    /// Identity of a canonical compiled plan.
    PlanId
);
identity!(
    /// Identity of a canonical backend capability descriptor.
    DescriptorId
);
identity!(
    /// Identity of a canonical calibration artifact.
    CalibrationId
);
identity!(
    /// Identity of a canonical context snapshot.
    SnapshotId
);
identity!(
    /// Identity of a canonical execution policy (spec 003, 3.2.2).
    ExecutionPolicyId
);
identity!(
    /// A backend artifact (model, tokenizer, preprocessing, truncation,
    /// precision), identified by its producer.
    ArtifactId
);
identity!(
    /// A dataset, identified by its producer.
    DatasetId
);
identity!(
    /// An evaluator configuration, identified by its producer.
    EvaluatorConfigId
);

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "sha256:0000000000000000000000000000000000000000000000000000000000000abc";

    #[test]
    fn identities_are_validated() {
        assert!(ArtifactId::parse(GOOD).is_ok());
        for bad in [
            "",
            "sha256:",
            "sha256:ABC0000000000000000000000000000000000000000000000000000000000000",
            "sha512:0000000000000000000000000000000000000000000000000000000000000abc",
            "sha256:000000000000000000000000000000000000000000000000000000000000abc",
            "sha256:00000000000000000000000000000000000000000000000000000000000000abc",
        ] {
            assert!(ArtifactId::parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn serde_round_trips_and_refuses_invalid() {
        let id: DatasetId = serde_json::from_str(&format!("\"{GOOD}\"")).unwrap();
        assert_eq!(serde_json::to_string(&id).unwrap(), format!("\"{GOOD}\""));
        assert!(serde_json::from_str::<DatasetId>("\"sha256:xyz\"").is_err());
    }
}
