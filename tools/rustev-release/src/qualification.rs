//! Spec 056 3.3: validate consumer evidence before recording qualification.
use std::{collections::BTreeMap, path::Path};

use serde::{Deserialize, Serialize};

use crate::{
    archive::sha256_hex,
    family_artifacts::PLATFORMS,
    family_lifecycle,
    lifecycle::{Entry, Event, Journal, Lifecycle, Lookup, State},
    manifest::{self, ReleaseManifest},
};

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

/// All validation happens before the journal is touched.
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

/// Restore released state only when the bound journal observes every hosted
/// and registry item with the expected checksum. No registry contact occurs.
pub fn restore_released(
    life: &mut Lifecycle,
    files: &BTreeMap<String, Vec<u8>>,
    journal: &Journal,
) -> Result<(), String> {
    if life.state != State::PublicationAuthorized {
        return Err("qualification requires the exact owner instruction".into());
    }
    family_lifecycle::bound(&life.manifest, journal)?;
    for item in family_lifecycle::items(&life.manifest, files)? {
        let observed = journal.entries.iter().rev().find_map(|entry| match entry {
            Entry::Query { package, observed } if *package == item.name => Some(observed),
            _ => None,
        });
        if observed
            != Some(&Lookup::Visible {
                checksum: item.checksum,
            })
        {
            return Err(format!("release is not complete: {}", item.name));
        }
    }
    // A refused transition leaves the lifecycle as it was found.
    let (observed, state) = (life.family_observed, life.state);
    life.family_observed = true;
    life.state = State::Published;
    life.apply(Event::ReleaseCreated).map(|_| ()).map_err(|e| {
        life.family_observed = observed;
        life.state = state;
        e.0
    })
}

pub fn run(
    dir: &Path,
    evidence_dir: &Path,
    instruction: &Path,
    journal: &Path,
    repo: &Path,
) -> Result<(), String> {
    let scratch = crate::repro::fresh_dir("rustev-qualification")?;
    let signers = scratch.join("allowed_signers");
    std::fs::write(&signers, manifest::ALLOWED_SIGNERS).map_err(|e| e.to_string())?;
    let errors = manifest::verify_release(dir, repo, &signers, manifest::OWNER_IDENTITY);
    if !errors.is_empty() {
        return Err(errors.join("\n"));
    }
    let m = crate::candidate::publication_manifest(
        &std::fs::read(dir.join("release-manifest.json")).map_err(|e| e.to_string())?,
    )?;
    let evidence = evidence(&m, evidence_dir)?;
    let mut life = Lifecycle::new(m.clone());
    life.apply(Event::GatesPass).map_err(|e| e.0)?;
    life.apply(Event::CandidateVerified {
        commit: m.commit.clone(),
    })
    .map_err(|e| e.0)?;
    life.authorize_file(instruction, &signers, manifest::OWNER_IDENTITY)?;
    let journal = crate::hosted::journal_path(journal, repo)?;
    let mut j =
        Journal::from_lines(&std::fs::read_to_string(&journal).map_err(|e| e.to_string())?)?;
    let mut files = BTreeMap::new();
    for name in m
        .packages
        .iter()
        .map(|p| p.crate_file.as_str())
        .chain(m.artifacts.iter().map(|a| a.name.as_str()))
        .chain([
            "release-manifest.json",
            "release-manifest.json.sig",
            "SHA256SUMS",
            "SHA256SUMS.sig",
        ])
    {
        files.insert(
            name.to_string(),
            std::fs::read(dir.join(name)).map_err(|e| format!("{name}: {e}"))?,
        );
    }
    let summary = j
        .entries
        .iter()
        .find_map(|e| match e {
            Entry::Summary { text } => Some(text.as_bytes().to_vec()),
            _ => None,
        })
        .ok_or("journal has no hosted summary")?;
    files.insert("journal-summary.jsonl".into(), summary);
    restore_released(&mut life, &files, &j)?;
    j.file = Some(journal);
    family_lifecycle::qualify(&mut life, evidence, &mut j)?;
    println!("lifecycle state: {:?}", life.state);
    Ok(())
}
