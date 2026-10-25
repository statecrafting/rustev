//! Spec 031 3.1 layers 6 to 8: anonymous observation and the gh process adapter.
use crate::{
    family_lifecycle::{FamilyRegistry, Item},
    lifecycle::{Lookup, Registry, Upload},
    manifest::ReleaseManifest,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
};

/// Spec 031 3.1: collect every missing or mismatched crate, without a token.
pub fn observe_crates(m: &ReleaseManifest, registry: &mut dyn Registry) -> Vec<String> {
    m.packages
        .iter()
        .filter_map(|p| match registry.lookup(&p.name, &m.version) {
            Lookup::Visible { checksum } if checksum == p.sha256 => None,
            Lookup::Visible { .. } => Some(format!("{}: checksum mismatch", p.name)),
            Lookup::Absent => Some(format!("{}: missing", p.name)),
            Lookup::Unknown { detail } => Some(format!(
                "{}: unknown ({})",
                p.name,
                crate::lifecycle::redact(&detail)
            )),
        })
        .collect()
}

/// Spec 031 3.1: npm dist.integrity is the public visibility checksum.
pub fn npm_lookup(name: &str, version: &str) -> Lookup {
    if !name
        .strip_prefix(crate::npm::SCOPE)
        .is_some_and(crate::family_artifacts::plain)
        || !crate::family_artifacts::plain(version)
    {
        return Lookup::Unknown {
            detail: "invalid npm identity".into(),
        };
    }
    match Command::new("curl")
        .args(["-q", "-sS", "--max-time", "120", "-w", "\n%{http_code}"])
        .arg(format!("https://registry.npmjs.org/{name}/{version}"))
        .output()
    {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout);
            let (body, code) = text.rsplit_once('\n').unwrap_or(("", ""));
            npm_response(code, body)
        }
        _ => Lookup::Unknown {
            detail: "npm query failed".into(),
        },
    }
}

pub fn npm_response(code: &str, body: &str) -> Lookup {
    match code {
        "404" => Lookup::Absent,
        "200" => match serde_json::from_str::<serde_json::Value>(body)
            .ok()
            .and_then(|v| v["dist"]["integrity"].as_str().map(str::to_string))
        {
            Some(checksum) => Lookup::Visible { checksum },
            None => Lookup::Unknown {
                detail: "npm response lacks integrity".into(),
            },
        },
        _ => Lookup::Unknown {
            detail: format!("npm HTTP {code}"),
        },
    }
}

/// Spec 031 3.11: gh owns credentials; this adapter never reads or logs them.
pub struct HostedRelease {
    pub gh: PathBuf,
    pub tag: String,
    pub changelog: PathBuf,
    pub staging: PathBuf,
}

impl HostedRelease {
    fn command(&self) -> Command {
        let mut c = Command::new(&self.gh);
        c.args(["--repo", "statecrafting/rustev"]);
        c
    }
    fn assets(&self) -> Result<Option<Vec<String>>, String> {
        let o = self
            .command()
            .args(["release", "view", &self.tag, "--json", "assets"])
            .output()
            .map_err(|_| "gh release view could not run")?;
        if !o.status.success() {
            // Spec 021 3.9: only an explicit not-found is absence.
            if String::from_utf8_lossy(&o.stderr).trim() == "release not found" {
                return Ok(None);
            }
            return Err("hosted release visibility unknown".into());
        }
        let v: serde_json::Value =
            serde_json::from_slice(&o.stdout).map_err(|_| "invalid gh assets response")?;
        let names = v["assets"]
            .as_array()
            .ok_or("missing assets")?
            .iter()
            .map(|a| {
                a["name"]
                    .as_str()
                    .map(str::to_string)
                    .ok_or_else(|| "asset without name".to_string())
            })
            .collect::<Result<_, _>>()?;
        Ok(Some(names))
    }
}

impl FamilyRegistry for HostedRelease {
    fn lookup(&mut self, i: &Item, version: &str) -> Lookup {
        match i.kind.as_str() {
            "crate" => {
                return crate::cratesio::CratesIo { wait_seconds: 0 }.lookup(&i.name, version);
            }
            "npm" => return npm_lookup(&i.name, version),
            _ => {}
        }
        match self.assets() {
            Ok(None) => Lookup::Absent,
            Ok(Some(names)) if !names.contains(&i.name) => Lookup::Absent,
            Ok(Some(_)) => {
                let temp = match crate::repro::fresh_dir("rustev-asset-observe") {
                    Ok(d) => d,
                    Err(e) => return Lookup::Unknown { detail: e },
                };
                let out = self
                    .command()
                    .args([
                        "release",
                        "download",
                        &self.tag,
                        "--pattern",
                        &i.name,
                        "--dir",
                    ])
                    .arg(&*temp)
                    .output();
                match out {
                    Ok(o) if o.status.success() => match std::fs::read(temp.join(&i.name)) {
                        Ok(b) => Lookup::Visible {
                            checksum: crate::archive::sha256_hex(&b),
                        },
                        Err(_) => Lookup::Unknown {
                            detail: "downloaded asset missing".into(),
                        },
                    },
                    _ => Lookup::Unknown {
                        detail: "asset download failed".into(),
                    },
                }
            }
            Err(detail) => Lookup::Unknown { detail },
        }
    }
    fn upload(&mut self, i: &Item, _: &str) -> Upload {
        if matches!(i.kind.as_str(), "crate" | "npm" | "image") {
            return Upload::Refused {
                reason: "hosted-release cannot upload to registries".into(),
            };
        }
        let path = self.staging.join(&i.file);
        if std::fs::write(&path, &i.bytes).is_err() {
            return Upload::Refused {
                reason: "asset staging failed".into(),
            };
        }
        let mut cmd = self.command();
        match self.assets() {
            Ok(None) => {
                cmd.args([
                    "release",
                    "create",
                    &self.tag,
                    "--verify-tag",
                    "--title",
                    &self.tag,
                    "--notes-file",
                ])
                .arg(&self.changelog)
                .arg(&path);
            }
            Ok(Some(_)) => {
                cmd.args(["release", "upload", &self.tag]).arg(&path);
            }
            Err(detail) => return Upload::Refused { reason: detail },
        }
        match cmd.output() {
            Ok(o) if o.status.success() => Upload::Accepted,
            _ => Upload::Ambiguous {
                detail: "gh write failed; query before any retry".into(),
            },
        }
    }
    fn wait(&mut self) {
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
}

/// Spec 021 3.9: canonicalize the file too, refusing symlink redirection.
pub fn journal_path(path: &Path, repo: &Path) -> Result<PathBuf, String> {
    let repo = repo.canonicalize().map_err(|e| e.to_string())?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let resolved = if path.exists() {
        path.canonicalize().map_err(|e| e.to_string())?
    } else {
        parent.join(path.file_name().ok_or("missing journal filename")?)
    };
    if resolved.starts_with(repo) {
        return Err("publication journal is inside the repository".into());
    }
    Ok(resolved)
}

/// Spec 031 3.11.2: verify all local bindings before even querying registries.
pub fn run(
    dir: &Path,
    artifacts: &Path,
    instruction: &Path,
    journal: &Path,
    repo: &Path,
    set: &crate::ReleaseSet,
) -> Result<(), String> {
    use crate::lifecycle::{Event, Instruction, Journal, Lifecycle};
    let journal = journal_path(journal, repo)?;
    let scratch = crate::repro::fresh_dir("rustev-hosted-release")?;
    let signers = scratch.join("allowed_signers");
    std::fs::write(&signers, crate::manifest::ALLOWED_SIGNERS).map_err(|e| e.to_string())?;
    let errors =
        crate::manifest::verify_release(dir, repo, &signers, crate::manifest::OWNER_IDENTITY);
    if !errors.is_empty() {
        return Err(errors.join("\n"));
    }
    let m = ReleaseManifest::parse(
        &std::fs::read(dir.join("release-manifest.json")).map_err(|e| e.to_string())?,
    )?;
    if m.limitations
        .iter()
        .any(|l| l == crate::family_artifacts::MISSING)
    {
        return Err("missing rehearsal artifacts; not publishable".into());
    }
    let expected = crate::family_artifacts::collect(set, dir, Some(artifacts))?;
    if expected != m.artifacts || set.version != m.version || set.upload_order != m.upload_order {
        return Err("release set differs from signed manifest".into());
    }
    let errors = crate::family_artifacts::verify(&m, artifacts);
    if !errors.is_empty() {
        return Err(errors.join("\n"));
    }
    for (name, digest) in &m.evidence {
        if !crate::family_artifacts::plain(name) {
            return Err("invalid evidence name".into());
        }
        if crate::archive::sha256_hex(&std::fs::read(dir.join(name)).map_err(|e| e.to_string())?)
            != *digest
        {
            return Err(format!("{name}: evidence differs"));
        }
    }
    let i: Instruction =
        serde_json::from_slice(&std::fs::read(instruction).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let mut life = Lifecycle::new(m.clone());
    life.apply(Event::GatesPass).map_err(|e| e.0)?;
    life.apply(Event::CandidateVerified {
        commit: m.commit.clone(),
    })
    .map_err(|e| e.0)?;
    life.apply(Event::Authorized(i)).map_err(|e| e.0)?;
    let mut j = match std::fs::read_to_string(&journal) {
        Ok(s) => Journal::from_lines(&s)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Journal::default(),
        Err(e) => return Err(e.to_string()),
    };
    j.file = Some(journal);
    let signature = std::fs::read_to_string(dir.join("release-manifest.json.sig"))
        .map_err(|e| e.to_string())?;
    crate::family_lifecycle::bind(&m, &signature, &mut j)?;
    // Spec 031 3.1, 3.11.2: record prerequisite observations before hosting.
    // An npm artifact without its package or integrity is refused here, so
    // the lookups below never meet a missing field.
    if let Some(a) = m
        .artifacts
        .iter()
        .find(|a| a.kind == "npm" && (a.package.is_none() || a.integrity.is_none()))
    {
        return Err(format!(
            "npm artifact {} lacks its package or integrity",
            a.name
        ));
    }
    let mut failures = Vec::new();
    for (name, expected, observed) in m
        .packages
        .iter()
        .map(|p| {
            (
                p.name.clone(),
                p.sha256.clone(),
                crate::cratesio::CratesIo { wait_seconds: 0 }.lookup(&p.name, &m.version),
            )
        })
        .chain(m.artifacts.iter().filter(|a| a.kind == "npm").map(|a| {
            let name = a.package.clone().unwrap_or_default();
            let observed = npm_lookup(&name, &m.version);
            (name, a.integrity.clone().unwrap_or_default(), observed)
        }))
    {
        j.record(crate::lifecycle::Entry::Query {
            package: name.clone(),
            observed: observed.clone(),
        });
        if observed != (Lookup::Visible { checksum: expected }) {
            failures.push(format!("{name}: not public with expected checksum"));
        }
    }
    if !failures.is_empty() {
        let reason = failures.join("\n");
        j.record(crate::lifecycle::Entry::Stopped {
            reason: reason.clone(),
        });
        return Err(reason);
    }
    if j.broken() {
        return Err("prerequisite observations could not be journaled".into());
    }
    let summary = j
        .entries
        .iter()
        .find_map(|e| {
            if let crate::lifecycle::Entry::Summary { text } = e {
                Some(text.clone())
            } else {
                None
            }
        })
        .unwrap_or_else(|| j.to_lines());
    if !j
        .entries
        .iter()
        .any(|e| matches!(e, crate::lifecycle::Entry::Summary { .. }))
    {
        j.record(crate::lifecycle::Entry::Summary {
            text: summary.clone(),
        });
    }
    let mut files = BTreeMap::new();
    for name in m
        .packages
        .iter()
        .map(|p| p.crate_file.as_str())
        .chain(m.artifacts.iter().map(|a| a.name.as_str()))
        .chain([
            "release-manifest.json",
            "release-manifest.json.sig",
            "SHA256SUMS",
            "SHA256SUMS.sig",
        ])
    {
        // As for the evidence names above: a plain file name, never a path.
        if !crate::family_artifacts::plain(name) {
            return Err(format!("{name:?} is not a plain release file name"));
        }
        files.insert(
            name.into(),
            std::fs::read(dir.join(name)).map_err(|e| format!("{name}: {e}"))?,
        );
    }
    files.insert("journal-summary.jsonl".into(), summary.into_bytes());
    let notes = scratch.join("changelog.md");
    std::fs::write(
        &notes,
        files.get("changelog.md").ok_or("missing changelog")?,
    )
    .map_err(|e| e.to_string())?;
    let mut adapter = HostedRelease {
        gh: "gh".into(),
        tag: m.tag.clone(),
        changelog: notes,
        staging: scratch.to_path_buf(),
    };
    crate::family_lifecycle::publish(&mut life, &files, &mut adapter, &mut j)
}
