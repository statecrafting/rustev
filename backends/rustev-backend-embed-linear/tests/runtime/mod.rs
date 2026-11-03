//! Runtime loading and the platform table (3.6), and the ONNX encoder
//! (3.9.1). The default tests need no runtime library: the runtime codes are
//! reached with no path and with a synthetic non-library file (3.19.3). The
//! ignored test needs the pinned checkpoint and ONNX Runtime and runs by
//! hand (`hand_real_encoder`).
use super::*;
use rustev_backend_embed_linear::manifest::{Library, Platform};
use rustev_backend_embed_linear::runtime::{
    self, ORT_VERSION, QUALIFIED_PLATFORMS, platform_key, qualified_platform,
};

mod hand;

/// The synthetic bundle's manifest with the pinned runtime versions and,
/// when `library` is given, that digest accepted for `platform`.
fn runtime_manifest(b: &Bundle, platform: Platform, library: Option<&[u8]>) -> EmbedManifest {
    let mut m = b.verify().unwrap().manifest;
    m.runtime.ort = ORT_VERSION.into();
    m.runtime.onnxruntime = "1.31.0".into();
    m.runtime.libraries = library
        .map(|bytes| {
            vec![Library {
                platform,
                sha256: sha256_hex(bytes),
            }]
        })
        .unwrap_or_default();
    m
}

fn code(r: Result<impl Sized, rustev_backend_embed_linear::EmbedSetupError>) -> SetupCode {
    r.err().expect("refused").code
}

/// Any platform key: the loading checks do not depend on qualification.
const ANY: Platform = Platform::LinuxX86_64Gnu;

#[test]
fn a_missing_or_relative_runtime_path_is_runtime_missing() {
    let b = new_bundle();
    let m = runtime_manifest(&b, ANY, Some(b"x"));
    assert_eq!(
        code(runtime::load(None, &m, ANY)),
        SetupCode::RuntimeMissing
    );
    for p in ["libonnxruntime.so", "lib/libonnxruntime.so"] {
        assert_eq!(
            code(runtime::load(Some(Path::new(p)), &m, ANY)),
            SetupCode::RuntimeMissing,
            "{p}"
        );
    }
    let absent = b.dir.join("absent-runtime");
    assert_eq!(
        code(runtime::load(Some(&absent), &m, ANY)),
        SetupCode::RuntimeMissing
    );
    assert_eq!(
        code(runtime::load(Some(&b.dir), &m, ANY)),
        SetupCode::RuntimeMissing,
        "a directory is not a library"
    );
}

#[test]
fn a_library_not_accepted_for_the_platform_is_runtime_digest_mismatch() {
    let b = new_bundle();
    let path = b.dir.join("not-a-library");
    fs::write(&path, b"not a runtime library").unwrap();
    let other = runtime_manifest(&b, ANY, Some(b"some other library"));
    assert_eq!(
        code(runtime::load(Some(&path), &other, ANY)),
        SetupCode::RuntimeDigestMismatch
    );
    let accepted = runtime_manifest(&b, ANY, Some(b"not a runtime library"));
    assert_eq!(
        code(runtime::load(
            Some(&path),
            &accepted,
            Platform::MacosAarch64
        )),
        SetupCode::RuntimeDigestMismatch,
        "no entry for the platform"
    );
    let none = runtime_manifest(&b, ANY, None);
    assert_eq!(
        code(runtime::load(Some(&path), &none, ANY)),
        SetupCode::RuntimeDigestMismatch
    );
}

#[test]
fn a_verified_file_reporting_no_version_is_runtime_version_mismatch() {
    let b = new_bundle();
    let path = b.dir.join("not-a-library");
    fs::write(&path, b"not a runtime library").unwrap();
    let m = runtime_manifest(&b, ANY, Some(b"not a runtime library"));
    assert_eq!(
        code(runtime::load(Some(&path), &m, ANY)),
        SetupCode::RuntimeVersionMismatch
    );
    let mut m2 = m.clone();
    m2.runtime.onnxruntime = "2.0.0".into();
    assert_eq!(
        code(runtime::load(Some(&path), &m2, ANY)),
        SetupCode::RuntimeVersionMismatch
    );
    let mut m3 = m;
    m3.runtime.ort = "2.0.0-rc.10".into();
    assert_eq!(
        code(runtime::load(Some(&path), &m3, ANY)),
        SetupCode::ManifestInvalid,
        "a manifest naming another ort"
    );
}

#[test]
fn the_platform_table_is_the_records_and_qualifies_nothing() {
    assert!(QUALIFIED_PLATFORMS.is_empty());
    assert_eq!(code(qualified_platform()), SetupCode::UnqualifiedPlatform);
    let expected = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some(Platform::MacosAarch64)
    } else if cfg!(all(
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
    } else {
        None
    };
    assert_eq!(platform_key(), expected);
}

#[test]
fn an_onnx_bundle_is_refused_before_its_runtime_path_is_read() {
    let mut b = new_bundle();
    let v = &mut b.manifest;
    v["encoder"]["kind"] = json!("onnx");
    v["encoder"]["graph"]["source"] = json!("onnx/model.onnx");
    v["checkpoint"]["repository"] = json!("sentence-transformers/all-MiniLM-L6-v2");
    v["checkpoint"]["files"] =
        json!([{"name":"onnx/model.onnx", "sha256":v["encoder"]["graph"]["sha256"]}]);
    v["runtime"]["libraries"] = json!([{"platform":"macos-aarch64", "sha256":"a".repeat(64)}]);
    v["tolerance"]["feasibility_record"] = json!(format!("sha256:{}", "b".repeat(64)));
    for h in v["heads"].as_array_mut().unwrap() {
        h["interpretation"] = json!("linear scores");
        h["provenance"]["synthetic"] = json!(false);
    }
    b.write();
    for runtime_path in [
        None,
        Some(PathBuf::from("relative")),
        Some(b.dir.join("absent")),
    ] {
        let e = rustev_backend_embed_linear::EmbedLinearBackend::open(
            rustev_backend_embed_linear::EmbedConfig {
                bundle_dir: b.dir.clone(),
                runtime_path,
                expected_artifact: b.artifact(),
                workers: 1,
                max_queued: 0,
                embedding_cache: None,
            },
        )
        .err()
        .unwrap();
        assert_eq!(
            (e.code, e.item.as_str()),
            (SetupCode::UnqualifiedPlatform, "platform")
        );
    }
}
