//! Runtime loading and the platform table (3.6), and the ONNX encoder
//! (3.9.1). The default tests need no runtime library: the runtime codes are
//! reached with no path and with a synthetic non-library file (3.19.3). The
//! ignored tests need the pinned checkpoint and ONNX Runtime and run by
//! hand (`hand_real_encoder`, `hand_complete_backend`).
use super::*;
use rustev_backend_embed_linear::cache::{CacheConfig, EmbeddingCache, RepeatabilityEvidence};
use rustev_backend_embed_linear::manifest::{Library, Platform};
use rustev_backend_embed_linear::runtime::{
    self, FEASIBILITY_RECORD, IN_PROCESS_REPEATABILITY, ORT_VERSION, QUALIFIED_PLATFORMS,
    platform_key, qualified_platform,
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
fn the_platform_table_is_the_records_and_qualifies_linux_x86_64_gnu() {
    assert_eq!(QUALIFIED_PLATFORMS, [Platform::LinuxX86_64Gnu]);
    match qualified_platform() {
        Ok(p) => assert_eq!(Some(p), platform_key()),
        Err(e) => assert_eq!(e.code, SetupCode::UnqualifiedPlatform),
    }
    assert_eq!(
        qualified_platform().is_ok(),
        platform_key() == Some(Platform::LinuxX86_64Gnu)
    );
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

/// An `onnx` bundle over the synthetic bundle's files, with the pinned
/// versions so a qualified target reaches the runtime path check.
fn onnx_bundle() -> Bundle {
    let mut b = new_bundle();
    let v = &mut b.manifest;
    v["encoder"]["kind"] = json!("onnx");
    v["encoder"]["graph"]["source"] = json!("onnx/model.onnx");
    v["checkpoint"]["repository"] = json!("sentence-transformers/all-MiniLM-L6-v2");
    v["checkpoint"]["files"] =
        json!([{"name":"onnx/model.onnx", "sha256":v["encoder"]["graph"]["sha256"]}]);
    v["runtime"]["libraries"] = json!([{"platform":"macos-aarch64", "sha256":"a".repeat(64)}]);
    // The pinned versions, so a qualified target reaches the path check.
    v["runtime"]["ort"] = json!(ORT_VERSION);
    v["runtime"]["onnxruntime"] = json!("1.31.0");
    v["tolerance"]["feasibility_record"] = json!(format!("sha256:{}", "b".repeat(64)));
    for h in v["heads"].as_array_mut().unwrap() {
        h["interpretation"] = json!("linear scores");
        h["provenance"]["synthetic"] = json!(false);
    }
    b.write();
    b
}

fn onnx_config(
    b: &Bundle,
    runtime_path: Option<PathBuf>,
) -> rustev_backend_embed_linear::EmbedConfig {
    rustev_backend_embed_linear::EmbedConfig {
        bundle_dir: b.dir.clone(),
        runtime_path,
        expected_artifact: b.artifact(),
        workers: 1,
        max_queued: 0,
        embedding_cache: None,
    }
}

#[test]
/// On an unqualified target an `onnx` bundle is refused before its runtime
/// path is read; on the qualified one the path is checked next (3.6.1).
fn an_onnx_bundle_is_refused_before_its_runtime_path_is_read() {
    let b = onnx_bundle();
    for runtime_path in [
        None,
        Some(PathBuf::from("relative")),
        Some(b.dir.join("absent")),
    ] {
        let e =
            rustev_backend_embed_linear::EmbedLinearBackend::open(onnx_config(&b, runtime_path))
                .err()
                .unwrap();
        assert_eq!(
            (e.code, e.item.as_str()),
            if qualified_platform().is_ok() {
                (SetupCode::RuntimeMissing, "runtime_path")
            } else {
                (SetupCode::UnqualifiedPlatform, "platform")
            }
        );
    }
}

/// The measured settings of `IN_PROCESS_REPEATABILITY` on an `onnx`
/// manifest: the decision of 3.14.3 reads the manifest only, so no real
/// graph or library is needed.
fn measured_manifest(b: &Bundle) -> EmbedManifest {
    let m = &IN_PROCESS_REPEATABILITY[0];
    let mut manifest = runtime_manifest(b, m.platform, None);
    manifest.encoder.kind = manifest::EncoderKind::Onnx;
    manifest.encoder.graph.sha256 = m.graph_sha256.into();
    manifest.encoder.precision = m.precision;
    manifest.runtime.onnxruntime = m.onnxruntime.into();
    manifest.runtime.ort = m.ort.into();
    manifest.runtime.execution_provider = m.execution_provider;
    manifest.runtime.graph_optimization = m.graph_optimization;
    manifest.runtime.intra_op_threads = m.intra_op_threads;
    manifest.runtime.inter_op_threads = m.inter_op_threads;
    manifest.runtime.libraries = vec![
        Library {
            platform: Platform::MacosAarch64,
            sha256: "a".repeat(64),
        },
        Library {
            platform: m.platform,
            sha256: m.runtime_sha256.into(),
        },
    ];
    manifest.pooling.mode = m.pooling_mode;
    manifest.pooling.normalize = m.normalize;
    manifest.tolerance.feasibility_record = FEASIBILITY_RECORD.into();
    manifest
}

/// The measured manifest with the measured library accepted for
/// `platform` instead: one entry, as a checked manifest has one per key.
fn retarget(manifest: &mut EmbedManifest, platform: Platform) {
    manifest.runtime.libraries = vec![Library {
        platform,
        sha256: IN_PROCESS_REPEATABILITY[0].runtime_sha256.into(),
    }];
}

/// The cache decision as the backend makes it at setup (3.14.3).
fn cache_on(manifest: &EmbedManifest, platform: Platform) -> Result<(), SetupCode> {
    let artifact = manifest.artifact_id().unwrap();
    let evidence = runtime::repeatability_evidence(manifest, &artifact, platform);
    let limits = rustev_backend_embed_linear::cache::CacheLimits {
        max_entries: 4,
        max_bytes: 1 << 20,
    };
    EmbeddingCache::new(
        CacheConfig {
            embedding_cache: Some(limits),
        },
        manifest,
        platform.key(),
        &evidence,
    )
    .map(drop)
    .map_err(|e| {
        assert_eq!(e.item, "embedding_cache");
        e.code
    })
}

#[test]
fn the_cache_is_allowed_for_an_onnx_bundle_with_the_measured_settings_on_the_qualified_platform() {
    assert_eq!(IN_PROCESS_REPEATABILITY.len(), 1);
    assert!(
        IN_PROCESS_REPEATABILITY
            .iter()
            .all(|m| QUALIFIED_PLATFORMS.contains(&m.platform))
    );
    let b = new_bundle();
    let manifest = measured_manifest(&b);
    assert_eq!(cache_on(&manifest, Platform::LinuxX86_64Gnu), Ok(()));
    let artifact = manifest.artifact_id().unwrap();
    assert_eq!(
        runtime::repeatability_evidence(&manifest, &artifact, Platform::LinuxX86_64Gnu),
        RepeatabilityEvidence::Measured {
            artifact,
            platform: "linux-x86_64-gnu".into(),
            feasibility_record: FEASIBILITY_RECORD.into(),
            bitwise_in_process: true,
        }
    );
    // Heads, cost and the backend id are not encoder settings.
    let mut other_heads = manifest.clone();
    other_heads.backend_id = "embed.other".into();
    other_heads.cost.units_per_call += 1;
    other_heads.heads[0].interpretation = "other linear scores".into();
    assert_eq!(cache_on(&other_heads, Platform::LinuxX86_64Gnu), Ok(()));
}

#[test]
fn the_cache_is_refused_on_every_other_platform_and_for_every_other_setting() {
    let b = new_bundle();
    let manifest = measured_manifest(&b);
    // The record shows bitwise repeatability on linux-aarch64-gnu too, but
    // the platform is not qualified; macOS is neither.
    for platform in [Platform::LinuxAarch64Gnu, Platform::MacosAarch64] {
        let mut m = manifest.clone();
        retarget(&mut m, platform);
        assert_eq!(
            cache_on(&m, platform),
            Err(SetupCode::CacheNotEquivalent),
            "{platform:?}"
        );
    }
    type Change = (&'static str, fn(&mut EmbedManifest));
    let changes: [Change; 11] = [
        ("synthetic encoder", |m| {
            m.encoder.kind = manifest::EncoderKind::Synthetic
        }),
        ("graph", |m| m.encoder.graph.sha256 = "c".repeat(64)),
        ("onnxruntime", |m| m.runtime.onnxruntime = "1.30.0".into()),
        ("ort", |m| m.runtime.ort = "2.0.0-rc.10".into()),
        ("library", |m| {
            m.runtime.libraries[1].sha256 = "d".repeat(64)
        }),
        ("no library", |m| {
            m.runtime.libraries.remove(1);
        }),
        ("optimization", |m| {
            m.runtime.graph_optimization = manifest::GraphOptimization::Basic
        }),
        ("intra-op threads", |m| m.runtime.intra_op_threads = 2),
        ("inter-op threads", |m| m.runtime.inter_op_threads = 2),
        ("normalization", |m| {
            m.pooling.normalize = manifest::Normalize::None
        }),
        ("record", |m| {
            m.tolerance.feasibility_record = format!("sha256:{}", "e".repeat(64))
        }),
    ];
    for (name, change) in changes {
        let mut m = manifest.clone();
        change(&mut m);
        assert_eq!(
            cache_on(&m, Platform::LinuxX86_64Gnu),
            Err(SetupCode::CacheNotEquivalent),
            "{name}"
        );
    }
}

#[test]
/// Through setup: on the qualified target an `onnx` bundle's cache is
/// decided after the platform and before the runtime path, so a bundle
/// without the measured settings is `cache_not_equivalent` with no runtime;
/// elsewhere the platform refuses first.
fn an_onnx_bundle_with_a_cache_and_unmeasured_settings_is_refused_at_setup() {
    let b = onnx_bundle();
    let mut config = onnx_config(&b, None);
    config.embedding_cache = Some(rustev_backend_embed_linear::cache::CacheLimits {
        max_entries: 4,
        max_bytes: 1 << 20,
    });
    let e = rustev_backend_embed_linear::EmbedLinearBackend::open(config)
        .err()
        .unwrap();
    assert_eq!(
        (e.code, e.item.as_str()),
        if qualified_platform().is_ok() {
            (SetupCode::CacheNotEquivalent, "embedding_cache")
        } else {
            (SetupCode::UnqualifiedPlatform, "platform")
        }
    );
}

#[test]
/// This target's own decision: the cache is allowed for the measured
/// settings on `linux-x86_64-gnu` and refused on every other key.
fn this_targets_cache_decision_follows_the_record() {
    let Some(platform) = platform_key() else {
        return; // No platform key: an `onnx` bundle is refused first.
    };
    let mut m = measured_manifest(&new_bundle());
    retarget(&mut m, platform);
    let expected = if platform == Platform::LinuxX86_64Gnu {
        Ok(())
    } else {
        Err(SetupCode::CacheNotEquivalent)
    };
    assert_eq!(cache_on(&m, platform), expected);
}
