//! Spec 058: one tagged run releases the family.
//!
//! [`check_tag`] is the `tag` job's refusal before any build (3.1.1).
//! [`publish_items`] is each registry publication step, idempotent against
//! public state (3.3, 3.5.2), recording every item in `publication.json`
//! (3.5.4). [`publish_release`] creates the run's GitHub release once
//! (3.3.3), and [`verify_assets`] checks a release directory against the
//! run's manifest (3.4.1). [`qualify`] records the lifecycle from the run's
//! manifest and publication record, with no instruction or journal (3.4.2).
//!
//! Registry and release state come through [`Publisher`] and
//! [`Releases`], so tests drive every rule offline. Nothing here reads a
//! registry token: `cargo` and `npm` read the trusted-publishing
//! credentials the workflow gives them (3.3.4).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::archive::sha256_hex;
use crate::lifecycle::{Event, Lifecycle, Lookup, State, redact};
use crate::manifest::{ReleaseManifest, sha256sums};
use crate::prerelease::{Assets, Outcome, Releases};

pub const RECORD_SCHEMA: &str = "rustev.publication-record/1";
pub const QUALIFICATION_SCHEMA: &str = "rustev.qualification-record/1";
const RELEASE_SET_PATH: &str = "tools/rustev-release/release-set.json";

/// What `tag` hands the run: the verified tag's identity (3.1.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagFacts {
    pub commit: String,
    pub tag_object: String,
    /// `X.Y.Z`, or `X.Y.Z-rc.N` for a candidate (053 3.2.3).
    pub version: String,
    pub candidate: bool,
    pub epoch: u64,
}

impl TagFacts {
    /// `key=value` lines for `$GITHUB_OUTPUT`.
    pub fn outputs(&self) -> String {
        format!(
            "commit={}\ntag_object={}\nversion={}\ncandidate={}\nepoch={}\n",
            self.commit, self.tag_object, self.version, self.candidate, self.epoch
        )
    }
}

fn number(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s == "0" || !s.starts_with('0'))
}

/// `vX.Y.Z` gives `(X.Y.Z, None)`; `vX.Y.Z-rc.N` gives `(X.Y.Z, Some(N))`.
pub fn parse_tag(tag: &str) -> Result<(String, Option<String>), String> {
    let refuse = || format!("058 3.1.1: {tag:?} is not vX.Y.Z or vX.Y.Z-rc.N");
    let rest = tag.strip_prefix('v').ok_or_else(refuse)?;
    let (base, rc) = match rest.split_once("-rc.") {
        Some((b, n)) if number(n) && n != "0" => (b, Some(n.to_string())),
        Some(_) => return Err(refuse()),
        None => (rest, None),
    };
    let parts: Vec<&str> = base.split('.').collect();
    if parts.len() != 3 || !parts.iter().all(|p| number(p)) {
        return Err(refuse());
    }
    Ok((base.to_string(), rc))
}

/// 3.1.1.5: some `ci-gate` check run on `commit` concluded `success`, in
/// the GitHub check-runs response (`commits/<sha>/check-runs`).
pub fn ci_gate(check_runs: &[u8], commit: &str) -> Result<(), String> {
    let v: serde_json::Value = serde_json::from_slice(check_runs)
        .map_err(|e| format!("058 3.1.1: unreadable check runs: {e}"))?;
    let runs = v["check_runs"]
        .as_array()
        .ok_or("058 3.1.1: the check-runs response lists no check_runs")?;
    if runs.iter().any(|r| {
        r["name"] == "ci-gate"
            && r["head_sha"] == commit
            && r["status"] == "completed"
            && r["conclusion"] == "success"
    }) {
        Ok(())
    } else {
        Err(format!(
            "058 3.1.1: ci-gate did not conclude success on {commit}"
        ))
    }
}

/// Every check of 3.1.1, in `repo`, before any build. `main_ref` is the
/// ref `main` is read from (`refs/remotes/origin/main` in the workflow);
/// `check_runs` is the commit's check-runs response.
pub fn check_tag(
    repo: &Path,
    tag: &str,
    main_ref: &str,
    allowed_signers: &Path,
    check_runs: &[u8],
) -> Result<TagFacts, String> {
    let (base, rc) = parse_tag(tag)?;
    let git = |args: &[&str]| crate::manifest::git(repo, args, allowed_signers);
    let tag_ref = format!("refs/tags/{tag}");
    match git(&["cat-file", "-t", &tag_ref]) {
        Ok(t) if t == "tag" => {}
        Ok(t) => return Err(format!("058 3.1.1: {tag} is a {t}, not an annotated tag")),
        Err(e) => return Err(format!("058 3.1.1: no tag {tag}: {e}")),
    }
    git(&["verify-tag", &tag_ref])
        .map_err(|e| format!("058 3.1.1: the signature of {tag} does not verify: {e}"))?;
    let tag_object = git(&["rev-parse", &tag_ref])?;
    let commit = git(&["rev-parse", &format!("{tag_ref}^{{commit}}")])?;
    let set = git(&["show", &format!("{commit}:{RELEASE_SET_PATH}")])
        .and_then(|text| crate::ReleaseSet::parse(text.as_bytes()))
        .map_err(|e| format!("058 3.1.1: the release set at {commit}: {e}"))?;
    if set.version != base {
        return Err(format!(
            "058 3.1.1: {tag} names {base}, the release set at {commit} is {}",
            set.version
        ));
    }
    git(&["merge-base", "--is-ancestor", &commit, main_ref])
        .map_err(|_| format!("058 3.1.1: {commit} is not reachable from {main_ref}"))?;
    ci_gate(check_runs, &commit)?;
    let epoch = git(&["log", "-1", "--format=%ct", &commit])?
        .parse()
        .map_err(|e| format!("commit time: {e}"))?;
    Ok(TagFacts {
        version: match &rc {
            Some(n) => format!("{base}-rc.{n}"),
            None => base,
        },
        candidate: rc.is_some(),
        commit,
        tag_object,
        epoch,
    })
}

/// One registry item: a `.crate` or an npm tarball of the run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub kind: String,
    pub name: String,
    pub file: String,
    pub sha256: String,
    /// The registry's checksum form: the index `cksum` (the archive's
    /// SHA-256) or npm's `dist.integrity`.
    pub expected: String,
}

/// One line of `publication.json` (3.5.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub kind: String,
    pub name: String,
    pub file: String,
    pub sha256: String,
    pub observed: String,
    /// `published`, or `present` when found already public.
    pub outcome: String,
    pub time: String,
}

/// `publication.json`: `rustev.publication-record/1`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub schema: String,
    pub version: String,
    pub tag: String,
    pub commit: String,
    pub manifest_sha256: String,
    pub entries: Vec<Entry>,
}

impl Record {
    pub fn new(m: &ReleaseManifest) -> Self {
        Record {
            schema: RECORD_SCHEMA.into(),
            version: m.version.clone(),
            tag: m.tag.clone(),
            commit: m.commit.clone(),
            manifest_sha256: m.digest(),
            entries: Vec::new(),
        }
    }

    /// The record at `path` when it exists (an earlier job's), else a new
    /// one; an existing record must be this manifest's.
    pub fn load(path: &Path, m: &ReleaseManifest) -> Result<Self, String> {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::new(m)),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        let r: Record = serde_json::from_slice(&bytes)
            .map_err(|e| format!("058 3.5.4: unreadable publication record: {e}"))?;
        let fresh = Self::new(m);
        if r.schema != fresh.schema
            || (&r.version, &r.tag, &r.commit, &r.manifest_sha256)
                != (
                    &fresh.version,
                    &fresh.tag,
                    &fresh.commit,
                    &fresh.manifest_sha256,
                )
        {
            return Err("058 3.5.4: the publication record is another release's".into());
        }
        Ok(r)
    }

    pub fn write(&self, path: &Path) -> Result<(), String> {
        let mut text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        text.push('\n');
        std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// The observed checksum recorded for `item`, if any.
    pub fn observed(&self, item: &Item) -> Option<&str> {
        self.entries
            .iter()
            .rev()
            .find(|e| e.kind == item.kind && e.name == item.name)
            .map(|e| e.observed.as_str())
    }
}

/// A registry the run publishes to.
pub trait Publisher {
    fn lookup(&mut self, item: &Item, version: &str) -> Lookup;
    fn publish(&mut self, item: &Item, version: &str) -> Result<(), String>;
    fn wait(&mut self);
    fn now(&mut self) -> String;
}

/// Polls of the registry after an upload before the run fails.
pub const POLLS: u32 = 60;

/// Publish `items` in order (3.3), each idempotent against public state
/// (3.5.2): present with the run's checksum is skipped, present with another
/// fails, absent is published and must then appear with the run's checksum
/// before the next item starts. Each item reached is recorded in `record`,
/// which the caller writes whatever the result.
pub fn publish_items(
    items: &[Item],
    version: &str,
    polls: u32,
    p: &mut dyn Publisher,
    record: &mut Record,
) -> Result<(), String> {
    for item in items {
        let id = format!("{} {}@{version}", item.kind, item.name);
        let outcome = match p.lookup(item, version) {
            Lookup::Visible { checksum } if checksum == item.expected => "present",
            Lookup::Visible { checksum } => {
                return Err(format!(
                    "058 3.5.2: {id} is public with {checksum}, not the run's {}; nothing is republished",
                    item.expected
                ));
            }
            Lookup::Unknown { detail } => {
                return Err(format!(
                    "058 3.5.2: the public state of {id} is unknown ({}); nothing is published",
                    redact(&detail)
                ));
            }
            Lookup::Absent => {
                // A failed command may still have reached the registry; the
                // polls below, not the exit status, decide.
                let failure = p.publish(item, version).err();
                let mut seen = None;
                for _ in 0..polls.max(1) {
                    match p.lookup(item, version) {
                        Lookup::Visible { checksum } if checksum == item.expected => {
                            seen = Some(());
                            break;
                        }
                        Lookup::Visible { checksum } => {
                            return Err(format!(
                                "058 3.3.1: {id} is listed with {checksum}, not the run's {}",
                                item.expected
                            ));
                        }
                        _ if failure.is_some() => break,
                        _ => p.wait(),
                    }
                }
                match (seen, failure) {
                    (Some(()), _) => "published",
                    (None, Some(e)) => {
                        return Err(format!("058 3.3: {id} was not published: {}", redact(&e)));
                    }
                    (None, None) => {
                        return Err(format!(
                            "058 3.3.1: {id} is not visible with the run's checksum after {polls} polls"
                        ));
                    }
                }
            }
        };
        let time = p.now();
        record.entries.push(Entry {
            kind: item.kind.clone(),
            name: item.name.clone(),
            file: item.file.clone(),
            sha256: item.sha256.clone(),
            observed: item.expected.clone(),
            outcome: outcome.into(),
            time,
        });
    }
    Ok(())
}

/// The manifest's items of `kind` (`crate` or `npm`) in publication order,
/// each read from `dir` and required to be the run's verified bytes.
pub fn items(m: &ReleaseManifest, kind: &str, dir: Option<&Path>) -> Result<Vec<Item>, String> {
    let list: Vec<(Item, u64)> = match kind {
        "crate" => m
            .packages
            .iter()
            .map(|p| {
                let item = Item {
                    kind: kind.into(),
                    name: p.name.clone(),
                    file: p.crate_file.clone(),
                    sha256: p.sha256.clone(),
                    expected: p.sha256.clone(),
                };
                (item, p.bytes)
            })
            .collect(),
        "npm" => m
            .artifacts
            .iter()
            .filter(|a| a.kind == "npm")
            .map(|a| {
                // A manifest npm entry names its package and registry
                // integrity; one without either cannot be published or
                // observed, so it is refused rather than compared as empty.
                let (Some(name), Some(expected)) = (a.package.clone(), a.integrity.clone()) else {
                    return Err(format!(
                        "{}: npm artifact without package or integrity",
                        a.name
                    ));
                };
                let item = Item {
                    kind: kind.into(),
                    name,
                    file: a.name.clone(),
                    sha256: a.sha256.clone(),
                    expected,
                };
                Ok((item, a.bytes))
            })
            .collect::<Result<_, String>>()?,
        _ => return Err(format!("unknown item kind {kind}")),
    };
    let mut out = Vec::new();
    for (item, bytes) in list {
        if let Some(dir) = dir {
            let b =
                std::fs::read(dir.join(&item.file)).map_err(|e| format!("{}: {e}", item.file))?;
            if sha256_hex(&b) != item.sha256 || b.len() as u64 != bytes {
                return Err(format!(
                    "058 3.3: {} is not the run's verified artifact",
                    item.file
                ));
            }
        }
        out.push(item);
    }
    Ok(out)
}

/// RFC 3339 UTC of `secs` since the epoch.
pub fn rfc3339(secs: u64) -> String {
    format!(
        "{}T{:02}:{:02}:{:02}Z",
        crate::audit::civil_date(secs),
        secs / 3600 % 24,
        secs / 60 % 60,
        secs % 60
    )
}

fn now() -> String {
    rfc3339(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    )
}

fn run(cmd: &mut Command) -> Result<(), String> {
    let status = cmd.status().map_err(|e| format!("{cmd:?}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{:?} exited {status}", cmd.get_program()))
    }
}

/// crates.io: the sparse index, and `cargo publish --locked` from the
/// tagged checkout (3.3.1).
pub struct Cargo {
    pub wait_seconds: u64,
}

impl Publisher for Cargo {
    fn lookup(&mut self, item: &Item, version: &str) -> Lookup {
        crate::lifecycle::Registry::lookup(
            &mut crate::cratesio::CratesIo { wait_seconds: 0 },
            &item.name,
            version,
        )
    }
    fn publish(&mut self, item: &Item, _: &str) -> Result<(), String> {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        run(Command::new(cargo).args(["publish", "--locked", "-p", &item.name]))
    }
    fn wait(&mut self) {
        std::thread::sleep(std::time::Duration::from_secs(self.wait_seconds));
    }
    fn now(&mut self) -> String {
        now()
    }
}

/// npm: `dist.integrity`, and `npm publish --provenance` of the run's
/// tarball in `dir` (3.3.2).
pub struct Npm {
    pub dir: PathBuf,
    pub wait_seconds: u64,
}

impl Publisher for Npm {
    fn lookup(&mut self, item: &Item, version: &str) -> Lookup {
        crate::hosted::npm_lookup(&item.name, version)
    }
    fn publish(&mut self, item: &Item, _: &str) -> Result<(), String> {
        run(Command::new("npm")
            .arg("publish")
            .arg(self.dir.join(&item.file))
            .args(["--provenance", "--access", "public"]))
    }
    fn wait(&mut self) {
        std::thread::sleep(std::time::Duration::from_secs(self.wait_seconds));
    }
    fn now(&mut self) -> String {
        now()
    }
}

/// The files a release of `m` holds, beside their `.intoto.jsonl` bundles.
fn attested(m: &ReleaseManifest) -> Vec<String> {
    m.packages
        .iter()
        .map(|p| p.crate_file.clone())
        .chain(m.artifacts.iter().map(|a| a.name.clone()))
        .chain(["release-manifest.json".into(), "SHA256SUMS".into()])
        .collect()
}

/// Every asset of the release of `m` (3.3.3) read from `dir`, or every
/// problem found: each artifact present with the manifest's length and
/// SHA-256, `SHA256SUMS` listing exactly them, each binary's `.sha256`
/// sidecar, a non-empty Sigstore bundle per attested file, and, for a
/// release, `publication.json`.
pub fn verify_assets(m: &ReleaseManifest, dir: &Path) -> Result<Assets, Vec<String>> {
    let mut problems = Vec::new();
    let mut assets = Assets::new();
    let mut take =
        |name: String, want: Option<(u64, &str)>, problems: &mut Vec<String>| match std::fs::read(
            dir.join(&name),
        ) {
            Ok(b) if b.is_empty() => problems.push(format!("058 3.4.1: {name} is empty")),
            Ok(b) => {
                if want.is_some_and(|(n, d)| b.len() as u64 != n || sha256_hex(&b) != d) {
                    problems.push(format!("058 3.4.1: {name} differs from the manifest"));
                }
                assets.insert(name, b);
            }
            Err(e) => problems.push(format!("058 3.4.1: {name}: {e}")),
        };
    for p in &m.packages {
        take(
            p.crate_file.clone(),
            Some((p.bytes, &p.sha256)),
            &mut problems,
        );
    }
    for a in &m.artifacts {
        take(a.name.clone(), Some((a.bytes, &a.sha256)), &mut problems);
        if a.kind == "binary" {
            take(format!("{}.sha256", a.name), None, &mut problems);
        }
    }
    take("release-manifest.json".into(), None, &mut problems);
    take("SHA256SUMS".into(), None, &mut problems);
    for name in attested(m) {
        take(format!("{name}.intoto.jsonl"), None, &mut problems);
    }
    let release = !crate::candidate::is_candidate_version(&m.version);
    if release {
        take("publication.json".into(), None, &mut problems);
    }
    if assets
        .get("release-manifest.json")
        .is_some_and(|b| *b != m.canonical())
    {
        problems.push("058 3.4.1: release-manifest.json is not this manifest".into());
    }
    if assets
        .get("SHA256SUMS")
        .is_some_and(|b| *b != sha256sums(m).into_bytes())
    {
        problems.push("058 3.4.1: SHA256SUMS does not list exactly the manifest's files".into());
    }
    for a in m.artifacts.iter().filter(|a| a.kind == "binary") {
        let want = format!("{}  {}\n", a.sha256, a.name);
        if assets
            .get(&format!("{}.sha256", a.name))
            .is_some_and(|b| *b != want.as_bytes())
        {
            problems.push(format!("058 3.4.1: {}.sha256 differs", a.name));
        }
    }
    if release
        && let Some(b) = assets.get("publication.json")
        && let Err(e) = released(m, b)
    {
        problems.push(e);
    }
    if problems.is_empty() {
        Ok(assets)
    } else {
        Err(problems)
    }
}

/// 3.5.4: `publication.json` records every crate and npm package of `m`
/// observed public with the run's checksum.
pub fn released(m: &ReleaseManifest, record: &[u8]) -> Result<Record, String> {
    let r: Record = serde_json::from_slice(record)
        .map_err(|e| format!("058 3.5.4: unreadable publication record: {e}"))?;
    let fresh = Record::new(m);
    if r.schema != fresh.schema || r.manifest_sha256 != fresh.manifest_sha256 {
        return Err("058 3.5.4: the publication record is another release's".into());
    }
    for item in items(m, "crate", None)?
        .iter()
        .chain(&items(m, "npm", None)?)
    {
        if r.observed(item) != Some(item.expected.as_str()) {
            return Err(format!(
                "058 3.5.4: {} {} is not recorded public with the run's checksum",
                item.kind, item.name
            ));
        }
    }
    Ok(r)
}

/// 3.3.3, 3.5.2: create the release `tag` once with `assets`; one already
/// present with exactly these assets and flag is success, any difference
/// fails and replaces nothing.
pub fn publish_release(
    api: &mut dyn Releases,
    tag: &str,
    notes: &str,
    assets: &Assets,
    prerelease: bool,
) -> Result<Outcome, String> {
    if assets.is_empty() || assets.keys().any(|n| !crate::family_artifacts::plain(n)) {
        return Err("058 3.3.3: missing assets or invalid asset name".into());
    }
    let digests: BTreeMap<String, String> = assets
        .iter()
        .map(|(n, b)| (n.clone(), sha256_hex(b)))
        .collect();
    if let Some(present) = api.lookup(tag)? {
        if present.prerelease != prerelease || present.assets != digests {
            return Err(format!(
                "058 3.5.2: the release {tag} exists with other assets or flag; nothing is replaced"
            ));
        }
        return Ok(Outcome::Present);
    }
    api.create_release(tag, notes, assets, prerelease)?;
    Ok(Outcome::Created)
}

/// The release notes: the changelog, or a candidate's limitation (053 3.4.3).
pub fn notes(m: &ReleaseManifest, assets: &Assets) -> Result<String, String> {
    if crate::candidate::is_candidate_version(&m.version) {
        return Ok(format!(
            "{}\n\nrelease-manifest-sha256: {}\n",
            crate::candidate::LIMITATION,
            m.digest()
        ));
    }
    let changelog = assets.get("changelog.md").ok_or("031 3.10: no changelog")?;
    String::from_utf8(changelog.clone()).map_err(|e| format!("changelog.md: {e}"))
}

/// The lifecycle record `qualify` prints (3.4.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Qualification {
    pub schema: String,
    pub version: String,
    pub tag: String,
    pub commit: String,
    pub manifest_sha256: String,
    pub state: State,
    pub evidence: BTreeMap<String, String>,
}

/// 3.4.2: the lifecycle of the tagged run whose manifest, `SHA256SUMS` and
/// `publication.json` are in `dir`, qualified by the step files in
/// `evidence_dir`. `tag_object` is the local tag's object id, which the
/// manifest must name. No instruction and no journal is read.
pub fn qualify(dir: &Path, evidence_dir: &Path, tag_object: &str) -> Result<Qualification, String> {
    let read = |n: &str| std::fs::read(dir.join(n)).map_err(|e| format!("{n}: {e}"));
    let m = crate::candidate::publication_manifest(&read("release-manifest.json")?)?;
    if read("SHA256SUMS")? != sha256sums(&m).into_bytes() {
        return Err("058 3.4.1: SHA256SUMS does not list exactly the manifest's files".into());
    }
    released(&m, &read("publication.json")?)?;
    let evidence = crate::qualification::evidence(&m, evidence_dir)?;
    let mut life = Lifecycle::new(m.clone());
    let step = |life: &mut Lifecycle, e: Event| life.apply(e).map(|_| ()).map_err(|r| r.0);
    step(&mut life, Event::GatesPass)?;
    step(
        &mut life,
        Event::CandidateVerified {
            commit: m.commit.clone(),
        },
    )?;
    step(
        &mut life,
        Event::TagAuthorized {
            tag_object: tag_object.into(),
        },
    )?;
    // Every registry item is recorded public above, and the release holds
    // the record: layers 1 to 8 are observed.
    life.family_observed = true;
    step(&mut life, Event::Observed { complete: true })?;
    step(&mut life, Event::ReleaseCreated)?;
    life.consumer_recorded = true;
    step(&mut life, Event::ConsumerQualified)?;
    Ok(Qualification {
        schema: QUALIFICATION_SCHEMA.into(),
        version: m.version.clone(),
        tag: m.tag.clone(),
        commit: m.commit.clone(),
        manifest_sha256: m.digest(),
        state: life.state,
        evidence,
    })
}
