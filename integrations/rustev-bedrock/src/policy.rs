//! Host policy is gating evidence, never backend identity.
use crate::binding::{BedrockBinding, LIMITS, Profile, Wire};
use rustev_contract::bounded::parse_bounded;
use rustev_contract::canonical::{canonical_bytes, tagged_digest};
use serde::{Deserialize, Serialize};

pub const SCHEMA: &str = "rustev.bedrock-model-policy/1";
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Selector<T> {
    All(String),
    Values(Vec<T>),
}
impl<T: PartialEq> Selector<T> {
    fn valid(&self) -> bool {
        match self {
            Self::All(s) => s == "*",
            Self::Values(v) => {
                !v.is_empty()
                    && v.len() <= 3
                    && !v
                        .iter()
                        .enumerate()
                        .any(|(i, value)| v[..i].contains(value))
            }
        }
    }
    fn matches(&self, value: &T) -> bool {
        match self {
            Self::All(s) => s == "*",
            Self::Values(v) => v.contains(value),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub model: String,
    pub region: String,
    pub profile_kind: Selector<Profile>,
    pub wire: Selector<Wire>,
}
impl Rule {
    fn matches(&self, b: &BedrockBinding) -> bool {
        glob(&self.model, &b.model_id)
            && glob(&self.region, &b.region)
            && self.profile_kind.matches(&b.inference_profile)
            && self.wire.matches(&b.wire)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPolicy {
    pub schema: String,
    pub allow: Vec<Rule>,
    pub deny: Vec<Rule>,
}
impl ModelPolicy {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let p: Self = parse_bounded(bytes, &LIMITS).map_err(|e| e.to_string())?;
        p.validate()?;
        Ok(p)
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SCHEMA || self.allow.len() + self.deny.len() > 256 {
            return Err("invalid_policy".into());
        }
        for r in self.allow.iter().chain(&self.deny) {
            if r.model.len() > 256
                || r.region.len() > 256
                || !r.profile_kind.valid()
                || !r.wire.valid()
            {
                return Err("invalid_policy_rule".into());
            }
        }
        Ok(())
    }
    /// Hashes the supplied policy even when invalid, so refusals retain its identity.
    pub fn digest(&self) -> Result<String, String> {
        Ok(tagged_digest(
            SCHEMA,
            &canonical_bytes(self).map_err(|e| e.to_string())?,
        ))
    }
    pub fn evaluate(&self, b: &BedrockBinding) -> Result<(), String> {
        self.validate()?;
        let digest = self.digest()?;
        for (index, rule) in self.deny.iter().enumerate() {
            if rule.matches(b) {
                return Err(format!(
                    "policy_denied{{index:{index},rule:{}}};policy:{digest}",
                    serde_json::to_string(rule).map_err(|e| e.to_string())?
                ));
            }
        }
        if !self.allow.iter().any(|r| r.matches(b)) {
            return Err(format!(
                "policy_not_allowed{{allow_rules:{}}};policy:{digest}",
                serde_json::to_string(&self.allow).map_err(|e| e.to_string())?
            ));
        }
        Ok(())
    }
}
/// Literal Unicode strings with only '*' interpreted. Bounded by rule limits.
fn glob(pattern: &str, value: &str) -> bool {
    let mut previous = vec![false; value.len() + 1];
    previous[0] = true;
    for p in pattern.bytes() {
        let mut next = vec![false; value.len() + 1];
        if p == b'*' {
            next[0] = previous[0];
        }
        for (i, v) in value.bytes().enumerate() {
            next[i + 1] = if p == b'*' {
                previous[i + 1] || next[i]
            } else {
                previous[i] && p == v
            };
        }
        previous = next;
    }
    previous[value.len()]
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wildcard_is_case_sensitive_and_other_symbols_literal() {
        assert!(glob("eu.*", "eu.model"));
        assert!(!glob("eu.*", "EU.model"));
        assert!(!glob("a?", "ab"));
        assert!(glob("a?", "a?"));
        assert!(glob("*", ""));
    }
}
