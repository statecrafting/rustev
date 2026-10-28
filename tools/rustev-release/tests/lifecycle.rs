//! Spec 021 acceptance: "A dry-run state machine covers every lifecycle
//! transition and section 4 row without credentials, network writes, tag
//! creation or publication" (3.1), with the verified tag of spec 058 3.1.2
//! as the owner's authority. Registry text is redacted (3.9).

use std::collections::BTreeMap;

use rustev_release::ReleaseSet;
use rustev_release::lifecycle::{Event, FIRST_RELEASE_VERSION, Lifecycle, State};
use rustev_release::manifest::{Inputs, ReleaseManifest, Toolchain, build};

const SET: &str = include_str!("../release-set.json");
const TAG_OBJECT: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

/// The real release set, with a synthetic archive per member.
fn fixture() -> ReleaseManifest {
    let mut set = ReleaseSet::parse(SET.as_bytes()).unwrap();
    set.version = "0.3.0".into();
    set.tag = format!("v{}", set.version);
    // One directory per fixture: tests run in parallel, and a shared one
    // would be rewritten while another test reads it.
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("rustev-release-life-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for name in &set.upload_order {
        let file = format!("{name}-{}.crate", set.version);
        std::fs::write(dir.join(&file), format!("archive of {name}")).unwrap();
    }
    let mut m = build(
        &set,
        &dir,
        Inputs {
            commit: "c".repeat(40),
            tree: "t".repeat(40),
            toolchain: Toolchain {
                rustc: "rustc".into(),
                cargo: "cargo".into(),
                host: "host".into(),
                audit: "cargo-deny 0.20.2".into(),
            },
            source_date_epoch: 0,
            evidence: BTreeMap::new(),
            limitations: vec![],
        },
    )
    .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    m.tag_object = Some(TAG_OBJECT.into());
    m
}

fn authorized(m: &ReleaseManifest) -> Lifecycle {
    let mut l = Lifecycle::new(m.clone());
    l.apply(Event::GatesPass).unwrap();
    l.apply(Event::CandidateVerified {
        commit: m.commit.clone(),
    })
    .unwrap();
    l.apply(Event::TagAuthorized {
        tag_object: TAG_OBJECT.into(),
    })
    .unwrap();
    l
}

#[test]
fn every_transition_in_order_reaches_qualified() {
    let mut l = authorized(&fixture());
    assert_eq!(l.state, State::PublicationAuthorized);
    l.apply(Event::Observed { complete: false }).unwrap();
    assert_eq!(l.state, State::PartiallyPublished);
    l.apply(Event::Observed { complete: true }).unwrap();
    assert_eq!(l.state, State::Published);
    assert!(
        !l.publicly_accessible(),
        "published without consumer proof is not qualified"
    );
    l.apply(Event::ReleaseCreated).unwrap();
    assert_eq!(l.state, State::Released);
    assert!(!l.publicly_accessible());
    l.apply(Event::ConsumerQualified).unwrap();
    assert!(l.publicly_accessible());
}

#[test]
fn no_state_is_skipped_or_reported_as_another() {
    let m = fixture();
    let mut l = Lifecycle::new(m.clone());
    let tag = || Event::TagAuthorized {
        tag_object: TAG_OBJECT.into(),
    };
    for e in [
        Event::CandidateVerified {
            commit: m.commit.clone(),
        },
        tag(),
        Event::Observed { complete: true },
        Event::ReleaseCreated,
        Event::ConsumerQualified,
    ] {
        assert!(l.apply(e).is_err());
        assert_eq!(l.state, State::Specified);
    }
    l.apply(Event::GatesPass).unwrap();
    assert!(l.apply(tag()).is_err(), "authorization needs a candidate");
    assert!(
        l.apply(Event::CandidateVerified {
            commit: "d".repeat(40)
        })
        .is_err(),
        "another commit"
    );
    l.apply(Event::CandidateVerified {
        commit: m.commit.clone(),
    })
    .unwrap();
    assert!(l.apply(Event::ReleaseCreated).is_err());
    assert_eq!(l.state, State::CandidateVerified);
}

#[test]
fn only_the_manifests_tag_object_authorizes_and_never_for_the_first_release() {
    // 058 3.1.2: another tag object, or a manifest naming none, is refused.
    let mut other = fixture();
    other.tag_object = None;
    for (m, object) in [(fixture(), "0".repeat(40)), (other, TAG_OBJECT.into())] {
        let mut l = Lifecycle::new(m.clone());
        l.apply(Event::GatesPass).unwrap();
        l.apply(Event::CandidateVerified { commit: m.commit })
            .unwrap();
        let refusal = l
            .apply(Event::TagAuthorized { tag_object: object })
            .unwrap_err();
        assert!(refusal.0.contains("058 3.1.2"), "{refusal:?}");
        assert_eq!(l.state, State::CandidateVerified);
    }
    // 031 3.11.1 and 058 3.6: 0.1.0's authority is spent.
    let mut first = fixture();
    first.version = FIRST_RELEASE_VERSION.into();
    let mut l = Lifecycle::new(first.clone());
    l.apply(Event::GatesPass).unwrap();
    l.apply(Event::CandidateVerified {
        commit: first.commit,
    })
    .unwrap();
    assert!(
        l.apply(Event::TagAuthorized {
            tag_object: TAG_OBJECT.into()
        })
        .is_err()
    );
}

#[test]
fn redaction_keeps_the_structure_around_a_credential() {
    let t = "A".repeat(32);
    assert_eq!(
        rustev_release::lifecycle::redact(&format!("{{\"errors\":[{{\"detail\":cio{t}}}]}}")),
        "{\"errors\":[{\"detail\":[redacted]}]}"
    );
}

#[test]
fn redaction_covers_a_private_key_body() {
    let key = "-----BEGIN RSA PRIVATE KEY-----\nMIIEsecretbody\n-----END RSA PRIVATE KEY-----";
    let out = rustev_release::lifecycle::redact(&format!("rejected: {key} (bad)"));
    assert_eq!(out, "rejected: [redacted] (bad)");
    let cut = rustev_release::lifecycle::redact("x -----BEGIN PRIVATE KEY-----\nMIIEsecret");
    assert_eq!(cut, "x [redacted]");
}

#[test]
fn redaction_never_splits_a_multibyte_character() {
    // Every scan rule begins and every token ends at an ASCII byte (or the
    // end of the text), so the splice points are character boundaries even
    // in non-ASCII registry text.
    let t = "A".repeat(32);
    let text = format!("é «cio{t}» ünd -----BEGIN PRIVATE KEY-----ß\n日本");
    assert_eq!(
        rustev_release::lifecycle::redact(&text),
        "é «[redacted] ünd [redacted]"
    );
    assert_eq!(
        rustev_release::lifecycle::redact("日本語 only"),
        "日本語 only"
    );
}
