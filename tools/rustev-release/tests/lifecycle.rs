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
    let mut set = ReleaseSet::parse(SET.as_bytes()).unwrap();
    // Spec 031 3.11.1: publication authorizes exactly the first release, so
    // the publication path runs on the release set at that version. A later
    // version's authority (056 3.4) is tested on its own.
    set.version = rustev_release::lifecycle::FIRST_RELEASE_VERSION.into();
    set.tag = format!("v{}", set.version);
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
        account: rustev_release::lifecycle::FAKE_ACCOUNT.into(),
        registries: m.registries.clone(),
        artifacts: vec![],
        continuation: false,
        action: "publish".into(),
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

// Spec 031 3.11.2: fake signatures still bind exact manifest bytes.
fn signed_journal(m: &ReleaseManifest) -> Journal {
    let mut j = Journal::default();
    rustev_release::family_lifecycle::bind(m, "fixture signature", &mut j).unwrap();
    j
}

#[test]
fn every_transition_in_order_reaches_qualified() {
    let (mut l, a) = authorized();
    assert_eq!(l.state, State::PublicationAuthorized);
    let mut reg = FakeRegistry::default();
    let mut j = signed_journal(&l.manifest);
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
    let mut j = signed_journal(&l.manifest);
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
    let mut j = signed_journal(&l.manifest);
    assert_eq!(publish(&mut l, &a, &mut reg, &mut j), Outcome::Complete);
    assert_eq!(
        reg.now,
        3 * l.manifest.packages.len() as u64,
        "three waits per package"
    );
    // Journal: the name's owners (none yet), attempt before result, the
    // observed checksum, then the owners again, now the uploading account.
    let first = &l.manifest.packages[0];
    assert_eq!(
        &j.entries[1..6],
        &[
            Entry::Owners {
                package: first.name.clone(),
                owners: vec![],
            },
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
            Entry::Owners {
                package: first.name.clone(),
                owners: vec![rustev_release::lifecycle::FAKE_ACCOUNT.into()],
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
    let mut j = signed_journal(&l.manifest);
    assert!(
        matches!(publish(&mut l, &a, &mut reg, &mut j), Outcome::Stopped { ref reason } if reason.contains("not visible"))
    );
    assert_eq!(reg.uploads.len(), 1, "the next layer never starts");
    assert_eq!(
        l.state,
        State::PartiallyPublished,
        "acceptance makes this a partial publication even before visibility"
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
    let mut j = signed_journal(&l.manifest);
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
    let mut j = signed_journal(&l.manifest);
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
        let mut j = signed_journal(&l.manifest);
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
    let mut j = signed_journal(&l.manifest);
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
    let mut j = signed_journal(&l.manifest);
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
    let mut j2 = signed_journal(&l2.manifest);
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
    let mut j3 = signed_journal(&l3.manifest);
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
    let mut j = signed_journal(&l.manifest);
    publish(&mut l, &a, &mut FakeRegistry::default(), &mut j);
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
    fn owners(&mut self, name: &str) -> Result<Vec<String>, String> {
        self.0.owners(name)
    }
    fn wait(&mut self) {
        self.0.wait()
    }
}

#[test]
fn a_first_package_public_with_other_bytes_is_partially_published() {
    let (mut l, a) = authorized();
    let mut reg = Corrupting(FakeRegistry::default());
    let mut j = signed_journal(&l.manifest);
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
    let mut j = signed_journal(&l.manifest);
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
    let mut j = signed_journal(&l.manifest);
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

#[test]
fn a_journal_write_failure_is_the_one_stop_entry() {
    let (mut l, a) = authorized();
    let first = l.manifest.packages[0].clone();
    let mut reg = FakeRegistry::default();
    reg.index.insert(
        (first.name.clone(), l.manifest.version.clone()),
        (sha256_hex(b"someone else's bytes"), 0),
    );
    // A directory is not appendable: the first record fails.
    let dir = std::env::temp_dir().join(format!("rustev-journal-dir-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut j = Journal {
        file: Some(dir.clone()),
        ..signed_journal(&l.manifest)
    };
    let out = publish(&mut l, &a, &mut reg, &mut j);
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(
        matches!(out, Outcome::Stopped { ref reason } if reason.contains("the version is spent"))
    );
    let stops: Vec<_> = j
        .entries
        .iter()
        .filter(|e| matches!(e, Entry::Stopped { .. }))
        .collect();
    assert_eq!(stops.len(), 1, "{stops:?}");
    assert!(matches!(stops[0], Entry::Stopped { reason } if reason.contains("not writable")));
    assert!(reg.uploads.is_empty());
}

#[test]
fn a_stop_that_cannot_be_written_is_still_one_stop_entry() {
    let dir = std::env::temp_dir().join(format!("rustev-journal-stop-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut j = Journal {
        file: Some(dir.clone()),
        ..Journal::default()
    };
    j.record(Entry::Stopped {
        reason: "an empty package set publishes nothing".into(),
    });
    std::fs::remove_dir_all(&dir).unwrap();
    let stops: Vec<_> = j
        .entries
        .iter()
        .filter_map(|e| match e {
            Entry::Stopped { reason } => Some(reason.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(stops.len(), 1, "{stops:?}");
    assert!(stops[0].contains("publishes nothing") && stops[0].contains("not writable"));
}

// Spec 021 3.9 account identity: crates.io names no API token's account,
// so every name's public owners must include the instruction's login.

#[test]
fn an_instruction_without_a_registry_account_is_refused() {
    let (m, _) = fixture();
    for account in ["", "bad login", "a/b"] {
        let mut l = Lifecycle::new(m.clone());
        l.apply(Event::GatesPass).unwrap();
        l.apply(Event::CandidateVerified {
            commit: m.commit.clone(),
        })
        .unwrap();
        let mut i = instruction(&m);
        i.account = account.into();
        assert!(l.apply(Event::Authorized(i)).is_err(), "{account:?}");
        assert_eq!(l.state, State::CandidateVerified);
    }
    let text = r#"{"version":"0.1.0","commit":"c","registry":"crates-io","packages":[]}"#;
    assert!(
        serde_json::from_str::<Instruction>(text).is_err(),
        "the account is never defaulted"
    );
}

#[test]
fn a_name_owned_by_another_account_is_never_uploaded() {
    let (mut l, a) = authorized();
    let mut reg = FakeRegistry::default();
    let first = l.manifest.upload_order[0].clone();
    reg.owners
        .insert(first.clone(), vec!["someone-else".into()]);
    let mut j = signed_journal(&l.manifest);
    assert!(
        matches!(publish(&mut l, &a, &mut reg, &mut j), Outcome::Stopped { ref reason } if reason.contains("someone-else"))
    );
    assert!(reg.uploads.is_empty());
    assert_eq!(l.state, State::PublicationAuthorized, "nothing is public");
    assert!(j.entries.iter().any(|e| matches!(
        e,
        Entry::Owners { package, owners } if *package == first && owners == &["someone-else"]
    )));
}

#[test]
fn an_upload_made_as_another_account_stops_before_the_next_package() {
    let (mut l, a) = authorized();
    let mut reg = FakeRegistry {
        uploader: Some("someone-else".into()),
        ..Default::default()
    };
    let mut j = signed_journal(&l.manifest);
    assert!(
        matches!(publish(&mut l, &a, &mut reg, &mut j), Outcome::Stopped { ref reason } if reason.contains("not the instruction's account"))
    );
    assert_eq!(reg.uploads, l.manifest.upload_order[..1]);
    assert_eq!(l.state, State::PartiallyPublished);
}

/// A registry whose owners cannot be read.
struct NoOwners(FakeRegistry);

impl rustev_release::lifecycle::Registry for NoOwners {
    fn lookup(&mut self, name: &str, version: &str) -> Lookup {
        self.0.lookup(name, version)
    }
    fn upload(&mut self, name: &str, version: &str, archive: &[u8]) -> Upload {
        self.0.upload(name, version, archive)
    }
    fn owners(&mut self, _: &str) -> Result<Vec<String>, String> {
        Err("owners HTTP 503".into())
    }
    fn wait(&mut self) {
        self.0.wait()
    }
}

#[test]
fn unknown_owners_are_never_taken_for_none() {
    let (mut l, a) = authorized();
    let mut reg = NoOwners(FakeRegistry::default());
    let mut j = signed_journal(&l.manifest);
    assert!(
        matches!(publish(&mut l, &a, &mut reg, &mut j), Outcome::Stopped { ref reason } if reason.contains("unknown"))
    );
    assert!(reg.0.uploads.is_empty());
}

#[test]
fn every_published_name_is_checked_against_the_account() {
    let (mut l, a) = authorized();
    let mut reg = FakeRegistry::default();
    let mut j = signed_journal(&l.manifest);
    assert_eq!(publish(&mut l, &a, &mut reg, &mut j), Outcome::Complete);
    for name in &l.manifest.upload_order {
        let checks: Vec<_> = j
            .entries
            .iter()
            .filter(|e| matches!(e, Entry::Owners { package, .. } if package == name))
            .collect();
        // Once before the upload (no owners yet) and once when visible.
        assert_eq!(checks.len(), 2, "{name}");
    }
}
