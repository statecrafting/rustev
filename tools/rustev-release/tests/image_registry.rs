//! Spec 060 3.3.3 and 3.4.2 against a fake registry: section 5's "Tag
//! `X.Y.Z` names another index digest" (the push fails, nothing moved) and
//! "The registry's index or platform digest differs from the manifest"
//! (`verify-release` fails).

use std::collections::BTreeMap;

use rustev_release::archive::sha256_hex;
use rustev_release::image::{self, ImagePlatform, ImageRelease};
use rustev_release::image_registry::{ImageRegistry, TagState, tag_state, verify};

#[derive(Default)]
struct Fake {
    /// Bytes by reference (tag or digest).
    refs: BTreeMap<String, Vec<u8>>,
    down: bool,
}

impl ImageRegistry for Fake {
    fn manifest(&mut self, repository: &str, reference: &str) -> Result<Option<Vec<u8>>, String> {
        assert_eq!(repository, image::REPOSITORY);
        if self.down {
            return Err("registry HTTP 503".into());
        }
        Ok(self.refs.get(reference).cloned())
    }
}

fn digest(b: &[u8]) -> String {
    format!("sha256:{}", sha256_hex(b))
}

fn platform(p: &str, body: &[u8]) -> ImagePlatform {
    ImagePlatform {
        platform: p.into(),
        variant: None,
        media_type: image::MANIFEST_MEDIA_TYPE.into(),
        digest: digest(body),
        size: body.len() as u64,
        sbom: image::sbom_name(p),
    }
}

const AMD: &[u8] = br#"{"schemaVersion":2,"config":"amd64"}"#;
const ARM: &[u8] = br#"{"schemaVersion":2,"config":"arm64"}"#;

fn image() -> ImageRelease {
    let platforms = vec![platform("linux/amd64", AMD), platform("linux/arm64", ARM)];
    ImageRelease {
        repository: image::REPOSITORY.into(),
        tag: "0.5.0".into(),
        layer: 7,
        index_digest: digest(&image::index_bytes(&platforms)),
        platforms,
        slsa_build_level: 2,
    }
}

/// The registry after a complete push of `image()`.
fn pushed() -> Fake {
    let i = image();
    let index = image::index_bytes(&i.platforms);
    let mut f = Fake::default();
    f.refs.insert(i.tag.clone(), index.clone());
    f.refs.insert(i.index_digest.clone(), index);
    f.refs.insert(i.platforms[0].digest.clone(), AMD.to_vec());
    f.refs.insert(i.platforms[1].digest.clone(), ARM.to_vec());
    f
}

#[test]
fn the_tag_is_absent_the_runs_index_or_refused() {
    let i = image();
    assert_eq!(
        tag_state(&mut Fake::default(), &i).unwrap(),
        TagState::Absent
    );
    assert_eq!(tag_state(&mut pushed(), &i).unwrap(), TagState::Same);
    let mut other = pushed();
    other.refs.insert(i.tag.clone(), b"{}".to_vec());
    assert_eq!(
        tag_state(&mut other, &i).unwrap(),
        TagState::Other(digest(b"{}"))
    );
    // An unreadable registry is no answer: the push does not proceed.
    let mut down = pushed();
    down.down = true;
    assert!(tag_state(&mut down, &i).is_err());
}

#[test]
fn verify_release_requires_the_manifests_index_and_platform_digests() {
    let i = image();
    assert!(verify(&mut pushed(), &i).is_empty());

    // The tag names another index.
    let mut f = pushed();
    f.refs.insert(i.tag.clone(), b"{\"other\":true}".to_vec());
    let v = verify(&mut f, &i);
    assert!(
        v.iter().any(|p| p.contains("not the manifest's index")),
        "{v:?}"
    );

    // The tag is absent.
    let mut f = pushed();
    f.refs.remove(&i.tag);
    assert!(verify(&mut f, &i).iter().any(|p| p.contains("is absent")));

    // A platform manifest is absent, or differs from its digest.
    let mut f = pushed();
    f.refs.remove(&i.platforms[1].digest);
    let v = verify(&mut f, &i);
    assert!(
        v.iter()
            .any(|p| p.contains("linux/arm64's manifest") && p.contains("absent")),
        "{v:?}"
    );
    let mut f = pushed();
    f.refs.insert(i.platforms[0].digest.clone(), ARM.to_vec());
    let v = verify(&mut f, &i);
    assert!(
        v.iter()
            .any(|p| p.contains("linux/amd64's manifest differs")),
        "{v:?}"
    );

    // A manifest whose platform digests differ from the registry's index.
    let mut changed = i.clone();
    changed.platforms[1] = platform("linux/arm64", b"{\"rebuilt\":1}");
    changed.index_digest = digest(&image::index_bytes(&changed.platforms));
    let v = verify(&mut pushed(), &changed);
    assert!(
        v.iter().any(|p| p.contains("not the manifest's index")),
        "{v:?}"
    );
    assert!(
        v.iter()
            .any(|p| p.contains("linux/arm64's manifest") && p.contains("absent")),
        "{v:?}"
    );

    // A failed lookup is a failure, never a pass.
    let mut down = pushed();
    down.down = true;
    assert_eq!(verify(&mut down, &i).len(), 3);
}
