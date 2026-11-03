//! Spec 060 3.3.3 and 3.4.2: the image's registry state against the
//! manifest. [`ImageRegistry`] reads manifests by tag or digest, so tests
//! use a fake; [`Ghcr`] reads `ghcr.io` through the system `curl`, with the
//! token the workflow gives it (or anonymously), never writing.

use std::io::Write;
use std::process::{Command, Stdio};

use crate::archive::sha256_hex;
use crate::image::{INDEX_MEDIA_TYPE, ImageRelease, MANIFEST_MEDIA_TYPE, index_bytes};

pub trait ImageRegistry {
    /// The bytes of the manifest or index that `reference` (a tag or a
    /// `sha256:` digest) names in `repository`; `None` only when the
    /// registry answers that it is absent.
    fn manifest(&mut self, repository: &str, reference: &str) -> Result<Option<Vec<u8>>, String>;
}

/// What the image's tag names (060 3.3.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagState {
    Absent,
    /// The run's index digest.
    Same,
    /// Another digest: the run fails and nothing is moved.
    Other(String),
}

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{}", sha256_hex(bytes))
}

/// 3.3.3: whether the tag `X.Y.Z` is absent, names the run's index, or
/// names another.
pub fn tag_state(
    registry: &mut dyn ImageRegistry,
    image: &ImageRelease,
) -> Result<TagState, String> {
    Ok(match registry.manifest(&image.repository, &image.tag)? {
        None => TagState::Absent,
        Some(b) if digest(&b) == image.index_digest => TagState::Same,
        Some(b) => TagState::Other(digest(&b)),
    })
}

/// 3.4.2: the tag resolves to the manifest's index digest, whose bytes are
/// the index of the manifest's platform descriptors, and each platform
/// manifest is present by its digest with its size. Every difference is
/// named; empty is the passing verdict.
pub fn verify(registry: &mut dyn ImageRegistry, image: &ImageRelease) -> Vec<String> {
    let mut problems = Vec::new();
    let repo = &image.repository;
    let expected = index_bytes(&image.platforms);
    match registry.manifest(repo, &image.tag) {
        Err(e) => problems.push(format!("060 3.4.2: {repo}:{}: {e}", image.tag)),
        Ok(None) => problems.push(format!("060 3.4.2: {repo}:{} is absent", image.tag)),
        Ok(Some(b)) if digest(&b) != image.index_digest => problems.push(format!(
            "060 3.4.2: {repo}:{} names {}, not the manifest's index {}",
            image.tag,
            digest(&b),
            image.index_digest
        )),
        Ok(Some(b)) if b != expected => problems.push(format!(
            "060 3.4.2: the index {} is not the index of the manifest's platform digests",
            image.index_digest
        )),
        Ok(Some(_)) => {}
    }
    for p in &image.platforms {
        match registry.manifest(repo, &p.digest) {
            Err(e) => problems.push(format!("060 3.4.2: {repo}@{}: {e}", p.digest)),
            Ok(None) => problems.push(format!(
                "060 3.4.2: {}'s manifest {} is absent",
                p.platform, p.digest
            )),
            Ok(Some(b)) if digest(&b) != p.digest || b.len() as u64 != p.size => {
                problems.push(format!(
                    "060 3.4.2: {}'s manifest differs from {}",
                    p.platform, p.digest
                ))
            }
            Ok(Some(_)) => {}
        }
    }
    problems
}

/// A tag `X.Y.Z` or a `sha256:` digest; nothing else reaches a URL.
fn reference_ok(r: &str) -> bool {
    r.strip_prefix("sha256:").map_or_else(
        || {
            let parts: Vec<&str> = r.split('.').collect();
            parts.len() == 3
                && parts
                    .iter()
                    .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        },
        |h| {
            h.len() == 64
                && h.bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        },
    )
}

/// `ghcr.io` over its registry API. A token in `GH_TOKEN` (the workflow's
/// `GITHUB_TOKEN`) authenticates the token exchange; it is passed to `curl`
/// on standard input, never on its command line, and never printed.
pub struct Ghcr {
    pub token: Option<String>,
}

impl Ghcr {
    pub fn from_env() -> Self {
        Ghcr {
            token: std::env::var("GH_TOKEN").ok().filter(|t| !t.is_empty()),
        }
    }

    /// `curl` with `config` (curl's config syntax) on standard input; the
    /// body goes to `out`, the HTTP status is returned.
    fn curl(config: &str, out: &std::path::Path, url: &str) -> Result<String, String> {
        let mut child = Command::new("curl")
            .args([
                "-q",
                "-sS",
                "--max-time",
                "120",
                "-K",
                "-",
                "-w",
                "%{http_code}",
                "-o",
            ])
            .arg(out)
            .arg(url)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("curl: {e}"))?;
        child
            .stdin
            .take()
            .ok_or("curl: no stdin")?
            .write_all(config.as_bytes())
            .map_err(|e| format!("curl: {e}"))?;
        let o = child.wait_with_output().map_err(|e| format!("curl: {e}"))?;
        if !o.status.success() {
            // `-sS` writes only curl's own error line to stderr; the config
            // (and with it any credential) goes through stdin, never there.
            return Err(format!(
                "registry transport failed ({}): {}",
                o.status,
                String::from_utf8_lossy(&o.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&o.stdout).trim().to_string())
    }
}

fn quoted(s: &str) -> Result<String, String> {
    if s.bytes()
        .any(|b| b == b'"' || b == b'\\' || b.is_ascii_control())
    {
        return Err("a credential holds a character curl's config cannot carry".into());
    }
    Ok(format!("\"{s}\""))
}

impl ImageRegistry for Ghcr {
    fn manifest(&mut self, repository: &str, reference: &str) -> Result<Option<Vec<u8>>, String> {
        let name = repository
            .strip_prefix("ghcr.io/")
            .filter(|n| crate::family_artifacts::plain(&n.replace('/', "")))
            .ok_or_else(|| format!("{repository} is not a ghcr.io repository"))?;
        if !reference_ok(reference) {
            return Err(format!("invalid reference {reference:?}"));
        }
        let scratch = crate::repro::fresh_dir("rustev-ghcr")?;
        let body = scratch.join("body");
        let auth = match &self.token {
            Some(t) => format!("user = {}\n", quoted(&format!("token:{t}"))?),
            None => String::new(),
        };
        let code = Self::curl(
            &auth,
            &body,
            &format!("https://ghcr.io/token?scope=repository:{name}:pull"),
        )?;
        if code != "200" {
            return Err(format!("registry token HTTP {code}"));
        }
        let v: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&body).map_err(|e| e.to_string())?)
                .map_err(|_| "unreadable registry token response".to_string())?;
        let bearer = v["token"].as_str().ok_or("no registry token")?;
        let config = format!(
            "header = {}\nheader = {}\n",
            quoted(&format!("Authorization: Bearer {bearer}"))?,
            quoted(&format!(
                "Accept: {INDEX_MEDIA_TYPE}, {MANIFEST_MEDIA_TYPE}"
            ))?
        );
        let code = Self::curl(
            &config,
            &body,
            &format!("https://ghcr.io/v2/{name}/manifests/{reference}"),
        )?;
        match code.as_str() {
            "200" => Ok(Some(std::fs::read(&body).map_err(|e| e.to_string())?)),
            "404" => Ok(None),
            other => Err(format!("registry HTTP {other}")),
        }
    }
}
