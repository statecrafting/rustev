//! Spec 031 3.1 and 3.11: the full family uses the same journal rules as 021.
use crate::{
    archive::sha256_hex,
    lifecycle::{Entry, Journal, Lifecycle, Lookup, State, Upload, VISIBILITY_POLLS, redact},
    manifest::ReleaseManifest,
};
use std::collections::BTreeMap;

/// Spec 031 3.11.2: call only after signature verification (fakes supply fixtures).
pub fn bind(m: &ReleaseManifest, signature: &str, j: &mut Journal) -> Result<(), String> {
    if signature.is_empty() {
        return Err("missing manifest signature".into());
    }
    if j.entries
        .iter()
        .any(|e| matches!(e, Entry::SignedManifest { .. }))
    {
        return bound(m, j);
    }
    // 3.11: the signed manifest is journaled before the first upload of any
    // kind, so a journal with attempts but no signed manifest is refused, not
    // resumed (no release was published under the unsigned `/1` form).
    if j.entries
        .iter()
        .any(|e| matches!(e, Entry::Attempt { .. } | Entry::Result { .. }))
    {
        return Err("upload predates signed manifest".into());
    }
    j.record(Entry::SignedManifest {
        manifest: String::from_utf8(m.canonical()).map_err(|e| e.to_string())?,
        signature: signature.into(),
    });
    bound(m, j)
}

pub fn bound(m: &ReleaseManifest, j: &Journal) -> Result<(), String> {
    if j.broken() {
        return Err("journal cannot be written".into());
    }
    let mut found = false;
    for e in &j.entries {
        match e {
            Entry::SignedManifest {
                manifest,
                signature,
            } => {
                if signature.is_empty() {
                    return Err("journaled manifest carries no signature".into());
                }
                if manifest.as_bytes() != m.canonical() {
                    return Err(
                        "journal binds another manifest; version or artifact set cannot change"
                            .into(),
                    );
                }
                found = true;
            }
            // Queries count too: any registry contact under the version
            // belongs to the bound manifest's run, never before it.
            Entry::Attempt { .. } | Entry::Result { .. } | Entry::Query { .. } if !found => {
                return Err("registry activity predates signed manifest".into());
            }
            _ => {}
        }
    }
    if found {
        Ok(())
    } else {
        Err("signed manifest must be journaled before upload".into())
    }
}

/// Spec 031 3.1: a checksum is SHA-256 except npm, whose registry uses SRI.
#[derive(Debug, Clone)]
pub struct Item {
    pub kind: String,
    pub name: String,
    pub file: String,
    pub layer: u8,
    pub checksum: String,
    pub bytes: Vec<u8>,
}

pub trait FamilyRegistry {
    fn lookup(&mut self, item: &Item, version: &str) -> Lookup;
    fn upload(&mut self, item: &Item, version: &str) -> Upload;
    fn wait(&mut self);
}

/// Spec 031 3.1: signed manifest, signatures and checksum inventory are
/// terminal assets; their hashes cannot recursively appear in the manifest.
pub fn items(m: &ReleaseManifest, files: &BTreeMap<String, Vec<u8>>) -> Result<Vec<Item>, String> {
    if files
        .get("release-manifest.json")
        .is_none_or(|b| *b != m.canonical())
        || files
            .get("SHA256SUMS")
            .is_none_or(|b| *b != crate::manifest::sha256sums(m).as_bytes())
    {
        return Err("terminal assets differ from manifest".into());
    }
    let mut out = Vec::new();
    for p in &m.packages {
        let b = files
            .get(&p.crate_file)
            .ok_or_else(|| format!("missing {}", p.crate_file))?;
        if b.len() as u64 != p.bytes || sha256_hex(b) != p.sha256 {
            return Err(format!("{} differs", p.crate_file));
        }
        out.push(Item {
            kind: "crate".into(),
            name: p.name.clone(),
            file: p.crate_file.clone(),
            layer: (m
                .layers
                .iter()
                .position(|layer| layer.contains(&p.name))
                .ok_or("crate missing from layers")?
                + 1) as u8,
            checksum: p.sha256.clone(),
            bytes: b.clone(),
        });
    }
    for a in &m.artifacts {
        let b = files
            .get(&a.name)
            .ok_or_else(|| format!("missing {}", a.name))?;
        if b.len() as u64 != a.bytes || sha256_hex(b) != a.sha256 {
            return Err(format!("{} differs", a.name));
        }
        out.push(Item {
            kind: a.kind.clone(),
            name: a.package.clone().unwrap_or(a.name.clone()),
            file: a.name.clone(),
            layer: a.layer,
            checksum: a.integrity.clone().unwrap_or(a.sha256.clone()),
            bytes: b.clone(),
        });
        if a.kind == "binary" {
            out.push(hosted_item(
                format!("{}.sha256", a.name),
                format!("{}  {}\n", a.sha256, a.name).into_bytes(),
                7,
            ));
        }
    }
    for name in [
        "release-manifest.json",
        "release-manifest.json.sig",
        "SHA256SUMS",
        "SHA256SUMS.sig",
        "journal-summary.jsonl",
    ] {
        out.push(hosted_item(
            name.into(),
            files
                .get(name)
                .ok_or_else(|| format!("missing {name}"))?
                .clone(),
            8,
        ));
    }
    // Spec 021 3.8: exact crates are also assets of the hosted release.
    for p in &m.packages {
        out.push(hosted_item(
            p.crate_file.clone(),
            files[&p.crate_file].clone(),
            8,
        ));
    }
    // Spec 021 3.9 keeps the upload order including development dependencies;
    // spec 031 3.1 gates npm on every crate even when crate depth reaches six.
    out.sort_by_key(|i| if i.kind == "crate" { 0 } else { i.layer });
    Ok(out)
}

fn hosted_item(name: String, bytes: Vec<u8>, layer: u8) -> Item {
    Item {
        kind: "hosted".into(),
        file: name.clone(),
        name,
        layer,
        checksum: sha256_hex(&bytes),
        bytes,
    }
}

/// Spec 031 3.11.4: continuations require a fresh explicit instruction.
pub fn publish(
    life: &mut Lifecycle,
    files: &BTreeMap<String, Vec<u8>>,
    registry: &mut dyn FamilyRegistry,
    j: &mut Journal,
) -> Result<(), String> {
    let result = run(life, files, registry, j);
    // A failure after the whole family was freshly observed public (3.1)
    // leaves the release published, never partially published.
    if let Err(reason) = &result {
        // Any artifact visible under the version, from this run or an earlier
        // one or another uploader, makes the version public in part, so a
        // stop is `partially published`; it only ever demands more of the
        // owner (a fresh instruction to continue, 3.11.4), never less.
        if j.entries.iter().any(|e| {
            matches!(
                e,
                Entry::Result {
                    outcome: Upload::Accepted | Upload::Ambiguous { .. },
                    ..
                } | Entry::Query {
                    observed: Lookup::Visible { .. },
                    ..
                }
            )
        }) && !life.family_observed
        {
            life.state = State::PartiallyPublished;
        }
        if !j.broken() {
            j.record(Entry::Stopped {
                reason: redact(reason),
            });
        }
    }
    result
}

fn run(
    life: &mut Lifecycle,
    files: &BTreeMap<String, Vec<u8>>,
    registry: &mut dyn FamilyRegistry,
    j: &mut Journal,
) -> Result<(), String> {
    if !matches!(
        life.state,
        State::PublicationAuthorized | State::PartiallyPublished
    ) {
        return Err("family publication requires owner instruction".into());
    }
    let m = &life.manifest;
    bound(m, j)?;
    let i = life.instruction.as_ref().ok_or("missing instruction")?;
    if i.version != m.version
        || i.commit != m.commit
        || i.registries != m.registries
        || i.artifacts
            != m.artifacts
                .iter()
                .map(|a| a.name.clone())
                .collect::<Vec<_>>()
        || i.packages != m.upload_order
        || i.action != "publish"
    {
        return Err("instruction differs from manifest".into());
    }
    // 3.11.1: the first-release instruction authorizes exactly 0.1.0, so a
    // matching instruction for any other version is refused on its own
    // ground rather than as a mismatch.
    if m.version != crate::lifecycle::FIRST_RELEASE_VERSION {
        return Err(format!(
            "family publication authorizes only {}, manifest names {}",
            crate::lifecycle::FIRST_RELEASE_VERSION,
            m.version
        ));
    }
    // 3.11.4: any earlier run that reached an upload, stopped or not (a
    // journal that could not record its stop, or a killed process, leaves
    // no `Stopped` entry), needs a fresh continuation instruction.
    if j.entries
        .iter()
        .any(|e| matches!(e, Entry::Stopped { .. } | Entry::Attempt { .. }))
        && !i.continuation
    {
        return Err("partial publication needs fresh continuation instruction".into());
    }
    if m.limitations
        .iter()
        .any(|l| l == crate::family_artifacts::MISSING)
    {
        return Err("missing rehearsal artifacts; not publishable".into());
    }
    crate::family_artifacts::validate(m)?;
    let items = items(m, files)?;
    for item in &items {
        if item.kind == "image" {
            return Err("container image is absent from this release".into());
        }
        // Spec 031 3.1: reobserve every earlier layer before advancing.
        for prior in items.iter().filter(|p| {
            if item.kind == "crate" {
                p.kind == "crate" && p.layer < item.layer
            } else {
                p.kind == "crate" || p.layer < item.layer
            }
        }) {
            let seen = query(registry, prior, &m.version, j);
            if seen
                != (Lookup::Visible {
                    checksum: prior.checksum.clone(),
                })
            {
                return Err(format!(
                    "earlier layer {}: {} not public with expected checksum",
                    prior.layer, prior.name
                ));
            }
        }
        let seen = query(registry, item, &m.version, j);
        match seen {
            Lookup::Visible { checksum } if checksum == item.checksum => continue,
            Lookup::Visible { .. } => {
                return Err(format!(
                    "{} public with other bytes; version spent",
                    item.name
                ));
            }
            Lookup::Unknown { .. } => return Err(format!("{} visibility unknown", item.name)),
            Lookup::Absent => {}
        }
        j.may_attempt(&item.name)?;
        // The manifest lists crates.io, npm and the hosted release, in order.
        let registry_name = match item.kind.as_str() {
            "crate" => Some(m.registry.clone()),
            "npm" => m.registries.get(1).cloned(),
            _ => m.registries.get(2).cloned(),
        }
        .ok_or_else(|| format!("manifest names no registry for {}", item.name))?;
        j.record(Entry::Attempt {
            package: item.name.clone(),
            version: m.version.clone(),
            sha256: sha256_hex(&item.bytes),
            registry: registry_name,
        });
        if j.broken() {
            return Err("attempt could not be journaled".into());
        }
        let outcome = match registry.upload(item, &m.version) {
            Upload::Refused { reason } => Upload::Refused {
                reason: redact(&reason),
            },
            Upload::Ambiguous { detail } => Upload::Ambiguous {
                detail: redact(&detail),
            },
            x => x,
        };
        j.record(Entry::Result {
            package: item.name.clone(),
            outcome: outcome.clone(),
        });
        if j.broken() {
            return Err("result could not be journaled".into());
        }
        match outcome {
            Upload::Refused { reason } => return Err(format!("{} refused: {reason}", item.name)),
            Upload::Ambiguous { .. } => {
                query(registry, item, &m.version, j);
                return Err(format!("{} ambiguous; query recorded, stop", item.name));
            }
            Upload::Accepted => {}
        }
        let mut visible = false;
        for _ in 0..VISIBILITY_POLLS {
            match query(registry, item, &m.version, j) {
                Lookup::Visible { checksum } if checksum == item.checksum => {
                    visible = true;
                    break;
                }
                Lookup::Visible { .. } => {
                    return Err(format!(
                        "{} public with other bytes; version spent",
                        item.name
                    ));
                }
                _ => registry.wait(),
            }
        }
        if !visible {
            return Err(format!("{} accepted but not visible", item.name));
        }
    }
    // Spec 031 3.1: completion is a fresh observation of the whole family.
    for item in &items {
        if query(registry, item, &m.version, j)
            != (Lookup::Visible {
                checksum: item.checksum.clone(),
            })
        {
            return Err(format!("{}: not public at completion", item.name));
        }
    }
    if j.broken() {
        return Err("completion observations could not be journaled".into());
    }
    life.family_observed = true;
    life.state = State::Published;
    life.apply(crate::lifecycle::Event::ReleaseCreated)
        .map_err(|e| e.0)?;
    Ok(())
}

fn query(r: &mut dyn FamilyRegistry, item: &Item, version: &str, j: &mut Journal) -> Lookup {
    let seen = match r.lookup(item, version) {
        Lookup::Unknown { detail } => Lookup::Unknown {
            detail: redact(&detail),
        },
        x => x,
    };
    j.record(Entry::Query {
        package: item.name.clone(),
        observed: seen.clone(),
    });
    seen
}

/// Spec 031 3.9.7: record the fresh-consumer matrix before qualification.
pub fn qualify(
    life: &mut Lifecycle,
    evidence: BTreeMap<String, String>,
    j: &mut Journal,
) -> Result<(), String> {
    if life.state != State::Released {
        return Err("release is not complete".into());
    }
    let mut required = vec!["crates-subsets".to_string()];
    for a in &life.manifest.artifacts {
        if matches!(a.kind.as_str(), "npm" | "binary") {
            required.push(a.name.clone());
        }
    }
    if required.iter().any(|k| {
        evidence
            .get(k)
            .is_none_or(|v| v.len() != 64 || !v.bytes().all(|b| b.is_ascii_hexdigit()))
    }) {
        return Err("missing fresh-consumer evidence".into());
    }
    j.record(Entry::Qualification { evidence });
    if j.broken() {
        return Err("qualification could not be journaled".into());
    }
    life.consumer_recorded = true;
    life.apply(crate::lifecycle::Event::ConsumerQualified)
        .map_err(|e| e.0)?;
    Ok(())
}

/// Spec 031 acceptance: scripted memory-only npm and hosted release seam.
#[derive(Default)]
pub struct FakeHostedRelease {
    pub registry: crate::lifecycle::FakeRegistry,
}
pub type FakeNpmRegistry = FakeHostedRelease;
impl FamilyRegistry for FakeHostedRelease {
    fn lookup(&mut self, i: &Item, v: &str) -> Lookup {
        crate::lifecycle::Registry::lookup(&mut self.registry, &i.name, v)
    }
    fn upload(&mut self, i: &Item, v: &str) -> Upload {
        let out = crate::lifecycle::Registry::upload(&mut self.registry, &i.name, v, &i.bytes);
        if i.kind == "npm"
            && let Some((checksum, _)) = self.registry.index.get_mut(&(i.name.clone(), v.into()))
        {
            *checksum = crate::family_artifacts::integrity(&i.bytes);
        }
        out
    }
    fn wait(&mut self) {
        crate::lifecycle::Registry::wait(&mut self.registry);
    }
}
