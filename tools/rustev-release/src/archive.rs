//! Registry contents (spec 021, 3.5): the reviewed file list of each
//! archive, its byte bounds, and the prohibited-content scan over the exact
//! unpacked files.
//!
//! A scan pass is evidence only for [`SCAN_RULESET`] (3.5): it proves no
//! listed pattern occurs, not that no secret can exist.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const ARCHIVE_POLICY_SCHEMA: &str = "rustev.archive-policy/1";
/// The identity of the prohibited-content rules below. Changing a rule
/// changes this name.
pub const SCAN_RULESET: &str = "rustev.archive-scan/1";

/// The reviewed archive policy (`archive-policy.json`).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchivePolicy {
    pub schema: String,
    pub scan_ruleset: String,
    /// Files Cargo writes into every archive rather than copying them from
    /// the source tree; listed so the evidence labels them generated.
    /// `Cargo.toml.orig` is not among them: it is the authored manifest,
    /// which Cargo copies verbatim under that name.
    pub generated: Vec<String>,
    pub max_file_bytes: u64,
    /// Exactly one entry per release-set member. The exclusions of
    /// `release-set.json` (`rustev-pkg-lodging` among them, 021 section 2)
    /// are never packaged, so they have none and their manifests stay
    /// `publish = false` without an `include`; `tests/archives.rs` pins the
    /// key set to the members.
    pub packages: BTreeMap<String, PackagePolicy>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackagePolicy {
    pub files: Vec<String>,
    pub max_archive_bytes: u64,
    pub max_unpacked_bytes: u64,
}

impl ArchivePolicy {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        serde_json::from_slice(bytes).map_err(|e| format!("unreadable archive policy: {e}"))
    }
}

/// One file of an unpacked archive, by its path inside the package root.
#[derive(Debug, Clone)]
pub struct UnpackedFile {
    pub path: String,
    pub bytes: Vec<u8>,
}

/// A prohibited pattern found in a file.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Finding {
    pub rule: &'static str,
    pub offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ArchiveViolation {
    /// The policy has no entry for the package, or is malformed.
    Policy { detail: String },
    /// A file is in the archive but not in the reviewed list.
    Unexpected { package: String, path: String },
    /// A reviewed file is missing from the archive.
    Missing { package: String, path: String },
    /// A file, the archive or the unpacked total exceeds its bound.
    Bound {
        package: String,
        what: String,
        bytes: u64,
        limit: u64,
    },
    /// A path names a prohibited file or directory.
    ProhibitedPath { package: String, path: String },
    /// A file's content matches a prohibited pattern.
    ProhibitedContent {
        package: String,
        path: String,
        finding: Finding,
    },
}

impl fmt::Display for ArchiveViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArchiveViolation::Policy { detail } => write!(f, "021 3.5: archive policy: {detail}"),
            ArchiveViolation::Unexpected { package, path } => {
                write!(f, "021 3.5: `{package}` archive holds unreviewed `{path}`")
            }
            ArchiveViolation::Missing { package, path } => {
                write!(f, "021 3.5: `{package}` archive lacks reviewed `{path}`")
            }
            ArchiveViolation::Bound {
                package,
                what,
                bytes,
                limit,
            } => write!(
                f,
                "021 3.5: `{package}` {what} is {bytes} bytes, over the reviewed bound {limit}"
            ),
            ArchiveViolation::ProhibitedPath { package, path } => {
                write!(f, "021 3.5: `{package}` archive holds prohibited `{path}`")
            }
            ArchiveViolation::ProhibitedContent {
                package,
                path,
                finding,
            } => write!(
                f,
                "021 3.5: `{package}` `{path}` matches {} at byte {} ({SCAN_RULESET})",
                finding.rule, finding.offset
            ),
        }
    }
}

/// Path components and names an archive never holds (3.5).
const PROHIBITED_COMPONENTS: &[&str] = &[
    ".git",
    ".statecraft",
    ".tooling",
    ".bin",
    "target",
    "evidence",
    "scratch",
    "node_modules",
    ".idea",
    ".vscode",
    ".DS_Store",
    ".npmrc",
    ".netrc",
    "credentials",
    "credentials.toml",
    "id_rsa",
    "id_ed25519",
    ".env",
    ".envrc",
];
/// Component prefixes an archive never holds (environment variants).
const PROHIBITED_PREFIXES: &[&str] = &[".env."];
/// Component suffixes an archive never holds (editor state, key material).
const PROHIBITED_SUFFIXES: &[&str] = &[".swp", ".pem", ".key", ".p12"];

fn prohibited_path(path: &str) -> bool {
    path.split('/').any(|c| {
        PROHIBITED_COMPONENTS.contains(&c)
            || PROHIBITED_PREFIXES.iter().any(|p| c.starts_with(p))
            || PROHIBITED_SUFFIXES.iter().any(|x| c.ends_with(x))
    })
}

fn run_len(bytes: &[u8], from: usize, ok: impl Fn(u8) -> bool) -> usize {
    bytes[from.min(bytes.len())..]
        .iter()
        .take_while(|b| ok(**b))
        .count()
}

fn alnum(b: u8) -> bool {
    b.is_ascii_alphanumeric()
}

/// A token character: what a token prefix is followed by, and what, before
/// a prefix, makes it part of a longer identifier rather than a token start.
fn token_char(b: u8) -> bool {
    alnum(b) || b == b'_' || b == b'-'
}

/// Token prefixes and the minimum run of token characters after them.
const TOKEN_RULES: &[(&str, &str, usize)] = &[
    ("github-token", "ghp_", 36),
    ("github-token", "gho_", 36),
    ("github-token", "ghu_", 36),
    ("github-token", "ghs_", 36),
    ("github-token", "ghr_", 36),
    ("github-token", "github_pat_", 22),
    ("npm-token", "npm_", 36),
    ("crates-io-token", "cio", 32),
    ("anthropic-key", "sk-ant-", 20),
    ("openai-key", "sk-proj-", 20),
    ("google-api-key", "AIza", 35),
    ("slack-token", "xoxb-", 10),
    ("slack-token", "xoxp-", 10),
    ("slack-token", "xoxa-", 10),
    ("slack-token", "xoxr-", 10),
    ("slack-token", "xoxs-", 10),
];

/// Absolute local path prefixes that would leak a build machine (3.5).
const LOCAL_PATHS: &[&str] = &["/Users/", "/home/", "C:\\Users\\", "/private/var/folders/"];

/// Every finding of [`SCAN_RULESET`] in `bytes`, in offset order.
pub fn scan(bytes: &[u8]) -> Vec<Finding> {
    let mut found = Vec::new();
    for i in 0..bytes.len() {
        let rest = &bytes[i..];
        if rest.starts_with(b"-----BEGIN ") {
            // The whole header line, however long its type name.
            let line = rest.split(|b| *b == b'\n').next().unwrap_or(rest);
            if line.windows(11).any(|w| w == b"PRIVATE KEY") {
                found.push(Finding {
                    rule: "private-key",
                    offset: i,
                });
            }
        }
        if (rest.starts_with(b"AKIA") || rest.starts_with(b"ASIA"))
            && (i == 0 || !token_char(bytes[i - 1]))
            && run_len(bytes, i + 4, |b| {
                b.is_ascii_uppercase() || b.is_ascii_digit()
            }) >= 16
        {
            found.push(Finding {
                rule: "aws-access-key",
                offset: i,
            });
        }
        for (rule, prefix, min) in TOKEN_RULES {
            if rest.starts_with(prefix.as_bytes())
                && (i == 0 || !token_char(bytes[i - 1]))
                && run_len(bytes, i + prefix.len(), token_char) >= *min
            {
                found.push(Finding { rule, offset: i });
            }
        }
        for p in LOCAL_PATHS {
            if rest.starts_with(p.as_bytes()) && run_len(bytes, i + p.len(), alnum) > 0 {
                found.push(Finding {
                    rule: "local-path",
                    offset: i,
                });
            }
        }
    }
    found
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Judge one package's unpacked archive. `files` are relative to the
/// package root (`<name>-<version>/`).
pub fn check_archive(
    policy: &ArchivePolicy,
    package: &str,
    archive_bytes: u64,
    files: &[UnpackedFile],
) -> Vec<ArchiveViolation> {
    let mut v = Vec::new();
    // An unrecognized policy is not evaluated: its lists and bounds would
    // be read under rules they were not written for.
    if policy.schema != ARCHIVE_POLICY_SCHEMA || policy.scan_ruleset != SCAN_RULESET {
        v.push(ArchiveViolation::Policy {
            detail: format!(
                "schema {:?} and ruleset {:?} are not {ARCHIVE_POLICY_SCHEMA:?} and {SCAN_RULESET:?}",
                policy.schema, policy.scan_ruleset
            ),
        });
        return v;
    }
    let Some(p) = policy.packages.get(package) else {
        v.push(ArchiveViolation::Policy {
            detail: format!("no entry for `{package}`"),
        });
        return v;
    };
    let name = || package.to_string();
    let present: BTreeMap<&str, &UnpackedFile> =
        files.iter().map(|f| (f.path.as_str(), f)).collect();
    for f in files {
        if !p.files.contains(&f.path) {
            v.push(ArchiveViolation::Unexpected {
                package: name(),
                path: f.path.clone(),
            });
        }
        if prohibited_path(&f.path) {
            v.push(ArchiveViolation::ProhibitedPath {
                package: name(),
                path: f.path.clone(),
            });
        }
        let len = f.bytes.len() as u64;
        if len > policy.max_file_bytes {
            v.push(ArchiveViolation::Bound {
                package: name(),
                what: format!("file `{}`", f.path),
                bytes: len,
                limit: policy.max_file_bytes,
            });
        }
        for finding in scan(&f.bytes) {
            v.push(ArchiveViolation::ProhibitedContent {
                package: name(),
                path: f.path.clone(),
                finding,
            });
        }
    }
    for path in &p.files {
        if !present.contains_key(path.as_str()) {
            v.push(ArchiveViolation::Missing {
                package: name(),
                path: path.clone(),
            });
        }
    }
    if archive_bytes > p.max_archive_bytes {
        v.push(ArchiveViolation::Bound {
            package: name(),
            what: "the archive".into(),
            bytes: archive_bytes,
            limit: p.max_archive_bytes,
        });
    }
    let unpacked: u64 = files.iter().map(|f| f.bytes.len() as u64).sum();
    if unpacked > p.max_unpacked_bytes {
        v.push(ArchiveViolation::Bound {
            package: name(),
            what: "the unpacked total".into(),
            bytes: unpacked,
            limit: p.max_unpacked_bytes,
        });
    }
    v.sort();
    v
}

/// The evidence of one archive: what was measured, at which digest.
#[derive(Debug, Clone, Serialize)]
pub struct ArchiveEvidence {
    pub package: String,
    pub version: String,
    pub crate_file: String,
    pub sha256: String,
    pub archive_bytes: u64,
    pub unpacked_bytes: u64,
    pub scan_ruleset: &'static str,
    pub files: Vec<FileEvidence>,
    pub violations: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileEvidence {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
    pub generated: bool,
}

pub fn evidence(
    policy: &ArchivePolicy,
    package: &str,
    version: &str,
    archive: &[u8],
    files: &[UnpackedFile],
) -> ArchiveEvidence {
    let mut listed: Vec<FileEvidence> = files
        .iter()
        .map(|f| FileEvidence {
            path: f.path.clone(),
            bytes: f.bytes.len() as u64,
            sha256: sha256_hex(&f.bytes),
            generated: policy.generated.contains(&f.path),
        })
        .collect();
    listed.sort_by(|a, b| a.path.cmp(&b.path));
    ArchiveEvidence {
        package: package.to_string(),
        version: version.to_string(),
        crate_file: format!("{package}-{version}.crate"),
        sha256: sha256_hex(archive),
        archive_bytes: archive.len() as u64,
        unpacked_bytes: files.iter().map(|f| f.bytes.len() as u64).sum(),
        scan_ruleset: SCAN_RULESET,
        files: listed,
        violations: check_archive(policy, package, archive.len() as u64, files)
            .iter()
            .map(ToString::to_string)
            .collect(),
    }
}

fn tar_list(archive: &std::path::Path, verbose: bool) -> Result<String, String> {
    let out = std::process::Command::new("tar")
        .arg(if verbose { "-tvzf" } else { "-tzf" })
        .arg(archive)
        .output()
        .map_err(|e| format!("could not run tar: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "tar refused {}: {}",
            archive.display(),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Refuse an archive before anything is written to disk when an entry is
/// absolute, climbs with `..`, or is anything but a regular file or a
/// directory (a link could point outside the extraction directory).
pub fn check_entries(archive: &std::path::Path) -> Result<(), String> {
    for name in tar_list(archive, false)?.lines() {
        if name.starts_with('/') || name.contains('\\') || name.split('/').any(|c| c == "..") {
            return Err(format!(
                "{} holds the unsafe entry {name:?}",
                archive.display()
            ));
        }
    }
    for line in tar_list(archive, true)?.lines() {
        // Both GNU tar and bsdtar give the entry type as the first letter
        // of a verbose line (`l` symlink, `h` hard link, as `tar -tvzf`
        // prints them), so only `-` (file) and `d` (directory) pass; a file
        // name is never inspected.
        if line.trim().is_empty() {
            continue;
        }
        if !(line.starts_with('-') || line.starts_with('d')) {
            return Err(format!(
                "{} holds a non-file entry: {line}",
                archive.display()
            ));
        }
    }
    Ok(())
}

/// Unpack a `.crate` (a gzip tar whose entries sit under `<name>-<version>/`)
/// into `into` with the system `tar`, and read every file back relative to
/// the package root.
pub fn unpack(
    archive: &std::path::Path,
    into: &std::path::Path,
    root: &str,
) -> Result<Vec<UnpackedFile>, String> {
    let bytes = std::fs::read(archive).map_err(|e| format!("{}: {e}", archive.display()))?;
    unpack_bytes(&bytes, archive, into, root)
}

/// [`unpack`] over archive bytes already read: checking and extraction both
/// use a private copy of exactly these bytes, so what is hashed, checked,
/// extracted and scanned is one archive.
pub fn unpack_bytes(
    bytes: &[u8],
    archive: &std::path::Path,
    into: &std::path::Path,
    root: &str,
) -> Result<Vec<UnpackedFile>, String> {
    let private = std::env::temp_dir().join(format!(
        "rustev-release-copy-{}-{}",
        std::process::id(),
        COPIES.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&private);
    std::fs::create_dir_all(&private).map_err(|e| format!("{}: {e}", private.display()))?;
    let copy = private.join("archive.crate");
    let result = std::fs::write(&copy, bytes)
        .map_err(|e| format!("{}: {e}", copy.display()))
        .and_then(|()| extract(&copy, archive, into, root));
    let _ = std::fs::remove_dir_all(&private);
    result
}

static COPIES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn extract(
    copy: &std::path::Path,
    archive: &std::path::Path,
    into: &std::path::Path,
    root: &str,
) -> Result<Vec<UnpackedFile>, String> {
    check_entries(copy)
        .map_err(|e| e.replace(&copy.display().to_string(), &archive.display().to_string()))?;
    std::fs::create_dir_all(into).map_err(|e| format!("{}: {e}", into.display()))?;
    let out = std::process::Command::new("tar")
        .arg("-xzf")
        .arg(copy)
        .arg("-C")
        .arg(into)
        .output()
        .map_err(|e| format!("could not run tar: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "tar refused {}: {}",
            archive.display(),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let mut files = walk(&into.join(root), "")?;
    // Anything beside the package root is itself a violation of 3.5; its
    // files keep their content so the scan sees them too.
    for entry in std::fs::read_dir(into).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == root {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            files.extend(walk(&path, &format!("../{name}/"))?);
        } else {
            let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            files.push(UnpackedFile {
                path: format!("../{name}"),
                bytes,
            });
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// Every regular file under `base`, its path relative to `base` after
/// `prefix`.
fn walk(base: &std::path::Path, prefix: &str) -> Result<Vec<UnpackedFile>, String> {
    let mut files = Vec::new();
    let mut stack = vec![base.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            if kind.is_dir() {
                stack.push(path);
            } else if kind.is_file() {
                // Independent of how a tar lists links: an extracted hard
                // link shares its inode, so a link count above one refuses it.
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    let meta = entry.metadata().map_err(|e| e.to_string())?;
                    if meta.nlink() > 1 {
                        return Err(format!("{} is a hard link", path.display()));
                    }
                }
                let rel = path
                    .strip_prefix(base)
                    .map_err(|e| e.to_string())?
                    .to_string_lossy()
                    .replace('\\', "/");
                let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                files.push(UnpackedFile {
                    path: format!("{prefix}{rel}"),
                    bytes,
                });
            } else {
                return Err(format!(
                    "{} is neither a file nor a directory",
                    path.display()
                ));
            }
        }
    }
    Ok(files)
}
