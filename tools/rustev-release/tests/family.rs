//! Spec 031 3.1, 3.7, 3.9 and 3.11: no network or credentials.
use rustev_release::{
    ReleaseSet,
    archive::sha256_hex,
    family_artifacts as fa, family_lifecycle as fl,
    lifecycle::{Entry, Event, Instruction, Journal, Lifecycle, Lookup, State, Upload},
    manifest::{self, Inputs, ReleaseManifest, Toolchain},
    repro::{Scratch, fresh_dir},
};
use std::{collections::BTreeMap, path::Path};

fn write(path: &Path, b: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, b).unwrap();
}
fn fixture() -> (
    Scratch,
    ReleaseSet,
    ReleaseManifest,
    BTreeMap<String, Vec<u8>>,
) {
    let root = fresh_dir("rustev-family-test").unwrap();
    let mut set = ReleaseSet::parse(include_bytes!("../release-set.json")).unwrap();
    set.upload_order = vec!["rustev-contract".into()];
    set.layers = vec![set.upload_order.clone()];
    set.npm = vec![set.npm.last().unwrap().clone()];
    let evidence = root.join("evidence");
    write(&evidence.join("rustev-contract-0.1.0.crate"), b"crate");
    write(
        &evidence.join("sbom-rustev-contract.cdx.json"),
        br#"{"bomFormat":"CycloneDX","specVersion":"1.5"}"#,
    );
    write(&evidence.join("changelog.md"), b"release notes");
    let artifacts = root.join("artifacts");
    let mut names = vec![("npm", "statecrafting-rustev-wasm-0.1.0.tgz".to_string())];
    // Spec 031 3.1: the release includes all seven native CLI platforms.
    for p in fa::PLATFORMS {
        names.push(("cli", format!("rustev-0.1.0-{p}.tar.gz")));
    }
    for (kind, name) in names {
        write(&artifacts.join(kind).join(&name), name.as_bytes());
        write(
            &artifacts.join("sbom").join(format!("{name}.cdx.json")),
            br#"{"bomFormat":"CycloneDX","specVersion":"1.5"}"#,
        );
        if kind == "cli" {
            write(
                &artifacts.join(kind).join(format!("{name}.sha256")),
                format!("{}  {name}\n", sha256_hex(name.as_bytes())).as_bytes(),
            );
        }
    }
    let mut m = manifest::build(
        &set,
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
    )
    .unwrap();
    m.artifacts = fa::collect(&set, &evidence, Some(&artifacts)).unwrap();
    m.msrv = "1.88".into();
    m.edition = "2024".into();
    m.plan_ids.insert("reference".into(), "plan-id".into());
    fa::stage(&m, &evidence, Some(&artifacts)).unwrap();
    let mut files = BTreeMap::new();
    for n in m
        .packages
        .iter()
        .map(|p| &p.crate_file)
        .chain(m.artifacts.iter().map(|a| &a.name))
    {
        files.insert(n.clone(), std::fs::read(evidence.join(n)).unwrap());
    }
    files.insert("release-manifest.json".into(), m.canonical());
    files.insert("SHA256SUMS".into(), manifest::sha256sums(&m).into_bytes());
    for n in ["release-manifest.json.sig", "SHA256SUMS.sig"] {
        files.insert(n.into(), b"fixture signature".to_vec());
    }
    files.insert("journal-summary.jsonl".into(), b"fixture journal".to_vec());
    (root, set, m, files)
}
fn instruction(m: &ReleaseManifest) -> Instruction {
    Instruction {
        version: m.version.clone(),
        commit: m.commit.clone(),
        registry: m.registry.clone(),
        packages: m.upload_order.clone(),
        registries: m.registries.clone(),
        artifacts: m.artifacts.iter().map(|a| a.name.clone()).collect(),
        continuation: false,
        action: "publish".into(),
    }
}
fn authorized(m: &ReleaseManifest) -> Lifecycle {
    let mut l = Lifecycle::new(m.clone());
    l.apply(Event::GatesPass).unwrap();
    l.apply(Event::CandidateVerified {
        commit: m.commit.clone(),
    })
    .unwrap();
    l.apply(Event::Authorized(instruction(m))).unwrap();
    l
}
fn journal(m: &ReleaseManifest) -> Journal {
    let mut j = Journal::default();
    fl::bind(m, "fixture signature", &mut j).unwrap();
    j
}

#[test]
fn manifest_two_is_deterministic_complete_and_refuses_old_schema() {
    let (root, set, m, _) = fixture();
    let mut again = m.clone();
    again.artifacts =
        fa::collect(&set, &root.join("evidence"), Some(&root.join("artifacts"))).unwrap();
    assert_eq!(m.canonical(), again.canonical());
    assert_eq!(ReleaseManifest::parse(&m.canonical()).unwrap(), m);
    assert_eq!(m.schema, "rustev.release-manifest/2");
    for a in &m.artifacts {
        assert!(manifest::sha256sums(&m).contains(&a.name));
    }
    assert!(fa::verify(&m, &root.join("artifacts")).is_empty());
    again.schema = "rustev.release-manifest/1".into();
    assert!(ReleaseManifest::parse(&again.canonical()).is_err());
    let platforms: Vec<_> = m
        .artifacts
        .iter()
        .filter(|a| a.kind == "binary")
        .map(|a| a.name.clone())
        .collect();
    assert_eq!(
        platforms,
        fa::PLATFORMS
            .iter()
            .map(|p| format!("rustev-0.1.0-{p}.tar.gz"))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        fa::integrity(b""),
        "sha512-z4PhNX7vuL3xVChQ1m2AB9Yg5AULVxXcg/SpIdNs6c5H0NE8XYXysP+DGNKHfuwvY7kxvUdBeoGlODJ6+SfaPg=="
    );
}

#[test]
fn artifact_verification_names_all_changes_and_builder_refusals() {
    let (root, set, m, _) = fixture();
    let dir = root.join("artifacts");
    let name = &m.artifacts[0].name;
    write(&dir.join("npm").join(name), b"changed");
    std::fs::remove_file(dir.join("sbom").join(format!("{name}.cdx.json"))).unwrap();
    write(&dir.join("npm/unexpected.tgz"), b"unexpected");
    let errors = fa::verify(&m, &dir).join("\n");
    assert!(
        errors.contains("bytes differ")
            && errors.contains("missing sbom/")
            && errors.contains("unexpected npm/unexpected"),
        "{errors}"
    );
    assert!(
        fa::collect(&set, &root.join("evidence"), Some(&dir))
            .unwrap_err()
            .contains("missing SBOM")
    );
}

#[test]
fn every_expected_tarball_and_binary_is_required_and_absence_is_enforced() {
    let (root, mut set, _, _) = fixture();
    let dir = root.join("artifacts");
    assert_eq!(fa::PLATFORMS.len(), 7);
    assert!(!set.absent.iter().any(|a| a.kind == "binary"));
    for platform in fa::PLATFORMS {
        let name = format!("rustev-0.1.0-{platform}.tar.gz");
        std::fs::remove_file(dir.join("cli").join(&name)).unwrap();
        assert!(
            fa::collect(&set, &root.join("evidence"), Some(&dir))
                .unwrap_err()
                .contains(&format!("missing cli/{name}"))
        );
        write(&dir.join("cli").join(&name), name.as_bytes());
    }
    // Spec 031 3.1: absence is an explicit alternate release-set policy.
    set.absent.push(rustev_release::npm::Absent {
        kind: "binary".into(),
        name: "rustev".into(),
        reason: "fixture excludes CLI binaries".into(),
    });
    let errors = fa::collect(&set, &root.join("evidence"), Some(&dir)).unwrap_err();
    for platform in fa::PLATFORMS {
        let name = format!("rustev-0.1.0-{platform}.tar.gz");
        assert!(errors.contains(&format!("unexpected cli/{name}")));
        std::fs::remove_file(dir.join("cli").join(&name)).unwrap();
        std::fs::remove_file(dir.join("cli").join(format!("{name}.sha256"))).unwrap();
        std::fs::remove_file(dir.join("sbom").join(format!("{name}.cdx.json"))).unwrap();
    }
    let artifacts = fa::collect(&set, &root.join("evidence"), Some(&dir)).unwrap();
    assert!(artifacts.iter().all(|a| a.kind != "binary"));
    let (root, set, _, _) = fixture();
    std::fs::remove_file(root.join("artifacts/npm/statecrafting-rustev-wasm-0.1.0.tgz")).unwrap();
    assert!(
        fa::collect(&set, &root.join("evidence"), Some(&root.join("artifacts")))
            .unwrap_err()
            .contains("missing npm/")
    );
}

#[test]
fn slsa_three_missing_sbom_unknown_kinds_and_paths_are_refused() {
    let (_, _, m, _) = fixture();
    for case in 0..5 {
        let mut x = m.clone();
        match case {
            0 => x.artifacts[0].slsa_build_level = Some(3),
            1 => x.artifacts[0].sbom = None,
            2 => x.artifacts[0].kind = "image".into(),
            3 => x.artifacts[0].name = "../escape".into(),
            _ => x.artifacts[0].layer = 7,
        }
        assert!(
            ReleaseManifest::parse(&x.canonical()).is_err(),
            "case {case}"
        );
    }
}

#[test]
fn family_reaches_released_only_after_all_layers_and_qualification_is_recorded() {
    let (_, _, m, files) = fixture();
    let mut l = authorized(&m);
    let mut j = journal(&m);
    let mut r = fl::FakeHostedRelease::default();
    r.registry.lag = 2;
    assert!(l.apply(Event::Observed { complete: true }).is_err());
    fl::publish(&mut l, &files, &mut r, &mut j).unwrap();
    assert_eq!(l.state, State::Released);
    assert!(l.apply(Event::ConsumerQualified).is_err());
    assert!(fl::qualify(&mut l, BTreeMap::new(), &mut j).is_err());
    let mut evidence = BTreeMap::from([("crates-subsets".into(), "a".repeat(64))]);
    for a in &m.artifacts {
        if matches!(a.kind.as_str(), "npm" | "binary") {
            evidence.insert(a.name.clone(), "b".repeat(64));
        }
    }
    fl::qualify(&mut l, evidence, &mut j).unwrap();
    assert_eq!(l.state, State::Qualified);
    assert!(matches!(
        j.entries.first(),
        Some(Entry::SignedManifest { .. })
    ));
    assert!(matches!(
        j.entries.last(),
        Some(Entry::Qualification { .. })
    ));
    let items = fl::items(&m, &files).unwrap();
    for (ix, e) in j.entries.iter().enumerate() {
        if let Entry::Attempt { package, .. } = e {
            let item = items.iter().find(|i| i.name == *package).unwrap();
            for p in items.iter().filter(|p| p.layer < item.layer) {
                assert!(j.entries[..ix].iter().any(|e|matches!(e,Entry::Query {package,observed:Lookup::Visible {checksum}} if *package==p.name && *checksum==p.checksum)));
            }
        }
    }
}

#[test]
fn no_upload_without_signed_manifest_or_writable_journal() {
    let (root, _, m, files) = fixture();
    for case in 0..3 {
        let mut l = authorized(&m);
        let mut j = Journal::default();
        let mut r = fl::FakeHostedRelease::default();
        if case == 1 {
            j.file = Some(root.to_path_buf());
            assert!(fl::bind(&m, "sig", &mut j).is_err());
        }
        if case == 2 {
            assert!(fl::bind(&m, "", &mut j).is_err());
        }
        assert!(fl::publish(&mut l, &files, &mut r, &mut j).is_err());
        assert!(r.registry.uploads.is_empty());
    }
}

#[test]
fn instruction_binds_every_registry_and_artifact_and_refuses_yank_second_version() {
    let (_, _, m, _) = fixture();
    for case in 0..7 {
        let mut l = Lifecycle::new(m.clone());
        l.apply(Event::GatesPass).unwrap();
        l.apply(Event::CandidateVerified {
            commit: m.commit.clone(),
        })
        .unwrap();
        let mut i = instruction(&m);
        match case {
            0 => i.version = "0.2.0".into(),
            1 => i.commit = "d".repeat(40),
            2 => i.registries.pop().map(|_| ()).unwrap(),
            3 => i.artifacts.pop().map(|_| ()).unwrap(),
            4 => i.action = "yank".into(),
            5 => i.registry = "other".into(),
            _ => i.packages.clear(),
        }
        assert!(l.apply(Event::Authorized(i)).is_err());
    }
    let mut j = journal(&m);
    let mut other = m.clone();
    other.artifacts.pop();
    assert!(fl::bind(&other, "sig", &mut j).is_err());
    other = m.clone();
    other.version = "0.2.0".into();
    assert!(fl::bind(&other, "sig", &mut j).is_err());
}

#[test]
fn refusals_ambiguity_and_spent_versions_stop_at_each_layer() {
    for kind in ["crate", "npm", "binary", "changelog"] {
        for ambiguous in [false, true] {
            let (_, _, m, files) = fixture();
            let item = fl::items(&m, &files)
                .unwrap()
                .into_iter()
                .find(|i| i.kind == kind)
                .unwrap();
            let mut l = authorized(&m);
            let mut j = journal(&m);
            let mut r = fl::FakeHostedRelease::default();
            r.registry.script.insert(
                item.name.clone(),
                if ambiguous {
                    Upload::Ambiguous {
                        detail: "lost".into(),
                    }
                } else {
                    Upload::Refused {
                        reason: "refused".into(),
                    }
                },
            );
            assert!(fl::publish(&mut l, &files, &mut r, &mut j).is_err());
            if ambiguous || kind != "crate" {
                assert_eq!(l.state, State::PartiallyPublished);
            }
            assert_eq!(r.registry.uploads.last(), Some(&item.name));
            if ambiguous {
                assert!(
                    j.entries
                        .iter()
                        .rev()
                        .nth(1)
                        .is_some_and(|e| matches!(e, Entry::Query { .. }))
                );
            }
        }
    }
    let (_, _, m, files) = fixture();
    let mut l = authorized(&m);
    let mut j = journal(&m);
    let mut r = fl::FakeHostedRelease::default();
    r.registry.index.insert(
        ("rustev-contract".into(), m.version.clone()),
        ("wrong".into(), 0),
    );
    assert!(
        fl::publish(&mut l, &files, &mut r, &mut j)
            .unwrap_err()
            .contains("spent")
    );
    assert!(r.registry.uploads.is_empty());
}

#[test]
fn accepted_but_not_visible_is_partial_and_blocks_later_layers() {
    let (_, _, m, files) = fixture();
    let mut l = authorized(&m);
    let mut j = journal(&m);
    let mut r = fl::FakeHostedRelease::default();
    r.registry.lag = 100;
    assert!(
        fl::publish(&mut l, &files, &mut r, &mut j)
            .unwrap_err()
            .contains("not visible")
    );
    assert_eq!(l.state, State::PartiallyPublished);
    assert_eq!(r.registry.uploads.len(), 1);
}

#[test]
fn continuation_requires_fresh_instruction_and_skips_matching_public_bytes() {
    for accepted in [false, true] {
        let (_, _, m, files) = fixture();
        let mut l = authorized(&m);
        let mut j = journal(&m);
        let mut r = fl::FakeHostedRelease::default();
        let name = m.artifacts[0].package.clone().unwrap();
        r.registry.script.insert(
            name.clone(),
            Upload::Ambiguous {
                detail: "lost".into(),
            },
        );
        if accepted {
            r.registry.ambiguous_accepts.push(name.clone());
        }
        assert!(fl::publish(&mut l, &files, &mut r, &mut j).is_err());
        let before = r.registry.uploads.len();
        assert!(
            fl::publish(&mut l, &files, &mut r, &mut j)
                .unwrap_err()
                .contains("continuation")
        );
        assert_eq!(r.registry.uploads.len(), before);
        l.instruction.as_mut().unwrap().continuation = true;
        r.registry.script.clear();
        fl::publish(&mut l, &files, &mut r, &mut j).unwrap();
        assert_eq!(
            r.registry.uploads.iter().filter(|n| **n == name).count(),
            if accepted { 1 } else { 2 }
        );
    }
}

#[test]
fn observations_report_all_missing_and_mismatched_and_npm_errors_stay_unknown() {
    let (_, _, mut m, _) = fixture();
    let mut p = m.packages[0].clone();
    p.name = "second".into();
    m.packages.push(p);
    let mut r = rustev_release::lifecycle::FakeRegistry::default();
    r.index
        .insert(("second".into(), m.version.clone()), ("wrong".into(), 0));
    let e = rustev_release::hosted::observe_crates(&m, &mut r);
    assert_eq!(e.len(), 2);
    assert!(e[0].contains("missing") && e[1].contains("mismatch"));
    assert_eq!(
        rustev_release::hosted::npm_response("200", r#"{"dist":{"integrity":"sha512-test"}}"#),
        Lookup::Visible {
            checksum: "sha512-test".into()
        }
    );
    for code in ["500", "403", "200"] {
        assert!(matches!(
            rustev_release::hosted::npm_response(code, "{}"),
            Lookup::Unknown { .. }
        ));
    }
}

#[test]
fn incomplete_rehearsal_and_tampered_terminal_assets_refuse_publication() {
    let (root, set, mut m, mut files) = fixture();
    assert!(
        !fa::collect(&set, &root.join("evidence"), None)
            .unwrap()
            .is_empty()
    );
    m.limitations.push(fa::MISSING.into());
    let mut l = authorized(&m);
    let mut j = journal(&m);
    let mut r = fl::FakeHostedRelease::default();
    assert!(
        fl::publish(&mut l, &files, &mut r, &mut j)
            .unwrap_err()
            .contains("not publishable")
    );
    assert!(r.registry.uploads.is_empty());
    m.limitations.clear();
    files.insert("SHA256SUMS".into(), b"wrong".to_vec());
    let mut l = authorized(&m);
    let mut j = journal(&m);
    assert!(
        fl::publish(&mut l, &files, &mut r, &mut j)
            .unwrap_err()
            .contains("terminal assets")
    );
}

#[test]
fn journal_cannot_live_in_repository_even_through_symlink() {
    let root = fresh_dir("rustev-journal-path").unwrap();
    let repo = root.join("repo");
    std::fs::create_dir(&repo).unwrap();
    assert!(rustev_release::hosted::journal_path(&repo.join("journal"), &repo).is_err());
    #[cfg(unix)]
    {
        let target = repo.join("journal");
        write(&target, b"");
        let link = root.join("link");
        std::os::unix::fs::symlink(target, &link).unwrap();
        assert!(rustev_release::hosted::journal_path(&link, &repo).is_err());
    }
    assert!(rustev_release::hosted::journal_path(&root.join("external"), &repo).is_ok());
}

#[cfg(unix)]
#[test]
fn real_gh_adapter_is_exercised_only_with_a_fake_process() {
    use fl::FamilyRegistry;
    use std::os::unix::fs::PermissionsExt;
    let root = fresh_dir("rustev-fake-gh").unwrap();
    let script = root.join("gh");
    write(
        &script,
        br##"#!/bin/sh
set -eu
base=$(dirname "$0")
printf '%s\n' "$*" >> "$base/calls"
shift 2
case "$1:$2" in
release:view)
  if [ ! -d "$base/public" ]; then echo 'release not found' >&2; exit 1; fi
  printf '{"assets":['
  sep=''
  for f in "$base/public/"*; do printf '%s{"name":"%s"}' "$sep" "$(basename "$f")"; sep=,; done
  printf ']}'
  ;;
release:create|release:upload)
  for arg do last=$arg; done
  mkdir -p "$base/public"
  cp "$last" "$base/public/"
  ;;
release:download)
  shift 3
  test "$1" = --pattern
  name=$2
  shift 2
  test "$1" = --dir
  cp "$base/public/$name" "$2/$name"
  ;;
*) exit 9 ;;
esac
"##,
    );
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let staging = root.join("staging");
    std::fs::create_dir(&staging).unwrap();
    let notes = root.join("changelog.md");
    write(&notes, b"notes");
    let mut adapter = rustev_release::hosted::HostedRelease {
        gh: script,
        tag: "v0.1.0".into(),
        changelog: notes,
        staging,
    };
    let item = fl::Item {
        kind: "binary".into(),
        name: "cli.tar.gz".into(),
        file: "cli.tar.gz".into(),
        layer: 7,
        checksum: sha256_hex(b"binary"),
        bytes: b"binary".to_vec(),
    };
    assert_eq!(adapter.lookup(&item, "0.1.0"), Lookup::Absent);
    assert_eq!(adapter.upload(&item, "0.1.0"), Upload::Accepted);
    assert_eq!(
        adapter.lookup(&item, "0.1.0"),
        Lookup::Visible {
            checksum: item.checksum.clone()
        }
    );
    let mut sbom = item.clone();
    sbom.kind = "sbom".into();
    sbom.name = "cli.cdx.json".into();
    sbom.file = sbom.name.clone();
    sbom.layer = 8;
    assert_eq!(adapter.upload(&sbom, "0.1.0"), Upload::Accepted);
    assert_eq!(
        adapter.lookup(&sbom, "0.1.0"),
        Lookup::Visible {
            checksum: sbom.checksum
        }
    );
    let calls = std::fs::read_to_string(root.join("calls")).unwrap();
    assert!(calls.contains("release create v0.1.0 --verify-tag --title v0.1.0 --notes-file"));
    assert!(calls.contains("release upload v0.1.0"));
    assert!(!calls.contains("--clobber"));
    let mut image = item;
    image.kind = "image".into();
    assert!(matches!(
        adapter.upload(&image, "0.1.0"),
        Upload::Refused { .. }
    ));
}

// Spec 031 3.1: an earlier acceptance cannot substitute for current visibility.
#[test]
fn disappearing_earlier_layer_blocks_later_uploads() {
    struct Disappearing {
        inner: fl::FakeHostedRelease,
        crate_queries: usize,
    }
    impl fl::FamilyRegistry for Disappearing {
        fn lookup(&mut self, i: &fl::Item, v: &str) -> Lookup {
            if i.kind == "crate" {
                self.crate_queries += 1;
                if self.crate_queries > 2 {
                    return Lookup::Absent;
                }
            }
            fl::FamilyRegistry::lookup(&mut self.inner, i, v)
        }
        fn upload(&mut self, i: &fl::Item, v: &str) -> Upload {
            fl::FamilyRegistry::upload(&mut self.inner, i, v)
        }
        fn wait(&mut self) {}
    }
    let (_root, _, m, files) = fixture();
    let mut l = authorized(&m);
    let mut j = journal(&m);
    let mut r = Disappearing {
        inner: Default::default(),
        crate_queries: 0,
    };
    assert!(
        fl::publish(&mut l, &files, &mut r, &mut j)
            .unwrap_err()
            .contains("earlier layer")
    );
    assert_eq!(r.inner.registry.uploads, m.upload_order);
    assert_eq!(l.state, State::PartiallyPublished);
}

// Spec 031 3.11.2: crate-only callers also need the immutable signed binding.
#[test]
fn crate_upload_without_signed_manifest_is_refused() {
    let (_root, _, mut m, files) = fixture();
    m.artifacts.clear();
    let mut l = authorized(&m);
    let mut r = rustev_release::lifecycle::FakeRegistry::default();
    assert!(
        matches!(rustev_release::lifecycle::publish(&mut l, &files, &mut r, &mut Journal::default()),
        rustev_release::lifecycle::Outcome::Stopped { reason } if reason.contains("signed manifest"))
    );
    assert!(r.uploads.is_empty());
}
