//! Runtime loading and platforms (spec 011, 3.6).
//!
//! The host names ONNX Runtime by an absolute path. The library's bytes are
//! hashed and compared with the manifest's entry for this platform before
//! anything loads it; no search path, `ORT_DYLIB_PATH`, `LD_LIBRARY_PATH` or
//! `DYLD_*` is consulted. ONNX Runtime is process-global, so the first
//! verified library is the only one: a later bundle naming another digest is
//! refused `runtime_conflict`, and an equal digest reuses the loaded runtime.
//!
//! Digest verification protects against a wrong or corrupted library, not
//! against a party that can write to the path between verification and load
//! (3.6.5); the host keeps the directory immutable and owned.

use std::io::Read;
use std::path::Path;
use std::sync::Mutex;

use ort::environment::Environment;
use sha2::{Digest, Sha256};

use crate::manifest::{EmbedManifest, Platform};
use crate::{EmbedSetupError, SetupCode};

/// The `ort` version this crate is built with; a manifest naming another is
/// refused, as the tokenizer's implementation version is (3.5.4).
pub const ORT_VERSION: &str = "2.0.0-rc.15";

/// The platforms qualified under 3.2.3 by the committed feasibility record
/// (`qualification/feasibility.json`): all four stages pass and the Node
/// binding builds with `local-embed` on the platform's native matrix target.
/// The record qualifies none, so every `onnx` bundle is refused
/// `unqualified_platform` (3.6.3). `tests/feasibility_record.rs` checks
/// that this table equals the record's `pass` verdicts.
pub const QUALIFIED_PLATFORMS: &[Platform] = &[];

/// The platform key of the compilation target (3.6.3), or `None` for any
/// target outside the three keys (musl, Windows, `darwin-x64`, ...).
pub const fn platform_key() -> Option<Platform> {
    if cfg!(all(
        target_os = "linux",
        target_arch = "x86_64",
        target_env = "gnu"
    )) {
        Some(Platform::LinuxX86_64Gnu)
    } else if cfg!(all(
        target_os = "linux",
        target_arch = "aarch64",
        target_env = "gnu"
    )) {
        Some(Platform::LinuxAarch64Gnu)
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some(Platform::MacosAarch64)
    } else {
        None
    }
}

/// The qualified platform this process runs on, or `unqualified_platform`.
pub fn qualified_platform() -> Result<Platform, EmbedSetupError> {
    match platform_key() {
        Some(p) if QUALIFIED_PLATFORMS.contains(&p) => Ok(p),
        Some(_) => Err(refusal(
            SetupCode::UnqualifiedPlatform,
            "platform",
            "the feasibility record does not qualify this platform",
        )),
        None => Err(refusal(
            SetupCode::UnqualifiedPlatform,
            "platform",
            "the target is not one of the platform keys of 3.6.3",
        )),
    }
}

/// The runtime loaded in this process: its verified digest, the minor
/// version it reported and its environment.
struct Loaded {
    sha256: String,
    minor: u32,
    environment: Environment,
}

static LOADED: Mutex<Option<Loaded>> = Mutex::new(None);

/// Verify the library at `path` against the manifest's entry for `platform`
/// and load it, or reuse the runtime already loaded with the same digest
/// (3.6.1, 3.6.2). Codes: `runtime_missing`, `runtime_digest_mismatch`,
/// `runtime_conflict`, `runtime_version_mismatch`; `manifest_invalid` when
/// the manifest names another `ort` than the one built in.
pub fn load(
    path: Option<&Path>,
    manifest: &EmbedManifest,
    platform: Platform,
) -> Result<Environment, EmbedSetupError> {
    use SetupCode as C;
    if manifest.runtime.ort != ORT_VERSION {
        return Err(refusal(
            C::ManifestInvalid,
            "runtime.ort",
            "differs from the ort version this crate is built with",
        ));
    }
    let version = minor_version(&manifest.runtime.onnxruntime).ok_or_else(|| {
        refusal(
            C::RuntimeVersionMismatch,
            "runtime.onnxruntime",
            "not an ONNX Runtime 1.x version",
        )
    })?;
    let path = match path {
        None => return Err(refusal(C::RuntimeMissing, "runtime_path", "not given")),
        Some(p) if !p.is_absolute() => {
            return Err(refusal(
                C::RuntimeMissing,
                "runtime_path",
                "not an absolute path",
            ));
        }
        Some(p) => p,
    };
    let sha256 = file_sha256(path)
        .map_err(|e| refusal(C::RuntimeMissing, "runtime_path", &e.kind().to_string()))?;
    let expected = manifest
        .runtime
        .libraries
        .iter()
        .find(|l| l.platform == platform)
        .ok_or_else(|| {
            refusal(
                C::RuntimeDigestMismatch,
                "runtime.libraries",
                "no library is accepted for this platform",
            )
        })?;
    if expected.sha256 != sha256 {
        return Err(refusal(
            C::RuntimeDigestMismatch,
            "runtime_path",
            "the library's sha256 is not the manifest's entry for this platform",
        ));
    }
    let mut loaded = LOADED.lock().unwrap_or_else(|e| e.into_inner());
    match decide(loaded.as_ref().map(|l| l.sha256.as_str()), &sha256) {
        Decision::Load => {}
        Decision::Reuse => {
            if let Some(l) = loaded.as_ref() {
                if l.minor != version {
                    return Err(refusal(
                        C::RuntimeVersionMismatch,
                        "runtime_path",
                        "the loaded library does not report the manifest's ONNX Runtime version",
                    ));
                }
                return Ok(l.environment.clone());
            }
        }
        Decision::Conflict => {
            return Err(refusal(
                C::RuntimeConflict,
                "runtime_path",
                "another runtime library digest is already loaded in this process",
            ));
        }
    }
    // `probe` reads the version the library reports (`OrtGetApiBase`) and
    // returns its minor number for a 1.x library. Its patch number cannot be
    // read without `unsafe`; the digest binds the exact bytes, whose full
    // version the feasibility record observed (3.2.2, stage 3).
    if ort::util::probe(path) != Some(version) {
        return Err(refusal(
            C::RuntimeVersionMismatch,
            "runtime_path",
            "the library does not report the manifest's ONNX Runtime version",
        ));
    }
    // `ort` refuses a library older than the API it is built for; any other
    // failure here is a failure to load the verified library.
    let environment = ort::init_from(path)
        .map_err(|e| {
            let code = match e {
                ort::LoadDynamicError::BadVersion { .. }
                | ort::LoadDynamicError::InvalidVersionString => C::RuntimeVersionMismatch,
                _ => C::RuntimeMissing,
            };
            refusal(code, "runtime_path", &e.to_string())
        })?
        .with_telemetry(false)
        .build()
        .map_err(|e| refusal(C::RuntimeMissing, "runtime_path", &e.to_string()))?;
    *loaded = Some(Loaded {
        sha256,
        minor: version,
        environment: environment.clone(),
    });
    Ok(environment)
}

#[derive(Debug, PartialEq, Eq)]
enum Decision {
    Load,
    Reuse,
    Conflict,
}

/// Load when nothing is loaded, reuse an equal digest, refuse another (3.6.2).
fn decide(loaded: Option<&str>, sha256: &str) -> Decision {
    match loaded {
        None => Decision::Load,
        Some(l) if l == sha256 => Decision::Reuse,
        Some(_) => Decision::Conflict,
    }
}

/// The minor number of an exact `1.<minor>.<patch>` version.
fn minor_version(version: &str) -> Option<u32> {
    let mut parts = version.split('.');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some("1"), Some(minor), Some(_), None) => minor.parse().ok(),
        _ => None,
    }
}

fn file_sha256(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::ErrorKind::InvalidInput.into());
    }
    let mut digest = Sha256::new();
    let mut buffer = vec![0; 1 << 16];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        digest.update(&buffer[..n]);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

fn refusal(code: SetupCode, item: &str, detail: &str) -> EmbedSetupError {
    EmbedSetupError {
        code,
        item: item.into(),
        detail: detail.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_exact_one_x() {
        assert_eq!(minor_version("1.31.0"), Some(31));
        for v in ["2.31.0", "1.31", "1.31.0.1", "1.x.0", ""] {
            assert_eq!(minor_version(v), None, "{v}");
        }
    }

    #[test]
    fn a_second_digest_conflicts_and_an_equal_one_reuses() {
        assert_eq!(decide(None, "a"), Decision::Load);
        assert_eq!(decide(Some("a"), "a"), Decision::Reuse);
        assert_eq!(decide(Some("a"), "b"), Decision::Conflict);
    }

    #[test]
    fn this_target_has_a_platform_key_and_none_is_qualified() {
        if cfg!(any(
            all(target_os = "macos", target_arch = "aarch64"),
            all(
                target_os = "linux",
                target_env = "gnu",
                any(target_arch = "x86_64", target_arch = "aarch64")
            )
        )) {
            assert!(platform_key().is_some());
        }
        assert_eq!(
            qualified_platform().unwrap_err().code,
            SetupCode::UnqualifiedPlatform
        );
    }
}
