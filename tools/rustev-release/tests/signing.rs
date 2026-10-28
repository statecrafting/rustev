//! Spec 021 acceptance: "Signature fixtures prove valid commit, tree, tag,
//! manifest, checksum and key binding, and reject substitution of any one
//! member" (3.8, I-3).
//!
//! Each test builds a fresh fixture: a throwaway ed25519 key generated in a
//! temporary directory (no private key is committed), a Git repository with
//! one commit, two archives, the canonical manifest and `SHA256SUMS`, their
//! detached SSH signatures, and a signed annotated tag naming the manifest.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use rustev_release::ReleaseSet;
use rustev_release::manifest::{
    Inputs, NAMESPACE, ReleaseManifest, Toolchain, build, sha256sums, tag_message, verify_release,
};

const IDENTITY: &str = "release@example.invalid";

struct Fixture {
    root: PathBuf,
    repo: PathBuf,
    dir: PathBuf,
    key: PathBuf,
    signers: PathBuf,
}

fn run(cmd: &mut Command) -> String {
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "{cmd:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn set() -> ReleaseSet {
    ReleaseSet::parse(
        br#"{"schema":"rustev.release-set/1","version":"0.1.0","tag":"v0.1.0",
        "registry":"crates-io","index":"registry+https://github.com/rust-lang/crates.io-index",
        "layers":[["pkg-a"],["pkg-b"]],"upload_order":["pkg-a","pkg-b"],"binaries":[],"npm":[],"absent":[],"excluded":[]}"#,
    )
    .unwrap()
}

/// The workspace's release set, which the candidate dispatch reads.
fn release_set() -> ReleaseSet {
    ReleaseSet::parse(include_bytes!("../release-set.json")).unwrap()
}

/// The first candidate tag of the workspace's family version (053 3.2).
fn candidate() -> String {
    format!("{}-rc.1", release_set().tag)
}

fn keygen(dir: &Path, name: &str) -> PathBuf {
    let key = dir.join(name);
    run(Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-C", IDENTITY, "-f"])
        .arg(&key));
    key
}

fn sign(key: &Path, file: &Path) {
    let sig = PathBuf::from(format!("{}.sig", file.display()));
    let _ = std::fs::remove_file(&sig);
    run(Command::new("ssh-keygen")
        .args(["-Y", "sign", "-n", NAMESPACE, "-f"])
        .arg(key)
        .arg(file));
}

fn git(repo: &Path, args: &[&str]) -> String {
    run(Command::new("git").arg("-C").arg(repo).args(args))
}

impl Fixture {
    fn new(name: &str) -> Fixture {
        let root =
            std::env::temp_dir().join(format!("rustev-release-sign-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (repo, dir) = (root.join("repo"), root.join("release"));
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        let key = keygen(&root, "owner");
        let public = std::fs::read_to_string(key.with_extension("pub")).unwrap();
        let mut parts = public.split_whitespace();
        let signers = root.join("allowed_signers");
        std::fs::write(
            &signers,
            format!(
                "{IDENTITY} {} {}\n",
                parts.next().unwrap(),
                parts.next().unwrap()
            ),
        )
        .unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        for (k, v) in [
            ("user.name", "Release Owner"),
            ("user.email", IDENTITY),
            ("gpg.format", "ssh"),
            ("commit.gpgsign", "false"),
            ("tag.gpgsign", "false"),
        ] {
            git(&repo, &["config", k, v]);
        }
        git(
            &repo,
            &["config", "user.signingkey", &key.display().to_string()],
        );
        std::fs::write(repo.join("lib.rs"), b"pub fn f() {}\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "release"]);
        for (n, body) in [("pkg-a", b"archive a".as_slice()), ("pkg-b", b"archive b")] {
            std::fs::write(dir.join(format!("{n}-0.1.0.crate")), body).unwrap();
        }
        let f = Fixture {
            root,
            repo,
            dir,
            key,
            signers,
        };
        let m = f.manifest(&f.head(), &f.tree());
        f.publish(&m);
        f.tag(&tag_message(&m), "HEAD");
        f
    }

    fn head(&self) -> String {
        git(&self.repo, &["rev-parse", "HEAD"])
    }

    fn tree(&self) -> String {
        git(&self.repo, &["rev-parse", "HEAD^{tree}"])
    }

    fn manifest(&self, commit: &str, tree: &str) -> ReleaseManifest {
        build(
            &set(),
            &self.dir,
            Inputs {
                commit: commit.into(),
                tree: tree.into(),
                toolchain: Toolchain {
                    rustc: "rustc 1.98.1".into(),
                    cargo: "cargo 1.98.1".into(),
                    host: "aarch64-apple-darwin".into(),
                    audit: "cargo-deny 0.20.2".into(),
                },
                source_date_epoch: 1_700_000_000,
                evidence: BTreeMap::from([("compare.txt".into(), "00".repeat(32))]),
                limitations: vec!["fixture".into()],
            },
        )
        .unwrap()
    }

    /// Write and sign the manifest and `SHA256SUMS` with the owner key.
    fn publish(&self, m: &ReleaseManifest) {
        let mf = self.dir.join("release-manifest.json");
        std::fs::write(&mf, m.canonical()).unwrap();
        sign(&self.key, &mf);
        let sums = self.dir.join("SHA256SUMS");
        std::fs::write(&sums, sha256sums(m)).unwrap();
        sign(&self.key, &sums);
    }

    fn tag(&self, message: &str, at: &str) {
        let msg = self.root.join("tag-message");
        std::fs::write(&msg, message).unwrap();
        git(
            &self.repo,
            &[
                "tag",
                "-f",
                "-s",
                "v0.1.0",
                "-F",
                &msg.display().to_string(),
                at,
            ],
        );
    }

    fn verify(&self) -> Vec<String> {
        verify_release(&self.dir, &self.repo, &self.signers, IDENTITY)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn fails(v: &[String], needle: &str) {
    assert!(
        v.iter().any(|x| x.contains(needle)),
        "expected {needle:?} in {v:#?}"
    );
}

#[test]
fn a_complete_release_verifies() {
    let f = Fixture::new("valid");
    assert_eq!(f.verify(), Vec::<String>::new());
    let m = ReleaseManifest::parse(&std::fs::read(f.dir.join("release-manifest.json")).unwrap())
        .unwrap();
    assert_eq!(m.commit, f.head());
    assert_eq!(m.tree, f.tree());
    assert_eq!(
        m.packages[1].sha256,
        rustev_release::archive::sha256_hex(b"archive b")
    );
}

#[test]
fn verification_needs_no_ssh_configuration_of_the_repository() {
    let f = Fixture::new("noformat");
    git(&f.repo, &["config", "--unset", "gpg.format"]);
    assert_eq!(f.verify(), Vec::<String>::new());
}

#[test]
fn a_substituted_archive_fails() {
    let f = Fixture::new("archive");
    std::fs::write(f.dir.join("pkg-b-0.1.0.crate"), b"rebuilt b").unwrap();
    fails(&f.verify(), "is not the archive the manifest names");
}

#[test]
fn a_manifest_signed_by_another_key_fails() {
    let f = Fixture::new("key");
    let other = keygen(&f.root, "intruder");
    sign(&other, &f.dir.join("release-manifest.json"));
    fails(&f.verify(), "manifest signature");
}

#[test]
fn an_edited_manifest_fails() {
    let f = Fixture::new("manifest");
    let mut m = f.manifest(&f.head(), &f.tree());
    m.limitations.clear();
    std::fs::write(f.dir.join("release-manifest.json"), m.canonical()).unwrap();
    let v = f.verify();
    fails(&v, "manifest signature");
    fails(&v, "tag message");
}

#[test]
fn substituted_checksums_fail_even_when_signed() {
    let f = Fixture::new("sums");
    let sums = f.dir.join("SHA256SUMS");
    std::fs::write(&sums, format!("{}  pkg-a-0.1.0.crate\n", "11".repeat(32))).unwrap();
    sign(&f.key, &sums);
    fails(&f.verify(), "does not list exactly");
}

#[test]
fn a_tag_on_another_commit_fails() {
    let f = Fixture::new("commit");
    std::fs::write(f.repo.join("lib.rs"), b"pub fn g() {}\n").unwrap();
    git(&f.repo, &["commit", "-q", "-am", "later"]);
    let m = f.manifest(
        &git(&f.repo, &["rev-parse", "HEAD~1"]),
        &git(&f.repo, &["rev-parse", "HEAD~1^{tree}"]),
    );
    f.tag(&tag_message(&m), "HEAD");
    fails(&f.verify(), "points at");
}

#[test]
fn a_manifest_naming_another_tree_fails() {
    let f = Fixture::new("tree");
    let m = f.manifest(&f.head(), &"0".repeat(40));
    // A well-formed tree id that is not this commit's tree.
    f.publish(&m);
    f.tag(&tag_message(&m), "HEAD");
    fails(&f.verify(), "has tree");
}

#[test]
fn a_tag_naming_another_manifest_fails() {
    let f = Fixture::new("message");
    f.tag(
        "rustev 0.1.0\n\nrelease-manifest-sha256: 00\npackages: pkg-a pkg-b\n",
        "HEAD",
    );
    fails(&f.verify(), "tag message");
}

#[test]
fn an_unsigned_or_lightweight_tag_fails() {
    let f = Fixture::new("unsigned");
    let msg = f.root.join("m");
    let m = f.manifest(&f.head(), &f.tree());
    std::fs::write(&msg, tag_message(&m)).unwrap();
    git(
        &f.repo,
        &[
            "tag",
            "-f",
            "-a",
            "v0.1.0",
            "-F",
            &msg.display().to_string(),
        ],
    );
    fails(&f.verify(), "tag signature");
    git(&f.repo, &["tag", "-f", "v0.1.0"]);
    fails(&f.verify(), "not an annotated tag");
}

#[test]
fn a_non_canonical_manifest_is_refused() {
    let f = Fixture::new("canonical");
    let m = f.manifest(&f.head(), &f.tree());
    let pretty = serde_json::to_vec_pretty(&m).unwrap();
    assert!(ReleaseManifest::parse(&pretty).is_err());
    let mut other = m.clone();
    other.schema = "rustev.release-manifest/0".into();
    assert!(ReleaseManifest::parse(&other.canonical()).is_err());
    assert_eq!(ReleaseManifest::parse(&m.canonical()).unwrap(), m);
}

#[test]
fn the_documented_owner_identity_is_the_committed_allowed_signer() {
    let signers = rustev_release::manifest::ALLOWED_SIGNERS;
    let line = signers.lines().next().unwrap();
    assert!(line.starts_with(rustev_release::manifest::OWNER_IDENTITY));
    assert!(line.contains(" ssh-ed25519 "));
    assert!(!signers.contains("PRIVATE"));
}

#[test]
fn names_from_the_manifest_are_refused_unless_plain() {
    let f = Fixture::new("names");
    let mut m = f.manifest(&f.head(), &f.tree());
    m.packages[0].crate_file = "../../etc/passwd".into();
    f.publish(&m);
    fails(&f.verify(), "is not <member>-<version>.crate");
    let mut m = f.manifest(&f.head(), &f.tree());
    m.tag = "--help".into();
    f.publish(&m);
    fails(&f.verify(), "is not exactly v<version>");
    let m = f.manifest("HEAD", &f.tree());
    f.publish(&m);
    fails(&f.verify(), "must be full object ids");
}

// Spec 031 3.11.2: workflow bytes share the signed manifest binding.
#[test]
fn signed_workflow_artifact_substitution_and_absence_fail() {
    let f = Fixture::new("workflow");
    let bytes = b"release notes";
    let mut m = f.manifest(&f.head(), &f.tree());
    m.artifacts
        .push(rustev_release::manifest::WorkflowArtifact {
            kind: "changelog".into(),
            name: "changelog.md".into(),
            layer: 8,
            bytes: bytes.len() as u64,
            sha256: rustev_release::archive::sha256_hex(bytes),
            package: None,
            integrity: None,
            sbom: None,
            slsa_build_level: None,
        });
    std::fs::write(f.dir.join("changelog.md"), bytes).unwrap();
    f.publish(&m);
    f.tag(&tag_message(&m), &f.head());
    assert!(f.verify().is_empty());
    std::fs::write(f.dir.join("changelog.md"), b"substitution").unwrap();
    fails(&f.verify(), "changelog.md");
    std::fs::remove_file(f.dir.join("changelog.md")).unwrap();
    fails(&f.verify(), "changelog.md");
}

// Spec 053 fixtures use the checkout's signers file, never an override.
impl Fixture {
    fn candidate_dispatch(
        &self,
        rehearsal: bool,
        publish: bool,
        tag: &str,
    ) -> std::process::Output {
        use std::os::unix::fs::PermissionsExt;
        let tool = self.repo.join("tools/rustev-release");
        std::fs::create_dir_all(&tool).unwrap();
        std::fs::copy(&self.signers, tool.join("allowed_signers")).unwrap();
        std::fs::write(
            tool.join("release-set.json"),
            include_bytes!("../release-set.json"),
        )
        .unwrap();
        let bin = self.root.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("cargo"), "#!/bin/sh\nwhile [ \"$1\" != -- ]; do shift; done\nshift\nexec \"$RELEASE_BINARY\" \"$@\"\n").unwrap();
        std::fs::set_permissions(bin.join("cargo"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        Command::new("sh")
            .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/artifacts.sh"))
            .args([
                "dispatch",
                if rehearsal { "true" } else { "false" },
                if publish { "true" } else { "false" },
                tag,
            ])
            .current_dir(&self.repo)
            .env(
                "PATH",
                format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()),
            )
            .env("RELEASE_BINARY", env!("CARGO_BIN_EXE_rustev-release"))
            .env("GITHUB_REF", format!("refs/tags/{tag}"))
            .env("GITHUB_SHA", self.head())
            .output()
            .unwrap()
    }
    fn candidate_tag(&self, kind: &str) {
        match kind {
            "lightweight" => {
                git(&self.repo, &["tag", &candidate()]);
            }
            "unsigned" => {
                git(&self.repo, &["tag", "-a", &candidate(), "-m", "candidate"]);
            }
            _ => {
                git(&self.repo, &["tag", "-s", &candidate(), "-m", "candidate"]);
            }
        }
    }
}
#[test]
fn candidate_dispatch_rejects_lightweight_unsigned_and_foreign_signer() {
    for kind in ["lightweight", "unsigned", "foreign"] {
        let f = Fixture::new(&format!("candidate-{kind}"));
        git(&f.repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
        if kind == "foreign" {
            let other = keygen(&f.root, "other");
            git(
                &f.repo,
                &["config", "user.signingkey", other.to_str().unwrap()],
            );
        }
        f.candidate_tag(kind);
        let out = f.candidate_dispatch(false, false, &candidate());
        assert!(!out.status.success());
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("021 3.8"),
            "{out:?}"
        );
    }
}
#[test]
fn candidate_dispatch_main_reachability_and_valid_signed_tag() {
    let f = Fixture::new("candidate-main");
    // A descendant as main is sufficient: the candidate need not be its tip.
    git(&f.repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    f.candidate_tag("signed");
    let out = f.candidate_dispatch(false, false, &candidate());
    assert!(out.status.success(), "{out:?}");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains(&format!("version={}\n", &candidate()[1..])));
    assert!(text.contains("candidate=true\n"));
    // An unrelated root commit, created without moving the fixture HEAD.
    let tree = f.tree();
    let unrelated = git(&f.repo, &["commit-tree", &tree, "-m", "unrelated"]);
    git(
        &f.repo,
        &["update-ref", "refs/remotes/origin/main", &unrelated],
    );
    let out = f.candidate_dispatch(false, false, &candidate());
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("053 3.2.4"));
}
#[test]
fn candidate_rehearsal_before_tag_and_publication_refusal() {
    let f = Fixture::new("candidate-rehearsal");
    git(&f.repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    let out = f.candidate_dispatch(true, false, &candidate());
    assert!(out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains(&format!("version={}", &candidate()[1..]))
    );
    for rehearsal in [false, true] {
        let out = f.candidate_dispatch(rehearsal, true, &candidate());
        assert!(!out.status.success());
        assert!(String::from_utf8_lossy(&out.stderr).contains("053 3.4.2"));
    }
    assert!(
        !f.candidate_dispatch(true, false, &release_set().tag)
            .status
            .success()
    );
    assert!(f.candidate_dispatch(true, false, "").status.success());
    let unrelated = git(&f.repo, &["commit-tree", &f.tree(), "-m", "unrelated"]);
    git(
        &f.repo,
        &["update-ref", "refs/remotes/origin/main", &unrelated],
    );
    let out = f.candidate_dispatch(true, false, &candidate());
    assert!(String::from_utf8_lossy(&out.stderr).contains("053 3.1.1"));
}
#[test]
fn tag_manifest_missing_mismatch_absent_digest_and_match() {
    let f = Fixture::new("candidate-digest");
    let file = f.root.join("candidate.json");
    let check = || {
        Command::new("sh")
            .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/artifacts.sh"))
            .arg("tag-manifest")
            .arg("v0.1.0")
            .arg(&file)
            .current_dir(&f.repo)
            .output()
            .unwrap()
    };
    assert!(String::from_utf8_lossy(&check().stderr).contains("053 3.3.1"));
    std::fs::write(&file, b"").unwrap();
    assert!(String::from_utf8_lossy(&check().stderr).contains("053 3.3.1"));
    std::fs::write(&file, b"manifest").unwrap();
    assert!(String::from_utf8_lossy(&check().stderr).contains("031 3.11.2"));
    f.tag("no digest", "HEAD");
    assert!(String::from_utf8_lossy(&check().stderr).contains("021 3.8"));
    f.tag(
        &format!(
            "release-manifest-sha256: {}",
            rustev_release::archive::sha256_hex(b"manifest")
        ),
        "HEAD",
    );
    assert!(check().status.success());
}
#[test]
fn npm_guard_refuses_candidate_allows_release() {
    for (tag, success) in [("v0.1.0-rc.1", false), ("v0.1.0", true)] {
        let out = Command::new("sh")
            .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/artifacts.sh"))
            .args(["npm-guard", tag])
            .output()
            .unwrap();
        assert_eq!(out.status.success(), success);
        if !success {
            assert!(String::from_utf8_lossy(&out.stderr).contains("053 3.4.2"));
        }
    }
}

#[test]
fn later_instructions_require_the_owner_signature_over_exact_bytes() {
    use rustev_release::lifecycle::{Event, Instruction, Journal, Lifecycle, State};
    let f = Fixture::new("instructions");
    for case in ["valid", "unsigned", "wrong-key", "altered", "first"] {
        let mut m = f.manifest(&f.head(), &f.tree());
        if case != "first" {
            m.version = "0.2.0".into();
            m.tag = "v0.2.0".into();
        }
        let i = Instruction {
            version: m.version.clone(),
            commit: m.commit.clone(),
            registry: m.registry.clone(),
            packages: m.upload_order.clone(),
            account: "owner".into(),
            registries: m.registries.clone(),
            artifacts: vec![],
            continuation: false,
            action: "publish".into(),
        };
        let path = f.root.join(format!("{case}.json"));
        let mut bytes = serde_json::to_vec(&i).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        match case {
            "valid" | "altered" => sign(&f.key, &path),
            "wrong-key" => sign(&keygen(&f.root, "stranger"), &path),
            _ => {}
        }
        if case == "altered" {
            bytes.push(b'\n'); // Same parsed instruction, different signed bytes.
            std::fs::write(&path, &bytes).unwrap();
        }
        let mut life = Lifecycle::new(m.clone());
        life.apply(Event::GatesPass).unwrap();
        life.apply(Event::CandidateVerified {
            commit: m.commit.clone(),
        })
        .unwrap();
        let journal_path = f.root.join(format!("{case}.journal"));
        std::fs::write(&journal_path, b"").unwrap();
        let mut journal = Journal {
            file: Some(journal_path.clone()),
            ..Journal::default()
        };
        let before = std::fs::read(&journal_path).unwrap();
        let result = life.authorize_file(&path, &f.signers, IDENTITY);
        if matches!(case, "valid" | "first") {
            result.unwrap();
            assert_eq!(life.state, State::PublicationAuthorized);
        } else {
            let error = result.unwrap_err();
            assert!(
                error.contains("instruction") && error.contains("signature"),
                "{error}"
            );
            assert_eq!(life.state, State::CandidateVerified);
            assert!(journal.entries.is_empty());
            assert_eq!(std::fs::read(&journal_path).unwrap(), before);
        }
        // As in both production entry points, binding follows authorization.
        if life.state == State::PublicationAuthorized {
            rustev_release::family_lifecycle::bind(&m, "verified manifest fixture", &mut journal)
                .unwrap();
            assert_eq!(journal.entries.len(), 1);
        }
    }
}
