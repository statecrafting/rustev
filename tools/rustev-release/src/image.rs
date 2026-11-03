//! Spec 060 3.1, 3.3.2 and 3.4.1: the `rustev-serve` image as a release
//! member. The run's verified OCI archives give each platform's manifest
//! descriptor; the OCI index that joins them is a pure function of those
//! descriptors in a fixed platform order, so the manifest names the index
//! digest before anything is pushed, and the release and its rebuild agree
//! on it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ReleaseSet;
use crate::archive::sha256_hex;

/// 060 3.1.1: the one repository the image is released to.
pub const REPOSITORY: &str = "ghcr.io/statecrafting/rustev-serve";
/// 060 3.1.1: the platforms, in the index's fixed order.
pub const PLATFORMS: &[&str] = &["linux/amd64", "linux/arm64"];
pub const INDEX_MEDIA_TYPE: &str = "application/vnd.oci.image.index.v1+json";
pub const MANIFEST_MEDIA_TYPE: &str = "application/vnd.oci.image.manifest.v1+json";
/// 060 3.3.4: the build level the attestations support.
pub const SLSA_BUILD_LEVEL: u8 = 2;
/// 031 3.1: the image is layer 7.
pub const LAYER: u8 = 7;

/// The release set's `image` entry (060 3.1.1).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageMember {
    pub repository: String,
    pub platforms: Vec<String>,
}

/// The manifest's image at layer 7 (060 3.4.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageRelease {
    pub repository: String,
    /// The family version `X.Y.Z` (060 3.1.2).
    pub tag: String,
    pub layer: u8,
    /// `sha256:<hex>` of [`index_bytes`] over `platforms`.
    pub index_digest: String,
    pub platforms: Vec<ImagePlatform>,
    pub slsa_build_level: u8,
}

/// One platform's image manifest, as its verified OCI archive names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImagePlatform {
    /// `linux/amd64` or `linux/arm64`.
    pub platform: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    pub media_type: String,
    /// `sha256:<hex>` of the platform manifest.
    pub digest: String,
    pub size: u64,
    /// The release asset holding this platform's CycloneDX SBOM.
    pub sbom: String,
}

/// The OCI archive and SBOM file names of `platform` (as `artifacts.sh`
/// writes them).
pub fn archive_name(platform: &str) -> String {
    format!("rustev-serve-{}.oci.tar", platform.replace('/', "-"))
}

pub fn sbom_name(platform: &str) -> String {
    format!("{}.cdx.json", archive_name(platform))
}

/// The image member of `set`, if any, checked against 060 3.1.1: the fixed
/// repository and platforms, and no absence of kind `image` beside it.
pub fn member(set: &ReleaseSet) -> Result<Option<&ImageMember>, String> {
    let Some(image) = &set.image else {
        return Ok(None);
    };
    if image.repository != REPOSITORY {
        return Err(format!(
            "060 3.1.1: the image repository is {REPOSITORY}, not {}",
            image.repository
        ));
    }
    if image.platforms != PLATFORMS {
        return Err(format!(
            "060 3.1.1: the image platforms are {}, in that order",
            PLATFORMS.join(", ")
        ));
    }
    if set.absent.iter().any(|a| a.kind == "image") {
        return Err("060 3.1.1: the image is both a member and absent".into());
    }
    Ok(Some(image))
}

fn digest_ok(d: &str) -> bool {
    d.strip_prefix("sha256:").is_some_and(|h| {
        h.len() == 64
            && h.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}

/// The members of an uncompressed tar archive by path (`./` stripped).
/// Regular files only; PAX and GNU long-name headers are honored.
pub fn tar_files(bytes: &[u8]) -> Result<BTreeMap<String, &[u8]>, String> {
    fn octal(field: &[u8]) -> Result<usize, String> {
        let s: String = field
            .iter()
            .take_while(|b| **b != 0)
            .map(|b| *b as char)
            .collect();
        usize::from_str_radix(s.trim(), 8).map_err(|_| format!("invalid tar size {s:?}"))
    }
    fn text(field: &[u8]) -> String {
        String::from_utf8_lossy(&field[..field.iter().position(|b| *b == 0).unwrap_or(field.len())])
            .into_owned()
    }
    let mut out = BTreeMap::new();
    let mut at = 0;
    let mut long_name: Option<String> = None;
    while at + 512 <= bytes.len() {
        let h = &bytes[at..at + 512];
        if h.iter().all(|b| *b == 0) {
            break;
        }
        let size = octal(&h[124..136])?;
        let start = at + 512;
        let end = start
            .checked_add(size)
            .filter(|e| *e <= bytes.len())
            .ok_or("060 3.4.1: a truncated tar member")?;
        let body = &bytes[start..end];
        at = start + size.div_ceil(512) * 512;
        let mut name = text(&h[0..100]);
        if &h[257..262] == b"ustar" {
            let prefix = text(&h[345..500]);
            if !prefix.is_empty() {
                name = format!("{prefix}/{name}");
            }
        }
        match h[156] {
            b'L' => {
                long_name = Some(text(body));
                continue;
            }
            b'x' => {
                // PAX: a `path=` record replaces the next member's name.
                let records = String::from_utf8_lossy(body);
                long_name = records.lines().find_map(|r| {
                    r.split_once(' ')
                        .and_then(|(_, kv)| kv.strip_prefix("path="))
                        .map(str::to_string)
                });
                continue;
            }
            b'g' => continue,
            b'0' | 0 => {
                let name = long_name.take().unwrap_or(name);
                let name = name.strip_prefix("./").unwrap_or(&name).to_string();
                if out.insert(name.clone(), body).is_some() {
                    return Err(format!("060 3.4.1: {name} appears twice in the archive"));
                }
            }
            _ => {
                long_name = None;
            }
        }
    }
    Ok(out)
}

/// The descriptor of the one image manifest an OCI archive of `platform`
/// holds, with its blob and config checked: the manifest's bytes hash to
/// its digest and size, and the config names the platform.
pub fn platform_manifest(archive: &[u8], platform: &str) -> Result<ImagePlatform, String> {
    let refuse = |why: String| format!("060 3.4.1: {}: {why}", archive_name(platform));
    let files = tar_files(archive).map_err(refuse)?;
    let blob = |digest: &str| -> Result<&[u8], String> {
        if !digest_ok(digest) {
            return Err(refuse(format!("invalid digest {digest:?}")));
        }
        let bytes = files
            .get(&format!("blobs/sha256/{}", &digest[7..]))
            .ok_or_else(|| refuse(format!("no blob {digest}")))?;
        if format!("sha256:{}", sha256_hex(bytes)) != digest {
            return Err(refuse(format!(
                "the blob {digest} does not hash to its name"
            )));
        }
        Ok(bytes)
    };
    let index: serde_json::Value = serde_json::from_slice(
        files
            .get("index.json")
            .ok_or_else(|| refuse("no index.json".into()))?,
    )
    .map_err(|e| refuse(format!("index.json: {e}")))?;
    let [d] = index["manifests"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[])
    else {
        return Err(refuse("index.json must name exactly one manifest".into()));
    };
    let media_type = d["mediaType"].as_str().unwrap_or_default();
    if media_type != MANIFEST_MEDIA_TYPE {
        return Err(refuse(format!(
            "the manifest's media type is {media_type:?}"
        )));
    }
    let digest = d["digest"].as_str().unwrap_or_default().to_string();
    let size = d["size"]
        .as_u64()
        .ok_or_else(|| refuse("no manifest size".into()))?;
    let manifest = blob(&digest)?;
    if manifest.len() as u64 != size {
        return Err(refuse(
            "the manifest's size differs from its descriptor".into(),
        ));
    }
    let manifest: serde_json::Value =
        serde_json::from_slice(manifest).map_err(|e| refuse(format!("manifest: {e}")))?;
    let config: serde_json::Value = serde_json::from_slice(blob(
        manifest["config"]["digest"].as_str().unwrap_or_default(),
    )?)
    .map_err(|e| refuse(format!("config: {e}")))?;
    let named = format!(
        "{}/{}",
        config["os"].as_str().unwrap_or_default(),
        config["architecture"].as_str().unwrap_or_default()
    );
    if named != platform {
        return Err(refuse(format!("the image is for {named}")));
    }
    Ok(ImagePlatform {
        platform: platform.into(),
        variant: config["variant"].as_str().map(str::to_string),
        media_type: media_type.into(),
        digest,
        size,
        sbom: sbom_name(platform),
    })
}

/// The OCI image index's platform object, in the image-spec's order:
/// `os`, `architecture`, then `variant`.
#[derive(Serialize)]
struct IndexPlatform<'a> {
    os: &'a str,
    architecture: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    variant: Option<&'a str>,
}

#[derive(Serialize)]
struct IndexEntry<'a> {
    #[serde(rename = "mediaType")]
    media_type: &'a str,
    digest: &'a str,
    size: u64,
    platform: IndexPlatform<'a>,
}

#[derive(Serialize)]
struct Index<'a> {
    #[serde(rename = "schemaVersion")]
    schema_version: u8,
    #[serde(rename = "mediaType")]
    media_type: &'a str,
    manifests: Vec<IndexEntry<'a>>,
}

/// 060 3.3.2: the OCI index naming `platforms` in their order, as compact
/// JSON in field order: a pure function of the platform descriptors.
pub fn index_bytes(platforms: &[ImagePlatform]) -> Vec<u8> {
    let manifests = platforms
        .iter()
        .map(|p| {
            let (os, architecture) = p.platform.split_once('/').unwrap_or((&p.platform, ""));
            IndexEntry {
                media_type: &p.media_type,
                digest: &p.digest,
                size: p.size,
                platform: IndexPlatform {
                    os,
                    architecture,
                    variant: p.variant.as_deref(),
                },
            }
        })
        .collect();
    serde_json::to_vec(&Index {
        schema_version: 2,
        media_type: INDEX_MEDIA_TYPE,
        manifests,
    })
    .expect("an index always serializes")
}

/// The manifest entry for `version` from each platform's OCI archive, in
/// the fixed platform order.
pub fn release(version: &str, archives: &[(&str, &[u8])]) -> Result<ImageRelease, String> {
    let names: Vec<&str> = archives.iter().map(|(p, _)| *p).collect();
    if names != PLATFORMS {
        return Err(format!(
            "060 3.4.1: the image needs {} archives, in that order",
            PLATFORMS.join(" and ")
        ));
    }
    let platforms = archives
        .iter()
        .map(|(p, bytes)| platform_manifest(bytes, p))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ImageRelease {
        repository: REPOSITORY.into(),
        tag: version.into(),
        layer: LAYER,
        index_digest: format!("sha256:{}", sha256_hex(&index_bytes(&platforms))),
        platforms,
        slsa_build_level: SLSA_BUILD_LEVEL,
    })
}

/// 060 3.4.1: every field of a manifest's image, and its SBOM files among
/// the manifest's `sbom` artifacts (`sboms`).
pub fn validate(image: &ImageRelease, version: &str, sboms: &[&str]) -> Result<(), String> {
    let refuse = |why: &str| Err(format!("060 3.4.1: the manifest's image {why}"));
    if image.repository != REPOSITORY {
        return refuse(&format!("is not in {REPOSITORY}"));
    }
    if image.tag != version {
        return refuse(&format!("tag is not the family version {version}"));
    }
    if image.layer != LAYER || image.slsa_build_level != SLSA_BUILD_LEVEL {
        return refuse("must be layer 7 at SLSA build level 2");
    }
    let names: Vec<&str> = image
        .platforms
        .iter()
        .map(|p| p.platform.as_str())
        .collect();
    if names != PLATFORMS {
        return refuse(&format!(
            "must name {}, in that order",
            PLATFORMS.join(", ")
        ));
    }
    for p in &image.platforms {
        if !digest_ok(&p.digest) || p.size == 0 || p.media_type != MANIFEST_MEDIA_TYPE {
            return refuse(&format!("lacks {}'s manifest digest", p.platform));
        }
        if p.sbom != sbom_name(&p.platform) || !sboms.contains(&p.sbom.as_str()) {
            return refuse(&format!("lacks {}'s SBOM", p.platform));
        }
    }
    if !digest_ok(&image.index_digest)
        || image.index_digest != format!("sha256:{}", sha256_hex(&index_bytes(&image.platforms)))
    {
        return refuse("index digest is not the index of its platform digests");
    }
    Ok(())
}
