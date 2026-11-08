//! Spec 056 3.3: validate consumer evidence before recording qualification.
use std::{collections::BTreeMap, path::Path};

use serde::{Deserialize, Serialize};

use crate::{archive::sha256_hex, family_artifacts::PLATFORMS, manifest::ReleaseManifest};

pub const SCHEMA: &str = "rustev.qualification-step/1";
pub const KINDS: &[&str] = &["crates", "npm", "cli", "image"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Status {
    Passed,
    Failed,
    NotRun { reason: String },
    Excluded { reason: String },
}

/// Spec 068 3.3.2: which npm rule a `@rustev/*` package passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NpmProvenance {
    /// The provenance rule (068 3.2) passed.
    Verified,
    /// A-74's rule applied (068 3.3.1), for `0.1.0` to `0.5.0`.
    NotRequired,
}

/// Spec 068 3.1: every release version greater than this one needs npm
/// provenance. A constant of the tool that runs, never read from a release.
pub const PROVENANCE_AFTER: (u64, u64, u64) = (0, 5, 0);

/// Spec 068 3.1: compare `X.Y.Z` numerically with [`PROVENANCE_AFTER`]. A
/// pre-release or build suffix is ignored; a version whose core does not
/// parse requires provenance, never less.
pub fn provenance_required(version: &str) -> bool {
    let core = version.split(['-', '+']).next().unwrap_or_default();
    let parts: Vec<_> = core.split('.').map(str::parse::<u64>).collect();
    match parts.as_slice() {
        [Ok(x), Ok(y), Ok(z)] => (*x, *y, *z) > PROVENANCE_AFTER,
        _ => true,
    }
}

/// One kind's consumer run on one platform. Records bind the exact manifest.
/// `lockfile_or_install_record` names the retained lockfile or install record.
/// Unexecuted steps use null exit status and may leave execution fields empty.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub schema: String,
    pub manifest_sha256: String,
    pub kind: String,
    pub platform: String,
    pub command: Vec<String>,
    pub exit_status: Option<i32>,
    pub output_digest: String,
    pub lockfile_or_install_record: String,
    pub checksums: BTreeMap<String, String>,
    pub host: String,
    pub os: String,
    pub architecture: String,
    pub toolchain: String,
    pub node: String,
    pub status: Status,
    /// Spec 068 3.3.2: the npm step's rule per installed `@rustev/*`
    /// package. Absent from the other kinds and from steps of a harness
    /// older than 068.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub npm_provenance: BTreeMap<String, NpmProvenance>,
}

pub fn required(m: &ReleaseManifest, kind: &str) -> bool {
    match kind {
        "crates" => !m.packages.is_empty(),
        "cli" => m.artifacts.iter().any(|a| a.kind == "binary"),
        "npm" => m.artifacts.iter().any(|a| a.kind == kind),
        // Spec 060 3.4.1: the image is named under `image`, never as an artifact.
        "image" => m.image.is_some(),
        _ => false,
    }
}

/// Spec 060 3.4.3: the platforms of the release matrix that qualify the
/// image, as the image platforms they run.
pub const IMAGE_PLATFORMS: &[(&str, &str)] = &[
    ("linux-x64-gnu", "linux/amd64"),
    ("linux-arm64-gnu", "linux/arm64"),
];

/// [`required`] on one platform: a named image is qualified on the
/// platforms of [`IMAGE_PLATFORMS`] only, and excluded with a reason on the
/// others.
pub fn required_on(m: &ReleaseManifest, kind: &str, platform: &str) -> bool {
    required(m, kind) && (kind != "image" || IMAGE_PLATFORMS.iter().any(|(p, _)| *p == platform))
}

/// Reuse the release set's native CLI/npm matrix from artifact collection.
/// Even portable npm packages must be consumed on each supported platform.
pub fn required_files(m: &ReleaseManifest) -> Vec<String> {
    PLATFORMS
        .iter()
        .flat_map(|platform| {
            KINDS
                .iter()
                .filter(move |kind| required_on(m, kind, platform))
                .map(move |kind| format!("{platform}.{kind}.json"))
        })
        .collect()
}

fn digest(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// A recorded checksum is in its registry's own form: a SHA-256 hex digest
/// (crates.io lockfile checksums, hosted archives) or an npm SRI
/// `sha512-<base64>` integrity string.
fn checksum(s: &str) -> bool {
    digest(s)
        || s.strip_prefix("sha512-").is_some_and(|b| {
            b.len() == 88
                && b.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'/' | b'='))
        })
}

/// All validation happens before the lifecycle is recorded.
pub fn evidence(m: &ReleaseManifest, dir: &Path) -> Result<BTreeMap<String, String>, String> {
    let mut evidence = BTreeMap::new();
    for platform in PLATFORMS {
        for kind in KINDS {
            let name = format!("{platform}.{kind}.json");
            let required = required_on(m, kind, platform);
            let bytes = match std::fs::read(dir.join(&name)) {
                Ok(b) => b,
                Err(e) if !required && e.kind() == std::io::ErrorKind::NotFound => {
                    continue;
                }
                Err(e) => return Err(format!("{name}: missing or unreadable step: {e}")),
            };
            let step: Step = serde_json::from_slice(&bytes).map_err(|e| format!("{name}: {e}"))?;
            if step.schema != SCHEMA
                || step.manifest_sha256 != m.digest()
                || step.platform != *platform
                || step.kind != *kind
            {
                return Err(format!("{name}: step identity differs from release"));
            }
            if !required {
                if !matches!(&step.status, Status::Excluded { reason } if !reason.trim().is_empty())
                {
                    return Err(format!(
                        "{name}: absent kind must be excluded with a reason"
                    ));
                }
            } else {
                if step.status != Status::Passed || step.exit_status != Some(0) {
                    return Err(format!(
                        "{name}: required step is not passed: {:?}",
                        step.status
                    ));
                }
                if step.command.is_empty()
                    || step.command.iter().any(|s| s.trim().is_empty())
                    || !digest(&step.output_digest)
                    || step.checksums.is_empty()
                    || step
                        .checksums
                        .iter()
                        .any(|(k, v)| k.trim().is_empty() || !checksum(v))
                    || [
                        &step.lockfile_or_install_record,
                        &step.host,
                        &step.os,
                        &step.architecture,
                        &step.toolchain,
                        &step.node,
                    ]
                    .iter()
                    .any(|s| s.trim().is_empty())
                {
                    return Err(format!("{name}: incomplete execution evidence"));
                }
                if *kind == "npm" && provenance_required(&m.version) {
                    npm_verified(&step).map_err(|e| format!("{name}: {e}"))?;
                }
            }
            evidence.insert(name, sha256_hex(&bytes));
        }
    }
    Ok(evidence)
}

/// Spec 068 3.3.3: after 0.5.0 an npm step qualifies only when every
/// `@rustev/*` package it lists, in its checksums or its provenance, records
/// `verified`; a step of an older harness records none and is refused.
fn npm_verified(step: &Step) -> Result<(), String> {
    let packages: std::collections::BTreeSet<&str> = step
        .checksums
        .keys()
        .chain(step.npm_provenance.keys())
        .map(String::as_str)
        .filter(|k| k.starts_with("@rustev/"))
        .collect();
    if packages.is_empty() {
        return Err("the npm step lists no @rustev/* package (068 3.3.3)".into());
    }
    match packages
        .iter()
        .find(|p| step.npm_provenance.get(**p) != Some(&NpmProvenance::Verified))
    {
        Some(p) => Err(format!("npm provenance of {p} is not verified (068 3.3.3)")),
        None => Ok(()),
    }
}
