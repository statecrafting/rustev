//! Replay isolation scopes (spec 004, 3.1.3 to 3.1.5).
//!
//! A scope is a host-configured isolation key for retained evidence, never
//! an authority grant. Its two modes are mutually exclusive; a serialized
//! scope always states its mode and every field of that mode, and a field of
//! the other mode is refused. Principal scope is the default: a missing
//! principal handle is an error, never a downgrade to tenant-only.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The most bytes of one scope handle.
pub const MAX_HANDLE_BYTES: usize = 256;

/// Why a string is not a scope handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandleError {
    pub len: usize,
}

impl fmt::Display for HandleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "a scope handle is 1 to {MAX_HANDLE_BYTES} bytes, not {}",
            self.len
        )
    }
}

impl std::error::Error for HandleError {}

/// An opaque, non-secret, non-empty host handle of at most 256 bytes: a
/// tenant, a context revision or a principal scope. Rustev compares
/// handles and nothing else; a raw credential never belongs in one.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Handle(String);

impl Handle {
    pub fn new(s: impl Into<String>) -> Result<Self, HandleError> {
        let s = s.into();
        if s.is_empty() || s.len() > MAX_HANDLE_BYTES {
            return Err(HandleError { len: s.len() });
        }
        Ok(Self(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for Handle {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Handle {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Handle::new(String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

/// The isolation scope of a capture, a bundle and every request identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum Scope {
    /// Reuse across principals within one tenant and context revision: an
    /// explicit host assertion that the computation is principal-independent
    /// and that the host authorizes every reader (spec 004, 3.1.4).
    TenantOnly {
        tenant: Handle,
        context_revision: Handle,
    },
    /// Reuse only within one tenant, context revision and effective
    /// principal scope. The default.
    Principal {
        tenant: Handle,
        context_revision: Handle,
        principal_scope: Handle,
    },
}

impl Scope {
    /// The default mode: principal scope, every handle required.
    pub fn principal(tenant: Handle, context_revision: Handle, principal_scope: Handle) -> Self {
        Scope::Principal {
            tenant,
            context_revision,
            principal_scope,
        }
    }

    /// The explicit tenant-only opt-in.
    pub fn tenant_only(tenant: Handle, context_revision: Handle) -> Self {
        Scope::TenantOnly {
            tenant,
            context_revision,
        }
    }

    pub fn mode(&self) -> &'static str {
        match self {
            Scope::TenantOnly { .. } => "tenant_only",
            Scope::Principal { .. } => "principal",
        }
    }

    pub fn tenant(&self) -> &Handle {
        match self {
            Scope::TenantOnly { tenant, .. } | Scope::Principal { tenant, .. } => tenant,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(s: &str) -> Handle {
        Handle::new(s).unwrap()
    }

    #[test]
    fn handles_are_bounded_and_non_empty() {
        assert!(Handle::new("").is_err());
        assert!(Handle::new("x".repeat(MAX_HANDLE_BYTES)).is_ok());
        assert!(Handle::new("x".repeat(MAX_HANDLE_BYTES + 1)).is_err());
        assert!(serde_json::from_str::<Handle>("\"\"").is_err());
    }

    #[test]
    fn both_modes_round_trip_and_state_their_mode() {
        for s in [
            Scope::tenant_only(h("t"), h("r1")),
            Scope::principal(h("t"), h("r1"), h("p")),
        ] {
            let text = serde_json::to_string(&s).unwrap();
            assert!(
                text.contains(&format!("\"mode\":\"{}\"", s.mode())),
                "{text}"
            );
            assert_eq!(serde_json::from_str::<Scope>(&text).unwrap(), s);
        }
    }

    #[test]
    fn omitted_unknown_or_mixed_modes_are_refused() {
        for bad in [
            // No mode: never an implied default or public namespace.
            r#"{"tenant":"t","context_revision":"r"}"#,
            r#"{"mode":"public","tenant":"t","context_revision":"r"}"#,
            // Fields of the other variant.
            r#"{"mode":"tenant_only","tenant":"t","context_revision":"r","principal_scope":"p"}"#,
            // A principal scope without its principal handle never downgrades.
            r#"{"mode":"principal","tenant":"t","context_revision":"r"}"#,
            r#"{"mode":"principal","tenant":"t","context_revision":"","principal_scope":"p"}"#,
        ] {
            assert!(serde_json::from_str::<Scope>(bad).is_err(), "{bad}");
        }
        // Negative control.
        assert!(
            serde_json::from_str::<Scope>(
                r#"{"mode":"principal","tenant":"t","context_revision":"r","principal_scope":"p"}"#
            )
            .is_ok()
        );
    }

    #[test]
    fn modes_never_compare_equal() {
        assert_ne!(
            Scope::tenant_only(h("t"), h("r")),
            Scope::principal(h("t"), h("r"), h("p"))
        );
    }
}
