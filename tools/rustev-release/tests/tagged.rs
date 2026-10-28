//! Spec 058 acceptance: each rule of 3.1.1 broken once, the manifest made
//! from a run's artifacts (3.2.4), and 3.5.2 for crates, npm and the GitHub
//! release (absent, present and equal, present and different). No network:
//! registries and releases are fakes, Git repositories are throwaway.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use rustev_release::archive::sha256_hex;
use rustev_release::family_artifacts as fa;
use rustev_release::lifecycle::{Lookup, State};
use rustev_release::manifest::{self, Inputs, ReleaseManifest, Toolchain};
use rustev_release::prerelease::{Assets, Outcome, Release, Releases};
use rustev_release::qualification::{KINDS, SCHEMA, Status, Step, required};
use rustev_release::repro::{Scratch, fresh_dir};
use rustev_release::tagged::{self, Item, Publisher, Record};
use rustev_release::{ReleaseSet, prerelease};

const IDENTITY: &str = "release@example.invalid";

fn run(cmd: &mut Command) -> String {
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "{cmd:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn write(path: &Path, b: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, b).unwrap();
}

// 3.1.1 -------------------------------------------------------------------

struct Repo {
    _root: Scratch,
    repo: PathBuf,
    signers: PathBuf,
    key: PathBuf,
    other: PathBuf,
}

fn keygen(dir: &Path, name: &str) -> PathBuf {
    let key = dir.join(name);
    run(Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-C", IDENTITY, "-f"])
        .arg(&key));
    key
}

impl Repo {
    /// One commit on `main` whose release set says `version`.
    fn new(version: &str) -> Repo {
        let root = fresh_dir("rustev-tagged-repo").unwrap();
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let key = keygen(&root, "owner");
        let other = keygen(&root, "other");
        let public = std::fs::read_to_string(key.with_extension("pub")).unwrap();
        let mut parts = public.split_whitespace();
        let signers = root.join("allowed_signers");
        let line = format!(
            "{IDENTITY} {} {}\n",
            parts.next().unwrap(),
            parts.next().unwrap()
        );
        std::fs::write(&signers, line).unwrap();
        let r = Repo {
            _root: root,
            repo,
            signers,
            key,
            other,
        };
        r.git(&["init", "-q", "-b", "main"]);
        for (k, v) in [
            ("user.name", "Release Owner"),
            ("user.email", IDENTITY),
            ("gpg.format", "ssh"),
            ("commit.gpgsign", "false"),
            ("tag.gpgsign", "false"),
        ] {
            r.git(&["config", k, v]);
        }
        r.commit(version);
        r
    }

    fn git(&self, args: &[&str]) -> String {
        run(Command::new("git").arg("-C").arg(&self.repo).args(args))
    }

    fn commit(&self, version: &str) -> String {
        let text = String::from_utf8(include_bytes!("../release-set.json").to_vec()).unwrap();
        let set = ReleaseSet::parse(text.as_bytes()).unwrap();
        let text = text.replacen(
            &format!("\"version\": \"{}\"", set.version),
            &format!("\"version\": \"{version}\""),
            1,
        );
        write(
            &self.repo.join("tools/rustev-release/release-set.json"),
            text.as_bytes(),
        );
        self.git(&["add", "."]);
        self.git(&["commit", "-q", "--allow-empty", "-m", version]);
        self.git(&["rev-parse", "HEAD"])
    }

    fn sign(&self, tag: &str, key: &Path) {
        self.git(&[
            "-c",
            &format!("user.signingkey={}", key.display()),
            "tag",
            "-s",
            tag,
            "-m",
            tag,
        ]);
    }

    fn check(&self, tag: &str, runs: &[u8]) -> Result<tagged::TagFacts, String> {
        tagged::check_tag(&self.repo, tag, "refs/heads/main", &self.signers, runs)
    }
}

fn gate(commit: &str, conclusion: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({"total_count": 1, "check_runs": [
        {"name": "ci-gate", "head_sha": commit, "status": "completed", "conclusion": conclusion}
    ]}))
    .unwrap()
}

#[test]
fn a_signed_annotated_tag_on_main_with_a_green_gate_passes() {
    let r = Repo::new("0.3.0");
    let head = r.git(&["rev-parse", "HEAD"]);
    r.sign("v0.3.0", &r.key);
    let facts = r.check("v0.3.0", &gate(&head, "success")).unwrap();
    assert_eq!(facts.commit, head);
    assert_eq!(facts.tag_object, r.git(&["rev-parse", "refs/tags/v0.3.0"]));
    assert_ne!(facts.tag_object, head);
    assert_eq!((facts.version.as_str(), facts.candidate), ("0.3.0", false));
    assert!(facts.outputs().contains("candidate=false\n"));
    // A candidate of the same version (3.7).
    r.sign("v0.3.0-rc.2", &r.key);
    let c = r.check("v0.3.0-rc.2", &gate(&head, "success")).unwrap();
    assert_eq!((c.version.as_str(), c.candidate), ("0.3.0-rc.2", true));
}

#[test]
fn each_rule_of_3_1_1_refuses_once() {
    let r = Repo::new("0.3.0");
    let head = r.git(&["rev-parse", "HEAD"]);
    let green = gate(&head, "success");
    // 1. The ref's form.
    for bad in [
        "0.3.0",
        "v0.3",
        "v0.3.0-rc.0",
        "v03.0.0",
        "v0.3.0-beta.1",
        "release",
    ] {
        assert!(
            r.check(bad, &green).unwrap_err().contains("058 3.1.1"),
            "{bad}"
        );
    }
    // 2. Annotated and signed by an allowed signer.
    r.git(&["tag", "v0.3.0"]);
    assert!(
        r.check("v0.3.0", &green)
            .unwrap_err()
            .contains("not an annotated tag")
    );
    r.git(&["tag", "-d", "v0.3.0"]);
    r.git(&["tag", "-a", "v0.3.0", "-m", "unsigned"]);
    assert!(
        r.check("v0.3.0", &green)
            .unwrap_err()
            .contains("does not verify")
    );
    r.git(&["tag", "-d", "v0.3.0"]);
    r.sign("v0.3.0", &r.other);
    assert!(
        r.check("v0.3.0", &green)
            .unwrap_err()
            .contains("does not verify")
    );
    r.git(&["tag", "-d", "v0.3.0"]);
    // 3. The release set's version.
    r.sign("v0.4.0", &r.key);
    assert!(
        r.check("v0.4.0", &green)
            .unwrap_err()
            .contains("the release set at")
    );
    // 5. A successful ci-gate on the tagged commit.
    r.sign("v0.3.0", &r.key);
    for runs in [
        gate(&head, "failure"),
        gate(&"0".repeat(40), "success"),
        br#"{"total_count":0,"check_runs":[]}"#.to_vec(),
        b"not json".to_vec(),
    ] {
        assert!(r.check("v0.3.0", &runs).unwrap_err().contains("058 3.1.1"));
    }
    assert!(r.check("v0.3.0", &green).is_ok());
    // 4. Reachable from main.
    r.git(&["checkout", "-q", "-b", "side"]);
    let side = r.commit("0.5.0");
    r.sign("v0.5.0", &r.key);
    let err = r.check("v0.5.0", &gate(&side, "success")).unwrap_err();
    assert!(err.contains("not reachable from refs/heads/main"), "{err}");
}

// 3.5.2 for crates and npm -------------------------------------------------

#[derive(Default)]
struct Fake {
    /// The public checksum per name.
    public: BTreeMap<String, String>,
    /// What an upload makes public, per name (absent: the item's own).
    lands_as: BTreeMap<String, String>,
    /// Uploads that fail without landing.
    fails: Vec<String>,
    unknown: Vec<String>,
    published: Vec<String>,
    lag: u32,
}

impl Publisher for Fake {
    fn lookup(&mut self, item: &Item, _: &str) -> Lookup {
        if self.unknown.contains(&item.name) {
            return Lookup::Unknown {
                detail: "index HTTP 503".into(),
            };
        }
        match self.public.get(&item.name) {
            Some(c) if self.lag == 0 => Lookup::Visible {
                checksum: c.clone(),
            },
            _ => Lookup::Absent,
        }
    }
    fn publish(&mut self, item: &Item, _: &str) -> Result<(), String> {
        self.published.push(item.name.clone());
        if self.fails.contains(&item.name) {
            return Err("HTTP 403: no trusted publisher".into());
        }
        let c = self
            .lands_as
            .get(&item.name)
            .unwrap_or(&item.expected)
            .clone();
        self.public.insert(item.name.clone(), c);
        Ok(())
    }
    fn wait(&mut self) {
        self.lag = self.lag.saturating_sub(1);
    }
    fn now(&mut self) -> String {
        tagged::rfc3339(1_761_634_800)
    }
}

fn item(kind: &str, name: &str) -> Item {
    let expected = if kind == "npm" {
        fa::integrity(name.as_bytes())
    } else {
        sha256_hex(name.as_bytes())
    };
    Item {
        kind: kind.into(),
        name: name.into(),
        file: format!("{name}.file"),
        sha256: sha256_hex(name.as_bytes()),
        expected,
    }
}

fn record() -> Record {
    Record {
        schema: tagged::RECORD_SCHEMA.into(),
        version: "0.3.0".into(),
        tag: "v0.3.0".into(),
        commit: "c".repeat(40),
        manifest_sha256: "d".repeat(64),
        entries: vec![],
    }
}

#[test]
fn absent_items_are_published_in_order_and_recorded() {
    for kind in ["crate", "npm"] {
        let items = [item(kind, "a"), item(kind, "b")];
        let mut fake = Fake {
            lag: 2,
            ..Fake::default()
        };
        let mut r = record();
        tagged::publish_items(&items, "0.3.0", 5, &mut fake, &mut r).unwrap();
        assert_eq!(fake.published, ["a", "b"]);
        assert_eq!(r.entries.len(), 2);
        assert!(r.entries.iter().all(|e| e.outcome == "published"));
        assert_eq!(r.entries[0].observed, items[0].expected);
        assert_eq!(r.entries[0].time, "2025-10-28T07:00:00Z");
    }
}

#[test]
fn present_and_equal_is_skipped_without_an_upload() {
    for kind in ["crate", "npm"] {
        let items = [item(kind, "a"), item(kind, "b")];
        let mut fake = Fake::default();
        fake.public.insert("a".into(), items[0].expected.clone());
        let mut r = record();
        tagged::publish_items(&items, "0.3.0", 5, &mut fake, &mut r).unwrap();
        assert_eq!(fake.published, ["b"]);
        assert_eq!(r.entries[0].outcome, "present");
        assert_eq!(r.entries[1].outcome, "published");
    }
}

#[test]
fn present_and_different_fails_and_republishes_nothing() {
    for kind in ["crate", "npm"] {
        let items = [item(kind, "a"), item(kind, "b")];
        let mut fake = Fake::default();
        fake.public.insert("a".into(), "other".into());
        let mut r = record();
        let err = tagged::publish_items(&items, "0.3.0", 5, &mut fake, &mut r).unwrap_err();
        assert!(
            err.contains("058 3.5.2") && err.contains("nothing is republished"),
            "{err}"
        );
        assert!(fake.published.is_empty() && r.entries.is_empty());
    }
}

#[test]
fn a_listing_with_another_checksum_fails_before_the_next_item() {
    let items = [item("crate", "a"), item("crate", "b")];
    let mut fake = Fake::default();
    fake.lands_as.insert("a".into(), "rebuilt".into());
    let mut r = record();
    let err = tagged::publish_items(&items, "0.3.0", 5, &mut fake, &mut r).unwrap_err();
    assert!(err.contains("058 3.3.1"), "{err}");
    assert_eq!(fake.published, ["a"]);
}

#[test]
fn unknown_state_a_refused_upload_or_no_visibility_fails() {
    let items = [item("crate", "a")];
    let mut fake = Fake {
        unknown: vec!["a".into()],
        ..Fake::default()
    };
    let err = tagged::publish_items(&items, "0.3.0", 5, &mut fake, &mut record()).unwrap_err();
    assert!(err.contains("unknown") && fake.published.is_empty());
    let mut fake = Fake {
        fails: vec!["a".into()],
        ..Fake::default()
    };
    let err = tagged::publish_items(&items, "0.3.0", 5, &mut fake, &mut record()).unwrap_err();
    assert!(err.contains("was not published"), "{err}");
    let mut fake = Fake {
        lag: 10,
        ..Fake::default()
    };
    let err = tagged::publish_items(&items, "0.3.0", 3, &mut fake, &mut record()).unwrap_err();
    assert!(err.contains("after 3 polls"), "{err}");
}

// The manifest from a run's artifacts (3.2.4) and the release ---------------

/// A run's manifest at 0.3.0 built from its evidence and artifacts, staged
/// with every bundle and a publication record into a release directory.
fn release() -> (Scratch, ReleaseManifest, PathBuf) {
    let root = fresh_dir("rustev-tagged-release").unwrap();
    let mut set = ReleaseSet::parse(include_bytes!("../release-set.json")).unwrap();
    set.version = "0.3.0".into();
    set.tag = "v0.3.0".into();
    set.upload_order = vec!["rustev-contract".into()];
    set.layers = vec![set.upload_order.clone()];
    set.npm = vec![set.npm.last().unwrap().clone()];
    let dir = root.join("release");
    write(&dir.join("rustev-contract-0.3.0.crate"), b"crate");
    let sbom = br#"{"bomFormat":"CycloneDX","specVersion":"1.5"}"#;
    write(&dir.join("sbom-rustev-contract.cdx.json"), sbom);
    write(&dir.join("changelog.md"), b"release notes");
    let artifacts = root.join("artifacts");
    let mut names = vec![("npm", "rustev-rustev-wasm-0.3.0.tgz".to_string())];
    for p in fa::PLATFORMS {
        names.push(("cli", format!("rustev-0.3.0-{p}.tar.gz")));
    }
    for (kind, name) in names {
        write(&artifacts.join(kind).join(&name), name.as_bytes());
        write(
            &artifacts.join("sbom").join(format!("{name}.cdx.json")),
            sbom,
        );
        if kind == "cli" {
            let line = format!("{}  {name}\n", sha256_hex(name.as_bytes()));
            write(
                &artifacts.join(kind).join(format!("{name}.sha256")),
                line.as_bytes(),
            );
            write(&dir.join(format!("{name}.sha256")), line.as_bytes());
        }
    }
    let inputs = Inputs {
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
    };
    let mut m = manifest::build(&set, &dir, inputs).unwrap();
    m.artifacts = fa::collect(&set, &dir, Some(&artifacts)).unwrap();
    m.msrv = "1.88".into();
    m.edition = "2024".into();
    m.tag_object = Some("e".repeat(40));
    fa::stage(&m, &dir, Some(&artifacts)).unwrap();
    manifest::write_files(&m, &dir).unwrap();
    for p in &m.packages {
        write(&dir.join(format!("{}.intoto.jsonl", p.crate_file)), b"{}");
    }
    for a in &m.artifacts {
        write(&dir.join(format!("{}.intoto.jsonl", a.name)), b"{}");
    }
    for n in ["release-manifest.json", "SHA256SUMS"] {
        write(&dir.join(format!("{n}.intoto.jsonl")), b"{}");
    }
    let mut r = Record::new(&m);
    for i in tagged::items(&m, "crate", Some(&dir))
        .unwrap()
        .into_iter()
        .chain(tagged::items(&m, "npm", Some(&dir)).unwrap())
    {
        r.entries.push(tagged::Entry {
            kind: i.kind,
            name: i.name,
            file: i.file,
            sha256: i.sha256,
            observed: i.expected,
            outcome: "published".into(),
            time: tagged::rfc3339(0),
        });
    }
    r.write(&dir.join("publication.json")).unwrap();
    (root, m, dir)
}

#[test]
fn the_run_manifest_names_the_tag_object_and_lists_every_artifact() {
    let (_root, m, dir) = release();
    let bytes = std::fs::read(dir.join("release-manifest.json")).unwrap();
    let parsed = ReleaseManifest::parse(&bytes).unwrap();
    assert_eq!(parsed.tag_object.as_deref(), Some("e".repeat(40).as_str()));
    // A manifest made before spec 058 names no tag object and still parses.
    let mut old = parsed.clone();
    old.tag_object = None;
    assert!(
        !String::from_utf8(old.canonical())
            .unwrap()
            .contains("tag_object")
    );
    ReleaseManifest::parse(&old.canonical()).unwrap();
    // SHA256SUMS lists every crate and artifact the run built.
    let sums = std::fs::read_to_string(dir.join("SHA256SUMS")).unwrap();
    assert_eq!(sums.lines().count(), m.packages.len() + m.artifacts.len());
    assert!(
        m.artifacts
            .iter()
            .any(|a| a.kind == "npm" && a.integrity.is_some())
    );
    // No signature is an input or an asset.
    let assets = tagged::verify_assets(&m, &dir).unwrap();
    assert!(!assets.keys().any(|n| n.ends_with(".sig")));
    assert!(assets.contains_key("publication.json"));
    assert!(assets.contains_key("SHA256SUMS.intoto.jsonl"));
}

#[test]
fn verify_assets_names_a_missing_bundle_a_changed_byte_and_an_unrecorded_package() {
    let (_root, m, dir) = release();
    std::fs::remove_file(dir.join("release-manifest.json.intoto.jsonl")).unwrap();
    std::fs::write(dir.join("rustev-contract-0.3.0.crate"), b"crate!").unwrap();
    let mut r: Record =
        serde_json::from_slice(&std::fs::read(dir.join("publication.json")).unwrap()).unwrap();
    r.entries.retain(|e| e.kind != "npm");
    r.write(&dir.join("publication.json")).unwrap();
    let problems = tagged::verify_assets(&m, &dir).unwrap_err();
    let all = problems.join("\n");
    assert!(all.contains("release-manifest.json.intoto.jsonl"), "{all}");
    assert!(all.contains("rustev-contract-0.3.0.crate differs"), "{all}");
    assert!(
        all.contains("npm @rustev/rustev-wasm is not recorded"),
        "{all}"
    );
    // A crate on disk that is not the manifest's never reaches a registry.
    assert!(
        tagged::items(&m, "crate", Some(&dir))
            .unwrap_err()
            .contains("not the run's verified")
    );
}

#[derive(Default)]
struct Gh {
    present: Option<Release>,
    creates: Vec<bool>,
}

impl Releases for Gh {
    fn lookup(&mut self, _: &str) -> Result<Option<Release>, String> {
        Ok(self.present.clone())
    }
    fn create(&mut self, _: &str, _: &str, _: &Assets) -> Result<(), String> {
        unreachable!("058 creates through create_release")
    }
    fn create_release(
        &mut self,
        _: &str,
        notes: &str,
        assets: &Assets,
        pre: bool,
    ) -> Result<(), String> {
        assert_eq!(notes, "release notes");
        self.creates.push(pre);
        self.present = Some(Release {
            prerelease: pre,
            assets: assets
                .iter()
                .map(|(n, b)| (n.clone(), sha256_hex(b)))
                .collect(),
        });
        Ok(())
    }
}

#[test]
fn the_github_release_is_created_once_and_never_replaced() {
    let (_root, m, dir) = release();
    let assets = tagged::verify_assets(&m, &dir).unwrap();
    let notes = tagged::notes(&m, &assets).unwrap();
    let mut gh = Gh::default();
    // Absent: created, as a release marked latest.
    let o = tagged::publish_release(&mut gh, "v0.3.0", &notes, &assets, false).unwrap();
    assert_eq!(
        (o, gh.creates.as_slice()),
        (Outcome::Created, [false].as_slice())
    );
    // Present and equal: success, nothing uploaded.
    let o = tagged::publish_release(&mut gh, "v0.3.0", &notes, &assets, false).unwrap();
    assert_eq!((o, gh.creates.len()), (Outcome::Present, 1));
    // Present and different (an asset, or the flag): fails, replaces nothing.
    let mut other = assets.clone();
    other.insert("changelog.md".into(), b"edited".to_vec());
    let err = tagged::publish_release(&mut gh, "v0.3.0", &notes, &other, false).unwrap_err();
    assert!(err.contains("058 3.5.2"), "{err}");
    assert!(tagged::publish_release(&mut gh, "v0.3.0", &notes, &assets, true).is_err());
    assert_eq!(gh.creates.len(), 1);
    // The adapter of 053 alone cannot create a release.
    struct Old;
    impl Releases for Old {
        fn lookup(&mut self, _: &str) -> Result<Option<Release>, String> {
            Ok(None)
        }
        fn create(&mut self, _: &str, _: &str, _: &prerelease::Assets) -> Result<(), String> {
            Ok(())
        }
    }
    assert!(tagged::publish_release(&mut Old, "v0.3.0", &notes, &assets, false).is_err());
}

// 3.4.2 ------------------------------------------------------------------

fn steps(m: &ReleaseManifest, dir: &Path) {
    for platform in fa::PLATFORMS {
        for kind in KINDS {
            let passed = required(m, kind);
            let step = Step {
                schema: SCHEMA.into(),
                manifest_sha256: m.digest(),
                kind: (*kind).into(),
                platform: (*platform).into(),
                command: vec!["consumer".into()],
                exit_status: passed.then_some(0),
                output_digest: "f".repeat(64),
                lockfile_or_install_record: "records".into(),
                checksums: BTreeMap::from([("x".into(), "f".repeat(64))]),
                host: "h".into(),
                os: "o".into(),
                architecture: "a".into(),
                toolchain: "t".into(),
                node: "n".into(),
                status: if passed {
                    Status::Passed
                } else {
                    Status::Excluded {
                        reason: "absent".into(),
                    }
                },
            };
            write(
                &dir.join(format!("{platform}.{kind}.json")),
                &serde_json::to_vec(&step).unwrap(),
            );
        }
    }
}

#[test]
fn qualify_needs_no_instruction_or_journal() {
    let (root, m, dir) = release();
    let evidence = root.join("evidence");
    steps(&m, &evidence);
    let q = tagged::qualify(&dir, &evidence, &"e".repeat(40)).unwrap();
    assert_eq!(q.state, State::Qualified);
    assert_eq!(q.manifest_sha256, m.digest());
    assert_eq!(q.evidence.len(), fa::PLATFORMS.len() * KINDS.len());
    // Another tag object than the manifest's is not this run's authority.
    let err = tagged::qualify(&dir, &evidence, &"0".repeat(40)).unwrap_err();
    assert!(err.contains("058 3.1.2"), "{err}");
    // A missing platform's step leaves the lifecycle unqualified.
    std::fs::remove_file(evidence.join("linux-x64-gnu.cli.json")).unwrap();
    assert!(tagged::qualify(&dir, &evidence, &"e".repeat(40)).is_err());
    // So does a package the record does not show public.
    steps(&m, &evidence);
    let mut r = Record::load(&dir.join("publication.json"), &m).unwrap();
    r.entries.pop();
    r.write(&dir.join("publication.json")).unwrap();
    let err = tagged::qualify(&dir, &evidence, &"e".repeat(40)).unwrap_err();
    assert!(err.contains("058 3.5.4"), "{err}");
}

#[test]
fn a_record_of_another_release_is_refused() {
    let (_root, m, dir) = release();
    let path = dir.join("publication.json");
    assert_eq!(Record::load(&path, &m).unwrap().entries.len(), 2);
    let mut other = m.clone();
    other.commit = "b".repeat(40);
    assert!(
        Record::load(&path, &other)
            .unwrap_err()
            .contains("another release")
    );
    assert!(
        Record::load(&dir.join("absent.json"), &m)
            .unwrap()
            .entries
            .is_empty()
    );
}

#[test]
fn an_npm_artifact_without_package_or_integrity_is_refused() {
    let (_root, m, _dir) = release();
    assert!(tagged::items(&m, "npm", None).is_ok());
    for field in ["package", "integrity"] {
        let mut m = m.clone();
        let a = m.artifacts.iter_mut().find(|a| a.kind == "npm").unwrap();
        match field {
            "package" => a.package = None,
            _ => a.integrity = None,
        }
        let err = tagged::items(&m, "npm", None).unwrap_err();
        assert!(
            err.contains("without package or integrity"),
            "{field}: {err}"
        );
    }
}
