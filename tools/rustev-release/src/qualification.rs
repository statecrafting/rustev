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
}

pub fn required(m: &ReleaseManifest, kind: &str) -> bool {
    match kind {
        "crates" => !m.packages.is_empty(),
        "cli" => m.artifacts.iter().any(|a| a.kind == "binary"),
        "npm" | "image" => m.artifacts.iter().any(|a| a.kind == kind),
        _ => false,
    }
}

/// Reuse the release set's native CLI/npm matrix from artifact collection.
/// Even portable npm packages must be consumed on each supported platform.
pub fn required_files(m: &ReleaseManifest) -> Vec<String> {
    PLATFORMS
        .iter()
        .flat_map(|platform| {
            KINDS
                .iter()
                .filter(|kind| required(m, kind))
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
            let bytes = match std::fs::read(dir.join(&name)) {
                Ok(b) => b,
                Err(e) if !required(m, kind) && e.kind() == std::io::ErrorKind::NotFound => {
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
            if !required(m, kind) {
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
            }
            evidence.insert(name, sha256_hex(&bytes));
        }
    }
    Ok(evidence)
}
