//! Spec 053 3.2: candidate identities and publication boundaries.
use crate::{ReleaseSet, manifest::ReleaseManifest};

#[derive(Debug, PartialEq, Eq)]
pub struct Candidate {
    pub tag: String,
    pub version: String,
    pub n: String,
}

/// Recognize even malformed candidate attempts at a publication boundary.
pub fn is_candidate_version(value: &str) -> bool {
    value.contains("-rc.")
}

pub fn parse(tag: &str, set_version: &str) -> Result<Candidate, String> {
    let (base, n) = tag
        .strip_prefix('v')
        .and_then(|v| v.split_once("-rc."))
        // 3.2.4 is the refusal clause for a tag of any other form (as for
        // lightweight, unsigned, foreign or off-main tags); 3.2.2 is the
        // version and numbering rule cited below.
        .ok_or("053 3.2.4: expected vX.Y.Z-rc.N")?;
    if base != set_version {
        return Err("053 3.2.2: candidate version differs from release-set.json".into());
    }
    if n.is_empty() || n.starts_with('0') || !n.bytes().all(|b| b.is_ascii_digit()) {
        return Err("053 3.2.2: N must be positive with no leading zeros".into());
    }
    Ok(Candidate {
        tag: tag.into(),
        version: tag[1..].into(),
        n: n.into(),
    })
}

pub fn release_set(set: &ReleaseSet, tag: &str) -> Result<ReleaseSet, String> {
    let c = parse(tag, &set.version)?;
    let mut result = set.clone();
    result.version = c.version;
    result.tag = c.tag;
    Ok(result)
}

pub const LIMITATION: &str = "053 3.4.3: a release candidate of spec 053, not a release; no availability, support window or compatibility promise of 031 3.6.";

pub fn refuse_publication(m: &ReleaseManifest) -> Result<(), String> {
    if is_candidate_version(&m.version) || is_candidate_version(&m.tag) {
        return Err(
            "053 3.4.2: a candidate is never published to registries or as a release".into(),
        );
    }
    Ok(())
}

pub fn check_manifest(bytes: &[u8], tag: &str, commit: &str) -> Result<ReleaseManifest, String> {
    let m = ReleaseManifest::parse(bytes)?;
    let set = ReleaseSet::parse(include_bytes!("../release-set.json"))?;
    let c = parse(tag, &set.version)?;
    if m.version != c.version || m.tag != tag || m.commit != commit {
        return Err("053 3.3.1: candidate manifest version, tag or commit differs".into());
    }
    Ok(m)
}

/// Refuse candidate attempts before canonical validation or any publisher effect.
pub fn publication_manifest(bytes: &[u8]) -> Result<ReleaseManifest, String> {
    let m: ReleaseManifest = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    refuse_publication(&m)?;
    ReleaseManifest::parse(bytes)
}
