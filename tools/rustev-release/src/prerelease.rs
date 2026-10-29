//! Spec 053 3.3.3 and 3.3.4, now for every release (058 3.5.2): the GitHub
//! release adapter `tagged::github_release` drives, created once with no
//! asset ever replaced.
use std::{collections::BTreeMap, process::Command};

pub type Assets = BTreeMap<String, Vec<u8>>;
#[derive(Debug, Clone)]
pub struct Release {
    pub prerelease: bool,
    pub assets: BTreeMap<String, String>,
}
pub trait Releases {
    fn lookup(&mut self, tag: &str) -> Result<Option<Release>, String>;
    /// Spec 058 3.3.3: a release (`prerelease` false, marked latest) or a
    /// candidate's pre-release.
    fn create_release(
        &mut self,
        tag: &str,
        notes: &str,
        assets: &Assets,
        prerelease: bool,
    ) -> Result<(), String>;
}
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Created,
    Present,
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
