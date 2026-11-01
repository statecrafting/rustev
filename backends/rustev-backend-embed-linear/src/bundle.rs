//! Reading and verifying a host-provisioned bundle (spec 011, 3.4.1 and
//! 3.7): the only file I/O of the crate. Every byte used is checked against
//! the manifest before use; nothing is fetched, and no runtime is touched.

use std::fmt;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use rustev_contract::Document;
use rustev_contract::ids::ArtifactId;
use sha2::{Digest, Sha256};

use crate::head::LinearHead;
use crate::manifest::{self, EmbedManifest, FileRef, limits};

/// The setup refusals of 3.7.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupCode {
    BundleMissing,
    BundleTooLarge,
    ManifestInvalid,
    DigestMismatch,
    ArtifactMismatch,
    UnqualifiedPlatform,
    RuntimeMissing,
    RuntimeDigestMismatch,
    RuntimeVersionMismatch,
    RuntimeConflict,
    GraphRejected,
    GraphSignatureMismatch,
    HeadInvalid,
    CacheNotEquivalent,
    FeatureDisabled,
}

impl SetupCode {
    /// The wire spelling of the code.
    pub fn as_str(self) -> &'static str {
        match self {
            SetupCode::BundleMissing => "bundle_missing",
            SetupCode::BundleTooLarge => "bundle_too_large",
            SetupCode::ManifestInvalid => "manifest_invalid",
            SetupCode::DigestMismatch => "digest_mismatch",
            SetupCode::ArtifactMismatch => "artifact_mismatch",
            SetupCode::UnqualifiedPlatform => "unqualified_platform",
            SetupCode::RuntimeMissing => "runtime_missing",
            SetupCode::RuntimeDigestMismatch => "runtime_digest_mismatch",
            SetupCode::RuntimeVersionMismatch => "runtime_version_mismatch",
            SetupCode::RuntimeConflict => "runtime_conflict",
            SetupCode::GraphRejected => "graph_rejected",
            SetupCode::GraphSignatureMismatch => "graph_signature_mismatch",
            SetupCode::HeadInvalid => "head_invalid",
            SetupCode::CacheNotEquivalent => "cache_not_equivalent",
            SetupCode::FeatureDisabled => "feature_disabled",
        }
    }
}

/// A setup refusal naming the item at fault (3.7.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbedSetupError {
    pub code: SetupCode,
    pub item: String,
    pub detail: String,
}

impl fmt::Display for EmbedSetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}: {}", self.code.as_str(), self.item, self.detail)
    }
}

impl std::error::Error for EmbedSetupError {}

fn refuse<T>(code: SetupCode, item: &str, detail: impl Into<String>) -> Result<T, EmbedSetupError> {
    Err(EmbedSetupError {
        code,
        item: item.to_string(),
        detail: detail.into(),
    })
}

/// A bundle whose every file matched the manifest, with the bytes that were
/// verified. Later stages use these bytes and never reread the files.
#[derive(Debug, Clone)]
pub struct VerifiedBundle {
    pub manifest: EmbedManifest,
    pub artifact: ArtifactId,
    pub graph: Vec<u8>,
    pub tokenizer: Vec<u8>,
    /// One per manifest head, in manifest order.
    pub heads: Vec<LinearHead>,
}

/// Read and verify the bundle at the absolute path `dir` (3.7.1): the
/// manifest, every named file's length and SHA-256, the identity against
/// `expected`, and the head weights.
pub fn verify_bundle(dir: &Path, expected: &ArtifactId) -> Result<VerifiedBundle, EmbedSetupError> {
    use SetupCode as C;
    if !dir.is_absolute() {
        return refuse(C::BundleMissing, "bundle_dir", "not an absolute path");
    }
    match fs::symlink_metadata(dir) {
        Ok(md) if md.is_dir() => {}
        Ok(_) => return refuse(C::BundleMissing, "bundle_dir", "not a directory"),
        Err(e) => return refuse(C::BundleMissing, "bundle_dir", e.to_string()),
    }

    let manifest_bytes = read_regular(
        dir,
        "manifest.json",
        manifest::EmbedManifest::LIMITS.max_bytes as u64,
    )?;
    let manifest = EmbedManifest::parse(&manifest_bytes)
        .map_err(manifest::ManifestError::from)
        .and_then(|m| manifest::check(&m).map(|()| m))
        .or_else(|e| refuse(C::ManifestInvalid, "manifest.json", e.to_string()))?;

    let (files, declared) = manifest::distinct_files(&manifest);
    if declared.saturating_add(manifest_bytes.len() as u64) > limits::BUNDLE_BYTES {
        return refuse(
            C::BundleTooLarge,
            "manifest.json",
            format!("declares {declared} bytes; the bundle bound is 150 MiB"),
        );
    }
    let mut verified: Vec<(FileRef, Vec<u8>)> = Vec::with_capacity(files.len());
    for f in files {
        let bytes = read_regular(dir, &f.file, f.bytes)?;
        if bytes.len() as u64 != f.bytes {
            return refuse(
                C::DigestMismatch,
                &f.file,
                format!("{} bytes, manifest declares {}", bytes.len(), f.bytes),
            );
        }
        if sha256_hex(&bytes) != f.sha256 {
            return refuse(
                C::DigestMismatch,
                &f.file,
                "sha256 differs from the manifest",
            );
        }
        verified.push((f, bytes));
    }

    let artifact = manifest
        .artifact_id()
        .or_else(|e| refuse(C::ManifestInvalid, "manifest.json", e.to_string()))?;
    if artifact != *expected {
        return refuse(
            C::ArtifactMismatch,
            "expected_artifact",
            format!("the bundle is {artifact}"),
        );
    }

    let bytes_of = |name: &str| -> Vec<u8> {
        verified
            .iter()
            .find(|(f, _)| f.file == name)
            .map(|(_, b)| b.clone())
            .unwrap_or_default()
    };
    let hidden = manifest.encoder.hidden_size as usize;
    let heads = manifest
        .heads
        .iter()
        .enumerate()
        .map(|(i, h)| {
            LinearHead::from_bytes(&bytes_of(&h.weights.file), h.options.len(), hidden)
                .or_else(|why| refuse(C::HeadInvalid, &format!("heads[{i}]"), why))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(VerifiedBundle {
        graph: bytes_of(&manifest.encoder.graph.file),
        tokenizer: bytes_of(&manifest.tokenizer.file),
        heads,
        artifact,
        manifest,
    })
}

/// Read a regular file under `dir`, refusing a symbolic link anywhere on
/// the relative path, and reading at most `max + 1` bytes so a file over
/// its declared length (`bundle_too_large`) is detected without reading it
/// whole.
fn read_regular(dir: &Path, rel: &str, max: u64) -> Result<Vec<u8>, EmbedSetupError> {
    use SetupCode as C;
    let mut path = PathBuf::from(dir);
    for part in Path::new(rel).components() {
        let Component::Normal(part) = part else {
            return refuse(C::ManifestInvalid, rel, "not a bundle-relative path");
        };
        path.push(part);
        match fs::symlink_metadata(&path) {
            Ok(md) if md.file_type().is_symlink() => {
                return refuse(C::BundleMissing, rel, "a symbolic link");
            }
            Ok(_) => {}
            Err(e) => return refuse(C::BundleMissing, rel, e.to_string()),
        }
    }
    let file = fs::File::open(&path).or_else(|e| refuse(C::BundleMissing, rel, e.to_string()))?;
    let md = file
        .metadata()
        .or_else(|e| refuse(C::BundleMissing, rel, e.to_string()))?;
    if !md.is_file() {
        return refuse(C::BundleMissing, rel, "not a regular file");
    }
    // The opened file must be the one now at the in-bundle path: its
    // canonical path is the bundle's own, with no link anywhere, and on
    // Unix the open handle and that path are the same inode. A path the
    // host lets another party rewrite between these checks and the read is
    // still bounded by the SHA-256 check that follows: only the declared
    // bytes are ever used (3.6.5, Decision 7).
    let canonical_dir =
        fs::canonicalize(dir).or_else(|e| refuse(C::BundleMissing, rel, e.to_string()))?;
    let canonical =
        fs::canonicalize(&path).or_else(|e| refuse(C::BundleMissing, rel, e.to_string()))?;
    if canonical != canonical_dir.join(rel) {
        return refuse(C::BundleMissing, rel, "resolves outside its bundle path");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let now = fs::symlink_metadata(&path)
            .or_else(|e| refuse(C::BundleMissing, rel, e.to_string()))?;
        if now.file_type().is_symlink() || (now.dev(), now.ino()) != (md.dev(), md.ino()) {
            return refuse(C::BundleMissing, rel, "changed while it was opened");
        }
    }
    let mut bytes = Vec::new();
    file.take(max.saturating_add(1))
        .read_to_end(&mut bytes)
        .or_else(|e| refuse(C::BundleMissing, rel, e.to_string()))?;
    if bytes.len() as u64 > max {
        return refuse(C::BundleTooLarge, rel, format!("larger than {max} bytes"));
    }
    Ok(bytes)
}

/// Lowercase hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
