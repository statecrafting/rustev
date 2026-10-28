//! Spec 053 3.3.3 and 3.3.4: create once, never replace an asset.
use crate::{archive::sha256_hex, candidate, manifest};
use std::{collections::BTreeMap, path::Path, process::Command};

pub type Assets = BTreeMap<String, Vec<u8>>;
#[derive(Debug, Clone)]
pub struct Release {
    pub prerelease: bool,
    pub assets: BTreeMap<String, String>,
}
pub trait Releases {
    fn lookup(&mut self, tag: &str) -> Result<Option<Release>, String>;
    fn create(&mut self, tag: &str, notes: &str, assets: &Assets) -> Result<(), String>;
    /// Spec 058 3.3.3: a release (`prerelease` false, marked latest) or a
    /// candidate's pre-release.
    fn create_release(
        &mut self,
        tag: &str,
        notes: &str,
        assets: &Assets,
        prerelease: bool,
    ) -> Result<(), String> {
        if prerelease {
            self.create(tag, notes, assets)
        } else {
            Err("058 3.3.3: this adapter creates pre-releases only".into())
        }
    }
}
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Created,
    Present,
}

pub fn publish_prerelease(
    api: &mut dyn Releases,
    tag: &str,
    assets: &Assets,
) -> Result<Outcome, String> {
    let set = crate::ReleaseSet::parse(include_bytes!("../release-set.json"))?;
    candidate::parse(tag, &set.version)?;
    if assets.is_empty() || assets.keys().any(|n| !crate::family_artifacts::plain(n)) {
        return Err("053 3.3.3: missing assets or invalid asset name".into());
    }
    let digests = assets
        .iter()
        .map(|(n, b)| (n.clone(), sha256_hex(b)))
        .collect();
    if let Some(present) = api.lookup(tag)? {
        if !present.prerelease || present.assets != digests {
            return Err(
                "053 3.3.4: existing release flag or assets differ; nothing replaced".into(),
            );
        }
        return Ok(Outcome::Present);
    }
    let manifest = assets
        .get("release-manifest.json")
        .ok_or("053 3.3.3: missing manifest")?;
    let notes = format!(
        "{}\n\nrelease-manifest-sha256: {}\n",
        candidate::LIMITATION,
        sha256_hex(manifest)
    );
    api.create(tag, &notes, assets)?;
    Ok(Outcome::Created)
}

/// `owner/name`, the form both `gh api` paths and `gh --repo` take.
fn repository() -> Result<&'static str, String> {
    crate::REPOSITORY
        .strip_prefix("https://github.com/")
        .ok_or_else(|| "invalid repository".into())
}

pub struct Gh {
    pub executable: std::path::PathBuf,
}
impl Releases for Gh {
    fn lookup(&mut self, tag: &str) -> Result<Option<Release>, String> {
        let repo = repository()?;
        let out = Command::new(&self.executable)
            .args(["api", &format!("repos/{repo}/releases/tags/{tag}")])
            .output()
            .map_err(|e| e.to_string())?;
        let value: serde_json::Value = serde_json::from_slice(&out.stdout)
            .map_err(|e| format!("053 3.3.4: unreadable release response: {e}"))?;
        if !out.status.success() {
            if value["status"] == "404" || value["status"] == 404 {
                return Ok(None);
            }
            return Err("053 3.3.4: release lookup failed".into());
        }
        let prerelease = value["prerelease"]
            .as_bool()
            .ok_or("053 3.3.4: missing prerelease flag")?;
        let mut assets = BTreeMap::new();
        for a in value["assets"]
            .as_array()
            .ok_or("053 3.3.4: missing assets")?
        {
            let name = a["name"].as_str().ok_or("053 3.3.4: missing asset name")?;
            let digest = a["digest"]
                .as_str()
                .and_then(|d| d.strip_prefix("sha256:"))
                .ok_or("053 3.3.4: missing SHA-256 digest")?;
            if digest.len() != 64
                || !digest
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
                || assets.insert(name.into(), digest.into()).is_some()
            {
                return Err("053 3.3.4: invalid digest or duplicate asset".into());
            }
        }
        Ok(Some(Release { prerelease, assets }))
    }
    fn create(&mut self, tag: &str, notes: &str, assets: &Assets) -> Result<(), String> {
        self.create_release(tag, notes, assets, true)
    }
    fn create_release(
        &mut self,
        tag: &str,
        notes: &str,
        assets: &Assets,
        prerelease: bool,
    ) -> Result<(), String> {
        let scratch = crate::repro::fresh_dir("rustev-prerelease")?;
        let mut cmd = Command::new(&self.executable);
        cmd.args(["release", "create", tag, "--repo", repository()?]);
        if prerelease {
            cmd.args(["--prerelease", "--latest=false"]);
        } else {
            cmd.arg("--latest");
        }
        cmd.args(["--verify-tag", "--title", tag, "--notes", notes]);
        for (name, bytes) in assets {
            let path = scratch.join(name);
            std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
            cmd.arg(path);
        }
        let out = cmd.output().map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err("053 3.3.4: pre-release creation failed; inspect before retrying".into());
        }
        Ok(())
    }
}

pub fn run(tag: &str, file: &Path, signature: &Path, artifacts: &Path) -> Result<(), String> {
    let bytes = std::fs::read(file).map_err(|e| e.to_string())?;
    let m = manifest::ReleaseManifest::parse(&bytes)?;
    // 053 3.2.4: the manifest names the signed tag's own commit.
    let out = Command::new("git")
        .args(["rev-parse", &format!("refs/tags/{tag}^{{commit}}")])
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("053 3.2.4: no tag {tag} in this checkout"));
    }
    candidate::check_manifest(&bytes, tag, String::from_utf8_lossy(&out.stdout).trim())?;
    let scratch = crate::repro::fresh_dir("rustev-prerelease-signers")?;
    let signers = scratch.join("allowed_signers");
    std::fs::write(&signers, manifest::ALLOWED_SIGNERS).map_err(|e| e.to_string())?;
    manifest::ssh_verify(&bytes, signature, &signers, manifest::OWNER_IDENTITY)?;
    let set = crate::ReleaseSet::parse(include_bytes!("../release-set.json"))?;
    let set = candidate::release_set(&set, tag)?;
    if crate::family_artifacts::collect(&set, Path::new("."), Some(artifacts))? != m.artifacts {
        return Err("053 3.3.1: manifest differs from the complete candidate artifact set".into());
    }
    let errors = crate::family_artifacts::verify(&m, artifacts);
    if !errors.is_empty() {
        return Err(errors.join("\n"));
    }
    let mut assets = Assets::new();
    for (path, bytes) in crate::family_artifacts::workflow_files(artifacts)? {
        let name = path.rsplit('/').next().ok_or("missing asset name")?;
        if name == "SHA256SUMS" {
            continue;
        }
        if assets.insert(name.into(), bytes).is_some() {
            return Err("053 3.3.3: duplicate asset name".into());
        }
    }
    assets.insert("release-manifest.json".into(), bytes);
    assets.insert(
        "release-manifest.json.sig".into(),
        std::fs::read(signature).map_err(|e| e.to_string())?,
    );
    let sums: String = assets
        .iter()
        .map(|(n, b)| format!("{}  {n}\n", sha256_hex(b)))
        .collect();
    assets.insert("SHA256SUMS".into(), sums.into_bytes());
    publish_prerelease(
        &mut Gh {
            executable: "gh".into(),
        },
        tag,
        &assets,
    )
    .map(|_| ())
}
