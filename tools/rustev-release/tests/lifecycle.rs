//! Spec 021 acceptance: "A dry-run state machine covers every lifecycle
//! transition and section 4 row without credentials, network writes, tag
//! creation or publication" and "A local fake-registry harness tests
//! topological upload, propagation waits, refusal, ambiguity, partial
//! publication and immutable-version recovery" (3.1, 3.9, section 4).

use std::collections::BTreeMap;

use rustev_release::ReleaseSet;
use rustev_release::archive::sha256_hex;
use rustev_release::lifecycle::{
    Entry, Event, FakeRegistry, Instruction, Journal, Lifecycle, Lookup, Outcome, State, Upload,
    VISIBILITY_POLLS, publish,
};
use rustev_release::manifest::{Inputs, ReleaseManifest, Toolchain, build};

const SET: &str = include_str!("../release-set.json");

/// The real release set, with a synthetic archive per member.
fn fixture() -> (ReleaseManifest, BTreeMap<String, Vec<u8>>) {
    let set = ReleaseSet::parse(SET.as_bytes()).unwrap();
    // One directory per fixture: tests run in parallel, and a shared one
    // would be rewritten while another test reads it.
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("rustev-release-life-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut archives = BTreeMap::new();
    for name in &set.upload_order {
        let file = format!("{name}-{}.crate", set.version);
        let bytes = format!("archive of {name}").into_bytes();
        std::fs::write(dir.join(&file), &bytes).unwrap();
        archives.insert(file, bytes);
    }
    let m = build(
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
    (m, archives)
}

fn instruction(m: &ReleaseManifest) -> Instruction {
    Instruction {
        version: m.version.clone(),
        commit: m.commit.clone(),
        registry: m.registry.clone(),
        packages: m.upload_order.clone(),
    }
}

fn authorized() -> (Lifecycle, BTreeMap<String, Vec<u8>>) {
    let (m, a) = fixture();
    // The tests address the first five packages by position.
    assert!(
        m.upload_order.len() >= 5,
        "the tests address the first five packages of the release set"
    );
    let mut l = Lifecycle::new(m.clone());
    l.apply(Event::GatesPass).unwrap();
    l.apply(Event::CandidateVerified {
        commit: m.commit.clone(),
    })
    .unwrap();
    l.apply(Event::Authorized(instruction(&m))).unwrap();
    (l, a)
}

#[test]
fn every_transition_in_order_reaches_qualified() {
    let (mut l, a) = authorized();
    assert_eq!(l.state, State::PublicationAuthorized);
    let mut reg = FakeRegistry::default();
    let mut j = Journal::default();
    assert_eq!(publish(&mut l, &a, &mut reg, &mut j), Outcome::Complete);
    assert_eq!(l.state, State::Published);
    assert!(
        !l.publicly_accessible(),
        "published without consumer proof is not qualified"
    );
    l.apply(Event::ReleaseCreated).unwrap();
    assert!(!l.publicly_accessible());
    l.apply(Event::ConsumerQualified).unwrap();
    assert!(l.publicly_accessible());
    // Upload order is the manifest's, which is topological (021 2).
    assert_eq!(reg.uploads, l.manifest.upload_order);
}

#[test]
fn no_state_is_skipped_or_reported_as_another() {
    let (m, _) = fixture();
    let mut l = Lifecycle::new(m.clone());
    for e in [
        Event::CandidateVerified {
            commit: m.commit.clone(),
        },
        Event::Authorized(instruction(&m)),
        Event::Observed { complete: true },
        Event::ReleaseCreated,
        Event::ConsumerQualified,
    ] {
        assert!(l.apply(e).is_err());
        assert_eq!(l.state, State::Specified);
    }
    l.apply(Event::GatesPass).unwrap();
    assert!(
        l.apply(Event::Authorized(instruction(&m))).is_err(),
        "authorization needs a candidate"
    );
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
}

#[test]
fn local_packaging_before_authorization_uploads_nothing() {
    // Section 4 row 1.
    let (m, a) = fixture();
    let mut l = Lifecycle::new(m.clone());
    l.apply(Event::GatesPass).unwrap();
    l.apply(Event::CandidateVerified {
        commit: m.commit.clone(),
    })
    .unwrap();
    let mut reg = FakeRegistry::default();
    let mut j = Journal::default();
    assert!(matches!(
        publish(&mut l, &a, &mut reg, &mut j),
        Outcome::Stopped { .. }
    ));
    assert!(reg.uploads.is_empty());
    assert_eq!(l.state, State::CandidateVerified);
}

#[test]
fn an_instruction_must_name_exactly_this_release() {
    let (m, _) = fixture();
    for change in 0..4 {
        let mut l = Lifecycle::new(m.clone());
        l.apply(Event::GatesPass).unwrap();
        l.apply(Event::CandidateVerified {
            commit: m.commit.clone(),
        })
        .unwrap();
        let mut i = instruction(&m);
        match change {
            0 => i.version = "0.1.1".into(),
            1 => i.commit = "d".repeat(40),
            2 => i.registry = "another".into(),
            _ => {
                i.packages.pop();
            }
        }
        assert!(l.apply(Event::Authorized(i)).is_err(), "change {change}");
        assert_eq!(l.state, State::CandidateVerified);
    }
}

#[test]
fn each_upload_waits_for_visibility_with_the_expected_checksum() {
    let (mut l, a) = authorized();
    let mut reg = FakeRegistry {
        lag: 3,
        ..Default::default()
    };
    let mut j = Journal::default();
    assert_eq!(publish(&mut l, &a, &mut reg, &mut j), Outcome::Complete);
    assert_eq!(
        reg.now,
        3 * l.manifest.packages.len() as u64,
        "three waits per package"
    );
    // Journal: attempt before result, then the observed checksum.
    let first = &l.manifest.packages[0];
    assert_eq!(
        &j.entries[..3],
        &[
            Entry::Attempt {
                package: first.name.clone(),
                version: l.manifest.version.clone(),
                sha256: first.sha256.clone(),
                registry: l.manifest.registry.clone(),
            },
            Entry::Result {
                package: first.name.clone(),
                outcome: Upload::Accepted
            },
            Entry::Query {
                package: first.name.clone(),
                observed: Lookup::Visible {
                    checksum: first.sha256.clone()
                },
            },
        ]
    );
    assert_eq!(Journal::from_lines(&j.to_lines()).unwrap(), j);
}

#[test]
fn an_accepted_upload_that_never_appears_stops() {
    let (mut l, a) = authorized();
    let mut reg = FakeRegistry {
        lag: u64::from(VISIBILITY_POLLS) + 5,
        ..Default::default()
    };
    let mut j = Journal::default();
    assert!(
        matches!(publish(&mut l, &a, &mut reg, &mut j), Outcome::Stopped { ref reason } if reason.contains("not visible"))
    );
    assert_eq!(reg.uploads.len(), 1, "the next layer never starts");
    assert_eq!(
        l.state,
        State::PublicationAuthorized,
        "nothing is confirmed public"
    );
}

#[test]
fn a_refusal_stops_and_records_it() {
    let (mut l, a) = authorized();
    let first = l.manifest.upload_order[0].clone();
    let mut reg = FakeRegistry::default();
    reg.script.insert(
        first.clone(),
        Upload::Refused {
            reason: "name taken".into(),
        },
    );
    let mut j = Journal::default();
    assert!(matches!(
        publish(&mut l, &a, &mut reg, &mut j),
        Outcome::Stopped { .. }
    ));
    assert_eq!(reg.uploads, [first]);
    assert_eq!(l.state, State::PublicationAuthorized, "nothing is public");
    assert!(matches!(j.entries.last(), Some(Entry::Stopped { .. })));
}

#[test]
fn four_public_and_the_fifth_failing_is_partially_published() {
    // Section 4 row 7.
    let (mut l, a) = authorized();
    let fifth = l.manifest.upload_order[4].clone();
    let mut reg = FakeRegistry::default();
    reg.script.insert(
        fifth.clone(),
        Upload::Refused {
            reason: "server error".into(),
        },
    );
    let mut j = Journal::default();
    assert!(matches!(
        publish(&mut l, &a, &mut reg, &mut j),
        Outcome::Stopped { .. }
    ));
    assert_eq!(l.state, State::PartiallyPublished);
    assert!(l.apply(Event::ReleaseCreated).is_err(), "never released");
    assert!(!l.publicly_accessible());
}

#[test]
fn an_ambiguous_upload_queries_public_state_records_it_and_stops() {
    // Section 4 row 6: query, record, stop; never a blind retry. The
    // ambiguous package is the first (nothing public before it) or the
    // second (one package public), and the query finds it absent or
    // accepted.
    for (position, accepted) in [(0, false), (0, true), (1, false), (1, true)] {
        let (mut l, a) = authorized();
        let second = l.manifest.upload_order[position].clone();
        let mut reg = FakeRegistry::default();
        reg.script.insert(
            second.clone(),
            Upload::Ambiguous {
                detail: "timeout".into(),
            },
        );
        if accepted {
            reg.ambiguous_accepts.push(second.clone());
        }
        let mut j = Journal::default();
        assert!(matches!(
            publish(&mut l, &a, &mut reg, &mut j),
            Outcome::Stopped { .. }
        ));
        assert_eq!(reg.uploads.iter().filter(|u| **u == second).count(), 1);
        assert!(
            j.entries
                .iter()
                .any(|e| matches!(e, Entry::Query { package, .. } if *package == second))
        );
        let public = position + usize::from(accepted);
        assert_eq!(
            l.state,
            if public == 0 {
                State::PublicationAuthorized
            } else {
                State::PartiallyPublished
            },
            "{public} package(s) public"
        );

        // A rerun never uploads the ambiguous package blindly: it is either
        // already public with the same checksum (skipped) or, having been
        // found absent, attempted once more after that recorded query.
        reg.script.remove(&second);
        let before = reg.uploads.len();
        assert_eq!(publish(&mut l, &a, &mut reg, &mut j), Outcome::Complete);
        let again = reg.uploads[before..]
            .iter()
            .filter(|u| **u == second)
            .count();
        assert_eq!(again, usize::from(!accepted));
        assert_eq!(l.state, State::Published);
    }
}

#[test]
fn an_ambiguous_attempt_without_a_later_absent_query_blocks_a_retry() {
    let (mut l, a) = authorized();
    let first = l.manifest.upload_order[0].clone();
    let mut j = Journal::default();
    j.entries.push(Entry::Result {
        package: first,
        outcome: Upload::Ambiguous {
            detail: "lost".into(),
        },
    });
    let mut reg = FakeRegistry::default();
    // A journal from an earlier run with an ambiguous result and no query:
    // the publisher's own lookup finds it absent but the journal still
    // demands the recorded query first, so it stops before uploading.
    let outcome = publish(&mut l, &a, &mut reg, &mut j);
    assert!(
        matches!(outcome, Outcome::Stopped { ref reason } if reason.contains("query public state first"))
    );
    assert!(reg.uploads.is_empty());
}

#[test]
fn a_rerun_skips_public_packages_and_never_reuses_a_version() {
    // Immutable-version recovery: the same bytes are skipped; different
    // bytes under a public version stop the run (a new version is needed).
    let (mut l, a) = authorized();
    let third = l.manifest.upload_order[2].clone();
    let mut reg = FakeRegistry::default();
    reg.script.insert(
        third.clone(),
        Upload::Refused {
            reason: "transient".into(),
        },
    );
    let mut j = Journal::default();
    publish(&mut l, &a, &mut reg, &mut j);
    reg.script.clear();
    assert_eq!(publish(&mut l, &a, &mut reg, &mut j), Outcome::Complete);
    assert_eq!(
        reg.uploads
            .iter()
            .filter(|u| **u == l.manifest.upload_order[0])
            .count(),
        1
    );

    let (mut l2, mut a2) = authorized();
    let first = l2.manifest.packages[0].clone();
    a2.insert(first.crate_file.clone(), b"a rebuilt archive".to_vec());
    let mut reg2 = FakeRegistry::default();
    let mut j2 = Journal::default();
    assert!(
        matches!(publish(&mut l2, &a2, &mut reg2, &mut j2), Outcome::Stopped { ref reason } if reason.contains("never upload a rebuild"))
    );
    assert!(reg2.uploads.is_empty());

    let (mut l3, a3) = authorized();
    let mut reg3 = FakeRegistry::default();
    reg3.index.insert(
        (first.name.clone(), l3.manifest.version.clone()),
        (sha256_hex(b"someone else's bytes"), 0),
    );
    let mut j3 = Journal::default();
    assert!(
        matches!(publish(&mut l3, &a3, &mut reg3, &mut j3), Outcome::Stopped { ref reason } if reason.contains("the version is spent"))
    );
    assert!(reg3.uploads.is_empty());
}

#[test]
fn a_consumer_failure_is_never_qualified() {
    // Section 4 rows 8 and 9: only the consumer event qualifies, and only
    // after the release exists.
    let (mut l, a) = authorized();
    publish(
        &mut l,
        &a,
        &mut FakeRegistry::default(),
        &mut Journal::default(),
    );
    assert!(l.apply(Event::ConsumerQualified).is_err(), "no release yet");
    l.apply(Event::ReleaseCreated).unwrap();
    assert_eq!(l.state, State::Released);
    assert!(!l.publicly_accessible());
}

/// A registry that stores other bytes than it was given.
struct Corrupting(FakeRegistry);

impl rustev_release::lifecycle::Registry for Corrupting {
    fn lookup(&mut self, name: &str, version: &str) -> Lookup {
        self.0.lookup(name, version)
    }
    fn upload(&mut self, name: &str, version: &str, _archive: &[u8]) -> Upload {
        self.0.upload(name, version, b"other bytes")
    }
    fn wait(&mut self) {
        self.0.wait()
    }
}

#[test]
fn a_first_package_public_with_other_bytes_is_partially_published() {
    let (mut l, a) = authorized();
    let mut reg = Corrupting(FakeRegistry::default());
    let mut j = Journal::default();
    assert!(
        matches!(publish(&mut l, &a, &mut reg, &mut j), Outcome::Stopped { ref reason } if reason.contains("different checksum"))
    );
    assert_eq!(
        l.state,
        State::PartiallyPublished,
        "something is public, and it is not the release"
    );
}

#[test]
fn registry_text_is_redacted_before_the_journal() {
    let (mut l, a) = authorized();
    let mut reg = FakeRegistry::default();
    let leaked = format!("bad token cio{}", "A".repeat(32));
    let first = l.manifest.upload_order[0].clone();
    reg.script.insert(first, Upload::Refused { reason: leaked });
    let mut j = Journal::default();
    publish(&mut l, &a, &mut reg, &mut j);
    let lines = j.to_lines();
    assert!(!lines.contains(&"A".repeat(32)), "{lines}");
    assert!(lines.contains("bad token [redacted]"), "{lines}");
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
fn a_later_refusal_never_clears_an_ambiguous_attempt() {
    let (mut l, a) = authorized();
    let first = l.manifest.upload_order[0].clone();
    let mut j = Journal::default();
    for outcome in [
        Upload::Ambiguous {
            detail: "lost".into(),
        },
        Upload::Refused {
            reason: "later".into(),
        },
    ] {
        j.entries.push(Entry::Result {
            package: first.clone(),
            outcome,
        });
    }
    let mut reg = FakeRegistry::default();
    let outcome = publish(&mut l, &a, &mut reg, &mut j);
    assert!(
        matches!(outcome, Outcome::Stopped { ref reason } if reason.contains("query public state first"))
    );
    assert!(reg.uploads.is_empty());
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
