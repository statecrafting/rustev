//! The release identity and its artifacts (spec 021, 3.8, as amended by
//! 058 3.2.4).
//!
//! [`ReleaseManifest`] is the canonical manifest: its bytes are compact JSON
//! in field order, so the same inputs give the same bytes and digest. A
//! tagged run writes it, with `SHA256SUMS`, from the artifacts it built and
//! attests both; neither is signed by the owner nor read as an input. The
//! owner's signature is on the tag, which `tagged::check_tag` verifies
//! against the built-in [`ALLOWED_SIGNERS`]. Nothing here
//! reads a private key.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::ReleaseSet;
use crate::archive::sha256_hex;

// Spec 031 3.11.2: no /1 manifest was ever signed or published; no /1 reader remains.
pub const MANIFEST_SCHEMA: &str = "rustev.release-manifest/2";
/// The documented owner signing identity, as `allowed_signers` names it.
pub const OWNER_IDENTITY: &str = "7887446+bartekus@users.noreply.github.com";
/// The documented owner's `allowed_signers` record, built into the tool so
/// verification never depends on where the source tree is.
pub const ALLOWED_SIGNERS: &str = include_str!("../allowed_signers");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseManifest {
    pub schema: String,
    pub version: String,
    pub tag: String,
    /// Spec 058 3.2.4: the object id of the annotated tag a tagged run
    /// built from; absent in a manifest made before spec 058.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag_object: Option<String>,
    pub commit: String,
    pub tree: String,
    pub registry: String,
    pub layers: Vec<Vec<String>>,
    pub upload_order: Vec<String>,
    pub packages: Vec<PackageArtifact>,
    pub artifacts: Vec<WorkflowArtifact>,
    pub registries: Vec<String>,
    pub plan_ids: BTreeMap<String, String>,
    pub plan_ids_changed: PlanChanges,
    pub msrv: String,
    pub edition: String,
    pub toolchain: Toolchain,
    pub source_date_epoch: u64,
    /// SHA-256 of each evidence file of the candidate run, by file name.
    pub evidence: BTreeMap<String, String>,
    pub limitations: Vec<String>,
}

/// Spec 031 3.7: a first release has no comparison baseline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PlanChanges {
    First(String),
    Changed(Vec<String>),
}

/// Spec 031 3.1 and 3.9.5: workflow artifacts bound before the tag build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowArtifact {
    pub kind: String,
    pub name: String,
    pub layer: u8,
    pub bytes: u64,
    pub sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub integrity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sbom: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slsa_build_level: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageArtifact {
    pub name: String,
    pub crate_file: String,
    pub bytes: u64,
    /// The archive SHA-256, which is also the checksum the registry is
    /// expected to record; the observed checksum belongs to the
    /// publication record.
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Toolchain {
    pub rustc: String,
    pub cargo: String,
    pub host: String,
    pub audit: String,
}

impl ReleaseManifest {
    /// The canonical bytes: compact JSON in field order, map keys sorted.
    pub fn canonical(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("a manifest always serializes")
    }

    pub fn digest(&self) -> String {
        sha256_hex(&self.canonical())
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let m: Self =
            serde_json::from_slice(bytes).map_err(|e| format!("unreadable manifest: {e}"))?;
        if m.schema != MANIFEST_SCHEMA {
            return Err(format!("schema {:?} is not {MANIFEST_SCHEMA:?}", m.schema));
        }
        if m.canonical() != bytes {
            return Err("the manifest is not in canonical form".into());
        }
        crate::family_artifacts::validate(&m)?;
        Ok(m)
    }
}

/// What a manifest records beyond the release set and the archives.
#[derive(Debug, Clone)]
pub struct Inputs {
    pub commit: String,
    pub tree: String,
    pub toolchain: Toolchain,
    pub source_date_epoch: u64,
    pub evidence: BTreeMap<String, String>,
    pub limitations: Vec<String>,
}

/// Build the manifest of `set` from the archives in `crates`.
pub fn build(set: &ReleaseSet, crates: &Path, inputs: Inputs) -> Result<ReleaseManifest, String> {
    let mut packages = Vec::new();
    for name in set
        .upload_order
        .iter()
        .filter(|_| !crate::candidate::is_candidate_version(&set.version))
    {
        let crate_file = format!("{name}-{}.crate", set.version);
        let bytes =
            std::fs::read(crates.join(&crate_file)).map_err(|e| format!("{crate_file}: {e}"))?;
        packages.push(PackageArtifact {
            name: name.clone(),
            crate_file,
            bytes: bytes.len() as u64,
            sha256: sha256_hex(&bytes),
        });
    }
    Ok(ReleaseManifest {
        schema: MANIFEST_SCHEMA.into(),
        version: set.version.clone(),
        tag: set.tag.clone(),
        tag_object: None,
        commit: inputs.commit,
        tree: inputs.tree,
        registry: set.registry.clone(),
        layers: set.layers.clone(),
        upload_order: set.upload_order.clone(),
        packages,
        artifacts: Vec::new(),
        registries: vec![
            set.registry.clone(),
            "https://registry.npmjs.org".into(),
            crate::REPOSITORY.to_string() + "/releases",
            "ghcr.io/statecrafting".into(),
        ],
        plan_ids: BTreeMap::new(),
        plan_ids_changed: PlanChanges::First("none".into()),
        msrv: String::new(),
        edition: String::new(),
        toolchain: inputs.toolchain,
        source_date_epoch: inputs.source_date_epoch,
        evidence: inputs.evidence,
        limitations: inputs.limitations,
    })
}

/// `SHA256SUMS`: one `<sha256>  <file>` line per archive, in upload order.
pub fn sha256sums(m: &ReleaseManifest) -> String {
    m.packages
        .iter()
        .map(|p| format!("{}  {}\n", p.sha256, p.crate_file))
        .chain(
            m.artifacts
                .iter()
                .map(|a| format!("{}  {}\n", a.sha256, a.name)),
        )
        .collect()
}

pub(crate) fn git(repo: &Path, args: &[&str], allowed_signers: &Path) -> Result<String, String> {
    // The signers file goes through Git's environment configuration, so no
    // character of its path can be read as part of a `-c key=value`.
    let mut cmd = Command::new("git");
    // No inherited `GIT_*` variable (such as `GIT_DIR`) may redirect these
    // checks away from `repo`.
    for (k, _) in std::env::vars_os() {
        if k.to_string_lossy().starts_with("GIT_") {
            cmd.env_remove(&k);
        }
    }
    let out = cmd
        .arg("-C")
        .arg(repo)
        // SSH signatures only, verified against the reviewed signers file,
        // whatever the repository's or user's configuration says.
        .env("GIT_CONFIG_COUNT", "2")
        .env("GIT_CONFIG_KEY_0", "gpg.ssh.allowedSignersFile")
        .env("GIT_CONFIG_VALUE_0", allowed_signers)
        .env("GIT_CONFIG_KEY_1", "gpg.format")
        .env("GIT_CONFIG_VALUE_1", "ssh")
        .args(args)
        .output()
        .map_err(|e| format!("could not run git: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Validate before writing either file; the canonical manifest is renamed last.
pub fn write_files(m: &ReleaseManifest, dir: &Path) -> Result<(), String> {
    crate::family_artifacts::validate(m)?;
    for (name, bytes) in [
        ("SHA256SUMS", sha256sums(m).into_bytes()),
        ("release-manifest.json", m.canonical()),
    ] {
        std::fs::write(dir.join(format!("{name}.partial")), bytes).map_err(|e| e.to_string())?;
    }
    for name in ["SHA256SUMS", "release-manifest.json"] {
        std::fs::rename(dir.join(format!("{name}.partial")), dir.join(name))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
