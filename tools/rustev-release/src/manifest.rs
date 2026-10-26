//! Signed release identity and artifacts (spec 021, 3.8).
//!
//! A release identity is one exact commit, signed tag, artifact manifest and
//! checksum set (I-3). [`ReleaseManifest`] is the canonical manifest: its
//! bytes are compact JSON in field order, so the same inputs give the same
//! bytes and digest. [`verify_release`] checks every binding: the manifest
//! and `SHA256SUMS` signatures under the documented owner identity, the
//! archives against both, and the signed tag against the manifest's commit,
//! tree, digest and package set. Substituting any one member fails.
//!
//! Signatures are SSH signatures (`ssh-keygen -Y`), the owner's Git signing
//! format; file signatures use the namespace [`NAMESPACE`]. Nothing here
//! reads a private key: signing is the owner's act (3.9).

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::ReleaseSet;
use crate::archive::sha256_hex;

// Spec 031 3.11.2: no /1 manifest was ever signed or published; no /1 reader remains.
pub const MANIFEST_SCHEMA: &str = "rustev.release-manifest/2";
/// The SSH signature namespace of the manifest and `SHA256SUMS`.
pub const NAMESPACE: &str = "rustev-release";
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
    /// expected to record; the observed checksum belongs to the journal.
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

/// The annotated tag message: it names the manifest digest and the set.
pub fn tag_message(m: &ReleaseManifest) -> String {
    format!(
        "rustev {}\n\nrelease-manifest-sha256: {}\npackages: {}\n",
        m.version,
        m.digest(),
        m.upload_order.join(" ")
    )
}

/// Verify `file` against its detached SSH signature `sig` for `identity`
/// under `allowed_signers`.
pub fn ssh_verify(
    file: &[u8],
    sig: &Path,
    allowed_signers: &Path,
    identity: &str,
) -> Result<(), String> {
    use std::io::Write;
    let mut child = Command::new("ssh-keygen")
        .args(["-Y", "verify", "-f"])
        .arg(allowed_signers)
        .args(["-I", identity, "-n", NAMESPACE, "-s"])
        .arg(sig)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not run ssh-keygen: {e}"))?;
    child
        .stdin
        .take()
        .ok_or("no stdin")?
        .write_all(file)
        .map_err(|e| e.to_string())?;
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "signature {} does not verify for {identity}: {}",
            sig.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

fn git(repo: &Path, args: &[&str], allowed_signers: &Path) -> Result<String, String> {
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

fn names_are_plain(m: &ReleaseManifest) -> Result<(), String> {
    // Object ids, not revisions: a name such as `HEAD` never reaches Git.
    let object_id = |s: &str| {
        (s.len() == 40 || s.len() == 64)
            && s.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    };
    if !object_id(&m.commit) || !object_id(&m.tree) {
        return Err(format!(
            "commit {:?} and tree {:?} must be full object ids",
            m.commit, m.tree
        ));
    }
    let word = |s: &str| {
        !s.is_empty()
            && !s.starts_with(['-', '.'])
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    };
    if !word(&m.version) || m.tag != format!("v{}", m.version) {
        return Err(format!("tag {:?} is not exactly v<version>", m.tag));
    }
    for p in &m.packages {
        if !word(&p.name) || p.crate_file != format!("{}-{}.crate", p.name, m.version) {
            return Err(format!(
                "archive name {:?} is not <member>-<version>.crate",
                p.crate_file
            ));
        }
    }
    Ok(())
}

/// Every binding of 3.8 for the release in `dir` (holding
/// `release-manifest.json`, `release-manifest.json.sig`, `SHA256SUMS`,
/// `SHA256SUMS.sig` and the `.crate` archives) against the tag in `repo`.
/// Empty is the passing verdict.
pub fn verify_release(
    dir: &Path,
    repo: &Path,
    allowed_signers: &Path,
    identity: &str,
) -> Vec<String> {
    let mut v = Vec::new();
    let read = |name: &str| std::fs::read(dir.join(name)).map_err(|e| format!("{name}: {e}"));
    let manifest_bytes = match read("release-manifest.json") {
        Ok(b) => b,
        Err(e) => return vec![e],
    };
    let m = match ReleaseManifest::parse(&manifest_bytes) {
        Ok(m) => m,
        Err(e) => return vec![format!("021 3.8: {e}")],
    };
    // Every name the manifest supplies is checked before it reaches a path
    // or a Git argument: the tag is exactly `v<version>` and each archive
    // is `<member>-<version>.crate`.
    if let Err(e) = names_are_plain(&m) {
        return vec![format!("021 3.8: {e}")];
    }
    if let Err(e) = ssh_verify(
        &manifest_bytes,
        &dir.join("release-manifest.json.sig"),
        allowed_signers,
        identity,
    ) {
        v.push(format!("021 3.8: manifest {e}"));
    }
    match read("SHA256SUMS") {
        Err(e) => v.push(e),
        Ok(sums) => {
            if let Err(e) = ssh_verify(
                &sums,
                &dir.join("SHA256SUMS.sig"),
                allowed_signers,
                identity,
            ) {
                v.push(format!("021 3.8: SHA256SUMS {e}"));
            }
            if sums != sha256sums(&m).into_bytes() {
                v.push("021 3.8: SHA256SUMS does not list exactly the manifest's archives".into());
            }
        }
    }
    for p in &m.packages {
        match read(&p.crate_file) {
            Err(e) => v.push(e),
            Ok(bytes) => {
                if sha256_hex(&bytes) != p.sha256 || bytes.len() as u64 != p.bytes {
                    v.push(format!(
                        "021 3.8: {} is not the archive the manifest names",
                        p.crate_file
                    ));
                }
            }
        }
    }
    for a in &m.artifacts {
        match read(&a.name) {
            Ok(bytes) if bytes.len() as u64 == a.bytes && sha256_hex(&bytes) == a.sha256 => {}
            _ => v.push(format!("031 3.11.2: {} missing or differs", a.name)),
        }
    }
    let tag_ref = format!("refs/tags/{}", m.tag);
    match git(repo, &["cat-file", "-t", &tag_ref], allowed_signers) {
        Ok(t) if t == "tag" => {}
        // A lightweight tag carries no signature to verify, so this is the
        // one finding about it rather than a second from `verify-tag`.
        Ok(t) => {
            v.push(format!("021 3.8: {} is a {t}, not an annotated tag", m.tag));
            return v;
        }
        Err(e) => {
            v.push(format!("021 3.8: {e}"));
            return v;
        }
    }
    if let Err(e) = git(repo, &["verify-tag", &tag_ref], allowed_signers) {
        v.push(format!("021 3.8: the tag signature does not verify: {e}"));
    }
    match git(
        repo,
        &["rev-parse", &format!("{tag_ref}^{{commit}}")],
        allowed_signers,
    ) {
        Ok(c) if c == m.commit => {}
        Ok(c) => v.push(format!(
            "021 3.8: {} points at {c}, the manifest names {}",
            m.tag, m.commit
        )),
        Err(e) => v.push(e),
    }
    match git(
        repo,
        &["rev-parse", &format!("{}^{{tree}}", m.commit)],
        allowed_signers,
    ) {
        Ok(t) if t == m.tree => {}
        Ok(t) => v.push(format!(
            "021 3.8: commit {} has tree {t}, the manifest names {}",
            m.commit, m.tree
        )),
        Err(e) => v.push(e),
    }
    match git(
        repo,
        &[
            "tag",
            "-l",
            "--format=%(contents:subject)%0a%0a%(contents:body)",
            &m.tag,
        ],
        allowed_signers,
    ) {
        Ok(msg) => {
            let want = tag_message(&m);
            if msg.trim_end() != want.trim_end() {
                v.push(format!(
                    "021 3.8: the tag message does not name this manifest's digest and set:\n{msg}"
                ));
            }
        }
        Err(e) => v.push(e),
    }
    v
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
