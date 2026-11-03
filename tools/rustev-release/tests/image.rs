//! Spec 060 3.1, 3.3.2 and 3.4.1: the image read from the run's OCI
//! archives, its index digest, and the manifest's refusals (section 5: "A
//! manifest names an image without its digests or SBOMs"; 053 3.4.2: a
//! candidate manifest naming an image). No network, no container engine:
//! the archives are written here.

use std::collections::BTreeMap;
use std::path::Path;

use rustev_release::archive::sha256_hex;
use rustev_release::family_artifacts as fa;
use rustev_release::image::{self, ImageMember, ImageRelease};
use rustev_release::manifest::{self, Inputs, ReleaseManifest, Toolchain};
use rustev_release::repro::{Scratch, fresh_dir};
use rustev_release::{ReleaseSet, check_set};

/// A ustar archive of `files`, in order.
fn tar(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, body) in files {
        let mut h = [0u8; 512];
        h[..name.len()].copy_from_slice(name.as_bytes());
        h[100..107].copy_from_slice(b"0000644");
        h[108..115].copy_from_slice(b"0000000");
        h[116..123].copy_from_slice(b"0000000");
        h[124..135].copy_from_slice(format!("{:011o}", body.len()).as_bytes());
        h[136..147].copy_from_slice(b"00000000000");
        h[156] = b'0';
        h[257..263].copy_from_slice(b"ustar\0");
        h[263..265].copy_from_slice(b"00");
        h[148..156].copy_from_slice(b"        ");
        let sum: u32 = h.iter().map(|b| u32::from(*b)).sum();
        h[148..155].copy_from_slice(format!("{sum:06o}\0").as_bytes());
        out.extend_from_slice(&h);
        out.extend_from_slice(body);
        out.resize(out.len().div_ceil(512) * 512, 0);
    }
    out.resize(out.len() + 1024, 0);
    out
}

fn digest(b: &[u8]) -> String {
    format!("sha256:{}", sha256_hex(b))
}

/// An OCI archive as buildx writes one platform: `index.json` naming one
/// image manifest, whose config names `os`/`arch`.
fn oci(os: &str, arch: &str, variant: Option<&str>) -> Vec<u8> {
    let mut config = serde_json::json!({ "architecture": arch, "os": os, "rootfs": { "type": "layers", "diff_ids": [] } });
    if let Some(v) = variant {
        config["variant"] = v.into();
    }
    let config = serde_json::to_vec(&config).unwrap();
    let manifest = serde_json::to_vec(&serde_json::json!({
        "schemaVersion": 2,
        "mediaType": image::MANIFEST_MEDIA_TYPE,
        "config": { "mediaType": "application/vnd.oci.image.config.v1+json", "digest": digest(&config), "size": config.len() },
        "layers": []
    }))
    .unwrap();
    let index = serde_json::to_vec(&serde_json::json!({
        "schemaVersion": 2,
        "mediaType": image::INDEX_MEDIA_TYPE,
        "manifests": [{ "mediaType": image::MANIFEST_MEDIA_TYPE, "digest": digest(&manifest), "size": manifest.len() }]
    }))
    .unwrap();
    let m = format!("blobs/sha256/{}", sha256_hex(&manifest));
    let c = format!("blobs/sha256/{}", sha256_hex(&config));
    tar(&[
        ("oci-layout", br#"{"imageLayoutVersion":"1.0.0"}"#),
        ("index.json", &index),
        (&m, &manifest),
        (&c, &config),
    ])
}

fn archives() -> (Vec<u8>, Vec<u8>) {
    (
        oci("linux", "amd64", None),
        oci("linux", "arm64", Some("v8")),
    )
}

fn image_release(version: &str) -> ImageRelease {
    let (amd, arm) = archives();
    image::release(version, &[("linux/amd64", &amd), ("linux/arm64", &arm)]).unwrap()
}

#[test]
fn the_index_is_a_pure_function_of_the_platform_manifests() {
    let (amd, arm) = archives();
    let i = image_release("0.5.0");
    assert_eq!(i.repository, "ghcr.io/statecrafting/rustev-serve");
    assert_eq!(
        (i.tag.as_str(), i.layer, i.slsa_build_level),
        ("0.5.0", 7, 2)
    );
    let amd_m = image::platform_manifest(&amd, "linux/amd64").unwrap();
    let arm_m = image::platform_manifest(&arm, "linux/arm64").unwrap();
    assert_eq!(i.platforms, [amd_m.clone(), arm_m.clone()]);
    assert_eq!(arm_m.variant.as_deref(), Some("v8"));
    assert_eq!(amd_m.sbom, "rustev-serve-linux-amd64.oci.tar.cdx.json");
    let bytes = image::index_bytes(&i.platforms);
    assert_eq!(
        String::from_utf8(bytes.clone()).unwrap(),
        format!(
            r#"{{"schemaVersion":2,"mediaType":"application/vnd.oci.image.index.v1+json","manifests":[{{"mediaType":"application/vnd.oci.image.manifest.v1+json","digest":"{}","size":{},"platform":{{"os":"linux","architecture":"amd64"}}}},{{"mediaType":"application/vnd.oci.image.manifest.v1+json","digest":"{}","size":{},"platform":{{"os":"linux","architecture":"arm64","variant":"v8"}}}}]}}"#,
            amd_m.digest, amd_m.size, arm_m.digest, arm_m.size
        )
    );
    assert_eq!(i.index_digest, digest(&bytes));
    // The same archives give the same index on a rebuild.
    assert_eq!(image_release("0.5.0"), i);
    // The platforms come in the fixed order only.
    assert!(image::release("0.5.0", &[("linux/arm64", &arm), ("linux/amd64", &amd)]).is_err());
}

#[test]
fn an_archive_that_is_not_its_platforms_image_is_refused() {
    let (amd, _) = archives();
    let err = image::platform_manifest(&amd, "linux/arm64").unwrap_err();
    assert!(err.contains("the image is for linux/amd64"), "{err}");
    // A blob whose bytes are not its name.
    let mut bad = amd.clone();
    let at = bad.windows(6).rposition(|w| w == b"rootfs").unwrap();
    bad[at] = b'R';
    let err = image::platform_manifest(&bad, "linux/amd64").unwrap_err();
    assert!(err.contains("does not hash to its name"), "{err}");
    // An index of two manifests, and no index at all.
    let two = tar(&[(
        "index.json",
        br#"{"manifests":[{"mediaType":"application/vnd.oci.image.manifest.v1+json"},{}]}"#,
    )]);
    assert!(
        image::platform_manifest(&two, "linux/amd64")
            .unwrap_err()
            .contains("exactly one")
    );
    let none = tar(&[("oci-layout", b"{}")]);
    assert!(
        image::platform_manifest(&none, "linux/amd64")
            .unwrap_err()
            .contains("no index.json")
    );
}

fn sboms() -> Vec<&'static str> {
    vec![
        "rustev-serve-linux-amd64.oci.tar.cdx.json",
        "rustev-serve-linux-arm64.oci.tar.cdx.json",
    ]
}

#[test]
fn a_manifest_image_without_its_digests_or_sboms_is_refused() {
    let good = image_release("0.5.0");
    image::validate(&good, "0.5.0", &sboms()).unwrap();
    let refused = |i: &ImageRelease, sboms: &[&str], why: &str| {
        let err = image::validate(i, "0.5.0", sboms).unwrap_err();
        assert!(err.contains("060 3.4.1") && err.contains(why), "{err}");
    };
    refused(&good, &sboms()[..1], "lacks linux/arm64's SBOM");
    let mut i = good.clone();
    i.platforms[0].digest = String::new();
    refused(&i, &sboms(), "lacks linux/amd64's manifest digest");
    let mut i = good.clone();
    i.platforms.pop();
    refused(&i, &sboms(), "must name linux/amd64, linux/arm64");
    let mut i = good.clone();
    i.index_digest = format!("sha256:{}", "0".repeat(64));
    refused(&i, &sboms(), "index digest");
    let mut i = good.clone();
    i.platforms[1].size += 1;
    refused(&i, &sboms(), "index digest");
    let mut i = good.clone();
    i.tag = "latest".into();
    refused(&i, &sboms(), "family version");
    let mut i = good.clone();
    i.repository = "docker.io/statecrafting/rustev-serve".into();
    refused(&i, &sboms(), "is not in ghcr.io/statecrafting/rustev-serve");
    let mut i = good;
    i.slsa_build_level = 3;
    refused(&i, &sboms(), "SLSA build level 2");
}

fn write(path: &Path, b: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, b).unwrap();
}

/// The committed release set with the image a member (060 3.1.1).
fn member_set() -> ReleaseSet {
    let mut set = ReleaseSet::parse(include_bytes!("../release-set.json")).unwrap();
    set.absent.retain(|a| a.kind != "image");
    set.image = Some(ImageMember {
        repository: image::REPOSITORY.into(),
        platforms: image::PLATFORMS.iter().map(|p| p.to_string()).collect(),
    });
    set
}

/// The release set with the image a member, and a run's evidence and
/// artifacts holding both archives, their sidecars and SBOMs.
fn member_fixture() -> (Scratch, ReleaseSet) {
    let root = fresh_dir("rustev-image-test").unwrap();
    let mut set = member_set();
    set.version = "0.5.0".into();
    set.tag = "v0.5.0".into();
    set.upload_order = vec!["rustev-contract".into()];
    set.layers = vec![set.upload_order.clone()];
    set.npm = vec![set.npm.last().unwrap().clone()];
    let cdx = br#"{"bomFormat":"CycloneDX","specVersion":"1.5"}"#;
    let evidence = root.join("evidence");
    write(&evidence.join("rustev-contract-0.5.0.crate"), b"crate");
    write(&evidence.join("sbom-rustev-contract.cdx.json"), cdx);
    write(&evidence.join("changelog.md"), b"release notes");
    let artifacts = root.join("artifacts");
    let mut names = vec![("npm", "rustev-rustev-wasm-0.5.0.tgz".to_string())];
    for p in fa::PLATFORMS {
        names.push(("cli", format!("rustev-0.5.0-{p}.tar.gz")));
    }
    for (kind, name) in names {
        write(&artifacts.join(kind).join(&name), name.as_bytes());
        write(
            &artifacts.join("sbom").join(format!("{name}.cdx.json")),
            cdx,
        );
        if kind == "cli" {
            write(
                &artifacts.join(kind).join(format!("{name}.sha256")),
                format!("{}  {name}\n", sha256_hex(name.as_bytes())).as_bytes(),
            );
        }
    }
    let (amd, arm) = archives();
    for (p, bytes) in [("linux/amd64", amd), ("linux/arm64", arm)] {
        let name = image::archive_name(p);
        write(&artifacts.join("image").join(&name), &bytes);
        write(
            &artifacts.join("image").join(format!("{name}.sha256")),
            format!("{}  {name}\n", sha256_hex(&bytes)).as_bytes(),
        );
        write(&artifacts.join("sbom").join(image::sbom_name(p)), cdx);
    }
    (root, set)
}

fn manifest_of(set: &ReleaseSet, root: &Path) -> Result<ReleaseManifest, String> {
    let evidence = root.join("evidence");
    let mut m = manifest::build(
        set,
        &evidence,
        Inputs {
            commit: "c".repeat(40),
            tree: "a".repeat(40),
            toolchain: Toolchain {
                rustc: "fixture".into(),
                cargo: "fixture".into(),
                host: "fixture".into(),
                audit: "fixture".into(),
            },
            source_date_epoch: 0,
            evidence: BTreeMap::new(),
            limitations: vec![],
        },
    )?;
    (m.artifacts, m.image) = fa::collect_family(set, &evidence, Some(&root.join("artifacts")))?;
    fa::validate(&m)?;
    Ok(m)
}

#[test]
fn a_member_image_is_named_in_the_manifest_with_its_sboms() {
    let (root, set) = member_fixture();
    assert!(
        check_set(&member_set()).is_empty(),
        "{:?}",
        check_set(&member_set())
    );
    let m = manifest_of(&set, &root).unwrap();
    let image = m.image.clone().unwrap();
    assert_eq!(image, image_release("0.5.0"));
    for s in sboms() {
        let a = m.artifacts.iter().find(|a| a.name == s).unwrap();
        assert_eq!((a.kind.as_str(), a.layer), ("sbom", 8));
        assert!(manifest::sha256sums(&m).contains(&format!("  {s}\n")));
    }
    // The archives are not release assets.
    assert!(!m.artifacts.iter().any(|a| a.name.ends_with(".oci.tar")));
    // The canonical bytes round-trip, image included.
    assert_eq!(ReleaseManifest::parse(&m.canonical()).unwrap(), m);
    // Without the image member, no image is named and its files are unexpected.
    let mut absent = set.clone();
    absent.image = None;
    let err = manifest_of(&absent, &root).unwrap_err();
    assert!(err.contains("unexpected image/"), "{err}");
}

#[test]
fn a_member_image_without_its_archives_or_sboms_is_refused() {
    for (missing, why) in [
        (
            "image/rustev-serve-linux-arm64.oci.tar",
            "missing image/rustev-serve-linux-arm64.oci.tar",
        ),
        (
            "image/rustev-serve-linux-amd64.oci.tar.sha256",
            "sha256: missing or differs",
        ),
        (
            "sbom/rustev-serve-linux-amd64.oci.tar.cdx.json",
            "missing SBOM rustev-serve-linux-amd64",
        ),
    ] {
        let (root, set) = member_fixture();
        std::fs::remove_file(root.join("artifacts").join(missing)).unwrap();
        let err = manifest_of(&set, &root).unwrap_err();
        assert!(
            err.contains("060 3.4.1") && err.contains(why),
            "{missing}: {err}"
        );
    }
}

#[test]
fn a_candidate_manifest_never_names_an_image() {
    let (root, set) = member_fixture();
    let mut m = manifest_of(&set, &root).unwrap();
    let base = ReleaseSet::parse(include_bytes!("../release-set.json"))
        .unwrap()
        .version;
    m.version = format!("{base}-rc.1");
    m.tag = format!("v{}", m.version);
    m.packages.clear();
    m.image.as_mut().unwrap().tag = m.version.clone();
    let err = fa::validate(&m).unwrap_err();
    assert!(err.contains("053 3.4.2"), "{err}");
    m.image = None;
    fa::validate(&m).unwrap();
    // An image as an artifact is refused for a release and a candidate.
    let mut a = m.artifacts[0].clone();
    a.kind = "image".into();
    a.name = "rustev-serve.oci.tar".into();
    m.artifacts.push(a);
    assert!(
        fa::validate(&m)
            .unwrap_err()
            .contains("never as an artifact")
    );
}

#[test]
fn the_release_set_names_the_image_once_with_its_fixed_repository_and_platforms() {
    let set = member_set();
    let mut both = set.clone();
    both.absent.push(rustev_release::npm::Absent {
        kind: "image".into(),
        name: image::REPOSITORY.into(),
        reason: "not released".into(),
    });
    let all: Vec<String> = check_set(&both).iter().map(|v| v.to_string()).collect();
    assert!(
        all.iter().any(|v| v.contains("both a member and absent")),
        "{all:?}"
    );
    let mut other = set.clone();
    other.image.as_mut().unwrap().repository = "ghcr.io/someone/rustev-serve".into();
    assert!(!check_set(&other).is_empty());
    let mut one = set;
    one.image.as_mut().unwrap().platforms.pop();
    assert!(!check_set(&one).is_empty());
}

/// One qualification step for `m` (056 3.3), passed or excluded.
fn step(m: &ReleaseManifest, platform: &str, kind: &str, passed: bool) -> Vec<u8> {
    use rustev_release::qualification::{SCHEMA, Status, Step};
    serde_json::to_vec(&Step {
        schema: SCHEMA.into(),
        manifest_sha256: m.digest(),
        platform: platform.into(),
        kind: kind.into(),
        command: vec!["consumer-check".into()],
        exit_status: Some(0),
        output_digest: sha256_hex(b"output"),
        lockfile_or_install_record: "records".into(),
        checksums: BTreeMap::from([("artifact".into(), sha256_hex(b"artifact"))]),
        host: "fixture-host".into(),
        os: "fixture-os".into(),
        architecture: "fixture-arch".into(),
        toolchain: "rustc fixture".into(),
        node: "node fixture".into(),
        status: if passed {
            Status::Passed
        } else {
            Status::Excluded {
                reason: "the image is qualified on linux/amd64 and linux/arm64 only".into(),
            }
        },
    })
    .unwrap()
}

#[test]
fn a_named_image_is_qualified_on_amd64_and_arm64_linux_only() {
    use rustev_release::qualification as q;
    let (root, set) = member_fixture();
    let m = manifest_of(&set, &root).unwrap();
    let files = q::required_files(&m);
    assert!(files.contains(&"linux-x64-gnu.image.json".to_string()));
    assert!(files.contains(&"linux-arm64-gnu.image.json".to_string()));
    assert!(
        !files
            .iter()
            .any(|f| f.ends_with(".image.json") && !f.starts_with("linux-"))
    );
    assert!(!files.contains(&"linux-x64-musl.image.json".to_string()));
    let write_all = |dir: &Path| {
        std::fs::create_dir_all(dir).unwrap();
        for p in fa::PLATFORMS {
            for k in q::KINDS {
                let passed = q::required_on(&m, k, p);
                std::fs::write(dir.join(format!("{p}.{k}.json")), step(&m, p, k, passed)).unwrap();
            }
        }
    };
    let dir = root.join("qualification");
    write_all(&dir);
    q::evidence(&m, &dir).unwrap();
    // An excluded image on a qualifying platform fails.
    std::fs::write(
        dir.join("linux-arm64-gnu.image.json"),
        step(&m, "linux-arm64-gnu", "image", false),
    )
    .unwrap();
    assert!(
        q::evidence(&m, &dir)
            .unwrap_err()
            .contains("linux-arm64-gnu.image.json")
    );
    // A missing image step on a qualifying platform fails.
    write_all(&dir);
    std::fs::remove_file(dir.join("linux-x64-gnu.image.json")).unwrap();
    assert!(
        q::evidence(&m, &dir)
            .unwrap_err()
            .contains("linux-x64-gnu.image.json")
    );
    // Elsewhere the image step must be excluded with a reason.
    write_all(&dir);
    std::fs::write(
        dir.join("darwin-arm64.image.json"),
        step(&m, "darwin-arm64", "image", true),
    )
    .unwrap();
    assert!(
        q::evidence(&m, &dir)
            .unwrap_err()
            .contains("darwin-arm64.image.json")
    );
}
